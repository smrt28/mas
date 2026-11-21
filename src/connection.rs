//! Persistent authenticated client connection shared by graphical and AI clients.
use crate::{Action, Reply, hard_config, net};
use std::{
    net::{Shutdown, TcpStream, ToSocketAddrs},
    sync::mpsc::{self, SyncSender},
    thread,
    time::Duration,
};

pub fn server_address(address: &str) -> String {
    let address = address.trim();
    let port = hard_config::DEFAULT_PORT;
    if address.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{address}]:{port}")
    } else if !address.is_empty() && (!address.contains(':') || address.ends_with(']')) {
        format!("{address}:{port}")
    } else {
        address.to_string()
    }
}

pub enum Event {
    ConnectingSocket(TcpStream),
    Connected(SyncSender<Action>, TcpStream),
    Reply(Reply),
    Failed(String),
}

/// Fixed diagnostics: never include deserialized payloads or configuration values.
fn failure_message(error: &std::io::Error) -> &'static str {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::UnexpectedEof => "Server closed the connection",
        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted => "Connection reset by server",
        ErrorKind::BrokenPipe => "Connection closed while sending",
        ErrorKind::ConnectionRefused => "Server refused the connection",
        ErrorKind::TimedOut | ErrorKind::WouldBlock => "Server connection timed out",
        ErrorKind::InvalidData => "Server sent invalid or incompatible protocol data",
        ErrorKind::AddrNotAvailable | ErrorKind::NotFound => "Server address could not be resolved",
        _ => "Server connection failed",
    }
}

pub fn spawn(address: String, session: u64, events: SyncSender<(u64, Event)>) {
    thread::spawn(move || {
        let result = (|| -> std::io::Result<TcpStream> {
            let addresses = address.to_socket_addrs()?;
            for a in addresses {
                if let Ok(s) = TcpStream::connect_timeout(&a, Duration::from_secs(3)) {
                    return Ok(s);
                }
            }
            Err(std::io::Error::other("Could not connect to server"))
        })();
        let mut stream = match result {
            Ok(s) => s,
            Err(e) => {
                let _ = events.send((session, Event::Failed(failure_message(&e).into())));
                return;
            }
        };
        // Expose the socket before authentication so cancel can interrupt
        // an in-flight handshake as well as an established connection.
        match stream.try_clone() {
            Ok(socket) => {
                if events
                    .send((session, Event::ConnectingSocket(socket)))
                    .is_err()
                {
                    let _ = stream.shutdown(Shutdown::Both);
                    return;
                }
            }
            Err(error) => {
                let _ = events.send((session, Event::Failed(failure_message(&error).into())));
                return;
            }
        }
        let (mut outgoing, mut incoming) = match net::client_handshake(&mut stream) {
            Ok(states) => states,
            Err(_) => {
                let _ = stream.shutdown(Shutdown::Both);
                let _ = events.send((
                    session,
                    Event::Failed("Server authentication failed".into()),
                ));
                return;
            }
        };
        let _ = stream.set_nodelay(true);
        let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
        let mut writer = match stream.try_clone() {
            Ok(s) => s,
            Err(e) => {
                let _ = events.send((session, Event::Failed(failure_message(&e).into())));
                return;
            }
        };
        let reader = match stream.try_clone() {
            Ok(s) => s,
            Err(e) => {
                let _ = events.send((session, Event::Failed(failure_message(&e).into())));
                return;
            }
        };
        let (tx, rx) = mpsc::sync_channel(32);
        if events
            .send((session, Event::Connected(tx, stream)))
            .is_err()
        {
            return;
        }
        thread::spawn(move || {
            while let Ok(a) = rx.recv() {
                if outgoing.send(&mut writer, &a).is_err() {
                    break;
                }
            }
            let _ = writer.shutdown(Shutdown::Both);
        });
        let mut reader = reader;
        let reason = loop {
            match incoming.receive::<Reply>(&mut reader, net::MAX_FRAME) {
                Ok(reply) => {
                    if events.send((session, Event::Reply(reply))).is_err() {
                        return;
                    }
                }
                Err(e) => break failure_message(&e).into(),
            }
        };
        let _ = events.send((session, Event::Failed(reason)));
    });
}
