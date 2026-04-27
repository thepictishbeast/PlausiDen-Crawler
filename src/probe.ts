/**
 * Adversarial URL-parameter probe.
 *
 * The discover module exhaustively walks a site read-only — every link,
 * every safe button, full aria + screenshot per page. That catches
 * regressions in what's REACHABLE. It does not catch what happens when
 * a parameter the server expects is malformed: the canonical Bug #33
 * shape was `/api/verify/:hash` accepting any 16-128 char string and
 * passing a null-byte payload straight to Postgres, which threw a 500.
 *
 * runProbe takes the URLs the discover step already harvested (read
 * from `${outDir}/discover-pages.json`) plus any explicitly listed in
 * the journey, classifies their path segments as int/hex/UUID, and
 * fires a small fixed set of malformed variants per template:
 *
 *   int  → -1, 0, INT32_OVERFLOW, BIGNUM, abc, null-byte, traversal, …
 *   hex  → empty, non-hex same-length, null-byte mid/suffix, under-min,
 *          over-max, oversize 10k, uppercase non-hex, traversal
 *   uuid → all-zero, all-f, no-hyphens, non-uuid, null-byte boundaries
 *
 * Findings:
 *   - 5xx response (server-side fault — Bug #33 class)
 *   - 200 on plainly malformed input (acceptance signal — auth bypass risk)
 *   - request error / timeout (DOS surface, slow-pole)
 *   - body echoes the injected variant verbatim (XSS / log-injection seed)
 *   - body contains a known hash prefix (Bug #28/#30/#32 class — partial
 *     entropy leak from a different audit surface)
 *
 * Read-only by default — only fires GETs, never submits. Operator must
 * still scope this away from production: a single 5xx-on-fuzz can mask
 * itself in audit logs at scale, and even GETs against rate-limited
 * endpoints can trip lockouts. The recommended pattern is a separate
 * journey that targets http://localhost:5000/ with explicit URLs.
 */
import type { Page } from 'playwright';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import type { CapturedEvent } from './report.js';
import type { StepResult } from './journey.js';

export interface ProbeConfig {
  /** Explicit URLs to probe. Combined with URLs from prior discover. */
  urls?: string[];
  /** Which segment classes to mutate. Default: int + hex + uuid. */
  mutators?: Array<'int' | 'hex' | 'uuid' | 'string'>;
  /** Cap requests per *template* (URL with one segment masked). Default 12. */
  maxRequestsPerUrl?: number;
  /** Global cap. Default 500. */
  maxRequestsTotal?: number;
  /** Per-request timeout (ms). Default 8000. */
  timeoutMs?: number;
  /** Status codes that count as findings. Default 500-599. */
  failStatuses?: number[];
  /** Status codes that are explicitly fine. Default sensible 4xx + 2xx + redirects. */
  okStatuses?: number[];
  /**
   * Known hash prefixes to scan response bodies for (Bug #28/#30/#32 leak
   * detection). Operator passes prefixes captured from log streams; if
   * any appear in a probe response body, that's cross-surface bleed.
   */
  hashLeakPrefixes?: string[];
  /** Allowlist regex on the *original* URL. */
  includePatterns?: string[];
  /** Denylist regex on the *original* URL. */
  denyPatterns?: string[];
  /** Read URLs from `${outDir}/discover-pages.json`. Default true. */
  inheritDiscoverUrls?: boolean;
  /**
   * Header-smuggling pass: after URL-mutation, replay each unique URL
   * with a spoofed client-IP / host / rewrite header and flag any
   * response that differs from the baseline (suggests the header was
   * honored, e.g., the route trusts X-Forwarded-For instead of
   * CF-Connecting-IP, allowing per-IP rate-limit bypass). Default off.
   */
  headerSmuggling?: boolean;
  /**
   * Method-fuzz pass: after URL-mutation, replay each unique URL with
   * OPTIONS/HEAD/PUT/DELETE/PATCH and flag any 5xx (should be 405).
   * Catches unhandled method dispatch in middleware. Default off.
   */
  methodFuzz?: boolean;
  /**
   * Stateless-GET pass: for each unique baseline URL, fetch twice with a
   * short gap and compare specific JSON fields whose values reflect
   * PERSISTED state (lastVerifiedAt, lastUpdatedAt, modifiedAt, etc.).
   * Any field whose value changes between two consecutive idempotent GETs
   * is a probable state-changing GET — the response from call 2 is showing
   * the mutation that call 1 produced.
   *
   * This catches the Bug #34 class: a GET handler that runs a mutating
   * action (DB write, phase transition, subprocess spawn) on every hit.
   * Email scanners pre-fetching such URLs silently mutate state per
   * inbound link. The natural defense is to factor the mutation into a
   * recordResult parameter that defaults false on the public path.
   *
   * Default off. Filtering: ONLY persisted-state field names match;
   * "now", "timestamp", "serverTime" are NOT compared (those are
   * legitimately query-time values). Override the field set via
   * `statelessGetFields` if a project uses different names.
   */
  statelessGet?: boolean;
  /**
   * Field-name allowlist for the stateless-GET comparator. Each name is
   * a JSON property key that, if present in BOTH responses, is checked
   * for equality. Non-matching field names are ignored. Default:
   * lastVerifiedAt, lastUpdatedAt, lastModifiedAt, lastAccessedAt,
   * verifiedAt, updatedAt, modifiedAt, accessedAt.
   */
  statelessGetFields?: string[];
}

