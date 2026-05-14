/**
 * uiOverflow.ts — detect viewport-overflow bugs the way a real
 * visitor would experience them, at the configured viewport. T28.
 *
 * Operates strictly on the rendered DOM via page.evaluate() — never
 * reads project source files. Mirrors the cssHealth.ts pattern.
 *
 * Owner directive 2026-04 (recent): "make sure loom and cms are
 * strict about not letting content run off the screen." This is
 * the runtime-side complement to forge.sh's static checks.
 *
 * Detection heuristics:
 *
 *   PAGE-LEVEL OVERFLOW (strict):
 *     overflow.page-horizontal-scroll
 *       documentElement.scrollWidth > clientWidth + 2px tolerance.
 *       Catches horizontal scrollbars caused by oversize fixed-width
 *       elements.
 *
 *   ELEMENT-LEVEL OVERFLOW (strict):
 *     overflow.element-bleeds-viewport
 *       At least one positioned element has rect.right > viewport
 *       width + 4px AND has visible content. Excludes off-screen
 *       elements (hidden via transform/clip).
 *     overflow.text-clipped
 *       At least one element has scrollWidth > clientWidth + 2 AND
 *       overflow-x is not 'auto' or 'scroll' (so the user genuinely
 *       cannot see the content).
 *
 * 2026-05-14 (T76): tap-target detection MOVED OUT of this file
 * into src/tapTargets.ts. The new module is WCAG-conformant
 * (handles SC 2.5.8 inline-in-sentence exception, two severity
 * tiers for AA-min vs AAA-recommendation, wider selector set
 * including input[type=checkbox|radio|file|...], role=switch,
 * role=menuitem). The smallTapTargets array on the snapshot is
 * retained for one release as a transitional shape but the
 * detector no longer emits the 'overflow.tap-target-too-small'
 * finding — see tapTargets.ts for the canonical replacement.
 *
 * The detector returns UIOverflowFinding[]. Pure-function fingerprint
 * (snapshot + detect) so the unit tests can hand-craft DOM states.
 */
import type { Page } from 'playwright';

export interface UIOverflowFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface OffenderRect {
  selector: string;
  left: number;
  top: number;
  width: number;
  height: number;
  right: number;
  text: string;
}

export interface UIOverflowSnapshot {
  pageUrl: string;
  viewport: { width: number; height: number };
  documentScrollWidth: number;
  documentClientWidth: number;
  pageHasHorizontalScroll: boolean;
  bleedingElements: OffenderRect[];
  textClippedElements: OffenderRect[];
  smallTapTargets: OffenderRect[];
}

/**
 * Capture an overflow/tap-target snapshot of the current page state.
 *
 * Runs ONE page.evaluate() that walks the DOM and gathers all the
 * metrics — the round-trip cost is the dominant overhead, so the
 * detector economizes by collecting everything in one pass.
 */
