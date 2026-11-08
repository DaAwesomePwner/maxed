# Contributor rules

Maxed is a Windows companion for Realm of the Mad God. Read [README.md](README.md), [CONTRIBUTING.md](CONTRIBUTING.md) and the [architecture overview](docs/architecture/README.md).

## Repository map

- `crates/`: passive capture, stream reconstruction, protocol, engine, assets, recording, storage, shared types and CLI.
- `app/src-tauri/`: native lifecycle, commands, delivery, updater and packaging configuration.
- `app/ui/`: Svelte UI and tests; generated DTOs come from `maxed-types`.
- `tests/fixtures/scrubbed/`: synthetic and scrubbed fixtures with provenance.
- `scripts/` and `tests/packaging/`: build, release and validation tools.
- `docs/`: architecture, development, parity and release references.

## Changes and checks

Branch from `main` and target pull requests at `main`. Keep changes focused and describe validation and unresolved risks. Follow `.editorconfig`, `rustfmt.toml` and surrounding style.

Use `npm ci` and Cargo `--locked`; add dependencies only when needed. Run the [development checks](docs/development/testing.md). On macOS, source `scripts/mac/env.sh` first; use `scripts/mac/check.sh quick` while iterating and `scripts/mac/check.sh full` before review. Lint for the Windows target. Native `maxed-app` tests, capture and installer behavior require Windows. On Windows, use [local UI review](docs/development/windows-ui-review.md) to build and drive the desktop app.

Regenerate bindings with `cargo test --locked -p maxed-types --test export_bindings`; never edit generated DTOs by hand. Review generated diffs. Maintain dependency notices through their generator and update inventory hashes only with reviewed payload changes.

## Privacy and evidence

Capture stays passive: no injection, packet modification or account-service calls. Keep unknown and partial observations visible. Do not claim live Npcap, game-client, WebView2 or installer behavior is verified by synthetic tests.

Never commit raw captures (`*.pcap`, `*.pcapng`, `*.mxrec`, `recordings/`), credentials, local account data or game sprites. Only reviewed fixtures under `tests/fixtures/scrubbed/` are allowed; follow the [fixture policy](docs/development/fixtures.md). Scrub certification and publication approval are separate requirements. Use synthetic identities in examples and sanitized evidence in issues.
