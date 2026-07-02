mod common;

use std::process::Command;

use ark_model::{read_replay, validate_replay};
use ark_replay::rebuild_manifest;
use common::parse_single_json;

#[test]
fn selfplay_writes_legal_games_only_artifact() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::temp_dir().join("ark-v4-selfplay-smoke.arkgames");
    let _ = std::fs::remove_file(&out);
    let output = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "selfplay",
            "--games",
            "2",
            "--search-depth",
            "1",
            "--max-plies",
            "8",
            "--seed",
            "42",
            "--out",
            out.to_str().ok_or("temp path is not utf8")?,
            "--json",
        ])
        .output()?;
    assert!(output.status.success());
    let json = parse_single_json(output.stdout)?;
    assert_eq!(json["schema_version"], "ark-v4-forge-bench-v1");
    assert_eq!(json["benchmark_id"], "forge-selfplay-adhoc");
    assert_eq!(json["status"], "pass");
    assert_eq!(json["metrics"]["games_completed"], 2);
    assert_eq!(json["metrics"]["illegal_moves"], 0);
    assert_eq!(json["metrics"]["unhandled_terminal_states"], 0);
    assert!(json["metrics"]["root_movegen_calls"].as_u64().is_some());
    assert!(json["metrics"]["node_movegen_calls"].as_u64().is_some());
    assert!(json["metrics"]["total_movegen_calls"].as_u64().is_some());
    let games = read_replay(&out).map_err(std::io::Error::other)?;
    assert_eq!(games.len(), 2);
    let validation = validate_replay(&games).map_err(std::io::Error::other)?;
    assert_eq!(validation.illegal_moves, 0);
    assert_eq!(validation.unhandled_terminal_states, 0);
    let _ = std::fs::remove_file(&out);
    Ok(())
}

#[test]
fn selfplay_chunked_writes_verified_replay_chunks() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::temp_dir().join(format!("ark-v4-selfplay-chunked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let output = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "selfplay",
            "--games",
            "4",
            "--actors",
            "2",
            "--search-depth",
            "1",
            "--max-plies",
            "8",
            "--chunked",
            "--chunk-size",
            "2",
            "--seed",
            "42",
            "--out",
            out.to_str().ok_or("temp path is not utf8")?,
            "--json",
        ])
        .output()?;
    assert!(output.status.success());
    let json = parse_single_json(output.stdout)?;
    assert_eq!(json["status"], "pass");
    assert_eq!(json["artifacts"]["replay_format"], "arkchunks-v2");
    assert_eq!(json["metrics"]["chunks_published"], 2);
    assert_eq!(json["metrics"]["illegal_moves"], 0);
    assert_eq!(json["metrics"]["unhandled_terminal_states"], 0);
    assert!(json["metrics"]["total_movegen_calls"].as_u64().is_some());

    let manifest = rebuild_manifest(&out.join("chunks")).map_err(std::io::Error::other)?;
    assert_eq!(manifest.games, 4);
    assert_eq!(manifest.chunks.len(), 2);
    let _ = std::fs::remove_dir_all(&out);
    Ok(())
}

#[test]
fn chunked_replay_trains_and_evals_from_run_dir() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::temp_dir().join(format!("ark-v4-chunked-train-{}", std::process::id()));
    let checkpoint = out.join("smoke.arkmodel");
    let _ = std::fs::remove_dir_all(&out);
    let selfplay = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "selfplay",
            "--games",
            "4",
            "--actors",
            "2",
            "--search-depth",
            "1",
            "--max-plies",
            "8",
            "--chunked",
            "--chunk-size",
            "2",
            "--seed",
            "42",
            "--out",
            out.to_str().ok_or("temp path is not utf8")?,
            "--json",
        ])
        .output()?;
    assert!(selfplay.status.success());

    let train = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "train",
            "--replay",
            out.to_str().ok_or("temp path is not utf8")?,
            "--steps",
            "2",
            "--checkpoint",
            checkpoint.to_str().ok_or("checkpoint path is not utf8")?,
            "--json",
        ])
        .output()?;
    assert!(train.status.success());
    let train_json = parse_single_json(train.stdout)?;
    assert_eq!(train_json["training_steps"], 2);
    assert_eq!(train_json["games_seen"], 8);

    let eval = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "eval",
            "baseline",
            "--replay",
            out.to_str().ok_or("temp path is not utf8")?,
            "--checkpoint",
            checkpoint.to_str().ok_or("checkpoint path is not utf8")?,
            "--json",
        ])
        .output()?;
    assert!(eval.status.success());
    let eval_json = parse_single_json(eval.stdout)?;
    assert_eq!(eval_json["games"], 4);
    assert_eq!(eval_json["illegal_moves"], 0);
    assert_eq!(eval_json["checkpoint_loaded"], true);

    let wdl_out = out.join("wdl-selfplay");
    let _ = std::fs::remove_dir_all(&wdl_out);
    let wdl_selfplay = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "selfplay",
            "--games",
            "2",
            "--actors",
            "1",
            "--search-depth",
            "1",
            "--tactical-extension-depth",
            "0",
            "--max-plies",
            "4",
            "--chunked",
            "--chunk-size",
            "1",
            "--leaf-eval",
            "wdl",
            "--checkpoint",
            checkpoint.to_str().ok_or("checkpoint path is not utf8")?,
            "--out",
            wdl_out.to_str().ok_or("temp path is not utf8")?,
            "--json",
        ])
        .output()?;
    assert!(wdl_selfplay.status.success());
    let wdl_json = parse_single_json(wdl_selfplay.stdout)?;
    assert_eq!(wdl_json["leaf_eval"], "wdl");
    assert_eq!(wdl_json["checkpoint_loaded"], true);
    assert_eq!(wdl_json["metrics"]["neutral_frontier_evals"], 0);
    assert_ne!(wdl_json["metrics"]["wdl_leaf_evals"], 0);

    let _ = std::fs::remove_dir_all(&out);
    Ok(())
}
