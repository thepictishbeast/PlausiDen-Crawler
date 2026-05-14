/**
 * Report — JSON bundle of a run's captured events, step results, and
 * a diff against the prior baseline. Consumed by the runner + CI
 * gate.
 */
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import type { StepResult } from './journey.js';

export interface CapturedEvent {
  t: number;
  kind:
    | 'console'
    | 'pageerror'
    | 'request-failed'
    | 'response-error'
    | 'csp-violation'
    | 'a11y-violation'
    | 'css-health'
    | 'ui-overflow'
    | 'runtime-contrast'
    | 'runtime-images'
    | 'runtime-focus'
    | 'web-vitals'
    | 'aria-drift'
    | 'heading-order'
    | 'runtime-landmarks'
    | 'link-text'
    | 'placeholder-text'
    | 'tap-targets'
    | 'form-labels'
    | 'viewport-meta'
    | 'doc-title'
    | 'html-lang'
    | 'skip-link'
    | 'outbound-links'
    | 'autocomplete'
    | 'meta-description'
    | 'favicon'
    | 'mixed-content';
  level?: string;
  text: string;
  url?: string;
  status?: number;
  stack?: string;
  impact?: 'minor' | 'moderate' | 'serious' | 'critical';
  ruleId?: string;
  /** css-health-only: severity bucket from cssHealth detector. */
  severity?: 'strict' | 'warn';
}

export interface Report {
  target: string;
  journey: string;
  viewport: { w: number; h: number };
  started: string;
  durationMs: number;
  counts: {
    consoleErrors: number;
    pageErrors: number;
    failedRequests: number;
    a11yViolations: number;
    cssHealthFindings: number;
    cssHealthFindingsStrict: number;
    uiOverflowFindings: number;
    uiOverflowFindingsStrict: number;
    runtimeContrastFindings: number;
    runtimeContrastFindingsStrict: number;
    runtimeImagesFindings: number;
    runtimeImagesFindingsStrict: number;
    runtimeFocusFindings: number;
    runtimeFocusFindingsStrict: number;
    webVitalsFindings: number;
    webVitalsFindingsStrict: number;
    tapTargetsFindings: number;
    tapTargetsFindingsStrict: number;
    formLabelsFindings: number;
    formLabelsFindingsStrict: number;
    viewportMetaFindings: number;
    viewportMetaFindingsStrict: number;
    docTitleFindings: number;
    docTitleFindingsStrict: number;
    htmlLangFindings: number;
    htmlLangFindingsStrict: number;
    skipLinkFindings: number;
    skipLinkFindingsStrict: number;
    outboundLinksFindings: number;
    outboundLinksFindingsStrict: number;
    autocompleteFindings: number;
    autocompleteFindingsStrict: number;
    metaDescriptionFindings: number;
    metaDescriptionFindingsStrict: number;
    faviconFindings: number;
    faviconFindingsStrict: number;
    mixedContentFindings: number;
    mixedContentFindingsStrict: number;
    cspViolations: number;
    total: number;
    stepsOk: number;
    stepsFailed: number;
  };
  events: CapturedEvent[];
  steps: StepResult[];
  /** Events grouped by what the crawler was doing when they fired.
   *  "during step N (label X): event kind/text" — answers the user's
   *  question "what was the crawler doing when the console log hit?" */
  eventsByStep?: Array<{
    stepIndex: number;
    stepLabel: string;
    stepKind: string;
    windowMs: [number, number];
    events: CapturedEvent[];
  }>;
}

