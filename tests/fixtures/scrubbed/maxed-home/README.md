# Synthetic Home acceptance fixtures

All inputs derive from the reviewed, generated `../maxed-demo/story.pcap` or
invented typed DTOs. No owner captures, recordings, credentials, client art or
network input are used. Packet bytes, synthetic identities and documentation
addresses are unchanged; only pcap timestamps and prefix length vary.

Regenerate deterministically from the repository root:

```powershell
python tests/fixtures/scrubbed/maxed-home/generate.py
python tests/fixtures/scrubbed/maxed-home/validate.py
cargo test --offline -p maxed-app home_replay_tests
```

The generator rejects an unexpected source hash. `manifest.json` records each
output hash and the distinction between packet replay, decoded packet injection
and typed DTO coverage. Synthetic time starts at 2026-03-08 05:59:55 UTC, five
seconds before Chicago midnight. All timestamps are nonnegative.

`complete.pcap` traverses the actual parser, TCP stream, protocol decoder,
engine and writer. Its dungeon target has 300 damage / null DPS (under three seconds), one observed
white bag and a clear. The domain parity validator separately expects 150 DPS
(300 / the two-second target window), without the product minimum; the Realm target's separate 50 damage is not dungeon
damage. Source attribution is unavailable, not inferred from the nearby boss.
`disconnected.pcap` ends before victory/loot. `zero-duration.pcap` ends after
one hit. Stop evidence and unknown DPS remain honest in the stored summaries.

The native acceptance tests collect Bridge snapshots, query through the frozen
runtime Home method, reopen SQLite and replay duplicate event receipts.
Independent replay comparisons normalize generated IDs explicitly while
preserving all numeric values, timestamps, evidence and issue fields.

Death-after-clear and consecutive sessions are typed events applied to the
store; missing identity, recovery and zero players are typed UI snapshots.
The late counter test uses real decoded base packets plus an explicitly
injected CREATE_SUCCESS counter and re-observed decoded identity. These tests
do not claim full encrypted-packet coverage for those variants.

`runtime/home_tests.rs::namespace_switch_rejects_inflight_home_read` covers native in-flight read/namespace retirement. The UI suite separately holds deferred responses across namespaces.
Natural gameplay, OS capture, WebView2 paint, physical input, DPI, Narrator,
resource budgets and owner acceptance are not established by these fixtures.
