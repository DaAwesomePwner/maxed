# Testing

Run commands from the repository root with the pinned Rust toolchain and installed UI dependencies (`npm --prefix app/ui ci`). Use `cargo fetch --locked` once before offline runs.

## Windows deterministic checks

```powershell
cargo test --locked --offline -p maxed-types --test export_bindings
cargo check --locked --offline --workspace --all-targets
cargo test --locked --offline --workspace
cargo fmt --all -- --check
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo check --locked --offline -p maxed-app --all-targets --features perf-live
cargo clippy --locked --offline -p maxed-app --all-targets --features perf-live -- -D warnings
npm --prefix app/ui test
npm --prefix app/ui run test:ui
npm --prefix app/ui run check
npm --prefix app/ui run check:tests
npm --prefix app/ui run build
git diff --check
```

Review generated bindings for unexpected drift. Record discovered tests and explicit skips; zero tests or a blocked test executable is not a pass. Run relevant focused tests while iterating, then required broader checks before review. Do not weaken fixtures or production validation to accommodate a missing test prerequisite.

Packaging changes also need each `tests/packaging/*.test.ps1` suite and the audit self-tests:

```powershell
pwsh -NoProfile -File tests/packaging/config.test.ps1
pwsh -NoProfile -File tests/packaging/public-snapshot.test.ps1
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

`.github/workflows/tests.yml` runs these checks on Windows for pull requests to
`main` and pushes to `main`. It also checks that generated bindings and
fixtures are up to date. While the repository is private, those automatic runs
are skipped to save Actions minutes, so maintainers start the workflow
manually. Release tags run the same workflow before packaging, and a failing
check blocks signing. `tests/packaging/workflow-policy.test.ps1` enforces these
rules; update it in the same pull request as any workflow change.
