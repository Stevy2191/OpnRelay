<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type SyncSummary } from '../lib/api';
  import { dateTime, errorText } from '../lib/format';

  type S = {
    configured: boolean; enabled: boolean; tenant_id: string; client_id: string; has_secret: boolean;
    jit_provisioning: boolean; default_role: string; sso_only: boolean; redirect_uri: string;
    group_sync_enabled: boolean; group_sync_last_at: string | null; group_sync_last_ok_at: string | null;
    group_sync_last_error: string | null; group_sync_last_warning: string | null; group_sync_brake_pending: number | null;
  };
  const PERMISSIONS = ['GroupMember.Read.All', 'User.Read.All'];
  let s = $state<S | null>(null);
  let secret = $state('');
  let error = $state('');
  let notice = $state('');
  let syncing = $state(false);
  let summary = $state<SyncSummary | null>(null);

  const stale = $derived(
    !!s && s.group_sync_enabled && (!s.group_sync_last_ok_at || Date.now() - Date.parse(s.group_sync_last_ok_at) > 24 * 3600 * 1000),
  );

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      notice = `Copied ${text}`;
    } catch {
      error = 'Could not copy. Select the text and copy it by hand.';
    }
  }

  async function syncNow(confirmBrake = false) {
    syncing = true;
    error = notice = '';
    try {
      summary = await api<SyncSummary>('/admin/sso/sync', { body: confirmBrake ? { confirm_brake: true } : {} });
      s = await api<S>('/admin/sso');
    } catch (err) {
      error = errorText(err);
    } finally {
      syncing = false;
    }
  }

  onMount(async () => {
    try {
      s = await api<S>('/admin/sso');
    } catch (e) {
      error = errorText(e);
    }
  });

  async function save(e: Event) {
    e.preventDefault();
    if (!s) return;
    error = notice = '';
    try {
      await api('/admin/sso', {
        method: 'PUT',
        body: { enabled: s.enabled, tenant_id: s.tenant_id, client_id: s.client_id, client_secret: secret,
          jit_provisioning: s.jit_provisioning, default_role: s.default_role, sso_only: s.sso_only,
          group_sync_enabled: s.group_sync_enabled },
      });
      secret = '';
      s = await api<S>('/admin/sso');
      notice = 'Saved. Try it: sign out and use “Sign in with Microsoft”.';
    } catch (err) {
      error = errorText(err);
    }
  }
</script>

