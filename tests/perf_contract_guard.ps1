$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
$Doc = Join-Path $Root "docs\ark-v4-forge-performance.md"
$Begin = "<!-- ark-v4-forge-performance-contract:begin -->"
$End = "<!-- ark-v4-forge-performance-contract:end -->"

function Assert-True($Condition, [string]$Message) {
  if (-not $Condition) {
    throw $Message
  }
}

function Read-Contract {
  $Text = Get-Content -LiteralPath $Doc -Raw
  Assert-True ($Text.Contains($Begin)) "performance contract begin marker missing"
  Assert-True ($Text.Contains($End)) "performance contract end marker missing"

  $Start = $Text.IndexOf($Begin) + $Begin.Length
  $Length = $Text.IndexOf($End) - $Start
  $Block = $Text.Substring($Start, $Length)
  $Match = [regex]::Match(
    $Block,
    '```json\s*(?<json>.*?)\s*```',
    [System.Text.RegularExpressions.RegexOptions]::Singleline
  )
  Assert-True $Match.Success "performance contract JSON block missing"
  return $Match.Groups["json"].Value | ConvertFrom-Json
}

$Contract = Read-Contract

Assert-True ($Contract.architecture.core_language -eq "rust") "core language must be Rust"
Assert-True ($Contract.architecture.core -eq "custom_bitboard") "core must be custom bitboards"
Assert-True ($Contract.architecture.search -eq "classical_nn_search") "search shape changed"
Assert-True ($Contract.architecture.self_play_bootstrap -eq "games_only_first") "self-play changed"
Assert-True (
  $Contract.architecture.leaf_eval -eq "terminal_default_wdl_checkpointed"
) "leaf eval changed"
Assert-True (
  $Contract.architecture.primary_target -eq "server_first_32_vcpu_cpu_actors"
) "primary target changed"
Assert-True (
  $Contract.architecture.later_target -eq "cpu_only_deeper_search_and_training"
) "later CPU-only lane changed"

@(
  "no_global_installs",
  "no_host_toolchain_mutations",
  "no_archive_imports_or_copies",
  "no_model_or_data_artifacts_committed",
  "no_python_hot_path",
  "rust_release_build_required"
) | ForEach-Object {
  Assert-True ($Contract.runtime_guardrails.$_ -eq $true) "guardrail not true: $_"
}

$ExpectedIds = @(
  "forge-core-perft-startpos-d4-gate",
  "forge-core-perft-kiwipete-d4-gate",
  "forge-search-terminal-leaf-gate",
  "forge-search-startpos-d6-single-baseline",
  "forge-selfplay-legal-d1-32cpu-baseline",
  "forge-selfplay-search-d3-32cpu-baseline",
  "forge-selfplay-endurance-30m-periodic"
)
$Ids = @($Contract.benchmarks | ForEach-Object { $_.id })
Assert-True (($Ids | Sort-Object -Unique).Count -eq $Ids.Count) "benchmark ids must be unique"
$ExpectedIds | ForEach-Object {
  Assert-True ($Ids -contains $_) "missing benchmark id: $_"
}

$Lanes = @($Contract.benchmarks | ForEach-Object { $_.lane } | Sort-Object -Unique)
Assert-True (($Lanes -join ",") -eq "baseline,gate,periodic_eval") "benchmark lanes changed"

$ById = @{}
$Contract.benchmarks | ForEach-Object {
  $ById[$_.id] = $_
  $Command = $_.command.ToLowerInvariant()
  Assert-True ($Command.StartsWith("cargo run --release -p ark_cli --")) "not ark_cli release: $($_.id)"
  Assert-True ($Command.Contains("--json")) "benchmark lacks JSON output: $($_.id)"
  Assert-True (-not ($Command -match "\bpython\b")) "benchmark command invokes Python: $($_.id)"
  Assert-True (-not ($Command -match "\barchive\b")) "benchmark command touches archive: $($_.id)"
  Assert-True ($null -ne $_.targets) "benchmark lacks targets: $($_.id)"
  Assert-True ($null -ne $_.target_profile) "benchmark lacks target profile: $($_.id)"
  if ($Command.StartsWith("cargo run --release -p ark_cli -- search")) {
    Assert-True (-not ($Command -match "--threads\s+([2-9]|[1-9][0-9]+)")) "active search benchmark uses unsupported threads: $($_.id)"
  }
}

