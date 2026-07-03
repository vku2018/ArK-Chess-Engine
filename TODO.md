# ArK Roadmap

ArK is an early-stage Rust chess engine. This roadmap tracks the work needed to move from a legal
V4 baseline to repeatable self-play, measurable search performance, and trustworthy training runs.

Use this file to pick issues, split PRs, and check whether a change has enough evidence to merge.
Prefer small PRs with one measurable outcome.

## Contribution Rules

- Keep active engine code in Rust.
- Keep Python out of move generation, search, actor scheduling, and game writing.
- Do not commit replay chunks, model checkpoints, logs, reports, or local run outputs.
- Do not make Elo or strength claims until fixed rating pools, paired openings, time controls,
  confidence intervals, and reproducible logs exist.
- Include tests or evals with every behavior change.

## P0: Rules Core

The engine needs a rule core that contributors can trust before search or training work scales.

- [x] Tighten FEN validation in `ark_core::Position::from_fen`.
  - Acceptance: reject rank digit `0`, multiple kings, pawns on first or eighth rank, invalid en
    passant squares, impossible castling rights, and positions where kings attack each other.
  - Done: PR #7 added strict validation, rule tests, and `tests/fen_contract_guard.ps1`.
- [ ] Expand the perft oracle suite.
  - Acceptance: fixture coverage includes startpos, Kiwipete, en passant pins, promotions, castling,
    discovered check, double check, pinned pieces, and stalemate-adjacent positions.
- [ ] Add `perft --divide`.
  - Acceptance: CLI prints each root move and node count in deterministic UCI order; the sum matches
    normal `perft`.
- [ ] Add draw-state search support.
  - Acceptance: search handles 50-move and repetition state, with tests where draw availability
    changes the preferred move.
- [ ] Add mate-distance scoring.
  - Acceptance: search prefers mate in 1 over mate in 2, delays forced mate when losing, and keeps
    terminal-leaf trace counters accurate.

## P1: Search And UCI

Search should expose clear contracts, truthful traces, and stable protocol behavior.

- [x] Remove duplicate legal move generation from terminal and search paths.
  - Acceptance: self-play and search share root legal moves where possible; trace output includes
    movegen call counts; fixed-seed replay hashes stay unchanged for representative runs.
  - Done: PR #11 added supplied-root search and movegen call counters.
- [x] Make `search --threads` real or reject it.
  - Acceptance: `--threads 32` either uses deterministic parallel search with measured speedup and
    CPU metrics, or exits with a clear unsupported-option error.
  - Done: PR #11 rejects unsupported search threads above 1.
- [x] Replace hand-built JSON output with typed writers.
  - Acceptance: search, self-play, train, eval, and benchmark JSON use one tested serialization path;
    tests parse stdout as JSON.
  - Done: PR #11 added typed JSON reports and structural JSON tests.
- [x] Build a protocol-level UCI harness.
  - Acceptance: tests cover `uci`, `isready`, `ucinewgame`, `position fen`, `position startpos moves`,
    malformed moves, `go depth`, `go nodes`, `go movetime`, `stop`, and `quit`.
  - Done: PR #11 added the UCI harness and protocol smoke tests.
- [x] Implement async UCI cancellation.
  - Acceptance: `go infinite` starts search, `stop` returns a legal `bestmove` within a tested
    latency bound, and `quit` cannot hang during active search.
  - Done: PR #11 added cancellable UCI search and stop/quit coverage.
- [x] Add UCI clock management.
  - Acceptance: `go wtime btime winc binc movestogo` computes a bounded move budget and records it
    in trace output.
  - Done: PR #11 added UCI budget selection and trace coverage.

## P2: Replay And Self-Play

Self-play data must be legal, deterministic, auditable, and honest about incomplete games.

- [ ] Route all CLI self-play through `ark_selfplay`.
  - Acceptance: remove the duplicate CLI-local actor loop; CLI config maps into
    `ark_selfplay::SelfPlayConfig`; legacy `.arkgames` and chunked outputs still validate.
- [ ] Add run-level replay metadata.
  - Acceptance: each run records config, seed, search depth, tactical extension depth, checkpoint
    hash, git SHA, actor count, chunk size, chunk list, and manifest hash.
- [ ] Add a replay audit command.
  - Acceptance: report result distribution, ply histogram, cap-hit games, illegal moves, terminal
    states, duplicate game indexes, chunk hashes, and manifest validity.
- [ ] Record capped games explicitly.
  - Acceptance: games stopped by `max_plies` use a distinct terminal reason so training and eval code
    can filter or weight them.
- [ ] Add resumable chunked self-play.
  - Acceptance: interrupted runs resume without duplicate or missing games and preserve deterministic
    chunk hashes.
- [ ] Land the first legal self-play milestone.
  - Acceptance: 100,000 games, zero illegal moves, zero unhandled terminal states, reproducible
    manifest, and a throughput report.

## P3: Benchmarking And Throughput

Performance work should speed up the same engine behavior, not a reduced-quality benchmark path.

- [x] Close the benchmark contract gaps.
  - Acceptance: documented gate commands run from the repo root, emit one JSON object, and fail when
    required target metrics are missing.
  - Done: PR #11 added benchmark contract evaluation and `tests/perf_contract_guard.ps1`.
- [ ] Implement `selfplay --duration`.
  - Acceptance: duration-mode self-play exits after the requested window, completes at least one game
    in smoke tests, and reports elapsed and requested duration fields.
