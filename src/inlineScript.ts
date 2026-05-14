/**
 * inlineScript.ts — inline-script + event-handler attribute
 * + javascript: URI per-element DOM audit. T76 cycle 30.
 *
 * Threat model: pages with `script-src 'nonce-<random>'` (the
 * modern best-practice CSP) require EVERY inline `<script>`
 * block to carry the matching nonce attribute. Without nonce,
 * the script is silently dropped — the page either breaks or
 * (more dangerously) silently misses the security control.
 * Even WITHOUT a strict CSP, inline scripts are the
 * traditional stored-XSS sink: any HTML-injection sink that
 * lets an attacker write a `<script>` tag executes
 * immediately. Pages that have NO inline scripts (the
 * supersociety baseline) close that surface entirely.
 *
 * Three sub-classes detected:
 *
 *   1. Inline <script>code</script> blocks. The most common.
 *      Should be migrated to external <script src> with SRI,
 *      or kept inline with a per-page nonce.
 *
 *   2. Event-handler attributes (onclick="...", onload="...",
 *      etc.). These are "inline scripts" too and bypass
 *      script-src nonce — they need 'unsafe-inline' or the
 *      newer 'unsafe-hashes' to work under CSP. Should be
 *      migrated to addEventListener.
 *
 *   3. javascript: URIs (<a href="javascript:...">,
 *      <iframe src="javascript:...">). Same XSS surface;
 *      harder to migrate but trivial to catch.
 *
 * Findings:
 *
 *   - inline-script.present-without-nonce       warn
 *     Inline <script>...</script> block without a `nonce`
 *     attribute. Strict-CSP bypass; pre-CSP stored-XSS sink.
 *
 *   - inline-script.event-handler-attribute     warn
 *     `onclick`, `onload`, `onmouseover`, etc. attribute on
 *     any element. CSP cannot nonce these.
 *
 *   - inline-script.javascript-uri              warn
 *     `<a href="javascript:...">`, `<iframe src="javascript:
 *     ...">`, etc. Old-school JS-execution sink.
 *
 *   - inline-script.no-csp-but-inline           warn
 *     Inline scripts present AND no Content-Security-Policy
 *     header at all. The page has nothing stopping a stored-
 *     XSS injection from executing. Lower-priority finding
 *     because it's redundant with `csp.missing` from the CSP
 *     detector — but specifically calls out that the inline
 *     scripts make the missing CSP more dangerous than usual.
 *
 * Out of scope:
 *   * <script src="..."> external scripts — covered by SRI
 *     and CSP detectors.
 *   * Dynamic injection via document.write, Element.innerHTML,
 *     eval, setTimeout(string), Function() — runtime sinks,
 *     not detectable from a static DOM walk.
 *   * Same-origin <iframe> contents — would need recursive
 *     walks; queued.
 *   * <noscript> contents — fall-through for JS-disabled
 *     environments, never executed.
 *
 * Per-element DOM walk via page.evaluate. SECOND per-element
 * security detector (after SRI). Bespoke wiring; the
 * `perElementDetector` helper extraction question is now
 * surfaceable but the SRI / inline-script detectors have
 * different walker shapes (SRI walks two specific tag classes,
 * inline-script walks all elements for handlers). Defer
 * extraction until a third per-element security detector
 * lands.
 */

export interface InlineScriptFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedInlineScript {
  /** Truncated source (first 200 chars). */
  src: string;
  /** True iff a 'nonce' attribute was present (any value). */
  hasNonce: boolean;
}

export interface CapturedEventHandler {
  tag: string;
  /** The attribute name (e.g. 'onclick'). */
  attribute: string;
  /** Truncated value (first 200 chars). */
  value: string;
}

export interface CapturedJavascriptUri {
  tag: string;
  /** Truncated href/src value. */
  uri: string;
}

export interface InlineScriptSnapshot {
  pageUrl: string;
  /** True iff Content-Security-Policy header is present. */
  hasCsp: boolean;
  inlineScripts: CapturedInlineScript[];
  eventHandlers: CapturedEventHandler[];
  javascriptUris: CapturedJavascriptUri[];
}

