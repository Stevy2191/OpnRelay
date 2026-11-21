<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { route, match, go } from './lib/router.svelte';
  import { session, refresh, logout, isAdmin, canAudit } from './lib/session.svelte';
  import Login from './routes/Login.svelte';
  import Invite from './routes/Invite.svelte';
  import Home from './routes/Home.svelte';
  import GuideView from './routes/GuideView.svelte';
  import GuideEdit from './routes/GuideEdit.svelte';
  import Account from './routes/Account.svelte';
  import Admin from './routes/Admin.svelte';
  import Audit from './routes/Audit.svelte';
  import WorkspaceSettings from './routes/WorkspaceSettings.svelte';

  onMount(refresh);

  const publicRoutes = ['/login', '/invite/:token'];
  let isPublic = $derived(publicRoutes.some((p) => match(p, route.path)));

  $effect(() => {
    if (session.loading) return;
    if (!session.me && !isPublic) go('/login');
    else if (session.me?.mfa_setup_required && !match('/account', route.path)) go('/account');
  });

  let search = $state('');
  function submitSearch(e: Event) {
    e.preventDefault();
    go(search.trim() ? `/?q=${encodeURIComponent(search.trim())}` : '/');
  }
</script>

{#if session.loading}
  <div class="center-page muted">Loading…</div>
{:else if match('/invite/:token', route.path)}
  <Invite token={match('/invite/:token', route.path)!.token} />
{:else if !session.me}
  <Login />
{:else}
  {@const me = session.me}
  <header class="topbar">
    <a class="brand" href="#/">
      <img src="/favicon.svg" alt="" />
      <span>{me.organization.name}</span>
    </a>
    <form class="search" onsubmit={submitSearch} role="search">
      <input type="search" placeholder="Search guides…" bind:value={search} aria-label="Search guides" />
    </form>
    <nav class="row">
      <a href="#/">Guides</a>
      {#if canAudit(me)}<a href="#/audit">Audit log</a>{/if}
      {#if isAdmin(me)}<a href="#/admin">Admin</a>{/if}
      <a href="#/account" title={me.email}>{me.display_name}</a>
      <button class="small" onclick={logout}>Sign out</button>
    </nav>
  </header>
  <main>
    {#if me.mfa_setup_required}
      <p class="error">Your organisation requires two-factor authentication. Set it up below to continue.</p>
    {/if}
    {#if match('/guides/:id/edit', route.path)}
      {#key route.path}<GuideEdit id={match('/guides/:id/edit', route.path)!.id} />{/key}
    {:else if match('/guides/:id', route.path)}
      {#key route.path}<GuideView id={match('/guides/:id', route.path)!.id} />{/key}
    {:else if match('/workspaces/:id/settings', route.path)}
      <WorkspaceSettings id={match('/workspaces/:id/settings', route.path)!.id} />
    {:else if match('/account', route.path)}
      <Account />
    {:else if match('/admin', route.path)}
      <Admin />
    {:else if match('/audit', route.path)}
      <Audit />
    {:else}
      <Home />
    {/if}
  </main>
{/if}

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.6rem 1.25rem;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    position: sticky;
    top: 0;
    z-index: 10;
    flex-wrap: wrap;
  }
  .brand { color: var(--text); }
  .search { flex: 1; min-width: 180px; max-width: 480px; }
  nav a { color: var(--text); padding: 0.25rem 0.5rem; }
  main { max-width: 1200px; margin: 0 auto; padding: 1.25rem; }
</style>
