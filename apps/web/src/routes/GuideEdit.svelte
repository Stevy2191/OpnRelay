<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api, ApiError, type Folder, type Guide, type Step, type StepKind } from '../lib/api';
  import { go } from '../lib/router.svelte';
  import { errorText, when } from '../lib/format';
  import Screenshot from '../lib/Screenshot.svelte';

  let { id }: { id: string } = $props();

  let guide = $state<Guide | null>(null);
  let steps = $state<Step[]>([]);
  let folders = $state<Folder[]>([]);
  let versions = $state<{ version: number; note: string; author_name: string | null; created_at: string }[]>([]);
  let meta = $state({ title: '', description: '', classification: 'internal', folder_id: '' as string });
  let dirty = $state(false);
  let saving = $state(false);
  let message = $state('');
  let error = $state('');
  let uploadInput: HTMLInputElement | undefined = $state();

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
      meta = { title: g.title, description: g.description, classification: g.classification, folder_id: g.folder_id ?? '' };
      dirty = false;
      [folders, versions] = await Promise.all([
        api<Folder[]>(`/workspaces/${g.workspace_id}/folders`),
        api<typeof versions>(`/guides/${id}/versions`),
      ]);
    } catch (e) {
      error = errorText(e);
    }
  }

  onMount(() => {
    load();
    const onPaste = (e: ClipboardEvent) => {
      const target = e.target as HTMLElement;
      if (target && ['INPUT', 'TEXTAREA'].includes(target.tagName)) return;
      const files = Array.from(e.clipboardData?.files ?? []).filter((f) => f.type.startsWith('image/'));
      if (files.length) {
        e.preventDefault();
        addImages(files);
      }
    };
    const beforeUnload = (e: BeforeUnloadEvent) => {
      if (dirty) e.preventDefault();
    };
    window.addEventListener('paste', onPaste);
    window.addEventListener('beforeunload', beforeUnload);
    return () => {
      window.removeEventListener('paste', onPaste);
      window.removeEventListener('beforeunload', beforeUnload);
    };
  });

  function touch() {
    dirty = true;
    message = '';
  }

  function add(kind: StepKind, at = steps.length) {
    const titles: Record<StepKind, string> = { action: 'New step', text: '', heading: 'New section', tip: '', alert: '' };
    steps.splice(at, 0, { id: newId(), kind, title: titles[kind], body_md: '', annotations: {} });
    touch();
  }

  function move(i: number, d: number) {
    const j = i + d;
    if (j < 0 || j >= steps.length) return;
    [steps[i], steps[j]] = [steps[j], steps[i]];
    touch();
  }

  function remove(i: number) {
    steps.splice(i, 1);
    touch();
  }

  async function addImages(files: File[]) {
    for (const f of files) {
      try {
        const m = await api<{ id: string; width: number; height: number }>(`/media?guide_id=${id}`, {
          raw: f,
          contentType: f.type || 'application/octet-stream',
        });
        steps.push({ id: newId(), kind: 'action', title: 'New step', body_md: '', media_id: m.id, annotations: {} });
        touch();
      } catch (e) {
        error = `Upload failed: ${errorText(e)}`;
      }
    }
  }

  function setHighlight(i: number, x: number, y: number, w: number, h: number) {
    const bw = Math.max(40, Math.round(w * 0.12));
    const bh = Math.max(24, Math.round(h * 0.06));
    steps[i].annotations = {
      ...steps[i].annotations,
      highlight: { x: Math.max(0, Math.round(x - bw / 2)), y: Math.max(0, Math.round(y - bh / 2)), w: bw, h: bh },
    };
    touch();
  }

  async function save(): Promise<boolean> {
    if (!guide) return false;
    saving = true;
    error = '';
    try {
      let g = await api<Guide>(`/guides/${id}`, {
        method: 'PATCH',
        ifMatch: guide.revision,
        body: {
          title: meta.title,
          description: meta.description,
          classification: meta.classification,
          folder_id: meta.folder_id || null,
        },
      });
      // Keep the latest revision even if the next request fails.
      guide = g;
      g = await api<Guide>(`/guides/${id}/steps`, { method: 'PUT', ifMatch: g.revision, body: { steps } });
      guide = g;
      dirty = false;
      message = 'Saved';
      return true;
    } catch (e) {
      error = e instanceof ApiError && e.status === 412 ? 'Someone else changed this guide. Reload to see their changes (your edits will be lost).' : errorText(e);
      return false;
    } finally {
      saving = false;
    }
  }

  async function publish() {
    if (dirty && !(await save())) return;
    const note = prompt('Optional note for this version (what changed?)', '') ?? '';
    try {
      guide = await api<Guide>(`/guides/${id}/publish`, { body: { note }, ifMatch: guide!.revision });
      versions = await api(`/guides/${id}/versions`);
      message = `Published version ${guide.current_version}`;
    } catch (e) {
      error = errorText(e);
    }
  }

  async function restore(v: number) {
    if (dirty && !confirm('Discard unsaved changes?')) return;
    if (!confirm(`Restore version ${v}? The current steps are replaced (they remain in earlier versions if published).`)) return;
    try {
      await api(`/guides/${id}/versions/${v}/restore`, { body: {} });
      await load();
      message = `Restored version ${v}`;
    } catch (e) {
      error = errorText(e);
    }
  }

  const kinds: { kind: StepKind; label: string }[] = [
    { kind: 'action', label: 'Step' },
    { kind: 'heading', label: 'Heading' },
    { kind: 'text', label: 'Text' },
    { kind: 'tip', label: 'Tip' },
    { kind: 'alert', label: 'Alert' },
  ];
</script>

