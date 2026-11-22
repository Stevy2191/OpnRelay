<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { api, type Folder, type GuideSummary } from '../lib/api';
  import { route, query, go } from '../lib/router.svelte';
  import { session, isAdmin, workspacesStore, shellUi } from '../lib/session.svelte';
  import { errorText, when } from '../lib/format';

  let workspaces = $derived(workspacesStore.list);
  let folders = $state<Folder[]>([]);
  let guides = $state<GuideSummary[]>([]);
  let error = $state('');
  let loading = $state(true);
  let workspacesLoaded = $derived(workspacesStore.loaded);

  let params = $derived(query(route.path));
  let q = $derived(params.get('q') ?? '');
  let wsId = $derived(params.get('ws') ?? '');
  let folderId = $derived(params.get('folder') ?? '');
  let statusFilter = $derived(params.get('status') ?? '');
  let reviewOnly = $derived(params.get('needs_review') === 'true');
  let current = $derived(workspaces.find((w) => w.id === wsId));
  let canEdit = $derived(current && (current.my_permission === 'edit' || current.my_permission === 'manage'));

  function link(ws: string, folder = '', review = false) {
    const p = new URLSearchParams();
    if (ws) p.set('ws', ws);
    if (folder) p.set('folder', folder);
    if (review) p.set('needs_review', 'true');
    const s = p.toString();
    return `#/${s ? '?' + s : ''}`;
  }

  // Only editors of a guide's workspace (or admins) are shown the review flag.
  function canEditGuide(g: GuideSummary): boolean {
    if (isAdmin(session.me)) return true;
    const w = workspaces.find((x) => x.id === g.workspace_id);
    return !!w && (w.my_permission === 'edit' || w.my_permission === 'manage');
  }
  let flagged = $state<GuideSummary[]>([]);
  // The server only returns flagged guides from workspaces the caller can edit.
  let flaggedCount = $derived(flagged.length);
  let shown = $derived(reviewOnly ? flagged : statusFilter ? guides.filter((g) => g.status === statusFilter) : guides);

  async function loadGuides() {
    loading = true;
    error = '';
    try {
      const p = new URLSearchParams();
      if (q) p.set('q', q);
      if (wsId && !q) p.set('workspace_id', wsId);
      if (folderId && !q) p.set('folder_id', folderId);
      const all = await api<GuideSummary[]>(`/guides?${p}`);
      guides = all;
      p.set('needs_review', 'true');
      flagged = await api<GuideSummary[]>(`/guides?${p}`);
      folders = wsId ? await api<Folder[]>(`/workspaces/${wsId}/folders`) : [];
    } catch (e) {
      error = errorText(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void q;
    void wsId;
    void folderId;
    loadGuides();
  });

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

  <section>
    {#if folders.length || canEdit}
      <div class="row folders" aria-label="Folders">
        {#if wsId}<a class="btn small" class:primary={!folderId} href={link(wsId)}>All in {current?.name ?? 'workspace'}</a>{/if}
        {#each folders as f (f.id)}
          <a class="btn small" class:primary={f.id === folderId} href={link(wsId, f.id)} title={f.name}><span class="fname">{f.name}</span></a>
        {/each}
        {#if canEdit}<button class="ghost small" onclick={newFolder}>+ Folder</button>{/if}
      </div>
    {/if}
    <div class="spread">
      <h1>
        {#if q}Results for “{q}”{:else if statusFilter === 'draft'}Drafts{:else if reviewOnly}Needs review{:else if current}{current.name}{:else}All guides{/if}
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

    {#if loading || !workspacesLoaded}
      <p class="muted">Loading…</p>
    {:else if workspaces.length === 0}
      <div class="card empty">
        {#if isAdmin(session.me)}
          <p><strong>Create your first workspace</strong></p>
          <p class="muted">Workspaces hold your guides and decide who can see them, for example one per department.</p>
          <p><button class="primary" onclick={() => (shellUi.newWorkspace = true)}>Create workspace</button></p>
        {:else}
          <p><strong>You haven't been added to a workspace yet.</strong></p>
          <p class="muted">Ask your administrator to add you. Guides will appear here once you are.</p>
        {/if}
      </div>
    {:else if shown.length === 0}
      {#if reviewOnly}
        <p class="review-bar"><a href={link(wsId, folderId)}>Clear “Needs review” filter</a></p>
      {/if}
      <div class="card empty">
        <p><strong>{reviewOnly ? 'Nothing needs review' : 'No guides here yet.'}</strong></p>
        <p class="muted">
          {#if reviewOnly}No guides need review.{:else if canEdit}Create one with <em>New guide</em>, then paste or upload screenshots. The desktop recorder and browser extension will add guides here automatically.{:else}Nothing has been published here yet.{/if}
        </p>
      </div>
    {:else}
      {#if flaggedCount > 0 || reviewOnly}
        <p class="review-bar">
          {#if reviewOnly}
            <span class="badge review">Needs review</span>
            <a href={link(wsId, folderId)}>Clear filter</a>
          {:else}
            <a href={link(wsId, folderId, true)}>{flaggedCount} {flaggedCount === 1 ? 'guide needs' : 'guides need'} review</a>
          {/if}
        </p>
      {/if}
      <div class="grid">
        {#each shown as g (g.id)}
          <a class="card guide" href={`#/guides/${g.id}`}>
            <div class="spread">
              <span class="badge {g.status}">{g.status}</span>
              {#if g.classification !== 'internal'}<span class="badge {g.classification}">{g.classification}</span>{/if}
              {#if g.needs_review && canEditGuide(g)}<span class="badge review">Needs review</span>{/if}
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

<style>
  .folders { margin-bottom: 0.75rem; }
  .fname { max-width: 14rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 1rem; margin-top: 1rem; }
  .guide { color: var(--text); display: block; }
  .guide:hover { border-color: var(--sig); text-decoration: none; }
  .guide h3 { margin: 0.6rem 0 0.25rem; }
  .desc { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; margin: 0; }
  .empty { margin-top: 1rem; }
  .review-bar { margin: 0.75rem 0 0; display: flex; gap: 0.75rem; align-items: center; }
</style>
