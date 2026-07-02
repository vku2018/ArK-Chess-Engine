$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Files = @(
  (Join-Path $Root "Cargo.toml"),
  (Join-Path $Root "Cargo.lock")
)
$Forbidden = @("pyo3", "tch", "torch-sys", "cuda", "cudarc", "candle-cuda")

foreach ($File in $Files) {
  if (-not (Test-Path -LiteralPath $File)) {
    continue
  }
  $Text = (Get-Content -LiteralPath $File -Raw).ToLowerInvariant()
  foreach ($Needle in $Forbidden) {
    if ($Text.Contains($Needle)) {
      throw "Forbidden dependency marker '$Needle' found in $File"
    }
  }
}

Write-Host "dependency guard passed"
