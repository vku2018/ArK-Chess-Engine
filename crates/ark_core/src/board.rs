use core::fmt;

use crate::mv::{Move, MoveFlag};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Color {
    White,
    Black,
}

impl Color {
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }

    #[must_use]
    pub const fn pawn_dir(self) -> i8 {
        match self {
            Self::White => 1,
            Self::Black => -1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Piece {
    pub color: Color,
    pub kind: PieceKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Square(u8);

impl Square {
    #[must_use]
    pub const fn new(index: u8) -> Option<Self> {
        if index < 64 {
            Some(Self(index))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub const fn file(self) -> i8 {
        (self.0 % 8) as i8
    }

    #[must_use]
    pub const fn rank(self) -> i8 {
        (self.0 / 8) as i8
    }

    #[must_use]
    pub fn from_coords(file: i8, rank: i8) -> Option<Self> {
        if (0..8).contains(&file) && (0..8).contains(&rank) {
            Some(Self((rank * 8 + file) as u8))
        } else {
            None
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 2 {
            return None;
        }
        let file = (bytes[0] as char).to_ascii_lowercase() as u8;
        let rank = bytes[1];
        if !(b'a'..=b'h').contains(&file) || !(b'1'..=b'8').contains(&rank) {
            return None;
        }
        Self::from_coords((file - b'a') as i8, (rank - b'1') as i8)
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let file = (b'a' + self.file() as u8) as char;
        let rank = (b'1' + self.rank() as u8) as char;
        write!(f, "{file}{rank}")
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum FenError {
    WrongFieldCount,
    BadBoard,
    BadSide,
    BadCastling,
    BadEnPassant,
    BadClock,
    MissingKing,
    TooManyKings,
    PawnOnBackRank,
    KingsTouch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    board: [Option<Piece>; 64],
    bitboards: [u64; 12],
    side_to_move: Color,
    castling: u8,
    en_passant: Option<Square>,
    halfmove_clock: u32,
    fullmove_number: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MoveUndo {
    mv: Move,
    moving: Option<Piece>,
    captured: Option<Piece>,
    en_passant_capture: Option<(Square, Piece)>,
    castle_rook: Option<(Square, Square, Option<Piece>)>,
    castling: u8,
    en_passant: Option<Square>,
    halfmove_clock: u32,
    fullmove_number: u32,
    side_to_move: Color,
}

pub type MoveList = Vec<Move>;

const CASTLE_WHITE_KING: u8 = 0b0001;
const CASTLE_WHITE_QUEEN: u8 = 0b0010;
const CASTLE_BLACK_KING: u8 = 0b0100;
const CASTLE_BLACK_QUEEN: u8 = 0b1000;
const WHITE_KING_START: Square = Square(4);
const WHITE_QUEEN_ROOK_START: Square = Square(0);
const WHITE_KING_ROOK_START: Square = Square(7);
const BLACK_KING_START: Square = Square(60);
const BLACK_QUEEN_ROOK_START: Square = Square(56);
const BLACK_KING_ROOK_START: Square = Square(63);
const MOVE_LIST_CAPACITY: usize = 96;

impl Position {
    pub const START_FEN: &'static str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[must_use]
    pub fn empty() -> Self {
        Self {
            board: [None; 64],
            bitboards: [0; 12],
            side_to_move: Color::White,
            castling: 0,
            en_passant: None,
            halfmove_clock: 0,
            fullmove_number: 1,
        }
    }

    pub fn startpos() -> Result<Self, FenError> {
        Self::from_fen(Self::START_FEN)
    }

    pub fn from_fen(fen: &str) -> Result<Self, FenError> {
        let fields: Vec<&str> = fen.split_whitespace().collect();
        if fields.len() != 6 {
            return Err(FenError::WrongFieldCount);
        }
        let mut position = Self::empty();
        parse_board(fields[0], &mut position)?;
        position.side_to_move = match fields[1] {
            "w" => Color::White,
            "b" => Color::Black,
            _ => return Err(FenError::BadSide),
        };
        position.castling = parse_castling(fields[2])?;
        position.en_passant = if fields[3] == "-" {
            None
        } else {
            Some(Square::parse(fields[3]).ok_or(FenError::BadEnPassant)?)
        };
        position.halfmove_clock = fields[4].parse().map_err(|_err| FenError::BadClock)?;
        position.fullmove_number = fields[5].parse().map_err(|_err| FenError::BadClock)?;
        validate_fen_position(&position)?;
        Ok(position)
    }

    #[must_use]
    pub fn to_fen(&self) -> String {
        let mut board = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                let sq = Square::from_coords(file, rank).unwrap_or(Square(0));
                match self.piece_at(sq) {
                    Some(piece) => {
                        if empty > 0 {
                            board.push(char::from_digit(empty, 10).unwrap_or('0'));
                            empty = 0;
                        }
                        board.push(piece_to_char(piece));
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                board.push(char::from_digit(empty, 10).unwrap_or('0'));
            }
            if rank > 0 {
                board.push('/');
            }
        }
        format!(
            "{} {} {} {} {} {}",
            board,
            if self.side_to_move == Color::White {
                "w"
            } else {
                "b"
            },
            castling_to_text(self.castling),
            self.en_passant
                .map_or_else(|| "-".to_string(), |sq| sq.to_string()),
            self.halfmove_clock,
            self.fullmove_number
        )
    }

    #[must_use]
    pub const fn side_to_move(&self) -> Color {
        self.side_to_move
    }

    #[must_use]
    pub fn piece_at(&self, square: Square) -> Option<Piece> {
        self.board[square.index()]
    }

    #[must_use]
    pub fn occupancy(&self) -> u64 {
        self.bitboards.iter().fold(0, |acc, board| acc | board)
    }

    #[must_use]
    pub fn color_occupancy(&self, color: Color) -> u64 {
        [
            PieceKind::Pawn,
            PieceKind::Knight,
            PieceKind::Bishop,
            PieceKind::Rook,
            PieceKind::Queen,
            PieceKind::King,
        ]
        .into_iter()
        .fold(0, |acc, kind| {
            acc | self.bitboards[piece_index(Piece { color, kind })]
        })
    }

    #[must_use]
    pub fn legal_moves(&self) -> MoveList {
        let mut legal = MoveList::with_capacity(MOVE_LIST_CAPACITY);
        self.legal_moves_into(&mut legal);
        legal
    }

    pub fn legal_moves_into(&self, legal: &mut MoveList) {
        let mut probe = self.clone();
        probe.legal_moves_in_place_into(legal);
    }

    pub fn legal_moves_in_place_into(&mut self, legal: &mut MoveList) {
        let mut pseudo = MoveList::with_capacity(MOVE_LIST_CAPACITY);
        self.legal_moves_in_place_with_workspace(legal, &mut pseudo);
    }

    pub fn legal_moves_in_place_with_workspace(
        &mut self,
        legal: &mut MoveList,
        pseudo: &mut MoveList,
    ) {
        legal.clear();
        self.pseudo_legal_moves_into(pseudo);
        legal.reserve(pseudo.len());
        let side = self.side_to_move;
        for mv in pseudo.iter().copied() {
            let undo = self.make_move_in_place(mv);
            if !self.in_check(side) {
                legal.push(mv);
            }
            self.unmake_move(undo);
        }
    }

    #[must_use]
    pub fn make_move(&self, mv: Move) -> Self {
        let mut next = self.clone();
        next.make_move_in_place(mv);
        next
    }

    pub fn make_move_in_place(&mut self, mv: Move) -> MoveUndo {
        let moving = self.piece_at(mv.from());
        let captured = if matches!(mv.flag(), MoveFlag::EnPassant) {
            None
        } else {
            self.piece_at(mv.to())
        };
        let en_passant_capture = if matches!(mv.flag(), MoveFlag::EnPassant) {
            Square::from_coords(mv.to().file(), mv.from().rank())
                .and_then(|square| self.piece_at(square).map(|piece| (square, piece)))
        } else {
            None
        };
        let castle_rook = castle_rook_move(mv).map(|(rook_from, rook_to)| {
            let rook = self.piece_at(rook_from);
            (rook_from, rook_to, rook)
        });
        let undo = MoveUndo {
            mv,
            moving,
            captured,
            en_passant_capture,
            castle_rook,
            castling: self.castling,
            en_passant: self.en_passant,
            halfmove_clock: self.halfmove_clock,
            fullmove_number: self.fullmove_number,
            side_to_move: self.side_to_move,
        };

        self.set_piece(mv.from(), None);
        if matches!(mv.flag(), MoveFlag::EnPassant) {
            if let Some((captured, _piece)) = en_passant_capture {
                self.set_piece(captured, None);
            }
        }
        if matches!(mv.flag(), MoveFlag::KingCastle | MoveFlag::QueenCastle) {
            self.move_castle_rook(mv);
        }
        self.set_piece(
            mv.to(),
            moving.map(|piece| Piece {
                kind: mv.promotion().unwrap_or(piece.kind),
                ..piece
            }),
        );
        self.update_castling_rights(mv, moving, captured);
        self.en_passant = if matches!(mv.flag(), MoveFlag::DoublePawnPush) {
            Square::from_coords(mv.from().file(), (mv.from().rank() + mv.to().rank()) / 2)
        } else {
            None
        };
        self.halfmove_clock =
            if moving.is_some_and(|p| p.kind == PieceKind::Pawn) || mv.is_capture() {
                0
            } else {
                self.halfmove_clock + 1
            };
        if undo.side_to_move == Color::Black {
            self.fullmove_number += 1;
        }
        self.side_to_move = self.side_to_move.opposite();
        undo
    }

    pub fn unmake_move(&mut self, undo: MoveUndo) {
        self.side_to_move = undo.side_to_move;
        self.castling = undo.castling;
        self.en_passant = undo.en_passant;
        self.halfmove_clock = undo.halfmove_clock;
        self.fullmove_number = undo.fullmove_number;

        self.set_piece(undo.mv.to(), undo.captured);
        self.set_piece(undo.mv.from(), undo.moving);
        if let Some((captured_square, captured_piece)) = undo.en_passant_capture {
            self.set_piece(captured_square, Some(captured_piece));
            self.set_piece(undo.mv.to(), None);
        }
        if let Some((rook_from, rook_to, rook)) = undo.castle_rook {
            self.set_piece(rook_to, None);
            self.set_piece(rook_from, rook);
        }
    }

    #[must_use]
    pub fn move_from_uci(&self, text: &str) -> Option<Move> {
        self.legal_moves()
            .into_iter()
            .find(|mv| mv.to_string().eq_ignore_ascii_case(text))
    }

    #[must_use]
    pub fn move_from_packed_id(&self, id: u16) -> Option<Move> {
        self.legal_moves()
            .into_iter()
            .find(|mv| mv.packed_id() == id)
    }

    #[must_use]
    pub fn make_uci_move(&self, text: &str) -> Option<Self> {
        self.move_from_uci(text).map(|mv| self.make_move(mv))
    }

    #[must_use]
    pub fn is_terminal(&self) -> Option<&'static str> {
        let mut legal = MoveList::with_capacity(MOVE_LIST_CAPACITY);
        self.legal_moves_into(&mut legal);
        if legal.is_empty() {
            if self.in_check(self.side_to_move) {
                Some(if self.side_to_move == Color::White {
                    "0-1"
                } else {
                    "1-0"
                })
            } else {
                Some("1/2-1/2")
            }
        } else if self.halfmove_clock >= 100 {
            Some("1/2-1/2")
        } else {
            None
        }
    }

    #[must_use]
    pub fn zobrist(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for (board_index, mut board) in self.bitboards.iter().copied().enumerate() {
            while board != 0 {
                let index = board.trailing_zeros() as usize;
                board &= board - 1;
                let piece = piece_from_index(board_index);
                hash = hash.wrapping_mul(0x100_0000_01b3);
                hash ^= piece_hash(piece).wrapping_add(index as u64);
            }
        }
        hash ^= match self.side_to_move {
            Color::White => 0x9e37_79b9_7f4a_7c15,
            Color::Black => 0xc2b2_ae3d_27d4_eb4f,
        };
        hash ^ ((self.castling as u64) << 48)
            ^ self.en_passant.map_or(0, |sq| (sq.index() as u64) << 40)
    }

    #[must_use]
    pub fn in_check(&self, color: Color) -> bool {
        self.king_square(color)
            .is_some_and(|king| self.is_square_attacked(king, color.opposite()))
    }

    #[must_use]
    pub fn is_square_attacked(&self, square: Square, by: Color) -> bool {
        for index in 0..64 {
            let from = Square(index as u8);
            if self.piece_at(from).is_some_and(|piece| piece.color == by)
                && self.piece_attacks_square(from, square)
            {
                return true;
            }
        }
        false
    }

    fn pseudo_legal_moves_into(&self, moves: &mut MoveList) {
        moves.clear();
        for index in 0..64 {
            let from = Square(index as u8);
            let Some(piece) = self.piece_at(from) else {
                continue;
            };
            if piece.color != self.side_to_move {
                continue;
            }
            match piece.kind {
                PieceKind::Pawn => self.pawn_moves(from, piece.color, moves),
                PieceKind::Knight => self.leaper_moves(from, &KNIGHT_DELTAS, moves),
                PieceKind::Bishop => self.slider_moves(from, &BISHOP_DELTAS, moves),
                PieceKind::Rook => self.slider_moves(from, &ROOK_DELTAS, moves),
                PieceKind::Queen => self.slider_moves(from, &QUEEN_DELTAS, moves),
                PieceKind::King => self.king_moves(from, moves),
            }
        }
    }

    fn pawn_moves(&self, from: Square, color: Color, moves: &mut Vec<Move>) {
        let dir = color.pawn_dir();
        let start_rank = if color == Color::White { 1 } else { 6 };
        let promotion_rank = if color == Color::White { 7 } else { 0 };
        if let Some(one) = Square::from_coords(from.file(), from.rank() + dir) {
            if self.piece_at(one).is_none() {
                self.push_pawn_move(from, one, false, promotion_rank, moves);
                if from.rank() == start_rank {
                    if let Some(two) = Square::from_coords(from.file(), from.rank() + 2 * dir) {
                        if self.piece_at(two).is_none() {
                            moves.push(Move::new(from, two, MoveFlag::DoublePawnPush));
                        }
                    }
                }
            }
        }
        for df in [-1, 1] {
            if let Some(to) = Square::from_coords(from.file() + df, from.rank() + dir) {
                let capture = self.piece_at(to).is_some_and(|piece| piece.color != color);
                if capture {
                    self.push_pawn_move(from, to, true, promotion_rank, moves);
                } else if self.en_passant == Some(to) {
                    moves.push(Move::new(from, to, MoveFlag::EnPassant));
                }
            }
        }
    }

    fn set_piece(&mut self, square: Square, piece: Option<Piece>) {
        let mask = bit(square);
        if let Some(existing) = self.board[square.index()] {
            self.bitboards[piece_index(existing)] &= !mask;
        }
        self.board[square.index()] = piece;
        if let Some(piece) = piece {
            self.bitboards[piece_index(piece)] |= mask;
        }
    }

    fn push_pawn_move(
        &self,
        from: Square,
        to: Square,
        capture: bool,
        promotion_rank: i8,
        moves: &mut Vec<Move>,
    ) {
        if to.rank() == promotion_rank {
            for kind in [
                PieceKind::Queen,
                PieceKind::Rook,
                PieceKind::Bishop,
                PieceKind::Knight,
            ] {
                moves.push(Move::new(
                    from,
                    to,
                    if capture {
                        MoveFlag::PromotionCapture(kind)
                    } else {
                        MoveFlag::Promotion(kind)
                    },
                ));
            }
        } else {
            moves.push(Move::new(
                from,
                to,
                if capture {
                    MoveFlag::Capture
                } else {
                    MoveFlag::Quiet
                },
            ));
        }
    }

    fn leaper_moves(&self, from: Square, deltas: &[(i8, i8)], moves: &mut Vec<Move>) {
        for (df, dr) in deltas {
            if let Some(to) = Square::from_coords(from.file() + df, from.rank() + dr) {
                self.push_non_pawn_move(from, to, moves);
            }
        }
    }

    fn slider_moves(&self, from: Square, deltas: &[(i8, i8)], moves: &mut Vec<Move>) {
        for (df, dr) in deltas {
            let mut file = from.file() + df;
            let mut rank = from.rank() + dr;
            while let Some(to) = Square::from_coords(file, rank) {
                if !self.push_non_pawn_move(from, to, moves) {
                    break;
                }
                file += df;
                rank += dr;
            }
        }
    }

    fn king_moves(&self, from: Square, moves: &mut Vec<Move>) {
        self.leaper_moves(from, &KING_DELTAS, moves);
        self.castle_moves(from, moves);
    }

    fn castle_moves(&self, from: Square, moves: &mut Vec<Move>) {
        if self.in_check(self.side_to_move) {
            return;
        }
        match self.side_to_move {
            Color::White if from == Square::parse("e1").unwrap_or(Square(4)) => {
                self.try_castle(
                    "h1",
                    "f1",
                    "g1",
                    CASTLE_WHITE_KING,
                    MoveFlag::KingCastle,
                    moves,
                );
                self.try_castle(
                    "a1",
                    "d1",
                    "c1",
                    CASTLE_WHITE_QUEEN,
                    MoveFlag::QueenCastle,
                    moves,
                );
            }
            Color::Black if from == Square::parse("e8").unwrap_or(Square(60)) => {
                self.try_castle(
                    "h8",
                    "f8",
                    "g8",
                    CASTLE_BLACK_KING,
                    MoveFlag::KingCastle,
                    moves,
                );
                self.try_castle(
                    "a8",
                    "d8",
                    "c8",
                    CASTLE_BLACK_QUEEN,
                    MoveFlag::QueenCastle,
                    moves,
                );
            }
            _ => {}
        }
    }

    fn try_castle(
        &self,
        rook_sq: &str,
        pass_sq: &str,
        king_to: &str,
        right: u8,
        flag: MoveFlag,
        moves: &mut Vec<Move>,
    ) {
        if self.castling & right == 0 {
            return;
        }
        let from = if self.side_to_move == Color::White {
            "e1"
        } else {
            "e8"
        };
        let Some(from) = Square::parse(from) else {
            return;
        };
        let Some(rook) = Square::parse(rook_sq) else {
            return;
        };
        let Some(pass) = Square::parse(pass_sq) else {
            return;
        };
        let Some(to) = Square::parse(king_to) else {
            return;
        };
        if !self
            .piece_at(rook)
            .is_some_and(|p| p.color == self.side_to_move && p.kind == PieceKind::Rook)
        {
            return;
        }
        let clear = match flag {
            MoveFlag::KingCastle => self.piece_at(pass).is_none() && self.piece_at(to).is_none(),
            MoveFlag::QueenCastle => {
                let between = Square::from_coords(1, from.rank());
                between.is_some_and(|sq| self.piece_at(sq).is_none())
                    && self.piece_at(pass).is_none()
                    && self.piece_at(to).is_none()
            }
            _ => false,
        };
        if clear
            && !self.is_square_attacked(pass, self.side_to_move.opposite())
            && !self.is_square_attacked(to, self.side_to_move.opposite())
        {
            moves.push(Move::new(from, to, flag));
        }
    }

    fn push_non_pawn_move(&self, from: Square, to: Square, moves: &mut Vec<Move>) -> bool {
        match self.piece_at(to) {
            Some(piece) if piece.color == self.side_to_move => false,
            Some(_) => {
                moves.push(Move::new(from, to, MoveFlag::Capture));
                false
            }
            None => {
                moves.push(Move::new(from, to, MoveFlag::Quiet));
                true
            }
        }
    }

    fn piece_attacks_square(&self, from: Square, target: Square) -> bool {
        let Some(piece) = self.piece_at(from) else {
            return false;
        };
        let df = target.file() - from.file();
        let dr = target.rank() - from.rank();
        match piece.kind {
            PieceKind::Pawn => dr == piece.color.pawn_dir() && df.abs() == 1,
            PieceKind::Knight => KNIGHT_DELTAS.contains(&(df, dr)),
            PieceKind::Bishop => self.clear_slider(from, target, df, dr, &BISHOP_DELTAS),
            PieceKind::Rook => self.clear_slider(from, target, df, dr, &ROOK_DELTAS),
            PieceKind::Queen => self.clear_slider(from, target, df, dr, &QUEEN_DELTAS),
            PieceKind::King => df.abs() <= 1 && dr.abs() <= 1 && (df != 0 || dr != 0),
        }
    }

    fn clear_slider(
        &self,
        from: Square,
        target: Square,
        df: i8,
        dr: i8,
        deltas: &[(i8, i8)],
    ) -> bool {
        let step_file = df.signum();
        let step_rank = dr.signum();
        if !deltas.contains(&(step_file, step_rank)) {
            return false;
        }
        if df != 0 && dr != 0 && df.abs() != dr.abs() {
            return false;
        }
        let mut file = from.file() + step_file;
        let mut rank = from.rank() + step_rank;
        while let Some(square) = Square::from_coords(file, rank) {
            if square == target {
                return true;
            }
            if self.piece_at(square).is_some() {
                return false;
            }
            file += step_file;
            rank += step_rank;
        }
        false
    }

    fn king_square(&self, color: Color) -> Option<Square> {
        let board = self.bitboards[piece_index(Piece {
            color,
            kind: PieceKind::King,
        })];
        if board == 0 {
            None
        } else {
            Square::new(board.trailing_zeros() as u8)
        }
    }

    fn move_castle_rook(&mut self, mv: Move) {
        if let Some((rook_from, rook_to)) = castle_rook_move(mv) {
            let rook = self.piece_at(rook_from);
            self.set_piece(rook_from, None);
            self.set_piece(rook_to, rook);
        }
    }

    fn update_castling_rights(&mut self, mv: Move, moving: Option<Piece>, captured: Option<Piece>) {
        if moving.is_some_and(|p| p.kind == PieceKind::King) {
            match moving.map(|p| p.color) {
                Some(Color::White) => self.castling &= !(CASTLE_WHITE_KING | CASTLE_WHITE_QUEEN),
                Some(Color::Black) => self.castling &= !(CASTLE_BLACK_KING | CASTLE_BLACK_QUEEN),
                None => {}
            }
        }
        for (sq, mask) in [
            ("h1", CASTLE_WHITE_KING),
            ("a1", CASTLE_WHITE_QUEEN),
            ("h8", CASTLE_BLACK_KING),
            ("a8", CASTLE_BLACK_QUEEN),
        ] {
            if Square::parse(sq)
                .is_some_and(|rook| mv.from() == rook || (captured.is_some() && mv.to() == rook))
            {
                self.castling &= !mask;
            }
        }
    }
}

fn validate_fen_position(position: &Position) -> Result<(), FenError> {
    validate_king_count(position, Color::White)?;
    validate_king_count(position, Color::Black)?;
    validate_kings_do_not_touch(position)?;
    validate_castling_rights(position)?;
    validate_en_passant(position)?;
    Ok(())
}

fn validate_king_count(position: &Position, color: Color) -> Result<(), FenError> {
    match position.bitboards[piece_index(Piece {
        color,
        kind: PieceKind::King,
    })]
    .count_ones()
    {
        0 => Err(FenError::MissingKing),
        1 => Ok(()),
        _ => Err(FenError::TooManyKings),
    }
}

fn validate_kings_do_not_touch(position: &Position) -> Result<(), FenError> {
    let white = position
        .king_square(Color::White)
        .ok_or(FenError::MissingKing)?;
    let black = position
        .king_square(Color::Black)
        .ok_or(FenError::MissingKing)?;
    let df = (white.file() - black.file()).abs();
    let dr = (white.rank() - black.rank()).abs();
    if df <= 1 && dr <= 1 {
        Err(FenError::KingsTouch)
    } else {
        Ok(())
    }
}

fn validate_castling_rights(position: &Position) -> Result<(), FenError> {
    for (right, color, king, rook) in [
        (
            CASTLE_WHITE_KING,
            Color::White,
            WHITE_KING_START,
            WHITE_KING_ROOK_START,
        ),
        (
            CASTLE_WHITE_QUEEN,
            Color::White,
            WHITE_KING_START,
            WHITE_QUEEN_ROOK_START,
        ),
        (
            CASTLE_BLACK_KING,
            Color::Black,
            BLACK_KING_START,
            BLACK_KING_ROOK_START,
        ),
        (
            CASTLE_BLACK_QUEEN,
            Color::Black,
            BLACK_KING_START,
            BLACK_QUEEN_ROOK_START,
        ),
    ] {
        if position.castling & right != 0
            && (!is_piece(position, king, color, PieceKind::King)
                || !is_piece(position, rook, color, PieceKind::Rook))
        {
            return Err(FenError::BadCastling);
        }
    }
    Ok(())
}

fn validate_en_passant(position: &Position) -> Result<(), FenError> {
    let Some(target) = position.en_passant else {
        return Ok(());
    };
    let expected_rank = match position.side_to_move {
        Color::White => 5,
        Color::Black => 2,
    };
    if target.rank() != expected_rank || position.piece_at(target).is_some() {
        return Err(FenError::BadEnPassant);
    }
    let just_moved = position.side_to_move.opposite();
    let Some(pawn) = Square::from_coords(target.file(), target.rank() + just_moved.pawn_dir())
    else {
        return Err(FenError::BadEnPassant);
    };
    let Some(source) = Square::from_coords(target.file(), target.rank() - just_moved.pawn_dir())
    else {
        return Err(FenError::BadEnPassant);
    };
    if is_piece(position, pawn, just_moved, PieceKind::Pawn) && position.piece_at(source).is_none()
    {
        Ok(())
    } else {
        Err(FenError::BadEnPassant)
    }
}

fn is_piece(position: &Position, square: Square, color: Color, kind: PieceKind) -> bool {
    position
        .piece_at(square)
        .is_some_and(|piece| piece.color == color && piece.kind == kind)
}

fn castle_rook_move(mv: Move) -> Option<(Square, Square)> {
    if !matches!(mv.flag(), MoveFlag::KingCastle | MoveFlag::QueenCastle) {
        return None;
    }
    let rank = mv.from().rank();
    let (rook_from_file, rook_to_file) = if matches!(mv.flag(), MoveFlag::KingCastle) {
        (7, 5)
    } else {
        (0, 3)
    };
    Some((
        Square::from_coords(rook_from_file, rank)?,
        Square::from_coords(rook_to_file, rank)?,
    ))
}

const KNIGHT_DELTAS: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];
const KING_DELTAS: [(i8, i8); 8] = [
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
];
const BISHOP_DELTAS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, -1), (-1, 1)];
const ROOK_DELTAS: [(i8, i8); 4] = [(1, 0), (0, -1), (-1, 0), (0, 1)];
const QUEEN_DELTAS: [(i8, i8); 8] = [
    (1, 1),
    (1, -1),
    (-1, -1),
    (-1, 1),
    (1, 0),
    (0, -1),
    (-1, 0),
    (0, 1),
];

fn parse_board(text: &str, position: &mut Position) -> Result<(), FenError> {
    let ranks: Vec<&str> = text.split('/').collect();
    if ranks.len() != 8 {
        return Err(FenError::BadBoard);
    }
    for (rank_index, rank_text) in ranks.iter().enumerate() {
        let rank = 7 - rank_index as i8;
        let mut file = 0_i8;
        let mut previous_digit = false;
        for ch in rank_text.chars() {
            if ch.is_ascii_digit() {
                if previous_digit {
                    return Err(FenError::BadBoard);
                }
                let digit = ch.to_digit(10).ok_or(FenError::BadBoard)? as i8;
                if digit == 0 || file + digit > 8 {
                    return Err(FenError::BadBoard);
                }
                file += digit;
                previous_digit = true;
                continue;
            }
            previous_digit = false;
            let piece = char_to_piece(ch).ok_or(FenError::BadBoard)?;
            if piece.kind == PieceKind::Pawn && (rank == 0 || rank == 7) {
                return Err(FenError::PawnOnBackRank);
            }
            let Some(square) = Square::from_coords(file, rank) else {
                return Err(FenError::BadBoard);
            };
            position.set_piece(square, Some(piece));
            file += 1;
        }
        if file != 8 {
            return Err(FenError::BadBoard);
        }
    }
    Ok(())
}

fn parse_castling(text: &str) -> Result<u8, FenError> {
    if text == "-" {
        return Ok(0);
    }
    let mut rights = 0_u8;
    for ch in text.chars() {
        let bit = match ch {
            'K' => CASTLE_WHITE_KING,
            'Q' => CASTLE_WHITE_QUEEN,
            'k' => CASTLE_BLACK_KING,
            'q' => CASTLE_BLACK_QUEEN,
            _ => return Err(FenError::BadCastling),
        };
        if rights & bit != 0 {
            return Err(FenError::BadCastling);
        }
        rights |= bit;
    }
    Ok(rights)
}

fn castling_to_text(rights: u8) -> String {
    let mut text = String::new();
    if rights & CASTLE_WHITE_KING != 0 {
        text.push('K');
    }
    if rights & CASTLE_WHITE_QUEEN != 0 {
        text.push('Q');
    }
    if rights & CASTLE_BLACK_KING != 0 {
        text.push('k');
    }
    if rights & CASTLE_BLACK_QUEEN != 0 {
        text.push('q');
    }
    if text.is_empty() {
        text.push('-');
    }
    text
}

fn char_to_piece(ch: char) -> Option<Piece> {
    let color = if ch.is_ascii_uppercase() {
        Color::White
    } else {
        Color::Black
    };
    let kind = match ch.to_ascii_lowercase() {
        'p' => PieceKind::Pawn,
        'n' => PieceKind::Knight,
        'b' => PieceKind::Bishop,
        'r' => PieceKind::Rook,
        'q' => PieceKind::Queen,
        'k' => PieceKind::King,
        _ => return None,
    };
    Some(Piece { color, kind })
}

fn piece_to_char(piece: Piece) -> char {
    let ch = match piece.kind {
        PieceKind::Pawn => 'p',
        PieceKind::Knight => 'n',
        PieceKind::Bishop => 'b',
        PieceKind::Rook => 'r',
        PieceKind::Queen => 'q',
        PieceKind::King => 'k',
    };
    if piece.color == Color::White {
        ch.to_ascii_uppercase()
    } else {
        ch
    }
}

fn piece_hash(piece: Piece) -> u64 {
    let color = match piece.color {
        Color::White => 1,
        Color::Black => 2,
    };
    let kind = match piece.kind {
        PieceKind::Pawn => 1,
        PieceKind::Knight => 2,
        PieceKind::Bishop => 3,
        PieceKind::Rook => 4,
        PieceKind::Queen => 5,
        PieceKind::King => 6,
    };
    ((color * 17 + kind * 31) as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

fn bit(square: Square) -> u64 {
    1_u64 << square.index()
}

fn piece_index(piece: Piece) -> usize {
    let color_offset = match piece.color {
        Color::White => 0,
        Color::Black => 6,
    };
    let kind_offset = match piece.kind {
        PieceKind::Pawn => 0,
        PieceKind::Knight => 1,
        PieceKind::Bishop => 2,
        PieceKind::Rook => 3,
        PieceKind::Queen => 4,
        PieceKind::King => 5,
    };
    color_offset + kind_offset
}

fn piece_from_index(index: usize) -> Piece {
    let color = if index < 6 {
        Color::White
    } else {
        Color::Black
    };
    let kind = match index % 6 {
        0 => PieceKind::Pawn,
        1 => PieceKind::Knight,
        2 => PieceKind::Bishop,
        3 => PieceKind::Rook,
        4 => PieceKind::Queen,
        _ => PieceKind::King,
    };
    Piece { color, kind }
}
