# maxed-engine

Pure, deterministic game interpretation with no file, network or notification I/O. See [runtime and engine contracts](../../docs/architecture/runtime.md), [deliberate differences](../../docs/parity/deliberate-differences.md), [kill evidence](../../docs/parity/kill-evidence.md) and [personal-best eligibility](../../docs/architecture/personal-bests.md).

Instance context, kill evidence, sparse player series, stat projections and run totals are implemented public outputs. Unknown inputs and provisional evidence remain explicit. Synthetic regressions are separate from live-game and performance acceptance.
