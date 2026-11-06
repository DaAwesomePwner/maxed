# Release checklist

Record version, source revision, exact artifact hashes, environment, result and sanitized evidence for each check. Unchecked items remain unverified. Builds and unit tests do not replace Windows installation or live capture evidence.

## Source and public readiness

- [ ] Run the [deterministic checks](../development/testing.md), including generated bindings, UI checks and all packaging suites. On macOS, also run `scripts/mac/check.sh full`; Windows-specific tests still require Windows.
- [ ] Match workspace, app, UI and lockfile versions to the intended release tag.
- [ ] Review the public source and history for credentials, local paths, personal data, raw captures and game art. Scan results apply only to the reviewed bytes.
- [ ] Confirm source, CI, release tags and update downloads all use `daplabs/maxed`; document manual reinstall for existing v0.1.1 installations.
- [ ] Review all five scrubbed-real fixture groups under the [fixture policy](../development/fixtures.md). Record explicit publication approval separately from scrub certification.
- [ ] Set the enforcement contact in [CODE_OF_CONDUCT.md](../../CODE_OF_CONDUCT.md) and enable GitHub private vulnerability reporting for [SECURITY.md](../../SECURITY.md).
- [ ] Confirm rights and actual payloads against [LICENSE-INVENTORY.md](LICENSE-INVENTORY.md). Resolve its stale lock provenance and 13 missing package-text review entries; correct the obsolete notification statement in `NOTICE.md` through a reviewed legal-payload/hash update. Do not silently edit hashed inputs.
- [ ] Confirm product/release notes name the tagged pre-release version (matching README and known limitations), Windows-only, with Loot Feed, Item history, Stats and Collection included. Link current [limitations](../known-limitations.md).

## Package and update integrity

- [ ] Verify updater public key and artifact configuration; restrict release tags and the protected signing environment as described in [signing](signing.md). `release-signing` accepts only `v*` tag deployments and has no required reviewers; the "Protect version tags" ruleset blocks updating or deleting `v*` tags, so a pushed tag cannot be redone. Verify both before tagging.
- [ ] Build with `scripts/build-release.ps1 -Package`; verify exact installer signature, authenticated version and size without exposing signing secrets.
- [ ] Inspect `bundle-manifest.json`, the resource allowlist, all three legal files, synthetic demo resources and NSIS support files. No raw captures, game assets, databases or private exports may ship.
- [ ] Generate staging metadata and run `tests/packaging/audit-bundle.ps1` against the actual installer and staged download manifest. Preserve exact verified bytes.
- [ ] Validate manual check/download/install, stable and beta behavior, newer-beta-to-stable waiting, valid updates, corrupt/truncated/missing/wrong-key payloads, rejected redirects, offline or interrupted updates, a channel change racing an in-flight check/download, and safe failures on an isolated test channel.
- [ ] Confirm unauthenticated download availability; developer credentials are not public availability evidence.

## Needs Windows live test

- [ ] Fresh Windows 10/11 x64 user: per-user install works without elevating Maxed; record actual unsigned-publisher, SmartScreen and Smart App Control behavior without disabling protections.
- [ ] Launch a second instance while Maxed is running: it must not create another capture/store owner or corrupt history.
- [ ] Npcap absent, incompatible, restricted and usable: guidance is accurate; default Npcap allows unelevated capture; retry succeeds after dependency repair.
- [ ] WebView2 present/absent, offline bootstrap, cancellation and bootstrap failure: installer behavior matches [INSTALL.md](INSTALL.md).
- [ ] Repair/update/uninstall: program files and registrations behave correctly; settings, summaries, recordings and cache retention are observed, not assumed. Shared Npcap/WebView2 remain intact.
- [ ] Realm → dungeon → realm → Nexus, reconnect and capture stop: focus, kill evidence, visit continuity, run outcomes, late loot and partial states stay accurate. Check under-three-second DPS, current per-player windows and run-wide totals separately.
- [ ] Verify new bags appear in Loot Feed, Item history, Stats and Collection during real Windows play; check ended-run rates, evidence labels and saved chase pins. Disclose the [settings downgrade caveat](../architecture/storage.md#chase-settings).
- [ ] History after durable save/restart, Session/Today across midnight/DST, run/fight/player drilldowns, partial runs and storage retry preserve facts and expose failure.
- [ ] Recording opt-in/cap/disable, adapter/catalog changes, update/exit/cancelled exit, queue pressure and actual disk exhaustion: only owned closed files are evicted; capture/summaries survive recorder failure.
- [ ] White-bag, key-pop and both PB alerts: filters, local-pop suppression, mute/volume, focus, Windows permission/Do not disturb and explicit Test behave correctly. Verify sound onset/listening quality and real toast display in portable and installed builds; demo/replay stay silent.
- [ ] Keyboard/side buttons, focus restoration, screen reader, reduced motion, second monitor and 100%/150% display scale remain usable. At the 800×560 minimum window, controls remain reachable without unintended horizontal clipping. Run [preview acceptance](../development/ui-preview.md) separately.
- [ ] Complete [network acceptance](network-audit.md) for installed startup, capture, updates, shutdown and bootstrap, including observation gaps and independent WebView2 traffic. Audit the installer with WebView2 present using `-Phase webview-present -Policy app`, and absent using `-Phase webview-absent -Policy installer`.
- [ ] Meet [performance gates](performance.md) on real busy/long workloads, populated history and warm startup; include every Maxed/WebView2 process. Measure the CPU gate over a full Shatters or comparably busy dungeon run, below 2% of total machine capacity. Current memory failures remain unresolved.

## Publish

- [ ] Resolve blocking privacy, rights, integrity and acceptance findings. Record any explicit release decision and remaining limitations; never turn an exception into a passing measurement.
- [ ] Follow [serialized promotion](UPDATES.md#serialized-promotion), publishing immutable assets before replacing the channel pointer. Verify remote bytes again and retain recovery evidence.
