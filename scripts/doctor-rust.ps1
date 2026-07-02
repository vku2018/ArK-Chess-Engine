$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$CargoHome = Join-Path $Root ".tools\cargo-home"
$RustupHome = Join-Path $Root ".tools\rustup-home"
$CargoBin = Join-Path $CargoHome "bin"
$Cargo = Join-Path $CargoBin "cargo.exe"
$RustLld = Join-Path $RustupHome "toolchains\1.79.0-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\rust-lld.exe"

Set-Location $Root
$env:CARGO_HOME = $CargoHome
$env:RUSTUP_HOME = $RustupHome
$env:PATH = "$CargoBin;$env:PATH"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $RustLld

Write-Host "ArK-V4 Forge doctor"
Write-Host "Root: $Root"
Write-Host "Cargo home: $CargoHome"
Write-Host "Rustup home: $RustupHome"

if (Test-Path $Cargo) {
  & $Cargo --version
} else {
  Write-Host "cargo: missing. Run scripts\\bootstrap-rust.ps1 after approval."
}

if (Test-Path -LiteralPath $RustLld) {
  Write-Host "linker: local rust-lld present"
} else {
  Write-Host "linker: missing until bootstrap completes"
}

Write-Host "Rust tooling check complete."
