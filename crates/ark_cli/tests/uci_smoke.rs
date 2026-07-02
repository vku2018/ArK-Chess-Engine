use std::io::{self, BufRead, BufReader, Write};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn run_uci_script(script: &str) -> Result<Output, Box<dyn std::error::Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ark"))
        .arg("uci")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "ark stdin was not piped"))?;
        stdin.write_all(script.as_bytes())?;
    }

    wait_with_timeout(child, Duration::from_secs(3))
}

fn run_uci_script_waiting_for_bestmove(
    before_wait: &str,
    after_wait: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ark"))
        .arg("uci")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "ark stdin was not piped"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "ark stdout was not piped"))?;
    let (line_tx, line_rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut output = String::new();
        for line in BufReader::new(stdout).lines() {
            let line = line?;
            let is_bestmove = line.starts_with("bestmove ");
            output.push_str(&line);
            output.push('\n');
            if is_bestmove {
                let _ = line_tx.send(());
            }
        }
        Ok::<String, io::Error>(output)
    });

    stdin.write_all(before_wait.as_bytes())?;
    line_rx.recv_timeout(Duration::from_millis(500))?;
    stdin.write_all(after_wait.as_bytes())?;
    drop(stdin);
    let output = wait_with_timeout(child, Duration::from_secs(3))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ark uci exited with {}\nstderr:\n{}", output.status, stderr).into());
    }
    Ok(reader.join().map_err(|_err| "stdout reader panicked")??)
}

fn run_uci_stop_sequence(
    before_stop: &str,
) -> Result<(String, Duration), Box<dyn std::error::Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ark"))
        .arg("uci")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "ark stdin was not piped"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "ark stdout was not piped"))?;
    let (line_tx, line_rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut output = String::new();
        for line in BufReader::new(stdout).lines() {
            let line = line?;
            let is_bestmove = line.starts_with("bestmove ");
            output.push_str(&line);
            output.push('\n');
            if is_bestmove {
                let _ = line_tx.send(Instant::now());
            }
        }
        Ok::<String, io::Error>(output)
    });

    stdin.write_all(before_stop.as_bytes())?;
    let stopped_at = Instant::now();
    stdin.write_all(b"stop\n")?;
    let bestmove_at = line_rx.recv_timeout(Duration::from_millis(500))?;
    stdin.write_all(b"quit\n")?;
    drop(stdin);
    let output = wait_with_timeout(child, Duration::from_secs(3))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ark uci exited with {}\nstderr:\n{}", output.status, stderr).into());
    }
    Ok((
        reader.join().map_err(|_err| "stdout reader panicked")??,
        bestmove_at.saturating_duration_since(stopped_at),
    ))
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Result<Output, Box<dyn std::error::Error>> {
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let output = child.wait_with_output()?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "ark uci timed out after {:?}\nstdout:\n{}\nstderr:\n{}",
                timeout, stdout, stderr
            )
            .into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn stdout_text(output: Output) -> Result<String, Box<dyn std::error::Error>> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ark uci exited with {}\nstderr:\n{}", output.status, stderr).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn bestmove_count(stdout: &str) -> usize {
    stdout
        .lines()
        .filter(|line| line.starts_with("bestmove "))
        .count()
}

fn bestmove(stdout: &str) -> Option<&str> {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("bestmove "))
}

#[test]
fn uci_startpos_moves_smoke_returns_legal_bestmove() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = run_uci_script_waiting_for_bestmove(
        "uci\nisready\nposition startpos moves e2e4 e7e5\ngo depth 2 nodes 5\n",
        "quit\n",
    )?;

    assert!(stdout.contains("id name ArK-V4 Forge"), "{stdout}");
    assert!(stdout.contains("uciok"), "{stdout}");
    assert!(stdout.contains("readyok"), "{stdout}");
    assert!(stdout.contains("nodes 5"), "{stdout}");
    assert_eq!(bestmove_count(&stdout), 1, "{stdout}");
    Ok(())
}

#[test]
fn uci_accepts_fen_moves_and_movetime() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = stdout_text(run_uci_script(
        "ucinewgame\nposition fen rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1 moves c7c5\ngo movetime 1\nquit\n",
    )?)?;

    assert!(stdout.contains("time "), "{stdout}");
    assert!(stdout.contains("budget_ms 1"), "{stdout}");
    assert_eq!(bestmove_count(&stdout), 1, "{stdout}");
    Ok(())
}

