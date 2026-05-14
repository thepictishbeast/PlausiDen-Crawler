/**
 * tapTargets.ts — touch target size detector. T76.
 *
 * WCAG 2.2 Success Criterion 2.5.8 (Target Size Minimum, AA):
 * the touch target for pointer inputs must be at least 24×24 CSS
 * pixels, except where:
 *   - the target is inline within a sentence
 *   - the target's function is achieved by an equivalent target on
 *     the same page that meets the size minimum
 *   - the target is essential to the information being conveyed
 *   - the size is determined by the user agent and not modified by
 *     the author
 *
 * WCAG 2.1 Success Criterion 2.5.5 (Target Size, AAA): 44×44 CSS px.
 *
 * We emit:
 *   - tap.too-small        strict   < 24×24, no inline-text exception
 *   - tap.below-recommended warn    24-44px, AAA recommends ≥44
 *
 * SUPERSOCIETY: this is one of the top three usability defects on
 * the modern mobile web. Tiny "Sign up" buttons, micro-icons, and
 * 12px close-X glyphs frustrate every thumb-driven user. Catching
 * them at audit time means the dev fixes it before users do.
 */
import type { Page } from 'playwright';

export interface TapTargetFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedTapTarget {
  selector: string;
  tag: string;
  role: string;
  width: number;
  height: number;
  inline: boolean;
  accessibleName: string;
}

export interface TapTargetsSnapshot {
  pageUrl: string;
  viewportWidth: number;
  viewportHeight: number;
  targets: CapturedTapTarget[];
}

/**
 * WCAG 2.2 AA minimum: 24×24 CSS px.
 * BUG ASSUMPTION: a target ≥ this size is almost certainly fine for
 * pointer accuracy. Anything below is unambiguously too small.
 */
const STRICT_MIN_PX = 24;

/**
 * WCAG 2.1 AAA recommendation: 44×44 CSS px (also Apple HIG, also
 * Material Design's recommended touch target).
 */
const RECOMMENDED_MIN_PX = 44;

