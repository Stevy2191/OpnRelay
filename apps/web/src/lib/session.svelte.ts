// SPDX-License-Identifier: AGPL-3.0-only
import { api, ApiError, setCsrf, type Me } from './api';

export const session = $state<{ me: Me | null; loading: boolean }>({ me: null, loading: true });

export async function refresh(): Promise<Me | null> {
  try {
    const me = await api<Me>('/me');
    setCsrf(me.csrf_token);
    session.me = me;
  } catch (e) {
    if (!(e instanceof ApiError) || e.status !== 401) console.error(e);
    session.me = null;
  } finally {
    session.loading = false;
  }
  return session.me;
}

export async function logout() {
  try {
    await api('/auth/logout', { method: 'POST' });
  } finally {
    session.me = null;
    window.location.hash = '/login';
  }
}

export const isAdmin = (me: Me | null) => me?.org_role === 'owner' || me?.org_role === 'admin';
export const canAudit = (me: Me | null) => isAdmin(me) || me?.org_role === 'auditor';
