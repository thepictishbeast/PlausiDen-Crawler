/**
 * PlausiDen-Crawler v0.2 — web-focused runner.
 *
 * Usage:
 *   node --loader ts-node/esm src/main.ts [--url URL] [--journey FILE.json]
 *
 * - Loads a journey from journeys/<name>.json (or --journey path).
 * - Drives Chromium through each step.
 * - Captures console, page errors, failed fetches, 4xx/5xx responses.
 * - Runs axe-core (if installed) after each screenshot step.
 * - Writes runs/<ts>/report.json + per-step PNGs.
 * - Diffs against the previous run and exits non-zero on NEW regressions.
 */
import { chromium, type Page } from 'playwright';
import { mkdirSync, writeFileSync, readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { runStep, type Journey, type StepResult } from './journey.js';
import { diffReports, findPriorRun, renderPositiveSignal, type CapturedEvent, type Report } from './report.js';
import { captureAriaTree, ariaTreeToText, interactableNodes, scoreAriaTree } from './aria.js';
import { installWebVitals, collectVitals } from './webVitals.js';
import { attachTelemetry, snapshotMemory, captureServiceWorker } from './telemetry.js';
import { aggregate, renderSummary } from './aggregates.js';
import { runDiscover, type DiscoveredPage } from './discover.js';
import { runProbe } from './probe.js';
import { runAxe, axeEventsFor, renderAxeFindings, annotateViolations, type AxePageResult } from './audit.js';
import { captureCSSHealthSnapshot, detectCSSHealthIssues, type CSSHealthFinding } from './cssHealth.js';
import { captureUIOverflowSnapshot, detectUIOverflowIssues, type UIOverflowFinding } from './uiOverflow.js';
import { captureRuntimeContrastSnapshot, detectRuntimeContrastIssues, type RuntimeContrastFinding } from './runtimeContrast.js';
import { captureRuntimeImagesSnapshot, detectRuntimeImageIssues, type RuntimeImageFinding } from './runtimeImages.js';
import { captureRuntimeFocusSnapshot, detectRuntimeFocusIssues, type RuntimeFocusFinding } from './runtimeFocus.js';

interface Budget {
  newConsoleErrors: number;
  newPageErrors: number;
  newFailedRequests: number;
  newA11yViolations: number;
  newCssHealthStrict: number;
  newUiOverflowStrict: number;
  newRuntimeContrastStrict: number;
  newRuntimeImagesStrict: number;
  newRuntimeFocusStrict: number;
  newlyBrokenSteps: number;
}

const DEFAULT_BUDGET: Budget = {
  newConsoleErrors: 0,
  newPageErrors: 0,
  newFailedRequests: 0,
  newA11yViolations: 0,
  newCssHealthStrict: 0,
  newUiOverflowStrict: 0,
  newRuntimeContrastStrict: 0,
  newRuntimeImagesStrict: 0,
  newRuntimeFocusStrict: 0,
  newlyBrokenSteps: 0,
};

async function main(args: string[]): Promise<number> {
  const urlIdx = args.indexOf('--url');
  const journeyIdx = args.indexOf('--journey');
  const autoIdx = args.indexOf('--auto');

  // --auto <URL> mode: synthesize a one-step discover-only journey on the
  // fly. Useful for "I just want to point this at a site and see what's
  // there" without authoring a journey file.
  let journey: Journey;
  if (autoIdx >= 0 && args[autoIdx + 1]) {
    const url = args[autoIdx + 1];
    const slug = url.replace(/^https?:\/\//, '').replace(/[^A-Za-z0-9]/g, '-').slice(0, 60);
    const maxPagesArg = args.indexOf('--max-pages');
    const maxDepthArg = args.indexOf('--max-depth');
    const interactArg = args.indexOf('--interact');
    journey = {
      name: `auto-${slug}`,
      description: `Autonomous discovery sweep of ${url}`,
      baseUrl: url,
      steps: [
        { kind: 'goto', url, label: 'auto-start', timeout: 30_000 },
        { kind: 'wait', ms: 1_000 },
        { kind: 'screenshot', label: '00-start' },
        {
          kind: 'discover',
          label: 'auto-discover',
          url,
          discover: {
            maxPages: maxPagesArg >= 0 ? parseInt(args[maxPagesArg + 1], 10) : 50,
            maxDepth: maxDepthArg >= 0 ? parseInt(args[maxDepthArg + 1], 10) : 3,
            sameOrigin: true,
            interactButtons: (interactArg >= 0 ? args[interactArg + 1] : 'never') as any,
          },
        },
      ],
    };
  } else {
    // Resolve journey: explicit --journey path, or ./journeys/plausiden-smoke.json.
    let journeyPath = journeyIdx >= 0 ? args[journeyIdx + 1] : 'journeys/plausiden-smoke.json';
    if (!existsSync(journeyPath)) {
      console.error(`[crawler] journey not found: ${journeyPath}`);
      return 2;
    }
    journey = JSON.parse(readFileSync(journeyPath, 'utf8'));
  }
  const targetUrl = urlIdx >= 0 ? args[urlIdx + 1] : journey.baseUrl;

  // #crawler-v0.3 — viewport is now per-journey + CLI-overridable.
  // Resolution order:
  //   1. --viewport 375x667 flag
  //   2. journey.viewport = { w, h } in the JSON
  //   3. default 1280×900 (desktop)
  // Mobile-variant journeys (plausiden-smoke-mobile.json) declare the small viewport;
  // the same smoke journey can be re-run at different sizes by passing --viewport.
  const vpArgIdx = args.indexOf('--viewport');
  let viewport = { w: 1280, h: 900 };
  if (vpArgIdx >= 0 && args[vpArgIdx + 1]) {
    const m = /^(\d+)x(\d+)$/.exec(args[vpArgIdx + 1]);
    if (m) viewport = { w: parseInt(m[1], 10), h: parseInt(m[2], 10) };
  } else if ((journey as any).viewport) {
    const jv = (journey as any).viewport;
    if (typeof jv.w === 'number' && typeof jv.h === 'number') viewport = { w: jv.w, h: jv.h };
  }

  const tsTag = new Date().toISOString().replace(/[:.]/g, '-');
  const runsDir = 'runs';
  const outDir = join(runsDir, `${journey.name}-${tsTag}`);
  mkdirSync(outDir, { recursive: true });
  const startEpoch = Date.now();
  const events: CapturedEvent[] = [];
  const stepResults: StepResult[] = [];
  const log = (e: Omit<CapturedEvent, 't'>) => events.push({ ...e, t: Date.now() - startEpoch });

  // --headful runs Chromium with a visible UI. Required for the one-time
  // interactive login that produces a storageState file (admin/voter
  // journeys). HEADFUL=1 env var is the equivalent shortcut.
  const headfulIdx = args.indexOf('--headful');
  const headful = headfulIdx >= 0 || process.env.HEADFUL === '1';

  // --state <path>: load a Playwright storageState JSON (cookies +
  // localStorage) captured from a prior interactive login. Falls back to
  // journey.storageState if --state is not passed. Missing file = warn +
  // continue (anonymous) so the same crawler invocation works whether the
  // operator has captured credentials or not.
  // --save-state <path>: dump the current context's storageState to disk
  // at the end of the run. Used in conjunction with --headful + a manual
  // login step to capture credentials for re-use.
  const stateIdx = args.indexOf('--state');
  const saveStateIdx = args.indexOf('--save-state');
  const statePath = stateIdx >= 0 ? args[stateIdx + 1] : (journey as any).storageState;
  const saveStatePath = saveStateIdx >= 0 ? args[saveStateIdx + 1] : undefined;

  console.log(`[crawler] journey=${journey.name} target=${targetUrl}${headful ? ' [headful]' : ''}${statePath ? ' [state=' + statePath + ']' : ''}`);
  const browser = await chromium.launch({ headless: !headful });
  // bypassCSP only affects this headless test browser — real user
  // browsers still receive the production CSP unchanged. Without this,
  // strict-CSP sites (script-src 'self') reject our axe-core injection
  // and every WCAG scan fails with an engine error.
  const contextOpts: Parameters<typeof browser.newContext>[0] = {
    viewport: { width: viewport.w, height: viewport.h },
    bypassCSP: true,
  };

  // Two state-file formats are supported:
  //   1. Playwright storageState — { cookies: [...], origins: [{...localStorage}] }.
  //      Loaded by the context constructor; used by interactive logins captured
  //      via scripts/capture-login.sh.
  //   2. Sacred.Vote auth seed — { sessionStorage: {...}, autoGatekeeper, voterCode }.
  //      Written by scripts/seed-auth.sh. We can't use context.storageState for this
  //      because Playwright doesn't capture/restore sessionStorage. Instead we apply
  //      it via addInitScript after the context is created.
  let svSeed: { sessionStorage?: Record<string, string>; autoGatekeeper?: boolean; voterCode?: string; voterHash?: string } | null = null;
  if (statePath && existsSync(statePath)) {
    try {
      const raw = JSON.parse(readFileSync(statePath, 'utf8'));
      const isPwStorageState = Array.isArray(raw.cookies) || Array.isArray(raw.origins);
      const isSvSeed = !!raw.sessionStorage || !!raw.autoGatekeeper || !!raw.voterCode;
      if (isSvSeed && !isPwStorageState) {
        svSeed = raw;
        console.log(`[crawler] loaded sv-seed from ${statePath} (role=${raw.role || '?'})`);
      } else {
        contextOpts.storageState = statePath;
        console.log(`[crawler] loaded storageState from ${statePath}`);
      }
    } catch (e) {
      console.log(`[crawler] WARN failed to parse state file ${statePath}: ${(e as Error).message}`);
    }
  } else if (statePath) {
    console.log(`[crawler] WARN state ${statePath} not found — continuing anonymous`);
  }
  const context = await browser.newContext(contextOpts);

  // Seed sessionStorage on every page load. Runs before any of the SPA's
  // own JS, so the SPA boots already authenticated and never shows the
  // login form. Re-fires on every navigation within the context, which
  // is exactly what discover needs.
  if (svSeed?.sessionStorage) {
    const seedScript = `(() => { try { const seed = ${JSON.stringify(svSeed.sessionStorage)}; for (const [k, v] of Object.entries(seed)) sessionStorage.setItem(k, v); } catch {} })();`;
    await context.addInitScript({ content: seedScript });
    console.log(`[crawler] sessionStorage seeded with ${Object.keys(svSeed.sessionStorage).length} entries`);
  }

  // Auto-gatekeeper: detects the voter-ID input and submits the TEST code
  // automatically whenever the gatekeeper appears. /voting-app and
  // /dashboard re-mount their gatekeeper on every page reload (voter
  // session lives in React state only), so a one-shot fill+click step
  // is not enough — discover would lose auth on the next navigation.
  if (svSeed?.autoGatekeeper && svSeed?.voterCode) {
    const code = svSeed.voterCode;
    // Dismiss first-time popups that occlude the gatekeeper:
    //   - "About Sacred Vote" modal (localStorage: sacred-vote-mission-seen)
    //   - Disclaimer amber banner (sessionStorage: sv_disclaimer_dismissed)
    //   - Voter-booth walkthrough (localStorage: sv_walkthrough_*)
    // The "Before You Vote" legal modal is keyed by sv_legal_accepted_<first8>.
    // For voterCode="TEST", first8 is "TEST". Pre-setting that key suppresses
    // the modal so auto-fill flows straight to poll-select after submit.
    const legalKey = `sv_legal_accepted_${code.trim().substring(0, 8)}`;
    const dismiss = `
      (function() {
        try {
          localStorage.setItem('sacred-vote-mission-seen', '1');
          localStorage.setItem('sv_walkthrough_voterbooth_seen', '1');
          localStorage.setItem('sv_walkthrough_voting_seen', '1');
          localStorage.setItem(${JSON.stringify(legalKey)}, new Date().toISOString());
          sessionStorage.setItem('sv_disclaimer_dismissed', 'true');
        } catch (e) {}
      })();
    `;
    await context.addInitScript({ content: dismiss });
    const auto = `
      (function() {
        var voterCode = ${JSON.stringify(code)};
        var attempts = 0;
        // Pages this auto-filler should target. Anything else (registration,
        // recover, public verify ballot lookup, etc.) is left alone — we
        // don't want to type "TEST" into a registration form.
        function isGatekeeperPage() {
          var p = location.pathname;
          return p === '/' || p === '/voting-app' || p === '/dashboard' || p === '/verify-identity';
        }
        function findVoterInput() {
          var byId = document.querySelector('[data-testid="input-voter-id"]');
          if (byId) return byId;
          var inputs = document.querySelectorAll('input[type="text"], input:not([type])');
          for (var i = 0; i < inputs.length; i++) {
            var el = inputs[i];
            var ph = (el.getAttribute('placeholder') || '').toLowerCase();
            var name = (el.getAttribute('name') || '').toLowerCase();
            var id = (el.getAttribute('id') || '').toLowerCase();
            if (
              ph.indexOf('voter') >= 0 ||
              ph.indexOf('access code') >= 0 ||
              ph.indexOf('id number') >= 0 ||
              ph.indexOf('sv-') >= 0 ||
              name === 'voter_id' || name === 'voter-code' || id === 'voter_id'
            ) {
              return el;
            }
          }
          return null;
        }
        function findSubmitButton(input) {
          // Prefer the input's own form submit button.
          var form = input.closest('form');
          if (form) {
            var sub = form.querySelector('button[type="submit"], input[type="submit"]');
            if (sub) return sub;
          }
          // Known testids first.
          var byId = document.querySelector('[data-testid="button-proceed"]');
          if (byId) return byId;
          // Fallback: nearest enabled button labelled "Continue" / "Submit" /
          // "Proceed" — verify-identity has no form, just <Button onClick=...>.
          var btns = document.querySelectorAll('button');
          for (var i = 0; i < btns.length; i++) {
            var b = btns[i];
            if (b.disabled) continue;
            var t = (b.textContent || '').trim().toLowerCase();
            if (t === 'continue' || t === 'submit' || t === 'proceed' || t.indexOf('access dashboard') >= 0) return b;
          }
          return null;
        }
        function setNativeValue(el, value) {
          var desc = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value');
          if (desc && desc.set) desc.set.call(el, value);
          else el.value = value;
          el.dispatchEvent(new Event('input', { bubbles: true }));
          el.dispatchEvent(new Event('change', { bubbles: true }));
        }
        function pressEnter(el) {
          // verify-identity wires onKeyDown=Enter→handleCodeSubmit on the
          // input itself (no <form>), so an Enter keydown is the most
          // reliable submit trigger when the button isn't easy to find.
          var ev = new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true });
          el.dispatchEvent(ev);
        }
        function tryFill() {
          if (!isGatekeeperPage()) return true; // nothing to do here
          var input = findVoterInput();
          if (!input) return false;
          if (input.value === voterCode) return true; // already filled
          if (input.value && input.value !== voterCode) return true; // user typed something else, leave alone
          setNativeValue(input, voterCode);
          // After React reconciles the controlled value, click the submit
          // button if we can find one; otherwise dispatch Enter on the
          // input (verify-identity uses onKeyDown=Enter → handleCodeSubmit).
          setTimeout(function() {
            try {
              var btn = findSubmitButton(input);
              if (btn && !btn.disabled) {
                btn.click();
                return;
              }
              var form = input.closest('form');
              if (form) {
                form.requestSubmit ? form.requestSubmit() : form.submit();
                return;
              }
              pressEnter(input);
            } catch (e) {}
          }, 120);
          return true;
        }
        function tick() {
          if (attempts++ > 30) return;
          if (!tryFill()) setTimeout(tick, 400);
        }
        if (document.readyState === 'loading') {
          document.addEventListener('DOMContentLoaded', function() { setTimeout(tick, 200); });
        } else {
          setTimeout(tick, 200);
        }
      })();
    `;
    await context.addInitScript({ content: auto });
    console.log(`[crawler] auto-gatekeeper enabled (voter=${code})`);
  }

  const page: Page = await context.newPage();
  // Per-screenshot axe results — written to findings.txt at end of run
  // so the user can triage WCAG violations alongside the JSON report.
  const screenshotAxe: Array<{ url: string; result: AxePageResult; annotated?: string }> = [];
  // Inject Google's web-vitals library before any navigation so LCP/CLS/
  // INP/TTFB/FCP are captured on every page the crawler visits.
  await installWebVitals(page);
  // Rich telemetry: all requests (not just failures), long JS tasks,
  // memory snapshots, broken images, CSP violations, unhandled rejections.
  // Gated behind CRAWLER_RICH_TELEMETRY=1 while we iron out any
  // exposeFunction / init-script bugs; the addInitScript path has
  // historically destabilized the browser context during the first
  // page navigation on some Playwright versions.
  const richTelemetry = process.env.CRAWLER_RICH_TELEMETRY === '1';
  const { makeEmptyBundle } = await import('./telemetry.js');
  const telemetry = richTelemetry
    ? await attachTelemetry(page, startEpoch)
    : makeEmptyBundle();

  page.on('console', (msg) => {
    log({ kind: 'console', level: msg.type(), text: msg.text(), url: msg.location().url });
  });
  page.on('pageerror', (err) => {
    log({ kind: 'pageerror', text: err.message, stack: err.stack });
  });
  // T71: cssHealth needs per-URL response metadata (status,
  // content-type, body length, error text). The existing log()
  // stream is event-flavored; cssHealth wants a Map<url,obs>. So
  // both listeners co-exist: log() captures per-event for the
  // diff stream, cssHealthNetworkResponses captures per-URL for
  // the detector. Single response/requestfailed pair feeds both.
  const cssHealthNetworkResponses = new Map<
    string,
    { status: number; contentType: string | null; bodyBytes: number; errorText: string | null }
  >();
  page.on('requestfailed', (req) => {
    log({ kind: 'request-failed', text: req.failure()?.errorText || 'unknown', url: req.url() });
    cssHealthNetworkResponses.set(req.url(), {
      status: 0,
      contentType: null,
      bodyBytes: 0,
      errorText: req.failure()?.errorText || 'request failed',
    });
  });
  page.on('response', async (res) => {
    if (res.status() >= 400) {
      log({ kind: 'response-error', text: res.statusText(), url: res.url(), status: res.status() });
    }
    // Capture metadata for the css-health detector. Best-effort —
    // body() can throw (canceled requests, redirects). We only need
    // size for stylesheets, so it's worth the call cost.
    try {
      const headers = res.headers();
      let bodyBytes = 0;
      try {
        const body = await res.body();
        bodyBytes = body.length;
      } catch {
        bodyBytes = 0;
      }
      cssHealthNetworkResponses.set(res.url(), {
        status: res.status(),
        contentType: headers['content-type'] ?? null,
        bodyBytes,
        errorText: null,
      });
    } catch {
      /* response handler is best-effort */
    }
  });

  /**
   * T71: run cssHealth detector on the *current* DOM + network
   * state and append findings to the events stream.
   *
   * Called after every goto step. Discover/probe steps are skipped
   * because the page may navigate many times during them; the
   * detector would fire stale snapshots. Click/fill/wait/screenshot
   * steps are skipped because the DOM hasn't been *intentionally*
   * loaded — the detector's job is to validate page-load CSS, not
   * runtime DOM mutations.
   */
  /**
   * T79: runtime focus-visible detector. WCAG 2.4.7 AA.
   * Programmatically focuses every interactive element and
   * compares before/after computed styles — catches outline:0
   * with no border/box-shadow replacement.
   */
  const runtimeFocusFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: RuntimeFocusFinding[] }> = [];
  const checkRuntimeFocus = async (afterLabel: string) => {
    try {
      const snap = await captureRuntimeFocusSnapshot(page);
      const findings = detectRuntimeFocusIssues(snap);
      runtimeFocusFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'runtime-focus',
          text: `[${f.kind}] ${f.detail}`,
          url: snap.pageUrl,
          severity: f.severity,
          ruleId: f.kind,
          impact: f.severity === 'strict' ? 'serious' : 'minor',
        });
      }
    } catch (e) {
      log({ kind: 'pageerror', text: `[runtimeFocus] detector threw on ${afterLabel}: ${(e as Error).message}` });
    }
  };

  /**
   * T75: runtime image-health detector. Catches broken / empty /
   * missing-alt / CLS-risk images at the rendered DOM level.
   */
  const runtimeImagesFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: RuntimeImageFinding[] }> = [];
  const checkRuntimeImages = async (afterLabel: string) => {
    try {
      const snap = await captureRuntimeImagesSnapshot(page);
      const findings = detectRuntimeImageIssues(snap);
      runtimeImagesFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'runtime-images',
          text: `[${f.kind}] ${f.detail}`,
          url: snap.pageUrl,
          severity: f.severity,
          ruleId: f.kind,
          impact: f.severity === 'strict' ? 'serious' : 'minor',
        });
      }
    } catch (e) {
      log({ kind: 'pageerror', text: `[runtimeImages] detector threw on ${afterLabel}: ${(e as Error).message}` });
    }
  };

  /**
   * T29: runtime contrast detector. Walks every visible text node
   * and computes effective fg/bg contrast — catches dynamic-color
   * bugs that the build-time forge contrast phase misses.
   */
  const runtimeContrastFindingsByStep: Array<{ stepLabel: string; pageUrl: string; viewport: { width: number; height: number }; findings: RuntimeContrastFinding[] }> = [];
  const checkRuntimeContrast = async (afterLabel: string) => {
    try {
      const snap = await captureRuntimeContrastSnapshot(page);
      const findings = detectRuntimeContrastIssues(snap);
      runtimeContrastFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, viewport: snap.viewport, findings });
      for (const f of findings) {
        log({
          kind: 'runtime-contrast',
          text: `[${f.kind}] ${f.detail}`,
          url: snap.pageUrl,
          severity: f.severity,
          ruleId: f.kind,
          impact: f.severity === 'strict' ? 'serious' : 'minor',
        });
      }
    } catch (e) {
      log({ kind: 'pageerror', text: `[runtimeContrast] detector threw on ${afterLabel}: ${(e as Error).message}` });
    }
  };

  /**
   * T28: ui-overflow + tap-target detector. Runs after each goto in
   * the same place as cssHealth.
   */
  const uiOverflowFindingsByStep: Array<{ stepLabel: string; pageUrl: string; viewport: { width: number; height: number }; findings: UIOverflowFinding[] }> = [];
  const checkUiOverflow = async (afterLabel: string) => {
    try {
      const snap = await captureUIOverflowSnapshot(page);
      const findings = detectUIOverflowIssues(snap);
      uiOverflowFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, viewport: snap.viewport, findings });
      for (const f of findings) {
        log({
          kind: 'ui-overflow',
          text: `[${f.kind}] ${f.detail}`,
          url: snap.pageUrl,
          severity: f.severity,
          ruleId: f.kind,
          impact: f.severity === 'strict' ? 'serious' : 'minor',
        });
      }
    } catch (e) {
      log({
        kind: 'pageerror',
        text: `[uiOverflow] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  const cssHealthFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: CSSHealthFinding[] }> = [];
  const checkCssHealth = async (afterLabel: string) => {
    try {
      const snap = await captureCSSHealthSnapshot(page, cssHealthNetworkResponses);
      const findings = detectCSSHealthIssues(snap);
      cssHealthFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'css-health',
          text: `[${f.kind}] ${f.detail}`,
          url: snap.pageUrl,
          severity: f.severity,
          ruleId: f.kind,
          impact: f.severity === 'strict' ? 'serious' : 'minor',
        });
      }
    } catch (e) {
      // Detector failure is itself worth surfacing — but as a soft
      // page error, not a css-health finding (avoid recursive
      // claims about css when the detector itself broke).
      log({
        kind: 'pageerror',
        text: `[cssHealth] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  // Deeper heuristics that the surface-level event capture misses:
  //  - WebSocket failures (onclose with non-1000 code)
  //  - "Could not load …" / "Backend busy" / "Unexpected token" error
  //    strings rendered by error boundaries / subtab alerts
  //  - Blank view: <main> or #root has <100 chars of text after a step
  //  - Long-pending <Suspense> fallback still visible 5s after a step
  // These are checked after each step via evaluateHandle and pushed as
  // synthetic CapturedEvents so the diff algorithm catches them.
  const checkUiHealth = async (afterLabel: string) => {
    try {
      const findings = await page.evaluate(() => {
        const out: { kind: string; text: string }[] = [];
        // Visible error copy that users would read as a bug.
        const errorPatterns = [
          /could not load/i,
          /backend busy/i,
          /backend offline/i,
          /unexpected token/i,
          /unrecognized verdict bucket/i,
          /reference.?error/i,
          /cannot read prop/i,
          /ui error/i,
        ];
        const text = document.body.innerText || '';
        for (const p of errorPatterns) {
          const m = text.match(p);
          if (m) out.push({ kind: 'ui-error-text', text: `Rendered error copy matched /${p.source}/: "${text.slice(Math.max(0, m.index! - 20), m.index! + 80)}"` });
        }
        // Suspense-fallback-looking text still on screen (view never hydrated).
        const loadingFallbacks = text.match(/Loading\s+(classroom|fleet|library|auditorium|admin|knowledge)/gi);
        if (loadingFallbacks && loadingFallbacks.length > 0) {
          out.push({ kind: 'stuck-loading', text: `Fallback copy still visible: ${loadingFallbacks.join(', ')}` });
        }
        // Blank-main: look for a visible <main> with almost no content.
        const main = document.querySelector('main');
        if (main) {
          const mt = (main as HTMLElement).innerText || '';
          if (mt.trim().length < 20 && (main as HTMLElement).offsetHeight > 200) {
            out.push({ kind: 'blank-main', text: `Main area rendered with <20 chars of visible text (height: ${(main as HTMLElement).offsetHeight}px).` });
          }
        }
        // Hidden-but-active error boundary card.
        const boundaryCard = document.querySelector('[role="alert"]');
        if (boundaryCard) {
          const t = (boundaryCard as HTMLElement).innerText || '';
          if (t.trim().length > 0) {
            out.push({ kind: 'error-boundary-visible', text: `role=alert present with text: "${t.slice(0, 120)}"` });
          }
        }
        return out;
      });
      for (const f of findings) {
        log({ kind: 'pageerror' as const, text: `[after step: ${afterLabel}] [${f.kind}] ${f.text}` });
      }
    } catch { /* page may have navigated — skip */ }
  };

  // WebSocket close tracking. Hook into CDP so we see genuine WS drops.
  try {
    const client = await page.context().newCDPSession(page);
    await client.send('Network.enable');
    client.on('Network.webSocketClosed', (ev: any) => {
      log({ kind: 'response-error', text: `WebSocket closed`, url: String(ev?.requestId || 'ws') });
    });
    client.on('Network.webSocketFrameError', (ev: any) => {
      log({ kind: 'pageerror', text: `WebSocket frame error: ${ev?.errorMessage || 'unknown'}` });
    });
  } catch { /* CDP unavailable on some platforms */ }

  // Execute each step sequentially. Screenshot steps are handled inline
  // (runStep is a no-op for them) so we can track the filename.
  for (let i = 0; i < journey.steps.length; i++) {
    const step = journey.steps[i];
    console.log(`[crawler] step ${i + 1}/${journey.steps.length}: ${step.kind}${step.label ? ' · ' + step.label : ''}`);

    // Screenshot steps: take the shot AND an accessibility snapshot.
    // Aria snapshots are the visionless-AI equivalent — a compact
    // semantic tree an LLM can reason about without pixel input.
    if (step.kind === 'screenshot') {
      const base = `${String(i + 1).padStart(2, '0')}-${step.label || 'shot'}`;
      const imgPath = join(outDir, `${base}.png`);
      const ariaPath = join(outDir, `${base}.aria.txt`);
      try { await page.screenshot({ path: imgPath, fullPage: true }); } catch { /* silent */ }
      try {
        const tree = await captureAriaTree(page);
        const text = ariaTreeToText(tree);
        const inter = interactableNodes(tree).map(n => `${n.role} "${n.name || '(unnamed)'}"`);
        const a11y = scoreAriaTree(tree);
        const body = [
          `# Aria snapshot — ${step.label || step.kind}`,
          `# URL: ${page.url()}`,
          `# Interactable count: ${inter.length}`,
          `# A11y warnings: ${a11y.score}${a11y.flags.length ? ' (' + a11y.flags.slice(0, 8).join('; ') + ')' : ''}`,
          '',
          text,
          '',
          '# --- Interactable nodes (LLM-friendly flat list) ---',
          ...inter,
        ].join('\n');
        writeFileSync(ariaPath, body);
        // If we find accessibility violations, log them as diag events
        // so the diff surfaces NEW a11y warnings across runs.
        for (const f of a11y.flags.slice(0, 10)) {
          log({ kind: 'a11y-violation', text: `${f} on step ${step.label || step.kind}`, impact: 'moderate' });
        }
      } catch { /* aria capture is best-effort */ }
      // Real axe-core scan — picks up contrast, ARIA misuse, missing
      // labels, etc. that the aria-tree heuristic can't see. Annotated
      // screenshot per-step has red outlines on every flagged element.
      try {
        const axeResult = await runAxe(page);
        let annotated: string | undefined;
        if (axeResult.ok && axeResult.violations.length > 0) {
          const annPath = join(outDir, `${base}.annotated.png`);
          annotated = await annotateViolations(page, axeResult, annPath);
        }
        screenshotAxe.push({ url: axeResult.url, result: axeResult, annotated });
        for (const ev of axeEventsFor(axeResult, startEpoch)) events.push(ev);
      } catch { /* axe is best-effort */ }
      stepResults.push({ step, index: i, ok: true, durationMs: 0, screenshot: imgPath });
      continue;
    }

    // Discover steps switch the runner into autonomous BFS mode for the
    // duration of the step. The discover module captures aria/screenshot/
    // events itself and returns a DiscoverResult that we splice into the
    // main report. Use the step.url as the start URL if provided, else
    // whatever URL the page is currently on.
    if (step.kind === 'discover') {
      const startUrl = step.url || page.url();
      console.log(`[crawler] discover starting at ${startUrl}`);
      const cfg = step.discover || {};
      const result = await runDiscover(page, startUrl, cfg, outDir, startEpoch, log);
      // Splice virtual step results + events into the main run.
      stepResults.push(...result.stepResults);
      for (const ev of result.events) events.push(ev);
      // Persist the per-page discovery details next to the report.
      writeFileSync(
        join(outDir, 'discover-pages.json'),
        JSON.stringify(result.pages, null, 2),
      );
      console.log(`[crawler] discover complete: ${result.pages.length} pages, ${result.events.length} events`);
      continue;
    }

    // Probe steps fire malformed-URL fuzz against int/hex/UUID-shaped
    // path segments harvested from prior discover output (or listed
    // explicitly in the journey). 5xx, body echoes, hash-prefix leaks,
    // and unexpected 200s on garbage input become CapturedEvents so the
    // diff/budget logic catches regressions across runs.
    if (step.kind === 'probe') {
      const cfg = step.probe || {};
      const explicitUrls = cfg.urls || (step.url ? [step.url] : []);
      console.log(`[crawler] probe starting (explicit=${explicitUrls.length}, inheritDiscover=${cfg.inheritDiscoverUrls !== false})`);
      const result = await runProbe(page, explicitUrls, cfg, outDir, startEpoch, log);
      stepResults.push(...result.stepResults);
      for (const ev of result.events) events.push(ev);
      console.log(`[crawler] probe complete: ${result.findings.length} findings, ${result.totalRequests} requests, ${result.templatesProbed} templates`);
      continue;
    }

    const result = await runStep(page, step);
    result.index = i;
    stepResults.push(result);
    if (!result.ok) {
      log({ kind: 'pageerror', text: `step failed: ${step.label || step.kind}: ${result.error}` });
    }
    // Give the page a beat to settle after interactive steps.
    await page.waitForTimeout(200);
    // Deep heuristics check — error copy, blank main, stuck loading.
    await checkUiHealth(step.label || step.kind);
    // T71: run cssHealth detector after every goto. Skipped on
    // wait/click/fill — those steps don't reload the page, so the
    // last goto's snapshot would still apply. Skipped on press/
    // type as well for the same reason.
    if (step.kind === 'goto') {
      await checkCssHealth(step.label || `goto-${i}`);
      await checkUiOverflow(step.label || `goto-${i}`);
      await checkRuntimeContrast(step.label || `goto-${i}`);
      await checkRuntimeImages(step.label || `goto-${i}`);
      await checkRuntimeFocus(step.label || `goto-${i}`);
    }
    // Memory snapshot at end of each step so the report shows heap growth
    // across the journey. Cheap (one page.evaluate call).
    await snapshotMemory(page, step.label || step.kind, startEpoch, telemetry);
  }

  // Capture service-worker state once before close.
  await captureServiceWorker(page, telemetry);
  // Save current cookies + localStorage so a subsequent run can re-enter
  // the authenticated session without another manual login. Done before
  // browser.close() so the context is still alive.
  if (saveStatePath) {
    try {
      await context.storageState({ path: saveStatePath });
      console.log(`[crawler] storageState saved to ${saveStatePath}`);
    } catch (e) {
      console.error(`[crawler] failed to save storageState: ${(e as Error).message}`);
    }
  }
  await browser.close();

  // Walk through events and bucket them by step — answers the user's
  // question "what was the crawler doing when this log happened?"
  // Each event.t is ms since run start. Steps don't carry their own
  // start offset, so we compute it from the cumulative durationMs.
  const stepWindows: Array<{ start: number; end: number; step: StepResult }> = [];
  {
    let cursor = 0;
    for (const s of stepResults) {
      const start = cursor;
      const end = cursor + Math.max(s.durationMs || 0, 100) + 200; // inclusive of the 200ms post-step settle
      stepWindows.push({ start, end, step: s });
      cursor = end;
    }
  }
  const eventsByStep = stepWindows.map(({ start, end, step }) => ({
    stepIndex: step.index,
    stepLabel: step.step.label || step.step.kind,
    stepKind: step.step.kind,
    windowMs: [start, end] as [number, number],
    events: events.filter(e => e.t >= start && e.t <= end),
  })).filter(b => b.events.length > 0);

  // Aggregate the rich telemetry bundle into actionable leaderboards.
  const agg = aggregate(telemetry);

  const report: Report & { telemetry?: typeof telemetry; aggregates?: typeof agg } = {
    target: targetUrl,
    journey: journey.name,
    viewport,
    started: new Date(startEpoch).toISOString(),
    durationMs: Date.now() - startEpoch,
    counts: {
      consoleErrors: events.filter(e => e.kind === 'console' && e.level === 'error').length,
      pageErrors: events.filter(e => e.kind === 'pageerror').length,
      failedRequests: events.filter(e => e.kind === 'request-failed' || e.kind === 'response-error').length,
      a11yViolations: events.filter(e => e.kind === 'a11y-violation').length,
      cssHealthFindings: events.filter(e => e.kind === 'css-health').length,
      cssHealthFindingsStrict: events.filter(e => e.kind === 'css-health' && e.severity === 'strict').length,
      uiOverflowFindings: events.filter(e => e.kind === 'ui-overflow').length,
      uiOverflowFindingsStrict: events.filter(e => e.kind === 'ui-overflow' && e.severity === 'strict').length,
      runtimeContrastFindings: events.filter(e => e.kind === 'runtime-contrast').length,
      runtimeContrastFindingsStrict: events.filter(e => e.kind === 'runtime-contrast' && e.severity === 'strict').length,
      runtimeImagesFindings: events.filter(e => e.kind === 'runtime-images').length,
      runtimeImagesFindingsStrict: events.filter(e => e.kind === 'runtime-images' && e.severity === 'strict').length,
      runtimeFocusFindings: events.filter(e => e.kind === 'runtime-focus').length,
      runtimeFocusFindingsStrict: events.filter(e => e.kind === 'runtime-focus' && e.severity === 'strict').length,
      total: events.length,
      stepsOk: stepResults.filter(s => s.ok).length,
      stepsFailed: stepResults.filter(s => !s.ok).length,
    },
    events,
    steps: stepResults,
    eventsByStep,
    telemetry,
    aggregates: agg,
  };
  // Per-step cssHealth findings, separate file so operators can
  // grep one place for "what CSS broke and where".
  if (cssHealthFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'css-health.json'),
      JSON.stringify(cssHealthFindingsByStep, null, 2),
    );
  }
  if (uiOverflowFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'ui-overflow.json'),
      JSON.stringify(uiOverflowFindingsByStep, null, 2),
    );
  }
  if (runtimeContrastFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'runtime-contrast.json'),
      JSON.stringify(runtimeContrastFindingsByStep, null, 2),
    );
  }
  if (runtimeImagesFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'runtime-images.json'),
      JSON.stringify(runtimeImagesFindingsByStep, null, 2),
    );
  }
  if (runtimeFocusFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'runtime-focus.json'),
      JSON.stringify(runtimeFocusFindingsByStep, null, 2),
    );
  }
  writeFileSync(join(outDir, 'report.json'), JSON.stringify(report, null, 2));
  // Write a terminal-friendly summary too so CI output is useful at a glance.
  writeFileSync(join(outDir, 'summary.txt'), renderSummary(agg));

  // Per-screenshot WCAG findings (axe-core), separate from discover sweep
  // findings. Both files share the same `renderAxeFindings` shape so a
  // human can read either without learning a second format.
  if (screenshotAxe.length > 0) {
    writeFileSync(join(outDir, 'findings.txt'), renderAxeFindings(screenshotAxe));
  }

  const prior = findPriorRun(runsDir, outDir);
  const diff = diffReports(report, prior);
  writeFileSync(join(outDir, 'diff.json'), JSON.stringify(diff, null, 2));

  // Summary to stdout.
  console.log(`\n[crawler] run complete: ${outDir}/report.json`);
  console.log(`  console errors:    ${report.counts.consoleErrors}`);
  console.log(`  page errors:       ${report.counts.pageErrors}`);
  console.log(`  failed fetches:    ${report.counts.failedRequests}`);
  console.log(`  a11y violations:   ${report.counts.a11yViolations}`);
  console.log(`  css health:        ${report.counts.cssHealthFindings} (strict ${report.counts.cssHealthFindingsStrict})`);
  console.log(`  ui overflow:       ${report.counts.uiOverflowFindings} (strict ${report.counts.uiOverflowFindingsStrict})`);
  console.log(`  runtime contrast:  ${report.counts.runtimeContrastFindings} (strict ${report.counts.runtimeContrastFindingsStrict})`);
  console.log(`  runtime images:    ${report.counts.runtimeImagesFindings} (strict ${report.counts.runtimeImagesFindingsStrict})`);
  console.log(`  runtime focus:     ${report.counts.runtimeFocusFindings} (strict ${report.counts.runtimeFocusFindingsStrict})`);
  console.log(`  steps ok/failed:   ${report.counts.stepsOk}/${report.counts.stepsFailed}`);
  if (prior) {
    console.log(`  diff vs prior run (${prior.journey}):`);
    console.log(`    NEW console errors:   ${diff.newConsoleErrors.length}`);
    console.log(`    NEW page errors:      ${diff.newPageErrors.length}`);
    console.log(`    NEW failed fetches:   ${diff.newFailedRequests.length}`);
    console.log(`    NEW a11y violations:  ${diff.newA11yViolations.length}`);
    const newCssHealthStrict = diff.newCssHealthFindings.filter(e => e.severity === 'strict').length;
    const newCssHealthWarn = diff.newCssHealthFindings.length - newCssHealthStrict;
    console.log(`    NEW css health:       ${diff.newCssHealthFindings.length} (strict ${newCssHealthStrict}, warn ${newCssHealthWarn})`);
    const newUiOverflowStrict = diff.newUiOverflowFindings.filter(e => e.severity === 'strict').length;
    const newUiOverflowWarn = diff.newUiOverflowFindings.length - newUiOverflowStrict;
    console.log(`    NEW ui overflow:      ${diff.newUiOverflowFindings.length} (strict ${newUiOverflowStrict}, warn ${newUiOverflowWarn})`);
    const newRuntimeContrastStrict = diff.newRuntimeContrastFindings.filter(e => e.severity === 'strict').length;
    const newRuntimeContrastWarn = diff.newRuntimeContrastFindings.length - newRuntimeContrastStrict;
    console.log(`    NEW runtime contrast: ${diff.newRuntimeContrastFindings.length} (strict ${newRuntimeContrastStrict}, warn ${newRuntimeContrastWarn})`);
    const newRuntimeImagesStrict = diff.newRuntimeImagesFindings.filter(e => e.severity === 'strict').length;
    const newRuntimeImagesWarn = diff.newRuntimeImagesFindings.length - newRuntimeImagesStrict;
    console.log(`    NEW runtime images:   ${diff.newRuntimeImagesFindings.length} (strict ${newRuntimeImagesStrict}, warn ${newRuntimeImagesWarn})`);
    const newRuntimeFocusStrict = diff.newRuntimeFocusFindings.filter(e => e.severity === 'strict').length;
    const newRuntimeFocusWarn = diff.newRuntimeFocusFindings.length - newRuntimeFocusStrict;
    console.log(`    NEW runtime focus:    ${diff.newRuntimeFocusFindings.length} (strict ${newRuntimeFocusStrict}, warn ${newRuntimeFocusWarn})`);
    console.log(`    newly broken steps:   ${diff.newlyBrokenSteps.length}`);
    console.log(`    fixed steps:          ${diff.fixedSteps.length}`);
  } else {
    console.log(`  (no prior run to diff against)`);
  }

  const newCssHealthStrictCount = diff.newCssHealthFindings.filter(e => e.severity === 'strict').length;
  const newUiOverflowStrictCount = diff.newUiOverflowFindings.filter(e => e.severity === 'strict').length;
  const newRuntimeContrastStrictCount = diff.newRuntimeContrastFindings.filter(e => e.severity === 'strict').length;
  const newRuntimeImagesStrictCount = diff.newRuntimeImagesFindings.filter(e => e.severity === 'strict').length;
  const newRuntimeFocusStrictCount = diff.newRuntimeFocusFindings.filter(e => e.severity === 'strict').length;
  const overBudget =
    diff.newConsoleErrors.length > DEFAULT_BUDGET.newConsoleErrors
    || diff.newPageErrors.length > DEFAULT_BUDGET.newPageErrors
    || diff.newFailedRequests.length > DEFAULT_BUDGET.newFailedRequests
    || diff.newA11yViolations.length > DEFAULT_BUDGET.newA11yViolations
    || newCssHealthStrictCount > DEFAULT_BUDGET.newCssHealthStrict
    || newUiOverflowStrictCount > DEFAULT_BUDGET.newUiOverflowStrict
    || newRuntimeContrastStrictCount > DEFAULT_BUDGET.newRuntimeContrastStrict
    || newRuntimeImagesStrictCount > DEFAULT_BUDGET.newRuntimeImagesStrict
    || newRuntimeFocusStrictCount > DEFAULT_BUDGET.newRuntimeFocusStrict
    || diff.newlyBrokenSteps.length > DEFAULT_BUDGET.newlyBrokenSteps;

  // T2: positive-signal report — make the silent-pass state legible.
  // Always emit (PASS or FAIL); also write to disk for audit trail.
  const positive = renderPositiveSignal(report, diff);
  console.log('\n' + positive);
  writeFileSync(join(outDir, 'positive-signal.txt'), positive + '\n');

  if (overBudget) {
    console.log('\n[crawler] FAIL — new regressions exceed budget.');
    return 1;
  }
  console.log('\n[crawler] PASS — no new regressions.');
  return 0;
}

main(process.argv.slice(2)).then((code) => process.exit(code)).catch((e) => {
  console.error('[crawler] fatal:', e);
  process.exit(2);
});