{#if error}<p class="error">{error}</p>{/if}
{#if notice}<p class="success">{notice}</p>{/if}
{#if s}
  <div class="grid">
    <form class="card" onsubmit={save}>
      <h2>Microsoft Entra ID</h2>
      <label class="inline"><input type="checkbox" bind:checked={s.enabled} /> Allow “Sign in with Microsoft”</label>
      <label for="t">Directory (tenant) ID</label>
      <input id="t" bind:value={s.tenant_id} placeholder="00000000-0000-0000-0000-000000000000" />
      <label for="c">Application (client) ID</label>
      <input id="c" bind:value={s.client_id} placeholder="00000000-0000-0000-0000-000000000000" />
      <label for="sec">Client secret {s.has_secret ? '(saved; leave empty to keep it)' : ''}</label>
      <input id="sec" type="password" autocomplete="off" bind:value={secret} />
      <h2>Accounts</h2>
      <label class="inline"><input type="checkbox" bind:checked={s.jit_provisioning} /> Create an account automatically the first time someone from your organisation signs in</label>
      <label for="role">Role for new accounts</label>
      <select id="role" bind:value={s.default_role}>
        <option value="member">Member (add them to workspaces afterwards)</option>
        <option value="auditor">Auditor</option>
        <option value="admin">Admin</option>
      </select>
      <label class="inline"><input type="checkbox" bind:checked={s.sso_only} /> Microsoft sign-in only (passwords stay available to owners as emergency access)</label>
      <h2>Group sync</h2>
      <label class="inline"><input type="checkbox" bind:checked={s.group_sync_enabled} /> Keep OpnRelay groups and roles in step with Microsoft groups (hourly and at sign-in)</label>
      {#if s.group_sync_enabled}
        <p class="small">In Entra, add these <strong>application</strong> permissions (Microsoft Graph) and grant admin consent:</p>
        <ul class="perms">
          {#each PERMISSIONS as perm}
            <li><code>{perm}</code> <button type="button" class="small" onclick={() => copy(perm)}>Copy</button></li>
          {/each}
        </ul>
        <p class="small muted">Save first, then link groups on the Groups tab. Needs a specific tenant ID (not “common”).</p>
      {/if}
      <p><button class="primary" type="submit">Save</button></p>
    </form>
    {#if s.group_sync_enabled}
      <div class="card sync">
        <h2>Group sync status</h2>
        <p class="small">Last run: {dateTime(s.group_sync_last_at)}<br />Last success: {dateTime(s.group_sync_last_ok_at)}</p>
        {#if s.group_sync_last_error}<p class="error" role="alert">{s.group_sync_last_error}</p>{/if}
        {#if s.group_sync_last_warning}<p class="warning" role="status">{s.group_sync_last_warning}</p>{/if}
        {#if stale}<p class="warning" role="status">No successful sync for over 24 hours.</p>{/if}
        {#if s.group_sync_brake_pending}
          <div class="warning" role="alert">
            <p>The sync paused itself: it would disable {s.group_sync_brake_pending} people. That is a lot at once, so check Microsoft before going ahead.</p>
            <button class="danger" disabled={syncing} onclick={() => syncNow(true)}>Disable {s.group_sync_brake_pending} people</button>
          </div>
        {/if}
        <p><button type="button" disabled={syncing} onclick={() => syncNow()}>{syncing ? 'Syncing…' : 'Sync now'}</button></p>
        {#if summary}
          <p class="small muted">
            {#if summary.error}Sync reported an error: {summary.error}{:else}Synced {summary.groups} groups: {summary.added} added, {summary.removed} removed, {summary.roles_changed} roles changed, {summary.disabled} disabled, {summary.missing} missing in Microsoft.{/if}
            {#each summary.warnings as w}<br />{w}{/each}
          </p>
        {/if}
      </div>
    {/if}
    <div class="card help">
      <h2>Setting up the app in Entra</h2>
      <ol class="small">
        <li>In the <strong>Microsoft Entra admin center</strong> open <em>App registrations → New registration</em>. Name it “OpnRelay”, single tenant.</li>
        <li>Redirect URI: platform <strong>Web</strong>, address:<br /><code class="uri">{s.redirect_uri}</code></li>
        <li>Copy the <em>Application (client) ID</em> and <em>Directory (tenant) ID</em> here.</li>
        <li><em>Certificates &amp; secrets → New client secret</em>; paste the value here. Note its expiry date.</li>
        <li><em>API permissions</em> (Microsoft Graph, delegated): <code>openid</code>, <code>profile</code>, <code>email</code>, and for OneDrive backups also <code>offline_access</code>, <code>User.Read</code>, <code>Files.ReadWrite</code>. Then <em>Grant admin consent</em>.</li>
      </ol>
      <p class="muted small">Microsoft enforces your company's MFA and Conditional Access, so Microsoft sign-ins count as two-factor in OpnRelay.</p>
    </div>
  </div>
{/if}

<style>
  .grid { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 1rem; align-items: start; }
  @media (max-width: 860px) { .grid { grid-template-columns: 1fr; } }
  h2 { font-size: 1.05rem; margin: 0.75rem 0 0.25rem; }
  .help ol { padding-left: 1.2rem; }
  .help li { margin-bottom: 0.5rem; }
  .perms { padding-left: 1.2rem; }
  .sync p { margin: 0.4rem 0; }
  .uri { word-break: break-all; }
</style>
