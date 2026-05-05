/**
 * placeholderText.ts — sentinel-text detector. T16 (Crawler).
 *
 * Catches paste-a-template-and-forgot signals in the rendered DOM:
 * developer copy-deck markers (TODO / FIXME / XXX / TBD), Lorem
 * ipsum filler, and obvious "coming soon" / "delete me" / "sample
 * text" strings that ship a placeholder to production.
 *
 * Why this exists: the loop protocol fires when the crawler reports
 * 0 findings across all axes — the assumption is the crawler is
 * blind, not that the codebase is perfect. This axis is cheap to
 * run, generic enough to catch real shipping mistakes across many
 * codebases, and stays silent on disciplined content.
 *
 *   - placeholder.dev-marker     strict   visible TODO/FIXME/XXX/TBD
 *                                         in body text. These are
 *                                         developer-only annotations
 *                                         that should never reach
 *                                         a user.
 *   - placeholder.lorem-ipsum    strict   visible Lorem ipsum filler.
 *                                         Forgot to write the copy.
 *   - placeholder.template       strict   "delete me" / "replace this"
 *                                         / "sample text" / "your
 *                                         text here" — explicit
 *                                         template instructions that
 *                                         leaked.
 *   - placeholder.coming-soon    warn     "coming soon" / "TBD" /
 *                                         "tbd". Acceptable on a
 *                                         marketing page when the
 *                                         intent is explicit, but
 *                                         worth a flag.
 *
 * Operates on rendered textContent (post-CSS, post-JS) — comments,
 * <script>, <style>, [aria-hidden=true], and elements with
 * display:none / visibility:hidden / opacity:0 are skipped.
 *
 * REGRESSION-GUARD: regex MUST use word boundaries (\b) on
 * three-letter codes (TODO/FIXME/XXX/TBD) so they don't false-
 * positive on words like "Toolkit", "FixMeUp", or random
 * substrings. Lorem-ipsum match is case-insensitive but requires
 * the literal phrase, not just "lorem" alone (which is a real
 * proper noun).
 */
import type { Page } from 'playwright';

export interface PlaceholderTextFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface PlaceholderHit {
  /** CSS-ish path of the element whose text matched. */
  selector: string;
  /** Pattern category that matched. */
  category: 'dev-marker' | 'lorem-ipsum' | 'template' | 'coming-soon';
  /** Exact substring that matched, capped at 80 chars. */
  match: string;
  /** Surrounding text for operator context, capped at 160 chars. */
  context: string;
}

export interface PlaceholderTextSnapshot {
  pageUrl: string;
  hits: PlaceholderHit[];
  /** Total visible text-node characters scanned — useful for noise floor. */
  scannedChars: number;
}

