//! Transport-independent room coordinator. The TCP server and session tests use this same code.
use crate::{Action, Config, Game, Phase, View};
use std::collections::HashMap;
struct Room {
    game: Game,
    seats: [Option<u64>; 2],
}
#[derive(Default)]
pub struct Sessions {
    rooms: HashMap<String, Room>,
    players: HashMap<u64, (String, usize)>,
}
impl Sessions {
    pub fn contains(&self, id: u64) -> bool {
        self.players.contains_key(&id)
    }
    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }
    pub fn command(
        &mut self,
        id: u64,
        action: Action,
        now: u64,
        cfg: &Config,
    ) -> Result<(), String> {
        if let Some((name, p)) = self.players.get(&id) {
            return self
                .rooms
                .get_mut(name)
                .unwrap()
                .game
                .apply(*p, action, now, cfg);
        }
        let Action::Join { game } = action else {
            return Err("Join a named game first".into());
        };
        let name = game.trim().to_string();
        if name.is_empty() || name.chars().count() > 48 || name.chars().any(char::is_control) {
            return Err("Game name must contain 1–48 printable characters".into());
        }
        if !self.rooms.contains_key(&name) && self.rooms.len() >= cfg.max_rooms {
            return Err("Server room limit reached".into());
        }
        let room = self.rooms.entry(name.clone()).or_insert_with(|| Room {
            game: Game::new(now, cfg),
            seats: [None, None],
        });
        room.game.tick(now, cfg);
        if !matches!(room.game.phase, Phase::Waiting) {
            return Err("That game has already started or ended. Choose a new name.".into());
        }
        let p = if room.seats[0].is_none() { 0 } else { 1 };
        room.seats[p] = Some(id);
        self.players.insert(id, (name, p));
        if room.seats.iter().all(Option::is_some) {
            room.game.start(now, cfg);
        }
        Ok(())
    }
    pub fn disconnect(&mut self, id: u64) {
        if let Some((name, p)) = self.players.remove(&id) {
            let room = self.rooms.get_mut(&name).unwrap();
            room.seats[p] = None;
            if room.seats.iter().all(Option::is_none) {
                self.rooms.remove(&name);
            } else if !matches!(room.game.phase, Phase::Finished { .. }) {
                room.game.finish(Some(1 - p), "Opponent disconnected");
            }
        }
    }
    pub fn tick(&mut self, now: u64, cfg: &Config) -> bool {
        let mut changed = false;
        for room in self.rooms.values_mut() {
            changed |= room.game.tick(now, cfg);
        }
        changed
    }
    pub fn view(&self, id: u64, now: u64) -> Option<View> {
        self.players
            .get(&id)
            .map(|(name, p)| self.rooms[name].game.view(*p, name, now))
    }
}
