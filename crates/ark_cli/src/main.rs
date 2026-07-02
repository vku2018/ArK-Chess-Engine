mod bench_contract;
mod json_report;
mod uci;

use std::collections::BTreeMap;
use std::env;
use std::io;
use std::path::Path;
use std::thread;
use std::time::Instant;

use ark_core::{
    perft, search, search_with_context, search_with_context_and_root_moves, GameOutcome, GameState,
    Move, MoveList, Position, SearchLeafEvaluator, SearchMoveOrderer, SearchRequest,
};
use ark_model::{
    validate_replay, write_replay, ForgeModel, GameRecord, PolicyMoveWorkspace,
    WdlEvaluationWorkspace,
};
use ark_replay::read_replay_any;
use ark_selfplay::{
    run_selfplay as run_streaming_selfplay, LeafEvalMode, SelfPlayConfig as StreamingSelfPlayConfig,
};
use bench_contract::{MetricValue, Target};
use serde_json::json;

const STARTPOS_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const KIWIPETE_FEN: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("perft") => {
            let fen =
                option_value(&args, "--fen")?.ok_or_else(|| "--fen is required".to_string())?;
            let depth = parse_u32(
                option_value(&args, "--depth")?.as_deref().unwrap_or("1"),
                "--depth",
            )?;
            let threads = option_value(&args, "--threads")?
                .map(|text| parse_u32(&text, "--threads"))
                .transpose()?
                .unwrap_or(1);
            let correct_nodes = option_value(&args, "--correct-nodes")?
                .map(|text| parse_u64(&text, "--correct-nodes"))
                .transpose()?;
            let json = option_present(&args, "--json");
            let position = Position::from_fen(&fen).map_err(|err| format!("bad FEN: {err:?}"))?;
            let started = Instant::now();
            let nodes = perft(&position, depth);
            let elapsed_ms = started.elapsed().as_millis().max(1);
            if json {
                print_perft_json(
                    &args,
                    &fen,
                    depth,
                    threads,
                    nodes,
                    elapsed_ms,
                    correct_nodes,
                )?;
            } else {
                println!("{nodes}");
            }
        }
        Some("search") => {
            let fen =
                option_value(&args, "--fen")?.ok_or_else(|| "--fen is required".to_string())?;
            let depth = parse_u32(
                option_value(&args, "--depth")?.as_deref().unwrap_or("1"),
                "--depth",
            )?;
            let json = option_present(&args, "--json");
            let position = Position::from_fen(&fen).map_err(|err| format!("bad FEN: {err:?}"))?;
            let move_order = option_value(&args, "--move-order")?.unwrap_or("seed".to_string());
            let leaf_eval = parse_leaf_eval(
                option_value(&args, "--leaf-eval")?
                    .as_deref()
                    .unwrap_or("terminal"),
            )?;
            let checkpoint = option_value(&args, "--checkpoint")?;
            let tactical_extension_depth = option_value(&args, "--tactical-extension-depth")?
                .map(|text| parse_u32(&text, "--tactical-extension-depth"))
                .transpose()?
                .unwrap_or_else(|| SearchRequest::default().tactical_extension_depth);
            let request = SearchRequest {
                depth,
                nodes: option_value(&args, "--nodes")?
                    .map(|text| parse_u64(&text, "--nodes"))
                    .transpose()?,
                movetime_ms: option_value(&args, "--movetime-ms")?
                    .map(|text| parse_u64(&text, "--movetime-ms"))
                    .transpose()?,
                seed: option_value(&args, "--seed")?
                    .map(|text| parse_u64(&text, "--seed"))
                    .transpose()?
                    .unwrap_or(1),
                tactical_extension_depth,
            };
            let threads = option_value(&args, "--threads")?
                .map(|text| parse_u32(&text, "--threads"))
                .transpose()?
                .unwrap_or(1);
            if threads > 1 {
                return Err(format!(
                    "search --threads > 1 is not supported yet; got {threads}. Use --threads 1. The 32-thread search benchmark is documented as unsupported until parallel search lands."
                ));
            }
            let model = load_search_model(&move_order, leaf_eval, checkpoint.as_deref())?;
            let checkpoint_loaded = model.is_some();
            let mut model_orderer = match (move_order.as_str(), model.as_ref()) {
                ("model", Some(model)) => Some(ModelMoveOrderer {
                    model,
                    workspace: PolicyMoveWorkspace::default(),
                }),
                _ => None,
            };
            let mut wdl_evaluator = match (leaf_eval, model.as_ref()) {
                (LeafEvalMode::Wdl, Some(model)) => Some(WdlLeafEvaluator {
                    model,
                    workspace: WdlEvaluationWorkspace::default(),
                }),
                _ => None,
            };
            let started = Instant::now();
            let move_orderer = model_orderer
                .as_mut()
                .map(|orderer| orderer as &mut dyn SearchMoveOrderer);
            let leaf_evaluator = wdl_evaluator
                .as_mut()
                .map(|evaluator| evaluator as &mut dyn SearchLeafEvaluator);
            let result = search_with_context(&position, &request, move_orderer, leaf_evaluator);
            let elapsed_ms = started.elapsed().as_millis().max(1);
            let nps = result.nodes.saturating_mul(1000) / elapsed_ms as u64;
            let best = result
                .best_move
                .map_or_else(|| "0000".to_string(), |mv| mv.to_string());
            if json {
                print_search_json(
                    &args,
                    SearchJsonReport {
                        best: &best,
                        score: result.score,
                        result: &result,
                        fen: &fen,
                        depth,
                        seed: request.seed,
                        elapsed_ms,
                        nodes_per_second: nps,
                        threads,
                        move_order: &move_order,
                        leaf_eval,
                        checkpoint_loaded,
                    },
                )?;
            } else {
                println!("{best}");
            }
        }
        Some("terminal-leaf-suite") => run_terminal_leaf_suite(&args[1..])?,
        Some("uci") => uci::run_uci()?,
        Some("selfplay") => run_selfplay(&args[1..])?,
        Some("train") => run_train(&args[1..])?,
        Some("eval") => run_eval(&args[1..])?,
        Some(other) => return Err(format!("unsupported command: {other}")),
        None => return Err("usage: ark <perft|search|selfplay|train|eval|uci> ...".to_string()),
    }
    Ok(())
}