export interface Diff {
  newConsoleErrors: CapturedEvent[];
  newPageErrors: CapturedEvent[];
  newFailedRequests: CapturedEvent[];
  newA11yViolations: CapturedEvent[];
  /**
   * cssHealth findings new in this run vs the prior baseline. A new
   * strict cssHealth finding is the same severity-class as a new
   * console error: ship-blocking. T71 (2026-05-04).
   */
  newCssHealthFindings: CapturedEvent[];
  /**
   * uiOverflow findings new in this run vs the prior baseline.
   * Strict = page-h-scroll, element-bleed, text-clipped, or
   * tap-target on mobile. T28 (2026-05-04).
   */
  newUiOverflowFindings: CapturedEvent[];
  /**
   * runtime-contrast findings new in this run vs prior. Strict =
   * body-text below WCAG AA 4.5:1, warn = large-text below 3:1.
   * T29 (2026-05-04).
   */
  newRuntimeContrastFindings: CapturedEvent[];
  /**
   * runtime-images findings new in this run vs prior. Strict =
   * broken/empty-src/missing-alt; warn = CLS-risk. T75 (2026-05-04).
   */
  newRuntimeImagesFindings: CapturedEvent[];
  /**
   * runtime-focus findings new in this run vs prior. Strict =
   * interactive element with no visible focus indicator (WCAG
   * 2.4.7). T79 (2026-05-04).
   */
  newRuntimeFocusFindings: CapturedEvent[];
  /**
   * web-vitals findings new in this run vs prior. Strict =
   * Core Web Vitals 'poor' band (LCP > 4s, CLS > 0.25, INP > 500ms).
   * Warn = 'needs-improvement'. T45 (2026-05-04).
   */
  newWebVitalsFindings: CapturedEvent[];
  /**
   * csp-violation events new in this run vs prior. Strict — any
   * new violation is ship-blocking (security regression). T80
   * (2026-05-04).
   */
  newCspViolations: CapturedEvent[];
  /**
   * aria-drift findings new in this run vs prior. Populated by
   * main.ts AFTER diffReports() runs (aria-drift events are
   * pushed onto report.events later in the pipeline, so we
   * back-fill this field once they exist). Strict = >30% line
   * delta in a step's aria-tree; warn = 10-30%. T83 (2026-05-04).
   *
   * BUG ASSUMPTION: any consumer reading this field BEFORE
   * main.ts back-fills it sees an empty array. The only safe
   * read site is renderPositiveSignal *after* the back-fill.
   */
  newAriaDriftFindings: CapturedEvent[];
  /**
   * heading-order findings new in this run vs prior. Strict =
   * !=1 h1; warn = level skip (h2 -> h4 without h3). T104 (TS port).
   */
  newHeadingOrderFindings: CapturedEvent[];
  /**
   * runtime-landmarks findings new in this run vs prior. Strict =
   * landmark uniqueness violation (no main / multiple main / etc.)
   * or same-role nesting (<main><main>). T105 (TS port).
   */
  newRuntimeLandmarksFindings: CapturedEvent[];
  /**
   * link-text findings new in this run vs prior. Strict = visible
   * link with no accessible name; warn = generic phrases (click
   * here / read more / etc.). WCAG 2.4.4. T106 (TS port).
   */
  newLinkTextFindings: CapturedEvent[];
  /**
   * placeholder-text findings new in this run vs prior. Strict =
   * Lorem ipsum / TODO|FIXME|XXX|HACK / template instructions in
   * rendered DOM. Warn = "coming soon" / "TBD". T16 (2026-05-04).
   */
  newPlaceholderTextFindings: CapturedEvent[];
  /**
   * tap-target findings new in this run vs prior. Strict = target
   * smaller than 24×24 CSS px (WCAG 2.5.8 AA). Warn = below 44×44
   * (WCAG 2.5.5 AAA + Apple/Material recommendation). T76.
   */
  newTapTargetsFindings: CapturedEvent[];
  /**
   * form-label findings new in this run vs prior. Strict = no
   * accessible name. Warn = placeholder-only or required without
   * visible indicator. WCAG 1.3.1 / 4.1.2 / 3.3.2. T76.
   */
  newFormLabelsFindings: CapturedEvent[];
  /**
   * viewport-meta findings new in this run vs prior. Strict =
   * missing tag / no width=device-width / zoom-disabled. T76.
   */
  newViewportMetaFindings: CapturedEvent[];
  /**
   * doc-title findings new in this run vs prior. Strict =
   * missing/empty. Warn = generic / too-short / too-long. T76.
   */
  newDocTitleFindings: CapturedEvent[];
  /**
   * html-lang findings new in this run vs prior. Strict =
   * missing/empty. Warn = invalid BCP-47 / unknown primary. T76.
   */
  newHtmlLangFindings: CapturedEvent[];
  /**
   * skip-link findings new in this run vs prior. Warn = missing /
   * not-first-focusable. Strict = broken target / permanently
   * hidden. WCAG 2.4.1. T76.
   */
  newSkipLinkFindings: CapturedEvent[];
  /**
   * outbound-link findings new in this run vs prior. Strict =
   * tabnab-vulnerable / opener-explicit. Warn = no-noreferrer.
   * Security defence-in-depth. T76.
   */
  newOutboundLinksFindings: CapturedEvent[];
  /**
   * autocomplete findings new in this run vs prior. Strict =
   * missing on credential field. Warn = missing on PII or invalid
   * token. WCAG 1.3.5 AA. T76.
   */
  newAutocompleteFindings: CapturedEvent[];
  /**
   * meta-description findings new in this run vs prior. All warn.
   * Missing/empty/too-short/too-long. T76.
   */
  newMetaDescriptionFindings: CapturedEvent[];
  /**
   * favicon findings new in this run vs prior. warn-only —
   * missing icon link in head. T76.
   */
  newFaviconFindings: CapturedEvent[];
  /**
   * mixed-content findings new in this run vs prior. Strict =
   * active (script/css/iframe) or form-action over http on https
   * page. Warn = passive (img/audio/video). T76.
   */
  newMixedContentFindings: CapturedEvent[];
  newlyBrokenSteps: StepResult[];
  fixedSteps: StepResult[];
}

