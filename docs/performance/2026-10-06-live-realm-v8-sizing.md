# Live realm V8 sizing without CDP - 2026-10-06

The uninstrumented `--optimize-for-size` run saves **41.302 MB (15.2%) of steady combined private memory** against the fresh control repeat, and 37.872 MB against Phase 2a. Its steady mean is **230.419 MB**, late mean **236.596 MB** and departure peak **347.103 MB**. These are measurements of one successful flag run, not a production-policy change or a gate pass.

The [JSON evidence](2026-10-06-live-realm-v8-sizing.json) retains exact bytes, per-process means/maxima, concurrent peak decompositions, CPU counters, thirty-second bins, boundary samples, flag verification and completeness. Raw process identities, command lines, logs, profiles and discovered game assets remain in temporary storage.

This extends the [Phase 2a allocation report](2026-10-06-live-realm-ui-churn.md) and [Phase 2b peak investigation](2026-10-06-live-realm-peaks.md). The original control was reanalyzed from its retained process counters; the three accepted new runs reuse its exact executable.

## Build, workload and controls

- Investigation: `3bf3d059851b035cd8d7e488d5d369bb7cc60993`, branch `perf/40-attribution`, initially clean. Only documentation and measurement tooling changed here; no product Rust/UI changes.
- Reused `target/release/maxed.exe`, byte-verified before and after measurement: SHA-256 `647bc06c45ca67b85d094de044e3e5e17c065b7c36c5fdeaaa9620bb5bfada9f`. Built from `48f4974fe954c68595c398bf78d040313bee46ec` with the built frontend and `custom-protocol,perf-live`; non-packaged release. No rebuild.
- Microsoft Windows 11 Pro `10.0.26200`, AMD Ryzen 9 7900X 12-Core Processor, 12 cores / 24 logical processors, 67,794,673,664 bytes usable RAM; WebView2 `154.0.4258.53`, PowerShell `7.6.6`, Node `v26.7.0`. Same host/runtime/executable as Phase 2a.
- Normal `live-realm-memory.ps1 -Mode after`: 1,536 fights, 24 synthetic players, 320 bags, 24 hits/s, 610 scheduled busy seconds and 180 idle seconds. No `-Diagnostic`, CDP, trace, forced GC, pressure or workload changes.
- Each attempt used an empty directory at the executable's already compiled temporary profile path. Previous profiles were archived only after owned-process shutdown. Normal local asset discovery remained enabled. No normal user profile, account service, live traffic or game capture was used.
- Accepted runs were sequential: control, optimize-for-size, then the narrower semi-space flag after the large effect was established. No builds or tests overlapped a workload. The launch quiet check and ongoing compiler/test/active-Node checks were clean.

The driver normally clears browser overrides. Its new opt-in `-AllowV8SizingOverride` accepts only either exact sizing argument, only with uninstrumented `-Mode after`, and rejects extra switches, old-space limits and diagnostics. The default workload is unchanged. The override is process-local and cleared after the run. Both owned browser and renderer command lines attest the requested sizing flag. No new feature-list switch was supplied; the original disabled features and `NetworkServiceInProcess2` remain present, the browser retains `--disable-gpu --in-process-gpu`, and no separate GPU/network or tracing utility was observed. All five processes are counted.

## Memory comparison

All MB are decimal. Each table cell below is **sample mean / sampled maximum**. Per-process maxima are independent and must not be added; the combined maximum is computed from concurrent sums. Means are arithmetic over samples, as in the prior peak report, not duration-weighted integrals. The observed seed excludes time before collector attachment and is not a startup-gate measurement.

| Interval, combined MB | Phase 2a control | Control repeat | Optimize for size | Semi-space 1 MiB |
| --- | ---: | ---: | ---: | ---: |
| Observed startup/seed | 208.086 / 549.188 | 207.226 / 379.580 | 190.736 / 475.992 | 216.481 / 587.350 |
| Early busy +0–30 s | 293.661 / 409.158 | 332.796 / 405.963 | 238.922 / 327.344 | 303.126 / 399.102 |
| Steady busy +30–580 s | 268.291 / 292.663 | 271.722 / 295.924 | 230.419 / 241.676 | 260.264 / 271.225 |
| Late busy +580–610 s | 285.725 / 294.253 | 288.311 / 296.399 | 236.596 / 243.057 | 266.914 / 268.878 |
| Departure +610 → idle +30 s | 323.670 / 595.505 | 325.028 / 609.653 | 230.473 / 347.103 | 316.938 / 607.748 |
| Idle +30–175 s | 211.111 / 215.126 | 211.736 / 214.934 | 202.161 / 206.856 | 213.135 / 215.450 |

