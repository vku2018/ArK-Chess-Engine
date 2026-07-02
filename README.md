
<h1 align="center">ArK</h1>

<p align="center">
  A Rust-native chess engine project exploring compact neural search, deterministic self-play, and measurable engine development.
</p>

<p align="center">
  <a href="https://github.com/bitlical/ArK-Chess-Engine/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/bitlical/ArK-Chess-Engine/actions/workflows/ci.yml/badge.svg"></a>
  <img alt="Rust 2021" src="https://img.shields.io/badge/Rust-2021-f74c00?logo=rust&logoColor=white">
  <img alt="Unsafe forbidden" src="https://img.shields.io/badge/unsafe-forbidden-2ea44f">
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
  <img alt="Status: new project" src="https://img.shields.io/badge/status-new%20project-6f42c1">
</p>

## What Is ArK?

ArK is an early-stage chess engine written in Rust. The active codebase is a fresh rebuild around
legal move correctness, search traceability, compact replay files, and repeatable self-play runs.

The project is intentionally not claiming rating strength yet. Elo claims belong behind fixed
opponent pools, paired openings, confidence intervals, and reproducible rating logs.

ArK is looking for active contributors interested in chess rules, engine search, Rust performance,
training data formats, command-line tooling, testing, and documentation.

## What Works Today

| Area | Current state |
| --- | --- |
| Rules core | FEN parsing, move generation, perft, game state, and rule tests in `crates/ark_core`. |
| Search | Depth-limited search contracts with JSON output and trace checks. |
| Replay | Chunked self-play storage with hashing, validation, and manifests in `crates/ark_replay`. |
| Model contracts | Board encoding, model-head structs, checkpoints, and trainer smoke tests in `crates/ark_model`. |
| Self-play | Deterministic self-play generation and replay writing in `crates/ark_selfplay`. |
| CLI | `perft`, `search`, `selfplay`, `train`, `eval`, `bench`, and `uci` entry points in `crates/ark_cli`. |

## Quick Start

Run from the repository root in PowerShell:

```powershell
.\scripts\bootstrap-rust.ps1
.\scripts\cargo-local.ps1 build --release -p ark_cli
```

`bootstrap-rust.ps1` installs the pinned Rust toolchain under `.tools` for this repository. It does
not require a global Rust install.

## CLI Examples

```powershell
.\scripts\cargo-local.ps1 run -p ark_cli -- perft --fen "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1" --depth 4 --json
.\scripts\cargo-local.ps1 run -p ark_cli -- search --fen "7k/6Q1/5K2/8/8/8/8/8 w - - 0 1" --depth 1 --json
.\scripts\cargo-local.ps1 run -p ark_cli -- selfplay --config configs/selfplay/baseline.toml --json
.\scripts\cargo-local.ps1 run -p ark_cli -- train --config configs/train/v4.toml --json
.\scripts\cargo-local.ps1 run -p ark_cli -- uci
```

Generated runs, replay chunks, model checkpoints, logs, and reports are ignored by Git. Do not
commit `.arkgames`, `.arkmodel`, `runs/`, `models/`, `logs/`, or `reports/`.

## Repository Map

```text
crates/
  ark_core      Rules, board state, move generation, perft, and search
  ark_replay    Compact replay chunks, hashing, validation, and manifests
  ark_model     Board encoding, model-head contracts, checkpoints, trainer smoke
  ark_selfplay  Deterministic self-play generation and replay writing
  ark_cli       CLI, UCI, train, eval, self-play, and benchmark entry points
configs/        Example self-play and training configs
docs/           Architecture and performance notes
tests/          PowerShell guard tests for repo health
```

Older engine generations are not part of the active tree. V4 code should stand on the Rust crates
listed above.

## Validation

```powershell
.\scripts\cargo-local.ps1 test
.\scripts\cargo-local.ps1 clippy --all-targets '--' '-D' 'warnings'
.\tests\active_tree_guard.ps1
.\tests\perf_contract_guard.ps1
.\tests\dependency_guard.ps1
.\tests\codex_review_gate.ps1
```

The gate checks cover Rust tests, Clippy, active-tree hygiene, performance-contract docs, dependency
boundaries, and the required Codex review signal.

## Good First Contributions

Strong starter issues usually touch one narrow path and leave a clear measurement behind:

- Add or tighten a perft or legality regression case.
- Improve search trace output without changing search semantics.
- Reduce allocation in self-play or replay writing, with a benchmark before and after.
- Add a CLI smoke test for an edge case.
- Improve docs where a command, invariant, or file format is unclear.

Before opening a PR, read [CONTRIBUTING.md](CONTRIBUTING.md). Every PR should include the validation
commands that were run and request a Codex connector review on the latest commit.

## Project Direction

ArK prioritizes correctness first, then throughput, then playing strength. The engine should never
trade legal move correctness, reproducible replay, or honest evaluation for a faster demo.

Near-term work is focused on:

- Wider rules coverage and perft confidence.
- Faster deterministic self-play.
- Better replay and benchmark reporting.
- Search-native model contracts that can be trained and evaluated cleanly.
- UCI stability for GUI and tournament use.

## Community

- Use [Issues](https://github.com/bitlical/ArK-Chess-Engine/issues) for confirmed bugs, scoped features, and performance tasks.
- Use [Discussions](https://github.com/bitlical/ArK-Chess-Engine/discussions) for questions, architecture tradeoffs, and open ideas.
- Use [Security](SECURITY.md) for vulnerability reporting.

## License

ArK is licensed under the [MIT License](LICENSE).
