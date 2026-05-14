/**
 * formLabels.ts — form-control labelling detector. T76 (TS port).
 *
 * Catches three high-impact form-UX bugs that hit real users:
 *
 *   - form.no-label                 strict
 *     A form control has no accessible name at all — no <label>,
 *     no aria-label, no aria-labelledby, no title, not even a
 *     placeholder. Screen-reader users hear "edit text" with no
 *     hint at what to type. WCAG 1.3.1 Info and Relationships
 *     (A) and 4.1.2 Name, Role, Value (A) — both A-level, both
 *     unconditionally broken.
 *
 *   - form.placeholder-only-label   warn
 *     Placeholder is the ONLY label. WCAG 3.3.2 Labels or
 *     Instructions (A): placeholders are not labels. They vanish
 *     on focus (so the user forgets what the field was), they're
 *     low-contrast by default (browser UA styling violates AA),
 *     and "placeholder X" is treated as "filled with X" by some
 *     autocomplete heuristics.
 *
 *   - form.required-no-indicator    warn
 *     Field has the `required` attribute or aria-required="true"
 *     but no visible indicator (no '*' or 'required' in the
 *     associated label). Sighted users won't know which fields
 *     they have to fill until submission fails. WCAG 3.3.2 again
 *     + the unwritten "be a decent UX" rule.
 *
 * Mirror in crates/crawler-detectors/src/form_labels.rs — keep
 * the kind+severity strings byte-equivalent.
 */
import type { Page } from 'playwright';

export interface FormLabelFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedFormControl {
  selector: string;
  /** lowercased tag name */
  tag: string;
  /** input type, '' for textarea/select */
  type: string;
  /** computed accessible name */
  accessibleName: string;
  /** how the name was derived: label-for | label-wrap | aria-label
   *  | aria-labelledby | title | placeholder | none */
  nameSource: string;
  /** placeholder attribute value, '' if absent */
  placeholder: string;
  /** required attribute or aria-required="true" */
  required: boolean;
  /** label text contains '*' or 'required' (case-insensitive) */
  requiredIndicated: boolean;
}

export interface FormLabelsSnapshot {
  pageUrl: string;
  controls: CapturedFormControl[];
}

/**
 * Input types that don't need a user-facing label — they're either
 * not user-fillable at all (hidden, submit, reset, button, image)
 * or they self-label (color picker is its own UI).
 */
const NON_LABELED_INPUT_TYPES = new Set([
  'hidden',
  'submit',
  'reset',
  'button',
  'image',
]);

