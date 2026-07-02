mod common;

use std::process::Command;

use common::parse_single_json;

#[test]
fn cli_selfplay_train_eval_smoke_loop() -> Result<(), Box<dyn std::error::Error>> {
    let replay = std::env::temp_dir().join("ark-v4-full-loop.arkgames");
    let checkpoint = std::env::temp_dir().join("ark-v4-full-loop.arkmodel");
    let _ = std::fs::remove_file(&replay);
    let _ = std::fs::remove_file(&checkpoint);

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
            "10",
            "--out",
            replay.to_str().ok_or("replay path is not utf8")?,
            "--json",
        ])
        .output()?;
    assert!(selfplay.status.success());
    let selfplay_json = parse_single_json(selfplay.stdout)?;
    assert_eq!(selfplay_json["metrics"]["games_completed"], 4);
    assert_eq!(selfplay_json["metrics"]["actor_crashes"], 0);
    assert!(selfplay_json["metrics"]["total_movegen_calls"]
        .as_u64()
        .is_some());

    let train = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "train",
            "--replay",
            replay.to_str().ok_or("replay path is not utf8")?,
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
    assert_ne!(train_json["policy_nonzero"], 0);

    let eval = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "eval",
            "baseline",
            "--replay",
            replay.to_str().ok_or("replay path is not utf8")?,
            "--checkpoint",
            checkpoint.to_str().ok_or("checkpoint path is not utf8")?,
            "--suite",
            "smoke",
            "--json",
        ])
        .output()?;
    assert!(eval.status.success());
    let eval_json = parse_single_json(eval.stdout)?;
    assert_eq!(eval_json["illegal_moves"], 0);
    assert_eq!(eval_json["checkpoint_loaded"], true);

    let search = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "search",
            "--fen",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "--depth",
            "1",
            "--checkpoint",
            checkpoint.to_str().ok_or("checkpoint path is not utf8")?,
            "--move-order",
            "model",
            "--json",
        ])
        .output()?;
    assert!(search.status.success());
    let search_json = parse_single_json(search.stdout)?;
    assert_eq!(search_json["checkpoint_loaded"], true);
    assert_eq!(search_json["move_order"], "model");
    assert_eq!(search_json["metrics"]["model_ordered_root_moves"], 20);
    assert_eq!(search_json["metrics"]["root_movegen_calls"], 1);

    let wdl_search = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "search",
            "--fen",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "--depth",
            "1",
            "--checkpoint",
            checkpoint.to_str().ok_or("checkpoint path is not utf8")?,
            "--leaf-eval",
            "wdl",
            "--json",
        ])
        .output()?;
    assert!(wdl_search.status.success());
    let wdl_search_json = parse_single_json(wdl_search.stdout)?;
    assert_eq!(wdl_search_json["leaf_eval"], "wdl");
    assert_eq!(wdl_search_json["checkpoint_loaded"], true);
    assert_ne!(wdl_search_json["metrics"]["wdl_leaf_evals"], 0);

    let _ = std::fs::remove_file(replay);
    let _ = std::fs::remove_file(checkpoint);
    Ok(())
}
