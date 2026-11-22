// SPDX-License-Identifier: AGPL-3.0-only
import { api, ApiError, setCsrf, type Branding, type Me, type Workspace } from './api';

export const session = $state<{ me: Me | null; loading: boolean; branding: Branding | null }>({ me: null, loading: true, branding: null });

export async function refresh(): Promise<Me | null> {
  try {
    const me = await api<Me>('/me');
    setCsrf(me.csrf_token);
    session.me = me;
    if (!me.mfa_setup_required) await loadBranding();
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

export async function loadBranding() {
  try {
    session.branding = await api<Branding>('/branding');
  } catch {
    session.branding = null;
  }
}

/** Workspace list shared by the shell sidebar and Home; fetched once, refreshed after changes. */
export const workspacesStore = $state<{ list: Workspace[]; loaded: boolean; refresh: () => Promise<void> }>({
  list: [],
  loaded: false,
  async refresh() {
    try {
      workspacesStore.list = await api<Workspace[]>('/workspaces');
    } finally {
      workspacesStore.loaded = true;
    }
  },
});

/** Cross-component shell actions (opened from the sidebar or from empty states). */
export const shellUi = $state({ newWorkspace: false, record: false, drawer: false });
