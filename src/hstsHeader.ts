/**
 * hstsHeader.ts — HSTS response-header detector. T76.
 *
 * Strict-Transport-Security (HSTS) tells browsers to ALWAYS load
 * the site over https, even if the user types `http://`. Without
 * it, the first request on a clean browser falls back to http and
 * can be MITM'd before the server's https redirect lands. Per
 * the project's state-actor threat model (CLAUDE.md), defence in
 * depth: every https response should pin TLS via HSTS.
 *
 * Findings:
 *
 *   - hsts.missing           strict
 *     Page is served over https but the response carries no
 *     `Strict-Transport-Security` header. Most browsers' HSTS
 *     preload lists won't cover the site; first-hit users
 *     remain MITM-vulnerable.
 *
 *   - hsts.max-age-too-short warn
 *     Header present but `max-age` is < 6 months (15768000s).
 *     Short max-age means the protection lapses if the user
 *     doesn't return within the window — undermines the policy.
 *
 *   - hsts.no-subdomains     warn
 *     Header present and max-age ≥ 6 months but missing
 *     `includeSubDomains`. Subdomain takeovers remain possible.
 *     (Not strict — sites with un-controlled subdomains may
 *     intentionally omit this.)
 *
 * Out of scope:
 *   * Pages served over http — HSTS doesn't apply, no findings
 *     fire.
 *   * Localhost (127.0.0.1 / ::1) — browsers don't honour HSTS
 *     on loopback regardless.
 *   * `preload` directive presence — checking HSTS preload list
 *     eligibility is a separate, fuzzier check.
 *
 * Mirror: crates/crawler-detectors/src/hsts_header.rs.
 */

export interface HstsFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface HstsSnapshot {
  pageUrl: string;
  /** True iff the page itself was loaded over https. */
  pageIsHttps: boolean;
  /** True iff the page is on localhost / 127.0.0.1 / ::1 — out of scope. */
  pageIsLocalhost: boolean;
  /** Raw value of the Strict-Transport-Security header, '' if absent. */
  hstsValue: string;
}

/**
 * Localhost detection: HSTS doesn't apply on loopback so any
 * detector firing on localhost would be false-positive noise.
 */
function isLocalhost(url: string): boolean {
  try {
    const u = new URL(url);
    const h = u.hostname;
    return h === 'localhost' || h === '127.0.0.1' || h === '::1' || h.endsWith('.localhost');
  } catch {
    return false;
  }
}

/**
 * Build a snapshot from a captured top-level navigation response's
 * headers. Caller obtains headers via main.ts's
 * `topLevelResponseHeaders.get(pageUrl)`.
 */
export function buildHstsSnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): HstsSnapshot {
  const pageIsHttps = pageUrl.startsWith('https://');
  const pageIsLocalhost = isLocalhost(pageUrl);
  // HTTP header names are case-insensitive. Lowercase the lookup.
  let hstsValue = '';
  if (headers) {
    for (const [k, v] of Object.entries(headers)) {
      if (k.toLowerCase() === 'strict-transport-security') {
        hstsValue = v;
        break;
      }
    }
  }
  return { pageUrl, pageIsHttps, pageIsLocalhost, hstsValue };
}

/**
 * Parse `max-age=N` (case-insensitive). Returns the number of
 * seconds, or null on parse error / missing directive.
 */
function parseMaxAge(headerValue: string): number | null {
  for (const part of headerValue.split(';')) {
    const t = part.trim();
    const m = /^max-age\s*=\s*"?(\d+)"?$/i.exec(t);
    if (m) {
      const n = parseInt(m[1], 10);
      if (Number.isFinite(n)) return n;
    }
  }
  return null;
}

/**
 * Case-insensitive token presence check (e.g. `includeSubDomains`).
 */
function hasDirective(headerValue: string, directive: string): boolean {
  const target = directive.toLowerCase();
  for (const part of headerValue.split(';')) {
    if (part.trim().toLowerCase() === target) return true;
  }
  return false;
}

/** 6 months in seconds — the de-facto minimum effective HSTS lifetime. */
const HSTS_SIX_MONTHS_SECONDS = 6 * 30 * 24 * 60 * 60; // ≈ 15_552_000

export function detectHstsIssues(snap: HstsSnapshot): HstsFinding[] {
  if (!snap.pageIsHttps) return [];
  if (snap.pageIsLocalhost) return [];

  const out: HstsFinding[] = [];

  const value = snap.hstsValue.trim();
  if (value === '') {
    out.push({
      severity: 'strict',
      kind: 'hsts.missing',
      detail: `Page is served over https but the response carries no Strict-Transport-Security header. First-hit users on a clean browser hit http:// (whatever they typed) and can be MITM'd before the server's redirect to https. Add 'Strict-Transport-Security: max-age=15768000; includeSubDomains' (6 months) as a minimum.`,
      evidence: { pageUrl: snap.pageUrl },
    });
    return out;
  }

  const maxAge = parseMaxAge(value);
  if (maxAge === null) {
    // Header present but unparseable. Treat as missing.
    out.push({
      severity: 'strict',
      kind: 'hsts.missing',
      detail: `Strict-Transport-Security header present ('${value}') but no max-age directive parsed. Browsers will ignore the policy. Fix the header syntax.`,
      evidence: { pageUrl: snap.pageUrl, hstsValue: value },
    });
    return out;
  }

  if (maxAge < HSTS_SIX_MONTHS_SECONDS) {
    out.push({
      severity: 'warn',
      kind: 'hsts.max-age-too-short',
      detail: `Strict-Transport-Security max-age is ${maxAge} seconds — less than 6 months (${HSTS_SIX_MONTHS_SECONDS}). Protection lapses if the user doesn't return within the window. Use 'max-age=31536000' (1 year) or longer for production sites.`,
      evidence: { pageUrl: snap.pageUrl, hstsValue: value, maxAge, threshold: HSTS_SIX_MONTHS_SECONDS },
    });
  } else if (!hasDirective(value, 'includeSubDomains')) {
    // Only warn on no-subdomains when max-age is already adequate.
    out.push({
      severity: 'warn',
      kind: 'hsts.no-subdomains',
      detail: `Strict-Transport-Security has adequate max-age (${maxAge}) but missing 'includeSubDomains'. Subdomain takeovers can serve http://attacker.example.com — the apex site's HSTS won't protect them. If subdomains are under your control, add includeSubDomains.`,
      evidence: { pageUrl: snap.pageUrl, hstsValue: value },
    });
  }

  return out;
}
