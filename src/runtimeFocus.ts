/**
 * runtimeFocus.ts — runtime focus-visible detector. T79.
 *
 * WCAG 2.4.7 AA: every interactive element must have a visible
 * focus indicator. Static-axe checks rules; this checks the
 * COMPUTED :focus state actually differs from the un-focused
 * state — catches outline:0 with no border/box-shadow replacement.
 *
 * Walk every <button>, <a href>, <input>, <select>, <textarea>,
 * [role=button|link|menuitem|tab|switch], [tabindex]. For each:
 *   1. Snapshot outline-width, box-shadow, border-* before focus.
 *   2. Programmatically .focus() the element.
 *   3. Snapshot the same properties after focus.
 *   4. If at least ONE property changed visibly → pass.
 *      Otherwise → fail.
 *
 * Decorative changes (only color shift below ~5% perceptual delta)
 * also count as a fail since they're invisible to most low-vision
 * users. Out of scope for v1 — flagged as future work.
 */
import type { Page } from 'playwright';

export interface RuntimeFocusFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface FocusOffender {
  selector: string;
  tag: string;
  text: string;
  beforeOutline: string;
  afterOutline: string;
  beforeBoxShadow: string;
  afterBoxShadow: string;
  beforeBorderTop: string;
  afterBorderTop: string;
}

export interface RuntimeFocusSnapshot {
  pageUrl: string;
  viewport: { width: number; height: number };
  totalInteractive: number;
  totalChecked: number;
  invisibleFocus: FocusOffender[];
}

export async function captureRuntimeFocusSnapshot(page: Page): Promise<RuntimeFocusSnapshot> {
  const pageUrl = page.url();

  // String-eval (consistent with uiOverflow / runtimeImages).
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
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return false;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return false;
      return true;
    };

    const focusSignature = function(el) {
      const cs = window.getComputedStyle(el);
      return {
        outline: cs.outlineWidth + ' ' + cs.outlineStyle + ' ' + cs.outlineColor,
        boxShadow: cs.boxShadow,
        borderTop: cs.borderTopColor + ' ' + cs.borderTopWidth,
      };
    };

    const interactiveSelectors = [
      'button',
      'a[href]',
      'input:not([type="hidden"])',
      'select',
      'textarea',
      'summary',
      '[tabindex]:not([tabindex="-1"])',
      '[role="button"]',
      '[role="link"]',
      '[role="menuitem"]',
      '[role="tab"]',
      '[role="switch"]'
    ];
    const seen = new Set();
    const all = [];
    for (const sel of interactiveSelectors) {
      const els = document.querySelectorAll(sel);
      for (let i = 0; i < els.length; i++) {
        if (!seen.has(els[i])) { seen.add(els[i]); all.push(els[i]); }
      }
    }

    const invisibleFocus = [];
    let checked = 0;
    const previouslyFocused = document.activeElement;

    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      if (!isVisible(el)) continue;
      // Skip elements that have data-focus-skip="true" — explicit
      // opt-out for cases where focus is delegated to a child
      // (e.g. composite controls).
      if (el.getAttribute('data-focus-skip') === 'true') continue;

      const before = focusSignature(el);
      try {
        el.focus({ preventScroll: true });
      } catch (e) {
        continue; // Some elements throw on focus; skip them.
      }
      const after = focusSignature(el);
      checked += 1;

      // Compare. Any of these indicates a visible focus change:
      //  - outline-width grew (e.g. 0 → 2px)
      //  - outline-color changed
      //  - box-shadow gained content (was 'none', became something)
      //  - border-top changed (color or width)
      const outlineChanged = before.outline !== after.outline;
      const boxShadowChanged = before.boxShadow !== after.boxShadow && (before.boxShadow === 'none' || after.boxShadow !== before.boxShadow);
      const borderChanged = before.borderTop !== after.borderTop;

      if (!outlineChanged && !boxShadowChanged && !borderChanged) {
        invisibleFocus.push({
          selector: selectorOf(el),
          tag: el.tagName.toLowerCase(),
          text: (el.textContent || el.value || el.getAttribute('aria-label') || '').trim().slice(0, 40),
          beforeOutline: before.outline,
          afterOutline: after.outline,
          beforeBoxShadow: before.boxShadow,
          afterBoxShadow: after.boxShadow,
          beforeBorderTop: before.borderTop,
          afterBorderTop: after.borderTop
        });
      }
    }

    // Restore prior focus so we don't pollute the page state.
    try {
      if (previouslyFocused && previouslyFocused.focus) previouslyFocused.focus({ preventScroll: true });
    } catch (e) { /* best effort */ }

    return {
      vpW: window.innerWidth,
      vpH: window.innerHeight,
      totalInteractive: all.length,
      totalChecked: checked,
      invisibleFocus: invisibleFocus
    };
  })()`;

  const result = await page.evaluate(evalFn) as {
    vpW: number;
    vpH: number;
    totalInteractive: number;
    totalChecked: number;
    invisibleFocus: FocusOffender[];
  };

  return {
    pageUrl,
    viewport: { width: result.vpW, height: result.vpH },
    totalInteractive: result.totalInteractive,
    totalChecked: result.totalChecked,
    invisibleFocus: result.invisibleFocus,
  };
}

export function detectRuntimeFocusIssues(snap: RuntimeFocusSnapshot): RuntimeFocusFinding[] {
  const out: RuntimeFocusFinding[] = [];
  if (snap.invisibleFocus.length > 0) {
    out.push({
      severity: 'strict',
      kind: 'focus.invisible-indicator',
      detail: `${snap.invisibleFocus.length} interactive element(s) of ${snap.totalChecked} checked have no visible focus indicator (outline + box-shadow + border-top all unchanged on :focus). WCAG 2.4.7 AA.`,
      evidence: {
        offenderCount: snap.invisibleFocus.length,
        offenders: snap.invisibleFocus.slice(0, 10),
        totalChecked: snap.totalChecked,
        totalInteractive: snap.totalInteractive,
      },
    });
  }
  return out;
}

export async function checkRuntimeFocus(
  page: Page,
): Promise<{ snapshot: RuntimeFocusSnapshot; findings: RuntimeFocusFinding[] }> {
  const snapshot = await captureRuntimeFocusSnapshot(page);
  const findings = detectRuntimeFocusIssues(snapshot);
  return { snapshot, findings };
}