- [ ] Fill runtime metrics.
  - Acceptance: `cpu_utilization_percent`, `rss_mb`, `rss_growth_percent`,
    `speedup_vs_single_thread`, repetitions, and CI95 fields are measured or marked as failing target
    results when unavailable.
- [ ] Add a Rust repeat runner for periodic benchmarks.
  - Acceptance: runs each selected benchmark at least five times and emits CI95 lower bounds for
    throughput plus CI95 upper bounds for memory and latency.
- [ ] Remove hot-path cloning and allocation churn.
  - Acceptance: `GameState::make_move` uses the in-place move path where safe; perft, search, and
    self-play tests pass; fixed-seed replay equality holds.
- [ ] Track actor scaling.
  - Acceptance: self-play reports actor count, actor skew, pending writer pressure, CPU utilization,
    and search nodes per second.
- [ ] Protect normal training semantics.
  - Acceptance: smoke-only runs may use tiny counts or low `max_plies`, but benchmark and training
    lanes keep full-game semantics and label capped games.

## P4: Model And Training Contracts

The current model and trainer are smoke contracts. Long runs need stronger data and eval gates first.

- [ ] Document the model contract.
  - Acceptance: specify board encoding, legal policy mask, WDL orientation, moves-left, uncertainty,
    risk, refutation, value ranges, checkpoint version, and migration rules.
- [ ] Add golden model fixtures.
  - Acceptance: fixed positions verify checkpoint load, legal move masking, WDL probabilities, and
    stable output.
- [ ] Move replay record ownership out of `ark_model`.
  - Acceptance: replay records and validation live in `ark_replay` or a small shared records crate;
    `ark_model` consumes records without owning storage contracts.
- [ ] Define training targets per head.
  - Acceptance: policy, WDL, moves-left, uncertainty, risk, and refutation targets have documented
    ranges and skip rules.
- [ ] Add deterministic training state.
  - Acceptance: shuffling, batch sizing, train/holdout split, resume state, optimizer state, and
    checkpoint metadata are reproducible.
- [ ] Emit training metrics.
  - Acceptance: train JSON reports policy loss, WDL loss, moves-left error, risk/refutation metrics,
    games seen, plies seen, skipped capped games, and checkpoint path.
- [ ] Add tiny-overfit and holdout gates.
  - Acceptance: the trainer can overfit a tiny replay fixture and must pass a holdout replay eval
    before long runs.

## P5: Evaluation

Eval gates decide whether a checkpoint or search change can move forward.

- [ ] Expand `eval baseline` into named suites.
  - Acceptance: suites cover rules/perft, terminal leaf behavior, search JSON contract, replay
    consistency, policy-order sanity, WDL calibration smoke, and fixed-seed self-play equality.
- [ ] Add a periodic tactical suite.
  - Acceptance: fixed FEN pack reports mate misses, blunders, legal move failures, draw handling, and
    node/time budget compliance as JSON.
- [ ] Add optional Stockfish oracle evals behind `STOCKFISH_PATH`.
  - Acceptance: no install is required; eval output records Stockfish path, version, skill level,
    openings, command, and license note.
- [ ] Build opponent harness after the legal self-play baseline.
  - Acceptance: fixed openings, time controls, opponent versions, confidence intervals, and report
    schema are documented before strength claims.
- [ ] Promote checkpoints by eval evidence.
  - Acceptance: a checkpoint promotion requires replay audit pass, fixed-seed expectations, benchmark
    gates, and named eval-suite results.

## P6: Fixtures, CI, And Contributor Workflow

Contributors need one clear path from local change to reviewable PR.

- [ ] Add fixture directories for positions, replay text, benchmarks, and oracles.
  - Acceptance: fixture guards reject duplicate IDs, malformed FEN, missing expected fields, and
    generated binary artifacts.
- [ ] Add `tests/forge_gate_benchmarks.ps1`.
  - Acceptance: startpos D4, Kiwipete D4, and terminal leaf suite run in release mode and emit
    `status:"pass"` with expected benchmark IDs.
- [ ] Add `tests/local_preflight.ps1`.
  - Acceptance: one command runs Rust tests, Clippy, guard scripts, and gate benchmarks.
- [ ] Add periodic eval workflow.
  - Acceptance: `workflow_dispatch` and scheduled runs upload JSONL reports without blocking normal
    PRs.
- [ ] Extend active-tree guard for generated artifacts.
  - Acceptance: tracked `.arkgames`, `.arkmodel`, `runs/*`, `models/*`, `logs/*`, and `reports/*`
    fail the guard.
- [ ] Update PR template for evidence.
  - Acceptance: PRs ask for local preflight output, gate benchmark result, eval report path when
    relevant, fixture/oracle updates, and known gaps.

## Good First Issues

- Add one documented perft fixture with expected counts.
- Add a bad-FEN regression test.
- Add a search fixture for mate, stalemate, or draw handling.
- Add a replay manifest example to the docs.
- Add `--help` output for one CLI command.
- Add a fixture guard for duplicate IDs.
- Improve an error message that currently lacks the command or benchmark ID.

## Out Of Scope For Now

- Rating claims without a frozen rating harness.
- Python in engine hot paths.
- Committed model weights or generated training data.
- Reviving archived prototype code in the active engine path.
- Silent quality cuts that make self-play faster by changing normal game semantics.
