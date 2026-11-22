<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { api, type Folder, type Group, type GuideSummary, type Member } from '../lib/api';
  import { route, query, go } from '../lib/router.svelte';
  import { session, isAdmin, workspacesStore, shellUi } from '../lib/session.svelte';
  import { errorText } from '../lib/format';
  import GuideCard from '../lib/GuideCard.svelte';
  import GuideTable from '../lib/GuideTable.svelte';

  let workspaces = $derived(workspacesStore.list);
  let folders = $state<Folder[]>([]);
  let members = $state<Member[]>([]);
  let msGroups = $state<Set<string>>(new Set());
  let guides = $state<GuideSummary[]>([]);
  let error = $state('');
  let loading = $state(true);
  let workspacesLoaded = $derived(workspacesStore.loaded);

  let params = $derived(query(route.path));
  let q = $derived(params.get('q') ?? '');
  let wsId = $derived(params.get('ws') ?? '');
  let folderSel = $derived(params.get('folder') ?? ''); // '' = all, 'none' = unfiled, else a folder id
  let statusFilter = $derived(params.get('status') ?? '');
  let reviewOnly = $derived(params.get('needs_review') === 'true');
  let inWorkspace = $derived(!!wsId && !q && !reviewOnly && !statusFilter);
  let current = $derived(workspaces.find((w) => w.id === wsId));
  let canEdit = $derived(!!current && (current.my_permission === 'edit' || current.my_permission === 'manage'));
  let isAll = $derived(!q && !wsId && !statusFilter && !reviewOnly);

  // Cards / List switch, remembered per browser.
  const VIEW_KEY = 'runbook.workspace-view';
  function readView(): 'cards' | 'list' {
    try {
      return localStorage.getItem(VIEW_KEY) === 'list' ? 'list' : 'cards';
    } catch {
      return 'cards';
    }
  }
  let view = $state<'cards' | 'list'>(readView());
  function setView(v: 'cards' | 'list') {
    view = v;
    try {
      localStorage.setItem(VIEW_KEY, v);
    } catch {
      /* storage unavailable: the choice just won't be remembered */
    }
  }

  function link(ws: string, folder = '') {
    const p = new URLSearchParams();
    if (ws) p.set('ws', ws);
    if (folder) p.set('folder', folder);
    const s = p.toString();
    return `#/${s ? '?' + s : ''}`;
  }

  // Only editors of a guide's workspace (or admins) are shown the review flag.
  function canEditGuide(g: GuideSummary): boolean {
    if (isAdmin(session.me)) return true;
    const w = workspaces.find((x) => x.id === g.workspace_id);
    return !!w && (w.my_permission === 'edit' || w.my_permission === 'manage');
  }
  let wsName = (id: string) => workspaces.find((w) => w.id === id)?.name ?? '';
  let canSeeReview = $derived(isAdmin(session.me) || workspaces.some((w) => w.my_permission === 'edit' || w.my_permission === 'manage'));

  let token = 0;
  async function loadGuides() {
    const mine = ++token;
    loading = true;
    error = '';
    try {
      const p = new URLSearchParams();
      if (q) p.set('q', q);
      else if (reviewOnly) p.set('needs_review', 'true');
      else if (statusFilter === 'draft') p.set('status', 'draft');
      else if (wsId) p.set('workspace_id', wsId);
      const list = await api<GuideSummary[]>(`/guides?${p}`);
      let f: Folder[] = [];
      let m: Member[] = [];
      if (inWorkspace) {
        [f, m] = await Promise.all([api<Folder[]>(`/workspaces/${wsId}/folders`), api<Member[]>(`/workspaces/${wsId}/members`).catch(() => [])]);
        if (isAdmin(session.me) && m.some((x) => x.principal_type === 'group')) {
          const gs = await api<Group[]>('/admin/groups').catch(() => []);
          if (mine === token) msGroups = new Set(gs.filter((g) => g.source === 'idp').map((g) => g.id));
        }
      }
      if (mine !== token) return;
      guides = list;
      folders = f;
      members = m;
    } catch (e) {
      if (mine === token) error = errorText(e);
    } finally {
      if (mine === token) loading = false;
    }
  }

  $effect(() => {
    void q;
    void wsId;
    void statusFilter;
    void reviewOnly;
    loadGuides();
  });

  // Inline folder creation (no browser prompt)
  let addingFolder = $state(false);
  let folderName = $state('');
  let busy = $state(false);
  async function newFolder(e: Event) {
    e.preventDefault();
    if (!folderName.trim() || !wsId || busy) return;
    busy = true;
    try {
      await api(`/workspaces/${wsId}/folders`, { body: { name: folderName.trim() } });
      folderName = '';
      addingFolder = false;
      await loadGuides();
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  async function newGuide(folderId: string | null = null) {
    if (!wsId || busy) return;
    busy = true;
    error = '';
    try {
      const g = await api<{ id: string }>('/guides', {
        body: { workspace_id: wsId, folder_id: folderId, title: 'Untitled guide' },
      });
      go(`/guides/${g.id}/edit`);
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  // Workspace view model
  let unfiled = $derived(guides.filter((g) => !g.folder_id || !folders.some((f) => f.id === g.folder_id)));
  let inFolder = (id: string) => guides.filter((g) => g.folder_id === id);
  let selectedGuides = $derived(folderSel === 'none' ? unfiled : folderSel ? inFolder(folderSel) : guides);
  let sections = $derived.by(() => {
    const out: { id: string; name: string; guides: GuideSummary[]; folderId: string | null }[] = [];
    for (const f of folders) {
      if (folderSel && folderSel !== f.id) continue;
      out.push({ id: f.id, name: f.name, guides: inFolder(f.id), folderId: f.id });
    }
    if ((!folderSel || folderSel === 'none') && (unfiled.length > 0 || folders.length === 0)) {
      out.push({ id: 'none', name: 'Unfiled', guides: unfiled, folderId: null });
    }
    return out;
  });

  let shownMembers = $derived(members.slice(0, 6));
  const roleLabel = (r: string) => r.charAt(0).toUpperCase() + r.slice(1);

  let counts = $derived({
    published: guides.filter((g) => g.status === 'published').length,
    drafts: guides.filter((g) => g.status === 'draft').length,
  });
  let list = $derived(inWorkspace ? selectedGuides : guides);
</script>

<section>
  {#if inWorkspace && current}
    <header class="ws-head">
      <nav class="crumb" aria-label="Breadcrumb"><a href="#/">All guides</a><span>Workspaces</span></nav>
      <div class="spread">
        <div class="ws-title">
          <h1 title={current.name}>{current.name}</h1>
          <p class="muted small">{guides.length} {guides.length === 1 ? 'guide' : 'guides'} · {folders.length} {folders.length === 1 ? 'folder' : 'folders'}</p>
        </div>
        <div class="row">
          {#if current.my_permission === 'manage'}<a class="btn" href={`#/workspaces/${current.id}/settings`}>Workspace settings</a>{/if}
          {#if canEdit}<button class="primary" onclick={() => newGuide(folderSel && folderSel !== 'none' ? folderSel : null)} disabled={busy}>+ New guide</button>{/if}
        </div>
      </div>
      {#if current.description}<p class="muted desc">{current.description}</p>{/if}
      {#if members.length}
        <div class="access" aria-label="Access">
          <span class="muted small">Access:</span>
          {#each shownMembers as m (m.principal_id)}
            <span class="chip" title={`${m.name ?? m.principal_id} (${m.role})`}>
              {#if m.principal_type === 'group'}
                {#if msGroups.has(m.principal_id)}
                  <svg class="ms" width="12" height="12" viewBox="0 0 12 12" role="img" aria-label="Microsoft group"><rect width="5.5" height="5.5" fill="#f25022" /><rect x="6.5" width="5.5" height="5.5" fill="#7fba00" /><rect y="6.5" width="5.5" height="5.5" fill="#00a4ef" /><rect x="6.5" y="6.5" width="5.5" height="5.5" fill="#ffb900" /></svg>
                {:else}<span aria-hidden="true">👥</span>{/if}
              {/if}
              <span class="cname">{m.name ?? 'Unknown'}</span>
              <span class="role mono">{m.role}</span>
            </span>
          {/each}
          {#if members.length > shownMembers.length}
            <span class="chip more">+{members.length - shownMembers.length} more</span>
          {/if}
        </div>
      {/if}
    </header>

    <div class="toolbar">
      <div class="chips" role="group" aria-label="Folders">
        <a class="fchip" class:on={!folderSel} href={link(wsId)} aria-current={!folderSel ? 'true' : undefined}>All <span class="n mono">{guides.length}</span></a>
        {#each folders as f (f.id)}
          <a class="fchip" class:on={folderSel === f.id} href={link(wsId, f.id)} title={f.name} aria-current={folderSel === f.id ? 'true' : undefined}><span class="fname">{f.name}</span> <span class="n mono">{inFolder(f.id).length}</span></a>
        {/each}
        {#if unfiled.length > 0 && folders.length > 0}
          <a class="fchip" class:on={folderSel === 'none'} href={link(wsId, 'none')} aria-current={folderSel === 'none' ? 'true' : undefined}>Unfiled <span class="n mono">{unfiled.length}</span></a>
        {/if}
        {#if canEdit && !addingFolder}<button class="ghost small" onclick={() => (addingFolder = true)}>+ Folder</button>{/if}
        {#if addingFolder}
          <form class="newfolder" onsubmit={newFolder}>
            <input aria-label="Folder name" placeholder="Folder name" bind:value={folderName} maxlength="120" required />
            <button class="primary small" type="submit" disabled={busy || !folderName.trim()}>Add</button>
            <button class="small" type="button" onclick={() => { addingFolder = false; folderName = ''; }}>Cancel</button>
          </form>
        {/if}
      </div>
      <div class="seg" role="group" aria-label="View">
        <button class="small" class:on={view === 'cards'} aria-pressed={view === 'cards'} onclick={() => setView('cards')}>Cards</button>
        <button class="small" class:on={view === 'list'} aria-pressed={view === 'list'} onclick={() => setView('list')}>List</button>
      </div>
    </div>
  {:else}
    <div class="spread page-head">
      <div>
        <h1 title={q}>
          {#if q}{#if !loading && list.length > 0}{list.length} {list.length === 1 ? 'result' : 'results'} for “{q}”{:else}Results for “{q}”{/if}{:else if statusFilter === 'draft'}Drafts{:else if reviewOnly}Needs review{:else}All guides{/if}
        </h1>
        {#if statusFilter === 'draft' && !loading}<p class="muted small">{list.length} {list.length === 1 ? 'draft' : 'drafts'}</p>{/if}
        {#if reviewOnly && !loading}<p class="muted small">{list.length} {list.length === 1 ? 'guide is' : 'guides are'} past the review date in workspaces you edit.</p>{/if}
      </div>
    </div>
  {/if}

  {#if error}<p class="error" role="alert">{error}</p>{/if}

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
  {:else if wsId && !current && !q && !reviewOnly && !statusFilter}
    <div class="card empty">
      <p><strong>Workspace not available</strong></p>
      <p class="muted">It may have been removed, or you don't have access to it. <a href="#/">Back to all guides</a></p>
    </div>
  {:else if inWorkspace && current}
    {#if guides.length === 0 && folders.length === 0}
      <div class="card empty">
        <p><strong>No guides here yet.</strong></p>
        <p class="muted">
          {#if canEdit}Create one with <em>New guide</em>, then paste or upload screenshots. The desktop recorder and browser extension will add guides here automatically.{:else}Nothing has been published here yet.{/if}
        </p>
      </div>
    {:else if view === 'list'}
      {#if selectedGuides.length === 0}
        <div class="card empty"><p><strong>No guides in this folder.</strong></p><p class="muted">{canEdit ? 'Create one with New guide.' : 'Nothing has been published here yet.'}</p></div>
      {:else}
        <GuideTable guides={selectedGuides} showReview={canEditGuide} />
      {/if}
    {:else}
      {#each sections as s (s.id)}
        <section class="fsec" aria-labelledby={`fs-${s.id}`}>
          <div class="fhead">
            <h2 id={`fs-${s.id}`} title={s.name}>{s.name}</h2>
            <span class="muted mono small">{s.guides.length}</span>
            <span class="rule"></span>
          </div>
          {#if s.guides.length === 0 && !canEdit}
            <p class="muted small">No guides in this folder yet.</p>
          {:else}
            <div class="cards">
              {#each s.guides as g (g.id)}<GuideCard guide={g} showReview={canEditGuide(g)} />{/each}
              {#if canEdit}
                <button class="newcard" onclick={() => newGuide(s.folderId)} disabled={busy}>
                  <span aria-hidden="true">+</span>
                  <span class="nl">New guide in {s.name}</span>
                </button>
              {/if}
            </div>
          {/if}
        </section>
      {/each}
    {/if}
  {:else if list.length === 0}
    <div class="card empty">
      {#if q}
        <p><strong>No results for “{q}”</strong></p>
        <p class="muted">Try different words, or search by app or step name.</p>
      {:else if statusFilter === 'draft'}
        <p><strong>No drafts</strong></p>
        <p class="muted">Drafts you or your team start will appear here.</p>
      {:else if reviewOnly}
        <p><strong>Nothing needs review</strong></p>
        <p class="muted">No guides need review.</p>
      {:else}
        <p><strong>No guides here yet.</strong></p>
        <p class="muted">Nothing has been published here yet. Editors can create guides from a workspace, or record one with the desktop recorder or browser extension.</p>
      {/if}
    </div>
  {:else}
    {#if isAll}
      <div class="summary">
        <div class="stat"><span class="num mono">{counts.published}</span><span class="muted small">Published</span></div>
        <div class="stat"><span class="num mono">{counts.drafts}</span><span class="muted small">Drafts</span></div>
        {#if canSeeReview}<a class="stat" href="#/?needs_review=true"><span class="num mono">{workspacesStore.reviewCount}</span><span class="muted small">Need review</span></a>{/if}
      </div>
    {/if}
    <GuideTable guides={list} wsName={wsName} showReview={canEditGuide} />
  {/if}
</section>

<style>
  .ws-head { margin-bottom: 1rem; }
  .ws-title { min-width: 0; flex: 1 1 14rem; }
  .ws-title h1 { margin: 0; overflow: hidden; text-overflow: ellipsis; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow-wrap: break-word; }
  .ws-title p { margin: 0.15rem 0 0; }
  .desc { margin: 0.5rem 0 0; max-width: 70ch; }
  .crumb { margin-bottom: 0.4rem; }
  .page-head h1 { margin: 0; }
  .page-head p { margin: 0.15rem 0 0; }
  .access { display: flex; flex-wrap: wrap; align-items: center; gap: 0.4rem; margin-top: 0.75rem; }
  .chip { display: inline-flex; align-items: center; gap: 0.35rem; max-width: 100%; padding: 0.1rem 0.5rem; border: 1px solid var(--line); border-radius: 999px; background: var(--card); font-size: 0.82rem; min-width: 0; }
  .cname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 14rem; }
  .role { font-size: 0.68rem; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted-strong); }
  .chip.more { color: var(--muted-strong); }
  .ms { flex: none; }

  .toolbar { display: flex; justify-content: space-between; align-items: flex-start; gap: 0.75rem; margin: 1rem 0 0.5rem; flex-wrap: wrap; }
  .chips { display: flex; flex-wrap: wrap; align-items: center; gap: 0.4rem; min-width: 0; flex: 1; }
  .fchip { display: inline-flex; align-items: center; gap: 0.4rem; padding: 0.2rem 0.7rem; border: 1px solid var(--line); border-radius: 999px; background: var(--card); color: var(--text); font-size: 0.88rem; max-width: 100%; min-width: 0; }
  .fchip:hover { border-color: var(--muted); text-decoration: none; }
  .fchip.on { background: var(--btn-primary); border-color: var(--btn-primary); color: var(--btn-primary-text); }
  .fchip .n { font-size: 0.75rem; opacity: 0.8; }
  .fname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 14rem; }
  .newfolder { display: flex; gap: 0.4rem; align-items: center; flex-wrap: wrap; }
  .newfolder input { width: 12rem; padding: 0.2rem 0.5rem; }
  .seg { display: inline-flex; border: 1px solid var(--line); border-radius: var(--radius); overflow: hidden; flex: none; }
  .seg button { border: 0; border-radius: 0; background: var(--card); }
  .seg button.on { background: var(--btn-primary); color: var(--btn-primary-text); }
  .seg button:focus-visible { outline-offset: -2px; }

  .fsec { margin-top: 1.25rem; }
  .fhead { display: flex; align-items: center; gap: 0.6rem; margin-bottom: 0.7rem; min-width: 0; }
  .fhead h2 { margin: 0; font-size: 1rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .rule { flex: 1; height: 1px; background: var(--line); min-width: 1rem; }
  .cards { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 0.9rem; }
  .newcard {
    flex-direction: column;
    min-height: 140px;
    border: 1.5px dashed var(--line);
    background: transparent;
    color: var(--muted-strong);
    border-radius: var(--radius-lg);
    white-space: normal;
    text-align: center;
    padding: 0.75rem;
    overflow-wrap: anywhere;
  }
  .newcard:hover:not(:disabled), .newcard:focus-visible { border-color: var(--sig); color: var(--sig-text); outline: 2px solid var(--sig); outline-offset: 0; }
  .newcard span[aria-hidden] { font-size: 1.4rem; line-height: 1; }
  .nl { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  @media (max-width: 1179px) { .cards { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @media (max-width: 599px) { .cards { grid-template-columns: minmax(0, 1fr); } }

  .summary { display: flex; gap: 0.75rem; margin: 0.75rem 0 1rem; flex-wrap: wrap; }
  .stat { display: flex; flex-direction: column; min-width: 8rem; padding: 0.6rem 1rem; background: var(--card); border: 1px solid var(--line); border-radius: var(--radius-lg); color: var(--text); }
  a.stat:hover { border-color: var(--sig); text-decoration: none; }
  .stat .num { font-size: 1.5rem; font-weight: 500; line-height: 1.2; }
  .empty { margin-top: 1rem; }
</style>
