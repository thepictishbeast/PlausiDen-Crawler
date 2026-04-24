// aggregates.ts — post-process telemetry into actionable leaderboards.
//
// Raw telemetry is noisy. Aggregations answer specific operator questions:
//   - "Which endpoints are slow?"
//   - "Which endpoints fail most often?"
//   - "Where is memory growing?"
//   - "Which clicks did nothing?"
//   - "Which resources are broken across the site?"

import type { TelemetryBundle, RequestRecord, ClickOutcome, MemorySnapshot } from './telemetry.js';

export interface EndpointStats {
  urlPattern: string;                 // normalized: :id, :uuid, etc.
  method: string;
  count: number;
  errorCount: number;                  // status >= 400 OR failed
  errorRate: number;                   // errorCount / count
  medianMs: number;
  p95Ms: number;
  maxMs: number;
  bytesTotal: number;
  status2xx: number;
  status3xx: number;
  status4xx: number;
  status5xx: number;
  failed: number;
}

export interface Aggregates {
  totalRequests: number;
  totalBytes: number;
  uniqueEndpoints: number;
  errorCount: number;
  errorRate: number;
  slowestEndpoints: EndpointStats[];
  errorProneEndpoints: EndpointStats[];
  longestTasks: Array<{ durationMs: number; attribution: string; t: number }>;
  memoryGrowth?: { startMb: number; endMb: number; peakMb: number; deltaMb: number };
  brokenResourceCount: number;
  cspViolationCount: number;
  unhandledRejectionCount: number;
  clicksWithoutEffect: ClickOutcome[];
  byResourceType: Record<string, { count: number; bytes: number; errors: number }>;
}

function normalizePath(url: string): string {
  try {
    const u = new URL(url);
    let p = u.pathname;
    // Collapse UUID / hex-id / numeric-id path segments to :id.
    p = p.replace(/\/[0-9a-f-]{36}(?=\/|$)/gi, '/:uuid');
    p = p.replace(/\/[0-9a-f]{8,64}(?=\/|$)/gi, '/:hash');
    p = p.replace(/\/\d{5,}(?=\/|$)/g, '/:id');
    // Keep host for absolute URLs so cross-host requests don't collide.
    return `${u.origin}${p}`;
  } catch {
    return url;
  }
}

function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) return 0;
  const idx = Math.min(sorted.length - 1, Math.max(0, Math.floor(p * sorted.length)));
  return sorted[idx];
}

export function aggregate(bundle: TelemetryBundle): Aggregates {
  const byKey = new Map<string, RequestRecord[]>();
  for (const r of bundle.requests) {
    const key = `${r.method} ${normalizePath(r.url)}`;
    const arr = byKey.get(key) ?? [];
    arr.push(r);
    byKey.set(key, arr);
  }

  const endpoints: EndpointStats[] = [];
  for (const [key, arr] of byKey) {
    const [method, urlPattern] = key.split(' ', 2);
    const durations = arr.map(r => r.durationMs ?? 0).filter(d => d > 0).sort((a, b) => a - b);
    const bytes = arr.reduce((s, r) => s + (r.respBodySize ?? 0), 0);
    const failed = arr.filter(r => r.failed).length;
    const errs = arr.filter(r => r.failed || (r.status && r.status >= 400)).length;
    endpoints.push({
      urlPattern, method,
      count: arr.length,
      errorCount: errs,
      errorRate: arr.length ? errs / arr.length : 0,
      medianMs: percentile(durations, 0.5),
      p95Ms: percentile(durations, 0.95),
      maxMs: durations[durations.length - 1] ?? 0,
      bytesTotal: bytes,
      status2xx: arr.filter(r => r.status && r.status >= 200 && r.status < 300).length,
      status3xx: arr.filter(r => r.status && r.status >= 300 && r.status < 400).length,
      status4xx: arr.filter(r => r.status && r.status >= 400 && r.status < 500).length,
      status5xx: arr.filter(r => r.status && r.status >= 500).length,
      failed,
    });
  }

  const slowest = [...endpoints].sort((a, b) => b.p95Ms - a.p95Ms).slice(0, 20);
  const errorProne = [...endpoints].filter(e => e.errorCount > 0).sort((a, b) => b.errorCount - a.errorCount).slice(0, 20);

  const totalRequests = bundle.requests.length;
  const totalBytes = bundle.requests.reduce((s, r) => s + (r.respBodySize ?? 0), 0);
  const errorCount = bundle.requests.filter(r => r.failed || (r.status && r.status >= 400)).length;
  const errorRate = totalRequests ? errorCount / totalRequests : 0;

  const longestTasks = [...bundle.longTasks]
    .sort((a, b) => b.durationMs - a.durationMs)
    .slice(0, 20)
    .map(lt => ({ durationMs: lt.durationMs, attribution: lt.attribution ?? lt.name, t: lt.t }));

  let memoryGrowth: Aggregates['memoryGrowth'];
  const memWithHeap = bundle.memory.filter(m => m.jsHeapUsedMb != null);
  if (memWithHeap.length >= 2) {
    const startMb = memWithHeap[0].jsHeapUsedMb!;
    const endMb = memWithHeap[memWithHeap.length - 1].jsHeapUsedMb!;
    const peakMb = Math.max(...memWithHeap.map(m => m.jsHeapUsedMb!));
    memoryGrowth = { startMb, endMb, peakMb, deltaMb: endMb - startMb };
  }

  // Clicks considered ineffective: no navigation, no modal, <3 network requests fired.
  // Coarse heuristic; refines if we add DOM-hash comparison pre/post click.
  const clicksWithoutEffect = bundle.clicks.filter(c =>
    !c.modalOpened && c.networkActivity < 3
  );

  // Bytes + errors grouped by resourceType so operators can see if images
  // dominate, or if scripts are erroring most.
  const byResourceType: Aggregates['byResourceType'] = {};
  for (const r of bundle.requests) {
    const k = r.resourceType ?? 'unknown';
    const entry = byResourceType[k] ?? { count: 0, bytes: 0, errors: 0 };
    entry.count++;
    entry.bytes += r.respBodySize ?? 0;
    if (r.failed || (r.status && r.status >= 400)) entry.errors++;
    byResourceType[k] = entry;
  }

  return {
    totalRequests,
    totalBytes,
    uniqueEndpoints: endpoints.length,
    errorCount,
    errorRate,
    slowestEndpoints: slowest,
    errorProneEndpoints: errorProne,
    longestTasks,
    memoryGrowth,
    brokenResourceCount: bundle.brokenResources.length,
    cspViolationCount: bundle.cspViolations.length,
    unhandledRejectionCount: bundle.unhandledRejections.length,
    clicksWithoutEffect,
    byResourceType,
  };
}

