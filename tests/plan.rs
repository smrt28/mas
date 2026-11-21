use marsal_a_spion::{
    plan::{self, Point},
    *,
};

#[test]
fn full_sheet_fits_window_with_only_the_info_bar_above_it() {
    assert_eq!(plan::WINDOW_HEIGHT, plan::HEIGHT + plan::BAR_HEIGHT);
    let layout = plan::Layout::default();
    assert!(
        (layout.board_width / layout.board_height - plan::PDF_WIDTH / plan::PDF_HEIGHT).abs()
            < 0.00001
    );
    let top = layout.world(Point { x: 0., y: 0. }, 0);
    let bottom = layout.world(
        Point {
            x: plan::PAGE_WIDTH,
            y: plan::PAGE_HEIGHT,
        },
        0,
    );
    assert!((top.x + layout.width / 2.).abs() < 0.001);
    assert!((bottom.x - layout.width / 2.).abs() < 0.001);
    assert!(top.y <= layout.height / 2. - plan::BAR_HEIGHT as f32);
    assert!(bottom.y >= -layout.height / 2.);
    assert!(layout.height / 2. - plan::BAR_HEIGHT as f32 - top.y < 1.);
}
#[test]
fn every_cell_and_its_edges_can_be_picked_from_either_perspective() {
    for you in 0..2 {
        for at in 0..CELLS {
            let area = plan::cell(at);
            for p in [
                area.center(),
                Point {
                    x: area.left + 1.,
                    y: area.top + 1.,
                },
                Point {
                    x: area.left + area.width - 1.,
                    y: area.top + area.height - 1.,
                },
            ] {
                let screen = plan::world(p, you);
                assert_eq!(
                    plan::pick_cell(screen, you),
                    if blocked(at) { None } else { Some(at) }
                );
                let inverse = plan::source(screen, you);
                assert!((inverse.x - p.x).abs() < 0.001 && (inverse.y - p.y).abs() < 0.001);
            }
        }
    }
}
#[test]
fn printed_bottom_cards_select_the_correct_ranks_after_rotation() {
    for you in 0..2 {
        let mut previous = f32::NEG_INFINITY;
        for rank in plan::STRIP {
            let area = plan::card(rank, you);
            let center = plan::world(area.center(), you);
            assert_eq!(plan::pick_rank(center, you), Some(rank));
            assert!(
                center.x > previous,
                "Printed rank order must remain left-to-right"
            );
            assert!(center.y < -(plan::WINDOW_HEIGHT as f32) / 2. + 90.);
            assert_eq!(plan::pick_cell(center, you), None);
            previous = center.x;
        }
    }
}
#[test]
fn own_four_deployment_rows_always_appear_below_the_neutral_rows() {
    for you in 0..2 {
        let neutral = plan::world(plan::cell(55).center(), you);
        for at in 0..CELLS {
            if home(you, at) {
                assert!(plan::world(plan::cell(at).center(), you).y < neutral.y);
            }
        }
    }
}
#[test]
fn legends_and_bar_cannot_be_mistaken_for_board_squares() {
    for you in 0..2 {
        for p in [
            Point {
                x: 0.,
                y: plan::WINDOW_HEIGHT as f32 / 2. - 10.,
            },
            Point {
                x: -(plan::WIDTH as f32) / 2. - 1.,
                y: 0.,
            },
            Point {
                x: plan::WIDTH as f32 / 2. + 1.,
                y: 0.,
            },
        ] {
            assert_eq!(plan::pick_cell(p, you), None);
            assert_eq!(plan::pick_rank(p, you), None);
        }
    }
    assert!(plan::in_bar(Point {
        x: 0.,
        y: plan::WINDOW_HEIGHT as f32 / 2. - 10.
    }));
    assert!(!plan::in_bar(Point { x: 0., y: 0. }));
}
#[test]
fn full_deployment_and_a_move_work_through_printed_card_and_cell_coordinates() {
    let cfg = Config::default();
    let mut game = Game::new(0, &cfg);
    game.start(0, &cfg);
    for you in 0..2 {
        let mut squares = (0..CELLS).filter(|at| home(you, *at));
        for rank in plan::STRIP {
            let clicked_rank =
                plan::pick_rank(plan::world(plan::card(rank, you).center(), you), you).unwrap();
            for _ in 0..rank.count() {
                let at = squares.next().unwrap();
                let clicked_square =
                    plan::pick_cell(plan::world(plan::cell(at).center(), you), you).unwrap();
                game.apply(
                    you,
                    Action::Place {
                        at: clicked_square,
                        rank: clicked_rank,
                    },
                    1,
                    &cfg,
                )
                .unwrap();
            }
        }
        assert!(
            game.view(you, "test", 1)
                .reserve
                .iter()
                .all(|(_, n)| *n == 0)
        );
        game.apply(you, Action::Ready, 1, &cfg).unwrap();
    }
    assert_eq!(game.phase, Phase::Playing);
    let you = game.turn;
    let (from, to) = (0..CELLS)
        .find_map(|from| {
            game.view(you, "test", 2)
                .legal_targets(from)
                .first()
                .map(|to| (from, *to))
        })
        .unwrap();
    let from = plan::pick_cell(plan::world(plan::cell(from).center(), you), you).unwrap();
    let to = plan::pick_cell(plan::world(plan::cell(to).center(), you), you).unwrap();
    game.apply(you, Action::Move { from, to }, 2, &cfg).unwrap();
    assert_eq!(game.ply, 1);
    assert_eq!(game.turn, 1 - you);
}

