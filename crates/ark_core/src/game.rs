use std::collections::HashMap;

use crate::{Move, Position};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameOutcome {
    WhiteWin,
    BlackWin,
    Draw,
}

impl GameOutcome {
    #[must_use]
    pub fn from_terminal_text(text: &str) -> Option<Self> {
        match text {
            "1-0" => Some(Self::WhiteWin),
            "0-1" => Some(Self::BlackWin),
            "1/2-1/2" => Some(Self::Draw),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_terminal_text(self) -> &'static str {
        match self {
            Self::WhiteWin => "1-0",
            Self::BlackWin => "0-1",
            Self::Draw => "1/2-1/2",
        }
    }

    #[must_use]
    pub const fn white_score(self) -> i8 {
        match self {
            Self::WhiteWin => 1,
            Self::BlackWin => -1,
            Self::Draw => 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct GameState {
    position: Position,
    repetitions: HashMap<u64, u8>,
    plies: u32,
}

impl GameState {
    pub fn startpos() -> Result<Self, crate::FenError> {
        Self::from_position(Position::startpos()?)
    }

    pub fn from_position(position: Position) -> Result<Self, crate::FenError> {
        let mut repetitions = HashMap::new();
        repetitions.insert(position.zobrist(), 1);
        Ok(Self {
            position,
            repetitions,
            plies: 0,
        })
    }

    #[must_use]
    pub const fn position(&self) -> &Position {
        &self.position
    }

    #[must_use]
    pub const fn plies(&self) -> u32 {
        self.plies
    }

    #[must_use]
    pub fn outcome(&self) -> Option<GameOutcome> {
        if self
            .repetitions
            .get(&self.position.zobrist())
            .copied()
            .unwrap_or(0)
            >= 3
        {
            return Some(GameOutcome::Draw);
        }
        self.position
            .is_terminal()
            .and_then(GameOutcome::from_terminal_text)
    }

    pub fn make_move(&mut self, mv: Move) {
        self.position = self.position.make_move(mv);
        self.plies += 1;
        let entry = self.repetitions.entry(self.position.zobrist()).or_insert(0);
        *entry = entry.saturating_add(1);
    }
}
