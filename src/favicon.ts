/**
 * favicon.ts — favicon link detector. T76.
 *
 * Browsers fetch `/favicon.ico` automatically when no `<link rel="icon">`
 * is declared in head; if that path 404s, the page ships without an
 * identifying icon in browser tabs, bookmarks, history, and PWA
 * install prompts. Real users see a generic globe glyph and the
 * site reads as unfinished.
 *
 * Findings (per-page):
 *
 *   - favicon.missing-link    warn
 *     No `<link rel="icon">`, `rel="shortcut icon"`, or
 *     `rel="apple-touch-icon"` in the page <head>. The default
 *     /favicon.ico fallback may or may not exist; this finding
 *     is about the EXPLICIT contract — declaring an icon makes
 *     the path stable and lets you serve a real PNG/SVG instead
 *     of a 16×16 ICO.
 *
 * The companion finding `favicon.broken` (link exists but the
 * resource 404s) is intentionally NOT in this detector — the
 * existing `failed-requests` axis already captures any /favicon.ico
 * 404 when the browser auto-fetches. A future detector could
 * cross-reference declared icon links with response status, but
 * for V1 the missing-link warn is the right scope.
 *
 * Mirror: crates/crawler-detectors/src/favicon.rs.
 */
import type { Page } from 'playwright';

export interface FaviconFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface FaviconSnapshot {
  pageUrl: string;
  /** Number of `<link rel*="icon">` (or apple-touch-icon) tags in head. */
  iconLinkCount: number;
  /** Distinct rel values seen (e.g. "icon", "shortcut icon",
   *  "apple-touch-icon"). Helps the report describe what's there. */
  relValues: string[];
}

export async function captureFaviconSnapshot(
  page: Page,
): Promise<FaviconSnapshot> {
  const pageUrl = page.url();
  const evalFn = `(() => {
    // Match rel tokens (case-insensitive) of icon / shortcut icon /
    // apple-touch-icon / mask-icon. Authors split these across
    // multiple <link> tags for different sizes/formats.
    const links = document.querySelectorAll('head link[rel]');
    const rels = [];
    let count = 0;
    for (let i = 0; i < links.length; i++) {
      const rel = (links[i].getAttribute('rel') || '').toLowerCase().trim();
      if (!rel) continue;
      const tokens = rel.split(/\\s+/).filter(Boolean);
      const isIcon = tokens.some(function(t) {
        return t === 'icon' || t === 'shortcut' || t === 'apple-touch-icon' || t === 'mask-icon';
      });
      if (isIcon) {
        count += 1;
        if (rels.indexOf(rel) < 0) rels.push(rel);
      }
    }
    return { iconLinkCount: count, relValues: rels };
  })()`;
  const result = (await page.evaluate(evalFn)) as { iconLinkCount: number; relValues: string[] };
  return { pageUrl, iconLinkCount: result.iconLinkCount, relValues: result.relValues };
}

export function detectFaviconIssues(snap: FaviconSnapshot): FaviconFinding[] {
  if (snap.iconLinkCount > 0) return [];
  return [
    {
      severity: 'warn',
      kind: 'favicon.missing-link',
      detail: `Page <head> declares no <link rel="icon"> (or apple-touch-icon / mask-icon / shortcut icon). Browsers fall back to fetching /favicon.ico automatically; if that path 404s, browser tabs / bookmarks / history / PWA-install prompts show a generic glyph and the site reads as unfinished. Add at least <link rel="icon" href="/favicon.svg" type="image/svg+xml">.`,
      evidence: { iconLinkCount: 0 },
    },
  ];
}

export async function checkFavicon(
  page: Page,
): Promise<{ snapshot: FaviconSnapshot; findings: FaviconFinding[] }> {
  const snapshot = await captureFaviconSnapshot(page);
  const findings = detectFaviconIssues(snapshot);
  return { snapshot, findings };
}
