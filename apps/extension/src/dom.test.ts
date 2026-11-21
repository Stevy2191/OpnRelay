// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest';
import { compilePatterns, interactiveTarget, isSecretField, label, luhn, role, sensitiveElements, sensitiveRanges } from './dom';
import { normalizeServer, uuidv7 } from './shared';
import { displayUrl, title } from './titles';

function html(s: string) {
  document.body.innerHTML = s;
}

describe('labels', () => {
  it('follows accessible-name order', () => {
    html(`
      <button id="a" aria-label="Close dialog"><svg></svg></button>
      <span id="lbl">Employee ID</span><input id="b" aria-labelledby="lbl">
      <label for="c">Last name</label><input id="c">
      <button id="d"><span>Save</span> <b>changes</b></button>
      <input id="e" placeholder="Search…">
      <input id="f" type="submit" value="Send">
      <a id="g" href="#"><img alt="Home"></a>`);
    const $ = (id: string) => document.getElementById(id)!;
    expect(label($('a'))).toBe('Close dialog');
    expect(label($('b'))).toBe('Employee ID');
    expect(label($('c'))).toBe('Last name');
    expect(label($('d'))).toBe('Save changes');
    expect(label($('e'))).toBe('Search…');
    expect(label($('f'))).toBe('Send');
    expect(label($('g'))).toBe('Home');
    expect(role($('f'))).toBe('button');
    expect(role($('g'))).toBe('link');
  });

  it('finds the interactive ancestor of an icon', () => {
    html(`<button id="btn"><i id="icon">x</i></button>`);
    expect(interactiveTarget(document.getElementById('icon')!).id).toBe('btn');
  });
});

describe('redaction', () => {
  it('detects secret fields and sensitive elements', () => {
    html(`<input id="p" type="password"><input id="cc" autocomplete="cc-number"><div data-opnrelay-redact>x</div><input id="n">`);
    expect(isSecretField(document.getElementById('p')!)).toBe(true);
    expect(isSecretField(document.getElementById('n')!)).toBe(false);
    expect(sensitiveElements(document).length).toBe(3);
    expect(sensitiveElements(document, ['#n', '<<bad selector']).length).toBe(4);
  });

  it('matches common personal data patterns', () => {
    const p = compilePatterns(['EMP-\\d{6}']);
    const s = 'Mail jane.doe@acme.org, SSN 123-45-6789, card 4111 1111 1111 1111, id EMP-123456, call (555) 123-4567.';
    const found = sensitiveRanges(s, p).map(([a, b]) => s.slice(a, b));
    expect(found).toContain('jane.doe@acme.org');
    expect(found).toContain('123-45-6789');
    expect(found.some((f) => f.includes('4111 1111 1111 1111'))).toBe(true);
    expect(found).toContain('EMP-123456');
    expect(found.some((f) => f.includes('123-4567'))).toBe(true);
    expect(sensitiveRanges('Order 1234 5678 9012 3456 shipped', compilePatterns([]))).toEqual([]);
    expect(luhn('4111111111111111')).toBe(true);
    expect(luhn('4111111111111112')).toBe(false);
  });
});

describe('titles and helpers', () => {
  it('generates the same titles as the server', () => {
    expect(title({ type: 'click', target_label: '  Save \n changes ' })).toBe('Click "Save changes"');
    expect(title({ type: 'type', target_label: 'Email' })).toBe('Type in "Email"');
    expect(title({ type: 'select', target_label: 'Dept', value_label: 'EMS' })).toBe('Select "EMS" from "Dept"');
    expect(title({ type: 'key_shortcut', keys: 'Ctrl+S' })).toBe('Press Ctrl+S');
    expect(displayUrl('https://hr.example.com/onboarding/?token=x#y')).toBe('hr.example.com/onboarding');
  });

  it('normalises server addresses and only allows http on localhost', () => {
    expect(normalizeServer('guides.acme.org/foo')).toBe('https://guides.acme.org');
    expect(normalizeServer('http://localhost:8080')).toBe('http://localhost:8080');
    expect(normalizeServer('http://guides.acme.org')).toBeNull();
    expect(normalizeServer('')).toBeNull();
  });

  it('makes v7-shaped ids', () => {
    expect(uuidv7()).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  });
});
