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
import { diffReports, findPriorRun, type CapturedEvent, type Report } from './report.js';
import { captureAriaTree, ariaTreeToText, interactableNodes, scoreAriaTree } from './aria.js';
import { installWebVitals, collectVitals } from './webVitals.js';
import { attachTelemetry, snapshotMemory, captureServiceWorker } from './telemetry.js';
import { aggregate, renderSummary } from './aggregates.js';
import { runDiscover, type DiscoveredPage } from './discover.js';
import { runProbe } from './probe.js';
import { runStress } from './stress.js';
import { runAxe, axeEventsFor, renderAxeFindings, annotateViolations, type AxePageResult } from './audit.js';
import { runD0Audit, d0EventsFor, renderD0Findings, type D0PageResult } from './d0Audit.js';

interface Budget {
  newConsoleErrors: number;
  newPageErrors: number;
  newFailedRequests: number;
  newA11yViolations: number;
  newD0Violations: number;
  newlyBrokenSteps: number;
}

const DEFAULT_BUDGET: Budget = {
  newConsoleErrors: 0,
  newPageErrors: 0,
  newFailedRequests: 0,
  newA11yViolations: 0,
  newD0Violations: 0,
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
    // Env-var substitution. Any string containing ${ENV.NAME} is replaced
    // with process.env.NAME at load time. Lets a journey reference a
    // secret (e.g. ADMIN_CODE) without checking it into the repo. Missing
    // env vars expand to '' and the step typically fails informatively
    // (e.g. "admin-code rejected") rather than crashing the run.
    const subEnv = (v: any): any => {
      if (typeof v === 'string') return v.replace(/\$\{ENV\.([A-Z0-9_]+)\}/g, (_m, k) => process.env[k] ?? '');
      if (Array.isArray(v)) return v.map(subEnv);
      if (v && typeof v === 'object') { const o: any = {}; for (const [k, val] of Object.entries(v)) o[k] = subEnv(val); return o; }
      return v;
    };
    journey = subEnv(journey);
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

  // Preflight /health gate. If a journey declares a preflight list, hit
  // every URL once before launching Chromium. Bail with exit code 3 on
  // any failure — diagnoses "the sidecar is dead" in <500ms instead of
  // running the entire journey and swallowing the failure as opaque
  // network errors (AppArmor denials, systemd-down, port conflicts).
  if (Array.isArray((journey as any).preflight) && (journey as any).preflight.length > 0) {
    const pf = (journey as any).preflight as { name: string; url: string; expectStatus?: number }[];
    console.log(`[crawler] preflight: checking ${pf.length} health endpoints`);
    const failures: string[] = [];
    for (const entry of pf) {
      const expect = entry.expectStatus ?? 200;
      try {
        const ac = new AbortController();
        const t = setTimeout(() => ac.abort(), 5_000);
        const resp = await fetch(entry.url, { signal: ac.signal, redirect: 'manual' });
        clearTimeout(t);
        const ok = resp.status === expect;
        console.log(`  [${ok ? 'ok' : 'FAIL'}] ${entry.name.padEnd(28)} ${entry.url} → ${resp.status}`);
        if (!ok) failures.push(`${entry.name} (${entry.url}): expected ${expect}, got ${resp.status}`);
      } catch (e: any) {
        console.log(`  [FAIL] ${entry.name.padEnd(28)} ${entry.url} → ${e?.message || e}`);
        failures.push(`${entry.name} (${entry.url}): ${e?.message || e}`);
      }
    }
    if (failures.length > 0) {
      console.error(`[crawler] PREFLIGHT FAILED — ${failures.length} of ${pf.length} endpoints unhealthy`);
      for (const f of failures) console.error(`  - ${f}`);
      console.error('[crawler] aborting journey before browser launch — fix the unhealthy services and re-run');
      return 3;
    }
    console.log(`[crawler] preflight: all ${pf.length} endpoints healthy`);
  }

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

  // tsx/esbuild compiles `const fn = () => {}` and similar named function
  // expressions in this codebase by wrapping them with the `__name(fn, name)`
  // helper to preserve the function's displayed name. When such a function
  // body ships into the browser via `page.evaluate(...)`, the page-side
  // execution context has no `__name` in scope and throws
  // `ReferenceError: __name is not defined`, which then surfaces as a
  // pageerror and pollutes every journey's diff/budget. Define a no-op
  // pass-through globally on every page so esbuild's wrapping is harmless
  // in evaluate bodies. Must be a context-level addInitScript (not per-page)
  // so it runs before any first navigation triggers our axe scan.
  await context.addInitScript({
    content: 'window.__name = window.__name || function (fn) { return fn; };',
  });

  // WebAuthn virtual authenticator. When enabled, the runner attaches a
  // CDP virtual authenticator to the context; from that point on, any
  // navigator.credentials.create()/get() call goes through the virtual
  // authenticator instead of the (non-existent in headless Chromium)
  // platform authenticator. Required to test passkey enroll/login flows
  // — without this, the SPA's navigator.credentials.create() rejects
  // immediately with NotAllowedError.
  const wa = (journey as any).webauthn as Journey['webauthn'];
  if (wa?.enabled) {
    try {
      const cdpPage = await context.newPage();
      const client = await context.newCDPSession(cdpPage);
      await client.send('WebAuthn.enable', { enableUI: false } as any);
      const addRes: any = await client.send('WebAuthn.addVirtualAuthenticator' as any, {
        options: {
          protocol: wa.protocol || 'ctap2',
          transport: wa.transport || 'internal',
          hasResidentKey: wa.hasResidentKey ?? true,
          hasUserVerification: wa.hasUserVerification ?? true,
          automaticPresenceSimulation: wa.automaticPresenceSimulation ?? true,
          isUserVerified: wa.isUserVerified ?? true,
        },
      } as any);
      console.log(`[crawler] webauthn virtual authenticator attached (id=${addRes?.authenticatorId || '?'}, transport=${wa.transport || 'internal'})`);
      await cdpPage.close();
    } catch (e) {
      console.error(`[crawler] failed to attach virtual authenticator: ${(e as Error).message}`);
    }
  }

  // jsqr injection. assertQR steps run an in-page script that decodes
  // the QR <svg>; jsqr is read from node_modules and concatenated into
  // an addInitScript so window.__jsqr is available before the SPA's own
  // scripts. Keeps assertQR a pure JSON step — no per-test setup.
  try {
    // ESM has no require.resolve; the crawler is always run with cwd at
    // PlausiDen-Crawler/ root (npm script + run script), so node_modules
    // is reliable here.
    const jsqrPath = join(process.cwd(), 'node_modules', 'jsqr', 'dist', 'jsQR.js');
    const jsqrSrc = readFileSync(jsqrPath, 'utf8');
    await context.addInitScript({
      content: `${jsqrSrc}\n;try{window.__jsqr=window.jsQR||(window.module&&module.exports)||window.default;}catch(e){}`,
    });
  } catch (e) {
    console.log(`[crawler] WARN jsqr injection skipped: ${(e as Error).message}`);
  }

  // Seed sessionStorage on every page load. Runs before any of the SPA's
  // own JS, so the SPA boots already authenticated and never shows the
  // login form. Re-fires on every navigation within the context, which
  // is exactly what discover needs.
  if (svSeed?.sessionStorage) {
    const seedScript = `(() => { try { const seed = ${JSON.stringify(svSeed.sessionStorage)}; for (const [k, v] of Object.entries(seed)) sessionStorage.setItem(k, v); } catch {} })();`;
    await context.addInitScript({ content: seedScript });
    console.log(`[crawler] sessionStorage seeded with ${Object.keys(svSeed.sessionStorage).length} entries`);
  }

  // Journey-level seedStorage: pre-seed localStorage / sessionStorage with
  // arbitrary keys before navigation. Needed for cold-login journeys that
  // would otherwise be blocked by first-time popups (mission modal,
  // walkthrough overlay) — same mechanism the SPA uses to remember "user
  // already saw this," just primed before the SPA boots.
  if (journey.seedStorage?.localStorage) {
    const ls = journey.seedStorage.localStorage;
    const lsScript = `(() => { try { const seed = ${JSON.stringify(ls)}; for (const [k, v] of Object.entries(seed)) localStorage.setItem(k, v); } catch {} })();`;
    await context.addInitScript({ content: lsScript });
    console.log(`[crawler] journey localStorage seeded with ${Object.keys(ls).length} entries`);
  }
  if (journey.seedStorage?.sessionStorage) {
    const ss = journey.seedStorage.sessionStorage;
    const ssScript = `(() => { try { const seed = ${JSON.stringify(ss)}; for (const [k, v] of Object.entries(seed)) sessionStorage.setItem(k, v); } catch {} })();`;
    await context.addInitScript({ content: ssScript });
    console.log(`[crawler] journey sessionStorage seeded with ${Object.keys(ss).length} entries`);
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
                // Forcibly downgrade type=submit → type=button to suppress
                // the native form POST. React's onClick handler still fires
                // first, so SPA state still updates; we just refuse to let
                // the browser do a hard navigation if the form lacks an
                // onSubmit preventDefault.
                try { if (btn.type === 'submit') btn.type = 'button'; } catch (e2) {}
                btn.click();
                return;
              }
              // No fallback to form.submit()/requestSubmit() — those do a
              // hard navigation if React hasn't claimed onSubmit, and the
              // browser ends up on a raw JSON response. Always prefer the
              // SPA's keydown handler.
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
  // Per-screenshot D₀ runtime audit results — PlausiDen-specific design-
  // system enforcement (44×44 touch targets, 36px control min-height, etc.).
  // Complements axe-core (looser WCAG floor) and the build-time audits
  // (source-only) by checking actual rendered geometry.
  const screenshotD0: Array<{ url: string; result: D0PageResult }> = [];
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
  // Third-party noise we never control and that adds nothing to the audit:
  //   - Cloudflare RUM / NEL beacons (/cdn-cgi/rum, /cdn-cgi/beacon, etc.)
  //     get injected at the edge when RUM is enabled in the CF dashboard;
  //     they fail (request-failed with no status) on every page in headless
  //     Chromium because the beacon protocol expects a sendBeacon endpoint
  //     that the dashboard, not the origin, controls.
  //   - Cloudflare email-decode shim (/cdn-cgi/scripts/...email-decode.min.js)
  //     can fail in headless, same root cause.
  // Filtering at log-time (not by aborting the request) is the conservative
  // choice — the page still sees the same network behavior it would in a
  // real browser; we just don't count the failures against budget.
  const isThirdPartyEdgeNoise = (url: string): boolean =>
    /\/cdn-cgi\//.test(url);
  page.on('requestfailed', (req) => {
    if (isThirdPartyEdgeNoise(req.url())) return;
    log({ kind: 'request-failed', text: req.failure()?.errorText || 'unknown', url: req.url() });
  });
  page.on('response', (res) => {
    if (res.status() >= 400) {
      if (isThirdPartyEdgeNoise(res.url())) return;
      log({ kind: 'response-error', text: res.statusText(), url: res.url(), status: res.status() });
    }
  });

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
      // D₀ runtime audit — PlausiDen design-system enforcement on the
      // rendered page. Pure DOM/computed-style inspection; no script
      // injection, no CSP risk. Best-effort like axe.
      try {
        const d0Result = await runD0Audit(page);
        screenshotD0.push({ url: d0Result.url, result: d0Result });
        for (const ev of d0EventsFor(d0Result, startEpoch)) events.push(ev);
      } catch { /* d0 audit is best-effort */ }
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

    // Stress steps hammer the current page: random clicks, edge-case form
    // fills (XSS / SQLi / unicode / max-length / etc.), keyboard fuzz,
    // viewport thrash, optional network chaos. The runner sees every
    // surfaced console error / page error / failed fetch through the same
    // CapturedEvent stream, so the diff/budget logic catches regressions.
    if (step.kind === 'stress') {
      const cfg = step.stress || {};
      const stressStart = Date.now();
      console.log(`[crawler] stress starting (intensity=${cfg.intensity || 'high'} duration=${cfg.durationMs ?? 60_000}ms)`);
      const ctx = {
        outDir,
        startEpoch,
        log,
        takeScreenshot: async (label: string): Promise<string | null> => {
          const safe = label.replace(/[^A-Za-z0-9_-]/g, '_').slice(0, 60);
          const p = join(outDir, `stress-${safe}-${Date.now()}.png`);
          try { await page.screenshot({ path: p, fullPage: false }); return p; } catch { return null; }
        },
      };
      try {
        const result = await runStress(page, cfg, ctx);
        // Persist the stress summary alongside the report.
        writeFileSync(join(outDir, `stress-${step.label || 'run'}-${stressStart}.json`), JSON.stringify(result, null, 2));
        const summary = `clicks=${result.clicks.ok}/${result.clicks.attempted}(failed=${result.clicks.failed}) fills=${result.fills.ok}/${result.fills.attempted}(failed=${result.fills.failed}) keys=${result.keyboards.attempted} resizes=${result.resizes.attempted} chaos=${result.networkChaosTrips} dur=${result.durationMs}ms domNodes=${result.finalDom.nodes} errorRoles=${result.finalDom.errorRoles}`;
        stepResults.push({ step, index: i, ok: !result.abortReason, durationMs: result.durationMs, error: result.abortReason });
        console.log(`[crawler] stress complete: ${summary}`);
      } catch (e: any) {
        log({ kind: 'pageerror', text: `stress threw: ${String(e?.message || e).slice(0, 200)}` });
        stepResults.push({ step, index: i, ok: false, durationMs: Date.now() - stressStart, error: String(e?.message || e) });
      }
      // Settle after stress and re-check UI health before moving on.
      await page.waitForTimeout(500);
      await checkUiHealth(`after-stress:${step.label || ''}`);
      await snapshotMemory(page, `after-stress:${step.label || ''}`, startEpoch, telemetry);
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
      d0Violations: events.filter(e => e.kind === 'd0-violation').length,
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
  writeFileSync(join(outDir, 'report.json'), JSON.stringify(report, null, 2));
  // Write a terminal-friendly summary too so CI output is useful at a glance.
  writeFileSync(join(outDir, 'summary.txt'), renderSummary(agg));

  // Per-screenshot WCAG findings (axe-core), separate from discover sweep
  // findings. Both files share the same `renderAxeFindings` shape so a
  // human can read either without learning a second format.
  if (screenshotAxe.length > 0) {
    writeFileSync(join(outDir, 'findings.txt'), renderAxeFindings(screenshotAxe));
  }
  // Per-screenshot D₀ runtime audit findings — PlausiDen-specific design
  // system invariants (44×44 touch targets, 36px control min-height).
  // Lives in its own file so a triager can answer "did this run regress
  // the design system?" without scrolling through axe noise.
  if (screenshotD0.length > 0) {
    writeFileSync(join(outDir, 'd0-findings.txt'), renderD0Findings(screenshotD0));
  }

  const prior = findPriorRun(runsDir, outDir, journey.name);
  const allowedErrors = (journey as any).expectedErrors || [];
  const diff = diffReports(report, prior, allowedErrors);
  writeFileSync(join(outDir, 'diff.json'), JSON.stringify(diff, null, 2));

  // Summary to stdout.
  console.log(`\n[crawler] run complete: ${outDir}/report.json`);
  console.log(`  console errors:    ${report.counts.consoleErrors}`);
  console.log(`  page errors:       ${report.counts.pageErrors}`);
  console.log(`  failed fetches:    ${report.counts.failedRequests}`);
  console.log(`  a11y violations:   ${report.counts.a11yViolations}`);
  console.log(`  D₀ violations:     ${report.counts.d0Violations}`);
  console.log(`  steps ok/failed:   ${report.counts.stepsOk}/${report.counts.stepsFailed}`);
  if (prior) {
    console.log(`  diff vs prior run (${prior.journey}):`);
    console.log(`    NEW console errors:   ${diff.newConsoleErrors.length}`);
    console.log(`    NEW page errors:      ${diff.newPageErrors.length}`);
    console.log(`    NEW failed fetches:   ${diff.newFailedRequests.length}`);
    console.log(`    NEW a11y violations:  ${diff.newA11yViolations.length}`);
    console.log(`    NEW D₀ violations:    ${diff.newD0Violations.length}`);
    console.log(`    newly broken steps:   ${diff.newlyBrokenSteps.length}`);
    console.log(`    fixed steps:          ${diff.fixedSteps.length}`);
  } else {
    console.log(`  (no prior run to diff against)`);
  }

  // Per-journey budget override merges over DEFAULT_BUDGET. Used when a
  // journey legitimately surfaces a known-tolerated noise event (e.g.,
  // third-party CSP warnings outside our control).
  const jb = (journey as any).budget || {};
  const budget: Budget = {
    newConsoleErrors: jb.newConsoleErrors ?? DEFAULT_BUDGET.newConsoleErrors,
    newPageErrors: jb.newPageErrors ?? DEFAULT_BUDGET.newPageErrors,
    newFailedRequests: jb.newFailedRequests ?? DEFAULT_BUDGET.newFailedRequests,
    newA11yViolations: jb.newA11yViolations ?? DEFAULT_BUDGET.newA11yViolations,
    // D₀ has a separate budget knob so existing journeys don't all
    // immediately fail when this audit lands. Per-journey override lets
    // legacy pages opt in gradually as they migrate to kit primitives.
    newD0Violations: jb.newD0Violations ?? DEFAULT_BUDGET.newD0Violations,
    newlyBrokenSteps: jb.newlyBrokenSteps ?? DEFAULT_BUDGET.newlyBrokenSteps,
  };
  const overBudget =
    diff.newConsoleErrors.length > budget.newConsoleErrors
    || diff.newPageErrors.length > budget.newPageErrors
    || diff.newFailedRequests.length > budget.newFailedRequests
    || diff.newA11yViolations.length > budget.newA11yViolations
    || diff.newD0Violations.length > budget.newD0Violations
    || diff.newlyBrokenSteps.length > budget.newlyBrokenSteps;

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