$SingleSearch = $ById["forge-search-startpos-d6-single-baseline"].targets
Assert-True ($SingleSearch.depth_completed_min -eq 6) "single search depth target changed"
Assert-True ($SingleSearch.nodes_per_second_min -ge 12000000) "single search NPS target too low"
Assert-True (
  $SingleSearch.non_terminal_static_eval_calls_max -eq 0
) "single search uses non-terminal static eval"
Assert-True ($SingleSearch.python_hot_path_ms_max -eq 0) "single search allows Python hot path"

$UnsupportedIds = @($Contract.unsupported_benchmarks | ForEach-Object { $_.id })
Assert-True ($UnsupportedIds -contains "forge-search-startpos-d6-32cpu-baseline") "missing unsupported 32-thread search benchmark"
$UnsupportedById = @{}
$Contract.unsupported_benchmarks | ForEach-Object { $UnsupportedById[$_.id] = $_ }
$UnsupportedSearch = $UnsupportedById["forge-search-startpos-d6-32cpu-baseline"]
Assert-True ($UnsupportedSearch.command.ToLowerInvariant().Contains("--threads 32")) "unsupported search must preserve 32-thread command"
Assert-True ($UnsupportedSearch.reason -eq "search_threads_above_one_not_supported") "unsupported search reason changed"
Assert-True ($UnsupportedSearch.supported_until -eq "search --threads 1") "supported search fallback changed"
Assert-True ($UnsupportedSearch.targets.nodes_per_second_min -ge 160000000) "32 CPU search NPS target too low"
Assert-True ($UnsupportedSearch.targets.speedup_vs_single_thread_min -ge 10.0) "32 CPU speedup target too low"
Assert-True ($UnsupportedSearch.targets.cpu_utilization_percent_min -ge 85) "CPU use target too low"

$SelfPlay = $ById["forge-selfplay-search-d3-32cpu-baseline"].targets
Assert-True ($SelfPlay.games_completed_min -ge 2048) "self-play completed games target too low"
Assert-True ($SelfPlay.games_per_second_min -ge 12) "self-play games/s target too low"
Assert-True ($SelfPlay.plies_per_second_min -ge 1500) "self-play plies/s target too low"
Assert-True (
  $SelfPlay.search_nodes_per_second_min -ge 60000000
) "self-play search NPS target too low"
Assert-True ($SelfPlay.illegal_moves_max -eq 0) "self-play permits illegal moves"
Assert-True ($SelfPlay.committed_artifacts_max -eq 0) "self-play permits committed artifacts"

$Endurance = $ById["forge-selfplay-endurance-30m-periodic"].targets
Assert-True ($Endurance.rss_growth_percent_max -le 3) "endurance memory growth target too loose"
Assert-True ($Endurance.actor_crashes_max -eq 0) "endurance permits actor crashes"

$SearchMetrics = @($Contract.required_metrics.search)
@(
  "python_hot_path_ms",
  "root_movegen_calls",
  "node_movegen_calls",
  "total_movegen_calls",
  "terminal_leaf_evals",
  "neutral_frontier_evals",
  "wdl_leaf_evals",
  "external_leaf_eval_calls",
  "non_terminal_static_eval_calls",
  "tactical_extension_nodes",
  "tactical_extension_moves",
  "model_ordered_root_moves",
  "model_ordered_moves",
  "nodes_per_second",
  "speedup_vs_single_thread"
) | ForEach-Object {
  Assert-True ($SearchMetrics -contains $_) "missing search metric: $_"
}

$SelfPlayMetrics = @($Contract.required_metrics.self_play)
@(
  "games_per_second",
  "plies_per_second",
  "search_nodes_per_second",
  "root_movegen_calls",
  "node_movegen_calls",
  "total_movegen_calls",
  "neutral_frontier_evals",
  "wdl_leaf_evals",
  "committed_artifacts",
  "python_hot_path_ms"
) | ForEach-Object {
  Assert-True ($SelfPlayMetrics -contains $_) "missing self-play metric: $_"
}

Assert-True ($Contract.statistical_policy.minimum_repetitions -ge 5) "repetition count too low"
Assert-True ($Contract.statistical_policy.confidence -eq 0.95) "confidence target changed"

$RequiredFields = @($Contract.required_stdout_fields)
@(
  "target_results",
  "failures"
) | ForEach-Object {
  Assert-True ($RequiredFields -contains $_) "missing required stdout field: $_"
}

