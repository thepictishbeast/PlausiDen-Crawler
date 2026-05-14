/**
 * reportingEndpoints.ts — Reporting API endpoint configuration
 * audit. T76 cycle 31.
 *
 * The Reporting API (W3C, browser-shipping since 2018,
 * `Reporting-Endpoints` header standardised 2023) is the
 * modern transport for browser-emitted security telemetry:
 *
 *   * CSP violation reports (script blocked, nonce mismatch,
 *     trusted-types violation).
 *   * COEP violation reports (cross-origin embed without CORP).
 *   * CORP violation reports.
 *   * Document-Policy violation reports.
 *   * Crash reports (page crashed via OOM, GPU loss).
 *   * Intervention reports (browser overrode the page —
 *     blocked autoplay, paused service worker).
 *   * Deprecation reports (page used a feature the browser is
 *     about to remove).
 *
 * Without endpoints configured, ALL these reports are lost.
 * The page's own security signal can't reach the operator —
 * a CSP that fires daily on production has zero ops
 * visibility, and the Sentinel-style supersociety
 * "observability layer" is incomplete.
 *
 * Modern best practice (2024+):
 *
 *   Reporting-Endpoints: csp-default="https://example.com/csp",
 *                        coep-violations="https://example.com/coep",
 *                        crash-reports="https://example.com/crashes"
 *
 *   Content-Security-Policy: ...; report-to csp-default
 *
 * The legacy `Report-To` header (a JSON-shaped value) is
 * deprecated; modern browsers prefer `Reporting-Endpoints`.
 *
 * Findings:
 *
 *   - reporting.no-endpoints                       warn
 *     Neither `Reporting-Endpoints` nor `Report-To` header is
 *     present. ALL browser-emitted security reports are lost.
 *
 *   - reporting.report-to-only                     warn
 *     Legacy `Report-To` header set but no modern
 *     `Reporting-Endpoints`. Modern browsers prefer
 *     Reporting-Endpoints; they may emit deprecation warnings
 *     and/or stop honouring Report-To in future versions.
 *
 *   - reporting.csp-report-uri-no-endpoints        warn
 *     CSP header includes `report-uri` or `report-to` directive
 *     but no Reporting-Endpoints / Report-To header is set up
 *     to receive them. Reports go nowhere.
 *
 *   - reporting.invalid                            warn
 *     Reporting-Endpoints header is present but no valid
 *     `name=URL` pair could be parsed. Browsers ignore.
 *
 * Out of scope:
 *   * Verifying the endpoint URLs actually accept reports
 *     (would require sending test reports — too invasive for
 *     a passive audit).
 *   * Endpoint TLS / origin / CORS checks (could be a future
 *     finding kind once real-world dogfood shows the need).
 *   * Per-feature opt-ins (Report-To with multiple groups for
 *     different feature buckets).
 *   * Localhost (consistent with the response-header detector
 *     family).
 *
 * Reads from the same `topLevelResponseHeaders` Map as the
 * other 11 response-header detectors. TWELFTH consumer of the
 * shared capture path. Uses the cycle-24 `responseHeader-
 * Detector` helper.
 */

export interface ReportingEndpointsFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface ParsedReportingEndpoint {
  name: string;
  url: string;
}

