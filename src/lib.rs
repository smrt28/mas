//! Authoritative rules and wire protocol. Coordinates are always server coordinates.
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

pub const SIZE: usize = 10;
pub const CELLS: usize = SIZE * SIZE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rank {
    Marshal,
    General,
    Captain,
    Cadet,
    Corporal,
    Rifleman,
    Scout,
    Miner,
    Spy,
    Mine,
    Flag,
}
impl Rank {
    pub const ALL: [Self; 11] = [
        Self::Marshal,
        Self::General,
        Self::Captain,
        Self::Cadet,
        Self::Corporal,
        Self::Rifleman,
        Self::Scout,
        Self::Miner,
        Self::Spy,
        Self::Mine,
        Self::Flag,
    ];
    pub fn count(self) -> usize {
        [1, 1, 2, 3, 4, 5, 8, 6, 1, 8, 1][self as usize]
    }
    pub fn mobile(self) -> bool {
        !matches!(self, Self::Mine | Self::Flag)
    }
    pub fn name(self) -> &'static str {
        [
            "Maršál",
            "Generál",
            "Kapitán",
            "Kadet",
            "Kaprál",
            "Střelec",
            "Průzkumník",
            "Minér",
            "Špión",
            "Minové pole",
            "Prapor",
        ][self as usize]
    }
    pub fn asset(self) -> &'static str {
        [
            "marshal", "general", "captain", "cadet", "corporal", "rifleman", "scout", "miner",
            "spy", "mine", "flag",
        ][self as usize]
    }
    pub fn short(self) -> &'static str {
        ["M", "G", "K", "Kd", "Kp", "St", "Pz", "Mi", "Š", "B", "F"][self as usize]
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Piece {
    pub owner: usize,
    pub rank: Rank,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Join { game: String },
    Place { at: usize, rank: Rank },
    Remove { at: usize },
    AutoPlace,
    Ready,
    Move { from: usize, to: usize },
    Resign,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Phase {
    Waiting,
    Placement,
    Playing,
    Finished {
        winner: Option<usize>,
        reason: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VisiblePiece {
    pub owner: usize,
    pub rank: Option<Rank>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View {
    pub game: String,
    pub you: usize,
    pub phase: Phase,
    pub board: Vec<Option<VisiblePiece>>,
    pub ready: [bool; 2],
    pub turn: usize,
    pub remaining_ms: u64,
    pub reserve: Vec<(Rank, usize)>,
    pub captured: Vec<Piece>,
    pub messages: Vec<String>,
    pub missed: [u32; 2],
    pub ply: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    State { view: View },
    Error { message: String },
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub bind: String,
    pub placement_seconds: u64,
    pub move_seconds: u64,
    pub max_missed_turns: u32,
    pub quiet_move_limit: u32,
    pub max_rooms: usize,
    pub max_connections: usize,
    pub waiting_seconds: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:4000".into(),
            placement_seconds: 120,
            move_seconds: 5,
            max_missed_turns: 3,
            quiet_move_limit: 200,
            max_rooms: 128,
            max_connections: 256,
            waiting_seconds: 600,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.placement_seconds == 0
            || self.move_seconds == 0
            || self.waiting_seconds == 0
            || self.placement_seconds > 86400
            || self.move_seconds > 86400
            || self.waiting_seconds > 86400
            || self.max_missed_turns == 0
            || self.quiet_move_limit == 0
            || self.max_rooms == 0
            || self.max_connections == 0
        {
            return Err(
                "Time limits must be 1..86400 seconds; all other limits must be positive".into(),
            );
        }
        Ok(())
    }
}
pub fn blocked(at: usize) -> bool {
    at < CELLS && matches!(at / 10, 4 | 5) && matches!(at % 10, 2 | 3 | 6 | 7)
}
pub fn home(owner: usize, at: usize) -> bool {
    at < CELLS
        && if owner == 0 {
            at / 10 >= 6
        } else {
            at / 10 <= 3
        }
}

pub struct Game {
    pub board: Vec<Option<Piece>>,
    pub phase: Phase,
    pub ready: [bool; 2],
    pub turn: usize,
    pub deadline: u64,
    pub captured: Vec<Piece>,
    pub messages: [Vec<String>; 2],
    pub missed: [u32; 2],
    pub ply: u32,
    quiet: u32,
}
impl Game {
    pub fn new(now: u64, cfg: &Config) -> Self {
        Self {
            board: vec![None; CELLS],
            phase: Phase::Waiting,
            ready: [false; 2],
            turn: 0,
            deadline: now + cfg.waiting_seconds * 1000,
            captured: vec![],
            messages: [vec![], vec![]],
            missed: [0; 2],
            ply: 0,
            quiet: 0,
        }
    }
    pub fn start(&mut self, now: u64, cfg: &Config) {
        self.phase = Phase::Placement;
        self.deadline = now + cfg.placement_seconds * 1000;
        self.broadcast("Both commanders connected. Deploy your 40 pieces in your four home rows.");
    }
    fn note(&mut self, p: usize, s: impl Into<String>) {
        let log = &mut self.messages[p];
        log.push(s.into());
        if log.len() > 8 {
            log.remove(0);
        }
    }
    fn broadcast(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.note(0, s.clone());
        self.note(1, s);
    }
    pub fn finish(&mut self, winner: Option<usize>, reason: impl Into<String>) {
        self.phase = Phase::Finished {
            winner,
            reason: reason.into(),
        };
        self.deadline = 0;
    }
    pub fn reserve(&self, p: usize, r: Rank) -> usize {
        r.count()
            - self
                .board
                .iter()
                .flatten()
                .filter(|v| v.owner == p && v.rank == r)
                .count()
    }
    fn fill(&mut self, p: usize) {
        let mut ranks = vec![];
        for r in Rank::ALL {
            ranks.extend(std::iter::repeat_n(r, self.reserve(p, r)));
        }
        ranks.shuffle(&mut rand::rng());
        for (at, cell) in self.board.iter_mut().enumerate() {
            if home(p, at) && cell.is_none() {
                *cell = Some(Piece {
                    owner: p,
                    rank: ranks.pop().expect("matching reserve"),
                });
            }
        }
    }
    fn begin_play(&mut self, now: u64, cfg: &Config) {
        self.phase = Phase::Playing;
        self.turn = rand::random::<bool>() as usize;
        self.deadline = now + cfg.move_seconds * 1000;
        self.broadcast(format!(
            "Deployment complete. Commander {} moves first.",
            self.turn + 1
        ));
    }
    pub fn apply(
        &mut self,
        p: usize,
        action: Action,
        now: u64,
        cfg: &Config,
    ) -> Result<(), String> {
        if p > 1 {
            return Err("Invalid player".into());
        }
        // The clock is settled before accepting a command, even if a tick is delayed.
        self.tick(now, cfg);
        if matches!(action, Action::Resign) {
            if matches!(self.phase, Phase::Finished { .. }) {
                return Err("Game has ended".into());
            }
            self.finish(Some(1 - p), "Opponent resigned");
            return Ok(());
        }
        match (&self.phase, action) {
            (Phase::Placement, action) => {
                if self.ready[p] {
                    return Err("Deployment already locked".into());
                }
                match action {
                    Action::Place { at, rank } => {
                        if !home(p, at) {
                            return Err("Place only in your four home rows".into());
                        }
                        if self.board[at].is_some() {
                            return Err("Remove the existing piece first".into());
                        }
                        if self.reserve(p, rank) == 0 {
                            return Err("No pieces of this rank left".into());
                        }
                        self.board[at] = Some(Piece { owner: p, rank });
                    }
                    Action::Remove { at } => {
                        if !home(p, at) {
                            return Err("Not your deployment square".into());
                        }
                        self.board[at] = None;
                    }
                    Action::AutoPlace => self.fill(p),
                    Action::Ready => {
                        if self.board.iter().flatten().filter(|v| v.owner == p).count() != 40 {
                            return Err("Place all 40 pieces before locking deployment".into());
                        }
                        self.ready[p] = true;
                        if self.ready == [true, true] {
                            self.begin_play(now, cfg);
                        }
                    }
                    _ => return Err("Deploy your army first".into()),
                }
            }
            (Phase::Playing, Action::Move { from, to }) => {
                if p != self.turn {
                    return Err("Wait for your turn".into());
                }
                if !self.legal(p, from, to) {
                    return Err("Illegal move: move one square in any direction; attack orthogonally. Scouts may travel on clear orthogonal lines.".into());
                }
                let attacker = self.board[from].take().unwrap();
                if let Some(defender) = self.board[to] {
                    self.note(
                        p,
                        format!("At {}: enemy {}.", square(to), defender.rank.name()),
                    );
                    self.note(
                        1 - p,
                        format!(
                            "Your {} was attacked at {}.",
                            defender.rank.name(),
                            square(to)
                        ),
                    );
                    let result = combat(attacker.rank, defender.rank);
                    if result >= 0 {
                        self.captured.push(defender);
                        self.board[to] = None;
                    }
                    if result <= 0 {
                        self.captured.push(attacker);
                    } else {
                        self.board[to] = Some(attacker);
                    }
                    if result < 0 {
                        self.note(1 - p, format!("Captured enemy {}.", attacker.rank.name()));
                    }
                    if result == 0 {
                        self.note(
                            1 - p,
                            format!("Equal ranks: enemy {} also fell.", attacker.rank.name()),
                        );
                    }
                    self.quiet = 0;
                    if defender.rank == Rank::Flag {
                        self.finish(Some(p), "Enemy flag captured");
                    }
                } else {
                    self.board[to] = Some(attacker);
                    self.quiet += 1;
                }
                self.missed[p] = 0;
                self.ply += 1;
                if matches!(self.phase, Phase::Playing) {
                    self.turn = 1 - p;
                    self.deadline = now + cfg.move_seconds * 1000;
                    self.check_draw(cfg);
                }
            }
            _ => return Err("Action unavailable in this phase".into()),
        }
        Ok(())
    }
    pub fn legal(&self, p: usize, from: usize, to: usize) -> bool {
        legal_move(&self.board, p, from, to)
    }
    fn has_move(&self, p: usize) -> bool {
        (0..CELLS).any(|a| (0..CELLS).any(|b| self.legal(p, a, b)))
    }
    fn check_draw(&mut self, cfg: &Config) {
        if !self.has_move(0) || !self.has_move(1) {
            self.finish(None, "Stalemate: a commander has no legal move");
        } else if self.quiet >= cfg.quiet_move_limit {
            self.finish(None, "No captures within the quiet move limit");
        }
    }
    pub fn tick(&mut self, now: u64, cfg: &Config) -> bool {
        if now < self.deadline || matches!(self.phase, Phase::Finished { .. }) {
            return false;
        }
        match self.phase {
            Phase::Waiting => self.finish(None, "Lobby expired"),
            Phase::Placement => {
                for p in 0..2 {
                    self.fill(p);
                    self.ready[p] = true;
                }
                self.broadcast("Deployment clock expired: remaining pieces placed automatically.");
                self.begin_play(now, cfg);
            }
            Phase::Playing => {
                let p = self.turn;
                self.missed[p] += 1;
                self.broadcast(format!(
                    "Commander {} ran out of time; turn forfeited ({}/{}).",
                    p + 1,
                    self.missed[p],
                    cfg.max_missed_turns
                ));
                if self.missed[p] >= cfg.max_missed_turns {
                    self.finish(Some(1 - p), "Opponent missed too many consecutive turns");
                } else {
                    self.turn = 1 - p;
                    self.deadline = now + cfg.move_seconds * 1000;
                    self.check_draw(cfg);
                }
            }
            Phase::Finished { .. } => {}
        }
        true
    }
    pub fn view(&self, p: usize, name: &str, now: u64) -> View {
        let placement = matches!(self.phase, Phase::Placement | Phase::Waiting);
        View {
            game: name.into(),
            you: p,
            phase: self.phase.clone(),
            board: self
                .board
                .iter()
                .map(|cell| {
                    cell.and_then(|v| {
                        if placement && v.owner != p {
                            None
                        } else {
                            Some(VisiblePiece {
                                owner: v.owner,
                                rank: if v.owner == p { Some(v.rank) } else { None },
                            })
                        }
                    })
                })
                .collect(),
            ready: self.ready,
            turn: self.turn,
            remaining_ms: self.deadline.saturating_sub(now),
            reserve: Rank::ALL
                .into_iter()
                .map(|r| (r, if placement { self.reserve(p, r) } else { 0 }))
                .collect(),
            captured: self.captured.clone(),
            messages: self.messages[p].clone(),
            missed: self.missed,
            ply: self.ply,
        }
    }
}
/// Positive: attacker survives. Zero: both removed. Negative: defender survives.
pub fn combat(a: Rank, b: Rank) -> i8 {
    if b == Rank::Flag {
        return 1;
    }
    if b == Rank::Mine {
        return if a == Rank::Miner { 1 } else { -1 };
    }
    if matches!((a, b), (Rank::Spy, Rank::Marshal)) {
        return 1;
    }
    if matches!((a, b), (Rank::Marshal, Rank::Spy)) {
        return -1;
    }
    match (b as usize).cmp(&(a as usize)) {
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Less => -1,
    }
}
pub fn square(at: usize) -> String {
    format!("{}{}", (b'A' + (at % 10) as u8) as char, 10 - at / 10)
}
pub mod net;
pub mod session;

pub fn legal_move(board: &[Option<Piece>], p: usize, from: usize, to: usize) -> bool {
    if board.len() != CELLS || from >= CELLS || to >= CELLS || from == to || blocked(to) {
        return false;
    }
    let Some(a) = board[from] else {
        return false;
    };
    if a.owner != p || !a.rank.mobile() || board[to].is_some_and(|b| b.owner == p) {
        return false;
    }
    let dx = (to % 10) as i32 - (from % 10) as i32;
    let dy = (to / 10) as i32 - (from / 10) as i32;
    if board[to].is_some() && dx != 0 && dy != 0 {
        return false;
    }
    if dx.abs() <= 1 && dy.abs() <= 1 {
        return true;
    }
    if a.rank != Rank::Scout || (dx != 0 && dy != 0) {
        return false;
    }
    let step = dx.signum() + 10 * dy.signum();
    let mut at = from as i32 + step;
    while at != to as i32 {
        if blocked(at as usize) || board[at as usize].is_some() {
            return false;
        }
        at += step;
    }
    true
}

impl View {
    pub fn legal_targets(&self, from: usize) -> Vec<usize> {
        if !matches!(self.phase, Phase::Playing) || self.turn != self.you {
            return vec![];
        }
        let board: Vec<_> = self
            .board
            .iter()
            .map(|cell| {
                cell.as_ref().map(|p| Piece {
                    owner: p.owner,
                    rank: p.rank.unwrap_or(Rank::Flag),
                })
            })
            .collect();
        (0..CELLS)
            .filter(|to| legal_move(&board, self.you, from, *to))
            .collect()
    }
}
