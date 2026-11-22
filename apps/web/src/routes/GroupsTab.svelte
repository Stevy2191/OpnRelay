<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type DirGroup, type Group, type Member, type SsoStatus, type SyncSummary, type User, type Workspace } from '../lib/api';
  import { dateTime, errorText } from '../lib/format';
  import MsMark from '../lib/MsMark.svelte';

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
  let deletePending = $state<string | null>(null);

  // "Add from Microsoft" popover.
  let adding = $state(false);
  let searchEl: HTMLInputElement | undefined = $state();
  let addBtn: HTMLButtonElement | undefined = $state();
  function closeAdd() {
    adding = false;
    addBtn?.focus();
  }
  $effect(() => {
    if (adding) searchEl?.focus();
  });
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

  async function removeGroup(g: Group) {
    deletePending = null;
    await run(() => api(`/admin/groups/${g.id}`, { method: 'DELETE' }), `${g.name} deleted`);
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

{#if localError}<p class="callout error" role="alert">{localError}</p>{/if}

{#if status?.group_sync_enabled}
  <div class="status stack">
    <div class="callout banner" class:ok={!status.group_sync_last_error && !!status.group_sync_last_ok_at} class:error={!!status.group_sync_last_error}>
      <div class="spread">
        <div>
          {#if status.group_sync_last_error}
            <p role="alert"><strong>Microsoft sync is failing.</strong> {status.group_sync_last_error}</p>
          {:else if status.group_sync_last_ok_at}
            <p><strong>Microsoft sync OK.</strong> Last success {dateTime(status.group_sync_last_ok_at)}.</p>
          {:else}
            <p class="muted">Microsoft sync has not run yet.</p>
          {/if}
        </div>
        <button class="small" disabled={syncing} onclick={syncNow}>{syncing ? 'Syncing…' : 'Sync now'}</button>
      </div>
    </div>
    {#if status.group_sync_last_warning}<p class="callout warning" role="status">{status.group_sync_last_warning}</p>{/if}
    {#if stale}<p class="callout warning" role="status">No successful sync for over 24 hours.</p>{/if}
    {#if status.group_sync_brake_pending}
      <p class="callout warning" role="status">Disabling accounts is on hold: the sync would disable {status.group_sync_brake_pending} people. Memberships and roles keep syncing. Review this on the Microsoft sign-in tab.</p>
    {/if}
    {#if summary}
      <p class="small muted">
        {#if summary.error}Sync reported an error: {summary.error}{:else}Synced {summary.groups} groups: {summary.added} added, {summary.removed} removed, {summary.roles_changed} roles changed, {summary.disabled} disabled.{/if}
        {#each summary.warnings as w}<br />{w}{/each}
      </p>
    {/if}
  </div>
{/if}

<div class="toolbar">
  <form class="newgroup row" onsubmit={(e) => { e.preventDefault(); run(() => api('/admin/groups', { body: { name: groupName } }), 'Group created').then(() => (groupName = '')); }}>
    <input placeholder="New group name (e.g. EMS Supervisors)" required bind:value={groupName} aria-label="Group name" />
    <button class="primary" type="submit">Create group</button>
  </form>
  {#if status?.group_sync_enabled}
    <div class="addwrap">
      <button bind:this={addBtn} aria-expanded={adding} aria-controls="ms-add" onclick={() => (adding ? closeAdd() : (adding = true))}>
        <MsMark /> Add from Microsoft
      </button>
      {#if adding}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div id="ms-add" class="card pop" role="group" aria-label="Add from Microsoft" onkeydown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); closeAdd(); } }}>
          <div class="spread">
            <h3>Add from Microsoft</h3>
            <button class="small ghost" onclick={closeAdd} aria-label="Close">Close</button>
          </div>
          <input bind:this={searchEl} type="search" placeholder="Search Microsoft groups by name" bind:value={q} aria-label="Search Microsoft groups" />
          {#if searchError}<p class="callout error" role="alert">{searchError}</p>{/if}
          {#if searching}<p class="muted small">Searching…</p>{/if}
          {#each results as d (d.id)}
            <div class="spread result">
              <div>
                <strong>{d.display_name}</strong>
                {#if d.description}<br /><span class="muted small">{d.description}</span>{/if}
              </div>
              {#if d.linked}<span class="tag">Linked</span>{:else}<button class="small primary" onclick={() => link(d)}>Link</button>{/if}
            </div>
          {:else}
            {#if q.trim() && !searching && !searchError}<p class="muted small">No groups found.</p>{/if}
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>
<p class="muted small hint">Add a group to a workspace (in the workspace's settings) to give all its members access at once.</p>

{#if groups.length === 0}
  <div class="card"><p class="muted">No groups yet.</p></div>
{:else}
  <div class="table-wrap">
  <table class="data stackable groups">
    <thead><tr><th>Group</th><th>Members</th><th>Opens workspaces</th><th>Grants role</th><th>Last synced</th><th>Actions</th></tr></thead>
    {#each groups as g (g.id)}
      {@const wss = wsByGroup[g.id] ?? []}
      <tbody>
        <tr>
          <td data-label="Group"><div class="c-name">
            <strong>{g.name}</strong>
            {#if g.source === 'idp'}<span class="tag ms"><MsMark /> Microsoft</span>{:else}<span class="tag">OpnRelay</span>{/if}
            {#if g.source === 'idp' && !syncOn}<span class="tag">Not syncing</span>{/if}
            {#if g.missing_since}<span class="tag bad">Deleted in Microsoft</span>{/if}
          </div></td>
          <td data-label="Members" class="mono small">{g.member_ids.length}</td>
          <td data-label="Opens workspaces" class="small">
            {#if g.source === 'idp'}{#if wss.length}{wss.join(', ')}{:else if wsLookupFailed}<span class="muted">Unknown</span>{:else}<span class="muted">None yet</span>{/if}{:else}<span class="muted">Set in each workspace</span>{/if}
          </td>
          <td data-label="Grants role">
            {#if g.source === 'idp'}
              <select
                class="role-select"
                value={rolePending?.id === g.id ? (rolePending.role ?? '') : (g.grants_role ?? '')}
                onchange={(e) => chooseRole(g, (e.currentTarget as HTMLSelectElement).value)}
                aria-label={`Also make ${g.name} members`}
              >
                <option value="">nothing</option>
                <option value="auditor">auditor</option>
                <option value="admin">admin</option>
              </select>
            {:else}<span class="muted">n/a</span>{/if}
          </td>
          <td data-label="Last synced" class="mono small">{#if g.source === 'idp' && g.last_synced_at}{dateTime(g.last_synced_at)}{:else}<span class="muted">n/a</span>{/if}</td>
          <td data-label="Actions">
            {#if g.source === 'idp'}
              <button class="small danger" onclick={() => (unlinkPending = g.id)}>Unlink</button>
            {:else}
              <button class="small danger" onclick={() => (deletePending = g.id)}>Delete</button>
            {/if}
          </td>
        </tr>
        <tr class="detail">
          <td colspan="6">
            {#if g.missing_since}
              <p class="callout error" role="alert">Deleted in Microsoft. Its members were removed; unlink this group.</p>
            {/if}
            {#if rolePending?.id === g.id}
              <div class="confirm callout warning" role="alertdialog" aria-label="Confirm role change">
                <span>This changes the role of {rolePending.changes} {rolePending.changes === 1 ? 'person' : 'people'}.</span>
                <span class="row">
                  <button class="small primary" onclick={() => confirmRole(g)}>Confirm</button>
                  <button class="small" onclick={() => (rolePending = null)}>Cancel</button>
                </span>
              </div>
            {/if}
            {#if unlinkPending === g.id}
              <div class="confirm callout error" role="alertdialog" aria-label="Confirm unlink">
                <span>
                  Unlink {g.name}?
                  {#if wsLookupFailed}Couldn't load the workspaces this group is on. Its members may lose access they get only through this group.{:else if wss.length}It will lose access to {wss.join(', ')} and its members will lose access they get only through this group.{:else}Its members will lose access they get only through this group.{/if}
                  {#if g.grants_role}{#if groups.filter((x) => x.source === 'idp' && x.grants_role).length === 1}This is the last group that grants roles: its members keep their current role and become manually managed.{:else}People who are {g.grants_role} only through this group go back to member.{/if}{/if}
                </span>
                <span class="row">
                  <button class="small danger" onclick={() => unlink(g)}>Unlink</button>
                  <button class="small" onclick={() => (unlinkPending = null)}>Cancel</button>
                </span>
              </div>
            {/if}
            {#if deletePending === g.id}
              <div class="confirm callout error" role="alertdialog" aria-label="Confirm delete">
                <span>Delete group {g.name}? Its members lose any workspace access they get only through this group.</span>
                <span class="row">
                  <button class="small danger" onclick={() => removeGroup(g)}>Delete</button>
                  <button class="small" onclick={() => (deletePending = null)}>Cancel</button>
                </span>
              </div>
            {/if}
            <details>
              <summary class="small">Members ({g.member_ids.length})</summary>
              {#if g.source === 'idp'}
                <div class="members">
                  {#each users.filter((u) => g.member_ids.includes(u.id)) as u (u.id)}<span class="small">{u.display_name}</span>{:else}<span class="muted small">No members.</span>{/each}
                </div>
                <p class="muted small">Members come from Microsoft and cannot be edited here. Currently also making members: {roleLabel(g.grants_role)}.</p>
              {:else}
                <div class="members">
                  {#each users.filter((u) => u.status !== 'disabled') as u (u.id)}
                    <label class="inline small"><input type="checkbox" checked={g.member_ids.includes(u.id)} onchange={(e) => toggleMember(g, u.id, (e.currentTarget as HTMLInputElement).checked)} /> {u.display_name}</label>
                  {/each}
                </div>
              {/if}
            </details>
          </td>
        </tr>
      </tbody>
    {/each}
  </table>
  </div>
{/if}

<style>
  .status { margin-bottom: 1rem; }
  .banner .spread { align-items: center; }
  .toolbar { display: flex; flex-wrap: wrap; gap: 0.75rem; align-items: center; justify-content: space-between; position: relative; }
  .newgroup { flex: 1; min-width: 260px; max-width: 520px; flex-wrap: nowrap; }
  .newgroup input { flex: 1; min-width: 0; }
  .addwrap { position: relative; }
  .pop { position: absolute; right: 0; top: calc(100% + 6px); width: min(420px, calc(100vw - 2rem)); z-index: 20; box-shadow: var(--shadow-pop); max-height: 70vh; overflow-y: auto; }
  .pop h3 { margin: 0; }
  .pop input { margin: 0.5rem 0; }
  .result { padding: 0.4rem 0; border-top: 1px solid var(--line); }
  .hint { margin: 0.5rem 0 1rem; }
  .c-name { display: flex; align-items: center; flex-wrap: wrap; gap: 0.3rem 0.5rem; }
  .role-select { width: auto; min-width: 6.5rem; font-family: var(--font-mono); font-size: 0.8rem; text-transform: uppercase; padding: 0.2rem 0.4rem; }
  table.groups tbody + tbody tr:first-child td { border-top: 1px solid var(--line); }
  @media (max-width: 720px) {
    table.groups tbody { padding: 0.25rem 0 0.5rem; border-top: 6px solid var(--surface-2); }
    table.groups tbody:first-of-type { border-top: 0; }
    table.groups tbody tr { border-bottom: 0; }
  }
  tr.detail td { padding-top: 0; }
  tr.detail td:empty { display: none; }
  tr.detail :global(.callout) { margin-bottom: 0.5rem; }
  summary { cursor: pointer; color: var(--muted-strong); }
  .members { display: flex; flex-wrap: wrap; gap: 0.25rem 1rem; margin: 0.5rem 0; }
  .confirm { display: flex; justify-content: space-between; align-items: center; gap: 0.5rem 1rem; flex-wrap: wrap; }
  @media (max-width: 720px) {
    .newgroup { flex-basis: 100%; max-width: none; }
    .pop { position: fixed; left: 1rem; right: 1rem; top: auto; bottom: 1rem; width: auto; }
  }
</style>
