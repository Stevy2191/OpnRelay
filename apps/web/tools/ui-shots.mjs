// SPDX-License-Identifier: AGPL-3.0-only
// Seeds a throwaway OpnRelay server with demo data through the REST API and
// saves screenshots of the main screens (light/dark desktop, light phone).
//
//   node tools/ui-shots.mjs --base http://127.0.0.1:8092 --out DIR \
//        [--owner-link URL | --owner-token TOKEN] [--owner-email owner@example.com] \
//        [--password PW] [--pages home,guide,edit,admin,...] [--database-url postgres://...]
//
// Playwright must be resolvable (`npm i -g playwright` + NODE_PATH, or local).
import { createRequire } from 'node:module';
import { mkdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';

const require = createRequire(import.meta.url);
let chromium;
try {
  ({ chromium } = require('playwright'));
} catch {
  ({ chromium } = await import('playwright'));
}

// ---- args ---------------------------------------------------------------
const args = {};
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (a.startsWith('--')) args[a.slice(2)] = process.argv[++i];
}
const base = (args.base ?? 'http://127.0.0.1:8092').replace(/\/$/, '');
const out = args.out ?? './ui-shots';
const password = args.password ?? 'Shots-Passw0rd-123!';
const ownerEmail = args['owner-email'] ?? 'owner@example.com';
const ALL_PAGES = ['login', 'home', 'all-guides', 'folder', 'ws-cards', 'ws-list', 'ws-empty', 'drafts', 'review', 'search', 'search-none', 'guide', 'guide-empty', 'edit', 'admin', 'account', 'audit', 'downloads', 'ws-settings'];
const pages = args.pages ? args.pages.split(',').map((s) => s.trim()) : ALL_PAGES;
const ownerToken = args['owner-token'] ?? (args['owner-link'] ? args['owner-link'].split('/invite/')[1] : undefined);

mkdirSync(out, { recursive: true });

// ---- tiny API client with a cookie jar ----------------------------------
class Client {
  constructor() { this.cookies = new Map(); this.csrf = ''; }
  async req(method, path, { body, raw, type, ifMatch, expect } = {}) {
    const headers = { Accept: 'application/json' };
    if (this.cookies.size) headers.Cookie = [...this.cookies].map(([k, v]) => `${k}=${v}`).join('; ');
    if (method !== 'GET') headers['X-CSRF-Token'] = this.csrf;
    if (ifMatch !== undefined) headers['If-Match'] = `"${ifMatch}"`;
    let payload;
    if (raw) { payload = raw; headers['Content-Type'] = type; }
    else if (body !== undefined) { payload = JSON.stringify(body); headers['Content-Type'] = 'application/json'; }
    const res = await fetch(base + '/api/v1' + path, { method, headers, body: payload });
    for (const c of res.headers.getSetCookie?.() ?? []) {
      const [kv] = c.split(';');
      const eq = kv.indexOf('=');
      this.cookies.set(kv.slice(0, eq).trim(), kv.slice(eq + 1));
    }
    const text = await res.text();
    let data; try { data = text ? JSON.parse(text) : undefined; } catch { data = text; }
    if (!res.ok && !(expect ?? []).includes(res.status)) {
      throw new Error(`${method} ${path} -> ${res.status} ${text.slice(0, 300)}`);
    }
    return { status: res.status, data };
  }
  async login(email) {
    const r = await this.req('POST', '/auth/login', { body: { email, password }, expect: [400, 401, 403] });
    if (r.status >= 400) return false;
    await this.refreshCsrf();
    return true;
  }
  async refreshCsrf() { this.csrf = (await this.req('GET', '/me')).data.csrf_token; }
  get(p) { return this.req('GET', p).then((r) => r.data); }
  post(p, body, o) { return this.req('POST', p, { body, ...o }).then((r) => r.data); }
}

async function acceptInvite(token, displayName) {
  const c = new Client();
  await c.req('POST', '/auth/invite/accept', { body: { token, password, display_name: displayName } });
}

