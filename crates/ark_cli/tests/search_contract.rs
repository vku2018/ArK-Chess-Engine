mod common;

use std::process::Command;

use common::parse_single_json;

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
    let json = parse_single_json(output.stdout)?;
    assert_eq!(json["schema_version"], "ark-v4-forge-bench-v1");
    assert_eq!(json["status"], "fail");
    assert!(json["git_sha"].as_str().is_some());
    assert!(json["command"]
        .as_str()
        .is_some_and(|command| command.starts_with("cargo run --release -p ark_cli -- search")));
    assert_eq!(json["metrics"]["nodes"], 1);
    assert_eq!(json["metrics"]["depth_completed"], 0);
    assert_eq!(json["metrics"]["root_movegen_calls"], 1);
    assert!(json["metrics"]["total_movegen_calls"]
        .as_u64()
        .is_some_and(|calls| calls >= 1));
    assert_eq!(json["metrics"]["non_terminal_static_eval_calls"], 0);
    assert_eq!(json["metrics"]["python_hot_path_ms"], 0);
    assert_eq!(json["targets"]["depth_completed_min"], 4);
    assert_eq!(
        json["target_results"][0]["target_name"],
        "depth_completed_min"
    );
    assert_eq!(json["target_results"][0]["observed"], 0);
    assert_eq!(json["target_results"][0]["target_value"], 4);
    assert_eq!(json["target_results"][0]["passed"], false);
    assert!(json["failures"][0]
        .as_str()
        .is_some_and(|failure| failure.contains("depth_completed_min failed")));
    assert_eq!(json["confidence"]["level"], 0.95);
    assert_eq!(json["confidence"]["repetitions"], 1);
    assert_eq!(json["confidence"]["minimum_repetitions"], 5);
    assert_eq!(json["confidence"]["complete"], false);
    assert_eq!(json["artifacts"]["committed_artifacts"], 0);
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
    let json = parse_single_json(output.stdout)?;
    assert_eq!(json["schema_version"], "ark-v4-forge-bench-v1");
    assert_eq!(json["benchmark_id"], "forge-search-adhoc");
    assert_eq!(json["status"], "pass");
    assert_eq!(json["target_profile"], "ad_hoc");
    assert_eq!(json["leaf_eval"], "terminal");
    assert_eq!(json["metrics"]["wdl_leaf_evals"], 0);
    assert_eq!(json["metrics"]["depth_completed"], 1);
    assert_eq!(json["trace"]["root_movegen_calls"], 1);
    assert!(json["trace"]["node_movegen_calls"].as_u64().is_some());
    assert!(json["trace"]["total_movegen_calls"].as_u64().is_some());
    assert_eq!(json["targets"]["depth_completed_min"], 1);
    assert_eq!(json["failures"].as_array().map(Vec::len), Some(0));
    assert_eq!(json["artifacts"]["committed_artifacts"], 0);
    Ok(())
}

#[test]
fn search_rejects_threads_above_one() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "search",
            "--fen",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "--depth",
            "1",
            "--threads",
            "2",
            "--json",
        ])
        .output()?;
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("search --threads > 1 is not supported yet; got 2. Use --threads 1."),
        "{stderr}"
    );
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
    let pass_json = parse_single_json(pass.stdout)?;
    assert_eq!(pass_json["status"], "pass");
    assert_eq!(pass_json["targets"]["correct_nodes"], 400);
    assert_eq!(
        pass_json["target_results"][0]["target_name"],
        "correct_nodes"
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
    let fail_json = parse_single_json(fail.stdout)?;
    assert_eq!(fail_json["status"], "fail");
    assert!(fail_json["failures"][0]
        .as_str()
        .is_some_and(|failure| failure == "correct_nodes failed: observed 400 != target 401"));
    Ok(())
}

#[test]
fn perft_bad_fen_exits_non_zero_with_bad_fen() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args([
            "perft",
            "--fen",
            "4k3/8/8/8/8/8/8/4K0N2 w - - 0 1",
            "--depth",
            "1",
        ])
        .output()?;
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("bad FEN"), "{stderr}");
    Ok(())
}

#[test]
fn terminal_leaf_suite_json_is_structural() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_ark"))
        .args(["terminal-leaf-suite", "--cases", "4", "--json"])
        .output()?;
    assert!(output.status.success());

    let json = parse_single_json(output.stdout)?;
    assert_eq!(json["schema_version"], "ark-v4-forge-bench-v1");
    assert_eq!(json["benchmark_id"], "forge-search-terminal-leaf-gate");
    assert_eq!(json["metrics"]["cases"], 4);
    assert_eq!(json["metrics"]["terminal_cases_passed"], 4);
    assert_eq!(json["metrics"]["non_terminal_static_eval_calls"], 0);
    assert!(json["targets"].is_object());
    assert!(json["target_results"].is_array());
    assert!(json["failures"].is_array());
    Ok(())
}
