use crate::board::{MoveList, Position};

#[must_use]
pub fn perft(position: &Position, depth: u32) -> u64 {
    let mut position = position.clone();
    perft_in_place(&mut position, depth)
}

fn perft_in_place(position: &mut Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut moves = MoveList::with_capacity(96);
    position.legal_moves_in_place_into(&mut moves);
    if depth == 1 {
        return moves.len() as u64;
    }
    let mut nodes = 0;
    for mv in moves {
        let undo = position.make_move_in_place(mv);
        nodes += perft_in_place(position, depth - 1);
        position.unmake_move(undo);
    }
    nodes
}