// ---- demo PNG screenshots rendered with Playwright ----------------------
async function makeShots(browser) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await ctx.newPage();
  const shots = [];
  const variants = [
    ['Inbox', '#1d4ed8', ['Compose', 'Drafts', 'Sent', 'Archive']],
    ['Settings', '#0f766e', ['Profile', 'Security', 'Billing', 'Team']],
    ['Reports', '#7c3aed', ['Overview', 'Revenue', 'Churn', 'Export']],
  ];
  for (const [title, color, items] of variants) {
    await page.setContent(`<body style="margin:0;font:20px system-ui;background:#f3f4f6">
      <div style="display:flex;height:800px">
        <div style="width:240px;background:${color};color:#fff;padding:24px"><h2>${title}</h2>${items.map((i) => `<p style="opacity:.85">${i}</p>`).join('')}</div>
        <div style="flex:1;padding:32px"><div style="background:#fff;border-radius:8px;padding:24px;height:200px">Main panel</div>
        <div style="margin-top:24px;background:#fff;border-radius:8px;padding:24px;height:260px">Details</div>
        <button style="margin-top:24px;font-size:22px;padding:12px 28px;background:${color};color:#fff;border:0;border-radius:6px">Continue</button></div></div></body>`);
    shots.push(await page.screenshot({ type: 'png' }));
  }
  await ctx.close();
  return shots;
}

// ---- seed ---------------------------------------------------------------
const LONG_TITLE = 'How to reconcile the quarterly vendor invoices against purchase orders when the supplier changes bank details mid-quarter and approvals are pending';
const WORKSPACES = [
  { name: 'Human Resources & Benefits Administration', description: 'Onboarding, offboarding and benefits enrolment.', folders: ['Onboarding', 'Offboarding and exit interviews for contractors and fixed-term staff', 'Benefits'] },
  { name: 'IT Helpdesk', description: 'Day-to-day support procedures.', folders: ['Accounts', 'Hardware'] },
  { name: 'Finance', description: 'Month-end and AP procedures.', folders: ['Month-end', 'Accounts payable', 'Quarter and year-end close for subsidiaries and joint ventures'] },
  { name: 'Legal', description: '', folders: [] },
];
// [workspace index, folder index|null, title, kind]
const GUIDES = [
  [0, 0, 'Create a new hire profile', 'normal'],
  [0, 0, 'Order equipment for a new starter', 'normal'],
  [1, 0, 'Reset a forgotten password', 'normal'],
  [1, null, 'Set up two-factor authentication on a phone', 'noshots'],
  [2, 0, LONG_TITLE, 'long'],
  [2, null, 'Approve an expense report', 'review'],
  [2, 1, 'Match a supplier invoice to its purchase order', 'normal'],
  [2, null, 'Close the books checklist', 'noshots'],
];

function stepsFor(kind, mediaIds) {
  const act = (title, body, i) => ({ kind: 'action', title, body_md: body, media_id: mediaIds?.[i % (mediaIds?.length || 1)] ?? null,
    action: { type: 'click', target_label: title.replace(/^Click /, ''), app: 'Browser' },
    annotations: mediaIds ? { highlight: { x: 800 + 40 * (i % 3), y: 300 + 90 * (i % 3), w: 260, h: 70 } } : {} });
  const steps = [
    { kind: 'heading', title: 'Before you start', body_md: '', annotations: {} },
    act('Click Compose', 'Open the **Compose** window from the left sidebar.', 0),
    act('Click Settings', 'Choose *Settings* and then the **Security** tab.', 1),
    { kind: 'tip', title: 'Tip', body_md: 'You can use the keyboard shortcut `g` then `s` to jump to settings.', annotations: {} },
    act('Click Continue', 'Confirm the change with **Continue**.', 2),
    { kind: 'alert', title: 'Important', body_md: 'Never share the recovery codes shown on the next screen.', annotations: {} },
    { kind: 'text', title: 'Wrap up', body_md: 'Tell the requester the change is complete.', annotations: {} },
  ];
  if (kind === 'noshots') return steps.map((s) => ({ ...s, media_id: null, annotations: {} }));
  return steps;
}

