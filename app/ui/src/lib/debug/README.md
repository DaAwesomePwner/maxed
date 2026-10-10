# Debug mode

Production UI shows titles, values and sprites only. Caveats about data quality
(estimates, partial or sampled coverage, provisional splits, "not observed" reasons)
belong in code comments, `caveat()` logs, and `<DebugNote>`, which renders only when
Debug mode is on (Settings → Debug, stored in `localStorage['maxed.debug']`).

- `<DebugNote text="…" />` / `<DebugNote inline>…</DebugNote>`: visible in debug mode only.
- `caveat(scope, message, detail?)`: `console.debug` in debug mode only.
- `$debugMode` / `isDebugMode()`: branch on it when a caveat changes markup (e.g. a badge).
