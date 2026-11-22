<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- Sortable-by-server guide list: one row per guide, click anywhere to open. -->
<script lang="ts">
  import type { GuideSummary } from './api';
  import { go } from './router.svelte';
  import { when } from './format';

  let {
    guides,
    wsName = null,
    showReview = () => false,
  }: {
    guides: GuideSummary[];
    /** Resolves a workspace id to a name; omit to hide the Workspace column. */
    wsName?: ((id: string) => string) | null;
    showReview?: (g: GuideSummary) => boolean;
  } = $props();

  function open(e: MouseEvent, g: GuideSummary) {
    if ((e.target as HTMLElement).closest('a')) return; // the title link handles itself
    go(`/guides/${g.id}`);
  }
</script>

<div class="table-wrap">
  <table class="data guides">
    <thead>
      <tr>
        <th scope="col">Guide</th>
        {#if wsName}<th scope="col" class="c-ws">Workspace</th>{/if}
        <th scope="col">Status</th>
        <th scope="col" class="c-class">Class.</th>
        <th scope="col" class="c-steps">Steps</th>
        <th scope="col" class="c-upd">Updated</th>
      </tr>
    </thead>
    <tbody>
      {#each guides as g (g.id)}
        <tr onclick={(e) => open(e, g)}>
          <td class="c-guide">
            <a class="title" href={`#/guides/${g.id}`} title={g.title}>{g.title}</a>
            {#if g.description}<span class="desc muted small" title={g.description}>{g.description}</span>{/if}
          </td>
          {#if wsName}<td class="c-ws"><span class="ell" title={wsName(g.workspace_id)}>{wsName(g.workspace_id)}</span></td>{/if}
          <td class="c-status">
            <div class="tags">
              {#if g.status === 'published'}<span class="tag pub">Published v{g.current_version}</span>{:else}<span class="tag draft">{g.status}</span>{/if}
              {#if g.needs_review && showReview(g)}<span class="tag review">Needs review</span>{/if}
            </div>
          </td>
          <td class="c-class"><span class="tag" class:class={g.classification !== 'internal'}>{g.classification}</span></td>
          <td class="c-steps mono">{g.step_count}</td>
          <td class="c-upd mono small">{when(g.updated_at)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  table.guides { table-layout: fixed; }
  tbody tr { cursor: pointer; }
  .c-guide { width: 37%; }
  .c-ws { width: 16%; }
  .c-status { width: 17%; white-space: normal; }
  .tags { display: flex; flex-wrap: wrap; gap: 0.3rem; }
  .c-class { width: 14%; }
  .c-steps { width: 6%; }
  .c-upd { width: 11%; white-space: nowrap; }
  td, th { overflow: hidden; }
  .title, .desc, .ell { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .title { color: var(--text); font-weight: 600; }
  .title:hover { color: var(--link); }
  tbody tr:focus-within td { background: var(--surface-2); }
  @media (max-width: 899px) {
    .c-ws, .c-class, .c-steps { display: none; }
    .c-guide { width: 55%; }
    .c-status { width: 25%; }
    .c-upd { width: 20%; }
  }
  @media (max-width: 520px) {
    .c-upd { display: none; }
    .c-guide { width: 65%; }
    .c-status { width: 35%; }
  }
</style>