async function seed(browser) {
  // owner
  let owner = new Client();
  if (!(await owner.login(ownerEmail))) {
    if (!ownerToken) throw new Error(`Cannot sign in as ${ownerEmail}. Pass --owner-link or --owner-token (from "admin create-owner").`);
    await acceptInvite(ownerToken, 'Olivia Owner');
    if (!(await owner.login(ownerEmail))) throw new Error('owner sign-in failed after accepting the invite');
  }
  const me = await owner.get('/me');
  console.log(`owner ok (${me.email})`);

  // editor + viewer
  const people = {};
  for (const [role, email, name] of [['member', 'editor@example.com', 'Eddie Editor'], ['member', 'viewer@example.com', 'Vera Viewer']]) {
    const c = new Client();
    if (!(await c.login(email))) {
      const r = await owner.req('POST', '/admin/users', { body: { email, display_name: name, org_role: role }, expect: [409] });
      if (r.status === 409) throw new Error(`${email} exists but cannot sign in with --password`);
      await acceptInvite(r.data.invite_url.split('/invite/')[1], name);
      await c.login(email);
    }
    people[email] = (await c.get('/me')).id;
  }
  console.log('editor + viewer ok');

  // workspaces, folders, members
  const existingWs = await owner.get('/workspaces');
  const ws = [];
  for (const w of WORKSPACES) {
    let row = existingWs.find((x) => x.name === w.name);
    if (!row) row = await owner.post('/workspaces', { name: w.name, description: w.description });
    const existingFolders = await owner.get(`/workspaces/${row.id}/folders`);
    const folders = [];
    for (const f of w.folders) {
      folders.push(existingFolders.find((x) => x.name === f) ?? (await owner.post(`/workspaces/${row.id}/folders`, { name: f, parent_id: null })));
    }
    const members = await owner.get(`/workspaces/${row.id}/members`);
    const want = [
      { principal_type: 'user', principal_id: people['editor@example.com'], role: 'editor' },
      { principal_type: 'user', principal_id: people['viewer@example.com'], role: 'viewer' },
    ];
    const keep = (Array.isArray(members) ? members : members.members ?? []).map((m) => ({ principal_type: m.principal_type ?? m.type, principal_id: m.principal_id ?? m.id, role: m.role }))
      .filter((m) => !want.some((x) => x.principal_id === m.principal_id));
    await owner.req('PUT', `/workspaces/${row.id}/members`, { body: { members: [...keep, ...want] } });
    ws.push({ ...row, folders });
  }
  console.log('workspaces ok');

  // guides
  const shots = await makeShots(browser);
  const existing = await owner.get('/guides');
  const list = Array.isArray(existing) ? existing : existing.items ?? existing.guides ?? [];
  const guides = [];
  for (const [wi, fi, title, kind] of GUIDES) {
    let g = list.find((x) => x.title === title);
    if (g && !(await owner.get(`/guides/${g.id}`)).steps?.length) g = undefined; // half-seeded earlier run: redo steps
    const seeded = g;
    if (!seeded) {
      g = list.find((x) => x.title === title) ?? await owner.post('/guides', { workspace_id: ws[wi].id, folder_id: fi === null ? null : ws[wi].folders[fi].id, title, description: 'Step-by-step procedure used by the team.' });
      let mediaIds = null;
      if (kind !== 'noshots') {
        mediaIds = [];
        for (const buf of shots) mediaIds.push((await owner.req('POST', `/media?guide_id=${g.id}`, { raw: buf, type: 'image/png' })).data.id);
      }
      let full = await owner.get(`/guides/${g.id}`);
      full = (await owner.req('PUT', `/guides/${g.id}/steps`, { body: { steps: stepsFor(kind, mediaIds) }, ifMatch: full.revision })).data;
      if (kind === 'long' || kind === 'review') {
        await owner.req('PATCH', `/guides/${g.id}`, { body: { classification: kind === 'long' ? 'restricted' : 'confidential' }, ifMatch: full.revision });
        full = await owner.get(`/guides/${g.id}`);
      }
      if (kind !== 'noshots' && title !== 'Order equipment for a new starter') {
        await owner.req('POST', `/guides/${g.id}/publish`, { body: { note: 'Initial version' }, ifMatch: full.revision });
      }
      g = await owner.get(`/guides/${g.id}`);
    }
    guides.push({ ...g, kind });
  }
  console.log(`guides ok (${guides.length})`);

  // needs-review: workspace review period + a published guide whose reviewed_at is old
  const reviewGuide = guides.find((g) => g.kind === 'review');
  await owner.req('PATCH', `/workspaces/${reviewGuide.workspace_id}`, { body: { review_period_days: 30 } });
  const sql = `update guides set reviewed_at = now() - interval '90 days' where id = '${reviewGuide.id}'`;
  if (args['database-url']) {
    execFileSync('psql', [args['database-url'], '-c', sql], { stdio: 'inherit' });
  } else {
    console.log(`NOTE: backdate the review guide so it shows as needing review:\n  psql "$OPNRELAY_DATABASE_URL" -c "${sql}"\n  (or pass --database-url)`);
  }
  return { ws, guides };
}

