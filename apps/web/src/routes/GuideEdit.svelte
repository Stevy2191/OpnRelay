<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- Guide editor: one step at a time, saved automatically. -->
<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { api, ApiError, type Folder, type Guide, type Step, type StepKind } from '../lib/api';
  import { go } from '../lib/router.svelte';
  import { workspacesStore } from '../lib/session.svelte';
  import { errorText, when } from '../lib/format';
  import { createAutosave, type SaveState } from '../lib/editor/autosave';
  import StepList from '../lib/editor/StepList.svelte';
  import StepCanvas from '../lib/editor/StepCanvas.svelte';
  import StepProps from '../lib/editor/StepProps.svelte';

  let { id }: { id: string } = $props();

  type Version = { version: number; note: string; author_name: string | null; created_at: string };

  let guide = $state<Guide | null>(null);
  let steps = $state<Step[]>([]);
  let folders = $state<Folder[]>([]);
  let versions = $state<Version[]>([]);
  let meta = $state({ title: '', description: '', classification: 'internal', folder_id: '' as string });
  let selectedId = $state('');
  let saveState = $state<SaveState>('saved');
  let conflict = $state(false);
  let error = $state('');
  let notice = $state('');
  let loadError = $state('');
  let uploading = $state(false);
  let titleInput: HTMLInputElement | undefined = $state();
  let uploadInput: HTMLInputElement | undefined = $state();
  /** Steps as last saved on the server (JSON), so unchanged steps are not re-sent. */
  let savedSteps = '';
  /** Set when the user chose to leave or reload; skips the unsaved-changes guard. */
  let leaving = false;

  const autosave = createAutosave(save, 1500, (s) => (saveState = s));

  function newId(): string {
    // UUIDv7-like time-ordered id generated client side.
    const ms = Date.now().toString(16).padStart(12, '0');
    const rnd = crypto.getRandomValues(new Uint8Array(10));
    const hex = Array.from(rnd, (b) => b.toString(16).padStart(2, '0')).join('');
    return `${ms.slice(0, 8)}-${ms.slice(8, 12)}-7${hex.slice(0, 3)}-${((parseInt(hex[3], 16) & 3) | 8).toString(16)}${hex.slice(4, 7)}-${hex.slice(7, 19)}`;
  }

  async function load() {
    try {
      const g = await api<Guide>(`/guides/${id}`);
      if (g.my_permission !== 'edit' && g.my_permission !== 'manage') {
        go(`/guides/${id}`);
        return;
      }
      guide = g;
      steps = g.steps.map((s) => ({ ...s, annotations: s.annotations ?? {} }));
      savedSteps = JSON.stringify(steps);
      meta = { title: g.title, description: g.description, classification: g.classification, folder_id: g.folder_id ?? '' };
      if (!steps.some((s) => s.id === selectedId)) selectedId = steps[0]?.id ?? '';
      [folders, versions] = await Promise.all([
        api<Folder[]>(`/workspaces/${g.workspace_id}/folders`),
        api<Version[]>(`/guides/${id}/versions`),
      ]);
    } catch (e) {
      if (guide) error = errorText(e);
      else loadError = errorText(e);
    }
  }

  /** Saves guide details (PATCH) and steps (PUT), each only when changed, with If-Match. */
  async function save(): Promise<boolean> {
    if (!guide || conflict) return false;
    try {
      const m = {
        title: meta.title,
        description: meta.description,
        classification: meta.classification,
        folder_id: meta.folder_id || null,
      };
      if (
        m.title !== guide.title ||
        m.description !== guide.description ||
        m.classification !== guide.classification ||
        m.folder_id !== guide.folder_id
      ) {
        // Keep the latest revision even if the next request fails.
        guide = await api<Guide>(`/guides/${id}`, { method: 'PATCH', ifMatch: guide.revision, body: m });
      }
      const snap = JSON.stringify($state.snapshot(steps));
      if (snap !== savedSteps) {
        guide = await api<Guide>(`/guides/${id}/steps`, { method: 'PUT', ifMatch: guide.revision, body: { steps: JSON.parse(snap) } });
        savedSteps = snap;
      }
      error = '';
      return true;
    } catch (e) {
      if (e instanceof ApiError && (e.status === 412 || e.status === 409)) {
        conflict = true;
        autosave.stop();
      } else {
        error = `Could not save: ${errorText(e)}`;
      }
      return false;
    }
  }

  function touch() {
    notice = '';
    autosave.touch();
  }

  onMount(() => {
    load();
    const onPaste = (e: ClipboardEvent) => {
      const target = e.target as HTMLElement;
      if (target && (['INPUT', 'TEXTAREA'].includes(target.tagName) || target.isContentEditable)) return;
      const files = Array.from(e.clipboardData?.files ?? []).filter((f) => f.type.startsWith('image/'));
      if (files.length) {
        e.preventDefault();
        addImages(files);
      }
    };
    const beforeUnload = (e: BeforeUnloadEvent) => {
      if (!leaving && (autosave.state !== 'saved' || uploading)) e.preventDefault();
    };
    window.addEventListener('paste', onPaste);
    window.addEventListener('beforeunload', beforeUnload);
    return () => {
      window.removeEventListener('paste', onPaste);
      window.removeEventListener('beforeunload', beforeUnload);
      // Route change (sidebar, back button): save what is left.
      void autosave.flush();
    };
  });

  // ---- Selection and step operations ----

  let selIdx = $derived(steps.findIndex((s) => s.id === selectedId));
  let numbers = $derived.by(() => {
    let n = 0;
    return steps.map((s) => (s.kind === 'action' ? ++n : 0));
  });

  function select(sid: string) {
    selectedId = sid;
  }

  async function editTitle(sid: string) {
    selectedId = sid;
    await tick();
    titleInput?.focus();
  }

  function insert(step: Step) {
    const at = selIdx >= 0 ? selIdx + 1 : steps.length;
    steps.splice(at, 0, step);
    selectedId = step.id;
    touch();
  }

  function add(kind: StepKind) {
    const titles: Record<StepKind, string> = { action: 'New step', text: '', heading: 'New section', tip: '', alert: '' };
    const s: Step = { id: newId(), kind, title: titles[kind], body_md: '', annotations: {} };
    insert(s);
    editTitle(s.id);
  }

  function move(from: number, to: number) {
    if (from === to || to < 0 || to >= steps.length) return;
    const [s] = steps.splice(from, 1);
    steps.splice(to, 0, s);
    touch();
  }

  function duplicate() {
    if (selIdx < 0) return;
    const copy = { ...$state.snapshot(steps[selIdx]), id: newId() } as Step;
    insert(copy);
  }

  function removeSelected() {
    const i = selIdx;
    if (i < 0) return;
    steps.splice(i, 1);
    selectedId = (steps[i] ?? steps[i - 1])?.id ?? '';
    touch();
    tick().then(() => document.querySelector<HTMLElement>(`.cols [data-sid="${selectedId}"]`)?.focus());
  }

  async function upload(f: File): Promise<string | null> {
    try {
      const m = await api<{ id: string; width: number; height: number }>(`/media?guide_id=${id}`, {
        raw: f,
        contentType: f.type || 'application/octet-stream',
      });
      return m.id;
    } catch (e) {
      error = `Upload failed: ${errorText(e)}`;
      return null;
    }
  }

  /** Each image becomes a new step after the selected one. */
  async function addImages(files: File[]) {
    uploading = true;
    try {
      for (const f of files) {
        const mid = await upload(f);
        if (mid) insert({ id: newId(), kind: 'action', title: 'New step', body_md: '', media_id: mid, annotations: {} });
      }
    } finally {
      uploading = false;
    }
  }

  /** Adds or replaces the selected step's screenshot. */
  async function setImage(f: File) {
    const sid = selectedId;
    uploading = true;
    const mid = await upload(f);
    uploading = false;
    const s = steps.find((x) => x.id === sid);
    if (!mid || !s) return;
    s.media_id = mid;
    // The highlight was placed on the old image.
    s.annotations = { ...s.annotations, highlight: undefined };
    touch();
  }

  function setHighlight(x: number, y: number, w: number, h: number) {
    const s = steps[selIdx];
    if (!s) return;
    const bw = Math.max(40, Math.round(w * 0.12));
    const bh = Math.max(24, Math.round(h * 0.06));
    s.annotations = {
      ...s.annotations,
      highlight: { x: Math.max(0, Math.round(x - bw / 2)), y: Math.max(0, Math.round(y - bh / 2)), w: bw, h: bh },
    };
    touch();
  }

  function clearHighlight() {
    const s = steps[selIdx];
    if (!s) return;
    s.annotations = { ...s.annotations, highlight: undefined };
    touch();
  }

  function removeMedia() {
    const s = steps[selIdx];
    if (!s) return;
    s.media_id = null;
    s.annotations = { ...s.annotations, highlight: undefined };
    touch();
  }

  // ---- Status, leaving, preview ----

  let edited = $derived(!!guide && (guide.status !== 'published' || (!!guide.published_at && guide.updated_at > guide.published_at) || saveState !== 'saved'));
  let nextVersion = $derived((guide?.current_version ?? 0) + 1);
  let statusTag = $derived(!guide ? '' : edited ? `DRAFT · editing v${nextVersion}` : `PUBLISHED v${guide.current_version}`);
  let saveText = $derived(
    conflict ? 'Not saved' : saveState === 'saving' ? 'Saving…' : saveState === 'pending' ? 'Unsaved changes' : saveState === 'error' ? 'Couldn’t save' : 'Saved',
  );

  /** Path the user wants to go to while changes are unsaved (in-page confirmation). */
  let leaveTo = $state('');

  async function leave(path: string) {
    await autosave.flush();
    if (conflict || autosave.state !== 'saved') {
      leaveTo = path;
      return;
    }
    leaving = true;
    go(path);
  }

  function leaveAnyway() {
    const path = leaveTo;
    leaveTo = '';
    leaving = true;
    autosave.stop();
    go(path);
  }

  async function preview() {
    // Open the tab now (popup blockers), point it at the reader once saved.
    const w = window.open('about:blank', '_blank');
    await autosave.flush();
    const url = `${location.pathname}${location.search}#/guides/${id}`;
    if (w) w.location.href = url;
    else go(`/guides/${id}`);
  }

  function reload() {
    leaving = true;
    location.reload();
  }

  // ---- Dialogs: guide details, history, publish ----

  let detailsDlg: HTMLDialogElement | undefined = $state();
  let historyDlg: HTMLDialogElement | undefined = $state();
  let publishDlg: HTMLDialogElement | undefined = $state();
  let publishNote = $state('');
  let publishing = $state(false);
  let restoreAsk = $state(0);
  let restoring = $state(false);

  async function openPublish() {
    error = '';
    await autosave.flush();
    if (conflict) return;
    if (autosave.state !== 'saved') {
      error ||= 'Could not save your changes, so the guide was not published. Try again.';
      return;
    }
    publishNote = '';
    publishDlg?.showModal();
  }

  async function publish(e: SubmitEvent) {
    e.preventDefault();
    if (!guide) return;
    publishing = true;
    error = '';
    try {
      await autosave.flush();
      if (conflict || autosave.state !== 'saved') return;
      guide = await api<Guide>(`/guides/${id}/publish`, { body: { note: publishNote.trim() }, ifMatch: guide.revision });
      workspacesStore.refreshReviewCount();
      versions = await api<Version[]>(`/guides/${id}/versions`);
      notice = `Published version ${guide.current_version}`;
      publishDlg?.close();
    } catch (err) {
      if (err instanceof ApiError && (err.status === 412 || err.status === 409)) {
        conflict = true;
        autosave.stop();
        publishDlg?.close();
      } else {
        error = `Could not publish: ${errorText(err)}`;
      }
    } finally {
      publishing = false;
    }
  }

  function openHistory() {
    restoreAsk = 0;
    historyDlg?.showModal();
  }

  async function restore(v: number) {
    restoring = true;
    error = '';
    try {
      await autosave.flush();
      if (conflict) return;
      await api(`/guides/${id}/versions/${v}/restore`, { body: {} });
      await load();
      selectedId = steps[0]?.id ?? '';
      notice = `Restored version ${v}`;
      restoreAsk = 0;
      historyDlg?.close();
    } catch (e) {
      error = `Could not restore: ${errorText(e)}`;
    } finally {
      restoring = false;
    }
  }
