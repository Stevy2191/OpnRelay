<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { api } from './api';
  import { route, query, match, go } from './router.svelte';
  import { session, logout, isAdmin, canAudit, workspacesStore, shellUi } from './session.svelte';
  import { errorText } from './format';
  import RecordPanel from './RecordPanel.svelte';

  let { children }: { children: Snippet } = $props();

  let me = $derived(session.me!);
  let params = $derived(query(route.path));
  let onHome = $derived(match('/', route.path) !== null);
  let wsId = $derived(onHome ? (params.get('ws') ?? '') : '');
  let q = $derived(onHome ? (params.get('q') ?? '') : '');
  let status = $derived(onHome ? (params.get('status') ?? '') : '');
  let reviewOnly = $derived(onHome && params.get('needs_review') === 'true');
  let path = $derived(route.path.split('?')[0]);
  let adminTab = $derived(params.get('tab') ?? '');

  let canEditSomewhere = $derived(isAdmin(me) || workspacesStore.list.some((w) => w.my_permission === 'edit' || w.my_permission === 'manage'));
  let reviewCount = $derived(workspacesStore.reviewCount);

  onMount(() => {
    workspacesStore.refresh().catch(() => {});
  });
  // Refetch the review count only when the workspace list or the page changes
  // (not the query string), and after guides are reviewed/published.
  $effect(() => {
    void workspacesStore.list;
    void path;
    workspacesStore.refreshReviewCount();
  });

  // Close the drawer on navigation.
  $effect(() => {
    void route.path;
    shellUi.drawer = false;
  });

  // Phone drawer: focus moves in when it opens and back to the menu button when it closes.
  let menuBtn: HTMLButtonElement | undefined = $state();
  let sidebarEl: HTMLElement | undefined = $state();
  let drawerWasOpen = false;
  $effect(() => {
    const open = shellUi.drawer;
    if (open === drawerWasOpen) return;
    drawerWasOpen = open;
    if (!menuBtn || menuBtn.offsetParent === null) return; // wide layout: sidebar is always visible
    if (open) {
      queueMicrotask(() => sidebarEl?.querySelector<HTMLElement>('.sb-nav a')?.focus());
    } else {
      const a = document.activeElement;
      if (!a || a === document.body || sidebarEl?.contains(a)) menuBtn.focus();
    }
  });

  // Search box mirrors the query.
  let search = $state('');
  $effect(() => {
    search = q;
  });
  let searchEl: HTMLInputElement | undefined = $state();
  function submitSearch(e: Event) {
    e.preventDefault();
    go(search.trim() ? `/?q=${encodeURIComponent(search.trim())}` : '/');
  }

  // User menu
  let menuOpen = $state(false);
  let menuEl: HTMLDivElement | undefined = $state();
  function onWindowClick(e: MouseEvent) {
    if (menuOpen && menuEl && !menuEl.contains(e.target as Node)) menuOpen = false;
  }
  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (menuOpen) {
        menuOpen = false;
        return;
      }
      if (shellUi.drawer) shellUi.drawer = false;
      return;
    }
    if (e.key === '/' && !e.ctrlKey && !e.metaKey && !e.altKey) {
      const t = e.target as HTMLElement | null;
      const typing = t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable);
      if (typing || document.querySelector('dialog[open]')) return;
      e.preventDefault();
      searchEl?.focus();
      searchEl?.select();
    }
  }

  // Modals (native dialog: focus trap and Escape for free)
  let recordDlg: HTMLDialogElement | undefined = $state();
  let wsDlg: HTMLDialogElement | undefined = $state();
  $effect(() => {
    if (!recordDlg) return;
    if (shellUi.record && !recordDlg.open) recordDlg.showModal();
    else if (!shellUi.record && recordDlg.open) recordDlg.close();
  });
  $effect(() => {
    if (!wsDlg) return;
    if (shellUi.newWorkspace && !wsDlg.open) wsDlg.showModal();
    else if (!shellUi.newWorkspace && wsDlg.open) wsDlg.close();
  });

  let wsName = $state('');
  let wsError = $state('');
  let wsBusy = $state(false);
  async function createWorkspace(e: Event) {
    e.preventDefault();
    if (!wsName.trim() || wsBusy) return;
    wsBusy = true;
    wsError = '';
    try {
      const ws = await api<{ id: string }>('/workspaces', { body: { name: wsName.trim() } });
      await workspacesStore.refresh();
      wsName = '';
      shellUi.newWorkspace = false;
      go(`/?ws=${ws.id}`);
    } catch (err) {
      wsError = errorText(err);
    } finally {
      wsBusy = false;
    }
  }

  function wsHref(id: string) {
    return `#/?ws=${id}`;
  }