{#if !guide}
  {#if error}<p class="error">{error}</p>{:else}<p class="muted">Loading…</p>{/if}
{:else}
  <div class="editor">
    <div class="toolbar card">
      <a href={`#/guides/${id}`} class="small" onclick={(e) => { if (dirty && !confirm('Discard unsaved changes?')) e.preventDefault(); }}>← Done</a>
      <span class="muted small">{dirty ? 'Unsaved changes' : message || `Saved ${when(guide.updated_at)}`}</span>
      <div class="row">
        <button onclick={save} disabled={saving || !dirty}>{saving ? 'Saving…' : 'Save'}</button>
        <button class="primary" onclick={publish} disabled={saving}>Publish</button>
      </div>
    </div>
    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <div class="cols">
      <div class="main stack">
        <div class="card">
          <label for="title">Title</label>
          <input id="title" bind:value={meta.title} oninput={touch} maxlength="200" />
          <label for="desc">Description</label>
          <textarea id="desc" bind:value={meta.description} oninput={touch} rows="2"></textarea>
          <div class="row two">
            <div>
              <label for="cls">Classification</label>
              <select id="cls" bind:value={meta.classification} onchange={touch}>
                <option value="internal">Internal</option>
                <option value="confidential">Confidential</option>
                <option value="restricted">Restricted</option>
              </select>
            </div>
            <div>
              <label for="folder">Folder</label>
              <select id="folder" bind:value={meta.folder_id} onchange={touch}>
                <option value="">(workspace root)</option>
                {#each folders as f (f.id)}<option value={f.id}>{f.name}</option>{/each}
              </select>
            </div>
          </div>
        </div>

        {#each steps as s, i (s.id)}
          <div class="card step {s.kind}">
            <div class="spread">
              <select bind:value={s.kind} onchange={touch} aria-label="Step type" class="kind">
                {#each kinds as k}<option value={k.kind}>{k.label}</option>{/each}
              </select>
              <div class="row">
                <button class="small ghost" onclick={() => move(i, -1)} disabled={i === 0} aria-label="Move up">↑</button>
                <button class="small ghost" onclick={() => move(i, 1)} disabled={i === steps.length - 1} aria-label="Move down">↓</button>
                <button class="small ghost" onclick={() => add('action', i + 1)} title="Insert step below">＋</button>
                <button class="small ghost danger" onclick={() => remove(i)} aria-label="Delete step">✕</button>
              </div>
            </div>
            <input class="title" placeholder={s.kind === 'heading' ? 'Section title' : 'Step title'} bind:value={s.title} oninput={touch} maxlength="300" />
            {#if s.kind !== 'heading'}
              <textarea placeholder="Details (optional). Supports **bold**, *italic*, `code` and - lists." bind:value={s.body_md} oninput={touch} rows="2"></textarea>
            {/if}
            {#if s.media_id}
              <p class="muted small">Click the screenshot to move the highlight.</p>
              <Screenshot mediaId={s.media_id} highlight={s.annotations?.highlight} alt={s.title} onpick={(x, y, w, h) => setHighlight(i, x, y, w, h)} />
              <div class="row">
                <button class="small" onclick={() => { s.media_id = null; touch(); }}>Remove screenshot</button>
                {#if s.annotations?.highlight}<button class="small" onclick={() => { s.annotations = { ...s.annotations, highlight: undefined }; touch(); }}>Remove highlight</button>{/if}
              </div>
            {/if}
          </div>
        {/each}

        <div class="card add">
          <div class="row">
            <strong>Add:</strong>
            {#each kinds as k}<button class="small" onclick={() => add(k.kind)}>{k.label}</button>{/each}
            <button class="small primary" onclick={() => uploadInput?.click()}>Screenshots…</button>
            <input bind:this={uploadInput} type="file" accept="image/png,image/jpeg,image/webp,image/gif" multiple hidden onchange={(e) => { const f = Array.from((e.currentTarget as HTMLInputElement).files ?? []); addImages(f); (e.currentTarget as HTMLInputElement).value = ''; }} />
          </div>
          <p class="muted small">Tip: paste a screenshot (Ctrl/⌘+V) anywhere on this page to add it as a new step.</p>
        </div>
      </div>

      <aside class="card side">
        <h3>Versions</h3>
        {#if versions.length === 0}
          <p class="muted small">Not published yet. Publishing saves a version you can restore later.</p>
        {:else}
          <ul>
            {#each versions as v (v.version)}
              <li>
                <div class="spread"><strong>v{v.version}</strong><button class="small" onclick={() => restore(v.version)}>Restore</button></div>
                <div class="muted small">{v.author_name ?? ''} · {when(v.created_at)}</div>
                {#if v.note}<div class="small">{v.note}</div>{/if}
              </li>
            {/each}
          </ul>
        {/if}
      </aside>
    </div>
  </div>
{/if}

<style>
  .editor { max-width: 1150px; margin: 0 auto; }
  .toolbar { display: flex; justify-content: space-between; align-items: center; gap: 1rem; position: sticky; top: 3.6rem; z-index: 5; padding: 0.6rem 1rem; margin-bottom: 1rem; }
  .cols { display: grid; grid-template-columns: 1fr 260px; gap: 1.25rem; align-items: start; }
  @media (max-width: 900px) { .cols { grid-template-columns: 1fr; } }
  .two > div { flex: 1; min-width: 160px; }
  .step .title { font-weight: 600; margin: 0.5rem 0; }
  .step.tip { border-left: 4px solid var(--ok); }
  .step.alert { border-left: 4px solid var(--danger); }
  .step.heading { border-left: 4px solid var(--primary); }
  .kind { width: auto; }
  .side ul { list-style: none; padding: 0; margin: 0; }
  .side li { padding: 0.5rem 0; border-bottom: 1px solid var(--border); }
  .add { border-style: dashed; }
</style>
