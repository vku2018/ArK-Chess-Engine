use std::process::Command;

#[test]
fn search_json_fails_when_depth_target_is_missed() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "search",
            "--fen",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "--depth",
            "4",
            "--nodes",
            "1",
            "--threads",
            "1",
            "--seed",
            "1729",
            "--json",
        ])
        .output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    for required in [
        "\"schema_version\":\"ark-v4-forge-bench-v1\"",
        "\"benchmark_id\":",
        "\"status\":\"fail\"",
        "\"git_sha\":",
        "\"command\":\"cargo run --release -p ark_cli -- search",
        "\"metrics\":",
        "\"nodes\":1",
        "\"depth_completed\":0",
        "\"targets\":{\"depth_completed_min\":4",
        "\"target_results\":",
        "\"target_name\":\"depth_completed_min\"",
        "\"observed\":0",
        "\"target_value\":4",
        "\"passed\":false",
        "\"failures\":[\"depth_completed_min failed",
        "\"confidence\":{\"level\":0.95,\"repetitions\":1,\"minimum_repetitions\":5,\"complete\":false",
        "\"non_terminal_static_eval_calls\":0",
        "\"python_hot_path_ms\":0",
        "\"artifacts\":",
    ] {
        assert!(stdout.contains(required), "missing {required} in {stdout}");
    }
    Ok(())
}

#[test]
fn search_json_passes_when_ad_hoc_targets_are_met() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "search",
            "--fen",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "--depth",
            "1",
            "--threads",
            "1",
            "--seed",
            "1729",
            "--json",
        ])
        .output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    for required in [
        "\"schema_version\":\"ark-v4-forge-bench-v1\"",
        "\"benchmark_id\":\"forge-search-adhoc\"",
        "\"status\":\"pass\"",
        "\"target_profile\":\"ad_hoc\"",
        "\"leaf_eval\":\"terminal\"",
        "\"wdl_leaf_evals\":0",
        "\"depth_completed\":1",
        "\"targets\":{\"depth_completed_min\":1",
        "\"failures\":[]",
        "\"committed_artifacts\":0",
    ] {
        assert!(stdout.contains(required), "missing {required} in {stdout}");
    }
    Ok(())
}

#[test]
fn search_wdl_leaf_eval_requires_checkpoint() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "search",
            "--fen",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "--depth",
            "1",
            "--leaf-eval",
            "wdl",
            "--json",
        ])
        .output()?;
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("--checkpoint is required for --leaf-eval wdl"),
        "{stderr}"
    );
    Ok(())
}

#[test]
fn perft_json_reports_correct_nodes_pass_and_fail() -> Result<(), Box<dyn std::error::Error>> {
    let pass = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "perft",
            "--fen",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "--depth",
            "2",
            "--correct-nodes",
            "400",
            "--json",
        ])
        .output()?;
    assert!(pass.status.success());
    let pass_stdout = String::from_utf8(pass.stdout)?;
    assert!(pass_stdout.contains("\"status\":\"pass\""), "{pass_stdout}");
    assert!(
        pass_stdout.contains("\"targets\":{\"correct_nodes\":400"),
        "{pass_stdout}"
    );
    assert!(
        pass_stdout.contains("\"target_name\":\"correct_nodes\""),
        "{pass_stdout}"
    );

    let fail = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "perft",
            "--fen",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "--depth",
            "2",
            "--correct-nodes",
            "401",
            "--json",
        ])
        .output()?;
    assert!(fail.status.success());
    let fail_stdout = String::from_utf8(fail.stdout)?;
    assert!(fail_stdout.contains("\"status\":\"fail\""), "{fail_stdout}");
    assert!(
        fail_stdout.contains("\"correct_nodes failed: observed 400 != target 401\""),
        "{fail_stdout}"
    );
    Ok(())
}
