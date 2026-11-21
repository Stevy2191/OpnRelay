<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { errorText, dateTime } from '../lib/format';

  type Settings = {
    enabled: boolean; schedule_hour: number; keep_local: number; onedrive_enabled: boolean; onedrive_folder: string;
    keep_remote: number; onedrive_account: string | null; last_run_at: string | null; last_status: string | null;
    last_error: string | null; last_file: string | null; last_size: number | null; last_remote_at: string | null;
  };
  let s = $state<Settings | null>(null);
  let files = $state<{ name: string; size: number }[]>([]);
  let msReady = $state(false);
  let error = $state('');
  let notice = $state('');
  let running = $state(false);

  const mb = (n: number | null) => (n == null ? '' : `${(n / 1024 / 1024).toFixed(1)} MB`);

  async function load() {
    const r = await api<{ settings: Settings; files: typeof files; microsoft_app_configured: boolean }>('/admin/backups');
    s = r.settings;
    files = r.files;
    msReady = r.microsoft_app_configured;
  }

  onMount(() => {
    load().catch((e) => (error = errorText(e)));
    if (location.hash.includes('onedrive=connected')) notice = 'OneDrive connected. Backups will be copied there.';
  });

  async function save(e: Event) {
    e.preventDefault();
    if (!s) return;
    error = notice = '';
    try {
      s = await api<Settings>('/admin/backups/settings', { method: 'PUT', body: s });
      notice = 'Saved';
    } catch (err) {
      error = errorText(err);
    }
  }

  async function runNow() {
    error = notice = '';
    running = true;
    const before = s?.last_run_at;
    try {
      await api('/admin/backups/run', { body: {} });
      for (let i = 0; i < 600 && s?.last_run_at === before; i++) {
        await new Promise((r) => setTimeout(r, 1000));
        await load();
      }
      notice = s?.last_status === 'ok' ? 'Backup finished.' : '';
    } catch (err) {
      error = errorText(err);
    } finally {
      running = false;
    }
  }

  async function disconnect() {
    if (!confirm('Disconnect OneDrive? Existing backups in OneDrive are not deleted.')) return;
    await api('/admin/backups/onedrive', { method: 'DELETE' });
    await load();
  }
</script>

{#if error}<p class="error">{error}</p>{/if}
{#if notice}<p class="success">{notice}</p>{/if}
{#if s}
  <div class="card status">
    <div class="spread">
      <div>
        <strong>Last backup:</strong>
        {#if s.last_run_at}
          {dateTime(s.last_run_at)} ·
          {#if s.last_status === 'ok'}✅ {mb(s.last_size)}{s.last_remote_at && s.onedrive_enabled ? ' · copied to OneDrive' : ''}{:else}❌ {s.last_error}{/if}
        {:else}never{/if}
      </div>
      <button class="primary" onclick={runNow} disabled={running}>{running ? 'Backing up…' : 'Back up now'}</button>
    </div>
    <p class="muted small">Backups contain every guide, screenshot, user, setting and the audit log. They are encrypted with this server's master key: keep a copy of that key somewhere safe, or backups cannot be restored.</p>
  </div>

  <form class="card" onsubmit={save}>
    <h2>Schedule</h2>
    <label class="inline"><input type="checkbox" bind:checked={s.enabled} /> Back up automatically every day</label>
    <div class="row two">
      <div><label for="h">At (hour, UTC)</label><input id="h" type="number" min="0" max="23" bind:value={s.schedule_hour} /></div>
      <div><label for="kl">Keep on this server (newest)</label><input id="kl" type="number" min="1" max="365" bind:value={s.keep_local} /></div>
    </div>

    <h2>OneDrive</h2>
    {#if !msReady}
      <p class="muted">First set up the Microsoft app under <em>Microsoft sign-in</em> (the same app is used for OneDrive).</p>
    {:else if s.onedrive_account}
      <p>Connected to <strong>{s.onedrive_account}</strong> · <button type="button" class="small" onclick={disconnect}>Disconnect</button></p>
      <label class="inline"><input type="checkbox" bind:checked={s.onedrive_enabled} /> Copy every backup to OneDrive</label>
      <div class="row two">
        <div><label for="f">Folder</label><input id="f" bind:value={s.onedrive_folder} /></div>
        <div><label for="kr">Keep in OneDrive (newest)</label><input id="kr" type="number" min="1" max="3650" bind:value={s.keep_remote} /></div>
      </div>
    {:else}
      <p class="muted small">Sign in with the Microsoft account whose OneDrive should receive the backups (for example a shared IT account).</p>
      <a class="btn" href="/api/v1/admin/backups/onedrive/connect">Connect OneDrive</a>
    {/if}
    <p><button class="primary" type="submit">Save</button></p>
  </form>

  <div class="card">
    <h2>Backups on this server</h2>
    {#if files.length === 0}<p class="muted">None yet.</p>{/if}
    <ul class="files">
      {#each files as f (f.name)}
        <li><a href={`/api/v1/admin/backups/files/${f.name}`} download>{f.name}</a> <span class="muted small">{mb(f.size)}</span></li>
      {/each}
    </ul>
    <p class="muted small">To restore on a new server, run the installer with <code>OPNRELAY_RESTORE</code> (see the deployment guide).</p>
  </div>
{/if}

<style>
  .card + .card, .status + .card { margin-top: 1rem; }
  h2 { font-size: 1.05rem; margin: 0.75rem 0 0.25rem; }
  .two > div { flex: 1; min-width: 180px; }
  .files { list-style: none; padding: 0; }
  .files li { padding: 0.25rem 0; }
</style>
