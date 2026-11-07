# Maxed

Maxed is a Windows desktop companion for Realm of the Mad God. It shows live dungeon and realm statistics and keeps a local history of your runs.

**Status: pre-release 0.1.5, Windows x64 only.** Live and release acceptance is incomplete, and memory use remains above the release limits. See [known limitations](docs/known-limitations.md).

- Live damage, per-player DPS, kills, realm progress and dungeon totals.
- Loot Feed, item history, observed per-run statistics and Collection with a chase list; source and capture uncertainty stay visible.
- Run history, fight and player breakdowns, fame summaries and personal bests.
- Configurable white-bag, personal-best and key-pop alerts, sounds and Windows notifications.
- Synthetic demo mode and optional local traffic recording.

Maxed only observes traffic passively through Npcap. It never injects or modifies game packets. Game assets are read from your local installation; they are not distributed with Maxed.

Maxed is an independent fan project, not affiliated with or endorsed by DECA Games. Realm of the Mad God is a trademark of its owner.

## Install and privacy

Follow the [installation guide](docs/release/INSTALL.md) for requirements, release downloads and setup. Npcap is installed separately. Read [PRIVACY.md](PRIVACY.md) before enabling recordings or sharing diagnostic information.

## Build from source

Install the pinned Rust toolchain, Node.js 22.16.0, MSVC C++ Build Tools with the Windows SDK, and WebView2. From the repository root:

```powershell
npm --prefix app/ui ci
npm --prefix app/ui run tauri dev
```

Use the npm `tauri` script to start the CLI in `app/src-tauri`, so the `beforeDevCommand` and `beforeBuildCommand` paths in `tauri.conf.json` resolve against `app/` and reach `app/ui`. In PowerShell, `npm.cmd` is an alternative if script execution policy blocks `npm`.

Build a desktop release executable with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-release.ps1
```

Add `-Offline` when Cargo dependencies are cached. The script builds the UI, enables `custom-protocol`, checks bundled resources and prints the executable path. Plain `cargo build --release` is insufficient: it leaves the frontend configured for the development server. Packaging and signing have separate [release requirements](docs/release/CHECKLIST.md).

## Contribute

Start with [CONTRIBUTING.md](CONTRIBUTING.md) and the [documentation index](docs/README.md). The project uses a Rust workspace, Tauri 2 and Svelte 5. macOS can run core checks and cross-build Windows test artifacts; it is not a supported application platform.

Use [GitHub issues](https://github.com/DaAwesomePwner/maxed/issues) for bugs and feature requests. Never post raw captures, player names or account data. Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md), and follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

Maxed is [MIT licensed](LICENSE). See [NOTICE.md](NOTICE.md) for upstream attribution and [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) for dependency terms. Game content and separately installed dependencies retain their own rights and terms.
