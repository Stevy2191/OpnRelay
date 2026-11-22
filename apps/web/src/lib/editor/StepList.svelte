<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- Editor step list: select, reorder (drag and drop or Alt+Up/Down) and add steps. -->
<script lang="ts">
  import { tick } from 'svelte';
  import { mediaUrl, type Step, type StepKind } from '../api';

  let {
    steps,
    selectedId,
    onselect,
    onmove,
    onadd,
    onupload,
    onopen,
  }: {
    steps: Step[];
    selectedId: string;
    onselect: (id: string) => void;
    /** Move the step at `from` so that it ends up at index `to`. */
    onmove: (from: number, to: number) => void;
    onadd: (kind: StepKind) => void;
    onupload: () => void;
    /** Enter on a row: start editing that step. */
    onopen: (id: string) => void;
  } = $props();

  const pad = (n: number) => String(n).padStart(2, '0');
  let numbers = $derived.by(() => {
    let n = 0;
    return steps.map((s) => (s.kind === 'action' ? ++n : 0));
  });
  let actionCount = $derived(numbers.filter(Boolean).length);

  const kindLabel: Record<StepKind, string> = { action: 'Step', heading: 'Heading', text: 'Text', tip: 'Tip', alert: 'Alert' };

  function rowTitle(s: Step): string {
    if (s.title.trim()) return s.title;
    if (s.kind === 'tip' || s.kind === 'alert' || s.kind === 'text') return s.body_md.trim().split('\n')[0] || kindLabel[s.kind];
    return s.kind === 'heading' ? 'Untitled section' : 'Untitled step';
  }

  let listEl: HTMLElement | undefined = $state();

  async function focusRow(id: string) {
    await tick();
    listEl?.querySelector<HTMLElement>(`[data-sid="${id}"]`)?.focus();
  }

  function keydown(e: KeyboardEvent, i: number) {
    const s = steps[i];
    if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      e.preventDefault();
      const to = e.key === 'ArrowUp' ? i - 1 : i + 1;
      if (to < 0 || to >= steps.length) return;
      onmove(i, to);
      focusRow(s.id);
      return;
    }
    let next = -1;
    if (e.key === 'ArrowUp') next = i - 1;
    else if (e.key === 'ArrowDown') next = i + 1;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = steps.length - 1;
    else if (e.key === 'Enter') {
      e.preventDefault();
      onopen(s.id);
      return;
    }
    if (next < 0 || next >= steps.length) return;
    e.preventDefault();
    onselect(steps[next].id);
    focusRow(steps[next].id);
  }

  // Drag and drop: `dropAt` is the gap index (0..steps.length) the dragged row would land in.
  let dragFrom = $state(-1);
  let dropAt = $state(-1);

  function dragstart(e: DragEvent, i: number) {
    dragFrom = i;
    e.dataTransfer?.setData('text/plain', steps[i].id);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
  }

  function dragover(e: DragEvent, i: number) {
    if (dragFrom < 0) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    dropAt = e.clientY < r.top + r.height / 2 ? i : i + 1;
  }

  function drop(e: DragEvent) {
    if (dragFrom < 0) return;
    e.preventDefault();
    const from = dragFrom;
    // Removing the row first shifts later gaps up by one.
    const to = dropAt > from ? dropAt - 1 : dropAt;
    const id = steps[from].id;
    dragend();
    if (to !== from && to >= 0) {
      onmove(from, to);
      onselect(id);
    }
  }

  function dragend() {
    dragFrom = -1;
    dropAt = -1;
  }
</script>

