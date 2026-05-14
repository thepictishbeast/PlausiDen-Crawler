/**
 * skipLink.ts — "skip to content" link detector. T76.
 *
 * WCAG 2.4.1 (Bypass Blocks, Level A): provide a way for users to
 * bypass blocks of repeated content (header navs, sidebars). The
 * canonical pattern is a "skip to main content" link as the FIRST
 * focusable element on the page, hidden until focused, jumping to
 * the page's main content region.
 *
 * Without it, a keyboard / screen-reader user has to tab through
 * every nav link on every page just to reach the content.
 *
 * Findings:
 *
 *   - skip.missing                  warn
 *     No element matching the skip-link heuristic on the page.
 *     Warn (not strict) because a `<main>` landmark + heading
 *     navigation already provides a partial bypass for screen
 *     readers — the skip link is most valuable for sighted
 *     keyboard users. Pages with very short navs may legitimately
 *     omit it.
 *
 *   - skip.broken-target            strict
 *     Skip link exists but its href points at an id that doesn't
 *     exist on the page. The link is dead — pressing Enter does
 *     nothing. This is a worse defect than no skip link at all
 *     (the user expects something to happen).
 *
 *   - skip.not-first-focusable      warn
 *     Skip link exists but isn't the FIRST focusable element. Per
 *     WCAG technique G1, it must be reachable on the first tab
 *     press from the page load. If a logo link, language switcher,
 *     or other control comes before it in tab order, the skip
 *     mechanism is partially defeated.
 *
 *   - skip.permanently-hidden       strict
 *     Skip link is `display:none` / `visibility:hidden` even when
 *     focused — it can never become visible to keyboard users.
 *     Common bug: `display:none` instead of the canonical "visually
 *     hidden until focused" CSS pattern (clip-path / position
 *     absolute -10000px).
 *
 * Heuristic for what counts as a "skip link":
 *   * `<a href="#...">` with text matching `/skip/i` or
 *     `/jump.{0,4}content/i`, OR
 *   * any anchor with class containing 'skip' (case-insensitive),
 *     OR
 *   * the first focusable anchor on the page whose href targets
 *     an element with id="main" / role="main" / `<main>` element.
 *
 * Mirror: crates/crawler-detectors/src/skip_link.rs.
 */
import type { Page } from 'playwright';

export interface SkipLinkFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface SkipLinkSnapshot {
  pageUrl: string;
  /** True iff the page has at least one skip-link candidate. */
  found: boolean;
  /** href of the candidate if found, '' otherwise. */
  href: string;
  /** Visible text of the candidate (first 60 chars). */
  text: string;
  /** True iff the href fragment resolves to an element on the page. */
  targetExists: boolean;
  /** True iff the candidate was the FIRST focusable element. */
  firstFocusable: boolean;
  /** True iff the element is permanently display:none / visibility:hidden
   *  (heuristic: tested against unfocused computed style). */
  permanentlyHidden: boolean;
}

