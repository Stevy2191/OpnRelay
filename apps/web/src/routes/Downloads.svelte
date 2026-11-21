<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { errorText } from '../lib/format';

  type File = { name: string; size: number; platform: string; label: string; sha256: string | null };
  let files = $state<File[]>([]);
  let server = $state('');
  let error = $state('');
  let loaded = $state(false);

  const ua = navigator.userAgent;
  const mine = ua.includes('Windows') ? 'windows' : ua.includes('Mac') ? 'macos' : ua.includes('Linux') ? 'linux' : '';
  const groups = [
    { key: 'windows', title: 'Windows' },
    { key: 'macos', title: 'macOS' },
    { key: 'linux', title: 'Linux' },
    { key: 'extension', title: 'Browser extension packages (for IT deployment)' },
    { key: 'other', title: 'Other files' },
  ];
  const mb = (n: number) => `${(n / 1024 / 1024).toFixed(1)} MB`;

  onMount(async () => {
    try {
      const r = await api<{ server_url: string; files: File[] }>('/downloads');
      files = r.files;
      server = r.server_url;
    } catch (e) {
      error = errorText(e);
    } finally {
      loaded = true;
    }
  });
</script>

<div class="downloads stack">
  <h1>Downloads</h1>
  <p class="muted">Installers for the OpnRelay desktop recorder, shipped with this server version. Only administrators can see this page; distribute the recorder to your staff (for example with Intune or Group Policy).</p>
  <div class="card">
    <strong>Server address for the recorder:</strong> <code>{server}</code>
    <button class="small" onclick={() => navigator.clipboard.writeText(server)}>Copy</button>
    <p class="muted small">Enter it the first time the recorder starts, or pre-set it for everyone with the <code>OPNRELAY_SERVER_URL</code> environment variable.</p>
  </div>
  {#if error}<p class="error">{error}</p>{/if}
  {#if loaded && files.length === 0 && !error}
    <div class="card"><p>No installers are bundled with this server build. Official release images include them; for a source build, see <code>deploy/downloads/README.md</code>.</p></div>
  {/if}
  {#each groups as g}
    {@const list = files.filter((f) => f.platform === g.key)}
    {#if list.length}
      <div class="card" class:mine={g.key === mine}>
        <h2>{g.title} {#if g.key === mine}<span class="badge published">your computer</span>{/if}</h2>
        <ul>
          {#each list as f (f.name)}
            <li>
              <a class="btn" class:primary={g.key === mine} href={`/api/v1/downloads/${f.name}`} download>{f.label}</a>
              <span class="muted small">{f.name} · {mb(f.size)}</span>
              {#if f.sha256}<div class="sha small muted">SHA-256 {f.sha256}</div>{/if}
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  {/each}
  <p class="muted small">The installers are signed with free signing only, so Windows and macOS show a one-time “unknown developer” prompt unless they are deployed by your management tools. See the deployment guide.</p>
</div>

<style>
  .downloads { max-width: 820px; margin: 0 auto; }
  h2 { font-size: 1.05rem; margin: 0 0 0.5rem; }
  ul { list-style: none; padding: 0; margin: 0; }
  li { padding: 0.4rem 0; }
  .mine { border-color: var(--primary); }
  .sha { font-family: ui-monospace, monospace; word-break: break-all; }
</style>
