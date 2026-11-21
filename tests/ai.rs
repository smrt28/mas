#![cfg(feature = "ai-client")]
use marsal_a_spion::{ai::*, *};

fn playing() -> View {
    let cfg = Config::default();
    let mut game = Game::new(0, &cfg);
    game.phase = Phase::Playing;
    game.turn = 0;
    game.board[70] = Some(Piece {
        owner: 0,
        rank: Rank::Scout,
    });
    game.board[60] = Some(Piece {
        owner: 1,
        rank: Rank::Marshal,
    });
    game.board[90] = Some(Piece {
        owner: 0,
        rank: Rank::Flag,
    });
    game.board[0] = Some(Piece {
        owner: 1,
        rank: Rank::Flag,
    });
    game.view(0, "fair", 0)
}
fn encoded(observer: &Observer) -> serde_json::Value {
    serde_json::to_value(observer.decision(0).unwrap()).unwrap()
}
#[test]
fn extra_hidden_ranks_cannot_change_requests_actions_keys_or_memory() {
    let raw = playing();
    let mut clean = Observer::default();
    clean.observe(&raw).unwrap();
    let expected = encoded(&clean);
    let key = clean.decision(0).unwrap().key;
    for rank in Rank::ALL {
        let mut poisoned = raw.clone();
        poisoned.board[60].as_mut().unwrap().rank = Some(rank);
        poisoned.board[0].as_mut().unwrap().rank = Some(rank);
        poisoned.messages = vec![format!("untrusted hidden server payload: {}", rank.name())];
        poisoned.motion = Some(Box::new(MoveView {
            ply: 0,
            from: 61,
            to: 60,
            piece: VisiblePiece {
                owner: 1,
                rank: Some(rank),
            },
            defender: None,
            piece_lost: false,
            defender_lost: false,
            elapsed_ms: 0,
        }));
        let mut ai = Observer::default();
        ai.observe(&poisoned).unwrap();
        assert_eq!(encoded(&ai), expected);
        assert_eq!(ai.decision(0).unwrap().key, key);
        assert!(ai.observation(0).unwrap().history.is_empty());
        assert!(
            ai.observation(0).unwrap().board[60]
                .piece
                .as_ref()
                .unwrap()
                .rank
                .is_none()
        );
    }
}
#[test]
fn deployment_shows_opponent_positions_but_hides_ranks_and_counters() {
    let cfg = Config {
        placement_seconds: 89,
        main_time_seconds: 99,
        turn_delay_seconds: 7,
        quiet_move_limit: 123,
        ..Config::default()
    };
    let mut game = Game::new(0, &cfg);
    game.start(0, &cfg);
    game.apply(
        1,
        Action::Place {
            at: 0,
            rank: Rank::Flag,
        },
        0,
        &cfg,
    )
    .unwrap();
    let raw = game.view(0, "deploy", 0);
    let mut clean = Observer::default();
    clean.observe(&raw).unwrap();
    let mut poisoned = raw.clone();
    poisoned.board[0] = Some(VisiblePiece {
        owner: 1,
        rank: Some(Rank::Flag),
    });
    let mut ai = Observer::default();
    ai.observe(&poisoned).unwrap();
    assert_eq!(encoded(&clean), encoded(&ai));
    let observation = ai.observation(0).unwrap();
    assert_eq!(
        observation.board[0].piece,
        Some(ObservedPiece {
            owner: 1,
            rank: None
        })
    );
    let pending = ai.decision(0).unwrap();
    game.apply(1, Action::Remove { at: 0 }, 1, &cfg).unwrap();
    ai.observe(&game.view(0, "deploy", 1)).unwrap();
    assert!(ai.observation(0).unwrap().board[0].piece.is_none());
    assert!(
        ai.validate(&pending, 0).is_some(),
        "opponent setup changes must not discard a still-legal paid decision"
    );
    assert!(ai.observation(0).unwrap().history.is_empty());
    assert_eq!(observation.settings, MatchSettings::from(&cfg));
    assert!(
        observation
            .counters
            .iter()
            .filter(|c| c.owner == 1)
            .all(|c| c.count == DisplayCount::Unknown("?"))
    );
    assert!(
        ai.decision(0)
            .unwrap()
            .actions
            .iter()
            .all(|choice| matches!(choice.action, Action::Place { at, .. } if home(0, at)))
    );
}
#[test]
fn brief_combat_reveals_enter_history_but_unrevealed_surviving_attackers_do_not() {
    let mut before = playing();
    before.ply = 4;
    let mut ai = Observer::default();
    ai.observe(&before).unwrap();
    let mut after = before.clone();
    after.ply = 5;
    after.motion = Some(Box::new(MoveView {
        ply: 5,
        from: 60,
        to: 70,
        piece: VisiblePiece {
            owner: 1,
            rank: Some(Rank::Marshal),
        },
        defender: Some(VisiblePiece {
            owner: 0,
            rank: Some(Rank::Scout),
        }),
        piece_lost: false,
        defender_lost: true,
        elapsed_ms: 220,
    }));
    ai.observe(&after).unwrap();
    let history = ai.observation(0).unwrap().history;
    assert!(
        history
            .iter()
            .any(|r| r.owner == 0 && r.rank == Rank::Scout)
    );
    assert!(!history.iter().any(|r| r.owner == 1));
    // An enemy defender is shown briefly even when its surviving board card hides again.
    after.ply = 6;
    after.motion = Some(Box::new(MoveView {
        ply: 6,
        from: 70,
        to: 60,
        piece: VisiblePiece {
            owner: 0,
            rank: Some(Rank::Scout),
        },
        defender: Some(VisiblePiece {
            owner: 1,
            rank: Some(Rank::Marshal),
        }),
        piece_lost: true,
        defender_lost: false,
        elapsed_ms: 250,
    }));
    ai.observe(&after).unwrap();
    let history = ai.observation(0).unwrap().history;
    assert!(
        history
            .iter()
            .any(|r| r.owner == 1 && r.rank == Rank::Marshal && r.source == "combat_animation")
    );
    after.motion.as_mut().unwrap().elapsed_ms = 2_000;
    ai.observe(&after).unwrap();
    assert_eq!(ai.observation(0).unwrap().history, history);
    // Joining late or receiving an already expired animation does not reveal its ranks.
    let mut late = Observer::default();
    late.observe(&after).unwrap();
    assert!(late.observation(0).unwrap().history.is_empty());
    let mut expired = Observer::default();
    expired.observe(&before).unwrap();
    expired.observe(&after).unwrap();
    assert!(expired.observation(0).unwrap().history.is_empty());
}
#[test]
fn only_canonical_visible_combat_details_are_observed_never_raw_messages() {
    let mut raw = playing();
    raw.messages = vec![
        "private text must not reach model".into(),
        "At A5: enemy Maršál.".into(),
    ];
    let mut ai = Observer::default();
    ai.observe(&raw).unwrap();
    let request = encoded(&ai).to_string();
    assert!(!request.contains("private text"));
    assert!(!request.contains("At A5"));
    assert!(
        ai.observation(0)
            .unwrap()
            .history
            .iter()
            .any(|r| r.owner == 1 && r.rank == Rank::Marshal && r.source == "combat_detail")
    );
}
#[test]
fn stale_turns_invalid_ids_claim_expiry_and_finished_games_have_no_action() {
    let mut raw = playing();
    let mut ai = Observer::default();
    ai.observe(&raw).unwrap();
    let pending = ai.decision(0).unwrap();
    let id = pending
        .actions
        .iter()
        .position(|c| matches!(c.action, Action::Move { .. }))
        .unwrap();
    assert!(ai.validate(&pending, id).is_some());
    assert!(ai.validate(&pending, usize::MAX).is_none());
    // Clock updates and opponent readiness must not invalidate an otherwise current action.
    raw.clocks.main_ms[0] = 1;
    raw.ready[1] = true;
    ai.observe(&raw).unwrap();
    assert!(ai.validate(&pending, id).is_some());
    raw.ply += 1;
    ai.observe(&raw).unwrap();
    assert!(ai.validate(&pending, id).is_none());
    raw.turn = 1;
    raw.clocks.claimable = true;
    ai.observe(&raw).unwrap();
    let claim = ai.decision(0).unwrap();
    assert!(matches!(
        ai.validate(&claim, 0),
        Some(Action::ClaimVictory { .. })
    ));
    raw.ply += 2;
    ai.observe(&raw).unwrap();
    assert!(ai.validate(&claim, 0).is_none());
    raw.phase = Phase::Finished {
        winner: Some(0),
        reason: "private extra reason".into(),
    };
    ai.observe(&raw).unwrap();
    assert!(ai.decision(0).is_none());
    assert!(ai.validate(&claim, 0).is_none());
    assert_eq!(ai.finished(), Some(Some(0)));
}
#[test]
fn deployment_locks_only_when_complete_and_decisions_expire_when_setup_ends() {
    let cfg = Config::default();
    let mut game = Game::new(0, &cfg);
    game.start(0, &cfg);
    let mut ai = Observer::default();
    ai.observe(&game.view(0, "deploy", 0)).unwrap();
    let initial = ai.decision(0).unwrap();
    game.apply(0, Action::AutoPlace, 0, &cfg).unwrap();
    ai.observe(&game.view(0, "deploy", 0)).unwrap();
    assert!(ai.validate(&initial, 0).is_none());
    assert_eq!(ai.decision(0).unwrap().actions.len(), 1);
    assert_eq!(ai.decision(0).unwrap().actions[0].action, Action::Ready);
    game.apply(0, Action::Ready, 0, &cfg).unwrap();
    ai.observe(&game.view(0, "deploy", 0)).unwrap();
    assert!(ai.decision(0).is_none());
}
#[test]
fn create_game_never_joins_a_name_already_in_use() {
    let cfg = Config::default();
    let mut sessions = session::Sessions::default();
    sessions
        .command(1, Action::CreateGame { game: "new".into() }, 0, &cfg)
        .unwrap();
    assert!(
        sessions
            .command(
                2,
                Action::CreateGame {
                    game: " new ".into()
                },
                0,
                &cfg
            )
            .is_err()
    );
    assert!(!sessions.contains(2));
    assert_eq!(sessions.waiting_games(0), vec!["new"]);
}

