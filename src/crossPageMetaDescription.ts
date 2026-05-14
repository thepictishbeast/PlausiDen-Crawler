/**
 * crossPageMetaDescription.ts — AGGREGATES-LAYER detector. T76.
 *
 * Sister to `crossPageTitle.ts`: walks accumulated cross-page
 * state to surface defects no single-page snapshot can see.
 * Where crossPageTitle catches duplicate `<title>` across a
 * journey, this catches duplicate `<meta name="description">`.
 *
 * Real-world impact: Google explicitly filters near-duplicate
 * meta descriptions in search results — the affected pages are
 * effectively invisible. Social-share previews collapse into a
 * single card if every page advertises the same blurb. The
 * baseline SEO doctrine is one description per route.
 *
 * Findings:
 *
 *   - meta-description.cross-page-dup     warn
 *     Two or more pages in the journey share the same trimmed
 *     `<meta name="description">` content.
 *
 * Out of scope:
 *   * Pages with NO description — covered by per-page
 *     `meta-description.missing` finding.
 *   * Empty descriptions — covered by `meta-description.empty`.
 *
 * No Rust mirror in v1 (same rationale as crossPageTitle).
 *
 * Pattern reuse: this is the SECOND aggregates-layer detector.
 * The shape is intentionally identical to crossPageTitle so
 * future ones can copy this template — accumulator + record +
 * detect-once-after-loop. If a third aggregates detector lands,
 * extract a generic `dupGroupDetector(accumulator, kind, label)`
 * helper.
 */

export interface CrossPageMetaDescriptionFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CrossPageMetaDescriptionAccumulator {
  /** Ordered list of {url, description} as the journey visits each page. */
  entries: Array<{ url: string; description: string }>;
}

/** Construct an empty accumulator. */
export function newCrossPageMetaDescriptionAccumulator():
  CrossPageMetaDescriptionAccumulator {
  return { entries: [] };
}

/**
 * Record a page's description. Caller passes the RAW description
 * (this function trims). Empty descriptions are skipped — the
 * per-page meta-description.empty / meta-description.missing
 * findings handle that case better than reporting "every empty
 * description is a duplicate".
 */
export function recordPageMetaDescription(
  acc: CrossPageMetaDescriptionAccumulator,
  url: string,
  description: string,
): void {
  const trimmed = description.trim();
  if (!trimmed) return;
  acc.entries.push({ url, description: trimmed });
}

/**
 * Walk the accumulator. For each description that appears on > 1
 * distinct URL, emit one warn finding listing the URLs that
 * share it.
 */
export function detectCrossPageMetaDescriptionDuplicates(
  acc: CrossPageMetaDescriptionAccumulator,
): CrossPageMetaDescriptionFinding[] {
  // T76 cycle 59: dedupe by URL pathname so journeys that revisit
  // the same path with different query strings (e.g. the
  // themes journey hitting `/?theme=dark`, `/?theme=light`, …)
  // don't get treated as distinct pages. See crossPageTitle.ts
  // for the matching fix.
  const pathKey = (u: string): string => {
    try {
      const parsed = new URL(u);
      return parsed.origin + parsed.pathname;
    } catch {
      return u;
    }
  };

  const groups = new Map<string, Map<string, string>>();
  for (const { url, description } of acc.entries) {
    let urls = groups.get(description);
    if (!urls) {
      urls = new Map<string, string>();
      groups.set(description, urls);
    }
    const k = pathKey(url);
    if (!urls.has(k)) urls.set(k, url);
  }

  const out: CrossPageMetaDescriptionFinding[] = [];
  for (const [description, urls] of groups) {
    if (urls.size < 2) continue;
    const urlList = Array.from(urls.values());
    // Truncate the description in the detail for readability —
    // these can be 150+ chars per the metaDescription floor.
    const preview = description.length > 80
      ? `${description.slice(0, 80)}…`
      : description;
    out.push({
      severity: 'warn',
      kind: 'meta-description.cross-page-dup',
      detail: `${urls.size} distinct URL(s) share the same <meta name="description"> content '${preview}'. Google filters near-duplicate descriptions in search results — affected pages become effectively invisible. Social-share preview cards collapse. Set a unique, page-specific description per route. URLs: ${urlList.slice(0, 5).join('; ')}`,
      evidence: { description, urlCount: urls.size, urls: urlList },
    });
  }

  return out;
}
