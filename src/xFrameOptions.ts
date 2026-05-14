/**
 * xFrameOptions.ts — clickjacking defence detector. T76.
 *
 * The page's framing policy decides whether other origins can
 * embed it in an `<iframe>`. Without a policy, an attacker can
 * iframe the site, overlay invisible UI on top, and trick the
 * user into clicking through (clickjacking). Two headers control
 * this:
 *
 *   - `X-Frame-Options` (legacy, but still honoured by every
 *     browser): DENY | SAMEORIGIN | ALLOW-FROM <uri>.
 *   - `Content-Security-Policy: frame-ancestors ...` (modern,
 *     supersedes XFO when present).
 *
 * Either one provides the protection. The detector flags pages
 * that have NEITHER.
 *
 * Findings:
 *
 *   - frame-options.missing             strict
 *     Page has no X-Frame-Options header AND no
 *     `frame-ancestors` directive in its Content-Security-Policy.
 *     Iframable by any origin → clickjacking risk.
 *
 *   - frame-options.allowall            warn
 *     X-Frame-Options is set but `ALLOW-FROM *` (or
 *     equivalent open `frame-ancestors *`). Effectively no
 *     protection — same as missing, but signals an intentional
 *     open policy that may have been a mistake.
 *
 *   - frame-options.invalid             warn
 *     X-Frame-Options is set to a value other than DENY /
 *     SAMEORIGIN / ALLOW-FROM <uri>. Browsers treat unrecognised
 *     values as no-protection.
 *
 * Out of scope:
 *   * http pages — clickjacking still applies but the project
 *     considers http-only sites already broken on transport
 *     security. Don't double-report. Same exemption as hsts.
 *   * Localhost — for dev-only.
 *   * Pages that legitimately want to be embedded everywhere
 *     (oEmbed widgets, embeddable players) should set
 *     `Content-Security-Policy: frame-ancestors *` to make the
 *     intent explicit; that gets the warn (not strict).
 *
 * Reads from the same `topLevelResponseHeaders` Map as
 * hstsHeader — single capture path, second consumer.
 *
 * Mirror: crates/crawler-detectors/src/x_frame_options.rs.
 */

export interface XFrameOptionsFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface XFrameOptionsSnapshot {
  pageUrl: string;
  /** True iff the page itself was loaded over https. */
  pageIsHttps: boolean;
  /** Localhost / loopback exemption. */
  pageIsLocalhost: boolean;
  /** Raw value of the X-Frame-Options header, '' if absent. */
  xfoValue: string;
  /** Raw value of the Content-Security-Policy header, '' if absent. */
  cspValue: string;
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

export function buildXFrameOptionsSnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): XFrameOptionsSnapshot {
  const pageIsHttps = pageUrl.startsWith('https://');
  const pageIsLocalhost = isLocalhost(pageUrl);
  let xfoValue = '';
  let cspValue = '';
  if (headers) {
    for (const [k, v] of Object.entries(headers)) {
      const lk = k.toLowerCase();
      if (lk === 'x-frame-options') xfoValue = v;
      else if (lk === 'content-security-policy') cspValue = v;
    }
  }
  return { pageUrl, pageIsHttps, pageIsLocalhost, xfoValue, cspValue };
}

/**
 * Extract the `frame-ancestors` directive value from a CSP
 * header. Returns null when absent. CSP directives are
 * semicolon-separated; values within a directive are
 * whitespace-separated source-list tokens.
 */
function parseFrameAncestors(csp: string): string | null {
  for (const part of csp.split(';')) {
    const t = part.trim();
    const m = /^frame-ancestors\s+(.+)$/i.exec(t);
    if (m) return m[1].trim();
  }
  return null;
}

/**
 * Validate an X-Frame-Options value against the spec
 * (RFC 7034). Returns 'deny' | 'sameorigin' | 'allow-from' |
 * 'invalid'. Case-insensitive.
 */
function classifyXfo(value: string): 'deny' | 'sameorigin' | 'allow-from' | 'invalid' {
  const v = value.trim().toLowerCase();
  if (v === 'deny') return 'deny';
  if (v === 'sameorigin') return 'sameorigin';
  if (/^allow-from\s+\S+/.test(v)) return 'allow-from';
  return 'invalid';
}

export function detectXFrameOptionsIssues(
  snap: XFrameOptionsSnapshot,
): XFrameOptionsFinding[] {
  if (!snap.pageIsHttps) return [];
  if (snap.pageIsLocalhost) return [];

  const out: XFrameOptionsFinding[] = [];
  const xfo = snap.xfoValue.trim();
  const csp = snap.cspValue.trim();
  const frameAncestors = csp ? parseFrameAncestors(csp) : null;

  // CSP frame-ancestors supersedes XFO. If present and not
  // open-wildcard, we're protected — no findings.
  if (frameAncestors && frameAncestors !== '*') {
    return [];
  }

  // Flag explicit "allow all" (CSP frame-ancestors * OR XFO
  // allow-from *). The wildcard form is intentional but worth
  // surfacing — operator should confirm it's deliberate.
  if (frameAncestors === '*') {
    out.push({
      severity: 'warn',
      kind: 'frame-options.allowall',
      detail: `Content-Security-Policy frame-ancestors '*' — page is iframable by any origin. If this is an embeddable widget, fine; if it's the main app, it's a clickjacking risk. Consider 'frame-ancestors 'none'' or a specific origin allowlist.`,
      evidence: { pageUrl: snap.pageUrl, cspValue: csp },
    });
    return out;
  }

  // Now check XFO. CSP didn't help (no frame-ancestors at all).
  if (xfo === '') {
    out.push({
      severity: 'strict',
      kind: 'frame-options.missing',
      detail: `Page has no X-Frame-Options header AND no Content-Security-Policy frame-ancestors directive. Any origin can iframe this page → clickjacking attacks possible. Add 'X-Frame-Options: SAMEORIGIN' (legacy + universal) OR 'Content-Security-Policy: frame-ancestors 'self'' (modern).`,
      evidence: { pageUrl: snap.pageUrl },
    });
    return out;
  }

  const cls = classifyXfo(xfo);
  if (cls === 'invalid') {
    out.push({
      severity: 'warn',
      kind: 'frame-options.invalid',
      detail: `X-Frame-Options has an unrecognised value '${xfo}'. Browsers treat invalid values as no-protection. Use DENY (no framing at all) or SAMEORIGIN (only your own origin can iframe).`,
      evidence: { pageUrl: snap.pageUrl, xfoValue: xfo },
    });
  }

  return out;
}
