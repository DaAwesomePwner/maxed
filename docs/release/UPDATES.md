# Release staging and updates

Releases build in the private `DaAwesomePwner/maxed-rotmg` repository and are distributed from the fixed public repository `DaAwesomePwner/maxed`, which the updater pins at build time. Renaming or transferring the public repository breaks updates for installed clients, because redirects to other GitHub paths are rejected. Public update acceptance requires unauthenticated access to reviewed release artifacts. No repository token ships in Maxed. A pre-release version is not a claim that the public release or installed-update checks are complete.

## Native updater rationale

The native adapter replaces `tauri-plugin-updater` because the inspected 2.13.1
transport accumulates an unbounded download, exposes no public bounded-download
verification boundary (`verify_signature` is private), and lets `install(bytes)`
skip signature verification. Its public check path also fetches unbounded
metadata. The replacement bounds metadata and installer downloads, verifies
with `minisign-verify`, and launches verified NSIS bytes with `/P /UPDATE`.
It supplies no `/R`, `/ARGS`, shell command or elevation request.

## Channels and native policy

Stable is the default; beta requires opt-in. Checking, downloading and installing are separate manual actions. Startup does not automatically check. Stable rejects prereleases; beta can offer newer stable or beta versions. Returning to stable from a newer beta waits for a newer stable version rather than downgrading.

| Channel | Fixed feed |
| --- | --- |
| Stable | `https://github.com/DaAwesomePwner/maxed/releases/download/update-stable/latest.json` |
| Beta | `https://github.com/DaAwesomePwner/maxed/releases/download/update-beta/latest.json` |

Pointer releases must be published and publicly downloadable, never drafts. Stable is non-prerelease and beta is prerelease; neither pointer should be marked GitHub's latest release. Clients use explicit URLs, not `/releases/latest`. Immutable installers use the exact `v<version>/Maxed_<version>_x64-setup.exe` path.

Every request begins at GitHub over HTTPS. Redirects are limited to `release-assets.githubusercontent.com` with a validated decimal repository ID and canonical UUID path; subsequent redirects retain that path, up to five redirects. Credentials, explicit ports, fragments and arbitrary hosts are rejected. Proxy use, automatic retries and referrer headers are disabled. See [privacy](../../PRIVACY.md) and [network auditing](network-audit.md).

## Build and stage

The release workflow builds version tags and stages artifacts; publication and channel promotion remain separate operations. The workflow uses read-only repository contents permission and a protected `release-signing` package step. Verify the actual workflow, immutable action pins, tag restrictions and environment approvals for each release. Node 22.16.0 and Rust 1.99.0 are pinned; use `npm ci` and locked Cargo dependencies.

Run `scripts/build-release.ps1 -Package -Offline` with cached dependencies, the locked Tauri CLI, 7-Zip on PATH and the [signing environment](signing.md). The frontend uses `custom-protocol`. The build checks generated notices, verifies the signature and authenticated version, then extracts the installer to validate the explicit payload allowlist. No post-build signing fallback is provided.

Expected resources are `maxed.exe`, `LICENSE`, `NOTICE.md`, `THIRD-PARTY-NOTICES.md`, `demo/catalog.json`, `demo/story.pcap` and the inventoried NSIS support files. 7-Zip 26.02 and later also list and extract the NSIS-generated `uninstall.exe`; it varies per build, so it must be a Windows executable and its hash is recorded in `bundle-manifest.json` rather than the static inventory. The build permits only Tauri's known three-byte `UNK` → `NSS` bundle-type marker patch between the built and packed executable; lengths and all other bytes must match. `bundle-manifest.json` records extracted paths, byte counts, hashes, original executable hash and marker offset. This resource manifest is distinct from the staged download manifest.

After packaging, generate metadata with explicit reviewed arguments. Set `$version` to the release being prepared:

```powershell
$version = '0.1.1'
$notes = [IO.File]::ReadAllText('reviewed-notes.txt')
./scripts/release/New-ReleaseMetadata.ps1 `
  -Version $version -Channel 'stable' `
  -Repository 'DaAwesomePwner/maxed' `
  -Installer "target/release/bundle/nsis/Maxed_${version}_x64-setup.exe" `
  -Signature "target/release/bundle/nsis/Maxed_${version}_x64-setup.exe.sig" `
  -OutputDir 'release-stage' `
  -LicenseInventory 'docs/release/distribution-inventory.json' `
  -Notes $notes
