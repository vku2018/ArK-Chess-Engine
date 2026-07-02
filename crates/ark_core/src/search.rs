use std::collections::HashMap;
use std::time::{Duration, Instant};

use core::fmt;

use crate::{perft::perft, Color, Move, MoveList, Position};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchRequest {
    pub depth: u32,
    pub nodes: Option<u64>,
    pub movetime_ms: Option<u64>,
    pub tactical_extension_depth: u32,
    pub seed: u64,
}

pub trait SearchMoveOrderer {
    fn order_moves(&mut self, position: &Position, moves: &mut [Move]);
}

pub trait SearchLeafEvaluator {
    fn evaluate_leaf(&mut self, position: &Position) -> i32;
}

impl Default for SearchRequest {
    fn default() -> Self {
        Self {
            depth: 1,
            nodes: None,
            movetime_ms: None,
            tactical_extension_depth: 2,
            seed: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopReason {
    Depth,
    Nodes,
    Time,
    Terminal,
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Depth => write!(f, "depth"),
            Self::Nodes => write!(f, "nodes"),
            Self::Time => write!(f, "time"),
            Self::Terminal => write!(f, "terminal"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchTrace {
    pub root_moves: usize,
    pub requested_depth: u32,
    pub node_limit: Option<u64>,
    pub movetime_ms: Option<u64>,
    pub terminal_only: bool,
    pub stopped_by: StopReason,
    pub pv_complete: bool,
    pub legal_moves_generated: u64,
    pub terminal_leaf_evals: u64,
    pub neutral_frontier_evals: u64,
    pub external_leaf_eval_calls: u64,
    pub non_terminal_static_eval_calls: u64,
    pub tactical_extension_depth: u32,
    pub tactical_extension_depth_reached: u32,
    pub tactical_extension_nodes: u64,
    pub tactical_extension_moves: u64,
    pub move_orderer_nodes: u64,
    pub move_orderer_moves: u64,
    pub move_orderer_root_moves: u64,
    pub transposition_table_probes: u64,
    pub transposition_table_hits: u64,
    pub cutoffs: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    pub best_move: Option<Move>,
    pub score: i32,
    pub pv: Vec<Move>,
    pub nodes: u64,
    pub depth_reached: u32,
    pub evals: u64,
    pub trace: SearchTrace,
}

#[derive(Clone, Debug)]
struct SearchState {
    deadline: Option<Instant>,
    node_limit: Option<u64>,
    table: HashMap<u64, TtEntry>,
    nodes: u64,
    evals: u64,
    legal_moves_generated: u64,
    terminal_leaf_evals: u64,
    neutral_frontier_evals: u64,
    external_leaf_eval_calls: u64,
    tactical_extension_depth: u32,
    tactical_extension_depth_reached: u32,
    tactical_extension_nodes: u64,
    tactical_extension_moves: u64,
    move_orderer_nodes: u64,
    move_orderer_moves: u64,
    move_orderer_root_moves: u64,
    transposition_table_probes: u64,
    transposition_table_hits: u64,
    cutoffs: u64,
    stopped_by: Option<StopReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TtEntry {
    depth: u32,
    score: i32,
    bound: Bound,
    best_move: Option<Move>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SearchWindow {
    alpha: i32,
    beta: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SearchFrame {
    depth: u32,
    extension_remaining: u32,
    window: SearchWindow,
}

struct SearchHooks<'m, 'l> {
    move_orderer: Option<&'m mut dyn SearchMoveOrderer>,
    leaf_evaluator: Option<&'l mut dyn SearchLeafEvaluator>,
}

impl SearchState {
    fn new(request: &SearchRequest) -> Self {
        let started = Instant::now();
        let deadline = request
            .movetime_ms
            .map(|ms| started + Duration::from_millis(ms.max(1)));
        Self {
            deadline,
            node_limit: request.nodes,
            table: HashMap::with_capacity(16_384),
            nodes: 0,
            evals: 0,
            legal_moves_generated: 0,
            terminal_leaf_evals: 0,
            neutral_frontier_evals: 0,
            external_leaf_eval_calls: 0,
            tactical_extension_depth: request.tactical_extension_depth,
            tactical_extension_depth_reached: 0,
            tactical_extension_nodes: 0,
            tactical_extension_moves: 0,
            move_orderer_nodes: 0,
            move_orderer_moves: 0,
            move_orderer_root_moves: 0,
            transposition_table_probes: 0,
            transposition_table_hits: 0,
            cutoffs: 0,
            stopped_by: None,
        }
    }

    fn enter_node(&mut self) -> bool {
        if self.stopped_by.is_some() {
            return false;
        }
        if self.node_limit.is_some_and(|limit| self.nodes >= limit) {
            self.stopped_by = Some(StopReason::Nodes);
            return false;
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.stopped_by = Some(StopReason::Time);
            return false;
        }
        self.nodes += 1;
        true
    }
}

pub fn search(position: &Position, request: &SearchRequest) -> SearchResult {
    search_with_context(position, request, None, None)
}

pub fn search_with_move_orderer(
    position: &Position,
    request: &SearchRequest,
    move_orderer: Option<&mut dyn SearchMoveOrderer>,
) -> SearchResult {
    search_with_context(position, request, move_orderer, None)
}

pub fn search_with_context(
    position: &Position,
    request: &SearchRequest,
    move_orderer: Option<&mut dyn SearchMoveOrderer>,
    leaf_evaluator: Option<&mut dyn SearchLeafEvaluator>,
) -> SearchResult {
    let mut work = position.clone();
    let mut root_moves = MoveList::with_capacity(96);
    work.legal_moves_in_place_into(&mut root_moves);
    if root_moves.is_empty() {
        return SearchResult {
            best_move: None,
            score: terminal_score(position, position.side_to_move()),
            pv: Vec::new(),
            nodes: 0,
            depth_reached: 0,
            evals: 1,
            trace: SearchTrace {
                root_moves: 0,
                requested_depth: request.depth,
                node_limit: request.nodes,
                movetime_ms: request.movetime_ms,
                terminal_only: true,
                stopped_by: StopReason::Terminal,
                pv_complete: true,
                legal_moves_generated: 0,
                terminal_leaf_evals: 1,
                neutral_frontier_evals: 0,
                external_leaf_eval_calls: 0,
                non_terminal_static_eval_calls: 0,
                tactical_extension_depth: request.tactical_extension_depth,
                tactical_extension_depth_reached: 0,
                tactical_extension_nodes: 0,
                tactical_extension_moves: 0,
                move_orderer_nodes: 0,
                move_orderer_moves: 0,
                move_orderer_root_moves: 0,
                transposition_table_probes: 0,
                transposition_table_hits: 0,
                cutoffs: 0,
            },
        };
    }
    let root_move_count = root_moves.len();
    order_root_moves(&mut root_moves, request.seed);
    let mut state = SearchState::new(request);
    let mut hooks = SearchHooks {
        move_orderer,
        leaf_evaluator,
    };
    apply_move_orderer(
        &work,
        &mut root_moves,
        &mut hooks.move_orderer,
        &mut state,
        true,
    );

    let max_depth = request.depth.max(1);
    let mut best_move = root_moves[0];
    let mut best_score = 0;
    let mut best_pv = vec![best_move];
    let mut depth_reached = 0;

    for depth in 1..=max_depth {
        let mut depth_best = best_move;
        let mut depth_score = i32::MIN + 1;
        let mut depth_pv = vec![depth_best];
        for mv in &root_moves {
            if !state.enter_node() {
                break;
            }
            let undo = work.make_move_in_place(*mv);
            let (score, line) = negamax(
                &mut work,
                SearchFrame {
                    depth: depth.saturating_sub(1),
                    extension_remaining: state.tactical_extension_depth,
                    window: SearchWindow {
                        alpha: i32::MIN + 1,
                        beta: i32::MAX - 1,
                    },
                },
                &mut state,
                &mut hooks,
            );
            work.unmake_move(undo);
            if state.stopped_by.is_some() {
                break;
            }
            let score = -score;
            if score > depth_score {
                depth_score = score;
                depth_best = *mv;
                depth_pv = vec![*mv];
                depth_pv.extend(line);
            }
            if state.stopped_by.is_some() {
                break;
            }
        }
        if state.stopped_by.is_some() {
            break;
        }
        best_move = depth_best;
        best_score = depth_score;
        best_pv = depth_pv;
        depth_reached = depth;
    }

    let stopped_by = state.stopped_by.unwrap_or(StopReason::Depth);
    SearchResult {
        best_move: Some(best_move),
        score: best_score,
        pv: best_pv,
        nodes: state.nodes,
        depth_reached,
        evals: state.evals,
        trace: SearchTrace {
            root_moves: root_move_count,
            requested_depth: request.depth,
            node_limit: request.nodes,
            movetime_ms: request.movetime_ms,
            terminal_only: state.external_leaf_eval_calls == 0,
            stopped_by,
            pv_complete: stopped_by == StopReason::Depth || stopped_by == StopReason::Terminal,
            legal_moves_generated: state.legal_moves_generated + root_move_count as u64,
            terminal_leaf_evals: state.terminal_leaf_evals,
            neutral_frontier_evals: state.neutral_frontier_evals,
            external_leaf_eval_calls: state.external_leaf_eval_calls,
            non_terminal_static_eval_calls: 0,
            tactical_extension_depth: state.tactical_extension_depth,
            tactical_extension_depth_reached: state.tactical_extension_depth_reached,
            tactical_extension_nodes: state.tactical_extension_nodes,
            tactical_extension_moves: state.tactical_extension_moves,
            move_orderer_nodes: state.move_orderer_nodes,
            move_orderer_moves: state.move_orderer_moves,
            move_orderer_root_moves: state.move_orderer_root_moves,
            transposition_table_probes: state.transposition_table_probes,
            transposition_table_hits: state.transposition_table_hits,
            cutoffs: state.cutoffs,
        },
    }
}

fn order_root_moves(moves: &mut [Move], seed: u64) {
    moves.sort_by_key(|mv| {
        (u64::from(mv.packed_id()) ^ seed)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15)
            .rotate_left(17)
    });
}

fn negamax(
    position: &mut Position,
    frame: SearchFrame,
    state: &mut SearchState,
    hooks: &mut SearchHooks<'_, '_>,
) -> (i32, Vec<Move>) {
    let depth = frame.depth;
    let extension_remaining = frame.extension_remaining;
    let mut alpha = frame.window.alpha;
    let beta = frame.window.beta;
    if let Some(_result) = position.is_terminal() {
        state.evals += 1;
        state.terminal_leaf_evals += 1;
        return (
            terminal_score(position, position.side_to_move()),
            Vec::new(),
        );
    }
    if depth == 0 && extension_remaining == 0 {
        return (
            evaluate_quiet_frontier(position, state, &mut hooks.leaf_evaluator),
            Vec::new(),
        );
    }

    let original_alpha = alpha;
    let hash = position.zobrist();
    let table_depth = depth + extension_remaining;
    state.transposition_table_probes += 1;
    if let Some(entry) = state.table.get(&hash).copied() {
        if entry.depth >= table_depth {
            match entry.bound {
                Bound::Exact => {
                    state.transposition_table_hits += 1;
                    return (entry.score, entry.best_move.into_iter().collect());
                }
                Bound::Lower if entry.score >= beta => {
                    state.transposition_table_hits += 1;
                    return (entry.score, entry.best_move.into_iter().collect());
                }
                Bound::Upper if entry.score <= alpha => {
                    state.transposition_table_hits += 1;
                    return (entry.score, entry.best_move.into_iter().collect());
                }
                _ => {}
            }
        }
    }

    let mut best = i32::MIN + 1;
    let mut best_move = None;
    let mut best_line = Vec::new();
    let mut moves = MoveList::with_capacity(96);
    position.legal_moves_in_place_into(&mut moves);
    state.legal_moves_generated += moves.len() as u64;
    if depth == 0 {
        let extend_all = position.in_check(position.side_to_move());
        moves = tactical_frontier_moves(position, &moves, extend_all);
        if moves.is_empty() {
            return (
                evaluate_quiet_frontier(position, state, &mut hooks.leaf_evaluator),
                Vec::new(),
            );
        }
        let used = state.tactical_extension_depth - extension_remaining + 1;
        state.tactical_extension_depth_reached = state.tactical_extension_depth_reached.max(used);
        state.tactical_extension_nodes += 1;
    }
    apply_move_orderer(position, &mut moves, &mut hooks.move_orderer, state, false);
    if let Some(entry) = state.table.get(&hash).copied() {
        if let Some(tt_move) = entry.best_move {
            moves.sort_by_key(|mv| if *mv == tt_move { 0 } else { 1 });
        }
    }
    for mv in moves {
        if !state.enter_node() {
            break;
        }
        let undo = position.make_move_in_place(mv);
        let (next_depth, next_extension_remaining) = if depth == 0 {
            state.tactical_extension_moves += 1;
            (0, extension_remaining - 1)
        } else {
            (depth - 1, extension_remaining)
        };
        let (score, line) = negamax(
            position,
            SearchFrame {
                depth: next_depth,
                extension_remaining: next_extension_remaining,
                window: SearchWindow {
                    alpha: -beta,
                    beta: -alpha,
                },
            },
            state,
            hooks,
        );
        position.unmake_move(undo);
        if state.stopped_by.is_some() {
            break;
        }
        let score = -score;
        if score > best {
            best = score;
            best_move = Some(mv);
            best_line = vec![mv];
            best_line.extend(line);
        }
        alpha = alpha.max(score);
        if alpha >= beta || state.stopped_by.is_some() {
            if alpha >= beta {
                state.cutoffs += 1;
            }
            break;
        }
    }
    let bound = if best <= original_alpha {
        Bound::Upper
    } else if best >= beta {
        Bound::Lower
    } else {
        Bound::Exact
    };
    if state.stopped_by.is_none() {
        state.table.insert(
            hash,
            TtEntry {
                depth: table_depth,
                score: best,
                bound,
                best_move,
            },
        );
    }
    (best, best_line)
}

fn evaluate_quiet_frontier(
    position: &Position,
    state: &mut SearchState,
    leaf_evaluator: &mut Option<&mut dyn SearchLeafEvaluator>,
) -> i32 {
    state.evals += 1;
    let Some(evaluator) = leaf_evaluator.as_deref_mut() else {
        state.neutral_frontier_evals += 1;
        return 0;
    };
    state.external_leaf_eval_calls += 1;
    evaluator.evaluate_leaf(position)
}

fn apply_move_orderer(
    position: &Position,
    moves: &mut [Move],
    move_orderer: &mut Option<&mut dyn SearchMoveOrderer>,
    state: &mut SearchState,
    is_root: bool,
) {
    let Some(orderer) = move_orderer.as_deref_mut() else {
        return;
    };
    if moves.is_empty() {
        return;
    }
    orderer.order_moves(position, moves);
    state.move_orderer_nodes += 1;
    state.move_orderer_moves += moves.len() as u64;
    if is_root {
        state.move_orderer_root_moves += moves.len() as u64;
    }
}

fn tactical_frontier_moves(position: &mut Position, moves: &[Move], extend_all: bool) -> MoveList {
    let mut tactical = MoveList::with_capacity(moves.len());
    for mv in moves.iter().copied() {
        if extend_all || is_tactical_frontier_move(position, mv) {
            tactical.push(mv);
        }
    }
    tactical
}

fn is_tactical_frontier_move(position: &mut Position, mv: Move) -> bool {
    if mv.is_capture() || mv.promotion().is_some() {
        return true;
    }
    let undo = position.make_move_in_place(mv);
    let gives_check = position.in_check(position.side_to_move());
    position.unmake_move(undo);
    gives_check
}

fn terminal_score(position: &Position, root_side: Color) -> i32 {
    match position.is_terminal() {
        Some("1-0") => {
            if root_side == Color::White {
                100_000
            } else {
                -100_000
            }
        }
        Some("0-1") => {
            if root_side == Color::Black {
                100_000
            } else {
                -100_000
            }
        }
        Some("1/2-1/2") => 0,
        _ => 0,
    }
}

#[must_use]
pub fn legal_baseline_nodes(position: &Position, depth: u32) -> u64 {
    perft(position, depth)
}
