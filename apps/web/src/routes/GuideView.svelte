<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { tick } from 'svelte';
  import { api, exportUrl, type Folder, type Guide, type Step } from '../lib/api';
  import { go } from '../lib/router.svelte';
  import { workspacesStore } from '../lib/session.svelte';
  import { errorText, dateTime, miniMarkdown } from '../lib/format';
  import Screenshot from '../lib/Screenshot.svelte';

  let { id }: { id: string } = $props();
  let guide = $state<Guide | null>(null);
  let error = $state('');
  let folder = $state<Folder | null>(null);

  $effect(() => {
    api<Guide>(`/guides/${id}`)
      .then((g) => (guide = g))
      .catch((e) => (error = errorText(e)));
  });

  // Folder name for the crumb; the workspace name comes from the shared store.
  $effect(() => {
    const g = guide;
    folder = null;
    if (!g?.folder_id) return;
    api<Folder[]>(`/workspaces/${g.workspace_id}/folders`)
      .then((fs) => (folder = fs.find((f) => f.id === g.folder_id) ?? null))
      .catch(() => {});
  });
  let workspaceName = $derived(workspacesStore.list.find((w) => w.id === guide?.workspace_id)?.name ?? 'Workspace');

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
  const pad = (n: number) => String(n).padStart(2, '0');

  // Index entries: section headings and numbered steps.
  type NavItem = { id: string; heading: boolean; n: number; title: string };
  let nav = $derived<NavItem[]>(
    (guide?.steps ?? []).flatMap((s, i): NavItem[] =>
      s.kind === 'heading' ? [{ id: s.id, heading: true, n: 0, title: s.title }]
      : s.kind === 'action' ? [{ id: s.id, heading: false, n: numbers[i], title: s.title || 'Untitled step' }]
      : [],
    ),
  );
  let stepCount = $derived(numbers.filter(Boolean).length);

  // Splits `Click "Employees"` into plain and quoted-label parts.
  function parts(title: string): { text: string; label: boolean }[] {
    return title
      .split(/"([^"]+)"/)
      .map((text, i) => ({ text, label: i % 2 === 1 }))
      .filter((p) => p.text);
  }

  function calloutTitle(s: Step): string {
    const base = s.kind === 'tip' ? 'Tip' : 'Important';
    const t = s.title.trim();
    return !t || t.toLowerCase() === base.toLowerCase() ? base : `${base}: ${t}`;
  }

  function shotLabel(s: Step): string {
    const a = s.action;
    if (a?.typed_text) return `TYPE · ${a.typed_text}`;
    return (a?.type || 'click').replace(/[_-]/g, ' ').toUpperCase();
  }

  function sourceText(g: Guide): string {
    const first = g.steps.find((s) => s.action?.url || s.action?.app)?.action;
    let where = first?.app ?? '';
    if (first?.url) {
      try {
        where = new URL(first.url).host;
      } catch {
        where = first.url;
      }
    }
    if (!where) return '';
    if (g.source === 'extension') return `Recorded in Chrome · ${where}`;
    if (g.source === 'desktop') return `Recorded on desktop · ${where}`;
    return '';
  }

  // True while a programmatic jump is scrolling, so the observer does not fight it.
  let jumping = false;
  let jumpTimer: ReturnType<typeof setTimeout> | undefined;
  const atBottom = () => window.scrollY > 0 && window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 2;

  // Current step, tracked with an IntersectionObserver over the step elements.
  let current = $state('');
  let currentIdx = $derived(nav.findIndex((n) => n.id === current));
  let stepEls: Record<string, HTMLElement> = {};
  function track(node: HTMLElement, sid: string) {
    stepEls[sid] = node;
    return { destroy: () => delete stepEls[sid] };
  }

  $effect(() => {
    const items = nav;
    if (!items.length) return;
    if (!items.some((n) => n.id === current)) current = items[0].id;
    const visible = new Set<string>();
    const order = new Map(items.map((n, i) => [n.id, i]));
    let obs: IntersectionObserver | undefined;
    tick().then(() => {
      obs = new IntersectionObserver(
        (entries) => {
          for (const e of entries) {
            const sid = (e.target as HTMLElement).dataset.sid!;
            if (e.isIntersecting) visible.add(sid);
            else visible.delete(sid);
          }
          if (jumping) return;
          if (atBottom()) current = items[items.length - 1].id;
          else if (visible.size) current = [...visible].sort((a, b) => order.get(a)! - order.get(b)!)[0];
        },
        // A band just below the sticky top bar.
        { rootMargin: `${window.matchMedia('(max-width: 1099px)').matches ? -124 : -80}px 0px -60% 0px` },
      );
      for (const n of items) if (stepEls[n.id]) obs.observe(stepEls[n.id]);
    });
    const onScroll = () => {
      if (jumping) {
        clearTimeout(jumpTimer);
        jumpTimer = setTimeout(() => (jumping = false), 120);
      } else if (atBottom()) current = items[items.length - 1].id;
    };
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      obs?.disconnect();
      window.removeEventListener('scroll', onScroll);
      clearTimeout(jumpTimer);
    };
  });

  let indexEl = $state<HTMLElement | null>(null);
  $effect(() => {
    // Keep the highlighted row visible inside the scrollable index.
    if (!indexEl || !current) return;
    const row = indexEl.querySelector<HTMLElement>('[aria-current="step"]');
    if (!row) return;
    const box = indexEl.getBoundingClientRect();
    const r = row.getBoundingClientRect();
    if (r.top < box.top || r.bottom > box.bottom) row.scrollIntoView({ block: 'nearest' });
  });

  function jump(sid: string) {
    const el = stepEls[sid];
    if (!el) return;
    current = sid;
    jumping = true;
    clearTimeout(jumpTimer);
    jumpTimer = setTimeout(() => (jumping = false), 150); // cleared sooner/later by scroll events
    const calm = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    el.scrollIntoView({ behavior: calm ? 'auto' : 'smooth', block: 'start' });
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    if (exportOpen || confirmDelete) return;
    if (e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const t = e.target as HTMLElement | null;
    if (t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))) return;
    if (document.querySelector('dialog[open]')) return;
    const steps = nav.filter((n) => !n.heading);
    if (!steps.length) return;
    const pos = currentIdx < 0 ? -1 : nav.slice(0, currentIdx + 1).filter((n) => !n.heading).length - 1;
    const atHeading = currentIdx >= 0 && nav[currentIdx].heading;
    let next: number;
    if (e.key === 'ArrowDown') next = pos + 1;
    else next = atHeading ? pos : pos - 1;
    next = Math.max(0, Math.min(steps.length - 1, next));
    e.preventDefault();
    jump(steps[next].id);
    tick().then(() => indexEl?.querySelector<HTMLElement>('.idx-row.cur')?.focus({ preventScroll: true }));
  }

  let exportOpen = $state(false);
  let exportBtn = $state<HTMLButtonElement | null>(null);
  let withImages = $state(true);
  const formats = [
    { f: 'pdf', label: 'PDF' },
    { f: 'docx', label: 'Word (.docx)' },
    { f: 'html', label: 'HTML (single file)' },
    { f: 'md', label: 'Markdown (.zip)' },
    { f: 'png', label: 'Screenshots (.zip)' },
  ];

  function onDocClick(e: MouseEvent) {
    if (exportOpen && !(e.target as HTMLElement).closest('.export')) exportOpen = false;
  }

  async function duplicate() {
    error = '';
    try {
      const g = await api<Guide>(`/guides/${id}/duplicate`, { body: {} });
      workspacesStore.refresh().catch(() => {});
      go(`/guides/${g.id}/edit`);
    } catch (e) {
      error = errorText(e);
    }
  }

  let confirmDelete = $state(false);
  let deleting = $state(false);
  async function remove() {
    deleting = true;
    error = '';
    try {
      await api(`/guides/${id}`, { method: 'DELETE' });
      workspacesStore.refresh().catch(() => {});
      go('/');
    } catch (e) {
      error = errorText(e);
      deleting = false;
    }
  }