$AllowedStatuses = @($Contract.status_policy.allowed_statuses | Sort-Object)
Assert-True (($AllowedStatuses -join ",") -eq "fail,pass") "allowed statuses changed"
Assert-True (
  $Contract.status_policy.pass_requires_non_empty_targets -eq $true
) "pass can be reported without targets"
Assert-True (
  $Contract.status_policy.pass_requires_all_target_results_passed -eq $true
) "pass can be reported with failed target results"
Assert-True (
  $Contract.status_policy.unmeasured_target_metric_is_failure -eq $true
) "unmeasured target metrics must fail"
Assert-True (
  $Contract.status_policy.failed_targets_must_be_named -eq $true
) "failed targets must be named"
Assert-True (
  $Contract.status_policy.stage0_confidence_complete -eq $false
) "Stage 0 must not claim completed confidence intervals"
Assert-True (
  $Contract.status_policy.search_threads_above_one -eq "reject"
) "search threads > 1 policy changed"

$Text = Get-Content -LiteralPath $Doc -Raw
$Examples = @()
[regex]::Matches(
  $Text,
  '```json\s*(?<json>.*?)\s*```',
  [System.Text.RegularExpressions.RegexOptions]::Singleline
) | ForEach-Object {
  $Parsed = $_.Groups["json"].Value | ConvertFrom-Json
  if ($Parsed.schema_version -eq "ark-v4-forge-bench-v1") {
    $Examples += $Parsed
  }
}

Assert-True ($Examples.Count -eq 2) "expected two CLI output examples"
$Examples | ForEach-Object {
  $Example = $_
  $Fields = @($Example.PSObject.Properties.Name)
  $RequiredFields | ForEach-Object {
    Assert-True ($Fields -contains $_) "CLI example missing field: $_"
  }
  Assert-True ($Example.status -eq "pass") "CLI example must be passing"
  Assert-True (
    $Example.command.StartsWith("cargo run --release -p ark_cli --")
  ) "CLI example must use ark_cli release"
  Assert-True (-not ($Example.command.ToLowerInvariant() -match "\bpython\b")) "example invokes Python"
  Assert-True ($Example.metrics.python_hot_path_ms -eq 0) "example allows Python hot path"
  Assert-True ($Example.artifacts.committed_artifacts -eq 0) "example commits artifacts"
  Assert-True (@($Example.targets.PSObject.Properties).Count -gt 0) "passing example has no targets"
  Assert-True (@($Example.target_results).Count -gt 0) "passing example has no target results"
  @($Example.target_results) | ForEach-Object {
    Assert-True ($_.passed -eq $true) "passing example has failed target result"
    Assert-True (-not [string]::IsNullOrWhiteSpace($_.target_name)) "target result lacks name"
    Assert-True (-not [string]::IsNullOrWhiteSpace($_.metric)) "target result lacks metric"
    Assert-True ($null -ne $_.observed) "target result lacks observed value"
    Assert-True ($null -ne $_.target_value) "target result lacks target value"
  }
  Assert-True (@($Example.failures).Count -eq 0) "passing example has failures"
  Assert-True ($Example.confidence.level -eq 0.95) "example confidence level changed"
  Assert-True ($Example.confidence.repetitions -ge 5) "example repetitions too low"
  Assert-True ($Example.confidence.minimum_repetitions -ge 5) "example minimum repetitions too low"
  Assert-True ($Example.confidence.complete -eq $true) "passing example confidence incomplete"
}

$SearchExample = $Examples | Where-Object { $_.benchmark_id -eq "forge-search-startpos-d6-single-baseline" } | Select-Object -First 1
Assert-True ($null -ne $SearchExample) "single-thread search example missing"
Assert-True ($SearchExample.metrics.threads -eq 1) "search example must use one thread"

$ArchiveResetSlash = "archive/" + "reset-"
$ArchiveResetBackslash = "archive" + "\\reset"
$ArchiveLegacySlash = "archive/" + "legacy"
Assert-True (-not ($Text.ToLowerInvariant() -match $ArchiveResetSlash)) "doc references reset archive"
Assert-True (-not ($Text.ToLowerInvariant() -match $ArchiveResetBackslash)) "doc references reset archive"
Assert-True (-not ($Text.ToLowerInvariant() -match $ArchiveLegacySlash)) "doc references legacy archive"
Assert-True (
  $Text.Contains("search --threads > 1") -and $Text.Contains("is rejected")
) "doc must state unsupported search threads"

Write-Host "perf contract guard passed"
