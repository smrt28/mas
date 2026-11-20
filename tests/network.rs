//! Run with: cargo test --no-default-features --features network-tests --test network
#![cfg(feature = "network-tests")]
use marsal_a_spion::{net, *};
use std::{
    fs,
    io::BufReader,
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
    reader: BufReader<TcpStream>,
}
impl Player {
    fn new(port: u16) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        Self {
            reader: BufReader::new(stream.try_clone().unwrap()),
            stream,
        }
    }
    fn send(&mut self, a: Action) {
        net::write_frame(&mut self.stream, &a).unwrap();
    }
    fn reply(&mut self) -> Reply {
        let line = net::read_frame(&mut self.reader, net::MAX_FRAME).unwrap();
        serde_json::from_str(&line).unwrap()
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
    fn join(&mut self, name: &str) {
        self.send(Action::Join { game: name.into() });
    }
}
#[test]
fn tcp_matchmaking_privacy_moves_timeouts_and_disconnects() {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = std::env::temp_dir().join(format!("mas-network-{}.toml", std::process::id()));
    fs::write(&config,format!("bind = \"127.0.0.1:{port}\"\nplacement_seconds = 3\nmove_seconds = 2\nmax_missed_turns = 1\n")).unwrap();
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
    let mut a = Player::new(port);
    a.join("test match");
    a.until(|v| matches!(v.phase, Phase::Waiting));
    let mut b = Player::new(port);
    b.join("test match");
    a.until(|v| matches!(v.phase, Phase::Placement));
    b.until(|v| matches!(v.phase, Phase::Placement));
    let mut third = Player::new(port);
    third.join("test match");
    assert!(matches!(third.reply(), Reply::Error { .. }));
    third.join("other match");
    third.until(|v| matches!(v.phase, Phase::Waiting));
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
        assert!(
            v.board
                .iter()
                .flatten()
                .filter(|p| p.owner != v.you)
                .all(|p| p.rank.is_none())
        );
    }
    if av.turn == 0 {
        a.send(Action::Move { from: 70, to: 60 });
    } else {
        b.send(Action::Move { from: 30, to: 40 });
    }
    a.until(|v| v.ply == 1);
    b.until(|v| v.ply == 1);
    // The server independently enforces the configured two-second turn limit.
    let end = a.until(|v| matches!(v.phase, Phase::Finished { .. }));
    assert!(matches!(
        end.phase,
        Phase::Finished {
            winner: Some(_),
            ..
        }
    ));
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
