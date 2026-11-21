<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { go } from '../lib/router.svelte';
  import { errorText } from '../lib/format';

  let { token }: { token: string } = $props();
  let info = $state<{ email: string; display_name: string; purpose: string } | null>(null);
  let name = $state('');
  let password = $state('');
  let confirm = $state('');
  let error = $state('');
  let done = $state(false);
  let busy = $state(false);

  onMount(async () => {
    try {
      info = await api('/auth/invite/inspect', { body: { token } });
      name = info!.display_name;
    } catch (e) {
      error = errorText(e);
    }
  });

  async function submit(e: Event) {
    e.preventDefault();
    error = '';
    if (password !== confirm) {
      error = 'The passwords do not match.';
      return;
    }
    busy = true;
    try {
      await api('/auth/invite/accept', { body: { token, password, display_name: name } });
      done = true;
      // Drop the token from the address bar and history.
      history.replaceState(null, '', '#/login');
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="center-page">
  <div class="card stack">
    <div class="brand"><img src="/favicon.svg" alt="" /> OpnRelay</div>
    {#if done}
      <p class="success">Your password is set.</p>
      <button class="primary" onclick={() => go('/login')}>Sign in</button>
    {:else if info}
      <form class="stack" onsubmit={submit}>
        <h1>{info.purpose === 'invite' ? 'Welcome!' : 'Reset your password'}</h1>
        <p class="muted">Account: <strong>{info.email}</strong></p>
        {#if info.purpose === 'invite'}
          <div><label for="name">Your name</label><input id="name" required bind:value={name} /></div>
        {/if}
        <div>
          <label for="pw">New password</label>
          <input id="pw" type="password" autocomplete="new-password" minlength="12" required bind:value={password} />
          <p class="muted small">At least 12 characters. A short sentence is easy to remember.</p>
          <label for="pw2">Confirm password</label>
          <input id="pw2" type="password" autocomplete="new-password" required bind:value={confirm} />
        </div>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <button class="primary" type="submit" disabled={busy}>Set password</button>
      </form>
    {:else if error}
      <p class="error">{error}</p>
      <a href="#/login">Go to sign in</a>
    {:else}
      <p class="muted">Checking your link…</p>
    {/if}
  </div>
</div>
