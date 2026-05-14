/**
 * speculationRules.ts — Speculation Rules API audit. T76 cycle 92.
 *
 * The Speculation Rules API (W3C Editor's Draft 2024, shipping in
 * Chromium 119+ from late 2023) lets pages declare URLs the
 * browser should speculatively prefetch (download + cache) or
 * prerender (download + parse + execute in a hidden tab).
 *
 *   <script type="speculationrules">
 *     { "prerender": [{ "where": { "href_matches": "/article/*" } }],
 *       "prefetch":  [{ "urls": ["/api/feed.json"] }] }
 *   </script>
 *
 * Three threat models this detector covers:
 *
 *   1. **Privacy leak via cross-origin prerender.** Prerendering
 *      a cross-origin URL fetches the resource AND executes its
 *      JavaScript inside a hidden tab BEFORE the user opts in
 *      to navigation. The visited cross-origin server learns the
 *      user's IP, browser fingerprint, Accept-Language, etc.,
 *      with no user interaction. The W3C spec accepts this only
 *      when the rule declares `"requires":
 *      ["anonymous-client-ip-when-cross-origin"]` AND the user's
 *      browser honours the anonymous-client-ip restriction. A
 *      cross-origin prerender WITHOUT that opt-out is a privacy
 *      regression. Strict.
 *
 *   2. **Deprecated `urls` list-rule form for cross-origin
 *      targets.** The 2023 draft permitted `[{ "urls":
 *      ["https://other.example/x"] }]`; the 2024 draft restricts
 *      cross-origin URLs to the document-rules form that includes
 *      `where` predicates and explicit referrer-policy controls.
 *      Pages still using the legacy form are speculating against
 *      origins they haven't audited. Warn.
 *
 *   3. **Malformed speculationrules JSON.** A typo silently
 *      disables ALL rules in the block — the browser parses the
 *      block as JSON, and an invalid block emits a single console
 *      warning + drops every rule. Hard to notice without an
 *      audit step. Warn.
 *
 *   4. **Empty rule sets.** A page that declares a
 *      speculationrules block but contains zero actionable rules
 *      (empty object, only unknown keys, etc.) is almost always
 *      a copy-paste error. Warn.
 *
 *   5. **Eager-policy prefetches.** Prefetches with
 *      `"eagerness": "eager"` start before the user even hovers
 *      the link. Used carelessly on a metered connection or with
 *      large assets, this burns the user's mobile data budget
 *      without their knowledge. The W3C-recommended default is
 *      `moderate` (start on hover) or `conservative` (start on
 *      pointerdown). Warn.
 *
 *   6. **Same-document prerender (the safe case).** A page that
 *      prerenders ITS OWN slugs is fine; the user's data is going
 *      to the same origin either way. No finding — captured for
 *      audit-trail completeness.
 *
 * REGRESSION-GUARD (cycle 92): the dogfood log entry 25 listed
 * three "remaining roadmap" detectors (fontLoading, hstsHeader,
 * xFrameOptions) that were already shipped — the log was stale.
 * The next genuine coverage gap is Speculation Rules; this
 * detector closes it. Detector axis count: 50 → 51.
 *
 * SUPERSOCIETY: prerendering is exactly the kind of "browser
 * helpfully fetches things before the user asked" feature that
 * lets a malicious or sloppy page leak the user's browsing
 * pattern to third parties. Defence-in-depth.
 */

export interface SpeculationRulesFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedSpecRulesBlock {
  /** The raw text content of the <script> tag. */
  rawText: string;
  /** True iff the JSON parses; below fields valid only if true. */
  parsedOk: boolean;
  /** Parse error message if parsedOk is false. */
  parseError: string | null;
  /** Number of rules under each action key (prefetch | prerender). */
  prefetchCount: number;
  prerenderCount: number;
  /** True iff any rule's eagerness is "eager". */
  hasEager: boolean;
  /** True iff any rule uses the legacy `urls` list form with a
   *  cross-origin URL. */
  hasLegacyCrossOriginUrls: boolean;
  /** True iff any prerender rule targets a cross-origin URL AND
   *  fails to declare requires:
   *  ["anonymous-client-ip-when-cross-origin"]. */
  hasUnshieldedCrossOriginPrerender: boolean;
  /** Examples for audit-trail (first 3 URLs of each problem). */
  unshieldedCrossOriginExamples: string[];
  legacyCrossOriginUrlExamples: string[];
  /** True iff parsed JSON has prefetch/prerender keys but every
   *  rule list is empty. */
  isEmptyRuleSet: boolean;
}

