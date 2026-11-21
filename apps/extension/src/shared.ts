// SPDX-License-Identifier: AGPL-3.0-only
// Types shared between the background worker, content script and popup.

export type CaptureMode = 'region' | 'full';

export interface RecordOptions {
  workspaceId: string;
  title: string;
  mode: CaptureMode;
  autoRedact: boolean;
  includeTypedText: boolean;
  /** Extra regular expressions from the organisation policy. */
  patterns: string[];
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface CapturedAction {
  type: 'click' | 'double_click' | 'right_click' | 'type' | 'select' | 'key_shortcut';
  target_label?: string;
  target_role?: string;
  typed_text?: string;
  value_label?: string;
  keys?: string;
  url?: string;
  window_title?: string;
}

/** Sent by the content script for each user action. */
export interface StepMessage {
  kind: 'step';
  action: CapturedAction;
  /** Target element bounds in CSS pixels, relative to the viewport. */
  target: Rect | null;
  /** Click position in CSS pixels. */
  point: { x: number; y: number } | null;
  viewport: { w: number; h: number; dpr: number };
}

export interface StepDraft {
  id: string;
  title: string;
  action: CapturedAction & { x?: number; y?: number };
  highlight?: Rect;
  hasImage: boolean;
}

export interface Status {
  serverUrl: string | null;
  signedIn: boolean;
  user?: { display_name: string; email: string };
  recording: null | { tabId: number; steps: number; title: string; paused: boolean };
  lastGuideUrl?: string;
  error?: string;
}

export type Message =
  | StepMessage
  | { kind: 'status' }
  | { kind: 'setServer'; url: string }
  | { kind: 'signIn' }
  | { kind: 'signOut' }
  | { kind: 'workspaces' }
  | { kind: 'policy' }
  | { kind: 'start'; tabId: number; options: RecordOptions }
  | { kind: 'pause'; paused: boolean }
  | { kind: 'stop' }
  | { kind: 'discard' }
  | { kind: 'recordingOptions' };

export function uuidv7(): string {
  const ms = Date.now().toString(16).padStart(12, '0');
  const r = crypto.getRandomValues(new Uint8Array(10));
  const h = Array.from(r, (b) => b.toString(16).padStart(2, '0')).join('');
  const variant = ((parseInt(h[3], 16) & 3) | 8).toString(16);
  return `${ms.slice(0, 8)}-${ms.slice(8, 12)}-7${h.slice(0, 3)}-${variant}${h.slice(4, 7)}-${h.slice(7, 19)}`;
}

/** Normalise a server URL typed by a user: https://host[:port] without path. */
export function normalizeServer(input: string): string | null {
  let s = input.trim();
  if (!s) return null;
  if (!/^https?:\/\//i.test(s)) s = `https://${s}`;
  try {
    const u = new URL(s);
    if (u.protocol !== 'https:' && !(u.protocol === 'http:' && ['localhost', '127.0.0.1'].includes(u.hostname))) return null;
    return u.origin;
  } catch {
    return null;
  }
}
