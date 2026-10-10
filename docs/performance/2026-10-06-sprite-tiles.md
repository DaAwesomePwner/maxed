# Sprite tile delivery - 2026-10-06

Implementation for issue #40 on `perf/40-sprites`, based on `71352f6`.
No live-realm memory workload or installed-release acceptance was run. Round 2
below adds isolated normal-release WebView2 and real-client extraction evidence. Sibling compilation and other checks
may contend for CPU; timings are synthetic observations, not acceptance gates.
The initial measurements are debug builds; the review follow-up below uses release.

## Decision and resource bounds

Use pre-extracted PNG tiles in one fingerprinted `tiles.pack`, with a sorted,
fixed-width disk index, searched with positional reads from each generation lease. This avoids
thousands of filesystem entries, request-time atlas decoding and full-atlas
WebView responses. Extraction uses bounded row-band reads from the existing
file-backed Unity RGBA readers;
there is no decoded atlas cache. Identical rectangles are encoded once.
The existing PNG dependency is reused. No dependencies, notices or DTOs change.

Cache format 10 changes the fingerprint, triggering a one-time extraction after
upgrade. Completed generations are validated and reused; partial publication is
never served. A changed client creates a new generation and new URLs. Generation
leases, cancellation, path/reparse checks, manifest hashes and immutable success
headers remain. Failed requests are not cached. Atlas PNGs remain on disk for
streamed trim computation, integrity checks and the owner-client oracle.

The response ceiling is `4*w*h + h + 1024` bytes: at most **263,424 bytes** for
256x256, matching the UI's pre-existing dimension limit. Its unscaled renderer
pixel footprint is at most **262,144 bytes**. A normal 8x8 tile has a 1,288-byte
encoded ceiling and 256 decoded bytes. Each concurrent request/wry copy adds
its own bounded tile cost; these bounds do not cap total browser working set or
prove the WebView's allocation behaviour.

The new extraction stage retains one RGBA tile (at most 262,144 bytes), one
fixed-capacity PNG output buffer (at most 263,424 bytes), and encoder working
memory. **The output slice bounds encoded output only**: png 0.18.1 grows its
own internal Vec, so this is not a bound on the encoder's working memory.
Extraction visits `(sheet, y, x, width, height)` while preserving the original
lookup-index order. Its temporary `(index, key)` array uses 32 bytes per entry
on Windows x64 (at most 8 MiB); building it can briefly coexist with the original
20-byte keys. Each accessed Unity sheet lazily buffers two 16-row bands, at most
2 MiB per sheet (four sheets). Pips add 4 KiB. Native code retains no decoded
tile-image collection. Unique RGBA input is limited to
1 GiB and pack size to 512 MiB. These limits bound total work/data; they are not
a wall-clock deadline. Existing text/catalog/Unity parsing has its own bounds,
so these figures are incremental tile-stage bounds, not total process memory.

The UI requests `<sheet>.png/<x>-<y>-<w>-<h>.png`, validates the loaded tile size,
and translates only relative trim coordinates. Frame dimensions, centering,
integer scaling, pixelated CSS and missing-art fallbacks are preserved. Pips
request single tiles. The prior blob tile cache was removed after the real
WebView2 comparison below found no remount refetches with plain images. Pending
frames are empty; failure fallbacks and dimension validation remain. Toast portal
art reads a tile through the existing leased delivery, with no new metadata/index
allocation on an icon-cache miss.

## Initial synthetic measurements (before review fixes)

A test-only counting global allocator records the maximum increase in live
requested heap bytes during each operation. It includes PNG-library allocations,
but excludes allocator slack, stacks, OS page cache, WebView and process overhead.
Inputs are generated before the measured scope; no game pixels are used.

| Test | Observed result |
| --- | --- |
| 4096x2048 incompressible RGBA source, 32 MiB, one 256x256 tile; full extraction | 1,241,821-byte additional live heap peak; 10.154 s cold |
| Serve that 256x256 tile | 266,787-byte additional live heap peak; 262,488-byte PNG |
| Reopen completed generation | 1.712 s; no tile encoding; pack bytes and modification time unchanged |
| Encode 66,807 unique synthetic 16x16 rectangles | 13.218 s; 37,268,798-byte pack, including index; excludes Unity reads, full-atlas publication and catalog construction |

