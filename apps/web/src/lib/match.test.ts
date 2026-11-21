// SPDX-License-Identifier: AGPL-3.0-only
import { describe, expect, it, vi } from 'vitest';

vi.stubGlobal('window', { location: { hash: '' }, addEventListener: () => {} });
const { match, query } = await import('./router.svelte');

describe('router', () => {
  it('matches patterns and decodes params', () => {
    expect(match('/guides/:id/edit', '/guides/abc%20d/edit')).toEqual({ id: 'abc d' });
    expect(match('/guides/:id', '/guides/1/edit')).toBeNull();
    expect(match('/', '/?q=x')).toEqual({});
  });

  it('parses query strings', () => {
    expect(query('/?ws=1&q=hello%20world').get('q')).toBe('hello world');
    expect(query('/guides').get('x')).toBeNull();
  });
});
