# Synthetic P5 acceptance fixtures

All DTOs are invented transformations of `app/ui/tests/fixtures/t0-live.json`,
the admitted synthetic `maxed-types::demo` contract preview. No owner data,
recordings, identity keys, client assets, Npcap or network are used.

`events.json` contains ordered source envelopes for a first boss baseline, a
strict PB, partial and estimated non-PBs, white types 1292/1293 repeated over
eight UTC dates, a contents/source revision, runless bags and death, and a
cleared-then-died run. The final envelope is an exact duplicate of the last
original bag occurrence, intentionally out of arrival order.

`ordinary-fight.json` is expanded to 400 distinct ordinary fights ending in the
same batch by `synthetic_fixture_pack_has_exact_durable_outputs`.
`expected.json` pins counts and ordinals. Clocks are decimal UTC microseconds;
base = 1800000000000000. The test injects UTC calendar dates, using event-day
plus six preceding dates; separate B2 tests exercise DST zones.

Fight DTOs explicitly use `player_active_window_v1` for the current live DPS
contract; legacy-basis rejection is covered by the store migration/repair tests.

These are direct DTO/storage seam tests, not end-to-end packet tests. Packet
pipeline integration reuses `../maxed-demo/story.pcap` and its admitted catalog
without copying or altering their bytes. Native demo/replay acceptance tests
exercise that story with sink spies and live database/settings hashes. The
source fixture README documents its exact packet provenance and SHA-256.