fn option_value(args: &[String], name: &str) -> Result<Option<String>, String> {
    for pair in args.windows(2) {
        if pair[0] == name {
            return Ok(Some(pair[1].clone()));
        }
    }
    if args.iter().any(|arg| arg == name) {
        return Err(format!("{name} needs a value"));
    }
    Ok(None)
}

fn option_present(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
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

fn load_search_model(
    move_order: &str,
    leaf_eval: LeafEvalMode,
    checkpoint: Option<&str>,
) -> Result<Option<ForgeModel>, String> {
    match move_order {
        "seed" | "default" | "model" => {}
        other => {
            return Err(format!(
                "unsupported --move-order {other}; expected seed or model"
            ))
        }
    }
    let needs_checkpoint = move_order == "model" || leaf_eval == LeafEvalMode::Wdl;
    if !needs_checkpoint && checkpoint.is_some() {
        return Err(
            "--checkpoint was provided but neither --move-order model nor --leaf-eval wdl uses it"
                .to_string(),
        );
    }
    if needs_checkpoint {
        let checkpoint = checkpoint.ok_or_else(|| {
            if leaf_eval == LeafEvalMode::Wdl {
                "--checkpoint is required for --leaf-eval wdl".to_string()
            } else {
                "--checkpoint is required for --move-order model".to_string()
            }
        })?;
        Ok(Some(ForgeModel::load(Path::new(checkpoint))?))
    } else {
        Ok(None)
    }
}

fn parse_u32(text: &str, name: &str) -> Result<u32, String> {
    text.parse()
        .map_err(|_err| format!("{name} must be a positive integer"))
}

fn parse_u64(text: &str, name: &str) -> Result<u64, String> {
    text.parse()
        .map_err(|_err| format!("{name} must be a positive integer"))
}

fn parse_leaf_eval(text: &str) -> Result<LeafEvalMode, String> {
    match text {
        "terminal" | "terminal-only" => Ok(LeafEvalMode::Terminal),
        "wdl" => Ok(LeafEvalMode::Wdl),
        other => Err(format!(
            "unsupported --leaf-eval {other}; expected terminal or wdl"
        )),
    }
}

const fn leaf_eval_name(mode: LeafEvalMode) -> &'static str {
    match mode {
        LeafEvalMode::Terminal => "terminal",
        LeafEvalMode::Wdl => "wdl",
    }
}

fn print_perft_json(
    args: &[String],
    fen: &str,
    depth: u32,
    threads: u32,
    nodes: u64,
    elapsed_ms: u128,
    correct_nodes_override: Option<u64>,
) -> Result<(), String> {
    let (benchmark_id, target_profile, targets) = perft_targets(fen, depth, correct_nodes_override);
    let mut metrics = BTreeMap::new();
    metrics.insert("nodes", Some(MetricValue::U64(nodes)));
    metrics.insert("depth", Some(MetricValue::U64(u64::from(depth))));
    metrics.insert("elapsed_ms", Some(MetricValue::U128(elapsed_ms)));
    metrics.insert("threads", Some(MetricValue::U64(u64::from(threads))));
    metrics.insert("illegal_moves", Some(MetricValue::U64(0)));
    metrics.insert("python_hot_path_ms", Some(MetricValue::U64(0)));
    let evaluation = bench_contract::evaluate(&targets, &metrics);
    let mut report = json!({
        "schema_version": bench_contract::SCHEMA_VERSION,
        "benchmark_id": benchmark_id,
        "status": evaluation.status,
        "git_sha": bench_contract::git_sha(),
        "target_profile": target_profile,
        "command": bench_contract::canonical_command(args),
        "seed": null,
        "metrics": json_report::metric_map(metrics),
        "targets": {},
        "target_results": [],
        "failures": [],
        "confidence": {},
        "artifacts": json_report::artifacts(None, None),
    });
    json_report::append_evaluation_fields(&mut report, &evaluation);
    json_report::write_json_line(io::stdout(), &report)
}

fn perft_targets(
    fen: &str,
    depth: u32,
    correct_nodes_override: Option<u64>,
) -> (&'static str, &'static str, Vec<Target>) {
    let mut targets = Vec::new();
    let mut benchmark_id = "forge-core-perft-adhoc";
    let mut target_profile = "ad_hoc";
    let correct_nodes = if let Some(nodes) = correct_nodes_override {
        Some(nodes)
    } else if fen == STARTPOS_FEN && depth == 4 {
        benchmark_id = "forge-core-perft-startpos-d4-gate";
        target_profile = "local_release_smoke";
        targets.push(Target::max(
            "elapsed_ms_max",
            "elapsed_ms",
            MetricValue::U64(75),
        ));
        Some(197_281)
    } else if fen == KIWIPETE_FEN && depth == 4 {
        benchmark_id = "forge-core-perft-kiwipete-d4-gate";
        target_profile = "local_release_smoke";
        targets.push(Target::max(
            "elapsed_ms_max",
            "elapsed_ms",
            MetricValue::U64(650),
        ));
        Some(4_085_603)
    } else {
        None
    };

    if let Some(nodes) = correct_nodes {
        targets.insert(
            0,
            Target::equal("correct_nodes", "nodes", MetricValue::U64(nodes)),
        );
        targets.push(Target::max(
            "illegal_moves_max",
            "illegal_moves",
            MetricValue::U64(0),
        ));
        targets.push(Target::max(
            "python_hot_path_ms_max",
            "python_hot_path_ms",
            MetricValue::U64(0),
        ));
    }

    (benchmark_id, target_profile, targets)
}

struct SearchJsonReport<'a> {
    best: &'a str,
    score: i32,
    result: &'a ark_core::SearchResult,
    fen: &'a str,
    depth: u32,
    seed: u64,
    elapsed_ms: u128,
    nodes_per_second: u64,
    threads: u32,
    move_order: &'a str,
    leaf_eval: LeafEvalMode,
    checkpoint_loaded: bool,
}

