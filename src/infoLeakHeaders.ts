/**
 * infoLeakHeaders.ts — opsec hygiene audit for response
 * headers that disclose server software, framework versions,
 * or debugging state. T76.
 *
 * Threat model: an adversary mapping a target site uses
 * version-disclosure to cross-reference public CVE databases
 * (NVD, GitHub Advisories, ExploitDB) and find the exact
 * pre-built exploit modules to use against the target. A
 * Server header reading `nginx/1.20.1` reveals which CVEs
 * apply, which patch levels are missing, and which auxiliary
 * intelligence (deployment date inferable from version) is
 * available. Removing or generalising the header forces the
 * adversary to enumerate the surface manually — significantly
 * raising the cost of opportunistic attacks and slowing
 * targeted ones.
 *
 * The page WORKS without these headers — they're pure
 * disclosure, no functional value to legitimate users. Best
 * practice: strip or set to a generic value (`Server: web`).
 *
 * Findings (all warn — opsec hygiene, not exploitability):
 *
 *   - info-leak.server-version
 *     Server header includes a version number. Examples:
 *     `nginx/1.20.1`, `Apache/2.4.41 (Ubuntu)`, `Microsoft-
 *     IIS/10.0`. The bare product name (`Server: nginx`) is
 *     less harmful — the version is what enables CVE lookup.
 *
 *   - info-leak.x-powered-by
 *     Any X-Powered-By header. Common emitters: PHP
 *     (PHP/7.4.3), Express (Express), ASP.NET (ASP.NET),
 *     Laravel (Laravel), Symfony, etc. Almost always
 *     auto-emitted — operator just needs to disable it in
 *     framework config.
 *
 *   - info-leak.x-aspnet-version
 *     X-AspNet-Version header. Microsoft-specific; reveals
 *     CLR / .NET version.
 *
 *   - info-leak.x-aspnetmvc-version
 *     X-AspNetMvc-Version header. Sister to x-aspnet-version.
 *
 *   - info-leak.x-runtime
 *     X-Runtime header. Rails / Sinatra / Django emit it with
 *     request handling time — useful for the operator to debug
 *     but exposes performance characteristics that aid
 *     timing-attack reconnaissance.
 *
 *   - info-leak.x-debug-token
 *     X-Debug-Token / X-Debug-Token-Link headers. Symfony
 *     web-profiler exposure; if these reach production the
 *     debug toolbar is also accessible — full route map +
 *     SQL queries + cache state visible.
 *
 *   - info-leak.via
 *     Via header. RFC 7230; legitimate intermediate-proxy
 *     trace, but in production it usually leaks internal
 *     hostnames or proxy software versions. Surfaced as warn.
 *
 *   - info-leak.x-generator
 *     X-Generator header. Drupal / WordPress / Hugo / Jekyll
 *     emit it identifying the CMS + version. Same threat
 *     model as Server.
 *
 * Out of scope (legitimately needed in production):
 *   * Server header WITHOUT a version (`Server: cloudflare`,
 *     `Server: web`). Some routing infra needs Server set for
 *     debugging; the bare product name without version is
 *     acceptable.
 *   * X-Frame-Options, Strict-Transport-Security, etc. — these
 *     are PROTECTIVE security headers; their own detectors
 *     handle them.
 *   * Cache-Control / Vary / ETag / Last-Modified — caching
 *     headers, separate concern (queued).
 *   * Localhost (consistent with the response-header detector
 *     family).
 *
 * Reads from the same `topLevelResponseHeaders` Map as the
 * other response-header detectors. NINTH consumer of the
 * shared capture path. Uses the `responseHeaderDetector`
 * helper in main.ts for wiring.
 */

export interface InfoLeakFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface InfoLeakSnapshot {
  pageUrl: string;
  pageIsLocalhost: boolean;
  /** Map of lowercased header name → raw value, for the headers we audit. */
  headers: Record<string, string>;
}

