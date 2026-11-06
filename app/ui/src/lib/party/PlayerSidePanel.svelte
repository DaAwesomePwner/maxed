<script lang="ts">
  import { tick, untrack } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { PartyMember } from '../bindings/PartyMember';
  import type { PartyMark } from '../bindings/PartyMark';
  import type { PartyPlayerView } from '../bindings/PartyPlayerView';
  import type { SpriteRef } from '../bindings/SpriteRef';
  import { createHomeVisuals, emptyHomeVisuals, type HomeVisuals } from '../home/visuals';
  import { spriteUrl } from '../bridge/visuals';
  import { objectVisual } from '../bridge/visual-index';
  import { routeHref } from '../navigation/routes';
  import { formatShare } from '../styles/format';
  import GearCard from '../players/GearCard.svelte';
  import { timestampMs } from '../players/player-view';
  import { partyDate, whisperText } from './party-screen';
  let { member: suppliedMember, runId, client, visuals, resolveSpriteUrl, onChanged, onClose, fullRuns = false, refreshToken = 0 }: {
    member: PartyMember; runId: string | null; client: Client; visuals: HomeVisuals;
    resolveSpriteUrl: (sprite: SpriteRef) => string | null; onChanged: (change: { key: string; mark: PartyMark | null; note: string | null }) => void; onClose: () => void;
    fullRuns?: boolean; refreshToken?: number;
  } = $props();
  let detail = $state.raw<PartyPlayerView | null>(null);
  let member = $derived<PartyMember>(fullRuns && detail ? { ...suppliedMember, name: detail.row.name,
    class_id: detail.last_profile?.class_id ?? detail.row.last_class_id, level: detail.last_profile?.level,
    stars: detail.last_profile?.stars, guild: detail.guild, maxed: detail.row.last_maxed, mark: detail.row.mark,
    runs_together: detail.row.runs_together } : suppliedMember);
  let mark = $state<PartyMark | null>(null), note = $state(''), savedNote = '';
  let loaded = $state(false), error = $state(''), copied = $state('');
  let failedAction = $state<'read' | 'save' | 'realmeye'>('read');
  let writes = Promise.resolve();
  let playerKey = $derived(suppliedMember.name_key);
  let paging = $state(false), historyLoading = $state(false), runsError = $state('');
  let generation = 0;
  let gearVisuals = $state.raw<HomeVisuals>(emptyHomeVisuals());
  let gearController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let retry = $state(0);
  let validName = $derived(/^[A-Za-z]{1,10}$/.test(member.name ?? ''));
  $effect(() => {
    const owner = client, key = playerKey, request = ++generation; retry; refreshToken;
    const preserveDraft = untrack(() => loaded && detail?.row.key === key);
    if (!preserveDraft) { mark = untrack(() => member.mark ?? null); note = ''; savedNote = ''; detail = null; loaded = false; }
    error = ''; failedAction = 'read'; runsError = ''; paging = false; historyLoading = true;
    let current = true;
    if (key) void owner.getPartyPlayer(key, null).then(reply => {
      if (!current) return;
      if (reply.status !== 'ready') { error = 'Player history is busy. Retry to load marks and notes.'; return; }
      const fresh = reply.value;
      if (preserveDraft && detail && fresh) {
        const refreshed = new Set(fresh.runs.map(run => run.run_id));
        const remaining = detail.runs.filter(run => !refreshed.has(run.run_id));
        detail = { ...fresh, runs: [...fresh.runs, ...remaining], next: remaining.length ? detail.next : fresh.next };
      } else detail = fresh;
      if (!preserveDraft) { mark = detail?.row.mark ?? null; note = detail?.row.note ?? ''; savedNote = note; }
      loaded = true;
    }).catch(() => { if (current) error = 'Could not load player history.'; })
      .finally(() => { if (current) historyLoading = false; });
    return () => { current = false; if (generation === request) generation++; };
  });
  $effect(() => {
    const controller = createHomeVisuals(client, next => { gearVisuals = next; });
    gearController = controller;
    return () => controller.dispose();
  });
  $effect(() => { gearController?.update([detail?.last_profile?.class_id ?? detail?.row.last_class_id,
    ...(detail?.last_profile?.equipment ?? []).map(item => item.item_type)].filter((type): type is number => type != null)); });
  async function loadMore() {
    if (paging || historyLoading || !detail?.next || !playerKey) return;
    const request = generation, owner = client, key = playerKey, cursor = detail.next;
    paging = true; runsError = '';
    try {
      const reply = await owner.getPartyPlayer(key, cursor);
      if (request !== generation) return;
      if (reply.status !== 'ready') { runsError = 'Player history is busy. Retry to load more runs.'; return; }
      if (!reply.value || !detail) { runsError = 'Player history changed. Reopen this player to refresh it.'; return; }
      detail = { ...detail, runs: [...new Map([...detail.runs, ...reply.value.runs].map(run => [run.run_id, run])).values()], next: reply.value.next };
    } catch { if (request === generation) runsError = 'Could not load more runs. Retry to load them.'; }
    finally { if (request === generation) paging = false; }
  }
  function save(nextMark: PartyMark | null = mark) {
    if (!loaded || member.is_local || !member.name_key || !member.name) return;
    const owner = client, key = member.name_key, name = member.name, text = note || null;
    // Blur can precede a toggle click. Queue both writes so neither the note nor the mark is lost.
    mark = nextMark;
    writes = writes.then(async () => {
      error = '';
      try { await owner.setPartyMark(key, name, nextMark, text); savedNote = text ?? ''; onChanged({ key, mark: nextMark, note: text }); }
      catch { failedAction = 'save'; error = 'Could not save marks and note.'; }
    });
    return writes;
  }
  async function copy(text: string) {
    if (!navigator.clipboard?.writeText) return;
    try { await navigator.clipboard.writeText(text); copied = ''; await tick(); copied = 'Copied'; } catch { /* Clipboard may be denied by the host. */ }
  }
  async function openRealmeye() {
    if (!validName || !member.name) return;
    error = '';
    try { await client.openRealmeye(member.name); } catch { failedAction = 'realmeye'; error = 'Could not open RealmEye.'; }
  }