#[test]
fn uci_accepts_clock_fields() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = stdout_text(run_uci_script(
        "position startpos\ngo wtime 1000 btime 1000 winc 20 binc 20 movestogo 20\nquit\n",
    )?)?;

    assert_eq!(bestmove_count(&stdout), 1, "{stdout}");
    assert!(stdout.contains("info depth "), "{stdout}");
    assert!(stdout.contains("budget_ms 57"), "{stdout}");
    Ok(())
}

#[test]
fn uci_movetime_overrides_clock_budget() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = stdout_text(run_uci_script(
        "position startpos\ngo movetime 7 wtime 1000 btime 1000 winc 20 binc 20 movestogo 20\nquit\n",
    )?)?;

    assert!(stdout.contains("budget_ms 7"), "{stdout}");
    assert_eq!(bestmove_count(&stdout), 1, "{stdout}");
    Ok(())
}

#[test]
fn malformed_position_moves_report_error_and_keep_engine_alive(
) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = stdout_text(run_uci_script(
        "position startpos moves e2e9\nisready\nposition startpos moves e2e4\ngo depth 1\nquit\n",
    )?)?;

    assert!(
        stdout.contains("info string error malformed UCI move: e2e9"),
        "{stdout}"
    );
    assert!(stdout.contains("readyok"), "{stdout}");
    assert_eq!(bestmove_count(&stdout), 1, "{stdout}");
    Ok(())
}

#[test]
fn illegal_position_moves_report_error_and_keep_engine_alive(
) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = stdout_text(run_uci_script(
        "position startpos moves e2e5\nposition startpos\ngo depth 1\nquit\n",
    )?)?;

    assert!(
        stdout.contains("info string error illegal UCI move in position command: e2e5"),
        "{stdout}"
    );
    assert_eq!(bestmove_count(&stdout), 1, "{stdout}");
    Ok(())
}

#[test]
fn malformed_go_reports_error_and_keep_engine_alive() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = run_uci_script_waiting_for_bestmove(
        "position startpos\ngo depth nope\nisready\ngo nodes 3\n",
        "quit\n",
    )?;

    assert!(
        stdout.contains("info string error go depth must be an integer"),
        "{stdout}"
    );
    assert!(stdout.contains("readyok"), "{stdout}");
    assert!(stdout.contains("nodes 3"), "{stdout}");
    assert_eq!(bestmove_count(&stdout), 1, "{stdout}");
    Ok(())
}

#[test]
fn go_infinite_stops_and_returns_bestmove() -> Result<(), Box<dyn std::error::Error>> {
    let (stdout, stop_latency) = run_uci_stop_sequence("position startpos\ngo infinite\n")?;

    assert!(stop_latency <= Duration::from_millis(500), "{stdout}");
    assert_eq!(bestmove_count(&stdout), 1, "{stdout}");
    let bestmove = bestmove(&stdout).ok_or("missing bestmove")?;
    assert_ne!(bestmove, "0000", "{stdout}");
    assert!(bestmove.len() == 4 || bestmove.len() == 5, "{stdout}");
    Ok(())
}

#[test]
fn isready_responds_while_search_is_active() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = stdout_text(run_uci_script(
        "position startpos\ngo infinite\nisready\nstop\nquit\n",
    )?)?;

    assert!(stdout.contains("readyok"), "{stdout}");
    assert_eq!(bestmove_count(&stdout), 1, "{stdout}");
    Ok(())
}

#[test]
fn duplicate_stop_does_not_emit_second_bestmove() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = stdout_text(run_uci_script(
        "position startpos\ngo infinite\nstop\nstop\nquit\n",
    )?)?;

    assert_eq!(bestmove_count(&stdout), 1, "{stdout}");
    Ok(())
}

#[test]
fn quit_during_active_search_does_not_hang() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = stdout_text(run_uci_script("position startpos\ngo infinite\nquit\n")?)?;

    assert!(bestmove_count(&stdout) <= 1, "{stdout}");
    Ok(())
}
