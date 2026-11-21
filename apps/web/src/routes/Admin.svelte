<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type Group, type OrgRole, type Policies, type User } from '../lib/api';
  import { session } from '../lib/session.svelte';
  import { errorText, when } from '../lib/format';
  import BrandingSettings from './BrandingSettings.svelte';
  import SsoSettings from './SsoSettings.svelte';
  import BackupSettings from './BackupSettings.svelte';
  import { route, query } from '../lib/router.svelte';

  type Tab = 'users' | 'groups' | 'policies' | 'branding' | 'sso' | 'backups';
  let tab = $state<Tab>((query(route.path).get('tab') as Tab) || 'users');
  let users = $state<User[]>([]);
  let groups = $state<Group[]>([]);
  let policies = $state<Policies | null>(null);
  let error = $state('');
  let notice = $state('');
  let link = $state('');

  let invite = $state({ email: '', display_name: '', org_role: 'member' as OrgRole });
  let groupName = $state('');
  let patterns = $state('');

  async function load() {
    error = '';
    try {
      [users, groups, policies] = await Promise.all([
        api<User[]>('/admin/users'),
        api<Group[]>('/admin/groups'),
        api<Policies>('/admin/policies'),
      ]);
      patterns = policies!.redaction_patterns.join('\n');
    } catch (e) {
      error = errorText(e);
    }
  }
  onMount(load);

  async function run(fn: () => Promise<unknown>, ok = '') {
    error = notice = '';
    try {
      await fn();
      notice = ok;
      await load();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function sendInvite(e: Event) {
    e.preventDefault();
    await run(async () => {
      const r = await api<{ invite_url: string }>('/admin/users', { body: invite });
      link = r.invite_url;
      invite = { email: '', display_name: '', org_role: 'member' };
    }, 'Invitation created. Send the link below to the person (it works once, for 7 days).');
  }

  const update = (u: User, body: Record<string, unknown>) =>
    run(() => api(`/admin/users/${u.id}`, { method: 'PATCH', body }), 'Saved');

  async function resetLink(u: User) {
    await run(async () => {
      link = (await api<{ url: string }>(`/admin/users/${u.id}/reset-link`, { body: {} })).url;
    }, `One-time link for ${u.email} created. Send it to them directly.`);
  }

  function copyLink() {
    navigator.clipboard.writeText(link);
    notice = 'Link copied';
  }

  const isOwner = $derived(session.me?.org_role === 'owner');
  const roles: OrgRole[] = ['member', 'auditor', 'admin', 'owner'];

  async function savePolicies(e: Event) {
    e.preventDefault();
    if (!policies) return;
    policies.redaction_patterns = patterns.split('\n').map((s) => s.trim()).filter(Boolean);
    await run(() => api('/admin/policies', { method: 'PUT', body: policies }), 'Policies saved');
  }

  async function toggleMember(g: Group, userId: string, on: boolean) {
    const ids = on ? [...g.member_ids, userId] : g.member_ids.filter((x) => x !== userId);
    await run(() => api(`/admin/groups/${g.id}/members`, { method: 'PUT', body: { user_ids: ids } }));
  }
</script>

<h1>Administration</h1>
<div class="tabs row" role="tablist">
  <button role="tab" aria-selected={tab === 'users'} class:primary={tab === 'users'} onclick={() => (tab = 'users')}>Users</button>
  <button role="tab" aria-selected={tab === 'groups'} class:primary={tab === 'groups'} onclick={() => (tab = 'groups')}>Groups</button>
  <button role="tab" aria-selected={tab === 'policies'} class:primary={tab === 'policies'} onclick={() => (tab = 'policies')}>Security & defaults</button>
  <button role="tab" aria-selected={tab === 'branding'} class:primary={tab === 'branding'} onclick={() => (tab = 'branding')}>Branding</button>
  <button role="tab" aria-selected={tab === 'sso'} class:primary={tab === 'sso'} onclick={() => (tab = 'sso')}>Microsoft sign-in</button>
  <button role="tab" aria-selected={tab === 'backups'} class:primary={tab === 'backups'} onclick={() => (tab = 'backups')}>Backups</button>
</div>

{#if error}<p class="error" role="alert">{error}</p>{/if}
{#if notice}<p class="success">{notice}</p>{/if}
{#if link}
  <div class="card linkbox">
    <code>{link}</code>
    <div class="row"><button class="small" onclick={copyLink}>Copy</button><button class="small ghost" onclick={() => (link = '')}>Hide</button></div>
  </div>
{/if}

{#if tab === 'users'}
  <form class="card invite" onsubmit={sendInvite}>
    <h2>Invite someone</h2>
    <div class="row fields">
      <input placeholder="Email" type="email" required bind:value={invite.email} aria-label="Email" />
      <input placeholder="Full name" required bind:value={invite.display_name} aria-label="Full name" />
      <select bind:value={invite.org_role} aria-label="Role">
        {#each roles as r}{#if r !== 'owner' || isOwner}<option value={r}>{r}</option>{/if}{/each}
      </select>
      <button class="primary" type="submit">Create invite link</button>
    </div>
    <p class="muted small">Members see only workspaces they are added to. Auditors can read every guide and the audit log but cannot change anything. Admins manage everything.</p>
  </form>

  <div class="card table-wrap">
    <table>
      <thead><tr><th>Name</th><th>Role</th><th>Status</th><th>2FA</th><th>Last sign-in</th><th></th></tr></thead>
      <tbody>
        {#each users as u (u.id)}
          {@const self = u.id === session.me?.id}
          {@const locked = u.org_role === 'owner' && !isOwner}
          <tr>
            <td><strong>{u.display_name}</strong><br /><span class="muted small">{u.email}</span></td>
            <td>
              <select value={u.org_role} disabled={self || locked} onchange={(e) => update(u, { org_role: (e.currentTarget as HTMLSelectElement).value })} aria-label="Role">
                {#each roles as r}{#if r !== 'owner' || isOwner || u.org_role === 'owner'}<option value={r}>{r}</option>{/if}{/each}
              </select>
            </td>
            <td><span class="badge">{u.status}</span></td>
            <td>{u.mfa_enabled ? '✅' : '—'}</td>
            <td class="small">{when(u.last_login_at)}</td>
            <td>
              <div class="actions">
              {#if !self && !locked}
                <button class="small" onclick={() => resetLink(u)}>{u.status === 'invited' ? 'New invite link' : 'Reset password link'}</button>
                {#if u.mfa_enabled}<button class="small" onclick={() => confirm(`Remove 2FA for ${u.email}?`) && run(() => api(`/admin/users/${u.id}/reset-mfa`, { body: {} }), '2FA removed')}>Reset 2FA</button>{/if}
                <button class="small" onclick={() => run(() => api(`/admin/users/${u.id}/revoke-sessions`, { body: {} }), 'Signed out everywhere')}>Sign out</button>
                {#if u.status === 'active'}
                  <button class="small danger" onclick={() => confirm(`Disable ${u.email}?`) && update(u, { status: 'disabled' })}>Disable</button>
                {:else if u.status === 'disabled'}
                  <button class="small" onclick={() => update(u, { status: 'active' })}>Enable</button>
                {/if}
              {/if}
              </div>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else if tab === 'groups'}
  <form class="card row" onsubmit={(e) => { e.preventDefault(); run(() => api('/admin/groups', { body: { name: groupName } }), 'Group created').then(() => (groupName = '')); }}>
    <input placeholder="New group name (e.g. EMS Supervisors)" required bind:value={groupName} aria-label="Group name" style="flex:1" />
    <button class="primary" type="submit">Create group</button>
  </form>
  <p class="muted small">Add a group to a workspace (in the workspace's settings) to give all its members access at once.</p>
  {#each groups as g (g.id)}
    <div class="card group">
      <div class="spread">
        <h3>{g.name} <span class="muted small">{g.member_ids.length} members</span></h3>
        <button class="small danger" onclick={() => confirm(`Delete group ${g.name}?`) && run(() => api(`/admin/groups/${g.id}`, { method: 'DELETE' }))}>Delete</button>
      </div>
      <div class="members">
        {#each users.filter((u) => u.status !== 'disabled') as u (u.id)}
          <label class="inline small"><input type="checkbox" checked={g.member_ids.includes(u.id)} onchange={(e) => toggleMember(g, u.id, (e.currentTarget as HTMLInputElement).checked)} /> {u.display_name}</label>
        {/each}
      </div>
    </div>
  {/each}
{:else if tab === 'branding'}
  <BrandingSettings />
{:else if tab === 'sso'}
  <SsoSettings />
{:else if tab === 'backups'}
  <BackupSettings />
{:else if policies}
  <form class="card policies" onsubmit={savePolicies}>
    <h2>Sign-in security</h2>
    <label for="mfa">Require two-factor authentication for</label>
    <select id="mfa" bind:value={policies.mfa_requirement}>
      <option value="off">Nobody (optional for everyone)</option>
      <option value="admins">Owners and admins</option>
      <option value="all">Everyone</option>
    </select>
    <div class="row two">
      <div><label for="idle">Sign out after inactivity (minutes)</label><input id="idle" type="number" min="5" max="43200" bind:value={policies.session_idle_minutes} /></div>
      <div><label for="abs">Maximum session length (hours)</label><input id="abs" type="number" min="1" max="8760" bind:value={policies.session_absolute_hours} /></div>
    </div>
    <h2>Sharing</h2>
    <label class="inline"><input type="checkbox" bind:checked={policies.public_links_enabled} /> Allow public share links</label>
    <h2>Recording defaults</h2>
    <p class="muted small">These are only defaults. Each person can change them when they record.</p>
    <label for="mode">Default screenshot mode</label>
    <select id="mode" bind:value={policies.default_capture_mode}>
      <option value="region">Area around the click</option>
      <option value="window">Active window</option>
      <option value="full">Full screen</option>
    </select>
    <label class="inline"><input type="checkbox" bind:checked={policies.auto_redaction_default} /> Turn on automatic redaction by default</label>
    <label for="pat">Extra redaction patterns (one regular expression per line)</label>
    <textarea id="pat" rows="3" bind:value={patterns} placeholder="e.g. EMP-\d{6}"></textarea>
    <p><button class="primary" type="submit">Save</button></p>
  </form>
{/if}

<style>
  .tabs { margin-bottom: 1rem; }
  .invite h2, .policies h2 { margin-top: 0.5rem; }
  .fields > * { flex: 1; min-width: 150px; }
  .fields > button { flex: none; }
  .actions { display: flex; gap: 0.35rem; flex-wrap: wrap; }
  .linkbox { margin: 1rem 0; display: flex; gap: 1rem; align-items: center; justify-content: space-between; flex-wrap: wrap; }
  .linkbox code { word-break: break-all; }
  .card + .card { margin-top: 1rem; }
  .group h3 { margin: 0; }
  .members { display: flex; flex-wrap: wrap; gap: 0.25rem 1rem; margin-top: 0.5rem; }
  .two > div { flex: 1; min-width: 200px; }
  .policies { max-width: 720px; }
</style>
