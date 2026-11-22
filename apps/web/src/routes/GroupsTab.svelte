<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type DirGroup, type Group, type Member, type SsoStatus, type SyncSummary, type User, type Workspace } from '../lib/api';
  import { dateTime, errorText } from '../lib/format';

  let { users, groups, run }: {
    users: User[];
    groups: Group[];
    run: (fn: () => Promise<unknown>, ok?: string) => Promise<void>;
  } = $props();

  let groupName = $state('');
  let status = $state<SsoStatus | null>(null);
  let wsByGroup = $state<Record<string, string[]>>({});
  let wsLookupFailed = $state(false);
  let syncing = $state(false);
  let summary = $state<SyncSummary | null>(null);
  let localError = $state('');

  // In-page confirmations.
  let rolePending = $state<{ id: string; role: Group['grants_role']; changes: number } | null>(null);
  let unlinkPending = $state<string | null>(null);

  // "Add from Microsoft".
  let q = $state('');
  let results = $state<DirGroup[]>([]);
  let searching = $state(false);
  let searchError = $state('');

  const syncOn = $derived(!!status?.group_sync_enabled);
  const stale = $derived(
    syncOn && !!status && !!status.group_sync_last_at && (!status.group_sync_last_ok_at || Date.now() - Date.parse(status.group_sync_last_ok_at) > 24 * 3600 * 1000),
  );

  async function loadStatus() {
    try {
      status = await api<SsoStatus>('/admin/sso');
    } catch (e) {
      localError = errorText(e);
    }
  }

  async function loadWorkspaces() {
    try {
      const map: Record<string, string[]> = {};
      const list = await api<Workspace[]>('/workspaces');
      const results = await Promise.allSettled(
        list.map(async (w) => {
          const ms = await api<Member[]>(`/workspaces/${w.id}/members`);
          for (const m of ms) if (m.principal_type === 'group') (map[m.principal_id] ??= []).push(w.name);
        }),
      );
      wsByGroup = map;
      wsLookupFailed = results.some((r) => r.status === 'rejected');
    } catch {
      // Workspace names are informational; the list still works without them.
      wsLookupFailed = true;
    }
  }

  onMount(() => {
    loadStatus();
    loadWorkspaces();
  });

  async function syncNow() {
    syncing = true;
    localError = '';
    try {
      summary = await api<SyncSummary>('/admin/sso/sync', { body: {} });
      await Promise.all([loadStatus(), run(async () => {})]);
    } catch (e) {
      localError = errorText(e);
    } finally {
      syncing = false;
    }
  }

  async function chooseRole(g: Group, value: string) {
    const role = (value || null) as Group['grants_role'];
    if (role === g.grants_role) {
      rolePending = null;
      return;
    }
    localError = '';
    try {
      const r = await api<{ changes: number }>(`/admin/groups/${g.id}/role-preview${role ? `?grants_role=${role}` : ''}`);
      rolePending = { id: g.id, role, changes: r.changes };
    } catch (e) {
      localError = errorText(e);
      rolePending = null;
    }
  }

  async function confirmRole(g: Group) {
    const p = rolePending;
    if (!p) return;
    rolePending = null;
    await run(() => api(`/admin/groups/${g.id}`, { method: 'PATCH', body: { grants_role: p.role } }), 'Saved');
  }

  async function unlink(g: Group) {
    unlinkPending = null;
    await run(() => api(`/admin/groups/${g.id}`, { method: 'DELETE' }), `${g.name} unlinked`);
    loadWorkspaces();
  }

  async function link(d: DirGroup) {
    await run(() => api('/admin/groups/link', { body: { external_id: d.id } }), `${d.display_name} linked`);
    results = results.map((r) => (r.id === d.id ? { ...r, linked: true } : r));
  }

  let seq = 0;
  $effect(() => {
    const term = q.trim();
    if (!syncOn) return;
    if (!term) {
      results = [];
      searchError = '';
      return;
    }
    const t = setTimeout(async () => {
      const mine = ++seq;
      searching = true;
      searchError = '';
      try {
        const r = await api<DirGroup[]>(`/admin/sso/groups/search?q=${encodeURIComponent(term)}`);
        if (mine === seq) results = r;
      } catch (e) {
        if (mine === seq) searchError = errorText(e);
      } finally {
        if (mine === seq) searching = false;
      }
    }, 300);
    return () => clearTimeout(t);
  });

  async function toggleMember(g: Group, userId: string, on: boolean) {
    const ids = on ? [...g.member_ids, userId] : g.member_ids.filter((x) => x !== userId);
    await run(() => api(`/admin/groups/${g.id}/members`, { method: 'PUT', body: { user_ids: ids } }));
  }

  const roleLabel = (r: string | null) => r ?? 'nothing';
