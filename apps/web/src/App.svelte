<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { route, match, go } from './lib/router.svelte';
  import { session, refresh } from './lib/session.svelte';
  import Shell from './lib/Shell.svelte';
  import Login from './routes/Login.svelte';
  import Invite from './routes/Invite.svelte';
  import Home from './routes/Home.svelte';
  import GuideView from './routes/GuideView.svelte';
  import GuideEdit from './routes/GuideEdit.svelte';
  import Account from './routes/Account.svelte';
  import Admin from './routes/Admin.svelte';
  import Audit from './routes/Audit.svelte';
  import WorkspaceSettings from './routes/WorkspaceSettings.svelte';
  import Connect from './routes/Connect.svelte';
  import Downloads from './routes/Downloads.svelte';

  onMount(refresh);

  const publicRoutes = ['/login', '/invite/:token'];
  let isPublic = $derived(publicRoutes.some((p) => match(p, route.path)));

  $effect(() => {
    if (session.loading) return;
    if (!session.me && !isPublic) {
      // Come back here after signing in (e.g. the app-connect page).
      if (route.path !== '/' && !match('/login', route.path)) {
        try { sessionStorage.setItem('after_login', route.path); } catch { /* storage unavailable */ }
      }
      go('/login');
    }
    else if (session.me?.mfa_setup_required && !match('/account', route.path)) go('/account');
  });
</script>

{#if session.loading}
  <div class="center-page muted">Loading…</div>
{:else if match('/invite/:token', route.path)}
  <Invite token={match('/invite/:token', route.path)!.token} />
{:else if !session.me}
  <Login />
{:else}
  {@const me = session.me}
  <Shell>
    {#if me.mfa_setup_required}
      <p class="error">Your organisation requires two-factor authentication. Set it up below to continue.</p>
    {/if}
    {#if match('/connect', route.path)}
      <Connect />
    {:else if match('/guides/:id/edit', route.path)}
      {#key route.path}<GuideEdit id={match('/guides/:id/edit', route.path)!.id} />{/key}
    {:else if match('/guides/:id', route.path)}
      {#key route.path}<GuideView id={match('/guides/:id', route.path)!.id} />{/key}
    {:else if match('/workspaces/:id/settings', route.path)}
      <WorkspaceSettings id={match('/workspaces/:id/settings', route.path)!.id} />
    {:else if match('/account', route.path)}
      <Account />
    {:else if match('/downloads', route.path)}
      <Downloads />
    {:else if match('/admin', route.path)}
      <Admin />
    {:else if match('/audit', route.path)}
      <Audit />
    {:else}
      <Home />
    {/if}
  </Shell>
{/if}

