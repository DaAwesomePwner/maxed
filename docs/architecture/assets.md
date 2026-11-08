# Native client assets

`load_or_extract(&AssetPaths, &AtomicBool, progress)` is the production entry
point. Call it on a background worker; obtain `app_data` from the application's
path resolver. Retain the returned `CachedCatalog` for as long as its catalog or
PNG paths are used. Its lease protects the generation from maintenance, including
readers in another Windows process. No helper, JVM, network operation, account
API or client-file write is involved.

## Reader and memory

The installed client uses serialized version 22, Unity 6000.0.58f2, little endian,
and stripped type trees. The Texture2D profile reads the current named fields,
including the mipmap-limit group string and separately aligned streaming flag.
It is restricted to this validated engine build, RGBA32, one image, one mip and
2D textures. Complete-image size, inline/stream payload length, object bounds and
the exact trailer must agree. Unsupported layouts fail explicitly. This is a
validated field layout, not fixed Java offsets or a claimed type-tree decode.

Serialized versions 17–22 have synthetic header/metadata/TextAsset tests in both
byte orders, with and without bounded type-tree blobs. Type trees are skipped as
opaque metadata; arbitrary type-tree object decoding is not implemented. The
actual client has 108,015 objects and 255 TextAssets. Streams must resolve inside
the selected client directory; absolute, traversal, alternate-stream and escaped
symlink paths are rejected.

`open_sheets` returns file-backed `SheetReader`s. `read_row` reverses the Unity row
order, retains all four source RGBA channels, and checks cancellation. PNG writing
uses the PNG encoder's streaming writer and one row (at most 64 KiB), never a
whole sheet. The four real sheets total 336 MiB decoded, but are never retained
together. Text input has a 128 MiB aggregate ceiling, each text is at most 64 MiB,
catalog construction has conservative retained-data accounting, and all counts,
dimensions, XML depth, record trees and FlatBuffer offsets are bounded.

`locate_enchant_pips` resolves the client's `RarityIcon_1..4` enchant pips by
sprite name: SpriteAtlas packed-sprite names lead to the Sprite objects, whose
render-data keys select each pip's texture and rectangle in the atlas render-data
map. The GUI atlas is repacked on patches, so neither its hashed texture name nor
any rectangle is fixed here. Only packed, unrotated 16×16 RGBA32 pips are
accepted. The cache writes them as its own 64×16 `enchantPips.png` strip (pip `n`
at x = 16·(n − 1)) with catalog refs `enchantPip:1..4`. Missing, ambiguous or
unsupported pip art leaves both out instead of failing extraction.

`read_resources` remains a bounded collecting convenience for small/synthetic
inputs. It rejects the complete installed client's sheets at its aggregate cap.
Production uses `read_text_resources` plus the streaming sheets through the cache.

## Catalog semantics

Catalog v1 includes sorted item/projectile/scaling definitions, objects, class
caps, enchant descriptions, dungeon rules, bags, damaging tiles and sprite refs.
Equipment/projectile-bearing definitions share `items`, including enemy/projectile
types needed for damage lookup. XML DTDs and undeclared entities are rejected;
there is no external entity resolver. XML failures identify a safe catalog section.

Provenance is RealmShark's AssetExtractor, IdToAsset, AbilityScalingManager,
CharacterClass, ParseEnchants, DungeonCatalog, BundledAreaCatalog,
CharacterStatistics, LootBags, SpriteFlatBuffer, ImageBuffer and generated
`assets/flattbuffer` schema accessors. `rules.rs` freezes reviewed area aliases and
completion bit indexes as rules v1. Java's explicitly unverified completion
indexes remain absent. Portal IDs alone do not establish dungeon classification;
reviewed rules, dungeon flags/templates and safe-zone evidence do. Ambiguous
display aliases are not introduced.

Duplicate IDs inside a single XML definition source fail with a safe section
error and a numeric-ID tracing diagnostic; raw XML is never logged. Across sources, the
explicit stable source-name order uses the reference's last-definition-wins
behavior; this exception is tested. Sprite mappings follow the reference's
static-then-animated source order, including its last-frame override. Rectangle
bounds use the actual sheet dimensions. Missing sprite keys become `None`.

Unavailable data stays unavailable:

- Out-of-u16 object types cannot occur in the frozen wire type contract; they are
  omitted with a numeric diagnostic, never truncated. Oracle ID checks use the
  same representable range.
