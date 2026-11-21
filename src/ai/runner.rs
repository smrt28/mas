use super::{AiConfig, Decision, LlmClient, Observer};
use crate::{Action, Reply, connection};
use std::{
    net::{Shutdown, TcpStream},
    sync::{
        Arc,
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

struct Socket(Option<TcpStream>);
impl Drop for Socket {
    fn drop(&mut self) {
        if let Some(socket) = &self.0 {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}

/// The network receiver remains live while HTTPS requests run on a worker.
/// Decisions carry an immutable visible-state identity and are checked again at send time.
pub fn run(config: AiConfig) -> Result<(), &'static str> {
    let api = LlmClient::new(&config)?;
    run_with(config, Arc::new(move |decision| api.decide(decision)))
}
type Decide = dyn Fn(&Decision) -> Result<usize, &'static str> + Send + Sync;
fn run_with(config: AiConfig, api: Arc<Decide>) -> Result<(), &'static str> {
    let (events_tx, events_rx) = mpsc::sync_channel(64);
    connection::spawn(config.address(), 1, events_tx);
    let (answers_tx, answers_rx) = mpsc::sync_channel(1);
    let mut socket = Socket(None);
    let mut actions: Option<SyncSender<Action>> = None;
    let mut observer = Observer::default();
    let mut received = Instant::now();
    let mut request: Option<Decision> = None;
    let mut sent: Option<([u8; 32], Instant)> = None;
    let mut retry_at = Instant::now();
    let mut failures: u32 = 0;
    loop {
        let first = match events_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(event) => Some(event),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(_) => return Err("Game connection worker stopped"),
        };
        // Process all queued state changes before considering an API result.
        for (_, event) in first.into_iter().chain(events_rx.try_iter()) {
            match event {
                connection::Event::ConnectingSocket(stream) => socket.0 = Some(stream),
                connection::Event::Connected(tx, stream) => {
                    socket.0 = Some(stream);
                    tx.try_send(Action::CreateGame {
                        game: config.game.clone(),
                    })
                    .map_err(|_| "Cannot create game")?;
                    actions = Some(tx);
                    println!("Authenticated. Sending game creation request.");
                }
                connection::Event::Reply(Reply::State { view }) => {
                    let first_state = observer.you().is_none();
                    observer.observe(&view)?;
                    if first_state {
                        println!("Game creation confirmed by server.");
                        if matches!(view.phase, crate::Phase::Waiting) {
                            println!("Waiting for an opponent.");
                        }
                    }
                    received = Instant::now();
                    if let Some(winner) = observer.finished() {
                        println!(
                            "{}",
                            match winner {
                                Some(p) if Some(p) == observer.you() => "You win!",
                                Some(_) => "You lose!",
                                None => "Game ended in a draw or was interrupted.",
                            }
                        );
                        return Ok(());
                    }
                }
                connection::Event::Reply(Reply::Error { .. }) => {
                    // The raw text may contain arbitrary server data; keep it out of requests/logs.
                    if observer.you().is_none() {
                        return Err("Server rejected game creation");
                    }
                    eprintln!("Server rejected the pending action; requesting a fresh decision.");
                    sent = None;
                    retry_at = Instant::now() + Duration::from_secs(1);
                }
                connection::Event::Reply(Reply::Games { .. }) => {}
                connection::Event::Failed(reason) => {
                    // The shared worker supplies fixed, payload-free diagnostics.
                    eprintln!("Game connection: {reason}");
                    if actions.is_some() && observer.you().is_none() {
                        return Err(
                            "Server disconnected before confirming game creation. An older mas-server cannot handle create_game: rebuild and restart the server from the same source as mas-ai-client.",
                        );
                    }
                    return Err("Game connection failed or disconnected");
                }
            }
        }
        let current = observer.decision(received.elapsed().as_millis() as u64);
        if let Some((key, at)) = sent {
            if current.as_ref().is_none_or(|decision| decision.key != key) {
                sent = None;
            } else if at.elapsed() > Duration::from_secs(10) {
                return Err("Server did not acknowledge the action");
            }
        }
        if let Ok(answer) = answers_rx.try_recv() {
            let pending = request.take().expect("one API result per request");
            if current
                .as_ref()
                .is_none_or(|decision| decision.key != pending.key)
            {
                println!("Discarded decision for an expired turn or deployment state.");
                failures = 0;
                retry_at = Instant::now();
                continue;
            }
            match answer {
                Ok(id) => {
                    if let Some(action) = observer.validate(&pending, id) {
                        actions
                            .as_ref()
                            .ok_or("Game is disconnected")?
                            .try_send(action)
                            .map_err(|_| "Game connection busy or closed")?;
                        sent = Some((pending.key, Instant::now()));
                        failures = 0;
                    } else {
                        eprintln!("AI returned an unavailable action ID; retrying.");
                        retry_at = Instant::now() + Duration::from_secs(1);
                    }
                }
                Err(message) => {
                    // These are fixed local diagnostics, never response bodies or credentials.
                    eprintln!("{message}");
                    failures = failures.saturating_add(1);
                    retry_at = Instant::now() + Duration::from_secs(1_u64 << failures.min(5));
                }
            }
        }
        if request.is_none()
            && sent.is_none()
            && Instant::now() >= retry_at
            && let Some(decision) = current
        {
            if decision.automatic_action().is_some()
                && let Some(action) = observer.validate(&decision, 0)
            {
                actions
                    .as_ref()
                    .ok_or("Game is disconnected")?
                    .try_send(action)
                    .map_err(|_| "Game connection busy or closed")?;
                sent = Some((decision.key, Instant::now()));
                println!("Only one interface action is available; sent without an LLM request.");
                continue;
            }
            let worker_api = api.clone();
            let answers = answers_tx.clone();
            let pending = decision.clone();
            thread::spawn(move || {
                let _ = answers.send(worker_api(&pending));
            });
            request = Some(decision);
        }
    }
}

#[cfg(all(test, feature = "network-tests"))]
mod tests {
    use super::*;
    use crate::{Config, Game, Phase, Piece, Rank, net};
    use std::{
        net::TcpListener,
        sync::atomic::{AtomicUsize, Ordering},
    };

    fn config(port: u16) -> AiConfig {
        toml::from_str(&format!("hostname='127.0.0.1'\nport={port}\ngame='robot'\nopenai_api_key='test-only-not-a-real-key'\nmodel='mock'\n")).unwrap()
    }
    #[test]
    fn older_server_rejecting_create_reports_rebuild_guidance_without_calling_ai() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let (_, mut incoming) = net::server_handshake(&mut stream).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            // A server built before CreateGame cannot deserialize this action.
            #[derive(serde::Deserialize)]
            #[serde(tag = "type", rename_all = "snake_case")]
            enum LegacyAction {
                ListGames,
            }
            assert!(incoming.receive::<LegacyAction>(&mut stream, 4096).is_err());
            let _ = stream.shutdown(Shutdown::Both);
        });
        let chooser: Arc<Decide> = Arc::new(|_| panic!("No AI request before a game exists"));
        let error = run_with(config(port), chooser).unwrap_err();
        assert!(error.contains("create_game"));
        assert!(error.contains("rebuild and restart the server"));
        server.join().unwrap();
    }
    #[test]
    fn headless_runner_creates_deploys_moves_and_stops_on_pushed_finish() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let (mut outgoing, mut incoming) = net::server_handshake(&mut stream).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            outgoing
                .send(&mut stream, &Reply::Games { games: vec![] })
                .unwrap();
            assert_eq!(
                incoming.receive::<Action>(&mut stream, 4096).unwrap(),
                Action::CreateGame {
                    game: "robot".into()
                }
            );
            let cfg = Config::default();
            let mut game = Game::new(0, &cfg);
            outgoing
                .send(
                    &mut stream,
                    &Reply::State {
                        view: game.view(0, "robot", 0),
                    },
                )
                .unwrap();
            game.start(0, &cfg);
            game.apply(1, Action::AutoPlace, 0, &cfg).unwrap();
            game.apply(1, Action::Ready, 0, &cfg).unwrap();
            outgoing
                .send(
                    &mut stream,
                    &Reply::State {
                        view: game.view(0, "robot", 0),
                    },
                )
                .unwrap();
            let mut placed = 0;
            loop {
                let action = incoming.receive::<Action>(&mut stream, 4096).unwrap();
                let moved = matches!(action, Action::Move { .. });
                if matches!(action, Action::Place { .. }) {
                    placed += 1;
                }
                game.apply(0, action, 0, &cfg).unwrap();
                if matches!(game.phase, Phase::Playing) {
                    game.turn = 0;
                }
                if moved {
                    game.finish(Some(0), "Enemy flag captured");
                }
                outgoing
                    .send(
                        &mut stream,
                        &Reply::State {
                            view: game.view(0, "robot", 0),
                        },
                    )
                    .unwrap();
                if moved {
                    break;
                }
            }
            assert_eq!(placed, 40);
            assert!(incoming.receive::<Action>(&mut stream, 4096).is_err());
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let chooser: Arc<Decide> = Arc::new(move |decision| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(decision
                .actions
                .iter()
                .find(|choice| {
                    matches!(
                        choice.action,
                        Action::Place { .. } | Action::Ready | Action::Move { .. }
                    )
                })
                .unwrap()
                .id)
        });
        assert_eq!(run_with(config(port), chooser), Ok(()));
        server.join().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 41);
    }
    #[test]
    fn pending_responses_expire_and_invalid_current_answers_retry() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let release = std::sync::Mutex::new(release_rx);
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let chooser: Arc<Decide> = Arc::new(move |decision| {
            let call = count.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                started_tx.send(()).unwrap();
                release.lock().unwrap().recv().unwrap();
            }
            if call == 1 {
                return Ok(usize::MAX);
            }
            Ok(decision
                .actions
                .iter()
                .find(|choice| matches!(choice.action, Action::Move { .. }))
                .unwrap()
                .id)
        });
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let (mut outgoing, mut incoming) = net::server_handshake(&mut stream).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            assert!(matches!(
                incoming.receive::<Action>(&mut stream, 4096).unwrap(),
                Action::CreateGame { .. }
            ));
            let cfg = Config::default();
            let mut game = Game::new(0, &cfg);
            game.phase = Phase::Playing;
            game.turn = 0;
            game.board[70] = Some(Piece {
                owner: 0,
                rank: Rank::Scout,
            });
            game.board[0] = Some(Piece {
                owner: 1,
                rank: Rank::Flag,
            });
            outgoing
                .send(
                    &mut stream,
                    &Reply::State {
                        view: game.view(0, "robot", 0),
                    },
                )
                .unwrap();
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            game.turn = 1;
            game.ply = 1;
            outgoing
                .send(
                    &mut stream,
                    &Reply::State {
                        view: game.view(0, "robot", 0),
                    },
                )
                .unwrap();
            // Wait for the pushed state to be read before unblocking the mock API.
            thread::sleep(Duration::from_millis(100));
            release_tx.send(()).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_millis(200)))
                .unwrap();
            assert!(incoming.receive::<Action>(&mut stream, 4096).is_err());
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            game.turn = 0;
            game.ply = 2;
            outgoing
                .send(
                    &mut stream,
                    &Reply::State {
                        view: game.view(0, "robot", 0),
                    },
                )
                .unwrap();
            let action = incoming.receive::<Action>(&mut stream, 4096).unwrap();
            assert!(matches!(action, Action::Move { from: 70, .. }));
            game.finish(Some(0), "Enemy flag captured");
            outgoing
                .send(
                    &mut stream,
                    &Reply::State {
                        view: game.view(0, "robot", 0),
                    },
                )
                .unwrap();
            assert!(incoming.receive::<Action>(&mut stream, 4096).is_err());
        });
        assert_eq!(run_with(config(port), chooser), Ok(()));
        server.join().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
}
