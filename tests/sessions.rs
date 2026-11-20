use marsal_a_spion::{session::Sessions, *};
fn join(s: &mut Sessions, id: u64, name: &str, now: u64, cfg: &Config) -> Result<(), String> {
    s.command(id, Action::Join { game: name.into() }, now, cfg)
}
#[test]
fn named_matchmaking_and_third_player_rejection() {
    let mut s = Sessions::default();
    let cfg = Config::default();
    join(&mut s, 10, "  Prague  ", 0, &cfg).unwrap();
    assert_eq!(s.room_count(), 1);
    assert_eq!(s.view(10, 0).unwrap().phase, Phase::Waiting);
    join(&mut s, 20, "Prague", 100, &cfg).unwrap();
    assert_eq!(s.view(10, 100).unwrap().phase, Phase::Placement);
    assert_eq!(s.view(20, 100).unwrap().remaining_ms, 120_000);
    assert!(join(&mut s, 30, "Prague", 100, &cfg).is_err());
    assert!(!s.contains(30));
    join(&mut s, 30, "Other game", 100, &cfg).unwrap();
    assert_eq!(s.room_count(), 2);
    assert_eq!(s.view(30, 100).unwrap().phase, Phase::Waiting);
}
#[test]
fn disconnect_breaks_game_and_last_departure_frees_name() {
    let mut s = Sessions::default();
    let cfg = Config::default();
    join(&mut s, 0, "x", 0, &cfg).unwrap();
    join(&mut s, 1, "x", 0, &cfg).unwrap();
    s.disconnect(0);
    assert!(matches!(
        s.view(1, 0).unwrap().phase,
        Phase::Finished {
            winner: None,
            ref reason
        } if reason == "Opponent disconnected"
    ));
    assert!(join(&mut s, 2, "x", 0, &cfg).is_err());
    s.disconnect(1);
    assert_eq!(s.room_count(), 0);
    assert!(s.view(1, 0).is_none());
    join(&mut s, 2, "x", 0, &cfg).unwrap();
    assert_eq!(s.view(2, 0).unwrap().phase, Phase::Waiting);
}
#[test]
fn lobby_expiry_cannot_be_joined_at_deadline() {
    let mut s = Sessions::default();
    let cfg = Config {
        waiting_seconds: 1,
        ..Default::default()
    };
    join(&mut s, 0, "x", 0, &cfg).unwrap();
    assert!(join(&mut s, 1, "x", 1000, &cfg).is_err());
    assert!(matches!(
        s.view(0, 1000).unwrap().phase,
        Phase::Finished { winner: None, .. }
    ));
}
#[test]
fn independent_room_clocks_and_custom_limits() {
    let mut s = Sessions::default();
    let cfg = Config {
        placement_seconds: 2,
        move_seconds: 1,
        max_missed_turns: 1,
        ..Default::default()
    };
    for (id, name, now) in [(0, "a", 0), (1, "a", 0), (2, "b", 500), (3, "b", 500)] {
        join(&mut s, id, name, now, &cfg).unwrap();
    }
    s.tick(2000, &cfg);
    assert_eq!(s.view(0, 2000).unwrap().phase, Phase::Playing);
    assert_eq!(s.view(2, 2000).unwrap().phase, Phase::Placement);
    s.tick(2500, &cfg);
    assert_eq!(s.view(2, 2500).unwrap().phase, Phase::Playing);
    s.tick(3000, &cfg);
    assert!(matches!(
        s.view(0, 3000).unwrap().phase,
        Phase::Finished { .. }
    ));
    assert_eq!(s.view(2, 3000).unwrap().phase, Phase::Playing);
}
#[test]
fn invalid_names_and_limits_do_not_allocate_rooms() {
    let mut s = Sessions::default();
    let cfg = Config {
        max_rooms: 1,
        ..Default::default()
    };
    for name in ["", "   ", "a\nb", &"x".repeat(49)] {
        assert!(join(&mut s, 0, name, 0, &cfg).is_err());
        assert_eq!(s.room_count(), 0);
    }
    join(&mut s, 0, "a", 0, &cfg).unwrap();
    assert!(join(&mut s, 1, "b", 0, &cfg).is_err());
    join(&mut s, 1, "a", 0, &cfg).unwrap();
    assert!(join(&mut s, 0, "b", 0, &cfg).is_err());
    assert_eq!(s.room_count(), 1);
}
#[test]
fn unjoined_clients_cannot_issue_game_actions() {
    let mut s = Sessions::default();
    let cfg = Config::default();
    assert!(s.command(99, Action::AutoPlace, 0, &cfg).is_err());
    assert!(s.command(99, Action::Resign, 0, &cfg).is_err());
    s.disconnect(99);
    assert_eq!(s.room_count(), 0);
}
#[test]
fn complete_matches_with_serialized_commands_and_private_views() {
    use rand::seq::IndexedRandom;
    // Real coordinator and rules, with the wire codec at both boundaries.
    for campaign in 0..12 {
        let cfg = Config {
            quiet_move_limit: 40,
            ..Default::default()
        };
        let mut s = Sessions::default();
        join(&mut s, 0, "match", 0, &cfg).unwrap();
        join(&mut s, 1, "match", 0, &cfg).unwrap();
        for id in 0..2 {
            s.command(id, Action::AutoPlace, 0, &cfg).unwrap();
            s.command(id, Action::Ready, 0, &cfg).unwrap();
        }
        let mut ended = false;
        for now in 1..2000 {
            let v = s.view(0, now).unwrap();
            if matches!(v.phase, Phase::Finished { .. }) {
                ended = true;
                break;
            }
            let id = v.turn as u64;
            let active = s.view(id, now).unwrap();
            let moves: Vec<_> = (0..CELLS)
                .flat_map(|from| {
                    active
                        .legal_targets(from)
                        .into_iter()
                        .map(move |to| (from, to))
                })
                .collect();
            let &(from, to) = moves
                .choose(&mut rand::rng())
                .expect("Playing phase has legal moves");
            let action = Action::Move { from, to };
            let mut bytes = vec![];
            net::write_frame(&mut bytes, &action).unwrap();
            let line = net::read_frame(&mut std::io::Cursor::new(bytes), 4096).unwrap();
            s.command(id, serde_json::from_str(&line).unwrap(), now, &cfg)
                .unwrap();
            for player in 0..2 {
                let view = s.view(player, now).unwrap();
                assert_eq!(view.board.len(), CELLS);
                assert_eq!(
                    view.board.iter().flatten().count() + view.captured.len(),
                    80
                );
                assert!(
                    view.board
                        .iter()
                        .flatten()
                        .filter(|p| p.owner != player as usize)
                        .all(|p| p.rank.is_some() == matches!(view.phase, Phase::Finished { .. }))
                );
                let mut bytes = vec![];
                net::write_frame(&mut bytes, &Reply::State { view }).unwrap();
                let line =
                    net::read_frame(&mut std::io::Cursor::new(bytes), net::MAX_FRAME).unwrap();
                let _: Reply = serde_json::from_str(&line).unwrap();
            }
        }
        if !ended {
            s.command(0, Action::Resign, 2000, &cfg).unwrap();
        }
        assert!(
            matches!(s.view(1, 2000).unwrap().phase, Phase::Finished { .. }),
            "campaign {campaign}"
        );
        s.disconnect(0);
        s.disconnect(1);
        assert_eq!(s.room_count(), 0);
    }
}
#[test]
fn client_legal_targets_match_authoritative_rules() {
    let cfg = Config::default();
    let mut g = Game::new(0, &cfg);
    g.start(0, &cfg);
    g.tick(120000, &cfg);
    let view = g.view(g.turn, "test", 120000);
    for from in 0..CELLS {
        let targets = view.legal_targets(from);
        for to in 0..CELLS {
            assert_eq!(targets.contains(&to), g.legal(g.turn, from, to));
        }
    }
}