- Fractional projectile lifetimes cannot fit Catalog v1's integer milliseconds;
  those optional lifetimes remain `None`. Missing projectile arithmetic is not
  converted to zero damage.
- Scaling and proc-reference searches visit descendants in document order, as
  Java DOM does. The first positive Activate scaling wins; OnConditionEndActivate
  is the fallback. Zero/negative scaling is skipped so later valid entries work.
- Only explicit, unambiguous proc references inherit a known parent's scaling and
  `origin_item`. Ambiguous/unscaled origins are unavailable in this DTO.
- Missing class caps remain absent; missing scaling inputs remain unavailable.
  The reference's documented scaling minimum/shots defaults are preserved.
- Enchant IDs are data, not rarity. Reviewed bag rules supply ranks (white is
  255); excluded soulbound bags have no rank. Enum declaration order is not used as rarity evidence.
- This client supplies no supported local crucible damage-multiplier definitions;
  `crucible_bonuses` is empty, not a fabricated multiplier of one.

## Character metadata: Gate X (2026-10-17)

Evidence revision: RealmShark `27d02b70de1c00616228dbe2f5ca4592c0fb4df0`.
This is a source-derived reference model, **not verified against the owner's RotMG 7.1.0.2.0 client**.
No installed XML was available on this Mac. No game XML or extracted catalog is included.
The linked GUI delegates XML loading to `Equip`/`Weapon`; those same-revision sources are included below.

| Field / question | Evidence and extraction decision |
| --- | --- |
| Rate units | `RateOfFire` is a decimal multiplier; 1 means 100%. [GUI lines 445–465][x-gui]. |
| Item rate and count | Child elements `RateOfFire`, `NumProjectiles`. [Equip lines 49–60][x-equip]. |
| Subattack | Attribute `projectileId` selects a projectile; child `RateOfFire` overrides item rate. [Equip][x-equip]. |
| Rate inheritance | Missing subattack rate inherits an explicit item rate. [Weapon lines 29–34][x-weapon]. |
| Omitted fields | No subattacks + explicit item rate: projectile 0, omitted item count 1. [Weapon][x-weapon]. |
| Missing subattack count | Missing `NumProjectiles` has no supported positive default. [Weapon][x-weapon]. |
| Missing rate / reference | No default is adopted for missing item rate or `projectileId`. [Weapon][x-weapon]. |
| Projectile lookup | Groups resolve the `Projectile` attribute `id`, not list position. [Equip][x-equip]. |
| Damage | Existing fixed `Damage` and ranged `MinDamage`/`MaxDamage` reconstruction stays intact. |
| Fire together | Plain subattacks are summed in this reference model. [GUI lines 449–467][x-gui]. |
| Special schedules | Alternating, charged, burst, conditional and interval semantics are unestablished. |
| Regen structure | `Enchantment` → `Mutators` → `ActivateOnEquip`, attributes `stat`, `amount`. [Parser][x-parse]. |
| Regen kind / stat | Text `FlatRegen` or `PercentageRegen`; exact stat `HP` or `MP`. [Parser lines 106–164][x-parse]. |
| Regen units | Flat amount is units/sec; percentage amount is fraction of maximum/sec, without division by 100. |
| Regen arithmetic | The fractional coefficient multiplies max HP/MP. [Parser lines 446–547][x-reg]. |
| Interval units | Neither interval attribute nor conversion is established. Explicit intervals remain unsupported. |
| Regen defaults | Missing/malformed amount or stat is unknown, never zero. Empty reviewed mutators mean no effect. |

The rate-less display fallback in `Weapon.fix` lines 35–43 invents one bullet per projectile; Maxed deliberately
rejects it. `ScalingDef.shots` remains ability metadata. Missing XML projectile IDs retain the existing positional
index only for reconstruction; Character cannot treat that index as established firing evidence. Duplicate IDs
are rejected even when damage is incomplete. Fractional lifetimes remain unavailable, without losing damage.

Supported firing requires explicit positive finite rates (inherited where above), counts 1–256, resolvable
projectile IDs and the plain normal-fire shape. Item-level normal fire without subattacks selects index 0 with
an omitted count of 1. Multiple plain subattacks use the reference's additive model, not verified live timing.
Unknown item firing structure or subattack attributes/children disable the affected schedule. Unreferenced or
incomplete projectile definitions prevent complete support; valid independently understood groups remain a
partial subtotal. Reasons are controlled codes, never XML values or names. Groups and projectile lists are
bounded to 256. Existing valid projectile definitions remain available to combat reconstruction.

