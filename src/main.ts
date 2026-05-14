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
import { mkdirSync, writeFileSync, readFileSync, existsSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { runStep, type Journey, type StepResult } from './journey.js';
import { diffReports, findPriorRun, renderPositiveSignal, compareAriaTrees, type CapturedEvent, type Report } from './report.js';
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
import { captureHeadingOrderSnapshot, detectHeadingOrderIssues, type HeadingOrderFinding } from './headingOrder.js';
import { captureRuntimeLandmarksSnapshot, detectRuntimeLandmarksIssues, type RuntimeLandmarksFinding } from './runtimeLandmarks.js';
import { captureLinkTextSnapshot, detectLinkTextIssues, type LinkTextFinding } from './linkText.js';
import { capturePlaceholderTextSnapshot, detectPlaceholderTextIssues, type PlaceholderTextFinding } from './placeholderText.js';
import { captureTapTargetsSnapshot, detectTapTargetIssues, type TapTargetFinding } from './tapTargets.js';
import { captureFormLabelsSnapshot, detectFormLabelIssues, type FormLabelFinding } from './formLabels.js';
import { captureViewportMetaSnapshot, detectViewportMetaIssues, type ViewportMetaFinding } from './viewportMeta.js';
import { captureDocTitleSnapshot, detectDocTitleIssues, type DocTitleFinding } from './docTitle.js';
import { captureHtmlLangSnapshot, detectHtmlLangIssues, type HtmlLangFinding } from './htmlLang.js';
import { captureSkipLinkSnapshot, detectSkipLinkIssues, type SkipLinkFinding } from './skipLink.js';
import { captureOutboundLinksSnapshot, detectOutboundLinkIssues, type OutboundLinkFinding } from './outboundLinks.js';
import { captureAutocompleteSnapshot, detectAutocompleteIssues, type AutocompleteFinding } from './autocomplete.js';
import { captureMetaDescriptionSnapshot, detectMetaDescriptionIssues, type MetaDescriptionFinding } from './metaDescription.js';
import { captureFaviconSnapshot, detectFaviconIssues, type FaviconFinding } from './favicon.js';
import { captureMixedContentSnapshot, detectMixedContentIssues, type MixedContentFinding } from './mixedContent.js';
import { captureLinkUnderlineSnapshot, detectLinkUnderlineIssues, type LinkUnderlineFinding } from './linkUnderline.js';
import { newCrossPageTitleAccumulator, recordPageTitle, detectCrossPageTitleDuplicates } from './crossPageTitle.js';
import { newCrossPageMetaDescriptionAccumulator, recordPageMetaDescription, detectCrossPageMetaDescriptionDuplicates } from './crossPageMetaDescription.js';
import { buildHstsSnapshot, detectHstsIssues, type HstsFinding } from './hstsHeader.js';
import { buildXFrameOptionsSnapshot, detectXFrameOptionsIssues, type XFrameOptionsFinding } from './xFrameOptions.js';

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
  newWebVitalsStrict: number;
  newCspViolations: number;
  // T83: strict aria-drift findings (>30% line-delta) were silently
  // missing from the gate. Default budget is 0 — any strict drift
  // blocks ship. Warn-band drift (10-30%) stays advisory.
  newAriaDriftStrict: number;
  // T16: axes that were registered in the positive-signal table but
  // not wired into the gate. Closing the gap so a strict regression
  // in any axis blocks ship — otherwise a "REGRESSION (1 strict)"
  // line in the table coexists with a top-level PASS, which is a
  // contradiction the operator can't trust.
  newHeadingOrderStrict: number;
  newRuntimeLandmarksStrict: number;
  newLinkTextStrict: number;
  newPlaceholderTextStrict: number;
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
  newWebVitalsStrict: 0,
  newCspViolations: 0,
  newAriaDriftStrict: 0,
  newHeadingOrderStrict: 0,
  newRuntimeLandmarksStrict: 0,
  newLinkTextStrict: 0,
  newPlaceholderTextStrict: 0,
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

  // T54: high-zoom emulation for WCAG 1.4.4 (resize text 200%) and
  // 1.4.10 (reflow at 320 CSS px). The journey field `zoom` is a
  // percent (200 = 200%); we override the root font-size proportional
  // to it so every rem cascades larger. Containers that use raw px
  // for height/width — instead of rem or fit-content — will visibly
  // overflow under zoom; the existing uiOverflow/runtimeContrast/
  // runtimeFocus detectors then fire on the broken state.
  //
  // BUG ASSUMPTION: pages that use `body { font-size: 16px }`
  // explicitly will override the :root override. We hit `:root`,
  // `html`, AND `body` via a single rule to defeat that, and use
  // !important since user-agent-stylesheet zoom is itself !important.
  const zoomPct = (journey as { zoom?: number }).zoom;
  if (typeof zoomPct === 'number' && zoomPct > 100) {
    const zoomFactor = zoomPct / 100;
    const fontSizePx = Math.round(16 * zoomFactor);
    const inj = `(function(){
      var s = document.createElement('style');
      s.id = '__loom_zoom_emul__';
      s.textContent = ':root, html, body { font-size: ${fontSizePx}px !important; }';
      function place(){
        if (document.documentElement && !document.getElementById('__loom_zoom_emul__')) {
          document.documentElement.appendChild(s);
        }
      }
      if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', place);
      } else {
        place();
      }
    })();`;
    await context.addInitScript({ content: inj });
    console.log(`[crawler] zoom=${zoomPct}% (root font-size: ${fontSizePx}px) — WCAG 1.4.4 text-resize emulation`);
  }

  // T50: first-time-visitor emulation. Wipes localStorage,
  // sessionStorage, and document.cookie at document_start on EVERY
  // navigation so each page-load is indistinguishable from "user
  // hits this URL with no prior history". Catches code paths that
  // assume defaults/preferences/auth-tokens already exist —
  // missing onboarding triggers, broken empty states, JS errors
  // when reading absent keys.
  //
  // BUG ASSUMPTION: pages that set a cookie THEN immediately read
  // it (sync, same tick) will see the wipe + write + read. Pages
  // that write on load and read on next-load see only the new
  // load's wipe — that's exactly what "first-time" means.
  if ((journey as { firstTime?: boolean }).firstTime) {
    const wipe = `(function(){
      try { localStorage.clear(); } catch (e) {}
      try { sessionStorage.clear(); } catch (e) {}
      try {
        // Cookie clear: iterate document.cookie and expire each one.
        var cookies = document.cookie ? document.cookie.split('; ') : [];
        for (var i = 0; i < cookies.length; i++) {
          var name = cookies[i].split('=')[0];
          document.cookie = name + '=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/';
        }
      } catch (e) {}
    })();`;
    await context.addInitScript({ content: wipe });
    console.log('[crawler] firstTime=true — localStorage/sessionStorage/cookies wiped on every navigation');
  }

  // T51: screen-reader UX sweep. Runs after page load and emits
  // console.warn for SR-specific structural issues that axe doesn't
  // always flag at default-rule strictness:
  //   1. aria-hidden=true on tab-focusable elements (the SR
  //      announces nothing but Tab still lands there → confusing).
  //   2. Heading-level skip (h1 → h3) which breaks heading nav.
  //   3. Roles requiring an accessible name with neither
  //      aria-label nor aria-labelledby nor associated <label>.
  //   4. <main> count != 1 (SR uses landmark nav and benefits
  //      from a single primary main region).
  // The diff against prior run flags any new SR ergonomic
  // regression on the very next forge build.
  if ((journey as { screenReader?: boolean }).screenReader) {
    const sr = `(function(){
      function audit(){
        var msgs = [];
        // 1. focusable + aria-hidden
        var focusableHidden = document.querySelectorAll('[aria-hidden="true"]:not([tabindex="-1"])');
        focusableHidden.forEach(function(el){
          if (el.matches('a[href], button, [tabindex]:not([tabindex="-1"]), input:not([type=hidden]), select, textarea')) {
            msgs.push('[sr.focus-hidden] aria-hidden=true on focusable: ' + (el.tagName + (el.id ? '#'+el.id : '')));
          }
        });
        // 2. heading skip
        var hs = Array.prototype.slice.call(document.querySelectorAll('h1,h2,h3,h4,h5,h6'));
        var prev = 0;
        hs.forEach(function(h){
          var lvl = parseInt(h.tagName.slice(1), 10);
          if (prev > 0 && lvl > prev + 1) {
            msgs.push('[sr.heading-skip] H' + prev + ' to H' + lvl + ' (skipped): "' + (h.textContent||'').trim().slice(0,40) + '"');
          }
          prev = lvl;
        });
        // 3. role-requires-name
        var labelable = document.querySelectorAll('[role="button"], [role="link"], [role="checkbox"], [role="tab"], [role="menuitem"], [role="switch"]');
        labelable.forEach(function(el){
          var hasName = !!(el.getAttribute('aria-label') || el.getAttribute('aria-labelledby') || (el.textContent||'').trim());
          if (!hasName) {
            msgs.push('[sr.role-no-name] role=' + el.getAttribute('role') + ' without accessible name: ' + el.tagName);
          }
        });
        // 4. <main> count
        var mains = document.querySelectorAll('main, [role="main"]');
        if (mains.length === 0) {
          msgs.push('[sr.no-main] no <main> or role=main on page (landmark navigation degraded)');
        } else if (mains.length > 1) {
          msgs.push('[sr.too-many-main] ' + mains.length + ' main landmarks (should be exactly 1)');
        }
        // Emit each msg as a console.warn so the diff axis catches it.
        msgs.forEach(function(m){ try { console.warn(m); } catch(e){} });
        // Diagnostic heartbeat — always emits one info line per page
        // so we can verify the audit ran. Counts aren't "found
        // problems" — they're page structural totals (aria-hidden
        // candidates, headings, role-labelable elements, mains).
        // Actual SR-ergonomic warns are in msgs above.
        try {
          console.info('[sr.audit] aria-hidden-cands=' + focusableHidden.length +
                       ' headings=' + hs.length +
                       ' role-labelable=' + labelable.length +
                       ' main=' + mains.length +
                       ' issues=' + msgs.length);
        } catch (e) {}
      }
      if (document.readyState === 'complete') {
        setTimeout(audit, 100);
      } else {
        window.addEventListener('load', function(){ setTimeout(audit, 100); });
      }
    })();`;
    await context.addInitScript({ content: sr });
    console.log('[crawler] screenReader=true — SR ergonomics audit injected on every navigation');
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

  // T80: always-on CSP-violation listener. The browser fires
  // 'securitypolicyviolation' on the document for every blocked
  // resource. We capture each as a diff-axis event so a future
  // accidental inline-style or unauthorized CDN gets flagged.
  // (telemetry.ts has a richer version gated behind
  // CRAWLER_RICH_TELEMETRY=1; this is the always-on minimum.)
  await page.exposeFunction('__crawler_cspViolation', (info: { directive: string; blockedURI: string; sourceFile?: string; lineNumber?: number }) => {
    log({
      kind: 'csp-violation',
      text: `${info.directive} blocked ${info.blockedURI || 'inline'}`,
      url: info.sourceFile || '',
      severity: 'strict',
      ruleId: `csp.${info.directive}`,
      impact: 'serious',
    });
  });
  await page.addInitScript(`
    document.addEventListener('securitypolicyviolation', (ev) => {
      // T72: previously this used try/catch to swallow ALL errors,
      // which masked bugs (exposeFunction not yet wired, listener
      // attached to wrong document, etc.) — fixture audits showed
      // the cspViolations axis silent on a known-violating page.
      // Now we emit a console.warn fallback so a missing handler
      // surfaces as a regular console error in the report.
      var info = {
        directive: ev.violatedDirective || '',
        blockedURI: ev.blockedURI || '',
        sourceFile: ev.sourceFile || '',
        lineNumber: ev.lineNumber || 0
      };
      // Always log to console so the event is observable even if
      // the exposeFunction binding hasn't reached this frame yet.
      // The kind='console' axis will catch this as a backup signal
      // — paired with the dedicated 'csp-violation' kind that the
      // exposeFunction handler emits.
      try {
        console.warn('[crawler.csp] ' + info.directive + ' blocked ' +
                     (info.blockedURI || 'inline') + ' @ ' +
                     info.sourceFile + ':' + info.lineNumber);
      } catch (e) { /* swallowed: console may be detached */ }
      try {
        if (typeof window.__crawler_cspViolation === 'function') {
          window.__crawler_cspViolation(info);
        }
      } catch (e) { /* swallowed: binding may race with iframe nav */ }
    }, { capture: true });
  `);

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
  // T76 (2026-05-14): hstsHeader detector reads response headers
  // from this Map. Populated in the response handler below for
  // EVERY top-level navigation response (request().isNavigationRequest()).
  // Indexed by the final URL (post-redirect) so a goto to
  // http://x.com that 301s to https://x.com produces a record
  // for the https URL.
  const topLevelResponseHeaders = new Map<string, Record<string, string>>();
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
      // T76: stash FULL headers for top-level navigation responses
      // so the hstsHeader detector can read Strict-Transport-Security
      // (and any future header-flavoured detectors don't need their
      // own listener).
      if (res.request().isNavigationRequest()) {
        topLevelResponseHeaders.set(res.url(), headers);
      }
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
   * T45: web-vitals detector. webVitals.ts already injects Google's
   * library at context init; we collect after each goto and convert
   * poor-band measurements into events.
   *
   * Thresholds match Google Core Web Vitals (good < 2.5s LCP, etc.).
   * Strict-fail any 'poor' band; warn 'needs-improvement'.
   */
  const webVitalsByStep: Array<{ stepLabel: string; pageUrl: string; vitals: any }> = [];
  const checkWebVitals = async (afterLabel: string) => {
    try {
      const snap = await collectVitals(page);
      webVitalsByStep.push({ stepLabel: afterLabel, pageUrl: page.url(), vitals: snap });
      const checks: Array<{ name: 'lcp' | 'cls' | 'inp'; m?: { value: number; band: string } }> = [
        { name: 'lcp', m: snap.lcp },
        { name: 'cls', m: snap.cls },
        { name: 'inp', m: snap.inp },
      ];
      for (const c of checks) {
        if (!c.m) continue;
        if (c.m.band === 'poor') {
          log({
            kind: 'web-vitals',
            text: `[vitals.${c.name}-poor] ${c.name.toUpperCase()}=${c.m.value} on ${afterLabel}`,
            url: page.url(),
            severity: 'strict',
            ruleId: `vitals.${c.name}-poor`,
            impact: 'serious',
          });
        } else if (c.m.band === 'needs-improvement') {
          log({
            kind: 'web-vitals',
            text: `[vitals.${c.name}-nti] ${c.name.toUpperCase()}=${c.m.value} on ${afterLabel}`,
            url: page.url(),
            severity: 'warn',
            ruleId: `vitals.${c.name}-nti`,
            impact: 'minor',
          });
        }
      }
    } catch (e) {
      // Silent — web-vitals may not have fired yet.
    }
  };

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
   * T104: heading-order detector. Walks h1-h6 in DOM order, flags
   * pages with !=1 h1 (strict) and any level skip h2→h4 etc (warn).
   * Mirrors crates/crawler-detectors/src/heading_order.rs — keep
   * the kind+severity strings in sync between the two impls.
   */
  const headingOrderFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: HeadingOrderFinding[] }> = [];
  const checkHeadingOrder = async (afterLabel: string) => {
    try {
      const snap = await captureHeadingOrderSnapshot(page);
      const findings = detectHeadingOrderIssues(snap);
      headingOrderFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'heading-order',
          text: `[${f.kind}] ${f.detail}`,
          url: snap.pageUrl,
          severity: f.severity,
          ruleId: f.kind,
          impact: f.severity === 'strict' ? 'serious' : 'minor',
        });
      }
    } catch (e) {
      log({ kind: 'pageerror', text: `[headingOrder] detector threw on ${afterLabel}: ${(e as Error).message}` });
    }
  };

  /**
   * T105: runtime-landmarks detector. Counts main/banner/contentinfo,
   * flags duplicates + same-role nesting. Mirrors
   * crates/crawler-detectors/src/runtime_landmarks.rs.
   */
  const runtimeLandmarksFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: RuntimeLandmarksFinding[] }> = [];
  const checkRuntimeLandmarks = async (afterLabel: string) => {
    try {
      const snap = await captureRuntimeLandmarksSnapshot(page);
      const findings = detectRuntimeLandmarksIssues(snap);
      runtimeLandmarksFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'runtime-landmarks',
          text: `[${f.kind}] ${f.detail}`,
          url: snap.pageUrl,
          severity: f.severity,
          ruleId: f.kind,
          impact: f.severity === 'strict' ? 'serious' : 'minor',
        });
      }
    } catch (e) {
      log({ kind: 'pageerror', text: `[runtimeLandmarks] detector threw on ${afterLabel}: ${(e as Error).message}` });
    }
  };

  /**
   * T106: link-text detector. WCAG 2.4.4 link purpose. Mirrors
   * crates/crawler-detectors/src/link_text.rs.
   */
  const linkTextFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: LinkTextFinding[] }> = [];
  const checkLinkText = async (afterLabel: string) => {
    try {
      const snap = await captureLinkTextSnapshot(page);
      const findings = detectLinkTextIssues(snap);
      linkTextFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'link-text',
          text: `[${f.kind}] ${f.detail}`,
          url: snap.pageUrl,
          severity: f.severity,
          ruleId: f.kind,
          impact: f.severity === 'strict' ? 'serious' : 'minor',
        });
      }
    } catch (e) {
      log({ kind: 'pageerror', text: `[linkText] detector threw on ${afterLabel}: ${(e as Error).message}` });
    }
  };

  /**
   * T16: placeholder-text detector. Lorem ipsum / dev-marker / template
   * leakage in the rendered DOM. One finding per category per page.
   */
  const placeholderTextFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: PlaceholderTextFinding[] }> = [];
  const checkPlaceholderText = async (afterLabel: string) => {
    try {
      const snap = await capturePlaceholderTextSnapshot(page);
      const findings = detectPlaceholderTextIssues(snap);
      placeholderTextFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'placeholder-text',
          text: `[${f.kind}] ${f.detail}`,
          url: snap.pageUrl,
          severity: f.severity,
          ruleId: f.kind,
          impact: f.severity === 'strict' ? 'serious' : 'minor',
        });
      }
    } catch (e) {
      log({ kind: 'pageerror', text: `[placeholderText] detector threw on ${afterLabel}: ${(e as Error).message}` });
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
   * T76: document title quality detector. Per-page check covering
   * missing/empty titles (strict), generic Word/IDE leftovers,
   * and length boundaries (warn). Title is the strongest single
   * signal for SEO and is what screen readers announce first.
   */
  const docTitleFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: DocTitleFinding[] }> = [];
  // T76 (2026-05-14): aggregates-layer accumulator for the
  // crossPageTitleDup detector. Populated piggyback off the
  // docTitle capture so we don't pay a second page.evaluate.
  const crossPageTitleAcc = newCrossPageTitleAccumulator();
  const checkDocTitle = async (afterLabel: string) => {
    try {
      const snap = await captureDocTitleSnapshot(page);
      const findings = detectDocTitleIssues(snap);
      docTitleFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      // Record the title for the cross-page aggregates pass.
      // The accumulator handles empty/whitespace skipping.
      recordPageTitle(crossPageTitleAcc, snap.pageUrl, snap.raw);
      for (const f of findings) {
        log({
          kind: 'doc-title',
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
        text: `[docTitle] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: skip-link detector. WCAG 2.4.1 (Bypass Blocks, Level A).
   * Finds the page's skip-to-content link and validates it works:
   * target exists, link is first focusable, link isn't permanently
   * hidden.
   */
  const skipLinkFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: SkipLinkFinding[] }> = [];
  const checkSkipLink = async (afterLabel: string) => {
    try {
      const snap = await captureSkipLinkSnapshot(page);
      const findings = detectSkipLinkIssues(snap);
      skipLinkFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'skip-link',
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
        text: `[skipLink] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: link-underline detector. WCAG 1.4.1 (Use of Color, A).
   * Flags inline links inside running text that distinguish
   * themselves from surrounding text ONLY by colour — fails
   * for ~8% of users (red-green colourblind) and many low-
   * contrast environments.
   */
  const linkUnderlineFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: LinkUnderlineFinding[] }> = [];
  const checkLinkUnderline = async (afterLabel: string) => {
    try {
      const snap = await captureLinkUnderlineSnapshot(page);
      const findings = detectLinkUnderlineIssues(snap);
      linkUnderlineFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'link-underline',
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
        text: `[linkUnderline] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: clickjacking-defence detector. Second consumer of
   * `topLevelResponseHeaders`. Reads X-Frame-Options +
   * Content-Security-Policy frame-ancestors and reports
   * pages with neither.
   */
  const xFrameOptionsFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: XFrameOptionsFinding[] }> = [];
  const checkXFrameOptions = async (afterLabel: string) => {
    try {
      const pageUrl = page.url();
      const headers = topLevelResponseHeaders.get(pageUrl);
      const snap = buildXFrameOptionsSnapshot(pageUrl, headers);
      const findings = detectXFrameOptionsIssues(snap);
      xFrameOptionsFindingsByStep.push({ stepLabel: afterLabel, pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'x-frame-options',
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
        text: `[xFrameOptions] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: HSTS response-header detector. SECURITY-flavoured.
   * Reads the Strict-Transport-Security header from the top-
   * level navigation response captured in
   * `topLevelResponseHeaders`. Short-circuits on http pages
   * and localhost.
   */
  const hstsFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: HstsFinding[] }> = [];
  const checkHsts = async (afterLabel: string) => {
    try {
      const pageUrl = page.url();
      const headers = topLevelResponseHeaders.get(pageUrl);
      const snap = buildHstsSnapshot(pageUrl, headers);
      const findings = detectHstsIssues(snap);
      hstsFindingsByStep.push({ stepLabel: afterLabel, pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'hsts',
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
        text: `[hsts] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: mixed-content detector. SECURITY-flavoured. Catches
   * https pages loading http resources via static markup
   * analysis — more reliable than the runtime browser signal
   * since browsers inconsistently auto-upgrade vs warn vs
   * block.
   */
  const mixedContentFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: MixedContentFinding[] }> = [];
  const checkMixedContent = async (afterLabel: string) => {
    try {
      const snap = await captureMixedContentSnapshot(page);
      const findings = detectMixedContentIssues(snap);
      mixedContentFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'mixed-content',
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
        text: `[mixedContent] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: favicon detector. warn-only — missing favicon doesn't
   * break the page but ships a generic browser-tab glyph and
   * reads as unfinished. Companion finding (broken icon URL)
   * is already covered by the failed-requests axis.
   */
  const faviconFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: FaviconFinding[] }> = [];
  const checkFavicon = async (afterLabel: string) => {
    try {
      const snap = await captureFaviconSnapshot(page);
      const findings = detectFaviconIssues(snap);
      faviconFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'favicon',
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
        text: `[favicon] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: meta-description detector. SEO + social-share preview
   * quality. All warn — no strict because a missing description
   * doesn't break the page; it just suboptimizes discovery.
   */
  const metaDescriptionFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: MetaDescriptionFinding[] }> = [];
  // T76 (2026-05-14): aggregates-layer accumulator for the
  // crossPageMetaDescription detector. Same pattern as
  // crossPageTitleAcc — piggyback off the per-page capture so
  // no extra page.evaluate cost.
  const crossPageMetaDescriptionAcc = newCrossPageMetaDescriptionAccumulator();
  const checkMetaDescription = async (afterLabel: string) => {
    try {
      const snap = await captureMetaDescriptionSnapshot(page);
      const findings = detectMetaDescriptionIssues(snap);
      metaDescriptionFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      recordPageMetaDescription(crossPageMetaDescriptionAcc, snap.pageUrl, snap.raw);
      for (const f of findings) {
        log({
          kind: 'meta-description',
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
        text: `[metaDescription] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: form autocomplete-attribute detector. WCAG 1.3.5
   * (Identify Input Purpose, AA). Strict on missing autocomplete
   * for credential fields (email/password/username); warn on
   * missing for PII (name/phone/address/etc.) or invalid tokens.
   */
  const autocompleteFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: AutocompleteFinding[] }> = [];
  const checkAutocomplete = async (afterLabel: string) => {
    try {
      const snap = await captureAutocompleteSnapshot(page);
      const findings = detectAutocompleteIssues(snap);
      autocompleteFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'autocomplete',
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
        text: `[autocomplete] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: outbound-link safety detector. SECURITY-flavoured: catches
   * tabbing-vulnerable target=_blank links (no rel=noopener),
   * explicit rel=opener (the worst), and outbound links missing
   * rel=noreferrer (Referer-leak through analytics).
   */
  const outboundLinksFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: OutboundLinkFinding[] }> = [];
  const checkOutboundLinks = async (afterLabel: string) => {
    try {
      const snap = await captureOutboundLinksSnapshot(page);
      const findings = detectOutboundLinkIssues(snap);
      outboundLinksFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'outbound-links',
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
        text: `[outboundLinks] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: <html lang> attribute detector. WCAG 3.1.1 (Language of
   * Page, Level A). Strict on missing/empty; warn on structurally
   * invalid BCP-47 or unknown-primary subtag (catches typos).
   */
  const htmlLangFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: HtmlLangFinding[] }> = [];
  const checkHtmlLang = async (afterLabel: string) => {
    try {
      const snap = await captureHtmlLangSnapshot(page);
      const findings = detectHtmlLangIssues(snap);
      htmlLangFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'html-lang',
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
        text: `[htmlLang] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: viewport meta tag detector. WCAG 1.4.10 (Reflow, AA) +
   * 1.4.4 (Resize text, AA). Catches missing tag, no
   * width=device-width, and zoom-disabling content (user-scalable=no
   * / maximum-scale ≤ 1) — three of the most common mobile-UX
   * failures and an explicit accessibility blocker for low-vision
   * users.
   */
  const viewportMetaFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: ViewportMetaFinding[] }> = [];
  const checkViewportMeta = async (afterLabel: string) => {
    try {
      const snap = await captureViewportMetaSnapshot(page);
      const findings = detectViewportMetaIssues(snap);
      viewportMetaFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'viewport-meta',
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
        text: `[viewportMeta] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: form-label association detector. WCAG 1.3.1 + 4.1.2 + 3.3.2.
   * Catches no-label / placeholder-only / required-no-indicator —
   * the three highest-impact form-UX bugs in real applications.
   */
  const formLabelsFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: FormLabelFinding[] }> = [];
  const checkFormLabels = async (afterLabel: string) => {
    try {
      const snap = await captureFormLabelsSnapshot(page);
      const findings = detectFormLabelIssues(snap);
      formLabelsFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'form-labels',
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
        text: `[formLabels] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
    }
  };

  /**
   * T76: tap-target size detector. WCAG 2.5.8 (24×24 AA strict) +
   * 2.5.5 (44×44 AAA warn). Runs after each goto so per-step
   * regressions are visible. Mobile UX defect — top-3 most
   * common usability issue per WebAIM 2024 survey.
   */
  const tapTargetsFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: TapTargetFinding[] }> = [];
  const checkTapTargets = async (afterLabel: string) => {
    try {
      const snap = await captureTapTargetsSnapshot(page);
      const findings = detectTapTargetIssues(snap);
      tapTargetsFindingsByStep.push({ stepLabel: afterLabel, pageUrl: snap.pageUrl, findings });
      for (const f of findings) {
        log({
          kind: 'tap-targets',
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
        text: `[tapTargets] detector threw on step ${afterLabel}: ${(e as Error).message}`,
      });
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

  // WebSocket close tracking + T53 network-throttling. Hook into
  // CDP for both. Throttle profile lives in journey.throttle:
  //   "slow-3g"    150ms RTT,  400 Kbps down, 400 Kbps up
  //   "fast-3g"    100ms RTT, 1.6 Mbps down, 750 Kbps up
  //   "regular-4g"  20ms RTT,  4 Mbps down, 3 Mbps up
  //   "offline"     network disabled
  //   <unset>       no throttling
  try {
    const client = await page.context().newCDPSession(page);
    await client.send('Network.enable');
    const throttle = (journey as { throttle?: string }).throttle;
    if (throttle) {
      const profiles: Record<string, { latency: number; downloadThroughput: number; uploadThroughput: number; offline: boolean }> = {
        'slow-3g':    { latency: 150, downloadThroughput:  50_000, uploadThroughput:  50_000, offline: false },
        'fast-3g':    { latency: 100, downloadThroughput: 200_000, uploadThroughput:  93_750, offline: false },
        'regular-4g': { latency:  20, downloadThroughput: 500_000, uploadThroughput: 375_000, offline: false },
        'offline':    { latency:   0, downloadThroughput:       0, uploadThroughput:       0, offline: true  },
      };
      const profile = profiles[throttle];
      if (profile) {
        await client.send('Network.emulateNetworkConditions', profile);
        console.log(`[crawler] throttle=${throttle} (RTT ${profile.latency}ms, ${(profile.downloadThroughput * 8 / 1000).toFixed(0)} Kbps down)`);
      } else {
        console.log(`[crawler] WARN unknown throttle "${throttle}" — ignored`);
      }
    }
    client.on('Network.webSocketClosed', (ev: any) => {
      log({ kind: 'response-error', text: `WebSocket closed`, url: String(ev?.requestId || 'ws') });
    });
    client.on('Network.webSocketFrameError', (ev: any) => {
      log({ kind: 'pageerror', text: `WebSocket frame error: ${ev?.errorMessage || 'unknown'}` });
    });
    // T84: CSP violations via CDP Audits domain. The DOM
    // 'securitypolicyviolation' event listener installed in
    // addInitScript above proved unreliable in Playwright's
    // headless Chromium — the listener attaches to a transient
    // document at document_start, the meta-CSP / header-CSP
    // block fires the violation against a Document object that
    // gets torn down before our exposeFunction binding is
    // visible. The Audits domain is connection-scoped (lives on
    // the CDPSession) and surfaces every violation via
    // Audits.issueAdded with code='ContentSecurityPolicyIssue'.
    // T72 fixture-perf-csp /csp-violation/ confirms the route
    // produces the event when Audits is enabled.
    try {
      await client.send('Audits.enable');
      let issueCount = 0;
      client.on('Audits.issueAdded', (ev: any) => {
        issueCount++;
        const issue = ev?.issue;
        const code = issue?.code || 'unknown';
        if (code !== 'ContentSecurityPolicyIssue') return;
        const d = issue?.details?.cspIssueDetails || {};
        const directive = String(d.violatedDirective || 'unknown');
        const blocked = String(d.blockedURL || d.sourceCodeLocation?.url || 'inline');
        const violationType = String(d.contentSecurityPolicyViolationType || 'unknown');
        log({
          kind: 'csp-violation',
          text: `[csp.${directive}] ${violationType} blocked ${blocked}`,
          url: d.sourceCodeLocation?.url || '',
          severity: 'strict',
          ruleId: `csp.${directive}`,
          impact: 'serious',
        });
      });
    } catch { /* Audits domain unavailable */ }
    // T84 second-line: CDP Log.entryAdded captures Chromium's
    // internal browser-emitted log entries — including CSP-block
    // notices that don't surface through page.on('console') in
    // headless mode. Catches what Audits misses.
    try {
      await client.send('Log.enable');
      client.on('Log.entryAdded', (ev: any) => {
        const entry = ev?.entry;
        if (!entry) return;
        // Only interested in CSP-relevant log entries here. The
        // `source` field on Chromium log entries is one of:
        //   xml | javascript | network | storage | appcache |
        //   rendering | security | deprecation | worker |
        //   violation | intervention | recommendation | other
        // CSP enforcement lands in `security` source with text
        // starting "Refused to" or "Content Security Policy".
        const src = String(entry.source || '');
        const txt = String(entry.text || '');
        if (src === 'security' && /content security policy|refused to (execute|load|connect|frame|run)/i.test(txt)) {
          log({
            kind: 'csp-violation',
            text: `[csp.${src}] ${txt}`,
            url: String(entry.url || ''),
            severity: 'strict',
            ruleId: 'csp.cdp-log',
            impact: 'serious',
          });
        }
      });
    } catch { /* Log domain unavailable */ }
  } catch { /* CDP unavailable on some platforms */ }

  // Execute each step sequentially. Screenshot steps are handled inline
  // (runStep is a no-op for them) so we can track the filename.
  // T76 fix: track WALL-CLOCK start/end of each step's full
  // processing (including the per-step detector calls AFTER
  // runStep returns). Without this, the per-step grouping in
  // report.eventsByStep used cumulative durationMs, which
  // under-counts the detector wall-clock time and causes
  // findings to spill into the next step's bucket. URL-based
  // grouping (used by scripts/check-t76-detectors.sh) doesn't
  // have this problem, but ANYTHING else that consumes
  // eventsByStep does — including a future visualizer.
  const stepClockWindows: Array<{ stepIndex: number; startedT: number; endedT: number }> = [];
  for (let i = 0; i < journey.steps.length; i++) {
    const step = journey.steps[i];
    const stepStartedT = Date.now() - startEpoch;
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
      stepClockWindows.push({ stepIndex: i, startedT: stepStartedT, endedT: Date.now() - startEpoch });
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
      stepClockWindows.push({ stepIndex: i, startedT: stepStartedT, endedT: Date.now() - startEpoch });
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
      stepClockWindows.push({ stepIndex: i, startedT: stepStartedT, endedT: Date.now() - startEpoch });
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
      // T76 (2026-05-14): linkUnderline runs FIRST — it inspects
      // computed styles which other detectors (focus simulation,
      // contrast walks) can mutate transiently. Catching the
      // pristine state avoids false negatives from sibling
      // detector side-effects.
      await checkLinkUnderline(step.label || `goto-${i}`);
      await checkCssHealth(step.label || `goto-${i}`);
      await checkUiOverflow(step.label || `goto-${i}`);
      await checkRuntimeContrast(step.label || `goto-${i}`);
      await checkRuntimeImages(step.label || `goto-${i}`);
      await checkRuntimeFocus(step.label || `goto-${i}`);
      await checkHeadingOrder(step.label || `goto-${i}`);
      await checkRuntimeLandmarks(step.label || `goto-${i}`);
      await checkLinkText(step.label || `goto-${i}`);
      await checkPlaceholderText(step.label || `goto-${i}`);
      await checkTapTargets(step.label || `goto-${i}`);
      await checkFormLabels(step.label || `goto-${i}`);
      await checkViewportMeta(step.label || `goto-${i}`);
      await checkDocTitle(step.label || `goto-${i}`);
      await checkHtmlLang(step.label || `goto-${i}`);
      await checkSkipLink(step.label || `goto-${i}`);
      await checkOutboundLinks(step.label || `goto-${i}`);
      await checkAutocomplete(step.label || `goto-${i}`);
      await checkMetaDescription(step.label || `goto-${i}`);
      await checkFavicon(step.label || `goto-${i}`);
      await checkMixedContent(step.label || `goto-${i}`);
      await checkHsts(step.label || `goto-${i}`);
      await checkXFrameOptions(step.label || `goto-${i}`);
      await checkWebVitals(step.label || `goto-${i}`);
    }
    // Memory snapshot at end of each step so the report shows heap growth
    // across the journey. Cheap (one page.evaluate call).
    await snapshotMemory(page, step.label || step.kind, startEpoch, telemetry);
    stepClockWindows.push({ stepIndex: i, startedT: stepStartedT, endedT: Date.now() - startEpoch });
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

  // T76 (2026-05-14): aggregates-layer detectors run AFTER the
  // goto loop, walking accumulated cross-page state. The first
  // such detector is crossPageTitleDup — every page in the
  // journey having the SAME <title> is a real SEO + UX defect
  // (tabs / bookmarks / history / search snippets indistinguishable).
  //
  // Future aggregates detectors (cross-page metaDescription dup,
  // cross-page H1 dup, etc.) will follow this same shape: an
  // accumulator that's piggy-backed off existing per-page
  // captures, and a single end-of-journey emit step here.
  for (const f of detectCrossPageTitleDuplicates(crossPageTitleAcc)) {
    log({
      kind: 'cross-page-title',
      text: `[${f.kind}] ${f.detail}`,
      severity: f.severity,
      ruleId: f.kind,
      impact: f.severity === 'strict' ? 'serious' : 'minor',
    });
  }
  for (const f of detectCrossPageMetaDescriptionDuplicates(crossPageMetaDescriptionAcc)) {
    log({
      kind: 'cross-page-meta-description',
      text: `[${f.kind}] ${f.detail}`,
      severity: f.severity,
      ruleId: f.kind,
      impact: f.severity === 'strict' ? 'serious' : 'minor',
    });
  }

  // Walk through events and bucket them by step — answers the user's
  // question "what was the crawler doing when this log happened?"
  // Each event.t is ms since run start.
  //
  // T76 fix (2026-05-14): use the WALL-CLOCK windows captured during
  // the goto loop (stepClockWindows). Previous logic used cumulative
  // s.durationMs which only measured the page action — not the
  // ~17 detector page.evaluate calls that happen AFTER runStep
  // returns. Detector findings landed in the NEXT step's window
  // (caught by scripts/check-t76-detectors.sh on the
  // t76-detector-fixtures journey).
  //
  // The new windows are aligned to actual elapsed time. Map step
  // index → result so the bucket carries the same shape downstream
  // consumers expect.
  const stepResultByIndex = new Map<number, StepResult>();
  for (const s of stepResults) stepResultByIndex.set(s.index, s);
  const eventsByStep = stepClockWindows.map(({ stepIndex, startedT, endedT }) => {
    const step = stepResultByIndex.get(stepIndex);
    return {
      stepIndex,
      stepLabel: step?.step.label || step?.step.kind || `step-${stepIndex}`,
      stepKind: step?.step.kind || 'unknown',
      windowMs: [startedT, endedT] as [number, number],
      events: events.filter(e => e.t >= startedT && e.t <= endedT),
    };
  }).filter(b => b.events.length > 0);

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
      webVitalsFindings: events.filter(e => e.kind === 'web-vitals').length,
      webVitalsFindingsStrict: events.filter(e => e.kind === 'web-vitals' && e.severity === 'strict').length,
      tapTargetsFindings: events.filter(e => e.kind === 'tap-targets').length,
      tapTargetsFindingsStrict: events.filter(e => e.kind === 'tap-targets' && e.severity === 'strict').length,
      formLabelsFindings: events.filter(e => e.kind === 'form-labels').length,
      formLabelsFindingsStrict: events.filter(e => e.kind === 'form-labels' && e.severity === 'strict').length,
      viewportMetaFindings: events.filter(e => e.kind === 'viewport-meta').length,
      viewportMetaFindingsStrict: events.filter(e => e.kind === 'viewport-meta' && e.severity === 'strict').length,
      docTitleFindings: events.filter(e => e.kind === 'doc-title').length,
      docTitleFindingsStrict: events.filter(e => e.kind === 'doc-title' && e.severity === 'strict').length,
      htmlLangFindings: events.filter(e => e.kind === 'html-lang').length,
      htmlLangFindingsStrict: events.filter(e => e.kind === 'html-lang' && e.severity === 'strict').length,
      skipLinkFindings: events.filter(e => e.kind === 'skip-link').length,
      skipLinkFindingsStrict: events.filter(e => e.kind === 'skip-link' && e.severity === 'strict').length,
      outboundLinksFindings: events.filter(e => e.kind === 'outbound-links').length,
      outboundLinksFindingsStrict: events.filter(e => e.kind === 'outbound-links' && e.severity === 'strict').length,
      autocompleteFindings: events.filter(e => e.kind === 'autocomplete').length,
      autocompleteFindingsStrict: events.filter(e => e.kind === 'autocomplete' && e.severity === 'strict').length,
      metaDescriptionFindings: events.filter(e => e.kind === 'meta-description').length,
      metaDescriptionFindingsStrict: events.filter(e => e.kind === 'meta-description' && e.severity === 'strict').length,
      faviconFindings: events.filter(e => e.kind === 'favicon').length,
      faviconFindingsStrict: events.filter(e => e.kind === 'favicon' && e.severity === 'strict').length,
      mixedContentFindings: events.filter(e => e.kind === 'mixed-content').length,
      mixedContentFindingsStrict: events.filter(e => e.kind === 'mixed-content' && e.severity === 'strict').length,
      hstsFindings: events.filter(e => e.kind === 'hsts').length,
      hstsFindingsStrict: events.filter(e => e.kind === 'hsts' && e.severity === 'strict').length,
      xFrameOptionsFindings: events.filter(e => e.kind === 'x-frame-options').length,
      xFrameOptionsFindingsStrict: events.filter(e => e.kind === 'x-frame-options' && e.severity === 'strict').length,
      linkUnderlineFindings: events.filter(e => e.kind === 'link-underline').length,
      linkUnderlineFindingsStrict: events.filter(e => e.kind === 'link-underline' && e.severity === 'strict').length,
      crossPageTitleFindings: events.filter(e => e.kind === 'cross-page-title').length,
      crossPageTitleFindingsStrict: events.filter(e => e.kind === 'cross-page-title' && e.severity === 'strict').length,
      crossPageMetaDescriptionFindings: events.filter(e => e.kind === 'cross-page-meta-description').length,
      crossPageMetaDescriptionFindingsStrict: events.filter(e => e.kind === 'cross-page-meta-description' && e.severity === 'strict').length,
      cspViolations: events.filter(e => e.kind === 'csp-violation').length,
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
  if (webVitalsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'web-vitals.json'),
      JSON.stringify(webVitalsByStep, null, 2),
    );
  }
  if (tapTargetsFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'tap-targets.json'),
      JSON.stringify(tapTargetsFindingsByStep, null, 2),
    );
  }
  if (formLabelsFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'form-labels.json'),
      JSON.stringify(formLabelsFindingsByStep, null, 2),
    );
  }
  if (viewportMetaFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'viewport-meta.json'),
      JSON.stringify(viewportMetaFindingsByStep, null, 2),
    );
  }
  if (docTitleFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'doc-title.json'),
      JSON.stringify(docTitleFindingsByStep, null, 2),
    );
  }
  if (htmlLangFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'html-lang.json'),
      JSON.stringify(htmlLangFindingsByStep, null, 2),
    );
  }
  if (skipLinkFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'skip-link.json'),
      JSON.stringify(skipLinkFindingsByStep, null, 2),
    );
  }
  if (outboundLinksFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'outbound-links.json'),
      JSON.stringify(outboundLinksFindingsByStep, null, 2),
    );
  }
  if (autocompleteFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'autocomplete.json'),
      JSON.stringify(autocompleteFindingsByStep, null, 2),
    );
  }
  if (metaDescriptionFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'meta-description.json'),
      JSON.stringify(metaDescriptionFindingsByStep, null, 2),
    );
  }
  if (faviconFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'favicon.json'),
      JSON.stringify(faviconFindingsByStep, null, 2),
    );
  }
  if (mixedContentFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'mixed-content.json'),
      JSON.stringify(mixedContentFindingsByStep, null, 2),
    );
  }
  if (hstsFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'hsts.json'),
      JSON.stringify(hstsFindingsByStep, null, 2),
    );
  }
  if (xFrameOptionsFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'x-frame-options.json'),
      JSON.stringify(xFrameOptionsFindingsByStep, null, 2),
    );
  }
  if (linkUnderlineFindingsByStep.length > 0) {
    writeFileSync(
      join(outDir, 'link-underline.json'),
      JSON.stringify(linkUnderlineFindingsByStep, null, 2),
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

  const prior = findPriorRun(runsDir, outDir, journey.name);
  const diff = diffReports(report, prior);
  writeFileSync(join(outDir, 'diff.json'), JSON.stringify(diff, null, 2));

  // T16: aria-tree drift between this run and prior run. Catches
  // panels disappearing or major structural changes that no other
  // detector flags. Findings are emitted into the events stream so
  // the diff axes carry them on next run.
  if (prior) {
    // T16 fix: filter prior-run candidates by EXACT journey name.
    // Run dirs are named "<journey>-<ISO timestamp>" where the timestamp
    // starts with a digit (year). startsWith(journey.name + '-') alone
    // would also match `skillshots-poc-mobile-...` for journey
    // `skillshots-poc`, since 'mobile' starts after the dash. Pin
    // with a regex that requires a digit immediately after the prefix.
    const journeyPattern = new RegExp('^' + journey.name.replace(/[.*+?^${}()|[\\]\\\\]/g, '\\\\$&') + '-\\d');
    const priorRunDir = (() => {
      const currentBase = outDir.split('/').pop() || '';
      const entries = readdirSync(runsDir)
        .filter((n: string) =>
          !n.startsWith('.')
          && n !== currentBase
          && journeyPattern.test(n),
        )
        .sort();
      return entries.length > 0 ? join(runsDir, entries[entries.length - 1]) : null;
    })();
    if (priorRunDir) {
      const drifts = compareAriaTrees(outDir, priorRunDir);
      if (drifts.length > 0) {
        writeFileSync(join(outDir, 'aria-drift.json'), JSON.stringify(drifts, null, 2));
        for (const d of drifts) {
          report.events.push({
            t: Date.now() - startEpoch,
            kind: 'aria-drift',
            severity: d.severity,
            text: `[aria.drift] ${d.step}: ${d.priorLines} → ${d.currentLines} lines (${Math.round(d.deltaPct * 100)}% drift)`,
            ruleId: 'aria.drift',
            impact: d.severity === 'strict' ? 'serious' : 'minor',
          });
        }
        // Re-write report.json with the new events.
        writeFileSync(join(outDir, 'report.json'), JSON.stringify(report, null, 2));
      }
    }
    // T83: aria-drift events are pushed onto report.events AFTER
    // diffReports() ran above (line 963), so diff.newAriaDriftFindings
    // was empty at construction. Back-fill it now that the events
    // exist. The aria-drift detector is itself a delta detector
    // (compares current step's aria.txt against the prior run), so
    // every aria-drift event is by definition new — no further diff
    // needed against priorKeys.
    diff.newAriaDriftFindings = report.events.filter((e) => e.kind === 'aria-drift');
    writeFileSync(join(outDir, 'diff.json'), JSON.stringify(diff, null, 2));
  }

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
  console.log(`  tap targets:       ${report.counts.tapTargetsFindings} (strict ${report.counts.tapTargetsFindingsStrict})`);
  console.log(`  form labels:       ${report.counts.formLabelsFindings} (strict ${report.counts.formLabelsFindingsStrict})`);
  console.log(`  viewport meta:     ${report.counts.viewportMetaFindings} (strict ${report.counts.viewportMetaFindingsStrict})`);
  console.log(`  doc title:         ${report.counts.docTitleFindings} (strict ${report.counts.docTitleFindingsStrict})`);
  console.log(`  html lang:         ${report.counts.htmlLangFindings} (strict ${report.counts.htmlLangFindingsStrict})`);
  console.log(`  skip link:         ${report.counts.skipLinkFindings} (strict ${report.counts.skipLinkFindingsStrict})`);
  console.log(`  outbound links:    ${report.counts.outboundLinksFindings} (strict ${report.counts.outboundLinksFindingsStrict})`);
  console.log(`  autocomplete:      ${report.counts.autocompleteFindings} (strict ${report.counts.autocompleteFindingsStrict})`);
  console.log(`  meta description:  ${report.counts.metaDescriptionFindings} (strict ${report.counts.metaDescriptionFindingsStrict})`);
  console.log(`  favicon:           ${report.counts.faviconFindings} (strict ${report.counts.faviconFindingsStrict})`);
  console.log(`  mixed content:     ${report.counts.mixedContentFindings} (strict ${report.counts.mixedContentFindingsStrict})`);
  console.log(`  hsts:              ${report.counts.hstsFindings} (strict ${report.counts.hstsFindingsStrict})`);
  console.log(`  x-frame-options:   ${report.counts.xFrameOptionsFindings} (strict ${report.counts.xFrameOptionsFindingsStrict})`);
  console.log(`  link underline:    ${report.counts.linkUnderlineFindings} (strict ${report.counts.linkUnderlineFindingsStrict})`);
  console.log(`  cross-page title:  ${report.counts.crossPageTitleFindings} (strict ${report.counts.crossPageTitleFindingsStrict})`);
  console.log(`  cross-page meta:   ${report.counts.crossPageMetaDescriptionFindings} (strict ${report.counts.crossPageMetaDescriptionFindingsStrict})`);
  console.log(`  web vitals:        ${report.counts.webVitalsFindings} (strict ${report.counts.webVitalsFindingsStrict})`);
  console.log(`  csp violations:    ${report.counts.cspViolations}`);
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
    const newWebVitalsStrict = diff.newWebVitalsFindings.filter(e => e.severity === 'strict').length;
    const newWebVitalsWarn = diff.newWebVitalsFindings.length - newWebVitalsStrict;
    console.log(`    NEW web vitals:       ${diff.newWebVitalsFindings.length} (strict ${newWebVitalsStrict}, warn ${newWebVitalsWarn})`);
    const newTapTargetsStrict = diff.newTapTargetsFindings.filter(e => e.severity === 'strict').length;
    const newTapTargetsWarn = diff.newTapTargetsFindings.length - newTapTargetsStrict;
    console.log(`    NEW tap targets:      ${diff.newTapTargetsFindings.length} (strict ${newTapTargetsStrict}, warn ${newTapTargetsWarn})`);
    const newFormLabelsStrict = diff.newFormLabelsFindings.filter(e => e.severity === 'strict').length;
    const newFormLabelsWarn = diff.newFormLabelsFindings.length - newFormLabelsStrict;
    console.log(`    NEW form labels:      ${diff.newFormLabelsFindings.length} (strict ${newFormLabelsStrict}, warn ${newFormLabelsWarn})`);
    const newViewportMetaStrict = diff.newViewportMetaFindings.filter(e => e.severity === 'strict').length;
    const newViewportMetaWarn = diff.newViewportMetaFindings.length - newViewportMetaStrict;
    console.log(`    NEW viewport meta:    ${diff.newViewportMetaFindings.length} (strict ${newViewportMetaStrict}, warn ${newViewportMetaWarn})`);
    const newDocTitleStrict = diff.newDocTitleFindings.filter(e => e.severity === 'strict').length;
    const newDocTitleWarn = diff.newDocTitleFindings.length - newDocTitleStrict;
    console.log(`    NEW doc title:        ${diff.newDocTitleFindings.length} (strict ${newDocTitleStrict}, warn ${newDocTitleWarn})`);
    const newHtmlLangStrict = diff.newHtmlLangFindings.filter(e => e.severity === 'strict').length;
    const newHtmlLangWarn = diff.newHtmlLangFindings.length - newHtmlLangStrict;
    console.log(`    NEW html lang:        ${diff.newHtmlLangFindings.length} (strict ${newHtmlLangStrict}, warn ${newHtmlLangWarn})`);
    const newSkipLinkStrict = diff.newSkipLinkFindings.filter(e => e.severity === 'strict').length;
    const newSkipLinkWarn = diff.newSkipLinkFindings.length - newSkipLinkStrict;
    console.log(`    NEW skip link:        ${diff.newSkipLinkFindings.length} (strict ${newSkipLinkStrict}, warn ${newSkipLinkWarn})`);
    const newOutboundLinksStrict = diff.newOutboundLinksFindings.filter(e => e.severity === 'strict').length;
    const newOutboundLinksWarn = diff.newOutboundLinksFindings.length - newOutboundLinksStrict;
    console.log(`    NEW outbound links:   ${diff.newOutboundLinksFindings.length} (strict ${newOutboundLinksStrict}, warn ${newOutboundLinksWarn})`);
    const newAutocompleteStrict = diff.newAutocompleteFindings.filter(e => e.severity === 'strict').length;
    const newAutocompleteWarn = diff.newAutocompleteFindings.length - newAutocompleteStrict;
    console.log(`    NEW autocomplete:     ${diff.newAutocompleteFindings.length} (strict ${newAutocompleteStrict}, warn ${newAutocompleteWarn})`);
    const newMetaDescStrict = diff.newMetaDescriptionFindings.filter(e => e.severity === 'strict').length;
    const newMetaDescWarn = diff.newMetaDescriptionFindings.length - newMetaDescStrict;
    console.log(`    NEW meta description: ${diff.newMetaDescriptionFindings.length} (strict ${newMetaDescStrict}, warn ${newMetaDescWarn})`);
    const newFaviconStrict = diff.newFaviconFindings.filter(e => e.severity === 'strict').length;
    const newFaviconWarn = diff.newFaviconFindings.length - newFaviconStrict;
    console.log(`    NEW favicon:          ${diff.newFaviconFindings.length} (strict ${newFaviconStrict}, warn ${newFaviconWarn})`);
    const newMixedStrict = diff.newMixedContentFindings.filter(e => e.severity === 'strict').length;
    const newMixedWarn = diff.newMixedContentFindings.length - newMixedStrict;
    console.log(`    NEW mixed content:    ${diff.newMixedContentFindings.length} (strict ${newMixedStrict}, warn ${newMixedWarn})`);
    const newHstsStrict = diff.newHstsFindings.filter(e => e.severity === 'strict').length;
    const newHstsWarn = diff.newHstsFindings.length - newHstsStrict;
    console.log(`    NEW hsts:             ${diff.newHstsFindings.length} (strict ${newHstsStrict}, warn ${newHstsWarn})`);
    const newXfoStrict = diff.newXFrameOptionsFindings.filter(e => e.severity === 'strict').length;
    const newXfoWarn = diff.newXFrameOptionsFindings.length - newXfoStrict;
    console.log(`    NEW x-frame-options:  ${diff.newXFrameOptionsFindings.length} (strict ${newXfoStrict}, warn ${newXfoWarn})`);
    const newLinkUnderlineStrict = diff.newLinkUnderlineFindings.filter(e => e.severity === 'strict').length;
    const newLinkUnderlineWarn = diff.newLinkUnderlineFindings.length - newLinkUnderlineStrict;
    console.log(`    NEW link underline:   ${diff.newLinkUnderlineFindings.length} (strict ${newLinkUnderlineStrict}, warn ${newLinkUnderlineWarn})`);
    const newCrossPageTitleStrict = diff.newCrossPageTitleFindings.filter(e => e.severity === 'strict').length;
    const newCrossPageTitleWarn = diff.newCrossPageTitleFindings.length - newCrossPageTitleStrict;
    console.log(`    NEW cross-page title: ${diff.newCrossPageTitleFindings.length} (strict ${newCrossPageTitleStrict}, warn ${newCrossPageTitleWarn})`);
    const newCpMdStrict = diff.newCrossPageMetaDescriptionFindings.filter(e => e.severity === 'strict').length;
    const newCpMdWarn = diff.newCrossPageMetaDescriptionFindings.length - newCpMdStrict;
    console.log(`    NEW cross-page meta:  ${diff.newCrossPageMetaDescriptionFindings.length} (strict ${newCpMdStrict}, warn ${newCpMdWarn})`);
    console.log(`    NEW csp violations:   ${diff.newCspViolations.length}`);
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
  const newWebVitalsStrictCount = diff.newWebVitalsFindings.filter(e => e.severity === 'strict').length;
  // T83: aria-drift was missing from the gate. Strict drift (>30%
  // line delta) is the same severity-class as a runtime contrast
  // violation — content gone or panels collapsed silently.
  const newAriaDriftStrictCount = diff.newAriaDriftFindings.filter(e => e.severity === 'strict').length;
  // T16: previously-unmetered axes — strict regressions used to print
  // "REGRESSION" in the positive-signal table while the top-line gate
  // emitted PASS. Hooking each one through the gate so a strict
  // regression in any registered axis fails ship.
  const newHeadingOrderStrictCount = diff.newHeadingOrderFindings.filter(e => e.severity === 'strict').length;
  const newRuntimeLandmarksStrictCount = diff.newRuntimeLandmarksFindings.filter(e => e.severity === 'strict').length;
  const newLinkTextStrictCount = diff.newLinkTextFindings.filter(e => e.severity === 'strict').length;
  const newPlaceholderTextStrictCount = diff.newPlaceholderTextFindings.filter(e => e.severity === 'strict').length;
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
    || newWebVitalsStrictCount > DEFAULT_BUDGET.newWebVitalsStrict
    || diff.newCspViolations.length > DEFAULT_BUDGET.newCspViolations
    || newAriaDriftStrictCount > DEFAULT_BUDGET.newAriaDriftStrict
    || newHeadingOrderStrictCount > DEFAULT_BUDGET.newHeadingOrderStrict
    || newRuntimeLandmarksStrictCount > DEFAULT_BUDGET.newRuntimeLandmarksStrict
    || newLinkTextStrictCount > DEFAULT_BUDGET.newLinkTextStrict
    || newPlaceholderTextStrictCount > DEFAULT_BUDGET.newPlaceholderTextStrict
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
