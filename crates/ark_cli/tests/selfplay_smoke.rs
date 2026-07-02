use std::process::Command;

use ark_model::{read_replay, validate_replay};
use ark_replay::rebuild_manifest;

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
    let stdout = String::from_utf8(output.stdout)?;
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(
        stdout.contains("\"schema_version\":\"ark-v4-forge-bench-v1\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"benchmark_id\":\"forge-selfplay-adhoc\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"status\":\"pass\""), "{stdout}");
    assert!(stdout.contains("\"games_completed\":2"));
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
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("\"status\":\"pass\""), "{stdout}");
    assert!(
        stdout.contains("\"replay_format\":\"arkchunks-v2\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"chunks_published\":2"), "{stdout}");
    assert!(stdout.contains("\"illegal_moves\":0"), "{stdout}");
    assert!(
        stdout.contains("\"unhandled_terminal_states\":0"),
        "{stdout}"
    );

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
    let train_stdout = String::from_utf8(train.stdout)?;
    assert!(
        train_stdout.contains("\"training_steps\":2"),
        "{train_stdout}"
    );
    assert!(train_stdout.contains("\"games_seen\":8"), "{train_stdout}");

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
    let eval_stdout = String::from_utf8(eval.stdout)?;
    assert!(eval_stdout.contains("\"games\":4"), "{eval_stdout}");
    assert!(eval_stdout.contains("\"illegal_moves\":0"), "{eval_stdout}");
    assert!(
        eval_stdout.contains("\"checkpoint_loaded\":true"),
        "{eval_stdout}"
    );

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
    let wdl_stdout = String::from_utf8(wdl_selfplay.stdout)?;
    assert!(wdl_stdout.contains("\"leaf_eval\":\"wdl\""), "{wdl_stdout}");
    assert!(
        wdl_stdout.contains("\"checkpoint_loaded\":true"),
        "{wdl_stdout}"
    );
    assert!(
        wdl_stdout.contains("\"neutral_frontier_evals\":0"),
        "{wdl_stdout}"
    );
    assert!(!wdl_stdout.contains("\"wdl_leaf_evals\":0"), "{wdl_stdout}");

    let _ = std::fs::remove_dir_all(&out);
    Ok(())
}
