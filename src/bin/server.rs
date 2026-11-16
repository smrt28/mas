use marsal_a_spion::{Action, Config, Reply, net, session::Sessions};
use std::{
    collections::HashMap,
    io::BufReader,
    net::{Shutdown, TcpListener, TcpStream},
    sync::mpsc::{self, SyncSender},
    thread,
    time::{Duration, Instant},
};

enum Event {
    Command(u64, Action),
    Gone(u64),
}
struct Peer {
    stream: TcpStream,
    out: SyncSender<Reply>,
    connected: u64,
    rate_start: u64,
    commands: u32,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or("config.toml".into());
    let cfg: Config = toml::from_str(
        &std::fs::read_to_string(&path).map_err(|e| format!("Cannot read {path}: {e}"))?,
    )?;
    cfg.validate()?;
    let listener = TcpListener::bind(&cfg.bind)?;
    listener.set_nonblocking(true)?;
    println!(
        "Maršál a špión — listening on {} (memory only)",
        listener.local_addr()?
    );
    let (tx, rx) = mpsc::sync_channel(1024);
    let mut peers: HashMap<u64, Peer> = HashMap::new();
    let mut sessions = Sessions::default();
    let epoch = Instant::now();
    let mut next = 0;
    let mut last_snapshot = 0;
    loop {
        let now = epoch.elapsed().as_millis() as u64;
        // Bound accept work so a flood cannot starve game clocks.
        for _ in 0..32 {
            let (stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    eprintln!("accept: {e}");
                    break;
                }
            };
            if peers.len() >= cfg.max_connections {
                let _ = stream.shutdown(Shutdown::Both);
                continue;
            }
            stream.set_nodelay(true)?;
            stream.set_write_timeout(Some(Duration::from_secs(3)))?;
            let id = next;
            next += 1;
            let (out, writer_rx) = mpsc::sync_channel(16);
            let mut writer = stream.try_clone()?;
            let writer_events = tx.clone();
            thread::spawn(move || {
                while let Ok(reply) = writer_rx.recv() {
                    if net::write_frame(&mut writer, &reply).is_err() {
                        break;
                    }
                }
                let _ = writer.shutdown(Shutdown::Both);
                let _ = writer_events.send(Event::Gone(id));
            });
            let reader_stream = stream.try_clone()?;
            let reader_events = tx.clone();
            thread::spawn(move || {
                let mut reader = BufReader::new(reader_stream);
                while let Ok(line) = net::read_frame(&mut reader, 4096) {
                    let Ok(action) = serde_json::from_str(&line) else {
                        break;
                    };
                    if reader_events.try_send(Event::Command(id, action)).is_err() {
                        break;
                    }
                }
                let _ = reader.get_ref().shutdown(Shutdown::Both);
                let _ = reader_events.send(Event::Gone(id));
            });
            peers.insert(
                id,
                Peer {
                    stream,
                    out,
                    connected: now,
                    rate_start: now,
                    commands: 0,
                },
            );
        }
        let mut dirty = false;
        // Settle deadlines before processing commands or joining a room.
        dirty |= sessions.tick(now, &cfg);
        for _ in 0..512 {
            let event = match rx.try_recv() {
                Ok(e) => e,
                Err(_) => break,
            };
            match event {
                Event::Gone(id) => {
                    disconnect(id, &mut peers, &mut sessions);
                    dirty = true;
                }
                Event::Command(id, action) => {
                    let Some(peer) = peers.get_mut(&id) else {
                        continue;
                    };
                    if now - peer.rate_start >= 1000 {
                        peer.rate_start = now;
                        peer.commands = 0;
                    }
                    peer.commands += 1;
                    if peer.commands > 100 {
                        disconnect(id, &mut peers, &mut sessions);
                        dirty = true;
                        continue;
                    }
                    let result = sessions.command(id, action, now, &cfg);
                    if let Err(message) = result
                        && peer.out.try_send(Reply::Error { message }).is_err()
                    {
                        disconnect(id, &mut peers, &mut sessions);
                    }
                    dirty = true;
                }
            }
        }
        let expired: Vec<_> = peers
            .iter()
            .filter(|(id, p)| !sessions.contains(**id) && now - p.connected >= 30_000)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            disconnect(id, &mut peers, &mut sessions);
            dirty = true;
        }
        if dirty || now - last_snapshot >= 250 {
            let mut slow = vec![];
            for (id, peer) in &peers {
                if let Some(view) = sessions.view(*id, now)
                    && peer.out.try_send(Reply::State { view }).is_err()
                {
                    slow.push(*id);
                }
            }
            for id in slow {
                disconnect(id, &mut peers, &mut sessions);
            }
            last_snapshot = now;
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn disconnect(id: u64, peers: &mut HashMap<u64, Peer>, sessions: &mut Sessions) {
    if let Some(peer) = peers.remove(&id) {
        let _ = peer.stream.shutdown(Shutdown::Both);
        sessions.disconnect(id);
    }
}