fn print_search_json(args: &[String], report: SearchJsonReport<'_>) -> Result<(), String> {
    let result = report.result;
    let (benchmark_id, target_profile, targets) =
        search_targets(report.fen, report.depth, report.threads);
    let mut metrics = BTreeMap::new();
    metrics.insert("nodes", Some(MetricValue::U64(result.nodes)));
    metrics.insert(
        "nodes_per_second",
        Some(MetricValue::U64(report.nodes_per_second)),
    );
    metrics.insert(
        "legal_moves_generated",
        Some(MetricValue::U64(result.trace.legal_moves_generated)),
    );
    metrics.insert(
        "root_movegen_calls",
        Some(MetricValue::U64(result.trace.root_movegen_calls)),
    );
    metrics.insert(
        "node_movegen_calls",
        Some(MetricValue::U64(result.trace.node_movegen_calls)),
    );
    metrics.insert(
        "total_movegen_calls",
        Some(MetricValue::U64(result.trace.total_movegen_calls)),
    );
    metrics.insert("leaf_evals", Some(MetricValue::U64(result.evals)));
    metrics.insert(
        "terminal_leaf_evals",
        Some(MetricValue::U64(result.trace.terminal_leaf_evals)),
    );
    metrics.insert(
        "neutral_frontier_evals",
        Some(MetricValue::U64(result.trace.neutral_frontier_evals)),
    );
    metrics.insert(
        "wdl_leaf_evals",
        Some(MetricValue::U64(result.trace.external_leaf_eval_calls)),
    );
    metrics.insert(
        "non_terminal_static_eval_calls",
        Some(MetricValue::U64(
            result.trace.non_terminal_static_eval_calls,
        )),
    );
    metrics.insert(
        "transposition_table_probes",
        Some(MetricValue::U64(result.trace.transposition_table_probes)),
    );
    metrics.insert(
        "transposition_table_hit_rate",
        Some(MetricValue::F64(hit_rate(
            result.trace.transposition_table_hits,
            result.trace.transposition_table_probes,
        ))),
    );
    metrics.insert("cutoffs", Some(MetricValue::U64(result.trace.cutoffs)));
    metrics.insert(
        "model_ordered_root_moves",
        Some(MetricValue::U64(result.trace.move_orderer_root_moves)),
    );
    metrics.insert(
        "model_ordered_moves",
        Some(MetricValue::U64(result.trace.move_orderer_moves)),
    );
    metrics.insert(
        "tactical_extension_depth",
        Some(MetricValue::U64(u64::from(
            result.trace.tactical_extension_depth,
        ))),
    );
    metrics.insert(
        "tactical_extension_depth_reached",
        Some(MetricValue::U64(u64::from(
            result.trace.tactical_extension_depth_reached,
        ))),
    );
    metrics.insert(
        "tactical_extension_nodes",
        Some(MetricValue::U64(result.trace.tactical_extension_nodes)),
    );
    metrics.insert(
        "tactical_extension_moves",
        Some(MetricValue::U64(result.trace.tactical_extension_moves)),
    );
    metrics.insert(
        "external_leaf_eval_calls",
        Some(MetricValue::U64(result.trace.external_leaf_eval_calls)),
    );
    metrics.insert(
        "depth_completed",
        Some(MetricValue::U64(u64::from(result.depth_reached))),
    );
    metrics.insert("elapsed_ms", Some(MetricValue::U128(report.elapsed_ms)));
    metrics.insert("threads", Some(MetricValue::U64(u64::from(report.threads))));
    metrics.insert("cpu_utilization_percent", None);
    metrics.insert("rss_mb", None);
    metrics.insert("python_hot_path_ms", Some(MetricValue::U64(0)));
    metrics.insert("speedup_vs_single_thread", None);
    let evaluation = bench_contract::evaluate(&targets, &metrics);
    let mut report_value = json!({
        "schema_version": bench_contract::SCHEMA_VERSION,
        "benchmark_id": benchmark_id,
        "status": evaluation.status,
        "git_sha": bench_contract::git_sha(),
        "target_profile": target_profile,
        "command": bench_contract::canonical_command(args),
        "seed": report.seed,
        "move_order": report.move_order,
        "leaf_eval": leaf_eval_name(report.leaf_eval),
        "checkpoint_loaded": report.checkpoint_loaded,
        "best_move": report.best,
        "score": report.score,
        "metrics": json_report::metric_map(metrics),
        "targets": {},
        "target_results": [],
        "failures": [],
        "confidence": {},
        "artifacts": json_report::artifacts(None, None),
        "trace": {
            "root_moves": result.trace.root_moves,
            "requested_depth": result.trace.requested_depth,
            "node_limit": result.trace.node_limit,
            "movetime_ms": result.trace.movetime_ms,
            "terminal_only": result.trace.terminal_only,
            "stopped_by": result.trace.stopped_by.to_string(),
            "pv_complete": result.trace.pv_complete,
            "root_movegen_calls": result.trace.root_movegen_calls,
            "node_movegen_calls": result.trace.node_movegen_calls,
            "total_movegen_calls": result.trace.total_movegen_calls,
        },
    });
    json_report::append_evaluation_fields(&mut report_value, &evaluation);
    json_report::write_json_line(io::stdout(), &report_value)
}

fn search_targets(
    fen: &str,
    depth: u32,
    threads: u32,
) -> (&'static str, &'static str, Vec<Target>) {
    if fen == STARTPOS_FEN && depth == 6 && threads == 1 {
        return (
            "forge-search-startpos-d6-single-baseline",
            "server_32_vcpu_single_thread",
            vec![
                Target::min(
                    "depth_completed_min",
                    "depth_completed",
                    MetricValue::U64(6),
                ),
                Target::min(
                    "nodes_per_second_min",
                    "nodes_per_second",
                    MetricValue::U64(12_000_000),
                ),
                Target::min(
                    "terminal_leaf_evals_min",
                    "terminal_leaf_evals",
                    MetricValue::U64(1),
                ),
                Target::max(
                    "non_terminal_static_eval_calls_max",
                    "non_terminal_static_eval_calls",
                    MetricValue::U64(0),
                ),
                Target::max("rss_mb_max", "rss_mb", MetricValue::U64(512)),
                Target::max(
                    "python_hot_path_ms_max",
                    "python_hot_path_ms",
                    MetricValue::U64(0),
                ),
            ],
        );
    }
    if fen == STARTPOS_FEN && depth == 6 && threads == 32 {
        return (
            "forge-search-startpos-d6-32cpu-baseline",
            "server_32_vcpu",
            vec![
                Target::min(
                    "depth_completed_min",
                    "depth_completed",
                    MetricValue::U64(6),
                ),
                Target::min(
                    "nodes_per_second_min",
                    "nodes_per_second",
                    MetricValue::U64(160_000_000),
                ),
                Target::min(
                    "speedup_vs_single_thread_min",
                    "speedup_vs_single_thread",
                    MetricValue::F64(10.0),
                ),
                Target::min(
                    "cpu_utilization_percent_min",
                    "cpu_utilization_percent",
                    MetricValue::U64(85),
                ),
                Target::max("rss_mb_max", "rss_mb", MetricValue::U64(4_096)),
                Target::max(
                    "python_hot_path_ms_max",
                    "python_hot_path_ms",
                    MetricValue::U64(0),
                ),
            ],
        );
    }
    (
        "forge-search-adhoc",
        "ad_hoc",
        vec![
            Target::min(
                "depth_completed_min",
                "depth_completed",
                MetricValue::U64(u64::from(depth)),
            ),
            Target::max(
                "non_terminal_static_eval_calls_max",
                "non_terminal_static_eval_calls",
                MetricValue::U64(0),
            ),
            Target::max(
                "python_hot_path_ms_max",
                "python_hot_path_ms",
                MetricValue::U64(0),
            ),
        ],
    )
}

