# ArK-V4 Forge Rust Performance Contract

This document defines the benchmark schema and acceptance rules for Ark engine performance work.
It covers runtime metrics, output contracts, and failure conditions for search and self-play.

## Locked Architecture

- Core: Rust custom bitboard core from day one.
- Search: classical NN search shape, with terminal leaf eval as the default and checkpointed WDL
  leaf eval available only behind an explicit flag.
- Self-play: games-only self-play first.
- Execution target: CPU-only actor, learner, and inference layout sized for workstation and
  multi-core server hosts.
- Later lane: deeper CPU search, model-guided move ordering, and chunked training throughput.
- Hot path: Rust only. Python may run repository tests or parse completed JSON output, but it must not
  generate moves, make moves, evaluate leaves, search, schedule actors, or write games during a run.

## Pass Rules

- Every active benchmark command must be runnable from the repo root with `cargo run --release`.
- Benchmark stdout must be exactly one JSON object per completed benchmark. Human progress goes to
  stderr only.
- Throughput metrics pass only when the one-sided 95 percent confidence lower bound meets the target.
- Latency, memory, error, and artifact metrics pass only when the 95 percent confidence upper bound is
  at or below the target.
- Any illegal move, panic, actor crash, Python process in the hot path, or committed model/data output
  is a hard failure.
- Self-play outputs go to `/tmp` or an ignored local output path. They are never committed.

## Machine Contract

The JSON block below is the source of truth for tests.

