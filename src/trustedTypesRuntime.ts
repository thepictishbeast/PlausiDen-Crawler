/**
 * trustedTypesRuntime.ts — runtime DOM-sink monitor for the
 * Trusted Types XSS-defence layer. T76 cycle 57.
 *
 * Trusted Types is a CSP-Level-3 extension: any string assigned
 * to a "sink" (innerHTML, document.write, eval, etc.) must be a
 * Trusted* policy-issued object, otherwise the browser rejects
 * the assignment. Strict CSP + Trusted Types together provide
 * the strongest in-browser DOM-XSS mitigation available today.
 *
 * The runtime monitor injects a content-script via addInitScript
 * that proxies every known sink. Each call is recorded to
 * `window.__loomTTSinks` with the sink kind, a trimmed value
 * preview, and whether the value was a Trusted* instance. The
 * detector reads this log after each step and emits:
 *
 *   - tt.unprotected-sink           warn
 *     A DOM-sink call where the argument was a plain string,
 *     AND the page has no `require-trusted-types-for 'script'`
 *     directive in its CSP. The browser executed the
 *     assignment; under strict CSP it would have been blocked.
 *
 *   - tt.directive-missing          warn
 *     The page has inline or external scripts AND no
 *     `require-trusted-types-for 'script'` directive. Even with
 *     hash-pinned CSP, runtime DOM-XSS sinks are unprotected.
 *
 *   - tt.policy-undeclared          info
 *     The page DOES have require-trusted-types-for but did NOT
 *     declare an allowlist via `trusted-types <names>`. Any
 *     policy can be installed; defense is weaker than
 *     name-restricted policies.
 *
 * Pages that emit no scripts AND no sink calls are silently
 * clean — Trusted Types has nothing to protect.
 *
 * SUPERSOCIETY: this is the next layer beyond hash-pinned CSP.
 * Cycle 54 protected against inline-script injection at parse
 * time. Trusted Types protects against runtime DOM-XSS — even
 * approved scripts can't pipe user-controlled data through a
 * sink without an explicit Trusted* conversion.
 */
import type { Page } from 'playwright';

export interface CapturedTrustedTypesSink {
  /** Which sink was called. */
  kind:
    | 'innerHTML'
    | 'outerHTML'
    | 'insertAdjacentHTML'
    | 'document.write'
    | 'document.writeln'
    | 'setTimeout(string)'
    | 'setInterval(string)'
    | 'createContextualFragment';
  /** First 200 chars of the assigned value (or stringified arg). */
  preview: string;
  /** True iff the argument was a Trusted* instance (covered). */
  trusted: boolean;
  /** When the call fired, relative to page open. */
  t: number;
}

export interface TrustedTypesSnapshot {
  pageUrl: string;
  /** Sink calls captured during this step. */
  sinks: CapturedTrustedTypesSink[];
  /** True iff the response has `require-trusted-types-for 'script'`. */
  hasRequireDirective: boolean;
  /** Raw value of the `trusted-types` directive, empty if absent. */
  trustedTypesDirective: string;
  /** True iff the page has any <script> tag (inline or external). */
  hasScripts: boolean;
}

