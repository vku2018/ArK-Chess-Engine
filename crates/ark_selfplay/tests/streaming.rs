use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ark_core::GameOutcome;
use ark_model::{game_from_uci, validate_replay, ForgeModel};
use ark_replay::rebuild_manifest;
use ark_selfplay::{
    actor_for_game_id, play_one_game, run_selfplay, run_selfplay_streaming, seed_for_ply,
    CompletedGame, LeafEvalMode, SelfPlayConfig, SelfPlayError, StreamingSelfPlayConfig,
};

#[test]
fn streaming_selfplay_writes_verified_chunks() -> Result<(), String> {
    let dir = temp_dir("ark-selfplay-streaming")?;
    let config = SelfPlayConfig {
        games: 6,
        search_depth: 1,
        tactical_extension_depth: 2,
        max_plies: 8,
        actors: 3,
        seed: 1729,
        chunk_size: 2,
        out_dir: dir.clone(),
        model: None,
        leaf_eval: LeafEvalMode::Terminal,
    };

    let summary = run_selfplay(config)?;
    assert_eq!(summary.games_requested, 6);
    assert_eq!(summary.games_completed, 6);
    assert_eq!(summary.actor_count, 3);
    assert_eq!(summary.actor_crashes, 0);
    assert_eq!(summary.illegal_moves, 0);
    assert_eq!(summary.unhandled_terminal_states, 0);
    assert_eq!(summary.chunks_published, 3);
    assert_eq!(summary.wdl_leaf_evals, 0);
    assert!(summary.neutral_frontier_evals > 0);
    assert!(summary.plies > 0);
    assert!(summary.p95_actor_skew >= 1.0);

    let manifest = rebuild_manifest(&dir.join("chunks"))?;
    assert_eq!(manifest.games, 6);
    assert_eq!(manifest.chunks.len(), 3);
    assert_eq!(manifest.chunks[0].first_game_index, 0);
    assert_eq!(manifest.chunks[1].first_game_index, 2);
    assert_eq!(manifest.chunks[2].first_game_index, 4);

    cleanup_dir(&dir)
}

#[test]
fn game_seed_does_not_depend_on_actor_count() -> Result<(), String> {
    let dir = temp_dir("ark-selfplay-seed")?;
    let mut config_a = SelfPlayConfig {
        games: 4,
        search_depth: 1,
        tactical_extension_depth: 2,
        max_plies: 6,
        actors: 1,
        seed: 99,
        chunk_size: 2,
        out_dir: dir.join("a"),
        model: None,
        leaf_eval: LeafEvalMode::Terminal,
    };
    let mut config_b = config_a.clone();
    config_b.actors = 4;
    config_b.out_dir = dir.join("b");

    for game_index in 0..config_a.games {
        let (a, _) = play_one_game(game_index, &config_a)?;
        let (b, _) = play_one_game(game_index, &config_b)?;
        assert_eq!(a, b);
    }
    config_a.actors = 2;
    let (a, _) = play_one_game(2, &config_a)?;
    let (b, _) = play_one_game(2, &config_b)?;
    assert_eq!(a, b);

    cleanup_dir(&dir)
}

#[test]
fn model_ordered_selfplay_uses_trained_policy_for_first_move() -> Result<(), String> {
    let dir = temp_dir("ark-selfplay-model-order")?;
    let mut model = ForgeModel::default();
    let training_game = game_from_uci(
        GameOutcome::WhiteWin,
        &["e2e4".to_string(), "e7e5".to_string()],
    )?;
    model.train_games(&[training_game], 4, 0.25);
    let preferred = ark_core::Position::startpos()
        .map_err(|err| format!("{err:?}"))?
        .move_from_uci("e2e4")
        .ok_or_else(|| "missing e2e4".to_string())?
        .packed_id();
    let config = SelfPlayConfig {
        games: 1,
        search_depth: 1,
        tactical_extension_depth: 0,
        max_plies: 1,
        actors: 1,
        seed: 7,
        chunk_size: 1,
        out_dir: dir.clone(),
        model: Some(model),
        leaf_eval: LeafEvalMode::Terminal,
    };

    let (record, _nodes) = play_one_game(0, &config)?;

    assert_eq!(record.moves, vec![preferred]);
    cleanup_dir(&dir)
}

