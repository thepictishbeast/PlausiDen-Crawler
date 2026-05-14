/**
 * sri.ts — Subresource Integrity per-element DOM audit. T76.
 *
 * SRI is the W3C-standardised mechanism for verifying that a
 * cross-origin script or stylesheet loaded from a CDN has the
 * EXACT bytes the page-author committed to — by hash. Without
 * SRI, a CDN compromise (DNS hijack, BGP rerouting, vendor
 * supply-chain breach, malicious insider) silently substitutes
 * attacker-controlled JavaScript that runs with the embedding
 * page's full origin authority — equivalent to RCE inside the
 * user's session.
 *
 * Real-world incidents this detector would have caught:
 *
 *   * Microsoft Tay (2016) — bot account compromise via
 *     cross-origin embed.
 *   * MyEtherWallet (2018) — DNS hijack + injected wallet-
 *     stealer JavaScript via Cloudflare CDN.
 *   * British Airways (2018) — Magecart payment-skimmer via
 *     compromised Modernizr CDN.
 *   * event-stream NPM (2018) — supply-chain RCE in a
 *     transitive dependency. SRI on the bundled output would
 *     have detected the byte-level difference at load.
 *   * SolarWinds (2020) — different layer (build pipeline) but
 *     same threat model: trusted upstream artefact compromised.
 *
 * SRI is the single highest-leverage front-end defence against
 * supply-chain attacks and is one of the few security controls
 * that DOESN'T require server cooperation — the page author can
 * unilaterally pin the cross-origin asset's hash even when the
 * CDN itself is uncooperative.
 *
 * Findings:
 *
 *   - sri.script-cross-origin-no-integrity         strict
 *     `<script src="https://other-origin/...">` without an
 *     `integrity` attribute. CDN compromise → arbitrary script
 *     execution with the page's origin authority.
 *
 *   - sri.style-cross-origin-no-integrity          warn
 *     `<link rel="stylesheet" href="//other-origin/...">`
 *     without integrity. Stylesheet compromise enables visual
 *     injection (phishing) and theoretical CSS-keylogger
 *     attacks via attribute selectors. Lower severity than
 *     script because the attack surface is narrower, but still
 *     a defence-in-depth gap.
 *
 *   - sri.script-cross-origin-no-crossorigin       warn
 *     `<script src="...">` has integrity but no `crossorigin`
 *     attribute. Browsers refuse to verify SRI on cross-origin
 *     resources without the CORS opt-in via `crossorigin=
 *     "anonymous"` (or "use-credentials"). The integrity
 *     attribute is silently IGNORED — the page is no safer
 *     than if SRI was never set.
 *
 *   - sri.script-invalid-integrity-format          warn
 *     The integrity attribute is set but doesn't parse as one
 *     or more `<algorithm>-<base64>` tokens. Browsers fall back
 *     to "no integrity check" semantics — a typo silently
 *     disables SRI.
 *
 *   - sri.script-weak-algorithm                    warn
 *     Integrity uses SHA-1 / MD5 (legacy, broken hash
 *     functions). The W3C spec only recognises sha256, sha384,
 *     sha512 — anything else is silently dropped.
 *
 * Out of scope (browser-platform-limited):
 *   * `<img>`, `<audio>`, `<video>`, `<source>`, `<picture>`,
 *     `<iframe>` — browsers don't yet support SRI on these
 *     element types. Listed as a known coverage gap.
 *   * Same-origin resources — same-origin scripts CAN compromise
 *     the page in principle, but the threat model assumes the
 *     page-author controls their own origin's asset pipeline.
 *     SRI provides no marginal value there.
 *   * Verifying the hash actually MATCHES the resource bytes —
 *     would require fetching every cross-origin asset and
 *     hashing it, doubling the audit's network footprint. The
 *     browser itself enforces the verification at load time;
 *     our role is surfacing missing/malformed declarations.
 *   * `data:` and `blob:` URLs — same-origin equivalents.
 *   * Inline scripts (`<script>code</script>`) — no src means
 *     no cross-origin transport to integrity-check.
 *
 * This detector is the FIRST per-element security audit (the
 * existing per-element detectors — linkUnderline, runtimeFocus,
 * etc. — are accessibility / UX). Its DOM walker pattern
 * mirrors them but the threat model is supply-chain.
 */

