/**
 * coep.ts — Cross-Origin-Embedder-Policy response-header audit.
 * T76.
 *
 * COEP, paired with COOP=`same-origin`, enables `crossOriginIsolated`
 * document state — the modern browser primitive that gates
 * `SharedArrayBuffer`, high-resolution `performance.now()`,
 * and `performance.measureUserAgentSpecificMemory()`. Without
 * cross-origin isolation, Spectre-class side-channel attacks
 * can leak data from co-tenant origins inside the same browser
 * process.
 *
 * COEP also restricts WHICH cross-origin sub-resources the
 * browser will load: with `require-corp`, every cross-origin
 * fetched asset must explicitly opt-in via Cross-Origin-Resource-
 * Policy. With `credentialless`, cross-origin requests are
 * allowed but credentials are stripped (cookies, client certs,
 * basic-auth — the resource is fetched as if anonymous).
 *
 * Acceptable values:
 *   * `require-corp`     — strictest. All cross-origin
 *                          embeds need CORP.
 *   * `credentialless`   — permissive but credential-stripped;
 *                          newer (Chrome 96+, Firefox 119+).
 *   * `unsafe-none`      — explicit opt-out of isolation.
 *                          Browser default if header absent.
 *
 * Findings:
 *
 *   - coep.missing       warn   No header at all → defaults to
 *                               `unsafe-none`. Cross-origin
 *                               isolation cannot be enabled.
 *                               SharedArrayBuffer and high-res
 *                               timers stay disabled.
 *   - coep.unsafe-none   warn   Header explicitly set to
 *                               `unsafe-none`. Operator made the
 *                               choice — surfaced for confirmation.
 *   - coep.invalid       warn   Value not in the recognised set.
 *                               Browsers ignore (effective
 *                               `unsafe-none`).
 *
 * Out of scope: localhost (same family). http pages still warn
 * for missing — the cost of setting it is zero and the signal
 * is useful even before TLS is enforced.
 *
 * Reads from the same `topLevelResponseHeaders` Map as the
 * other response-header detectors. EIGHTH consumer of the
 * shared capture path.
 */

export interface CoepFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CoepSnapshot {
  pageUrl: string;
  pageIsLocalhost: boolean;
  raw: string | null;
}

const ACCEPTABLE: ReadonlySet<string> = new Set([
  'require-corp',
  'credentialless',
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

export function buildCoepSnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): CoepSnapshot {
  const pageIsLocalhost = isLocalhost(pageUrl);
  const raw = getHeader(headers, 'cross-origin-embedder-policy');
  return {
    pageUrl,
    pageIsLocalhost,
    raw: raw === null ? null : raw.trim().toLowerCase(),
  };
}

export function detectCoepIssues(snap: CoepSnapshot): CoepFinding[] {
  if (snap.pageIsLocalhost) return [];
  if (snap.raw === null) {
    return [{
      severity: 'warn',
      kind: 'coep.missing',
      detail: `No Cross-Origin-Embedder-Policy header. Defaults to 'unsafe-none' — cross-origin isolation cannot be enabled, so SharedArrayBuffer + high-resolution timers stay disabled and Spectre-class side-channel mitigations are unavailable. Set to 'require-corp' (strictest, requires every cross-origin sub-resource to opt in via CORP) or 'credentialless' (newer; allows cross-origin embeds without credentials).`,
      evidence: {},
    }];
  }
  if (snap.raw === 'unsafe-none') {
    return [{
      severity: 'warn',
      kind: 'coep.unsafe-none',
      detail: `Cross-Origin-Embedder-Policy explicitly set to 'unsafe-none'. Cross-origin isolation is disabled. Confirm this is intentional — the supersociety baseline is 'require-corp'.`,
      evidence: { value: snap.raw },
    }];
  }
  if (!RECOGNISED.has(snap.raw)) {
    return [{
      severity: 'warn',
      kind: 'coep.invalid',
      detail: `Cross-Origin-Embedder-Policy value '${snap.raw}' is not in the W3C-recognised set ('require-corp', 'credentialless', 'unsafe-none'). Browsers ignore unknown values and fall back to 'unsafe-none'.`,
      evidence: { value: snap.raw },
    }];
  }
  return [];
}
