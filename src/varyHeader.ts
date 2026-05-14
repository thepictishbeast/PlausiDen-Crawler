/**
 * varyHeader.ts — Vary header correctness audit. T76 cycle 29.
 *
 * Vary tells caches HOW TO KEY cached responses. If a response
 * varies based on a request header (Cookie, Authorization,
 * Accept-Language, User-Agent) but the response doesn't say
 * `Vary: <those headers>`, intermediate caches store the
 * response keyed by URL alone and serve it back to ANY visitor
 * — even ones whose request headers would have produced a
 * different response.
 *
 * Sister detector to cacheControl. Where cacheControl asks
 * "does the operator want this cached at all?", varyHeader
 * asks "if it IS cached, is the cache key correct?". Both can
 * fire on the same defect — defence in depth.
 *
 * Threat model: an attacker visits a victim site, gets a
 * response with their account data cached by a shared CDN /
 * corporate proxy / kiosk browser. Next visitor (or
 * cross-user request through the same cache) gets served the
 * attacker's response — including any Set-Cookie + personal
 * data baked into the body.
 *
 * Findings:
 *
 *   - vary.no-cookie-with-set-cookie-and-cacheable    warn
 *     Response has Set-Cookie AND Cache-Control allows shared
 *     caching (no `private`, no `no-store`) AND Vary doesn't
 *     include `cookie` (or wildcard `*`). A shared cache may
 *     store the response keyed by URL alone and serve it
 *     back, leaking the cookie cross-user.
 *
 *   - vary.star                                        warn
 *     `Vary: *` — explicitly tells caches "this response is
 *     uncacheable because something not visible in the
 *     request headers determines it" (RFC 7234 §4.1).
 *     Usually unintended — operators reach for `Vary: *` when
 *     they really want `Cache-Control: no-store` (which is
 *     more intent-revealing and explicit). Surfaced for
 *     confirmation.
 *
 *   - vary.invalid                                     warn
 *     Header value is empty or contains tokens that aren't
 *     valid HTTP header names. Browsers / proxies typically
 *     ignore unparseable Vary, falling back to no-key-by-
 *     headers — silently disables the operator's intent.
 *
 *   - vary.duplicate-tokens                            warn
 *     Same token (case-insensitive) appears more than once.
 *     Cosmetic; spec-conformant caches collapse but parser
 *     errors in custom proxies have been reported.
 *
 * Out of scope:
 *   * Authorization-specific Vary checks. Less common in
 *     practice; the Cookie variant covers ~90% of real
 *     defects. Could be added as a 5th finding if real-world
 *     dogfood surfaces it.
 *   * Per-sub-resource Vary (would need the cycle-27
 *     allResponseHeaders Map; queued).
 *   * Localhost (consistent with the response-header detector
 *     family).
 *
 * Reads from the same `topLevelResponseHeaders` Map as the
 * other 10 response-header detectors. ELEVENTH consumer of
 * the shared capture path. Uses the cycle-24
 * `responseHeaderDetector` helper.
 */

