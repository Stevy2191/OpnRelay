// SPDX-License-Identifier: AGPL-3.0-only
// Builds dist/chromium (Chrome, Edge) and dist/firefox.
// TEST=1 grants host access at install time (for automated tests only).
import { build } from 'esbuild';
import { cpSync, mkdirSync, rmSync, writeFileSync, readFileSync } from 'node:fs';

const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
const test = process.env.TEST === '1';

const base = {
  manifest_version: 3,
  name: 'OpnRelay Recorder',
  short_name: 'OpnRelay',
  version: pkg.version,
  description: "Record step-by-step guides straight into your organisation's own OpnRelay server.",
  icons: { 16: 'icons/icon-16.png', 32: 'icons/icon-32.png', 48: 'icons/icon-48.png', 128: 'icons/icon-128.png' },
  action: { default_popup: 'popup.html', default_title: 'OpnRelay Recorder', default_icon: { 16: 'icons/icon-16.png', 32: 'icons/icon-32.png' } },
  permissions: ['activeTab', 'scripting', 'storage', 'identity'],
  optional_host_permissions: ['<all_urls>'],
  ...(test ? { host_permissions: ['<all_urls>'] } : {}),
  storage: { managed_schema: 'managed-schema.json' },
  content_security_policy: { extension_pages: "script-src 'self'; object-src 'none'" },
};

const targets = {
  chromium: { ...base, background: { service_worker: 'background.js' }, minimum_chrome_version: '116' },
  firefox: {
    ...base,
    background: { scripts: ['background.js'] },
    browser_specific_settings: { gecko: { id: 'recorder@opnrelay.dev', strict_min_version: '128.0' } },
  },
};

for (const [name, manifest] of Object.entries(targets)) {
  const out = `dist/${name}`;
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  await build({
    entryPoints: { background: 'src/background.ts', content: 'src/content.ts', popup: 'src/popup.ts' },
    bundle: true,
    format: 'iife',
    target: name === 'firefox' ? 'firefox128' : 'chrome116',
    outdir: out,
    minify: !test,
    legalComments: 'none',
  });
  cpSync('static', out, { recursive: true });
  if (name === 'firefox') delete manifest.storage; // Firefox uses enterprise policies.json instead
  writeFileSync(`${out}/manifest.json`, JSON.stringify(manifest, null, 2));
  console.log(`built ${out}`);
}