</script>

<svelte:document onclick={onDocClick} />
<svelte:window onkeydown={(e) => {
  if (e.key !== 'Escape') return;
  if (exportOpen) { exportOpen = false; exportBtn?.focus(); }
  confirmDelete = false;
}} />

{#if error && !guide}
  <p class="error">{error}</p>
{:else if !guide}
  <p class="muted">Loading…</p>
{:else}
  <header class="ghead">
    <nav class="crumb" aria-label="Location">
      <a href={`#/?ws=${guide.workspace_id}`} title={workspaceName}>{workspaceName}</a>
      {#if folder}<a href={`#/?ws=${guide.workspace_id}&folder=${folder.id}`} title={folder.name}>{folder.name}</a>{/if}
    </nav>
    <div class="spread top">
      <h1 title={guide.title}>{guide.title}</h1>
      <div class="row actions">
        <div class="export">
          <button bind:this={exportBtn} onclick={() => (exportOpen = !exportOpen)} aria-expanded={exportOpen} aria-haspopup="true">Export ▾</button>
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
          <button class="danger" onclick={() => (confirmDelete = !confirmDelete)} aria-expanded={confirmDelete}>Delete</button>
          <a class="btn primary" href={`#/guides/${guide.id}/edit`}>Edit</a>
        {/if}
      </div>
    </div>
    {#if confirmDelete}
      <div class="callout alert confirm" role="alertdialog" aria-label="Confirm delete">
        <p><strong>Delete this guide?</strong> <span class="small">Administrators can still see it in the audit log.</span></p>
        <div class="row">
          <button class="danger" onclick={remove} disabled={deleting}>Delete guide</button>
          <button onclick={() => (confirmDelete = false)} disabled={deleting}>Cancel</button>
        </div>
      </div>
    {/if}
    <div class="meta row">
      {#if guide.status === 'published'}<span class="tag pub">Published v{guide.current_version}</span>{:else}<span class="tag draft">{guide.status}</span>{/if}
      {#if guide.needs_review}<span class="tag review">Needs review</span>{/if}
      <span class="tag class">{guide.classification}</span>
      <span class="small muted">{guide.author_name ?? 'Unknown'} · updated {dateTime(guide.updated_at)}</span>
      {#if guide.reviewed_at}<span class="small muted">Last reviewed {new Date(guide.reviewed_at).toLocaleDateString()}</span>{/if}
      {#if sourceText(guide)}<span class="small muted mono">{sourceText(guide)}</span>{/if}
    </div>
  </header>

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

  <div class="reader">
    {#if nav.length}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <nav class="index" aria-label="Steps" bind:this={indexEl} onkeydown={onKey}>
        <div class="idx-title mono">STEPS · {stepCount}</div>
        <ul>
          {#each nav as n (n.id)}
            <li>
              {#if n.heading}
                <button class="idx-head" onclick={() => jump(n.id)} aria-current={n.id === current ? 'step' : undefined} title={n.title}>{n.title}</button>
              {:else}
                <button
                  class="idx-row"
                  class:cur={n.id === current}
                  class:done={currentIdx >= 0 && nav.indexOf(n) < currentIdx}
                  onclick={() => jump(n.id)}
                  aria-current={n.id === current ? 'step' : undefined}
                  title={n.title}
                >
                  <span class="idx-n mono">{pad(n.n)}</span>
                  <span class="idx-t">{n.title}</span>
                </button>
              {/if}
            </li>
          {/each}
        </ul>
        <div class="idx-hint small muted"><span class="kbd">↑</span> <span class="kbd">↓</span> to move</div>
      </nav>
      <div class="idx-select">
        <label for="step-jump" class="small muted">Steps</label>
        <select id="step-jump" value={current} onchange={(e) => jump(e.currentTarget.value)}>
          {#each nav as n (n.id)}
            <option value={n.id}>{n.heading ? n.title : `${pad(n.n)}  ${n.title}`}</option>
          {/each}
        </select>
      </div>
    {/if}

    <ol class="steps">
      {#each guide.steps as s, i (s.id)}
        <li class="step {s.kind}" data-sid={s.id} use:track={s.id}>
          {#if s.kind === 'heading'}
            <h2 title={s.title}>{s.title}</h2>
          {:else if s.kind === 'tip' || s.kind === 'alert'}
            <div class="callout {s.kind}">
              <strong>{calloutTitle(s)}</strong>
              {#if s.body_md}<div>{@html miniMarkdown(s.body_md)}</div>{/if}
            </div>
          {:else if s.kind === 'text'}
            <div class="step-card text-step">
              {#if s.title}<h3 title={s.title}>{#each parts(s.title) as p}{#if p.label}<span class="chip">{p.text}</span>{:else}{p.text}{/if}{/each}</h3>{/if}
              {#if s.body_md}<div class="body">{@html miniMarkdown(s.body_md)}</div>{/if}
              {#if s.media_id}
                <div class="shotwrap">
                  <Screenshot mediaId={s.media_id} highlight={s.annotations?.highlight} alt={s.title || 'Screenshot'} label={s.annotations?.highlight ? shotLabel(s) : ''} />
                </div>
              {/if}
            </div>
          {:else}
            <div class="step-card">
              <div class="big mono" aria-hidden="true">{pad(numbers[i])}</div>
              <h3>
                <span class="sr">Step {numbers[i]}: </span>
                {#each parts(s.title || 'Untitled step') as p}{#if p.label}<span class="chip">{p.text}</span>{:else}{p.text}{/if}{/each}
              </h3>
              {#if s.body_md}<div class="body">{@html miniMarkdown(s.body_md)}</div>{/if}
              {#if s.media_id}
                <div class="shotwrap">
                  <Screenshot mediaId={s.media_id} highlight={s.annotations?.highlight} alt={s.title} label={s.annotations?.highlight ? shotLabel(s) : ''} />
                </div>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ol>
  </div>
  {#if guide.steps.length === 0}<p class="muted">This guide has no steps yet.</p>{/if}
{/if}

<style>
  .ghead { margin-bottom: 1rem; }
  .ghead .crumb a { max-width: 24ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .top { align-items: flex-start; margin-top: 0.35rem; }
  h1 { margin: 0; font-size: 1.6rem; line-height: 1.25; min-width: 0; flex: 1 1 24rem; overflow-wrap: anywhere; }
  .actions { flex: none; }
  .meta { margin-top: 0.6rem; gap: 0.25rem 0.9rem; }
  .mono { font-family: var(--font-mono); }
  .export { position: relative; }
  .menu { position: absolute; right: 0; top: calc(100% + 4px); z-index: 30; min-width: 220px; padding: 0.4rem; display: flex; flex-direction: column; }
  .menu a { padding: 0.4rem 0.6rem; border-radius: 6px; color: var(--text); }
  .menu a:hover { background: var(--surface-2); text-decoration: none; }
  .menu label { padding: 0.4rem 0.6rem; margin: 0; border-top: 1px solid var(--border); }
  .confirm { display: flex; justify-content: space-between; align-items: center; gap: 1rem; flex-wrap: wrap; margin-top: 0.75rem; }
  .confirm p { margin: 0; }
  .review-notice { justify-content: space-between; gap: 1rem; margin: 0.75rem 0; padding: 0.6rem 1rem; border-radius: var(--radius); border: 1px solid var(--warn-line); background: var(--warn-soft); color: var(--text); }
  .review-notice p { margin: 0; }
  .lead { font-size: 1.05rem; max-width: 70ch; }

  .reader { display: grid; grid-template-columns: 260px minmax(0, 1fr); gap: 2rem; align-items: start; margin-top: 1.25rem; }
  .index { position: sticky; top: 72px; max-height: calc(100vh - 88px); overflow-y: auto; border: 1px solid var(--line); border-radius: var(--radius-lg); background: var(--card); padding: 0.6rem 0; }
  .index ul { list-style: none; margin: 0; padding: 0; }
  .idx-title { font-size: 0.72rem; letter-spacing: 0.06em; color: var(--muted); padding: 0.1rem 0.9rem 0.45rem; }
  .index button { display: flex; justify-content: flex-start; width: 100%; align-items: baseline; gap: 0.55rem; text-align: left; border: 0; border-radius: 0; background: transparent; color: var(--text); padding: 0.32rem 0.9rem; font-size: 0.88rem; box-shadow: inset 2px 0 0 transparent; }
  .index button:hover { background: var(--surface-2); }
  .index button:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
  .idx-head { font-weight: 700; margin-top: 0.4rem; padding-top: 0.5rem !important; }
  .idx-n { flex: none; font-size: 0.75rem; color: var(--muted-strong); }
  .idx-t { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .idx-row.done .idx-n { color: var(--ok-text); }
  .idx-row.cur { background: var(--sig-soft); box-shadow: inset 2px 0 0 var(--sig); font-weight: 500; }
  .idx-row.cur:hover { background: var(--sig-soft); }
  .idx-row.cur .idx-n { color: var(--sig-text); }
  .idx-hint { padding: 0.5rem 0.9rem 0.15rem; border-top: 1px solid var(--line); margin-top: 0.5rem; }
  .idx-select { display: none; }

  .steps { list-style: none; padding: 0; margin: 0; max-width: 760px; min-width: 0; }
  .step { margin-bottom: 2rem; scroll-margin-top: 76px; }
  .step.heading h2 { margin: 1.25rem 0 0; font-size: 1.25rem; padding-bottom: 0.4rem; border-bottom: 1px solid var(--line); overflow-wrap: anywhere; }
  .step-card { border-top: 3px solid var(--sig); padding-top: 0.5rem; }
  .step-card.text-step { border-top: 0; padding-top: 0; }
  .big { font-size: 1.9rem; font-weight: 500; line-height: 1.1; color: var(--sig-text); }
  .step-card h3 { margin: 0.25rem 0 0; font-size: 1.1rem; line-height: 1.5; overflow-wrap: anywhere; }
  .chip { font-family: var(--font-mono); font-size: 0.92em; font-weight: 500; background: var(--sig-soft); color: var(--sig-text); padding: 0.05rem 0.4rem; border-radius: 4px; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .body { margin: 0.4rem 0 0.2rem; }
  .shotwrap { margin-top: 0.75rem; max-width: 620px; }
  .shotwrap :global(.shot) { display: block; }
  .callout { overflow-wrap: anywhere; }
  .callout :global(p) { margin: 0.3rem 0 0; }

  @media (max-width: 1099px) {
    .reader { grid-template-columns: minmax(0, 1fr); gap: 0.75rem; }
    .index { display: none; }
    .idx-select { display: flex; align-items: center; gap: 0.6rem; position: sticky; top: 56px; z-index: 5; background: var(--paper); padding: 0.5rem 0; }
    .idx-select label { margin: 0; flex: none; font-family: var(--font-mono); text-transform: uppercase; letter-spacing: 0.05em; }
    .idx-select select { min-width: 0; }
    .step { scroll-margin-top: 120px; }
  }
</style>