const AUDITED_HEADERS: ReadonlySet<string> = new Set([
  'server',
  'x-powered-by',
  'x-aspnet-version',
  'x-aspnetmvc-version',
  'x-runtime',
  'x-debug-token',
  'x-debug-token-link',
  'via',
  'x-generator',
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

export function buildInfoLeakSnapshot(
  pageUrl: string,
  rawHeaders: Record<string, string> | undefined,
): InfoLeakSnapshot {
  const pageIsLocalhost = isLocalhost(pageUrl);
  const headers: Record<string, string> = {};
  if (rawHeaders) {
    for (const [k, v] of Object.entries(rawHeaders)) {
      const lower = k.toLowerCase();
      if (AUDITED_HEADERS.has(lower)) {
        headers[lower] = v;
      }
    }
  }
  return { pageUrl, pageIsLocalhost, headers };
}

/**
 * Heuristic: does this Server-header value contain a version
 * number? Versions are usually `Product/X.Y.Z` or `Product
 * X.Y.Z` (some emitters use space). We require at least
 * <digit>.<digit> to count — bare product names like
 * `Server: nginx` are NOT flagged.
 */
function hasVersionToken(value: string): boolean {
  return /\d+\.\d+/.test(value);
}

export function detectInfoLeakIssues(snap: InfoLeakSnapshot): InfoLeakFinding[] {
  if (snap.pageIsLocalhost) return [];
  const out: InfoLeakFinding[] = [];

  const server = snap.headers['server'];
  if (server !== undefined && hasVersionToken(server)) {
    out.push({
      severity: 'warn',
      kind: 'info-leak.server-version',
      detail: `Server header reveals version: '${server.slice(0, 200)}'. Enables CVE lookup against the exact build. Strip the header (nginx: 'server_tokens off;'; Apache: 'ServerTokens Prod' + 'ServerSignature Off') or set to a generic value ('Server: web').`,
      evidence: { value: server.slice(0, 200) },
    });
  }

  const xpb = snap.headers['x-powered-by'];
  if (xpb !== undefined) {
    out.push({
      severity: 'warn',
      kind: 'info-leak.x-powered-by',
      detail: `X-Powered-By header present: '${xpb.slice(0, 200)}'. Identifies the application framework — almost always auto-emitted and almost never needed in production. Disable in framework config (PHP: 'expose_php=Off'; Express: 'app.disable("x-powered-by")'; ASP.NET: '<httpRuntime enableVersionHeader="false">').`,
      evidence: { value: xpb.slice(0, 200) },
    });
  }

  const xav = snap.headers['x-aspnet-version'];
  if (xav !== undefined) {
    out.push({
      severity: 'warn',
      kind: 'info-leak.x-aspnet-version',
      detail: `X-AspNet-Version header reveals CLR/.NET version: '${xav.slice(0, 200)}'. Disable via '<httpRuntime enableVersionHeader="false">' in web.config.`,
      evidence: { value: xav.slice(0, 200) },
    });
  }

  const xamv = snap.headers['x-aspnetmvc-version'];
  if (xamv !== undefined) {
    out.push({
      severity: 'warn',
      kind: 'info-leak.x-aspnetmvc-version',
      detail: `X-AspNetMvc-Version header reveals MVC framework version: '${xamv.slice(0, 200)}'. Disable in Global.asax: 'MvcHandler.DisableMvcResponseHeader = true;'.`,
      evidence: { value: xamv.slice(0, 200) },
    });
  }

  const xr = snap.headers['x-runtime'];
  if (xr !== undefined) {
    out.push({
      severity: 'warn',
      kind: 'info-leak.x-runtime',
      detail: `X-Runtime header present: '${xr.slice(0, 200)}'. Rails/Sinatra/Django emit per-request handling time, useful for operator debugging but reveals performance characteristics that aid timing-attack reconnaissance. Strip in production middleware.`,
      evidence: { value: xr.slice(0, 200) },
    });
  }

  if (snap.headers['x-debug-token'] !== undefined || snap.headers['x-debug-token-link'] !== undefined) {
    const v = snap.headers['x-debug-token'] ?? snap.headers['x-debug-token-link'];
    out.push({
      severity: 'warn',
      kind: 'info-leak.x-debug-token',
      detail: `X-Debug-Token / X-Debug-Token-Link header present: '${v.slice(0, 200)}'. Symfony web-profiler exposure — if this reached production, the debug toolbar is ALSO accessible (full route map, SQL query log, cache state). Disable the WebProfilerBundle in prod.`,
      evidence: { value: v.slice(0, 200) },
    });
  }

  const via = snap.headers['via'];
  if (via !== undefined) {
    out.push({
      severity: 'warn',
      kind: 'info-leak.via',
      detail: `Via header present: '${via.slice(0, 200)}'. Legitimate intermediate-proxy trace per RFC 7230, but in production it usually leaks internal hostnames or proxy software versions. Strip at the edge.`,
      evidence: { value: via.slice(0, 200) },
    });
  }

  const xgen = snap.headers['x-generator'];
  if (xgen !== undefined) {
    out.push({
      severity: 'warn',
      kind: 'info-leak.x-generator',
      detail: `X-Generator header reveals CMS/SSG: '${xgen.slice(0, 200)}'. Drupal/WordPress/Hugo/Jekyll auto-emit. Same CVE-targeting threat as the Server header. Strip via web-server config or CMS plugin.`,
      evidence: { value: xgen.slice(0, 200) },
    });
  }

  return out;
}
