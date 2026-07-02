use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread;

use ark_core::{
    search_with_context_and_root_moves, GameOutcome, GameState, Move, MoveList, Position,
    SearchLeafEvaluator, SearchMoveOrderer, SearchRequest,
};
use ark_model::{ForgeModel, GameRecord, PolicyMoveWorkspace, WdlEvaluationWorkspace};
use ark_replay::{rebuild_manifest, write_chunk_atomic, ChunkWriteOptions};

pub type SelfPlayResult<T> = Result<T, String>;

pub type StreamingSelfPlayResult<T> = Result<T, SelfPlayError>;

#[derive(Clone, Debug, PartialEq)]
pub struct StreamingSelfPlayConfig {
    pub games: u32,
    pub search_depth: u32,
    pub tactical_extension_depth: u32,
    pub max_plies: u32,
    pub actors: u32,
    pub run_seed: u64,
    pub result_channel_bound: usize,
    pub leaf_eval: LeafEvalMode,
    pub model: Option<ForgeModel>,
}

impl Default for StreamingSelfPlayConfig {
    fn default() -> Self {
        Self {
            games: 16,
            search_depth: 1,
            tactical_extension_depth: SearchRequest::default().tactical_extension_depth,
            max_plies: 256,
            actors: 1,
            run_seed: 1,
            result_channel_bound: 16,
            leaf_eval: LeafEvalMode::Terminal,
            model: None,
        }
    }
}

pub trait GameSink {
    fn write_game(&mut self, game: &CompletedGame) -> Result<(), String>;
}

impl<F> GameSink for F
where
    F: FnMut(&CompletedGame) -> Result<(), String>,
{
    fn write_game(&mut self, game: &CompletedGame) -> Result<(), String> {
        self(game)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActorCounter {
    pub actor_id: u32,
    pub games: u32,
    pub plies: u64,
    pub search_nodes: u64,
    pub root_movegen_calls: u64,
    pub node_movegen_calls: u64,
    pub total_movegen_calls: u64,
    pub neutral_frontier_evals: u64,
    pub wdl_leaf_evals: u64,
    pub send_failures: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActorSkew {
    pub min_games: u32,
    pub max_games: u32,
    pub game_skew: u32,
    pub min_plies: u64,
    pub max_plies: u64,
    pub ply_skew: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamingSelfPlaySummary {
    pub games_requested: u32,
    pub games_completed: u32,
    pub plies: u64,
    pub search_nodes: u64,
    pub root_movegen_calls: u64,
    pub node_movegen_calls: u64,
    pub total_movegen_calls: u64,
    pub neutral_frontier_evals: u64,
    pub wdl_leaf_evals: u64,
    pub illegal_moves: u32,
    pub unhandled_terminal_states: u32,
    pub actor_count: u32,
    pub actor_crashes: u32,
    pub result_channel_bound: usize,
    pub max_pending_games: usize,
    pub actor_counters: Vec<ActorCounter>,
    pub actor_skew: ActorSkew,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelfPlayError {
    InvalidConfig(&'static str),
    StartPosition(String),
    SearchReturnedNoMove {
        game_id: u32,
        ply: u32,
    },
    IllegalMoveSelected {
        game_id: u32,
        ply: u32,
        move_text: String,
    },
    UnhandledTerminalState {
        game_id: u32,
        ply: u32,
    },
    DuplicateGame {
        game_id: u32,
    },
    MissingGames {
        expected: u32,
        received: u32,
    },
    Sink(String),
    ResultChannelClosed {
        actor_id: u32,
    },
    ActorPanicked {
        actor_crashes: u32,
    },
}

impl fmt::Display for SelfPlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(f, "invalid self-play config: {message}"),
            Self::StartPosition(message) => write!(f, "start position failed: {message}"),
            Self::SearchReturnedNoMove { game_id, ply } => {
                write!(f, "search returned no move for game_id={game_id} ply={ply}")
            }
            Self::IllegalMoveSelected {
                game_id,
                ply,
                move_text,
            } => write!(
                f,
                "search selected illegal move {move_text} for game_id={game_id} ply={ply}"
            ),
            Self::UnhandledTerminalState { game_id, ply } => write!(
                f,
                "position had no legal moves without terminal outcome for game_id={game_id} ply={ply}"
            ),
            Self::DuplicateGame { game_id } => {
                write!(f, "self-play produced duplicate game_id={game_id}")
            }
            Self::MissingGames { expected, received } => write!(
                f,
                "self-play produced {received} games but expected {expected}"
            ),
            Self::Sink(message) => write!(f, "self-play sink failed: {message}"),
            Self::ResultChannelClosed { actor_id } => {
                write!(f, "result channel closed for actor_id={actor_id}")
            }
            Self::ActorPanicked { actor_crashes } => {
                write!(f, "self-play actor panics: {actor_crashes}")
            }
        }
    }
}

impl std::error::Error for SelfPlayError {}

#[derive(Clone, Debug, PartialEq)]
pub struct SelfPlayConfig {
    pub games: u32,
    pub search_depth: u32,
    pub tactical_extension_depth: u32,
    pub max_plies: u32,
    pub actors: u32,
    pub seed: u64,
    pub chunk_size: u32,
    pub out_dir: PathBuf,
    pub model: Option<ForgeModel>,
    pub leaf_eval: LeafEvalMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LeafEvalMode {
    #[default]
    Terminal,
    Wdl,
}

struct ModelMoveOrderer<'a> {
    model: &'a ForgeModel,
    workspace: PolicyMoveWorkspace,
}

impl SearchMoveOrderer for ModelMoveOrderer<'_> {
    fn order_moves(&mut self, position: &Position, moves: &mut [Move]) {
        self.model
            .order_legal_moves_by_policy(position, moves, &mut self.workspace);
    }
}

struct WdlLeafEvaluator<'a> {
    model: &'a ForgeModel,
    workspace: WdlEvaluationWorkspace,
}

impl SearchLeafEvaluator for WdlLeafEvaluator<'_> {
    fn evaluate_leaf(&mut self, position: &Position) -> i32 {
        self.model
            .evaluate_wdl_leaf_into(position, &mut self.workspace)
            .side_to_move_centipawns
    }
}

