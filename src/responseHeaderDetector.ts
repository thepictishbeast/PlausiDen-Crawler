/**
 * responseHeaderDetector.ts — generic check-runner for the
 * response-header detector family. T76.
 *
 * Eight detectors (and counting) share the exact same wiring:
 *
 *   1. Read the current page URL.
 *   2. Look up its top-level navigation response headers from
 *      the shared `topLevelResponseHeaders` Map.
 *   3. Build a snapshot via the detector's parser.
 *   4. Apply the `disableLocalhostExemption` opt-out if set.
 *   5. Run the detector to get findings.
 *   6. Push the per-step record onto an array for JSON dump.
 *   7. Emit one captured event per finding with severity / kind /
 *      ruleId / impact derived from the finding.
 *   8. Catch any throw and emit a pageerror so the audit doesn't
 *      lose track of which detector blew up.
 *
 * Before extraction this lived inline in main.ts, ~25 lines per
 * detector × 8 detectors = ~200 lines of pure boilerplate. After
 * extraction it's one factory call per detector, ~6 lines each.
 *
 * The boilerplate equivalence is the WHOLE point of the helper.
 * Single point of audit for:
 *   * the localhost-exemption opt-out path,
 *   * the `topLevelResponseHeaders` Map read,
 *   * the captured-event emission shape,
 *   * the per-step record format,
 *   * the pageerror swallow on throw.
 *
 * REGRESSION-GUARD: any change to this helper's emission format
 * affects ALL response-header detectors at once. The HTTPS gate's
 * 34 routes + the unit-test suite for each detector module are
 * the safety net — keep them green on every change here.
 *
 * Generics:
 *   S = snapshot shape. Constrained: must have `pageIsLocalhost`
 *       so the env-var opt-out can flip it. Everything else is
 *       opaque to this helper.
 *   F = finding shape. Constrained: must have `severity`, `kind`,
 *       `detail` so we can build the captured event. The detector
 *       can stash additional evidence in there — opaque to this
 *       helper.
 *
 * NOT extracted:
 *   * The header-name lookup that each detector's
 *     `buildXxxSnapshot` does internally — the lookup form is
 *     identical (case-insensitive Object.entries scan) but lives
 *     inside each detector module. Could itself be a tiny helper
 *     `getHeader(headers, lowercaseName)`, but the duplication is
 *     5 lines × 8 detectors and refactoring there would obscure
 *     the per-detector header name. Left as is.
 *   * The classifier logic. Genuinely heterogeneous (single-value
 *     enum classification, per-cookie aggregation, per-feature
 *     high-risk-set membership, per-directive script-src fallback
 *     + structural-baseline absence). Each detector keeps its own
 *     `detectXxxIssues` function — this helper is just the
 *     wiring around it.
 */

import type { Page } from 'playwright';

export interface ResponseHeaderCheckFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
}

export interface ResponseHeaderCheckSnapshot {
  pageIsLocalhost: boolean;
}

export interface PerStepRecord<F> {
  stepLabel: string;
  pageUrl: string;
  findings: F[];
}

export interface MakeResponseHeaderCheckOpts<
  S extends ResponseHeaderCheckSnapshot,
  F extends ResponseHeaderCheckFinding,
> {
  /** Detector tag used in pageerror messages on throw. */
  detectorName: string;
  /** Captured event `kind` field (e.g. 'hsts', 'coop', 'csp-policy'). */
  eventKind: string;
  /** Live Playwright page handle. */
  page: Page;
  /** Shared accumulator written by the response listener in main.ts. */
  topLevelResponseHeaders: Map<string, Record<string, string>>;
  /** True when the CRAWLER_DISABLE_LOCALHOST_EXEMPTION env var is set. */
  disableLocalhostExemption: boolean;
  /** Per-step record sink — caller-owned, dumped to JSON later. */
  findingsByStep: Array<PerStepRecord<F>>;
  /** Emit-into-the-event-stream callback. Loose typing matches main.ts. */
  log: (e: {
    kind: string;
    text: string;
    url?: string;
    severity?: 'strict' | 'warn';
    ruleId?: string;
    impact?: string;
  }) => void;
  /** Per-detector parser. */
  buildSnapshot: (pageUrl: string, headers: Record<string, string> | undefined) => S;
  /** Per-detector classifier. */
  detectIssues: (snap: S) => F[];
}

/**
 * Build a `check<DetectorName>(afterLabel)` function for one
 * response-header detector. The returned function is what main.ts
 * awaits inside the goto loop.
 */
export function makeResponseHeaderCheck<
  S extends ResponseHeaderCheckSnapshot,
  F extends ResponseHeaderCheckFinding,
>(opts: MakeResponseHeaderCheckOpts<S, F>): (afterLabel: string) => Promise<void> {
  const {
    detectorName,
    eventKind,
    page,
    topLevelResponseHeaders,
    disableLocalhostExemption,
    findingsByStep,
    log,
    buildSnapshot,
    detectIssues,
  } = opts;

  return async (afterLabel: string) => {
    try {
      const pageUrl = page.url();
      const headers = topLevelResponseHeaders.get(pageUrl);
      const snap = buildSnapshot(pageUrl, headers);
      if (disableLocalhostExemption) {
        snap.pageIsLocalhost = false;
      }
      const findings = detectIssues(snap);
      findingsByStep.push({ stepLabel: afterLabel, pageUrl, findings });
      for (const f of findings) {
        log({
          kind: eventKind,
          text: `[${f.kind}] ${f.detail}`,
          url: pageUrl,
          severity: f.severity,
          ruleId: f.kind,
          impact: f.severity === 'strict' ? 'serious' : 'minor',
        });
      }
    } catch (e) {
      log({
        kind: 'pageerror',
        text: `[${detectorName}] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };
}