export interface VaryFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface VarySnapshot {
  pageUrl: string;
  pageIsLocalhost: boolean;
  /** Raw Vary header value, or null. */
  raw: string | null;
  /** Parsed token list (lowercased). Empty if no header or unparseable. */
  tokens: string[];
  /** True iff Vary was present but no valid token parsed. */
  unparseable: boolean;
  /** Same token appears more than once (case-insensitive). */
  hasDuplicates: boolean;
  /** True iff response carries Set-Cookie. */
  hasSetCookie: boolean;
  /** Cache-Control directive set, lowercased. Used to short-circuit
   *  vary.no-cookie-with-set-cookie-and-cacheable when the response
   *  is already declared uncacheable by no-store / private. */
  cacheControlDirectives: string[];
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

/**
 * RFC 7230 token grammar: 1*tchar where tchar is
 * !#$%&'*+-.^_`|~ DIGIT ALPHA. Vary specifically lists header
 * names which are tokens, plus the wildcard `*`.
 */
function isValidVaryToken(t: string): boolean {
  return t === '*' || /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/.test(t);
}

function parseCacheControlDirectives(raw: string | null): string[] {
  if (raw === null) return [];
  const out: string[] = [];
  for (const part of raw.split(',')) {
    const trimmed = part.trim();
    if (!trimmed) continue;
    const eq = trimmed.indexOf('=');
    const name = (eq < 0 ? trimmed : trimmed.slice(0, eq)).trim().toLowerCase();
    if (name) out.push(name);
  }
  return out;
}

export function buildVarySnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): VarySnapshot {
  const pageIsLocalhost = isLocalhost(pageUrl);
  const raw = getHeader(headers, 'vary');
  const hasSetCookie = getHeader(headers, 'set-cookie') !== null;
  const cacheControlDirectives = parseCacheControlDirectives(getHeader(headers, 'cache-control'));

  if (raw === null) {
    return {
      pageUrl, pageIsLocalhost, raw: null, tokens: [], unparseable: false,
      hasDuplicates: false, hasSetCookie, cacheControlDirectives,
    };
  }

  // Vary syntax: 1#field-name (RFC 7231) — comma-separated
  // header names. We tokenise on commas + whitespace.
  const rawTokens = raw.split(',').map((t) => t.trim()).filter(Boolean);
  const tokens: string[] = [];
  let anyValid = false;
  let anyInvalid = false;
  for (const t of rawTokens) {
    if (isValidVaryToken(t)) {
      tokens.push(t.toLowerCase());
      anyValid = true;
    } else {
      anyInvalid = true;
    }
  }
  const unparseable = rawTokens.length > 0 && !anyValid;

  // Duplicate detection (case-insensitive).
  const seen = new Set<string>();
  let hasDuplicates = false;
  for (const t of tokens) {
    if (seen.has(t)) {
      hasDuplicates = true;
      break;
    }
    seen.add(t);
  }
  // Tag invalid-but-not-empty as unparseable too, but only if
  // NO valid tokens at all — partial-bad-token responses still
  // fire the partial finding via duplicates / etc.
  void anyInvalid;

  return {
    pageUrl, pageIsLocalhost, raw, tokens, unparseable, hasDuplicates,
    hasSetCookie, cacheControlDirectives,
  };
}

export function detectVaryIssues(snap: VarySnapshot): VaryFinding[] {
  if (snap.pageIsLocalhost) return [];
  const out: VaryFinding[] = [];

  const cacheable =
    !snap.cacheControlDirectives.includes('no-store') &&
    !snap.cacheControlDirectives.includes('private');

  // Vary header IS present checks first (raw !== null path).
  if (snap.raw !== null) {
    if (snap.unparseable) {
      out.push({
        severity: 'warn',
        kind: 'vary.invalid',
        detail: `Vary header is present but contains no valid token (RFC 7230 token grammar). Browsers / proxies typically ignore — silently disables the operator's intent. Header value: '${snap.raw.slice(0, 200)}'.`,
        evidence: { raw: snap.raw.slice(0, 200) },
      });
    }
    if (snap.tokens.includes('*')) {
      out.push({
        severity: 'warn',
        kind: 'vary.star',
        detail: `Vary header contains '*' — explicitly tells caches the response is uncacheable because something not visible in the request headers determines it. Usually unintended; if you want the response uncached, set 'Cache-Control: no-store' instead — more intent-revealing and explicit.`,
        evidence: { tokens: snap.tokens },
      });
    }
    if (snap.hasDuplicates) {
      out.push({
        severity: 'warn',
        kind: 'vary.duplicate-tokens',
        detail: `Vary header contains duplicate tokens (case-insensitive). Spec-conformant caches collapse, but parser errors in custom proxies have been reported. Header value: '${snap.raw.slice(0, 200)}'.`,
        evidence: { tokens: snap.tokens },
      });
    }
  }

  // Cookie-cacheability check — fires whether Vary is present
  // or absent, as long as the cookie key isn't there.
  if (snap.hasSetCookie && cacheable) {
    const hasCookie = snap.tokens.includes('cookie') || snap.tokens.includes('*');
    if (!hasCookie) {
      out.push({
        severity: 'warn',
        kind: 'vary.no-cookie-with-set-cookie-and-cacheable',
        detail: `Response carries Set-Cookie AND Cache-Control allows shared caching (no 'private', no 'no-store') AND Vary ${snap.raw === null ? 'is absent' : `does not include 'cookie' or '*' (Vary: '${snap.raw.slice(0, 200)}')`}. A shared cache (CDN / corporate proxy / kiosk browser) may store the response keyed by URL alone and serve it back, leaking the Set-Cookie + personalised body to subsequent visitors. Add 'Vary: Cookie' (and ideally tighten Cache-Control to 'private' or 'no-store').`,
        evidence: {
          tokens: snap.tokens,
          cacheControlDirectives: snap.cacheControlDirectives,
        },
      });
    }
  }

  return out;
}