export function detectInlineScriptIssues(
  snap: InlineScriptSnapshot,
): InlineScriptFinding[] {
  const out: InlineScriptFinding[] = [];

  const noNonce = snap.inlineScripts.filter((s) => !s.hasNonce);
  if (noNonce.length > 0) {
    const examples = noNonce.slice(0, 5).map((s) => `<script> '${s.src}'`);
    out.push({
      severity: 'warn',
      kind: 'inline-script.present-without-nonce',
      detail: `${noNonce.length} inline <script> block(s) without a 'nonce' attribute. Under a strict CSP ('script-src nonce-<random>'), these are silently dropped. Without a CSP, they're stored-XSS sinks: any HTML-injection vulnerability that writes a <script> tag executes immediately. Migrate to external <script src> with SRI, or add a per-page nonce. Examples: ${examples.join('; ')}`,
      evidence: { count: noNonce.length, examples },
    });
  }

  if (snap.eventHandlers.length > 0) {
    const examples = snap.eventHandlers.slice(0, 5).map(
      (h) => `<${h.tag} ${h.attribute}="${h.value}">`,
    );
    out.push({
      severity: 'warn',
      kind: 'inline-script.event-handler-attribute',
      detail: `${snap.eventHandlers.length} element(s) carry an event-handler attribute (onclick, onload, onmouseover, etc.). CSP cannot nonce these — they require 'unsafe-inline' or 'unsafe-hashes' to work, both of which weaken protection. Migrate to addEventListener in an external script. Examples: ${examples.join('; ')}`,
      evidence: { count: snap.eventHandlers.length, examples },
    });
  }

  if (snap.javascriptUris.length > 0) {
    const examples = snap.javascriptUris.slice(0, 5).map(
      (u) => `<${u.tag} src/href="${u.uri}">`,
    );
    out.push({
      severity: 'warn',
      kind: 'inline-script.javascript-uri',
      detail: `${snap.javascriptUris.length} element(s) use a 'javascript:' URI. Old-school JS-execution sink; CSP cannot block without 'unsafe-inline'. Replace with addEventListener-bound handlers. Examples: ${examples.join('; ')}`,
      evidence: { count: snap.javascriptUris.length, examples },
    });
  }

  // Composite finding: inline scripts without ANY CSP at all.
  // Prioritises actionability — if csp.missing already fires
  // separately, this adds the "...AND you have inline scripts
  // that an injection can use" detail.
  if (!snap.hasCsp && (noNonce.length > 0 || snap.eventHandlers.length > 0 || snap.javascriptUris.length > 0)) {
    out.push({
      severity: 'warn',
      kind: 'inline-script.no-csp-but-inline',
      detail: `Page has inline scripts/handlers AND no Content-Security-Policy header. Nothing stops a stored-XSS injection from executing. The cspPolicy detector also fires 'csp.missing' on this; the additional finding here calls out that the inline scripts make the missing CSP particularly dangerous. Add a strict CSP first (script-src 'self' 'nonce-<random>'), then migrate inline scripts to external src + nonce.`,
      evidence: {
        inlineScripts: noNonce.length,
        eventHandlers: snap.eventHandlers.length,
        javascriptUris: snap.javascriptUris.length,
      },
    });
  }

  return out;
}

/**
 * Browser-side capture function. main.ts wraps this in a
 * page.evaluate. Mirrors the SRI_DOM_CAPTURE_JS pattern so a
 * future Rust mirror (T75) can compile-check the source.
 *
 * Inline event-handler attribute names are exhaustively listed
 * because querySelector can't match attribute-name patterns.
 * Pulled from MDN's Element event-handler property table —
 * any DOM property that starts with 'on'.
 */