<nav class="list" aria-label="Steps">
  <div class="head mono">
    <span>STEPS · {actionCount}</span>
    <span class="hint" aria-hidden="true">Alt+↑↓ moves</span>
  </div>
  {#if steps.length === 0}
    <p class="empty muted small">No steps yet. Add one below, or paste a screenshot (Ctrl/⌘+V).</p>
  {:else}
    <ol bind:this={listEl} ondragleave={(e) => { if (!(e.currentTarget as HTMLElement).contains(e.relatedTarget as Node)) dropAt = -1; }}>
      {#each steps as s, i (s.id)}
        <li
          class:drop-before={dropAt === i && dragFrom !== i && dragFrom !== i - 1}
          class:drop-after={i === steps.length - 1 && dropAt === steps.length && dragFrom !== i}
          ondragover={(e) => dragover(e, i)}
          ondrop={drop}
        >
          <button
            type="button"
            class="row {s.kind}"
            class:selected={s.id === selectedId}
            class:dragging={dragFrom === i}
            data-sid={s.id}
            tabindex={s.id === selectedId ? 0 : -1}
            aria-current={s.id === selectedId ? 'step' : undefined}
            draggable="true"
            ondragstart={(e) => dragstart(e, i)}
            ondragend={dragend}
            onclick={() => onselect(s.id)}
            onkeydown={(e) => keydown(e, i)}
            title={rowTitle(s)}
          >
            {#if s.kind === 'heading'}
              <span class="sec">{rowTitle(s)}</span>
            {:else}
              <span class="num mono">{s.kind === 'action' ? pad(numbers[i]) : ''}</span>
              {#if s.media_id}
                <img class="thumb" src={mediaUrl(s.media_id)} alt="" loading="lazy" />
              {:else if s.kind === 'tip' || s.kind === 'alert' || s.kind === 'text'}
                <span class="badge-kind mono">{kindLabel[s.kind].toUpperCase()}</span>
              {:else}
                <span class="thumb none" aria-hidden="true"></span>
              {/if}
              <span class="t">{rowTitle(s)}</span>
            {/if}
          </button>
        </li>
      {/each}
    </ol>
  {/if}
  <div class="add" role="group" aria-label="Add">
    <button type="button" class="small ghost plus" onclick={() => onadd('action')}>+ Step</button>
    <button type="button" class="small ghost" onclick={() => onadd('heading')}>Heading</button>
    <button type="button" class="small ghost" onclick={() => onadd('tip')}>Tip</button>
    <button type="button" class="small ghost" onclick={() => onadd('alert')}>Alert</button>
    <button type="button" class="small ghost" onclick={onupload}>Screenshots…</button>
  </div>
</nav>

<style>
  .list { border: 1px solid var(--line); border-radius: var(--radius-lg); background: var(--card); display: flex; flex-direction: column; min-height: 0; }
  .head { display: flex; justify-content: space-between; align-items: baseline; gap: 0.5rem; padding: 0.7rem 0.85rem 0.45rem; font-size: 0.72rem; letter-spacing: 0.06em; color: var(--muted); }
  .hint { font-size: 0.68rem; letter-spacing: 0; }
  .empty { padding: 0.5rem 0.85rem 0.75rem; margin: 0; }
  ol { list-style: none; margin: 0; padding: 0 0 0.4rem; overflow-y: auto; flex: 1; min-height: 0; }
  li { position: relative; }
  li.drop-before::before, li.drop-after::after { content: ''; position: absolute; left: 0.5rem; right: 0.5rem; height: 2px; background: var(--sig); border-radius: 2px; z-index: 1; }
  li.drop-before::before { top: -1px; }
  li.drop-after::after { bottom: -1px; }
  .row {
    display: flex; align-items: center; justify-content: flex-start; gap: 0.55rem; width: 100%; min-height: 40px;
    padding: 0.35rem 0.85rem; border: 0; border-left: 3px solid transparent; border-radius: 0;
    background: transparent; color: var(--text); text-align: left; font-weight: 400; cursor: pointer;
  }
  .row:hover:not(.selected) { background: var(--surface-2); }
  .row.selected { background: var(--sig-soft); border-left-color: var(--sig); }
  .row:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
  .row.dragging { opacity: 0.45; }
  .row.heading { margin-top: 0.35rem; padding-top: 0.55rem; }
  .sec { font-weight: 700; font-size: 0.88rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .num { width: 1.4rem; flex: none; font-size: 0.78rem; font-weight: 500; color: var(--muted); }
  .selected .num { color: var(--sig-text); }
  .thumb { width: 46px; height: 28px; flex: none; object-fit: cover; object-position: top left; border: 1px solid var(--line); border-radius: 3px; background: var(--surface-2); }
  .thumb.none { display: inline-block; }
  .badge-kind { width: 46px; flex: none; text-align: center; font-size: 0.62rem; letter-spacing: 0.05em; padding: 0.15rem 0; border-radius: 3px; background: var(--surface-2); color: var(--muted-strong); }
  .tip .badge-kind { background: var(--ok-soft); color: var(--ok-text); }
  .alert .badge-kind { background: var(--danger-soft); color: var(--danger); }
  .t { font-size: 0.86rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; flex: 1; }
  .add { display: flex; flex-wrap: wrap; gap: 0.1rem; padding: 0.45rem 0.5rem; border-top: 1px dashed var(--line); }
  .add button { padding: 0.25rem 0.45rem; font-size: 0.8rem; color: var(--muted-strong); }
  .add .plus { color: var(--sig-text); font-weight: 600; }
</style>
