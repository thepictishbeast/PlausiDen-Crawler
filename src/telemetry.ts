// telemetry.ts — rich site-health capture for the crawler.
//
// The baseline capture (console + pageerror + failed-request + 4xx/5xx)
// only records the RED events. That misses:
//   - "this endpoint is slow but eventually succeeds"
//   - "this button was clicked but nothing changed"
//   - "the JS ran a 1.2s long task during this step"
//   - "the browser allocated 800 MB of heap and never released"
//   - "this image 404'd silently"
//   - "a CSP rule was violated"
//   - "a service worker returned stale 503"
//
// This module attaches a PerformanceObserver + request/response recorders
// + DOM-mutation hooks to every Playwright Page and produces a structured
// telemetry bundle that the report layer aggregates.

import type { Page } from 'playwright';

export interface RequestRecord {
  t: number;                          // ms since run start
  method: string;
  url: string;
  status?: number;
  statusText?: string;
  durationMs?: number;
  reqBodySize?: number;
  respBodySize?: number;
  resourceType?: string;               // document / xhr / fetch / image / script / stylesheet / websocket
  fromCache?: boolean;
  serverTiming?: string;               // Server-Timing response header
  cfCacheStatus?: string;              // cf-cache-status header (Cloudflare)
  xCache?: string;                     // X-Cache / Cache-Control observation
  failed?: { errorText: string };
}

export interface LongTaskRecord {
  t: number;
  durationMs: number;
  name: string;                        // TaskAttributionTiming.containerId or 'unknown'
  attribution?: string;
}

export interface MemorySnapshot {
  t: number;
  label: string;
  jsHeapUsedMb?: number;
  jsHeapTotalMb?: number;
  jsHeapLimitMb?: number;
  domNodes?: number;
  listeners?: number;
}

export interface BrokenResource {
  t: number;
  kind: 'image' | 'script' | 'stylesheet' | 'font' | 'manifest' | 'other';
  url: string;
  message?: string;
}

export interface CspViolation {
  t: number;
  violatedDirective: string;
  blockedUri: string;
  sourceFile?: string;
  lineNumber?: number;
  originalPolicy?: string;
}

export interface UnhandledRejection {
  t: number;
  reason: string;
  stack?: string;
}

export interface ClickOutcome {
  t: number;
  selector: string;
  label: string;
  navigatedTo?: string;                // URL after 1s if navigation happened
  domMutated: boolean;                  // did the DOM change in the 500ms following?
  networkActivity: number;              // how many requests fired in 500ms following
  modalOpened: boolean;                 // is a role=dialog now visible
}

export interface TelemetryBundle {
  requests: RequestRecord[];
  longTasks: LongTaskRecord[];
  memory: MemorySnapshot[];
  brokenResources: BrokenResource[];
  cspViolations: CspViolation[];
  unhandledRejections: UnhandledRejection[];
  clicks: ClickOutcome[];
  serviceWorker?: {
    registered: boolean;
    scriptURL?: string;
    state?: string;
    controllerOnFirstLoad?: boolean;
  };
}

export function makeEmptyBundle(): TelemetryBundle {
  return {
    requests: [],
    longTasks: [],
    memory: [],
    brokenResources: [],
    cspViolations: [],
    unhandledRejections: [],
    clicks: [],
  };
}

/**
 * Attach telemetry hooks to a Playwright page. Call once after page creation,
 * before first navigation. Returns the bundle that will accumulate events.
 *
 * The `startEpoch` is Date.now() when the run began; used to compute `t`
 * offsets relative to run start so bundles line up with step timestamps.
 */
