/**
 * cacheControl.ts — Cache-Control directive hygiene audit.
 * T76 cycle 28.
 *
 * Real defect class this catches: Web Cache Deception (Omer
 * Gil, 2017). An attacker tricks an intermediate cache (CDN,
 * reverse proxy, browser cache shared across users on a
 * kiosk) into storing a per-user personalised response by
 * appending a fake static-asset extension to the URL
 * (`/account/foo.css`). If the response carries Set-Cookie
 * but Cache-Control allows shared caching, the next visitor
 * to the cached URL receives the previous user's session +
 * personal data.
 *
 * The defence is a single Cache-Control directive: `no-store`
 * forbids any cache from storing the response. `private`
 * allows browser-only caching but forbids shared/proxy
 * caches — adequate for personalised but non-sensitive
 * content. `public` explicitly invites shared caching and is
 * incompatible with Set-Cookie.
 *
 * Findings:
 *
 *   - cache-control.missing                warn
 *     No Cache-Control header at all. The browser falls back
 *     to RFC 7234 heuristic freshness (typically 10% of the
 *     Last-Modified age). Unpredictable across cache
 *     implementations — be explicit.
 *
 *   - cache-control.public-with-cookie     strict
 *     Response carries Set-Cookie AND Cache-Control includes
 *     `public`. Cache deception risk: an intermediate cache
 *     can store the response keyed by URL alone, then serve
 *     it (with the original Set-Cookie!) to the next visitor.
 *
 *   - cache-control.no-private-with-cookie warn
 *     Response carries Set-Cookie AND Cache-Control omits
 *     both `no-store` and `private`. Less acute than
 *     public-with-cookie but still cacheable by sufficiently
 *     permissive proxies.
 *
 *   - cache-control.invalid                warn
 *     Header value couldn't be parsed into any directive.
 *     Browsers / proxies fall back to no-Cache-Control
 *     semantics — silently disables the operator's intent.
 *
 *   - cache-control.unrealistic-maxage     warn
 *     `max-age` greater than 31536000 (1 year). RFC 7234
 *     section 5.2.1.1 says caches SHOULD treat values
 *     greater than 1 year as 1 year — anything larger is
 *     dead code at best, intent-obscuring at worst.
 *
 *   - cache-control.contradictory          warn
 *     Directive set contains contradictions: both `no-store`
 *     and `max-age` (no-store wins, max-age is dead). Both
 *     `private` and `public` (the spec is ambiguous; most
 *     implementations honour `private`). Both `no-cache` and
 *     `immutable` (immutable means "skip revalidation",
 *     no-cache means "always revalidate" — they cancel).
 *
 * Out of scope:
 *   * Pragma: no-cache (legacy HTTP/1.0 fallback) — separate
 *     concern; if the operator sets BOTH Pragma + Cache-
 *     Control with consistent intent, no defect.
 *   * Expires header — superseded by Cache-Control max-age.
 *     Don't double-flag.
 *   * Per-resource Cache-Control on sub-resources — out of
 *     scope for this top-level detector. Could be a future
 *     per-sub-resource detector once the capture path
 *     established by CORP gets reused.
 *   * Localhost (consistent with the response-header detector
 *     family).
 *
 * Reads from the same `topLevelResponseHeaders` Map as the
 * other 9 response-header detectors. TENTH consumer of the
 * shared capture path. Uses the cycle-24 `responseHeader-
 * Detector` helper.
 */

export interface CacheControlFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface ParsedCacheControl {
  /** All directive names (lowercased), in declaration order. */
  directives: string[];
  /** Lookup of directive name → value (for value-bearing directives). */
  values: Record<string, string>;
  /** True if the header was present but no directive parsed. */
  unparseable: boolean;
}

export interface CacheControlSnapshot {
  pageUrl: string;
  pageIsLocalhost: boolean;
  /** Raw Cache-Control header value, or null. */
  raw: string | null;
  /** Parsed form. Empty if no header. */
  parsed: ParsedCacheControl;
  /** True iff the response also carries Set-Cookie. */
  hasSetCookie: boolean;
}

function isLocalhost(url: string): boolean {
  try {
    const u = new URL(url);
    const h = u.hostname;
    return h === 'localhost' || h === '127.0.0.1' || h === '::1' || h.endsWith('.localhost');
  } catch {
    return false;
  }
}

function getHeader(headers: Record<string, string> | undefined, name: string): string | null {
  if (!headers) return null;
  for (const [k, v] of Object.entries(headers)) {
    if (k.toLowerCase() === name) return v;
  }
  return null;
}

function parseCacheControl(raw: string): ParsedCacheControl {
  const directives: string[] = [];
  const values: Record<string, string> = {};
  for (const part of raw.split(',')) {
    const trimmed = part.trim();
    if (!trimmed) continue;
    const eq = trimmed.indexOf('=');
    let name: string;
    let value = '';
    if (eq < 0) {
      name = trimmed.toLowerCase();
    } else {
      name = trimmed.slice(0, eq).trim().toLowerCase();
      value = trimmed.slice(eq + 1).trim();
      // Strip quotes from values (max-age="3600").
      if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
        value = value.slice(1, -1);
      }
    }
    if (!name) continue;
    directives.push(name);
    if (value) values[name] = value;
  }
  const unparseable = raw.trim().length > 0 && directives.length === 0;
  return { directives, values, unparseable };
}

