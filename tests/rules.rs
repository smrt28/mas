use marsal_a_spion::*;
fn sparse() -> (Game, Config) {
    let cfg = Config::default();
    let mut g = Game::new(0, &cfg);
    g.phase = Phase::Playing;
    g.deadline = 5000;
    // Both sides retain an unblocked mobile piece for stalemate checks.
    g.board[90] = Some(Piece {
        owner: 0,
        rank: Rank::Miner,
    });
    g.board[9] = Some(Piece {
        owner: 1,
        rank: Rank::Miner,
    });
    (g, cfg)
}
fn put(g: &mut Game, at: usize, owner: usize, rank: Rank) {
    g.board[at] = Some(Piece { owner, rank });
}
#[test]
fn army_is_exactly_forty() {
    assert_eq!(Rank::ALL.into_iter().map(Rank::count).sum::<usize>(), 40);
}
#[test]
fn setup_validates_zone_inventory_and_readiness() {
    let cfg = Config::default();
    let mut g = Game::new(0, &cfg);
    g.start(0, &cfg);
    assert!(
        g.apply(
            0,
            Action::Place {
                at: 0,
                rank: Rank::Marshal
            },
            0,
            &cfg
        )
        .is_err()
    );
    assert!(
        g.apply(
            0,
            Action::Place {
                at: CELLS,
                rank: Rank::Marshal
            },
            0,
            &cfg
        )
        .is_err()
    );
    g.apply(
        0,
        Action::Place {
            at: CELLS - 1,
            rank: Rank::Marshal,
        },
        0,
        &cfg,
    )
    .unwrap();
    assert!(
        g.apply(
            0,
            Action::Place {
                at: CELLS - 2,
                rank: Rank::Marshal
            },
            0,
            &cfg
        )
        .is_err()
    );
    assert!(g.apply(0, Action::Ready, 0, &cfg).is_err());
    g.apply(0, Action::AutoPlace, 0, &cfg).unwrap();
    assert_eq!(g.board[CELLS - 1].unwrap().rank, Rank::Marshal);
    g.apply(0, Action::Ready, 0, &cfg).unwrap();
    assert!(
        g.apply(0, Action::Remove { at: CELLS - 1 }, 0, &cfg)
            .is_err()
    );
    g.apply(1, Action::AutoPlace, 0, &cfg).unwrap();
    g.apply(1, Action::Ready, 0, &cfg).unwrap();
    assert_eq!(g.phase, Phase::Playing);
    for p in 0..2 {
        for r in Rank::ALL {
            assert_eq!(
                g.board
                    .iter()
                    .flatten()
                    .filter(|v| v.owner == p && v.rank == r)
                    .count(),
                r.count()
            );
        }
    }
}
#[test]
fn timeout_preserves_placement_and_fills_both_armies() {
    let cfg = Config::default();
    let mut g = Game::new(0, &cfg);
    g.start(10, &cfg);
    g.apply(
        0,
        Action::Place {
            at: CELLS - 1,
            rank: Rank::Flag,
        },
        20,
        &cfg,
    )
    .unwrap();
    assert!(!g.tick(120009, &cfg));
    assert!(g.tick(120010, &cfg));
    assert_eq!(g.phase, Phase::Playing);
    assert_eq!(g.board.iter().flatten().count(), 80);
    assert_eq!(g.board[CELLS - 1].unwrap().rank, Rank::Flag);
}
#[test]
fn setup_is_private_on_wire() {
    let cfg = Config::default();
    let mut g = Game::new(0, &cfg);
    g.start(0, &cfg);
    g.apply(0, Action::AutoPlace, 0, &cfg).unwrap();
    g.apply(1, Action::AutoPlace, 0, &cfg).unwrap();
    let v = g.view(0, "secret", 0);
    assert_eq!(v.board.iter().flatten().count(), 40);
    assert!(v.board.iter().flatten().all(|p| p.owner == 0));
    let reply = serde_json::to_string(&Reply::State { view: v }).unwrap();
    let parsed: Reply = serde_json::from_str(&reply).unwrap();
    let Reply::State { view } = parsed else {
        panic!()
    };
    assert!(view.board[..40].iter().all(Option::is_none));
    g.phase = Phase::Playing;
    let v = g.view(0, "secret", 0);
    assert_eq!(v.board.iter().flatten().count(), 80);
    assert!(
        v.board
            .iter()
            .flatten()
            .filter(|p| p.owner == 1)
            .all(|p| p.rank.is_none())
    );
}
#[test]
fn diagonal_moves_but_never_diagonal_attacks() {
    let (mut g, _) = sparse();
    put(&mut g, 44, 0, Rank::Captain);
    assert!(g.legal(0, 44, 55));
    put(&mut g, 55, 1, Rank::Spy);
    assert!(!g.legal(0, 44, 55));
    put(&mut g, 45, 1, Rank::Spy);
    assert!(g.legal(0, 44, 45));
    assert!(!g.legal(0, 44, 64));
    assert!(!g.legal(1, 44, 45));
}
#[test]
fn scouts_stop_at_pieces_and_flag_gaps() {
    let (mut g, _) = sparse();
    put(&mut g, 60, 0, Rank::Scout);
    assert!(g.legal(0, 60, 20));
    assert!(!g.legal(0, 60, 24));
    put(&mut g, 30, 1, Rank::General);
    assert!(g.legal(0, 60, 30));
    assert!(!g.legal(0, 60, 20));
    put(&mut g, 61, 0, Rank::Spy);
    assert!(!g.legal(0, 60, 69));
    put(&mut g, 73, 0, Rank::Scout);
    assert!(!g.legal(0, 73, 33));
    put(&mut g, 76, 0, Rank::Scout);
    assert!(!g.legal(0, 76, 36));
}
#[test]
fn immobile_pieces_cannot_move_or_attack() {
    let (mut g, _) = sparse();
    put(&mut g, 44, 0, Rank::Mine);
    assert!(!g.legal(0, 44, 45));
    put(&mut g, 44, 0, Rank::Flag);
    assert!(!g.legal(0, 44, 45));
}
#[test]
fn rank_order_and_equal_combat() {
    for a in Rank::ALL.into_iter().filter(|r| r.mobile()) {
        for b in Rank::ALL.into_iter().filter(|r| r.mobile()) {
            if matches!(
                (a, b),
                (Rank::Marshal, Rank::Spy) | (Rank::Spy, Rank::Marshal)
            ) {
                continue;
            }
            assert_eq!(
                combat(a, b),
                match (a as usize).cmp(&(b as usize)) {
                    std::cmp::Ordering::Less => 1,
                    std::cmp::Ordering::Equal => 0,
                    _ => -1,
                }
            );
        }
    }
}
#[test]
fn spy_always_defeats_marshal() {
    assert_eq!(combat(Rank::Spy, Rank::Marshal), 1);
    assert_eq!(combat(Rank::Marshal, Rank::Spy), -1);
    assert_eq!(combat(Rank::Miner, Rank::Spy), 1);
}
#[test]
fn only_miner_clears_mines() {
    for r in Rank::ALL.into_iter().filter(|r| r.mobile()) {
        assert_eq!(combat(r, Rank::Mine), if r == Rank::Miner { 1 } else { -1 });
    }
}
#[test]
fn attacker_learns_defender_but_surviving_attacker_is_hidden() {
    let (mut g, cfg) = sparse();
    put(&mut g, 44, 0, Rank::General);
    put(&mut g, 45, 1, Rank::Rifleman);
    g.apply(0, Action::Move { from: 44, to: 45 }, 1, &cfg)
        .unwrap();
    assert_eq!(g.board[45].unwrap().rank, Rank::General);
    assert!(g.board[44].is_none());
    assert!(g.messages[0].iter().any(|s| s.contains("Střelec")));
    let enemy =
        serde_json::to_string(&(g.view(1, "x", 1).board, g.view(1, "x", 1).messages)).unwrap();
    assert!(!enemy.contains("general"));
    assert!(!enemy.contains("Generál"));
    assert!(g.view(1, "x", 1).board[45].as_ref().unwrap().rank.is_none());
}
#[test]
fn losing_attacker_and_equal_ranks_are_removed() {
    for (a, b, expected) in [
        (Rank::Spy, Rank::Miner, Some(Rank::Miner)),
        (Rank::General, Rank::General, None),
    ] {
        let (mut g, cfg) = sparse();
        put(&mut g, 44, 0, a);
        put(&mut g, 45, 1, b);
        g.apply(0, Action::Move { from: 44, to: 45 }, 1, &cfg)
            .unwrap();
        assert!(g.board[44].is_none());
        assert_eq!(g.board[45].map(|p| p.rank), expected);
        assert_eq!(g.captured.len(), if expected.is_none() { 2 } else { 1 });
    }
}
#[test]
fn any_mobile_piece_captures_flag() {
    for r in Rank::ALL.into_iter().filter(|r| r.mobile()) {
        let (mut g, cfg) = sparse();
        put(&mut g, 44, 0, r);
        put(&mut g, 45, 1, Rank::Flag);
        g.apply(0, Action::Move { from: 44, to: 45 }, 1, &cfg)
            .unwrap();
        assert!(matches!(
            g.phase,
            Phase::Finished {
                winner: Some(0),
                ..
            }
        ));
        for player in 0..2 {
            let view = g.view(player, "flag capture", 1);
            // Assert the serialized reply reveals exactly the authoritative ranks.
            let view: View = serde_json::from_str(&serde_json::to_string(&view).unwrap()).unwrap();
            for (actual, visible) in g.board.iter().zip(&view.board) {
                assert_eq!(
                    actual.map(|piece| (piece.owner, piece.rank)),
                    visible
                        .as_ref()
                        .map(|piece| (piece.owner, piece.rank.unwrap()))
                );
            }
        }
        assert!(
            g.apply(1, Action::Move { from: 9, to: 8 }, 2, &cfg)
                .is_err()
        );
    }
}
#[test]
fn invalid_moves_cannot_reset_clock() {
    let (mut game, cfg) = sparse();
    assert!(
        game.apply(0, Action::Move { from: CELLS, to: 0 }, 100, &cfg)
            .is_err()
    );
    assert_eq!(game.view(0, "clock", 100).clocks.delay_ms, 9900);
    assert!(
        game.apply(1, Action::Move { from: 9, to: 8 }, 200, &cfg)
            .is_err()
    );
    assert_eq!(game.view(0, "clock", 200).clocks.delay_ms, 9800);
    assert_eq!(game.view(0, "clock", 200).clocks.main_ms, [180_000; 2]);
    assert_eq!(game.turn, 0);
}
#[test]
fn no_mobile_pieces_is_a_draw_not_a_loss() {
    let (mut g, cfg) = sparse();
    g.board[9] = None;
    put(&mut g, 8, 1, Rank::Flag);
    g.apply(0, Action::Move { from: 90, to: 80 }, 1, &cfg)
        .unwrap();
    assert!(matches!(g.phase, Phase::Finished { winner: None, .. }));
}
#[test]
fn quiet_limit_draws() {
    let (mut g, mut cfg) = sparse();
    cfg.quiet_move_limit = 2;
    g.apply(0, Action::Move { from: 90, to: 80 }, 1, &cfg)
        .unwrap();
    g.apply(1, Action::Move { from: 9, to: 8 }, 2, &cfg)
        .unwrap();
    assert!(matches!(g.phase, Phase::Finished { winner: None, .. }));
}
#[test]
fn frame_size_is_bounded_and_eof_detected() {
    use std::io::Cursor;
    assert!(net::read_frame(&mut Cursor::new(b"12345\n"), 4).is_err());
    assert!(net::read_frame(&mut Cursor::new(b"123"), 4).is_err());
    let mut r = Cursor::new(b"abc\ndef\n");
    assert_eq!(net::read_frame(&mut r, 4).unwrap(), "abc\n");
    assert_eq!(net::read_frame(&mut r, 4).unwrap(), "def\n");
    assert!(net::read_frame(&mut r, 4).is_err());
}
#[test]
fn config_rejects_zero_and_unbounded_clocks() {
    let mut cfg = Config::default();
    assert!(cfg.validate().is_ok());
    cfg.main_time_seconds = 0;
    assert!(cfg.validate().is_err());
    cfg.main_time_seconds = u64::MAX;
    assert!(cfg.validate().is_err());
    cfg = Config::default();
    cfg.turn_delay_seconds = 0;
    assert!(cfg.validate().is_ok());
    cfg.turn_delay_seconds = u64::MAX;
    assert!(cfg.validate().is_err());
    assert!(toml::from_str::<Config>("main_time_second = 1").is_err());
}

