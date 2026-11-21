//! Authoritative rules and wire protocol. Coordinates are always server coordinates.
#[cfg(feature = "ai-client")]
pub mod ai;
pub mod connection;
pub mod hard_config;
pub mod visibility;

use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

pub const COLS: usize = 10;
pub const ROWS: usize = 11;
pub const CELLS: usize = COLS * ROWS;
pub const HOME_ROWS: usize = 4;

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
    CreateGame { game: String },
    JoinWaiting { game: String },
    ListGames,
    Place { at: usize, rank: Rank },
    Remove { at: usize },
    AutoPlace,
    Ready,
    Move { from: usize, to: usize },
    ClaimVictory { turn_id: u32 },
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
/// One authoritative move event, personalized only for rank visibility.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MoveView {
    pub ply: u32,
    pub from: usize,
    pub to: usize,
    pub piece: VisiblePiece,
    pub defender: Option<VisiblePiece>,
    pub piece_lost: bool,
    pub defender_lost: bool,
    pub elapsed_ms: u64,
}
struct MoveEvent {
    ply: u32,
    from: usize,
    to: usize,
    piece: Piece,
    defender: Option<Piece>,
    started_ms: u64,
}
/// Public per-match rules and clock settings; no server admission limits or secrets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchSettings {
    pub placement_seconds: u64,
    pub turn_delay_seconds: u64,
    pub main_time_seconds: u64,
    pub quiet_move_limit: u32,
}
impl From<&Config> for MatchSettings {
    fn from(config: &Config) -> Self {
        Self {
            placement_seconds: config.placement_seconds,
            turn_delay_seconds: config.turn_delay_seconds,
            main_time_seconds: config.main_time_seconds,
            quiet_move_limit: config.quiet_move_limit,
        }
    }
}
impl Default for MatchSettings {
    fn default() -> Self {
        Self::from(&Config::default())
    }
}
/// An authoritative snapshot; array indices always use server player IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockView {
    pub main_ms: [u64; 2],
    pub delay_ms: u64,
    pub claimable: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View {
    pub game: String,
    #[serde(default)]
    pub settings: Box<MatchSettings>,
    pub you: usize,
    pub phase: Phase,
    pub board: Vec<Option<VisiblePiece>>,
    pub ready: [bool; 2],
    pub turn: usize,
    pub remaining_ms: u64,
    pub reserve: Vec<(Rank, usize)>,
    pub captured: Vec<Piece>,
    pub messages: Vec<String>,
    pub clocks: Box<ClockView>,
    pub ply: u32,
    #[serde(default)]
    pub motion: Option<Box<MoveView>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    Games { games: Vec<String> },
    State { view: View },
    Error { message: String },
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub bind: String,
    pub placement_seconds: u64,
    pub turn_delay_seconds: u64,
    pub main_time_seconds: u64,
    pub quiet_move_limit: u32,
    pub max_rooms: usize,
    pub max_connections: usize,
    pub waiting_seconds: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            bind: format!("0.0.0.0:{}", hard_config::DEFAULT_PORT),
            placement_seconds: 120,
            turn_delay_seconds: 10,
            main_time_seconds: 180,
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
            || self.main_time_seconds == 0
            || self.waiting_seconds == 0
            || self.placement_seconds > 86400
            || self.main_time_seconds > 86400
            || self.turn_delay_seconds > 86400
            || self.waiting_seconds > 86400
            || self.quiet_move_limit == 0
            || self.max_rooms == 0
            || self.max_connections == 0
        {
            return Err(
                "Main/setup/waiting time must be 1..86400 seconds; turn delay 0..86400; other limits must be positive".into(),
            );
        }
        Ok(())
    }
}
pub fn blocked(at: usize) -> bool {
    at < CELLS && (4..=6).contains(&(at / COLS)) && matches!(at % COLS, 3 | 6)
}
pub fn home(owner: usize, at: usize) -> bool {
    at < CELLS
        && if owner == 0 {
            at / COLS >= ROWS - HOME_ROWS
        } else {
            at / COLS < HOME_ROWS
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
    main_ms: [u64; 2],
    clock_accounted_at: u64,
    delay_until: u64,
    claimable: bool,
    pub ply: u32,
    quiet: u32,
    motion: Option<MoveEvent>,
    settings: MatchSettings,
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
            main_ms: [cfg.main_time_seconds * 1000; 2],
            clock_accounted_at: now,
            delay_until: now + cfg.turn_delay_seconds * 1000,
            claimable: false,
            ply: 0,
            quiet: 0,
            motion: None,
            settings: MatchSettings::from(cfg),
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
        self.main_ms = [cfg.main_time_seconds * 1000; 2];
        self.begin_turn(now, cfg);
        self.broadcast(format!(
            "Deployment complete. Commander {} moves first.",
            self.turn + 1
        ));
    }
    fn begin_turn(&mut self, now: u64, cfg: &Config) {
        self.delay_until = now + cfg.turn_delay_seconds * 1000;
        self.clock_accounted_at = now;
        self.claimable = false;
        self.deadline = 0;
    }
    fn clock_view(&self, now: u64) -> ClockView {
        let mut clocks = ClockView {
            main_ms: self.main_ms,
            delay_ms: 0,
            claimable: false,
        };
        if matches!(self.phase, Phase::Playing) {
            let now = now.max(self.clock_accounted_at);
            clocks.delay_ms = self.delay_until.saturating_sub(now);
            let charged = now.saturating_sub(self.clock_accounted_at.max(self.delay_until));
            clocks.main_ms[self.turn] = clocks.main_ms[self.turn].saturating_sub(charged);
            clocks.claimable = clocks.delay_ms == 0 && clocks.main_ms[self.turn] == 0;
        }
        clocks
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
            (Phase::Playing, Action::ClaimVictory { turn_id }) => {
                if p == self.turn || turn_id != self.ply || !self.clock_view(now).claimable {
                    return Err("Victory can only be claimed against an expired opponent clock in the current turn".into());
                }
                self.finish(Some(p), "Victory claimed on time");
            }
            (Phase::Playing, Action::Move { from, to }) => {
                if p != self.turn {
                    return Err("Wait for your turn".into());
                }
                if !self.legal(p, from, to) {
                    return Err("Illegal move: move one square in any direction; attack orthogonally. Scouts may travel on clear orthogonal lines.".into());
                }
                let attacker = self.board[from].take().unwrap();
                self.motion = Some(MoveEvent {
                    ply: self.ply + 1,
                    from,
                    to,
                    piece: attacker,
                    defender: self.board[to],
                    started_ms: now,
                });
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
                self.ply += 1;
                if matches!(self.phase, Phase::Playing) {
                    self.turn = 1 - p;
                    self.begin_turn(now, cfg);
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
        if matches!(self.phase, Phase::Playing) {
            let clocks = self.clock_view(now);
            let changed = clocks.claimable != self.claimable;
            self.main_ms = clocks.main_ms;
            self.clock_accounted_at = now.max(self.clock_accounted_at);
            self.claimable = clocks.claimable;
            return changed;
        }
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
            Phase::Playing | Phase::Finished { .. } => {}
        }
        true
    }
    pub fn view(&self, p: usize, name: &str, now: u64) -> View {
        let placement = matches!(self.phase, Phase::Placement | Phase::Waiting);
        View {
            game: name.into(),
            settings: Box::new(self.settings.clone()),
            you: p,
            phase: self.phase.clone(),
            board: self
                .board
                .iter()
                .map(|cell| {
                    cell.and_then(|v| {
                        if matches!(self.phase, Phase::Waiting) && v.owner != p {
                            None
                        } else {
                            Some(VisiblePiece {
                                owner: v.owner,
                                rank: if v.owner == p
                                    || matches!(self.phase, Phase::Finished { .. })
                                {
                                    Some(v.rank)
                                } else {
                                    None
                                },
                            })
                        }
                    })
                })
                .collect(),
            ready: self.ready,
            turn: self.turn,
            remaining_ms: if placement {
                self.deadline.saturating_sub(now)
            } else {
                0
            },
            reserve: Rank::ALL
                .into_iter()
                .map(|r| (r, if placement { self.reserve(p, r) } else { 0 }))
                .collect(),
            captured: self.captured.clone(),
            messages: self.messages[p].clone(),
            clocks: Box::new(self.clock_view(now)),
            ply: self.ply,
            motion: self
                .motion
                .as_ref()
                .map(|event| {
                    let result = event
                        .defender
                        .map(|defender| combat(event.piece.rank, defender.rank));
                    let finished = matches!(self.phase, Phase::Finished { .. });
                    let piece_lost = result.is_some_and(|result| result <= 0);
                    MoveView {
                        ply: event.ply,
                        from: event.from,
                        to: event.to,
                        piece: VisiblePiece {
                            owner: event.piece.owner,
                            rank: (event.piece.owner == p || piece_lost || finished)
                                .then_some(event.piece.rank),
                        },
                        // The attacker already receives the defending rank privately.
                        defender: event.defender.map(|defender| VisiblePiece {
                            owner: defender.owner,
                            rank: Some(defender.rank),
                        }),
                        piece_lost,
                        defender_lost: result.is_some_and(|result| result >= 0),
                        elapsed_ms: now.saturating_sub(event.started_ms),
                    }
                })
                .map(Box::new),
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
    format!("{}{}", (b'A' + (at % COLS) as u8) as char, ROWS - at / COLS)
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
    let dx = (to % COLS) as i32 - (from % COLS) as i32;
    let dy = (to / COLS) as i32 - (from / COLS) as i32;
    if board[to].is_some() && dx != 0 && dy != 0 {
        return false;
    }
    if dx.abs() <= 1 && dy.abs() <= 1 {
        return true;
    }
    if a.rank != Rank::Scout || (dx != 0 && dy != 0) {
        return false;
    }
    let step = dx.signum() + COLS as i32 * dy.signum();
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
    /// Interpolate a server snapshot locally; claim eligibility stays server-owned.
    pub fn clock_after(&self, elapsed_ms: u64) -> ClockView {
        let mut clocks = *self.clocks;
        if matches!(self.phase, Phase::Playing) {
            let charged = elapsed_ms.saturating_sub(clocks.delay_ms);
            clocks.delay_ms = clocks.delay_ms.saturating_sub(elapsed_ms);
            clocks.main_ms[self.turn] = clocks.main_ms[self.turn].saturating_sub(charged);
        }
        clocks
    }
    pub fn can_claim_victory(&self) -> bool {
        matches!(self.phase, Phase::Playing) && self.you != self.turn && self.clocks.claimable
    }
    /// Remaining deployed inventory, using only the public capture history.
    pub fn remaining(&self, owner: usize, rank: Rank) -> usize {
        rank.count().saturating_sub(
            self.captured
                .iter()
                .filter(|piece| piece.owner == owner && piece.rank == rank)
                .count(),
        )
    }
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

/// Rotate the board for commander two. The transform is its own inverse.
pub fn orient(at: usize, owner: usize) -> usize {
    if owner == 0 { at } else { CELLS - 1 - at }
}
pub mod plan;

/// Irreversible runtime confinement for the dedicated multiplayer server.
pub mod sandbox;
