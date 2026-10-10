# Live realm fixes and revised memory limits - 2026-10-06

Final measurement for issue #40, covering ordered departure batches and sprite tile delivery. **The combined build measured 272,547,840 busy / 174,989,312 idle combined private bytes.** On 2026-10-06 the owner raised the 0.1.0 release limits to busy < 300,000,000 and idle < 200,000,000 bytes. The startup limit is unchanged. Against the revised limits, the combined build passes both: 27.5 MB of busy headroom and 25.0 MB of idle headroom. These are single synthetic runs. The original 150 / 200 MB limits still fail.

## Method

The workload is `tests/performance/live-realm-memory.ps1 -Mode after`: 1,536 fights, 24 synthetic players, 320 bags, 24 hits/s, 610 s busy, then Nexus and 180 s idle. Each run used a fresh temporary profile and was uninstrumented, with no CDP. Acceleration was off. All five WebView2 and Maxed processes were counted concurrently. Each launch had no detected competing load and no incomplete samples, and both phases were verified as rendered. Phase windows follow [the peaks report](2026-10-06-live-realm-peaks.md). The release-comparison busy peak covers the logged busy interval, including the departure samples before the idle marker. Idle is +30 to +175 s after leaving.

Builds were `custom-protocol,perf-live` release, non-packaged. The host was Windows 11 Pro 10.0.26200, a Ryzen 9 7900X (24 logical processors), 67.8 GB RAM, WebView2 154.0.4258.53 and Rust 1.99.0.

| Build | Source | Executable SHA-256 |
| --- | --- | --- |
| Departure batches | `746466f` | `16a60b905dbf4a9597b2b200b2cac201420b0a2f28324ad277d1a2b8530ba4c6` |
| Sprite tiles | `0952795` | `b6759aa4aa558ae1d5f258037d06abb7094cf4000f63ac60ecd13ac12c51b7a1` |
| Combined | `46d2569` | `72f1d978539d6472dce3cee345a384353f51d9a130ceaeecb5ae168c5aa4b120` |

## Results

Values are decimal MB of combined private bytes. The control row is the Phase 2a repeat from [the V8 sizing report](2026-10-06-live-realm-v8-sizing.md).

| Run | Steady busy mean (+30–580 s) | Busy gate peak | Departure (+610 s → idle +30 s) peak | Idle gate peak | Renderer busy cores |
| --- | ---: | ---: | ---: | ---: | ---: |
| Control (Phase 2a repeat) | 271.7 | 528.8 | 609.7 | 214.9 | 0.108 |
| Departure batches | 237.1 | 263.7 | 263.7 | 169.9 | 0.063 |
| Sprite tiles | 273.6 | 624.1 | 624.1 | 227.4 | 0.101 |
| **Combined** | **260.6** | **272.5** | **257.2** | **175.0** | **0.073** |

Exact combined gate bytes:

| Phase | Peak bytes | Original limit | Revised limit |
| --- | ---: | --- | --- |
| Busy | 272,547,840 | fail (< 200,000,000) | **pass** (< 300,000,000) |
| Idle | 174,989,312 | fail (< 150,000,000) | **pass** (< 200,000,000) |

The combined idle peak breaks down as renderer 70.2 MB, browser 49.1 MB, Maxed 45.1 MB, storage utility 7.7 MB and crashpad 3.0 MB.

## Interpretation

- Ordered departure batches remove the realm-departure spike: the departure peak fell from about 610 to 257–264 MB and the busy gate peak from about 529 to 264–273 MB. They also lowered steady busy and idle memory.
- Sprite tiles bound native sprite responses and keep whole atlases out of the WebView. In this synthetic workload they did not lower the process-tree peaks on their own. The seed spike of the earlier runs did not reproduce in any of these three runs, so their native seed benefit is not established here.
- Combined steady busy (260.6 MB) is higher than departure-only (237.1 MB). That 23.5 MB difference is larger than the roughly 3 MB between earlier control repeats. It was not investigated, and with one run each its cause is not attributed.
- The investigation targets (130 / 140 MB) are unchanged and not met.
- `--js-flags=--optimize-for-size` was not adopted.

## Limits

Each configuration was measured once, so run-to-run variance is unquantified; earlier control repeats differed by about 3 MB steady and 15 MB at departure. None of this establishes live gameplay, installed-package, startup-memory or long-session behaviour. A 30+ minute owner realm session remains the acceptance check for #40. Raw process records stay in temporary storage.
