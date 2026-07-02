param(
  [string]$Repository = $env:GITHUB_REPOSITORY,
  [int]$PullRequestNumber,
  [string]$HeadSha,
  [string]$Token = $env:GITHUB_TOKEN,
  [string[]]$Reviewers = @("chatgpt-codex-connector", "chatgpt-codex-connector[bot]"),
  [string]$ReviewsJsonPath
)

$ErrorActionPreference = "Stop"

function Read-ReviewsFromJson {
  param([string]$Path)

  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    throw "reviews JSON file does not exist: $Path"
  }

  $Parsed = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
  if ($Parsed.PSObject.Properties.Name -contains "reviews") {
    return @($Parsed.reviews)
  }

  return @($Parsed)
}

function Read-GitHubPages {
  param(
    [string]$UriTemplate,
    [string]$Token
  )

  if (-not $Token) {
    throw "GITHUB_TOKEN is required when ReviewsJsonPath is not provided"
  }

  $Headers = @{
    "Accept" = "application/vnd.github+json"
    "Authorization" = "Bearer $Token"
    "X-GitHub-Api-Version" = "2022-11-28"
  }

  $AllItems = @()
  $Page = 1
  while ($true) {
    $Uri = $UriTemplate -f $Page
    $Batch = @(Invoke-RestMethod -Method Get -Uri $Uri -Headers $Headers)
    if ($Batch.Count -eq 0) {
      break
    }

    $AllItems += $Batch
    if ($Batch.Count -lt 100) {
      break
    }

    $Page += 1
  }

  return $AllItems
}

if (-not $Repository) {
  throw "Repository is required"
}

if ($PullRequestNumber -le 0) {
  throw "PullRequestNumber is required"
}

if (-not $HeadSha) {
  throw "HeadSha is required"
}

$Reviews = if ($ReviewsJsonPath) {
  Read-ReviewsFromJson -Path $ReviewsJsonPath
} else {
  Read-GitHubPages `
    -UriTemplate "https://api.github.com/repos/$Repository/pulls/$PullRequestNumber/reviews?per_page=100&page={0}" `
    -Token $Token
}

$MatchingReviews = @(
  $Reviews | Where-Object {
    $Reviewers -contains $_.user.login -and
    $_.commit_id -eq $HeadSha -and
    $_.state -ne "DISMISSED"
  }
)

if ($MatchingReviews.Count -eq 0) {
  $ReviewerList = $Reviewers -join ", "
  Write-Error "Codex review required: no PR review by $ReviewerList for $Repository PR #$PullRequestNumber at head $HeadSha. Trigger a fresh review after the latest push, for example with '@codex review'."
  exit 1
}

$LatestReview = $MatchingReviews |
  Sort-Object -Property submitted_at |
  Select-Object -Last 1
Write-Host "Codex review found: $($LatestReview.user.login) reviewed $HeadSha with state $($LatestReview.state)."
