param(
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$CargoArgs
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$CargoHome = Join-Path $Root ".tools\cargo-home"
$RustupHome = Join-Path $Root ".tools\rustup-home"
$CargoBin = Join-Path $CargoHome "bin"
$Cargo = Join-Path $CargoBin "cargo.exe"
$Toolchain = "1.79.0-x86_64-pc-windows-gnu"
$HostTriple = "x86_64-pc-windows-gnu"
$RustLld = Join-Path $RustupHome "toolchains\$Toolchain\lib\rustlib\$HostTriple\bin\rust-lld.exe"

if (-not (Test-Path -LiteralPath $Cargo)) {
  throw "local cargo is missing. Run scripts\bootstrap-rust.ps1 first."
}

Set-Location $Root
$env:CARGO_HOME = $CargoHome
$env:RUSTUP_HOME = $RustupHome
$env:PATH = "$CargoBin;$env:PATH"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $RustLld

& $Cargo "+$Toolchain" @CargoArgs
exit $LASTEXITCODE
