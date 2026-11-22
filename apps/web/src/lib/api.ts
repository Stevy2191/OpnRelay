// SPDX-License-Identifier: AGPL-3.0-only
// Thin typed client for the OpnRelay REST API.

export type OrgRole = 'owner' | 'admin' | 'member' | 'auditor';
export type Perm = 'none' | 'read' | 'edit' | 'manage';
export type StepKind = 'action' | 'text' | 'heading' | 'tip' | 'alert';

export interface Me {
  id: string;
  email: string;
  display_name: string;
  org_role: OrgRole;
  csrf_token: string;
  mfa_enabled: boolean;
  mfa_setup_required: boolean;
  recovery_codes_remaining: number;
  organization: {
    id: string;
    name: string;
    mfa_requirement: 'off' | 'admins' | 'all';
    public_links_enabled: boolean;
    default_capture_mode: string;
    auto_redaction_default: boolean;
  };
}

export interface Workspace {
  id: string;
  name: string;
  description: string;
  legal_hold: boolean;
  guide_count: number;
  review_period_days: number | null;
  created_at: string;
  my_permission: Perm;
}

export interface Folder {
  id: string;
  workspace_id: string;
  parent_id: string | null;
  name: string;
  position: number;
}

export interface Rect { x: number; y: number; w: number; h: number }

export interface Action {
  type: string;
  target_label?: string;
  target_role?: string;
  typed_text?: string;
  keys?: string;
  app?: string;
  window_title?: string;
  url?: string;
  x?: number;
  y?: number;
}

export interface Step {
  id: string;
  kind: StepKind;
  title: string;
  body_md: string;
  action?: Action | null;
  media_id?: string | null;
  annotations: { highlight?: Rect; crop?: Rect; boxes?: Rect[] };
}

export interface GuideSummary {
  id: string;
  workspace_id: string;
  folder_id: string | null;
  title: string;
  description: string;
  status: 'draft' | 'published' | 'archived';
  classification: 'internal' | 'confidential' | 'restricted';
  source: string;
  revision: number;
  current_version: number;
  tags: string[];
  legal_hold: boolean;
  author_name: string | null;
  step_count: number;
  created_at: string;
  updated_at: string;
  published_at: string | null;
  reviewed_at: string | null;
  review_due_at: string | null;
  needs_review: boolean;
  /** Screenshot of the first step that has one, for card thumbnails. */
  thumb_media_id: string | null;
}

export interface Guide extends GuideSummary {
  steps: Step[];
  my_permission: Perm;
}

export interface User {
  id: string;
  email: string;
  display_name: string;
  org_role: OrgRole;
  status: 'invited' | 'active' | 'disabled';
  mfa_enabled: boolean;
  last_login_at: string | null;
  created_at: string;
  role_source: 'manual' | 'idp';
  role_from: string | null;
}

export interface Group {
  id: string;
  name: string;
  source: 'manual' | 'idp';
  external_id: string | null;
  grants_role: 'admin' | 'auditor' | null;
  missing_since: string | null;
  last_synced_at: string | null;
  member_ids: string[];
}

/** A group found in the Microsoft directory (GET /admin/sso/groups/search). */
export interface DirGroup {
  id: string;
  display_name: string;
  description: string | null;
  kind: string;
  on_prem: boolean;
  linked: boolean;
}

/** Result of POST /admin/sso/sync. */
export interface SyncSummary {
  groups: number;
  added: number;
  removed: number;
  roles_changed: number;
  disabled: number;
  missing: number;
  error: string | null;
  warnings: string[];
  brake: number | null;
}

export interface SsoStatus {
  enabled: boolean;
  group_sync_enabled: boolean;
  group_sync_last_at: string | null;
  group_sync_last_ok_at: string | null;
  group_sync_last_error: string | null;
  group_sync_last_warning: string | null;
  group_sync_brake_pending: number | null;
}

export interface Member {
  principal_type: 'user' | 'group';
  principal_id: string;
  role: 'manager' | 'editor' | 'viewer';
  name?: string | null;
}

export interface AuditEvent {
  seq: number;
  ts: string;
  actor_id: string | null;
  actor_label: string;
  actor_ip: string | null;
  action: string;
  target_type: string;
  target_id: string | null;
  metadata: Record<string, unknown>;
  hash: string;
}

export interface Policies {
  mfa_requirement: 'off' | 'admins' | 'all';
  session_idle_minutes: number;
  session_absolute_hours: number;
  public_links_enabled: boolean;
  default_capture_mode: 'full' | 'window' | 'region';
  auto_redaction_default: boolean;
  redaction_patterns: string[];
}

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) {
    super(message);
  }
}

let csrf = '';
export function setCsrf(token: string) {
  csrf = token;
}

type Opts = { method?: string; body?: unknown; raw?: BodyInit; contentType?: string; ifMatch?: number };

export async function api<T>(path: string, opts: Opts = {}): Promise<T> {
  const method = opts.method ?? (opts.body !== undefined || opts.raw !== undefined ? 'POST' : 'GET');
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (method !== 'GET') headers['X-CSRF-Token'] = csrf;
  if (opts.ifMatch !== undefined) headers['If-Match'] = `"${opts.ifMatch}"`;
  let body: BodyInit | undefined;
  if (opts.raw !== undefined) {
    body = opts.raw;
    if (opts.contentType) headers['Content-Type'] = opts.contentType;
  } else if (opts.body !== undefined) {
    body = JSON.stringify(opts.body);
    headers['Content-Type'] = 'application/json';
  }
  const res = await fetch(`/api/v1${path}`, { method, headers, body, credentials: 'same-origin' });
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  let data: unknown = undefined;
  try {
    data = text ? JSON.parse(text) : undefined;
  } catch {
    data = undefined;
  }
  if (!res.ok) {
    const d = (data ?? {}) as { code?: string; detail?: string };
    throw new ApiError(res.status, d.code ?? 'error', d.detail ?? res.statusText);
  }
  return data as T;
}

export const mediaUrl = (id: string) => `/api/v1/media/${id}`;

export interface Branding {
  logo_media_id: string | null;
  primary_color: string;
  accent_color: string;
  header_text: string;
  footer_text: string;
  cover_page: boolean;
  show_step_numbers: boolean;
  watermark_exports: boolean;
}

export const exportUrl = (guideId: string, format: string, images = true) =>
  `/api/v1/guides/${guideId}/export?format=${format}${images ? '' : '&images=false'}`;