#[test]
fn board_matches_plan_with_two_one_by_three_gaps() {
    assert_eq!((COLS, ROWS, CELLS), (10, 11, 110));
    assert_eq!(
        (0..CELLS).filter(|at| blocked(*at)).collect::<Vec<_>>(),
        vec![43, 46, 53, 56, 63, 66]
    );
    assert!(!blocked(CELLS));
    assert_eq!((0..CELLS).filter(|at| !blocked(*at)).count(), 104);
    assert_eq!(square(0), "A11");
    assert_eq!(square(9), "J11");
    assert_eq!(square(50), "A6");
    assert_eq!(square(CELLS - 1), "J1");
}
#[test]
fn deployment_has_four_rows_per_side_and_three_neutral_rows() {
    for at in 0..CELLS {
        assert_eq!(home(0, at), at >= 70);
        assert_eq!(home(1, at), at < 40);
        if (40..70).contains(&at) {
            assert!(!home(0, at) && !home(1, at));
        }
    }
    assert_eq!((0..CELLS).filter(|at| home(0, *at)).count(), 40);
    assert_eq!((0..CELLS).filter(|at| home(1, *at)).count(), 40);
    assert!(!home(0, CELLS));
}
#[test]
fn both_perspectives_rotate_all_eleven_rows_and_preserve_gaps() {
    assert_eq!(orient(0, 1), 109);
    assert_eq!(orient(9, 1), 100);
    assert_eq!(orient(100, 1), 9);
    for at in 0..CELLS {
        assert_eq!(orient(at, 0), at);
        assert_eq!(orient(orient(at, 1), 1), at);
        assert_eq!(blocked(orient(at, 1)), blocked(at));
        assert_eq!(home(0, orient(at, 1)), home(1, at));
    }
}
#[test]
fn gaps_block_destinations_and_scout_paths_but_adjacent_corridors_are_open() {
    let mut board = vec![None; CELLS];
    for (origin, gap, across) in [(33, 43, 73), (36, 46, 76), (73, 63, 33), (76, 66, 36)] {
        board.fill(None);
        board[origin] = Some(Piece {
            owner: 0,
            rank: Rank::Scout,
        });
        assert!(!legal_move(&board, 0, origin, gap));
        assert!(!legal_move(&board, 0, origin, across));
    }
    for col in [0, 1, 2, 4, 5, 7, 8, 9] {
        let origin = 30 + col;
        board.fill(None);
        board[origin] = Some(Piece {
            owner: 0,
            rank: Rank::Scout,
        });
        assert!(legal_move(&board, 0, origin, 70 + col));
    }
}
#[test]
fn new_bottom_row_moves_attacks_and_bounds_work() {
    let (mut g, cfg) = sparse();
    put(&mut g, 109, 0, Rank::Captain);
    assert!(g.legal(0, 109, 108));
    assert!(g.legal(0, 109, 99));
    assert!(g.legal(0, 109, 98));
    assert!(!g.legal(0, 109, 100));
    assert!(!g.legal(0, 109, 110));
    put(&mut g, 108, 1, Rank::Spy);
    g.apply(0, Action::Move { from: 109, to: 108 }, 1, &cfg)
        .unwrap();
    assert_eq!(g.board[108].unwrap().rank, Rank::Captain);
    assert!(g.board[109].is_none());
}

