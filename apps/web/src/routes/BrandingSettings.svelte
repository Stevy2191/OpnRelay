<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api, mediaUrl, type Branding } from '../lib/api';
  import { loadBranding } from '../lib/session.svelte';
  import { errorText } from '../lib/format';

  let b = $state<Branding | null>(null);
  let error = $state('');
  let notice = $state('');
  let fileInput: HTMLInputElement | undefined = $state();

  onMount(async () => {
    try {
      b = await api<Branding>('/branding');
    } catch (e) {
      error = errorText(e);
    }
  });

  async function save(e: Event) {
    e.preventDefault();
    error = notice = '';
    try {
      b = await api<Branding>('/admin/branding', { method: 'PUT', body: b });
      await loadBranding();
      notice = 'Branding saved. It applies to the app and every export.';
    } catch (err) {
      error = errorText(err);
    }
  }

  async function upload(e: Event) {
    const f = (e.currentTarget as HTMLInputElement).files?.[0];
    if (!f) return;
    error = notice = '';
    try {
      b = await api<Branding>('/admin/branding/logo', { method: 'PUT', raw: f, contentType: f.type });
      await loadBranding();
      notice = 'Logo updated';
    } catch (err) {
      error = errorText(err);
    } finally {
      if (fileInput) fileInput.value = '';
    }
  }

  async function removeLogo() {
    await api('/admin/branding/logo', { method: 'DELETE' });
    if (b) b.logo_media_id = null;
    await loadBranding();
  }
</script>

{#if error}<p class="error">{error}</p>{/if}
{#if notice}<p class="success">{notice}</p>{/if}
{#if b}
  <div class="grid">
    <form class="card" onsubmit={save}>
      <h2>Company logo</h2>
      <div class="row">
        {#if b.logo_media_id}
          <img class="logo" src={mediaUrl(b.logo_media_id)} alt="Company logo" />
          <button type="button" class="small" onclick={removeLogo}>Remove</button>
        {:else}
          <span class="muted">No logo yet</span>
        {/if}
        <button type="button" class="small primary" onclick={() => fileInput?.click()}>Upload PNG or JPEG…</button>
        <input bind:this={fileInput} type="file" accept="image/png,image/jpeg" hidden onchange={upload} />
      </div>

      <h2>Colours</h2>
      <div class="row">
        <label class="inline">Primary <input type="color" bind:value={b.primary_color} /></label>
        <label class="inline">Highlight <input type="color" bind:value={b.accent_color} /></label>
      </div>

      <h2>Header & footer</h2>
      <label for="hdr">Header text</label>
      <input id="hdr" bind:value={b.header_text} maxlength="200" />
      <label for="ftr">Footer text</label>
      <input id="ftr" bind:value={b.footer_text} maxlength="200" />
      <p class="muted small">
        Placeholders: <code>{'{company}'}</code> <code>{'{guide_title}'}</code> <code>{'{date}'}</code> <code>{'{author}'}</code>
        <code>{'{version}'}</code> <code>{'{classification}'}</code>, and in PDF/Word <code>{'{page}'}</code> <code>{'{pages}'}</code>.
      </p>

      <h2>Exports</h2>
      <label class="inline"><input type="checkbox" bind:checked={b.cover_page} /> Cover page (PDF)</label>
      <label class="inline"><input type="checkbox" bind:checked={b.show_step_numbers} /> Number the steps</label>
      <label class="inline"><input type="checkbox" bind:checked={b.watermark_exports} /> Stamp “Exported by &lt;name&gt; &lt;date&gt;” on exports</label>
      <p><button class="primary" type="submit">Save branding</button></p>
    </form>

    <div class="card preview" style:--p={b.primary_color} style:--a={b.accent_color}>
      <p class="muted small">Preview</p>
      <div class="pv-head">
        {#if b.logo_media_id}<img src={mediaUrl(b.logo_media_id)} alt="" />{/if}
        <span>{b.header_text.replace('{company}', 'Your company')}</span>
      </div>
      <h3 class="pv-title">Example guide</h3>
      <div class="pv-step"><span class="pv-num">1</span> Click “Save”</div>
      <div class="pv-shot"><span class="pv-hl"></span></div>
    </div>
  </div>
{/if}

<style>
  .grid { display: grid; grid-template-columns: minmax(0, 1fr) 300px; gap: 1rem; align-items: start; }
  @media (max-width: 860px) { .grid { grid-template-columns: 1fr; } }
  h2 { margin: 1rem 0 0.5rem; font-size: 1.05rem; }
  .logo { max-height: 48px; max-width: 200px; border: 1px solid var(--border); border-radius: 6px; padding: 4px; background: #fff; }
  input[type='color'] { width: 48px; height: 32px; padding: 2px; }
  .pv-head { display: flex; align-items: center; gap: 8px; border-bottom: 3px solid var(--p); padding-bottom: 6px; font-size: 0.8rem; color: var(--muted); }
  .pv-head img { max-height: 24px; max-width: 100px; }
  .pv-title { color: var(--p); }
  .pv-step { display: flex; gap: 8px; align-items: center; font-weight: 600; }
  .pv-num { width: 22px; height: 22px; border-radius: 50%; background: var(--p); color: #fff; display: grid; place-items: center; font-size: 0.75rem; }
  .pv-shot { margin-top: 8px; height: 90px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface-2); position: relative; }
  .pv-hl { position: absolute; left: 30%; top: 30%; width: 35%; height: 25%; border: 3px solid var(--a); border-radius: 4px; }
</style>