impl SelfPlayConfig {
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.actors = self.actors.max(1);
        self.chunk_size = self.chunk_size.max(1);
        self.max_plies = self.max_plies.max(1);
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelfPlaySummary {
    pub games_requested: u32,
    pub games_completed: u32,
    pub plies: u64,
    pub search_nodes: u64,
    pub root_movegen_calls: u64,
    pub node_movegen_calls: u64,
    pub total_movegen_calls: u64,
    pub neutral_frontier_evals: u64,
    pub wdl_leaf_evals: u64,
    pub actor_count: u32,
    pub actor_crashes: u32,
    pub p95_actor_skew: f64,
    pub chunks_published: u32,
    pub illegal_moves: u32,
    pub unhandled_terminal_states: u32,
    pub out_dir: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedGame {
    pub game_id: u32,
    pub record: GameRecord,
    pub search_nodes: u64,
    pub root_movegen_calls: u64,
    pub node_movegen_calls: u64,
    pub total_movegen_calls: u64,
    pub neutral_frontier_evals: u64,
    pub wdl_leaf_evals: u64,
    pub actor_id: u32,
    pub plies: u32,
    pub max_plies_reached: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ActorSummary {
    games: u32,
}

#[must_use]
pub const fn actor_for_game_id(game_id: u32, actors: u32) -> u32 {
    if actors == 0 {
        0
    } else {
        game_id % actors
    }
}

#[must_use]
pub fn seed_for_ply(run_seed: u64, game_id: u32, ply: u32) -> u64 {
    run_seed
        .wrapping_add(u64::from(game_id).wrapping_mul(0x9e37_79b9_7f4a_7c15))
        .wrapping_add(u64::from(ply).wrapping_mul(0xbf58_476d_1ce4_e5b9))
}

#[derive(Clone, Debug)]
struct WriterSummary {
    games_completed: u32,
    plies: u64,
    search_nodes: u64,
    root_movegen_calls: u64,
    node_movegen_calls: u64,
    total_movegen_calls: u64,
    neutral_frontier_evals: u64,
    wdl_leaf_evals: u64,
    chunks_published: u32,
}

#[derive(Clone, Copy)]
struct GamePlaySpec<'a> {
    game_id: u32,
    actor_id: u32,
    search_depth: u32,
    tactical_extension_depth: u32,
    max_plies: u32,
    run_seed: u64,
    model: Option<&'a ForgeModel>,
    leaf_eval: LeafEvalMode,
}

pub fn run_selfplay(config: SelfPlayConfig) -> SelfPlayResult<SelfPlaySummary> {
    let config = config.normalized();
    std::fs::create_dir_all(&config.out_dir)
        .map_err(|err| format!("failed to create self-play output dir: {err}"))?;
    let chunks_dir = config.out_dir.join("chunks");
    std::fs::create_dir_all(&chunks_dir)
        .map_err(|err| format!("failed to create self-play chunk dir: {err}"))?;

    let queue_bound = usize::try_from(config.chunk_size.saturating_mul(4).max(config.actors * 2))
        .map_err(|_err| "self-play queue bound does not fit usize".to_string())?;
    let (sender, receiver) = sync_channel(queue_bound);
    let writer_games = config.games;
    let writer_chunk_size = config.chunk_size;
    let writer = thread::spawn(move || {
        writer_loop(
            receiver,
            writer_games,
            writer_chunk_size,
            chunks_dir.as_path(),
        )
    });

    let mut handles = Vec::new();
    for actor_id in 0..config.actors {
        let actor_config = config.clone();
        let actor_sender = sender.clone();
        handles.push(thread::spawn(move || {
            let mut summary = ActorSummary::default();
            let mut game_index = actor_id;
            while game_index < actor_config.games {
                let game = play_completed_game(GamePlaySpec {
                    game_id: game_index,
                    actor_id,
                    search_depth: actor_config.search_depth,
                    tactical_extension_depth: actor_config.tactical_extension_depth,
                    max_plies: actor_config.max_plies,
                    run_seed: actor_config.seed,
                    model: actor_config.model.as_ref(),
                    leaf_eval: actor_config.leaf_eval,
                })
                .map_err(|err| err.to_string())?;
                actor_sender
                    .send(game)
                    .map_err(|err| format!("self-play writer channel closed: {err}"))?;
                summary.games += 1;
                game_index = game_index.saturating_add(actor_config.actors);
            }
            Ok::<ActorSummary, String>(summary)
        }));
    }
    drop(sender);

    let mut actor_crashes = 0_u32;
    let mut actor_summaries = Vec::new();
    for handle in handles {
        match handle.join() {
            Ok(Ok(summary)) => actor_summaries.push(summary),
            Ok(Err(err)) => return Err(err),
            Err(_panic) => actor_crashes += 1,
        }
    }

    let writer_summary = match writer.join() {
        Ok(Ok(summary)) => summary,
        Ok(Err(err)) => return Err(err),
        Err(_panic) => return Err("self-play writer thread panicked".to_string()),
    };

    let replay_manifest = rebuild_manifest(&config.out_dir.join("chunks"))?;
    Ok(SelfPlaySummary {
        games_requested: config.games,
        games_completed: writer_summary.games_completed,
        plies: writer_summary.plies,
        search_nodes: writer_summary.search_nodes,
        root_movegen_calls: writer_summary.root_movegen_calls,
        node_movegen_calls: writer_summary.node_movegen_calls,
        total_movegen_calls: writer_summary.total_movegen_calls,
        neutral_frontier_evals: writer_summary.neutral_frontier_evals,
        wdl_leaf_evals: writer_summary.wdl_leaf_evals,
        actor_count: config.actors,
        actor_crashes,
        p95_actor_skew: actor_skew(&actor_summaries),
        chunks_published: writer_summary.chunks_published,
        illegal_moves: replay_manifest
            .chunks
            .iter()
            .map(|chunk| chunk.validation_illegal_moves)
            .sum(),
        unhandled_terminal_states: replay_manifest
            .chunks
            .iter()
            .map(|chunk| chunk.validation_unhandled_terminal_states)
            .sum(),
        out_dir: config.out_dir,
    })
}

pub fn run_selfplay_streaming<S>(
    config: &StreamingSelfPlayConfig,
    sink: &mut S,
) -> StreamingSelfPlayResult<StreamingSelfPlaySummary>
where
    S: GameSink + ?Sized,
{
    validate_streaming_config(config)?;
    let (sender, receiver) = sync_channel(config.result_channel_bound);
    let mut handles = Vec::new();
    for actor_id in 0..config.actors {
        let actor_config = config.clone();
        let actor_sender = sender.clone();
        handles.push(thread::spawn(move || {
            run_streaming_actor(actor_id, actor_config, actor_sender)
        }));
    }
    drop(sender);

    let mut summary = empty_streaming_summary(config);
    let mut pending = BTreeMap::new();
    let mut next_game_id = 0_u32;
    let mut games_received = 0_u32;
    let mut first_error = None;

    while let Ok(game) = receiver.recv() {
        games_received = games_received.saturating_add(1);
        if first_error.is_some() {
            continue;
        }
        let game_id = game.game_id;
        if pending.insert(game_id, game).is_some() {
            first_error = Some(SelfPlayError::DuplicateGame { game_id });
            continue;
        }
        summary.max_pending_games = summary.max_pending_games.max(pending.len());
        flush_streaming_games(
            sink,
            &mut pending,
            &mut next_game_id,
            &mut summary,
            &mut first_error,
        );
    }

    let mut actor_counters = Vec::new();
    let mut actor_crashes = 0_u32;
    let mut actor_error = None;
    for handle in handles {
        match handle.join() {
            Ok(Ok(counter)) => actor_counters.push(counter),
            Ok(Err(err)) => {
                if actor_error.is_none() {
                    actor_error = Some(err);
                }
            }
            Err(_panic) => actor_crashes = actor_crashes.saturating_add(1),
        }
    }

    if let Some(err) = first_error {
        return Err(err);
    }
    if actor_crashes > 0 {
        return Err(SelfPlayError::ActorPanicked { actor_crashes });
    }
    if let Some(err) = actor_error {
        return Err(err);
    }
    if games_received != config.games {
        return Err(SelfPlayError::MissingGames {
            expected: config.games,
            received: games_received,
        });
    }
    if !pending.is_empty() {
        return Err(SelfPlayError::MissingGames {
            expected: config.games,
            received: summary.games_completed,
        });
    }

    actor_counters.sort_by_key(|counter| counter.actor_id);
    summary.actor_counters = actor_counters;
    summary.actor_skew = ActorSkew::from_counters(&summary.actor_counters);
    summary.actor_crashes = actor_crashes;
    Ok(summary)
}

fn validate_streaming_config(config: &StreamingSelfPlayConfig) -> StreamingSelfPlayResult<()> {
    if config.actors == 0 {
        return Err(SelfPlayError::InvalidConfig(
            "actors must be greater than zero",
        ));
    }
    if config.search_depth == 0 {
        return Err(SelfPlayError::InvalidConfig(
            "search_depth must be greater than zero",
        ));
    }
    if config.max_plies == 0 {
        return Err(SelfPlayError::InvalidConfig(
            "max_plies must be greater than zero",
        ));
    }
    if config.result_channel_bound == 0 {
        return Err(SelfPlayError::InvalidConfig(
            "result_channel_bound must be greater than zero",
        ));
    }
    if config.leaf_eval == LeafEvalMode::Wdl && config.model.is_none() {
        return Err(SelfPlayError::InvalidConfig(
            "model is required for WDL leaf evaluation",
        ));
    }
    Ok(())
}

fn empty_streaming_summary(config: &StreamingSelfPlayConfig) -> StreamingSelfPlaySummary {
    StreamingSelfPlaySummary {
        games_requested: config.games,
        games_completed: 0,
        plies: 0,
        search_nodes: 0,
        root_movegen_calls: 0,
        node_movegen_calls: 0,
        total_movegen_calls: 0,
        neutral_frontier_evals: 0,
        wdl_leaf_evals: 0,
        illegal_moves: 0,
        unhandled_terminal_states: 0,
        actor_count: config.actors,
        actor_crashes: 0,
        result_channel_bound: config.result_channel_bound,
        max_pending_games: 0,
        actor_counters: Vec::new(),
        actor_skew: ActorSkew::default(),
    }
}

fn flush_streaming_games<S>(
    sink: &mut S,
    pending: &mut BTreeMap<u32, CompletedGame>,
    next_game_id: &mut u32,
    summary: &mut StreamingSelfPlaySummary,
    first_error: &mut Option<SelfPlayError>,
) where
    S: GameSink + ?Sized,
{
    while let Some(game) = pending.remove(next_game_id) {
        if let Err(err) = sink.write_game(&game) {
            *first_error = Some(SelfPlayError::Sink(err));
            return;
        }
        summary.games_completed = summary.games_completed.saturating_add(1);
        summary.plies = summary.plies.saturating_add(u64::from(game.plies));
        summary.search_nodes = summary.search_nodes.saturating_add(game.search_nodes);
        summary.root_movegen_calls = summary
            .root_movegen_calls
            .saturating_add(game.root_movegen_calls);
        summary.node_movegen_calls = summary
            .node_movegen_calls
            .saturating_add(game.node_movegen_calls);
        summary.total_movegen_calls = summary
            .total_movegen_calls
            .saturating_add(game.total_movegen_calls);
        summary.neutral_frontier_evals = summary
            .neutral_frontier_evals
            .saturating_add(game.neutral_frontier_evals);
        summary.wdl_leaf_evals = summary.wdl_leaf_evals.saturating_add(game.wdl_leaf_evals);
        *next_game_id = next_game_id.saturating_add(1);
    }
}

fn run_streaming_actor(
    actor_id: u32,
    config: StreamingSelfPlayConfig,
    sender: SyncSender<CompletedGame>,
) -> StreamingSelfPlayResult<ActorCounter> {
    let mut counter = ActorCounter {
        actor_id,
        ..ActorCounter::default()
    };
    let mut game_id = actor_id;
    while game_id < config.games {
        if actor_for_game_id(game_id, config.actors) == actor_id {
            let game = play_completed_game(GamePlaySpec {
                game_id,
                actor_id,
                search_depth: config.search_depth,
                tactical_extension_depth: config.tactical_extension_depth,
                max_plies: config.max_plies,
                run_seed: config.run_seed,
                model: config.model.as_ref(),
                leaf_eval: config.leaf_eval,
            })?;
            counter.games = counter.games.saturating_add(1);
            counter.plies = counter.plies.saturating_add(u64::from(game.plies));
            counter.search_nodes = counter.search_nodes.saturating_add(game.search_nodes);
            counter.root_movegen_calls = counter
                .root_movegen_calls
                .saturating_add(game.root_movegen_calls);
            counter.node_movegen_calls = counter
                .node_movegen_calls
                .saturating_add(game.node_movegen_calls);
            counter.total_movegen_calls = counter
                .total_movegen_calls
                .saturating_add(game.total_movegen_calls);
            counter.neutral_frontier_evals = counter
                .neutral_frontier_evals
                .saturating_add(game.neutral_frontier_evals);
            counter.wdl_leaf_evals = counter.wdl_leaf_evals.saturating_add(game.wdl_leaf_evals);
            if sender.send(game).is_err() {
                counter.send_failures = counter.send_failures.saturating_add(1);
                return Err(SelfPlayError::ResultChannelClosed { actor_id });
            }
        }
        game_id = game_id.saturating_add(config.actors);
    }
    Ok(counter)
}

fn writer_loop(
    receiver: Receiver<CompletedGame>,
    games_requested: u32,
    chunk_size: u32,
    chunks_dir: &Path,
) -> SelfPlayResult<WriterSummary> {
    let mut pending = BTreeMap::new();
    let mut next_game = 0_u32;
    let mut chunk_games = Vec::with_capacity(chunk_size as usize);
    let mut chunk_first_game = 0_u64;
    let mut summary = WriterSummary {
        games_completed: 0,
        plies: 0,
        search_nodes: 0,
        root_movegen_calls: 0,
        node_movegen_calls: 0,
        total_movegen_calls: 0,
        neutral_frontier_evals: 0,
        wdl_leaf_evals: 0,
        chunks_published: 0,
    };

    while let Ok(completed) = receiver.recv() {
        pending.insert(completed.game_id, completed);
        while let Some(completed) = pending.remove(&next_game) {
            if chunk_games.is_empty() {
                chunk_first_game = u64::from(next_game);
            }
            summary.plies += completed.record.moves.len() as u64;
            summary.search_nodes += completed.search_nodes;
            summary.root_movegen_calls += completed.root_movegen_calls;
            summary.node_movegen_calls += completed.node_movegen_calls;
            summary.total_movegen_calls += completed.total_movegen_calls;
            summary.neutral_frontier_evals += completed.neutral_frontier_evals;
            summary.wdl_leaf_evals += completed.wdl_leaf_evals;
            summary.games_completed += 1;
            chunk_games.push(completed.record);
            next_game = next_game.saturating_add(1);
            if chunk_games.len() >= chunk_size as usize {
                write_chunk_atomic(
                    chunks_dir,
                    ChunkWriteOptions {
                        first_game_index: chunk_first_game,
                    },
                    &chunk_games,
                )?;
                summary.chunks_published += 1;
                chunk_games.clear();
            }
        }
    }

    if !pending.is_empty() {
        return Err("self-play writer stopped with missing game indexes".to_string());
    }
    if summary.games_completed != games_requested {
        return Err(format!(
            "self-play completed {} games but expected {games_requested}",
            summary.games_completed
        ));
    }
    if !chunk_games.is_empty() {
        write_chunk_atomic(
            chunks_dir,
            ChunkWriteOptions {
                first_game_index: chunk_first_game,
            },
            &chunk_games,
        )?;
        summary.chunks_published += 1;
    }
    Ok(summary)
}

pub fn play_one_game(
    game_index: u32,
    config: &SelfPlayConfig,
) -> SelfPlayResult<(GameRecord, u64)> {
    let completed = play_completed_game(GamePlaySpec {
        game_id: game_index,
        actor_id: actor_for_game_id(game_index, config.actors),
        search_depth: config.search_depth,
        tactical_extension_depth: config.tactical_extension_depth,
        max_plies: config.max_plies,
        run_seed: config.seed,
        model: config.model.as_ref(),
        leaf_eval: config.leaf_eval,
    })
    .map_err(|err| err.to_string())?;
    Ok((completed.record, completed.search_nodes))
}

fn play_completed_game(spec: GamePlaySpec<'_>) -> StreamingSelfPlayResult<CompletedGame> {
    let mut state =
        GameState::startpos().map_err(|err| SelfPlayError::StartPosition(format!("{err:?}")))?;
    let mut moves = Vec::with_capacity(spec.max_plies as usize);
    let mut result = GameOutcome::Draw;
    let mut search_nodes = 0_u64;
    let mut root_movegen_calls = 0_u64;
    let mut node_movegen_calls = 0_u64;
    let mut total_movegen_calls = 0_u64;
    let mut neutral_frontier_evals = 0_u64;
    let mut wdl_leaf_evals = 0_u64;
    let mut max_plies_reached = true;
    let mut legal_moves = MoveList::with_capacity(96);
    let mut orderer = spec.model.map(|model| ModelMoveOrderer {
        model,
        workspace: PolicyMoveWorkspace::default(),
    });
    let mut leaf_evaluator = match spec.leaf_eval {
        LeafEvalMode::Terminal => None,
        LeafEvalMode::Wdl => {
            let model = spec.model.ok_or(SelfPlayError::InvalidConfig(
                "model is required for WDL leaf evaluation",
            ))?;
            Some(WdlLeafEvaluator {
                model,
                workspace: WdlEvaluationWorkspace::default(),
            })
        }
    };
    for ply in 0..spec.max_plies {
        legal_moves.clear();
        state.position().legal_moves_into(&mut legal_moves);
        root_movegen_calls = root_movegen_calls.saturating_add(1);
        total_movegen_calls = total_movegen_calls.saturating_add(1);
        if let Some(outcome) = state.outcome_from_legal_moves(&legal_moves) {
            result = outcome;
            max_plies_reached = false;
            break;
        }
        if legal_moves.is_empty() {
            return Err(SelfPlayError::UnhandledTerminalState {
                game_id: spec.game_id,
                ply,
            });
        }
        let request = SearchRequest {
            depth: spec.search_depth,
            tactical_extension_depth: spec.tactical_extension_depth,
            seed: seed_for_ply(spec.run_seed, spec.game_id, ply),
            ..SearchRequest::default()
        };
        let move_orderer = orderer
            .as_mut()
            .map(|orderer| orderer as &mut dyn SearchMoveOrderer);
        let leaf_evaluator = leaf_evaluator
            .as_mut()
            .map(|evaluator| evaluator as &mut dyn SearchLeafEvaluator);
        let search_result = search_with_context_and_root_moves(
            state.position(),
            &request,
            &legal_moves,
            move_orderer,
            leaf_evaluator,
        );
        search_nodes = search_nodes.saturating_add(search_result.nodes);
        root_movegen_calls =
            root_movegen_calls.saturating_add(search_result.trace.root_movegen_calls);
        node_movegen_calls =
            node_movegen_calls.saturating_add(search_result.trace.node_movegen_calls);
        total_movegen_calls =
            total_movegen_calls.saturating_add(search_result.trace.total_movegen_calls);
        neutral_frontier_evals =
            neutral_frontier_evals.saturating_add(search_result.trace.neutral_frontier_evals);
        wdl_leaf_evals =
            wdl_leaf_evals.saturating_add(search_result.trace.external_leaf_eval_calls);
        let best = search_result
            .best_move
            .ok_or(SelfPlayError::SearchReturnedNoMove {
                game_id: spec.game_id,
                ply,
            })?;
        if !legal_moves.contains(&best) {
            return Err(SelfPlayError::IllegalMoveSelected {
                game_id: spec.game_id,
                ply,
                move_text: best.to_string(),
            });
        }
        state.make_move(best);
        moves.push(best.packed_id());
    }
    if max_plies_reached {
        legal_moves.clear();
        state.position().legal_moves_into(&mut legal_moves);
        root_movegen_calls = root_movegen_calls.saturating_add(1);
        total_movegen_calls = total_movegen_calls.saturating_add(1);
        if let Some(outcome) = state.outcome_from_legal_moves(&legal_moves) {
            result = outcome;
            max_plies_reached = false;
        }
    }
    let plies = moves.len() as u32;
    Ok(CompletedGame {
        game_id: spec.game_id,
        record: GameRecord { result, moves },
        search_nodes,
        root_movegen_calls,
        node_movegen_calls,
        total_movegen_calls,
        neutral_frontier_evals,
        wdl_leaf_evals,
        actor_id: spec.actor_id,
        plies,
        max_plies_reached,
    })
}

fn actor_skew(summaries: &[ActorSummary]) -> f64 {
    if summaries.is_empty() {
        return 0.0;
    }
    let total: u32 = summaries.iter().map(|summary| summary.games).sum();
    if total == 0 {
        return 0.0;
    }
    let mut counts: Vec<u32> = summaries.iter().map(|summary| summary.games).collect();
    counts.sort_unstable();
    let p95_index = ((counts.len() * 95).div_ceil(100)).saturating_sub(1);
    let p95 = f64::from(counts[p95_index.min(counts.len() - 1)]);
    let mean = f64::from(total) / summaries.len() as f64;
    if mean == 0.0 {
        0.0
    } else {
        p95 / mean
    }
}

impl ActorSkew {
    #[must_use]
    pub fn from_counters(counters: &[ActorCounter]) -> Self {
        let Some(first) = counters.first().copied() else {
            return Self::default();
        };
        let mut min_games = first.games;
        let mut max_games = first.games;
        let mut min_plies = first.plies;
        let mut max_plies = first.plies;
        for counter in counters {
            min_games = min_games.min(counter.games);
            max_games = max_games.max(counter.games);
            min_plies = min_plies.min(counter.plies);
            max_plies = max_plies.max(counter.plies);
        }
        Self {
            min_games,
            max_games,
            game_skew: max_games.saturating_sub(min_games),
            min_plies,
            max_plies,
            ply_skew: max_plies.saturating_sub(min_plies),
        }
    }
}

#[must_use]
pub fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
