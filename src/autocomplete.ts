/**
 * autocomplete.ts — form autocomplete-attribute hint detector. T76.
 *
 * Per WCAG 2.1 SC 1.3.5 (Identify Input Purpose, Level AA), inputs
 * collecting user info MUST have programmatically-determinable
 * purpose. The mechanism is the HTML `autocomplete` attribute with
 * one of the WHATWG-defined tokens (`name`, `email`, `tel`,
 * `street-address`, etc.). This:
 *
 *   1. Lets browsers offer accurate autofill (faster checkout,
 *      faster login, faster signup → conversion ↑).
 *   2. Lets assistive tech (e.g. password managers, accessibility
 *      tools) know what the field is for and offer help.
 *   3. Reduces user error: users won't accidentally type their
 *      address into the email field if browser-autofill recognized
 *      both correctly.
 *
 * Findings:
 *
 *   - autocomplete.missing-credentials   strict
 *     A login or registration password / email field has NO
 *     autocomplete attribute. Catastrophic — password managers
 *     can't reliably save or fill, security AND UX harm. WCAG
 *     1.3.5 AA + WHATWG Autofill spec.
 *
 *   - autocomplete.missing-pii           warn
 *     A field whose name/id/label suggests PII (name, email, phone,
 *     address, postal code, country) lacks an autocomplete attribute.
 *     Hurts conversion but doesn't break security.
 *
 *   - autocomplete.invalid-token         warn
 *     The autocomplete value isn't `on`, `off`, or one of the
 *     WHATWG tokens. Browser ignores it; functionally same as
 *     missing.
 *
 * Mirror: crates/crawler-detectors/src/autocomplete.rs.
 */
import type { Page } from 'playwright';

export interface AutocompleteFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedAutocompleteField {
  selector: string;
  /** lowercased input type, '' for textarea/select */
  type: string;
  /** HTML name attribute, lowercased */
  name: string;
  /** id attribute (case-preserved) */
  id: string;
  /** Lowercased autocomplete attribute value, '' if absent */
  autocomplete: string;
  /** True iff the autocomplete attribute is present on the element. */
  hasAutocomplete: boolean;
  /** Computed accessible name (label text), first 60 chars */
  accessibleName: string;
}

export interface AutocompleteSnapshot {
  pageUrl: string;
  fields: CapturedAutocompleteField[];
}

/**
 * WHATWG autofill tokens (HTML Living Standard §field.autofill).
 * Truncated to the most common ~50 — we accept anything in this set
 * as "valid". The full grammar allows section-* prefixes and
 * shipping/billing modifiers; we don't try to validate those.
 */
const VALID_AUTOCOMPLETE_TOKENS = new Set<string>([
  'on', 'off',
  'name', 'honorific-prefix', 'given-name', 'additional-name', 'family-name', 'honorific-suffix', 'nickname',
  'email', 'username',
  'new-password', 'current-password', 'one-time-code',
  'organization-title', 'organization',
  'street-address',
  'address-line1', 'address-line2', 'address-line3',
  'address-level1', 'address-level2', 'address-level3', 'address-level4',
  'country', 'country-name', 'postal-code',
  'cc-name', 'cc-given-name', 'cc-additional-name', 'cc-family-name',
  'cc-number', 'cc-exp', 'cc-exp-month', 'cc-exp-year', 'cc-csc', 'cc-type',
  'transaction-currency', 'transaction-amount',
  'language',
  'bday', 'bday-day', 'bday-month', 'bday-year',
  'sex', 'tel', 'tel-country-code', 'tel-national', 'tel-area-code', 'tel-local',
  'tel-extension', 'impp', 'url', 'photo',
  'webauthn',
]);

export async function captureAutocompleteSnapshot(
  page: Page,
): Promise<AutocompleteSnapshot> {
  const pageUrl = page.url();
  const evalFn = `(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const same = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (same.length > 1) parts.unshift(tag + ':nth-of-type(' + (same.indexOf(node) + 1) + ')');
          else parts.unshift(tag);
        } else parts.unshift(tag);
        node = parent;
        depth += 1;
      }
      return 'body > ' + parts.join(' > ');
    };

    const isVisible = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) return false;
      return true;
    };

    const accessibleName = function(el) {
      if (el.id) {
        const lab = document.querySelector('label[for="' + CSS.escape(el.id) + '"]');
        if (lab) return (lab.textContent || '').trim().slice(0, 60);
      }
      let parent = el.parentElement;
      let hops = 0;
      while (parent && hops < 4) {
        if (parent.tagName === 'LABEL') return (parent.textContent || '').trim().slice(0, 60);
        parent = parent.parentElement;
        hops += 1;
      }
      const aria = el.getAttribute('aria-label');
      if (aria && aria.trim()) return aria.trim().slice(0, 60);
      return '';
    };

    const out = [];
    const els = document.querySelectorAll('input,textarea,select');
    for (let i = 0; i < els.length; i++) {
      const el = els[i];
      if (!isVisible(el)) continue;
      const tag = el.tagName.toLowerCase();
      const type = (el.getAttribute('type') || '').toLowerCase();
      // Skip non-data-collecting input types.
      if (tag === 'input') {
        const skip = ['hidden', 'submit', 'reset', 'button', 'image', 'checkbox', 'radio', 'file', 'color', 'range'];
        if (skip.indexOf(type) >= 0) continue;
      }
      const hasAutocomplete = el.hasAttribute('autocomplete');
      const ac = (el.getAttribute('autocomplete') || '').trim().toLowerCase();
      out.push({
        selector: selectorOf(el),
        type: type,
        name: (el.getAttribute('name') || '').toLowerCase(),
        id: el.getAttribute('id') || '',
        autocomplete: ac,
        hasAutocomplete: hasAutocomplete,
        accessibleName: accessibleName(el),
      });
    }
    return { fields: out };
  })()`;

  const result = (await page.evaluate(evalFn)) as { fields: CapturedAutocompleteField[] };
  return { pageUrl, fields: result.fields };
}

