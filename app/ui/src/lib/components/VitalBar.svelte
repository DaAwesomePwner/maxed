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
  /* Flat game hues, darkened enough to carry light text without an outline. */
  .vbar { --hue: var(--text-muted); position: relative; box-sizing: border-box; width: 100%; min-width: 0; height: 21px; border: 1px solid var(--hairline-strong); border-radius: var(--radius-tag); background: var(--bar-track); isolation: isolate; overflow: hidden; }
  [data-kind="hp"] { --hue: #6cb534; }
  [data-kind="mp"] { --hue: #5677d6; }
  [data-kind="fame"] { --hue: #e8842c; }
  .fill { position: absolute; inset: 0 auto 0 0; z-index: -1; background: color-mix(in srgb, var(--hue) 70%, #000); }
  .label, .value { position: absolute; top: 0; bottom: 0; display: flex; align-items: center; white-space: nowrap; font: var(--weight-semibold) var(--rail-vital-size, var(--text-meta))/1.2 var(--font-ui); color: var(--text-primary); text-shadow: 0 1px 0 rgb(0 0 0 / 50%); }
  .label { left: var(--space-1); font-size: var(--rail-label-size, var(--text-meta)); font-weight: var(--weight-medium); }
  .value { left: 24px; right: 24px; justify-content: center; font-variant-numeric: tabular-nums; }
  [data-kind="fame"] .value { left: 32px; right: 32px; }
  [data-tone="max"] .value { color: var(--stat-maxed); }
  [data-tone="boosted"] .value { color: var(--stat-boosted); }
</style>
