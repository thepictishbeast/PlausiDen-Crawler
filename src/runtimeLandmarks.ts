/**
 * runtimeLandmarks.ts — landmark uniqueness + nesting detector. T105 (TS port).
 *
 * WCAG 1.3.1 + ARIA 1.2 require landmarks to be unique per-page
 * and not nested. This detector catches:
 *
 *   - landmarks.no-main           strict   document has no <main>
 *   - landmarks.multiple-main     strict   document has >1 <main>
 *   - landmarks.multiple-banner   strict   >1 top-level banner
 *                                          (header outside article/section,
 *                                          or [role=banner])
 *   - landmarks.multiple-contentinfo strict same idea for footer
 *   - landmarks.nested-same-role  strict   <main><main>, <nav><nav>, etc.
 *
 * Multiple <nav> WITH distinct aria-labels is the correct ARIA
 * pattern (primary + footer + breadcrumb), so nav uniqueness is
 * NOT enforced; this matches the Rust impl.
 *
 * Mirrors crates/crawler-detectors/src/runtime_landmarks.rs —
 * keep them in sync. Both must produce the same kind+severity
 * for the same snapshot input.
 */
import type { Page } from 'playwright';

export interface RuntimeLandmarksFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface NestedLandmark {
  role: string;
  outer: string;
  inner: string;
}

export interface RuntimeLandmarksSnapshot {
  pageUrl: string;
  mainCount: number;
  bannerCount: number;
  contentinfoCount: number;
  navigationCount: number;
  complementaryCount: number;
  nestedSameRole: NestedLandmark[];
}

export async function captureRuntimeLandmarksSnapshot(
  page: Page,
): Promise<RuntimeLandmarksSnapshot> {
  const pageUrl = page.url();
  const evalFn = `(() => {
    const isLandmark = function(el, role) {
      const tag = el.tagName.toLowerCase();
      if (role === 'main') {
        return tag === 'main' || el.getAttribute('role') === 'main';
      }
      if (role === 'banner') {
        if (el.getAttribute('role') === 'banner') return true;
        if (tag !== 'header') return false;
        let p = el.parentElement;
        while (p && p !== document.body) {
          const pt = p.tagName.toLowerCase();
          if (pt === 'article' || pt === 'section' || pt === 'aside' || pt === 'nav') return false;
          p = p.parentElement;
        }
        return true;
      }
      if (role === 'contentinfo') {
        if (el.getAttribute('role') === 'contentinfo') return true;
        if (tag !== 'footer') return false;
        let p = el.parentElement;
        while (p && p !== document.body) {
          const pt = p.tagName.toLowerCase();
          if (pt === 'article' || pt === 'section' || pt === 'aside' || pt === 'nav') return false;
          p = p.parentElement;
        }
        return true;
      }
      if (role === 'navigation') {
        return tag === 'nav' || el.getAttribute('role') === 'navigation';
      }
      if (role === 'complementary') {
        return tag === 'aside' || el.getAttribute('role') === 'complementary';
      }
      return false;
    };

    const collect = function(role) {
      const out = [];
      const all = document.body ? document.body.querySelectorAll('*') : [];
      for (let i = 0; i < all.length; i++) {
        const el = all[i];
        if (isLandmark(el, role)) out.push(el);
      }
      return out;
    };

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

    const nestedSameRole = [];
    const checkNesting = function(role) {
      const els = collect(role);
      for (let i = 0; i < els.length; i++) {
        for (let j = 0; j < els.length; j++) {
          if (i === j) continue;
          if (els[i].contains(els[j])) {
            nestedSameRole.push({ role: role, outer: selectorOf(els[i]), inner: selectorOf(els[j]) });
          }
        }
      }
    };
    for (const r of ['main', 'banner', 'contentinfo', 'navigation', 'complementary']) {
      checkNesting(r);
    }

    return {
      mainCount: collect('main').length,
      bannerCount: collect('banner').length,
      contentinfoCount: collect('contentinfo').length,
      navigationCount: collect('navigation').length,
      complementaryCount: collect('complementary').length,
      nestedSameRole: nestedSameRole,
    };
  })()`;

  const result = (await page.evaluate(evalFn)) as Omit<RuntimeLandmarksSnapshot, 'pageUrl'>;
  return { pageUrl, ...result };
}

export function detectRuntimeLandmarksIssues(
  snap: RuntimeLandmarksSnapshot,
): RuntimeLandmarksFinding[] {
  const out: RuntimeLandmarksFinding[] = [];

  if (snap.mainCount === 0) {
    out.push({
      severity: 'strict',
      kind: 'landmarks.no-main',
      detail:
        'Document has no <main> landmark. Screen-reader users cannot jump to primary content with the main-landmark shortcut.',
      evidence: { mainCount: 0 },
    });
  } else if (snap.mainCount > 1) {
    out.push({
      severity: 'strict',
      kind: 'landmarks.multiple-main',
      detail: `Document has ${snap.mainCount} <main> elements; should have exactly 1.`,
      evidence: { mainCount: snap.mainCount },
    });
  }

  if (snap.bannerCount > 1) {
    out.push({
      severity: 'strict',
      kind: 'landmarks.multiple-banner',
      detail: `Document has ${snap.bannerCount} top-level banner landmarks (header outside article/section, or [role=banner]); should have at most 1.`,
      evidence: { bannerCount: snap.bannerCount },
    });
  }
  if (snap.contentinfoCount > 1) {
    out.push({
      severity: 'strict',
      kind: 'landmarks.multiple-contentinfo',
      detail: `Document has ${snap.contentinfoCount} top-level contentinfo landmarks; should have at most 1.`,
      evidence: { contentinfoCount: snap.contentinfoCount },
    });
  }

  for (const nest of snap.nestedSameRole) {
    out.push({
      severity: 'strict',
      kind: 'landmarks.nested-same-role',
      detail: `${nest.role} landmark nested inside another ${nest.role} landmark: outer=${nest.outer}, inner=${nest.inner}`,
      evidence: { role: nest.role, outer: nest.outer, inner: nest.inner },
    });
  }

  return out;
}

export async function checkRuntimeLandmarks(
  page: Page,
): Promise<{ snapshot: RuntimeLandmarksSnapshot; findings: RuntimeLandmarksFinding[] }> {
  const snapshot = await captureRuntimeLandmarksSnapshot(page);
  const findings = detectRuntimeLandmarksIssues(snapshot);
  return { snapshot, findings };
}
