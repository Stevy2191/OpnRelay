<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type Group, type Member, type User, type Workspace } from '../lib/api';
  import { go } from '../lib/router.svelte';
  import { session, isAdmin } from '../lib/session.svelte';
  import { errorText } from '../lib/format';

  let { id }: { id: string } = $props();
  let ws = $state<Workspace | null>(null);
  let members = $state<Member[]>([]);
  let users = $state<User[]>([]);
  let groups = $state<Group[]>([]);
  let error = $state('');
  let notice = $state('');
  let pick = $state('');
  let reviewDays = $state(90);
  let reviewOff = $state(false);
  let reviewError = $state('');
  function syncReview(w: Workspace) {
    reviewOff = w.review_period_days === null;
    reviewDays = w.review_period_days ?? 90;
  }
  let pickRole = $state<Member['role']>('viewer');

  async function load() {
    try {
      [ws, members, users] = await Promise.all([
        api<Workspace>(`/workspaces/${id}`),
        api<Member[]>(`/workspaces/${id}/members`),
        api<User[]>('/admin/users'),
      ]);
      syncReview(ws);
      groups = await api<Group[]>('/admin/groups');
    } catch (e) {
      error = errorText(e);
    }
  }
  onMount(load);

  async function saveMembers(next: Member[]) {
    error = notice = '';
    try {
      members = await api<Member[]>(`/workspaces/${id}/members`, {
        method: 'PUT',
        body: { members: next.map(({ principal_type, principal_id, role }) => ({ principal_type, principal_id, role })) },
      });
      notice = 'Members updated';
    } catch (e) {
      error = errorText(e);
    }
  }

  function add() {
    if (!pick) return;
    const [type, pid] = pick.split(':') as ['user' | 'group', string];
    saveMembers([...members.filter((m) => m.principal_id !== pid), { principal_type: type, principal_id: pid, role: pickRole }]);
    pick = '';
  }

  async function saveInfo(e: Event) {
    e.preventDefault();
    if (!ws) return;
    try {
      ws = await api<Workspace>(`/workspaces/${id}`, { method: 'PATCH', body: { name: ws.name, description: ws.description } });
      notice = 'Saved';
    } catch (err) {
      error = errorText(err);
    }
  }

  async function saveReview(e: Event) {
    e.preventDefault();
    error = notice = reviewError = '';
    if (!reviewOff && (!Number.isInteger(reviewDays) || reviewDays < 1 || reviewDays > 3650)) {
      reviewError = 'Enter a whole number of days between 1 and 3650, or choose Off.';
      return;
    }
    try {
      ws = await api<Workspace>(`/workspaces/${id}`, { method: 'PATCH', body: { review_period_days: reviewOff ? null : reviewDays } });
      syncReview(ws);
      notice = 'Review period saved';
    } catch (err) {
      reviewError = errorText(err);
    }
  }

  async function setHold(on: boolean) {
    try {
      ws = await api<Workspace>(`/workspaces/${id}`, { method: 'PATCH', body: { legal_hold: on } });
    } catch (e) {
      error = errorText(e);
    }
  }

  async function remove() {
    if (!confirm(`Delete workspace "${ws?.name}" and hide all its guides?`)) return;
    try {
      await api(`/workspaces/${id}`, { method: 'DELETE' });
      go('/');
    } catch (e) {
      error = errorText(e);
    }
  }

  let candidates = $derived([
    ...users.filter((u) => u.status !== 'disabled' && !members.some((m) => m.principal_id === u.id)).map((u) => ({ key: `user:${u.id}`, label: `${u.display_name} (${u.email})` })),
    ...groups.filter((g) => !members.some((m) => m.principal_id === g.id)).map((g) => ({ key: `group:${g.id}`, label: `Group: ${g.name}` })),
  ]);
</script>

{#if ws}
  <div class="settings stack">
    <a href={`#/?ws=${id}`} class="small">← Back to {ws.name}</a>
    <h1>{ws.name}: settings</h1>
    {#if error}<p class="error">{error}</p>{/if}
    {#if notice}<p class="success">{notice}</p>{/if}

    <form class="card" onsubmit={saveInfo}>
      <label for="n">Name</label><input id="n" bind:value={ws.name} required />
      <label for="d">Description</label><textarea id="d" bind:value={ws.description} rows="2"></textarea>
      <p><button class="primary" type="submit">Save</button></p>
    </form>

    <form class="card" onsubmit={saveReview} novalidate>
      <h2>Review</h2>
      <div class="row">
        <label for="rd" class="inline">Review guides every</label>
        <input id="rd" type="number" min="1" max="3650" step="1" bind:value={reviewDays} disabled={reviewOff} style="width:6rem" />
        <span>days</span>
        <label class="inline"><input type="checkbox" bind:checked={reviewOff} /> Off</label>
      </div>
      <p class="muted small">Published guides not reviewed within this time are flagged “Needs review” for editors. Publishing a new version or choosing “Still accurate” counts as a review.</p>
      {#if reviewError}<p class="error">{reviewError}</p>{/if}
      <p><button class="primary" type="submit">Save</button></p>
    </form>

    <div class="card">
      <h2>Members</h2>
      <p class="muted small">Viewers can read and export. Editors can also create and change guides. Managers can also manage members. Admins and auditors always have access.</p>
      <table>
        <tbody>
          {#each members as m (m.principal_id)}
            <tr>
              <td>{m.principal_type === 'group' ? '👥 ' : ''}{m.name ?? m.principal_id}</td>
              <td>
                <select value={m.role} aria-label="Role" onchange={(e) => saveMembers(members.map((x) => (x.principal_id === m.principal_id ? { ...x, role: (e.currentTarget as HTMLSelectElement).value as Member['role'] } : x)))}>
                  <option value="viewer">Viewer</option><option value="editor">Editor</option><option value="manager">Manager</option>
                </select>
              </td>
              <td><button class="small danger" disabled={m.principal_id === session.me?.id && !isAdmin(session.me)} onclick={() => saveMembers(members.filter((x) => x.principal_id !== m.principal_id))}>Remove</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      <div class="row add">
        <select bind:value={pick} aria-label="Person or group" style="flex:1">
          <option value="">Add a person or group…</option>
          {#each candidates as c (c.key)}<option value={c.key}>{c.label}</option>{/each}
        </select>
        <select bind:value={pickRole} aria-label="Role" style="width:auto">
          <option value="viewer">Viewer</option><option value="editor">Editor</option><option value="manager">Manager</option>
        </select>
        <button class="primary" onclick={add} disabled={!pick}>Add</button>
      </div>
    </div>

    {#if isAdmin(session.me)}
      <div class="card">
        <h2>Records</h2>
        <label class="inline"><input type="checkbox" checked={ws.legal_hold} onchange={(e) => setHold((e.currentTarget as HTMLInputElement).checked)} /> Legal hold (guides in this workspace cannot be deleted)</label>
        <p><button class="danger" onclick={remove} disabled={ws.legal_hold}>Delete workspace</button></p>
      </div>
    {/if}
  </div>
{:else if error}
  <p class="error">{error}</p>
{:else}
  <p class="muted">Loading…</p>
{/if}

<style>
  .settings { max-width: 760px; margin: 0 auto; }
  .settings h2 { margin-top: 0; }
  .add { margin-top: 1rem; }
</style>
