<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { session, refresh } from '../lib/session.svelte';
  import { errorText, when } from '../lib/format';

  type SessionInfo = { id: string; ip: string | null; user_agent: string | null; created_at: string; last_seen_at: string; current: boolean };

  type Device = { id: string; client_id: string; device_name: string; ip: string | null; last_used_at: string };
  let sessions = $state<SessionInfo[]>([]);
  let devices = $state<Device[]>([]);
  let pw = $state({ current: '', next: '', confirm: '' });
  let pwMsg = $state('');
  let pwErr = $state('');

  let enrol = $state<{ factor_id: string; secret: string; qr_svg: string } | null>(null);
  let code = $state('');
  let recovery = $state<string[]>([]);
  let mfaErr = $state('');

  async function loadSessions() {
    try {
      [sessions, devices] = await Promise.all([api<SessionInfo[]>('/me/sessions'), api<Device[]>('/me/devices')]);
    } catch {
      sessions = [];
    }
  }

  async function disconnect(id: string) {
    await api(`/me/devices/${id}`, { method: 'DELETE' });
    loadSessions();
  }
  onMount(loadSessions);

  async function changePassword(e: Event) {
    e.preventDefault();
    pwMsg = pwErr = '';
    if (pw.next !== pw.confirm) {
      pwErr = 'The new passwords do not match.';
      return;
    }
    try {
      await api('/me/password', { method: 'PUT', body: { current_password: pw.current, new_password: pw.next } });
      pw = { current: '', next: '', confirm: '' };
      pwMsg = 'Password changed. Other devices were signed out.';
      loadSessions();
    } catch (err) {
      pwErr = errorText(err);
    }
  }

  async function startEnrol() {
    mfaErr = '';
    recovery = [];
    try {
      enrol = await api('/me/mfa/totp', { body: {} });
    } catch (e) {
      mfaErr = errorText(e);
    }
  }

  async function confirmEnrol(e: Event) {
    e.preventDefault();
    mfaErr = '';
    try {
      const r = await api<{ recovery_codes: string[] }>('/me/mfa/totp/confirm', { body: { factor_id: enrol!.factor_id, code } });
      recovery = r.recovery_codes;
      enrol = null;
      code = '';
      await refresh();
    } catch (err) {
      mfaErr = errorText(err);
    }
  }

  let regen = $state(false);
  let regenPw = $state('');
  async function regenerate(e: Event) {
    e.preventDefault();
    mfaErr = '';
    try {
      recovery = (await api<{ recovery_codes: string[] }>('/me/mfa/recovery-codes', { body: { password: regenPw } })).recovery_codes;
      regen = false;
      regenPw = '';
      await refresh();
    } catch (err) {
      mfaErr = errorText(err);
    }
  }

  async function revoke(id: string) {
    await api(`/me/sessions/${id}`, { method: 'DELETE' });
    loadSessions();
  }

  function downloadCodes() {
    const blob = new Blob([`OpnRelay recovery codes for ${session.me?.email}\n\n${recovery.join('\n')}\n`], { type: 'text/plain' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'opnrelay-recovery-codes.txt';
    a.click();
    URL.revokeObjectURL(a.href);
  }
</script>

<div class="account stack">
  <h1>Your account</h1>
  <div class="card">
    <p><strong>{session.me?.display_name}</strong> · {session.me?.email}</p>
    <p class="muted small">Role: <span class="rolepill {session.me?.org_role}">{session.me?.org_role}</span></p>
  </div>

  <div class="card">
    <h2>Two-factor authentication</h2>
    {#if recovery.length}
      <div class="callout ok">
        <p><strong>Save these recovery codes somewhere safe.</strong> Each code works once if you lose your phone. They will not be shown again.</p>
        <pre class="codes">{recovery.join('\n')}</pre>
        <button class="small" onclick={downloadCodes}>Download as text file</button>
      </div>
    {/if}
    {#if session.me?.mfa_enabled && !enrol}
      <p><span class="tag ok">Enabled</span> Authenticator app. <span class="mono">{session.me.recovery_codes_remaining}</span> recovery codes left.</p>
      {#if regen}
        <form class="stack" onsubmit={regenerate}>
          <label for="regen-pw">Confirm your password to generate new recovery codes</label>
          <input id="regen-pw" type="password" autocomplete="current-password" required bind:value={regenPw} />
          <div class="row"><button class="primary" type="submit">Generate new codes</button><button type="button" onclick={() => { regen = false; regenPw = ''; }}>Cancel</button></div>
        </form>
      {:else}
        <button onclick={() => (regen = true)}>New recovery codes</button>
      {/if}
    {:else if enrol}
      <form class="stack" onsubmit={confirmEnrol}>
        <p>1. Scan this QR code with an authenticator app (Microsoft Authenticator, Google Authenticator, 1Password, Aegis…).</p>
        <div class="qr">{@html enrol.qr_svg}</div>
        <p class="muted small">Can't scan? Enter this key manually: <code>{enrol.secret}</code></p>
        <label for="code">2. Enter the 6-digit code the app shows</label>
        <input id="code" inputmode="numeric" autocomplete="one-time-code" maxlength="6" required bind:value={code} />
        <div class="row"><button class="primary" type="submit">Turn on</button><button type="button" onclick={() => (enrol = null)}>Cancel</button></div>
      </form>
    {:else}
      <p class="muted">Protect your account with a code from your phone in addition to your password.</p>
      <button class="primary" onclick={startEnrol}>Set up two-factor authentication</button>
    {/if}
    {#if mfaErr}<p class="callout error" role="alert">{mfaErr}</p>{/if}
  </div>

  {#if !session.me?.mfa_setup_required}
    <form class="card" onsubmit={changePassword}>
      <h2>Change password</h2>
      <label for="cur">Current password</label>
      <input id="cur" type="password" autocomplete="current-password" required bind:value={pw.current} />
      <label for="new">New password</label>
      <input id="new" type="password" autocomplete="new-password" minlength="12" required bind:value={pw.next} />
      <label for="conf">Confirm new password</label>
      <input id="conf" type="password" autocomplete="new-password" required bind:value={pw.confirm} />
      <p><button class="primary" type="submit">Change password</button></p>
      {#if pwMsg}<p class="callout ok" role="status">{pwMsg}</p>{/if}
      {#if pwErr}<p class="callout error" role="alert">{pwErr}</p>{/if}
    </form>

    <div class="card">
      <h2>Signed-in devices</h2>
      <div class="table-wrap">
        <table class="data stackable">
          <thead><tr><th>Device</th><th>IP</th><th>Last active</th><th></th></tr></thead>
          <tbody>
            {#each sessions as s (s.id)}
              <tr>
                <td class="small" data-label="Device">{s.user_agent ?? 'Unknown'} {#if s.current}<span class="tag pub">this device</span>{/if}</td>
                <td class="mono small" data-label="IP">{s.ip ?? ''}</td>
                <td class="small" data-label="Last active">{when(s.last_seen_at)}</td>
                <td>{#if !s.current}<button class="small" onclick={() => revoke(s.id)}>Sign out</button>{/if}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>

    <div class="card">
      <h2>Connected apps</h2>
      {#if devices.length === 0}
        <p class="muted">No desktop recorder or browser extension is connected.</p>
      {:else}
        <div class="table-wrap">
          <table class="data stackable">
            <thead><tr><th>App</th><th>Device</th><th>Last used</th><th></th></tr></thead>
            <tbody>
              {#each devices as d (d.id)}
                <tr>
                  <td class="small" data-label="App">{d.client_id === 'opnrelay-extension' ? 'Browser extension' : 'Desktop recorder'}</td>
                  <td class="small" data-label="Device">{d.device_name || '—'}</td>
                  <td class="small" data-label="Last used">{when(d.last_used_at)}</td>
                  <td><button class="small" onclick={() => disconnect(d.id)}>Disconnect</button></td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .account { max-width: 720px; margin: 0 auto; }
  .account h2 { margin-top: 0; }
  .qr :global(svg) { width: 200px; height: 200px; background: #fff; padding: 8px; border-radius: 8px; }
  .codes { font-family: var(--font-mono); font-size: 1rem; background: var(--card); color: var(--text); padding: 0.75rem; border-radius: 8px; columns: 2; margin: 0.5rem 0; }
</style>
