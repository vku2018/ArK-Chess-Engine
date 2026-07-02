$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
$CargoHome = Join-Path $Root ".tools\cargo-home"
$RustupHome = Join-Path $Root ".tools\rustup-home"
$CargoBin = Join-Path $CargoHome "bin"
$Cargo = Join-Path $CargoBin "cargo.exe"
$RustLld = Join-Path $RustupHome "toolchains\1.79.0-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\rust-lld.exe"

$env:CARGO_HOME = $CargoHome
$env:RUSTUP_HOME = $RustupHome
$env:PATH = "$CargoBin;$env:PATH"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $RustLld

function Assert-True($Condition, [string]$Message) {
  if (-not $Condition) {
    throw $Message
  }
}

function Invoke-ArkPerft([string]$Fen) {
  $Args = @(
    "run",
    "-p",
    "ark_cli",
    "--quiet",
    "--",
    "perft",
    "--fen",
    $Fen,
    "--depth",
    "1"
  )
  $PreviousErrorActionPreference = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  try {
    $Output = & $Cargo @Args 2>&1
    return [pscustomobject]@{
      ExitCode = $LASTEXITCODE
      Output = ($Output | Out-String)
    }
  }
  finally {
    $ErrorActionPreference = $PreviousErrorActionPreference
  }
}

$Valid = @(
  [pscustomobject]@{
    Name = "startpos"
    Fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
  },
  [pscustomobject]@{
    Name = "valid en passant target without capturer"
    Fen = "4k3/8/8/8/4P3/8/8/4K3 b - e3 0 1"
  },
  [pscustomobject]@{
    Name = "side to move in check"
    Fen = "4k3/8/8/8/8/8/4r3/4K3 w - - 0 1"
  }
)

$Invalid = @(
  [pscustomobject]@{
    Name = "zero rank digit"
    Fen = "4k3/8/8/8/8/8/8/4K0N2 w - - 0 1"
  },
  [pscustomobject]@{
    Name = "adjacent rank digits"
    Fen = "4k3/8/8/8/8/8/8/4K12 w - - 0 1"
  },
  [pscustomobject]@{
    Name = "multiple white kings"
    Fen = "4k3/8/8/8/8/8/4K3/4K3 w - - 0 1"
  },
  [pscustomobject]@{
    Name = "white pawn on eighth rank"
    Fen = "P3k3/8/8/8/8/8/8/4K3 w - - 0 1"
  },
  [pscustomobject]@{
    Name = "black pawn on first rank"
    Fen = "4k3/8/8/8/8/8/8/p3K3 w - - 0 1"
  },
  [pscustomobject]@{
    Name = "bad en passant rank"
    Fen = "4k3/8/8/8/8/8/8/4K3 w - e4 0 1"
  },
  [pscustomobject]@{
    Name = "occupied en passant target"
    Fen = "4k3/8/8/8/4P3/4N3/8/4K3 b - e3 0 1"
  },
  [pscustomobject]@{
    Name = "missing just-moved en passant pawn"
    Fen = "4k3/8/8/8/8/8/8/4K3 b - e3 0 1"
  },
  [pscustomobject]@{
    Name = "castling right with missing rook"
    Fen = "4k3/8/8/8/8/8/8/4K3 w K - 0 1"
  },
  [pscustomobject]@{
    Name = "castling right with wrong rook color"
    Fen = "4k3/8/8/8/8/8/8/4K2r w K - 0 1"
  },
  [pscustomobject]@{
    Name = "castling right with king off start square"
    Fen = "4k3/8/8/8/8/8/8/R2K3R w K - 0 1"
  },
  [pscustomobject]@{
    Name = "adjacent kings"
    Fen = "8/8/8/8/8/8/4k3/4K3 w - - 0 1"
  }
)

$Accepted = 0
foreach ($Case in $Valid) {
  $Result = Invoke-ArkPerft $Case.Fen
  Assert-True ($Result.ExitCode -eq 0) "valid FEN rejected: $($Case.Name)`n$($Result.Output)"
  $Accepted += 1
}

$Rejected = 0
foreach ($Case in $Invalid) {
  $Result = Invoke-ArkPerft $Case.Fen
  Assert-True ($Result.ExitCode -ne 0) "invalid FEN accepted: $($Case.Name)"
  Assert-True ($Result.Output.Contains("bad FEN")) "invalid FEN did not report bad FEN: $($Case.Name)`n$($Result.Output)"
  $Rejected += 1
}

Write-Output "fen_contract_guard: accepted=$Accepted rejected=$Rejected"
