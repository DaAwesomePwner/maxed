<script lang="ts">
  let { kind, label, text, fill, tone }: {
    kind: 'hp' | 'mp' | 'fame'; label: string; text: string; fill: number;
    tone: 'max' | 'boosted' | 'plain';
  } = $props();
  let percent = $derived(Number.isFinite(fill) ? Math.max(0, Math.min(100, fill)) : 0);
</script>

<div class="vbar" data-kind={kind} data-tone={tone} role="meter" aria-label={label}
  aria-valuemin="0" aria-valuemax="100" aria-valuenow={percent} aria-valuetext={text}>
  <i class="fill" style:width={`${percent}%`} aria-hidden="true"></i>
  <span class="label" aria-hidden="true">{label}</span>
  <b class="value" aria-hidden="true">{text}</b>
</div>

<style>
  .vbar { position: relative; box-sizing: border-box; width: 100%; min-width: 0; height: 21px; border: 2px solid #111; border-radius: 2px; background: #4a4650; isolation: isolate; }
  .fill { position: absolute; inset: 0 auto 0 0; z-index: -1; }
  [data-kind="hp"] .fill { background: linear-gradient(#8fd451, #6cb534); }
  [data-kind="mp"] .fill { background: linear-gradient(#7193ea, #5677d6); }
  [data-kind="fame"] { background: #4a2608; }
  [data-kind="fame"] .fill { background: linear-gradient(#f4a24c, #e8842c 60%, #c9681a); }
  .label, .value { position: absolute; top: 0; bottom: 0; display: flex; align-items: center; white-space: nowrap; font: 700 var(--rail-vital-size, 11px)/1.2 var(--font-ui); color: #fff; text-shadow: 1px 0 #000, -1px 0 #000, 0 1px #000, 0 -1px #000; }
  .label { left: 3px; font-size: var(--rail-label-size, 10px); }
  .value { left: 22px; right: 22px; justify-content: center; font-variant-numeric: tabular-nums; letter-spacing: -.03em; }
  [data-kind="fame"] .value { left: 30px; right: 30px; }
  [data-tone="max"] .value { color: #ffe04a; }
  [data-tone="boosted"] .value { color: #5cff5c; }
</style>
