<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { api, exportUrl, type Guide } from '../lib/api';
  import { go } from '../lib/router.svelte';
  import { workspacesStore } from '../lib/session.svelte';
  import { errorText, dateTime, miniMarkdown } from '../lib/format';
  import Screenshot from '../lib/Screenshot.svelte';

  let { id }: { id: string } = $props();
  let guide = $state<Guide | null>(null);
  let error = $state('');

  $effect(() => {
    api<Guide>(`/guides/${id}`)
      .then((g) => (guide = g))
      .catch((e) => (error = errorText(e)));
  });

  let reviewNotice = $state('');
  let reviewing = $state(false);
  async function markReviewed() {
    reviewing = true;
    reviewNotice = error = '';
    try {
      guide = await api<Guide>(`/guides/${id}/reviewed`, { body: {} });
      workspacesStore.refreshReviewCount();
      reviewNotice = 'Marked as reviewed';
    } catch (e) {
      error = errorText(e);
    } finally {
      reviewing = false;
    }
  }

  let canEdit = $derived(guide && (guide.my_permission === 'edit' || guide.my_permission === 'manage'));

  // Number only action steps, like the exports do.
  let numbers = $derived.by(() => {
    let n = 0;
    return (guide?.steps ?? []).map((s) => (s.kind === 'action' ? ++n : 0));
  });

  let exportOpen = $state(false);
  let withImages = $state(true);
  const formats = [
    { f: 'pdf', label: 'PDF' },
    { f: 'docx', label: 'Word (.docx)' },
    { f: 'html', label: 'HTML (single file)' },
    { f: 'md', label: 'Markdown (.zip)' },
    { f: 'png', label: 'Screenshots (.zip)' },
  ];

  async function duplicate() {
    try {
      const g = await api<Guide>(`/guides/${id}/duplicate`, { body: {} });
      go(`/guides/${g.id}/edit`);
    } catch (e) {
      alert(errorText(e));
    }
  }

  async function remove() {
    if (!confirm('Delete this guide? Administrators can still see it in the audit log.')) return;
    try {
      await api(`/guides/${id}`, { method: 'DELETE' });
      go('/');
    } catch (e) {
      alert(errorText(e));
    }
  }
</script>

{#if error && !guide}
  <p class="error">{error}</p>
{:else if !guide}
  <p class="muted">Loading…</p>
{:else}
  <article class="guide">
    <div class="spread">
      <div>
        <a href={`#/?ws=${guide.workspace_id}`} class="small">← Back</a>
        <h1>{guide.title}</h1>
        <p class="muted small">
          <span class="badge {guide.status}">{guide.status}</span>
          {#if guide.classification !== 'internal'}<span class="badge {guide.classification}">{guide.classification}</span>{/if}
          {guide.step_count} steps · by {guide.author_name ?? 'unknown'} · updated {dateTime(guide.updated_at)}
          {#if guide.current_version}· version {guide.current_version}{/if}
          {#if guide.reviewed_at}· Last reviewed {new Date(guide.reviewed_at).toLocaleDateString()}{/if}
        </p>
      </div>
      <div class="row">
        <div class="export">
          <button onclick={() => (exportOpen = !exportOpen)} aria-expanded={exportOpen}>Export ▾</button>
          {#if exportOpen}
            <div class="menu card">
              {#each formats as x}
                <a href={exportUrl(guide.id, x.f, withImages)} download onclick={() => (exportOpen = false)}>{x.label}</a>
              {/each}
              <label class="inline small"><input type="checkbox" bind:checked={withImages} /> Include screenshots</label>
            </div>
          {/if}
        </div>
        {#if canEdit}
          <button onclick={duplicate}>Duplicate</button>
          <button class="danger" onclick={remove}>Delete</button>
          <a class="btn primary" href={`#/guides/${guide.id}/edit`}>Edit</a>
        {/if}
      </div>
    </div>
    {#if error}<p class="error">{error}</p>{/if}
    {#if canEdit && guide.needs_review}
      <div class="review-notice row">
        <p>
          <strong>This guide is due for review.</strong>
          <span class="small">Confirm it is still correct, or publish a new version, which also counts as a review.</span>
        </p>
        <button class="primary" onclick={markReviewed} disabled={reviewing}>Still accurate</button>
      </div>
    {:else if reviewNotice}
      <p class="success" role="status">{reviewNotice}</p>
    {/if}
    {#if guide.description}<p class="lead">{guide.description}</p>{/if}

    <ol class="steps">
      {#each guide.steps as s, i (s.id)}
        <li class="step {s.kind}">
          {#if s.kind === 'heading'}
            <h2>{s.title}</h2>
          {:else if s.kind === 'tip' || s.kind === 'alert'}
            <div class="callout">
              <strong>{s.kind === 'tip' ? '💡 Tip' : '⚠️ Important'}{s.title ? `: ${s.title}` : ''}</strong>
              {#if s.body_md}<div>{@html miniMarkdown(s.body_md)}</div>{/if}
            </div>
          {:else}
            <div class="step-head">
              {#if numbers[i]}<span class="num">{numbers[i]}</span>{/if}
              <h3>{s.title || 'Untitled step'}</h3>
            </div>
            {#if s.body_md}<div class="body">{@html miniMarkdown(s.body_md)}</div>{/if}
            {#if s.media_id}<Screenshot mediaId={s.media_id} highlight={s.annotations?.highlight} alt={s.title} />{/if}
          {/if}
        </li>
      {/each}
    </ol>
    {#if guide.steps.length === 0}<p class="muted">This guide has no steps yet.</p>{/if}
  </article>
{/if}

<style>
  .guide { max-width: 900px; margin: 0 auto; }
  .export { position: relative; }
  .menu { position: absolute; right: 0; top: calc(100% + 4px); z-index: 20; min-width: 220px; padding: 0.4rem; display: flex; flex-direction: column; }
  .menu a { padding: 0.4rem 0.6rem; border-radius: 6px; color: var(--text); }
  .menu a:hover { background: var(--surface-2); text-decoration: none; }
  .menu label { padding: 0.4rem 0.6rem; margin: 0; border-top: 1px solid var(--border); }
  .review-notice { justify-content: space-between; gap: 1rem; margin: 0.75rem 0; padding: 0.6rem 1rem; border-radius: var(--radius); border: 1px solid #f59e0b; background: color-mix(in srgb, #f59e0b 15%, var(--surface)); }
  .review-notice p { margin: 0; }
  .lead { font-size: 1.05rem; }
  .steps { list-style: none; padding: 0; margin: 1.5rem 0; }
  .step { margin-bottom: 1.75rem; }
  .step-head { display: flex; align-items: center; gap: 0.75rem; }
  .step-head h3 { margin: 0; font-size: 1.05rem; }
  .num { flex: none; width: 2rem; height: 2rem; border-radius: 50%; background: var(--primary); color: var(--primary-text); display: grid; place-items: center; font-weight: 700; }
  .body { margin: 0.4rem 0 0.6rem 2.75rem; }
  .action :global(.shot) { margin-left: 2.75rem; max-width: calc(100% - 2.75rem); }
  .callout { padding: 0.75rem 1rem; border-radius: var(--radius); }
  .tip .callout { background: var(--tip); }
  .alert .callout { background: var(--alert); }
</style>