export interface SpeculationRulesSnapshot {
  pageUrl: string;
  pageOrigin: string;
  /** Zero or more <script type="speculationrules"> blocks. */
  blocks: CapturedSpecRulesBlock[];
}

/**
 * Pure-function classifier. Runs in Node; the snapshot is built
 * via page.evaluate(SPECULATION_RULES_DOM_CAPTURE_JS) in main.ts.
 */
export function detectSpeculationRulesIssues(
  snap: SpeculationRulesSnapshot,
): SpeculationRulesFinding[] {
  const out: SpeculationRulesFinding[] = [];

  for (const block of snap.blocks) {
    if (!block.parsedOk) {
      out.push({
        severity: 'warn',
        kind: 'speculation-rules.invalid-json',
        detail:
          `A <script type="speculationrules"> block contains invalid JSON: ` +
          `${block.parseError ?? '(unknown)'}. The browser silently drops the ` +
          `ENTIRE block — every rule inside fails. A typo here looks like ` +
          `"performance fine but features somehow broken" with no console ` +
          `error visible to the operator. Validate the JSON.`,
        evidence: {
          parseError: block.parseError,
          rawPreview: block.rawText.slice(0, 120),
        },
      });
      continue;
    }

    if (block.isEmptyRuleSet) {
      out.push({
        severity: 'warn',
        kind: 'speculation-rules.empty-rule-set',
        detail:
          `A <script type="speculationrules"> block parses to JSON with ` +
          `prefetch/prerender keys but every rule list is empty — almost ` +
          `always a copy-paste error. Either remove the block or populate ` +
          `at least one rule.`,
        evidence: {
          prefetchCount: block.prefetchCount,
          prerenderCount: block.prerenderCount,
        },
      });
    }

    if (block.hasUnshieldedCrossOriginPrerender) {
      out.push({
        severity: 'strict',
        kind: 'speculation-rules.cross-origin-prerender-no-anonymous-ip',
        detail:
          `Prerender rule targets a cross-origin URL WITHOUT declaring ` +
          `"requires": ["anonymous-client-ip-when-cross-origin"]. The ` +
          `browser will fetch + parse + execute the target page in a ` +
          `hidden tab BEFORE the user opts in to navigation, leaking IP ` +
          `+ browser fingerprint + Accept-Language to the cross-origin ` +
          `server with no user interaction. Add the requires clause OR ` +
          `switch to "prefetch" (no JS execution). Examples: ` +
          `${block.unshieldedCrossOriginExamples.slice(0, 3).join(', ')}`,
        evidence: {
          examples: block.unshieldedCrossOriginExamples,
        },
      });
    }

    if (block.hasLegacyCrossOriginUrls) {
      out.push({
        severity: 'warn',
        kind: 'speculation-rules.legacy-cross-origin-urls-form',
        detail:
          `A rule uses the legacy {"urls": [...]} list form with a ` +
          `cross-origin URL. The 2024 W3C draft restricts cross-origin ` +
          `targets to the document-rules form with "where" predicates + ` +
          `explicit referrer-policy controls; the legacy form is being ` +
          `phased out. Migrate to {"where": {...}, "referrer_policy": ` +
          `"strict-origin-when-cross-origin"} or similar. Examples: ` +
          `${block.legacyCrossOriginUrlExamples.slice(0, 3).join(', ')}`,
        evidence: {
          examples: block.legacyCrossOriginUrlExamples,
        },
      });
    }

    if (block.hasEager) {
      out.push({
        severity: 'warn',
        kind: 'speculation-rules.eager-eagerness',
        detail:
          `A rule has "eagerness": "eager" — the browser starts the ` +
          `prefetch/prerender as soon as the rule is seen, before any ` +
          `user interaction. On metered connections or large assets ` +
          `this burns the user's mobile data budget without their ` +
          `knowledge. Prefer "moderate" (start on hover, the default) ` +
          `or "conservative" (start on pointerdown).`,
        evidence: {},
      });
    }
  }

  return out;
}

/**
 * In-browser snapshot builder. Stringified for page.evaluate.
 * Hand-rolled DOM walk; no external deps. Matches the cycle 25
 * SRI pattern (capture-then-classify, snapshot is serialisable).
 *
 * BUG ASSUMPTION: the page may have multiple
 * <script type="speculationrules"> blocks. We process every one.
 */
