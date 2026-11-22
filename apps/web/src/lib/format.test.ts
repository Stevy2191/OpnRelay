// SPDX-License-Identifier: AGPL-3.0-only
import { describe, expect, it } from 'vitest';
import { initials, miniMarkdown, monthsOverdue } from './format';

describe('miniMarkdown', () => {
  it('escapes HTML so step text cannot inject markup', () => {
    const out = miniMarkdown('<img src=x onerror=alert(1)> "q" \'s\'');
    expect(out).not.toContain('<img');
    expect(out).toContain('&lt;img');
    expect(out).toContain('&quot;q&quot;');
    expect(out).toContain('&#39;s&#39;');
  });

  it('renders the supported subset', () => {
    expect(miniMarkdown('**bold** and *it* and `c`')).toBe('<p><strong>bold</strong> and <em>it</em> and <code>c</code></p>');
    expect(miniMarkdown('- a\n- b')).toBe('<ul><li>a</li><li>b</li></ul>');
    expect(miniMarkdown('one\ntwo\n\nthree')).toBe('<p>one<br>two</p><p>three</p>');
  });

  it('does not create links or attributes', () => {
    const out = miniMarkdown('[x](javascript:alert(1)) **<b onclick=x>**');
    // Only bare tags we generate; nothing with attributes and no anchors.
    expect(out).not.toMatch(/<[a-z]+\s/i);
    expect(out).not.toContain('<a');
  });
});

describe('initials', () => {
  it('uses first and last word', () => {
    expect(initials('Ada Byron Lovelace')).toBe('AL');
    expect(initials('ada')).toBe('A');
    expect(initials(null)).toBe('?');
  });
});

describe('monthsOverdue', () => {
  const now = Date.parse('2026-11-22T00:00:00Z');
  it('counts whole 30-day months', () => {
    expect(monthsOverdue('2026-11-10T00:00:00Z', now)).toBe(0);
    expect(monthsOverdue('2026-08-20T00:00:00Z', now)).toBe(3);
    expect(monthsOverdue(null, now)).toBe(0);
  });
});