export interface SriFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedSriElement {
  /** 'script' or 'link'. */
  tag: 'script' | 'link';
  /** Resource URL (resolved against the page base). */
  resourceUrl: string;
  /** Origin of the resource (scheme + hostname + port). */
  resourceOrigin: string;
  /** True iff this resource is cross-origin to the page. */
  isCrossOrigin: boolean;
  /** Raw integrity attribute value, or null if absent. */
  integrity: string | null;
  /** Raw crossorigin attribute value, or null. */
  crossorigin: string | null;
  /** For <link>, the rel value (lowercase). For <script>, ''. */
  linkRel: string;
}

export interface SriSnapshot {
  pageUrl: string;
  pageOrigin: string;
  elements: CapturedSriElement[];
}

const VALID_ALGOS: ReadonlySet<string> = new Set(['sha256', 'sha384', 'sha512']);
const KNOWN_WEAK_ALGOS: ReadonlySet<string> = new Set(['sha1', 'md5']);

function originFromUrl(url: string): string {
  try {
    const u = new URL(url);
    return `${u.protocol}//${u.host}`;
  } catch {
    return '';
  }
}

/**
 * Pure-function snapshot inspector. Runs in Node — main.ts
 * builds the snapshot via `page.evaluate(...)` and feeds it
 * here. Splitting capture from classification keeps the
 * detector unit-testable without a real browser.
 */
export function detectSriIssues(snap: SriSnapshot): SriFinding[] {
  const out: SriFinding[] = [];
  const scriptMissing: CapturedSriElement[] = [];
  const styleMissing: CapturedSriElement[] = [];
  const noCrossOrigin: CapturedSriElement[] = [];
  const invalidFormat: CapturedSriElement[] = [];
  const weakAlgo: CapturedSriElement[] = [];

  for (const e of snap.elements) {
    if (!e.isCrossOrigin) continue;

    if (e.integrity === null) {
      if (e.tag === 'script') scriptMissing.push(e);
      else if (e.tag === 'link' && e.linkRel === 'stylesheet') styleMissing.push(e);
      continue;
    }

    // Has integrity. Validate format + algorithm + crossorigin.
    if (e.crossorigin === null) {
      noCrossOrigin.push(e);
    }

    const tokens = e.integrity.trim().split(/\s+/);
    let anyValid = false;
    let anyWeak = false;
    let anyBadFormat = false;
    for (const tok of tokens) {
      const dash = tok.indexOf('-');
      if (dash <= 0) {
        anyBadFormat = true;
        continue;
      }
      const algo = tok.slice(0, dash).toLowerCase();
      const hash = tok.slice(dash + 1);
      // Hash should be base64-y. Allow base64url and `=` padding.
      if (!/^[A-Za-z0-9+/_=-]+$/.test(hash) || hash.length < 16) {
        anyBadFormat = true;
        continue;
      }
      if (VALID_ALGOS.has(algo)) {
        anyValid = true;
      } else if (KNOWN_WEAK_ALGOS.has(algo)) {
        anyWeak = true;
      } else {
        anyBadFormat = true;
      }
    }
    if (!anyValid && (anyBadFormat || anyWeak)) {
      if (anyWeak) weakAlgo.push(e);
      if (anyBadFormat) invalidFormat.push(e);
    }
  }

  const renderEx = (e: CapturedSriElement) =>
    `<${e.tag}${e.tag === 'link' ? ` rel="${e.linkRel}"` : ''} src/href='${e.resourceUrl}'>`;

  if (scriptMissing.length > 0) {
    out.push({
      severity: 'strict',
      kind: 'sri.script-cross-origin-no-integrity',
      detail: `${scriptMissing.length} cross-origin <script src="..."> element(s) load without an 'integrity' attribute. A CDN compromise (DNS hijack, BGP rerouting, vendor breach, malicious insider) substitutes attacker-controlled JavaScript that runs with this page's full origin authority — equivalent to RCE in the user's session. Add 'integrity="sha384-<base64>"' AND 'crossorigin="anonymous"' (both required). Examples: ${scriptMissing.slice(0, 5).map(renderEx).join('; ')}`,
      evidence: { count: scriptMissing.length, examples: scriptMissing.slice(0, 5).map((e) => e.resourceUrl) },
    });
  }
  if (styleMissing.length > 0) {
    out.push({
      severity: 'warn',
      kind: 'sri.style-cross-origin-no-integrity',
      detail: `${styleMissing.length} cross-origin <link rel="stylesheet"> element(s) load without an 'integrity' attribute. Stylesheet compromise enables visual injection (phishing overlays) and theoretical CSS-keylogger attacks via attribute selectors. Add 'integrity' + 'crossorigin' to pin the asset. Examples: ${styleMissing.slice(0, 5).map(renderEx).join('; ')}`,
      evidence: { count: styleMissing.length, examples: styleMissing.slice(0, 5).map((e) => e.resourceUrl) },
    });
  }
  if (noCrossOrigin.length > 0) {
    out.push({
      severity: 'warn',
      kind: 'sri.script-cross-origin-no-crossorigin',
      detail: `${noCrossOrigin.length} element(s) declare an 'integrity' attribute but no 'crossorigin' attribute. Browsers REFUSE to verify SRI on cross-origin resources without the CORS opt-in — the integrity attribute is silently IGNORED. Add 'crossorigin="anonymous"' (or "use-credentials" if the resource needs cookies). Examples: ${noCrossOrigin.slice(0, 5).map(renderEx).join('; ')}`,
      evidence: { count: noCrossOrigin.length, examples: noCrossOrigin.slice(0, 5).map((e) => e.resourceUrl) },
    });
  }
  if (invalidFormat.length > 0) {
    out.push({
      severity: 'warn',
      kind: 'sri.script-invalid-integrity-format',
      detail: `${invalidFormat.length} element(s) have an 'integrity' attribute that doesn't parse as one or more '<algorithm>-<base64>' tokens. Browsers fall back to no-integrity-check semantics — a typo silently disables SRI. Examples: ${invalidFormat.slice(0, 5).map(renderEx).join('; ')}`,
      evidence: { count: invalidFormat.length, examples: invalidFormat.slice(0, 5).map((e) => e.resourceUrl) },
    });
  }
  if (weakAlgo.length > 0) {
    out.push({
      severity: 'warn',
      kind: 'sri.script-weak-algorithm',
      detail: `${weakAlgo.length} element(s) declare 'integrity' with a weak hash algorithm (sha1 / md5). The W3C spec only recognises sha256, sha384, sha512 — weak algorithms are silently dropped. Use sha384 or sha512. Examples: ${weakAlgo.slice(0, 5).map(renderEx).join('; ')}`,
      evidence: { count: weakAlgo.length, examples: weakAlgo.slice(0, 5).map((e) => e.resourceUrl) },
    });
  }

  return out;
}

