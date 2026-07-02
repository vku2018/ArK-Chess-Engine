# ArK-V4 Forge Architecture

ArK-V4 Forge is a Rust-first engine architecture. The active codebase is independent from older
prototype implementations.

## Locked Decisions

- Rules and search are Rust-native from day one.
- The chess core is custom bitboards and must pass perft and oracle tests before training work.
- Search is classical negamax/PVS/alpha-beta first, with neural policy/value integration behind
  explicit replay/checkpoint contracts.
- The initial learning signal is games-only self-play.
- Leaf eval is terminal-only by default. Checkpointed WDL leaf eval is explicit and requires
  `--leaf-eval wdl --checkpoint <model>`.
- Stockfish is only an evaluator and later hard-negative source after a legal self-play baseline.
- Rust tooling is managed through the repository wrapper scripts.

## Build Order

1. Active-tree guard and Rust workspace.
2. `ark_core` FEN, legal moves, make/unmake, perft, terminal detection, search trace.
3. `ark` CLI: `perft`, `search`, `uci`.
4. Gate tests and contamination checks.
5. Self-play baseline runner with deterministic actor assignment and compact `.arkchunk` replay.
6. ArKNet-V4 Forge checkpoint lane with board encoder, legal-policy mask, WDL, moves-left,
   uncertainty, risk, and refutation heads.
7. Training and eval smoke gates before any long training run.

## First Milestone

The first success claim is not Elo. It is a legal self-play baseline:

- 100,000 self-play games.
- Zero illegal moves.
- Zero unhandled terminal states.
- Reproducible manifest and throughput report.