export async function captureSkipLinkSnapshot(
  page: Page,
): Promise<SkipLinkSnapshot> {
  const pageUrl = page.url();
  const evalFn = `(() => {
    const isSkipLinkText = function(el) {
      const t = (el.textContent || '').trim().toLowerCase();
      if (/skip/.test(t)) return true;
      if (/jump.{0,4}content/.test(t)) return true;
      const cls = (el.className || '').toString().toLowerCase();
      if (/skip/.test(cls)) return true;
      return false;
    };

    const isLandmarkTarget = function(el) {
      const id = el.id;
      if (!id) return false;
      // Anchor href targets either #main / #content / #main-content,
      // OR an element that's a <main>, role=main, or has id main/content.
      const ref = document.getElementById(id);
      if (!ref) return false;
      if (ref.tagName === 'MAIN') return true;
      if (ref.getAttribute('role') === 'main') return true;
      if (/^main(-content)?$/i.test(id)) return true;
      if (/^content$/i.test(id)) return true;
      return false;
    };

    // First-focusable computation. Skip elements with negative
    // tabindex, disabled, or display:none.
    const focusableSel = 'a[href], button:not([disabled]), input:not([disabled]):not([type=hidden]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
    const allFocusable = Array.from(document.querySelectorAll(focusableSel))
      .filter(function(el) {
        const cs = window.getComputedStyle(el);
        if (cs.display === 'none' || cs.visibility === 'hidden') return false;
        return true;
      });
    const firstFoc = allFocusable[0] || null;

    // Look for skip-link candidates: anchor with href starting #
    // matching skip-text heuristics, OR an early anchor pointing at
    // a landmark target.
    const anchors = Array.from(document.querySelectorAll('a[href^="#"]'));
    let candidate = null;
    for (const a of anchors) {
      if (isSkipLinkText(a)) { candidate = a; break; }
    }
    if (!candidate) {
      // Fall back: first 3 anchors on the page that target a landmark.
      for (const a of anchors.slice(0, 3)) {
        const href = a.getAttribute('href') || '';
        if (href.length > 1) {
          const id = href.slice(1);
          const ref = document.getElementById(id);
          if (ref && (ref.tagName === 'MAIN' || ref.getAttribute('role') === 'main' || /^main(-content)?$/i.test(id) || /^content$/i.test(id))) {
            candidate = a;
            break;
          }
        }
      }
    }

    if (!candidate) {
      return {
        found: false,
        href: '',
        text: '',
        targetExists: false,
        firstFocusable: false,
        permanentlyHidden: false,
      };
    }

    const href = candidate.getAttribute('href') || '';
    const text = (candidate.textContent || '').trim().slice(0, 60);
    const targetId = href.startsWith('#') ? href.slice(1) : '';
    const targetExists = targetId.length > 0 && document.getElementById(targetId) !== null;
    const firstFocusable = candidate === firstFoc;
    const cs = window.getComputedStyle(candidate);
    // Permanently hidden = display:none or visibility:hidden in the
    // unfocused computed-style baseline. We cannot easily simulate
    // :focus in evaluate without driving the page; this catches the
    // most common bug (someone wrote display:none instead of the
    // canonical .sr-only / visually-hidden pattern).
    const permanentlyHidden = cs.display === 'none' || cs.visibility === 'hidden';

    return {
      found: true,
      href: href,
      text: text,
      targetExists: targetExists,
      firstFocusable: firstFocusable,
      permanentlyHidden: permanentlyHidden,
    };
  })()`;

  const result = (await page.evaluate(evalFn)) as Omit<SkipLinkSnapshot, 'pageUrl'>;
  return { pageUrl, ...result };
}

export function detectSkipLinkIssues(snap: SkipLinkSnapshot): SkipLinkFinding[] {
  const out: SkipLinkFinding[] = [];

  if (!snap.found) {
    out.push({
      severity: 'warn',
      kind: 'skip.missing',
      detail: `No "skip to content" link found on the page. Keyboard / screen-reader users must tab through every nav item to reach content. WCAG 2.4.1 (Bypass Blocks, A). Add an <a href="#main">Skip to main content</a> as the first focusable element, visually hidden until focused.`,
      evidence: { found: false },
    });
    return out;
  }

  if (snap.permanentlyHidden) {
    out.push({
      severity: 'strict',
      kind: 'skip.permanently-hidden',
      detail: `Skip link found ('${snap.text}' → ${snap.href}) but it has display:none or visibility:hidden — keyboard users can never focus it. Use the canonical "visually hidden until focused" pattern (clip + absolute positioning) instead.`,
      evidence: { href: snap.href, text: snap.text },
    });
  }

  if (!snap.targetExists) {
    out.push({
      severity: 'strict',
      kind: 'skip.broken-target',
      detail: `Skip link '${snap.text}' targets ${snap.href} but no element with that id exists on the page. The link is dead — pressing Enter does nothing. Add id="${snap.href.replace(/^#/, '')}" to your <main> element.`,
      evidence: { href: snap.href, text: snap.text },
    });
  }

  if (!snap.firstFocusable) {
    out.push({
      severity: 'warn',
      kind: 'skip.not-first-focusable',
      detail: `Skip link '${snap.text}' isn't the first focusable element on the page. WCAG technique G1: the bypass mechanism must be reachable on the very first tab press. Move it before any other focusable element.`,
      evidence: { href: snap.href, text: snap.text },
    });
  }

  return out;
}

export async function checkSkipLink(
  page: Page,
): Promise<{ snapshot: SkipLinkSnapshot; findings: SkipLinkFinding[] }> {
  const snapshot = await captureSkipLinkSnapshot(page);
  const findings = detectSkipLinkIssues(snapshot);
  return { snapshot, findings };
}
