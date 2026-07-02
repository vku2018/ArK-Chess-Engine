use std::path::PathBuf;

use ark_core::{GameOutcome, Position};
use ark_model::{
    game_from_uci, read_replay, validate_replay, wdl_logits_to_probabilities, write_replay,
    BatchInferenceWorkspace, ForgeModel, PolicyMoveWorkspace, WdlEvaluationWorkspace,
    WdlProbabilities, HIDDEN_SIZE, INPUT_SIZE, WDL_EXPECTATION_TO_CENTIPAWNS,
};

#[test]
fn replay_roundtrip_and_validation() -> Result<(), String> {
    let game = game_from_uci(
        GameOutcome::Draw,
        &["e2e4".to_string(), "e7e5".to_string(), "g1f3".to_string()],
    )?;
    let path = temp_path("ark-v4-replay-roundtrip.arkgames");
    let _ = std::fs::remove_file(&path);
    let summary = write_replay(&path, &[game.clone()])?;
    assert_eq!(summary.games, 1);
    assert_eq!(summary.plies, 3);
    let loaded = read_replay(&path)?;
    assert_eq!(loaded, vec![game]);
    let validation = validate_replay(&loaded)?;
    assert_eq!(validation.illegal_moves, 0);
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[test]
fn model_train_save_load_roundtrip() -> Result<(), String> {
    let game = game_from_uci(
        GameOutcome::WhiteWin,
        &["e2e4".to_string(), "e7e5".to_string(), "d1h5".to_string()],
    )?;
    let mut model = ForgeModel::default();
    let summary = model.train_games(&[game], 2, 0.05);
    assert_eq!(summary.training_steps, 2);
    assert!(summary.policy_nonzero > 0);
    let path = temp_path("ark-v4-model-roundtrip.arkmodel");
    let _ = std::fs::remove_file(&path);
    model.save(&path)?;
    let loaded = ForgeModel::load(&path)?;
    assert_eq!(loaded.training_steps, 2);
    assert_eq!(loaded.policy, model.policy);
    let position = ark_core::Position::startpos().map_err(|err| format!("{err:?}"))?;
    assert_eq!(loaded.forward(&position), model.forward(&position));
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[test]
fn forward_outputs_are_finite_and_legal_masked() -> Result<(), String> {
    let model = ForgeModel::default();
    let position = ark_core::Position::startpos().map_err(|err| format!("{err:?}"))?;
    let output = model.forward(&position);
    assert_eq!(output.policy.len(), position.legal_moves().len());
    assert_eq!(output.refutation.len(), position.legal_moves().len());
    for (_packed, score) in &output.policy {
        assert!(score.is_finite());
    }
    for value in output.wdl {
        assert!(value.is_finite());
    }
    assert!(output.moves_left.is_finite());
    assert!((0.0..=1.0).contains(&output.uncertainty));
    assert!((0.0..=1.0).contains(&output.risk));
    Ok(())
}

#[test]
fn wdl_leaf_probabilities_are_finite_and_sum_to_one() -> Result<(), String> {
    let model = ForgeModel::default();
    let position = Position::startpos().map_err(|err| format!("{err:?}"))?;
    let evaluation = model.evaluate_wdl_leaf(&position);

    for logit in evaluation.logits {
        assert!(logit.is_finite());
    }
    assert_valid_wdl_probabilities(evaluation.probabilities);
    assert!(evaluation.white_perspective_centipawns.abs() <= WDL_EXPECTATION_TO_CENTIPAWNS);
    assert_eq!(
        evaluation.side_to_move_centipawns,
        evaluation.white_perspective_centipawns
    );

    let extreme = wdl_logits_to_probabilities([10_000.0, 0.0, -10_000.0]);
    assert_valid_wdl_probabilities(extreme);
    assert!(extreme.white_win > 0.999);
    assert!(extreme.black_win < 0.001);
    Ok(())
}

#[test]
fn wdl_training_moves_startpos_score_by_game_result() -> Result<(), String> {
    let position = Position::startpos().map_err(|err| format!("{err:?}"))?;
    let before = ForgeModel::default().evaluate_wdl_leaf(&position);
    let white_win_game = game_from_uci(
        GameOutcome::WhiteWin,
        &["e2e4".to_string(), "e7e5".to_string()],
    )?;
    let black_win_game = game_from_uci(
        GameOutcome::BlackWin,
        &["e2e4".to_string(), "e7e5".to_string()],
    )?;

    let mut white_model = ForgeModel::default();
    white_model.train_games(&[white_win_game], 8, 0.5);
    let white_after = white_model.evaluate_wdl_leaf(&position);

    assert!(
        white_after.probabilities.white_win > before.probabilities.white_win,
        "WhiteWin training did not increase white win probability: before={} after={}",
        before.probabilities.white_win,
        white_after.probabilities.white_win
    );
    assert!(
        white_after.white_perspective_centipawns > before.white_perspective_centipawns,
        "WhiteWin training did not increase white-perspective score: before={} after={}",
        before.white_perspective_centipawns,
        white_after.white_perspective_centipawns
    );

    let mut black_model = ForgeModel::default();
    black_model.train_games(&[black_win_game], 8, 0.5);
    let black_after = black_model.evaluate_wdl_leaf(&position);

    assert!(
        black_after.probabilities.black_win > before.probabilities.black_win,
        "BlackWin training did not increase black win probability: before={} after={}",
        before.probabilities.black_win,
        black_after.probabilities.black_win
    );
    assert!(
        black_after.white_perspective_centipawns < before.white_perspective_centipawns,
        "BlackWin training did not reduce white-perspective score: before={} after={}",
        before.white_perspective_centipawns,
        black_after.white_perspective_centipawns
    );
    assert!(white_after.white_perspective_centipawns - before.white_perspective_centipawns > 0);
    assert!(black_after.white_perspective_centipawns - before.white_perspective_centipawns < 0);
    Ok(())
}

#[test]
fn wdl_leaf_evaluation_reuses_workspace() -> Result<(), String> {
    let model = trained_model()?;
    let position = position_after(&["e2e4"])?;
    let mut workspace = WdlEvaluationWorkspace::default();

    let first = model.evaluate_wdl_leaf_into(&position, &mut workspace);
    assert_eq!(workspace.input_values().len(), INPUT_SIZE);
    assert_eq!(workspace.hidden_values().len(), HIDDEN_SIZE);
    let input_capacity = workspace.input_capacity();
    let hidden_capacity = workspace.hidden_capacity();

    let second = model.evaluate_wdl_leaf_into(&position, &mut workspace);
    assert_eq!(second, first);
    assert_eq!(workspace.input_capacity(), input_capacity);
    assert_eq!(workspace.hidden_capacity(), hidden_capacity);
    assert_eq!(
        first.side_to_move_centipawns,
        first.white_perspective_centipawns.saturating_neg()
    );
    Ok(())
}

#[test]
fn forward_batch_matches_individual_forward_for_several_positions() -> Result<(), String> {
    let model = trained_model()?;
    let positions = test_positions()?;

    let batch = model.forward_batch(&positions);
    let individual: Vec<_> = positions
        .iter()
        .map(|position| model.forward(position))
        .collect();

    assert_eq!(batch, individual);
    Ok(())
}

#[test]
fn forward_batch_into_reuses_workspace_and_reports_metrics() -> Result<(), String> {
    let model = trained_model()?;
    let positions = test_positions()?;
    let expected_legal_moves = positions
        .iter()
        .map(|position| position.legal_moves().len())
        .sum();
    let mut workspace = BatchInferenceWorkspace::default();
    let mut outputs = Vec::new();

    let metrics = model.forward_batch_into(&positions, &mut outputs, &mut workspace);
    assert_eq!(outputs, model.forward_batch(&positions));
    assert_eq!(metrics.positions, positions.len());
    assert_eq!(metrics.legal_moves, expected_legal_moves);
    assert_eq!(metrics.input_values, positions.len() * INPUT_SIZE);
    assert_eq!(metrics.hidden_values, positions.len() * HIDDEN_SIZE);
    assert_eq!(workspace.encoded().len(), positions.len());
    assert_eq!(
        workspace.encoded().inputs().len(),
        positions.len() * INPUT_SIZE
    );
    assert_eq!(
        workspace.hidden_values().len(),
        positions.len() * HIDDEN_SIZE
    );

    let output_capacity = outputs.capacity();
    let input_capacity = workspace.encoded().input_capacity();
    let hidden_capacity = workspace.hidden_capacity();
    let first_policy_capacity = outputs
        .first()
        .map(|output| output.policy.capacity())
        .ok_or_else(|| "missing first batch output".to_string())?;
    let first_refutation_capacity = outputs
        .first()
        .map(|output| output.refutation.capacity())
        .ok_or_else(|| "missing first batch output".to_string())?;

    let second_metrics = model.forward_batch_into(&positions, &mut outputs, &mut workspace);
    assert_eq!(second_metrics, metrics);
    assert!(outputs.capacity() >= output_capacity);
    assert!(workspace.encoded().input_capacity() >= input_capacity);
    assert!(workspace.hidden_capacity() >= hidden_capacity);
    let Some(first_output) = outputs.first() else {
        return Err("missing first batch output".to_string());
    };
    assert!(first_output.policy.capacity() >= first_policy_capacity);
    assert!(first_output.refutation.capacity() >= first_refutation_capacity);
    Ok(())
}

#[test]
fn policy_move_scores_cover_only_legal_moves() -> Result<(), String> {
    let model = trained_model()?;
    let position = position_after(&["e2e4", "e7e5"])?;
    let legal_moves = position.legal_moves();

    let scores = model.score_legal_moves(&position);

    assert_eq!(scores.len(), legal_moves.len());
    for scored in &scores {
        assert!(
            legal_moves.contains(&scored.mv),
            "scored illegal move {}",
            scored.mv
        );
        assert_eq!(scored.packed, scored.mv.packed_id());
        assert!(scored.score.is_finite());
    }
    for legal in &legal_moves {
        assert!(
            scores.iter().any(|scored| scored.mv == *legal),
            "missing legal move score for {legal}"
        );
    }
    Ok(())
}

#[test]
fn trained_games_raise_selected_policy_score_and_order_moves() -> Result<(), String> {
    let position = Position::startpos().map_err(|err| format!("{err:?}"))?;
    let selected = position
        .move_from_uci("e2e4")
        .ok_or_else(|| "missing legal e2e4 from startpos".to_string())?;
    let game = game_from_uci(
        GameOutcome::WhiteWin,
        &["e2e4".to_string(), "e7e5".to_string()],
    )?;
    let mut model = ForgeModel::default();
    let mut workspace = PolicyMoveWorkspace::default();
    let mut selected_scores = Vec::new();

    model.score_legal_move_slice_into(&position, &[selected], &mut selected_scores, &mut workspace);
    let before = only_policy_score(&selected_scores)?;

    model.train_games(&[game], 4, 0.25);
    model.score_legal_move_slice_into(&position, &[selected], &mut selected_scores, &mut workspace);
    let after = only_policy_score(&selected_scores)?;

    assert!(
        after > before,
        "trained move policy score did not increase: before={before} after={after}"
    );

    let mut ordered = position.legal_moves();
    let metrics = model.order_legal_moves_by_policy(&position, &mut ordered, &mut workspace);
    assert_eq!(metrics.moves, ordered.len());
    assert_eq!(metrics.input_values, INPUT_SIZE);
    assert_eq!(metrics.hidden_values, HIDDEN_SIZE);
    assert_eq!(
        ordered.first().copied(),
        Some(selected),
        "trained policy did not promote e2e4 to the front of startpos legal moves"
    );

    let mut ordered_scores = Vec::new();
    model.score_legal_move_slice_into(&position, &ordered, &mut ordered_scores, &mut workspace);
    assert!(
        ordered_scores
            .windows(2)
            .all(|pair| pair[0].score >= pair[1].score),
        "policy-ordered moves are not sorted by descending policy score"
    );
    Ok(())
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

fn only_policy_score(scores: &[ark_model::PolicyMoveScore]) -> Result<f32, String> {
    match scores {
        [score] => Ok(score.score),
        other => Err(format!("expected one policy score, got {}", other.len())),
    }
}

fn assert_valid_wdl_probabilities(probabilities: WdlProbabilities) {
    for value in probabilities.as_array() {
        assert!(value.is_finite());
        assert!((0.0..=1.0).contains(&value));
    }
    assert!(
        (probabilities.sum() - 1.0).abs() <= 0.000_001,
        "WDL probabilities do not sum to 1: {:?}",
        probabilities
    );
}

fn trained_model() -> Result<ForgeModel, String> {
    let game = game_from_uci(
        GameOutcome::WhiteWin,
        &[
            "e2e4".to_string(),
            "e7e5".to_string(),
            "g1f3".to_string(),
            "b8c6".to_string(),
            "f1b5".to_string(),
        ],
    )?;
    let mut model = ForgeModel::default();
    model.train_games(&[game], 3, 0.025);
    Ok(model)
}

fn test_positions() -> Result<Vec<Position>, String> {
    Ok(vec![
        position_after(&[])?,
        position_after(&["e2e4"])?,
        position_after(&["e2e4", "e7e5", "g1f3"])?,
        position_after(&["d2d4", "g8f6", "c2c4", "e7e6"])?,
    ])
}

fn position_after(moves: &[&str]) -> Result<Position, String> {
    let mut position = Position::startpos().map_err(|err| format!("{err:?}"))?;
    for text in moves {
        let Some(next) = position.make_uci_move(text) else {
            return Err(format!("illegal test move: {text}"));
        };
        position = next;
    }
    Ok(position)
}
