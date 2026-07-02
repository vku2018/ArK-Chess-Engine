$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$GateScript = Join-Path $Root ".github\scripts\require-codex-review.ps1"
$PowerShell = (Get-Process -Id $PID).Path
$HeadSha = "1111111111111111111111111111111111111111"
$OldSha = "2222222222222222222222222222222222222222"
$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ark-codex-review-gate-" + [System.Guid]::NewGuid().ToString("N"))

function Invoke-GateCase {
  param(
    [string]$Name,
    [string]$ReviewsJson,
    [int]$ExpectedExitCode
  )

  $ReviewsPath = Join-Path $TempRoot "$Name-reviews.json"
  Set-Content -LiteralPath $ReviewsPath -Value $ReviewsJson -Encoding UTF8

  $PreviousErrorActionPreference = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  try {
    $Output = & $PowerShell `
      -NoProfile `
      -ExecutionPolicy Bypass `
      -File $GateScript `
      -Repository "bitlical/Ark" `
      -PullRequestNumber 42 `
      -HeadSha $HeadSha `
      -ReviewsJsonPath $ReviewsPath 2>&1
    $ExitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $PreviousErrorActionPreference
  }

  if ($ExitCode -ne $ExpectedExitCode) {
    throw "$Name expected exit $ExpectedExitCode but got $ExitCode. Output: $Output"
  }
}

New-Item -ItemType Directory -Force -Path $TempRoot | Out-Null

try {
  Invoke-GateCase `
    -Name "passes-on-codex-head-review" `
    -ExpectedExitCode 0 `
    -ReviewsJson @"
[
  {
    "user": { "login": "chatgpt-codex-connector" },
    "commit_id": "$HeadSha",
    "state": "COMMENTED",
    "submitted_at": "2026-07-02T12:00:00Z"
  }
]
"@

  Invoke-GateCase `
    -Name "passes-on-codex-bot-head-review" `
    -ExpectedExitCode 0 `
    -ReviewsJson @"
[
  {
    "user": { "login": "chatgpt-codex-connector[bot]" },
    "commit_id": "$HeadSha",
    "state": "COMMENTED",
    "submitted_at": "2026-07-02T12:00:00Z"
  }
]
"@

  Invoke-GateCase `
    -Name "fails-without-codex-review" `
    -ExpectedExitCode 1 `
    -ReviewsJson @"
[
  {
    "user": { "login": "human-reviewer" },
    "commit_id": "$HeadSha",
    "state": "APPROVED",
    "submitted_at": "2026-07-02T12:00:00Z"
  }
]
"@

  Invoke-GateCase `
    -Name "fails-on-stale-codex-review" `
    -ExpectedExitCode 1 `
    -ReviewsJson @"
[
  {
    "user": { "login": "chatgpt-codex-connector" },
    "commit_id": "$OldSha",
    "state": "COMMENTED",
    "submitted_at": "2026-07-02T12:00:00Z"
  }
]
"@

  Invoke-GateCase `
    -Name "fails-on-dismissed-codex-review" `
    -ExpectedExitCode 1 `
    -ReviewsJson @"
[
  {
    "user": { "login": "chatgpt-codex-connector" },
    "commit_id": "$HeadSha",
    "state": "DISMISSED",
    "submitted_at": "2026-07-02T12:00:00Z"
  }
]
"@
} finally {
  Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "codex review gate passed"
exit 0
