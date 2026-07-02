use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use ark_core::{Color, GameOutcome, GameState, Move, PieceKind, Position};

const REPLAY_MAGIC: &[u8; 8] = b"ARKG4\0\0\0";
const MODEL_MAGIC: &[u8; 8] = b"ARKM4\0\0\0";
const VERSION: u32 = 1;
pub const POLICY_SIZE: usize = 20_480;
pub const INPUT_SIZE: usize = 12 * 64;
pub const HIDDEN_SIZE: usize = 32;
pub const WDL_EXPECTATION_TO_CENTIPAWNS: i32 = 1000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameRecord {
    pub result: GameOutcome,
    pub moves: Vec<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplaySummary {
    pub games: u32,
    pub plies: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayValidation {
    pub games: u32,
    pub plies: u64,
    pub illegal_moves: u32,
    pub unhandled_terminal_states: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ForgeModel {
    pub training_steps: u64,
    pub games_seen: u64,
    pub input_hidden: Vec<f32>,
    pub hidden_bias: Vec<f32>,
    pub policy_head: Vec<f32>,
    pub policy: Vec<f32>,
    pub wdl_head: Vec<f32>,
    pub wdl: [f32; 3],
    pub moves_left_head: Vec<f32>,
    pub moves_left: f32,
    pub uncertainty_head: Vec<f32>,
    pub uncertainty: f32,
    pub risk_head: Vec<f32>,
    pub risk: f32,
    pub refutation_head: Vec<f32>,
    pub refutation: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelOutput {
    pub policy: Vec<(u16, f32)>,
    pub wdl: [f32; 3],
    pub moves_left: f32,
    pub uncertainty: f32,
    pub risk: f32,
    pub refutation: Vec<(u16, f32)>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PolicyMoveMetrics {
    pub moves: usize,
    pub input_values: usize,
    pub hidden_values: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolicyMoveScore {
    pub mv: Move,
    pub packed: u16,
    pub score: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolicyMoveWorkspace {
    input: Vec<f32>,
    hidden: Vec<f32>,
    legal_moves: Vec<Move>,
    scores: Vec<PolicyMoveScore>,
}

impl PolicyMoveWorkspace {
    #[must_use]
    pub fn input_capacity(&self) -> usize {
        self.input.capacity()
    }

    #[must_use]
    pub fn hidden_capacity(&self) -> usize {
        self.hidden.capacity()
    }

    #[must_use]
    pub fn legal_moves_capacity(&self) -> usize {
        self.legal_moves.capacity()
    }

    #[must_use]
    pub fn scores_capacity(&self) -> usize {
        self.scores.capacity()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WdlProbabilities {
    pub white_win: f32,
    pub draw: f32,
    pub black_win: f32,
}

impl WdlProbabilities {
    #[must_use]
    pub const fn as_array(self) -> [f32; 3] {
        [self.white_win, self.draw, self.black_win]
    }

    #[must_use]
    pub fn sum(self) -> f32 {
        self.white_win + self.draw + self.black_win
    }

    #[must_use]
    pub fn expected_white_score(self) -> f32 {
        self.white_win - self.black_win
    }

    #[must_use]
    pub fn white_perspective_centipawns(self) -> i32 {
        wdl_probabilities_to_white_centipawns(self)
    }

    #[must_use]
    pub fn side_to_move_centipawns(self, side_to_move: Color) -> i32 {
        white_centipawns_to_side_to_move_centipawns(
            self.white_perspective_centipawns(),
            side_to_move,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WdlLeafEvaluation {
    pub logits: [f32; 3],
    pub probabilities: WdlProbabilities,
    pub white_perspective_centipawns: i32,
    pub side_to_move_centipawns: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WdlEvaluationWorkspace {
    input: Vec<f32>,
    hidden: Vec<f32>,
}

impl WdlEvaluationWorkspace {
    #[must_use]
    pub fn input_values(&self) -> &[f32] {
        &self.input
    }

    #[must_use]
    pub fn hidden_values(&self) -> &[f32] {
        &self.hidden
    }

    #[must_use]
    pub fn input_capacity(&self) -> usize {
        self.input.capacity()
    }

    #[must_use]
    pub fn hidden_capacity(&self) -> usize {
        self.hidden.capacity()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BatchInferenceMetrics {
    pub positions: usize,
    pub legal_moves: usize,
    pub input_values: usize,
    pub hidden_values: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EncodedBatch {
    inputs: Vec<f32>,
    legal_moves: Vec<Vec<Move>>,
}

impl EncodedBatch {
    #[must_use]
    pub fn len(&self) -> usize {
        self.legal_moves.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.legal_moves.is_empty()
    }

    #[must_use]
    pub fn inputs(&self) -> &[f32] {
        &self.inputs
    }

    #[must_use]
    pub fn input_capacity(&self) -> usize {
        self.inputs.capacity()
    }

    #[must_use]
    pub fn legal_moves(&self, position_index: usize) -> Option<&[Move]> {
        self.legal_moves
            .get(position_index)
            .map(std::vec::Vec::as_slice)
    }

    fn encode_positions(&mut self, positions: &[Position]) -> usize {
        self.inputs.clear();
        self.inputs.resize(positions.len() * INPUT_SIZE, 0.0);
        self.legal_moves.clear();
        self.legal_moves.reserve(positions.len());

        let mut legal_moves = 0;
        for (position_index, position) in positions.iter().enumerate() {
            let input_offset = position_index * INPUT_SIZE;
            encode_position_into(
                position,
                &mut self.inputs[input_offset..input_offset + INPUT_SIZE],
            );
            let legal = position.legal_moves();
            legal_moves += legal.len();
            self.legal_moves.push(legal);
        }
        legal_moves
    }

    fn input(&self, position_index: usize) -> &[f32] {
        let input_offset = position_index * INPUT_SIZE;
        &self.inputs[input_offset..input_offset + INPUT_SIZE]
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BatchInferenceWorkspace {
    encoded: EncodedBatch,
    hidden: Vec<f32>,
}

impl BatchInferenceWorkspace {
    #[must_use]
    pub fn encoded(&self) -> &EncodedBatch {
        &self.encoded
    }

    #[must_use]
    pub fn hidden_values(&self) -> &[f32] {
        &self.hidden
    }

    #[must_use]
    pub fn hidden_capacity(&self) -> usize {
        self.hidden.capacity()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrainSummary {
    pub training_steps: u64,
    pub games_seen: u64,
    pub plies_seen: u64,
    pub policy_nonzero: usize,
}

impl Default for ForgeModel {
    fn default() -> Self {
        Self {
            training_steps: 0,
            games_seen: 0,
            input_hidden: init_weights(INPUT_SIZE * HIDDEN_SIZE, 0xA4C0_0001),
            hidden_bias: vec![0.0; HIDDEN_SIZE],
            policy_head: init_weights(HIDDEN_SIZE * POLICY_SIZE, 0xA4C0_0002),
            policy: vec![0.0; POLICY_SIZE],
            wdl_head: init_weights(HIDDEN_SIZE * 3, 0xA4C0_0003),
            wdl: [0.0; 3],
            moves_left_head: init_weights(HIDDEN_SIZE, 0xA4C0_0004),
            moves_left: 0.0,
            uncertainty_head: init_weights(HIDDEN_SIZE, 0xA4C0_0005),
            uncertainty: 1.0,
            risk_head: init_weights(HIDDEN_SIZE, 0xA4C0_0006),
            risk: 0.0,
            refutation_head: init_weights(HIDDEN_SIZE * POLICY_SIZE, 0xA4C0_0007),
            refutation: 0.0,
        }
    }
}

impl ForgeModel {
    #[must_use]
    pub fn forward(&self, position: &Position) -> ModelOutput {
        let input = encode_position(position);
        let hidden = self.hidden(&input);
        let legal = position.legal_moves();
        let mut output = ModelOutput::empty();
        self.fill_output(&hidden, &legal, &mut output);
        output
    }

    #[must_use]
    pub fn forward_batch(&self, positions: &[Position]) -> Vec<ModelOutput> {
        let mut outputs = Vec::with_capacity(positions.len());
        let mut workspace = BatchInferenceWorkspace::default();
        self.forward_batch_into(positions, &mut outputs, &mut workspace);
        outputs
    }

    pub fn forward_batch_into(
        &self,
        positions: &[Position],
        outputs: &mut Vec<ModelOutput>,
        workspace: &mut BatchInferenceWorkspace,
    ) -> BatchInferenceMetrics {
        let legal_moves = workspace.encoded.encode_positions(positions);
        workspace.hidden.clear();
        workspace.hidden.resize(positions.len() * HIDDEN_SIZE, 0.0);
        resize_outputs(outputs, positions.len());

        for (position_index, output) in outputs.iter_mut().enumerate() {
            let input = workspace.encoded.input(position_index);
            let hidden_offset = position_index * HIDDEN_SIZE;
            let hidden = &mut workspace.hidden[hidden_offset..hidden_offset + HIDDEN_SIZE];
            self.write_hidden(input, hidden);
            let hidden = &workspace.hidden[hidden_offset..hidden_offset + HIDDEN_SIZE];
            self.fill_output(
                hidden,
                &workspace.encoded.legal_moves[position_index],
                output,
            );
        }

        BatchInferenceMetrics {
            positions: positions.len(),
            legal_moves,
            input_values: workspace.encoded.inputs.len(),
            hidden_values: workspace.hidden.len(),
        }
    }

    #[must_use]
    pub fn evaluate_wdl_leaf(&self, position: &Position) -> WdlLeafEvaluation {
        let mut workspace = WdlEvaluationWorkspace::default();
        self.evaluate_wdl_leaf_into(position, &mut workspace)
    }

    #[must_use]
    pub fn evaluate_wdl_leaf_into(
        &self,
        position: &Position,
        workspace: &mut WdlEvaluationWorkspace,
    ) -> WdlLeafEvaluation {
        self.write_wdl_hidden(position, workspace);
        let logits = self.wdl_logits_from_hidden(&workspace.hidden);
        wdl_leaf_evaluation_from_logits(logits, position.side_to_move())
    }

    pub fn train_games(
        &mut self,
        games: &[GameRecord],
        steps: u32,
        learning_rate: f32,
    ) -> TrainSummary {
        let mut plies_seen = 0_u64;
        for _ in 0..steps.max(1) {
            for game in games {
                self.games_seen += 1;
                let target = f32::from(game.result.white_score());
                let wdl_index = match game.result {
                    GameOutcome::WhiteWin => 0,
                    GameOutcome::Draw => 1,
                    GameOutcome::BlackWin => 2,
                };
                self.wdl[wdl_index] += learning_rate;
                let total = game.moves.len().max(1) as f32;
                for (ply, packed) in game.moves.iter().copied().enumerate() {
                    let Some(slot) = self.policy.get_mut(usize::from(packed)) else {
                        continue;
                    };
                    let side_sign = if ply % 2 == 0 { 1.0 } else { -1.0 };
                    let remaining = (game.moves.len() - ply) as f32;
                    *slot += learning_rate * (1.0 + 0.25 * target * side_sign);
                    self.moves_left += learning_rate * (remaining / total);
                    self.risk += learning_rate * if target * side_sign < 0.0 { 1.0 } else { -0.25 };
                    self.refutation += learning_rate
                        * if remaining <= 2.0 && target * side_sign > 0.0 {
                            1.0
                        } else {
                            0.0
                        };
                    plies_seen += 1;
                }
                self.uncertainty *= 0.999_f32.powi(game.moves.len() as i32);
            }
            self.training_steps += 1;
        }
        TrainSummary {
            training_steps: self.training_steps,
            games_seen: self.games_seen,
            plies_seen,
            policy_nonzero: self.policy.iter().filter(|value| **value != 0.0).count(),
        }
    }

    #[must_use]
    pub fn policy_score(&self, mv: Move) -> f32 {
        self.policy
            .get(usize::from(mv.packed_id()))
            .copied()
            .unwrap_or(0.0)
    }

    #[must_use]
    pub fn score_legal_moves(&self, position: &Position) -> Vec<PolicyMoveScore> {
        let mut output = Vec::new();
        let mut workspace = PolicyMoveWorkspace::default();
        self.score_legal_moves_into(position, &mut output, &mut workspace);
        output
    }

    pub fn score_legal_moves_into(
        &self,
        position: &Position,
        output: &mut Vec<PolicyMoveScore>,
        workspace: &mut PolicyMoveWorkspace,
    ) -> PolicyMoveMetrics {
        workspace.legal_moves.clear();
        position.legal_moves_into(&mut workspace.legal_moves);
        self.write_policy_hidden(position, workspace);
        write_policy_move_scores(self, &workspace.hidden, &workspace.legal_moves, output);
        PolicyMoveMetrics {
            moves: workspace.legal_moves.len(),
            input_values: workspace.input.len(),
            hidden_values: workspace.hidden.len(),
        }
    }

    pub fn score_legal_move_slice_into(
        &self,
        position: &Position,
        legal_moves: &[Move],
        output: &mut Vec<PolicyMoveScore>,
        workspace: &mut PolicyMoveWorkspace,
    ) -> PolicyMoveMetrics {
        self.write_policy_hidden(position, workspace);
        write_policy_move_scores(self, &workspace.hidden, legal_moves, output);
        PolicyMoveMetrics {
            moves: legal_moves.len(),
            input_values: workspace.input.len(),
            hidden_values: workspace.hidden.len(),
        }
    }

    pub fn order_legal_moves_by_policy(
        &self,
        position: &Position,
        legal_moves: &mut [Move],
        workspace: &mut PolicyMoveWorkspace,
    ) -> PolicyMoveMetrics {
        self.write_policy_hidden(position, workspace);
        write_policy_move_scores(self, &workspace.hidden, legal_moves, &mut workspace.scores);
        workspace.scores.sort_by(policy_score_descending);
        for (mv, scored) in legal_moves.iter_mut().zip(workspace.scores.iter().copied()) {
            *mv = scored.mv;
        }
        PolicyMoveMetrics {
            moves: legal_moves.len(),
            input_values: workspace.input.len(),
            hidden_values: workspace.hidden.len(),
        }
    }

    pub fn ordered_legal_moves_into(
        &self,
        position: &Position,
        output: &mut Vec<Move>,
        workspace: &mut PolicyMoveWorkspace,
    ) -> PolicyMoveMetrics {
        workspace.legal_moves.clear();
        position.legal_moves_into(&mut workspace.legal_moves);
        output.clear();
        output.extend_from_slice(&workspace.legal_moves);
        self.order_legal_moves_by_policy(position, output, workspace)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
            }
        }
        let mut file = File::create(path).map_err(|err| err.to_string())?;
        file.write_all(MODEL_MAGIC).map_err(|err| err.to_string())?;
        write_u32(&mut file, VERSION)?;
        write_u64(&mut file, self.training_steps)?;
        write_u64(&mut file, self.games_seen)?;
        write_vec_f32(&mut file, &self.input_hidden)?;
        write_vec_f32(&mut file, &self.hidden_bias)?;
        write_vec_f32(&mut file, &self.policy_head)?;
        for value in &self.policy {
            write_f32(&mut file, *value)?;
        }
        write_vec_f32(&mut file, &self.wdl_head)?;
        for value in self.wdl {
            write_f32(&mut file, value)?;
        }
        write_vec_f32(&mut file, &self.moves_left_head)?;
        write_f32(&mut file, self.moves_left)?;
        write_vec_f32(&mut file, &self.uncertainty_head)?;
        write_f32(&mut file, self.uncertainty)?;
        write_vec_f32(&mut file, &self.risk_head)?;
        write_f32(&mut file, self.risk)?;
        write_vec_f32(&mut file, &self.refutation_head)?;
        write_f32(&mut file, self.refutation)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|err| err.to_string())?;
        let mut magic = [0_u8; 8];
        file.read_exact(&mut magic).map_err(|err| err.to_string())?;
        if &magic != MODEL_MAGIC {
            return Err("bad ArK model magic".to_string());
        }
        let version = read_u32(&mut file)?;
        if version != VERSION {
            return Err(format!("unsupported ArK model version: {version}"));
        }
        let training_steps = read_u64(&mut file)?;
        let games_seen = read_u64(&mut file)?;
        let input_hidden = read_vec_f32(&mut file, INPUT_SIZE * HIDDEN_SIZE, "input_hidden")?;
        let hidden_bias = read_vec_f32(&mut file, HIDDEN_SIZE, "hidden_bias")?;
        let policy_head = read_vec_f32(&mut file, HIDDEN_SIZE * POLICY_SIZE, "policy_head")?;
        let mut policy = vec![0.0; POLICY_SIZE];
        for value in &mut policy {
            *value = read_f32(&mut file)?;
        }
        let wdl_head = read_vec_f32(&mut file, HIDDEN_SIZE * 3, "wdl_head")?;
        let mut wdl = [0.0; 3];
        for value in &mut wdl {
            *value = read_f32(&mut file)?;
        }
        let moves_left_head = read_vec_f32(&mut file, HIDDEN_SIZE, "moves_left_head")?;
        let moves_left = read_f32(&mut file)?;
        let uncertainty_head = read_vec_f32(&mut file, HIDDEN_SIZE, "uncertainty_head")?;
        let uncertainty = read_f32(&mut file)?;
        let risk_head = read_vec_f32(&mut file, HIDDEN_SIZE, "risk_head")?;
        let risk = read_f32(&mut file)?;
        let refutation_head =
            read_vec_f32(&mut file, HIDDEN_SIZE * POLICY_SIZE, "refutation_head")?;
        Ok(Self {
            training_steps,
            games_seen,
            input_hidden,
            hidden_bias,
            policy_head,
            policy,
            wdl_head,
            wdl,
            moves_left_head,
            moves_left,
            uncertainty_head,
            uncertainty,
            risk_head,
            risk,
            refutation_head,
            refutation: read_f32(&mut file)?,
        })
    }

    fn hidden(&self, input: &[f32]) -> Vec<f32> {
        let mut hidden = vec![0.0; HIDDEN_SIZE];
        self.write_hidden(input, &mut hidden);
        hidden
    }

    fn write_hidden(&self, input: &[f32], hidden: &mut [f32]) {
        for (hidden_index, hidden_value) in hidden.iter_mut().enumerate() {
            let mut sum = self.hidden_bias[hidden_index];
            for (input_index, value) in input.iter().copied().enumerate() {
                sum += value * self.input_hidden[input_index * HIDDEN_SIZE + hidden_index];
            }
            *hidden_value = sum.max(0.0);
        }
    }

    fn fill_output(&self, hidden: &[f32], legal: &[Move], output: &mut ModelOutput) {
        output.policy.clear();
        output.policy.reserve(legal.len());
        output.refutation.clear();
        output.refutation.reserve(legal.len());

        for mv in legal.iter().copied() {
            let packed = mv.packed_id();
            output.policy.push((
                packed,
                self.move_head_score(hidden, packed, &self.policy_head) + self.policy_score(mv),
            ));
            output.refutation.push((
                packed,
                self.move_head_score(hidden, packed, &self.refutation_head) + self.refutation,
            ));
        }

        output.wdl = self.wdl_logits_from_hidden(hidden);
        output.moves_left = dot_scalar(hidden, &self.moves_left_head) + self.moves_left;
        output.uncertainty = sigmoid(dot_scalar(hidden, &self.uncertainty_head) + self.uncertainty);
        output.risk = sigmoid(dot_scalar(hidden, &self.risk_head) + self.risk);
    }

    fn wdl_logits_from_hidden(&self, hidden: &[f32]) -> [f32; 3] {
        [
            dot_head(hidden, &self.wdl_head, 0) + self.wdl[0],
            dot_head(hidden, &self.wdl_head, 1) + self.wdl[1],
            dot_head(hidden, &self.wdl_head, 2) + self.wdl[2],
        ]
    }

    fn move_head_score(&self, hidden: &[f32], packed: u16, weights: &[f32]) -> f32 {
        let offset = usize::from(packed) * HIDDEN_SIZE;
        hidden
            .iter()
            .copied()
            .enumerate()
            .map(|(index, value)| value * weights[offset + index])
            .sum()
    }

    fn write_policy_hidden(&self, position: &Position, workspace: &mut PolicyMoveWorkspace) {
        workspace.input.clear();
        workspace.input.resize(INPUT_SIZE, 0.0);
        encode_position_into(position, &mut workspace.input);
        workspace.hidden.clear();
        workspace.hidden.resize(HIDDEN_SIZE, 0.0);
        self.write_hidden(&workspace.input, &mut workspace.hidden);
    }

    fn write_wdl_hidden(&self, position: &Position, workspace: &mut WdlEvaluationWorkspace) {
        workspace.input.clear();
        workspace.input.resize(INPUT_SIZE, 0.0);
        encode_position_into(position, &mut workspace.input);
        workspace.hidden.clear();
        workspace.hidden.resize(HIDDEN_SIZE, 0.0);
        self.write_hidden(&workspace.input, &mut workspace.hidden);
    }
}

impl ModelOutput {
    fn empty() -> Self {
        Self {
            policy: Vec::new(),
            wdl: [0.0; 3],
            moves_left: 0.0,
            uncertainty: 0.0,
            risk: 0.0,
            refutation: Vec::new(),
        }
    }
}

fn resize_outputs(outputs: &mut Vec<ModelOutput>, len: usize) {
    while outputs.len() < len {
        outputs.push(ModelOutput::empty());
    }
    outputs.truncate(len);
}

fn write_policy_move_scores(
    model: &ForgeModel,
    hidden: &[f32],
    legal_moves: &[Move],
    output: &mut Vec<PolicyMoveScore>,
) {
    output.clear();
    output.reserve(legal_moves.len());
    for mv in legal_moves.iter().copied() {
        let packed = mv.packed_id();
        output.push(PolicyMoveScore {
            mv,
            packed,
            score: model.move_head_score(hidden, packed, &model.policy_head)
                + model.policy_score(mv),
        });
    }
}

fn policy_score_descending(left: &PolicyMoveScore, right: &PolicyMoveScore) -> std::cmp::Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| left.packed.cmp(&right.packed))
}

pub fn write_replay(path: &Path, games: &[GameRecord]) -> Result<ReplaySummary, String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
    }
    let mut file = File::create(path).map_err(|err| err.to_string())?;
    file.write_all(REPLAY_MAGIC)
        .map_err(|err| err.to_string())?;
    write_u32(&mut file, VERSION)?;
    write_u32(&mut file, games.len() as u32)?;
    let mut plies = 0_u64;
    for game in games {
        file.write_all(&[game.result.white_score() as u8])
            .map_err(|err| err.to_string())?;
        write_u16(&mut file, game.moves.len() as u16)?;
        for mv in &game.moves {
            write_u16(&mut file, *mv)?;
            plies += 1;
        }
    }
    Ok(ReplaySummary {
        games: games.len() as u32,
        plies,
    })
}

pub fn read_replay(path: &Path) -> Result<Vec<GameRecord>, String> {
    let mut file = File::open(path).map_err(|err| err.to_string())?;
    let mut magic = [0_u8; 8];
    file.read_exact(&mut magic).map_err(|err| err.to_string())?;
    if &magic != REPLAY_MAGIC {
        return Err("bad ArK replay magic".to_string());
    }
    let version = read_u32(&mut file)?;
    if version != VERSION {
        return Err(format!("unsupported ArK replay version: {version}"));
    }
    let games = read_u32(&mut file)?;
    let mut records = Vec::with_capacity(games as usize);
    for _ in 0..games {
        let mut result = [0_u8; 1];
        file.read_exact(&mut result)
            .map_err(|err| err.to_string())?;
        let result = match result[0] as i8 {
            1 => GameOutcome::WhiteWin,
            0 => GameOutcome::Draw,
            -1 => GameOutcome::BlackWin,
            other => return Err(format!("bad replay result: {other}")),
        };
        let plies = read_u16(&mut file)?;
        let mut moves = Vec::with_capacity(plies as usize);
        for _ in 0..plies {
            moves.push(read_u16(&mut file)?);
        }
        records.push(GameRecord { result, moves });
    }
    Ok(records)
}

pub fn validate_replay(games: &[GameRecord]) -> Result<ReplayValidation, String> {
    let mut validation = ReplayValidation {
        games: games.len() as u32,
        plies: 0,
        illegal_moves: 0,
        unhandled_terminal_states: 0,
    };
    for game in games {
        let mut state = GameState::startpos().map_err(|err| format!("startpos failed: {err:?}"))?;
        for packed in &game.moves {
            if state.outcome().is_some() {
                validation.unhandled_terminal_states += 1;
                break;
            }
            let Some(mv) = state.position().move_from_packed_id(*packed) else {
                validation.illegal_moves += 1;
                break;
            };
            state.make_move(mv);
            validation.plies += 1;
        }
    }
    Ok(validation)
}

pub fn result_for_position(result: GameOutcome, color: Color) -> f32 {
    let white = f32::from(result.white_score());
    if color == Color::White {
        white
    } else {
        -white
    }
}

#[must_use]
pub fn wdl_logits_to_probabilities(logits: [f32; 3]) -> WdlProbabilities {
    let positive_infinity_count = logits
        .iter()
        .filter(|value| value.is_infinite() && value.is_sign_positive())
        .count();
    if positive_infinity_count > 0 {
        let value = 1.0 / positive_infinity_count as f32;
        return WdlProbabilities {
            white_win: if is_positive_infinity(logits[0]) {
                value
            } else {
                0.0
            },
            draw: if is_positive_infinity(logits[1]) {
                value
            } else {
                0.0
            },
            black_win: if is_positive_infinity(logits[2]) {
                value
            } else {
                0.0
            },
        };
    }
    if logits.iter().any(|value| value.is_nan()) {
        return uniform_wdl_probabilities();
    }

    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !max.is_finite() {
        return uniform_wdl_probabilities();
    }

    let white_win = (logits[0] - max).exp();
    let draw = (logits[1] - max).exp();
    let black_win = (logits[2] - max).exp();
    let sum = white_win + draw + black_win;
    if !sum.is_finite() || sum <= 0.0 {
        return uniform_wdl_probabilities();
    }

    WdlProbabilities {
        white_win: white_win / sum,
        draw: draw / sum,
        black_win: black_win / sum,
    }
}

#[must_use]
pub fn wdl_probabilities_to_white_centipawns(probabilities: WdlProbabilities) -> i32 {
    let expectation = probabilities.expected_white_score();
    if !expectation.is_finite() {
        return 0;
    }
    (expectation.clamp(-1.0, 1.0) * WDL_EXPECTATION_TO_CENTIPAWNS as f32).round() as i32
}

#[must_use]
pub const fn white_centipawns_to_side_to_move_centipawns(
    white_perspective_centipawns: i32,
    side_to_move: Color,
) -> i32 {
    match side_to_move {
        Color::White => white_perspective_centipawns,
        Color::Black => white_perspective_centipawns.saturating_neg(),
    }
}

fn write_u16(file: &mut File, value: u16) -> Result<(), String> {
    file.write_all(&value.to_le_bytes())
        .map_err(|err| err.to_string())
}

fn write_u32(file: &mut File, value: u32) -> Result<(), String> {
    file.write_all(&value.to_le_bytes())
        .map_err(|err| err.to_string())
}

fn write_u64(file: &mut File, value: u64) -> Result<(), String> {
    file.write_all(&value.to_le_bytes())
        .map_err(|err| err.to_string())
}

fn write_f32(file: &mut File, value: f32) -> Result<(), String> {
    file.write_all(&value.to_le_bytes())
        .map_err(|err| err.to_string())
}

fn write_vec_f32(file: &mut File, values: &[f32]) -> Result<(), String> {
    write_u32(file, values.len() as u32)?;
    for value in values {
        write_f32(file, *value)?;
    }
    Ok(())
}

fn read_u16(file: &mut File) -> Result<u16, String> {
    let mut bytes = [0_u8; 2];
    file.read_exact(&mut bytes).map_err(|err| err.to_string())?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(file: &mut File) -> Result<u32, String> {
    let mut bytes = [0_u8; 4];
    file.read_exact(&mut bytes).map_err(|err| err.to_string())?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(file: &mut File) -> Result<u64, String> {
    let mut bytes = [0_u8; 8];
    file.read_exact(&mut bytes).map_err(|err| err.to_string())?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_f32(file: &mut File) -> Result<f32, String> {
    let mut bytes = [0_u8; 4];
    file.read_exact(&mut bytes).map_err(|err| err.to_string())?;
    Ok(f32::from_le_bytes(bytes))
}

fn read_vec_f32(file: &mut File, expected: usize, name: &str) -> Result<Vec<f32>, String> {
    let len = read_u32(file)? as usize;
    if len != expected {
        return Err(format!("bad {name} length: expected {expected}, got {len}"));
    }
    let mut values = Vec::with_capacity(len);
    for _ in 0..len {
        values.push(read_f32(file)?);
    }
    Ok(values)
}

pub fn game_from_uci(result: GameOutcome, moves: &[String]) -> Result<GameRecord, String> {
    let mut position = Position::startpos().map_err(|err| format!("startpos failed: {err:?}"))?;
    let mut packed = Vec::with_capacity(moves.len());
    for text in moves {
        let Some(mv) = position.move_from_uci(text) else {
            return Err(format!("illegal UCI move in game: {text}"));
        };
        packed.push(mv.packed_id());
        position = position.make_move(mv);
    }
    Ok(GameRecord {
        result,
        moves: packed,
    })
}

fn encode_position(position: &Position) -> Vec<f32> {
    let mut features = vec![0.0; INPUT_SIZE];
    encode_position_into(position, &mut features);
    features
}

fn encode_position_into(position: &Position, features: &mut [f32]) {
    features.fill(0.0);
    for index in 0..64 {
        let Some(square) = ark_core::Square::new(index as u8) else {
            continue;
        };
        let Some(piece) = position.piece_at(square) else {
            continue;
        };
        let color_offset = if piece.color == Color::White { 0 } else { 6 };
        let kind_offset = match piece.kind {
            PieceKind::Pawn => 0,
            PieceKind::Knight => 1,
            PieceKind::Bishop => 2,
            PieceKind::Rook => 3,
            PieceKind::Queen => 4,
            PieceKind::King => 5,
        };
        features[(color_offset + kind_offset) * 64 + index] = 1.0;
    }
}

fn init_weights(len: usize, seed: u64) -> Vec<f32> {
    (0..len)
        .map(|index| {
            let bits = splitmix64(seed ^ index as u64);
            let centered = (bits % 2001) as f32 - 1000.0;
            centered / 1_000_000.0
        })
        .collect()
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn dot_head(hidden: &[f32], weights: &[f32], head: usize) -> f32 {
    let offset = head * HIDDEN_SIZE;
    dot_scalar(hidden, &weights[offset..offset + HIDDEN_SIZE])
}

fn dot_scalar(hidden: &[f32], weights: &[f32]) -> f32 {
    hidden
        .iter()
        .copied()
        .zip(weights.iter().copied())
        .map(|(value, weight)| value * weight)
        .sum()
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn wdl_leaf_evaluation_from_logits(logits: [f32; 3], side_to_move: Color) -> WdlLeafEvaluation {
    let probabilities = wdl_logits_to_probabilities(logits);
    let white_perspective_centipawns = wdl_probabilities_to_white_centipawns(probabilities);
    WdlLeafEvaluation {
        logits,
        probabilities,
        white_perspective_centipawns,
        side_to_move_centipawns: white_centipawns_to_side_to_move_centipawns(
            white_perspective_centipawns,
            side_to_move,
        ),
    }
}

fn uniform_wdl_probabilities() -> WdlProbabilities {
    WdlProbabilities {
        white_win: 1.0 / 3.0,
        draw: 1.0 / 3.0,
        black_win: 1.0 / 3.0,
    }
}

fn is_positive_infinity(value: f32) -> bool {
    value.is_infinite() && value.is_sign_positive()
}
