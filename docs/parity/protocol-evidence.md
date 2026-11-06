# Protocol evidence and limits

Packet definitions and typed decoders in `crates/maxed-protocol/src` are authoritative for implemented layouts. Historical capture research supplies hypotheses and regression cases, not permission to consume unknown tails or infer unsupported semantics.

| Observation | Supported interpretation | Limit |
| --- | --- | --- |
| DEATH (46) | Trimmed `killedBy` is the killer display text; a case-insensitive catalog name match supplies a type only when unique | `totalFame` is still ignored; recap fame is the observed character fame and HP is only the local HP explicitly observed before death |
| QUESTOBJID (82) | Signed big-endian object ID, compressed count and list; count bounded to 0–64; -1 clears the target | Quest switches alone do not prove a kill; generation resolution is local |
| REALM_SCORE_UPDATE (169) | One signed big-endian absolute score | MAPINFO supplies the maximum; sentinels stay unavailable; score does not prove a realm has closed |
| MAPINFO | Area, realm name, seed, open time, capacity and optional difficulty/modifiers | Parent realm names may appear in dungeons; a name alone is not an instance identity; unknown grades/modifiers stay unknown |
| Player UPDATE objects and drops | Observed instance population | A derived count, not a verified server population counter; NEWTICK alone cannot admit a player |
| PlayerDeath / PortalOpened notices | Exact recognized template data | Chat (TEXT) is stored for Logs; notice templates still do not retain their raw text or export opener/victim identities. Missing identity prevents a reliable join |
| Server HP and removal | Target observations keyed by generation | Positive HP/HP1, quest changes, bags and portals alone do not confirm death |
| Oryx death/killed templates | Recognized instance announcement, optionally corroborated by removal | A distant announcement creates no fabricated fight or damage |
| Stat 57 | Observed current fame | Stat 39 and death total fame are not current-fame samples; stat 80 is not a supported fame goal |
| Stat 62 | Observed guild name, retained only for player entities and stored in their scope profiles | Capped at 64 UTF-8 bytes on a character boundary; never written to diagnostics or logs; guild names do not establish player identity |
| Stat 115 / name strings | Sensitive identity data; Party links player history by its display-name key | `maxed_types::party::name_key` takes text before the first comma and ASCII-lowercases it; renamed players start new history. Never expose diagnostic values or infer grave identity from names alone |

`NOTIFICATION` effect 24/extra 32 has an unexplained two-byte tail in historical evidence. Some CREATE_SUCCESS PcStats blobs leave 25 bytes after a 119-of-144-byte interpretation. Keep strict rejection/incomplete counters; do not invent field layouts or completion baselines. Startup field failures before verified tick alignment may be mis-decryptions rather than protocol changes.

Bag birth and proximity do not prove a dropper or kill. The existing reference attribution runs first; the unique killed-target fallback within 1.5 tiles in the same/next tick is explicitly estimated. Unknown contents and later enrichment remain distinct from ownership or who took an item.

The current detector uses [kill evidence](kill-evidence.md), including announcement plus removal; the historical claim that every removal without HP0 means view exit is unsupported. [Scrubbing](../development/scrubbing.md) deliberately recognizes a narrower literal grammar than some live notice paths. Older fixtures with filled announcement/death text cannot establish the absence of the original notice.

The earlier research used three private captures decoded at `72475aa` on 2026-10-02. Their sample population, portal timing and difficulty values were not universal guarantees. Public replay cases and their exact limitations are documented under [fixtures](../development/fixtures.md). Unknown grave behavior, modifier/grade meaning and exaltation interpretation need independent evidence before becoming UI facts.

## Other players' weapon shots (2026-10-10)

The owner's private capture covered 25 maps and 4,730 other-player sightings; the scrubbed Shaitan fixture covered 16 other players. These are aggregate observations, with no identities recorded here.

Other players' weapon shots were not announced: there were no `ALLYSHOOT` packets in the live capture, with 160,847 `DAMAGE` packets from other players and 105 local. In the owner's saved history (89 runs), other players' damage was 85.2% `other`, 7.8% summon, 6.5% ability, 0.4% item effect and 0.0% weapon; the local player's was 53.7% weapon, 25.1% ability, 20.2% summon and 0.7% other.

For a directly resolved non-local player with no known shot, Maxed now credits damage to the current equipment slot 0 item if its catalog entry has projectiles. The source is an estimated weapon with the slot 0 enchant and reason `inferred-equipped-weapon`; gear swaps affect subsequent hits. Unannounced ability or effect damage may be included in that estimate. Missing, unknown or projectile-less equipment stays Other; local players, summons and known shots retain their existing attribution. Saved fights are not reclassified. Synthetic tests cover these rules; live gameplay acceptance remains pending.

## Unexalted-character stat split: Gate U (2026-10-17)

The [sanitized live acceptance report](../development/character-live-acceptance.md) records **Gate U: pass**
for Wizard L20 (second account): stats 105–112 are explicitly zero at initial materialization and both
re-materializations (Nexus → Vault → Nexus). All 57 later local status updates omit those fields; sparse-update
omission does not negate the observed zeros. The owner accepted this scoped Gate U pass on 2026-10-17 in
[the acceptance comment on PR #176](https://github.com/daplabs/maxed-rotmg/pull/176#issuecomment-6241933324).
Absent initial levels remain unknown.

**Gate P remains open:** the matching catalog, gear bonuses and caps are unavailable, and zero levels with
unchanged equipment cannot establish exalt placement. Client build 7.1.0.2.0 matches the owner-supplied version;
Npcap 1.88 is owner-supplied. The report separates coverage limits, raw packet 114/stat 113 observations,
multi-account risks and the pending Task 11 Windows/coefficient checklist. It adds no stat 113 semantics.
