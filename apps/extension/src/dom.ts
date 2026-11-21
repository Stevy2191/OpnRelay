// SPDX-License-Identifier: AGPL-3.0-only
// DOM helpers used by the content script: human-readable labels for elements
// and detection of sensitive content to redact. Kept free of chrome.* so they
// can be unit tested.

const INTERACTIVE = 'a,button,input,select,textarea,summary,label,[role],[onclick],[tabindex]';

function text(el: Element | null | undefined): string {
  return (el?.textContent ?? '').replace(/\s+/g, ' ').trim();
}

/** The element a user meant to interact with (e.g. the button, not its icon). */
export function interactiveTarget(el: Element): Element {
  return el.closest(INTERACTIVE) ?? el;
}

export function role(el: Element): string {
  const explicit = el.getAttribute('role');
  if (explicit) return explicit;
  const tag = el.tagName.toLowerCase();
  if (tag === 'a') return 'link';
  if (tag === 'button' || tag === 'summary') return 'button';
  if (tag === 'select') return 'combobox';
  if (tag === 'textarea') return 'textbox';
  if (tag === 'input') {
    const t = (el as HTMLInputElement).type;
    if (['button', 'submit', 'reset'].includes(t)) return 'button';
    if (t === 'checkbox' || t === 'radio') return t;
    return 'textbox';
  }
  return tag;
}

/** Accessible-name-like label, in the order a screen reader would use. */
export function label(el: Element): string {
  const doc = el.ownerDocument;
  const aria = el.getAttribute('aria-label');
  if (aria?.trim()) return aria.trim();
  const labelledby = el.getAttribute('aria-labelledby');
  if (labelledby) {
    const t = labelledby
      .split(/\s+/)
      .map((id) => text(doc.getElementById(id)))
      .filter(Boolean)
      .join(' ');
    if (t) return t;
  }
  if (el instanceof doc.defaultView!.HTMLElement) {
    const labels = (el as HTMLInputElement).labels;
    if (labels && labels.length) {
      const t = text(labels[0]);
      if (t) return t;
    }
  }
  const tag = el.tagName.toLowerCase();
  if (tag === 'input') {
    const input = el as HTMLInputElement;
    if (['button', 'submit', 'reset'].includes(input.type) && input.value) return input.value;
  }
  if (tag !== 'input' && tag !== 'textarea' && tag !== 'select') {
    const t = text(el);
    if (t && t.length <= 80) return t;
  }
  for (const attr of ['title', 'placeholder', 'alt', 'name']) {
    const v = el.getAttribute(attr);
    if (v?.trim()) return v.trim();
  }
  const img = el.querySelector('img[alt]');
  if (img?.getAttribute('alt')) return img.getAttribute('alt')!;
  return '';
}

/** Fields whose contents must never be captured or shown. */
export function isSecretField(el: Element): boolean {
  if (el.tagName.toLowerCase() !== 'input') return false;
  const input = el as HTMLInputElement;
  const ac = (input.autocomplete || '').toLowerCase();
  return input.type === 'password' || ac.includes('password') || ac.startsWith('cc-') || ac === 'one-time-code';
}

/** Elements that should be masked when automatic redaction is on. */
export function sensitiveElements(root: Document, extraSelectors: string[] = []): Element[] {
  const selectors = [
    'input[type=password]',
    'input[autocomplete^="cc-"]',
    'input[autocomplete="one-time-code"]',
    '[data-opnrelay-redact]',
    ...extraSelectors,
  ];
  const out = new Set<Element>();
  for (const s of selectors) {
    try {
      root.querySelectorAll(s).forEach((e) => out.add(e));
    } catch {
      // Ignore invalid admin-provided selectors.
    }
  }
  return [...out];
}

export const DEFAULT_PATTERNS: RegExp[] = [
  /[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}/gi, // email
  /\b\d{3}-\d{2}-\d{4}\b/g, // US SSN
  /\b(?:\d[ -]?){13,19}\b/g, // card-like numbers (Luhn-checked below)
  /(?:\+\d{1,3}[ .-]?)?\(?\d{3}\)?[ .-]?\d{3}[ .-]?\d{4}\b/g, // phone
  /\b(?:0[1-9]|1[0-2])\/(?:0[1-9]|[12]\d|3[01])\/(?:19|20)\d{2}\b/g, // DOB-like date
];

export function luhn(digits: string): boolean {
  const d = digits.replace(/\D/g, '');
  if (d.length < 13 || d.length > 19) return false;
  let sum = 0;
  for (let i = 0; i < d.length; i++) {
    let n = Number(d[d.length - 1 - i]);
    if (i % 2 === 1) {
      n *= 2;
      if (n > 9) n -= 9;
    }
    sum += n;
  }
  return sum % 10 === 0;
}

export function compilePatterns(extra: string[]): RegExp[] {
  const out = [...DEFAULT_PATTERNS];
  for (const p of extra) {
    try {
      out.push(new RegExp(p, 'g'));
    } catch {
      // ignore invalid patterns
    }
  }
  return out;
}

/** Character ranges in `s` that match a sensitive pattern. */
export function sensitiveRanges(s: string, patterns: RegExp[]): [number, number][] {
  const out: [number, number][] = [];
  for (const re of patterns) {
    re.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(s)) !== null) {
      if (m[0].length === 0) {
        re.lastIndex++;
        continue;
      }
      const isCardPattern = re.source.startsWith('\\b(?:\\d[ -]?)');
      if (isCardPattern && !luhn(m[0])) continue;
      out.push([m.index, m.index + m[0].length]);
    }
  }
  return out;
}