`tile_memory` enforces an 8 MiB extraction ceiling for the 32 MiB source and a
384 KiB serving ceiling for the maximum incompressible tile. It independently
compares every tile RGBA byte with the same rectangle from the streamed atlas
PNG and verifies serving still succeeds when the atlas PNG is unavailable.
The pack tests additionally cover alpha/transparent RGB, deduplication, corrupt
spans/lengths/payloads, unknown tiles, fingerprint mismatch and cancellation.

Reproduce these synthetic checks (not the live WebView workload):

```powershell
cargo test --locked -p maxed-assets --test tile_memory -- --nocapture
cargo test --release --locked -p maxed-assets --lib synthetic_catalog -- --ignored --nocapture --test-threads=1
```

## Validation and outstanding evidence

The UI tests were changed before implementation and failed on atlas URLs,
atlas-relative transforms and tile load dimensions. Rust tile tests were added
before their implementation. Initial commit validation included 463 passing affected Rust/app tests (five existing
real-client tests ignored),
workspace Clippy, maxed-app Clippy with `perf-live`, formatting, UI type checks,
1,539 Vitest tests across 154 files, four Node tests, frontend build and diff
checks. `export_bindings` is unnecessary because DTOs are unchanged. Existing
owner-client/oracle tests remain ignored and require their private local inputs.

Round 2 below supplies real-client pack size, dimension coverage, cold/warm
extraction and route-remount evidence. Total WebView retention, concurrency,
startup memory and installed-release acceptance remain unmeasured. Background
maintenance now runs only after lease publication; it skips current/leased
generations, validates eligible v9/v10 generations once and retires pre-v9
manifests by safe identity. Interrupted deletions retain retryable names.

Interrupted first extraction restarts that generation; it does not resume a
partial pack. Oversized (>256x256) catalog refs retain the UI's existing fallback
behaviour and are omitted from the tile pack.

Central measurement must verify identical rendering with real assets, route
cache reuse, request concurrency, decoded tile retention, wry copy overhead,
startup/seed peaks and uninstrumented combined private bytes. The route-cache
probe now recognizes tile URLs; only its syntax was checked here. This work
makes no claim that the <150 MB startup/idle or <200 MB busy gates pass.

## Round 1: release measurements and request behavior (historical)

Single serial release runs on the same machine, before/after the changes, using
66,807 unique synthetic 16x16 rectangles. Cargo builds were limited to two jobs;
no perf-live memory workloads were run. This is a one-time tile-pack extraction
comparison with warm OS filesystem caches, **not physical cold-disk or complete
application startup timing**. The file-backed fixture is an 8192x2096 Unity RGBA
source; source construction, full-atlas publication, trims and catalog JSON are
outside the timed tile stage. The generated pack is 78,297,880 bytes in both runs.

| Release operation | Before | After |
| --- | ---: | ---: |
| File-backed tile extraction | 3.467532 s | 0.814496 s |
| Same pack: whole-file SHA-256 + structural validation | 316.965 ms | 68.459 ms |
| Procedural pixel encoding (no Unity reads), 37,268,798-byte pack | 628.956 ms | 645.806 ms |
| Procedural pack: structural validation only | 274.207 ms | 15.335 ms |

File-backed extraction improves by about 4.3x and hash plus validation by 4.6x.
The slight procedural-encoding increase reflects added extraction-order work;
there is no claim that encoding itself became faster.

Validation reads the entire bounded index once (`count * 64`, at most 16 MiB),
then checks payload headers in file order with a 64 KiB reader. A temporary span
array (40 bytes per entry on Windows x64, at most 10 MiB) proves exact payload
coverage with no gaps or overlaps. Existing full-file SHA-256 uses 64 KiB reads
and is retained, as are catalog membership, sorted unique keys, bounds, PNG
headers, dimensions and fingerprint checks. Old index-order payload layouts
remain readable. These timings cover one validation pass; publication pruning
also validates generations, so they are not end-to-end warm-launch timings.

Each leased delivery retains a checked open pack, expected length and immutable
64-byte index entries: 4,275,648 bytes for 66,807 keys, at most 16 MiB. Cloned
leases share them. The manifest is parsed once at lease creation; requests no
longer scan the catalog, parse JSON, canonicalize paths, or seek through the
index. A hit needs one payload seek/read plus PNG-header and per-tile SHA-256
verification. Unknown/invalid rectangles and retired protocol fingerprints
remain rejected. The blocking worker is retained for disk I/O. Toast creation
opens checked delivery metadata on an icon-cache miss.