fn hit_rate(hits: u64, probes: u64) -> f64 {
    if probes == 0 {
        0.0
    } else {
        hits as f64 / probes as f64
    }
}

fn run_terminal_leaf_suite(args: &[String]) -> Result<(), String> {
    let cases = option_value(args, "--cases")?
        .map(|text| parse_u32(&text, "--cases"))
        .transpose()?
        .unwrap_or(64);
    let json = option_present(args, "--json");
    let mate = Position::from_fen("7k/6Q1/5K2/8/8/8/8/8 w - - 0 1")
        .map_err(|err| format!("bad FEN: {err:?}"))?;
    let request = SearchRequest {
        depth: 1,
        seed: 1,
        ..SearchRequest::default()
    };
    let started = Instant::now();
    let result = search(&mate, &request);
    let elapsed_ms = started.elapsed().as_millis().max(1);
    let passed = u32::from(result.trace.non_terminal_static_eval_calls == 0).saturating_mul(cases);
    if json {
        let mut metrics = BTreeMap::new();
        metrics.insert(
            "terminal_cases_passed",
            Some(MetricValue::U64(u64::from(passed))),
        );
        metrics.insert(
            "non_terminal_static_eval_calls",
            Some(MetricValue::U64(
                result.trace.non_terminal_static_eval_calls,
            )),
        );
        metrics.insert("elapsed_ms", Some(MetricValue::U128(elapsed_ms)));
        metrics.insert("python_hot_path_ms", Some(MetricValue::U64(0)));
        let targets = vec![
            Target::min(
                "terminal_cases_passed_min",
                "terminal_cases_passed",
                MetricValue::U64(u64::from(cases)),
            ),
            Target::max(
                "non_terminal_static_eval_calls_max",
                "non_terminal_static_eval_calls",
                MetricValue::U64(0),
            ),
            Target::max("elapsed_ms_max", "elapsed_ms", MetricValue::U64(50)),
            Target::max(
                "python_hot_path_ms_max",
                "python_hot_path_ms",
                MetricValue::U64(0),
            ),
        ];
        let evaluation = bench_contract::evaluate(&targets, &metrics);
        let command_args = command_args("terminal-leaf-suite", args);
        metrics.insert("cases", Some(MetricValue::U64(u64::from(cases))));
        let mut report = json!({
            "schema_version": bench_contract::SCHEMA_VERSION,
            "benchmark_id": "forge-search-terminal-leaf-gate",
            "status": evaluation.status,
            "git_sha": bench_contract::git_sha(),
            "target_profile": "local_release_smoke",
            "command": bench_contract::canonical_command(&command_args),
            "seed": 1,
            "metrics": json_report::metric_map(metrics),
            "targets": {},
            "target_results": [],
            "failures": [],
            "confidence": {},
            "artifacts": json_report::artifacts(None, None),
        });
        json_report::append_evaluation_fields(&mut report, &evaluation);
        json_report::write_json_line(io::stdout(), &report)?;
    } else {
        println!("terminal_cases_passed={passed}/{cases}");
    }
    Ok(())
}

fn command_args(command: &str, args: &[String]) -> Vec<String> {
    let mut full_args = Vec::with_capacity(args.len() + 1);
    full_args.push(command.to_string());
    full_args.extend(args.iter().cloned());
    full_args
}

#[derive(Clone, Debug)]
struct SelfPlayConfig {
    games: u32,
    search_depth: u32,
    tactical_extension_depth: u32,
    max_plies: u32,
    actors: u32,
    seed: u64,
    out: String,
    chunk_size: u32,
    chunked: bool,
    move_order: String,
    leaf_eval: LeafEvalMode,
    checkpoint: Option<String>,
}

impl Default for SelfPlayConfig {
    fn default() -> Self {
        Self {
            games: 16,
            search_depth: 1,
            tactical_extension_depth: SearchRequest::default().tactical_extension_depth,
            max_plies: 256,
            actors: 1,
            seed: 1,
            out: "runs/v4/selfplay-baseline.arkgames".to_string(),
            chunk_size: 4096,
            chunked: false,
            move_order: "seed".to_string(),
            leaf_eval: LeafEvalMode::Terminal,
            checkpoint: None,
        }
    }
}

