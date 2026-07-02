use ark_core::{
    search, search_with_context, search_with_context_and_root_moves,
    search_with_context_and_stopper, search_with_move_orderer, Color, GameOutcome, GameState, Move,
    MoveList, Position, SearchLeafEvaluator, SearchMoveOrderer, SearchRequest, SearchStopper,
    StopReason,
};

fn request(depth: u32) -> SearchRequest {
    SearchRequest {
        depth,
        seed: 1,
        ..SearchRequest::default()
    }
}

#[test]
fn search_prefers_immediate_mate() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen("7k/6Q1/5K2/8/8/8/8/8 w - - 0 1")?;
    let request = request(1);
    let result = search(&position, &request);
    let best = result.best_move.ok_or(ark_core::FenError::BadBoard)?;
    let child = position.make_move(best);
    assert_eq!(child.is_terminal(), Some("1-0"));
    assert_eq!(result.score, 100_000);
    Ok(())
}

#[test]
fn quiet_non_terminal_leaf_stays_neutral() -> Result<(), ark_core::FenError> {
    let position = Position::startpos()?;
    let request = request(1);
    let result = search(&position, &request);

    assert_eq!(result.score, 0);
    assert!(result.trace.terminal_only);
    assert_eq!(result.trace.terminal_leaf_evals, 0);
    assert!(result.trace.neutral_frontier_evals > 0);
    assert_eq!(result.trace.external_leaf_eval_calls, 0);
    assert_eq!(result.trace.non_terminal_static_eval_calls, 0);
    assert_eq!(result.trace.tactical_extension_nodes, 0);
    assert_eq!(result.trace.tactical_extension_moves, 0);
    Ok(())
}

#[test]
fn tactical_check_extension_sees_forced_mate_at_frontier() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen("6k1/7N/5KQ1/8/8/8/8/8 b - - 0 1")?;
    let legal: Vec<String> = position
        .legal_moves()
        .into_iter()
        .map(|mv| mv.to_string())
        .collect();
    assert_eq!(legal, vec!["g8h8".to_string()]);

    let mut shallow = request(1);
    shallow.tactical_extension_depth = 0;
    let shallow_result = search(&position, &shallow);
    assert_eq!(shallow_result.score, 0);
    assert_eq!(shallow_result.trace.tactical_extension_nodes, 0);
    assert_eq!(shallow_result.trace.external_leaf_eval_calls, 0);
    assert_eq!(shallow_result.trace.non_terminal_static_eval_calls, 0);

    let mut extended = request(1);
    extended.tactical_extension_depth = 1;
    let result = search(&position, &extended);
    let pv: Vec<String> = result.pv.iter().map(|mv| mv.to_string()).collect();

    assert_eq!(result.score, -100_000);
    assert_eq!(pv, vec!["g8h8".to_string(), "g6g7".to_string()]);
    assert_eq!(result.trace.tactical_extension_depth, 1);
    assert_eq!(result.trace.tactical_extension_depth_reached, 1);
    assert!(result.trace.tactical_extension_nodes > 0);
    assert!(result.trace.tactical_extension_moves > 0);
    assert!(result.trace.terminal_leaf_evals > 0);
    assert_eq!(result.trace.external_leaf_eval_calls, 0);
    assert_eq!(result.trace.non_terminal_static_eval_calls, 0);
    Ok(())
}

#[test]
fn external_leaf_evaluator_changes_startpos_root_best_move() -> Result<(), ark_core::FenError> {
    let position = Position::startpos()?;
    let request = request(1);
    let neutral = search(&position, &request);
    let neutral_best = neutral.best_move.ok_or(ark_core::FenError::BadBoard)?;
    let preferred = position
        .legal_moves()
        .into_iter()
        .find(|mv| *mv != neutral_best)
        .ok_or(ark_core::FenError::BadBoard)?;
    let mut evaluator = PreferredChildEvaluator {
        from: preferred.from(),
        to: preferred.to(),
        calls: 0,
    };

    let result = search_with_context(&position, &request, None, Some(&mut evaluator));

    assert_ne!(neutral.best_move, Some(preferred));
    assert_eq!(result.best_move, Some(preferred));
    assert_eq!(result.score, 500);
    assert!(!result.trace.terminal_only);
    assert_eq!(result.trace.neutral_frontier_evals, 0);
    assert!(result.trace.external_leaf_eval_calls > 0);
    assert_eq!(result.trace.external_leaf_eval_calls, evaluator.calls);
    assert_eq!(result.trace.non_terminal_static_eval_calls, 0);
    assert_eq!(
        result.evals,
        result.trace.terminal_leaf_evals
            + result.trace.neutral_frontier_evals
            + result.trace.external_leaf_eval_calls
    );
    Ok(())
}