Per-process detail, MB; the JSON also records each component at the concurrent combined peak.

| Interval / process | Control repeat | Optimize for size | Semi-space 1 MiB |
| --- | ---: | ---: | ---: |
| Observed startup/seed / Renderer | 91.536 / 210.010 | 71.692 / 188.977 | 96.674 / 234.922 |
| Observed startup/seed / Browser | 53.655 / 65.991 | 55.503 / 80.634 | 53.391 / 65.790 |
| Observed startup/seed / Maxed | 51.636 / 281.965 | 53.161 / 344.457 | 56.024 / 426.217 |
| Observed startup/seed / Storage utility | 7.786 / 7.786 | 7.770 / 7.770 | 7.782 / 7.782 |
| Observed startup/seed / Crashpad | 2.613 / 2.613 | 2.609 / 2.609 | 2.609 / 2.609 |
| Observed startup/seed / Combined | 207.226 / 379.580 | 190.736 / 475.992 | 216.481 / 587.350 |
| Early busy +0–30 s / Renderer | 198.833 / 272.622 | 104.191 / 162.324 | 171.804 / 268.452 |
| Early busy +0–30 s / Browser | 59.483 / 95.007 | 59.719 / 89.354 | 59.097 / 94.204 |
| Early busy +0–30 s / Maxed | 63.389 / 66.171 | 63.948 / 65.286 | 61.145 / 63.492 |
| Early busy +0–30 s / Storage utility | 7.786 / 7.786 | 7.770 / 7.770 | 7.782 / 7.782 |
| Early busy +0–30 s / Crashpad | 3.305 / 3.539 | 3.294 / 3.523 | 3.298 / 3.527 |
| Early busy +0–30 s / Combined | 332.796 / 405.963 | 238.922 / 327.344 | 303.126 / 399.102 |
| Steady busy +30–580 s / Renderer | 135.389 / 152.429 | 93.828 / 106.217 | 124.408 / 135.164 |
| Steady busy +30–580 s / Browser | 54.472 / 59.769 | 54.882 / 59.748 | 54.340 / 59.306 |
| Steady busy +30–580 s / Maxed | 71.032 / 77.562 | 70.929 / 77.816 | 70.738 / 78.705 |
| Steady busy +30–580 s / Storage utility | 7.730 / 7.909 | 7.696 / 7.782 | 7.710 / 7.803 |
| Steady busy +30–580 s / Crashpad | 3.098 / 3.535 | 3.085 / 3.523 | 3.068 / 3.514 |
| Steady busy +30–580 s / Combined | 271.722 / 295.924 | 230.419 / 241.676 | 260.264 / 271.225 |
| Late busy +580–610 s / Renderer | 145.107 / 153.649 | 93.014 / 99.631 | 123.280 / 125.407 |
| Late busy +580–610 s / Browser | 54.248 / 54.653 | 54.660 / 55.181 | 53.909 / 54.276 |
| Late busy +580–610 s / Maxed | 77.925 / 78.393 | 78.031 / 78.987 | 78.825 / 79.032 |
| Late busy +580–610 s / Storage utility | 7.909 / 7.909 | 7.782 / 7.782 | 7.803 / 7.803 |
| Late busy +580–610 s / Crashpad | 3.121 / 3.121 | 3.109 / 3.109 | 3.097 / 3.097 |
| Late busy +580–610 s / Combined | 288.311 / 296.399 | 236.596 / 243.057 | 266.914 / 268.878 |
| Departure +610 → idle +30 s / Renderer | 206.186 / 482.091 | 113.142 / 214.774 | 196.847 / 476.598 |
| Departure +610 → idle +30 s / Browser | 60.354 / 115.569 | 59.382 / 94.183 | 60.564 / 98.222 |
| Departure +610 → idle +30 s / Maxed | 47.459 / 59.625 | 47.059 / 60.113 | 48.630 / 78.975 |
| Departure +610 → idle +30 s / Storage utility | 7.909 / 7.909 | 7.782 / 7.782 | 7.803 / 7.803 |
| Departure +610 → idle +30 s / Crashpad | 3.120 / 3.121 | 3.107 / 3.109 | 3.095 / 3.097 |
| Departure +610 → idle +30 s / Combined | 325.028 / 609.653 | 230.473 / 347.103 | 316.938 / 607.748 |
| Idle +30–175 s / Renderer | 99.410 / 102.740 | 89.238 / 92.553 | 100.222 / 102.556 |
| Idle +30–175 s / Browser | 55.089 / 55.349 | 55.798 / 57.197 | 55.564 / 57.491 |
| Idle +30–175 s / Maxed | 46.379 / 46.535 | 46.409 / 46.469 | 46.631 / 46.633 |
| Idle +30–175 s / Storage utility | 7.829 / 7.909 | 7.695 / 7.782 | 7.711 / 7.803 |
| Idle +30–175 s / Crashpad | 3.029 / 3.121 | 3.021 / 3.109 | 3.007 / 3.092 |
| Idle +30–175 s / Combined | 211.736 / 214.934 | 202.161 / 206.856 | 213.135 / 215.450 |