#[test]
fn capture_events_share_outcomes_and_timeline_without_revealing_surviving_attackers() {
    for (attacker, defender, attacker_lost, defender_lost) in [
        (Rank::General, Rank::Cadet, false, true),
        (Rank::Cadet, Rank::General, true, false),
        (Rank::General, Rank::General, true, true),
        (Rank::Spy, Rank::Marshal, false, true),
        (Rank::Miner, Rank::Mine, false, true),
        (Rank::Cadet, Rank::Flag, false, true),
    ] {
        let (mut game, cfg) = sparse();
        put(&mut game, 44, 0, attacker);
        put(&mut game, 45, 1, defender);
        assert!(game.view(0, "capture", 0).motion.is_none());
        game.apply(0, Action::Move { from: 44, to: 45 }, 100, &cfg)
            .unwrap();
        for you in 0..2 {
            let view: View = serde_json::from_str(
                &serde_json::to_string(&game.view(you, "capture", 125)).unwrap(),
            )
            .unwrap();
            let event = view.motion.unwrap();
            assert_eq!(
                (event.ply, event.from, event.to, event.elapsed_ms),
                (1, 44, 45, 25)
            );
            assert_eq!(
                (event.piece_lost, event.defender_lost),
                (attacker_lost, defender_lost)
            );
            assert_eq!(event.defender.as_ref().unwrap().rank, Some(defender));
            let visible = you == 0 || attacker_lost || matches!(game.phase, Phase::Finished { .. });
            assert_eq!(event.piece.rank, visible.then_some(attacker));
            assert_eq!(
                game.view(you, "capture", 350).motion.unwrap().elapsed_ms,
                250
            );
        }
        if matches!(game.phase, Phase::Playing) {
            game.apply(1, Action::Move { from: 9, to: 8 }, 130, &cfg)
                .unwrap();
            let motion = game.view(0, "capture", 130).motion.unwrap();
            assert_eq!((motion.ply, motion.from, motion.to), (2, 9, 8));
            assert!(motion.defender.is_none());
            assert!(!motion.piece_lost && !motion.defender_lost);
        }
    }
}

