# Fixture policy

Only reviewed fixtures under `tests/fixtures/scrubbed/` may be committed. Never commit raw recordings, tokens, account data, player names, game sprites or extracted client catalogs. The directory name alone is not evidence of safe content.

## Three distinct states

| Classification | Meaning |
| --- | --- |
| Synthetic | Every packet/identity is invented or deterministically generated from admitted synthetic inputs. Use documentation network addresses and clearly synthetic identities such as DemoHero. |
| Scrubbed real | A real observation transformed by the scrubber, with its exact source/scrubber revisions, profile hash, output hashes, certification and limitations recorded. |
| Approved for publication | A maintainer explicitly reviewed rights, identity/credential risks, retained literals and provenance for public release. This is required in addition to scrub certification. |

There are five scrubbed-real fixture groups: `maxed-nexus-key-pop`, `maxed-realm-dungeon-return`, `maxed-realm-focus`, `maxed-realm-plague-doctor` and `maxed-shaitan-run`. Their README/provenance files describe historical observations. All five have the separate publication approval recorded below; other fixtures require their own review.

## Publication approval (2026-11-05)

**Approved for publication:** the owner (maintainer) explicitly approved the five scrubbed-real fixture groups listed above for the one-time export to `daplabs/maxed` on 2026-11-05.

The owner's recorded basis is each fixture README's `maxed-cli scrub` certification, `verified=true provisional=false`: tokens, keys and account IDs filled; player, guild and pet names pseudonymized; chat filled; and network addresses replaced. A root `strings` scan found no emails, JWTs, long token-like strings or file paths. This records the owner's review evidence; it does not claim a new scrub or independent re-certification.

The approval retains the README provenance, admitted literal coincidences and historical limitations, including the short opaque-interior coincidences in `maxed-realm-dungeon-return`. Capture bytes and `expected.json` files are unchanged. The approval covers those exact bytes; changed captures require renewed review.

The one-time export to this public repository (2026-11-05) ran with the owner's account name as a deny pattern and passed. Never commit the account name. Scrub certification and publication approval do not establish live-game acceptance.

## Admission and change review

Record immutable input/output hashes, generator or scrubber revision, profile version/hash, transformations and independently justified expectations. Keep synthetic packet coverage, decoded packet injection, typed DTO tests and live observations distinct. Preserve historical `source_build` values verbatim.

Treat existing capture bytes and provenance as immutable unless a replacement is explicitly reviewed. Generators must reject changed inputs rather than silently repin expected results. A new scrub profile does not retroactively certify existing fixtures. Regenerate to a new local output, replay through the real parser/engine, compare supported invariants, inspect privacy evidence locally and review the new hashes before replacing anything.

The scrubber removes credentials, pseudonymizes supported identities, fills unclassified strings and replaces network addresses. Its [notice grammar](scrubbing.md) admits only exact templates. Collision checks remain mandatory even for retained game data. `PublicLiteralCoincidence` records harmless chat/game-literal overlap; identity or credential collisions still refuse certification. Certification reports identify fields and secret classes, not source secrets.

An older fixture may have permanently filled evidence that the current scrubber preserves. Keep the conservative expectation until a reviewed replacement exists; never invent the missing player or announcement. Shaitan's filled death notice and the older realm-focus announcement loss are documented examples.

## Public evidence

Review all fixture directories and the complete release snapshot before publication. Keep raw captures, private catalogs and inspection exports outside version control. Public issues and screenshots must exclude real player/character names, account information, user paths and non-documentation network addresses. Required third-party attribution is retained verbatim in legal inputs.
