<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { api, type Folder, type GuideSummary, type Workspace } from '../lib/api';
  import { route, query, go } from '../lib/router.svelte';
  import { session, isAdmin } from '../lib/session.svelte';
  import { errorText, when } from '../lib/format';

  let workspaces = $state<Workspace[]>([]);
  let folders = $state<Folder[]>([]);
  let guides = $state<GuideSummary[]>([]);
  let error = $state('');
  let loading = $state(true);

  let params = $derived(query(route.path));
  let q = $derived(params.get('q') ?? '');
  let wsId = $derived(params.get('ws') ?? '');
  let folderId = $derived(params.get('folder') ?? '');
  let current = $derived(workspaces.find((w) => w.id === wsId));
  let canEdit = $derived(current && (current.my_permission === 'edit' || current.my_permission === 'manage'));

  function link(ws: string, folder = '') {
    const p = new URLSearchParams();
    if (ws) p.set('ws', ws);
    if (folder) p.set('folder', folder);
    const s = p.toString();
    return `#/${s ? '?' + s : ''}`;
  }

  async function loadWorkspaces() {
    workspaces = await api<Workspace[]>('/workspaces');
  }

  async function loadGuides() {
    loading = true;
    error = '';
    try {
      const p = new URLSearchParams();
      if (q) p.set('q', q);
      if (wsId && !q) p.set('workspace_id', wsId);
      if (folderId && !q) p.set('folder_id', folderId);
      guides = await api<GuideSummary[]>(`/guides?${p}`);
      folders = wsId ? await api<Folder[]>(`/workspaces/${wsId}/folders`) : [];
    } catch (e) {
      error = errorText(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    loadWorkspaces().catch((e) => (error = errorText(e)));
  });
  $effect(() => {
    void q;
    void wsId;
    void folderId;
    loadGuides();
  });

  async function newWorkspace() {
    const name = prompt('Workspace name (e.g. HR, EMS, IT)');
    if (!name) return;
    try {
      const ws = await api<{ id: string }>('/workspaces', { body: { name } });
      await loadWorkspaces();
      window.location.hash = link(ws.id);
    } catch (e) {
      alert(errorText(e));
    }
  }

  async function newFolder() {
    const name = prompt('Folder name');
    if (!name || !wsId) return;
    try {
      await api(`/workspaces/${wsId}/folders`, { body: { name } });
      await loadGuides();
    } catch (e) {
      alert(errorText(e));
    }
  }

  async function newGuide() {
    if (!wsId) return;
    const title = prompt('Guide title', 'Untitled guide');
    if (!title) return;
    try {
      const g = await api<{ id: string }>('/guides', {
        body: { workspace_id: wsId, folder_id: folderId || null, title },
      });
      go(`/guides/${g.id}/edit`);
    } catch (e) {
      alert(errorText(e));
    }
  }

  let wsName = (id: string) => workspaces.find((w) => w.id === id)?.name ?? '';
</script>

<div class="layout">
  <aside>
    <div class="spread">
      <h3>Workspaces</h3>
      {#if isAdmin(session.me)}<button class="small" onclick={newWorkspace} title="New workspace">+ New</button>{/if}
    </div>
    <ul class="nav">
      <li><a class:active={!wsId && !q} href="#/">All guides</a></li>
      {#each workspaces as w (w.id)}
        <li>
          <a class:active={w.id === wsId && !folderId} href={link(w.id)}>{w.name} <span class="muted small">{w.guide_count}</span></a>
          {#if w.id === wsId}
            <ul class="nav sub">
              {#each folders as f (f.id)}
                <li><a class:active={f.id === folderId} href={link(w.id, f.id)}>📁 {f.name}</a></li>
              {/each}
              {#if canEdit}<li><button class="ghost small" onclick={newFolder}>+ Folder</button></li>{/if}
            </ul>
          {/if}
        </li>
      {:else}
        <li class="muted small">
          {isAdmin(session.me) ? 'Create a workspace for each team to get started.' : 'You have not been added to a workspace yet.'}
        </li>
      {/each}
    </ul>
  </aside>

  <section>
    <div class="spread">
      <h1>
        {#if q}Results for “{q}”{:else if current}{current.name}{:else}All guides{/if}
      </h1>
      <div class="row">
        {#if current && current.my_permission === 'manage'}
          <a class="btn" href={`#/workspaces/${current.id}/settings`}>Members & settings</a>
        {/if}
        {#if canEdit}<button class="primary" onclick={newGuide}>+ New guide</button>{/if}
      </div>
    </div>
    {#if current?.description}<p class="muted">{current.description}</p>{/if}
    {#if error}<p class="error">{error}</p>{/if}

    {#if loading}
      <p class="muted">Loading…</p>
    {:else if guides.length === 0}
      <div class="card empty">
        <p><strong>No guides here yet.</strong></p>
        <p class="muted">
          {#if canEdit}Create one with <em>New guide</em>, then paste or upload screenshots. The desktop recorder and browser extension will add guides here automatically.{:else if !wsId}Pick a workspace on the left.{:else}Nothing has been published here yet.{/if}
        </p>
      </div>
    {:else}
      <div class="grid">
        {#each guides as g (g.id)}
          <a class="card guide" href={`#/guides/${g.id}`}>
            <div class="spread">
              <span class="badge {g.status}">{g.status}</span>
              {#if g.classification !== 'internal'}<span class="badge {g.classification}">{g.classification}</span>{/if}
            </div>
            <h3>{g.title}</h3>
            {#if g.description}<p class="muted small desc">{g.description}</p>{/if}
            <p class="muted small">
              {g.step_count} steps · {g.author_name ?? 'unknown'} · {when(g.updated_at)}
              {#if !wsId}<br />{wsName(g.workspace_id)}{/if}
            </p>
          </a>
        {/each}
      </div>
    {/if}
  </section>
</div>

<style>
  .layout { display: grid; grid-template-columns: 240px 1fr; gap: 1.5rem; }
  @media (max-width: 760px) { .layout { grid-template-columns: 1fr; } }
  aside h3 { margin: 0; }
  .nav { list-style: none; padding: 0; margin: 0.5rem 0; }
  .nav a { display: flex; justify-content: space-between; padding: 0.35rem 0.6rem; border-radius: 6px; color: var(--text); }
  .nav a.active { background: var(--surface-2); font-weight: 600; }
  .nav.sub { margin: 0 0 0 0.75rem; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 1rem; margin-top: 1rem; }
  .guide { color: var(--text); display: block; }
  .guide:hover { border-color: var(--primary); text-decoration: none; }
  .guide h3 { margin: 0.6rem 0 0.25rem; }
  .desc { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; margin: 0; }
  .empty { margin-top: 1rem; }
</style>