// ---- screenshots --------------------------------------------------------
const THEMES = [['light', 'desktop', { width: 1280, height: 800 }], ['dark', 'desktop', { width: 1280, height: 800 }], ['light', 'phone', { width: 390, height: 844 }]];

async function shoot(browser, data) {
  const byKind = (k) => data.guides.find((g) => g.kind === k);
  const normal = data.guides[0];
  const targets = {
    login: { anon: true, path: '/login' },
    home: { path: '/' },
    'all-guides': { path: '/' },
    'ws-cards': { path: `/?ws=${data.ws[2].id}`, view: 'cards' },
    'ws-list': { path: `/?ws=${data.ws[2].id}`, view: 'list' },
    'ws-empty': { path: `/?ws=${data.ws[3].id}`, view: 'cards' },
    drafts: { path: '/?status=draft' },
    review: { path: '/?needs_review=true' },
    search: { path: '/?q=invoice' },
    'search-none': { path: '/?q=zzzzqx' },
    folder: { path: `/?ws=${data.ws[0].id}&folder=${data.ws[0].folders[0].id}` },
    guide: { path: `/guides/${normal.id}` },
    'guide-empty': { path: `/guides/${byKind('noshots').id}` },
    edit: { path: `/guides/${normal.id}/edit` },
    admin: { path: '/admin' },
    account: { path: '/account' },
    audit: { path: '/audit' },
    downloads: { path: '/downloads' },
    'ws-settings': { path: `/workspaces/${data.ws[0].id}/settings` },
  };
  for (const [scheme, form, viewport] of THEMES) {
    const ctx = await browser.newContext({ viewport, colorScheme: scheme, deviceScaleFactor: form === 'phone' ? 2 : 1 });
    const anon = await ctx.newPage();
    const snap = async (p, name, path, view) => {
      await p.goto(`${base}/#${path}`);
      if (view) await p.evaluate((v) => localStorage.setItem('runbook.workspace-view', v), view);
      await p.reload();
      await p.waitForLoadState('networkidle');
      await p.waitForTimeout(400);
      const file = `${out}/${name}-${scheme}-${form}.png`;
      await p.screenshot({ path: file, fullPage: true });
      const overflow = await p.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      console.log(`${file}${overflow > 0 ? `  (horizontal overflow ${overflow}px)` : ''}`);
    };
    // signed-out pages first, then sign in once per browser context
    for (const name of pages) if (targets[name]?.anon) await snap(anon, name, targets[name].path);
    if (pages.some((n) => targets[n] && !targets[n].anon)) {
      const r = await ctx.request.post(base + '/api/v1/auth/login', { data: { email: ownerEmail, password }, timeout: 60000 });
      if (!r.ok()) throw new Error('browser login failed: ' + r.status());
      const authed = await ctx.newPage();
      for (const name of pages) {
        if (!targets[name]) console.warn('unknown page', name);
        else if (!targets[name].anon) await snap(authed, name, targets[name].path, targets[name].view);
      }
    }
    await ctx.close();
  }
}

const browser = await chromium.launch();
try {
  const data = await seed(browser);
  await shoot(browser, data);
} finally {
  await browser.close();
}
