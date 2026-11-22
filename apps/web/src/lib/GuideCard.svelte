<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- Workspace card: screenshot thumbnail, status, title, author and age. -->
<script lang="ts">
  import { mediaUrl, type GuideSummary } from './api';
  import { initials, monthsOverdue, when } from './format';

  let { guide: g, showReview = false }: { guide: GuideSummary; showReview?: boolean } = $props();
  let overdue = $derived(showReview && g.needs_review ? monthsOverdue(g.review_due_at) : 0);
  let thumbFailed = $state(false);
</script>

<a class="gcard" href={`#/guides/${g.id}`} title={g.title}>
  <div class="thumb" class:empty={!g.thumb_media_id || thumbFailed}>
    {#if g.thumb_media_id && !thumbFailed}
      <img src={mediaUrl(g.thumb_media_id)} alt="" loading="lazy" onerror={() => (thumbFailed = true)} />
    {:else}
      <span class="none">No screenshots yet</span>
    {/if}
    <span class="st">
      {#if g.status === 'published'}<span class="tag pub">Published v{g.current_version}</span>{:else}<span class="tag draft">{g.status}</span>{/if}
    </span>
    <span class="steps tag mono">{g.step_count} {g.step_count === 1 ? 'step' : 'steps'}</span>
  </div>
  <div class="body">
    <h3 class="gtitle">{g.title}</h3>
    <div class="meta">
      <span class="avatar" aria-hidden="true">{initials(g.author_name)}</span>
      <span class="who">{g.author_name ?? 'Unknown'}</span>
      {#if showReview && g.needs_review}
        <span class="rev mono">NEEDS REVIEW{overdue > 0 ? ` · ${overdue} mo` : ''}</span>
      {:else}
        <span class="when mono">{when(g.updated_at)}</span>
      {/if}
    </div>
  </div>
</a>

<style>
  .gcard {
    display: flex;
    flex-direction: column;
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    color: var(--text);
    overflow: hidden;
    min-width: 0;
    text-decoration: none;
  }
  .gcard:hover, .gcard:focus-visible { outline: 2px solid var(--sig); outline-offset: 0; text-decoration: none; }
  .thumb { position: relative; height: 92px; background: var(--surface-2); overflow: hidden; border-bottom: 1px solid var(--line); flex: none; }
  .thumb img { width: 100%; height: 100%; object-fit: cover; object-position: top; display: block; }
  .thumb.empty {
    background: repeating-linear-gradient(135deg, var(--surface-2) 0 10px, var(--card) 10px 20px);
    display: grid;
    place-items: center;
  }
  .none { font-family: var(--font-mono); font-size: 0.72rem; color: var(--muted-strong); background: var(--card); padding: 0.1rem 0.45rem; border-radius: 4px; border: 1px solid var(--line); }
  .st { position: absolute; top: 0.4rem; left: 0.4rem; }
  .steps { position: absolute; right: 0.4rem; bottom: 0.4rem; background: var(--card); color: var(--muted-strong); }
  .body { padding: 0.65rem 0.8rem 0.75rem; display: flex; flex-direction: column; gap: 0.5rem; min-width: 0; flex: 1; }
  .gtitle { margin: 0; font-size: 0.95rem; line-height: 1.3; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
  .meta { display: flex; align-items: center; gap: 0.45rem; margin-top: auto; font-size: 0.8rem; color: var(--muted); min-width: 0; }
  .avatar { width: 20px; height: 20px; border-radius: 50%; background: var(--surface-2); color: var(--muted-strong); display: grid; place-items: center; font-size: 0.62rem; font-weight: 600; flex: none; }
  .who { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; flex: 1; }
  .when, .rev { flex: none; font-size: 0.72rem; }
  .rev { color: var(--sig-text); font-weight: 500; }
</style>
