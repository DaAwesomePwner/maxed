# Contributing to Maxed

Use issues to describe the problem, expected behavior and a minimal reproduction. Include the Maxed version, Windows version and whether the issue occurred in live capture, demo or replay. Never post raw captures, player names, account data, tokens or unsanitized logs. Use [private vulnerability reporting](SECURITY.md) for security issues.

## Windows setup

Install Git, Rust through rustup, Visual Studio 2022 Build Tools with the C++ workload and Windows SDK, Node.js 22.16.0 and WebView2. `rust-toolchain.toml` pins Rust 1.99.0 and its components. PowerShell 7 is useful for checks; the release scripts also support Windows PowerShell 5.1. Live capture additionally needs separately installed Npcap in its default, non-admin-restricted mode; see [installation](docs/release/INSTALL.md).

From the repository root:

```powershell
rustup show
cargo fetch --locked
npm --prefix app/ui ci
npm --prefix app/ui run tauri dev
```

Use `scripts/build-release.ps1` for a release executable. Do not use an ordinary Cargo release build for desktop or performance acceptance. See [testing](docs/development/testing.md) for checks, [UI preview](docs/development/ui-preview.md) for synthetic browser scenarios, and [local UI review](docs/development/windows-ui-review.md) for driving a debug build of the desktop app.

## macOS development

Follow [the Mac-to-Windows workflow](docs/development/mac-windows-workflow.md). Source `scripts/mac/env.sh` before Rust commands. Run `scripts/mac/check.sh quick` while iterating and `full` before review; use `scripts/mac/build-windows.sh` for Windows test artifacts. `maxed-app` is Windows-specific: cross-linking it on macOS does not run its tests. Do not port Windows ownership/capture code simply to make native macOS tests compile.

## Branches and pull requests

Branch from `main` and open pull requests against `main`. Keep a change focused, explain the user-visible result and list checks run, failures and remaining Windows live tests. Include sanitized screenshots when changing UI behavior. Passing automated checks does not establish installation or real-game acceptance.

Follow `.editorconfig`, `rustfmt.toml` and existing code idioms. Keep `Cargo.lock` and `package-lock.json` consistent; use `--locked` and `npm ci`. New dependencies need a concrete reason and a license review.

## Tests, fixtures and generated files

Prefer deterministic synthetic inputs for unit tests. Preserve fixture hashes, expected results, scrub profiles and historical provenance. [Fixture policy](docs/development/fixtures.md) and [scrubbing rules](docs/development/scrubbing.md) explain what can be committed. Never add game sprites, extracted client catalogs or raw traffic.

Rust DTOs in `maxed-types` own the bridge contract. Regenerate TypeScript bindings with:

```powershell
cargo test --locked -p maxed-types --test export_bindings
```

Review the generated changes with the source changes; do not hand-edit generated bindings. Regenerate dependency notices with the maintained [license tooling](docs/release/LICENSE-INVENTORY.md#regeneration-and-checks); legal payloads and inventory hashes must agree.

## Runtime boundaries

Keep capture passive. Do not inject or modify packets or call account services. The engine stays deterministic and free of I/O; native workers own capture, storage, recording and delivery. Unknown inputs must remain unknown, and inferred results must retain their evidence. Follow [privacy](PRIVACY.md) when adding diagnostics or outbound behavior.

Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).