export async function captureFormLabelsSnapshot(
  page: Page,
): Promise<FormLabelsSnapshot> {
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
      // Form controls in a collapsed container have 0×0; not user
      // visible right now. We skip them because we can't know if
      // they'll be revealed by the time the user lands on the form.
      if (rect.width === 0 && rect.height === 0) return false;
      return true;
    };

    /**
     * Compute accessible name + the SOURCE that produced it. The
     * source is what makes this detector richer than axe — we can
     * tell the dev "this field has only a placeholder" instead of
     * just "this field has a name (somehow)".
     *
     * Priority order matches WAI-ARIA 1.2 name computation:
     *   1. aria-labelledby (referenced element text)
     *   2. aria-label
     *   3. <label for=id> or wrapping <label>
     *   4. title
     *   5. placeholder (FALLBACK only — explicitly NOT a label
     *      per WCAG 3.3.2; we record it to surface a warn)
     */
    const nameAndSource = function(el) {
      const labelledby = el.getAttribute('aria-labelledby');
      if (labelledby) {
        const ids = labelledby.split(/\\s+/).filter(Boolean);
        const parts = [];
        for (const id of ids) {
          const ref = document.getElementById(id);
          if (ref) parts.push((ref.textContent || '').trim());
        }
        const joined = parts.join(' ').trim();
        if (joined) return { name: joined, source: 'aria-labelledby' };
      }
      const aria = el.getAttribute('aria-label');
      if (aria && aria.trim()) return { name: aria.trim(), source: 'aria-label' };
      // <label for=id>
      if (el.id) {
        const lab = document.querySelector('label[for="' + CSS.escape(el.id) + '"]');
        if (lab) {
          const t = (lab.textContent || '').trim();
          if (t) return { name: t, source: 'label-for' };
        }
      }
      // wrapping <label>
      let parent = el.parentElement;
      let hops = 0;
      while (parent && hops < 4) {
        if (parent.tagName === 'LABEL') {
          const t = (parent.textContent || '').trim();
          if (t) return { name: t, source: 'label-wrap' };
          break;
        }
        parent = parent.parentElement;
        hops += 1;
      }
      const title = el.getAttribute('title');
      if (title && title.trim()) return { name: title.trim(), source: 'title' };
      const placeholder = el.getAttribute('placeholder');
      if (placeholder && placeholder.trim()) {
        return { name: placeholder.trim(), source: 'placeholder' };
      }
      return { name: '', source: 'none' };
    };

    const out = [];
    const els = document.querySelectorAll('input,textarea,select');
    for (let i = 0; i < els.length; i++) {
      const el = els[i];
      if (!isVisible(el)) continue;
      const tag = el.tagName.toLowerCase();
      const type = (el.getAttribute('type') || '').toLowerCase();
      // Skip non-user-labeled types.
      if (tag === 'input') {
        const skip = ['hidden', 'submit', 'reset', 'button', 'image'];
        if (skip.indexOf(type) >= 0) continue;
      }
      const ns = nameAndSource(el);
      const required = el.hasAttribute('required') ||
                       el.getAttribute('aria-required') === 'true';
      // requiredIndicated: visible '*' or the word 'required' in
      // the visible label text (case-insensitive). aria-required
      // alone is not a VISIBLE indicator.
      let requiredIndicated = false;
      if (required && ns.name) {
        const lower = ns.name.toLowerCase();
        if (ns.name.indexOf('*') >= 0 || lower.indexOf('required') >= 0) {
          requiredIndicated = true;
        }
      }
      out.push({
        selector: selectorOf(el),
        tag: tag,
        type: type,
        accessibleName: ns.name.slice(0, 120),
        nameSource: ns.source,
        placeholder: (el.getAttribute('placeholder') || '').slice(0, 80),
        required: required,
        requiredIndicated: requiredIndicated,
      });
    }
    return { controls: out };
  })()`;

  const result = (await page.evaluate(evalFn)) as {
    controls: CapturedFormControl[];
  };
  return { pageUrl, controls: result.controls };
}

export function detectFormLabelIssues(
  snap: FormLabelsSnapshot,
): FormLabelFinding[] {
  const noLabel: CapturedFormControl[] = [];
  const placeholderOnly: CapturedFormControl[] = [];
  const requiredNoIndicator: CapturedFormControl[] = [];

  for (const c of snap.controls) {
    // Defensive: respect NON_LABELED_INPUT_TYPES even if the
    // page-side filter let one slip (different browsers, custom
    // elements with type=button etc).
    if (c.tag === 'input' && NON_LABELED_INPUT_TYPES.has(c.type)) {
      continue;
    }

    if (c.nameSource === 'none' || c.accessibleName === '') {
      noLabel.push(c);
      continue;
    }
    if (c.nameSource === 'placeholder') {
      placeholderOnly.push(c);
    }
    if (c.required && !c.requiredIndicated) {
      requiredNoIndicator.push(c);
    }
  }

  const out: FormLabelFinding[] = [];

  if (noLabel.length > 0) {
    const examples = noLabel.slice(0, 5).map((c) => {
      const t = c.tag === 'input' ? `${c.tag}[type=${c.type || 'text'}]` : c.tag;
      return `${c.selector} ${t}`;
    });
    out.push({
      severity: 'strict',
      kind: 'form.no-label',
      detail: `${noLabel.length} form control(s) have no accessible name (no <label>, aria-label, aria-labelledby, title, or placeholder). WCAG 1.3.1 + 4.1.2 (both A) — screen-reader users hear 'edit text' with no hint. Examples: ${examples.join('; ')}`,
      evidence: { count: noLabel.length, examples },
    });
  }

  if (placeholderOnly.length > 0) {
    const examples = placeholderOnly.slice(0, 5).map((c) => {
      const t = c.tag === 'input' ? `${c.tag}[type=${c.type || 'text'}]` : c.tag;
      return `${c.selector} ${t} (placeholder='${c.placeholder}')`;
    });
    out.push({
      severity: 'warn',
      kind: 'form.placeholder-only-label',
      detail: `${placeholderOnly.length} form control(s) use a placeholder as the ONLY label. WCAG 3.3.2: placeholders are not labels — they vanish on focus, default to low contrast, and confuse autofill. Add a <label> or aria-label. Examples: ${examples.join('; ')}`,
      evidence: { count: placeholderOnly.length, examples },
    });
  }

  if (requiredNoIndicator.length > 0) {
    const examples = requiredNoIndicator.slice(0, 5).map((c) => {
      const t = c.tag === 'input' ? `${c.tag}[type=${c.type || 'text'}]` : c.tag;
      return `${c.selector} ${t} (label='${c.accessibleName}')`;
    });
    out.push({
      severity: 'warn',
      kind: 'form.required-no-indicator',
      detail: `${requiredNoIndicator.length} required field(s) have no visible required indicator (no '*' or 'required' in the label text). Sighted users discover the requirement only on submission failure. WCAG 3.3.2 + UX best practice. Examples: ${examples.join('; ')}`,
      evidence: { count: requiredNoIndicator.length, examples },
    });
  }

  return out;
}

export async function checkFormLabels(
  page: Page,
): Promise<{ snapshot: FormLabelsSnapshot; findings: FormLabelFinding[] }> {
  const snapshot = await captureFormLabelsSnapshot(page);
  const findings = detectFormLabelIssues(snapshot);
  return { snapshot, findings };
}
