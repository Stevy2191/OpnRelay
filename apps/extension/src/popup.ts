// SPDX-License-Identifier: AGPL-3.0-only
// Toolbar popup: configure server, sign in, start/stop recording.
import type { CaptureMode, Message, Status } from './shared';

const app = document.getElementById('app')!;

async function send<T>(msg: Message): Promise<T> {
  const res = await chrome.runtime.sendMessage(msg);
  if (!res?.ok) throw new Error(res?.error ?? 'Something went wrong');
  return res.data as T;
}

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!);
}

function h(html: string) {
  app.innerHTML = html;
}

function showError(e: unknown) {
  const p = document.getElementById('err');
  if (p) p.textContent = e instanceof Error ? e.message : String(e);
}

function bind(id: string, fn: () => unknown) {
  document.getElementById(id)?.addEventListener('click', async () => {
    const btn = document.getElementById(id) as HTMLButtonElement;
    btn.disabled = true;
    try {
      await fn();
    } catch (e) {
      showError(e);
    } finally {
      btn.disabled = false;
    }
  });
}

async function render() {
  let s: Status;
  try {
    s = await send<Status>({ kind: 'status' });
  } catch (e) {
    h(`<p class="error">${esc((e as Error).message)}</p>`);
    return;
  }
  if (!s.serverUrl) return renderServer();
  if (!s.signedIn) return renderSignIn(s);
  if (s.recording) return renderRecording(s);
  return renderStart(s);
}

function renderServer(current = '') {
  h(`<p>Enter your company's OpnRelay address.</p>
     <label for="srv">Server address</label>
     <input id="srv" type="url" placeholder="https://guides.company.com" value="${esc(current)}">
     <div class="row"><button id="save" class="primary">Continue</button></div>
     <p id="err" class="error small"></p>`);
  bind('save', async () => {
    const url = (document.getElementById('srv') as HTMLInputElement).value;
    const origin = new URL(/^https?:\/\//.test(url) ? url : `https://${url}`).origin;
    // Ask for access to the server only (not every site).
    const granted = await chrome.permissions.request({ origins: [`${origin}/*`] });
    if (!granted) throw new Error('Permission to reach the server is needed.');
    await send({ kind: 'setServer', url });
    await render();
  });
}

function renderSignIn(s: Status) {
  h(`<p>Sign in to <strong>${esc(new URL(s.serverUrl!).host)}</strong> to record guides.</p>
     <div class="row"><button id="in" class="primary">Sign in</button></div>
     <p class="small"><a href="#" id="change">Use a different server</a></p>
     ${s.error ? `<p class="error small">${esc(s.error)}</p>` : ''}
     <p id="err" class="error small"></p>`);
  bind('in', async () => {
    await send({ kind: 'signIn' });
    await render();
  });
  document.getElementById('change')!.addEventListener('click', (e) => {
    e.preventDefault();
    renderServer(s.serverUrl ?? '');
  });
}

async function renderStart(s: Status) {
  const [workspaces, policy] = await Promise.all([
    send<{ id: string; name: string }[]>({ kind: 'workspaces' }),
    send<{ mode: CaptureMode; autoRedact: boolean; includeTypedText: boolean; patterns: string[] }>({ kind: 'policy' }),
  ]);
  const { lastWorkspace } = await chrome.storage.local.get('lastWorkspace');
  if (workspaces.length === 0) {
    h(`<p>You can't create guides yet: ask an administrator to make you an editor in a workspace.</p>`);
    return;
  }
  h(`${s.lastGuideUrl ? `<p class="small">✅ Last guide uploaded. <a href="${esc(s.lastGuideUrl)}" target="_blank">Open it</a></p>` : ''}
    <label for="title">Guide title</label>
    <input id="title" type="text" maxlength="200" placeholder="e.g. Add a new employee">
    <label for="ws">Workspace</label>
    <select id="ws">${workspaces.map((w) => `<option value="${esc(w.id)}" ${w.id === lastWorkspace ? 'selected' : ''}>${esc(w.name)}</option>`).join('')}</select>
    <label for="mode">Screenshots</label>
    <select id="mode">
      <option value="region" ${policy.mode === 'region' ? 'selected' : ''}>Area around each click</option>
      <option value="full" ${policy.mode === 'full' ? 'selected' : ''}>Whole page view</option>
    </select>
    <label class="inline"><input id="redact" type="checkbox" ${policy.autoRedact ? 'checked' : ''}> Hide emails, phone numbers and other sensitive data</label>
    <label class="inline"><input id="typed" type="checkbox" ${policy.includeTypedText ? 'checked' : ''}> Include what I type (never passwords)</label>
    <div class="row"><button id="start" class="primary">● Start recording</button></div>
    <p class="muted small">Signed in as ${esc(s.user!.display_name)} · <a href="#" id="out">Sign out</a></p>
    <p id="err" class="error small"></p>`);
  bind('start', async () => {
    // Must be the first await: browsers only show permission prompts during a click.
    // Needed to keep recording when the tab moves to other pages or sites.
    const granted = await chrome.permissions.request({ origins: ['<all_urls>'] });
    if (!granted) throw new Error('OpnRelay needs access to web pages to take screenshots while you record.');
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab?.id || !/^https?:/.test(tab.url ?? '')) throw new Error('Open the web page you want to record first.');
    const workspaceId = (document.getElementById('ws') as HTMLSelectElement).value;
    await chrome.storage.local.set({ lastWorkspace: workspaceId });
    await send({
      kind: 'start',
      tabId: tab.id,
      options: {
        workspaceId,
        title: (document.getElementById('title') as HTMLInputElement).value.trim() || tab.title || 'New guide',
        mode: (document.getElementById('mode') as HTMLSelectElement).value as CaptureMode,
        autoRedact: (document.getElementById('redact') as HTMLInputElement).checked,
        includeTypedText: (document.getElementById('typed') as HTMLInputElement).checked,
        patterns: policy.patterns,
      },
    });
    window.close();
  });
  document.getElementById('out')!.addEventListener('click', async (e) => {
    e.preventDefault();
    await send({ kind: 'signOut' });
    await render();
  });
}

function renderRecording(s: Status) {
  const r = s.recording!;
  h(`<p class="rec"><span class="dot ${r.paused ? 'paused' : ''}"></span>${r.paused ? 'Paused' : 'Recording'} <span class="mono">· ${r.steps} step${r.steps === 1 ? '' : 's'}</span></p>
     <p class="muted small">${esc(r.title)}</p>
     <div class="row">
       <button id="pause">${r.paused ? 'Resume' : 'Pause'}</button>
       <button id="stop" class="primary">Finish & upload</button>
     </div>
     <div class="row"><button id="discard" class="danger">Discard recording</button></div>
     <p id="err" class="error small"></p>`);
  bind('pause', async () => {
    await send({ kind: 'pause', paused: !r.paused });
    await render();
  });
  bind('stop', async () => {
    (document.getElementById('stop') as HTMLButtonElement).textContent = 'Uploading…';
    await send({ kind: 'stop' });
    await render();
  });
  bind('discard', async () => {
    if (!confirm('Discard all recorded steps?')) return;
    await send({ kind: 'discard' });
    await render();
  });
}

render();
