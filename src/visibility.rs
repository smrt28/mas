//! Rank disclosure policy used by both clients, independent of supplied rank fields.
use crate::{CELLS, Phase, View};

pub const MOVE_TRAVEL_MS: u64 = 180;
pub const CAPTURE_FADE_MS: u64 = 220;

pub fn motion_duration_ms(has_defender: bool) -> u64 {
    MOVE_TRAVEL_MS + if has_defender { CAPTURE_FADE_MS } else { 0 }
}

/// Do not treat an unsolicited opponent rank as permission to reveal it.
/// Defenders are shown during combat; a surviving enemy attacker remains hidden.
pub fn filtered_view(raw: &View) -> Result<View, &'static str> {
    if raw.you > 1
        || raw.turn > 1
        || raw.board.len() != CELLS
        || raw.board.iter().flatten().any(|p| p.owner > 1)
        || raw.captured.iter().any(|p| p.owner > 1)
    {
        return Err("Invalid player view");
    }
    let mut view = raw.clone();
    let finished = matches!(view.phase, Phase::Finished { .. });
    for cell in &mut view.board {
        if let Some(piece) = cell {
            if piece.owner != view.you && matches!(view.phase, Phase::Waiting) {
                *cell = None;
            } else if piece.owner != view.you && !finished {
                piece.rank = None;
            }
        }
    }
    if let Some(motion) = &mut view.motion {
        if motion.from >= CELLS
            || motion.to >= CELLS
            || motion.piece.owner > 1
            || motion.defender.as_ref().is_some_and(|p| p.owner > 1)
        {
            return Err("Invalid move view");
        }
        if motion.piece.owner != view.you && !motion.piece_lost && !finished {
            motion.piece.rank = None;
        }
        // No animation may disclose deployment or waiting-state data.
        if !matches!(view.phase, Phase::Playing | Phase::Finished { .. }) {
            view.motion = None;
        }
    }
    Ok(view)
}

/// Whether a newly received move would actually play in the graphical client.
pub fn new_visible_motion(previous: Option<&View>, view: &View) -> bool {
    previous.is_some_and(|old| old.game == view.game && view.ply > old.ply)
        && view.motion.as_ref().is_some_and(|motion| {
            motion.ply == view.ply
                && motion.elapsed_ms < motion_duration_ms(motion.defender.is_some())
        })
}
