/**
 * networkErrorLogging.ts — Network-Error-Logging (NEL) response-
 * header audit. T76 cycle 65.
 *
 * NEL is a W3C spec (shipped in Chromium since 2018) that
 * extends the Reporting-API to network-level failures. Where
 * CSP / COEP / Document-Policy report POLICY violations, NEL
 * reports TRANSPORT failures the browser sees BEFORE the
 * document even loads:
 *
 *   * TLS handshake failures (cert errors, name mismatches,
 *     SNI failures, expired certs, weak cipher)
 *   * DNS resolution failures (NXDOMAIN, timeout, refused)
 *   * TCP RST / connection-reset before HTTP
 *   * HTTP status codes (when configured)
 *   * Aborted / refused / timeout requests
 *
 * Without NEL, these failures are INVISIBLE — the user sees
 * "this site can't be reached" in their browser, the operator
 * has no signal. With NEL configured, the browser POSTs a
 * structured report to the same Reporting-Endpoints collector
 * that CSP violations land in. This is the network-telemetry
 * half of the supersociety observability stack.
 *
 * Header format (W3C, Structured Fields):
 *
 *   NEL: {"report_to": "default",
 *         "max_age": 2592000,
 *         "include_subdomains": false,
 *         "success_fraction": 0.0,
 *         "failure_fraction": 1.0}
 *
 * Findings:
 *
 *   - nel.missing                   warn
 *     No NEL header. Network-level failures are unreportable.
 *     Pairs with the cycle 31 reportingEndpoints + cycle 63
 *     collector — if observability is the goal, NEL is the
 *     transport-layer companion to CSP-violation reporting.
 *
 *   - nel.invalid                   warn
 *     NEL header is present but not a parseable JSON object.
 *     Browsers ignore — same silent-fail mode as a typo'd
 *     Reporting-Endpoints group name.
 *
 *   - nel.report-to-missing         warn
 *     NEL is set but contains no `report_to` field. Browsers
 *     have nowhere to send reports.
 *
 *   - nel.failure-fraction-zero     warn
 *     `failure_fraction: 0.0` — explicitly opted OUT of
 *     failure reporting. Operator may have a reason
 *     (privacy / data-volume) but it defeats the primary
 *     purpose of NEL.
 *
 *   - nel.max-age-zero              warn
 *     `max_age: 0` — explicit opt-out. Browser drops the
 *     policy immediately. Operator may have a reason; surface
 *     so the choice can be confirmed.
 *
 * Out of scope:
 *   * Localhost (consistent with the response-header family).
 *   * Validation that the `report_to` group is actually
 *     declared in Reporting-Endpoints — that's reportingEndpoints
 *     detector territory; we just check NEL's internal
 *     structure here.
 *   * NEL request_headers / response_headers redaction lists
 *     (advanced field; not common in dogfood targets).
 *
 * Reads from the same `topLevelResponseHeaders` Map as the
 * other response-header detectors. 15th consumer of the
 * shared capture path. Uses the cycle-24 `responseHeader-
 * Detector` helper.
 */

export interface NelFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface NelSnapshot {
  pageUrl: string;
  pageIsLocalhost: boolean;
  /** Raw NEL header value (trimmed), or null. */
  raw: string | null;
  /**
   * Parsed NEL policy. Empty object when header is null or
   * unparseable.
   */
  parsed: {
    report_to?: string;
    max_age?: number;
    include_subdomains?: boolean;
    success_fraction?: number;
    failure_fraction?: number;
  };
  /** True iff the header was present but JSON.parse failed. */
  unparseable: boolean;
}

function isLocalhost(url: string): boolean {
  try {
    const u = new URL(url);
    const h = u.hostname;
    return (
      h === 'localhost' ||
      h === '127.0.0.1' ||
      h === '::1' ||
      h.endsWith('.localhost')
    );
  } catch {
    return false;
  }
}

function getHeader(
  headers: Record<string, string> | undefined,
  name: string,
): string | null {
  if (!headers) return null;
  for (const [k, v] of Object.entries(headers)) {
    if (k.toLowerCase() === name) return v;
  }
  return null;
}

export function buildNelSnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): NelSnapshot {
  const pageIsLocalhost = isLocalhost(pageUrl);
  const rawHeader = getHeader(headers, 'nel');
  const trimmed = rawHeader === null ? null : rawHeader.trim();
  let parsed: NelSnapshot['parsed'] = {};
  let unparseable = false;
  if (trimmed && trimmed.length > 0) {
    try {
      const obj = JSON.parse(trimmed);
      if (obj && typeof obj === 'object' && !Array.isArray(obj)) {
        parsed = obj;
      } else {
        unparseable = true;
      }
    } catch {
      unparseable = true;
    }
  }
  return {
    pageUrl,
    pageIsLocalhost,
    raw: trimmed,
    parsed,
    unparseable,
  };
}

export function detectNelIssues(snap: NelSnapshot): NelFinding[] {
  if (snap.pageIsLocalhost) return [];

  if (snap.raw === null) {
    return [
      {
        severity: 'warn',
        kind: 'nel.missing',
        detail: `No NEL (Network-Error-Logging) header. TLS handshake failures, DNS errors, TCP RSTs, and other pre-HTTP failures are unreportable. Pairs with Reporting-Endpoints — add 'NEL: {"report_to":"default","max_age":2592000,"failure_fraction":1.0}' so the browser POSTs network-level failures to your collector.`,
        evidence: {},
      },
    ];
  }

  if (snap.unparseable) {
    return [
      {
        severity: 'warn',
        kind: 'nel.invalid',
        detail: `NEL header present but not a valid JSON object: '${snap.raw.slice(0, 200)}'. Browsers silently ignore unparseable values. Expected form: '{"report_to":"<group>","max_age":<seconds>,"failure_fraction":<0..1>}'.`,
        evidence: { raw: snap.raw.slice(0, 200) },
      },
    ];
  }

  const out: NelFinding[] = [];

  if (typeof snap.parsed.report_to !== 'string' || !snap.parsed.report_to) {
    out.push({
      severity: 'warn',
      kind: 'nel.report-to-missing',
      detail: `NEL header parses but contains no 'report_to' field (or it's empty). The browser has nowhere to send network-error reports. Add 'report_to': '<Reporting-Endpoints group name>'.`,
      evidence: { parsed: snap.parsed },
    });
  }

  if (snap.parsed.max_age === 0) {
    out.push({
      severity: 'warn',
      kind: 'nel.max-age-zero',
      detail: `NEL 'max_age' is 0 — explicit opt-out. The browser drops the policy immediately on receipt. If this is intentional (decommissioning the collector), confirm; otherwise set 'max_age' to a positive seconds value (recommended: 2592000 = 30 days).`,
      evidence: { max_age: 0 },
    });
  }

  if (snap.parsed.failure_fraction === 0) {
    out.push({
      severity: 'warn',
      kind: 'nel.failure-fraction-zero',
      detail: `NEL 'failure_fraction' is 0 — failure reports are explicitly disabled. The primary purpose of NEL (catching TLS / DNS / TCP failures) is defeated. If privacy / data volume is the concern, sample at 0.01-0.1 instead.`,
      evidence: { failure_fraction: 0 },
    });
  }

  return out;
}