Supported regen effects contain an explicit finite nonnegative amount and HP/MP stat, with no extra attributes
or nested content. Flat and fractional effects remain separate. Explicit intervals (including an apparent
one-second interval), unknown mutators, and conditional activations stay unsupported; known independent effects
remain partial. Reviewed damage/rate/loot/stat-damage mutators establish no regen effect, as does an empty
mutator list. OOC doubling shown by the reference is excluded from this baseline metadata. No description text
is interpreted. Unknown metadata from old JSON remains absent, distinct from supported empty effects.

[x-gui]: https://github.com/X-com/RealmShark/blob/27d02b70/src/main/java/tomato/gui/myinfo/MyInfoGUI.java#L445-L467
[x-equip]: https://github.com/X-com/RealmShark/blob/27d02b70/src/main/java/tomato/gui/myinfo/Equip.java#L49-L105
[x-weapon]: https://github.com/X-com/RealmShark/blob/27d02b70/src/main/java/tomato/gui/myinfo/Weapon.java#L18-L50
[x-parse]: https://github.com/X-com/RealmShark/blob/27d02b70/src/main/java/tomato/realmshark/ParseEnchants.java#L92-L174
[x-reg]: https://github.com/X-com/RealmShark/blob/27d02b70/src/main/java/tomato/realmshark/ParseEnchants.java#L446-L547

Version 12 invalidates version 11 fingerprints and manifests so old additions remain unknown until rebuilding.
Catalog format stays at 1. Rebuild errors preserve the prior publication; native capture already handles these
errors with an unavailable catalog and missing-assets coverage (`app/src-tauri/src/runtime/storage.rs:101–110`).
This extraction change does not alter that fallback or claim live capture verification.

## History classification metadata

The asset bridge retains one immutable, shared `CatalogQueryData` per published
catalog fingerprint. It prepares normalized tier/potion SQL parameters, the UT/ST
item set and representative potion item types once. Requests clone its Arc-backed
handle; their history keys contain only a process-local generation identity and
validation status. Replacing the catalog changes every catalog-dependent history
key, including run recap. Recap only consumes UT/ST membership and retains its
previous behavior for unrelated tier values exceeding loot/HUD metadata limits.
Failed selections retain the previous metadata alongside
the last-good catalog. Synthetic demo mode uses its own shared metadata instance.

`PotionCount.object_type` supplies the HUD sprite lookup directly. Representatives
prefer the exact base potion name for the classified stat, then the lowest object
type; ties between base definitions also use the lowest type. Missing
classification remains `None`; Home does not search item names or duplicate potion
classification rules. Counts and stat grouping still come from the native catalog.

## Cache and cancellation

Generations live at `<app_data>/assets/v1/<fingerprint>/`. Fingerprints cover
reader/catalog versions, cache format version 12, the SHA-256 of `rules.rs`,
and sorted relative input names, sizes and SHA-256
content hashes, including selected referenced companion resources. Extraction
hashes inputs before and after; a client patched during extraction is rejected.

Each generation contains `catalog.json`, PNG sheets, `tiles.pack` and a bounded
manifest of sizes/hashes/dimensions. Loading validates format, hashes, unique/sorted catalog
keys, finite math, references, PNG metadata and rectangle bounds. An invalid
generation is never served. After a brief lease check, an invalid generation without
readers is exclusively leased, revalidated, and renamed to an invocation-unique
`.corrupt-<fingerprint>-<pid>-<timestamp>-<counter>` sibling before re-extraction.
Its original files remain intact. Reader-held invalid generations return
`CacheInvalid`; retry can repair them after all readers release their leases.
The same recovery applies if an invalid generation appears during publication.

Publication prepares and flushes an invocation-owned staging sibling, validates
it, renames the generation, then atomically replaces a small `current` pointer.
If Windows cannot replace that pointer, the old pointer remains and the call
returns `CacheInvalid`; there is no delete-then-rename fallback. An unpublished
complete generation is harmless and can be reused on retry.

