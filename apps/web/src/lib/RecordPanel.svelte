<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { api } from './api';
  import { go } from './router.svelte';
  import { session, isAdmin, workspacesStore } from './session.svelte';
  import { errorText } from './format';

  let { onclose }: { onclose: () => void } = $props();

  let editable = $derived(workspacesStore.list.filter((w) => w.my_permission === 'edit' || w.my_permission === 'manage'));
  let wsId = $state('');
  let title = $state('Untitled guide');
  let busy = $state(false);
  let error = $state('');

  $effect(() => {
    if (!wsId && editable.length) wsId = editable[0].id;
  });

  async function create(e: Event) {
    e.preventDefault();
    if (!wsId || !title.trim() || busy) return;
    busy = true;
    error = '';
    try {
      const g = await api<{ id: string }>('/guides', { body: { workspace_id: wsId, folder_id: null, title: title.trim() } });
      await workspacesStore.refresh();
      onclose();
      go(`/guides/${g.id}/edit`);
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="panel">
  <h2 id="record-title">Record a guide</h2>
  <p class="muted">
    Guides are captured while you work. Install the <strong>desktop recorder</strong> to record any Windows or macOS
    application, or the <strong>browser extension</strong> to record web apps. Finished recordings arrive here as drafts
    for you to edit and publish.
  </p>
  {#if isAdmin(session.me)}
    <p><a class="btn" href="#/downloads" onclick={onclose}>Get the recorder and extension</a></p>
  {:else}
    <p class="muted small">Ask your administrator for the desktop recorder and browser extension.</p>
  {/if}

  <hr />

  <h3>New guide from screenshots</h3>
  {#if editable.length === 0}
    <p class="muted small">You need edit access to a workspace to create guides.</p>
  {:else}
    <form onsubmit={create}>
      <label for="rp-ws">Workspace</label>
      <select id="rp-ws" bind:value={wsId}>
        {#each editable as w (w.id)}<option value={w.id}>{w.name}</option>{/each}
      </select>
      <label for="rp-title">Guide title</label>
      <input id="rp-title" bind:value={title} maxlength="200" required />
      {#if error}<p class="error">{error}</p>{/if}
      <div class="row actions">
        <button type="button" onclick={onclose}>Cancel</button>
        <button class="primary" type="submit" disabled={busy || !title.trim()}>Create and add screenshots</button>
      </div>
    </form>
  {/if}
  {#if editable.length === 0}
    <div class="row actions"><button type="button" onclick={onclose}>Close</button></div>
  {/if}
</div>

<style>
  .panel h2 { margin-top: 0; }
  hr { border: 0; border-top: 1px solid var(--line); margin: 1.25rem 0 0.5rem; }
  .actions { justify-content: flex-end; margin-top: 1rem; }
</style>