export async function capturePlaceholderTextSnapshot(
  page: Page,
): Promise<PlaceholderTextSnapshot> {
  const pageUrl = page.url();

  // String-eval, consistent with linkText / runtimeFocus / runtimeImages.
  // Patterns live INSIDE the eval'd string so they execute in the
  // browser context — keep them in sync with the categoriseHit() reducer
  // below if you change one without the other.
  //
  // Word-boundary on dev-markers: `\bTODO\b` matches "TODO:" but
  // not "Antodora" (random substring). Letter-only word boundaries
  // are reliable in JS regex.
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

    const isHidden = function(el) {
      if (!el || el.nodeType !== 1) return false;
      if (el.getAttribute && el.getAttribute('aria-hidden') === 'true') return true;
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return true;
      return false;
    };

    const ancestorHidden = function(node) {
      let cur = node;
      while (cur && cur !== document.body) {
        if (cur.nodeType === 1 && isHidden(cur)) return true;
        cur = cur.parentElement;
      }
      return false;
    };

    // Categorised patterns. Order matters: more-specific patterns
    // (e.g. lorem ipsum) are tested before generic ones so the
    // category reflects the strongest signal.
    const patterns = [
      { category: 'lorem-ipsum',  re: /lorem ipsum/i },
      { category: 'template',     re: /\\b(delete me|remove me|replace this|sample text|placeholder text|your text here|insert .{1,20} here)\\b/i },
      { category: 'dev-marker',   re: /\\b(TODO|FIXME|XXX|HACK)\\b/ },
      { category: 'coming-soon',  re: /\\b(coming soon|tbd|to be (?:announced|determined))\\b/i },
    ];

    const hits = [];
    let scannedChars = 0;
    const walker = document.createTreeWalker(
      document.body,
      NodeFilter.SHOW_TEXT,
      {
        acceptNode: function(node) {
          // Reject script/style/noscript text outright.
          const p = node.parentElement;
          if (!p) return NodeFilter.FILTER_REJECT;
          const tag = p.tagName;
          if (tag === 'SCRIPT' || tag === 'STYLE' || tag === 'NOSCRIPT' || tag === 'TEMPLATE') {
            return NodeFilter.FILTER_REJECT;
          }
          // Skip the forge overlay — it's debug UI, not site content.
          if (p.closest && p.closest('.forge-overlay, .loom-skip-link, [data-forge-overlay]')) {
            return NodeFilter.FILTER_REJECT;
          }
          if (ancestorHidden(p)) return NodeFilter.FILTER_REJECT;
          return NodeFilter.FILTER_ACCEPT;
        }
      }
    );

    let node;
    while ((node = walker.nextNode())) {
      const raw = (node.nodeValue || '').trim();
      if (!raw) continue;
      scannedChars += raw.length;
      for (const pat of patterns) {
        const m = raw.match(pat.re);
        if (!m) continue;
        const matchText = (m[0] || '').slice(0, 80);
        const idx = m.index !== undefined ? m.index : raw.indexOf(m[0]);
        const start = Math.max(0, idx - 40);
        const end = Math.min(raw.length, idx + matchText.length + 40);
        const ctx = raw.slice(start, end);
        hits.push({
          selector: selectorOf(node.parentElement),
          category: pat.category,
          match: matchText,
          context: ctx,
        });
        break; // one category per text node; stop on first hit
      }
    }
    return { hits: hits, scannedChars: scannedChars };
  })()`;

  const result = (await page.evaluate(evalFn)) as {
    hits: PlaceholderHit[];
    scannedChars: number;
  };
  return { pageUrl, hits: result.hits, scannedChars: result.scannedChars };
}

const STRICT_CATEGORIES = new Set<PlaceholderHit['category']>([
  'lorem-ipsum',
  'template',
  'dev-marker',
]);

export function detectPlaceholderTextIssues(
  snap: PlaceholderTextSnapshot,
): PlaceholderTextFinding[] {
  const out: PlaceholderTextFinding[] = [];
  // Bucket by category so we emit one finding per category per page,
  // not one per hit (avoids noise spam if a single typo repeats).
  const buckets = new Map<PlaceholderHit['category'], PlaceholderHit[]>();
  for (const h of snap.hits) {
    if (!buckets.has(h.category)) buckets.set(h.category, []);
    buckets.get(h.category)!.push(h);
  }

  for (const [category, hits] of buckets) {
    const examples = hits.slice(0, 5).map((h) => `${h.selector} → "${h.match}"`);
    const severity: 'strict' | 'warn' = STRICT_CATEGORIES.has(category)
      ? 'strict'
      : 'warn';
    const kind = `placeholder.${category}`;
    let description = '';
    if (category === 'lorem-ipsum') {
      description =
        'Lorem ipsum filler text reached the rendered DOM. Replace with real copy before shipping.';
    } else if (category === 'template') {
      description =
        'Template instructions ("delete me", "sample text", "your text here", etc.) leaked into the rendered DOM.';
    } else if (category === 'dev-marker') {
      description =
        'Developer-only marker (TODO / FIXME / XXX / HACK) is visible to end users.';
    } else if (category === 'coming-soon') {
      description =
        '"Coming soon" / "TBD" placeholder copy in the rendered DOM. Acceptable when intentional, but flag for review.';
    }
    out.push({
      severity,
      kind,
      detail: `${hits.length} hit(s): ${description} Examples: ${examples.join('; ')}`,
      evidence: { count: hits.length, examples: hits.slice(0, 5) },
    });
  }

  return out;
}

export async function checkPlaceholderText(
  page: Page,
): Promise<{
  snapshot: PlaceholderTextSnapshot;
  findings: PlaceholderTextFinding[];
}> {
  const snapshot = await capturePlaceholderTextSnapshot(page);
  const findings = detectPlaceholderTextIssues(snapshot);
  return { snapshot, findings };
}
