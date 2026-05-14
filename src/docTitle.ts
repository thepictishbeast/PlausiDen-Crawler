/**
 * docTitle.ts — document title quality detector. T76.
 *
 * The page `<title>` is the first thing screen readers announce,
 * the only text users see in browser tabs / history / search
 * results, and the strongest single signal for SEO. Missing,
 * generic, or pathological titles are a high-leverage fix.
 *
 * Findings (per-page):
 *
 *   - title.missing            strict
 *     No `<title>` element at all. Browsers fall back to the URL.
 *     Screen readers announce "untitled document".
 *
 *   - title.empty              strict
 *     `<title></title>` or whitespace-only. Same effect.
 *
 *   - title.generic            warn
 *     Common Word/IDE defaults: "Document", "Untitled",
 *     "Untitled Document", "New Document", "New Page". Almost
 *     always copy-paste leftover.
 *
 *   - title.too-short          warn
 *     ≤ 2 characters after trimming. Likely a placeholder.
 *
 *   - title.too-long           warn
 *     > 70 characters. Search engines truncate; the user sees a
 *     fragmented preview.
 *
 * Cross-page duplication (e.g. same title on every URL of the site)
 * is a real defect but lives in the aggregates layer, not here —
 * this detector is per-page only.
 *
 * Mirror: crates/crawler-detectors/src/doc_title.rs.
 */
import type { Page } from 'playwright';

export interface DocTitleFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface DocTitleSnapshot {
  pageUrl: string;
  /** True iff at least one `<title>` element exists in `<head>`. */
  present: boolean;
  /** Inner text of the first `<title>`, untrimmed. Empty string if absent. */
  raw: string;
}

export async function captureDocTitleSnapshot(
  page: Page,
): Promise<DocTitleSnapshot> {
  const pageUrl = page.url();
  const evalFn = `(() => {
    const t = document.querySelector('head > title');
    if (!t) return { present: false, raw: '' };
    return { present: true, raw: t.textContent || '' };
  })()`;
  const result = (await page.evaluate(evalFn)) as { present: boolean; raw: string };
  return { pageUrl, present: result.present, raw: result.raw };
}

/**
 * Lowercased exact-match list of generic-default titles. Compared
 * against the trimmed-and-lowercased title — a title like
 * "Document - Acme" would NOT match "document" because we compare
 * the whole string. Keeping the list strict avoids false positives
 * on legitimately-titled pages.
 */
const GENERIC_TITLES = new Set<string>([
  'document',
  'untitled',
  'untitled document',
  'untitled page',
  'untitled-1',
  'new document',
  'new page',
  'new tab',
  'page',
  'home',  // arguable — leaving in since "Home" alone for a non-homepage is suspicious
  'index',
  'welcome',
  'title',
  'about:blank',
]);

const TITLE_TOO_SHORT_MAX = 2;
const TITLE_TOO_LONG_MIN = 70;

export function detectDocTitleIssues(
  snap: DocTitleSnapshot,
): DocTitleFinding[] {
  const out: DocTitleFinding[] = [];

  if (!snap.present) {
    out.push({
      severity: 'strict',
      kind: 'title.missing',
      detail: `Page has no <title> element in <head>. Browsers fall back to the URL; screen readers announce 'untitled document'. Add a unique, descriptive <title>.`,
      evidence: { present: false },
    });
    return out;
  }

  const trimmed = snap.raw.trim();
  if (trimmed === '') {
    out.push({
      severity: 'strict',
      kind: 'title.empty',
      detail: `Page <title> is empty or whitespace-only. Same effect as missing — the URL becomes the fallback title.`,
      evidence: { rawLength: snap.raw.length },
    });
    return out;
  }

  const lower = trimmed.toLowerCase();
  if (GENERIC_TITLES.has(lower)) {
    out.push({
      severity: 'warn',
      kind: 'title.generic',
      detail: `Page <title> is a generic default ('${trimmed}') — almost always copy-paste leftover from a template / IDE. Replace with a descriptive page-specific title.`,
      evidence: { title: trimmed },
    });
  }

  if (trimmed.length <= TITLE_TOO_SHORT_MAX) {
    out.push({
      severity: 'warn',
      kind: 'title.too-short',
      detail: `Page <title> is only ${trimmed.length} characters ('${trimmed}'). Search-result previews and screen-reader announcements need more context.`,
      evidence: { title: trimmed, length: trimmed.length },
    });
  }

  if (trimmed.length >= TITLE_TOO_LONG_MIN) {
    out.push({
      severity: 'warn',
      kind: 'title.too-long',
      detail: `Page <title> is ${trimmed.length} characters — Google truncates around 60-70 chars in search results. Trim or move detail into <meta name="description">.`,
      evidence: { title: trimmed.slice(0, 80) + '…', length: trimmed.length },
    });
  }

  return out;
}

export async function checkDocTitle(
  page: Page,
): Promise<{ snapshot: DocTitleSnapshot; findings: DocTitleFinding[] }> {
  const snapshot = await captureDocTitleSnapshot(page);
  const findings = detectDocTitleIssues(snapshot);
  return { snapshot, findings };
}
