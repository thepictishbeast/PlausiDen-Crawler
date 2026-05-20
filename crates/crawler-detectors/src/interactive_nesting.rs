//! `interactive_nesting` — flags interactive elements nested inside
//! other interactive elements.
//!
//! HTML spec forbids interactive content nested inside other
//! interactive content — `<a>` cannot contain `<a>`, `<button>`
//! cannot contain `<button>`, `<a>` cannot contain `<button>`, etc.
//! The browser's parser may silently flatten the nesting; the inner
//! element's click handler / focus / accessible name fights with
//! the outer element for events. Real-world symptom: a "Read more"
//! anchor inside a card's wrapping `<a>` link — clicking it
//! activates the OUTER link, never the inner.
//!
//! Spec reference:
//! <https://html.spec.whatwg.org/#interactive-content>
//!
//! Interactive elements (per the HTML living standard):
//! `a` (with href), `button`, `input` (except type=hidden),
//! `select`, `textarea`, `label`, `details`, `summary`, `audio`
//! (with controls), `video` (with controls), `iframe`, `embed`,
//! `object` (with usemap), `img` (with usemap).
//!
//! Out of scope:
//! * Custom elements with `role="button"` etc. — separate detector
//!   axis; this one targets the spec-defined HTML interactive set.
//! * `<a>` without href — treated as plain text per spec.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const INTERACTIVE_NESTING_JS: &str = r##"(() => {
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

    const isInteractive = function(el) {
      const tag = el.tagName.toLowerCase();
      if (tag === 'a') return el.hasAttribute('href');
      if (tag === 'button') return true;
      if (tag === 'input') {
        const type = (el.getAttribute('type') || 'text').toLowerCase();
        return type !== 'hidden';
      }
      if (tag === 'select' || tag === 'textarea' || tag === 'label'
          || tag === 'details' || tag === 'summary'
          || tag === 'iframe' || tag === 'embed') return true;
      if (tag === 'audio' || tag === 'video') return el.hasAttribute('controls');
      if (tag === 'object' || tag === 'img') return el.hasAttribute('usemap');
      return false;
    };

    let scanned = 0;
    const offenders = [];

    // Walk every interactive element. For each, check its ancestor
    // chain (up to body) for another interactive element.
    const allEls = document.querySelectorAll(
      'a[href], button, input, select, textarea, label, details, summary, audio, video, iframe, embed, object, img'
    );
    for (let i = 0; i < allEls.length; i++) {
      const el = allEls[i];
      scanned += 1;
      if (!isInteractive(el)) continue;
      // Walk ancestor chain.
      let parent = el.parentElement;
      while (parent && parent !== document.body) {
        if (isInteractive(parent)) {
          offenders.push({
            innerSelector: selectorOf(el),
            innerTag: el.tagName.toLowerCase(),
            innerText: (el.textContent || el.value || '').trim().slice(0, 40),
            outerSelector: selectorOf(parent),
            outerTag: parent.tagName.toLowerCase()
          });
          break;
        }
        parent = parent.parentElement;
      }
      if (offenders.length >= 50) break;
    }

    return {
      scanned: scanned,
      offenderCount: offenders.length,
      offenders: offenders
    };
})()"##;

/// One interactive-nesting offender row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct InteractiveNestingOffender {
    /// CSS selector for the inner (nested) interactive element.
    pub inner_selector: String,
    /// Inner tag (lowercased).
    pub inner_tag: String,
    /// First 40 chars of inner element's text/value.
    pub inner_text: String,
    /// CSS selector for the outer interactive element.
    pub outer_selector: String,
    /// Outer tag (lowercased).
    pub outer_tag: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct InteractiveNestingSnapshot {
    /// Total interactive-tag elements walked.
    pub scanned: u32,
    /// Number of nested offenders (capped at 50; count honest).
    pub offender_count: u32,
    /// Per-offender rows.
    pub offenders: Vec<InteractiveNestingOffender>,
}

