# Replay Chunk Format

This note is for contributors changing self-play storage, replay validation, or replay readers.
Start with these files:

- `crates/ark_replay/src/lib.rs` owns the `.arkchunk` format, chunk manifests, atomic writes,
  readback validation, and manifest rebuilding.
- `crates/ark_replay/tests/chunks.rs` is the main replay chunk test suite. Read it before changing
  the format or validation behavior.
- `crates/ark_selfplay/src/lib.rs` streams completed games into chunk writers.
- `crates/ark_selfplay/README.md` documents deterministic actor assignment and streaming self-play.
- `crates/ark_cli/tests/selfplay_smoke.rs` covers CLI-level chunked self-play output.

## Files On Disk

Chunked self-play writes sealed replay chunks under:

```text
<out_dir>/chunks/<chunk_id>.arkchunk
```

Each chunk is first written to a temporary path ending in `.tmp`, flushed, synced, and then renamed
into the sealed `.arkchunk` path. A sealed chunk must be treated as immutable. If a sealed file with
the same chunk id already exists, the writer may only accept it when both the manifest and decoded
games match exactly.

Legacy `.arkgames` files are still readable for compatibility, but generated `.arkgames`,
`.arkchunk`, run directories, model checkpoints, logs, and reports are outputs. Do not commit them.

## Chunk Contents

The chunk header is the manifest for that sealed file. It contains:

- magic bytes and schema version
- deterministic `chunk_id`
- `first_game_index`
- game count and ply count
- white win, draw, and black win counts
- validation counters for illegal moves and unhandled terminal states
- content hash
- decoded body byte count
- sealed file byte count

The body is a compact sequence of games. Each game stores the result byte, a `u16` ply count, and
the packed `u16` moves. Readers must reject chunks when the sealed byte count, decoded body length,
content hash, validation summary, or rebuilt manifest do not match. Directory replay readers also
reject chunks whose filenames do not match the manifest `chunk_id`.

## Manifest And Ordering

`ChunkManifest` describes one sealed chunk. `ReplayManifest` is rebuilt from a chunk directory by
reading every `.arkchunk`, sorting chunks by `first_game_index` and then `chunk_id`, and aggregating:

- total games
- total plies
- white wins, draws, and black wins
- total sealed bytes

`read_chunked_replay_dir` also sorts by `first_game_index`, but it requires chunks to cover a
contiguous game-index range starting at zero. Missing indexes and overlaps are errors because they
would make training input depend on directory listing order or partial output.

## Determinism Rules

These details must stay deterministic:

- `deterministic_chunk_id(first_game_index, games)` is derived from the first game index, game count,
  and game content hash.
- Self-play actor assignment is derived from `game_id`, and search seeds are derived from
  `run_seed`, `game_id`, and `ply`.
- Chunk writes validate game records before writing and verify the sealed file by reading it back.
- Chunk filenames must match the manifest `chunk_id`.
- Readers must ignore non-chunk files but reject corrupt, truncated, mismatched, empty, gap, and
  overlap cases.

When changing this area, run at least:

```sh
cargo test -p ark_replay --locked
```

For CLI or self-play integration changes, also run the relevant `ark_cli` or `ark_selfplay` tests.
