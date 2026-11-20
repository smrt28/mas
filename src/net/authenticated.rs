//! Versioned, authenticated binary frames. JSON is only the inner payload.
use hmac::{Hmac, Mac};
use rand::{TryRngCore, rngs::OsRng};
use serde::{Serialize, de::DeserializeOwned};
use sha2::Sha256;
use std::{
    io::{self, Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

pub const AUTH_TIMEOUT: Duration = Duration::from_secs(3);
const KEY: &[u8; 32] = include_bytes!(concat!(env!("OUT_DIR"), "/shared-key.bin"));
const VERSION: u16 = 1;
const CHALLENGE: &[u8; 4] = b"MAS\x01";
const DOMAIN: &[u8] = b"MAS-HMAC\0";
const HEADER: usize = 24;
const PROOF: u8 = 1;
const ACCEPTED: u8 = 2;
const ACTION: u8 = 3;
const REPLY: u8 = 4;
type HmacSha256 = Hmac<Sha256>;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Authentication or protocol verification failed",
    )
}

/// Each half has its own monotonic counter, including the proof/ack at sequence 1.
/// No Debug implementation: key material must not enter logs.
pub struct SendState {
    session: u64,
    direction: u8,
    next: Option<u64>,
}
pub struct ReceiveState {
    session: u64,
    direction: u8,
    next: Option<u64>,
}

fn header(session: u64, direction: u8, sequence: u64, kind: u8, length: u32) -> [u8; HEADER] {
    let mut bytes = [0; HEADER];
    bytes[0..2].copy_from_slice(&VERSION.to_be_bytes());
    bytes[2..10].copy_from_slice(&session.to_be_bytes());
    bytes[10] = direction;
    bytes[11..19].copy_from_slice(&sequence.to_be_bytes());
    bytes[19] = kind;
    bytes[20..24].copy_from_slice(&length.to_be_bytes());
    bytes
}
fn mac(key: &[u8; 32], header: &[u8], payload: &[u8]) -> HmacSha256 {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts a 256-bit key");
    mac.update(DOMAIN);
    mac.update(header);
    mac.update(payload);
    mac
}
impl SendState {
    fn new(session: u64, direction: u8) -> Self {
        Self {
            session,
            direction,
            next: Some(1),
        }
    }
    fn write_payload(
        &mut self,
        writer: &mut impl Write,
        kind: u8,
        payload: &[u8],
        key: &[u8; 32],
    ) -> io::Result<()> {
        let sequence = self.next.ok_or_else(invalid)?;
        if payload.len() as u64 > super::MAX_FRAME {
            return Err(invalid());
        }
        let header = header(
            self.session,
            self.direction,
            sequence,
            kind,
            payload.len() as u32,
        );
        let tag = mac(key, &header, payload).finalize().into_bytes();
        writer.write_all(&header)?;
        writer.write_all(payload)?;
        writer.write_all(&tag)?;
        writer.flush()?;
        self.next = sequence.checked_add(1);
        Ok(())
    }
    pub fn send(&mut self, writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
        let payload = serde_json::to_vec(value)?;
        let kind = if self.direction == 1 { ACTION } else { REPLY };
        self.write_payload(writer, kind, &payload, KEY)
    }
}
impl ReceiveState {
    fn new(session: u64, direction: u8) -> Self {
        Self {
            session,
            direction,
            next: Some(1),
        }
    }
    fn read_payload(
        &mut self,
        reader: &mut impl Read,
        kind: u8,
        limit: u64,
        key: &[u8; 32],
    ) -> io::Result<Vec<u8>> {
        let sequence = self.next.ok_or_else(invalid)?;
        let mut received = [0; HEADER];
        reader.read_exact(&mut received)?;
        let length = u32::from_be_bytes(received[20..24].try_into().unwrap());
        if received != header(self.session, self.direction, sequence, kind, length)
            || u64::from(length) > limit.min(super::MAX_FRAME)
        {
            return Err(invalid());
        }
        let mut payload = vec![0; length as usize];
        reader.read_exact(&mut payload)?;
        let mut tag = [0; 32];
        reader.read_exact(&mut tag)?;
        // The crypto library verifies the full SHA-256 tag in constant time.
        mac(key, &received, &payload)
            .verify_slice(&tag)
            .map_err(|_| invalid())?;
        self.next = sequence.checked_add(1);
        Ok(payload)
    }
    pub fn receive<T: DeserializeOwned>(
        &mut self,
        reader: &mut impl Read,
        limit: u64,
    ) -> io::Result<T> {
        let kind = if self.direction == 1 { ACTION } else { REPLY };
        let payload = self.read_payload(reader, kind, limit, KEY)?;
        serde_json::from_slice(&payload).map_err(|_| invalid())
    }
}