Exact release-comparison peaks retain the existing busy/logged-idle boundaries. Departure can peak just after the idle marker; the separate departure window above prevents that cost being hidden. Every value below is compared using exact bytes, not rounded MB.

| Run | Labelled busy peak, bytes | Result (<200,000,000) | Idle +30–175 peak, bytes | Result (<150,000,000) |
| --- | ---: | --- | ---: | --- |
| Phase 2a control | 552,910,848 | FAIL | 215,126,016 | FAIL |
| Control repeat | 528,760,832 | FAIL | 214,933,504 | FAIL |
| Optimize for size | 347,103,232 | FAIL | 206,856,192 | FAIL |
| Semi-space 1 MiB | 607,748,096 | FAIL | 215,449,600 | FAIL |

## Control repeatability

Two controls describe an observed range, not a confidence interval. Their steady, late and idle memory levels are close, while CPU and transition-peak sampling vary more. Observed seed peaks vary substantially: 549.188 MB originally versus 379.580 MB in the repeat. The new flag runs reach 475.992 and 587.350 MB during seed, with large native Maxed contributions. These results do not establish a seed-memory benefit from either flag; startup/asset transients remain a separate problem.

| Metric | Phase 2a | Control repeat | Change |
| --- | ---: | ---: | ---: |
| Steady combined mean, MB | 268.2910 | 271.7219 | +3.431 (+1.279%) |
| Late combined mean, MB | 285.7253 | 288.3106 | +2.585 (+0.905%) |
| Departure combined maximum, MB | 595.5052 | 609.6527 | +14.148 (+2.376%) |
| Labelled busy peak, MB | 552.9108 | 528.7608 | -24.150 (-4.368%) |
| Idle gate peak, MB | 215.1260 | 214.9335 | -0.193 (-0.089%) |
| Steady renderer CPU, cores | 0.0857 | 0.1033 | +0.018 (+20.544%) |
| Overall renderer CPU, cores | 0.0734 | 0.0853 | +0.012 (+16.190%) |
| Scheduled departure → idle log, s | 2.7393 | 2.4009 | -0.338 (-12.354%) |

## CPU and departure latency

CPU cores = cumulative CPU-seconds divided by sampled wall-seconds; machine percentage = cores / 24 × 100. Overall spans the observed approximately 840-second collector interval, including seed and idle; it is not lifetime CPU before attachment. Busy uses the logged busy-to-idle interval. Steady uses +30–580 seconds. CPU differences use the collector's monotonic elapsed seconds. All five identities remained stable, and the overall combined CPU calculation agrees with the maintained collector.

The idle gate ends before the driver's +180-second completion. Overall CPU also includes the short period after that synthetic driver teardown, so it is not a continuing-capture CPU average.