<!-- ark-v4-forge-performance-contract:begin -->
```json
{
  "contract_version": "ark-v4-forge-perf-v1",
  "architecture": {
    "core_language": "rust",
    "core": "custom_bitboard",
    "search": "classical_nn_search",
    "self_play_bootstrap": "games_only_first",
    "leaf_eval": "terminal_default_wdl_checkpointed",
    "primary_target": "server_first_32_vcpu_cpu_actors",
    "later_target": "cpu_only_deeper_search_and_training"
  },
  "runtime_guardrails": {
    "no_global_installs": true,
    "no_host_toolchain_mutations": true,
    "no_archive_imports_or_copies": true,
    "no_model_or_data_artifacts_committed": true,
    "no_python_hot_path": true,
    "rust_release_build_required": true
  },
  "statistical_policy": {
    "minimum_repetitions": 5,
    "throughput_pass_rule": "ci95_lower_gte_target",
    "latency_pass_rule": "ci95_upper_lte_target",
    "error_pass_rule": "observed_lte_target",
    "confidence": 0.95
  },
  "required_metrics": {
    "search": [
      "nodes",
      "nodes_per_second",
      "speedup_vs_single_thread",
      "legal_moves_generated",
      "root_movegen_calls",
      "node_movegen_calls",
      "total_movegen_calls",
      "leaf_evals",
      "terminal_leaf_evals",
      "neutral_frontier_evals",
      "wdl_leaf_evals",
      "external_leaf_eval_calls",
      "non_terminal_static_eval_calls",
      "tactical_extension_depth",
      "tactical_extension_depth_reached",
      "tactical_extension_nodes",
      "tactical_extension_moves",
      "model_ordered_root_moves",
      "model_ordered_moves",
      "transposition_table_probes",
      "transposition_table_hit_rate",
      "cutoffs",
      "depth_completed",
      "elapsed_ms",
      "threads",
      "cpu_utilization_percent",
      "rss_mb",
      "python_hot_path_ms"
    ],
    "self_play": [
      "games_requested",
      "games_completed",
      "games_per_second",
      "plies",
      "plies_per_second",
      "positions_emitted",
      "search_nodes",
      "root_movegen_calls",
      "node_movegen_calls",
      "total_movegen_calls",
      "search_nodes_per_second",
      "neutral_frontier_evals",
      "wdl_leaf_evals",
      "illegal_moves",
      "timeouts",
      "actor_crashes",
      "actor_count",
      "p95_actor_skew",
      "cpu_utilization_percent",
      "rss_mb",
      "rss_growth_percent",
      "committed_artifacts",
      "python_hot_path_ms"
    ]
  },
  "required_stdout_fields": [
    "schema_version",
    "benchmark_id",
    "status",
    "git_sha",
    "target_profile",
    "command",
    "seed",
    "metrics",
    "targets",
    "target_results",
    "failures",
    "confidence",
    "artifacts"
  ],
  "status_policy": {
    "allowed_statuses": ["pass", "fail"],
    "pass_requires_non_empty_targets": true,
    "pass_requires_all_target_results_passed": true,
    "unmeasured_target_metric_is_failure": true,
    "failed_targets_must_be_named": true,
    "stage0_confidence_complete": false,
    "search_threads_above_one": "reject"
  },
  "benchmarks": [
    {
      "id": "forge-core-perft-startpos-d4-gate",
      "lane": "gate",
      "command": "cargo run --release -p ark_cli -- perft --fen \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\" --depth 4 --threads 1 --json",
      "target_profile": "local_release_smoke",
      "targets": {
        "correct_nodes": 197281,
        "elapsed_ms_max": 75,
        "illegal_moves_max": 0,
        "python_hot_path_ms_max": 0
      }
    },
    {
      "id": "forge-core-perft-kiwipete-d4-gate",
      "lane": "gate",
      "command": "cargo run --release -p ark_cli -- perft --fen \"r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1\" --depth 4 --threads 1 --json",
      "target_profile": "local_release_smoke",
      "targets": {
        "correct_nodes": 4085603,
        "elapsed_ms_max": 650,
        "illegal_moves_max": 0,
        "python_hot_path_ms_max": 0
      }
    },
    {
      "id": "forge-search-terminal-leaf-gate",
      "lane": "gate",
      "command": "cargo run --release -p ark_cli -- terminal-leaf-suite --cases 64 --json",
      "target_profile": "local_release_smoke",
      "targets": {
        "terminal_cases_passed_min": 64,
        "non_terminal_static_eval_calls_max": 0,
        "elapsed_ms_max": 50,
        "python_hot_path_ms_max": 0
      }
    },
    {
      "id": "forge-search-startpos-d6-single-baseline",
      "lane": "baseline",
      "command": "cargo run --release -p ark_cli -- search --fen \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\" --depth 6 --threads 1 --seed 1729 --json",
      "target_profile": "server_32_vcpu_single_thread",
      "targets": {
        "depth_completed_min": 6,
        "nodes_per_second_min": 12000000,
        "terminal_leaf_evals_min": 1,
        "non_terminal_static_eval_calls_max": 0,
        "rss_mb_max": 512,
        "python_hot_path_ms_max": 0
      }
    },
    {
      "id": "forge-selfplay-legal-d1-32cpu-baseline",
      "lane": "baseline",
      "command": "cargo run --release -p ark_cli -- selfplay --games 16384 --actors 32 --search-depth 1 --seed 1729 --out /tmp/ark-v4-forge/selfplay-d1 --chunked --chunk-size 4096 --json",
      "target_profile": "server_32_vcpu",
      "targets": {
        "games_completed_min": 16384,
        "games_per_second_min": 800,
        "plies_per_second_min": 120000,
        "illegal_moves_max": 0,
        "timeouts_max": 0,
        "actor_crashes_max": 0,
        "p95_actor_skew_max": 1.5,
        "committed_artifacts_max": 0,
        "python_hot_path_ms_max": 0
      }
    },
    {
      "id": "forge-selfplay-search-d3-32cpu-baseline",
      "lane": "baseline",
      "command": "cargo run --release -p ark_cli -- selfplay --games 2048 --actors 32 --search-depth 3 --seed 1729 --out /tmp/ark-v4-forge/selfplay-d3 --chunked --chunk-size 4096 --json",
      "target_profile": "server_32_vcpu",
      "targets": {
        "games_completed_min": 2048,
        "games_per_second_min": 12,
        "plies_per_second_min": 1500,
        "search_nodes_per_second_min": 60000000,
        "illegal_moves_max": 0,
        "timeouts_max": 0,
        "actor_crashes_max": 0,
        "cpu_utilization_percent_min": 85,
        "committed_artifacts_max": 0,
        "python_hot_path_ms_max": 0
      }
    },
    {
      "id": "forge-selfplay-endurance-30m-periodic",
      "lane": "periodic_eval",
      "command": "cargo run --release -p ark_cli -- selfplay --duration 30m --actors 32 --search-depth 2 --seed 1729 --out /tmp/ark-v4-forge/selfplay-endurance --chunked --chunk-size 4096 --json",
      "target_profile": "server_32_vcpu",
      "targets": {
        "games_per_second_min": 120,
        "illegal_moves_max": 0,
        "timeouts_max": 0,
        "actor_crashes_max": 0,
        "rss_growth_percent_max": 3,
        "cpu_utilization_percent_min": 85,
        "committed_artifacts_max": 0,
        "python_hot_path_ms_max": 0
      }
    }
  ],
  "unsupported_benchmarks": [
    {
      "id": "forge-search-startpos-d6-32cpu-baseline",
      "lane": "baseline",
      "command": "cargo run --release -p ark_cli -- search --fen \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\" --depth 6 --threads 32 --seed 1729 --json",
      "target_profile": "server_32_vcpu",
      "reason": "search_threads_above_one_not_supported",
      "supported_until": "search --threads 1",
      "enablement": "parallel_root_search",
      "targets": {
        "depth_completed_min": 6,
        "nodes_per_second_min": 160000000,
        "speedup_vs_single_thread_min": 10.0,
        "cpu_utilization_percent_min": 85,
        "rss_mb_max": 4096,
        "python_hot_path_ms_max": 0
      }
    }
  ]
}
```
<!-- ark-v4-forge-performance-contract:end -->