#[test]
fn live_animation_does_not_reveal_a_surviving_enemy_attacker_or_allow_early_input() {
    let before = playing();
    let mut after = before.clone();
    after.ply = 1;
    after.motion = Some(Box::new(MoveView {
        ply: 1,
        from: 60,
        to: 70,
        piece: VisiblePiece {
            owner: 1,
            rank: None,
        },
        defender: Some(VisiblePiece {
            owner: 0,
            rank: Some(Rank::Scout),
        }),
        piece_lost: false,
        defender_lost: true,
        elapsed_ms: 0,
    }));
    let mut clean = Observer::default();
    clean.observe(&before).unwrap();
    clean.observe(&after).unwrap();
    let expected = serde_json::to_value(clean.observation(0).unwrap()).unwrap();
    assert!(
        clean.decision(0).is_none(),
        "GUI input is paused during animation"
    );
    for rank in Rank::ALL {
        let mut poisoned = after.clone();
        poisoned.motion.as_mut().unwrap().piece.rank = Some(rank);
        poisoned.board[60].as_mut().unwrap().rank = Some(rank);
        let mut observer = Observer::default();
        observer.observe(&before).unwrap();
        observer.observe(&poisoned).unwrap();
        assert_eq!(
            serde_json::to_value(observer.observation(0).unwrap()).unwrap(),
            expected
        );
        assert!(
            !observer
                .observation(0)
                .unwrap()
                .history
                .iter()
                .any(|r| r.owner == 1)
        );
    }
    after.ply = 2;
    after.motion.as_mut().unwrap().ply = 2;
    after.motion.as_mut().unwrap().piece_lost = true;
    after.motion.as_mut().unwrap().piece.rank = Some(Rank::Marshal);
    clean.observe(&after).unwrap();
    assert!(
        clean
            .observation(0)
            .unwrap()
            .history
            .iter()
            .any(|r| r.owner == 1 && r.rank == Rank::Marshal)
    );
    // Hidden phase messages do not count as visible combat detail.
    let mut hidden_message = before;
    hidden_message.phase = Phase::Placement;
    hidden_message.messages = vec!["At A5: enemy Maršál.".into()];
    let mut observer = Observer::default();
    observer.observe(&hidden_message).unwrap();
    assert!(observer.observation(0).unwrap().history.is_empty());
}

