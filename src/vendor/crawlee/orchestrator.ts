/**
 * Crawlee orchestration layer.
 *
 * Crawlee absorbed via npm (crawlee@3.16.x). Per AVP-2 thin-adapter
 * principle, only this file touches the Crawlee API; the Runner
 * consumes the output.
 *
 * What Crawlee adds:
 *   • Request queue with priorities + dedup.
 *   • Session pool (cookies / storage persist across requests).
 *   • Browser pool (spin up 5-20 parallel browsers, auto-concurrent).
 *   • Network chaos: intercept + fail N% of requests for resilience tests.
 *   • Proxy rotation (future).
 *   • Auto-retry with backoff.
 *
 * v0.3 use case: concurrent stress test.
 *   runConcurrentJourneys([smokePath, deepPath], { workers: 10, target })
 *   → runs both journeys 10× in parallel, collects aggregated events.
 *
 * Stub only for now. Full wiring lands when v0.3 rolls.
 */

export interface ConcurrentRunOpts {
  target: string;
  workers: number;
  journeyPaths: string[];
  /** 0.0..1.0 — probability of failing each request for chaos testing. */
  networkFailureRate?: number;
}

export const CRAWLEE_VERSION = '3.16.0';

/**
 * Placeholder. v0.3 will wire PlaywrightCrawler + Dataset +
 * BrowserPool here. The current Runner already does single-journey
 * runs; this fans that out.
 */
export async function runConcurrentJourneys(opts: ConcurrentRunOpts): Promise<{ ok: number; failed: number }> {
  // Full impl:
  //   const crawler = new PlaywrightCrawler({
  //     maxConcurrency: opts.workers,
  //     async requestHandler({ page, request }) {
  //       // load journey from request.userData.journeyPath
  //       // reuse our Runner's step executor
  //     },
  //   });
  //   for (const p of opts.journeyPaths) {
  //     for (let i = 0; i < opts.workers; i++) {
  //       await crawler.addRequests([{ url: opts.target, userData: { journeyPath: p, iter: i } }]);
  //     }
  //   }
  //   await crawler.run();
  console.warn('[crawlee-orchestrator] v0.3 stub — fan-out not yet wired. Opts:', opts);
  return { ok: 0, failed: 0 };
}