Cancellation precedes filesystem access, is checked during hashing, per object,
per PNG row and before publication, and cleans only this invocation's verified
stage. Unrelated/interrupted stages are retained. After a caller receives and
publishes a lease, `prune_in_background` schedules best-effort maintenance;
`load_or_extract` never waits for that sweep. Current and in-process leased
generations are skipped. An exclusive Windows sharing lease protects readers
and extractors in other processes, and the current pointer is rechecked after
acquiring it. The LEASES mutex covers only eligibility and exclusive-lease
acquisition, never hashing, directory traversal, rename or deletion. A separate
nonblocking OS maintenance lease serializes sweeps across processes.

Eligible v9/v10 generations are validated once under the exclusive lease.
Pre-v9 generations require a safely parsed matching manifest identity and version,
without deserializing obsolete catalogs. Every retirement checks the canonical
cache boundary and rejects all descendant links/reparse points. Before deletion,
the directory is renamed to `.retired-<fingerprint>-<unique>`. Eligible old toast
artwork is moved into that retirement so failed artwork cleanup is retryable too. Recognizable
retirements are retried without content validation, including partially deleted
trees. Per-entry failures warn using only sanitized error variants and do not
abort other entries. Unknown directories and malformed names remain untouched.

Recognized `.corrupt-*` quarantines use their own exclusive lease, so a shared
lease on the repaired generation does not prevent cleanup. They must predate the
current published manifest; that timestamp is read only if quarantines exist.
Maintenance never rehashes the current generation to find a timestamp. Without
a newer completed publication, cancelled repairs retain their quarantine.
Non-Windows recovery/pruning fails closed because no exclusive lease is available.


## Tile delivery

Cache version 10 also prepares an immutable PNG tile pack. UI URLs are
`http://maxed-sprite.localhost/<fingerprint>/<sheet>.png/<x>-<y>-<w>-<h>.png`.
The native scheme form is `maxed-sprite://localhost/...`. Full-atlas requests
are rejected. URL parsing accepts only canonical lowercase fingerprints,
allowlisted sheets and decimal rectangles; no URL component becomes a disk path.

The pack has a 76-byte version/fingerprint/count header, a sorted 64-byte index
entry per unique rectangle, and individual RGBA PNGs. Each entry contains the
sheet/rectangle, offset, length and PNG SHA-256. Generation validation matches
this index exactly against the renderable catalog rectangles, checks contiguous
spans and PNG dimensions, and verifies the whole pack via the manifest. Delivery
checks manifest identity/length, pack identity and index ordering/spans once per
lease with a temporary 64 KiB reader. It keeps the open file, count and length,
with **zero resident index entries** (24 bytes of file/scalar fields on Windows
x64). Each request binary-searches 64-byte disk entries using positional reads
(`seek_read` on Windows, `read_at` on Unix), then reads the bounded payload and
checks PNG dimensions and SHA-256 against its disk entry. There is no shared
cursor mutex, catalog scan or request-time manifest parse. Native toasts reuse
the same delivery. The pack is immutable by application contract; index entries
and hashes are read afresh, not snapshots resistant to coordinated in-place
index/payload tampering. Reload verifies the whole pack against the manifest.
The generation lease and before/after bridge fingerprint checks protect
replacement. Successes retain one-year immutable headers; failures are `no-store`.

Extraction visits sprites by `(sheet, y, x, width, height)`, with two lazy 16-row
source bands per accessed Unity sheet (at most 2 MiB per sheet, 8 MiB across four
sheets). Alternating slots reuse both bands for unaligned 16px tiles, including
a short last band. Larger tiles may revisit bands within this fixed bound. The disk index
keeps its original key order; payload spans are validated in file order. Top-down
orientation and every channel, including transparent RGB, are preserved. One tile is encoded at a time using the existing `png` dependency. Duplicate
rectangles share one entry. Pips use the existing 4 KiB extracted strip as input.
There is no full-atlas decode or encoded-atlas response on either the UI or toast
path. Atlas PNGs remain on disk for the existing streaming trim pass, integrity
checks and owner-client oracle; they are not WebView resources.