| Run / window | Renderer cores | Renderer machine % | Combined cores | Combined machine % | Sampled seconds |
| --- | ---: | ---: | ---: | ---: | ---: |
| Phase 2a control / overall | 0.073395 | 0.3058 | 0.165972 | 0.6916 | 839.843 |
| Phase 2a control / busy | 0.091782 | 0.3824 | 0.202856 | 0.8452 | 610.654 |
| Phase 2a control / steady | 0.085686 | 0.3570 | 0.186702 | 0.7779 | 548.333 |
| Control repeat / overall | 0.085278 | 0.3553 | 0.167133 | 0.6964 | 839.900 |
| Control repeat / busy | 0.108378 | 0.4516 | 0.205984 | 0.8583 | 610.711 |
| Control repeat / steady | 0.103290 | 0.4304 | 0.191185 | 0.7966 | 549.124 |
| Optimize for size / overall | 0.091198 | 0.3800 | 0.171420 | 0.7142 | 839.861 |
| Optimize for size / busy | 0.116409 | 0.4850 | 0.212535 | 0.8856 | 610.856 |
| Optimize for size / steady | 0.110803 | 0.4617 | 0.196310 | 0.8180 | 549.116 |
| Semi-space 1 MiB / overall | 0.060333 | 0.2514 | 0.141839 | 0.5910 | 839.864 |
| Semi-space 1 MiB / busy | 0.075531 | 0.3147 | 0.173700 | 0.7238 | 611.506 |
| Semi-space 1 MiB / steady | 0.068249 | 0.2844 | 0.155423 | 0.6476 | 549.004 |

| Run | Scheduled departure → `PERF_LIVE idle`, seconds |
| --- | ---: |
| Phase 2a control | 2.7393268 |
| Control repeat | 2.4009145 |
| Optimize for size | 2.6626495 |
| Semi-space 1 MiB | 2.6368237 |

Departure timing is the logged busy-to-idle duration minus the source-derived 610-second schedule. It includes mass fight finalization/publisher flushing and scheduling effects, and is not an input-latency or frame-time measure. Renderer transition work can continue after the native idle marker. The existing one-time Windows UI Automation checks found Realm and Nexus in every accepted run, without the persistent subscription-failure label. Repeated automation was avoided to preserve comparability. No click-to-frame latency, GC pause distribution or p95/p99 responsiveness is established.

## What the switches change

