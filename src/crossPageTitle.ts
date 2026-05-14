/**
 * crossPageTitle.ts — AGGREGATES-LAYER detector. T76.
 *
 * Unlike per-page detectors (which run on each goto and produce
 * findings from one page's snapshot), aggregates detectors run
 * AFTER the journey completes and walk accumulated cross-page
 * state. The Crawler T76 catalog had this in its roadmap from
 * cycle 4 as the canonical first cross-page check: every page
 * in a journey having the SAME `<title>` is a real SEO + UX
 * defect (tabs, bookmarks, history, and search snippets are
 * indistinguishable).
 *
 * The detector pattern:
 *
 *   1. main.ts creates a CrossPageTitleAccumulator before the
 *      goto loop.
 *   2. Per-goto, main.ts calls `recordPageTitle(acc, url, title)`
 *      with whatever title the docTitle detector captured.
 *   3. After the loop completes, main.ts calls
 *      `detectCrossPageTitleDuplicates(acc)` once, gets a
 *      single set of findings, emits them as events with
 *      kind = 'cross-page-title-dup'.
 *
 * Findings:
 *
 *   - title.cross-page-dup       warn
 *     Two or more pages in the journey share the same trimmed
 *     `<title>` text. SEO suffers (Google often filters dup-title
 *     results) and users can't distinguish open tabs / bookmarks.
 *     Distinct titles per route are baseline SEO doctrine.
 *
 * Out of scope:
 *   * "Untitled" / generic titles — covered by the per-page
 *     `docTitle.generic` finding.
 *   * Single-page apps where every URL is the SAME canonical
 *     page (then title equality is correct). Caller should
 *     filter out non-distinct URLs before recording.
 *
 * No Rust mirror for v1: the aggregates layer is conceptually
 * different from the per-page-snapshot pattern that the
 * `crawler-detectors` Rust crate is shaped around. A future
 * `crawler-aggregates` crate would be the right home for this
 * if Rust parity becomes load-bearing for the chromiumoxide
 * port.
 */

export interface CrossPageTitleFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CrossPageTitleAccumulator {
  /** Ordered list of {url, title} as the journey visits each page. */
  entries: Array<{ url: string; title: string }>;
}

/** Construct an empty accumulator. */
export function newCrossPageTitleAccumulator(): CrossPageTitleAccumulator {
  return { entries: [] };
}

/**
 * Record a page's title. Caller should pass the TRIMMED title.
 * Empty titles are skipped (the per-page `title.empty` finding
 * covers that case better than reporting "every empty title is
 * a duplicate").
 */
export function recordPageTitle(
  acc: CrossPageTitleAccumulator,
  url: string,
  title: string,
): void {
  const trimmed = title.trim();
  if (!trimmed) return;
  acc.entries.push({ url, title: trimmed });
}

/**
 * Walk the accumulator. For each title that appears on > 1
 * distinct URL, emit one warn finding listing the URLs that
 * share it. Multiple duplicate-groups produce multiple
 * findings — keeps the per-group evidence focused.
 */
export function detectCrossPageTitleDuplicates(
  acc: CrossPageTitleAccumulator,
): CrossPageTitleFinding[] {
  // T76 cycle 59: dedupe URLs by pathname before grouping. The
  // SAME page visited multiple times with different query
  // strings (e.g. `?theme=dark`, `?theme=light` in the themes
  // journey) was being treated as "5 distinct URLs sharing a
  // title" — which is structurally wrong: it's ONE page audited
  // five times under different render modes. The dedupe key is
  // the URL's origin + pathname; the evidence preserves the
  // first representative URL with its query.
  const pathKey = (u: string): string => {
    try {
      const parsed = new URL(u);
      return parsed.origin + parsed.pathname;
    } catch {
      return u;
    }
  };

  // Group URLs by title. Use a Map for stable iteration.
  const groups = new Map<string, Map<string, string>>();
  for (const { url, title } of acc.entries) {
    let urls = groups.get(title);
    if (!urls) {
      urls = new Map<string, string>();
      groups.set(title, urls);
    }
    const k = pathKey(url);
    if (!urls.has(k)) urls.set(k, url);
  }

  const out: CrossPageTitleFinding[] = [];
  for (const [title, urls] of groups) {
    if (urls.size < 2) continue;
    const urlList = Array.from(urls.values());
    out.push({
      severity: 'warn',
      kind: 'title.cross-page-dup',
      detail: `${urls.size} distinct URL(s) share the same <title> '${title}'. SEO suffers (Google often filters duplicate-title results) and users can't tell open tabs / bookmarks / history entries apart. Set a unique, page-specific title per route. URLs: ${urlList.slice(0, 5).join('; ')}`,
      evidence: { title, urlCount: urls.size, urls: urlList },
    });
  }

  return out;
}
