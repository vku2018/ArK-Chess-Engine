#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

run_step() {
  name=$1
  shift
  printf '==> %s\n' "$name"
  "$@"
}

run_step "cargo fmt" cargo fmt --all -- --check
run_step "cargo test" cargo test --workspace --locked
run_step "cargo clippy" cargo clippy --workspace --all-targets --locked -- -D warnings

if [ "${1:-}" = "--release" ]; then
  run_step "cargo release build" cargo build --release -p ark_cli --locked
fi

if ! command -v pwsh >/dev/null 2>&1; then
  printf '%s\n' "PowerShell Core (pwsh) is required for repository guard scripts." >&2
  exit 1
fi

run_step "active tree guard" pwsh -NoProfile -File tests/active_tree_guard.ps1
run_step "fen contract guard" pwsh -NoProfile -File tests/fen_contract_guard.ps1
run_step "performance contract guard" pwsh -NoProfile -File tests/perf_contract_guard.ps1
run_step "dependency guard" pwsh -NoProfile -File tests/dependency_guard.ps1

printf '%s\n' "All checks passed."