export async function captureUIOverflowSnapshot(page: Page): Promise<UIOverflowSnapshot> {
  const pageUrl = page.url();

  // tsx/esbuild rewrites in-callback function expressions with
  // __name() wrappers that don't exist in the browser. Workaround:
  // pass the function as a string literal (page.evaluate(string))
  // — Playwright evaluates the string verbatim, no transform applied.
  // The string is plain JS (no TS), authored to match the
  // CapturedSnapshot return contract.
  const evalFn = `(() => {
    const docEl = document.documentElement;
    const docScrollWidth = docEl.scrollWidth;
    const docClientWidth = docEl.clientWidth;
    const vpW = window.innerWidth;
    const vpH = window.innerHeight;

    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const sameTag = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (sameTag.length > 1) {
            const idx = sameTag.indexOf(node) + 1;
            parts.unshift(tag + ':nth-of-type(' + idx + ')');
          } else { parts.unshift(tag); }
        } else { parts.unshift(tag); }
        node = parent;
        depth += 1;
      }
      return 'body > ' + parts.join(' > ');
    };

    const isVisible = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return false;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return false;
      if (rect.right < -100 || rect.bottom < -100) return false;
      return true;
    };

    const truncateText = function(s) { return (s || '').replace(/\\s+/g, ' ').trim().slice(0, 60); };

    const bleedingElements = [];
    const textClippedElements = [];
    const smallTapTargets = [];

    const all = document.querySelectorAll('body *');
    const tapSelectors = new Set(['button','a','input','select','textarea','summary']);

    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      if (!isVisible(el)) continue;
      const rect = el.getBoundingClientRect();
      const cs = window.getComputedStyle(el);

      if (rect.right > vpW + 4 && rect.width > 0 && rect.left >= -100) {
        const parent = el.parentElement;
        const parentOverflowX = parent ? window.getComputedStyle(parent).overflowX : 'visible';
        if (parentOverflowX !== 'auto' && parentOverflowX !== 'scroll' && parentOverflowX !== 'hidden') {
          bleedingElements.push({
            selector: selectorOf(el),
            left: Math.round(rect.left),
            top: Math.round(rect.top),
            width: Math.round(rect.width),
            height: Math.round(rect.height),
            right: Math.round(rect.right),
            text: truncateText(el.textContent || '')
          });
        }
      }

      // Skip native form controls — input/textarea have intrinsic
      // horizontal scroll for the value and that's not a clipping
      // bug. Also skip select since the dropdown handles overflow.
      const tagLower = el.tagName.toLowerCase();
      const isFormCtrl = tagLower === 'input' || tagLower === 'textarea' || tagLower === 'select';
      if (!isFormCtrl && el.scrollWidth > el.clientWidth + 2 && cs.overflowX !== 'auto' && cs.overflowX !== 'scroll' && cs.textOverflow !== 'ellipsis') {
        textClippedElements.push({
          selector: selectorOf(el),
          left: Math.round(rect.left),
          top: Math.round(rect.top),
          width: Math.round(rect.width),
          height: Math.round(rect.height),
          right: Math.round(rect.right),
          text: truncateText(el.textContent || '')
        });
      }

      const isTap = tapSelectors.has(el.tagName.toLowerCase()) || el.getAttribute('role') === 'button' || el.getAttribute('role') === 'link' || el.hasAttribute('onclick');
      if (isTap) {
        const inputType = el.type || '';
        if (inputType === 'hidden') continue;
        // Opt-out: data-tap="compact" is the doctrine'd escape hatch
        // for hover-driven desktop UIs (icon buttons, dev tools). The
        // detector honors it; a separate forge phase counts how many
        // exist so abuse becomes visible.
        if (el.getAttribute('data-tap') === 'compact') continue;
        if (rect.width < 44 || rect.height < 44) {
          smallTapTargets.push({
            selector: selectorOf(el),
            left: Math.round(rect.left),
            top: Math.round(rect.top),
            width: Math.round(rect.width),
            height: Math.round(rect.height),
            right: Math.round(rect.right),
            text: truncateText(el.textContent || el.getAttribute('aria-label') || '')
          });
        }
      }
    }

    return { vpW: vpW, vpH: vpH, docScrollWidth: docScrollWidth, docClientWidth: docClientWidth, bleedingElements: bleedingElements, textClippedElements: textClippedElements, smallTapTargets: smallTapTargets };
  })()`;
  const result = await page.evaluate(evalFn) as {
    vpW: number;
    vpH: number;
    docScrollWidth: number;
    docClientWidth: number;
    bleedingElements: OffenderRect[];
    textClippedElements: OffenderRect[];
    smallTapTargets: OffenderRect[];
  };

  return {
    pageUrl,
    viewport: { width: result.vpW, height: result.vpH },
    documentScrollWidth: result.docScrollWidth,
    documentClientWidth: result.docClientWidth,
    pageHasHorizontalScroll: result.docScrollWidth > result.docClientWidth + 2,
    bleedingElements: result.bleedingElements,
    textClippedElements: result.textClippedElements,
    smallTapTargets: result.smallTapTargets,
  };
}

/**
 * Apply detection heuristics to a snapshot. Pure function — no I/O.
 */
export function detectUIOverflowIssues(snap: UIOverflowSnapshot): UIOverflowFinding[] {
  const out: UIOverflowFinding[] = [];

  if (snap.pageHasHorizontalScroll) {
    out.push({
      severity: 'strict',
      kind: 'overflow.page-horizontal-scroll',
      detail: `Page produces horizontal scrollbar — documentElement scrollWidth ${snap.documentScrollWidth}px > clientWidth ${snap.documentClientWidth}px (delta ${snap.documentScrollWidth - snap.documentClientWidth}px).`,
      evidence: {
        viewport: snap.viewport,
        documentScrollWidth: snap.documentScrollWidth,
        documentClientWidth: snap.documentClientWidth,
        delta: snap.documentScrollWidth - snap.documentClientWidth,
        // Up to 5 most-likely culprits (the elements that bleed
        // furthest past the right edge).
        likelyCulprits: snap.bleedingElements
          .slice()
          .sort((a, b) => b.right - a.right)
          .slice(0, 5)
          .map((e) => ({ selector: e.selector, right: e.right, text: e.text })),
      },
    });
  }

  // Element-level overflow — collapse N similar offenders into one
  // finding so the report doesn't drown in 200 individual events.
  if (snap.bleedingElements.length > 0) {
    out.push({
      severity: 'strict',
      kind: 'overflow.element-bleeds-viewport',
      detail: `${snap.bleedingElements.length} element(s) extend past the viewport's right edge with no overflow-x scroll affordance.`,
      evidence: {
        viewport: snap.viewport,
        offenderCount: snap.bleedingElements.length,
        offenders: snap.bleedingElements.slice(0, 10),
      },
    });
  }

  if (snap.textClippedElements.length > 0) {
    out.push({
      severity: 'strict',
      kind: 'overflow.text-clipped',
      detail: `${snap.textClippedElements.length} element(s) have content wider than their container with no scroll affordance and no text-overflow:ellipsis.`,
      evidence: {
        viewport: snap.viewport,
        offenderCount: snap.textClippedElements.length,
        offenders: snap.textClippedElements.slice(0, 10),
      },
    });
  }

  // T76 2026-05-14: tap-target finding moved to tapTargets.ts.
  // Kept the snapshot field above so downstream tooling can still
  // serialize/deserialize the legacy shape; the new detector is
  // the single source of truth for tap-target findings.

  return out;
}

/**
 * Convenience wrapper.
 */
export async function checkUIOverflow(
  page: Page,
): Promise<{ snapshot: UIOverflowSnapshot; findings: UIOverflowFinding[] }> {
  const snapshot = await captureUIOverflowSnapshot(page);
  const findings = detectUIOverflowIssues(snapshot);
  return { snapshot, findings };
}