```

Resolve the actual target directory with Cargo metadata when it differs. Version must match workspace, app and UI configuration. Output must not exist; its parent must exist and ancestors must not be reparse points. Input names must match the immutable installer/signature convention. Installer size is positive and strictly below 20,000,000 bytes. Notes are plain text, at most 8,192 UTF-8 bytes; never construct shell commands from them.

[latest.schema.json](latest.schema.json) documents the feed shape. The platform key is `windows-x86_64-nsis`; `signature` contains the base64 signature contents, not a URL. The whole feed is at most 65,536 bytes. Tooling also validates version/repository equality and cryptographic authenticity.

Staging copies the exact installer/signature, `LICENSE` and `NOTICE.md`, writes `latest.json`, `license-inventory.json` and `release-manifest.json`, and records hashes/byte counts. The static [distribution inventory](distribution-inventory.json) must cover actual shipped stable resources. Each static inventory input row has exactly `file`, `license` and `sha256`: a validated payload-relative path, its license identifier, and the lowercase SHA-256 of the exact shipped bytes. The only installer-private path form is `$PLUGINSDIR/` followed by one filename. Metadata generation cannot infer installer completeness. Run `tests/packaging/audit-bundle.ps1` against the real installer and staged `release-manifest.json`; the workflow invokes this audit before upload. Never bypass a failed audit. A partial failed stage is unusable. The separate staged download allowlist currently omits `THIRD-PARTY-NOTICES.md` even though it is included inside the installer. Adding it to standalone release assets requires coordinated metadata, manifest and test changes; do not claim the current script emits it.

## Public source snapshot

Release downloads must be publicly readable, so they live in a public repository whose source is a reviewed snapshot of each release commit, never this repository's history. Export the tagged commit:

```powershell
./scripts/release/Export-PublicSnapshot.ps1 -Commit "v$version" -OutputDir 'public-snapshot' -DenyPattern '<your account name>'
```

The export contains the commit's tracked files minus [public-exclude.txt](../../scripts/release/public-exclude.txt); untracked, ignored and uncommitted files never reach it. It then rejects recording, database (including SQLite sidecars), log, key and environment files, private keys, Tauri signing keys (plain or base64), GitHub tokens, local user paths and every `-DenyPattern` regular expression. Pass personal patterns at run time rather than committing them. Captures, found by `.pcap` name or pcap/pcapng magic, are denied by default: [public-allow.txt](../../scripts/release/public-allow.txt) lists each synthetic or publication-approved capture file and each reviewed invented canary, with the approved git blob for every entry, so any change to an allowed file needs renewed review. Stale exclude or allow entries fail the export, and a failed export leaves no output. The scan backs up the snapshot review in the [checklist](CHECKLIST.md); it does not replace it.

To publish, add `-PublicClone <path>` with a clean clone of the public repository. After a passing export, the script replaces the clone's tracked files with the snapshot and stages them with the source file modes. A plain copy followed by `git add` would drop executable bits wherever `core.filemode` is false, as on Windows. The script then requires every staged blob and mode to match the reviewed commit; it never commits or pushes. Review `git diff --cached`, commit once as `Maxed <version>`, tag that commit `v<version>` and push before creating the version release below.

## Serialized promotion

1. Verify downloaded staging hashes, signature, authenticated version, resource inventory and all [release gates](CHECKLIST.md). Preserve the exact verified bytes; do not rebuild afterward.
2. Create a draft immutable `v<version>` release and upload the exact installer, signature, notices, inventory and manifest. Validate remote bytes and URLs. Never silently replace a published version asset.
3. Test valid and changed/truncated/missing/wrong-key installed-update cases with disposable keys/builds on an isolated test channel. Production repository/channel/key restrictions remain fixed.
4. Publish the version release with the correct stable/prerelease status. Confirm unauthenticated access and back up the old selected channel manifest and its hash/version.
5. Replace the selected pointer's `latest.json` last, only after every immutable asset exists. Serialize promotions. Beta never changes stable; stable can advance beta only when strictly newer by SemVer precedence. Re-download and verify the feed and exact referenced bytes.
6. On failure, retain or restore the backed-up pointer and verify it. GitHub delete/upload replacement is not transactional, so a briefly unavailable feed must fail safely. Withdraw a bad version by stopping advertisement; installed clients never automatically downgrade.

Record sanitized acceptance and recovery evidence. Signing alone does not prove Npcap, installation, network, memory, startup or gameplay behavior.