## Required CLI Output Examples

Search benchmark stdout:

```json
{
  "schema_version": "ark-v4-forge-bench-v1",
  "benchmark_id": "forge-search-startpos-d6-single-baseline",
  "status": "pass",
  "git_sha": "local-or-ci-sha",
  "target_profile": "server_32_vcpu_single_thread",
  "command": "cargo run --release -p ark_cli -- search --fen \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\" --depth 6 --threads 1 --seed 1729 --json",
  "seed": 1729,
  "move_order": "seed",
  "leaf_eval": "terminal",
  "checkpoint_loaded": false,
  "metrics": {
    "nodes": 78000000,
    "nodes_per_second": 13000000,
    "speedup_vs_single_thread": null,
    "legal_moves_generated": 94000000,
    "leaf_evals": 28000000,
    "terminal_leaf_evals": 1024,
    "neutral_frontier_evals": 27998976,
    "wdl_leaf_evals": 0,
    "external_leaf_eval_calls": 0,
    "non_terminal_static_eval_calls": 0,
    "model_ordered_root_moves": 0,
    "model_ordered_moves": 0,
    "tactical_extension_depth": 2,
    "tactical_extension_depth_reached": 2,
    "tactical_extension_nodes": 1200000,
    "tactical_extension_moves": 3600000,
    "transposition_table_probes": 41000000,
    "transposition_table_hit_rate": 0.31,
    "cutoffs": 18000000,
    "depth_completed": 6,
    "elapsed_ms": 6000,
    "threads": 1,
    "cpu_utilization_percent": null,
    "rss_mb": 256,
    "python_hot_path_ms": 0
  },
  "targets": {
    "depth_completed_min": 6,
    "nodes_per_second_min": 12000000,
    "terminal_leaf_evals_min": 1,
    "non_terminal_static_eval_calls_max": 0,
    "rss_mb_max": 512,
    "python_hot_path_ms_max": 0
  },
  "target_results": [
    {
      "target_name": "depth_completed_min",
      "metric": "depth_completed",
      "rule": "gte",
      "observed": 6,
      "target_value": 6,
      "passed": true
    },
    {
      "target_name": "nodes_per_second_min",
      "metric": "nodes_per_second",
      "rule": "gte",
      "observed": 13000000,
      "target_value": 12000000,
      "passed": true
    },
    {
      "target_name": "terminal_leaf_evals_min",
      "metric": "terminal_leaf_evals",
      "rule": "gte",
      "observed": 1024,
      "target_value": 1,
      "passed": true
    },
    {
      "target_name": "python_hot_path_ms_max",
      "metric": "python_hot_path_ms",
      "rule": "lte",
      "observed": 0,
      "target_value": 0,
      "passed": true
    }
  ],
  "failures": [],
  "confidence": {
    "level": 0.95,
    "repetitions": 5,
    "minimum_repetitions": 5,
    "complete": true,
    "ci95_lower": {
      "nodes_per_second": 12600000
    },
    "ci95_upper": {}
  },
  "artifacts": {
    "committed_artifacts": 0,
    "game_output_path": null
  }
}
```

Self-play benchmark stdout:

```json
{
  "schema_version": "ark-v4-forge-bench-v1",
  "benchmark_id": "forge-selfplay-search-d3-32cpu-baseline",
  "status": "pass",
  "git_sha": "local-or-ci-sha",
  "target_profile": "server_32_vcpu",
  "command": "cargo run --release -p ark_cli -- selfplay --games 2048 --actors 32 --search-depth 3 --seed 1729 --out /tmp/ark-v4-forge/selfplay-d3 --chunked --chunk-size 4096 --json",
  "seed": 1729,
  "move_order": "seed",
  "leaf_eval": "terminal",
  "checkpoint_loaded": false,
  "metrics": {
    "games_requested": 2048,
    "games_completed": 2048,
    "games_per_second": 14.2,
    "plies": 312400,
    "plies_per_second": 2168,
    "positions_emitted": 312400,
    "search_nodes": 10400000000,
    "search_nodes_per_second": 72000000,
    "neutral_frontier_evals": 3124000,
    "wdl_leaf_evals": 0,
    "illegal_moves": 0,
    "timeouts": 0,
    "actor_crashes": 0,
    "actor_count": 32,
    "p95_actor_skew": 1.18,
    "cpu_utilization_percent": 89,
    "rss_mb": 3072,
    "rss_growth_percent": 0.7,
    "committed_artifacts": 0,
    "python_hot_path_ms": 0
  },
  "targets": {
    "games_per_second_min": 12,
    "plies_per_second_min": 1500,
    "search_nodes_per_second_min": 60000000,
    "illegal_moves_max": 0,
    "committed_artifacts_max": 0,
    "python_hot_path_ms_max": 0
  },
  "target_results": [
    {
      "target_name": "games_per_second_min",
      "metric": "games_per_second",
      "rule": "gte",
      "observed": 14.2,
      "target_value": 12,
      "passed": true
    },
    {
      "target_name": "search_nodes_per_second_min",
      "metric": "search_nodes_per_second",
      "rule": "gte",
      "observed": 72000000,
      "target_value": 60000000,
      "passed": true
    },
    {
      "target_name": "committed_artifacts_max",
      "metric": "committed_artifacts",
      "rule": "lte",
      "observed": 0,
      "target_value": 0,
      "passed": true
    }
  ],
  "failures": [],
  "confidence": {
    "level": 0.95,
    "repetitions": 5,
    "minimum_repetitions": 5,
    "complete": true,
    "ci95_lower": {
      "games_per_second": 13.1,
      "search_nodes_per_second": 69000000
    },
    "ci95_upper": {}
  },
  "artifacts": {
    "committed_artifacts": 0,
    "game_output_path": "/tmp/ark-v4-forge/selfplay-d3"
  }
}
```

## Implementation Requirements

- The Rust benchmark binary owns timing. External shells or Python wrappers do not measure elapsed
  time for pass/fail.
- `status` is computed from `targets` and `target_results`; it is never a literal success string
  emitted before target evaluation.
- A benchmark cannot pass with an empty `targets` object, a missing observed metric for a target, or
  a failed target comparison.
- Stage 0 CLI output uses one measured repetition and sets `confidence.complete=false` with null CI
  bounds. Full SLA acceptance still requires the periodic runner to aggregate at least five
  repetitions and fill the 95 percent confidence bounds.
- Perft benchmarks must validate exact node counts before reporting throughput.
- Ad-hoc perft JSON must provide `--correct-nodes` or match one of the locked contract positions;
  otherwise it fails with `missing_targets`.
- Search benchmarks must report both raw search nodes and legal moves generated so movegen and search
  regressions are separable.
- `search --threads > 1` is rejected until parallel root search exists. The 32-thread search
  target remains documented under `unsupported_benchmarks`, not as an active runnable benchmark.
- Terminal leaf eval is the default baseline: non-terminal depth leaves return the neutral bootstrap
  value without a handcrafted evaluator and without a Python callback. WDL leaf eval is allowed only
  when a checkpoint is explicitly supplied.
- Self-play actor scheduling must live in Rust. A coordinator may write JSONL games, but games are
  performance outputs, not source files.
- The first 32-vCPU self-play target is accepted only when actor throughput and search-node
  throughput are both present, because aggregate throughput without search evidence hides contention.
- The deferred 32-thread search target must not move into active benchmarks until `ark search`
  reports measured `speedup_vs_single_thread`, CPU use, and RSS for `--threads 32`.
- A failed target must print the observed metric, target metric, confidence bound, and benchmark id.

## Failure Modes

- Correct perft with throughput below target: core is correct but not accepted for V4 Forge.
- Fast self-play with any illegal move: run fails and the game writer output is not trusted.
- Search reports non-terminal static eval calls above zero: run fails the no-handcrafted-static-eval
  constraint.
- Benchmark stdout is human text or multiple mixed records: run fails the CLI contract.
- Benchmark stdout reports `status=pass` with empty targets, missing `target_results`, or an
  unmeasured metric used by a target: run fails the CLI contract.
- Any command needs a Python process for movegen, search, actor scheduling, or game writing: run fails.