/**
 * Patterns that mark a field as collecting credentials. If any
 * matches, missing autocomplete is STRICT (security-relevant).
 */
const CREDENTIAL_PATTERNS = [
  /password/,
  /^pass$/,
  /^pw$/,
  /\busername\b/,
  /\bemail\b/,
  /\bsignin\b/,
  /\bsign-in\b/,
  /\blogin\b/,
];

/**
 * Patterns that mark a field as collecting general PII. If any
 * matches and credential patterns don't, missing autocomplete is
 * WARN.
 */
const PII_PATTERNS = [
  /\bname\b/,
  /first.?name/,
  /last.?name/,
  /given.?name/,
  /family.?name/,
  /\bphone\b/,
  /\btel\b/,
  /\baddress\b/,
  /\bcity\b/,
  /\bstate\b/,
  /\bzip\b/,
  /\bpostal\b/,
  /\bpostcode\b/,
  /\bcountry\b/,
  /\bbirth/,
  /\bdob\b/,
  /\bcc\b/,
  /credit.?card/,
];

function classifyField(field: CapturedAutocompleteField): 'credential' | 'pii' | 'other' {
  const haystack = [field.name, field.id.toLowerCase(), field.accessibleName.toLowerCase(), field.type].join(' ');
  for (const p of CREDENTIAL_PATTERNS) {
    if (p.test(haystack)) return 'credential';
  }
  // Type=email is always credential-class regardless of name.
  if (field.type === 'email' || field.type === 'password') return 'credential';
  if (field.type === 'tel') return 'pii';
  for (const p of PII_PATTERNS) {
    if (p.test(haystack)) return 'pii';
  }
  return 'other';
}

export function detectAutocompleteIssues(
  snap: AutocompleteSnapshot,
): AutocompleteFinding[] {
  const missingCred: CapturedAutocompleteField[] = [];
  const missingPii: CapturedAutocompleteField[] = [];
  const invalidToken: CapturedAutocompleteField[] = [];

  for (const f of snap.fields) {
    if (f.hasAutocomplete) {
      // If present, check if it's a known token (or section-* prefix
      // pattern). `off` and `on` count as valid.
      const ac = f.autocomplete;
      // Strip optional shipping/billing prefix.
      const tokens = ac.split(/\s+/).filter(Boolean);
      // Multi-token autocomplete like "shipping street-address" — accept
      // if ALL tokens are recognized OR start with `section-`.
      const allValid = tokens.length > 0 && tokens.every((t) => {
        if (VALID_AUTOCOMPLETE_TOKENS.has(t)) return true;
        if (t.startsWith('section-')) return true;
        if (t === 'shipping' || t === 'billing' || t === 'home' || t === 'work' || t === 'mobile' || t === 'fax' || t === 'pager') return true;
        return false;
      });
      if (!allValid) {
        invalidToken.push(f);
      }
      continue;
    }
    const cls = classifyField(f);
    if (cls === 'credential') missingCred.push(f);
    else if (cls === 'pii') missingPii.push(f);
  }

  const out: AutocompleteFinding[] = [];

  if (missingCred.length > 0) {
    const examples = missingCred.slice(0, 5).map((f) => {
      const t = f.type ? `[type=${f.type}]` : '';
      return `${f.selector} ${f.name || f.id || '?'}${t} (label='${f.accessibleName}')`;
    });
    out.push({
      severity: 'strict',
      kind: 'autocomplete.missing-credentials',
      detail: `${missingCred.length} credential field(s) (login/email/password) have no autocomplete attribute. Password managers can't reliably save or autofill — security AND UX harm. WCAG 1.3.5 (Identify Input Purpose, AA). Add autocomplete="username" / "email" / "current-password" / "new-password" as appropriate. Examples: ${examples.join('; ')}`,
      evidence: { count: missingCred.length, examples },
    });
  }

  if (missingPii.length > 0) {
    const examples = missingPii.slice(0, 5).map((f) => {
      const t = f.type ? `[type=${f.type}]` : '';
      return `${f.selector} ${f.name || f.id || '?'}${t} (label='${f.accessibleName}')`;
    });
    out.push({
      severity: 'warn',
      kind: 'autocomplete.missing-pii',
      detail: `${missingPii.length} PII field(s) (name/phone/address/etc.) have no autocomplete attribute. Browsers can't autofill — slower checkout / higher form abandonment. Add the appropriate WHATWG token. Examples: ${examples.join('; ')}`,
      evidence: { count: missingPii.length, examples },
    });
  }

  if (invalidToken.length > 0) {
    const examples = invalidToken.slice(0, 5).map((f) => {
      return `${f.selector} (autocomplete='${f.autocomplete}', label='${f.accessibleName}')`;
    });
    out.push({
      severity: 'warn',
      kind: 'autocomplete.invalid-token',
      detail: `${invalidToken.length} field(s) have autocomplete values that aren't recognized WHATWG tokens. Browsers will ignore and fall back to default behaviour. Examples: ${examples.join('; ')}`,
      evidence: { count: invalidToken.length, examples },
    });
  }

  return out;
}

export async function checkAutocomplete(
  page: Page,
): Promise<{
  snapshot: AutocompleteSnapshot;
  findings: AutocompleteFinding[];
}> {
  const snapshot = await captureAutocompleteSnapshot(page);
  const findings = detectAutocompleteIssues(snapshot);
  return { snapshot, findings };
}
