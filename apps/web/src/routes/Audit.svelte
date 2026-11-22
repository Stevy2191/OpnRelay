<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type AuditEvent } from '../lib/api';
  import { errorText, dateTime } from '../lib/format';

  let events = $state<AuditEvent[]>([]);
  let action = $state('');
  let error = $state('');
  let more = $state(true);
  let verify = $state<{ ok: boolean; events_checked: number; first_bad_seq: number | null } | null>(null);

  async function load(reset = true) {
    error = '';
    try {
      const p = new URLSearchParams({ limit: '100' });
      if (action) p.set('action', action);
      if (!reset && events.length) p.set('before_seq', String(events[events.length - 1].seq));
      const page = await api<AuditEvent[]>(`/audit/events?${p}`);
      events = reset ? page : [...events, ...page];
      more = page.length === 100;
    } catch (e) {
      error = errorText(e);
    }
  }
  onMount(() => load());

  let verifying = $state(false);
  async function runVerify() {
    verifying = true;
    error = '';
    try {
      verify = await api('/audit/verify');
    } catch (e) {
      error = errorText(e);
    } finally {
      verifying = false;
    }
  }

  const exportUrl = (fmt: string) => `/api/v1/audit/export?format=${fmt}${action ? `&action=${encodeURIComponent(action)}` : ''}`;
  const filters = ['', 'auth.', 'guide.', 'user.', 'workspace.', 'org.', 'audit.'];
</script>

<div class="spread head">
  <h1>Audit log</h1>
  <div class="row">
    <button onclick={runVerify} disabled={verifying}>{verifying ? 'Verifying…' : 'Verify integrity'}</button>
    <a class="btn" href={exportUrl('csv')}>Export CSV</a>
    <a class="btn" href={exportUrl('json')}>Export JSON</a>
  </div>
</div>
<p class="muted">Every sign-in, view, change, export and admin action is recorded here. Entries are chained with SHA-256 hashes, so any edit or deletion is detectable.</p>
{#if verify}
  <p class="callout {verify.ok ? 'ok' : 'error'}" role="status">
    {#if verify.ok}<strong>Intact.</strong> {verify.events_checked} events verified.{:else}<strong>Chain broken</strong> at event #{verify.first_bad_seq}. Investigate immediately.{/if}
  </p>
{/if}

<div class="row filters">
  <label for="act" class="inline">Filter</label>
  <select id="act" bind:value={action} onchange={() => load()} style="width:auto">
    {#each filters as f}<option value={f}>{f ? f.replace('.', '') : 'All events'}</option>{/each}
  </select>
</div>
{#if error}<p class="callout error" role="alert">{error}</p>{/if}

<div class="table-wrap">
  <table class="data audit">
    <thead><tr><th>#</th><th>Time</th><th>Who</th><th>Action</th><th>Target</th><th>Details</th></tr></thead>
    <tbody>
      {#each events as e (e.seq)}
        <tr>
          <td class="mono small muted">{e.seq}</td>
          <td class="mono small time">{dateTime(e.ts)}</td>
          <td class="small">{e.actor_label}<br /><span class="muted mono">{e.actor_ip ?? ''}</span></td>
          <td><code class="act">{e.action}</code></td>
          <td class="small">
            {#if e.target_type === 'guide' && e.target_id}<a href={`#/guides/${e.target_id}`}>guide</a>{:else}{e.target_type}{/if}
          </td>
          <td class="small details">{Object.keys(e.metadata).length ? JSON.stringify(e.metadata) : ''}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>
{#if more}<p class="more"><button onclick={() => load(false)}>Load older events</button></p>{/if}

<style>
  .filters { margin: 0.75rem 0; }
  .details { max-width: 360px; word-break: break-word; font-family: var(--font-mono); font-size: 0.75rem; }
  .time { white-space: nowrap; }
  table.audit { min-width: 720px; }
  .act { background: transparent; padding: 0; font-size: 0.8rem; overflow-wrap: anywhere; }
</style>
