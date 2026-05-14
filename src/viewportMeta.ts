/**
 * viewportMeta.ts — viewport meta tag detector. T76.
 *
 * Three findings:
 *
 *   - viewport.missing                strict
 *     No `<meta name="viewport" ...>` at all. The page renders at
 *     the desktop default of 980px and is then scaled by the
 *     mobile browser, producing the "tiny everything" experience
 *     that has plagued legacy sites since 2008. WCAG 1.4.10
 *     (Reflow, AA) requires content to be presented without
 *     two-dimensional scrolling — a missing viewport tag breaks
 *     this for almost every screen narrower than 980px.
 *
 *   - viewport.no-device-width        strict
 *     Tag exists but content lacks `width=device-width`. Same
 *     symptom as missing tag for most pages — the browser uses
 *     the default 980px viewport width.
 *
 *   - viewport.zoom-disabled          strict
 *     Content includes `user-scalable=no`, `user-scalable=0`, or
 *     `maximum-scale` ≤ 1. Users with low vision cannot pinch-zoom.
 *     WCAG 1.4.4 (Resize text, AA) — explicitly forbidden by the
 *     spec since 2018. iOS Safari started ignoring it for the
 *     same reason; Android Chrome still honors it. Even if iOS
 *     bypasses it, the AUTHOR INTENT is hostile and the page
 *     should be fixed.
 *
 * Mirror: crates/crawler-detectors/src/viewport_meta.rs.
 */
import type { Page } from 'playwright';

export interface ViewportMetaFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface ViewportMetaSnapshot {
  pageUrl: string;
  /** True iff the page has at least one <meta name="viewport"> in head. */
  present: boolean;
  /** Raw `content` attribute of the first viewport meta, '' if absent. */
  content: string;
}

export async function captureViewportMetaSnapshot(
  page: Page,
): Promise<ViewportMetaSnapshot> {
  const pageUrl = page.url();
  const evalFn = `(() => {
    const m = document.querySelector('head meta[name="viewport"]');
    if (!m) return { present: false, content: '' };
    const c = m.getAttribute('content') || '';
    return { present: true, content: c };
  })()`;
  const result = (await page.evaluate(evalFn)) as {
    present: boolean;
    content: string;
  };
  return { pageUrl, present: result.present, content: result.content };
}

/**
 * Parse a viewport `content` string into a key→value map.
 *
 * `width=device-width, initial-scale=1, user-scalable=no` →
 *   { width: 'device-width', 'initial-scale': '1', 'user-scalable': 'no' }
 *
 * Defensive: tolerates extra whitespace, trailing commas, missing
 * value (key with no `=`), and case differences in keys (per spec
 * keys are case-insensitive). Values are trimmed but case-preserved
 * since some are numeric and case doesn't matter.
 */
export function parseViewportContent(content: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const segment of content.split(/[,;]/)) {
    const trimmed = segment.trim();
    if (!trimmed) continue;
    const eq = trimmed.indexOf('=');
    if (eq < 0) {
      out[trimmed.toLowerCase()] = '';
      continue;
    }
    const k = trimmed.slice(0, eq).trim().toLowerCase();
    const v = trimmed.slice(eq + 1).trim();
    if (k) out[k] = v;
  }
  return out;
}

export function detectViewportMetaIssues(
  snap: ViewportMetaSnapshot,
): ViewportMetaFinding[] {
  const out: ViewportMetaFinding[] = [];

  if (!snap.present) {
    out.push({
      severity: 'strict',
      kind: 'viewport.missing',
      detail: `No <meta name="viewport"> in the page <head>. Mobile browsers will render at the legacy 980px default viewport then scale down — text is unreadable, tap targets are unreachable, and WCAG 1.4.10 (Reflow, AA) fails. Add <meta name="viewport" content="width=device-width, initial-scale=1">.`,
      evidence: { present: false },
    });
    return out;
  }

  const parts = parseViewportContent(snap.content);

  if ((parts['width'] || '').toLowerCase() !== 'device-width') {
    out.push({
      severity: 'strict',
      kind: 'viewport.no-device-width',
      detail: `<meta name="viewport"> is present but content lacks 'width=device-width'. Mobile browsers fall back to the legacy 980px default. Got: '${snap.content}'. Fix: 'width=device-width, initial-scale=1'.`,
      evidence: { content: snap.content, parsed: parts },
    });
  }

  // Zoom-disabling forms.
  const userScalable = (parts['user-scalable'] || '').toLowerCase();
  const maxScaleStr = parts['maximum-scale'] || '';
  const maxScale = parseFloat(maxScaleStr);
  const zoomDisabled =
    userScalable === 'no' ||
    userScalable === '0' ||
    (Number.isFinite(maxScale) && maxScale <= 1);

  if (zoomDisabled) {
    out.push({
      severity: 'strict',
      kind: 'viewport.zoom-disabled',
      detail: `<meta name="viewport"> disables user pinch-zoom (user-scalable='${userScalable}', maximum-scale='${maxScaleStr}'). WCAG 1.4.4 (Resize text, AA): users must be able to scale text up to 200%. Hostile to low-vision users. Remove user-scalable=no and any maximum-scale ≤ 1.`,
      evidence: { content: snap.content, userScalable, maximumScale: maxScaleStr },
    });
  }

  return out;
}

export async function checkViewportMeta(
  page: Page,
): Promise<{ snapshot: ViewportMetaSnapshot; findings: ViewportMetaFinding[] }> {
  const snapshot = await captureViewportMetaSnapshot(page);
  const findings = detectViewportMetaIssues(snapshot);
  return { snapshot, findings };
}
