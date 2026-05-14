/**
 * permissionsPolicy.ts — Permissions-Policy response-header
 * audit. T76.
 *
 * The Permissions-Policy header (W3C; formerly Feature-Policy)
 * gates which browser APIs the page AND any embedded iframes
 * can use. Without the header — or with overly permissive
 * directives — third-party scripts and iframes inherit ambient
 * permission to access:
 *
 *   * camera         — surveillance vector. JS can request video.
 *   * microphone     — same, audio.
 *   * geolocation    — physical-location leak.
 *   * payment        — Payment Request API (financial data).
 *   * usb / serial /
 *     midi / hid     — physical-device access; unique fingerprint
 *                      surface even if user denies.
 *   * accelerometer /
 *     gyroscope /
 *     magnetometer   — high-resolution device-motion fingerprinting,
 *                      side-channel for keystroke recovery on mobile.
 *   * display-capture / screen-wake-lock — surveillance, power-cost
 *                      attacks, screen-content recording.
 *
 * Browsers default each feature to `*` (allowed for the page
 * AND every embedded iframe) when no policy is set. Modern best
 * practice: deny by default, allow per-feature only as needed.
 *
 * Syntax: comma-separated `feature=allowlist` directives, where
 * `allowlist` is a parenthesized list:
 *   * `()`                          — deny entirely (best)
 *   * `(self)`                      — page itself only (good)
 *   * `(self "https://example.com")` — page + specific origin
 *   * `*`                           — page + all iframes (bad)
 *
 * Findings:
 *
 *   - permissions-policy.missing                warn
 *     No Permissions-Policy header. All features default to `*`
 *     — every embedded iframe can use camera/mic/geo/payment/etc.
 *
 *   - permissions-policy.allow-all-<feature>    strict
 *     A high-risk feature (camera / microphone / geolocation /
 *     payment / usb / serial / midi / hid) is set to `*` —
 *     explicitly allowing every embedded iframe. The strictest
 *     finding — operator either misunderstood the directive or
 *     forgot to restrict it.
 *
 *   - permissions-policy.invalid                warn
 *     Header value couldn't be parsed (no `=` separators, only
 *     unknown features, malformed allowlist). Browsers ignore
 *     unparseable directives → effectively missing.
 *
 * Out of scope (consistent with the response-header detector
 * family doctrine):
 *   * http pages — Permissions-Policy is honored on http but
 *     hostile networks can strip it; HSTS is the prerequisite.
 *     The detector still fires on http because there is no
 *     downside to setting the header over http (unlike Secure
 *     cookies which can't apply); but the localhost exemption
 *     stays.
 *   * Localhost / 127.0.0.1 / *.localhost — fixture-mode
 *     short-circuit, same family as hsts/xframe/referrer/cookie.
 *
 * Reads from the same `topLevelResponseHeaders` Map as hsts +
 * xFrameOptions + referrerPolicy + cookieSecurity — FIFTH
 * consumer of the shared capture path. Pattern-extraction
 * (`multiValueHeaderDetector`) remains deferred — Permissions-
 * Policy IS multi-directive (comma-separated `feature=allowlist`)
 * but each directive's allowlist parsing is feature-dependent
 * enough that extracting now would be premature. After a SIXTH
 * multi-value header detector lands (probable: full CSP), the
 * shape will be clearer.
 */

export interface PermissionsPolicyFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface ParsedDirective {
  feature: string;
  /**
   * Raw allowlist text inside the parens (or `*` for unparenned
   * star). Empty string for `()`.
   */
  allowlistRaw: string;
  /** True iff the allowlist is `*` — every iframe allowed. */
  isAllowAll: boolean;
  /** True iff the allowlist is `()` — denied for everyone. */
  isDeny: boolean;
  /** True iff `self` appears in the allowlist. */
  hasSelf: boolean;
  /** Origins beyond `self` listed in the allowlist (quoted). */
  origins: string[];
}

export interface PermissionsPolicySnapshot {
  pageUrl: string;
  pageIsLocalhost: boolean;
  /** Raw header value, or null if absent. */
  raw: string | null;
  /** Parsed directives in declaration order. Empty if no header. */
  directives: ParsedDirective[];
  /**
   * True iff the header was present but unparseable into ANY
   * directives. Distinguishes "missing" from "garbage".
   */
  unparseable: boolean;
}

/**
 * Features the detector classifies as high-risk for the strict
 * `allow-all-<feature>` finding. Drawn from the W3C
 * Permissions-Policy spec's privacy-impacting features list +
 * payment + device-access APIs known to enable cross-origin
 * fingerprinting.
 *
 * `accelerometer`, `gyroscope`, `magnetometer` are intentionally
 * INCLUDED — even though websites use them for legitimate
 * orientation features, the side-channel literature (TouchLogger,
 * AccessLogger, etc.) proves they enable keystroke recovery on
 * mobile when allowed cross-origin.
 */