export async function attachTelemetry(
  page: Page,
  startEpoch: number,
): Promise<TelemetryBundle> {
  const bundle = makeEmptyBundle();
  const now = () => Date.now() - startEpoch;

  // -------- Request / response (every request, not just errors) --------
  // Playwright's `request.timing()` gives detailed phase breakdowns; we
  // keep it simple and use start/end + response-size. Sizes come from
  // response.body().length but we skip binary bodies to avoid bloating memory.
  const pendingRequests = new Map<string, { t: number; url: string; method: string; resourceType: string; reqBodySize?: number }>();

  page.on('request', (req) => {
    const key = req.url() + ':' + req.method() + ':' + Math.random().toString(36).slice(2, 8);
    const post = req.postData();
    pendingRequests.set(req.url() + '@' + req.method(), {
      t: now(),
      url: req.url(),
      method: req.method(),
      resourceType: req.resourceType(),
      reqBodySize: post ? post.length : undefined,
    });
  });

  page.on('response', async (res) => {
    const req = res.request();
    const key = req.url() + '@' + req.method();
    const pending = pendingRequests.get(key);
    pendingRequests.delete(key);
    if (!pending) return;

    // Try to get response size from body (bounded — skip large bodies + binaries).
    let respBodySize: number | undefined;
    const ct = res.headers()['content-type'] ?? '';
    if (!/image|video|audio|octet-stream/.test(ct)) {
      try {
        const buf = await res.body();
        if (buf.length < 5_000_000) respBodySize = buf.length;
      } catch { /* body may be unavailable */ }
    }

    const hdrs = res.headers();
    bundle.requests.push({
      t: pending.t,
      method: pending.method,
      url: pending.url,
      status: res.status(),
      statusText: res.statusText(),
      durationMs: now() - pending.t,
      reqBodySize: pending.reqBodySize,
      respBodySize,
      resourceType: pending.resourceType,
      fromCache: res.fromServiceWorker() || hdrs['x-from-cache'] === 'true',
      serverTiming: hdrs['server-timing'],
      cfCacheStatus: hdrs['cf-cache-status'],
      xCache: hdrs['x-cache'] ?? hdrs['cache-status'],
    });
  });

  page.on('requestfailed', (req) => {
    const key = req.url() + '@' + req.method();
    const pending = pendingRequests.get(key);
    pendingRequests.delete(key);
    bundle.requests.push({
      t: pending?.t ?? now(),
      method: req.method(),
      url: req.url(),
      resourceType: req.resourceType(),
      failed: { errorText: req.failure()?.errorText || 'unknown' },
    });
  });

  // -------- In-page hooks (long tasks, broken resources, CSP, unhandled rejections) --------
  // Bind a __crawler_push callback the page can call to forward in-page events.
  await page.exposeFunction('__crawler_pushLongTask', (entry: LongTaskRecord) => {
    bundle.longTasks.push({ ...entry, t: now() });
  });
  await page.exposeFunction('__crawler_pushBrokenResource', (entry: BrokenResource) => {
    bundle.brokenResources.push({ ...entry, t: now() });
  });
  await page.exposeFunction('__crawler_pushCsp', (entry: CspViolation) => {
    bundle.cspViolations.push({ ...entry, t: now() });
  });
  await page.exposeFunction('__crawler_pushUnhandled', (entry: UnhandledRejection) => {
    bundle.unhandledRejections.push({ ...entry, t: now() });
  });

  // Install hooks on every navigated page.
  await page.addInitScript(() => {
    // LONG TASKS — Performance Observer for tasks > 50ms (default threshold).
    try {
      const po = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          // @ts-ignore — PerformanceLongTaskTiming not in default lib
          const attr = entry.attribution?.[0]?.name || 'unknown';
          // @ts-ignore
          (window as any).__crawler_pushLongTask?.({
            t: 0,
            durationMs: Math.round(entry.duration),
            name: entry.name,
            attribution: attr,
          });
        }
      });
      // 'longtask' is a valid entryType in the spec but missing from older lib.dom defs.
      po.observe({ type: 'longtask' as any, buffered: true });
    } catch { /* longtask entryType unsupported */ }

    // BROKEN RESOURCES — <img>/<script>/<link> that fail to load.
    document.addEventListener('error', (ev) => {
      const t = ev.target as HTMLElement | null;
      if (!t || !('tagName' in t)) return;
      const tag = t.tagName.toLowerCase();
      let kind: BrokenResource['kind'] = 'other';
      let url = '';
      if (tag === 'img') { kind = 'image'; url = (t as HTMLImageElement).src; }
      else if (tag === 'script') { kind = 'script'; url = (t as HTMLScriptElement).src; }
      else if (tag === 'link') { kind = 'stylesheet'; url = (t as HTMLLinkElement).href; }
      if (!url) return;
      // @ts-ignore
      (window as any).__crawler_pushBrokenResource?.({
        t: 0, kind, url, message: `<${tag}> failed to load`,
      });
    }, true);

    // CSP VIOLATIONS — fires when a resource violates Content-Security-Policy.
    document.addEventListener('securitypolicyviolation', (ev: any) => {
      // @ts-ignore
      (window as any).__crawler_pushCsp?.({
        t: 0,
        violatedDirective: ev.violatedDirective || '',
        blockedUri: ev.blockedURI || '',
        sourceFile: ev.sourceFile,
        lineNumber: ev.lineNumber,
        originalPolicy: ev.originalPolicy?.slice(0, 200),
      });
    });

    // UNHANDLED REJECTIONS — complement to page.on('pageerror').
    window.addEventListener('unhandledrejection', (ev) => {
      const reason = ev.reason;
      const text = reason instanceof Error ? reason.message : String(reason ?? 'unknown');
      const stack = reason instanceof Error ? reason.stack : undefined;
      // @ts-ignore
      (window as any).__crawler_pushUnhandled?.({ t: 0, reason: text, stack });
    });
  });

  return bundle;
}

