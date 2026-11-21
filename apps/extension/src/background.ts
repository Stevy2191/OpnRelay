// SPDX-License-Identifier: AGPL-3.0-only
// Background worker: sign-in (OAuth + PKCE), recording state, screenshots,
// and upload to the OpnRelay server.
import { normalizeServer, uuidv7, type Message, type RecordOptions, type Rect, type Status, type StepDraft, type StepMessage } from './shared';
import { title } from './titles';

const CLIENT_ID = 'opnrelay-extension';
const REGION_CSS = { w: 1000, h: 640 };
const MIN_CAPTURE_GAP_MS = 550; // browsers allow ~2 captures per second

// ---------------------------------------------------------------- storage --
interface Recording {
  tabId: number;
  options: RecordOptions;
  steps: StepDraft[];
  paused: boolean;
}

const local = chrome.storage.local;
const session = chrome.storage.session ?? chrome.storage.local;

async function getServer(): Promise<string | null> {
  const managed = await chrome.storage.managed?.get('serverUrl').catch(() => ({}) as Record<string, unknown>);
  const fromPolicy = typeof managed?.serverUrl === 'string' ? normalizeServer(managed.serverUrl) : null;
  if (fromPolicy) return fromPolicy;
  const { serverUrl } = await local.get('serverUrl');
  return typeof serverUrl === 'string' ? serverUrl : null;
}

async function getRecording(): Promise<Recording | null> {
  const { recording } = await session.get('recording');
  return (recording as Recording) ?? null;
}

async function setRecording(r: Recording | null) {
  if (r) await session.set({ recording: r });
  else await session.remove('recording');
  await chrome.action.setBadgeText({ text: r ? (r.paused ? '❚❚' : 'REC') : '' });
  await chrome.action.setBadgeBackgroundColor({ color: r?.paused ? '#5d6673' : '#c4314b' });
}

// Screenshots live in IndexedDB (too large for extension storage).
function idb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open('opnrelay', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('images');
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function idbOp<T>(mode: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await idb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction('images', mode);
    const req = fn(tx.objectStore('images'));
    tx.oncomplete = () => resolve(req.result);
    tx.onerror = () => reject(tx.error);
  });
}

const putImage = (id: string, blob: Blob) => idbOp('readwrite', (s) => s.put(blob, id));
const getImage = (id: string) => idbOp<Blob | undefined>('readonly', (s) => s.get(id));
const clearImages = () => idbOp('readwrite', (s) => s.clear());

// ------------------------------------------------------------------ auth --
function b64url(bytes: ArrayBuffer | Uint8Array): string {
  const b = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let s = '';
  b.forEach((x) => (s += String.fromCharCode(x)));
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

interface Tokens {
  access_token: string;
  refresh_token: string;
  expires_in: number;
}

async function saveTokens(t: Tokens) {
  await local.set({ refreshToken: t.refresh_token });
  await session.set({ accessToken: t.access_token, accessExp: Date.now() + (t.expires_in - 30) * 1000 });
}

async function clearTokens() {
  await local.remove('refreshToken');
  await session.remove(['accessToken', 'accessExp']);
}

async function tokenRequest(server: string, body: Record<string, string>): Promise<Tokens> {
  const res = await fetch(`${server}/api/v1/oauth/token`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error((await res.json().catch(() => ({}))).detail ?? `sign-in failed (${res.status})`);
  return res.json();
}

async function accessToken(server: string): Promise<string | null> {
  const { accessToken: at, accessExp } = await session.get(['accessToken', 'accessExp']);
  if (typeof at === 'string' && typeof accessExp === 'number' && accessExp > Date.now()) return at;
  const { refreshToken } = await local.get('refreshToken');
  if (typeof refreshToken !== 'string') return null;
  try {
    const t = await tokenRequest(server, { grant_type: 'refresh_token', refresh_token: refreshToken });
    await saveTokens(t);
    return t.access_token;
  } catch {
    await clearTokens();
    return null;
  }
}

async function signIn(): Promise<void> {
  const server = await getServer();
  if (!server) throw new Error('Set your OpnRelay server address first.');
  const verifier = b64url(crypto.getRandomValues(new Uint8Array(32)));
  const challenge = b64url(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verifier)));
  const state = b64url(crypto.getRandomValues(new Uint8Array(16)));
  const redirect = chrome.identity.getRedirectURL();
  const browser = navigator.userAgent.includes('Firefox') ? 'Firefox' : navigator.userAgent.includes('Edg/') ? 'Edge' : 'Chrome';
  const p = new URLSearchParams({
    client_id: CLIENT_ID,
    redirect_uri: redirect,
    code_challenge: challenge,
    code_challenge_method: 'S256',
    state,
    device_name: `${browser} extension`,
  });
  const result = await chrome.identity.launchWebAuthFlow({ url: `${server}/#/connect?${p}`, interactive: true });
  if (!result) throw new Error('Sign-in was cancelled.');
  const back = new URL(result);
  if (back.searchParams.get('state') !== state) throw new Error('Sign-in failed: state mismatch.');
  const code = back.searchParams.get('code');
  if (!code) throw new Error('Sign-in failed: no authorisation code.');
  await saveTokens(
    await tokenRequest(server, { grant_type: 'authorization_code', code, code_verifier: verifier, redirect_uri: redirect, client_id: CLIENT_ID }),
  );
}