/// Apply detection rules. Pure function.
///
/// Emits one strict finding per snapshot containing offenders.
/// The HTML spec is clear that interactive content cannot nest;
/// the browser's parser silently flattens the violation, which
/// means click handlers / focus / AT names of the INNER element
/// silently fight the OUTER for events. Real bug, not aesthetic.
#[must_use]
pub fn detect_interactive_nesting_issues(
    snap: &InteractiveNestingSnapshot,
) -> Vec<crate::AxisFinding> {
    if snap.offenders.is_empty() {
        return Vec::new();
    }
    let first = &snap.offenders[0];
    let mut out = Vec::with_capacity(1);
    out.push(crate::AxisFinding {
        severity: crate::AxisSeverity::Strict,
        kind: "interactive-nesting.invalid-nest".to_owned(),
        detail: format!(
            "{} interactive element(s) nested inside another interactive element. The HTML spec (`https://html.spec.whatwg.org/#interactive-content`) forbids this — the browser silently flattens the nesting; the inner element's click handler / focus / accessible name fights the outer for events. First: <{}> \"{}\" inside <{}>. Move the inner element outside the outer, or replace the outer's interactivity (e.g., use `<div role=\"link\">` style only where the spec allows).",
            snap.offender_count,
            first.inner_tag,
            first.inner_text,
            first.outer_tag,
        ),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AxisSeverity;

    #[test]
    fn js_balanced() {
        assert_eq!(
            INTERACTIVE_NESTING_JS.matches('(').count(),
            INTERACTIVE_NESTING_JS.matches(')').count()
        );
        assert_eq!(
            INTERACTIVE_NESTING_JS.matches('{').count(),
            INTERACTIVE_NESTING_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(INTERACTIVE_NESTING_JS.starts_with("(() => {"));
        assert!(INTERACTIVE_NESTING_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "scanned",
            "offenderCount",
            "offenders",
            "innerSelector",
            "innerTag",
            "innerText",
            "outerSelector",
            "outerTag",
        ] {
            assert!(INTERACTIVE_NESTING_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_recognizes_interactive_tag_set() {
        // All HTML-spec interactive elements appear in either the
        // querySelectorAll or the isInteractive dispatch.
        for tag in [
            "a[href]", "button", "input", "select", "textarea", "label", "details", "summary",
            "audio", "video", "iframe", "embed", "object", "img",
        ] {
            assert!(
                INTERACTIVE_NESTING_JS.contains(tag),
                "missing interactive tag in querySelectorAll: {tag}"
            );
        }
    }

    #[test]
    fn js_treats_a_without_href_as_inactive() {
        assert!(INTERACTIVE_NESTING_JS.contains("el.hasAttribute('href')"));
    }

    #[test]
    fn js_excludes_input_hidden() {
        assert!(INTERACTIVE_NESTING_JS.contains("type !== 'hidden'"));
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = InteractiveNestingSnapshot {
            scanned: 15,
            offender_count: 0,
            offenders: vec![],
        };
        let findings = detect_interactive_nesting_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn one_nested_offender_emits_strict() {
        let snap = InteractiveNestingSnapshot {
            scanned: 3,
            offender_count: 1,
            offenders: vec![InteractiveNestingOffender {
                inner_selector: "body > a > button".to_owned(),
                inner_tag: "button".to_owned(),
                inner_text: "Read more".to_owned(),
                outer_selector: "body > a".to_owned(),
                outer_tag: "a".to_owned(),
            }],
        };
        let findings = detect_interactive_nesting_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "interactive-nesting.invalid-nest");
        assert!(findings[0].detail.contains("html.spec.whatwg.org"));
        assert!(findings[0].detail.contains("<button>"));
        assert!(findings[0].detail.contains("<a>"));
        assert!(findings[0].detail.contains("Read more"));
    }

    #[test]
    fn anchor_inside_anchor_finding() {
        // Common LinkCard pattern: outer <a> wraps a "Read more" inner <a>.
        let snap = InteractiveNestingSnapshot {
            scanned: 4,
            offender_count: 1,
            offenders: vec![InteractiveNestingOffender {
                inner_selector: "body > article > a".to_owned(),
                inner_tag: "a".to_owned(),
                inner_text: "Read full article".to_owned(),
                outer_selector: "body > a".to_owned(),
                outer_tag: "a".to_owned(),
            }],
        };
        let findings = detect_interactive_nesting_issues(&snap);
        assert!(findings[0].detail.contains("<a>"));
    }

    #[test]
    fn label_inside_button_finding() {
        // <button><label>x</label></button> — label is interactive
        // per spec, so nested inside button is a violation.
        let snap = InteractiveNestingSnapshot {
            scanned: 2,
            offender_count: 1,
            offenders: vec![InteractiveNestingOffender {
                inner_selector: "x".to_owned(),
                inner_tag: "label".to_owned(),
                inner_text: "X".to_owned(),
                outer_selector: "y".to_owned(),
                outer_tag: "button".to_owned(),
            }],
        };
        let findings = detect_interactive_nesting_issues(&snap);
        assert!(findings[0].detail.contains("<label>"));
        assert!(findings[0].detail.contains("<button>"));
    }

    #[test]
    fn truncated_count_above_array_len_honest() {
        let snap = InteractiveNestingSnapshot {
            scanned: 200,
            offender_count: 73,
            offenders: vec![InteractiveNestingOffender {
                inner_selector: "x".to_owned(),
                inner_tag: "a".to_owned(),
                inner_text: "x".to_owned(),
                outer_selector: "y".to_owned(),
                outer_tag: "a".to_owned(),
            }],
        };
        let findings = detect_interactive_nesting_issues(&snap);
        assert!(findings[0].detail.contains("73 interactive"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = InteractiveNestingSnapshot {
            scanned: 5,
            offender_count: 1,
            offenders: vec![InteractiveNestingOffender {
                inner_selector: "x".to_owned(),
                inner_tag: "button".to_owned(),
                inner_text: "RT".to_owned(),
                outer_selector: "y".to_owned(),
                outer_tag: "a".to_owned(),
            }],
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"scanned\":5"));
        assert!(json.contains("\"innerTag\":\"button\""));
        assert!(json.contains("\"outerTag\":\"a\""));
        let back: InteractiveNestingSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.offenders.len(), 1);
        assert_eq!(back.offenders[0].inner_tag, "button");
        assert_eq!(back.offenders[0].outer_tag, "a");
    }
}