const HIGH_RISK_FEATURES: ReadonlySet<string> = new Set([
  'camera',
  'microphone',
  'geolocation',
  'payment',
  'usb',
  'serial',
  'midi',
  'hid',
  'bluetooth',
  'accelerometer',
  'gyroscope',
  'magnetometer',
  'display-capture',
  'screen-wake-lock',
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

/**
 * Parse one `feature=allowlist` directive.
 *
 * Examples:
 *   `camera=()`                          → deny
 *   `camera=*`                           → allow-all
 *   `camera=(self)`                      → self only
 *   `camera=(self "https://e.com")`      → self + origin
 *   `camera=("https://e.com")`           → origin only
 *
 * Returns null on malformed input (no `=`, no value at all).
 */
function parseDirective(raw: string): ParsedDirective | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  const eq = trimmed.indexOf('=');
  if (eq <= 0) return null;
  const feature = trimmed.slice(0, eq).trim().toLowerCase();
  if (!feature) return null;
  const value = trimmed.slice(eq + 1).trim();
  if (!value) return null;

  if (value === '*') {
    return { feature, allowlistRaw: '*', isAllowAll: true, isDeny: false, hasSelf: false, origins: [] };
  }
  // Must be parenthesized.
  if (!value.startsWith('(') || !value.endsWith(')')) {
    // The W3C spec also allows `feature=self` shorthand without
    // parens. Treat as self-only.
    if (value.toLowerCase() === 'self') {
      return { feature, allowlistRaw: 'self', isAllowAll: false, isDeny: false, hasSelf: true, origins: [] };
    }
    return null;
  }
  const inner = value.slice(1, -1).trim();
  if (inner === '') {
    return { feature, allowlistRaw: '', isAllowAll: false, isDeny: true, hasSelf: false, origins: [] };
  }
  // Tokenise on whitespace; quoted strings are origins.
  const tokens = inner.split(/\s+/);
  let hasSelf = false;
  const origins: string[] = [];
  let allowAll = false;
  for (const t of tokens) {
    if (t === '*') {
      allowAll = true;
    } else if (t.toLowerCase() === 'self') {
      hasSelf = true;
    } else if ((t.startsWith('"') && t.endsWith('"')) || (t.startsWith("'") && t.endsWith("'"))) {
      origins.push(t.slice(1, -1));
    } else {
      // Bare origin (non-spec but common in the wild).
      origins.push(t);
    }
  }
  return { feature, allowlistRaw: inner, isAllowAll: allowAll, isDeny: false, hasSelf, origins };
}

function getHeader(headers: Record<string, string> | undefined, name: string): string | null {
  if (!headers) return null;
  for (const [k, v] of Object.entries(headers)) {
    if (k.toLowerCase() === name) return v;
  }
  return null;
}

export function buildPermissionsPolicySnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): PermissionsPolicySnapshot {
  const pageIsLocalhost = isLocalhost(pageUrl);
  const raw = getHeader(headers, 'permissions-policy');
  if (raw === null) {
    return { pageUrl, pageIsLocalhost, raw: null, directives: [], unparseable: false };
  }
  // Comma-split is correct for Permissions-Policy: per W3C the
  // allowlist itself never contains a comma at top level
  // (origins are quoted+space-separated). Empty directives are
  // tolerated by browsers (e.g. trailing comma).
  const parts = raw.split(',').map((s) => s.trim()).filter(Boolean);
  const directives: ParsedDirective[] = [];
  for (const p of parts) {
    const d = parseDirective(p);
    if (d) directives.push(d);
  }
  // Header present but no directive parsed → garbage.
  const unparseable = parts.length > 0 && directives.length === 0;
  return { pageUrl, pageIsLocalhost, raw, directives, unparseable };
}

export function detectPermissionsPolicyIssues(
  snap: PermissionsPolicySnapshot,
): PermissionsPolicyFinding[] {
  if (snap.pageIsLocalhost) return [];
  const out: PermissionsPolicyFinding[] = [];

  if (snap.raw === null) {
    out.push({
      severity: 'warn',
      kind: 'permissions-policy.missing',
      detail: `No Permissions-Policy header on this page. Every browser API (camera, microphone, geolocation, payment, USB, serial, MIDI, accelerometer, etc.) defaults to '*' — every embedded iframe inherits ambient permission. Set the header to deny each high-risk feature you don't use, e.g. 'Permissions-Policy: camera=(), microphone=(), geolocation=(), payment=()'.`,
      evidence: {},
    });
    return out;
  }

  if (snap.unparseable) {
    out.push({
      severity: 'warn',
      kind: 'permissions-policy.invalid',
      detail: `Permissions-Policy header is set but couldn't be parsed into any valid directive. Browsers ignore unparseable values, so the header has no effect. Header value: '${snap.raw.slice(0, 200)}'.`,
      evidence: { raw: snap.raw.slice(0, 200) },
    });
    return out;
  }

  // Per-directive checks.
  for (const d of snap.directives) {
    if (!HIGH_RISK_FEATURES.has(d.feature)) continue;
    if (d.isAllowAll) {
      out.push({
        severity: 'strict',
        kind: `permissions-policy.allow-all-${d.feature}`,
        detail: `Permissions-Policy directive '${d.feature}=${d.allowlistRaw}' explicitly allows every embedded iframe to use a high-risk feature. Restrict to '${d.feature}=()' (denied) or '${d.feature}=(self)' (page-only) unless an embed genuinely needs it.`,
        evidence: { feature: d.feature, allowlist: d.allowlistRaw },
      });
    }
  }

  // Detect HIGH_RISK features OMITTED from the policy. Because
  // the browser default is `*`, an explicit policy that lists
  // some features but not (e.g.) `camera` leaves camera at `*`.
  // We surface this so the operator knows to be exhaustive.
  const declaredFeatures = new Set(snap.directives.map((d) => d.feature));
  const omittedHighRisk: string[] = [];
  for (const f of HIGH_RISK_FEATURES) {
    if (!declaredFeatures.has(f)) omittedHighRisk.push(f);
  }
  if (omittedHighRisk.length > 0 && snap.directives.length > 0) {
    out.push({
      severity: 'warn',
      kind: 'permissions-policy.high-risk-omitted',
      detail: `Permissions-Policy declares ${snap.directives.length} directive(s) but omits ${omittedHighRisk.length} high-risk feature(s) which therefore default to '*'. Add restrictions for: ${omittedHighRisk.join(', ')}.`,
      evidence: { omitted: omittedHighRisk, declaredCount: snap.directives.length },
    });
  }

  return out;
}