</script>

{#if !guide}
  {#if loadError}<p class="error">{loadError}</p>{:else}<p class="muted">Loading…</p>{/if}
{:else}
  <div class="editor">
    <header class="top">
      <button type="button" class="small ghost back" onclick={() => leave(`/guides/${id}`)}>← Done</button>
      <button type="button" class="gtitle" onclick={() => detailsDlg?.showModal()} title={`${meta.title} (edit guide details)`}>
        <span class="gt">{meta.title || 'Untitled guide'}</span>
        <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"><path d="M11.5 2.5l2 2L6 12l-2.6.6L4 10z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" /></svg>
        <span class="visually-hidden">Guide details</span>
      </button>
      <span class="tag {edited ? 'draft' : 'pub'}">{statusTag}</span>
      <span class="save small" class:bad={conflict || saveState === 'error'} aria-live="polite">
        <span class="dot s-{conflict ? 'error' : saveState}" aria-hidden="true"></span>{saveText}
      </span>
      <div class="actions">
        <button type="button" class="small ghost" onclick={openHistory}>History</button>
        <button type="button" class="small" onclick={preview}>Preview</button>
        <button type="button" class="small signal" onclick={openPublish} disabled={conflict || publishing}>Publish v{nextVersion}</button>
      </div>
    </header>

    {#if conflict}
      <div class="callout warning banner" role="alert">
        <span>Someone else changed this guide — reload to see their changes. Your latest edits here were not saved.</span>
        <button type="button" class="small" onclick={reload}>Reload</button>
      </div>
    {/if}
    {#if leaveTo}
      <div class="callout warning banner" role="alertdialog" aria-label="Unsaved changes">
        <span>Your latest changes are not saved. Leave anyway?</span>
        <div class="row">
          <button type="button" class="small danger" onclick={leaveAnyway}>Leave without saving</button>
          <button type="button" class="small" onclick={() => (leaveTo = '')}>Stay</button>
        </div>
      </div>
    {/if}
    {#if error}
      <div class="error banner" role="alert">
        <span>{error}</span>
        {#if saveState === 'error' && !conflict}<button type="button" class="small" onclick={() => autosave.flush()}>Retry</button>{/if}
      </div>
    {/if}
    {#if notice}<p class="success banner" role="status">{notice}</p>{/if}

    <div class="cols">
      <div class="col-list">
        <StepList
          {steps}
          {selectedId}
          onselect={select}
          onmove={move}
          onadd={add}
          onupload={() => uploadInput?.click()}
          onopen={editTitle}
        />
        <p class="muted small paste">Paste a screenshot (Ctrl/⌘+V) anywhere to add it as a step.</p>
        <input
          bind:this={uploadInput}
          type="file"
          accept="image/png,image/jpeg,image/webp,image/gif"
          multiple
          hidden
          onchange={(e) => {
            const input = e.currentTarget as HTMLInputElement;
            const f = Array.from(input.files ?? []);
            input.value = '';
            addImages(f);
          }}
        />
      </div>

      <StepCanvas
        step={steps[selIdx] ?? null}
        number={numbers[selIdx] ?? 0}
        {uploading}
        onpick={setHighlight}
        onclearhighlight={clearHighlight}
        onremovemedia={removeMedia}
        onimage={setImage}
      />

      {#if selIdx >= 0}
        <StepProps bind:step={steps[selIdx]} bind:titleInput onchange={touch} onduplicate={duplicate} ondelete={removeSelected} />
      {:else}
        <div class="props-empty muted small">Select a step to edit its type, title and details.</div>
      {/if}
    </div>
  </div>

  <dialog bind:this={detailsDlg} class="dlg" aria-labelledby="details-title">
    <form method="dialog">
      <h2 id="details-title">Guide details</h2>
      <label for="g-title">Title</label>
      <input id="g-title" bind:value={meta.title} oninput={touch} maxlength="200" required />
      <label for="g-desc">Description</label>
      <textarea id="g-desc" bind:value={meta.description} oninput={touch} rows="3"></textarea>
      <div class="two">
        <div>
          <label for="g-cls">Classification</label>
          <select id="g-cls" bind:value={meta.classification} onchange={touch}>
            <option value="internal">Internal</option>
            <option value="confidential">Confidential</option>
            <option value="restricted">Restricted</option>
          </select>
        </div>
        <div>
          <label for="g-folder">Folder</label>
          <select id="g-folder" bind:value={meta.folder_id} onchange={touch}>
            <option value="">(workspace root)</option>
            {#each folders as f (f.id)}<option value={f.id}>{f.name}</option>{/each}
          </select>
        </div>
      </div>
      <p class="muted small">Changes are saved automatically.</p>
      <div class="dlg-actions"><button class="primary">Done</button></div>
    </form>
  </dialog>

  <dialog bind:this={historyDlg} class="dlg" aria-labelledby="history-title">
    <div class="spread">
      <h2 id="history-title">History</h2>
      <button type="button" class="small ghost" onclick={() => historyDlg?.close()} aria-label="Close history">✕</button>
    </div>
    {#if versions.length === 0}
      <p class="muted small">Not published yet. Publishing saves a version you can restore later.</p>
    {:else}
      <ul class="versions">
        {#each versions as v (v.version)}
          <li>
            <div class="spread">
              <span class="tag {v.version === guide.current_version ? 'pub' : 'draft'}">v{v.version}</span>
              {#if restoreAsk !== v.version}
                <button type="button" class="small" onclick={() => (restoreAsk = v.version)}>Restore</button>
              {/if}
            </div>
            <div class="muted small">{v.author_name ?? ''}{v.author_name ? ' · ' : ''}{when(v.created_at)}</div>
            {#if v.note}<div class="small note">{v.note}</div>{/if}
            {#if restoreAsk === v.version}
              <div class="callout warning confirm" role="alertdialog" aria-label={`Confirm restore v${v.version}`}>
                <p class="small">Restore version {v.version}? The current steps are replaced (they remain in earlier versions if published).</p>
                <div class="row">
                  <button type="button" class="small primary" onclick={() => restore(v.version)} disabled={restoring}>{restoring ? 'Restoring…' : `Restore v${v.version}`}</button>
                  <button type="button" class="small" onclick={() => (restoreAsk = 0)}>Cancel</button>
                </div>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </dialog>

  <dialog bind:this={publishDlg} class="dlg" aria-labelledby="publish-title">
    <form onsubmit={publish}>
      <h2 id="publish-title">Publish version {nextVersion}</h2>
      <p class="muted small">Readers see this version, and the review clock restarts.</p>
      <label for="pub-note">What changed? <span class="muted">(optional)</span></label>
      <textarea id="pub-note" bind:value={publishNote} rows="3" maxlength="500" placeholder="e.g. New screenshots for the 2026 payroll screen"></textarea>
      <div class="dlg-actions">
        <button type="button" onclick={() => publishDlg?.close()}>Cancel</button>
        <button class="signal" disabled={publishing}>{publishing ? 'Publishing…' : `Publish v${nextVersion}`}</button>
      </div>
    </form>
  </dialog>
{/if}

<style>
  .editor { display: flex; flex-direction: column; gap: 0.75rem; }
  .top {
    position: sticky; top: 56px; z-index: 10;
    display: flex; align-items: center; flex-wrap: wrap; gap: 0.5rem 0.75rem;
    margin: -1.5rem -1.75rem 0; padding: 0.6rem 1.75rem;
    background: var(--paper); border-bottom: 1px solid var(--line);
  }
  .back { flex: none; }
  .gtitle {
    display: inline-flex; align-items: center; gap: 0.4rem; min-width: 0; max-width: min(48ch, 100%); flex: 0 1 auto;
    border-color: transparent; background: transparent; padding: 0.25rem 0.45rem; font-weight: 600; font-size: 1rem; color: var(--text);
  }
  .gtitle:hover:not(:disabled) { background: var(--surface-2); border-color: transparent; }
  .gtitle svg { flex: none; color: var(--muted); }
  .gt { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .visually-hidden { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  .tag { flex: none; }
  .save { display: inline-flex; align-items: center; gap: 0.35rem; color: var(--muted); flex: none; }
  .save.bad { color: var(--danger); }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); flex: none; }
  .dot.s-pending { background: var(--warn); }
  .dot.s-saving { background: var(--sig); }
  .dot.s-error { background: var(--danger); }
  .actions { display: flex; gap: 0.4rem; margin-left: auto; flex: none; }
  .banner { display: flex; justify-content: space-between; align-items: center; gap: 0.75rem; flex-wrap: wrap; margin: 0; }

  .cols { display: grid; grid-template-columns: 250px minmax(0, 1fr) 270px; gap: 1rem; align-items: start; }
  .col-list { position: sticky; top: 124px; max-height: calc(100vh - 140px); display: flex; flex-direction: column; min-height: 0; }
  .col-list > :global(nav) { flex: 1 1 auto; min-height: 0; }
  .paste { margin: 0.5rem 0.25rem 0; flex: none; }
  .props-empty { border: 1px dashed var(--line); border-radius: var(--radius-lg); padding: 1rem; }

  @media (max-width: 1180px) {
    .cols { grid-template-columns: 230px minmax(0, 1fr); }
    .cols > :global(.props), .cols > .props-empty { grid-column: 2; }
    .col-list { grid-row: span 2; }
  }
  @media (max-width: 900px) {
    .top { margin: -1rem -1rem 0; padding: 0.5rem 1rem; }
    .gtitle { max-width: 100%; order: 5; flex: 1 1 100%; justify-content: flex-start; padding-left: 0; }
    .actions { margin-left: auto; }
    .cols { grid-template-columns: minmax(0, 1fr); }
    .cols > :global(.props), .cols > .props-empty { grid-column: auto; }
    .col-list { position: static; max-height: 320px; grid-row: auto; }
  }

  .dlg { border: 1px solid var(--line); border-radius: var(--radius-lg); background: var(--card); color: var(--text); box-shadow: var(--shadow-pop); padding: 1.25rem 1.5rem; width: min(520px, calc(100vw - 2rem)); max-height: calc(100vh - 2rem); overflow-y: auto; }
  .dlg::backdrop { background: rgba(11, 20, 36, 0.45); }
  .dlg h2 { margin-top: 0; }
  .dlg-actions { display: flex; justify-content: flex-end; gap: 0.5rem; margin-top: 1rem; }
  .two { display: flex; gap: 0.75rem; flex-wrap: wrap; }
  .two > div { flex: 1; min-width: 160px; }
  .versions { list-style: none; padding: 0; margin: 0; }
  .versions li { padding: 0.6rem 0; border-bottom: 1px solid var(--line); }
  .versions li:last-child { border-bottom: 0; }
  .note { margin-top: 0.2rem; overflow-wrap: anywhere; }
  .confirm { margin-top: 0.5rem; }
  .confirm p { margin: 0 0 0.5rem; }
</style>
