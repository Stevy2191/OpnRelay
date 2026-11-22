// SPDX-License-Identifier: AGPL-3.0-only
// Minimal hash router. Tokens in invite links live in the fragment so they
// are never sent to the server or written to access logs.

function current(): string {
  const h = window.location.hash.replace(/^#/, '');
  return h || '/';
}

export const route = $state({ path: current() });

/**
 * Optional guard asked before the route changes (e.g. the editor saving its
 * changes first). Return `true` to allow at once, or a promise of the answer.
 */
type NavGuard = (to: string) => boolean | Promise<boolean>;
let guard: NavGuard | null = null;

/** Install a navigation guard; returns a function that removes it. */
export function setNavGuard(g: NavGuard): () => void {
  guard = g;
  return () => {
    if (guard === g) guard = null;
  };
}

window.addEventListener('hashchange', () => {
  const to = current();
  if (to === route.path) return;
  const answer = guard ? guard(to) : true;
  if (answer === true) {
    route.path = to;
    return;
  }
  // Put the address back while the guard decides; replaceState fires no hashchange.
  const from = route.path;
  history.replaceState(history.state, '', `#${from}`);
  Promise.resolve(answer).then((ok) => {
    if (!ok || route.path !== from) return;
    history.replaceState(history.state, '', `#${to}`);
    route.path = to;
  });
});

// While a guard is set, in-app links go through go() so a refused navigation
// leaves no extra history entry (back/forward still use the hashchange path).
window.addEventListener(
  'click',
  (e) => {
    if (!guard || e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = (e.target as Element | null)?.closest?.('a[href^="#/"]') as HTMLAnchorElement | null;
    if (!a || (a.target && a.target !== '_self')) return;
    e.preventDefault();
    go(a.getAttribute('href')!.slice(1));
  },
  true,
);

function navigate(path: string) {
  if (current() !== path) window.location.hash = path;
  // history.replaceState() changes the hash without a hashchange event, so
  // always sync the reactive route as well.
  route.path = path;
}

/** Navigate; resolves true once the route changed, false if a guard refused. */
export function go(path: string): Promise<boolean> {
  const answer = guard && path !== route.path ? guard(path) : true;
  if (answer === true) {
    navigate(path);
    return Promise.resolve(true);
  }
  return Promise.resolve(answer).then((ok) => {
    if (ok) navigate(path);
    return !!ok;
  });
}

/** Ask the guard (if any) whether leaving for `path` is allowed, without navigating. */
export async function canLeave(path: string): Promise<boolean> {
  return !guard || path === route.path ? true : !!(await guard(path));
}

/** Match `pattern` like `/guides/:id/edit` against the current path. */
export function match(pattern: string, path: string): Record<string, string> | null {
  const p = pattern.split('/').filter(Boolean);
  const s = path.split('?')[0].split('/').filter(Boolean);
  if (p.length !== s.length) return null;
  const out: Record<string, string> = {};
  for (let i = 0; i < p.length; i++) {
    if (p[i].startsWith(':')) out[p[i].slice(1)] = decodeURIComponent(s[i]);
    else if (p[i] !== s[i]) return null;
  }
  return out;
}

export function query(path: string): URLSearchParams {
  const i = path.indexOf('?');
  return new URLSearchParams(i >= 0 ? path.slice(i + 1) : '');
}
