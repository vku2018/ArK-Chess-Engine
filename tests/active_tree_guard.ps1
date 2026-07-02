$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Forbidden = @(
  "services",
  "pyproject.toml",
  "requirements-dev.lock",
  "requirements-training.lock",
  "requirements-gpu.lock",
  "ark_neural_chess.egg-info",
  "build",
  ".pytest_cache",
  ".ruff_cache"
)

foreach ($Path in $Forbidden) {
  $Full = Join-Path $Root $Path
  if (Test-Path -LiteralPath $Full) {
    throw "Active tree violation: $Path still exists"
  }
}

$TrackedPaths = & git -C $Root ls-files
if ($LASTEXITCODE -ne 0) {
  throw "failed to list tracked files"
}

$ArchiveResetPattern = "archive/" + "reset-" + "2026-06-30-v3-v32"
$ArchiveLegacyPattern = "archive/" + "legacy-" + "v1-v2"
$ArkNetPattern = "ArkNet" + "V3"
$V32Pattern = "V3" + "\.2"
$ContaminationPattern = "$ArchiveResetPattern|$ArchiveLegacyPattern|$ArkNetPattern|$V32Pattern"

foreach ($Path in $TrackedPaths) {
  $Full = Join-Path $Root $Path
  if ($Full -eq $PSCommandPath -or -not (Test-Path -LiteralPath $Full -PathType Leaf)) {
    continue
  }
  $Text = Get-Content -LiteralPath $Full -Raw -ErrorAction SilentlyContinue
  if ($null -ne $Text -and $Text -match $ContaminationPattern) {
    throw "Archive contamination in $Full"
  }
}

Write-Host "active tree guard passed"
