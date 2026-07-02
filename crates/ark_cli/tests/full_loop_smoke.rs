use std::process::Command;

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
    let selfplay_stdout = String::from_utf8(selfplay.stdout)?;
    assert!(selfplay_stdout.contains("\"games_completed\":4"));
    assert!(selfplay_stdout.contains("\"actor_crashes\":0"));

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
    let train_stdout = String::from_utf8(train.stdout)?;
    assert!(train_stdout.contains("\"training_steps\":2"));
    assert!(!train_stdout.contains("\"policy_nonzero\":0"));

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
    let eval_stdout = String::from_utf8(eval.stdout)?;
    assert!(eval_stdout.contains("\"illegal_moves\":0"));
    assert!(eval_stdout.contains("\"checkpoint_loaded\":true"));

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
    let search_stdout = String::from_utf8(search.stdout)?;
    assert!(search_stdout.contains("\"checkpoint_loaded\":true"));
    assert!(search_stdout.contains("\"move_order\":\"model\""));
    assert!(
        search_stdout.contains("\"model_ordered_root_moves\":20"),
        "{search_stdout}"
    );

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
    let wdl_search_stdout = String::from_utf8(wdl_search.stdout)?;
    assert!(
        wdl_search_stdout.contains("\"leaf_eval\":\"wdl\""),
        "{wdl_search_stdout}"
    );
    assert!(
        wdl_search_stdout.contains("\"checkpoint_loaded\":true"),
        "{wdl_search_stdout}"
    );
    assert!(
        !wdl_search_stdout.contains("\"wdl_leaf_evals\":0"),
        "{wdl_search_stdout}"
    );

    let _ = std::fs::remove_file(replay);
    let _ = std::fs::remove_file(checkpoint);
    Ok(())
}