export const SPECULATION_RULES_DOM_CAPTURE_JS = `
(function() {
  function originOf(u) {
    try {
      var x = new URL(u, document.baseURI);
      return x.protocol + '//' + x.host;
    } catch (_) { return ''; }
  }
  var pageOrigin = window.location.origin;
  var nodes = document.querySelectorAll('script[type="speculationrules"]');
  var blocks = [];

  for (var i = 0; i < nodes.length; i++) {
    var raw = nodes[i].textContent || '';
    var parsed = null;
    var parseError = null;
    try {
      parsed = JSON.parse(raw);
    } catch (e) {
      parseError = (e && e.message) ? String(e.message) : 'parse failed';
    }

    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      blocks.push({
        rawText: raw,
        parsedOk: parseError === null && parsed !== null,
        parseError: parseError,
        prefetchCount: 0,
        prerenderCount: 0,
        hasEager: false,
        hasLegacyCrossOriginUrls: false,
        hasUnshieldedCrossOriginPrerender: false,
        unshieldedCrossOriginExamples: [],
        legacyCrossOriginUrlExamples: [],
        isEmptyRuleSet: false,
      });
      continue;
    }

    var prefetch = Array.isArray(parsed.prefetch) ? parsed.prefetch : [];
    var prerender = Array.isArray(parsed.prerender) ? parsed.prerender : [];
    var hasEager = false;
    var hasLegacy = false;
    var hasUnshielded = false;
    var legacyExamples = [];
    var unshieldedExamples = [];

    function inspectRule(rule, action) {
      if (!rule || typeof rule !== 'object') return;
      if (rule.eagerness === 'eager') hasEager = true;
      var requires = Array.isArray(rule.requires) ? rule.requires : [];
      var hasAnonymousIp = requires.indexOf(
        'anonymous-client-ip-when-cross-origin'
      ) >= 0;
      // legacy {urls: [...]} form
      if (Array.isArray(rule.urls)) {
        for (var k = 0; k < rule.urls.length; k++) {
          var u = String(rule.urls[k]);
          var resolved;
          try {
            resolved = new URL(u, document.baseURI).toString();
          } catch (_) { continue; }
          var ro = originOf(resolved);
          var isCross = ro !== pageOrigin && ro !== '';
          if (isCross) {
            if (legacyExamples.length < 5) legacyExamples.push(resolved);
            hasLegacy = true;
            if (action === 'prerender' && !hasAnonymousIp) {
              if (unshieldedExamples.length < 5) unshieldedExamples.push(resolved);
              hasUnshielded = true;
            }
          }
        }
      }
      // 2024 form: {where: {...}} — cross-origin detection here
      // requires href_matches to be a full URL with origin (rare;
      // most use path-only patterns). If the pattern itself names
      // a cross-origin host, we flag it.
      if (rule.where && typeof rule.where === 'object') {
        var hrefMatches = rule.where.href_matches;
        var arr = Array.isArray(hrefMatches) ? hrefMatches : (hrefMatches ? [hrefMatches] : []);
        for (var m = 0; m < arr.length; m++) {
          var pat = String(arr[m]);
          // Cheap heuristic: pattern starts with scheme://host?
          var schemeMatch = pat.match(/^https?:\\/\\/[^\\/]+/);
          if (schemeMatch) {
            var pro = originOf(schemeMatch[0]);
            if (pro !== pageOrigin && pro !== '') {
              if (action === 'prerender' && !hasAnonymousIp) {
                if (unshieldedExamples.length < 5) unshieldedExamples.push(pat);
                hasUnshielded = true;
              }
            }
          }
        }
      }
    }

    for (var p = 0; p < prefetch.length; p++) inspectRule(prefetch[p], 'prefetch');
    for (var q = 0; q < prerender.length; q++) inspectRule(prerender[q], 'prerender');

    var isEmpty = prefetch.length === 0 && prerender.length === 0;

    blocks.push({
      rawText: raw,
      parsedOk: true,
      parseError: null,
      prefetchCount: prefetch.length,
      prerenderCount: prerender.length,
      hasEager: hasEager,
      hasLegacyCrossOriginUrls: hasLegacy,
      hasUnshieldedCrossOriginPrerender: hasUnshielded,
      unshieldedCrossOriginExamples: unshieldedExamples,
      legacyCrossOriginUrlExamples: legacyExamples,
      isEmptyRuleSet: isEmpty,
    });
  }

  return {
    pageUrl: window.location.href,
    pageOrigin: pageOrigin,
    blocks: blocks,
  };
})()
`;