#[test]
fn wdl_leaf_eval_selfplay_uses_model_frontier() -> Result<(), String> {
    let dir = temp_dir("ark-selfplay-wdl-leaf")?;
    let config = SelfPlayConfig {
        games: 2,
        search_depth: 1,
        tactical_extension_depth: 0,
        max_plies: 4,
        actors: 1,
        seed: 11,
        chunk_size: 1,
        out_dir: dir.clone(),
        model: Some(ForgeModel::default()),
        leaf_eval: LeafEvalMode::Wdl,
    };

    let summary = run_selfplay(config)?;

    assert_eq!(summary.games_completed, 2);
    assert_eq!(summary.illegal_moves, 0);
    assert_eq!(summary.unhandled_terminal_states, 0);
    assert_eq!(summary.neutral_frontier_evals, 0);
    assert!(summary.wdl_leaf_evals > 0);
    cleanup_dir(&dir)
}

#[test]
fn actor_assignment_and_ply_seed_are_deterministic() {
    let assignments: Vec<u32> = (0..8)
        .map(|game_id| actor_for_game_id(game_id, 3))
        .collect();
    assert_eq!(assignments, vec![0, 1, 2, 0, 1, 2, 0, 1]);

    let seed = seed_for_ply(7, 3, 2);
    assert_eq!(seed, seed_for_ply(7, 3, 2));
    assert_ne!(seed, seed_for_ply(8, 3, 2));
    assert_ne!(seed, seed_for_ply(7, 4, 2));
    assert_ne!(seed, seed_for_ply(7, 3, 3));
}

#[test]
fn callback_runtime_streams_games_with_actor_counters() -> Result<(), SelfPlayError> {
    let config = callback_config(3);
    let mut seen = Vec::new();
    let summary = run_selfplay_streaming(&config, &mut |game: &CompletedGame| {
        seen.push((game.game_id, game.actor_id, game.plies));
        Ok(())
    })?;

    assert_eq!(summary.games_requested, config.games);
    assert_eq!(summary.games_completed, config.games);
    assert_eq!(summary.actor_count, config.actors);
    assert_eq!(summary.result_channel_bound, 1);
    assert_eq!(summary.illegal_moves, 0);
    assert_eq!(summary.unhandled_terminal_states, 0);
    assert_eq!(summary.actor_crashes, 0);
    assert_eq!(summary.actor_counters.len(), config.actors as usize);
    assert_eq!(summary.actor_skew.game_skew, 0);
    assert_eq!(seen.len(), config.games as usize);
    for (expected_game_id, (game_id, actor_id, plies)) in (0_u32..).zip(seen) {
        assert_eq!(game_id, expected_game_id);
        assert_eq!(actor_id, actor_for_game_id(game_id, config.actors));
        assert!(plies <= config.max_plies);
    }
    Ok(())
}

#[test]
fn callback_runtime_records_validate_and_ignore_actor_count() -> Result<(), SelfPlayError> {
    let one_actor = callback_config(1);
    let three_actors = callback_config(3);
    let mut serial = Vec::new();
    let mut parallel = Vec::new();

    run_selfplay_streaming(&one_actor, &mut |game: &CompletedGame| {
        serial.push((game.game_id, game.record.clone()));
        Ok(())
    })?;
    run_selfplay_streaming(&three_actors, &mut |game: &CompletedGame| {
        parallel.push((game.game_id, game.record.clone()));
        Ok(())
    })?;

    assert_eq!(serial, parallel);
    let records: Vec<_> = serial
        .into_iter()
        .map(|(_game_id, record)| record)
        .collect();
    let validation = validate_replay(&records).map_err(SelfPlayError::Sink)?;
    assert_eq!(validation.games, three_actors.games);
    assert_eq!(validation.illegal_moves, 0);
    assert_eq!(validation.unhandled_terminal_states, 0);
    Ok(())
}

#[test]
fn callback_runtime_rejects_empty_result_channel() {
    let mut config = callback_config(1);
    config.result_channel_bound = 0;
    let mut calls = 0_u32;
    let err = run_selfplay_streaming(&config, &mut |_game: &CompletedGame| {
        calls = calls.saturating_add(1);
        Ok(())
    });

    assert!(matches!(err, Err(SelfPlayError::InvalidConfig(_))));
    assert_eq!(calls, 0);
}

fn temp_dir(name: &str) -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()));
    if dir.exists() {
        cleanup_dir(&dir)?;
    }
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir)
}

fn callback_config(actors: u32) -> StreamingSelfPlayConfig {
    StreamingSelfPlayConfig {
        games: 6,
        search_depth: 1,
        tactical_extension_depth: 2,
        max_plies: 6,
        actors,
        run_seed: 42,
        result_channel_bound: 1,
        leaf_eval: LeafEvalMode::Terminal,
        model: None,
    }
}

fn cleanup_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|err| err.to_string())?;
    }
    Ok(())
}