`--js-flags=--optimize-for-size` asks V8 to favor memory size over execution speed. Current V8 source makes it imply `max_semi_space_size=1`. `--js-flags=--max-semi-space-size=1` sets that young-generation semi-space ceiling directly, without opting into the broader size preference. It is a narrower intervention, not a guaranteed smaller CPU tradeoff. It caps a young space; neither experiment supplies an old-space or total-heap ceiling such as `--max-old-space-size` or `--max-heap-size`. [V8 flag definitions](https://raw.githubusercontent.com/v8/v8/main/src/flags/flag-definitions.h), [V8 heap configuration](https://raw.githubusercontent.com/v8/v8/main/src/heap/heap.cc).

Smaller allocation capacity can exchange memory slack for more collection work and latency. These upstream sources explain the experiment; they are not a source-level attestation of the exact V8 revision bundled in this Edge runtime. No heap-capacity or allocator-pool claim is inferred from these uninstrumented private-byte counters. [V8 memory tradeoffs](https://v8.dev/blog/optimizing-v8-memory).

## Verdict and policy boundary

The full-window result establishes an uninstrumented saving of **41.302 MB steady combined** versus the repeat (37.872 MB versus Phase 2a). Renderer steady CPU changes by **+7.274%**, from 0.103290 to 0.110803 cores (0.4304% to 0.4617% of this machine). Compare that with the **20.5% control-to-control CPU increase**. One run per flag cannot establish a precise CPU cost or tail-latency safety.

Steady renderer mean falls from **135.389 to 93.828 MB** (41.561 MB saved). The roughly 100 MB renderer saving seen under CDP does not transfer intact to an ordinary run. Departure falls by **262.550 MB**, while idle mean improves by only **9.575 MB**. No malloc-pool size is measured here.

The narrower semi-space run gives **260.264 MB steady**, **266.914 MB late**, **607.748 MB departure peak** and **215.450 MB idle peak**. Its steady saving is only **11.458 MB (4.2%)** versus the repeat, and 8.027 MB versus the original control; the large departure reduction does not reproduce. Its steady renderer CPU is **0.068249 cores / 0.2844% of the machine**, -33.925% versus the repeat. This is another one-run observation, not proof that its narrower flag scope guarantees a better CPU/latency tradeoff or that the entire optimize-for-size benefit comes from semi-space sizing alone.

The optimize-for-size departure delay is **2.6626495 s**, versus controls **2.7393268 / 2.4009145 s**. It is within their observed range. Its steady CPU is +29.313% above the original control as well as +7.274% above the repeat: the data establish the measured averages, not a stable 7% tax or latency neutrality.

**The flag alone does not reach <200 MB steady busy.** Optimize-for-size still needs more than **30.419 MB** off the steady mean, **41.676 MB** off its steady maximum, and **43.057 MB** off its late maximum. Idle still needs more than **56.856 MB** off its gate peak. The departure maximum also remains 147.103 MB above the busy limit.

A combined path remains plausible as an experiment, but no tested combination meets the gates. The earlier 10–15 MB native-retention opportunity alone is smaller than the remaining busy gap even if it could be realized. Additional active-state/projection reductions and native retention work must be measured; departure delivery and whole-atlas copying target different transients. The existing acceleration-off policy and Phase 2a allocation fixes are already in these numbers and cannot be credited again. Diagnostic GC savings are not additive production savings. A policy exception would buy a substantial measured reduction, not compliance or established long-session stability.

The release policy still rejects JavaScript heap-limit flags; this report does not silently approve a sizing-flag exception. This is an experimental WebView2 configuration even though V8 defines the switch as a regular Boolean, not its `experimental` feature flag. Microsoft explicitly advises against browser flags in production because Chromium/Edge can alter or remove them. A decision to ship would require an explicit policy decision plus representative history, transitions, long-session, lower-end CPU and responsiveness testing. [WebView2 browser flag guidance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/webview-features-flags).

Browser arguments configure the shared browser environment/process group, not just the Realm view; other WebViews using that environment can share renderers and inherit the tradeoff. This experiment isolates the environment with its temporary user-data folder. A conflicting environment configuration can also fail when reusing a folder. [WebView2 process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model).

`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` interacts with application-supplied arguments and duplicate switches. Follow the repository's explicit feature-list repetition rule: any replacement disabled-feature list must retain `msWebOOUI,msPdfOOUI,msSmartScreenProtection`, and any enabled-feature list must retain `NetworkServiceInProcess2`. These experiments supplied no feature-list override at all. Do not persist the test override globally or assume it only affects one view. Restart the shared environment when changing arguments and re-attest effective command lines. Runtime updates require repeating memory, CPU, latency and crash/recovery checks; this result applies only to WebView2 154.0.4258.53 on this host. [Argument semantics](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2environmentoptions.additionalbrowserarguments).

## Validation and reproduction

Collector self-test and interactive-startup self-test (83 assertions) passed before measurement. The memory-tooling suite passed under PowerShell 7 and Windows PowerShell 5.1 between runs, including the sizing-override rejection cases and both report-tool self-tests. All accepted runs have zero incomplete memory/CPU samples, verified phase rendering, no detected competing load and complete idle-gate coverage. The first optimize-for-size attempt is excluded: one inventory-identity failure at collector line 124, zero samples, 0.415 seconds observed; no product crash or flag regression is established. It was stopped and retried from a fresh profile without relaxing the collector.

Run each workload separately; do not overlap reports/tests/builds with a workload. `$exe` is the byte-verified executable, `$profile` its compiled temporary path, and every `$out` is a newly empty temporary directory. Archive only the owned temporary profile after its PID-plus-birth processes have exited, then recreate it empty.

```powershell
# Control
Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
pwsh -NoProfile -File tests/performance/live-realm-memory.ps1 -Exe $exe -Profile $profile -OutputDir $control -Mode after

# Each sizing run, separately, after shutdown and profile reset
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--js-flags=--optimize-for-size'
# For the narrower run use: --js-flags=--max-semi-space-size=1
try {
    pwsh -NoProfile -File tests/performance/live-realm-memory.ps1 -Exe $exe -Profile $profile -OutputDir $out -Mode after -AllowV8SizingOverride
} finally {
    Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
}

# After each completed run
pwsh -NoProfile -File tests/performance/live-realm-report.ps1 -EvidenceDir $out
node tests/performance/live-realm-peaks-report.mjs $out
```

The report tools supplied the retained gate peaks and phase statistics. Departure combines busy +610 through logged idle +30; CPU comes from the same retained process-counter rows. Concurrent sums, CPU arithmetic, exact-byte gate results, JSON structure, privacy filtering and `git diff --check` were validated. The executable hash remained unchanged and bundled synthetic resource hashes matched their repository fixtures. Each accepted profile produced nine local asset-cache files totaling 103,159,076 bytes. Overrides were cleared and no launched Maxed/WebView2 PID-plus-birth identity remained; the compiled temporary profile was archived and recreated empty. No product tests were rerun because no product code changed; no live Npcap, game-client, installed-package, installer, startup-memory or production stability acceptance is claimed. Local commit only; no push or pull request.
