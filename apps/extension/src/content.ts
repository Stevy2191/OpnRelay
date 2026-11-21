// SPDX-License-Identifier: AGPL-3.0-only
// Content script: injected only into the tab being recorded. Observes user
// actions, masks sensitive content before each screenshot, and reports steps
// to the background worker.
import { compilePatterns, interactiveTarget, isSecretField, label, role, sensitiveElements, sensitiveRanges } from './dom';
import type { CapturedAction, RecordOptions, StepMessage } from './shared';

type Win = Window & { __opnrelayRecorder?: boolean };

const OVERLAY_ATTR = 'data-opnrelay-overlay';
const MAX_TEXT_NODES = 3000;

function rectOf(el: Element) {
  const r = el.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height };
}

function inViewport(r: DOMRect): boolean {
  return r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth && r.width > 0 && r.height > 0;
}

function addMask(container: HTMLElement, r: DOMRect | { left: number; top: number; width: number; height: number }) {
  const d = document.createElement('div');
  d.setAttribute(OVERLAY_ATTR, '');
  d.style.cssText = `position:fixed;left:${r.left - 2}px;top:${r.top - 2}px;width:${r.width + 4}px;height:${r.height + 4}px;background:#1b1f24;border-radius:3px;z-index:2147483647;pointer-events:none;`;
  container.appendChild(d);
}

/** Draw masks over sensitive content currently on screen. Returns a cleanup. */
function maskSensitive(opts: RecordOptions): () => void {
  const container = document.createElement('div');
  container.setAttribute(OVERLAY_ATTR, '');
  container.style.cssText = 'position:fixed;inset:0;pointer-events:none;z-index:2147483647;';
  // Password and card fields are always masked, whatever the settings.
  for (const el of sensitiveElements(document)) {
    const r = el.getBoundingClientRect();
    if (inViewport(r)) addMask(container, r);
  }
  if (opts.autoRedact) {
    const patterns = compilePatterns(opts.patterns);
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    let n = 0;
    for (let node = walker.nextNode(); node && n < MAX_TEXT_NODES; node = walker.nextNode(), n++) {
      const t = node.nodeValue ?? '';
      if (t.trim().length < 5) continue;
      const parent = node.parentElement;
      if (!parent || parent.closest(`[${OVERLAY_ATTR}],script,style,noscript`)) continue;
      const ranges = sensitiveRanges(t, patterns);
      for (const [start, end] of ranges) {
        const range = document.createRange();
        range.setStart(node, start);
        range.setEnd(node, end);
        for (const r of Array.from(range.getClientRects())) if (inViewport(r)) addMask(container, r);
      }
    }
    document.querySelectorAll('input:not([type=password]),textarea').forEach((el) => {
      const v = (el as HTMLInputElement).value;
      if (v && sensitiveRanges(v, patterns).length) {
        const r = el.getBoundingClientRect();
        if (inViewport(r)) addMask(container, r);
      }
    });
  }
  document.documentElement.appendChild(container);
  return () => container.remove();
}

const nextFrame = () => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));

(async () => {
  const w = window as Win;
  if (w.__opnrelayRecorder) return;
  w.__opnrelayRecorder = true;

  let opts: RecordOptions | null = null;
  async function options(): Promise<RecordOptions | null> {
    const res = await chrome.runtime.sendMessage({ kind: 'recordingOptions' });
    opts = res?.ok ? res.data : null;
    return opts;
  }
  await options();

  let busy = Promise.resolve();
  function report(action: CapturedAction, el: Element | null, point: { x: number; y: number } | null) {
    busy = busy.then(async () => {
      const o = await options();
      if (!o) return;
      const cleanup = maskSensitive(o);
      await nextFrame();
      const msg: StepMessage = {
        kind: 'step',
        action,
        target: el ? rectOf(el) : null,
        point,
        viewport: { w: innerWidth, h: innerHeight, dpr: devicePixelRatio },
      };
      try {
        await chrome.runtime.sendMessage(msg);
      } finally {
        cleanup();
      }
    });
  }

  window.addEventListener(
    'pointerdown',
    (e) => {
      if (!e.isTrusted || !(e.target instanceof Element) || e.target.closest(`[${OVERLAY_ATTR}]`)) return;
      if (e.button !== 0 && e.button !== 2) return;
      const el = interactiveTarget(e.target);
      const tag = el.tagName.toLowerCase();
      // Typing into a field is reported on change; selecting is reported as such.
      if (tag === 'select') return;
      report(
        { type: e.button === 2 ? 'right_click' : 'click', target_label: label(el), target_role: role(el) },
        el,
        { x: e.clientX, y: e.clientY },
      );
    },
    true,
  );

  window.addEventListener(
    'change',
    (e) => {
      if (!e.isTrusted || !(e.target instanceof Element)) return;
      const el = e.target;
      const tag = el.tagName.toLowerCase();
      const r = el.getBoundingClientRect();
      const point = { x: r.left + r.width / 2, y: r.top + r.height / 2 };
      if (tag === 'select') {
        const sel = el as HTMLSelectElement;
        report(
          { type: 'select', target_label: label(el), target_role: 'combobox', value_label: sel.selectedOptions[0]?.textContent?.trim() },
          el,
          point,
        );
      } else if (tag === 'textarea' || (tag === 'input' && !['checkbox', 'radio', 'button', 'submit', 'file', 'range', 'color'].includes((el as HTMLInputElement).type))) {
        const value = (el as HTMLInputElement).value;
        const showText = opts?.includeTypedText && !isSecretField(el) && value.length <= 200;
        report({ type: 'type', target_label: label(el), target_role: 'textbox', typed_text: showText ? value : undefined }, el, point);
      }
    },
    true,
  );

  window.addEventListener(
    'keydown',
    (e) => {
      if (!e.isTrusted || !(e.ctrlKey || e.metaKey || e.altKey) || ['Control', 'Meta', 'Alt', 'Shift'].includes(e.key)) return;
      const keys = [e.ctrlKey && 'Ctrl', e.metaKey && '⌘', e.altKey && 'Alt', e.shiftKey && 'Shift', e.key.length === 1 ? e.key.toUpperCase() : e.key]
        .filter(Boolean)
        .join('+');
      report({ type: 'key_shortcut', keys }, null, null);
    },
    true,
  );
})();
