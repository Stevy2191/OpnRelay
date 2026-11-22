<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- Consent page opened by the desktop recorder / browser extension sign-in. -->
<script lang="ts">
  import { api } from '../lib/api';
  import { route, query } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';
  import { errorText } from '../lib/format';

  let p = $derived(query(route.path));
  let clientId = $derived(p.get('client_id') ?? '');
  let device = $derived(p.get('device_name') ?? '');
  let appName = $derived(
    clientId === 'opnrelay-extension' ? 'OpnRelay browser extension' : clientId === 'opnrelay-desktop' ? 'OpnRelay desktop recorder' : 'Unknown app',
  );
  let error = $state('');
  let busy = $state(false);
  let done = $state(false);

  async function allow() {
    busy = true;
    error = '';
    try {
      const r = await api<{ redirect_to: string }>('/oauth/authorize', {
        body: {
          client_id: clientId,
          redirect_uri: p.get('redirect_uri') ?? '',
          code_challenge: p.get('code_challenge') ?? '',
          code_challenge_method: p.get('code_challenge_method') ?? '',
          state: p.get('state') ?? '',
          device_name: device,
        },
      });
      done = true;
      window.location.href = r.redirect_to;
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="connect card stack">
  <div class="brand"><img src="/favicon.svg" alt="" /> OpnRelay</div>
  <h1>Connect {appName}?</h1>
  {#if device}<p class="muted">Device: <strong>{device}</strong></p>{/if}
  <p>
    It will be able to create guides and upload screenshots as <strong>{session.me?.display_name}</strong> ({session.me?.email}) in the
    workspaces you can edit. You can disconnect it at any time under <a href="#/account">Account</a>.
  </p>
  {#if error}<p class="callout error" role="alert">{error}</p>{/if}
  {#if done}
    <p class="callout ok" role="status">Connected. You can close this window and return to the app.</p>
  {:else}
    <div class="row">
      <button class="primary" onclick={allow} disabled={busy || clientId === ''}>Allow</button>
      <a class="btn" href="#/">Cancel</a>
    </div>
  {/if}
</div>

<style>
  .connect { max-width: 520px; margin: 3rem auto; padding: 1.5rem; }
  .connect h1 { margin: 0; }
</style>