export interface ProbeFinding {
  url: string;
  template: string;
  variant: string;
  segment: string;
  segmentIndex: number;
  mutator: string;
  status?: number;
  durationMs: number;
  bodyPreview?: string;
  errorText?: string;
  reason: string;
}

export interface ProbeResult {
  findings: ProbeFinding[];
  stepResults: StepResult[];
  events: CapturedEvent[];
  totalRequests: number;
  templatesProbed: number;
}

const HEX_RE = /^[a-fA-F0-9]+$/;
const INT_RE = /^-?\d+$/;
const UUID_RE = /^[a-fA-F0-9]{8}-[a-fA-F0-9]{4}-[a-fA-F0-9]{4}-[a-fA-F0-9]{4}-[a-fA-F0-9]{12}$/;

type MutatorKind = 'int' | 'hex' | 'uuid' | 'string';

function classify(seg: string): MutatorKind | null {
  if (!seg) return null;
  if (UUID_RE.test(seg)) return 'uuid';
  // Hex must look hash-y — skip 1-byte words that happen to be hex like "abc".
  if (HEX_RE.test(seg) && seg.length >= 16) return 'hex';
  if (INT_RE.test(seg)) return 'int';
  return null;
}

function variantsFor(seg: string, kind: MutatorKind): Array<{ variant: string; label: string }> {
  switch (kind) {
    case 'int':
      return [
        { variant: '-1', label: 'negative' },
        { variant: '0', label: 'zero' },
        { variant: '2147483648', label: 'int32-overflow' },
        { variant: '99999999999999999999', label: 'bignum' },
        { variant: 'abc', label: 'non-numeric' },
        { variant: '%00', label: 'null-byte-only' },
        { variant: `${seg}%00`, label: 'null-byte-suffix' },
        { variant: '../etc/passwd', label: 'path-traversal' },
        { variant: '<script>alert(1)</script>', label: 'xss-tag' },
        { variant: "' OR 1=1--", label: 'sql-meta' },
      ];
    case 'hex': {
      const half = Math.floor(seg.length / 2);
      const half1 = seg.slice(0, half);
      const half2 = seg.slice(half + 1);
      return [
        { variant: '', label: 'empty' },
        { variant: 'z'.repeat(seg.length), label: 'non-hex-same-length' },
        { variant: `${half1}%00${half2}`, label: 'null-byte-mid' },
        { variant: `${seg}%00`, label: 'null-byte-suffix' },
        { variant: seg.slice(0, 15), label: 'under-minimum-15' },
        { variant: 'a'.repeat(129), label: 'over-maximum-129' },
        { variant: 'a'.repeat(10000), label: 'huge-10k' },
        { variant: 'G'.repeat(seg.length), label: 'uppercase-non-hex' },
        { variant: '../etc/passwd', label: 'path-traversal' },
        { variant: `${seg}/extra`, label: 'segment-injection' },
      ];
    }
    case 'uuid':
      return [
        { variant: '00000000-0000-0000-0000-000000000000', label: 'zero-uuid' },
        { variant: 'ffffffff-ffff-ffff-ffff-ffffffffffff', label: 'all-f-uuid' },
        { variant: 'not-a-uuid', label: 'non-uuid' },
        { variant: seg.replace(/-/g, ''), label: 'no-hyphens' },
        { variant: `${seg}%00`, label: 'null-byte-suffix' },
        { variant: `%00${seg}`, label: 'null-byte-prefix' },
        { variant: '../etc/passwd', label: 'path-traversal' },
      ];
    case 'string':
      return [
        { variant: '', label: 'empty' },
        { variant: '%00', label: 'null-byte' },
        { variant: '../etc/passwd', label: 'path-traversal' },
        { variant: '<script>alert(1)</script>', label: 'xss-tag' },
      ];
  }
}

