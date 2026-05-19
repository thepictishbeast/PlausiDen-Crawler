//! `status_messages` — flag dynamic-content widgets that aren't
//! announced via `aria-live` to assistive tech.
//!
//! WCAG 2.1 SC 4.1.3 (Status Messages, Level AA). A status message
//! that's added to the DOM at runtime — toast, alert, search result
//! count, form-validation summary, signin-success banner, Crucible-
//! challenge result, etc. — must be announceable without moving
//! focus to it. The mechanism is `aria-live` (or an equivalent
//! `role="status"` / `role="alert"` which imply aria-live).
//!
//! ## Heuristic
//!
//! Snapshot scans for known dynamic-content widget signatures in
//! the rendered DOM:
//!
//! * `[role="status"]` / `[role="alert"]` (acceptable — they
//!   imply aria-live).
//! * `[aria-live]` (acceptable — explicit).
//! * Class-keyed widgets that are typically dynamic but often miss
//!   the announcement attr:
//!     - `.loom-toast`, `.loom-alert`, `.loom-flash`, `.loom-banner`
//!     - `.loom-form-error`, `.loom-form-success`
//!     - `.loom-search-results`, `.loom-results-count`
//!     - `.loom-crucible-result`, `.loom-auth-success`
//!     - `.loom-snackbar`, `.loom-notification`
//!
//! Anything matching the known-dynamic class set BUT carrying
//! neither `aria-live` nor `role` ∈ {status, alert, log, marquee,
//! timer} is flagged.
//!
//! ## Severity
//!
//! Strict — 4.1.3 is Level AA.
//!
//! Caller opt-out: `data-loom-aria-live-exempt="true"` on the
//! element. Use for intentionally non-announced affordances
//! (e.g. a decorative banner that re-renders on hover).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offender — a dynamic-content widget without an
/// announcement attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct StatusMessageHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Which known-dynamic class matched (e.g. "loom-toast").
    pub matched_class: String,
    /// First 60 chars of the element's text (for human review).
    pub text_preview: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct StatusMessagesSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every dynamic-content widget that fired the heuristic.
    pub hits: Vec<StatusMessageHit>,
    /// Total known-dynamic elements walked — noise-floor signal.
    pub scanned_widgets: u32,
}

/// Pure detector: snapshot → findings. Aggregates all hits into
/// one finding; examples capped at 5.
#[must_use]
pub fn detect_status_messages(snap: &StatusMessagesSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let examples: Vec<String> = snap
        .hits
        .iter()
        .take(5)
        .map(|h| {
            format!(
                "{} (.{}, text: \"{}\")",
                h.selector, h.matched_class, h.text_preview
            )
        })
        .collect();
    vec![AxisFinding {
        severity: AxisSeverity::Strict,
        kind: "status-message.missing-aria-live".to_owned(),
        detail: format!(
            "WCAG 4.1.3 — {} dynamic-content widget(s) lack an `aria-live` attribute and role is not status/alert/log. Add `aria-live=\"polite\"` (or `role=\"status\"` / `role=\"alert\"`) so assistive tech announces the change without moving focus. Examples: {}",
            snap.hits.len(),
            examples.join("; ")
        ),
    }]
}

