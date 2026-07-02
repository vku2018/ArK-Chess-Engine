use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn uci_startpos_moves_smoke_returns_legal_bestmove() -> Result<(), Box<dyn std::error::Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ark"))
        .arg("uci")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    {
        let stdin = child.stdin.as_mut().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "ark stdin closed")
        })?;
        writeln!(stdin, "uci")?;
        writeln!(stdin, "isready")?;
        writeln!(stdin, "position startpos moves e2e4 e7e5")?;
        writeln!(stdin, "go depth 2 nodes 5")?;
        writeln!(stdin, "quit")?;
    }

    let output = child.wait_with_output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("uciok"));
    assert!(stdout.contains("readyok"));
    assert!(stdout.contains("nodes 5"));
    assert!(stdout.lines().any(|line| line.starts_with("bestmove ")));
    Ok(())
}