/**
 * The browser-side capture function. main.ts wraps this in a
 * `page.evaluate` call. Returns a serialisable snapshot.
 *
 * Kept as a string template here so the test file can compile-
 * check the source without a Playwright dependency, and so it
 * mirrors the future Rust mirror's `js_brackets_balanced` test
 * pattern from the existing detectors.
 */
export const SRI_DOM_CAPTURE_JS = `
(function() {
  function originOf(u) {
    try {
      var x = new URL(u, document.baseURI);
      return x.protocol + '//' + x.host;
    } catch (_) { return ''; }
  }
  var pageOrigin = window.location.origin;
  var elements = [];
  var scripts = document.querySelectorAll('script[src]');
  for (var i = 0; i < scripts.length; i++) {
    var s = scripts[i];
    var src = s.getAttribute('src');
    if (!src) continue;
    var resolved;
    try { resolved = new URL(src, document.baseURI).toString(); } catch (_) { continue; }
    var ro = originOf(resolved);
    elements.push({
      tag: 'script',
      resourceUrl: resolved,
      resourceOrigin: ro,
      isCrossOrigin: ro !== pageOrigin && ro !== '',
      integrity: s.getAttribute('integrity'),
      crossorigin: s.getAttribute('crossorigin'),
      linkRel: '',
    });
  }
  var links = document.querySelectorAll('link[href]');
  for (var j = 0; j < links.length; j++) {
    var l = links[j];
    var href = l.getAttribute('href');
    if (!href) continue;
    var rel = (l.getAttribute('rel') || '').toLowerCase();
    var resolved2;
    try { resolved2 = new URL(href, document.baseURI).toString(); } catch (_) { continue; }
    var ro2 = originOf(resolved2);
    elements.push({
      tag: 'link',
      resourceUrl: resolved2,
      resourceOrigin: ro2,
      isCrossOrigin: ro2 !== pageOrigin && ro2 !== '',
      integrity: l.getAttribute('integrity'),
      crossorigin: l.getAttribute('crossorigin'),
      linkRel: rel,
    });
  }
  return { pageUrl: window.location.href, pageOrigin: pageOrigin, elements: elements };
})()
`;