/// Browser-side DOM-capture script. Same structural shape as
/// link_color_only / placeholder_text scripts so the runner
/// plug-in pattern stays uniform.
///
/// REGRESSION-GUARD: `dynamicClasses` list MUST stay in sync with
/// the Rust-side module doc + Loom primitive naming. Adding a new
/// Loom dynamic widget (e.g. `.loom-paywall-banner`) requires
/// adding the class here AND updating the module doc.
pub const STATUS_MESSAGES_DOM_CAPTURE_JS: &str = r#"
(() => {
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

    const dynamicClasses = [
      'loom-toast',
      'loom-alert',
      'loom-flash',
      'loom-banner',
      'loom-form-error',
      'loom-form-success',
      'loom-search-results',
      'loom-results-count',
      'loom-crucible-result',
      'loom-auth-success',
      'loom-snackbar',
      'loom-notification',
      'loom-announcement-bar',
    ];

    const announcedRoles = new Set(['status', 'alert', 'log', 'marquee', 'timer']);

    const hits = [];
    let scanned = 0;

    const selector = dynamicClasses.map(function(c) { return '.' + c; }).join(', ');
    const els = document.querySelectorAll(selector);
    for (let i = 0; i < els.length; i++) {
      const el = els[i];
      scanned += 1;
      // Caller opt-out.
      if (el.getAttribute('data-loom-aria-live-exempt') === 'true') continue;
      // Implicit announcement via role.
      const role = (el.getAttribute('role') || '').toLowerCase();
      if (announcedRoles.has(role)) continue;
      // Explicit aria-live.
      const live = el.getAttribute('aria-live');
      if (live && (live === 'polite' || live === 'assertive')) continue;
      // Find which class matched (first hit).
      let matched = '';
      for (let c = 0; c < dynamicClasses.length; c++) {
        if (el.classList.contains(dynamicClasses[c])) {
          matched = dynamicClasses[c];
          break;
        }
      }
      const text = (el.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 60);
      hits.push({
        selector: selectorOf(el),
        matchedClass: matched,
        textPreview: text,
      });
      if (hits.length >= 50) break;
    }
    return {
      pageUrl: window.location.href,
      hits: hits,
      scannedWidgets: scanned,
    };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(hits: Vec<StatusMessageHit>) -> StatusMessagesSnapshot {
        StatusMessagesSnapshot {
            page_url: "https://dev.plausiden.com/".to_owned(),
            hits,
            scanned_widgets: 10,
        }
    }

    fn hit(selector: &str, matched_class: &str, text: &str) -> StatusMessageHit {
        StatusMessageHit {
            selector: selector.to_owned(),
            matched_class: matched_class.to_owned(),
            text_preview: text.to_owned(),
        }
    }

    #[test]
    fn empty_snapshot_no_findings() {
        let s = snap(Vec::new());
        assert!(detect_status_messages(&s).is_empty());
    }

    #[test]
    fn one_hit_emits_strict_finding() {
        let s = snap(vec![hit(
            "body > main > div",
            "loom-form-success",
            "Form submitted",
        )]);
        let f = detect_status_messages(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert_eq!(f[0].kind, "status-message.missing-aria-live");
        assert!(f[0].detail.contains("WCAG 4.1.3"));
    }

    #[test]
    fn multiple_hits_aggregate() {
        let s = snap(vec![
            hit("body > div:nth-of-type(1)", "loom-toast", "Saved"),
            hit("body > div:nth-of-type(2)", "loom-alert", "Error"),
            hit("body > footer > div", "loom-announcement-bar", "Sale"),
        ]);
        let f = detect_status_messages(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("3 dynamic-content widget(s)"));
    }

    #[test]
    fn examples_capped_at_5() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!("body > div:nth-of-type({})", i + 1),
                "loom-toast",
                "T",
            ));
        }
        let s = snap(hits);
        let f = detect_status_messages(&s);
        assert!(f[0].detail.contains("10 dynamic"));
        let arrows = f[0].detail.matches(" (.").count();
        assert_eq!(arrows, 5);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap(vec![hit("body > div", "loom-toast", "Hello")]);
        let j = serde_json::to_string(&s).expect("ser");
        let back: StatusMessagesSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.hits.len(), 1);
        assert_eq!(back.hits[0].matched_class, "loom-toast");
    }

    #[test]
    fn js_brackets_balanced() {
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        let mut bracket: i32 = 0;
        for c in STATUS_MESSAGES_DOM_CAPTURE_JS.chars() {
            match c {
                '(' => paren += 1,
                ')' => paren -= 1,
                '{' => brace += 1,
                '}' => brace -= 1,
                '[' => bracket += 1,
                ']' => bracket -= 1,
                _ => {}
            }
        }
        assert_eq!(paren, 0, "unbalanced parens in capture JS");
        assert_eq!(brace, 0, "unbalanced braces in capture JS");
        assert_eq!(bracket, 0, "unbalanced brackets in capture JS");
    }

    #[test]
    fn js_includes_opt_out_marker() {
        assert!(
            STATUS_MESSAGES_DOM_CAPTURE_JS.contains("data-loom-aria-live-exempt"),
            "capture JS missing the caller-side opt-out marker"
        );
    }

    #[test]
    fn js_includes_announced_roles() {
        for role in ["status", "alert", "log"] {
            assert!(
                STATUS_MESSAGES_DOM_CAPTURE_JS.contains(&format!("'{role}'")),
                "capture JS missing announced-role literal: {role}"
            );
        }
    }
}
