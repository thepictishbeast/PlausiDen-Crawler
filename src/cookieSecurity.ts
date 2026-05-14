/**
 * cookieSecurity.ts — Set-Cookie attribute audit. T76.
 *
 * Reads the response's `Set-Cookie` header(s) from the captured
 * top-level navigation response and surfaces cookies that lack
 * the modern security attributes:
 *
 *   * `Secure` — cookie is only sent over https. Required on
 *     https pages; absence allows the cookie to leak over an
 *     http downgrade (MITM-able even briefly).
 *   * `SameSite` — `Strict` blocks the cookie from cross-site
 *     fetches entirely; `Lax` allows top-level GETs but blocks
 *     POSTs and sub-resource fetches; `None` (with `Secure`)
 *     opts back in to all cross-site sends. Missing → modern
 *     browsers default `Lax` but old clients default to no
 *     restriction, allowing CSRF.
 *   * `HttpOnly` — JS can't read the cookie via `document.cookie`.
 *     Critical for session cookies — XSS can't steal them
 *     without it.
 *
 * Findings:
 *
 *   - cookie.no-secure                strict (https only)
 *     Cookie set without `Secure` on an https page.
 *
 *   - cookie.no-samesite              warn
 *     Cookie set without `SameSite` attribute. Modern browsers
 *     default `Lax` but old clients leave it unrestricted.
 *
 *   - cookie.session-no-httponly      warn
 *     Cookie name looks session-like (matches /sess|sid|auth|
 *     token|jwt/i) AND lacks `HttpOnly`. XSS attacker can read
 *     and exfiltrate.
 *
 *   - cookie.samesite-none-no-secure  strict
 *     `SameSite=None` set without `Secure`. Browsers REJECT
 *     this combination — the cookie isn't stored at all.
 *
 * Out of scope:
 *   * http pages — Secure attribute can't apply, no-samesite
 *     warn still fires but no-secure does not.
 *   * Localhost — same exemption family as hsts/xFrameOptions.
 *
 * Reads from the same `topLevelResponseHeaders` Map as
 * hsts + xFrameOptions + referrerPolicy. This is the FOURTH
 * response-header detector — per docs/DETECTORS.md note from
 * cycle 17, this is the right point to extract a generic
 * helper. Deliberately NOT extracted here yet — see the
 * "TODO" in DOGFOOD_RUNS.md cycle 19 — the per-cookie shape
 * is genuinely different from the per-header shape (one
 * response can have MULTIPLE Set-Cookie headers, each with
 * its own attributes), so a generic helper would be
 * shoe-horning. Pattern-extraction deferred to a sibling
 * cycle.
 */

export interface CookieSecurityFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedCookie {
  name: string;
  /** Empty string if not set. */
  sameSite: string;
  hasSecure: boolean;
  hasHttpOnly: boolean;
  /** Raw Set-Cookie line, truncated. */
  raw: string;
}

export interface CookieSecuritySnapshot {
  pageUrl: string;
  pageIsHttps: boolean;
  pageIsLocalhost: boolean;
  cookies: CapturedCookie[];
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

/**
 * Parse one Set-Cookie line. Returns null on malformed (no `=`
 * before the first attribute separator). The raw line is
 * preserved on the result for the audit trail.
 *
 * Set-Cookie syntax: `name=value; Attr1; Attr2=val; ...`
 *
 * Browsers tolerate a lot of weirdness here (whitespace, casing,
 * mixed quoting). We normalise attribute names to lowercase.
 */
function parseSetCookie(raw: string): CapturedCookie | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  const parts = trimmed.split(';');
  const head = parts[0].trim();
  const eq = head.indexOf('=');
  if (eq <= 0) return null;
  const name = head.slice(0, eq).trim();
  if (!name) return null;
  let sameSite = '';
  let hasSecure = false;
  let hasHttpOnly = false;
  for (let i = 1; i < parts.length; i++) {
    const a = parts[i].trim();
    if (!a) continue;
    const lower = a.toLowerCase();
    if (lower === 'secure') {
      hasSecure = true;
    } else if (lower === 'httponly') {
      hasHttpOnly = true;
    } else if (lower.startsWith('samesite=')) {
      sameSite = a.slice('samesite='.length).trim();
    } else if (lower === 'samesite') {
      // Bare `SameSite` (no value) — browsers treat as no-attribute,
      // record empty.
      sameSite = '';
    }
  }
  return {
    name,
    sameSite,
    hasSecure,
    hasHttpOnly,
    raw: trimmed.slice(0, 200),
  };
}

/**
 * Extract every Set-Cookie value from a headers map. HTTP
 * historically allows multiple Set-Cookie headers per response;
 * different servers (and Playwright's exposure) collapse them
 * into one comma-separated value OR keep them separate. We
 * accept both — splitting on `\n` first (Playwright's
 * representation when multiple are present), then on commas
 * IF the first header value contains multiple cookies (rare).
 *
 * The comma-split is risky because cookie expires-attr values
 * may contain commas. Solution: only comma-split when we see a
 * known-cookie-attribute keyword on the right side, leaving
 * Expires=... commas alone.
 */