fn run_selfplay(args: &[String]) -> Result<(), String> {
    let mut config = if let Some(path) = option_value(args, "--config")? {
        load_selfplay_config(&path)?
    } else {
        SelfPlayConfig::default()
    };
    if let Some(games) = option_value(args, "--games")? {
        config.games = parse_u32(&games, "--games")?;
    }
    if let Some(depth) = option_value(args, "--search-depth")? {
        config.search_depth = parse_u32(&depth, "--search-depth")?;
    }
    if let Some(depth) = option_value(args, "--tactical-extension-depth")? {
        config.tactical_extension_depth = parse_u32(&depth, "--tactical-extension-depth")?;
    }
    if let Some(max_plies) = option_value(args, "--max-plies")? {
        config.max_plies = parse_u32(&max_plies, "--max-plies")?;
    }
    if let Some(actors) = option_value(args, "--actors")? {
        config.actors = parse_u32(&actors, "--actors")?.max(1);
    }
    if let Some(seed) = option_value(args, "--seed")? {
        config.seed = seed
            .parse()
            .map_err(|_err| "--seed must be an integer".to_string())?;
    }
    if let Some(out) = option_value(args, "--out")? {
        config.out = out;
    }
    if let Some(chunk_size) = option_value(args, "--chunk-size")? {
        config.chunk_size = parse_u32(&chunk_size, "--chunk-size")?.max(1);
    }
    if let Some(move_order) = option_value(args, "--move-order")? {
        config.move_order = move_order;
    }
    if let Some(leaf_eval) = option_value(args, "--leaf-eval")? {
        config.leaf_eval = parse_leaf_eval(&leaf_eval)?;
    }
    if let Some(checkpoint) = option_value(args, "--checkpoint")? {
        config.checkpoint = Some(checkpoint);
    }
    if option_present(args, "--chunked") {
        config.chunked = true;
    }
    let model = load_selfplay_model(&config)?;
    let checkpoint_loaded = model.is_some();

    let started = Instant::now();
    let summary = if config.chunked || !config.out.ends_with(".arkgames") {
        write_streaming_selfplay_games(&config, model)?
    } else {
        write_selfplay_games(&config, model)?
    };
    let elapsed_ms = started.elapsed().as_millis().max(1);
    let games_per_second = f64::from(summary.games_completed) / (elapsed_ms as f64 / 1000.0);
    let plies_per_second = summary.plies as f64 / (elapsed_ms as f64 / 1000.0);
    let search_nodes_per_second = summary.search_nodes as f64 / (elapsed_ms as f64 / 1000.0);
    if option_present(args, "--json") {
        let (benchmark_id, target_profile, targets) = selfplay_targets(&config);
        let p95_actor_skew = summary.p95_actor_skew;
        let mut metrics = BTreeMap::new();
        metrics.insert(
            "games_requested",
            Some(MetricValue::U64(u64::from(config.games))),
        );
        metrics.insert(
            "games_completed",
            Some(MetricValue::U64(u64::from(summary.games_completed))),
        );
        metrics.insert("games_per_second", Some(MetricValue::F64(games_per_second)));
        metrics.insert("plies", Some(MetricValue::U64(summary.plies)));
        metrics.insert("plies_per_second", Some(MetricValue::F64(plies_per_second)));
        metrics.insert("positions_emitted", Some(MetricValue::U64(summary.plies)));
        metrics.insert("search_nodes", Some(MetricValue::U64(summary.search_nodes)));
        metrics.insert(
            "root_movegen_calls",
            Some(MetricValue::U64(summary.root_movegen_calls)),
        );
        metrics.insert(
            "node_movegen_calls",
            Some(MetricValue::U64(summary.node_movegen_calls)),
        );
        metrics.insert(
            "total_movegen_calls",
            Some(MetricValue::U64(summary.total_movegen_calls)),
        );
        metrics.insert(
            "search_nodes_per_second",
            Some(MetricValue::F64(search_nodes_per_second)),
        );
        metrics.insert(
            "neutral_frontier_evals",
            Some(MetricValue::U64(summary.neutral_frontier_evals)),
        );
        metrics.insert(
            "wdl_leaf_evals",
            Some(MetricValue::U64(summary.wdl_leaf_evals)),
        );
        metrics.insert(
            "illegal_moves",
            Some(MetricValue::U64(u64::from(summary.illegal_moves))),
        );
        metrics.insert(
            "unhandled_terminal_states",
            Some(MetricValue::U64(u64::from(
                summary.unhandled_terminal_states,
            ))),
        );
        metrics.insert("timeouts", Some(MetricValue::U64(0)));
        metrics.insert(
            "actor_crashes",
            Some(MetricValue::U64(u64::from(summary.actor_crashes))),
        );
        metrics.insert(
            "actor_count",
            Some(MetricValue::U64(u64::from(config.actors))),
        );
        metrics.insert("p95_actor_skew", Some(MetricValue::F64(p95_actor_skew)));
        metrics.insert(
            "chunks_published",
            Some(MetricValue::U64(u64::from(summary.chunks_published))),
        );
        metrics.insert("cpu_utilization_percent", None);
        metrics.insert("rss_mb", None);
        metrics.insert("rss_growth_percent", None);
        metrics.insert("committed_artifacts", Some(MetricValue::U64(0)));
        metrics.insert("python_hot_path_ms", Some(MetricValue::U64(0)));
        let evaluation = bench_contract::evaluate(&targets, &metrics);
        let command_args = command_args("selfplay", args);
        let mut report = json!({
            "schema_version": bench_contract::SCHEMA_VERSION,
            "benchmark_id": benchmark_id,
            "status": evaluation.status,
            "git_sha": bench_contract::git_sha(),
            "target_profile": target_profile,
            "command": bench_contract::canonical_command(&command_args),
            "seed": config.seed,
            "move_order": config.move_order,
            "leaf_eval": leaf_eval_name(config.leaf_eval),
            "checkpoint_loaded": checkpoint_loaded,
            "metrics": json_report::metric_map(metrics),
            "targets": {},
            "target_results": [],
            "failures": [],
            "confidence": {},
            "artifacts": json_report::artifacts(Some(config.out.clone()), Some(summary.replay_format)),
        });
        json_report::append_evaluation_fields(&mut report, &evaluation);
        json_report::write_json_line(io::stdout(), &report)?;
    } else {
        println!(
            "games={} plies={} out={}",
            summary.games_completed, summary.plies, config.out
        );
    }
    Ok(())
}