/**
 * Render a human-friendly summary of aggregates for the terminal.
 */
export function renderSummary(agg: Aggregates): string {
  const lines: string[] = [];
  lines.push('# Crawl summary');
  lines.push(`  ${agg.totalRequests} requests · ${(agg.totalBytes / 1024).toFixed(1)} KB · ${(agg.errorRate * 100).toFixed(1)}% error rate`);
  lines.push(`  ${agg.uniqueEndpoints} unique endpoints, ${agg.brokenResourceCount} broken resources, ${agg.cspViolationCount} CSP violations, ${agg.unhandledRejectionCount} unhandled rejections`);
  if (agg.memoryGrowth) {
    lines.push(`  JS heap: ${agg.memoryGrowth.startMb} MB → ${agg.memoryGrowth.endMb} MB (peak ${agg.memoryGrowth.peakMb} MB, Δ${agg.memoryGrowth.deltaMb >= 0 ? '+' : ''}${agg.memoryGrowth.deltaMb} MB)`);
  }
  if (agg.clicksWithoutEffect.length > 0) {
    lines.push(`\n# Clicks with no observed effect (${agg.clicksWithoutEffect.length})`);
    for (const c of agg.clicksWithoutEffect.slice(0, 5)) {
      lines.push(`  ${c.label || c.selector}`);
    }
  }
  if (agg.slowestEndpoints.length > 0) {
    lines.push('\n# Slowest endpoints (p95)');
    for (const e of agg.slowestEndpoints.slice(0, 8)) {
      lines.push(`  p95=${e.p95Ms}ms max=${e.maxMs}ms n=${e.count} ${e.errorRate > 0 ? '(' + (e.errorRate * 100).toFixed(0) + '% err) ' : ''}${e.method} ${e.urlPattern}`);
    }
  }
  if (agg.errorProneEndpoints.length > 0) {
    lines.push('\n# Error-prone endpoints');
    for (const e of agg.errorProneEndpoints.slice(0, 8)) {
      lines.push(`  err=${e.errorCount}/${e.count} 5xx=${e.status5xx} 4xx=${e.status4xx} failed=${e.failed} ${e.method} ${e.urlPattern}`);
    }
  }
  if (agg.longestTasks.length > 0 && agg.longestTasks[0].durationMs > 100) {
    lines.push('\n# Longest JS tasks (>100ms blocks main thread)');
    for (const lt of agg.longestTasks.slice(0, 5)) {
      lines.push(`  ${lt.durationMs}ms at t=${lt.t}ms  ${lt.attribution}`);
    }
  }
  const rt = Object.entries(agg.byResourceType).sort((a, b) => b[1].bytes - a[1].bytes);
  if (rt.length > 0) {
    lines.push('\n# By resource type');
    for (const [k, v] of rt) {
      lines.push(`  ${k.padEnd(12)} n=${v.count}  bytes=${(v.bytes / 1024).toFixed(1)} KB  err=${v.errors}`);
    }
  }
  return lines.join('\n');
}
