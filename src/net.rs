//! Bounded newline-delimited JSON transport, shared by server and native client.
use std::io::{self, BufRead, Read, Write};

pub const MAX_FRAME: u64 = 65536;
pub fn read_frame(reader: &mut impl BufRead, max: u64) -> io::Result<String> {
    let mut bytes = Vec::new();
    let n = (&mut *reader).take(max + 1).read_until(b'\n', &mut bytes)?;
    if n == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Connection closed",
        ));
    }
    if n as u64 > max || bytes.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Frame exceeds limit",
        ));
    }
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
pub fn write_frame(writer: &mut impl Write, value: &impl serde::Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()
}
