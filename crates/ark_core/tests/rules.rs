use ark_core::{perft, Color, FenError, MoveFlag, MoveList, Position};

const START: &str = Position::START_FEN;
const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";

#[test]
fn startpos_fen_round_trips() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen(START)?;
    assert_eq!(position.to_fen(), START);
    assert_eq!(position.occupancy().count_ones(), 32);
    assert_eq!(
        position
            .color_occupancy(ark_core::Color::White)
            .count_ones(),
        16
    );
    assert_eq!(
        position
            .color_occupancy(ark_core::Color::Black)
            .count_ones(),
        16
    );
    Ok(())
}

#[test]
fn fen_rejects_structural_rule_violations() {
    let cases = [
        (
            "zero rank digit",
            "4k3/8/8/8/8/8/8/4K0N2 w - - 0 1",
            FenError::BadBoard,
        ),
        (
            "adjacent rank digits",
            "4k3/8/8/8/8/8/8/4K12 w - - 0 1",
            FenError::BadBoard,
        ),
        (
            "multiple white kings",
            "4k3/8/8/8/8/8/4K3/4K3 w - - 0 1",
            FenError::TooManyKings,
        ),
        (
            "white pawn on eighth rank",
            "P3k3/8/8/8/8/8/8/4K3 w - - 0 1",
            FenError::PawnOnBackRank,
        ),
        (
            "black pawn on first rank",
            "4k3/8/8/8/8/8/8/p3K3 w - - 0 1",
            FenError::PawnOnBackRank,
        ),
        (
            "bad en passant rank",
            "4k3/8/8/8/8/8/8/4K3 w - e4 0 1",
            FenError::BadEnPassant,
        ),
        (
            "occupied en passant target",
            "4k3/8/8/8/4P3/4N3/8/4K3 b - e3 0 1",
            FenError::BadEnPassant,
        ),
        (
            "missing just-moved en passant pawn",
            "4k3/8/8/8/8/8/8/4K3 b - e3 0 1",
            FenError::BadEnPassant,
        ),
        (
            "occupied en passant source",
            "4k3/8/8/8/4P3/8/4N3/4K3 b - e3 0 1",
            FenError::BadEnPassant,
        ),
        (
            "castling right with missing rook",
            "4k3/8/8/8/8/8/8/4K3 w K - 0 1",
            FenError::BadCastling,
        ),
        (
            "castling right with wrong rook color",
            "4k3/8/8/8/8/8/8/4K2r w K - 0 1",
            FenError::BadCastling,
        ),
        (
            "castling right with king off start square",
            "4k3/8/8/8/8/8/8/R2K3R w K - 0 1",
            FenError::BadCastling,
        ),
        (
            "adjacent kings",
            "8/8/8/8/8/8/4k3/4K3 w - - 0 1",
            FenError::KingsTouch,
        ),
    ];

    for (name, fen, expected) in cases {
        assert_eq!(
            Position::from_fen(fen),
            Err(expected),
            "{name} returned the wrong FEN error"
        );
    }
}

#[test]
fn fen_accepts_valid_en_passant_target_without_capturer() -> Result<(), ark_core::FenError> {
    let fen = "4k3/8/8/8/4P3/8/8/4K3 b - e3 0 1";
    let position = Position::from_fen(fen)?;
    assert_eq!(position.to_fen(), fen);
    Ok(())
}

#[test]
fn startpos_perft_matches_known_counts() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen(START)?;
    assert_eq!(perft(&position, 1), 20);
    assert_eq!(perft(&position, 2), 400);
    assert_eq!(perft(&position, 3), 8_902);
    Ok(())
}

#[test]
fn kiwipete_perft_covers_castling_and_pins() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen(KIWIPETE)?;
    assert_eq!(perft(&position, 1), 48);
    assert_eq!(perft(&position, 2), 2_039);
    Ok(())
}

#[test]
fn tactical_perft_position_matches_known_counts() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1")?;
    assert_eq!(perft(&position, 1), 14);
    assert_eq!(perft(&position, 2), 191);
    assert_eq!(perft(&position, 3), 2_812);
    Ok(())
}