#[test]
fn external_leaf_evaluator_counts_tactical_extension_quiet_frontier(
) -> Result<(), ark_core::FenError> {
    let position = Position::startpos()?;
    let mut request = request(1);
    request.tactical_extension_depth = 1;
    let preferred = position
        .move_from_uci("e2e4")
        .ok_or(ark_core::FenError::BadBoard)?;
    let mut evaluator = PreferredChildEvaluator {
        from: preferred.from(),
        to: preferred.to(),
        calls: 0,
    };

    let result = search_with_context(&position, &request, None, Some(&mut evaluator));

    assert_eq!(result.best_move, Some(preferred));
    assert_eq!(result.score, 500);
    assert!(!result.trace.terminal_only);
    assert_eq!(result.trace.tactical_extension_depth, 1);
    assert_eq!(result.trace.tactical_extension_nodes, 0);
    assert_eq!(result.trace.tactical_extension_moves, 0);
    assert_eq!(result.trace.neutral_frontier_evals, 0);
    assert!(result.trace.external_leaf_eval_calls > 0);
    assert_eq!(result.trace.external_leaf_eval_calls, evaluator.calls);
    assert_eq!(result.trace.non_terminal_static_eval_calls, 0);
    Ok(())
}

#[test]
fn node_limit_does_not_claim_completed_depth() -> Result<(), ark_core::FenError> {
    let position = Position::startpos()?;
    let request = SearchRequest {
        depth: 4,
        nodes: Some(1),
        seed: 1,
        ..SearchRequest::default()
    };
    let result = search(&position, &request);
    assert!(result.best_move.is_some());
    assert!(result.nodes <= 1);
    assert_eq!(result.depth_reached, 0);
    assert_eq!(result.trace.requested_depth, 4);
    assert_eq!(result.trace.node_limit, Some(1));
    assert_eq!(result.trace.stopped_by, StopReason::Nodes);
    assert!(!result.trace.pv_complete);
    assert_eq!(result.trace.external_leaf_eval_calls, 0);
    assert_eq!(result.trace.non_terminal_static_eval_calls, 0);
    Ok(())
}

#[test]
fn node_limit_with_external_leaf_evaluator_stays_truthful() -> Result<(), ark_core::FenError> {
    let position = Position::startpos()?;
    let request = SearchRequest {
        depth: 4,
        nodes: Some(1),
        seed: 1,
        ..SearchRequest::default()
    };
    let preferred = position
        .move_from_uci("e2e4")
        .ok_or(ark_core::FenError::BadBoard)?;
    let mut evaluator = PreferredChildEvaluator {
        from: preferred.from(),
        to: preferred.to(),
        calls: 0,
    };

    let result = search_with_context(&position, &request, None, Some(&mut evaluator));

    assert!(result.best_move.is_some());
    assert!(result.nodes <= 1);
    assert_eq!(result.depth_reached, 0);
    assert_eq!(result.trace.requested_depth, 4);
    assert_eq!(result.trace.node_limit, Some(1));
    assert_eq!(result.trace.stopped_by, StopReason::Nodes);
    assert!(!result.trace.pv_complete);
    assert!(!result.trace.terminal_only);
    assert!(result.trace.external_leaf_eval_calls > 0);
    assert_eq!(result.trace.external_leaf_eval_calls, evaluator.calls);
    assert_eq!(result.trace.non_terminal_static_eval_calls, 0);
    Ok(())
}

#[test]
fn time_limit_does_not_claim_completed_requested_depth() -> Result<(), ark_core::FenError> {
    let position = Position::startpos()?;
    let request = SearchRequest {
        depth: 128,
        movetime_ms: Some(1),
        tactical_extension_depth: 0,
        seed: 1,
        ..SearchRequest::default()
    };
    let result = search(&position, &request);

    assert_eq!(result.trace.requested_depth, 128);
    assert_eq!(result.trace.movetime_ms, Some(1));
    assert_eq!(result.trace.stopped_by, StopReason::Time);
    assert!(result.depth_reached < request.depth);
    assert!(!result.trace.pv_complete);
    assert_eq!(result.trace.external_leaf_eval_calls, 0);
    assert_eq!(result.trace.non_terminal_static_eval_calls, 0);
    Ok(())
}

#[test]
fn search_reports_terminal_only_trace_counters() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen("7k/6Q1/5K2/8/8/8/8/8 w - - 0 1")?;
    let request = request(2);
    let result = search(&position, &request);
    assert!(result.trace.terminal_only);
    assert_eq!(result.trace.requested_depth, 2);
    assert!(result.trace.legal_moves_generated > 0);
    assert_eq!(result.trace.external_leaf_eval_calls, 0);
    assert_eq!(result.trace.non_terminal_static_eval_calls, 0);
    assert!(result.trace.terminal_leaf_evals > 0);
    Ok(())
}

