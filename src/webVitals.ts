/**
 * web-vitals capture — Largest Contentful Paint, Cumulative Layout
 * Shift, Interaction to Next Paint.
 *
 * Injects Google's web-vitals library into every page via addInitScript.
 * Metrics land on window.__lfiVitals as { lcp, cls, inp, ttfb, fcp } as
 * they fire. `collectVitals(page)` reads the current snapshot.
 *
 * Thresholds per Google's own Core Web Vitals targets:
 *   LCP  good <2.5s | needs-improvement 2.5-4.0s | poor >4.0s
 *   CLS  good <0.1  | needs-improvement 0.1-0.25 | poor >0.25
 *   INP  good <200  | needs-improvement 200-500  | poor >500 (ms)
 *
 * The crawler tags each captured vital with its band so the diff
 * surfaces regressions ("INP was 'good' yesterday, 'poor' today").
 */
import type { Page } from 'playwright';

export interface VitalSnapshot {
  lcp?: { value: number; band: 'good' | 'needs-improvement' | 'poor' };
  cls?: { value: number; band: 'good' | 'needs-improvement' | 'poor' };
  inp?: { value: number; band: 'good' | 'needs-improvement' | 'poor' };
  ttfb?: { value: number };
  fcp?: { value: number };
  capturedAt: number;
}

const bandLcp = (v: number) => v < 2500 ? 'good' : v < 4000 ? 'needs-improvement' : 'poor';
const bandCls = (v: number) => v < 0.1 ? 'good' : v < 0.25 ? 'needs-improvement' : 'poor';
const bandInp = (v: number) => v < 200 ? 'good' : v < 500 ? 'needs-improvement' : 'poor';

/**
 * Inject the web-vitals library into every new page. Must be called on
 * the BrowserContext before navigation so the library is present when
 * the first-paint metrics fire.
 *
 * We inline the web-vitals IIFE build from node_modules so the target
 * page doesn't need network access to load it.
 */
export async function installWebVitals(page: Page): Promise<void> {
  // web-vitals ships an IIFE build at dist/web-vitals.iife.js
  // Read it at init time and addInitScript to the page.
  let src: string;
  try {
    const fs = await import('node:fs');
    const url = new URL('../node_modules/web-vitals/dist/web-vitals.iife.js', import.meta.url);
    src = fs.readFileSync(url, 'utf8');
  } catch {
    console.warn('[web-vitals] library not found; metrics capture disabled');
    return;
  }
  const init = `
    ${src}
    (function() {
      window.__lfiVitals = window.__lfiVitals || {};
      try {
        webVitals.onLCP(m => { window.__lfiVitals.lcp = m.value; });
        webVitals.onCLS(m => { window.__lfiVitals.cls = m.value; });
        webVitals.onINP(m => { window.__lfiVitals.inp = m.value; });
        webVitals.onTTFB(m => { window.__lfiVitals.ttfb = m.value; });
        webVitals.onFCP(m => { window.__lfiVitals.fcp = m.value; });
      } catch (e) { /* web-vitals API surface changed — non-fatal */ }
    })();
  `;
  await page.addInitScript(init);
}

/**
 * Read current vitals from the page. Should be called at the END of the
 * journey (metrics like INP only finalize on page unload or inputs).
 */
export async function collectVitals(page: Page): Promise<VitalSnapshot> {
  const snap = await page.evaluate(() => (window as any).__lfiVitals || {});
  const out: VitalSnapshot = { capturedAt: Date.now() };
  if (typeof snap.lcp === 'number') out.lcp = { value: snap.lcp, band: bandLcp(snap.lcp) as any };
  if (typeof snap.cls === 'number') out.cls = { value: snap.cls, band: bandCls(snap.cls) as any };
  if (typeof snap.inp === 'number') out.inp = { value: snap.inp, band: bandInp(snap.inp) as any };
  if (typeof snap.ttfb === 'number') out.ttfb = { value: snap.ttfb };
  if (typeof snap.fcp === 'number') out.fcp = { value: snap.fcp };
  return out;
}
