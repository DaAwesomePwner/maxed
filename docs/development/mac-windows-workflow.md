# macOS development and Windows testing

Maxed ships for Windows only. macOS can run core tests, lint for Windows and cross-build Windows artifacts. Npcap, native application tests, installed builds and live gameplay require Windows.

## Mac toolchain

| Tool | Why |
|---|---|
| `rustup` (Homebrew, keg-only) | Picks Rust 1.99.0 from `rust-toolchain.toml` |
| Rust target `x86_64-pc-windows-msvc` + `cargo-xwin` | Lint, link and build Windows code on the Mac |
| `llvm` (Homebrew, keg-only) | clang-cl and lld-link for cargo-xwin |
| `fnm` + Node 22.16.0 | Same Node pin as CI |
| `powershell` (`pwsh`) | Portable `.ps1` self-tests |

Fresh Mac, from the repo root. `env.sh` must come before any `cargo` or `rustup` call,
because Homebrew's rustup is keg-only and is not on PATH by default:

```sh
brew install rustup llvm fnm powershell
fnm install 22.16.0
. scripts/mac/env.sh                       # rustup, ~/.cargo/bin, Node 22.16.0 on PATH
rustup toolchain install                   # 1.99.0 + components from rust-toolchain.toml
rustup target add x86_64-pc-windows-msvc
cargo install --locked cargo-xwin
cargo fetch --locked                       # check.sh also fetches before its --offline steps
npm ci --prefix app/ui
```

The first cargo-xwin run downloads the MSVC CRT and Windows SDK into `~/Library/Caches/cargo-xwin`.

## Scripts

- `scripts/mac/check.sh [quick|full]`: the macOS check entry point.
- `scripts/mac/build-windows.sh`: builds `target/maxed-win-x64-<sha>[-dirty].zip`. It contains
  `maxed.exe`, `maxed-cli.exe`, PDBs, the demo files, notices and `BUILD-INFO.txt`
  (branch and commit). It uses the same frontend build and `custom-protocol` feature as
  `build-release.ps1`. It is unsigned, has no NSIS installer, and links with lld-link plus
  the xwin CRT. Use it for testing only. Releases still come from `scripts/build-release.ps1`
  on Windows.

## Platform coverage

| CI gate | On Mac | Notes |
|---|---|---|
| `cargo fmt --check` | yes | |
| `cargo clippy -D warnings` | yes, Windows target via cargo-xwin | Native clippy fails because `cfg(windows)` code goes unused or missing |
| Perf feature clippy (`perf-live`) | yes, Windows target via cargo-xwin | |
| `cargo test` library crates | yes, native | Skips two maxed-assets lease tests that need Windows share-mode locking |
| `cargo test -p maxed-app` | compile and link only | Native build fails: `maxed_recording` owner types are `cfg(windows)`, and Tauri wants `icons/icon.png` |
| UI check / check:tests / test / test:ui / build | yes | `test:ui` smoke-replay spawns `cargo`, so Rust must be on PATH |
| `workflow-policy.test.ps1`, memory and scenario self-tests | yes, under pwsh | |
| `config`, `release-build`, `release-metadata` `.test.ps1` | no | Windows signing profile, ACL and reparse-point checks |

## Development loop

1. Branch from `main`. Source `scripts/mac/env.sh`, use `scripts/mac/check.sh quick` while iterating and run `scripts/mac/check.sh full` before review.
2. Open a pull request against `main` with changes, validation and remaining Windows checks.
3. Run `scripts/mac/build-windows.sh` and transfer the test zip privately to Windows. Check `BUILD-INFO.txt` against the revision under test. The unsigned zip is for testing, not public release distribution.
4. On Windows, use [local UI review](windows-ui-review.md) to build and drive the branch, then run `cargo test --locked -p maxed-app` and `cargo test --locked -p maxed-assets` as applicable, plus the packaging suites for release-path changes. Use `scripts/build-release.ps1 -Package` for installer acceptance.
5. Record the exact revision/artifact hash, steps, expected and observed behavior, and sanitized evidence in the pull request. Fix issues and rerun affected checks.

Windows prerequisites are Rust 1.99.0 (MSVC), Visual Studio 2022 C++ Build Tools and Windows SDK, Node 22.16.0, WebView2, and Npcap without its admin-only restriction. Follow [installation](../release/INSTALL.md) and [fixture policy](fixtures.md). Never attach raw traffic or account data.
