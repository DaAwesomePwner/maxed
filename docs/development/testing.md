# Testing

Run commands from the repository root with the pinned Rust toolchain and installed UI dependencies (`npm --prefix app/ui ci`). Use `cargo fetch --locked` once before offline runs.

## Windows deterministic checks

```powershell
cargo test --locked --offline -p maxed-types --test export_bindings
cargo test --locked --offline --workspace
cargo fmt --all -- --check
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo clippy --locked --offline -p maxed-app --all-targets --features perf-live -- -D warnings
npm --prefix app/ui test
npm --prefix app/ui run test:ui
npm --prefix app/ui run check
npm --prefix app/ui run check:tests
npm --prefix app/ui run build
git diff --check
```

Clippy type-checks every target it lints, so no separate `cargo check` pass is needed. Development and test builds link with `rust-lld` and keep only line-table debug info for workspace crates (`.cargo/config.toml`, `[profile.dev]`); `scripts/build-release.ps1` links releases with the MSVC linker.

Review generated bindings for unexpected drift. Record discovered tests and explicit skips; zero tests or a blocked test executable is not a pass. Run relevant focused tests while iterating, then required broader checks before review. Do not weaken fixtures or production validation to accommodate a missing test prerequisite.

### Integration test targets

Ordinary suites share an `integration` binary. With `autotests = false`, register new
`tests/*.rs` suites in `tests/integration/main.rs` or as explicit `[[test]]` paths.
The workspace guard checks coverage and file existence. Filter one suite with:

```powershell
cargo test --locked --offline -p maxed-engine --test integration damage::
```

Separate targets preserve existing commands or process isolation:

- `maxed-types`: `export_bindings` (TypeScript generation).
- `maxed-cli`: `parity`, `domain_parity`, `demo`, `realm_focus`, `realm_kill_evidence` (parity and fixture commands).
- `maxed-store`: `loot_performance`, `chat_performance`, `d_perf`, `stats_performance`, `retention_performance` (ignored performance suites).
- `maxed-assets`: `oracle`, `unity` (installed-client checks), `tile_memory` (process-wide allocator measurements).
- `maxed-stream`: `reassembly` (already one suite; automatic discovery stays enabled).

Phase 1 retention evidence has its own explicit ignored target:

```powershell
cargo test --locked -p maxed-store --test retention_performance phase1_600mb -- --ignored --nocapture
cargo test --locked -p maxed-types --test integration integration_registration::
cargo test --locked -p maxed-store --test integration
```

Run Cargo commands serially. The scale test creates and removes private temporary
databases of at least 600,000,000 actual bytes for exact installed v12, v14 and v15.
Allow minutes and roughly 1.3 GB peak disk per schema sample for the database,
WAL and verified backup; never substitute an installed database. It prints
environment, sample counts, separate upgrade/startup/backup/conversion timings,
unit distributions, throughput and byte
counters. Working set is explicitly unavailable without a supported measurement.
See [storage maintenance](storage-maintenance.md) for the fixture shape, dated
baseline, recovery checks and Gate A's pending owner-on-Windows work.

Packaging changes also need each `tests/packaging/*.test.ps1` suite and the audit self-tests:

```powershell
pwsh -NoProfile -File tests/packaging/config.test.ps1
pwsh -NoProfile -File tests/packaging/release-build.test.ps1
pwsh -NoProfile -File tests/packaging/release-metadata.test.ps1
pwsh -NoProfile -File tests/packaging/workflow-policy.test.ps1
pwsh -NoProfile -File tests/packaging/audit-bundle.ps1 -SelfTest
pwsh -NoProfile -File tests/packaging/network-audit.ps1 -SelfTest
```

The build/config suites exercise synthetic boundaries, not a signed installed release. Some require Windows signing tooling, ACLs or reparse-point behavior. Keep the locked npm CLI available. For parity-ledger changes, run `cargo test --locked -p maxed-cli --test parity` and the affected domain comparator tests.

## macOS

Use [the cross-build workflow](mac-windows-workflow.md). `scripts/mac/check.sh quick` runs native core tests and Windows-target lint; `full` adds test linking, UI checks and portable PowerShell checks. Windows test binaries are only linked there, not executed. Windows share-mode asset tests and native application tests require Windows.

## Evidence boundaries