Tile dimensions follow the existing UI limit of 256x256; larger catalog refs
continue to render as fallbacks. Scratch includes at most 262,144 RGBA bytes,
263,424 encoded-output bytes and PNG encoder working memory. The fixed output
slice does not bound png 0.18.1's internal growing Vec. Extraction temporarily
holds 32-byte `(index, key)` entries on Windows x64 (at most 8 MiB), which can
briefly coexist with the original 20-byte keys. Validation reads the <=16 MiB
index once and uses a temporary <=10 MiB span array and 64 KiB buffered payload
reader; full-file hashing retains its existing 64 KiB reads. The lease keeps
only scalar metadata and the file, without a resident index or decoded tile collection. The aggregate unique RGBA input is capped
at 1 GiB and the pack at 512 MiB to bound malformed overlapping-catalog work.
Cancellation is checked between tiles and source rows. Existing staged,
hash-validated publication is unchanged: a completed fingerprint is reused
without encoding again; failure/cancellation removes only the invocation's stage.
Interrupted partial packs are not reused, and a new cache/client fingerprint
creates a new complete generation. Successful cold and warm publication can
schedule pruning after handing out the lease; catalog readiness and capture
start never wait on maintenance.

`SpriteImage` uses a plain local `<img>` and relative trim offset for fitted art.
Frames remain blank while loading, show the tile after a dimension-checked load,
and show the missing-art glyph only for missing/invalid art or an actual failure.
Item pips request individual tiles too. There is no JS blob cache, cross-origin
fetch or CORS header, and CSP needs only the existing sprite `img-src` origin.
The [real WebView2 route experiment](../performance/2026-10-06-sprite-tiles.md#round-2-real-webview2-and-client-evidence)
observed 63 initial protocol hits and zero additional hits across five remounts
for both blob and plain-image delivery. This justifies the simpler path for the
measured case; it is not a browser eviction or total-memory guarantee.

## Validation

All normal fixtures are synthetic. Owner-client tests are ignored and gated by
`MAXED_CLIENT_ASSETS` (resources.assets) and `MAXED_ASSET_ORACLE` (the RealmShark
`assets` directory). Game art/XML are never copied into the repository.

```powershell
cargo test --offline -p maxed-assets
cargo test --offline -p maxed-assets --test unity installed_client_spike -- --ignored --nocapture
cargo test --offline -p maxed-assets --test oracle -- --ignored --nocapture --test-threads=1
cargo test --offline --workspace --no-fail-fast
cargo clippy --offline --workspace --all-targets -- -D warnings
cargo fmt --check
```

Verified against resources.assets SHA-256
`78c1a9ffde9776c2bd5db7f19a1857c44ed16e5dfd986848c24a921dc4933328`:

- 246 extracted XML files and the 13,730,968-byte spritesheet mapping match the
  oracle byte-for-byte. Representable object IDs, player/boss/miniboss flags,
  ASCII display names (trimmed scalar whitespace), damaging-tile IDs and damage
  ranges match the Java ID lists. Non-ASCII ID-list names cannot be used as a
  lossless oracle because the Java list uses the Windows default charset.
- Built 36,404 objects, 19,657 item/projectile-bearing definitions, 19 classes,
  1,016 enchants, 66,807 sprites and 190 damaging tiles.
- All 88,080,384 pixels of the four native PNGs decode back to the original client
  RGBA rows exactly. The Java pixel oracle also passes: every opaque pixel is
  identical, and every other oracle pixel exactly equals `java_png_model(native)`.
  There are no unexplained pixels and no tolerance/exemption bucket. See
  D-sprite-alpha below for the deliberate native/Java difference.

| Sheet | Modeled transparent differences | Modeled partial-alpha differences | Unexpected |
|---|---:|---:|---:|
| characters | 19,529,682 | 39,165 | 0 |
| characters_masks | 7,470,501 | 19 | 0 |
| groundTiles | 1,311,534 | 9,730 | 0 |
| mapObjects | 19,634,142 | 96,197 | 0 |

## D-sprite-alpha: preserve the client's exact RGBA

Maxed deliberately retains the client's original RGBA bytes, including RGB at
zero alpha. RealmShark's exported sheet PNGs differ because its
`assets/resextractor/UnityExtractor.java:92-106` wraps those straight RGBA bytes
in an interleaved raster and a **premultiplied** sRGB `ComponentColorModel`. It
constructs a premultiplied `BufferedImage`, vertically flips it using
`Graphics2D.drawImage` into a new, transparent **TYPE_INT_ARGB** image, then writes
that image through `ImageIO.write(..., "png", ...)`. `ImageBuffer.getSprite`
subsequently reads/crops those PNGs; its outlined-icon rendering is not part of
sheet publication. There is no TYPE_INT_ARGB_PRE destination in this sheet path.

The exact test-only `java_png_model` accounts for two conversions:

1. `ComponentColorModel.extractComponent/getRGB` expands each RGB component
   using Java's single-precision arithmetic and rounds it, then packs ARGB with
   bitwise OR. It does not clamp expanded components before packing. Since
   the input was straight RGBA, a component greater than alpha can exceed 255
   and spill bits into adjacent channels, including alpha.
2. Java2D SrcOver onto the transparent destination quantizes RGB through its
   8-bit premultiply/unpremultiply tables. The model computes those fixed-point
   table entries exactly. PNG writing preserves the resulting ARGB values.

This is more than a one-byte alpha rounding difference: for example,
`[240,0,238,128]` becomes `[221,2,217,129]`, while `[63,0,0,3]` becomes
`[233,0,0,23]`. The source behavior is confirmed by
[OpenJDK ComponentColorModel](https://github.com/openjdk/jdk/blob/master/src/java.desktop/share/classes/java/awt/image/ComponentColorModel.java)
and [Java2D AlphaMath](https://github.com/openjdk/jdk/blob/master/src/java.desktop/share/native/libawt/java2d/loops/AlphaMath.c).
The Rust helper is a mathematical test model of this particular Java pipeline,
not an alternate production decoder or a general-purpose color conversion.

Independent synthetic validation uses `tests/common/JavaPngVectors.java`, which
calls AWT and round-trips ImageIO PNG without implementing the Rust equations.
Its 262,144 inputs cover every channel value at every alpha, isolated channels,
and mixed-channel spill. OpenJDK 17.0.20.1 on Windows produces RGBA SHA-256
`4aa41606dcc71600ef5ae7cb7c4ebd911764ea2cec46e4368da572d45b821e29`;
the normal Rust test must match that digest. A JDK can regenerate it with
`java -Xmx64m crates/maxed-assets/tests/common/JavaPngVectors.java`.
Normal Cargo tests and production do not invoke Java or require a JVM.

The ignored real-client oracle checks **every** pixel, including cases where
native equals oracle: opaque pixels must remain identical, and nonopaque pixels
must equal the exact model in all four channels. Synthetic mutation tests reject
every single-channel deviation from the expected value, including transparent
RGB and alpha. All native/Java differences are recorded as exact per-row
`[x_start,x_end)` regions in private temporary JSONL files, and any unmodeled
pixel makes the test fail with its coordinates and native/oracle/modeled RGBA.
The historical real-client check records zero unmodeled differences, including
all 41,392 alpha mismatches that the RGB-only rule cannot explain.
**No bounded-tolerance fallback is used.** See the [parity ledger](../parity/deliberate-differences.md).


## Presentation metadata

Sprite references include validated trim rectangles for visible-content fitting. Enchant pips are resolved by name rather than fixed atlas coordinates. Dungeon portal visuals use catalog portal types; missing or ambiguous mappings stay unavailable. Consumables, stacks and object-class metadata prevent a game-supplied UT label from making an ordinary potion a chase item. These fields are part of cache version 10; use the cache fingerprint for UI invalidation.

The crate's [UnityPy license](../../crates/maxed-assets/UNITYPY-LICENSE.txt) and root [notices](../../NOTICE.md) preserve attribution. Asset oracle measurements above are historical, tied to the stated client hash; they do not establish compatibility with another client build.

## Character calculation handoff

Character consumes the reviewed [Gate X metadata](#character-metadata-gate-x-2026-10-17)
from the same catalog generation as the local observation. Gate X establishes the
source model above; it does not establish current-client firing or regeneration
behavior. Task 11's Windows acceptance must verify those coefficients separately.
Only plain normal fire and independently supported subattacks contribute; charged,
alternating, burst, conditional and interval schedules remain unavailable. Regen
retains flat units/sec and fractions of maximum/sec separately, without a percent
or interval guess. Unknown effects preserve supported partial subtotals, never a
complete fabricated zero.

Cache version 12 invalidates older metadata; catalog format remains 1. The UI uses
the catalog fingerprint in calculation revisions and refreshes visual lookups on
catalog replacement. No game XML or art is included in the Character preview:
synthetic item names and missing-art fallbacks suffice. Missing catalogs preserve
observations and link to Settings for extraction; they do not create a new network
metadata source or change capture behavior.