export const INLINE_SCRIPT_DOM_CAPTURE_JS = `
(function() {
  var EVENT_ATTRS = [
    'onabort','onafterprint','onanimationend','onanimationiteration','onanimationstart',
    'onauxclick','onbeforeprint','onbeforeunload','onblur','oncancel','oncanplay',
    'oncanplaythrough','onchange','onclick','onclose','oncontextmenu','oncopy','oncuechange',
    'oncut','ondblclick','ondrag','ondragend','ondragenter','ondragexit','ondragleave',
    'ondragover','ondragstart','ondrop','ondurationchange','onemptied','onended','onerror',
    'onfocus','onformdata','ongotpointercapture','onhashchange','oninput','oninvalid',
    'onkeydown','onkeypress','onkeyup','onlanguagechange','onload','onloadeddata',
    'onloadedmetadata','onloadstart','onlostpointercapture','onmessage','onmessageerror',
    'onmousedown','onmouseenter','onmouseleave','onmousemove','onmouseout','onmouseover',
    'onmouseup','onoffline','ononline','onpagehide','onpageshow','onpaste','onpause',
    'onplay','onplaying','onpointercancel','onpointerdown','onpointerenter','onpointerleave',
    'onpointermove','onpointerout','onpointerover','onpointerup','onpopstate','onprogress',
    'onratechange','onreset','onresize','onscroll','onsecuritypolicyviolation','onseeked',
    'onseeking','onselect','onselectionchange','onselectstart','onslotchange','onstalled',
    'onstorage','onsubmit','onsuspend','ontimeupdate','ontoggle','ontouchcancel','ontouchend',
    'ontouchmove','ontouchstart','ontransitionend','onunhandledrejection','onunload',
    'onvolumechange','onwaiting','onwheel'
  ];
  var inlineScripts = [];
  var scripts = document.querySelectorAll('script');
  for (var i = 0; i < scripts.length; i++) {
    var s = scripts[i];
    if (s.hasAttribute('src')) continue; // external — out of scope
    var src = (s.textContent || '').slice(0, 200);
    inlineScripts.push({
      src: src,
      hasNonce: s.hasAttribute('nonce'),
    });
  }
  var eventHandlers = [];
  // Walk every element for known event-handler attribute names.
  var all = document.querySelectorAll('*');
  for (var j = 0; j < all.length; j++) {
    var el = all[j];
    for (var k = 0; k < EVENT_ATTRS.length; k++) {
      var attr = EVENT_ATTRS[k];
      if (el.hasAttribute(attr)) {
        eventHandlers.push({
          tag: el.tagName.toLowerCase(),
          attribute: attr,
          value: (el.getAttribute(attr) || '').slice(0, 200),
        });
      }
    }
  }
  var javascriptUris = [];
  var hrefEls = document.querySelectorAll('[href], [src], [action], [formaction]');
  for (var m = 0; m < hrefEls.length; m++) {
    var he = hrefEls[m];
    var attrs = ['href', 'src', 'action', 'formaction'];
    for (var n = 0; n < attrs.length; n++) {
      var a = attrs[n];
      var v = he.getAttribute(a);
      if (v && v.toLowerCase().trim().indexOf('javascript:') === 0) {
        javascriptUris.push({
          tag: he.tagName.toLowerCase(),
          uri: v.slice(0, 200),
        });
      }
    }
  }
  // CSP detection: enforced (response Content-Security-Policy)
  // OR meta http-equiv. We can't read response headers from
  // page-script context; main.ts overrides hasCsp with the
  // header-truth source after capture. Here we cover the meta
  // case as a fallback.
  var hasCsp = false;
  var metas = document.querySelectorAll('meta[http-equiv]');
  for (var p = 0; p < metas.length; p++) {
    var equiv = (metas[p].getAttribute('http-equiv') || '').toLowerCase();
    if (equiv === 'content-security-policy') {
      hasCsp = true;
      break;
    }
  }
  return {
    pageUrl: window.location.href,
    hasCsp: hasCsp,
    inlineScripts: inlineScripts,
    eventHandlers: eventHandlers,
    javascriptUris: javascriptUris,
  };
})()
`;
