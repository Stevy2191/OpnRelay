<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- Shows a step screenshot with its highlight annotation overlaid. -->
<script lang="ts">
  import { mediaUrl, type Rect } from './api';

  let {
    mediaId,
    highlight,
    alt = '',
    onpick,
    label = '',
  }: { mediaId: string; highlight?: Rect; alt?: string; label?: string; onpick?: (x: number, y: number, w: number, h: number) => void } = $props();

  let natural = $state({ w: 0, h: 0 });

  function loaded(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    natural = { w: img.naturalWidth, h: img.naturalHeight };
  }

  function pick(e: MouseEvent) {
    if (!onpick || !natural.w) return;
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = ((e.clientX - rect.left) / rect.width) * natural.w;
    const y = ((e.clientY - rect.top) / rect.height) * natural.h;
    onpick(Math.round(x), Math.round(y), natural.w, natural.h);
  }

  // Reader-only action label; sits above the box, or below it when the box is near the top edge.
  let labelBelow = $derived(!!highlight && natural.h > 0 && highlight.y / natural.h < 0.09);

  const pct = (v: number, total: number) => `${(v / total) * 100}%`;
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="shot" class:pickable={!!onpick} onclick={pick}>
  <img src={mediaUrl(mediaId)} {alt} onload={loaded} loading="lazy" />
  {#if highlight && natural.w}
    <div
      class="hl"
      style:left={pct(highlight.x, natural.w)}
      style:top={pct(highlight.y, natural.h)}
      style:width={pct(highlight.w, natural.w)}
      style:height={pct(highlight.h, natural.h)}
    ></div>
    {#if label}
      <span
        class="hl-label"
        class:below={labelBelow}
        style:left={pct(highlight.x, natural.w)}
        style:top={pct(labelBelow ? highlight.y + highlight.h : highlight.y, natural.h)}
      >{label}</span>
    {/if}
  {/if}
</div>

<style>
  .shot { position: relative; display: inline-block; max-width: 100%; border: 1px solid var(--border); border-radius: 8px; overflow: hidden; line-height: 0; }
  .shot img { max-width: 100%; height: auto; display: block; }
  .pickable { cursor: crosshair; }
  .hl { position: absolute; border: 3px solid var(--accent); border-radius: 6px; box-shadow: 0 0 0 9999px rgba(0, 0, 0, 0.18); pointer-events: none; }
  .hl-label {
    position: absolute;
    transform: translateY(-100%);
    max-width: 90%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0.12rem 0.45rem;
    font-family: var(--font-mono);
    font-size: 0.7rem;
    font-weight: 500;
    letter-spacing: 0.04em;
    line-height: 1.4;
    background: var(--sig);
    color: var(--btn-signal-text);
    border-radius: 4px 4px 0 0;
    pointer-events: none;
  }
  .hl-label.below { transform: none; border-radius: 0 0 4px 4px; }
</style>