export interface TrustedTypesFinding {
  severity: 'strict' | 'warn' | 'info';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

/**
 * Install the runtime sink monitor on every page in the context.
 * Must be called BEFORE the first navigation — addInitScript is
 * fire-and-forget; only pages created after install carry the
 * probe.
 *
 * The injected script:
 *   1. Captures the ORIGINAL sink setters/methods (Object.
 *      getOwnPropertyDescriptor / native references).
 *   2. Defines proxy getters/setters that record the call,
 *      then forward to the original.
 *   3. Idempotent — re-running on the same window is a no-op.
 *
 * REGRESSION-GUARD: monkey-patching DOM globals is fragile.
 * Each proxy wraps the call in try/catch and falls back to the
 * original on any error, so the probe NEVER breaks the page
 * even if its assumptions about prototype shape are wrong.
 */
export async function installTrustedTypesProbe(
  page: Page,
): Promise<void> {
  const probe = `
    (function() {
      if (window.__loomTTProbeInstalled) return;
      window.__loomTTProbeInstalled = true;
      var sinks = [];
      var startedAt = performance.now();
      window.__loomTTSinks = sinks;

      function record(kind, value, trusted) {
        try {
          var preview = '';
          if (typeof value === 'string') preview = value;
          else if (value && typeof value.toString === 'function') preview = String(value);
          if (preview.length > 200) preview = preview.slice(0, 200);
          sinks.push({
            kind: kind,
            preview: preview,
            trusted: !!trusted,
            t: Math.round(performance.now() - startedAt),
          });
        } catch (e) { /* never break the page */ }
      }

      function isTrusted(v) {
        try {
          return !!(window.TrustedHTML && v instanceof window.TrustedHTML) ||
                 !!(window.TrustedScript && v instanceof window.TrustedScript) ||
                 !!(window.TrustedScriptURL && v instanceof window.TrustedScriptURL);
        } catch (e) { return false; }
      }

      // Monkey-patch innerHTML/outerHTML setters on every Element
      // subclass. The descriptor lives on Element.prototype.
      try {
        var elProto = Element.prototype;
        var ihDesc = Object.getOwnPropertyDescriptor(elProto, 'innerHTML');
        if (ihDesc && ihDesc.set) {
          var origIH = ihDesc.set;
          Object.defineProperty(elProto, 'innerHTML', {
            configurable: true,
            enumerable: ihDesc.enumerable,
            get: ihDesc.get,
            set: function(v) {
              record('innerHTML', v, isTrusted(v));
              try { return origIH.call(this, v); }
              catch (e) { throw e; }
            },
          });
        }
        var ohDesc = Object.getOwnPropertyDescriptor(elProto, 'outerHTML');
        if (ohDesc && ohDesc.set) {
          var origOH = ohDesc.set;
          Object.defineProperty(elProto, 'outerHTML', {
            configurable: true,
            enumerable: ohDesc.enumerable,
            get: ohDesc.get,
            set: function(v) {
              record('outerHTML', v, isTrusted(v));
              try { return origOH.call(this, v); }
              catch (e) { throw e; }
            },
          });
        }
        var origIAH = elProto.insertAdjacentHTML;
        if (typeof origIAH === 'function') {
          elProto.insertAdjacentHTML = function(pos, html) {
            record('insertAdjacentHTML', html, isTrusted(html));
            return origIAH.call(this, pos, html);
          };
        }
      } catch (e) { /* prototype patching failed; non-fatal */ }

      // document.write / writeln
      try {
        var origWrite = document.write;
        document.write = function() {
          for (var i = 0; i < arguments.length; i++)
            record('document.write', arguments[i], isTrusted(arguments[i]));
          return origWrite.apply(this, arguments);
        };
        var origWriteln = document.writeln;
        document.writeln = function() {
          for (var i = 0; i < arguments.length; i++)
            record('document.writeln', arguments[i], isTrusted(arguments[i]));
          return origWriteln.apply(this, arguments);
        };
      } catch (e) { }

      // REGRESSION-GUARD cycle 57: eval / Function not proxied.
      // Playwright page.evaluate API serialises the function
      // and invokes eval inside the page context, which makes
      // the proxy fire dozens of false positives per audit
      // step. Discriminating Playwright-internal eval from
      // app-level eval requires call-site introspection that
      // is not reliable. The DOM-sink proxies below are the
      // primary value of this detector; eval/Function abuse is
      // already covered by the cspPolicy detector (no
      // unsafe-eval in script-src) and by inlineScript
      // (event-handler / javascript: URI walks).

      // setTimeout / setInterval — only flag string-form (the
      // implicit-eval form). Function-form is safe.
      try {
        var origSetTimeout = window.setTimeout;
        window.setTimeout = function(handler) {
          if (typeof handler === 'string') {
            record('setTimeout(string)', handler, isTrusted(handler));
          }
          return origSetTimeout.apply(this, arguments);
        };
        var origSetInterval = window.setInterval;
        window.setInterval = function(handler) {
          if (typeof handler === 'string') {
            record('setInterval(string)', handler, isTrusted(handler));
          }
          return origSetInterval.apply(this, arguments);
        };
      } catch (e) { }

      // Range.prototype.createContextualFragment
      try {
        if (typeof Range !== 'undefined' && Range.prototype.createContextualFragment) {
          var origCCF = Range.prototype.createContextualFragment;
          Range.prototype.createContextualFragment = function(html) {
            record('createContextualFragment', html, isTrusted(html));
            return origCCF.call(this, html);
          };
        }
      } catch (e) { }
    })();
  `;
  await page.addInitScript({ content: probe });
}

/**
 * Read the current sink log + CSP context from a page after
 * navigation. Pure read — doesn't mutate the probe state, but
 * also doesn't clear it (call again for each step to see the
 * cumulative log per page).
 */
export async function captureTrustedTypesSnapshot(
  page: Page,
  cspHeader: string | undefined,
): Promise<TrustedTypesSnapshot> {
  const pageUrl = page.url();
  const raw = await page.evaluate(() => {
    const sinks = ((window as any).__loomTTSinks || []) as CapturedTrustedTypesSink[];
    // Also check meta-CSP for require-trusted-types-for and
    // trusted-types directives. Response-header CSP is enriched
    // by main.ts (more authoritative).
    let metaCsp = '';
    const metas = document.querySelectorAll('meta[http-equiv]');
    for (let i = 0; i < metas.length; i++) {
      const equiv = (metas[i].getAttribute('http-equiv') || '').toLowerCase();
      if (equiv === 'content-security-policy') {
        metaCsp = metas[i].getAttribute('content') || '';
        break;
      }
    }
    const hasScripts = document.querySelectorAll('script').length > 0;
    return { sinks, metaCsp, hasScripts };
  }) as { sinks: CapturedTrustedTypesSink[]; metaCsp: string; hasScripts: boolean };

  const cspText = cspHeader || raw.metaCsp || '';
  let hasRequireDirective = false;
  let trustedTypesDirective = '';
  for (const seg of cspText.split(';')) {
    const t = seg.trim().toLowerCase();
    if (t.startsWith("require-trusted-types-for")) {
      // accept variants like 'require-trusted-types-for \'script\''
      hasRequireDirective = true;
    } else if (t.startsWith('trusted-types ') || t === 'trusted-types') {
      trustedTypesDirective = seg.trim().slice('trusted-types'.length).trim();
    }
  }

  return {
    pageUrl,
    sinks: raw.sinks,
    hasRequireDirective,
    trustedTypesDirective,
    hasScripts: raw.hasScripts,
  };
}

export function detectTrustedTypesIssues(
  snap: TrustedTypesSnapshot,
): TrustedTypesFinding[] {
  const out: TrustedTypesFinding[] = [];

  // T76 cycle 57: tt.unprotected-sink — sink calls observed
  // without require-trusted-types-for. The browser executed the
  // assignment; if user-controlled data flowed through, this is
  // a DOM-XSS sink waiting to happen.
  const untrusted = snap.sinks.filter((s) => !s.trusted);
  if (untrusted.length > 0 && !snap.hasRequireDirective) {
    const byKind: Record<string, number> = {};
    for (const s of untrusted) byKind[s.kind] = (byKind[s.kind] || 0) + 1;
    const examples = untrusted.slice(0, 5).map(
      (s) => `${s.kind} <- '${s.preview.slice(0, 80)}'`,
    );
    out.push({
      severity: 'warn',
      kind: 'tt.unprotected-sink',
      detail: `${untrusted.length} DOM-sink call(s) executed with plain (non-Trusted) values AND the page has no \`require-trusted-types-for 'script'\` CSP directive. Under strict CSP the browser would reject these. Sinks: ${Object.entries(byKind).map(([k, v]) => `${k}×${v}`).join(', ')}. Examples: ${examples.join('; ')}`,
      evidence: { count: untrusted.length, byKind, examples },
    });
  }

  // T76 cycle 57: tt.directive-missing — page has scripts but
  // no require-trusted-types-for. Even with hash-pinned inline
  // scripts, runtime DOM-XSS via innerHTML etc is unprotected.
  if (snap.hasScripts && !snap.hasRequireDirective) {
    out.push({
      severity: 'warn',
      kind: 'tt.directive-missing',
      detail: `Page has scripts AND no \`require-trusted-types-for 'script'\` directive in CSP. Add the directive (CSP-Level-3) so the browser rejects DOM-sink assignments of plain strings. Trusted Types is the strongest in-browser DOM-XSS mitigation currently shipping.`,
      evidence: {
        hasScripts: true,
        hasRequireDirective: false,
      },
    });
  }

  // T76 cycle 57: tt.policy-undeclared — has require-trusted-
  // types-for but no `trusted-types <names>` allowlist. Any
  // policy can be installed at runtime; defense in depth would
  // restrict to a named policy.
  if (snap.hasRequireDirective && !snap.trustedTypesDirective) {
    out.push({
      severity: 'info',
      kind: 'tt.policy-undeclared',
      detail: `\`require-trusted-types-for 'script'\` is set but no \`trusted-types <policy-names>\` allowlist is declared. Any policy can register; restrict to named policies for tighter defense.`,
      evidence: { hasRequireDirective: true, trustedTypesDirective: '' },
    });
  }

  return out;
}