const key = (e: CapturedEvent) =>
  `${e.kind}|${(e.text || '').slice(0, 200)}|${e.url || ''}|${e.status || ''}|${e.ruleId || ''}`;

export function diffReports(current: Report, prior: Report | null): Diff {
  const out: Diff = {
    newConsoleErrors: [],
    newPageErrors: [],
    newFailedRequests: [],
    newA11yViolations: [],
    newCssHealthFindings: [],
    newUiOverflowFindings: [],
    newRuntimeContrastFindings: [],
    newRuntimeImagesFindings: [],
    newRuntimeFocusFindings: [],
    newWebVitalsFindings: [],
    newCspViolations: [],
    // T83: back-filled by main.ts after aria-drift events get
    // pushed onto report.events; default to empty here so consumers
    // see a stable shape regardless of pipeline ordering.
    newAriaDriftFindings: [],
    newHeadingOrderFindings: [],
    newRuntimeLandmarksFindings: [],
    newLinkTextFindings: [],
    newPlaceholderTextFindings: [],
    newTapTargetsFindings: [],
    newFormLabelsFindings: [],
    newViewportMetaFindings: [],
    newDocTitleFindings: [],
    newHtmlLangFindings: [],
    newSkipLinkFindings: [],
    newOutboundLinksFindings: [],
    newAutocompleteFindings: [],
    newMetaDescriptionFindings: [],
    newFaviconFindings: [],
    newMixedContentFindings: [],
    newlyBrokenSteps: [],
    fixedSteps: [],
  };
  if (!prior) {
    out.newConsoleErrors = current.events.filter(e => e.kind === 'console' && e.level === 'error');
    out.newPageErrors = current.events.filter(e => e.kind === 'pageerror');
    out.newFailedRequests = current.events.filter(e => e.kind === 'request-failed' || e.kind === 'response-error');
    out.newA11yViolations = current.events.filter(e => e.kind === 'a11y-violation');
    out.newCssHealthFindings = current.events.filter(e => e.kind === 'css-health');
    out.newUiOverflowFindings = current.events.filter(e => e.kind === 'ui-overflow');
    out.newRuntimeContrastFindings = current.events.filter(e => e.kind === 'runtime-contrast');
    out.newRuntimeImagesFindings = current.events.filter(e => e.kind === 'runtime-images');
    out.newRuntimeFocusFindings = current.events.filter(e => e.kind === 'runtime-focus');
    out.newWebVitalsFindings = current.events.filter(e => e.kind === 'web-vitals');
    out.newCspViolations = current.events.filter(e => e.kind === 'csp-violation');
    out.newHeadingOrderFindings = current.events.filter(e => e.kind === 'heading-order');
    out.newRuntimeLandmarksFindings = current.events.filter(e => e.kind === 'runtime-landmarks');
    out.newLinkTextFindings = current.events.filter(e => e.kind === 'link-text');
    out.newPlaceholderTextFindings = current.events.filter(e => e.kind === 'placeholder-text');
    out.newTapTargetsFindings = current.events.filter(e => e.kind === 'tap-targets');
    out.newFormLabelsFindings = current.events.filter(e => e.kind === 'form-labels');
    out.newViewportMetaFindings = current.events.filter(e => e.kind === 'viewport-meta');
    out.newDocTitleFindings = current.events.filter(e => e.kind === 'doc-title');
    out.newHtmlLangFindings = current.events.filter(e => e.kind === 'html-lang');
    out.newSkipLinkFindings = current.events.filter(e => e.kind === 'skip-link');
    out.newOutboundLinksFindings = current.events.filter(e => e.kind === 'outbound-links');
    out.newAutocompleteFindings = current.events.filter(e => e.kind === 'autocomplete');
    out.newMetaDescriptionFindings = current.events.filter(e => e.kind === 'meta-description');
    out.newFaviconFindings = current.events.filter(e => e.kind === 'favicon');
    out.newMixedContentFindings = current.events.filter(e => e.kind === 'mixed-content');
    return out;
  }
  const priorKeys = new Set(prior.events.map(key));
  for (const e of current.events) {
    if (priorKeys.has(key(e))) continue;
    if (e.kind === 'console' && e.level === 'error') out.newConsoleErrors.push(e);
    else if (e.kind === 'pageerror') out.newPageErrors.push(e);
    else if (e.kind === 'request-failed' || e.kind === 'response-error') out.newFailedRequests.push(e);
    else if (e.kind === 'a11y-violation') out.newA11yViolations.push(e);
    else if (e.kind === 'css-health') out.newCssHealthFindings.push(e);
    else if (e.kind === 'ui-overflow') out.newUiOverflowFindings.push(e);
    else if (e.kind === 'runtime-contrast') out.newRuntimeContrastFindings.push(e);
    else if (e.kind === 'runtime-images') out.newRuntimeImagesFindings.push(e);
    else if (e.kind === 'runtime-focus') out.newRuntimeFocusFindings.push(e);
    else if (e.kind === 'web-vitals') out.newWebVitalsFindings.push(e);
    else if (e.kind === 'csp-violation') out.newCspViolations.push(e);
    else if (e.kind === 'heading-order') out.newHeadingOrderFindings.push(e);
    else if (e.kind === 'runtime-landmarks') out.newRuntimeLandmarksFindings.push(e);
    else if (e.kind === 'link-text') out.newLinkTextFindings.push(e);
    else if (e.kind === 'placeholder-text') out.newPlaceholderTextFindings.push(e);
    else if (e.kind === 'tap-targets') out.newTapTargetsFindings.push(e);
    else if (e.kind === 'form-labels') out.newFormLabelsFindings.push(e);
    else if (e.kind === 'viewport-meta') out.newViewportMetaFindings.push(e);
    else if (e.kind === 'doc-title') out.newDocTitleFindings.push(e);
    else if (e.kind === 'html-lang') out.newHtmlLangFindings.push(e);
    else if (e.kind === 'skip-link') out.newSkipLinkFindings.push(e);
    else if (e.kind === 'outbound-links') out.newOutboundLinksFindings.push(e);
    else if (e.kind === 'autocomplete') out.newAutocompleteFindings.push(e);
    else if (e.kind === 'meta-description') out.newMetaDescriptionFindings.push(e);
    else if (e.kind === 'favicon') out.newFaviconFindings.push(e);
    else if (e.kind === 'mixed-content') out.newMixedContentFindings.push(e);
  }
  const priorStepLabels = new Map(
    prior.steps.map((s, i) => [s.step.label || `${s.step.kind}-${i}`, s])
  );
  for (const s of current.steps) {
    const id = s.step.label || `${s.step.kind}-${s.index}`;
    const was = priorStepLabels.get(id);
    if (s.ok && was && !was.ok) out.fixedSteps.push(s);
    if (!s.ok && was && was.ok) out.newlyBrokenSteps.push(s);
  }
  return out;
}