#[test]
fn supplied_root_moves_match_regular_search_result() -> Result<(), ark_core::FenError> {
    let position = Position::startpos()?;
    let request = request(3);
    let regular = search(&position, &request);
    let mut root_moves = MoveList::with_capacity(96);
    position.legal_moves_into(&mut root_moves);
    let original_root_moves = root_moves.clone();

    let supplied = search_with_context_and_root_moves(&position, &request, &root_moves, None, None);

    assert_eq!(root_moves, original_root_moves);
    assert_eq!(supplied.best_move, regular.best_move);
    assert_eq!(supplied.score, regular.score);
    assert_eq!(supplied.pv, regular.pv);
    assert_eq!(supplied.depth_reached, regular.depth_reached);
    assert_eq!(regular.trace.root_movegen_calls, 1);
    assert_eq!(supplied.trace.root_movegen_calls, 0);
    assert!(supplied.trace.node_movegen_calls > 0);
    assert_eq!(
        supplied.trace.total_movegen_calls,
        supplied.trace.root_movegen_calls + supplied.trace.node_movegen_calls
    );
    Ok(())
}

#[test]
fn cancelled_search_reports_cancelled_stop_reason() -> Result<(), ark_core::FenError> {
    let position = Position::startpos()?;
    let request = request(8);
    let stopper = AlwaysStop;

    let result = search_with_context_and_stopper(&position, &request, None, None, &stopper);

    assert_eq!(result.trace.stopped_by, StopReason::Cancelled);
    assert_eq!(result.depth_reached, 0);
    assert!(!result.trace.pv_complete);
    assert_eq!(result.trace.root_movegen_calls, 1);
    assert_eq!(result.trace.node_movegen_calls, 0);
    assert_eq!(result.trace.total_movegen_calls, 1);
    Ok(())
}

#[test]
fn terminal_root_counts_only_root_move_generation() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen("7k/5Q2/7K/8/8/8/8/8 b - - 0 1")?;
    let result = search(&position, &request(4));

    assert_eq!(result.trace.stopped_by, StopReason::Terminal);
    assert_eq!(result.trace.root_moves, 0);
    assert_eq!(result.trace.root_movegen_calls, 1);
    assert_eq!(result.trace.node_movegen_calls, 0);
    assert_eq!(result.trace.total_movegen_calls, 1);
    Ok(())
}

#[test]
fn terminal_helpers_reuse_supplied_legal_moves() -> Result<(), ark_core::FenError> {
    let position = Position::from_fen("7k/6Q1/5K2/8/8/8/8/8 b - - 0 1")?;
    let mut legal = MoveList::with_capacity(96);
    position.legal_moves_into(&mut legal);
    let state = GameState::from_position(position.clone())?;

    assert!(legal.is_empty());
    assert_eq!(position.terminal_from_legal_moves(&legal), Some("1-0"));
    assert_eq!(
        state.outcome_from_legal_moves(&legal),
        Some(GameOutcome::WhiteWin)
    );
    Ok(())
}

#[test]
fn external_orderer_can_choose_root_move_without_static_eval() -> Result<(), ark_core::FenError> {
    let position = Position::startpos()?;
    let preferred = position
        .move_from_uci("e2e4")
        .ok_or(ark_core::FenError::BadBoard)?;
    let request = request(1);
    let mut orderer = PreferredMoveOrderer { preferred };

    let result = search_with_move_orderer(&position, &request, Some(&mut orderer));

    assert_eq!(result.best_move, Some(preferred));
    assert_eq!(result.score, 0);
    assert_eq!(result.trace.move_orderer_root_moves, 20);
    assert!(result.trace.move_orderer_moves >= 20);
    assert_eq!(result.trace.external_leaf_eval_calls, 0);
    assert_eq!(result.trace.non_terminal_static_eval_calls, 0);
    Ok(())
}

struct PreferredChildEvaluator {
    from: ark_core::Square,
    to: ark_core::Square,
    calls: u64,
}

impl SearchLeafEvaluator for PreferredChildEvaluator {
    fn evaluate_leaf(&mut self, position: &Position) -> i32 {
        self.calls += 1;
        let preferred_child = position.side_to_move() == Color::Black
            && position.piece_at(self.from).is_none()
            && position
                .piece_at(self.to)
                .is_some_and(|piece| piece.color == Color::White);
        if preferred_child {
            -500
        } else {
            0
        }
    }
}

struct PreferredMoveOrderer {
    preferred: Move,
}

impl SearchMoveOrderer for PreferredMoveOrderer {
    fn order_moves(&mut self, _position: &Position, moves: &mut [Move]) {
        moves.sort_by_key(|mv| if *mv == self.preferred { 0 } else { 1 });
    }
}

struct AlwaysStop;

impl SearchStopper for AlwaysStop {
    fn should_stop(&self) -> bool {
        true
    }
}