async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const server = await getServer();
  if (!server) throw new Error('No server configured');
  const token = await accessToken(server);
  if (!token) throw new Error('Not signed in');
  const res = await fetch(`${server}/api/v1${path}`, {
    ...init,
    headers: { ...(init.headers as Record<string, string>), Authorization: `Bearer ${token}` },
  });
  if (res.status === 401) {
    await session.remove(['accessToken', 'accessExp']);
    throw new Error('Your session expired. Sign in again.');
  }
  if (!res.ok) throw new Error((await res.json().catch(() => ({}))).detail ?? `request failed (${res.status})`);
  return (res.status === 204 ? undefined : await res.json()) as T;
}

// --------------------------------------------------------------- capture --
let lastCapture = 0;
let chain: Promise<unknown> = Promise.resolve();

function cropBox(mode: string, vp: StepMessage['viewport'], point: StepMessage['point'], img: { w: number; h: number }): Rect {
  if (mode !== 'region' || !point) return { x: 0, y: 0, w: img.w, h: img.h };
  const w = Math.min(img.w, Math.round(REGION_CSS.w * vp.dpr));
  const h = Math.min(img.h, Math.round(REGION_CSS.h * vp.dpr));
  const cx = point.x * vp.dpr;
  const cy = point.y * vp.dpr;
  const x = Math.max(0, Math.min(img.w - w, Math.round(cx - w / 2)));
  const y = Math.max(0, Math.min(img.h - h, Math.round(cy - h / 2)));
  return { x, y, w, h };
}

async function captureStep(msg: StepMessage, tabId: number): Promise<void> {
  const rec = await getRecording();
  if (!rec || rec.tabId !== tabId || rec.paused) return;
  const wait = lastCapture + MIN_CAPTURE_GAP_MS - Date.now();
  if (wait > 0) await new Promise((r) => setTimeout(r, wait));
  lastCapture = Date.now();
  const tab = await chrome.tabs.get(tabId);
  const dataUrl = await chrome.tabs.captureVisibleTab(tab.windowId, { format: 'png' });
  const bitmap = await createImageBitmap(await (await fetch(dataUrl)).blob());
  // Screenshots can be taken at a different scale than devicePixelRatio.
  const scale = bitmap.width / msg.viewport.w;
  const vp = { ...msg.viewport, dpr: scale };
  const crop = cropBox(rec.options.mode, vp, msg.point, { w: bitmap.width, h: bitmap.height });
  const canvas = new OffscreenCanvas(crop.w, crop.h);
  canvas.getContext('2d')!.drawImage(bitmap, crop.x, crop.y, crop.w, crop.h, 0, 0, crop.w, crop.h);
  bitmap.close();
  const blob = await canvas.convertToBlob({ type: 'image/png' });

  const id = uuidv7();
  let highlight: Rect | undefined;
  if (msg.target) {
    const pad = 4 * scale;
    highlight = {
      x: Math.round(msg.target.x * scale - crop.x - pad),
      y: Math.round(msg.target.y * scale - crop.y - pad),
      w: Math.round(msg.target.w * scale + 2 * pad),
      h: Math.round(msg.target.h * scale + 2 * pad),
    };
  }
  await putImage(id, blob);
  const step: StepDraft = {
    id,
    title: title(msg.action),
    action: {
      ...msg.action,
      url: tab.url,
      window_title: tab.title,
      x: msg.point ? Math.round(msg.point.x * scale - crop.x) : undefined,
      y: msg.point ? Math.round(msg.point.y * scale - crop.y) : undefined,
    },
    highlight,
    hasImage: true,
  };
  const latest = await getRecording();
  if (!latest) return;
  latest.steps.push(step);
  await setRecording(latest);
}

async function inject(tabId: number) {
  await chrome.scripting.executeScript({ target: { tabId }, files: ['content.js'] }).catch(() => {
    // Pages like the browser's own settings cannot be recorded.
  });
}

chrome.tabs.onUpdated.addListener(async (tabId, info) => {
  if (info.status !== 'complete') return;
  const rec = await getRecording();
  if (rec && rec.tabId === tabId) await inject(tabId);
});

chrome.tabs.onRemoved.addListener(async (tabId) => {
  const rec = await getRecording();
  if (rec && rec.tabId === tabId) {
    rec.paused = true;
    await setRecording(rec);
  }
});