</script>

{#if localError}<p class="error" role="alert">{localError}</p>{/if}

{#if status?.group_sync_enabled}
  <div class="card status">
    <div class="spread">
      <div>
        {#if status.group_sync_last_error}
          <p class="error" role="alert"><strong>Microsoft sync is failing.</strong> {status.group_sync_last_error}</p>
        {:else if status.group_sync_last_ok_at}
          <p class="success">Microsoft sync OK. Last success {dateTime(status.group_sync_last_ok_at)}.</p>
        {:else}
          <p class="muted">Microsoft sync has not run yet.</p>
        {/if}
        {#if status.group_sync_last_warning}<p class="warning" role="status">{status.group_sync_last_warning}</p>{/if}
        {#if stale}<p class="warning" role="status">No successful sync for over 24 hours.</p>{/if}
        {#if status.group_sync_brake_pending}
          <p class="warning" role="status">Sync is paused: it would disable {status.group_sync_brake_pending} people. Review this on the Microsoft sign-in tab.</p>
        {/if}
      </div>
      <button class="small" disabled={syncing} onclick={syncNow}>{syncing ? 'Syncing…' : 'Sync now'}</button>
    </div>
    {#if summary}
      <p class="small muted">
        {#if summary.error}Sync reported an error: {summary.error}{:else}Synced {summary.groups} groups: {summary.added} added, {summary.removed} removed, {summary.roles_changed} roles changed, {summary.disabled} disabled.{/if}
        {#each summary.warnings as w}<br />{w}{/each}
      </p>
    {/if}
  </div>

  <div class="card add">
    <h3>Add from Microsoft</h3>
    <input type="search" placeholder="Search Microsoft groups by name" bind:value={q} aria-label="Search Microsoft groups" />
    {#if searchError}<p class="error" role="alert">{searchError}</p>{/if}
    {#if searching}<p class="muted small">Searching…</p>{/if}
    {#each results as d (d.id)}
      <div class="spread result">
        <div>
          <strong>{d.display_name}</strong>
          {#if d.description}<br /><span class="muted small">{d.description}</span>{/if}
        </div>
        {#if d.linked}<span class="badge">Linked</span>{:else}<button class="small primary" onclick={() => link(d)}>Link</button>{/if}
      </div>
    {:else}
      {#if q.trim() && !searching && !searchError}<p class="muted small">No groups found.</p>{/if}
    {/each}
  </div>
{/if}

<form class="card row" onsubmit={(e) => { e.preventDefault(); run(() => api('/admin/groups', { body: { name: groupName } }), 'Group created').then(() => (groupName = '')); }}>
  <input placeholder="New group name (e.g. EMS Supervisors)" required bind:value={groupName} aria-label="Group name" style="flex:1" />
  <button class="primary" type="submit">Create group</button>
</form>
<p class="muted small">Add a group to a workspace (in the workspace's settings) to give all its members access at once.</p>

{#each groups as g (g.id)}
  {@const wss = wsByGroup[g.id] ?? []}
  <div class="card group">
    <div class="spread">
      <h3>
        {g.name}
        {#if g.source === 'idp'}<span class="badge">Microsoft</span>{/if}
        <span class="muted small">{g.member_ids.length} members</span>
      </h3>
      {#if g.source === 'idp'}
        <button class="small danger" onclick={() => (unlinkPending = g.id)}>Unlink</button>
      {:else}
        <button class="small danger" onclick={() => confirm(`Delete group ${g.name}?`) && run(() => api(`/admin/groups/${g.id}`, { method: 'DELETE' }))}>Delete</button>
      {/if}
    </div>

    {#if g.source === 'idp'}
      <p class="small muted">
        {#if wss.length}In workspaces: {wss.join(', ')}.{:else}Not in any workspace yet.{/if}
        {#if !syncOn}<span class="badge">Not syncing</span>{:else if g.last_synced_at}Last synced {dateTime(g.last_synced_at)}.{/if}
      </p>
      {#if g.missing_since}
        <p class="error" role="alert">Deleted in Microsoft. Members are kept until you unlink this group.</p>
      {/if}
      <label class="inline small">Also make members:
        <select
          value={rolePending?.id === g.id ? (rolePending.role ?? '') : (g.grants_role ?? '')}
          onchange={(e) => chooseRole(g, (e.currentTarget as HTMLSelectElement).value)}
          aria-label="Also make members"
        >
          <option value="">nothing</option>
          <option value="auditor">auditor</option>
          <option value="admin">admin</option>
        </select>
      </label>
      {#if rolePending?.id === g.id}
        <div class="confirm warning" role="alertdialog" aria-label="Confirm role change">
          <span>This changes the role of {rolePending.changes} {rolePending.changes === 1 ? 'person' : 'people'}.</span>
          <span class="row">
            <button class="small primary" onclick={() => confirmRole(g)}>Confirm</button>
            <button class="small" onclick={() => (rolePending = null)}>Cancel</button>
          </span>
        </div>
      {/if}
      {#if unlinkPending === g.id}
        <div class="confirm error" role="alertdialog" aria-label="Confirm unlink">
          <span>
            Unlink {g.name}?
            {#if wsLookupFailed}Couldn't load the workspaces this group is on. Its members may lose access they get only through this group.{:else if wss.length}It will lose access to {wss.join(', ')} and its members will lose access they get only through this group.{:else}Its members will lose access they get only through this group.{/if}
            {#if g.grants_role}People who are {g.grants_role} only through this group go back to member.{/if}
          </span>
          <span class="row">
            <button class="small danger" onclick={() => unlink(g)}>Unlink</button>
            <button class="small" onclick={() => (unlinkPending = null)}>Cancel</button>
          </span>
        </div>
      {/if}
      <div class="members">
        {#each users.filter((u) => g.member_ids.includes(u.id)) as u (u.id)}<span class="small">{u.display_name}</span>{/each}
      </div>
      <p class="muted small">Members come from Microsoft and cannot be edited here. Currently also making members: {roleLabel(g.grants_role)}.</p>
    {:else}
      <div class="members">
        {#each users.filter((u) => u.status !== 'disabled') as u (u.id)}
          <label class="inline small"><input type="checkbox" checked={g.member_ids.includes(u.id)} onchange={(e) => toggleMember(g, u.id, (e.currentTarget as HTMLInputElement).checked)} /> {u.display_name}</label>
        {/each}
      </div>
    {/if}
  </div>
{/each}

<style>
  .card + .card, .card + form { margin-top: 1rem; }
  .group h3, .add h3 { margin: 0; }
  .status p { margin: 0.25rem 0; }
  .members { display: flex; flex-wrap: wrap; gap: 0.25rem 1rem; margin-top: 0.5rem; }
  .result { padding: 0.4rem 0; border-top: 1px solid var(--border); }
  .add input { width: 100%; margin: 0.5rem 0; }
  .confirm { display: flex; justify-content: space-between; align-items: center; gap: 1rem; flex-wrap: wrap; margin: 0.5rem 0; }
</style>