#[test]
fn ordinary_moves_and_long_scout_moves_share_a_timeline_without_exposing_ranks() {
    for (rank, to) in [(Rank::Cadet, 45), (Rank::Scout, 74)] {
        for mover in 0..2 {
            let (mut game, cfg) = sparse();
            game.turn = mover;
            put(&mut game, 44, mover, rank);
            game.apply(mover, Action::Move { from: 44, to }, 100, &cfg)
                .unwrap();
            for you in 0..2 {
                let view: View = serde_json::from_str(
                    &serde_json::to_string(&game.view(you, "motion", 150)).unwrap(),
                )
                .unwrap();
                let event = view.motion.unwrap();
                assert_eq!(
                    (event.ply, event.from, event.to, event.elapsed_ms),
                    (1, 44, to, 50)
                );
                assert_eq!(event.piece.owner, mover);
                assert_eq!(event.piece.rank, (mover == you).then_some(rank));
                assert!(event.defender.is_none());
                assert!(!event.piece_lost && !event.defender_lost);
                assert!(view.captured.is_empty());
                assert!(view.board[44].is_none());
                assert_eq!(view.board[to].as_ref().unwrap().owner, mover);
            }
        }
    }
}

#[test]
fn public_capture_history_gives_both_armies_remaining_counts() {
    for (attacker, defender) in [
        (Rank::General, Rank::Cadet),
        (Rank::Cadet, Rank::General),
        (Rank::General, Rank::General),
        (Rank::Spy, Rank::Marshal),
        (Rank::Miner, Rank::Mine),
        (Rank::Cadet, Rank::Flag),
    ] {
        let cfg = Config::default();
        let mut game = Game::new(0, &cfg);
        game.start(0, &cfg);
        for owner in 0..2 {
            game.apply(owner, Action::AutoPlace, 0, &cfg).unwrap();
            game.apply(owner, Action::Ready, 0, &cfg).unwrap();
        }
        for you in 0..2 {
            let view = game.view(you, "counts", 0);
            assert!(view.reserve.iter().all(|(_, count)| *count == 0));
            for owner in 0..2 {
                assert_eq!(
                    Rank::ALL
                        .into_iter()
                        .map(|rank| view.remaining(owner, rank))
                        .sum::<usize>(),
                    40
                );
            }
        }
        let from = game
            .board
            .iter()
            .position(|piece| {
                *piece
                    == Some(Piece {
                        owner: 0,
                        rank: attacker,
                    })
            })
            .unwrap();
        let to = game
            .board
            .iter()
            .position(|piece| {
                *piece
                    == Some(Piece {
                        owner: 1,
                        rank: defender,
                    })
            })
            .unwrap();
        game.board[44] = game.board[from].take();
        game.board[45] = game.board[to].take();
        game.turn = 0;
        game.apply(0, Action::Move { from: 44, to: 45 }, 1, &cfg)
            .unwrap();
        for you in 0..2 {
            let view: View =
                serde_json::from_str(&serde_json::to_string(&game.view(you, "counts", 1)).unwrap())
                    .unwrap();
            for owner in 0..2 {
                for rank in Rank::ALL {
                    let actual = game
                        .board
                        .iter()
                        .flatten()
                        .filter(|piece| piece.owner == owner && piece.rank == rank)
                        .count();
                    assert_eq!(view.remaining(owner, rank), actual);
                }
            }
            if matches!(view.phase, Phase::Playing) {
                assert!(
                    view.board
                        .iter()
                        .flatten()
                        .filter(|piece| piece.owner != you)
                        .all(|piece| piece.rank.is_none())
                );
            }
        }
    }
}
