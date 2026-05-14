/**
 * referrerPolicy.ts — Referrer-Policy response-header detector.
 * T76.
 *
 * The Referrer-Policy header tells browsers how much of the
 * current URL to expose in the `Referer` header on outbound
 * requests (navigation + sub-resource fetches). Bad policies
 * leak:
 *   * The full URL (including paths + query strings) to every
 *     third-party fetch — analytics, ads, fonts, CDNs.
 *   * Session tokens / user IDs that appear in the URL.
 *   * Internal-only paths that reveal infrastructure shape.
 *
 * Modern browsers default to `strict-origin-when-cross-origin`
 * (which only sends the origin cross-site, no path/query) when
 * no header is set — safe-ish but inconsistent across older
 * clients. Best practice: set the header explicitly.
 *
 * Findings:
 *
 *   - referrer-policy.missing       warn
 *     No Referrer-Policy header. Modern browsers fall back to a
 *     safe default but older browsers / embedded views may leak
 *     full URLs.
 *
 *   - referrer-policy.permissive    strict
 *     Policy explicitly set to `unsafe-url`, `no-referrer-when-downgrade`,
 *     or `origin-when-cross-origin` — these all leak more
 *     information than the modern default. `unsafe-url` always
 *     sends full URL; `no-referrer-when-downgrade` sends full
 *     URL on same-protocol fetches; `origin-when-cross-origin`
 *     leaks path/query on same-origin fetches.
 *
 *   - referrer-policy.invalid       warn
 *     Policy is set to an unknown token. Browsers fall back to
 *     their default, but the operator's INTENT was lost.
 *
 * Out of scope:
 *   * http pages + localhost — same exemptions as hsts /
 *     xFrameOptions (consistent with the response-header detector
 *     family doctrine).
 *
 * Reads from the same `topLevelResponseHeaders` Map as hsts +
 * xFrameOptions — third consumer of the shared capture path.
 *
 * Mirror: crates/crawler-detectors/src/referrer_policy.rs (not
 * shipped in v1; no Rust mirror for response-header detectors
 * until the chromiumoxide port T75 needs them).
 */

export interface ReferrerPolicyFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface ReferrerPolicySnapshot {
  pageUrl: string;
  /** True iff the page itself was loaded over https. */
  pageIsHttps: boolean;
  /** Localhost / loopback exemption. */
  pageIsLocalhost: boolean;
  /** Raw value of the Referrer-Policy header, '' if absent. */
  policyValue: string;
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

export function buildReferrerPolicySnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): ReferrerPolicySnapshot {
  const pageIsHttps = pageUrl.startsWith('https://');
  const pageIsLocalhost = isLocalhost(pageUrl);
  let policyValue = '';
  if (headers) {
    for (const [k, v] of Object.entries(headers)) {
      if (k.toLowerCase() === 'referrer-policy') {
        policyValue = v;
        break;
      }
    }
  }
  return { pageUrl, pageIsHttps, pageIsLocalhost, policyValue };
}

/**
 * Valid Referrer-Policy tokens per the W3C spec
 * (https://www.w3.org/TR/referrer-policy/#referrer-policies).
 * Multi-token policies are allowed and the first one the browser
 * recognises wins — we accept any line that has at least one
 * recognised token.
 */
const VALID_TOKENS = new Set<string>([
  'no-referrer',
  'no-referrer-when-downgrade',
  'origin',
  'origin-when-cross-origin',
  'same-origin',
  'strict-origin',
  'strict-origin-when-cross-origin',
  'unsafe-url',
]);

/** Tokens that LEAK more than the modern default. */
const PERMISSIVE_TOKENS = new Set<string>([
  'unsafe-url',                  // ALWAYS sends full URL.
  'no-referrer-when-downgrade',  // sends full URL on same-protocol.
  'origin-when-cross-origin',    // leaks same-origin path/query.
]);

/**
 * Parse a policy header. Multi-value form: comma-separated, last
 * recognised token wins (W3C). For findings we report the FIRST
 * recognised token (most prominent) and treat unknown tokens as
 * invalid.
 */
function classifyPolicy(value: string): 'safe' | 'permissive' | 'invalid' {
  const tokens = value
    .split(',')
    .map((t) => t.trim().toLowerCase())
    .filter(Boolean);
  if (tokens.length === 0) return 'invalid';

  // Browsers use the LAST recognised token (per the spec — later
  // values override earlier ones, allowing safe defaults with a
  // permissive override). Walk right-to-left, return on first
  // valid token.
  for (let i = tokens.length - 1; i >= 0; i--) {
    const t = tokens[i];
    if (!VALID_TOKENS.has(t)) continue;
    return PERMISSIVE_TOKENS.has(t) ? 'permissive' : 'safe';
  }
  return 'invalid';
}

export function detectReferrerPolicyIssues(
  snap: ReferrerPolicySnapshot,
): ReferrerPolicyFinding[] {
  if (!snap.pageIsHttps) return [];
  if (snap.pageIsLocalhost) return [];

  const value = snap.policyValue.trim();
  if (value === '') {
    return [
      {
        severity: 'warn',
        kind: 'referrer-policy.missing',
        detail: `No Referrer-Policy response header. Modern browsers fall back to 'strict-origin-when-cross-origin' (safe) but older browsers and embedded views may leak the full URL + query string to every third-party fetch (analytics, fonts, CDNs). Set 'Referrer-Policy: strict-origin-when-cross-origin' explicitly.`,
        evidence: { pageUrl: snap.pageUrl },
      },
    ];
  }

  const cls = classifyPolicy(value);
  if (cls === 'permissive') {
    return [
      {
        severity: 'strict',
        kind: 'referrer-policy.permissive',
        detail: `Referrer-Policy is '${value}' — explicitly LESS safe than the modern browser default. Every cross-origin fetch may include the page's full URL + query string. Switch to 'strict-origin-when-cross-origin' or 'no-referrer'.`,
        evidence: { pageUrl: snap.pageUrl, policyValue: value },
      },
    ];
  }
  if (cls === 'invalid') {
    return [
      {
        severity: 'warn',
        kind: 'referrer-policy.invalid',
        detail: `Referrer-Policy value '${value}' contains no recognised W3C token. Browsers fall back to their default; the operator's INTENT is lost. Use one of: no-referrer, no-referrer-when-downgrade, origin, origin-when-cross-origin, same-origin, strict-origin, strict-origin-when-cross-origin, unsafe-url.`,
        evidence: { pageUrl: snap.pageUrl, policyValue: value },
      },
    ];
  }

  return [];
}
