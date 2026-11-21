// SPDX-License-Identifier: AGPL-3.0-only
// Step titles. Mirrors opnrelay-core::title so guides read the same whichever
// recorder produced them.
import type { CapturedAction } from './shared';

const MAX = 60;

function clean(s: string | undefined): string | null {
  if (!s) return null;
  const c = s.split(/\s+/).filter(Boolean).join(' ');
  if (!c) return null;
  return c.length > MAX ? `${c.slice(0, MAX - 1)}…` : c;
}

export function displayUrl(url: string): string {
  const noScheme = url.includes('://') ? url.split('://')[1] : url;
  const end = noScheme.search(/[?#]/);
  return (end >= 0 ? noScheme.slice(0, end) : noScheme).replace(/\/+$/, '');
}

export function title(a: CapturedAction): string {
  const l = clean(a.target_label);
  switch (a.type) {
    case 'click':
      return l ? `Click "${l}"` : 'Click here';
    case 'double_click':
      return l ? `Double-click "${l}"` : 'Double-click here';
    case 'right_click':
      return l ? `Right-click "${l}"` : 'Right-click here';
    case 'type': {
      const t = clean(a.typed_text);
      if (t && l) return `Type "${t}" in "${l}"`;
      if (l) return `Type in "${l}"`;
      return t ? `Type "${t}"` : 'Type text';
    }
    case 'select': {
      const v = clean(a.value_label);
      if (v && l) return `Select "${v}" from "${l}"`;
      if (v) return `Select "${v}"`;
      return l ? `Choose an option in "${l}"` : 'Select an option';
    }
    case 'key_shortcut':
      return a.keys ? `Press ${a.keys}` : 'Press a key';
  }
}