export interface ReportingEndpointsSnapshot {
  pageUrl: string;
  pageIsLocalhost: boolean;
  /** Raw Reporting-Endpoints value, or null. */
  rawReportingEndpoints: string | null;
  /** Parsed Reporting-Endpoints entries. */
  endpoints: ParsedReportingEndpoint[];
  /** True iff Reporting-Endpoints was present but unparseable. */
  reportingEndpointsUnparseable: boolean;
  /** Raw Report-To value, or null. Legacy. */
  rawReportTo: string | null;
  /** Raw Content-Security-Policy value (for cross-checking
   *  report-uri / report-to references), or null. */
  rawCsp: string | null;
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
 * Parse Reporting-Endpoints. Syntax (RFC 8941 structured
 * fields, a Dictionary of name → string-value):
 *
 *   csp-default="https://example.com/csp", crash-reports="https://example.com/crashes"
 *
 * Each value is a sf-string in double quotes. Token names are
 * lowercase; URLs must be HTTPS for cross-origin (W3C spec
 * requirement) but we don't enforce that here — the
 * `reporting.invalid` finding catches structural defects;
 * URL-scheme validity could be a future finding kind.
 */
function parseReportingEndpoints(raw: string): ParsedReportingEndpoint[] {
  const out: ParsedReportingEndpoint[] = [];
  for (const part of raw.split(',')) {
    const trimmed = part.trim();
    if (!trimmed) continue;
    const eq = trimmed.indexOf('=');
    if (eq <= 0) continue;
    const name = trimmed.slice(0, eq).trim();
    let value = trimmed.slice(eq + 1).trim();
    if (!name) continue;
    if (value.startsWith('"') && value.endsWith('"') && value.length >= 2) {
      value = value.slice(1, -1);
    }
    if (!value) continue;
    out.push({ name, url: value });
  }
  return out;
}

function cspMentionsReporting(rawCsp: string | null): boolean {
  if (rawCsp === null) return false;
  return /\b(?:report-uri|report-to)\b/i.test(rawCsp);
}

export function buildReportingEndpointsSnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): ReportingEndpointsSnapshot {
  const pageIsLocalhost = isLocalhost(pageUrl);
  const rawReportingEndpoints = getHeader(headers, 'reporting-endpoints');
  const endpoints = rawReportingEndpoints === null ? [] : parseReportingEndpoints(rawReportingEndpoints);
  const reportingEndpointsUnparseable =
    rawReportingEndpoints !== null && rawReportingEndpoints.trim().length > 0 && endpoints.length === 0;
  const rawReportTo = getHeader(headers, 'report-to');
  const rawCsp = getHeader(headers, 'content-security-policy');
  return {
    pageUrl, pageIsLocalhost,
    rawReportingEndpoints, endpoints, reportingEndpointsUnparseable,
    rawReportTo, rawCsp,
  };
}

export function detectReportingEndpointsIssues(
  snap: ReportingEndpointsSnapshot,
): ReportingEndpointsFinding[] {
  if (snap.pageIsLocalhost) return [];
  const out: ReportingEndpointsFinding[] = [];

  const hasModern = snap.endpoints.length > 0;
  const hasLegacy = snap.rawReportTo !== null && snap.rawReportTo.trim().length > 0;

  if (snap.reportingEndpointsUnparseable) {
    out.push({
      severity: 'warn',
      kind: 'reporting.invalid',
      detail: `Reporting-Endpoints header is set but no valid 'name=URL' pair could be parsed. Browsers ignore unparseable values — the entire reporting pipeline is silently broken. Header value: '${(snap.rawReportingEndpoints ?? '').slice(0, 200)}'.`,
      evidence: { raw: (snap.rawReportingEndpoints ?? '').slice(0, 200) },
    });
  }

  if (!hasModern && !hasLegacy) {
    out.push({
      severity: 'warn',
      kind: 'reporting.no-endpoints',
      detail: `Neither Reporting-Endpoints nor Report-To header is present. ALL browser-emitted security reports (CSP violations, COEP violations, crash reports, intervention reports, deprecation warnings) are LOST — the page's own telemetry has nowhere to go. Add a 'Reporting-Endpoints: csp-default="https://your-collector.example/csp"' header and reference it from CSP via 'report-to csp-default'.`,
      evidence: {},
    });
  } else if (!hasModern && hasLegacy) {
    out.push({
      severity: 'warn',
      kind: 'reporting.report-to-only',
      detail: `Legacy Report-To header set but no modern Reporting-Endpoints. Modern browsers prefer Reporting-Endpoints (W3C 2023); they may emit deprecation warnings and stop honouring Report-To in future versions. Migrate.`,
      evidence: { rawReportTo: (snap.rawReportTo ?? '').slice(0, 200) },
    });
  }

  if (cspMentionsReporting(snap.rawCsp) && !hasModern && !hasLegacy) {
    out.push({
      severity: 'warn',
      kind: 'reporting.csp-report-uri-no-endpoints',
      detail: `Content-Security-Policy includes 'report-uri' or 'report-to' directive but no Reporting-Endpoints / Report-To header is set up to receive the reports. They go nowhere. Pair the CSP directive with a Reporting-Endpoints header.`,
      evidence: { rawCsp: (snap.rawCsp ?? '').slice(0, 200) },
    });
  }

  return out;
}