</script>
<aside aria-label="Player details">
  <section class="panel">
    <header><h3 tabindex="-1">{member.name ?? (fullRuns ? 'Player' : 'No name yet')}</h3><button class="ctl ghost" aria-label="Close player details" onclick={onClose}>Close</button></header>
    {#if fullRuns && loaded && !detail}<p class="muted">Player history not found.</p>{/if}
    <p class="muted">{objectVisual(visuals.objects, member.class_id)?.name ?? objectVisual(gearVisuals.objects, member.class_id)?.name ?? 'Unknown class'} · Level {member.level ?? '—'} · {member.stars ?? '—'} stars</p>
    <dl>
      <dt>Guild</dt><dd>{member.guild ?? detail?.guild ?? '—'}</dd>
      <dt>First seen</dt><dd>{partyDate(detail?.first_seen)}</dd>
      <dt>Runs together</dt><dd>{detail?.row.runs_together ?? member.runs_together}</dd>
      <dt>Died in</dt><dd>{detail ? `${detail.row.deaths} of ${detail.row.runs_together}` : '—'}</dd>
      <dt>Left early in</dt><dd>{detail ? `${detail.row.left_early} of ${detail.row.runs_together}` : '—'}</dd>
      <dt>Avg damage share</dt><dd>{formatShare(detail?.row.avg_share ?? null)}</dd>
      <dt>Last gear</dt><dd>{partyDate(detail?.last_profile?.last_seen)} · {detail?.last_profile?.maxed == null ? '—' : `${detail.last_profile.maxed}/8`}</dd>
    </dl>
    {#if member.name && member.name_key}
      <div class="actions">
        {#if !member.is_local}
          <button class="ctl" aria-pressed={mark === 'watched'} disabled={!loaded} onclick={() => save(mark === 'watched' ? null : 'watched')}>Watch</button>
          <button class="ctl" aria-pressed={mark === 'banned'} disabled={!loaded} onclick={() => save(mark === 'banned' ? null : 'banned')}>Ban</button>
        {/if}
        <button class="ctl" onclick={() => copy(member.name!)}>Copy name</button>
        <button class="ctl" onclick={() => copy(whisperText(member.name!))}>Copy /t</button>
        <button class="ctl" disabled={!validName} title={validName ? 'Open this player in your browser' : 'RealmEye names must contain 1 to 10 letters.'} onclick={openRealmeye}>Open on RealmEye</button>
        {#if runId}<a class="ctl" href={routeHref({ kind: 'run-player', runId, playerId: member.player_id })}>Run view</a>{/if}
      </div>
      {#if !member.is_local}<label class="note">Private note<textarea maxlength="500" disabled={!loaded} bind:value={note} onblur={() => { if (note !== savedNote) void save(); }}></textarea></label>{/if}
    {/if}
    <p role="status">{copied}</p>
    {#if error}<p role="alert">{error}</p><button class="ctl" onclick={() => { if (failedAction === 'save') void save(); else if (failedAction === 'realmeye') void openRealmeye(); else retry++; }}>Retry</button>{/if}
  </section>
  {#if detail?.last_profile}
    <GearCard equipment={detail.last_profile.equipment} observedAt={timestampMs(detail.last_profile.last_seen)} swaps={[]} visuals={gearVisuals}
      resolveSpriteUrl={sprite => spriteUrl(gearVisuals.client_fingerprint, sprite) ?? resolveSpriteUrl(sprite)} />
  {/if}
  <section class="panel" aria-label={fullRuns ? 'Runs together' : 'Recent runs together'}><h3>{fullRuns ? 'Runs together' : 'Recent runs together'}</h3>
    {#if !loaded}<p class="muted">Loading player history</p>
    {:else if !detail?.runs.length}<p class="muted">No saved runs together.</p>
    {:else}<ul>{#each fullRuns ? detail.runs : detail.runs.slice(0, 5) as run (run.run_id)}<li>
      <a href={routeHref({ kind: 'party-run', runId: run.run_id })}>{run.dungeon}</a>
      <small>{partyDate(run.started)} · {run.died ? 'Died' : run.left_early ? 'Left early' : run.share != null ? formatShare(run.share) : run.outcome ?? 'Unknown'}</small>
    </li>{/each}</ul>{/if}
    {#if fullRuns && detail?.next}<button class="ctl" disabled={paging || historyLoading} onclick={loadMore}>{paging ? 'Loading runs…' : 'Load more'}</button>{/if}
    {#if runsError}<p role="alert">{runsError}</p>{/if}
  </section>
</aside>
<style>
  aside { display: grid; gap: var(--space-3); min-width: 0; } .panel { background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-card); padding: var(--space-3); }
  header { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); } h3 { margin: 0; font-size: 13px; }
  dl { display: grid; grid-template-columns: 1fr auto; gap: 4px 8px; font-size: 12px; } dt, .muted, small { color: var(--text-secondary); } dd { margin: 0; text-align: right; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; } .actions a { text-decoration: none; } button[aria-pressed=true] { border-color: var(--gold); color: var(--gold); }
  .note { display: grid; gap: 4px; margin-top: var(--space-3); font-size: 12px; } textarea { width: 100%; min-height: 72px; box-sizing: border-box; resize: vertical; background: var(--surface-raised); color: var(--text-primary); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 6px; font: inherit; }
  ul { list-style: none; padding: 0; margin-bottom: 0; } li { border-top: 1px solid var(--border); padding: 6px 0; } li small { display: block; } a { color: var(--violet-text); }
  [role=status]:empty { display: none; } [role=alert] { color: var(--danger); }
</style>
