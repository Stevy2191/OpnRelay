// SPDX-License-Identifier: AGPL-3.0-only
import { beforeEach, describe, expect, it, vi } from 'vitest';

const calls: string[] = [];
let failPost = false;
let allow = true;
vi.mock('./router.svelte', () => ({
  canLeave: async () => allow,
  goUnguarded: (p: string) => void calls.push(`go ${p}`),
}));
vi.mock('./api', () => ({
  api: async (p: string) => {
    calls.push(`api ${p}`);
    if (failPost) throw new Error('boom');
  },
  ApiError: class extends Error {},
  setCsrf: () => {},
}));

import { completeLogout, logout, logoutIntent, session, workspacesStore } from './session.svelte';

beforeEach(() => {
  calls.length = 0;
  allow = true;
  failPost = false;
  logoutIntent.error = '';
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
  it('stays signed in, does not route, and reports an error when the POST fails', async () => {
    failPost = true;
    await logout();
    expect(calls).toEqual(['api /auth/logout']);
    expect(session.me).not.toBeNull();
    expect(workspacesStore.reviewCount).toBe(3);
    expect(logoutIntent.error).toMatch(/sign out/);
    failPost = false;
    expect(await completeLogout()).toBe(true);
    expect(logoutIntent.error).toBe('');
  });
});