export async function captureTapTargetsSnapshot(
  page: Page,
): Promise<TapTargetsSnapshot> {
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

    const isVisible = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) return false;
      return true;
    };

    /**
     * Detect the WCAG 2.5.8 'inline within a sentence' exception.
     * If the element is an inline link nested inside a paragraph or
     * sentence, skip it — author can't easily make every word in
     * a sentence 24×24 without breaking text layout.
     */
    const isInlineInSentence = function(el) {
      const tag = el.tagName.toLowerCase();
      if (tag !== 'a') return false;
      const cs = window.getComputedStyle(el);
      const display = cs.display;
      if (display !== 'inline' && display !== 'inline-block') return false;
      // Walk up — if the nearest block ancestor is a paragraph,
      // li, td, blockquote, dd, dt, h1-h6, or generic text-bearing
      // wrapper, treat as inline-in-sentence.
      const blockTags = ['p', 'li', 'td', 'th', 'blockquote', 'dd', 'dt',
                         'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'figcaption',
                         'caption', 'cite', 'em', 'span'];
      let parent = el.parentElement;
      let hops = 0;
      while (parent && hops < 4) {
        const ptag = parent.tagName.toLowerCase();
        if (blockTags.indexOf(ptag) >= 0) {
          // Is there sibling text? If the link is one of several
          // text nodes, it's mid-sentence.
          const siblings = parent.childNodes;
          let textBefore = false;
          let textAfter = false;
          let foundEl = false;
          for (let i = 0; i < siblings.length; i++) {
            const n = siblings[i];
            if (n === el) { foundEl = true; continue; }
            if (n.nodeType === 3 && (n.textContent || '').trim().length > 0) {
              if (foundEl) textAfter = true;
              else textBefore = true;
            }
          }
          if (textBefore || textAfter) return true;
        }
        parent = parent.parentElement;
        hops += 1;
      }
      return false;
    };

    const accessibleName = function(el) {
      const aria = el.getAttribute('aria-label');
      if (aria && aria.trim()) return aria.trim();
      const text = (el.textContent || '').trim();
      if (text) return text.slice(0, 60);
      const title = el.getAttribute('title');
      if (title && title.trim()) return title.trim().slice(0, 60);
      return '';
    };

    /**
     * Selector for plausibly-clickable targets. We deliberately
     * don't include every [tabindex>=0] node (too many false
     * positives from focusable wrappers). We DO include role-based
     * widgets since they're often custom buttons.
     */
    const sel = [
      'a[href]',
      'button',
      'input[type=button]',
      'input[type=submit]',
      'input[type=reset]',
      'input[type=checkbox]',
      'input[type=radio]',
      'input[type=image]',
      'input[type=file]',
      'select',
      'summary',
      '[role=button]',
      '[role=link]',
      '[role=checkbox]',
      '[role=radio]',
      '[role=menuitem]',
      '[role=tab]',
      '[role=switch]',
      '[onclick]',
    ].join(',');

    const out = [];
    const els = document.querySelectorAll(sel);
    for (let i = 0; i < els.length; i++) {
      const el = els[i];
      if (!isVisible(el)) continue;
      // Project-wide opt-out doctrine: data-tap="compact" marks a
      // hover-driven desktop control where the author has accepted
      // the size trade-off. Mirrors uiOverflow.ts so a single
      // attribute suppresses both detectors on the same element.
      if (el.getAttribute('data-tap') === 'compact') continue;
      const r = el.getBoundingClientRect();
      out.push({
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase(),
        role: el.getAttribute('role') || '',
        width: Math.round(r.width),
        height: Math.round(r.height),
        inline: isInlineInSentence(el),
        accessibleName: accessibleName(el),
      });
    }

    return {
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
      targets: out,
    };
  })()`;

  const result = (await page.evaluate(evalFn)) as {
    viewportWidth: number;
    viewportHeight: number;
    targets: CapturedTapTarget[];
  };
  return {
    pageUrl,
    viewportWidth: result.viewportWidth,
    viewportHeight: result.viewportHeight,
    targets: result.targets,
  };
}

export function detectTapTargetIssues(
  snap: TapTargetsSnapshot,
): TapTargetFinding[] {
  const tooSmall: CapturedTapTarget[] = [];
  const belowRecommended: CapturedTapTarget[] = [];

  for (const t of snap.targets) {
    // Inline-in-sentence link → WCAG exception applies, skip.
    if (t.inline) continue;
    // 0×0 = not actually rendered (eg. transparent overlay we
    // missed). Skip — captured = visible heuristic already
    // filtered display:none / visibility:hidden.
    if (t.width === 0 || t.height === 0) continue;

    const minDim = Math.min(t.width, t.height);
    if (minDim < STRICT_MIN_PX) {
      tooSmall.push(t);
    } else if (minDim < RECOMMENDED_MIN_PX) {
      belowRecommended.push(t);
    }
  }

  const out: TapTargetFinding[] = [];

  if (tooSmall.length > 0) {
    const examples = tooSmall.slice(0, 5).map((t) => {
      const name = t.accessibleName || '(no name)';
      return `${t.selector} ${t.width}×${t.height}px — '${name}'`;
    });
    out.push({
      severity: 'strict',
      kind: 'tap.too-small',
      detail: `${tooSmall.length} interactive target(s) are smaller than 24×24 CSS pixels. WCAG 2.2 SC 2.5.8 (Target Size Minimum, AA): touch targets must be ≥24×24 unless inline within a sentence or the function is duplicated by a larger target. Mobile / touchscreen users will misfire. Examples: ${examples.join('; ')}`,
      evidence: {
        count: tooSmall.length,
        examples,
        viewport: `${snap.viewportWidth}×${snap.viewportHeight}`,
      },
    });
  }

  if (belowRecommended.length > 0) {
    const examples = belowRecommended.slice(0, 5).map((t) => {
      const name = t.accessibleName || '(no name)';
      return `${t.selector} ${t.width}×${t.height}px — '${name}'`;
    });
    out.push({
      severity: 'warn',
      kind: 'tap.below-recommended',
      detail: `${belowRecommended.length} interactive target(s) are 24-43px on the smallest dimension. WCAG 2.1 SC 2.5.5 (AAA), Apple HIG, and Material Design all recommend ≥44×44. Acceptable for AA but increases mis-tap rate on touchscreens. Examples: ${examples.join('; ')}`,
      evidence: {
        count: belowRecommended.length,
        examples,
        viewport: `${snap.viewportWidth}×${snap.viewportHeight}`,
      },
    });
  }

  return out;
}

export async function checkTapTargets(
  page: Page,
): Promise<{ snapshot: TapTargetsSnapshot; findings: TapTargetFinding[] }> {
  const snapshot = await captureTapTargetsSnapshot(page);
  const findings = detectTapTargetIssues(snapshot);
  return { snapshot, findings };
}
