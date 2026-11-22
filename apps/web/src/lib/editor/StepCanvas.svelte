<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- Editor canvas: the selected step's screenshot with the tools that exist today. -->
<script lang="ts">
  import type { Step } from '../api';
  import { miniMarkdown } from '../format';
  import Screenshot from '../Screenshot.svelte';

  let {
    step,
    number,
    uploading = false,
    onpick,
    onclearhighlight,
    onremovemedia,
    onimage,
  }: {
    step: Step | null;
    /** Step number for action steps, 0 otherwise. */
    number: number;
    uploading?: boolean;
    onpick: (x: number, y: number, w: number, h: number) => void;
    onclearhighlight: () => void;
    onremovemedia: () => void;
    /** Add or replace this step's screenshot. */
    onimage: (file: File) => void;
  } = $props();

  let fileInput: HTMLInputElement | undefined = $state();
  const pad = (n: number) => String(n).padStart(2, '0');

  // Same wording as the reader: "Tip" / "Important", plus the title unless it only repeats that word.
  function calloutTitle(s: Step): string {
    const base = s.kind === 'tip' ? 'Tip' : 'Important';
    const t = s.title.trim();
    return !t || t.toLowerCase() === base.toLowerCase() ? base : `${base}: ${t}`;
  }

  function picked(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const f = input.files?.[0];
    input.value = '';
    if (f) onimage(f);
  }
</script>

<section class="canvas" aria-label="Screenshot">
  {#if !step}
    <div class="blank muted">
      <p>This guide has no steps yet.</p>
      <p class="small">Add a step from the list, or paste a screenshot (Ctrl/⌘+V) to start.</p>
    </div>
  {:else}
    {#if step.media_id || step.kind !== 'heading' || uploading}
    <div class="toolbar" role="toolbar" aria-label="Screenshot tools">
      {#if step.media_id}
        <button type="button" class="small tool active" aria-pressed="true" title="Click the screenshot to place the highlight">
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><rect x="2" y="4" width="12" height="8" rx="2" fill="none" stroke="currentColor" stroke-width="1.6" /></svg>
          Highlight
        </button>
        {#if step.annotations?.highlight}
          <button type="button" class="small ghost" onclick={onclearhighlight} aria-label="Remove highlight" title="Remove highlight">Clear</button>
        {/if}
        <span class="grow" aria-hidden="true"></span>
        <button type="button" class="small ghost" onclick={() => fileInput?.click()} disabled={uploading}>Replace image</button>
        <button type="button" class="small ghost danger" onclick={onremovemedia}>Remove screenshot</button>
      {:else if step.kind !== 'heading'}
        <button type="button" class="small" onclick={() => fileInput?.click()} disabled={uploading}>Add screenshot</button>
      {/if}
      {#if uploading}<span class="muted small" role="status">Uploading…</span>{/if}
    </div>
    {/if}

    <div class="stage">
      {#if step.media_id}
        <div class="shot">
          {#key step.id}
            <Screenshot mediaId={step.media_id} highlight={step.annotations?.highlight} alt={step.title} {onpick} />
          {/key}
        </div>
        <p class="muted small help">
          {step.annotations?.highlight ? 'Click the screenshot to move the highlight.' : 'Click the screenshot to place the highlight.'}
        </p>
      {:else if step.kind === 'heading'}
        <div class="preview">
          <span class="kind mono">SECTION</span>
          <p class="sec">{step.title || 'Untitled section'}</p>
          <p class="muted small">Section headings group the steps below them. They have no screenshot.</p>
        </div>
      {:else if step.kind === 'tip' || step.kind === 'alert'}
        <div class="preview">
          <div class="callout {step.kind}">
            <strong>{calloutTitle(step)}</strong>
            {#if step.body_md.trim()}<div class="md">{@html miniMarkdown(step.body_md)}</div>{/if}
          </div>
          <p class="muted small">No screenshot. Tips and alerts usually do not need one.</p>
        </div>
      {:else}
        <div class="preview">
          {#if number}<span class="kind mono">STEP {pad(number)}</span>{:else}<span class="kind mono">TEXT</span>{/if}
          <p class="sec">{step.title || 'Untitled step'}</p>
          <div class="noshot">
            <p>No screenshot for this step.</p>
            <p class="small muted">Use <strong>Add screenshot</strong>, or paste one (Ctrl/⌘+V) to add it as a new step.</p>
          </div>
        </div>
      {/if}
    </div>
  {/if}
  <input bind:this={fileInput} type="file" accept="image/png,image/jpeg,image/webp,image/gif" hidden onchange={picked} />
</section>

<style>
  .canvas { border: 1px solid var(--line); border-radius: var(--radius-lg); background: var(--card); min-width: 0; display: flex; flex-direction: column; }
  .toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 0.3rem; padding: 0.5rem 0.6rem; border-bottom: 1px solid var(--line); min-height: 46px; }
  .tool { display: inline-flex; align-items: center; gap: 0.35rem; }
  .tool.active { background: var(--sig-soft); border-color: var(--sig); color: var(--sig-text); font-weight: 600; cursor: default; }
  .grow { flex: 1 1 0; }
  .toolbar button { padding: 0.22rem 0.5rem; font-size: 0.82rem; }
  .stage { padding: 1rem; background: var(--surface-2); flex: 1; border-radius: var(--radius-lg); min-width: 0; }
  .toolbar + .stage { border-radius: 0 0 var(--radius-lg) var(--radius-lg); }
  .shot { text-align: center; line-height: 0; }
  .help.muted { margin: 0.6rem 0 0; text-align: center; color: var(--muted-strong); }
  .preview { background: var(--card); border: 1px solid var(--line); border-radius: var(--radius-lg); padding: 1.25rem; max-width: 560px; margin: 1.5rem auto; }
  .preview > p:last-child { margin-bottom: 0; }
  .kind { font-size: 0.72rem; letter-spacing: 0.06em; color: var(--sig-text); font-weight: 500; }
  .sec { font-size: 1.15rem; font-weight: 600; margin: 0.3rem 0 0.75rem; overflow-wrap: anywhere; }
  .noshot { border: 1px dashed var(--line); border-radius: var(--radius); padding: 1.25rem; text-align: center; }
  .noshot p { margin: 0.2rem 0; }
  .md :global(p) { margin: 0.35rem 0 0; }
  .md :global(ul) { margin: 0.35rem 0 0; padding-left: 1.2rem; }
  .blank { padding: 3rem 1.5rem; text-align: center; }
</style>
