// SPDX-License-Identifier: AGPL-3.0-only
import { beforeEach, describe, expect, it, vi } from 'vitest';

const calls: string[] = [];
let allow = true;
vi.mock('./router.svelte', () => ({
  canLeave: async () => allow,
  go: async (p: string) => (calls.push(`go ${p}`), true),
}));
vi.mock('./api', () => ({
  api: async (p: string) => void calls.push(`api ${p}`),
  ApiError: class extends Error {},
  setCsrf: () => {},
}));

import { completeLogout, logout, logoutIntent, session, workspacesStore } from './session.svelte';

beforeEach(() => {
  calls.length = 0;
  allow = true;
  logoutIntent.pending = false;
  session.me = { id: 'u' } as never;
  workspacesStore.loaded = true;
  workspacesStore.reviewCount = 3;
});

describe('logout', () => {
  it('posts, resets, then routes', async () => {
    await logout();
    expect(calls).toEqual(['api /auth/logout', 'go /login']);
    expect(session.me).toBeNull();
    expect(workspacesStore.reviewCount).toBe(0);
  });
  it('stays signed in and records intent when the guard refuses', async () => {
    allow = false;
    await logout();
    expect(calls).toEqual([]);
    expect(session.me).not.toBeNull();
    expect(logoutIntent.pending).toBe(true);
    await completeLogout();
    expect(calls).toEqual(['api /auth/logout', 'go /login']);
    expect(session.me).toBeNull();
    expect(logoutIntent.pending).toBe(false);
  });
});