function extractSetCookieLines(
  headers: Record<string, string> | undefined,
): string[] {
  if (!headers) return [];
  for (const [k, v] of Object.entries(headers)) {
    if (k.toLowerCase() === 'set-cookie') {
      // Playwright's `headers()` joins multiple Set-Cookie with
      // newlines. Split on \n first.
      return v.split('\n').map((s) => s.trim()).filter(Boolean);
    }
  }
  return [];
}

export function buildCookieSecuritySnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): CookieSecuritySnapshot {
  const pageIsHttps = pageUrl.startsWith('https://');
  const pageIsLocalhost = isLocalhost(pageUrl);
  const lines = extractSetCookieLines(headers);
  const cookies: CapturedCookie[] = [];
  for (const line of lines) {
    const c = parseSetCookie(line);
    if (c) cookies.push(c);
  }
  return { pageUrl, pageIsHttps, pageIsLocalhost, cookies };
}

/**
 * Heuristic for "looks like a session cookie". Names that
 * commonly carry session tokens. False positives are fine
 * (warn-only); false negatives mean we miss real defects, so
 * be liberal.
 */
function looksLikeSessionCookie(name: string): boolean {
  return /sess|sid|auth|token|jwt|bearer/i.test(name);
}

export function detectCookieSecurityIssues(
  snap: CookieSecuritySnapshot,
): CookieSecurityFinding[] {
  if (snap.pageIsLocalhost) return [];
  if (snap.cookies.length === 0) return [];

  const noSecure: CapturedCookie[] = [];
  const noSameSite: CapturedCookie[] = [];
  const sessionNoHttpOnly: CapturedCookie[] = [];
  const sameSiteNoneNoSecure: CapturedCookie[] = [];

  for (const c of snap.cookies) {
    // SameSite=None requires Secure; browsers reject otherwise.
    if (c.sameSite.toLowerCase() === 'none' && !c.hasSecure) {
      sameSiteNoneNoSecure.push(c);
    }
    if (snap.pageIsHttps && !c.hasSecure) {
      noSecure.push(c);
    }
    if (!c.sameSite) {
      noSameSite.push(c);
    }
    if (looksLikeSessionCookie(c.name) && !c.hasHttpOnly) {
      sessionNoHttpOnly.push(c);
    }
  }

  const out: CookieSecurityFinding[] = [];
  const renderEx = (c: CapturedCookie) => `'${c.name}' raw='${c.raw}'`;

  if (noSecure.length > 0) {
    const examples = noSecure.slice(0, 5).map(renderEx);
    out.push({
      severity: 'strict',
      kind: 'cookie.no-secure',
      detail: `${noSecure.length} cookie(s) set without 'Secure' attribute on this https page. The cookie can be sent over http if the user is briefly downgraded (MITM, network rewrite, mixed-content). Add '; Secure' to every Set-Cookie. Examples: ${examples.join('; ')}`,
      evidence: { count: noSecure.length, examples },
    });
  }
  if (sameSiteNoneNoSecure.length > 0) {
    const examples = sameSiteNoneNoSecure.slice(0, 5).map(renderEx);
    out.push({
      severity: 'strict',
      kind: 'cookie.samesite-none-no-secure',
      detail: `${sameSiteNoneNoSecure.length} cookie(s) set with 'SameSite=None' but no 'Secure' attribute. Browsers REJECT this combination — the cookie isn't stored at all. Either add Secure (and only set on https) or change to 'SameSite=Lax'. Examples: ${examples.join('; ')}`,
      evidence: { count: sameSiteNoneNoSecure.length, examples },
    });
  }
  if (noSameSite.length > 0) {
    const examples = noSameSite.slice(0, 5).map(renderEx);
    out.push({
      severity: 'warn',
      kind: 'cookie.no-samesite',
      detail: `${noSameSite.length} cookie(s) set without 'SameSite' attribute. Modern browsers default 'Lax' (safe) but older clients leave it unrestricted, exposing the user to CSRF. Set 'SameSite=Strict' (or 'Lax' if cross-site GETs are needed). Examples: ${examples.join('; ')}`,
      evidence: { count: noSameSite.length, examples },
    });
  }
  if (sessionNoHttpOnly.length > 0) {
    const examples = sessionNoHttpOnly.slice(0, 5).map(renderEx);
    out.push({
      severity: 'warn',
      kind: 'cookie.session-no-httponly',
      detail: `${sessionNoHttpOnly.length} session-looking cookie(s) (name matches sess/sid/auth/token/jwt/bearer) lack 'HttpOnly'. JS — including injected XSS — can read these via document.cookie and exfiltrate. Add '; HttpOnly'. Examples: ${examples.join('; ')}`,
      evidence: { count: sessionNoHttpOnly.length, examples },
    });
  }

  return out;
}