#[test]
fn resized_sheet_preserves_exact_pdf_aspect_and_fits_below_fixed_bar() {
    for (w, h) in [
        (360., 480.),
        (600., 881.),
        (1200., 900.),
        (400., 1600.),
        (1600., 400.),
        (1920., 1080.),
        (643.5, 900.25),
    ] {
        let layout = plan::Layout::new(w, h);
        assert!((layout.board_width / layout.board_height - plan::ASPECT).abs() < 0.00001);
        assert!(
            layout.board_width <= w + 0.001
                && layout.board_height <= h - plan::BAR_HEIGHT as f32 + 0.001
        );
        assert!(
            (layout.board_width - w).abs() < 0.001
                || (layout.board_height - (h - plan::BAR_HEIGHT as f32)).abs() < 0.001
        );
        for p in [
            Point { x: 0., y: 0. },
            Point {
                x: plan::PAGE_WIDTH,
                y: plan::PAGE_HEIGHT,
            },
        ] {
            let p = layout.world(p, 0);
            assert!(p.x.abs() <= w / 2. + 0.001);
            assert!(p.y >= -h / 2. - 0.001 && p.y <= h / 2. - plan::BAR_HEIGHT as f32 + 0.001);
        }
        assert!(layout.in_bar(Point {
            x: 0.,
            y: h / 2. - 1.
        }));
        assert!(layout.in_bar(Point {
            x: 0.,
            y: h / 2. - plan::BAR_HEIGHT as f32 + 1.
        }));
        assert!(!layout.in_bar(Point {
            x: 0.,
            y: h / 2. - plan::BAR_HEIGHT as f32 - 1.
        }));
    }
}
#[test]
fn all_cells_and_printed_cards_stay_aligned_after_wide_and_tall_resizes() {
    for (w, h) in [
        (360., 480.),
        (600., 881.),
        (1200., 900.),
        (400., 1600.),
        (1600., 400.),
    ] {
        let layout = plan::Layout::new(w, h);
        for you in 0..2 {
            for at in 0..CELLS {
                let center = layout.world(plan::cell(at).center(), you);
                assert_eq!(
                    layout.pick_cell(center, you),
                    if blocked(at) { None } else { Some(at) }
                );
                let original = layout.source(center, you);
                assert!((original.x - plan::cell(at).center().x).abs() < 0.001);
                assert!((original.y - plan::cell(at).center().y).abs() < 0.001);
            }
            for rank in plan::STRIP {
                let center = layout.world(plan::card(rank, you).center(), you);
                assert_eq!(layout.pick_rank(center, you), Some(rank));
                assert_eq!(layout.pick_cell(center, you), None);
            }
        }
    }
}
#[test]
fn letterbox_margins_do_not_select_cells_or_pieces() {
    for (w, h) in [(1600., 600.), (400., 1600.)] {
        let layout = plan::Layout::new(w, h);
        let side = Point {
            x: (layout.board_width + w) / 4.,
            y: -(plan::BAR_HEIGHT as f32) / 2.,
        };
        let above = Point {
            x: 0.,
            y: (h / 2. - plan::BAR_HEIGHT as f32 + layout.world(Point { x: 0., y: 0. }, 0).y) / 2.,
        };
        let margin = if layout.board_width < w - 1. {
            side
        } else {
            above
        };
        for you in 0..2 {
            assert_eq!(layout.pick_cell(margin, you), None);
            assert_eq!(layout.pick_rank(margin, you), None);
        }
        assert!(!layout.in_bar(margin));
    }
}
#[test]
fn board_points_and_piece_strip_scale_by_the_same_factor() {
    let base = plan::Layout::default();
    for (w, h) in [(360., 480.), (1800., 1600.), (400., 1600.), (1600., 400.)] {
        let layout = plan::Layout::new(w, h);
        for you in 0..2 {
            let origin = plan::cell(0).center();
            for p in [
                plan::cell(109).center(),
                plan::card(Rank::Flag, you).center(),
                plan::card(Rank::Mine, you).center(),
            ] {
                let a = base.world(origin, you);
                let b = base.world(p, you);
                let resized_a = layout.world(origin, you);
                let resized_b = layout.world(p, you);
                assert!(((resized_b.x - resized_a.x) - (b.x - a.x) * layout.scale).abs() < 0.001);
                assert!(((resized_b.y - resized_a.y) - (b.y - a.y) * layout.scale).abs() < 0.001);
            }
        }
    }
}

#[test]
fn edited_png_slots_have_uniform_centers_and_count_anchors_inside_cards() {
    // Independent measurements in the 1059×1485 asset, not round trips through
    // card(): catches accidental return to the old PDF legend coordinates.
    for you in 0..2 {
        for (i, rank) in plan::STRIP.into_iter().enumerate() {
            let area = plan::card(rank, you);
            let center = area.center();
            let expected_x = if you == 0 {
                49. + 96. * i as f32
            } else {
                1010. - 96. * i as f32
            };
            assert!((center.x / plan::PAGE_WIDTH * 1059. - expected_x).abs() < 0.001);
            assert!(area.contains(plan::count_position(rank, you)));
            for (w, h) in [(360., 480.), (600., 881.), (1200., 900.), (400., 1600.)] {
                let layout = plan::Layout::new(w, h);
                for p in [
                    center,
                    Point {
                        x: area.left + 1.,
                        y: area.top + 1.,
                    },
                    Point {
                        x: area.left + area.width - 1.,
                        y: area.top + area.height - 1.,
                    },
                ] {
                    assert_eq!(layout.pick_rank(layout.world(p, you), you), Some(rank));
                }
            }
        }
    }
}