Metadata is deliberately a lease snapshot: edits to manifest/index/path names
after validation do not replace an active lease's metadata or open file. Reload
fully revalidates disk contents. In-place payload corruption is still rejected
on every serve against the retained hash. Tests cover both behaviors.

The removed atlas module explicitly recorded WebView2 intercepted-response
refetches. There is no new live evidence establishing that immutable headers
prevent tile refetches, so explicit bounded blob retention is used. Synthetic
request counters assert 64 fetches for 128 cells with 64 unique tiles and **zero
additional fetches** on remount; a 16-cell real `SpriteImage` mount/unmount test
likewise asserts 16 then zero. A changed fingerprint fetches anew. Tests also
cover byte/count limits, LRU eviction, pinned entries, pending-request eviction,
failed/oversized responses and stale component completions. CSP permits fetches
only to the existing local sprite origin and blob image sources; protocol
responses expose CORS for those fetches. Actual WebView route behavior, concurrency
and total browser memory still require central acceptance.

The large extraction fixture now starts at y=37 and compares every served RGBA
row with the independently streamed atlas. All four pip tiles are decoded and
compared row-by-row with their 16x16 block in `enchantPips.png`. Mutation checks
were run before restoration: replacing `key.y + y` with `y` failed the large
pixel comparison; removing `key.x` from pip arithmetic failed at `enchantPip:2`
row 0. Both restored tests pass. Automatic pruning is tested with a live v9 OS
reader, a simultaneously paused extraction, and a newer publishing generation.

Follow-up validation: `cargo fmt --check`; workspace/all-target Clippy with
`--locked -- -D warnings`; the same maxed-app check with `--features perf-live`;
463 passing maxed-assets/maxed-app tests; full Vitest (1,545 tests, 155 files);
both UI type checks (zero errors/warnings); four Node tests; and frontend build.
The five private-input tests remain ignored. The two explicitly ignored synthetic
timing tests were run separately in release mode as shown above. No DTOs,
dependencies, lockfiles or game artifacts changed. Packaging configuration,
release-build, release-metadata, workflow-policy, bundle-audit self-tests and
network-audit self-tests also pass, as does `git diff --check`. These are static
and synthetic checks, not installed WebView2, Npcap or installer acceptance.

## Round 2: real WebView2 and client evidence

A normal Windows `custom-protocol` release was compiled using the temporary
absolute `TAURI_CONFIG.identifier` plus window `dataDirectory` technique in the
[live-realm attribution notes](2026-10-06-live-realm-attribution.md). `perf-live`
was absent and no memory workload or capture was started. The executable's
compiled temporary identifier was attested before launch. Only owned processes
were closed; the production profile and main checkout were not accessed.

A temporary environment-gated handler counter recorded one line per protocol
request, without URLs or user data; load timing used `Instant` around the normal
native asset load. Both diagnostic hooks were removed before commit. Installed
client discovery populated only the temporary profile. A temporary 64-entry
chase list selected real catalog items (63 distinct tile URLs). UI Automation
navigated Collection -> Home -> Loot -> Collection five times; keyed navigation
unmounts the previous route. Windows screenshots confirmed matching rendered
sprites in each path and in the final tightened-CSP build. Screenshots, game
pixels, catalogs and local profiles remain outside the repository.

| Path | First mount | Additional hits on remounts 1 / 2 / 3 / 4 / 5 | Cumulative hits |
| --- | ---: | --- | ---: |
| A: bounded blob cache | 63 | 0 / 0 / 0 / 0 / 0 | 63 |
| B: plain `<img src=tileUrl>` | 63 | 0 / 0 / 0 / 0 / 0 | 63 |

**Decision: remove the blob cache and fetch, the wildcard CORS header, sprite
`connect-src`, and `img-src blob:`.** The final configuration also rendered
successfully with 63 initial hits. This is direct evidence for same-process route
remounts on this WebView2 installation, not a claim about eviction under memory
pressure or every WebView version. No total browser-memory result is inferred.

