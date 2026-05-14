/**
 * coop.ts — Cross-Origin-Opener-Policy response-header audit.
 * T76.
 *
 * COOP controls the relationship between the page and any
 * cross-origin browsing context that opens it (or that it
 * opens). Without COOP, `window.opener` provides a script
 * handle to the opener — a malicious tab can read or rewrite
 * its opener's location, watch its navigation history, run
 * timing attacks across origins, or pop a phishing dialog
 * targeting the user's actual logged-in session.
 *
 * COOP is also one of the TWO headers (with COEP) required to
 * enable cross-origin isolation, which gates `SharedArrayBuffer`
 * and the high-resolution timing primitives needed to mitigate
 * Spectre-class side-channel attacks. Modern security-sensitive
 * apps (any banking, payments, healthcare, comms client) should
 * set both.
 *
 * Acceptable values:
 *   * `same-origin`                — strictest. Cross-origin
 *                                    openers/openees lose the
 *                                    opener relationship entirely.
 *   * `same-origin-allow-popups`   — strict for openers, permissive
 *                                    for popups the page itself opens.
 *   * `same-origin-plus-coep`      — newer Chrome shorthand for
 *                                    `same-origin` + COEP combined.
 *   * `noopener-allow-popups`      — newest, opt-in to break
 *                                    the opener handle for popups
 *                                    without enforcing isolation.
 *   * `unsafe-none`                — explicit opt-out of isolation.
 *                                    Browser default if header absent.
 *
 * Findings:
 *
 *   - coop.missing       warn   No header at all → defaults to
 *                               `unsafe-none`. Tab-nabbing surface
 *                               remains; cross-origin isolation
 *                               cannot be enabled.
 *   - coop.unsafe-none   warn   Header explicitly set to
 *                               `unsafe-none`. Operator made the
 *                               choice intentionally — surfaced so
 *                               they can confirm.
 *   - coop.invalid       warn   Value not in the recognised set.
 *                               Browsers ignore unknown values
 *                               (effective `unsafe-none`).
 *
 * Out of scope: localhost (same family as hsts/xframe/referrer/
 * cookie/permissions/csp). http pages: COOP CAN be set on http
 * but offers minimal value when the connection itself is
 * unauthenticated — we still fire the missing-warn though, since
 * the cost of setting it is zero.
 *
 * Reads from the same `topLevelResponseHeaders` Map as the
 * other response-header detectors. SEVENTH consumer of the
 * shared capture path.
 */

export interface CoopFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CoopSnapshot {
  pageUrl: string;
  pageIsLocalhost: boolean;
  /** Raw header value, lowercased + trimmed, or null. */
  raw: string | null;
}

const ACCEPTABLE: ReadonlySet<string> = new Set([
  'same-origin',
  'same-origin-allow-popups',
  'same-origin-plus-coep',
  'noopener-allow-popups',
]);

const RECOGNISED: ReadonlySet<string> = new Set([
  ...ACCEPTABLE,
  'unsafe-none',
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

function getHeader(headers: Record<string, string> | undefined, name: string): string | null {
  if (!headers) return null;
  for (const [k, v] of Object.entries(headers)) {
    if (k.toLowerCase() === name) return v;
  }
  return null;
}

export function buildCoopSnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): CoopSnapshot {
  const pageIsLocalhost = isLocalhost(pageUrl);
  const raw = getHeader(headers, 'cross-origin-opener-policy');
  return {
    pageUrl,
    pageIsLocalhost,
    raw: raw === null ? null : raw.trim().toLowerCase(),
  };
}

export function detectCoopIssues(snap: CoopSnapshot): CoopFinding[] {
  if (snap.pageIsLocalhost) return [];
  if (snap.raw === null) {
    return [{
      severity: 'warn',
      kind: 'coop.missing',
      detail: `No Cross-Origin-Opener-Policy header. Defaults to 'unsafe-none' — cross-origin openers can read window.opener and run tab-nabbing or timing attacks. Cross-origin isolation (required for SharedArrayBuffer + Spectre mitigation) cannot be enabled. Set to 'same-origin' for the strictest protection or 'same-origin-allow-popups' if you open popups.`,
      evidence: {},
    }];
  }
  if (snap.raw === 'unsafe-none') {
    return [{
      severity: 'warn',
      kind: 'coop.unsafe-none',
      detail: `Cross-Origin-Opener-Policy explicitly set to 'unsafe-none'. The opener relationship remains scriptable across origins; cross-origin isolation is disabled. Confirm this is intentional — the supersociety baseline is 'same-origin'.`,
      evidence: { value: snap.raw },
    }];
  }
  if (!RECOGNISED.has(snap.raw)) {
    return [{
      severity: 'warn',
      kind: 'coop.invalid',
      detail: `Cross-Origin-Opener-Policy value '${snap.raw}' is not in the W3C-recognised set ('same-origin', 'same-origin-allow-popups', 'same-origin-plus-coep', 'noopener-allow-popups', 'unsafe-none'). Browsers ignore unknown values and fall back to 'unsafe-none'.`,
      evidence: { value: snap.raw },
    }];
  }
  return [];
}