// ---------------------------------------------------------------- upload --
async function upload(rec: Recording): Promise<string> {
  const server = (await getServer())!;
  const guide = await api<{ id: string }>('/guides', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ workspace_id: rec.options.workspaceId, title: rec.options.title || 'New guide', source: 'extension' }),
  });
  const steps = [];
  for (const s of rec.steps) {
    let media_id: string | null = null;
    if (s.hasImage) {
      const blob = await getImage(s.id);
      if (blob) {
        const m = await api<{ id: string }>(`/media?guide_id=${guide.id}`, {
          method: 'POST',
          headers: { 'Content-Type': 'image/png' },
          body: blob,
        });
        media_id = m.id;
      }
    }
    steps.push({
      id: s.id,
      kind: 'action',
      title: s.title,
      body_md: '',
      action: s.action,
      media_id,
      annotations: s.highlight ? { highlight: s.highlight } : {},
    });
  }
  await api(`/guides/${guide.id}/steps`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ steps }),
  });
  return `${server}/#/guides/${guide.id}/edit`;
}

// -------------------------------------------------------------- messages --
async function status(): Promise<Status> {
  const server = await getServer();
  const rec = await getRecording();
  const { lastGuideUrl } = await local.get('lastGuideUrl');
  const out: Status = {
    serverUrl: server,
    signedIn: false,
    recording: rec ? { tabId: rec.tabId, steps: rec.steps.length, title: rec.options.title, paused: rec.paused } : null,
    lastGuideUrl: typeof lastGuideUrl === 'string' ? lastGuideUrl : undefined,
  };
  if (server && (await accessToken(server))) {
    try {
      out.user = await api('/me');
      out.signedIn = true;
    } catch (e) {
      out.error = (e as Error).message;
    }
  }
  return out;
}

async function handle(msg: Message, sender: chrome.runtime.MessageSender): Promise<unknown> {
  switch (msg.kind) {
    case 'step': {
      if (!sender.tab?.id) return { ok: false };
      const tabId = sender.tab.id;
      const run = chain.then(() => captureStep(msg, tabId));
      chain = run.catch(() => undefined);
      await run;
      return { ok: true };
    }
    case 'recordingOptions': {
      const rec = await getRecording();
      if (!rec || rec.tabId !== sender.tab?.id || rec.paused) return null;
      return rec.options;
    }
    case 'status':
      return status();
    case 'setServer': {
      const url = normalizeServer(msg.url);
      if (!url) throw new Error('Enter an https:// address, e.g. https://guides.company.com');
      await local.set({ serverUrl: url });
      await clearTokens();
      return status();
    }
    case 'signIn':
      await signIn();
      return status();
    case 'signOut': {
      const server = await getServer();
      const { refreshToken } = await local.get('refreshToken');
      if (server && typeof refreshToken === 'string') {
        await fetch(`${server}/api/v1/oauth/revoke`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ token: refreshToken }),
        }).catch(() => undefined);
      }
      await clearTokens();
      return status();
    }
    case 'workspaces': {
      const ws = await api<{ id: string; name: string; my_permission: string }[]>('/workspaces');
      return ws.filter((w) => w.my_permission === 'edit' || w.my_permission === 'manage');
    }
    case 'policy': {
      const me = await api<{ organization: { default_capture_mode: string; auto_redaction_default: boolean; redaction_patterns?: string[] } }>('/me');
      const prefs = await api<{ capture_mode: string | null; auto_redaction: boolean | null; include_typed_text: boolean }>('/me/preferences').catch(() => null);
      return {
        mode: prefs?.capture_mode === 'full' || me.organization.default_capture_mode === 'full' ? 'full' : 'region',
        autoRedact: prefs?.auto_redaction ?? me.organization.auto_redaction_default,
        includeTypedText: prefs?.include_typed_text ?? false,
        patterns: me.organization.redaction_patterns ?? [],
      };
    }
    case 'start': {
      await clearImages();
      await local.remove('lastGuideUrl');
      await setRecording({ tabId: msg.tabId, options: msg.options, steps: [], paused: false });
      await inject(msg.tabId);
      return status();
    }
    case 'pause': {
      const rec = await getRecording();
      if (rec) {
        rec.paused = msg.paused;
        await setRecording(rec);
        if (!msg.paused) await inject(rec.tabId);
      }
      return status();
    }
    case 'stop': {
      await chain;
      const rec = await getRecording();
      if (!rec) return status();
      if (rec.steps.length === 0) {
        await setRecording(null);
        throw new Error('No steps were recorded.');
      }
      const url = await upload(rec);
      await setRecording(null);
      await clearImages();
      await local.set({ lastGuideUrl: url });
      await chrome.tabs.create({ url });
      return status();
    }
    case 'discard':
      await setRecording(null);
      await clearImages();
      return status();
  }
}

chrome.runtime.onMessage.addListener((msg: Message, sender, sendResponse) => {
  handle(msg, sender).then(
    (r) => sendResponse({ ok: true, data: r }),
    (e: Error) => sendResponse({ ok: false, error: e.message }),
  );
  return true; // async response
});

// Exposed for automated end-to-end tests only (not reachable from web pages).
(globalThis as unknown as { opnrelay: unknown }).opnrelay = { handle, saveTokens };
