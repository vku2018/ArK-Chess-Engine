# Ark

Ark is a Rust chess engine research project focused on a compact neural-search stack. The current
codebase includes a rules engine, search contracts, chunked self-play replay, model checkpointing,
and a command-line interface for training and evaluation experiments.

## Status

Ark is under active development. The engine is not rated, and Elo claims are out of scope until the
rating harness, opponent pool, openings, and confidence gates are fixed.

## Architecture

- `crates/ark_core`: board representation, FEN, move generation, perft, game state, and search.
- `crates/ark_replay`: chunked replay storage, validation, hashing, and manifest handling.
- `crates/ark_model`: board encoding, model heads, checkpoint format, and trainer smoke tests.
- `crates/ark_selfplay`: deterministic self-play generation and replay writing.
- `crates/ark_cli`: `ark` CLI, UCI entry point, self-play, train, eval, and benchmark commands.
- `configs/`: example self-play and training configurations.
- `docs/`: architecture notes and benchmark contracts.

## Build

```powershell
.\scripts\bootstrap-rust.ps1
.\scripts\cargo-local.ps1 build --release -p ark_cli
```

The bootstrap script installs the pinned Rust toolchain under `.tools`. Run it once before using
the Cargo wrapper.

## Test

```powershell
.\scripts\cargo-local.ps1 test
.\scripts\cargo-local.ps1 clippy --all-targets '--' '-D' 'warnings'
.\tests\active_tree_guard.ps1
.\tests\perf_contract_guard.ps1
.\tests\dependency_guard.ps1
```

## CLI Examples

```powershell
.\scripts\cargo-local.ps1 run -p ark_cli -- perft --fen "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1" --depth 4 --json
.\scripts\cargo-local.ps1 run -p ark_cli -- search --fen "7k/6Q1/5K2/8/8/8/8/8 w - - 0 1" --depth 1 --json
.\scripts\cargo-local.ps1 run -p ark_cli -- selfplay --config configs/selfplay/baseline.toml --json
.\scripts\cargo-local.ps1 run -p ark_cli -- train --config configs/train/v4.toml --json
.\scripts\cargo-local.ps1 run -p ark_cli -- uci
```

Self-play and training outputs are written to ignored paths such as `runs/` and `models/`.
Generated replay chunks, `.arkgames` files, and `.arkmodel` checkpoints should not be committed.

## Project Direction

Ark prioritizes correctness before playing strength. New search, self-play, and model changes must
preserve legal move generation, deterministic replay, and explicit benchmark output. See
`CONTRIBUTING.md` for contribution standards.

## Community

- Use issues for confirmed bugs, scoped features, and measurable performance tasks.
- Use discussions for questions, architecture tradeoffs, and open-ended ideas.
- Report security concerns through `SECURITY.md`.

## License

Ark is licensed under the MIT License.