#[test]
fn compact_prompt_preserves_visible_board_counters_and_action_ids() {
    let cfg = Config::default();
    let mut game = Game::new(0, &cfg);
    game.start(0, &cfg);
    game.apply(0, Action::AutoPlace, 0, &cfg).unwrap();
    game.apply(1, Action::AutoPlace, 0, &cfg).unwrap();
    game.phase = Phase::Playing;
    game.turn = 0;
    let mut observer = Observer::default();
    observer.observe(&game.view(0, "compact", 0)).unwrap();
    let decision = observer.decision(0).unwrap();
    let prompt = decision.prompt();
    for cell in &decision.observation.board {
        let compact = &prompt["observation"]["board"][cell.index];
        if cell.blocked {
            assert_eq!(compact, "#");
        } else if let Some(piece) = &cell.piece {
            assert_eq!(compact[0], piece.owner);
            assert_eq!(
                compact[1],
                serde_json::json!(piece.rank.map(|r| r as usize))
            );
        } else {
            assert!(compact.is_null());
        }
    }
    for (index, counter) in decision.observation.counters.iter().enumerate() {
        let compact = &prompt["observation"]["counters"][index];
        assert_eq!(compact[0], counter.owner);
        assert_eq!(compact[1], counter.rank as usize);
        assert_eq!(compact[2], serde_json::to_value(&counter.count).unwrap());
    }
    for (index, choice) in decision.actions.iter().enumerate() {
        assert_eq!(prompt["actions"][index][0], choice.id);
        if let Action::Move { from, to } = choice.action {
            assert_eq!(
                prompt["actions"][index],
                serde_json::json!([choice.id, "m", from, to])
            );
        }
    }
    let instructions = prompt_instructions();
    assert!(instructions.contains(RULES));
    for rank in Rank::ALL {
        assert!(instructions.contains(rank.name()));
    }
    assert!(prompt["observation"].get("rules").is_none());
    assert!(prompt["observation"].get("inventory").is_none());
    assert_eq!(
        prompt["observation"]["settings"],
        serde_json::to_value(&decision.observation.settings).unwrap()
    );
}
#[test]
fn forced_readiness_and_claims_skip_llm_but_resignation_never_does() {
    let mut observer = Observer::default();
    observer.observe(&playing()).unwrap();
    let mut decision = observer.decision(0).unwrap();
    assert!(decision.automatic_action().is_none());
    for action in [Action::Ready, Action::ClaimVictory { turn_id: 8 }] {
        decision.actions = vec![Choice {
            id: 0,
            action: action.clone(),
        }];
        assert_eq!(decision.automatic_action(), Some(action));
    }
    decision.actions = vec![Choice {
        id: 0,
        action: Action::Resign,
    }];
    assert!(decision.automatic_action().is_none());
}
