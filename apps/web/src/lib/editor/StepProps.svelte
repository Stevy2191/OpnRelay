<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- Editor properties for the selected step: type, title, details, recorded action. -->
<script lang="ts">
  import type { Step, StepKind } from '../api';

  let {
    step = $bindable(),
    onchange,
    onduplicate,
    ondelete,
    titleInput = $bindable(),
  }: {
    step: Step;
    onchange: () => void;
    onduplicate: () => void;
    ondelete: () => void;
    titleInput?: HTMLInputElement;
  } = $props();

  const kinds: { kind: StepKind; label: string }[] = [
    { kind: 'action', label: 'Step' },
    { kind: 'heading', label: 'Heading' },
    { kind: 'text', label: 'Text' },
    { kind: 'tip', label: 'Tip' },
    { kind: 'alert', label: 'Alert' },
  ];

  let confirming = $state(false);
  // Close the delete confirmation when another step is selected.
  $effect(() => {
    void step.id;
    confirming = false;
  });

  function setKind(k: StepKind) {
    if (step.kind === k) return;
    step.kind = k;
    onchange();
  }

  // Arrow keys move between the type options (radio group pattern).
  function kindKey(e: KeyboardEvent, i: number) {
    const d = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    const next = (i + d + kinds.length) % kinds.length;
    setKind(kinds[next].kind);
    ((e.currentTarget as HTMLElement).parentElement?.children[next] as HTMLElement | undefined)?.focus();
  }

  let actionRows = $derived.by(() => {
    const a = step.action;
    if (!a) return [];
    const rows: [string, string][] = [['type', a.type]];
    if (a.target_label) rows.push(['label', a.target_label]);
    if (a.target_role) rows.push(['role', a.target_role]);
    if (a.typed_text) rows.push(['typed', a.typed_text]);
    if (a.keys) rows.push(['keys', a.keys]);
    if (a.app) rows.push(['app', a.app]);
    if (a.window_title) rows.push(['window', a.window_title]);
    if (a.url) rows.push(['url', a.url]);
    return rows;
  });
</script>

<section class="props" aria-label="Step properties">
  <div class="field">
    <span class="lbl" id="kind-lbl">Type</span>
    <div class="seg" role="radiogroup" aria-labelledby="kind-lbl">
      {#each kinds as k, i (k.kind)}
        <button
          type="button"
          role="radio"
          aria-checked={step.kind === k.kind}
          tabindex={step.kind === k.kind ? 0 : -1}
          class:on={step.kind === k.kind}
          onclick={() => setKind(k.kind)}
          onkeydown={(e) => kindKey(e, i)}
        >{k.label}</button>
      {/each}
    </div>
  </div>

  <label for="step-title">{step.kind === 'heading' ? 'Section title' : 'Title'}</label>
  <input
    id="step-title"
    bind:this={titleInput}
    bind:value={step.title}
    oninput={onchange}
    maxlength="300"
    placeholder={step.kind === 'heading' ? 'Section title' : step.kind === 'action' ? 'Step title' : 'Optional title'}
  />

  {#if step.kind !== 'heading'}
    <label for="step-body">Details</label>
    <textarea
      id="step-body"
      bind:value={step.body_md}
      oninput={onchange}
      rows="5"
      placeholder="Optional. Supports **bold**, *italic*, `code` and - lists."
    ></textarea>
  {/if}

  {#if actionRows.length}
    <span class="lbl">Recorded action</span>
    <dl class="action mono">
      {#each actionRows as [k, v] (k)}
        <div><dt>{k}</dt><dd>{v}</dd></div>
      {/each}
    </dl>
  {/if}

  <div class="foot">
    {#if confirming}
      <div class="confirm callout alert" role="alertdialog" aria-label="Confirm delete step" tabindex="-1" onkeydown={(e) => { if (e.key === 'Escape') confirming = false; }}>
        <p>Delete this step? This cannot be undone after it is saved.</p>
        <div class="row">
          <button type="button" class="small danger" onclick={() => { confirming = false; ondelete(); }}>Delete step</button>
          <!-- svelte-ignore a11y_autofocus -->
          <button type="button" class="small" autofocus onclick={() => (confirming = false)}>Cancel</button>
        </div>
      </div>
    {:else}
      <div class="row">
        <button type="button" class="small" onclick={onduplicate}>Duplicate</button>
        <button type="button" class="small danger" onclick={() => (confirming = true)}>Delete step</button>
      </div>
    {/if}
  </div>
</section>

<style>
  .props { border: 1px solid var(--line); border-radius: var(--radius-lg); background: var(--card); padding: 0.4rem 1rem 1rem; min-width: 0; }
  .lbl { display: block; font-weight: 500; font-size: 0.9rem; margin: 0.75rem 0 0.3rem; }
  .seg { display: flex; border: 1px solid var(--line); border-radius: var(--radius); overflow: hidden; background: var(--surface-2); }
  .seg button { flex: 1 1 auto; border: 0; border-radius: 0; padding: 0.35rem 0.15rem; font-size: 0.78rem; background: transparent; color: var(--muted-strong); min-width: 0; }
  .seg button + button { border-left: 1px solid var(--line); }
  .seg button:hover:not(.on) { background: var(--card); }
  .seg button.on { background: var(--sig-soft); color: var(--sig-text); font-weight: 600; box-shadow: inset 0 -2px 0 var(--sig); }
  .seg button:focus-visible { outline-offset: -2px; }
  .action { margin: 0; padding: 0.55rem 0.7rem; background: var(--surface-2); border: 1px solid var(--line); border-radius: var(--radius); font-size: 0.78rem; }
  .action div { display: grid; grid-template-columns: 4.2rem minmax(0, 1fr); gap: 0.5rem; padding: 0.1rem 0; }
  .action dt { color: var(--muted-strong); }
  .action dd { margin: 0; overflow-wrap: anywhere; }
  .foot { margin-top: 1.25rem; padding-top: 0.85rem; border-top: 1px solid var(--line); }
  .confirm p { margin: 0 0 0.5rem; font-size: 0.88rem; }
</style>