</script>

<svelte:window onkeydown={onKeydown} onclick={onWindowClick} />

<div class="shell">
  <a class="skip" href="#main-content" onclick={(e) => { e.preventDefault(); document.getElementById('main-content')?.focus(); }}>Skip to content</a>

  {#if shellUi.drawer}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="backdrop" onclick={() => (shellUi.drawer = false)}></div>
  {/if}

  <aside bind:this={sidebarEl} class="sidebar" class:open={shellUi.drawer} id="sidebar" aria-label="Main">
    <a class="sb-brand" href="#/" title={me.organization.name}>
      {#if session.branding?.logo_media_id}
        <img class="logo" src={`/api/v1/media/${session.branding.logo_media_id}`} alt="" />
      {:else}
        <img class="mark" src="/favicon.svg" alt="" />
      {/if}
      <span class="org">{me.organization.name}</span>
    </a>

    <nav class="sb-nav">
      <h2 class="sb-h">Library</h2>
      <ul>
        <li><a class:active={onHome && !wsId && !q && !status && !reviewOnly} href="#/" aria-current={onHome && !wsId && !q && !status && !reviewOnly ? 'page' : undefined}><span class="lbl">All guides</span></a></li>
        <li><a class:active={status === 'draft' && !reviewOnly} href="#/?status=draft" aria-current={status === 'draft' && !reviewOnly ? 'page' : undefined}><span class="lbl">Drafts</span></a></li>
        {#if canEditSomewhere}
          <li>
            <a class:active={reviewOnly} href="#/?needs_review=true" aria-current={reviewOnly ? 'page' : undefined}>
              <span class="lbl">Needs review</span><span class="count mono">{reviewCount}</span>
            </a>
          </li>
        {/if}
      </ul>

      <div class="sb-hrow">
        <h2 class="sb-h">Workspaces</h2>
        {#if isAdmin(me)}<button class="sb-new" onclick={() => (shellUi.newWorkspace = true)} title="New workspace">+ New</button>{/if}
      </div>
      <ul>
        {#each workspacesStore.list as w (w.id)}
          <li>
            <a class:active={w.id === wsId && !reviewOnly && !status} href={wsHref(w.id)} title={w.name} aria-current={w.id === wsId && !reviewOnly && !status ? 'page' : undefined}>
              <span class="lbl">{w.name}</span><span class="count mono">{w.guide_count}</span>
            </a>
          </li>
        {:else}
          {#if workspacesStore.loaded}
            <li class="sb-empty">{isAdmin(me) ? 'No workspaces yet.' : 'No workspaces yet. Ask your administrator.'}</li>
          {/if}
        {/each}
      </ul>

      {#if canAudit(me)}
        <h2 class="sb-h">Admin</h2>
        <ul>
          {#if isAdmin(me)}
            <li><a class:active={path === '/admin' && adminTab !== 'policies'} href="#/admin" aria-current={path === '/admin' && adminTab !== 'policies' ? 'page' : undefined}><span class="lbl">People &amp; groups</span></a></li>
          {/if}
          <li><a class:active={path === '/audit'} href="#/audit" aria-current={path === '/audit' ? 'page' : undefined}><span class="lbl">Audit log</span></a></li>
          {#if isAdmin(me)}
            <li><a class:active={path === '/downloads'} href="#/downloads" aria-current={path === '/downloads' ? 'page' : undefined}><span class="lbl">Downloads</span></a></li>
            <li><a class:active={path === '/admin' && adminTab === 'policies'} href="#/admin?tab=policies" aria-current={path === '/admin' && adminTab === 'policies' ? 'page' : undefined}><span class="lbl">Settings</span></a></li>
          {/if}
        </ul>
      {/if}
    </nav>

    <div class="sb-foot">
      <button class="signal record" onclick={() => { shellUi.drawer = false; shellUi.record = true; }}>
        <span aria-hidden="true">●</span> Record a guide
      </button>
    </div>
  </aside>

  <div class="col">
    <header class="topbar">
      <button bind:this={menuBtn} class="ghost menu" aria-label="Open menu" aria-controls="sidebar" aria-expanded={shellUi.drawer} onclick={() => (shellUi.drawer = !shellUi.drawer)}>
        <svg width="20" height="20" viewBox="0 0 20 20" aria-hidden="true"><path d="M3 5h14M3 10h14M3 15h14" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" /></svg>
      </button>
      <form class="search" onsubmit={submitSearch} role="search">
        <input bind:this={searchEl} type="search" placeholder="Search guides, steps, apps…" bind:value={search} aria-label="Search guides" aria-keyshortcuts="/" />
        <span class="kbd hint" aria-hidden="true">/</span>
      </form>
      <div class="user" bind:this={menuEl}>
        <button class="ghost who" aria-haspopup="menu" aria-expanded={menuOpen} onclick={() => (menuOpen = !menuOpen)} title={me.email}>
          <span class="avatar" aria-hidden="true">{(me.display_name || me.email).trim().charAt(0).toUpperCase()}</span>
          <span class="uname">{me.display_name}</span>
          <svg width="10" height="6" viewBox="0 0 10 6" aria-hidden="true"><path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" /></svg>
        </button>
        {#if menuOpen}
          <div class="menu-pop" role="menu">
            <div class="who-info"><strong>{me.display_name}</strong><span class="muted small">{me.email}</span></div>
            <a role="menuitem" href="#/account" onclick={() => (menuOpen = false)}>Account</a>
            <button role="menuitem" class="ghost" onclick={() => { menuOpen = false; logout(); }}>Sign out</button>
          </div>
        {/if}
      </div>
    </header>
    <main id="main-content" tabindex="-1">
      {@render children()}
    </main>
  </div>
</div>

<dialog bind:this={recordDlg} class="modal" aria-labelledby="record-title" onclose={() => (shellUi.record = false)}>
  {#if shellUi.record}<RecordPanel onclose={() => (shellUi.record = false)} />{/if}
</dialog>

<dialog bind:this={wsDlg} class="modal" aria-labelledby="ws-title" onclose={() => (shellUi.newWorkspace = false)}>
  {#if shellUi.newWorkspace}
    <form onsubmit={createWorkspace}>
      <h2 id="ws-title" style="margin-top:0">New workspace</h2>
      <p class="muted small">Workspaces hold guides and decide who can see them, for example one per team (HR, EMS, IT).</p>
      <label for="ws-name">Workspace name</label>
      <input id="ws-name" bind:value={wsName} maxlength="120" required />
      {#if wsError}<p class="error">{wsError}</p>{/if}
      <div class="row end">
        <button type="button" onclick={() => (shellUi.newWorkspace = false)}>Cancel</button>
        <button class="primary" type="submit" disabled={wsBusy || !wsName.trim()}>Create workspace</button>
      </div>
    </form>
  {/if}
</dialog>

<style>
  .shell { display: flex; min-height: 100vh; }
  .col { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .skip { position: absolute; left: -999px; top: 0; z-index: 100; background: var(--card); padding: 0.5rem 1rem; }
  .skip:focus { left: 0.5rem; top: 0.5rem; }

  /* Sidebar */
  .sidebar {
    width: 232px;
    flex: none;
    background: var(--ink);
    color: #e6ebf3;
    display: flex;
    flex-direction: column;
    position: sticky;
    top: 0;
    height: 100vh;
    z-index: 30;
  }
  .sb-brand { display: flex; align-items: center; gap: 0.6rem; padding: 0 1rem; height: 56px; flex: none; color: #fff; font-weight: 700; min-width: 0; border-bottom: 1px solid rgba(255, 255, 255, 0.07); }
  .sb-brand:hover { text-decoration: none; }
  .sb-brand .mark { width: 26px; height: 26px; flex: none; }
  .sb-brand .logo { width: auto; max-width: 120px; height: 26px; object-fit: contain; flex: none; }
  .org { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .sb-nav { flex: 1; overflow-y: auto; padding: 0.75rem 0 1rem; }
  .sb-h {
    font-size: 0.68rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: #9aa8c0;
    margin: 1rem 0 0.35rem;
    padding: 0 1rem;
  }
  .sb-hrow { display: flex; align-items: center; justify-content: space-between; padding-right: 0.75rem; }
  .sb-new { padding: 0.1rem 0.4rem; font-size: 0.78rem; margin-top: 0.65rem; background: transparent; border-color: transparent; color: #cfd8e8; }
  .sb-new:hover:not(:disabled) { background: var(--ink-2); border-color: transparent; color: #fff; }
  ul { list-style: none; margin: 0; padding: 0; }
  .sb-nav a {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.42rem 1rem;
    color: #cfd8e8;
    font-size: 0.92rem;
    min-width: 0;
    box-shadow: inset 2px 0 0 transparent;
  }
  .sb-nav a:hover { background: var(--ink-2); color: #fff; text-decoration: none; }
  .sb-nav a.active { background: var(--ink-2); color: #fff; font-weight: 500; box-shadow: inset 2px 0 0 var(--sig); }
  .lbl { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .count { font-size: 0.75rem; color: #9aa8c0; flex: none; }
  .sb-empty { padding: 0.3rem 1rem; font-size: 0.82rem; color: #9aa8c0; }
  .sb-foot { padding: 0.75rem 1rem; border-top: 1px solid rgba(255, 255, 255, 0.07); flex: none; }
  .record { width: 100%; padding: 0.55rem 0.9rem; }
  .sidebar :focus-visible { outline-offset: -2px; }

  /* Top bar */
  .topbar {
    height: 56px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0 1.25rem;
    background: var(--card);
    border-bottom: 1px solid var(--line);
    position: sticky;
    top: 0;
    z-index: 20;
  }
  .menu { display: none; padding: 0.35rem; }
  .search { flex: 1; max-width: 520px; position: relative; }
  .search input { padding-right: 2.2rem; }
  .hint { position: absolute; right: 0.5rem; top: 50%; transform: translateY(-50%); pointer-events: none; }
  .user { margin-left: auto; position: relative; flex: none; }
  .who { gap: 0.5rem; }
  .avatar { width: 24px; height: 24px; border-radius: 50%; background: var(--ink); color: #fff; display: grid; place-items: center; font-size: 0.78rem; font-weight: 600; }
  .uname { max-width: 160px; overflow: hidden; text-overflow: ellipsis; }
  .menu-pop {
    position: absolute;
    right: 0;
    top: calc(100% + 6px);
    min-width: 220px;
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-pop);
    padding: 0.4rem;
    display: flex;
    flex-direction: column;
    z-index: 40;
  }
  .who-info { display: flex; flex-direction: column; padding: 0.4rem 0.6rem 0.5rem; border-bottom: 1px solid var(--line); margin-bottom: 0.3rem; overflow-wrap: anywhere; }
  .menu-pop a, .menu-pop button { padding: 0.45rem 0.6rem; border-radius: var(--radius); color: var(--text); justify-content: flex-start; text-align: left; }
  .menu-pop a:hover { background: var(--surface-2); text-decoration: none; }

  main { flex: 1; width: 100%; max-width: 1280px; margin: 0 auto; padding: 1.5rem 1.75rem; min-width: 0; }
  main:focus { outline: none; }

  /* Modals */
  .modal { border: 1px solid var(--line); border-radius: var(--radius-lg); background: var(--card); color: var(--text); box-shadow: var(--shadow-pop); padding: 1.5rem; width: min(520px, calc(100vw - 2rem)); max-height: calc(100vh - 2rem); overflow-y: auto; }
  .modal::backdrop { background: rgba(8, 14, 26, 0.55); }
  .end { justify-content: flex-end; margin-top: 1rem; }

  /* Phone / narrow: sidebar becomes a drawer */
  .backdrop { display: none; }
  @media (max-width: 899px) {
    .menu { display: inline-flex; }
    .sidebar {
      position: fixed;
      left: 0;
      top: 0;
      bottom: 0;
      transform: translateX(-100%);
      visibility: hidden;
      transition: transform 0.18s ease, visibility 0.18s;
      box-shadow: var(--shadow-pop);
      width: min(280px, 85vw);
    }
    .sidebar.open { transform: none; visibility: visible; }
    .backdrop { display: block; position: fixed; inset: 0; background: rgba(8, 14, 26, 0.55); z-index: 25; }
    .topbar { padding: 0 0.75rem; }
    .uname { display: none; }
    main { padding: 1rem; }
    .hint { display: none; }
  }
  @media (prefers-reduced-motion: reduce) { .sidebar { transition: none; } }
</style>