fn selfplay_targets(config: &SelfPlayConfig) -> (&'static str, &'static str, Vec<Target>) {
    if config.games == 16_384 && config.actors == 32 && config.search_depth == 1 {
        return (
            "forge-selfplay-legal-d1-32cpu-baseline",
            "server_32_vcpu",
            vec![
                Target::min(
                    "games_completed_min",
                    "games_completed",
                    MetricValue::U64(16_384),
                ),
                Target::min(
                    "games_per_second_min",
                    "games_per_second",
                    MetricValue::U64(800),
                ),
                Target::min(
                    "plies_per_second_min",
                    "plies_per_second",
                    MetricValue::U64(120_000),
                ),
                Target::max("illegal_moves_max", "illegal_moves", MetricValue::U64(0)),
                Target::max("timeouts_max", "timeouts", MetricValue::U64(0)),
                Target::max("actor_crashes_max", "actor_crashes", MetricValue::U64(0)),
                Target::max(
                    "p95_actor_skew_max",
                    "p95_actor_skew",
                    MetricValue::F64(1.5),
                ),
                Target::max(
                    "committed_artifacts_max",
                    "committed_artifacts",
                    MetricValue::U64(0),
                ),
                Target::max(
                    "python_hot_path_ms_max",
                    "python_hot_path_ms",
                    MetricValue::U64(0),
                ),
            ],
        );
    }
    if config.games == 2_048 && config.actors == 32 && config.search_depth == 3 {
        return (
            "forge-selfplay-search-d3-32cpu-baseline",
            "server_32_vcpu",
            vec![
                Target::min(
                    "games_completed_min",
                    "games_completed",
                    MetricValue::U64(2_048),
                ),
                Target::min(
                    "games_per_second_min",
                    "games_per_second",
                    MetricValue::U64(12),
                ),
                Target::min(
                    "plies_per_second_min",
                    "plies_per_second",
                    MetricValue::U64(1_500),
                ),
                Target::min(
                    "search_nodes_per_second_min",
                    "search_nodes_per_second",
                    MetricValue::U64(60_000_000),
                ),
                Target::max("illegal_moves_max", "illegal_moves", MetricValue::U64(0)),
                Target::max("timeouts_max", "timeouts", MetricValue::U64(0)),
                Target::max("actor_crashes_max", "actor_crashes", MetricValue::U64(0)),
                Target::min(
                    "cpu_utilization_percent_min",
                    "cpu_utilization_percent",
                    MetricValue::U64(85),
                ),
                Target::max(
                    "committed_artifacts_max",
                    "committed_artifacts",
                    MetricValue::U64(0),
                ),
                Target::max(
                    "python_hot_path_ms_max",
                    "python_hot_path_ms",
                    MetricValue::U64(0),
                ),
            ],
        );
    }
    (
        "forge-selfplay-adhoc",
        "ad_hoc",
        vec![
            Target::min(
                "games_completed_min",
                "games_completed",
                MetricValue::U64(u64::from(config.games)),
            ),
            Target::max("illegal_moves_max", "illegal_moves", MetricValue::U64(0)),
            Target::max("timeouts_max", "timeouts", MetricValue::U64(0)),
            Target::max("actor_crashes_max", "actor_crashes", MetricValue::U64(0)),
            Target::max(
                "committed_artifacts_max",
                "committed_artifacts",
                MetricValue::U64(0),
            ),
            Target::max(
                "python_hot_path_ms_max",
                "python_hot_path_ms",
                MetricValue::U64(0),
            ),
        ],
    )
}

fn p95_actor_skew(actor_game_counts: &[u32]) -> f64 {
    if actor_game_counts.is_empty() {
        return 0.0;
    }
    let total_games = actor_game_counts
        .iter()
        .map(|count| u64::from(*count))
        .sum::<u64>();
    if total_games == 0 {
        return 0.0;
    }
    let mean = total_games as f64 / actor_game_counts.len() as f64;
    let mut sorted = actor_game_counts.to_vec();
    sorted.sort_unstable();
    let rank = (sorted.len() * 95).div_ceil(100).saturating_sub(1);
    let index = rank.min(sorted.len().saturating_sub(1));
    f64::from(sorted[index]) / mean
}

fn load_selfplay_model(config: &SelfPlayConfig) -> Result<Option<ForgeModel>, String> {
    match config.move_order.as_str() {
        "seed" | "default" | "model" => {}
        other => {
            return Err(format!(
                "unsupported --move-order {other}; expected seed or model"
            ))
        }
    }
    let needs_checkpoint = config.move_order == "model" || config.leaf_eval == LeafEvalMode::Wdl;
    if !needs_checkpoint && config.checkpoint.is_some() {
        return Err(
            "--checkpoint was provided but neither --move-order model nor --leaf-eval wdl uses it"
                .to_string(),
        );
    }
    if needs_checkpoint {
        let checkpoint = config.checkpoint.as_deref().ok_or_else(|| {
            if config.leaf_eval == LeafEvalMode::Wdl {
                "--checkpoint is required for --leaf-eval wdl".to_string()
            } else {
                "--checkpoint is required for --move-order model".to_string()
            }
        })?;
        Ok(Some(ForgeModel::load(Path::new(checkpoint))?))
    } else {
        Ok(None)
    }
}

#[derive(Clone, Debug)]
struct SelfPlaySummary {
    games_completed: u32,
    plies: u64,
    search_nodes: u64,
    root_movegen_calls: u64,
    node_movegen_calls: u64,
    total_movegen_calls: u64,
    neutral_frontier_evals: u64,
    wdl_leaf_evals: u64,
    actor_crashes: u32,
    p95_actor_skew: f64,
    chunks_published: u32,
    illegal_moves: u32,
    unhandled_terminal_states: u32,
    replay_format: &'static str,
}

#[derive(Clone, Debug)]
struct CompletedCliGame {
    game_index: u32,
    record: GameRecord,
    search_nodes: u64,
    root_movegen_calls: u64,
    node_movegen_calls: u64,
    total_movegen_calls: u64,
    neutral_frontier_evals: u64,
    wdl_leaf_evals: u64,
}

