/**
 * corp.ts — Cross-Origin-Resource-Policy per-sub-resource
 * audit. T76 cycle 27.
 *
 * CORP is the third member of the cross-origin-isolation triad
 * (with COOP + COEP). Where COOP controls window.opener and
 * COEP controls which sub-resources the page is willing to
 * embed, CORP is set on the RESOURCE side to opt INTO being
 * embedded by cross-origin pages.
 *
 * Threat model: a page with `Cross-Origin-Embedder-Policy:
 * require-corp` REQUIRES every cross-origin sub-resource it
 * fetches to carry a CORP header that allows the embedding
 * origin. Without CORP, the resource is BLOCKED by the
 * browser. So CORP-less sub-resources are:
 *
 *   * STRICT defects on a require-corp page (broken loads).
 *   * WARN defects on any page (forward-compat gap — the
 *     moment the page adopts COEP=require-corp, this resource
 *     stops working).
 *
 * CORP is also a Spectre-class side-channel mitigation in its
 * own right: when set to `same-origin`, browsers refuse to
 * even FETCH the resource cross-origin, which prevents some
 * cache-timing attacks that observe whether the resource was
 * already cached.
 *
 * Acceptable values (W3C):
 *   * `same-origin` — most restrictive; resource only loadable
 *                     by same-origin pages.
 *   * `same-site`   — same eTLD+1.
 *   * `cross-origin` — anyone can embed (CDN-style).
 *
 * Findings:
 *
 *   - corp.cross-origin-resource-no-corp     (warn or strict)
 *     A cross-origin sub-resource the page fetched lacks a
 *     CORP header. Strict if the page itself sets COEP=
 *     require-corp (the resource will be blocked at load);
 *     warn otherwise.
 *
 *   - corp.cross-origin-resource-invalid     warn
 *     A cross-origin sub-resource has CORP set to a value
 *     not in the W3C-recognised set. Browsers may reject the
 *     resource entirely.
 *
 * Out of scope:
 *   * Same-origin sub-resources — CORP doesn't apply.
 *   * The page's OWN top-level CORP — separate concern (most
 *     top-level HTML pages legitimately don't set CORP). A
 *     future top-level CORP detector could surface
 *     `CORP: cross-origin` on a top-level HTML page where
 *     `same-origin` would be safer; not in this cycle.
 *   * Localhost (consistent with the response-header detector
 *     family).
 *   * data: / blob: / about: URLs (no transport).
 *
 * Architecture: this is the FIRST per-sub-resource detector.
 * It reads from a NEW capture path — `allResponseHeaders` Map
 * — populated by the response listener in main.ts for every
 * response (not just top-level navigation). The existing
 * response-header detectors keep using `topLevelResponseHeaders`.
 *
 * The CORP detector takes BOTH:
 *   * `pageUrl` — to determine same-origin vs cross-origin.
 *   * `pageCoepValue` — the page's own COEP header value, to
 *     decide whether missing CORP is strict or warn.
 *   * `allResponseHeaders` — Map of every sub-resource's
 *     headers.
 *
 * It does NOT use the existing `responseHeaderDetector` helper
 * because the helper's contract is "one snapshot per page
 * navigation, classify into findings". CORP's contract is
 * "walk every sub-resource, classify each, aggregate". A new
 * shape — kept bespoke for now per the cycle-22 verdict on
 * heterogeneous classifier shapes.
 */

export interface CorpFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedSubResource {
  url: string;
  origin: string;
  /** Lowercased + trimmed CORP value, or null if absent. */
  corp: string | null;
}

export interface CorpSnapshot {
  pageUrl: string;
  pageOrigin: string;
  pageIsLocalhost: boolean;
  /** True iff the page's own COEP is 'require-corp'. */
  pageRequiresCorp: boolean;
  /** Cross-origin sub-resources only — same-origin filtered out. */
  crossOriginSubResources: CapturedSubResource[];
}

const ACCEPTABLE_CORP: ReadonlySet<string> = new Set([
  'same-origin',
  'same-site',
  'cross-origin',
]);

function isLocalhost(url: string): boolean {
  try {
    const u = new URL(url);
    const h = u.hostname;
    return h === 'localhost' || h === '127.0.0.1' || h === '::1' || h.endsWith('.localhost');
  } catch {
    return false;
  }
}

