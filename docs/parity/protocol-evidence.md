# Protocol evidence and limits

Packet definitions and typed decoders in `crates/maxed-protocol/src` are authoritative for implemented layouts. Historical capture research supplies hypotheses and regression cases, not permission to consume unknown tails or infer unsupported semantics.

| Observation | Supported interpretation | Limit |
| --- | --- | --- |
| QUESTOBJID (82) | Signed big-endian object ID, compressed count and list; count bounded to 0–64; -1 clears the target | Quest switches alone do not prove a kill; generation resolution is local |
| REALM_SCORE_UPDATE (169) | One signed big-endian absolute score | MAPINFO supplies the maximum; sentinels stay unavailable; score does not prove a realm has closed |
| MAPINFO | Area, realm name, seed, open time, capacity and optional difficulty/modifiers | Parent realm names may appear in dungeons; a name alone is not an instance identity; unknown grades/modifiers stay unknown |
| Player UPDATE objects and drops | Observed instance population | A derived count, not a verified server population counter; NEWTICK alone cannot admit a player |
| PlayerDeath / PortalOpened notices | Exact recognized template data | Chat (TEXT) is stored for Logs; notice templates still do not retain their raw text or export opener/victim identities. Missing identity prevents a reliable join |
| Server HP and removal | Target observations keyed by generation | Positive HP/HP1, quest changes, bags and portals alone do not confirm death |
| Oryx death/killed templates | Recognized instance announcement, optionally corroborated by removal | A distant announcement creates no fabricated fight or damage |
| Stat 57 | Observed current fame | Stat 39 and death total fame are not current-fame samples; stat 80 is not a supported fame goal |
| Stat 115 / name strings | Sensitive identity data | Never expose diagnostic values or join grave/player records from names alone |

`NOTIFICATION` effect 24/extra 32 has an unexplained two-byte tail in historical evidence. Some CREATE_SUCCESS PcStats blobs leave 25 bytes after a 119-of-144-byte interpretation. Keep strict rejection/incomplete counters; do not invent field layouts or completion baselines. Startup field failures before verified tick alignment may be mis-decryptions rather than protocol changes.

Bag birth and proximity do not prove a dropper or kill. The existing reference attribution runs first; the unique killed-target fallback within 1.5 tiles in the same/next tick is explicitly estimated. Unknown contents and later enrichment remain distinct from ownership or who took an item.

The current detector uses [kill evidence](kill-evidence.md), including announcement plus removal; the historical claim that every removal without HP0 means view exit is unsupported. [Scrubbing](../development/scrubbing.md) deliberately recognizes a narrower literal grammar than some live notice paths. Older fixtures with filled announcement/death text cannot establish the absence of the original notice.

The earlier research used three private captures decoded at `72475aa` on 2026-10-02. Their sample population, portal timing and difficulty values were not universal guarantees. Public replay cases and their exact limitations are documented under [fixtures](../development/fixtures.md). Unknown grave behavior, modifier/grade meaning and exaltation interpretation need independent evidence before becoming UI facts.
