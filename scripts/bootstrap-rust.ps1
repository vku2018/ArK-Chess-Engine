param(
  [string]$Toolchain = "1.79.0-x86_64-pc-windows-gnu"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Tools = Join-Path $Root ".tools"
$RustupHome = Join-Path $Tools "rustup-home"
$CargoHome = Join-Path $Tools "cargo-home"
$CargoBin = Join-Path $CargoHome "bin"
$Cargo = Join-Path $CargoBin "cargo.exe"
$Rustup = Join-Path $CargoBin "rustup.exe"
$HostTriple = "x86_64-pc-windows-gnu"
$RustLld = Join-Path $RustupHome "toolchains\$Toolchain\lib\rustlib\$HostTriple\bin\rust-lld.exe"

Set-Location $Root
New-Item -ItemType Directory -Force $Tools, $RustupHome, $CargoHome | Out-Null

$env:RUSTUP_HOME = $RustupHome
$env:CARGO_HOME = $CargoHome
$env:PATH = "$CargoBin;$env:PATH"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $RustLld

if (-not (Test-Path $Rustup)) {
  $Installer = Join-Path $Tools "rustup-init.exe"
  $Url = "https://win.rustup.rs/x86_64"
  Write-Host "Downloading project-local rustup: $Url"
  Invoke-WebRequest -Uri $Url -OutFile $Installer
  & $Installer -y --no-modify-path --profile minimal --default-toolchain $Toolchain
  if ($LASTEXITCODE -ne 0) {
    throw "rustup-init failed"
  }
}

& $Rustup toolchain install $Toolchain --profile minimal --component clippy --component rustfmt
if ($LASTEXITCODE -ne 0) {
  throw "rust toolchain install failed"
}

if (-not (Test-Path -LiteralPath $RustLld)) {
  throw "rust-lld missing from local toolchain: $RustLld"
}

& $Cargo "+$Toolchain" --version
& $Cargo "+$Toolchain" test
if ($LASTEXITCODE -ne 0) {
  throw "cargo test failed"
}
Write-Host "Rust bootstrap complete."
