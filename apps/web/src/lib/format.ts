// SPDX-License-Identifier: AGPL-3.0-only

export function when(iso: string | null | undefined): string {
  if (!iso) return '—';
  const d = new Date(iso);
  const diff = (Date.now() - d.getTime()) / 1000;
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)} min ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} h ago`;
  if (diff < 86400 * 7) return `${Math.floor(diff / 86400)} d ago`;
  return d.toLocaleDateString();
}

export function dateTime(iso: string | null | undefined): string {
  return iso ? new Date(iso).toLocaleString() : '—';
}

export function errorText(e: unknown): string {
  if (e instanceof Error) return e.message;
  return String(e);
}

/**
 * Very small, safe Markdown subset renderer: paragraphs, **bold**, *italic*,
 * `code`, and bullet lists. Everything is escaped first; no raw HTML passes.
 */
export function miniMarkdown(src: string): string {
  const esc = (s: string) =>
    s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  const inline = (s: string) =>
    esc(s)
      .replace(/`([^`]+)`/g, '<code>$1</code>')
      .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
      .replace(/\*([^*]+)\*/g, '<em>$1</em>');
  const blocks = src.trim().split(/\n{2,}/);
  return blocks
    .map((b) => {
      const lines = b.split('\n');
      if (lines.every((l) => /^\s*[-*]\s+/.test(l))) {
        return '<ul>' + lines.map((l) => `<li>${inline(l.replace(/^\s*[-*]\s+/, ''))}</li>`).join('') + '</ul>';
      }
      return `<p>${lines.map(inline).join('<br>')}</p>`;
    })
    .join('');
}

/** Up to two initials for an avatar: "Ada Lovelace" -> "AL", "ada@x.test" -> "A". */
export function initials(name: string | null | undefined): string {
  const parts = (name ?? '').trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return '?';
  if (parts.length === 1) return parts[0].charAt(0).toUpperCase();
  return (parts[0].charAt(0) + parts[parts.length - 1].charAt(0)).toUpperCase();
}

/** Whole months a review has been overdue (0 when under a month or not due). */
export function monthsOverdue(dueIso: string | null | undefined, now: number = Date.now()): number {
  if (!dueIso) return 0;
  const days = (now - new Date(dueIso).getTime()) / 86400000;
  return days >= 30 ? Math.floor(days / 30) : 0;
}