- Synthetic packet tests prove their authored cases; typed DTO tests do not prove packet decoding.
- Scrubbed replay proves the retained observation, not omitted notices, complete live capture or publication approval.
- UI content tests do not prove browser layout; use [UI preview acceptance](ui-preview.md).
- Ignored installed-client/oracle and timing tests need their documented local inputs and environment. Never commit those assets or raw captures.
- A built executable and passing tests do not establish the [release checklist](../release/CHECKLIST.md), [network audit](../release/network-audit.md) or [performance gates](../release/performance.md).

## Release workflow format

Workflow files use JSON-syntax YAML because
`tests/packaging/workflow-policy.test.ps1` parses them with `ConvertFrom-Json`.
Keep this representation when editing workflows. Every action pin must be a
full 40-hex commit SHA; tags and abbreviated hashes do not satisfy the policy.

## Continuous integration

`.github/workflows/tests.yml` runs on GitHub-hosted `windows-2022` for
pull requests to `main`, including forks, daily at 09:17 UTC (`17 9 * * *`),
pushes to `main`, manual dispatch and release workflow calls. Every PR runs tests,
including docs-only changes, so required checks report. Only main push events
skip the listed documentation paths; the executable parity ledger, legal files
and release JSON inputs still trigger checks.
Contents permission is read-only and tests use no secrets.
The hosted Rust cache restores on eligible runs, prunes workspace artifacts and
saves only after a passing non-PR run on `refs/heads/main`. No persistent machine
paths are used.
Five parallel jobs run: `rust-lint` (fmt and both clippy checks), `rust-test`
(workspace tests excluding maxed-store, then bindings freshness), `store-test`
(maxed-store tests), `ui` (all five UI scripts), and `packaging` (offline memory
tooling and packaging/collector checks). Each Rust job has its own cache key.
The `tests` aggregator on `ubuntu-latest` is the single required check for this
workflow (alongside the separate `gui-smoke` check). Keep its job ID `tests` and
leave its name absent: GitHub uses the job name, falling back to its ID, as the
check name. It needs every test job and runs unless cancelled, failing unless
all results are `success`. It checks results without checkout or tools.
An aggregator failure also fails a reusable workflow call, so release packaging
remains blocked by `needs.tests.result == 'success'` when tests run.
The UI job also installs the pinned minimal Rust toolchain and fetches locked
Cargo dependencies before its offline CLI smoke-replay test, using a separate
`rust-ui-` cache with the same restore, prune and save conditions.
Generated bindings and fixtures must remain current. Release tags reuse a
completed successful main push, schedule or dispatch run for the exact SHA,
with its URL in the job summary; otherwise all tests run before packaging.
Lookup errors fall back to testing; failed tests or lookup jobs block signing. The signing job builds from
scratch on `windows-2022` in the `release-signing` environment.
Quick successive merges can cancel an earlier queued main run; tagging that commit simply runs tests in the release as a safe fallback.
The reusable `release` input remains for caller compatibility.

`.github/workflows/gui-smoke.yml` independently uses GitHub-hosted `windows-2022`
daily at 09:47 UTC (`47 9 * * *`), for pull requests to `main`, including forks,
and manual dispatch. Pushes to `main` do not trigger it.
This job is not reusable and does not gate release signing. It has a 45-minute
timeout, read-only contents permission, no secrets or environment scopes, and
no persistent caches. Both workflows use `pull_request`, never privileged PR events.

Fork PR runs are gated by the repository's Actions setting "Require approval for
all external contributors" (enabled on 2026-11-05); the workflows do not skip them.

The daily schedules are staggered by 30 minutes. Concurrency groups separate
events and branches; only superseded PR runs are cancelled.

The GUI job restores locked tools, runs
`node --test tests/gui-smoke/*.test.mjs` and
`pwsh -NoProfile -File tests/gui-smoke/Invoke-GuiSmoke.test.ps1`, then builds and
launches the isolated debug app. The launcher disables Npcap and game discovery,
checks the rendered Setup screen over WebView2 CDP for ten seconds, and records
a screenshot and JSON reports. Its `finally` kills the app's Windows Job Object;
an `always()` cleanup step can terminate only the recorded launcher PID after
checking its start time and executable. An `always()` artifact upload is limited
to that run's smoke output directory, excludes hidden files, and retains results
for three days. Missing artifacts warn because an earlier step may fail before
the smoke starts. See [the harness README](../../tests/gui-smoke/README.md) for
commands, source evidence, isolation trade-offs and Windows validation limits.

`tests/packaging/workflow-policy.test.ps1` enforces these rules; update it in the
same pull request as any workflow change.