// Enforce a total handshake deadline, including peers that drip individual bytes.
struct HandshakeIo<'a> {
    stream: &'a mut TcpStream,
    deadline: Instant,
}
impl HandshakeIo<'_> {
    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Authentication timed out"))
    }
    fn finish(&self) -> io::Result<()> {
        self.stream.set_read_timeout(None)?;
        self.stream.set_write_timeout(Some(AUTH_TIMEOUT))
    }
}
impl Read for HandshakeIo<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        self.stream.read(bytes)
    }
}
impl Write for HandshakeIo<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

/// The server's only unauthenticated output is its versioned 64-bit OS-random challenge.
pub fn server_handshake(stream: &mut TcpStream) -> io::Result<(SendState, ReceiveState)> {
    let mut handshake = HandshakeIo {
        stream,
        deadline: Instant::now() + AUTH_TIMEOUT,
    };
    let session = OsRng
        .try_next_u64()
        .map_err(|_| io::Error::other("Session randomness unavailable"))?;
    handshake.write_all(CHALLENGE)?;
    handshake.write_all(&session.to_be_bytes())?;
    handshake.flush()?;
    let mut incoming = ReceiveState::new(session, 1);
    incoming.read_payload(&mut handshake, PROOF, 0, KEY)?;
    let mut outgoing = SendState::new(session, 2);
    outgoing.write_payload(&mut handshake, ACCEPTED, &[], KEY)?;
    handshake.finish()?;
    Ok((outgoing, incoming))
}
pub fn client_handshake(stream: &mut TcpStream) -> io::Result<(SendState, ReceiveState)> {
    let mut handshake = HandshakeIo {
        stream,
        deadline: Instant::now() + AUTH_TIMEOUT,
    };
    let mut challenge = [0; 12];
    handshake.read_exact(&mut challenge)?;
    if &challenge[..4] != CHALLENGE {
        return Err(invalid());
    }
    let session = u64::from_be_bytes(challenge[4..].try_into().unwrap());
    let mut outgoing = SendState::new(session, 1);
    outgoing.write_payload(&mut handshake, PROOF, &[], KEY)?;
    let mut incoming = ReceiveState::new(session, 2);
    incoming.read_payload(&mut handshake, ACCEPTED, 0, KEY)?;
    handshake.finish()?;
    Ok((outgoing, incoming))
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEST_KEY: [u8; 32] = [17; 32];
    fn frame(session: u64, direction: u8, sequence: u64, kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut state = SendState::new(session, direction);
        state.next = Some(sequence);
        let mut bytes = vec![];
        state
            .write_payload(&mut bytes, kind, payload, &TEST_KEY)
            .unwrap();
        bytes
    }
    #[test]
    fn every_header_field_payload_byte_and_tag_is_authenticated_without_advancing_on_failure() {
        let valid = frame(42, 1, 1, ACTION, b"{ \"value\" : 123 }");
        for index in 0..valid.len() {
            let mut changed = valid.clone();
            changed[index] ^= 1;
            let mut receiver = ReceiveState::new(42, 1);
            assert!(
                receiver
                    .read_payload(
                        &mut changed.as_slice(),
                        ACTION,
                        super::super::MAX_FRAME,
                        &TEST_KEY
                    )
                    .is_err()
            );
            assert_eq!(receiver.next, Some(1));
            assert!(
                receiver
                    .read_payload(
                        &mut valid.as_slice(),
                        ACTION,
                        super::super::MAX_FRAME,
                        &TEST_KEY
                    )
                    .is_ok()
            );
        }
    }
    #[test]
    fn wrong_keys_truncation_and_oversized_frames_are_rejected() {
        let valid = frame(42, 1, 1, ACTION, b"{}");
        let mut receiver = ReceiveState::new(42, 1);
        assert!(
            receiver
                .read_payload(&mut valid.as_slice(), ACTION, 4096, &[18; 32])
                .is_err()
        );
        for length in 0..valid.len() {
            assert!(
                receiver
                    .read_payload(&mut &valid[..length], ACTION, 4096, &TEST_KEY)
                    .is_err()
            );
            assert_eq!(receiver.next, Some(1));
        }
        let oversized = header(42, 1, 1, ACTION, 65537);
        assert!(
            receiver
                .read_payload(&mut oversized.as_slice(), ACTION, 65536, &TEST_KEY)
                .is_err()
        );
    }
    #[test]
    fn ordered_counters_reject_duplicates_gaps_old_sessions_and_reflection() {
        for direction in [1, 2] {
            let mut receiver = ReceiveState::new(42, direction);
            let valid = frame(42, direction, 1, ACTION, b"{}");
            receiver
                .read_payload(&mut valid.as_slice(), ACTION, 4096, &TEST_KEY)
                .unwrap();
            for invalid in [
                valid,
                frame(42, direction, 3, ACTION, b"{}"),
                frame(41, direction, 2, ACTION, b"{}"),
                frame(42, 3 - direction, 2, ACTION, b"{}"),
                frame(42, direction, 2, PROOF, b""),
            ] {
                assert!(
                    receiver
                        .read_payload(&mut invalid.as_slice(), ACTION, 4096, &TEST_KEY)
                        .is_err()
                );
                assert_eq!(receiver.next, Some(2));
            }
            let next = frame(42, direction, 2, ACTION, b"{}");
            receiver
                .read_payload(&mut next.as_slice(), ACTION, 4096, &TEST_KEY)
                .unwrap();
        }
    }
    #[test]
    fn proof_and_ack_are_distinct_and_payloads_are_preserved_exactly() {
        let payload = b"{ \"value\" : \"spaces\" }";
        let bytes = frame(42, 1, 1, ACTION, payload);
        let mut receiver = ReceiveState::new(42, 1);
        assert!(
            receiver
                .read_payload(&mut bytes.as_slice(), PROOF, 0, &TEST_KEY)
                .is_err()
        );
        assert_eq!(
            receiver
                .read_payload(&mut bytes.as_slice(), ACTION, 4096, &TEST_KEY)
                .unwrap(),
            payload
        );
        let proof = frame(42, 1, 1, PROOF, b"");
        let mut receiver = ReceiveState::new(42, 2);
        assert!(
            receiver
                .read_payload(&mut proof.as_slice(), ACCEPTED, 0, &TEST_KEY)
                .is_err()
        );
    }
    #[test]
    fn counters_never_wrap() {
        let mut sender = SendState::new(42, 1);
        let mut receiver = ReceiveState::new(42, 1);
        sender.next = Some(u64::MAX);
        receiver.next = Some(u64::MAX);
        let mut bytes = vec![];
        sender
            .write_payload(&mut bytes, ACTION, b"{}", &TEST_KEY)
            .unwrap();
        receiver
            .read_payload(&mut bytes.as_slice(), ACTION, 4096, &TEST_KEY)
            .unwrap();
        assert!(
            sender
                .write_payload(&mut vec![], ACTION, b"{}", &TEST_KEY)
                .is_err()
        );
        assert!(
            receiver
                .read_payload(&mut bytes.as_slice(), ACTION, 4096, &TEST_KEY)
                .is_err()
        );
    }
}
