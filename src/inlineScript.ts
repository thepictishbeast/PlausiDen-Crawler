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
  /**
   * Base64 SHA-256 of the FULL inline script body (matches the
   * CSP `'sha256-<b64>'` source-expression format). T76 cycle 54:
   * lets the detector credit hash-pinned inline scripts as
   * CSP-covered, equivalent to nonce-pinned. Optional because
   * pre-cycle-54 captures don't include it.
   */
  sha256?: string;
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
  /**
   * Raw value of the CSP `script-src` directive (header OR
   * meta http-equiv). Used by the detector to credit hash-
   * pinned inline scripts. Empty string when no script-src
   * directive is observable.
   */
  cspScriptSrc: string;
  inlineScripts: CapturedInlineScript[];
  eventHandlers: CapturedEventHandler[];
  javascriptUris: CapturedJavascriptUri[];
}

export function detectInlineScriptIssues(
  snap: InlineScriptSnapshot,
): InlineScriptFinding[] {
  const out: InlineScriptFinding[] = [];

  // T76 cycle 54: a script is "CSP-covered" if EITHER it carries
  // a nonce attribute OR its sha256 hash appears in the CSP
  // `script-src` directive. Both forms are CSP-Level-2/3
  // sanctioned ways to allow specific inline blocks; the
  // detector now recognises both.
  const scriptSrc = snap.cspScriptSrc || '';
  const isHashPinned = (s: CapturedInlineScript): boolean => {
    if (!s.sha256) return false;
    // The hash token in CSP looks like `'sha256-<b64>'`. Match
    // case-insensitively on the algorithm name (CSP is case-
    // insensitive there) and exact-match on the base64 body.
    const needle = `sha256-${s.sha256}`;
    return scriptSrc.toLowerCase().includes(needle.toLowerCase());
  };
  const uncovered = snap.inlineScripts.filter((s) => !s.hasNonce && !isHashPinned(s));
  if (uncovered.length > 0) {
    const examples = uncovered.slice(0, 5).map((s) => `<script> '${s.src}'`);
    out.push({
      severity: 'warn',
      kind: 'inline-script.present-without-nonce',
      detail: `${uncovered.length} inline <script> block(s) without a 'nonce' attribute OR a matching sha256 hash in CSP \`script-src\`. Under a strict CSP, these are silently dropped. Without a CSP, they're stored-XSS sinks: any HTML-injection vulnerability that writes a <script> tag executes immediately. Migrate to external <script src> with SRI, add a per-page nonce, OR pin the hash via \`script-src 'sha256-<b64>'\`. Examples: ${examples.join('; ')}`,
      evidence: { count: uncovered.length, examples },
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
  if (!snap.hasCsp && (uncovered.length > 0 || snap.eventHandlers.length > 0 || snap.javascriptUris.length > 0)) {
    out.push({
      severity: 'warn',
      kind: 'inline-script.no-csp-but-inline',
      detail: `Page has inline scripts/handlers AND no Content-Security-Policy header. Nothing stops a stored-XSS injection from executing. The cspPolicy detector also fires 'csp.missing' on this; the additional finding here calls out that the inline scripts make the missing CSP particularly dangerous. Add a strict CSP first (script-src 'self' 'nonce-<random>'), then migrate inline scripts to external src + nonce.`,
      evidence: {
        inlineScripts: uncovered.length,
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
/**
 * Browser-side hash helper for INLINE_SCRIPT_DOM_CAPTURE_JS.
 * Computes base64 SHA-256 of a string body via SubtleCrypto
 * (HTTPS/localhost only — file:// / non-secure contexts cannot
 * call crypto.subtle). Returns empty string on failure rather
 * than rejecting; the detector treats absent hash as
 * "not pinnable" which is the conservative outcome.
 *
 * T76 cycle 54: emitted from INLINE_SCRIPT_DOM_CAPTURE_JS so the
 * capture can mark which scripts are CSP-hash-pinned.
 */
export const INLINE_SCRIPT_DOM_CAPTURE_JS = `
(async function() {
  async function _loomHash(s) {
    try {
      if (!window.crypto || !window.crypto.subtle) return '';
      var enc = new TextEncoder();
      var buf = await window.crypto.subtle.digest('SHA-256', enc.encode(s));
      var bytes = new Uint8Array(buf);
      var bin = '';
      for (var i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
      return window.btoa(bin);
    } catch (e) {
      return '';
    }
  }
  // SUPERSOCIETY: keep the original synchronous flow for non-
  // hash work, then await hashes at the end to fill in.
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
  var inlineScriptBodies = [];
  var scripts = document.querySelectorAll('script');
  for (var i = 0; i < scripts.length; i++) {
    var s = scripts[i];
    if (s.hasAttribute('src')) continue; // external — out of scope
    var body = s.textContent || '';
    var src = body.slice(0, 200);
    inlineScripts.push({
      src: src,
      hasNonce: s.hasAttribute('nonce'),
    });
    inlineScriptBodies.push(body);
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
  var cspText = '';
  var metas = document.querySelectorAll('meta[http-equiv]');
  for (var p = 0; p < metas.length; p++) {
    var equiv = (metas[p].getAttribute('http-equiv') || '').toLowerCase();
    if (equiv === 'content-security-policy') {
      hasCsp = true;
      cspText = metas[p].getAttribute('content') || '';
      break;
    }
  }
  // T76 cycle 54: pull the script-src directive value out of
  // the CSP string. Order: directives are semicolon-separated;
  // first match wins. The detector also accepts an enriched
  // value from response headers via main.ts.
  var cspScriptSrc = '';
  if (cspText) {
    var parts = cspText.split(';');
    for (var q = 0; q < parts.length; q++) {
      var seg = parts[q].trim();
      if (seg.toLowerCase().indexOf('script-src ') === 0 ||
          seg.toLowerCase() === 'script-src') {
        cspScriptSrc = seg.slice('script-src'.length).trim();
        break;
      }
    }
  }
  // Fill in sha256 hashes for each captured inline script.
  for (var r = 0; r < inlineScripts.length; r++) {
    inlineScripts[r].sha256 = await _loomHash(inlineScriptBodies[r]);
  }
  return {
    pageUrl: window.location.href,
    hasCsp: hasCsp,
    cspScriptSrc: cspScriptSrc,
    inlineScripts: inlineScripts,
    eventHandlers: eventHandlers,
    javascriptUris: javascriptUris,
  };
})()
`;
