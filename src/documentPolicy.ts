/**
 * documentPolicy.ts — Document-Policy response-header audit.
 * T76 cycle 60.
 *
 * Document-Policy is a 2024-shipped W3C header (companion to
 * Permissions-Policy) that lets a document opt INTO additional
 * runtime constraints on its own features. Unlike Permissions-
 * Policy (which controls cross-origin access to powerful APIs
 * like camera, mic, geolocation), Document-Policy controls
 * features within the document itself:
 *
 *   * `document-write=?0` — disable document.write/writeln.
 *     Eliminates a classic DOM-XSS sink that also blocks
 *     async parsing. Modern apps shouldn't need it.
 *
 *   * `force-load-at-top=?1` — disable scroll restoration.
 *     Always load at the top of the page, never restore
 *     scroll position. Predictable UX for spa-like content.
 *
 *   * `unsized-media=?0` — require explicit width/height on
 *     <img> / <video> / <iframe>. Reduces CLS (Cumulative
 *     Layout Shift, a Core Web Vital).
 *
 *   * `oversized-images=?<ratio>` — reject images whose
 *     transmitted dimensions exceed the displayed
 *     dimensions by the given ratio. Catches "1080p hero
 *     image displayed at 200px" perf bugs.
 *
 *   * `js-profiling=?1` — opt INTO the JS Self-Profiling API.
 *     Lets the page collect performance.measureUserAgentSpecificMemory
 *     and JSCpuProfiler samples — observability primitive.
 *
 *   * `frame-loading=eager|lazy` — default iframe loading mode.
 *
 * Header reporting variant `Document-Policy-Report-Only:` is
 * also accepted by the spec for canary deploys.
 *
 * Findings:
 *
 *   - document-policy.missing                   warn
 *     No header. The document operates under default
 *     behaviour (document.write allowed, oversized images
 *     permitted, scroll restoration on, etc). For a
 *     supersociety baseline, set at minimum:
 *       Document-Policy: document-write=?0, force-load-at-top
 *
 *   - document-policy.permits-document-write    warn
 *     Header set but doesn't include `document-write=?0`.
 *     document.write is a parser-blocking DOM-XSS sink;
 *     modern apps should explicitly disable.
 *
 *   - document-policy.invalid                   warn
 *     Header value doesn't parse as Structured Fields
 *     (RFC 8941) dictionary. Browsers reject silently.
 *
 * Out of scope:
 *   * Localhost (consistent with the response-header detector
 *     family — local dev servers rarely set the full security
 *     header suite).
 *   * http pages — the header still works but the page has
 *     bigger problems.
 *
 * Reads from the same `topLevelResponseHeaders` Map as the
 * other response-header detectors. FOURTEENTH consumer of the
 * shared capture path. Uses the cycle-24
 * `responseHeaderDetector` helper.
 */

export interface DocumentPolicyFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface DocumentPolicySnapshot {
  pageUrl: string;
  pageIsLocalhost: boolean;
  /** Raw `Document-Policy` value (trimmed), or null. */
  raw: string | null;
  /**
   * Parsed directives. Each key is lowercase; value is the raw
   * structured-field token (e.g. '?0', '?1', '0.5'). Empty
   * map when header is null or unparseable.
   */
  directives: Record<string, string>;
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

/**
 * Best-effort parse of a Document-Policy header value. The
 * Structured Fields spec defines this as a Dictionary; we
 * implement just enough to support the common forms:
 *   `document-write=?0, force-load-at-top, oversized-images=2.0`
 * is parsed as:
 *   { 'document-write': '?0', 'force-load-at-top': '?1',
 *     'oversized-images': '2.0' }
 *
 * Bare keys imply `?1` (RFC 8941 §3.1.2). Token values are
 * kept as raw strings; we don't validate ranges.
 */
function parseDirectives(raw: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const part of raw.split(',')) {
    const trimmed = part.trim();
    if (!trimmed) continue;
    const eq = trimmed.indexOf('=');
    if (eq === -1) {
      out[trimmed.toLowerCase()] = '?1';
    } else {
      const key = trimmed.slice(0, eq).trim().toLowerCase();
      const value = trimmed.slice(eq + 1).trim();
      if (key) out[key] = value;
    }
  }
  return out;
}

export function buildDocumentPolicySnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): DocumentPolicySnapshot {
  const pageIsLocalhost = isLocalhost(pageUrl);
  const rawHeader = getHeader(headers, 'document-policy');
  const trimmed = rawHeader === null ? null : rawHeader.trim();
  return {
    pageUrl,
    pageIsLocalhost,
    raw: trimmed,
    directives: trimmed ? parseDirectives(trimmed) : {},
  };
}

export function detectDocumentPolicyIssues(
  snap: DocumentPolicySnapshot,
): DocumentPolicyFinding[] {
  if (snap.pageIsLocalhost) return [];

  if (snap.raw === null) {
    return [
      {
        severity: 'warn',
        kind: 'document-policy.missing',
        detail: `No Document-Policy header. The document operates under default behaviour: document.write is allowed (DOM-XSS sink that blocks async parsing), oversized images are permitted (CLS hit), scroll restoration is on (unpredictable UX on back-nav). Set at minimum 'Document-Policy: document-write=?0, force-load-at-top'.`,
        evidence: {},
      },
    ];
  }

  if (Object.keys(snap.directives).length === 0) {
    return [
      {
        severity: 'warn',
        kind: 'document-policy.invalid',
        detail: `Document-Policy header set to '${snap.raw}' but no recognisable directives parsed. Expected Structured Fields Dictionary form (RFC 8941) — e.g. 'document-write=?0, force-load-at-top'. Browsers silently reject the entire header on parse failure.`,
        evidence: { raw: snap.raw },
      },
    ];
  }

  const out: DocumentPolicyFinding[] = [];

  // Check that document.write is explicitly disabled. Bare key
  // `document-write` (without =) means `?1` = ENABLED, which is
  // the default behaviour — the operator set the header but
  // left this dangerous default on. Same for explicit `?1`.
  const docWrite = snap.directives['document-write'];
  if (docWrite === undefined) {
    out.push({
      severity: 'warn',
      kind: 'document-policy.permits-document-write',
      detail: `Document-Policy is set but does not include 'document-write=?0'. document.write is a parser-blocking DOM-XSS sink; modern apps should explicitly disable. Add 'document-write=?0' to the policy.`,
      evidence: { raw: snap.raw, directives: snap.directives },
    });
  } else if (docWrite === '?1') {
    out.push({
      severity: 'warn',
      kind: 'document-policy.permits-document-write',
      detail: `Document-Policy explicitly permits document.write ('document-write=?1'). Modern apps don't need it; the default-off stance is safer. Change to 'document-write=?0' unless you've audited every document.write call site.`,
      evidence: { raw: snap.raw, directives: snap.directives },
    });
  }

  return out;
}