/**
 * T2: render a "positive signal" summary that makes the silent-pass
 * state legible. The diff alone says "0 NEW errors" — that could
 * mean "all checks passed" OR "no checks ran". This makes the
 * difference visible: per axis, "checked N steps, K total findings,
 * J new vs prior".
 *
 * Operator sees:
 *
 *   axis            steps  total  new  status
 *   axe-static      27     40     0    pass (40 baseline frozen)
 *   cssHealth       27     0      0    pass (silent)
 *   uiOverflow      27     0      0    pass (silent)
 *   runtimeContrast 27     0      0    pass (silent)
 *   runtimeImages   27     0      0    pass (silent)
 *
 * 5 detection axes, all silent — that's a positive confirmation,
 * not a maybe.
 */
export function renderPositiveSignal(report: Report, diff: Diff): string {
  const stepCount = report.steps.length;
  // T83: per-axis status now distinguishes strict-news (gate-blocking
  // REGRESSION) from warn-news (within-budget warning). Without this
  // split, a row with 6 NEW warn findings was labelled "REGRESSION"
  // even though the gate emits PASS — operator learns to ignore the
  // red label.
  //
  // Severity-bucketed axes set strictNews from the diff array; the
  // remaining axes (where every finding is implicitly strict —
  // console errors, page errors, failed requests, a11y violations,
  // CSP violations) treat all news as strict.
  const strict = (events: CapturedEvent[]) =>
    events.filter((e) => e.severity === 'strict').length;
  const axes: { name: string; total: number; news: number; strictNews: number }[] = [
    { name: 'console-errors',  total: report.counts.consoleErrors,           news: diff.newConsoleErrors.length,           strictNews: diff.newConsoleErrors.length },
    { name: 'page-errors',     total: report.counts.pageErrors,              news: diff.newPageErrors.length,              strictNews: diff.newPageErrors.length },
    { name: 'failed-requests', total: report.counts.failedRequests,          news: diff.newFailedRequests.length,          strictNews: diff.newFailedRequests.length },
    { name: 'axe-static-a11y', total: report.counts.a11yViolations,          news: diff.newA11yViolations.length,          strictNews: diff.newA11yViolations.length },
    { name: 'cssHealth',       total: report.counts.cssHealthFindings,       news: diff.newCssHealthFindings.length,       strictNews: strict(diff.newCssHealthFindings) },
    { name: 'uiOverflow',      total: report.counts.uiOverflowFindings,      news: diff.newUiOverflowFindings.length,      strictNews: strict(diff.newUiOverflowFindings) },
    { name: 'runtimeContrast', total: report.counts.runtimeContrastFindings, news: diff.newRuntimeContrastFindings.length, strictNews: strict(diff.newRuntimeContrastFindings) },
    { name: 'runtimeImages',   total: report.counts.runtimeImagesFindings,   news: diff.newRuntimeImagesFindings.length,   strictNews: strict(diff.newRuntimeImagesFindings) },
    { name: 'runtimeFocus',    total: report.counts.runtimeFocusFindings,    news: diff.newRuntimeFocusFindings.length,    strictNews: strict(diff.newRuntimeFocusFindings) },
    { name: 'webVitals',       total: report.counts.webVitalsFindings,       news: diff.newWebVitalsFindings.length,       strictNews: strict(diff.newWebVitalsFindings) },
    { name: 'cspViolations',   total: report.counts.cspViolations,           news: diff.newCspViolations.length,           strictNews: diff.newCspViolations.length },
    {
      name: 'ariaDrift',
      total: report.events.filter((e) => e.kind === 'aria-drift').length,
      // T83: read from diff.newAriaDriftFindings (back-filled by
      // main.ts) instead of report.events directly. Previously
      // news==total because diffReports() ran before aria-drift
      // events were pushed.
      news: diff.newAriaDriftFindings.length,
      strictNews: strict(diff.newAriaDriftFindings),
    },
    {
      // T104 (TS port): heading_order — h1 count + level skips.
      // Mirrors crates/crawler-detectors/src/heading_order.rs.
      name: 'headingOrder',
      total: report.events.filter((e) => e.kind === 'heading-order').length,
      news: diff.newHeadingOrderFindings.length,
      strictNews: strict(diff.newHeadingOrderFindings),
    },
    {
      // T105 (TS port): runtime_landmarks — main/banner/contentinfo
      // uniqueness + same-role nesting.
      // Mirrors crates/crawler-detectors/src/runtime_landmarks.rs.
      name: 'runtimeLandmarks',
      total: report.events.filter((e) => e.kind === 'runtime-landmarks').length,
      news: diff.newRuntimeLandmarksFindings.length,
      strictNews: strict(diff.newRuntimeLandmarksFindings),
    },
    {
      // T106 (TS port): link_text — empty + generic link text.
      // WCAG 2.4.4. Mirrors crates/crawler-detectors/src/link_text.rs.
      name: 'linkText',
      total: report.events.filter((e) => e.kind === 'link-text').length,
      news: diff.newLinkTextFindings.length,
      strictNews: strict(diff.newLinkTextFindings),
    },
    {
      // T16 (Crawler): placeholder-text — Lorem ipsum, dev markers,
      // template instructions, "coming soon" in rendered DOM.
      // Catches paste-and-forgot signals across any codebase.
      name: 'placeholderText',
      total: report.events.filter((e) => e.kind === 'placeholder-text').length,
      news: diff.newPlaceholderTextFindings.length,
      strictNews: strict(diff.newPlaceholderTextFindings),
    },
    {
      // T76 (Crawler): tap-targets — WCAG 2.5.8 AA (24×24 strict)
      // + 2.5.5 AAA recommendation (44×44 warn). Top mobile-UX
      // defect.
      name: 'tapTargets',
      total: report.events.filter((e) => e.kind === 'tap-targets').length,
      news: diff.newTapTargetsFindings.length,
      strictNews: strict(diff.newTapTargetsFindings),
    },
    {
      // T76 (Crawler): form-labels — WCAG 1.3.1 + 4.1.2 + 3.3.2.
      // no-label (strict), placeholder-only (warn),
      // required-no-indicator (warn).
      name: 'formLabels',
      total: report.events.filter((e) => e.kind === 'form-labels').length,
      news: diff.newFormLabelsFindings.length,
      strictNews: strict(diff.newFormLabelsFindings),
    },
    {
      // T76 (Crawler): viewport-meta — WCAG 1.4.10 + 1.4.4.
      // missing tag / no width=device-width / zoom-disabled.
      name: 'viewportMeta',
      total: report.events.filter((e) => e.kind === 'viewport-meta').length,
      news: diff.newViewportMetaFindings.length,
      strictNews: strict(diff.newViewportMetaFindings),
    },
    {
      // T76 (Crawler): doc-title — missing/empty (strict),
      // generic / too-short / too-long (warn).
      name: 'docTitle',
      total: report.events.filter((e) => e.kind === 'doc-title').length,
      news: diff.newDocTitleFindings.length,
      strictNews: strict(diff.newDocTitleFindings),
    },
    {
      // T76 (Crawler): html-lang — WCAG 3.1.1 (Level A).
      // missing/empty (strict), invalid BCP-47 / unknown primary (warn).
      name: 'htmlLang',
      total: report.events.filter((e) => e.kind === 'html-lang').length,
      news: diff.newHtmlLangFindings.length,
      strictNews: strict(diff.newHtmlLangFindings),
    },
    {
      // T76 (Crawler): skip-link — WCAG 2.4.1 (Level A).
      // missing/not-first-focusable (warn), broken-target/permanently-
      // hidden (strict).
      name: 'skipLink',
      total: report.events.filter((e) => e.kind === 'skip-link').length,
      news: diff.newSkipLinkFindings.length,
      strictNews: strict(diff.newSkipLinkFindings),
    },
    {
      // T76 (Crawler): outbound-links — security defence-in-depth.
      // tabnab-vulnerable / opener-explicit (strict),
      // outbound-no-noreferrer (warn).
      name: 'outboundLinks',
      total: report.events.filter((e) => e.kind === 'outbound-links').length,
      news: diff.newOutboundLinksFindings.length,
      strictNews: strict(diff.newOutboundLinksFindings),
    },
    {
      // T76 (Crawler): autocomplete — WCAG 1.3.5 AA. Strict on
      // missing for credential fields; warn on PII / invalid token.
      name: 'autocomplete',
      total: report.events.filter((e) => e.kind === 'autocomplete').length,
      news: diff.newAutocompleteFindings.length,
      strictNews: strict(diff.newAutocompleteFindings),
    },
    {
      // T76 (Crawler): meta-description — SEO + social-share preview.
      // missing/empty/too-short/too-long (all warn).
      name: 'metaDescription',
      total: report.events.filter((e) => e.kind === 'meta-description').length,
      news: diff.newMetaDescriptionFindings.length,
      strictNews: strict(diff.newMetaDescriptionFindings),
    },
    {
      // T76 (Crawler): favicon — missing icon link in head (warn).
      name: 'favicon',
      total: report.events.filter((e) => e.kind === 'favicon').length,
      news: diff.newFaviconFindings.length,
      strictNews: strict(diff.newFaviconFindings),
    },
    {
      // T76 (Crawler): mixed-content — security defence in depth.
      // active/form (strict), passive (warn).
      name: 'mixedContent',
      total: report.events.filter((e) => e.kind === 'mixed-content').length,
      news: diff.newMixedContentFindings.length,
      strictNews: strict(diff.newMixedContentFindings),
    },
  ];
  const lines: string[] = [];
  lines.push(`=== positive signal (${axes.length} detection axes) ===`);
  lines.push('');
  lines.push(`  axis              steps   total    new   status`);
  lines.push(`  ----------------  ------  -------  ----  --------`);
  for (const a of axes) {
    let status: string;
    if (a.strictNews > 0) {
      status = `REGRESSION (${a.strictNews} strict)`;
    } else if (a.news > 0) {
      // News exist but all are warn-severity (within budget). Still
      // worth surfacing — these are NEW issues that didn't exist in
      // the prior run — but they don't block ship.
      status = `warn (${a.news} new — within budget)`;
    } else if (a.total > 0) {
      status = `pass (${a.total} baseline frozen)`;
    } else {
      status = 'pass (silent)';
    }
    lines.push(
      `  ${a.name.padEnd(16)}  ${String(stepCount).padStart(6)}  ${String(a.total).padStart(7)}  ${String(a.news).padStart(4)}  ${status}`,
    );
  }
  // Footer reflects the gate semantics, not the row labels: the gate
  // blocks on strict-news only, so the footer flags strict-news axes
  // separately from warn-news axes.
  const strictAxes = axes.filter((a) => a.strictNews > 0);
  const warnAxes = axes.filter((a) => a.strictNews === 0 && a.news > 0);
  lines.push('');
  if (strictAxes.length === 0 && warnAxes.length === 0) {
    lines.push(`  ✓ all ${axes.length} axes silent vs prior run — positive PASS confirmation`);
  } else if (strictAxes.length > 0) {
    const dirty = strictAxes.map((a) => `${a.name} (+${a.strictNews})`).join(', ');
    lines.push(`  ✗ ${strictAxes.length} axis/axes regressed (strict): ${dirty}`);
    if (warnAxes.length > 0) {
      lines.push(`  · ${warnAxes.length} axis/axes also have new warn findings: ${warnAxes.map((a) => a.name).join(', ')}`);
    }
  } else {
    const w = warnAxes.map((a) => `${a.name} (+${a.news})`).join(', ');
    lines.push(`  ⚠ ${warnAxes.length} axis/axes have new warn findings (within budget): ${w}`);
  }
  return lines.join('\n');
}

