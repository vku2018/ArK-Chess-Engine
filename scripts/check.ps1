param(
  [switch]$Release
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

function Resolve-Cargo {
  if ($env:ARK_CARGO) {
    return [pscustomobject]@{
      Command = $env:ARK_CARGO
      PrefixArgs = @()
    }
  }

  $GlobalCargo = Get-Command cargo -ErrorAction SilentlyContinue
  if ($null -ne $GlobalCargo) {
    return [pscustomobject]@{
      Command = $GlobalCargo.Source
      PrefixArgs = @()
    }
  }

  $CargoHome = Join-Path $Root ".tools\cargo-home"
  $RustupHome = Join-Path $Root ".tools\rustup-home"
  $CargoBin = Join-Path $CargoHome "bin"
  $LocalCargo = Join-Path $CargoBin "cargo.exe"
  $Toolchain = "1.79.0-x86_64-pc-windows-gnu"
  $HostTriple = "x86_64-pc-windows-gnu"
  $RustLld = Join-Path $RustupHome "toolchains\$Toolchain\lib\rustlib\$HostTriple\bin\rust-lld.exe"

  if (Test-Path -LiteralPath $LocalCargo) {
    $env:CARGO_HOME = $CargoHome
    $env:RUSTUP_HOME = $RustupHome
    $env:PATH = "$CargoBin;$env:PATH"
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $RustLld
    return [pscustomobject]@{
      Command = $LocalCargo
      PrefixArgs = @("+$Toolchain")
    }
  }

  throw "cargo not found. Install Rust or run scripts\bootstrap-rust.ps1."
}

function Invoke-Cargo {
  param(
    [string[]]$CargoArgs
  )

  $Args = @($script:Cargo.PrefixArgs) + $CargoArgs
  & $script:Cargo.Command @Args
}

function Invoke-Step {
  param(
    [string]$Name,
    [scriptblock]$Command
  )

  Write-Host "==> $Name"
  & $Command
  if ($LASTEXITCODE -ne 0) {
    throw "$Name failed"
  }
}

Set-Location $Root
$script:Cargo = Resolve-Cargo

Invoke-Step "cargo fmt" { Invoke-Cargo @("fmt", "--all", "--", "--check") }
Invoke-Step "cargo test" { Invoke-Cargo @("test", "--workspace", "--locked") }
Invoke-Step "cargo clippy" {
  Invoke-Cargo @("clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings")
}

if ($Release) {
  Invoke-Step "cargo release build" { Invoke-Cargo @("build", "--release", "-p", "ark_cli", "--locked") }
}

Invoke-Step "active tree guard" { & (Join-Path $Root "tests\active_tree_guard.ps1") }
Invoke-Step "fen contract guard" { & (Join-Path $Root "tests\fen_contract_guard.ps1") }
Invoke-Step "performance contract guard" { & (Join-Path $Root "tests\perf_contract_guard.ps1") }
Invoke-Step "dependency guard" { & (Join-Path $Root "tests\dependency_guard.ps1") }

Write-Host "All checks passed."
