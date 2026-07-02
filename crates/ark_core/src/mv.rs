use core::fmt;

use crate::board::{PieceKind, Square};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoveFlag {
    Quiet,
    Capture,
    DoublePawnPush,
    KingCastle,
    QueenCastle,
    EnPassant,
    Promotion(PieceKind),
    PromotionCapture(PieceKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Move {
    from: Square,
    to: Square,
    flag: MoveFlag,
}

impl Move {
    #[must_use]
    pub const fn new(from: Square, to: Square, flag: MoveFlag) -> Self {
        Self { from, to, flag }
    }

    #[must_use]
    pub const fn from(self) -> Square {
        self.from
    }

    #[must_use]
    pub const fn to(self) -> Square {
        self.to
    }

    #[must_use]
    pub const fn flag(self) -> MoveFlag {
        self.flag
    }

    #[must_use]
    pub const fn promotion(self) -> Option<PieceKind> {
        match self.flag {
            MoveFlag::Promotion(kind) | MoveFlag::PromotionCapture(kind) => Some(kind),
            _ => None,
        }
    }

    #[must_use]
    pub const fn is_capture(self) -> bool {
        matches!(
            self.flag,
            MoveFlag::Capture | MoveFlag::EnPassant | MoveFlag::PromotionCapture(_)
        )
    }

    #[must_use]
    pub fn packed_id(self) -> u16 {
        let promotion = match self.promotion() {
            Some(PieceKind::Knight) => 1,
            Some(PieceKind::Bishop) => 2,
            Some(PieceKind::Rook) => 3,
            Some(PieceKind::Queen) => 4,
            _ => 0,
        };
        self.from.index() as u16 | ((self.to.index() as u16) << 6) | (promotion << 12)
    }

    #[must_use]
    pub fn from_packed_id(id: u16) -> Option<Self> {
        let from = Square::new((id & 0b11_1111) as u8)?;
        let to = Square::new(((id >> 6) & 0b11_1111) as u8)?;
        let promotion = (id >> 12) & 0b111;
        let flag = match promotion {
            0 => MoveFlag::Quiet,
            1 => MoveFlag::Promotion(PieceKind::Knight),
            2 => MoveFlag::Promotion(PieceKind::Bishop),
            3 => MoveFlag::Promotion(PieceKind::Rook),
            4 => MoveFlag::Promotion(PieceKind::Queen),
            _ => return None,
        };
        Some(Self::new(from, to, flag))
    }
}

impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.from, self.to)?;
        if let Some(kind) = self.promotion() {
            let suffix = match kind {
                PieceKind::Queen => "q",
                PieceKind::Rook => "r",
                PieceKind::Bishop => "b",
                PieceKind::Knight => "n",
                PieceKind::Pawn | PieceKind::King => "",
            };
            write!(f, "{suffix}")?;
        }
        Ok(())
    }
}
