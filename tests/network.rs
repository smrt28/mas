//! Run with: cargo test --no-default-features --features network-tests --test network
#![cfg(feature = "network-tests")]
use marsal_a_spion::{net, *};
use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
struct Server {
    child: Child,
    config: PathBuf,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.config);
    }
}
struct Player {
    stream: TcpStream,
    outgoing: net::SendState,
    incoming: net::ReceiveState,
}
impl Player {
    fn new(port: u16) -> Self {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let (outgoing, mut incoming) = net::client_handshake(&mut stream).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        assert!(matches!(
            incoming
                .receive::<Reply>(&mut stream, net::MAX_FRAME)
                .unwrap(),
            Reply::Games { .. }
        ));
        Self {
            stream,
            outgoing,
            incoming,
        }
    }
    fn send(&mut self, a: Action) {
        self.outgoing.send(&mut self.stream, &a).unwrap();
    }
    fn reply(&mut self) -> Reply {
        self.incoming
            .receive(&mut self.stream, net::MAX_FRAME)
            .unwrap()
    }
    fn until(&mut self, predicate: impl Fn(&View) -> bool) -> View {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            assert!(
                Instant::now() < deadline,
                "Timed out waiting for game state"
            );
            match self.reply() {
                Reply::State { view } if predicate(&view) => return view,
                Reply::Error { message } => panic!("Server rejected action: {message}"),
                _ => {}
            }
        }
    }
    fn until_error(&mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                Instant::now() < deadline,
                "Timed out waiting for claim rejection"
            );
            if let Reply::Error { message } = self.reply() {
                return message;
            }
        }
    }
    fn join(&mut self, name: &str) {
        self.send(Action::Join { game: name.into() });
    }
}
fn raw_connection(port: u16) -> (TcpStream, [u8; 12]) {
    use std::io::Read;
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut challenge = [0; 12];
    stream.read_exact(&mut challenge).unwrap();
    assert_eq!(&challenge[..4], b"MAS\x01");
    (stream, challenge)
}
fn assert_closed(mut stream: TcpStream) {
    use std::io::{ErrorKind, Read};
    let mut byte = [0];
    match stream.read(&mut byte) {
        Ok(0) => {}
        Err(e)
            if matches!(
                e.kind(),
                ErrorKind::ConnectionReset
                    | ErrorKind::UnexpectedEof
                    | ErrorKind::ConnectionAborted
            ) => {}
        _ => panic!("Rejected connection must close without sending game data"),
    }
}
fn proof(challenge: &[u8; 12], key: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut bytes = vec![];
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    bytes.extend_from_slice(&challenge[4..]);
    bytes.push(1); // client to server
    bytes.extend_from_slice(&1_u64.to_be_bytes());
    bytes.push(1); // proof, with empty payload
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
    mac.update(b"MAS-HMAC\0");
    mac.update(&bytes);
    bytes.extend_from_slice(&mac.finalize().into_bytes());
    bytes
}
fn check_rejected_connections(port: u16) {
    use std::io::{Read, Write};
    // No secret material is printed. This is the same generated build input as
    // the distributed client/server, not a fixed development bypass key.
    let key = include_bytes!(concat!(env!("OUT_DIR"), "/shared-key.bin"));
    let (mut stream, challenge) = raw_connection(port);
    stream.write_all(&proof(&challenge, &[0; 32])).unwrap();
    assert_closed(stream);
    let (mut stream, _) = raw_connection(port);
    // Rejection may close the socket before the legacy writer finishes.
    let _ = net::write_frame(
        &mut stream,
        &Action::Join {
            game: "unauthenticated".into(),
        },
    );
    assert_closed(stream);
    // An idle peer receives no game data and is closed at the admission deadline.
    let (stream, _) = raw_connection(port);
    let started = Instant::now();
    assert_closed(stream);
    assert!(started.elapsed() < Duration::from_secs(5));
    // Record a valid proof, authenticate it, then replay it on a new connection.
    let (mut stream, first_challenge) = raw_connection(port);
    let recorded_proof = proof(&first_challenge, key);
    stream.write_all(&recorded_proof).unwrap();
    let mut ack = [0; 56];
    stream.read_exact(&mut ack).unwrap();
    stream.write_all(&recorded_proof).unwrap(); // duplicate sequence 1
    assert_closed(stream);
    let (mut stream, next_challenge) = raw_connection(port);
    assert_ne!(first_challenge, next_challenge);
    stream.write_all(&recorded_proof).unwrap();
    assert_closed(stream);
    // Tampering and duplicate application frames after a valid handshake.
    let mut player = Player::new(port);
    let mut captured = vec![];
    player
        .outgoing
        .send(&mut captured, &Action::ListGames)
        .unwrap();
    let mut tampered = captured.clone();
    tampered[24] ^= 1;
    player.stream.write_all(&tampered).unwrap();
    assert_closed(player.stream);
    let mut player = Player::new(port);
    let mut captured = vec![];
    player
        .outgoing
        .send(&mut captured, &Action::ListGames)
        .unwrap();
    player.stream.write_all(&captured).unwrap();
    assert!(matches!(player.reply(), Reply::Games { .. }));
    player.stream.write_all(&captured).unwrap();
    assert_closed(player.stream);
    // Reconnection resets counters but never accepts the previous session's bytes.
    let mut player = Player::new(port);
    player.stream.write_all(&captured).unwrap();
    assert_closed(player.stream);
    let mut browser = Player::new(port);
    browser.send(Action::ListGames);
    assert!(matches!(browser.reply(), Reply::Games { games } if games.is_empty()));
}
#[test]
fn tcp_matchmaking_privacy_moves_timeouts_and_disconnects() {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = std::env::temp_dir().join(format!("mas-network-{}.toml", std::process::id()));
    fs::write(&config,format!("bind = \"127.0.0.1:{port}\"\nplacement_seconds = 3\nturn_delay_seconds = 1\nmain_time_seconds = 1\n")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_mas-server"))
        .arg(&config)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut server = Server { child, config };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        assert!(
            server.child.try_wait().unwrap().is_none(),
            "Server exited before listening"
        );
        assert!(Instant::now() < deadline, "Server did not start");
        thread::sleep(Duration::from_millis(20));
    }
    check_rejected_connections(port);
    let mut browser = Player::new(port);
    let mut a = Player::new(port);
    a.join("test match");
    a.until(|v| matches!(v.phase, Phase::Waiting));
    let Reply::Games { games } = browser.reply() else {
        panic!("Expected lobby list")
    };
    assert_eq!(games, vec!["test match"]);
    let mut b = Player::new(port);
    b.send(Action::JoinWaiting {
        game: games[0].clone(),
    });
    a.until(|v| matches!(v.phase, Phase::Placement));
    b.until(|v| matches!(v.phase, Phase::Placement));
    assert!(matches!(browser.reply(), Reply::Games { games } if games.is_empty()));
    browser.send(Action::JoinWaiting {
        game: "test match".into(),
    });
    assert!(matches!(browser.reply(), Reply::Error { .. }));
    let mut third = Player::new(port);
    third.join("test match");
    assert!(matches!(third.reply(), Reply::Error { .. }));
    third.join("other match");
    third.until(|v| matches!(v.phase, Phase::Waiting));
    // A second observer receives additions/removals without sending any request.
    let mut observer = Player::new(port);
    let mut temporary = Player::new(port);
    temporary.join("temporary match");
    temporary.until(|v| matches!(v.phase, Phase::Waiting));
    assert!(
        matches!(observer.reply(), Reply::Games { games } if games == vec!["other match", "temporary match"])
    );
    drop(temporary);
    assert!(matches!(observer.reply(), Reply::Games { games } if games == vec!["other match"]));
    drop(observer);
    // Reauthentication receives a fresh initial list on the same protocol.
    let _reconnected_observer = Player::new(port);
    // Guaranteed front-row mobile piece, then preserve it while filling the army.
    a.send(Action::Place {
        at: 70,
        rank: Rank::Marshal,
    });
    a.send(Action::AutoPlace);
    a.send(Action::Ready);
    let av = a.until(|v| v.ready[0]);
    assert_eq!(av.board.iter().flatten().count(), 40);
    assert!(av.board.iter().flatten().all(|p| p.owner == 0));
    b.send(Action::Place {
        at: 30,
        rank: Rank::Marshal,
    });
    b.send(Action::AutoPlace);
    b.send(Action::Ready);
    let av = a.until(|v| matches!(v.phase, Phase::Playing));
    let bv = b.until(|v| matches!(v.phase, Phase::Playing));
    for v in [&av, &bv] {
        assert_eq!(v.board.len(), CELLS);
        assert_eq!(v.board.iter().flatten().count(), 80);
        for owner in 0..2 {
            for rank in Rank::ALL {
                assert_eq!(v.remaining(owner, rank), rank.count());
            }
        }
        assert!(
            v.board
                .iter()
                .flatten()
                .filter(|p| p.owner != v.you)
                .all(|p| p.rank.is_none())
        );
    }
    // Both marshals advance through the neutral column and meet on move four.
    // The same authoritative capture reaches both authenticated clients.
    for ply in 1..=4 {
        let you = (av.turn + ply - 1) % 2;
        let from = match (you, ply <= 2) {
            (0, true) => 70,
            (1, true) => 30,
            (0, false) => 60,
            _ => 40,
        };
        let to = if ply > 2 {
            50
        } else if you == 0 {
            60
        } else {
            40
        };
        if you == 0 {
            a.send(Action::Move { from, to });
        } else {
            b.send(Action::Move { from, to });
        }
        let first = a.until(|v| v.ply == ply as u32);
        let second = b.until(|v| v.ply == ply as u32);
        if ply == 4 {
            let capture = first.motion.as_ref().unwrap();
            assert_eq!((capture.ply, capture.from, capture.to), (4, from, 50));
            assert!(capture.piece_lost && capture.defender_lost);
            assert_eq!(capture.piece.rank, Some(Rank::Marshal));
            assert_eq!(
                serde_json::to_value(&first.motion).unwrap(),
                serde_json::to_value(&second.motion).unwrap()
            );
            assert!(first.board[50].is_none() && second.board[50].is_none());
            for view in [&first, &second] {
                for owner in 0..2 {
                    for rank in Rank::ALL {
                        assert_eq!(
                            view.remaining(owner, rank),
                            if rank == Rank::Marshal {
                                0
                            } else {
                                rank.count()
                            }
                        );
                    }
                    assert_eq!(
                        Rank::ALL
                            .into_iter()
                            .map(|rank| view.remaining(owner, rank))
                            .sum::<usize>(),
                        39
                    );
                }
            }
        } else {
            let first = first.motion.as_ref().unwrap();
            let second = second.motion.as_ref().unwrap();
            assert_eq!((first.ply, first.from, first.to), (ply as u32, from, to));
            assert_eq!(
                (first.from, first.to, first.elapsed_ms),
                (second.from, second.to, second.elapsed_ms)
            );
            assert!(first.defender.is_none() && second.defender.is_none());
            assert!(!first.piece_lost && !second.piece_lost);
        }
    }
    // Expiry leaves play active and makes the current turn claimable.
    let expired = a.until(|v| matches!(v.phase, Phase::Playing) && v.clocks.claimable);
    let active = expired.turn;
    let active_view = if active == 0 {
        expired.clone()
    } else {
        b.until(|v| v.ply == expired.ply && v.clocks.claimable)
    };
    assert_eq!(active_view.clocks.main_ms[active], 0);
    assert_eq!(active_view.clocks.delay_ms, 0);
    let (from, to) = (0..CELLS)
        .find_map(|from| {
            active_view
                .legal_targets(from)
                .first()
                .copied()
                .map(|to| (from, to))
        })
        .unwrap();
    if active == 0 {
        a.send(Action::Move { from, to });
    } else {
        b.send(Action::Move { from, to });
    }
    let next = a.until(|v| v.ply == expired.ply + 1);
    b.until(|v| v.ply == expired.ply + 1);
    assert!(!next.clocks.claimable);
    assert_eq!(next.clocks.main_ms[active], 0);
    // A previously displayed claim may arrive after the legal move. It cannot
    // affect the new turn, even though its sender is still authenticated.
    let stale = Action::ClaimVictory {
        turn_id: expired.ply,
    };
    if active == 0 {
        b.send(stale);
        assert!(b.until_error().contains("current turn"));
    } else {
        a.send(stale);
        assert!(a.until_error().contains("current turn"));
    }
    let expired_next = a.until(|v| v.ply == next.ply && v.clocks.claimable);
    let claimant = 1 - expired_next.turn;
    let action = Action::ClaimVictory {
        turn_id: expired_next.ply,
    };
    if claimant == 0 {
        a.send(action);
    } else {
        b.send(action);
    }
    let end = a.until(|v| matches!(v.phase, Phase::Finished { .. }));
    assert!(
        matches!(end.phase, Phase::Finished { winner: Some(winner), ref reason }
        if winner == claimant && reason == "Victory claimed on time")
    );
    drop(a);
    drop(b);
    // Independent room still works. A disconnect ends only its own match.
    let mut fourth = Player::new(port);
    fourth.join("other match");
    third.until(|v| matches!(v.phase, Phase::Placement));
    drop(fourth);
    let end = third.until(|v| matches!(v.phase, Phase::Finished { .. }));
    assert!(matches!(
        end.phase,
        Phase::Finished {
            winner: None,
            ref reason
        } if reason == "Opponent disconnected"
    ));
    // Setup deadline also fires without clients sending Ready.
    let mut c = Player::new(port);
    let mut d = Player::new(port);
    c.join("setup clock");
    d.join("setup clock");
    let v = c.until(|v| matches!(v.phase, Phase::Playing));
    assert_eq!(v.board.len(), CELLS);
    assert_eq!(v.board.iter().flatten().count(), 80);
    assert_eq!(v.ready, [true, true]);
}
