/**
 * headingOrder.ts — heading hierarchy detector. T104 (TS port).
 *
 * WCAG 1.3.1 (Info and Relationships) requires that heading
 * structure be programmatically determinable AND meaningful.
 * Two violations this detector catches:
 *
 *   1. Wrong h1 count. A document has exactly ONE <h1>. Zero or
 *      multiple confuse screen-reader nav and SERP indexing.
 *   2. Level skipping. Headings should not jump levels (h2
 *      followed by h4 without h3 between). Skip indicates either
 *      missing intermediate structure or visual-weight misuse.
 *
 * What this DOES NOT enforce (out of scope for v1):
 *   - heading text quality / uniqueness
 *   - heading-to-content correspondence
 *   - h1 == document title
 *
 * Mirrors the Rust implementation at
 * crates/crawler-detectors/src/heading_order.rs — keep them in
 * sync. Both must produce the same kind+severity for the same
 * snapshot input.
 */
import type { Page } from 'playwright';

export interface HeadingOrderFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedHeading {
  level: number;
  text: string;
  selector: string;
}

export interface HeadingOrderSnapshot {
  pageUrl: string;
  headings: CapturedHeading[];
}

export async function captureHeadingOrderSnapshot(
  page: Page,
): Promise<HeadingOrderSnapshot> {
  const pageUrl = page.url();
  const evalFn = `(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const same = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (same.length > 1) parts.unshift(tag + ':nth-of-type(' + (same.indexOf(node) + 1) + ')');
          else parts.unshift(tag);
        } else parts.unshift(tag);
        node = parent;
        depth += 1;
      }
      return 'body > ' + parts.join(' > ');
    };

    const headings = [];
    const all = document.querySelectorAll('h1, h2, h3, h4, h5, h6');
    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      const level = parseInt(el.tagName.substring(1), 10);
      const text = (el.textContent || '').trim().slice(0, 80);
      headings.push({ level: level, text: text, selector: selectorOf(el) });
    }
    return { headings: headings };
  })()`;

  const result = (await page.evaluate(evalFn)) as { headings: CapturedHeading[] };
  return { pageUrl, headings: result.headings };
}

export function detectHeadingOrderIssues(
  snap: HeadingOrderSnapshot,
): HeadingOrderFinding[] {
  const out: HeadingOrderFinding[] = [];

  const h1Count = snap.headings.filter((h) => h.level === 1).length;
  if (h1Count === 0) {
    out.push({
      severity: 'strict',
      kind: 'headings.no-h1',
      detail:
        "Document has no <h1>. Screen-reader page nav and SERP both rely on a top-level heading; pages without one announce as 'untitled'.",
      evidence: { totalHeadings: snap.headings.length },
    });
  } else if (h1Count > 1) {
    out.push({
      severity: 'strict',
      kind: 'headings.multiple-h1',
      detail: `Document has ${h1Count} <h1> elements; should have exactly 1. Multiple top-level headings break document outline + screen-reader navigation.`,
      evidence: {
        h1Count,
        h1Texts: snap.headings.filter((h) => h.level === 1).map((h) => h.text),
      },
    });
  }

  let prevLevel: number | null = null;
  for (const h of snap.headings) {
    if (prevLevel !== null && h.level > prevLevel + 1) {
      out.push({
        severity: 'warn',
        kind: 'headings.level-skip',
        detail: `Heading skips from h${prevLevel} to h${h.level}: '${h.text}'. Insert the intermediate level(s) for accessible document outline.`,
        evidence: {
          fromLevel: prevLevel,
          toLevel: h.level,
          text: h.text,
          selector: h.selector,
        },
      });
    }
    prevLevel = h.level;
  }

  return out;
}

export async function checkHeadingOrder(
  page: Page,
): Promise<{ snapshot: HeadingOrderSnapshot; findings: HeadingOrderFinding[] }> {
  const snapshot = await captureHeadingOrderSnapshot(page);
  const findings = detectHeadingOrderIssues(snapshot);
  return { snapshot, findings };
}