export async function runProbe(
  page: Page,
  startUrls: string[],
  cfg: ProbeConfig,
  outDir: string,
  startEpoch: number,
  log: (e: Omit<CapturedEvent, 't'>) => void,
): Promise<ProbeResult> {
  const ctx = page.context();
  const timeout = cfg.timeoutMs ?? 8_000;
  const maxPerUrl = cfg.maxRequestsPerUrl ?? 12;
  const maxTotal = cfg.maxRequestsTotal ?? 500;
  const failStatuses = cfg.failStatuses ?? Array.from({ length: 100 }, (_, i) => 500 + i);
  const okStatuses = cfg.okStatuses ?? [200, 204, 301, 302, 303, 307, 308, 400, 401, 403, 404, 405, 409, 410, 413, 415, 422, 429];
  const enabledMutators = cfg.mutators ?? ['int', 'hex', 'uuid'];
  const inheritDiscover = cfg.inheritDiscoverUrls !== false;

  const findings: ProbeFinding[] = [];
  const stepResults: StepResult[] = [];
  const events: CapturedEvent[] = [];
  let totalRequests = 0;

  const targetSet = new Set<string>();
  for (const u of startUrls) targetSet.add(u);
  if (inheritDiscover) {
    const path = join(outDir, 'discover-pages.json');
    if (existsSync(path)) {
      try {
        const raw = readFileSync(path, 'utf8');
        const pages = JSON.parse(raw) as Array<{ url?: string }>;
        for (const p of pages) if (p.url) targetSet.add(p.url);
      } catch (e: any) {
        log({ kind: 'pageerror', text: `probe: failed to read discover-pages.json: ${e?.message || e}` });
      }
    }
  }

  // Collapse to templates: any URL where one path segment is fuzz-able
  // gets its own template. Multiple URLs sharing the same template (only
  // differ in that segment) are deduplicated so we don't re-probe the
  // same shape 50 times.
  const seenTemplates = new Set<string>();

  outer: for (const url of targetSet) {
    let parsed: URL;
    try { parsed = new URL(url); } catch { continue; }
    if (cfg.denyPatterns?.some(p => safeMatch(p, url))) continue;
    if (cfg.includePatterns && !cfg.includePatterns.some(p => safeMatch(p, url))) continue;
    const segments = parsed.pathname.split('/').filter(Boolean);
    if (segments.length === 0) continue;

    for (let i = 0; i < segments.length; i++) {
      const seg = segments[i];
      const kind = classify(seg);
      if (!kind || !enabledMutators.includes(kind)) continue;

      const tmpl = parsed.origin + '/' + segments.map((s, j) => j === i ? `<${kind}>` : s).join('/');
      if (seenTemplates.has(tmpl)) continue;
      seenTemplates.add(tmpl);

      const variants = variantsFor(seg, kind);
      let perUrlBudget = maxPerUrl;

      for (const v of variants) {
        if (perUrlBudget <= 0) break;
        if (totalRequests >= maxTotal) break outer;
        perUrlBudget--;
        totalRequests++;

        const probedSegments = segments.slice();
        probedSegments[i] = v.variant;
        const probedUrl = parsed.origin + '/' + probedSegments.join('/') + parsed.search;

        const t0 = Date.now();
        let status: number | undefined;
        let bodyPreview: string | undefined;
        let errorText: string | undefined;

        try {
          const resp = await ctx.request.get(probedUrl, { timeout, failOnStatusCode: false, maxRedirects: 0 });
          status = resp.status();
          try {
            const txt = await resp.text();
            bodyPreview = txt.slice(0, 2048);
          } catch { /* binary or empty */ }
        } catch (e: any) {
          errorText = String(e?.message || e).slice(0, 200);
        }

        const durationMs = Date.now() - t0;
        const isFail = status !== undefined && failStatuses.includes(status);
        const isUnexpectedOk = status === 200 && (kind === 'hex' || kind === 'uuid' || kind === 'int') &&
          (v.label === 'null-byte-mid' || v.label === 'null-byte-suffix' || v.label === 'null-byte-prefix' ||
           v.label === 'non-hex-same-length' || v.label === 'non-uuid' || v.label === 'non-numeric' ||
           v.label === 'huge-10k' || v.label === 'over-maximum-129' || v.label === 'path-traversal');
        const echoesInput = !!(bodyPreview && v.variant.length > 6 && bodyPreview.includes(v.variant));
        const leaksHashPrefix = !!(bodyPreview && cfg.hashLeakPrefixes &&
          cfg.hashLeakPrefixes.some(prefix => prefix.length >= 4 && bodyPreview!.includes(prefix)));
        const isUnexpectedStatus = status !== undefined && !okStatuses.includes(status) && !isFail;

        const reasonParts: string[] = [];
        if (isFail) reasonParts.push(`5xx=${status}`);
        if (errorText) reasonParts.push(`err=${errorText.slice(0, 60)}`);
        if (leaksHashPrefix) reasonParts.push('hash-leak');
        if (isUnexpectedOk) reasonParts.push('unexpected-200');
        if (echoesInput) reasonParts.push('echoes-input');
        if (isUnexpectedStatus) reasonParts.push(`unexpected-${status}`);

        const isFinding = reasonParts.length > 0;

        if (isFinding) {
          const finding: ProbeFinding = {
            url: probedUrl,
            template: tmpl,
            variant: v.variant.length > 80 ? v.variant.slice(0, 80) + '...' : v.variant,
            segment: seg.length > 40 ? seg.slice(0, 40) + '...' : seg,
            segmentIndex: i,
            mutator: `${kind}:${v.label}`,
            status,
            durationMs,
            bodyPreview: bodyPreview?.slice(0, 240),
            errorText,
            reason: reasonParts.join(', '),
          };
          findings.push(finding);
          events.push({
            kind: isFail ? 'response-error' : 'pageerror',
            text: `probe: ${tmpl} segment[${i}] ${kind}:${v.label} → ${finding.reason}`,
            url: probedUrl,
            status,
            t: Date.now() - startEpoch,
          });
          log({
            kind: isFail ? 'response-error' : 'pageerror',
            text: `probe finding: ${kind}:${v.label} on ${tmpl} → ${finding.reason}`,
            url: probedUrl,
            status,
          });
        }

        stepResults.push({
          step: { kind: 'goto', url: probedUrl, label: `probe ${kind}:${v.label}` },
          index: 2000 + totalRequests,
          ok: !isFinding,
          durationMs,
          error: isFinding ? reasonParts.join(', ') : undefined,
        });
      }

      if (totalRequests >= maxTotal) break outer;
    }
  }

  // Optional pass 2: header smuggling. For each unique URL we already
  // probed, fire one baseline GET (no spoofed headers) and one GET per
  // spoofed-header variant, then flag any variant whose status differs
  // from baseline. Differing status = the header was honored, which on
  // a CF-Connecting-IP-only server would be a per-IP rate-limit bypass
  // / audit-log-evasion bug.
  if (cfg.headerSmuggling) {
    const headerVariants: Array<{ name: string; headers: Record<string, string> }> = [
      { name: 'xff-localhost', headers: { 'X-Forwarded-For': '127.0.0.1' } },
      { name: 'xff-rfc1918', headers: { 'X-Forwarded-For': '10.0.0.1' } },
      { name: 'xri-localhost', headers: { 'X-Real-IP': '127.0.0.1' } },
      { name: 'xff-multi', headers: { 'X-Forwarded-For': '127.0.0.1, 10.0.0.1, 1.1.1.1' } },
      { name: 'host-evil', headers: { 'Host': 'evil.example.com' } },
      { name: 'x-original-url', headers: { 'X-Original-URL': '/admin' } },
      { name: 'x-rewrite-url', headers: { 'X-Rewrite-URL': '/admin' } },
      { name: 'cf-spoof', headers: { 'CF-Connecting-IP': '127.0.0.1' } },
    ];
    const baselineUrls = Array.from(seenTemplates).slice(0, 20).map(t => t.replace(/<int>|<hex>|<uuid>/, 'baseline'));
    // Add explicit URLs too, since some may not have a fuzzable segment.
    for (const u of cfg.urls || []) baselineUrls.push(u);
    const uniqueBaselines = Array.from(new Set(baselineUrls));

    for (const url of uniqueBaselines) {
      if (totalRequests >= maxTotal) break;
      let baseStatus: number | undefined;
      try {
        const resp = await ctx.request.get(url, { timeout, failOnStatusCode: false, maxRedirects: 0 });
        baseStatus = resp.status();
      } catch { continue; }
      totalRequests++;
      if (baseStatus === undefined) continue;

      for (const v of headerVariants) {
        if (totalRequests >= maxTotal) break;
        totalRequests++;
        const t0 = Date.now();
        let status: number | undefined;
        let errorText: string | undefined;
        let bodyPreview: string | undefined;
        try {
          const resp = await ctx.request.get(url, { timeout, failOnStatusCode: false, maxRedirects: 0, headers: v.headers });
          status = resp.status();
          try { bodyPreview = (await resp.text()).slice(0, 240); } catch { /* binary */ }
        } catch (e: any) {
          errorText = String(e?.message || e).slice(0, 200);
        }
        const durationMs = Date.now() - t0;
        const isFail = status !== undefined && failStatuses.includes(status);
        const statusDiff = status !== undefined && status !== baseStatus;
        const reasonParts: string[] = [];
        if (isFail) reasonParts.push(`5xx=${status}`);
        if (statusDiff && !isFail) reasonParts.push(`status-diff: baseline=${baseStatus} spoofed=${status}`);
        if (errorText) reasonParts.push(`err=${errorText.slice(0, 60)}`);

        if (reasonParts.length > 0) {
          const finding: ProbeFinding = {
            url,
            template: url,
            variant: JSON.stringify(v.headers),
            segment: '',
            segmentIndex: -1,
            mutator: `header:${v.name}`,
            status,
            durationMs,
            bodyPreview,
            errorText,
            reason: reasonParts.join(', '),
          };
          findings.push(finding);
          events.push({
            kind: isFail ? 'response-error' : 'pageerror',
            text: `probe header: ${url} ${v.name} → ${finding.reason}`,
            url,
            status,
            t: Date.now() - startEpoch,
          });
          log({
            kind: isFail ? 'response-error' : 'pageerror',
            text: `probe header finding: ${v.name} on ${url} → ${finding.reason}`,
            url,
            status,
          });
        }
        stepResults.push({
          step: { kind: 'goto', url, label: `probe header:${v.name}` },
          index: 2000 + totalRequests,
          ok: reasonParts.length === 0,
          durationMs,
          error: reasonParts.join(', ') || undefined,
        });
      }
    }
  }

  // Optional pass 3: method-fuzz. Replay each URL with OPTIONS/HEAD/
  // PUT/DELETE/PATCH and flag any 5xx — the server should respond with
  // 405 Method Not Allowed, not crash. Some Express handlers register
  // only `app.get(...)` and the framework's default fall-through can
  // surface unhandled exceptions on PATCH/PUT to read-only endpoints.
  if (cfg.methodFuzz) {
    const methods: Array<'OPTIONS' | 'HEAD' | 'PUT' | 'DELETE' | 'PATCH'> = ['OPTIONS', 'HEAD', 'PUT', 'DELETE', 'PATCH'];
    const targets = Array.from(seenTemplates).slice(0, 20).map(t => t.replace(/<int>|<hex>|<uuid>/, 'baseline'));
    for (const u of cfg.urls || []) targets.push(u);
    const unique = Array.from(new Set(targets));

    for (const url of unique) {
      if (totalRequests >= maxTotal) break;
      for (const m of methods) {
        if (totalRequests >= maxTotal) break;
        totalRequests++;
        const t0 = Date.now();
        let status: number | undefined;
        let errorText: string | undefined;
        let bodyPreview: string | undefined;
        try {
          const resp = await ctx.request.fetch(url, { method: m, timeout, failOnStatusCode: false, maxRedirects: 0 });
          status = resp.status();
          try { bodyPreview = (await resp.text()).slice(0, 240); } catch { /* binary */ }
        } catch (e: any) {
          errorText = String(e?.message || e).slice(0, 200);
        }
        const durationMs = Date.now() - t0;
        const isFail = status !== undefined && failStatuses.includes(status);
        const reasonParts: string[] = [];
        if (isFail) reasonParts.push(`5xx=${status}`);
        if (errorText) reasonParts.push(`err=${errorText.slice(0, 60)}`);

        if (reasonParts.length > 0) {
          const finding: ProbeFinding = {
            url,
            template: url,
            variant: m,
            segment: '',
            segmentIndex: -1,
            mutator: `method:${m}`,
            status,
            durationMs,
            bodyPreview,
            errorText,
            reason: reasonParts.join(', '),
          };
          findings.push(finding);
          events.push({
            kind: isFail ? 'response-error' : 'pageerror',
            text: `probe method: ${m} ${url} → ${finding.reason}`,
            url,
            status,
            t: Date.now() - startEpoch,
          });
          log({
            kind: isFail ? 'response-error' : 'pageerror',
            text: `probe method finding: ${m} on ${url} → ${finding.reason}`,
            url,
            status,
          });
        }
        stepResults.push({
          step: { kind: 'goto', url, label: `probe method:${m}` },
          index: 2000 + totalRequests,
          ok: reasonParts.length === 0,
          durationMs,
          error: reasonParts.join(', ') || undefined,
        });
      }
    }
  }

  // Optional pass 4: stateless-GET. For each unique baseline URL, fetch
  // twice with a 250ms gap, parse JSON, and compare any field whose name
  // is in the persisted-state allowlist (lastVerifiedAt, lastUpdatedAt,
  // …). If any such field's value differs between the two calls, the
  // GET handler is mutating that field on every hit — the Bug #34 class.
  // Email scanners pre-fetching the URL would silently mutate state per
  // inbound link. False-positive shape: an endpoint that legitimately
  // returns a current-time field whose name happens to match the
  // allowlist; tune via cfg.statelessGetFields to remove the noise.
  if (cfg.statelessGet) {
    const persistedFields = new Set(
      cfg.statelessGetFields && cfg.statelessGetFields.length > 0
        ? cfg.statelessGetFields
        : ['lastVerifiedAt', 'lastUpdatedAt', 'lastModifiedAt', 'lastAccessedAt',
           'verifiedAt', 'updatedAt', 'modifiedAt', 'accessedAt'],
    );
    const targets = Array.from(seenTemplates).slice(0, 20).map(t => t.replace(/<int>|<hex>|<uuid>/, 'baseline'));
    for (const u of cfg.urls || []) targets.push(u);
    const unique = Array.from(new Set(targets));

    for (const url of unique) {
      if (totalRequests >= maxTotal) break;
      let bodyA: string | undefined;
      let bodyB: string | undefined;
      let statusA: number | undefined;
      let statusB: number | undefined;
      try {
        const respA = await ctx.request.get(url, { timeout, failOnStatusCode: false, maxRedirects: 0 });
        statusA = respA.status();
        bodyA = await respA.text();
      } catch { continue; }
      totalRequests++;
      // Small gap so the second call isn't deduped at any caching layer.
      await new Promise(r => setTimeout(r, 250));
      try {
        const respB = await ctx.request.get(url, { timeout, failOnStatusCode: false, maxRedirects: 0 });
        statusB = respB.status();
        bodyB = await respB.text();
      } catch { continue; }
      totalRequests++;

      if (statusA !== 200 || statusB !== 200 || !bodyA || !bodyB) continue;

      let jsonA: unknown, jsonB: unknown;
      try { jsonA = JSON.parse(bodyA); } catch { continue; }
      try { jsonB = JSON.parse(bodyB); } catch { continue; }

      // Walk both responses in parallel; on every key whose name is in
      // the persisted-state allowlist AND whose value differs between
      // calls, record a finding. JSON values can nest; recurse with a
      // small depth cap.
      const drift: Array<{ path: string; a: unknown; b: unknown }> = [];
      const walk = (a: unknown, b: unknown, path: string, depth: number): void => {
        if (depth > 6) return;
        if (a == null || b == null) return;
        if (typeof a !== typeof b) return;
        if (typeof a !== 'object') return;
        const ao = a as Record<string, unknown>;
        const bo = b as Record<string, unknown>;
        for (const key of Object.keys(ao)) {
          const next = path ? `${path}.${key}` : key;
          if (persistedFields.has(key)) {
            if (ao[key] !== undefined && bo[key] !== undefined && JSON.stringify(ao[key]) !== JSON.stringify(bo[key])) {
              drift.push({ path: next, a: ao[key], b: bo[key] });
            }
          }
          if (ao[key] && bo[key] && typeof ao[key] === 'object' && typeof bo[key] === 'object') {
            walk(ao[key], bo[key], next, depth + 1);
          }
        }
      };
      walk(jsonA, jsonB, '', 0);

      if (drift.length > 0) {
        const reason = `stateless-get violation: ${drift.map(d => `${d.path}: ${JSON.stringify(d.a)}→${JSON.stringify(d.b)}`).join('; ')}`;
        const finding: ProbeFinding = {
          url,
          template: url,
          variant: 'baseline+baseline',
          segment: '',
          segmentIndex: -1,
          mutator: 'stateless-get',
          status: statusA,
          durationMs: 0,
          bodyPreview: bodyA.slice(0, 240),
          reason,
        };
        findings.push(finding);
        events.push({
          kind: 'pageerror',
          text: `probe stateless-get: ${url} → ${reason}`,
          url,
          status: statusA,
          t: Date.now() - startEpoch,
        });
        log({
          kind: 'pageerror',
          text: `probe stateless-get finding: ${url} → ${reason}`,
          url,
          status: statusA,
        });
      }
      stepResults.push({
        step: { kind: 'goto', url, label: 'probe stateless-get' },
        index: 2000 + totalRequests,
        ok: drift.length === 0,
        durationMs: 0,
        error: drift.length > 0 ? `${drift.length} field(s) drifted` : undefined,
      });
    }
  }

  // Persist the findings. probe-findings.json is the machine-readable
  // form; probe-summary.txt is the at-a-glance triage view.
  try {
    writeFileSync(
      join(outDir, 'probe-findings.json'),
      JSON.stringify({ totalRequests, templatesProbed: seenTemplates.size, findings }, null, 2),
    );
  } catch { /* ignore */ }

  try {
    const lines = [
      `# Adversarial URL-param probe`,
      `# Targets:    ${targetSet.size}`,
      `# Templates:  ${seenTemplates.size}`,
      `# Requests:   ${totalRequests}`,
      `# Findings:   ${findings.length}`,
      ``,
      ...findings.map(f =>
        `[${f.status ?? 'ERR'}] ${f.mutator} ${f.template}\n` +
        `  url=${f.url}\n` +
        `  variant=${JSON.stringify(f.variant)}\n` +
        `  reason=${f.reason}\n` +
        `  preview=${(f.bodyPreview || '').replace(/\s+/g, ' ').slice(0, 160)}`
      ),
    ].join('\n');
    writeFileSync(join(outDir, 'probe-summary.txt'), lines);
  } catch { /* ignore */ }

  console.log(`[crawler] probe complete: ${findings.length} findings in ${totalRequests} requests across ${seenTemplates.size} templates`);
  return { findings, stepResults, events, totalRequests, templatesProbed: seenTemplates.size };
}

function safeMatch(pattern: string, s: string): boolean {
  try { return new RegExp(pattern).test(s); } catch { return false; }
}