/**
 * T16: aria-tree drift detector. Compares per-step .aria.txt files
 * between the current run and the prior run; emits findings when a
 * step's structural line-count diverges more than the bands below.
 *
 * Bands (delta = |current_lines - prior_lines| / max(prior_lines, 1)):
 *   delta < 0.10   silent (page content edits, normal flux)
 *   0.10-0.30      warn (significant structural drift)
 *   > 0.30         strict (major regression — content gone, panels collapsed)
 *
 * Catches what the per-event diff misses: a panel disappearing
 * silently (no console error, no missing-file error, just gone).
 *
 * Pure function — caller passes the two run-directory paths.
 */
export interface AriaDriftFinding {
  severity: 'strict' | 'warn';
  step: string;
  priorLines: number;
  currentLines: number;
  deltaPct: number;
}

export function compareAriaTrees(
  currentDir: string,
  priorDir: string,
): AriaDriftFinding[] {
  const out: AriaDriftFinding[] = [];
  if (!existsSync(currentDir) || !existsSync(priorDir)) return out;
  const currentFiles = readdirSync(currentDir)
    .filter((f: string) => f.endsWith('.aria.txt'));
  for (const fname of currentFiles) {
    const priorPath = join(priorDir, fname);
    if (!existsSync(priorPath)) continue; // step is new
    let curLines = 0;
    let priorLines = 0;
    try {
      curLines = readFileSync(join(currentDir, fname), 'utf-8').split('\n').length;
      priorLines = readFileSync(priorPath, 'utf-8').split('\n').length;
    } catch {
      continue;
    }
    if (priorLines < 5) continue; // tiny snapshots are noisy
    const deltaPct = Math.abs(curLines - priorLines) / priorLines;
    if (deltaPct < 0.10) continue;
    out.push({
      severity: deltaPct >= 0.30 ? 'strict' : 'warn',
      step: fname.replace(/\.aria\.txt$/, ''),
      priorLines,
      currentLines: curLines,
      deltaPct: Math.round(deltaPct * 100) / 100,
    });
  }
  return out;
}

export function findPriorRun(
  runsDir: string,
  exceptPath?: string,
  journeyName?: string,
): Report | null {
  if (!existsSync(runsDir)) return null;
  // Journey-name filter: run dirs are "<journey>-<ISO timestamp>".
  // Without this, mobile/tablet/themes/etc runs cross-pollute each
  // other's diff baselines. Pattern requires a digit (year) right
  // after the prefix dash so `skillshots-poc` doesn't also match
  // `skillshots-poc-mobile-...`. Same root cause as T16 fix —
  // broadened from aria-tree comparison to ALL diff-axis events.
  const journeyPattern = journeyName
    ? new RegExp('^' + journeyName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '-\\d')
    : null;
  const entries = readdirSync(runsDir).filter(n => !n.startsWith('.')).sort();
  const candidates = entries.filter(n =>
    (!exceptPath || !exceptPath.endsWith(n))
    && (!journeyPattern || journeyPattern.test(n)),
  );
  const prior = candidates[candidates.length - 1];
  if (!prior) return null;
  const path = join(runsDir, prior, 'report.json');
  if (!existsSync(path)) return null;
  try { return JSON.parse(readFileSync(path, 'utf8')); } catch { return null; }
}
