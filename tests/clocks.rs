use marsal_a_spion::*;

fn game(delay: u64, main: u64) -> (Game, Config) {
    let cfg = Config {
        turn_delay_seconds: delay,
        main_time_seconds: main,
        ..Config::default()
    };
    let mut game = Game::new(0, &cfg);
    game.phase = Phase::Playing;
    game.turn = 0;
    game.board[90] = Some(Piece {
        owner: 0,
        rank: Rank::Miner,
    });
    game.board[9] = Some(Piece {
        owner: 1,
        rank: Rank::Miner,
    });
    (game, cfg)
}
fn claim(turn_id: u32) -> Action {
    Action::ClaimVictory { turn_id }
}

#[test]
fn defaults_and_setup_do_not_spend_main_time() {
    let cfg = Config::default();
    assert_eq!((cfg.turn_delay_seconds, cfg.main_time_seconds), (10, 180));
    let mut game = Game::new(0, &cfg);
    game.start(100, &cfg);
    for player in 0..2 {
        game.apply(player, Action::AutoPlace, 200, &cfg).unwrap();
        game.apply(player, Action::Ready, 300, &cfg).unwrap();
    }
    let first = game.view(0, "clocks", 300);
    let second = game.view(1, "clocks", 300);
    assert_eq!(first.phase, Phase::Playing);
    assert_eq!(*first.clocks, *second.clocks);
    assert_eq!(first.clocks.main_ms, [180_000; 2]);
    assert_eq!(first.clocks.delay_ms, 10_000);
    assert!(!first.clocks.claimable);
}

#[test]
fn delay_pauses_both_clocks_then_only_active_clock_spends_time_and_carries_over() {
    let (mut game, cfg) = game(10, 180);
    game.tick(9999, &cfg);
    assert_eq!(game.view(0, "clocks", 9999).clocks.main_ms, [180_000; 2]);
    assert_eq!(game.view(0, "clocks", 9999).clocks.delay_ms, 1);
    assert_eq!(game.view(0, "clocks", 10_000).clocks.main_ms, [180_000; 2]);
    game.tick(12_500, &cfg);
    game.tick(12_500, &cfg); // repeated ticks must not charge twice
    assert_eq!(
        game.view(0, "clocks", 12_500).clocks.main_ms,
        [177_500, 180_000]
    );
    game.apply(0, Action::Move { from: 90, to: 80 }, 15_000, &cfg)
        .unwrap();
    assert_eq!(game.turn, 1);
    assert_eq!(
        game.view(1, "clocks", 15_000).clocks.main_ms,
        [175_000, 180_000]
    );
    assert_eq!(game.view(1, "clocks", 15_000).clocks.delay_ms, 10_000);
    game.apply(1, Action::Move { from: 9, to: 8 }, 20_000, &cfg)
        .unwrap();
    assert_eq!(
        game.view(0, "clocks", 20_000).clocks.main_ms,
        [175_000, 180_000]
    );
    assert_eq!(game.view(0, "clocks", 20_000).clocks.delay_ms, 10_000);
    assert_eq!(
        game.view(0, "clocks", 30_001).clocks.main_ms,
        [174_999, 180_000]
    );
}

#[test]
fn expiry_never_changes_turn_or_finishes_without_a_claim_and_only_opponent_can_claim() {
    let (mut game, cfg) = game(1, 1);
    assert!(game.apply(1, claim(0), 1999, &cfg).is_err());
    assert_eq!(game.view(0, "clocks", 1999).clocks.main_ms[0], 1);
    assert!(game.tick(2000, &cfg)); // first eligibility transition is broadcast
    assert!(!game.tick(2000, &cfg));
    assert_eq!(game.phase, Phase::Playing);
    assert_eq!((game.turn, game.ply), (0, 0));
    assert!(!game.view(0, "clocks", 2000).can_claim_victory());
    assert!(game.view(1, "clocks", 2000).can_claim_victory());
    assert!(game.apply(0, claim(0), 2000, &cfg).is_err());
    game.tick(1_000_000, &cfg);
    assert_eq!(game.phase, Phase::Playing);
    assert_eq!(game.view(1, "clocks", 1_000_000).clocks.main_ms, [0, 1000]);
    game.apply(1, claim(0), 1_000_000, &cfg).unwrap();
    assert!(matches!(
        game.phase,
        Phase::Finished {
            winner: Some(1),
            ..
        }
    ));
    assert!(!game.view(1, "clocks", 1_000_000).can_claim_victory());
}

