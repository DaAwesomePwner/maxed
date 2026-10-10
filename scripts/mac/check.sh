#!/usr/bin/env bash
# macOS stand-in for .github/workflows/tests.yml. Usage: scripts/mac/check.sh [quick|full]
#   quick: fmt, Windows-target clippy (authoritative lint), native crate tests.
#   full:  quick + Windows test-binary link, UI suite, portable PowerShell checks.
# Windows-only behaviour (Npcap, share-mode leases, maxed-app tests, NSIS, signing)
# is NOT exercised here; it belongs to the Windows live-test machine.
set -uo pipefail
cd "$(dirname "$0")/../.."
. scripts/mac/env.sh
MODE="${1:-quick}"
case "$MODE" in quick|full) ;; *) echo "usage: scripts/mac/check.sh [quick|full]" >&2; exit 2 ;; esac
[ "$(node -p process.version)" = v22.16.0 ] || { echo "Node 22.16.0 required (CI pin); see scripts/mac/env.sh" >&2; exit 1; }
WIN=x86_64-pc-windows-msvc
FAILED=()

step() {
  local name="$1"; shift
  echo "==> $name"
  if "$@"; then echo "    ok: $name"; else echo "    FAIL: $name"; FAILED+=("$name"); fi
}

xwin() { PATH="$MAXED_XWIN_PATH:$PATH" cargo xwin "$@"; }

# Native maxed-app does not compile off Windows (cfg(windows) recording types, icon.png),
# so it is linted/linked via the Windows target. These two maxed-assets tests depend on
# Windows share-mode exclusive leases (cache.rs returns CacheInvalid elsewhere).
NATIVE_SKIPS=(--skip pruning_waits_for_all_readers_and_leaves_unknown_directories
              --skip cache_rebuilds_manifest_from_before_consumable_tier_rules)

# CI restores with a locked fetch before its --offline steps; mirror it so fresh checkouts work.
step "cargo fetch" cargo fetch --locked
step "cargo fmt" cargo fmt --all -- --check
step "clippy (windows target)" xwin clippy --offline --locked --workspace --all-targets --target "$WIN" -- -D warnings
step "cargo test (native, excl. maxed-app)" cargo test --offline --locked --workspace --exclude maxed-app -- "${NATIVE_SKIPS[@]}"

if [ "$MODE" = full ]; then
  step "clippy perf-live (windows target)" xwin clippy --offline --locked -p maxed-app --all-targets --features perf-live --target "$WIN" -- -D warnings
  step "test link (windows target)" xwin test --no-run --offline --locked --workspace --target "$WIN"
  for s in check check:tests test test:ui build; do
    step "ui $s" npm --prefix app/ui run "$s"
  done
  step "ps workflow-policy" pwsh -NoProfile -File tests/packaging/workflow-policy.test.ps1
  step "ps memory self-test" pwsh -NoProfile -File scripts/Measure-MaxedMemory.ps1 -SelfTest
  step "ps scenario self-test" pwsh -NoProfile -File tests/performance/home-scenarios.ps1 -SelfTest
fi

if [ ${#FAILED[@]} -eq 0 ]; then echo "ALL PASSED ($MODE)"; exit 0; fi
printf 'FAILED: %s\n' "${FAILED[@]}"; exit 1
