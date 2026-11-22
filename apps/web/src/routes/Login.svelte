<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { api } from '../lib/api';
  import { refresh } from '../lib/session.svelte';
  import { go } from '../lib/router.svelte';
  import { errorText } from '../lib/format';
  import { onMount } from 'svelte';
  import MsMark from '../lib/MsMark.svelte';
  import { route, query } from '../lib/router.svelte';

  let providers = $state<{ microsoft: boolean; password: boolean }>({ microsoft: false, password: true });
  onMount(async () => {
    try {
      providers = await api('/auth/providers');
    } catch {
      /* keep defaults */
    }
  });

  function microsoft() {
    let next = '/';
    try {
      next = sessionStorage.getItem('after_login') || '/';
      sessionStorage.removeItem('after_login');
    } catch { /* storage unavailable */ }
    window.location.href = `/api/v1/auth/sso/microsoft/start?return_to=${encodeURIComponent(next)}`;
  }

  let email = $state('');
  let password = $state('');
  let code = $state('');
  let step = $state<'password' | 'mfa'>('password');
  let error = $state(ssoErrorInit());
  function ssoErrorInit() {
    const e = new URLSearchParams(window.location.hash.split('?')[1] ?? '').get('error');
    return e ?? '';
  }
  let busy = $state(false);

  async function submit(e: Event) {
    e.preventDefault();
    error = '';
    busy = true;
    try {
      if (step === 'password') {
        const r = await api<{ mfa_required: boolean }>('/auth/login', { body: { email, password } });
        password = '';
        if (r.mfa_required) {
          step = 'mfa';
          return;
        }
      } else {
        await api('/auth/mfa/verify', { body: { code } });
      }
      await refresh();
      let next = '/';
      try {
        next = sessionStorage.getItem('after_login') || '/';
        sessionStorage.removeItem('after_login');
      } catch { /* storage unavailable */ }
      go(next.startsWith('/') ? next : '/');
    } catch (err) {
      const msg = errorText(err);
      error = msg === 'authentication required' ? (step === 'mfa' ? 'That code is not valid.' : 'Incorrect email or password.') : msg;
      if (step === 'mfa' && msg.includes('too many')) step = 'password';
    } finally {
      busy = false;
    }
  }
</script>

<main class="auth-page">
  <form class="card stack" onsubmit={submit}>
    <div class="brand"><img src="/favicon.svg" alt="" /> OpnRelay</div>
    {#if step === 'password' && providers.microsoft}
      <button type="button" class="btn-ms" onclick={microsoft}><MsMark /> Sign in with Microsoft</button>
      {#if providers.password}<p class="muted small or">or use your OpnRelay password</p>{/if}
    {/if}
    {#if step === 'password'}
      <div>
        <label for="email">Email</label>
        <input id="email" type="email" autocomplete="username" required bind:value={email} />
        <label for="password">Password</label>
        <input id="password" type="password" autocomplete="current-password" required bind:value={password} />
      </div>
    {:else}
      <div>
        <p>Enter the 6-digit code from your authenticator app, or one of your recovery codes.</p>
        <label for="code">Verification code</label>
        <!-- svelte-ignore a11y_autofocus -->
        <input id="code" autocomplete="one-time-code" inputmode="text" required autofocus bind:value={code} />
      </div>
    {/if}
    {#if error}<p class="callout error" role="alert">{error}</p>{/if}
    <button class="primary" type="submit" disabled={busy}>{step === 'password' ? 'Sign in' : 'Verify'}</button>
    <p class="muted small">Forgot your password? Ask an administrator for a reset link.</p>
  </form>
</main>

<style>
  .or { text-align: center; margin: 0.25rem 0; }
</style>