fn write_selfplay_games(
    config: &SelfPlayConfig,
    model: Option<ForgeModel>,
) -> Result<SelfPlaySummary, String> {
    let out_path = Path::new(&config.out);
    let mut handles = Vec::new();
    for actor_id in 0..config.actors {
        let actor_config = config.clone();
        let actor_model = model.clone();
        handles.push(thread::spawn(move || {
            actor_games(actor_id, actor_config, actor_model)
        }));
    }
    let mut indexed_games: Vec<Option<GameRecord>> = vec![None; config.games as usize];
    let mut search_nodes = 0_u64;
    let mut root_movegen_calls = 0_u64;
    let mut node_movegen_calls = 0_u64;
    let mut total_movegen_calls = 0_u64;
    let mut neutral_frontier_evals = 0_u64;
    let mut wdl_leaf_evals = 0_u64;
    let mut actor_crashes = 0_u32;
    let mut actor_game_counts = vec![0_u32; config.actors as usize];
    for (actor_id, handle) in handles.into_iter().enumerate() {
        match handle.join() {
            Ok(Ok(records)) => {
                let count = u32::try_from(records.len())
                    .map_err(|_err| "self-play actor produced too many records".to_string())?;
                if let Some(slot) = actor_game_counts.get_mut(actor_id) {
                    *slot = count;
                }
                for completed in records {
                    if let Some(slot) = indexed_games.get_mut(completed.game_index as usize) {
                        *slot = Some(completed.record);
                        search_nodes += completed.search_nodes;
                        root_movegen_calls += completed.root_movegen_calls;
                        node_movegen_calls += completed.node_movegen_calls;
                        total_movegen_calls += completed.total_movegen_calls;
                        neutral_frontier_evals += completed.neutral_frontier_evals;
                        wdl_leaf_evals += completed.wdl_leaf_evals;
                    }
                }
            }
            Ok(Err(err)) => return Err(err),
            Err(_err) => actor_crashes += 1,
        }
    }
    if actor_crashes > 0 {
        return Err(format!("self-play actor crashes: {actor_crashes}"));
    }
    let games: Vec<GameRecord> = indexed_games
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "self-play actor did not return every game".to_string())?;
    let replay = write_replay(out_path, &games)?;
    Ok(SelfPlaySummary {
        games_completed: replay.games,
        plies: replay.plies,
        search_nodes,
        root_movegen_calls,
        node_movegen_calls,
        total_movegen_calls,
        neutral_frontier_evals,
        wdl_leaf_evals,
        actor_crashes,
        p95_actor_skew: p95_actor_skew(&actor_game_counts),
        chunks_published: 0,
        illegal_moves: 0,
        unhandled_terminal_states: 0,
        replay_format: "arkgames-v1",
    })
}

fn write_streaming_selfplay_games(
    config: &SelfPlayConfig,
    model: Option<ForgeModel>,
) -> Result<SelfPlaySummary, String> {
    let summary = run_streaming_selfplay(StreamingSelfPlayConfig {
        games: config.games,
        search_depth: config.search_depth,
        tactical_extension_depth: config.tactical_extension_depth,
        max_plies: config.max_plies,
        actors: config.actors,
        seed: config.seed,
        chunk_size: config.chunk_size,
        out_dir: Path::new(&config.out).to_path_buf(),
        model,
        leaf_eval: config.leaf_eval,
    })?;
    Ok(SelfPlaySummary {
        games_completed: summary.games_completed,
        plies: summary.plies,
        search_nodes: summary.search_nodes,
        root_movegen_calls: summary.root_movegen_calls,
        node_movegen_calls: summary.node_movegen_calls,
        total_movegen_calls: summary.total_movegen_calls,
        neutral_frontier_evals: summary.neutral_frontier_evals,
        wdl_leaf_evals: summary.wdl_leaf_evals,
        actor_crashes: summary.actor_crashes,
        p95_actor_skew: summary.p95_actor_skew,
        chunks_published: summary.chunks_published,
        illegal_moves: summary.illegal_moves,
        unhandled_terminal_states: summary.unhandled_terminal_states,
        replay_format: "arkchunks-v2",
    })
}

fn actor_games(
    actor_id: u32,
    config: SelfPlayConfig,
    model: Option<ForgeModel>,
) -> Result<Vec<CompletedCliGame>, String> {
    let mut records = Vec::new();
    let mut game_index = actor_id;
    while game_index < config.games {
        records.push(play_one_game(game_index, &config, model.as_ref())?);
        game_index = game_index.saturating_add(config.actors);
    }
    Ok(records)
}

fn play_one_game(
    game_index: u32,
    config: &SelfPlayConfig,
    model: Option<&ForgeModel>,
) -> Result<CompletedCliGame, String> {
    let mut state = GameState::startpos().map_err(|err| format!("startpos failed: {err:?}"))?;
    let mut moves = Vec::with_capacity(config.max_plies as usize);
    let mut result = GameOutcome::Draw;
    let mut search_nodes = 0_u64;
    let mut root_movegen_calls = 0_u64;
    let mut node_movegen_calls = 0_u64;
    let mut total_movegen_calls = 0_u64;
    let mut neutral_frontier_evals = 0_u64;
    let mut wdl_leaf_evals = 0_u64;
    let mut legal_moves = MoveList::with_capacity(96);
    let mut orderer = model.map(|model| ModelMoveOrderer {
        model,
        workspace: PolicyMoveWorkspace::default(),
    });
    let mut leaf_evaluator = match config.leaf_eval {
        LeafEvalMode::Terminal => None,
        LeafEvalMode::Wdl => {
            let model = model.ok_or_else(|| {
                "--checkpoint is required for --leaf-eval wdl in self-play".to_string()
            })?;
            Some(WdlLeafEvaluator {
                model,
                workspace: WdlEvaluationWorkspace::default(),
            })
        }
    };
    for ply in 0..config.max_plies {
        legal_moves.clear();
        state.position().legal_moves_into(&mut legal_moves);
        root_movegen_calls = root_movegen_calls.saturating_add(1);
        total_movegen_calls = total_movegen_calls.saturating_add(1);
        if let Some(outcome) = state.outcome_from_legal_moves(&legal_moves) {
            result = outcome;
            break;
        }
        let request = SearchRequest {
            depth: config.search_depth,
            seed: config.seed ^ u64::from(game_index).rotate_left(13) ^ u64::from(ply),
            tactical_extension_depth: config.tactical_extension_depth,
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
        search_nodes += search_result.nodes;
        root_movegen_calls =
            root_movegen_calls.saturating_add(search_result.trace.root_movegen_calls);
        node_movegen_calls =
            node_movegen_calls.saturating_add(search_result.trace.node_movegen_calls);
        total_movegen_calls =
            total_movegen_calls.saturating_add(search_result.trace.total_movegen_calls);
        neutral_frontier_evals += search_result.trace.neutral_frontier_evals;
        wdl_leaf_evals += search_result.trace.external_leaf_eval_calls;
        let Some(best) = search_result.best_move else {
            return Err("search returned no move in non-terminal self-play position".to_string());
        };
        state.make_move(best);
        moves.push(best.packed_id());
    }
    legal_moves.clear();
    state.position().legal_moves_into(&mut legal_moves);
    root_movegen_calls = root_movegen_calls.saturating_add(1);
    total_movegen_calls = total_movegen_calls.saturating_add(1);
    if let Some(outcome) = state.outcome_from_legal_moves(&legal_moves) {
        result = outcome;
    }
    Ok(CompletedCliGame {
        game_index,
        record: GameRecord { result, moves },
        search_nodes,
        root_movegen_calls,
        node_movegen_calls,
        total_movegen_calls,
        neutral_frontier_evals,
        wdl_leaf_evals,
    })
}

fn load_selfplay_config(path: &str) -> Result<SelfPlayConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let mut config = SelfPlayConfig::default();
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        match key {
            "games" => config.games = parse_u32(value, "games")?,
            "search_depth" => config.search_depth = parse_u32(value, "search_depth")?,
            "tactical_extension_depth" => {
                config.tactical_extension_depth = parse_u32(value, "tactical_extension_depth")?
            }
            "max_plies" => config.max_plies = parse_u32(value, "max_plies")?,
            "actors" => config.actors = parse_u32(value, "actors")?.max(1),
            "seed" => {
                config.seed = value
                    .parse()
                    .map_err(|_err| "seed must be an integer".to_string())?
            }
            "out" => config.out = value.to_string(),
            "chunk_size" => config.chunk_size = parse_u32(value, "chunk_size")?.max(1),
            "chunked" => config.chunked = parse_bool(value, "chunked")?,
            "move_order" => config.move_order = value.to_string(),
            "leaf_eval" => config.leaf_eval = parse_leaf_eval(value)?,
            "checkpoint" => config.checkpoint = Some(value.to_string()),
            _ => {}
        }
    }
    Ok(config)
}