| Real-client observation | Result |
| --- | ---: |
| Catalog sprite refs | 66,811 |
| Unique encoded tiles | 59,250 |
| Pack bytes, including index | 27,977,805 |
| Refs exceeding either 256px dimension | 0 |
| Cold normal load into empty temporary cache | 4.238 s |
| Warm normal loads | 0.525 / 0.534 s |
| Warm load with retirement fixture present | 0.556 s |

Cold means a cache miss, not flushed OS caches. These are single diagnostic
observations, not startup gates; sibling build/test activity can contend for CPU.
For migration, a v9-format generation was reconstructed in the same temporary
profile from the real atlas/catalog bytes: no pack, version 9, a separate safe
fingerprint and recalculated catalog stamp. On normal v10 publication the
background sweep removed it: two generations became one, with zero retired
directories remaining. This checks real-sized v9-format retirement, not a launch
of a historical v9 executable. Synthetic Windows tests additionally protect a
live v9 lease and a concurrently paused extraction.

### Resident index and unaligned extraction

The old delivery retained `64 * tile_count` index bytes. The new delivery keeps
**zero resident index bytes**, plus a 24-byte `TilePack` (file/count/length) on
Windows x64. That is **4,275,648 -> 0 bytes** for the review's 66,807-entry example,
or **3,792,000 -> 0 bytes** for this client's 59,250 unique entries. Arc and OS-file
overhead are not included. Opening uses a temporary 64 KiB validation reader.
Each lookup needs at most 16 index reads for this real pack, followed by one
payload read and SHA-256 verification; concurrent reads have no global mutex.
Native notifications reuse the lease's delivery instead of reopening it.

The release fixture now uses 66,807 16x16 tiles offset by eight pixels in both
axes on an 8200x2104 synthetic Unity sheet. Serial measurements changed only
single- versus two-band selection; both produced 78,297,880-byte packs. These
numbers exclude fixture construction, atlas publication, trims and catalog JSON.

| File-backed unaligned tile stage | Single band | Two bands |
| --- | ---: | ---: |
| Extraction | 2.860334 s | 0.859177 s |
| Whole-pack SHA-256 + structural validation | 71.343 ms | 67.502 ms |

Two bands were about 3.33x faster here, with a documented 2 MiB maximum per sheet
(8 MiB across four sheets). Tests check byte-exact unaligned rows, a 29-row sheet's
13-row last band, and cached reads after replacing the backing handle with an
empty file.

Pruning holds LEASES only for the in-process eligibility check and exclusive OS
lease acquisition. Current is never hashed by maintenance; ordinary old
candidates are validated once. Quarantine timestamps are checked only when
needed and quarantines lease their own identity, so a repaired current lease
cannot pin them. Each eligible generation is renamed before deletion. A Windows
held-handle test models an indexer opening that retired tree: the first sweep
partially removes it and continues to another entry; after closing the handle,
the next sweep deletes the remainder without needing a valid manifest. A handle
already open before retirement can prevent the rename itself on Windows; that
leaves the original generation intact for retry. Pre-v9 identity retirement and
cached native-toast delivery also have regression coverage.

### Round 2 validation

All required checks pass: `cargo fmt --check`, workspace/all-target Clippy with
`--locked -- -D warnings`, maxed-app/all-target Clippy with `--features perf-live`,
and 470 passing maxed-assets/maxed-app tests. Seven tests remain ignored in the
normal run (five private-input tests and two opt-in timing tests); the unaligned
timing test was run separately as above. Full Vitest passes 1,541 tests in 154
files; both Svelte checks report zero errors/warnings; four Node tests and the
frontend build pass. Packaging config (53 checks), release-build,
release-metadata, workflow-policy and both audit self-tests pass. The final
normal custom-protocol release uses the same isolated compiled profile and
contains neither the perf-live marker nor the diagnostic hook. `git diff --check`
passes. No dependencies, DTOs, lockfiles, sprites or screenshots changed.

Additional coverage includes concurrent publication of the same fingerprint,
concurrent positional delivery, stale UI load events and held toast-icon handles.
Old toast artwork moves inside its generation's retryable retirement before
recursive deletion, so its own sharing violations cannot strand an untracked
icon directory. Native validation and delivery setup also run outside LEASES;
only publication coordination and lease bookkeeping use that mutex.
