use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use ark_core::{
    search_with_context_and_stopper, Color, Position, SearchRequest, SearchResult, StopReason,
};

const DEFAULT_SEARCH_DEPTH: u32 = 1;
const TIMED_SEARCH_DEPTH: u32 = 128;
const QUIT_SEARCH_GRACE: Duration = Duration::from_millis(500);
const QUIT_PRE_CANCEL_GRACE: Duration = Duration::from_millis(25);

pub fn run_uci() -> Result<(), String> {
    let (event_tx, event_rx) = mpsc::channel();
    spawn_input_reader(event_tx.clone());

    let mut stdout = io::stdout();
    let mut engine = UciEngine::new()?;

    loop {
        match event_rx.recv().map_err(|err| err.to_string())? {
            EngineEvent::InputLine(line) => {
                let should_quit = engine.handle_line(&line, &event_tx, &event_rx, &mut stdout)?;
                stdout.flush().map_err(|err| err.to_string())?;
                if should_quit {
                    break;
                }
            }
            EngineEvent::InputClosed => {
                engine.drain_active_search(&event_rx, &mut stdout, QUIT_SEARCH_GRACE)?;
                break;
            }
            EngineEvent::InputError(err) => return Err(err),
            EngineEvent::SearchFinished(finished) => {
                engine.complete_search(*finished, &mut stdout)?;
                stdout.flush().map_err(|err| err.to_string())?;
            }
        }
    }

    Ok(())
}

fn spawn_input_reader(event_tx: mpsc::Sender<EngineEvent>) {
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let event = match line {
                Ok(line) => EngineEvent::InputLine(line),
                Err(err) => EngineEvent::InputError(err.to_string()),
            };
            if event_tx.send(event).is_err() {
                return;
            }
        }
        let _ = event_tx.send(EngineEvent::InputClosed);
    });
}

#[derive(Debug)]
enum EngineEvent {
    InputLine(String),
    InputClosed,
    InputError(String),
    SearchFinished(Box<SearchFinished>),
}

#[derive(Debug)]
struct UciEngine {
    position: Position,
    active: Option<ActiveSearch>,
    next_search_id: u64,
}

impl UciEngine {
    fn new() -> Result<Self, String> {
        Ok(Self {
            position: Position::startpos().map_err(|err| format!("startpos failed: {err:?}"))?,
            active: None,
            next_search_id: 1,
        })
    }

    fn handle_line(
        &mut self,
        line: &str,
        event_tx: &mpsc::Sender<EngineEvent>,
        event_rx: &mpsc::Receiver<EngineEvent>,
        stdout: &mut impl Write,
    ) -> Result<bool, String> {
        let command = line.trim();
        if command.is_empty() {
            return Ok(false);
        }

        let command = match UciCommand::parse(command) {
            Ok(command) => command,
            Err(err) => {
                write_uci_error(stdout, &err)?;
                return Ok(false);
            }
        };

        match command {
            UciCommand::Uci => {
                writeln!(stdout, "id name ArK-V4 Forge").map_err(|err| err.to_string())?;
                writeln!(stdout, "id author Ark Contributors").map_err(|err| err.to_string())?;
                writeln!(stdout, "uciok").map_err(|err| err.to_string())?;
            }
            UciCommand::IsReady => {
                writeln!(stdout, "readyok").map_err(|err| err.to_string())?;
            }
            UciCommand::UciNewGame => {
                self.cancel_active_search();
                self.position =
                    Position::startpos().map_err(|err| format!("startpos failed: {err:?}"))?;
            }
            UciCommand::Position(position_command) => match position_command.position() {
                Ok(position) => self.position = position,
                Err(err) => write_uci_error(stdout, &err)?,
            },
            UciCommand::Go(go_command) => {
                self.start_search(go_command, event_tx.clone());
            }
            UciCommand::Stop => {
                self.stop_active_search();
            }
            UciCommand::Quit => {
                self.cancel_and_drain_active_search(event_rx, stdout, QUIT_SEARCH_GRACE)?;
                return Ok(true);
            }
            UciCommand::Ignored => {}
        }

        Ok(false)
    }