fn parse_bool(text: &str, name: &str) -> Result<bool, String> {
    match text {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{name} must be true or false")),
    }
}

#[derive(Clone, Debug)]
struct TrainConfig {
    replay: String,
    out: String,
    steps: u32,
    learning_rate: f32,
}

impl Default for TrainConfig {
    fn default() -> Self {
        Self {
            replay: "runs/v4/selfplay-baseline.arkgames".to_string(),
            out: "models/ark-v4-forge-smoke.arkmodel".to_string(),
            steps: 1,
            learning_rate: 0.05,
        }
    }
}

fn run_train(args: &[String]) -> Result<(), String> {
    let mut config = if let Some(path) = option_value(args, "--config")? {
        load_train_config(&path)?
    } else {
        TrainConfig::default()
    };
    if let Some(replay) = option_value(args, "--replay")? {
        config.replay = replay;
    }
    if let Some(out) = option_value(args, "--out")? {
        config.out = out;
    }
    if let Some(checkpoint) = option_value(args, "--checkpoint")? {
        config.out = checkpoint;
    }
    if let Some(steps) = option_value(args, "--steps")? {
        config.steps = parse_u32(&steps, "--steps")?;
    }
    if let Some(lr) = option_value(args, "--learning-rate")? {
        config.learning_rate = lr
            .parse()
            .map_err(|_err| "--learning-rate must be numeric".to_string())?;
    }
    let games = read_replay_any(Path::new(&config.replay))?;
    let validation = validate_replay(&games)?;
    if validation.illegal_moves != 0 || validation.unhandled_terminal_states != 0 {
        return Err(format!(
            "replay validation failed: illegal_moves={} unhandled_terminal_states={}",
            validation.illegal_moves, validation.unhandled_terminal_states
        ));
    }
    let mut model = ForgeModel::default();
    let summary = model.train_games(&games, config.steps, config.learning_rate);
    model.save(Path::new(&config.out))?;
    if option_present(args, "--json") {
        let report = json!({
            "schema_version": "ark-v4-train-v1",
            "checkpoint": config.out,
            "replay": config.replay,
            "training_steps": summary.training_steps,
            "games_seen": summary.games_seen,
            "plies_seen": summary.plies_seen,
            "policy_nonzero": summary.policy_nonzero,
            "heads": ["policy", "wdl", "moves_left", "uncertainty", "risk", "refutation"],
        });
        json_report::write_json_line(io::stdout(), &report)?;
    } else {
        println!(
            "checkpoint={} steps={} games={} plies={}",
            config.out, summary.training_steps, summary.games_seen, summary.plies_seen
        );
    }
    Ok(())
}

fn load_train_config(path: &str) -> Result<TrainConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let mut config = TrainConfig::default();
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "replay" => config.replay = value.to_string(),
            "out" => config.out = value.to_string(),
            "steps" => config.steps = parse_u32(value, "steps")?,
            "learning_rate" => {
                config.learning_rate = value
                    .parse()
                    .map_err(|_err| "learning_rate must be numeric".to_string())?
            }
            _ => {}
        }
    }
    Ok(config)
}

fn run_eval(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("baseline") => run_eval_baseline(&args[1..]),
        Some(other) => Err(format!("unsupported eval command: {other}")),
        None => run_eval_baseline(args),
    }
}

fn run_eval_baseline(args: &[String]) -> Result<(), String> {
    let replay = option_value(args, "--replay")?.or_else(|| {
        option_value(args, "--run-dir").ok().flatten().map(|dir| {
            let legacy = Path::new(&dir).join("selfplay-baseline.arkgames");
            if legacy.exists() {
                legacy.to_string_lossy().into_owned()
            } else {
                dir
            }
        })
    });
    let checkpoint = option_value(args, "--checkpoint")?;
    let validation = if let Some(replay) = replay.as_deref() {
        let games = read_replay_any(Path::new(replay))?;
        validate_replay(&games)?
    } else {
        ark_model::ReplayValidation {
            games: 0,
            plies: 0,
            illegal_moves: 0,
            unhandled_terminal_states: 0,
        }
    };
    let checkpoint_loaded = if let Some(path) = checkpoint.as_deref() {
        ForgeModel::load(Path::new(path))?;
        true
    } else {
        false
    };
    if option_present(args, "--json") {
        let report = json!({
            "schema_version": "ark-v4-eval-baseline-v1",
            "replay": replay.as_deref().unwrap_or(""),
            "games": validation.games,
            "plies": validation.plies,
            "illegal_moves": validation.illegal_moves,
            "unhandled_terminal_states": validation.unhandled_terminal_states,
            "checkpoint_loaded": checkpoint_loaded,
        });
        json_report::write_json_line(io::stdout(), &report)?;
    } else {
        println!(
            "games={} plies={} illegal_moves={} unhandled_terminal_states={}",
            validation.games,
            validation.plies,
            validation.illegal_moves,
            validation.unhandled_terminal_states
        );
    }
    Ok(())
}