#[test]
fn waiting_list_only_contains_joinable_rooms_and_stale_entries_never_create_rooms() {
    let cfg = Config {
        waiting_seconds: 1,
        ..Config::default()
    };
    let mut sessions = Sessions::default();
    assert!(sessions.waiting_games(0).is_empty());
    join(&mut sessions, 0, "Zulu", 0, &cfg).unwrap();
    join(&mut sessions, 1, "Alpha", 0, &cfg).unwrap();
    assert_eq!(sessions.waiting_games(10), vec!["Alpha", "Zulu"]);
    sessions
        .command(
            2,
            Action::JoinWaiting {
                game: "Alpha".into(),
            },
            10,
            &cfg,
        )
        .unwrap();
    assert_eq!(sessions.view(2, 10).unwrap().phase, Phase::Placement);
    assert_eq!(sessions.waiting_games(10), vec!["Zulu"]);
    assert!(
        sessions
            .command(
                3,
                Action::JoinWaiting {
                    game: "Alpha".into()
                },
                10,
                &cfg
            )
            .is_err()
    );
    sessions.disconnect(0);
    assert!(sessions.waiting_games(10).is_empty());
    assert!(
        sessions
            .command(
                3,
                Action::JoinWaiting {
                    game: "Zulu".into()
                },
                10,
                &cfg
            )
            .is_err()
    );
    assert_eq!(sessions.room_count(), 1);
    join(&mut sessions, 4, "Expiring", 0, &cfg).unwrap();
    // Listing and joining both respect expiry, even before the next server tick.
    assert!(sessions.waiting_games(1000).is_empty());
    assert!(
        sessions
            .command(
                5,
                Action::JoinWaiting {
                    game: "Expiring".into()
                },
                1000,
                &cfg
            )
            .is_err()
    );
    assert!(!sessions.contains(3));
    assert!(!sessions.contains(5));
}