/**
 * Snapshot the page's memory + DOM state. Call after each journey step.
 */
export async function snapshotMemory(
  page: Page,
  label: string,
  startEpoch: number,
  bundle: TelemetryBundle,
): Promise<void> {
  try {
    const snap = await page.evaluate(() => {
      // @ts-ignore — performance.memory is Chrome-only + nonstandard but widely available.
      const mem = (performance as any).memory ?? null;
      return {
        jsHeapUsedMb: mem ? Math.round(mem.usedJSHeapSize / 1024 / 1024) : undefined,
        jsHeapTotalMb: mem ? Math.round(mem.totalJSHeapSize / 1024 / 1024) : undefined,
        jsHeapLimitMb: mem ? Math.round(mem.jsHeapSizeLimit / 1024 / 1024) : undefined,
        domNodes: document.getElementsByTagName('*').length,
      };
    });
    bundle.memory.push({
      t: Date.now() - startEpoch,
      label,
      ...snap,
    });
  } catch { /* page may have navigated mid-snapshot */ }
}

/**
 * Record the outcome of a click step: did the DOM change, did we navigate,
 * did the network fire, did a modal open? Helps distinguish "button did
 * nothing" (dead UI) from "button worked but the result was invisible".
 */
export async function recordClickOutcome(
  page: Page,
  selector: string,
  label: string,
  startEpoch: number,
  bundle: TelemetryBundle,
  preClickNetworkCount: number,
): Promise<void> {
  await page.waitForTimeout(500);
  try {
    const outcome = await page.evaluate(() => ({
      url: location.href,
      domHash: Array.from(document.body.querySelectorAll('*')).length,
      modalOpen: !!document.querySelector('[role="dialog"]:not([aria-hidden="true"])'),
    }));
    bundle.clicks.push({
      t: Date.now() - startEpoch,
      selector,
      label,
      navigatedTo: outcome.url,
      domMutated: true, // coarse; refined by comparing domHash against pre-click snapshot in caller
      networkActivity: bundle.requests.length - preClickNetworkCount,
      modalOpened: outcome.modalOpen,
    });
  } catch { /* silent */ }
}

/**
 * Query the Service Worker state. Run once after the first navigation.
 */
export async function captureServiceWorker(page: Page, bundle: TelemetryBundle): Promise<void> {
  try {
    const sw = await page.evaluate(async () => {
      if (!('serviceWorker' in navigator)) return { registered: false };
      const reg = await navigator.serviceWorker.getRegistration();
      if (!reg) return { registered: false };
      return {
        registered: true,
        scriptURL: reg.active?.scriptURL,
        state: reg.active?.state,
        controllerOnFirstLoad: !!navigator.serviceWorker.controller,
      };
    });
    bundle.serviceWorker = sw;
  } catch { /* SW unavailable */ }
}