function originOf(url: string): string {
  try {
    const u = new URL(url);
    return `${u.protocol}//${u.host}`;
  } catch {
    return '';
  }
}

function isAuditableScheme(url: string): boolean {
  return url.startsWith('http://') || url.startsWith('https://');
}

function getHeaderValue(
  headers: Record<string, string> | undefined,
  name: string,
): string | null {
  if (!headers) return null;
  for (const [k, v] of Object.entries(headers)) {
    if (k.toLowerCase() === name) return v;
  }
  return null;
}

export function buildCorpSnapshot(
  pageUrl: string,
  pageHeaders: Record<string, string> | undefined,
  allHeaders: Map<string, Record<string, string>>,
): CorpSnapshot {
  const pageOrigin = originOf(pageUrl);
  const pageIsLocalhost = isLocalhost(pageUrl);
  const coep = getHeaderValue(pageHeaders, 'cross-origin-embedder-policy');
  const pageRequiresCorp = coep !== null && coep.trim().toLowerCase() === 'require-corp';

  const crossOrigin: CapturedSubResource[] = [];
  for (const [url, headers] of allHeaders.entries()) {
    if (!isAuditableScheme(url)) continue;
    const ro = originOf(url);
    if (!ro || ro === pageOrigin) continue;
    const corp = getHeaderValue(headers, 'cross-origin-resource-policy');
    crossOrigin.push({
      url,
      origin: ro,
      corp: corp === null ? null : corp.trim().toLowerCase(),
    });
  }

  return {
    pageUrl,
    pageOrigin,
    pageIsLocalhost,
    pageRequiresCorp,
    crossOriginSubResources: crossOrigin,
  };
}

export function detectCorpIssues(snap: CorpSnapshot): CorpFinding[] {
  if (snap.pageIsLocalhost) return [];

  const noCorp: CapturedSubResource[] = [];
  const invalid: CapturedSubResource[] = [];

  for (const r of snap.crossOriginSubResources) {
    if (r.corp === null) {
      noCorp.push(r);
    } else if (!ACCEPTABLE_CORP.has(r.corp)) {
      invalid.push(r);
    }
  }

  const out: CorpFinding[] = [];
  const renderEx = (r: CapturedSubResource) => `${r.origin} ← '${r.url}'`;

  if (noCorp.length > 0) {
    const examples = noCorp.slice(0, 5).map(renderEx);
    const severity: 'strict' | 'warn' = snap.pageRequiresCorp ? 'strict' : 'warn';
    const blocker = snap.pageRequiresCorp
      ? 'The page sets Cross-Origin-Embedder-Policy: require-corp, so the browser BLOCKS these resources at load — they fail to render.'
      : 'The page does not currently enforce COEP=require-corp, so the resources still load. The moment the page adopts COEP=require-corp (a supersociety baseline for any app handling sensitive data), every one of these sub-resources stops working.';
    out.push({
      severity,
      kind: 'corp.cross-origin-resource-no-corp',
      detail: `${noCorp.length} cross-origin sub-resource(s) lack a Cross-Origin-Resource-Policy header. ${blocker} The fix lives on the SERVER side of each resource — set 'Cross-Origin-Resource-Policy: cross-origin' on the asset response (or 'same-site' / 'same-origin' for tighter scoping). Examples: ${examples.join('; ')}`,
      evidence: {
        count: noCorp.length,
        examples,
        pageRequiresCorp: snap.pageRequiresCorp,
      },
    });
  }

  if (invalid.length > 0) {
    const examples = invalid.slice(0, 5).map((r) => `${renderEx(r)} (CORP='${r.corp}')`);
    out.push({
      severity: 'warn',
      kind: 'corp.cross-origin-resource-invalid',
      detail: `${invalid.length} cross-origin sub-resource(s) have a Cross-Origin-Resource-Policy header set to a value not in the W3C-recognised set ('same-origin', 'same-site', 'cross-origin'). Browsers may reject the resource entirely. Examples: ${examples.join('; ')}`,
      evidence: { count: invalid.length, examples },
    });
  }

  return out;
}