export function buildCacheControlSnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): CacheControlSnapshot {
  const pageIsLocalhost = isLocalhost(pageUrl);
  const raw = getHeader(headers, 'cache-control');
  const parsed = raw === null
    ? { directives: [], values: {}, unparseable: false }
    : parseCacheControl(raw);
  const hasSetCookie = getHeader(headers, 'set-cookie') !== null;
  return { pageUrl, pageIsLocalhost, raw, parsed, hasSetCookie };
}

const ONE_YEAR_SECONDS = 31536000;

export function detectCacheControlIssues(snap: CacheControlSnapshot): CacheControlFinding[] {
  if (snap.pageIsLocalhost) return [];
  const out: CacheControlFinding[] = [];

  if (snap.raw === null) {
    out.push({
      severity: 'warn',
      kind: 'cache-control.missing',
      detail: `No Cache-Control header. Browsers and intermediate caches fall back to RFC 7234 heuristic freshness (typically 10% of the Last-Modified age) — unpredictable across implementations. Be explicit: 'Cache-Control: no-store' for sensitive pages, 'private, max-age=<seconds>' for personalised but cacheable, 'public, max-age=<seconds>, immutable' for static assets.`,
      evidence: { hasSetCookie: snap.hasSetCookie },
    });
    return out;
  }

  if (snap.parsed.unparseable) {
    out.push({
      severity: 'warn',
      kind: 'cache-control.invalid',
      detail: `Cache-Control header is set but couldn't be parsed into any directive. Browsers and proxies fall back to no-Cache-Control semantics, silently disabling the operator's intent. Header value: '${snap.raw.slice(0, 200)}'.`,
      evidence: { raw: snap.raw.slice(0, 200) },
    });
    return out;
  }

  const dset = new Set(snap.parsed.directives);

  // --- Set-Cookie + caching combinations ---
  if (snap.hasSetCookie && dset.has('public')) {
    out.push({
      severity: 'strict',
      kind: 'cache-control.public-with-cookie',
      detail: `Response sets a Set-Cookie header AND Cache-Control includes 'public'. Web Cache Deception risk: an intermediate cache (CDN, reverse proxy, kiosk browser) can store the response keyed by URL, then serve it WITH THE ORIGINAL Set-Cookie to the next visitor. Replace 'public' with 'no-store' (sensitive) or 'private' (personalised, browser-only).`,
      evidence: { directives: snap.parsed.directives },
    });
  } else if (snap.hasSetCookie && !dset.has('no-store') && !dset.has('private')) {
    out.push({
      severity: 'warn',
      kind: 'cache-control.no-private-with-cookie',
      detail: `Response sets a Set-Cookie header but Cache-Control omits both 'no-store' and 'private'. Sufficiently permissive proxies (some CDNs, corporate caches) may store and serve the response cross-user. Add 'private' (browser-only caching) or 'no-store' (no caching at all).`,
      evidence: { directives: snap.parsed.directives },
    });
  }

  // --- max-age sanity ---
  const maxAge = snap.parsed.values['max-age'];
  if (maxAge !== undefined) {
    const n = Number(maxAge);
    if (!Number.isFinite(n) || n < 0) {
      out.push({
        severity: 'warn',
        kind: 'cache-control.invalid',
        detail: `Cache-Control max-age value '${maxAge}' is not a non-negative integer. Browsers / proxies typically ignore.`,
        evidence: { maxAge },
      });
    } else if (n > ONE_YEAR_SECONDS) {
      out.push({
        severity: 'warn',
        kind: 'cache-control.unrealistic-maxage',
        detail: `Cache-Control max-age=${n} exceeds 1 year (31536000s). RFC 7234 §5.2.1.1: caches SHOULD treat values greater than 1 year as 1 year — anything larger is dead code at best.`,
        evidence: { maxAge: n },
      });
    }
  }

  // --- Contradictory directive combinations ---
  const contradictions: string[] = [];
  if (dset.has('no-store') && (dset.has('max-age') || dset.has('s-maxage'))) {
    contradictions.push("'no-store' + 'max-age': no-store wins, max-age is dead");
  }
  if (dset.has('public') && dset.has('private')) {
    contradictions.push("'public' + 'private': spec ambiguous, most implementations honour 'private'");
  }
  if (dset.has('no-cache') && dset.has('immutable')) {
    contradictions.push("'no-cache' + 'immutable': cancel each other (revalidate-always vs skip-revalidate)");
  }
  if (dset.has('no-store') && dset.has('immutable')) {
    contradictions.push("'no-store' + 'immutable': no-store forbids any cache, immutable assumes a cache");
  }
  if (contradictions.length > 0) {
    out.push({
      severity: 'warn',
      kind: 'cache-control.contradictory',
      detail: `Cache-Control contains contradictory directives. ${contradictions.join('. ')}. Pick one intent and stick with it.`,
      evidence: { directives: snap.parsed.directives, contradictions },
    });
  }

  return out;
}
