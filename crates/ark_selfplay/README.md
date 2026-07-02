# ark_selfplay

Stage 3 deterministic streaming self-play runtime for Ark V4.

The integration point is:

```rust
use ark_selfplay::{run_selfplay_streaming, CompletedGame, StreamingSelfPlayConfig};

let config = StreamingSelfPlayConfig {
    games: 128,
    search_depth: 1,
    tactical_extension_depth: 2,
    max_plies: 256,
    actors: 4,
    run_seed: 1,
    result_channel_bound: 16,
};

let summary = run_selfplay_streaming(&config, &mut |game: &CompletedGame| {
    // Stream game.record into a replay writer here.
    // The runtime does not store all games in one Vec.
    Ok(())
})?;
```

Actor assignment is `actor_for_game_id(game_id, actors)`, so scheduling is derived from the
game id. Search seeds are derived from `run_seed`, `game_id`, and `ply`; actor id is not part of
the seed path. The result channel uses `std::sync::mpsc::sync_channel` for bounded backpressure.

`run_selfplay(SelfPlayConfig)` remains available as a chunk-writer adapter around the same legal
move generation path. It writes sealed `.arkchunk` files under `<out_dir>/chunks/`.