#[test]
fn claim_first_wins_and_move_first_invalidates_the_claim() {
    let (mut first, cfg) = game(1, 1);
    first.apply(1, claim(0), 2000, &cfg).unwrap();
    assert!(
        first
            .apply(0, Action::Move { from: 90, to: 80 }, 2000, &cfg)
            .is_err()
    );
    assert_eq!(first.board[90].unwrap().owner, 0);
    let (mut second, cfg) = game(1, 1);
    second
        .apply(0, Action::Move { from: 90, to: 80 }, 2000, &cfg)
        .unwrap();
    assert!(second.apply(1, claim(0), 2000, &cfg).is_err());
    assert_eq!(second.phase, Phase::Playing);
    assert_eq!((second.turn, second.ply), (1, 1));
    assert_eq!(second.view(1, "clocks", 2000).clocks.delay_ms, 1000);
    assert!(!second.view(0, "clocks", 2000).can_claim_victory());
}

#[test]
fn zero_main_time_gets_a_fresh_delay_every_turn_and_old_claims_never_revive() {
    let (mut game, cfg) = game(1, 1);
    game.apply(0, Action::Move { from: 90, to: 80 }, 2000, &cfg)
        .unwrap();
    game.apply(1, Action::Move { from: 9, to: 8 }, 2500, &cfg)
        .unwrap();
    assert_eq!((game.turn, game.ply), (0, 2));
    assert_eq!(game.view(0, "clocks", 2500).clocks.main_ms, [0, 1000]);
    assert_eq!(game.view(0, "clocks", 2500).clocks.delay_ms, 1000);
    assert!(!game.view(1, "clocks", 3499).can_claim_victory());
    assert!(game.apply(1, claim(2), 3499, &cfg).is_err());
    assert!(game.view(1, "clocks", 3500).can_claim_victory());
    // Same player is eligible again, but the token from their previous chance
    // still cannot claim this later turn.
    assert!(game.apply(1, claim(0), 3500, &cfg).is_err());
    assert!(
        game.apply(0, Action::Move { from: 90, to: 80 }, 3600, &cfg)
            .is_err()
    );
    assert_eq!(game.view(1, "clocks", 3600).clocks.delay_ms, 0);
    game.apply(0, Action::Move { from: 80, to: 90 }, 4000, &cfg)
        .unwrap();
    game.apply(1, Action::Move { from: 8, to: 9 }, 4500, &cfg)
        .unwrap();
    assert_eq!(game.view(0, "clocks", 4500).clocks.delay_ms, 1000);
    assert!(!game.view(1, "clocks", 5499).can_claim_victory());
    assert!(game.apply(1, claim(2), 5500, &cfg).is_err());
    game.apply(1, claim(4), 5500, &cfg).unwrap();
    assert!(matches!(
        game.phase,
        Phase::Finished {
            winner: Some(1),
            ..
        }
    ));
}

#[test]
fn local_interpolation_matches_authoritative_clocks_but_does_not_invent_claim_eligibility() {
    for turn in 0..2 {
        let (mut game, cfg) = game(10, 180);
        game.turn = turn;
        game.tick(9000, &cfg);
        for you in 0..2 {
            let view = game.view(you, "clocks", 9000);
            for elapsed in [0, 999, 1000, 1001, 50_000, 181_000] {
                let local = view.clock_after(elapsed);
                let authoritative = game.view(you, "clocks", 9000 + elapsed);
                assert_eq!(local.main_ms, authoritative.clocks.main_ms);
                assert_eq!(local.delay_ms, authoritative.clocks.delay_ms);
                assert!(!local.claimable);
            }
        }
    }
}

#[test]
fn zero_delay_is_configurable_and_final_clocks_are_frozen() {
    let (mut game, cfg) = game(0, 1);
    assert_eq!(game.view(0, "clocks", 1).clocks.main_ms, [999, 1000]);
    game.apply(1, claim(0), 1000, &cfg).unwrap();
    let final_clock = *game.view(0, "clocks", 1000).clocks;
    assert_eq!(final_clock.main_ms, [0, 1000]);
    assert_eq!(*game.view(0, "clocks", 99_000).clocks, final_clock);
    assert_eq!(
        game.view(0, "clocks", 1000).clock_after(99_000),
        final_clock
    );
    assert!(
        game.apply(0, Action::Move { from: 90, to: 80 }, 1001, &cfg)
            .is_err()
    );
}
