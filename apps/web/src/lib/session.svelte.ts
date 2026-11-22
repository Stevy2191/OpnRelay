// SPDX-License-Identifier: AGPL-3.0-only
import { canLeave, go } from './router.svelte';
import { api, ApiError, setCsrf, type Branding, type GuideSummary, type Me, type Workspace } from './api';

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

/** Set while a sign-out waits on the editor's leave-confirm; "Leave anyway" completes it, "Stay" cancels it. */
export const logoutIntent = $state({ pending: false });

/** Destroys the session and resets client state, then routes to /login (never while still signed in). */
export async function completeLogout() {
  logoutIntent.pending = false;
  try {
    await api('/auth/logout', { method: 'POST' });
  } finally {
    session.me = null;
    workspacesStore.list = [];
    workspacesStore.loaded = false;
    workspacesStore.reviewCount = 0;
    await go('/login');
  }
}

export async function logout() {
  // Ask the router's guard first so an editor with a pending autosave saves
  // (or the user stays) before the session is destroyed. If it refuses, its
  // leave-confirm is showing; remember the intent so "Leave anyway" signs out.
  if (!(await canLeave('/login'))) {
    logoutIntent.pending = true;
    return;
  }
  await completeLogout();
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

let reviewSeq = 0;

/** Workspace list shared by the shell sidebar and Home; fetched once, refreshed after changes. */
export const workspacesStore = $state<{
  list: Workspace[];
  loaded: boolean;
  /** Guides awaiting review in workspaces the user can edit (sidebar badge, All guides summary). */
  reviewCount: number;
  refresh: () => Promise<void>;
  refreshReviewCount: () => Promise<void>;
}>({
  list: [],
  loaded: false,
  reviewCount: 0,
  async refresh() {
    try {
      workspacesStore.list = await api<Workspace[]>('/workspaces');
    } finally {
      workspacesStore.loaded = true;
    }
  },
  /** Call after a guide is marked reviewed or published; the shell also calls it when workspaces or the page change. */
  async refreshReviewCount() {
    const mine = ++reviewSeq;
    const canEdit =
      isAdmin(session.me) || workspacesStore.list.some((w) => w.my_permission === 'edit' || w.my_permission === 'manage');
    if (!canEdit) {
      workspacesStore.reviewCount = 0;
      return;
    }
    try {
      const n = (await api<GuideSummary[]>('/guides?needs_review=true')).length;
      if (mine === reviewSeq) workspacesStore.reviewCount = n; // ignore stale responses
    } catch {
      if (mine === reviewSeq) workspacesStore.reviewCount = 0;
    }
  },
});

/** Cross-component shell actions (opened from the sidebar or from empty states). */
export const shellUi = $state({ newWorkspace: false, record: false, drawer: false });