#[test]
fn en_passant_that_exposes_king_is_illegal() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen("8/6bb/8/8/R1pP2k1/4P3/P7/K7 b - d3 0 1")?;
    let moves: Vec<String> = position
        .legal_moves()
        .into_iter()
        .map(|mv| mv.to_string())
        .collect();
    assert!(!moves.iter().any(|mv| mv == "c4d3"));
    Ok(())
}

#[test]
fn castling_through_attacked_square_is_illegal() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen("r3k2r/8/8/8/8/5r2/8/R3K2R w KQkq - 0 1")?;
    let moves: Vec<String> = position
        .legal_moves()
        .into_iter()
        .map(|mv| mv.to_string())
        .collect();
    assert!(!moves.iter().any(|mv| mv == "e1g1"));
    assert!(moves.iter().any(|mv| mv == "e1c1"));
    Ok(())
}

#[test]
fn promotions_emit_all_four_piece_choices() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1")?;
    let mut promotions: Vec<String> = position
        .legal_moves()
        .into_iter()
        .filter(|mv| matches!(mv.flag(), MoveFlag::Promotion(_)))
        .map(|mv| mv.to_string())
        .collect();
    promotions.sort();
    assert_eq!(promotions, ["a7a8b", "a7a8n", "a7a8q", "a7a8r"]);
    Ok(())
}

#[test]
fn make_and_unmake_restores_position() -> Result<(), ark_core::FenError> {
    let mut position = Position::startpos()?;
    let before = position.clone();
    let mv = position
        .move_from_uci("e2e4")
        .ok_or(ark_core::FenError::BadBoard)?;
    let undo = position.make_move_in_place(mv);
    assert_ne!(position, before);
    position.unmake_move(undo);
    assert_eq!(position, before);
    Ok(())
}

#[test]
fn make_and_unmake_restores_every_legal_move() -> Result<(), ark_core::FenError> {
    let mut position = Position::from_fen(KIWIPETE)?;
    let before = position.clone();
    let moves = position.legal_moves();
    assert_eq!(moves.len(), 48);
    for mv in moves {
        let undo = position.make_move_in_place(mv);
        assert_ne!(position, before);
        position.unmake_move(undo);
        assert_eq!(position, before);
    }
    Ok(())
}

#[test]
fn make_and_unmake_restores_special_moves() -> Result<(), ark_core::FenError> {
    let cases = [
        (
            "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
            "e1g1",
            "r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1",
        ),
        (
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
            "e5d6",
            "4k3/8/3P4/8/8/8/8/4K3 b - - 0 1",
        ),
        (
            "1n2k3/P7/8/8/8/8/8/4K3 w - - 0 1",
            "a7b8q",
            "1Q2k3/8/8/8/8/8/8/4K3 b - - 0 1",
        ),
    ];
    for (fen, uci, after) in cases {
        let mut position = Position::from_fen(fen)?;
        let before = position.clone();
        let mv = position
            .move_from_uci(uci)
            .ok_or(ark_core::FenError::BadBoard)?;
        let undo = position.make_move_in_place(mv);
        assert_eq!(position.to_fen(), after);
        position.unmake_move(undo);
        assert_eq!(position, before);
        assert_eq!(position.to_fen(), fen);
    }
    Ok(())
}

#[test]
fn legal_moves_from_check_all_resolve_check() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1")?;
    assert!(position.in_check(Color::White));
    let moves = position.legal_moves();
    assert!(!moves.is_empty());
    for mv in moves {
        let child = position.make_move(mv);
        assert!(!child.in_check(Color::White));
    }
    Ok(())
}

#[test]
fn reusable_move_list_matches_public_api_and_restores_position() -> Result<(), ark_core::FenError> {
    let mut position = Position::from_fen(KIWIPETE)?;
    let before = position.clone();
    let mut public: Vec<String> = position
        .legal_moves()
        .into_iter()
        .map(|mv| mv.to_string())
        .collect();
    let mut legal = MoveList::with_capacity(128);
    let mut pseudo = MoveList::with_capacity(128);
    position.legal_moves_in_place_with_workspace(&mut legal, &mut pseudo);
    let mut reusable: Vec<String> = legal.into_iter().map(|mv| mv.to_string()).collect();
    public.sort();
    reusable.sort();
    assert_eq!(reusable, public);
    assert_eq!(position, before);
    Ok(())
}