    fn start_search(&mut self, go_command: GoCommand, event_tx: mpsc::Sender<EngineEvent>) {
        self.cancel_active_search();
        let id = self.next_search_id;
        self.next_search_id = self.next_search_id.saturating_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let position = self.position.clone();
        self.active = Some(ActiveSearch {
            id,
            cancel: Arc::clone(&cancel),
        });

        thread::spawn(move || {
            let finished = run_search_job(id, position, go_command, cancel);
            let _ = event_tx.send(EngineEvent::SearchFinished(Box::new(finished)));
        });
    }

    fn stop_active_search(&self) {
        if let Some(active) = &self.active {
            active.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn cancel_active_search(&mut self) {
        self.stop_active_search();
        self.active = None;
    }

    fn complete_search(
        &mut self,
        finished: SearchFinished,
        stdout: &mut impl Write,
    ) -> Result<(), String> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        if active.id != finished.id {
            return Ok(());
        }
        write_search_report(stdout, &finished.report)?;
        self.active = None;
        Ok(())
    }

    fn drain_active_search(
        &mut self,
        event_rx: &mpsc::Receiver<EngineEvent>,
        stdout: &mut impl Write,
        grace: Duration,
    ) -> Result<(), String> {
        if self.active.is_none() {
            return Ok(());
        }

        let pre_cancel = QUIT_PRE_CANCEL_GRACE.min(grace);
        let pre_cancel_deadline = Instant::now() + pre_cancel;
        while self.active.is_some() && Instant::now() < pre_cancel_deadline {
            let timeout = pre_cancel_deadline.saturating_duration_since(Instant::now());
            match event_rx.recv_timeout(timeout) {
                Ok(EngineEvent::SearchFinished(finished)) => {
                    self.complete_search(*finished, stdout)?;
                    stdout.flush().map_err(|err| err.to_string())?;
                }
                Ok(EngineEvent::InputError(err)) => return Err(err),
                Ok(EngineEvent::InputLine(_) | EngineEvent::InputClosed) => {}
                Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }
        }

        self.cancel_and_drain_active_search(event_rx, stdout, grace.saturating_sub(pre_cancel))
    }

    fn cancel_and_drain_active_search(
        &mut self,
        event_rx: &mpsc::Receiver<EngineEvent>,
        stdout: &mut impl Write,
        grace: Duration,
    ) -> Result<(), String> {
        self.stop_active_search();
        let deadline = Instant::now() + grace;
        while self.active.is_some() {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let timeout = deadline.duration_since(now);
            match event_rx.recv_timeout(timeout) {
                Ok(EngineEvent::SearchFinished(finished)) => {
                    self.complete_search(*finished, stdout)?;
                    stdout.flush().map_err(|err| err.to_string())?;
                }
                Ok(EngineEvent::InputError(err)) => return Err(err),
                Ok(EngineEvent::InputLine(_) | EngineEvent::InputClosed) => {}
                Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct ActiveSearch {
    id: u64,
    cancel: Arc<AtomicBool>,
}

#[derive(Debug)]
struct SearchFinished {
    id: u64,
    report: SearchReport,
}

#[derive(Debug)]
struct SearchReport {
    result: SearchResult,
    elapsed_ms: u128,
    nodes: u64,
    depth_reached: u32,
    chosen_budget_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum UciCommand {
    Uci,
    IsReady,
    UciNewGame,
    Position(PositionCommand),
    Go(GoCommand),
    Stop,
    Quit,
    Ignored,
}

impl UciCommand {
    fn parse(command: &str) -> Result<Self, String> {
        let tokens = command.split_whitespace().collect::<Vec<_>>();
        let Some((head, tail)) = tokens.split_first() else {
            return Ok(Self::Ignored);
        };
        match *head {
            "uci" => Ok(Self::Uci),
            "isready" => Ok(Self::IsReady),
            "ucinewgame" => Ok(Self::UciNewGame),
            "position" => PositionCommand::parse(tail).map(Self::Position),
            "go" => GoCommand::parse(tail).map(Self::Go),
            "stop" => Ok(Self::Stop),
            "quit" => Ok(Self::Quit),
            "setoption" | "debug" | "ponderhit" => Ok(Self::Ignored),
            other => Err(format!("unsupported UCI command: {other}")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PositionCommand {
    base: PositionBase,
    moves: Vec<String>,
}

impl PositionCommand {
    fn parse(tokens: &[&str]) -> Result<Self, String> {
        let Some((head, tail)) = tokens.split_first() else {
            return Err("position needs startpos or fen".to_string());
        };

        let (base, rest) = match *head {
            "startpos" => (PositionBase::Startpos, tail),
            "fen" => {
                if tail.len() < 6 {
                    return Err("position fen needs six FEN fields".to_string());
                }
                let fen = tail[..6].join(" ");
                (PositionBase::Fen(fen), &tail[6..])
            }
            other => {
                return Err(format!(
                    "unsupported position source: {other}; expected startpos or fen"
                ))
            }
        };

        let moves = if rest.is_empty() {
            Vec::new()
        } else {
            let Some((moves_marker, move_tokens)) = rest.split_first() else {
                return Err("position moves parser failed".to_string());
            };
            if *moves_marker != "moves" {
                return Err(format!(
                    "unexpected position token: {moves_marker}; expected moves"
                ));
            }
            move_tokens.iter().map(|text| (*text).to_string()).collect()
        };

        Ok(Self { base, moves })
    }

    fn position(&self) -> Result<Position, String> {
        let mut position = match &self.base {
            PositionBase::Startpos => {
                Position::startpos().map_err(|err| format!("startpos failed: {err:?}"))?
            }
            PositionBase::Fen(fen) => {
                Position::from_fen(fen).map_err(|err| format!("bad FEN: {err:?}"))?
            }
        };

        for text in &self.moves {
            validate_uci_move_text(text)?;
            position = position
                .make_uci_move(text)
                .ok_or_else(|| format!("illegal UCI move in position command: {text}"))?;
        }

        Ok(position)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PositionBase {
    Startpos,
    Fen(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct GoCommand {
    depth: Option<u32>,
    nodes: Option<u64>,
    movetime_ms: Option<u64>,
    infinite: bool,
    wtime_ms: Option<u64>,
    btime_ms: Option<u64>,
    winc_ms: Option<u64>,
    binc_ms: Option<u64>,
    movestogo: Option<u32>,
    ponder: bool,
}

impl GoCommand {
    fn parse(tokens: &[&str]) -> Result<Self, String> {
        let mut command = Self::default();
        let mut index = 0;
        while index < tokens.len() {
            match tokens[index] {
                "depth" => {
                    command.depth = Some(parse_u32_field(tokens, index, "depth", false)?);
                    index += 2;
                }
                "nodes" => {
                    command.nodes = Some(parse_u64_field(tokens, index, "nodes", false)?);
                    index += 2;
                }
                "movetime" => {
                    command.movetime_ms = Some(parse_u64_field(tokens, index, "movetime", false)?);
                    index += 2;
                }
                "wtime" => {
                    command.wtime_ms = Some(parse_u64_field(tokens, index, "wtime", true)?);
                    index += 2;
                }
                "btime" => {
                    command.btime_ms = Some(parse_u64_field(tokens, index, "btime", true)?);
                    index += 2;
                }
                "winc" => {
                    command.winc_ms = Some(parse_u64_field(tokens, index, "winc", true)?);
                    index += 2;
                }
                "binc" => {
                    command.binc_ms = Some(parse_u64_field(tokens, index, "binc", true)?);
                    index += 2;
                }
                "movestogo" => {
                    command.movestogo = Some(parse_u32_field(tokens, index, "movestogo", false)?);
                    index += 2;
                }
                "infinite" => {
                    command.infinite = true;
                    index += 1;
                }
                "ponder" => {
                    command.ponder = true;
                    index += 1;
                }
                other => return Err(format!("unsupported go token: {other}")),
            }
        }
        Ok(command)
    }

    fn effective_movetime_ms(&self, position: &Position) -> Option<u64> {
        if let Some(movetime_ms) = self.movetime_ms {
            return Some(movetime_ms.max(1));
        }

        let (time_ms, increment_ms) = match position.side_to_move() {
            Color::White => (self.wtime_ms, self.winc_ms),
            Color::Black => (self.btime_ms, self.binc_ms),
        };
        let time_ms = time_ms?;
        let usable = time_ms.saturating_sub(50);
        if usable == 0 {
            return Some(1);
        }
        let moves = u64::from(self.movestogo.unwrap_or(30).max(1));
        let budget = usable / moves + increment_ms.unwrap_or(0) / 2;
        Some(budget.clamp(1, usable))
    }

    fn target_depth(&self, movetime_ms: Option<u64>) -> u32 {
        if let Some(depth) = self.depth {
            return depth.max(1);
        }
        if self.infinite {
            return u32::MAX;
        }
        if self.nodes.is_some() || movetime_ms.is_some() {
            return TIMED_SEARCH_DEPTH;
        }
        DEFAULT_SEARCH_DEPTH
    }
}

fn run_search_job(
    id: u64,
    position: Position,
    go_command: GoCommand,
    cancel: Arc<AtomicBool>,
) -> SearchFinished {
    let started = Instant::now();
    let movetime_ms = go_command.effective_movetime_ms(&position);
    let target_depth = go_command.target_depth(movetime_ms);
    let mut next_depth = 1;
    let mut latest = None;
    let mut cumulative_nodes = 0_u64;
    let mut remaining_nodes = go_command.nodes;
    let mut depth_reached = 0_u32;
    let stopper = || cancel.load(Ordering::Relaxed);

    while next_depth <= target_depth {
        if cancel.load(Ordering::Relaxed) && latest.is_some() {
            break;
        }

        let remaining_time = remaining_movetime_ms(movetime_ms, started);
        if movetime_ms.is_some() && remaining_time.is_none() && latest.is_some() {
            break;
        }

        let request = SearchRequest {
            depth: next_depth,
            nodes: remaining_nodes,
            movetime_ms: remaining_time,
            seed: 1,
            ..SearchRequest::default()
        };
        let result = search_with_context_and_stopper(&position, &request, None, None, &stopper);
        cumulative_nodes = cumulative_nodes.saturating_add(result.nodes);
        depth_reached = depth_reached.max(result.depth_reached);
        if let Some(limit) = remaining_nodes {
            remaining_nodes = Some(limit.saturating_sub(result.nodes));
        }

        let stopped_by = result.trace.stopped_by;
        latest = Some(result);
        if matches!(
            stopped_by,
            StopReason::Nodes | StopReason::Time | StopReason::Terminal | StopReason::Cancelled
        ) {
            break;
        }
        if cancel.load(Ordering::Relaxed) || next_depth == u32::MAX {
            break;
        }
        next_depth = next_depth.saturating_add(1);
    }

    let mut result = latest.unwrap_or_else(|| {
        search_with_context_and_stopper(
            &position,
            &SearchRequest {
                depth: DEFAULT_SEARCH_DEPTH,
                seed: 1,
                ..SearchRequest::default()
            },
            None,
            None,
            &stopper,
        )
    });
    if let Some(chosen_budget_ms) = movetime_ms {
        result.trace.movetime_ms = Some(chosen_budget_ms);
    }
    let nodes = cumulative_nodes.max(result.nodes);
    let depth_reached = depth_reached.max(result.depth_reached);
    SearchFinished {
        id,
        report: SearchReport {
            result,
            elapsed_ms: started.elapsed().as_millis().max(1),
            nodes,
            depth_reached,
            chosen_budget_ms: movetime_ms,
        },
    }
}

fn remaining_movetime_ms(movetime_ms: Option<u64>, started: Instant) -> Option<u64> {
    let budget = movetime_ms?;
    let elapsed = started.elapsed().as_millis();
    if elapsed >= u128::from(budget) {
        return None;
    }
    let remaining = u128::from(budget) - elapsed;
    Some(u64::try_from(remaining).unwrap_or(u64::MAX).max(1))
}

fn write_search_report(stdout: &mut impl Write, report: &SearchReport) -> Result<(), String> {
    let result = &report.result;
    let best = result
        .best_move
        .map_or_else(|| "0000".to_string(), |mv| mv.to_string());
    let pv = result
        .pv
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let nps = report.nodes.saturating_mul(1000) / u64::try_from(report.elapsed_ms).unwrap_or(1);

    let budget = report
        .chosen_budget_ms
        .map_or_else(String::new, |budget_ms| format!(" budget_ms {budget_ms}"));

    if pv.is_empty() {
        writeln!(
            stdout,
            "info depth {} nodes {} time {} nps {}{} score cp {}",
            report.depth_reached, report.nodes, report.elapsed_ms, nps, budget, result.score
        )
        .map_err(|err| err.to_string())?;
    } else {
        writeln!(
            stdout,
            "info depth {} nodes {} time {} nps {}{} score cp {} pv {}",
            report.depth_reached, report.nodes, report.elapsed_ms, nps, budget, result.score, pv
        )
        .map_err(|err| err.to_string())?;
    }
    writeln!(stdout, "bestmove {best}").map_err(|err| err.to_string())
}

fn write_uci_error(stdout: &mut impl Write, message: &str) -> Result<(), String> {
    writeln!(
        stdout,
        "info string error {}",
        sanitize_info_string(message)
    )
    .map_err(|err| err.to_string())
}

fn sanitize_info_string(message: &str) -> String {
    message
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn validate_uci_move_text(text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    if !(bytes.len() == 4 || bytes.len() == 5) {
        return Err(format!("malformed UCI move: {text}"));
    }
    if !is_file(bytes[0]) || !is_rank(bytes[1]) || !is_file(bytes[2]) || !is_rank(bytes[3]) {
        return Err(format!("malformed UCI move: {text}"));
    }
    if bytes.len() == 5 && !matches!(bytes[4], b'q' | b'r' | b'b' | b'n') {
        return Err(format!("malformed UCI move: {text}"));
    }
    Ok(())
}

const fn is_file(byte: u8) -> bool {
    matches!(byte, b'a'..=b'h')
}

const fn is_rank(byte: u8) -> bool {
    matches!(byte, b'1'..=b'8')
}

fn parse_u32_field(
    tokens: &[&str],
    index: usize,
    name: &str,
    allow_zero: bool,
) -> Result<u32, String> {
    let Some(value) = tokens.get(index + 1) else {
        return Err(format!("go {name} needs a value"));
    };
    let parsed = value
        .parse::<u32>()
        .map_err(|_err| format!("go {name} must be an integer"))?;
    if !allow_zero && parsed == 0 {
        return Err(format!("go {name} must be greater than zero"));
    }
    Ok(parsed)
}

fn parse_u64_field(
    tokens: &[&str],
    index: usize,
    name: &str,
    allow_zero: bool,
) -> Result<u64, String> {
    let Some(value) = tokens.get(index + 1) else {
        return Err(format!("go {name} needs a value"));
    };
    let parsed = value
        .parse::<u64>()
        .map_err(|_err| format!("go {name} must be an integer"))?;
    if !allow_zero && parsed == 0 {
        return Err(format!("go {name} must be greater than zero"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_budget_uses_reserve_and_increment() -> Result<(), String> {
        let position = Position::startpos().map_err(|err| format!("{err:?}"))?;
        let command = GoCommand {
            wtime_ms: Some(1_000),
            btime_ms: Some(2_000),
            winc_ms: Some(20),
            binc_ms: Some(40),
            movestogo: Some(20),
            ..GoCommand::default()
        };

        assert_eq!(command.effective_movetime_ms(&position), Some(57));
        Ok(())
    }

    #[test]
    fn movetime_overrides_clock_budget() -> Result<(), String> {
        let position = Position::startpos().map_err(|err| format!("{err:?}"))?;
        let command = GoCommand {
            movetime_ms: Some(7),
            wtime_ms: Some(1_000),
            winc_ms: Some(20),
            movestogo: Some(20),
            ..GoCommand::default()
        };

        assert_eq!(command.effective_movetime_ms(&position), Some(7));
        Ok(())
    }

    #[test]
    fn uci_search_trace_records_chosen_movetime_budget() -> Result<(), String> {
        let position = Position::startpos().map_err(|err| format!("{err:?}"))?;
        let finished = run_search_job(
            1,
            position,
            GoCommand {
                nodes: Some(1),
                movetime_ms: Some(7),
                ..GoCommand::default()
            },
            Arc::new(AtomicBool::new(false)),
        );

        assert_eq!(finished.report.chosen_budget_ms, Some(7));
        assert_eq!(finished.report.result.trace.movetime_ms, Some(7));
        Ok(())
    }

    #[test]
    fn uci_search_trace_records_chosen_clock_budget() -> Result<(), String> {
        let position = Position::startpos().map_err(|err| format!("{err:?}"))?;
        let finished = run_search_job(
            1,
            position,
            GoCommand {
                nodes: Some(1),
                wtime_ms: Some(1_000),
                btime_ms: Some(1_000),
                winc_ms: Some(20),
                binc_ms: Some(20),
                movestogo: Some(20),
                ..GoCommand::default()
            },
            Arc::new(AtomicBool::new(false)),
        );

        assert_eq!(finished.report.chosen_budget_ms, Some(57));
        assert_eq!(finished.report.result.trace.movetime_ms, Some(57));
        Ok(())
    }
}
