//! `tabindex_positive` — flags positive tabindex usage.
//!
//! Positive tabindex (`tabindex="1"`, `tabindex="2"`, etc.) is
//! universally regarded as an accessibility antipattern. It overrides
//! the natural DOM-order tab traversal with a manual sequence the
//! operator has to maintain by hand; the sequence breaks the moment
//! anyone reorders the markup, inserts new content, or adds a
//! component to the page.
//!
//! Distinct from:
//! * `tab_order` — checks whether the traversal order matches the
//!   visual reading order. THIS check flags positive tabindex
//!   unconditionally, regardless of whether the resulting order is
//!   currently consistent with reading order.
//! * `runtime_focus` — checks focus indicator visibility.
//! * `offscreen_focusable` — checks focusable elements pulled off-screen.
//!
//! Valid tabindex values per the HTML living standard:
//! * `tabindex="0"` — element is in the natural tab order.
//! * `tabindex="-1"` — element is focusable programmatically but not
//!   via Tab. Common for managed-focus widgets.
//! * `tabindex=""` (empty) → equivalent to omitting the attribute.
//! * `tabindex="1+"` → ANTIPATTERN, this detector's target.
//!
//! Authoritative references:
//! * <https://html.spec.whatwg.org/multipage/interaction.html#the-tabindex-attribute>
//! * WAI-ARIA Authoring Practices Guide explicitly discourages
//!   positive tabindex for the same brittleness reasons.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const TABINDEX_POSITIVE_JS: &str = r##"(() => {
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

    let scanned = 0;
    const offenders = [];
    let maxValue = 0;

    const tabbed = document.querySelectorAll('[tabindex]');
    for (let i = 0; i < tabbed.length; i++) {
      const el = tabbed[i];
      scanned += 1;
      const raw = el.getAttribute('tabindex');
      if (raw === null) continue;
      const trimmed = raw.trim();
      if (trimmed === '') continue;
      const n = parseInt(trimmed, 10);
      if (isNaN(n)) continue;
      if (n > 0) {
        if (n > maxValue) maxValue = n;
        offenders.push({
          selector: selectorOf(el),
          tag: el.tagName.toLowerCase(),
          value: n,
          rawValue: raw,
          text: (el.textContent || '').trim().slice(0, 40)
        });
        if (offenders.length >= 100) break;
      }
    }

    return {
      scanned: scanned,
      offenderCount: offenders.length,
      maxValue: maxValue,
      offenders: offenders.slice(0, 50)
    };
})()"##;

/// One positive-tabindex offender row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct PositiveTabindexOffender {
    /// CSS selector for the element.
    pub selector: String,
    /// Element tag (lowercased).
    pub tag: String,
    /// Parsed numeric tabindex value.
    pub value: i32,
    /// Raw attribute string (in case of formatting quirks).
    pub raw_value: String,
    /// First 40 chars of element text (operator-recognisable fingerprint).
    pub text: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct TabindexPositiveSnapshot {
    /// Total `[tabindex]` elements walked.
    pub scanned: u32,
    /// Offender count (may exceed `offenders.len()` if truncated).
    pub offender_count: u32,
    /// Largest positive value found. Useful to see scope of the
    /// "manual sequence" the operator was maintaining.
    pub max_value: i32,
    /// Top 50 offender rows.
    pub offenders: Vec<PositiveTabindexOffender>,
}

/// Apply detection rules. Pure function.
///
/// Emits one warn finding per snapshot that contained any offender.
/// Warn (not strict) because positive tabindex is an antipattern but
/// not a strict spec violation — the HTML living standard explicitly
/// allows it. The brittleness + maintenance cost is real but not a
/// gate-blocking failure.
#[must_use]
pub fn detect_tabindex_positive_issues(
    snap: &TabindexPositiveSnapshot,
) -> Vec<crate::AxisFinding> {
    if snap.offenders.is_empty() {
        return Vec::new();
    }
    let first = &snap.offenders[0];
    let mut out = Vec::with_capacity(1);
    out.push(crate::AxisFinding {
        severity: crate::AxisSeverity::Warn,
        kind: "tabindex-positive.antipattern".to_owned(),
        detail: format!(
            "{} element(s) use positive tabindex (max value {}). Positive tabindex overrides DOM-order tab traversal with a manual sequence that breaks the moment markup gets reordered or new content lands. WAI-ARIA APG + the HTML living standard both discourage it. First: <{} tabindex=\"{}\"> \"{}\" @ {}. Replace with `tabindex=\"0\"` (natural order) or restructure the DOM so the desired focus order matches reading order.",
            snap.offender_count,
            snap.max_value,
            first.tag,
            first.value,
            first.text,
            first.selector,
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
            TABINDEX_POSITIVE_JS.matches('(').count(),
            TABINDEX_POSITIVE_JS.matches(')').count()
        );
        assert_eq!(
            TABINDEX_POSITIVE_JS.matches('{').count(),
            TABINDEX_POSITIVE_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(TABINDEX_POSITIVE_JS.starts_with("(() => {"));
        assert!(TABINDEX_POSITIVE_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "scanned", "offenderCount", "maxValue", "offenders",
            "selector", "tag", "value", "rawValue", "text",
        ] {
            assert!(TABINDEX_POSITIVE_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_only_flags_positive_values() {
        // `n > 0` gate is the central decision.
        assert!(TABINDEX_POSITIVE_JS.contains("if (n > 0)"));
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = TabindexPositiveSnapshot {
            scanned: 12,
            offender_count: 0,
            max_value: 0,
            offenders: vec![],
        };
        let findings = detect_tabindex_positive_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn one_offender_emits_warn() {
        let snap = TabindexPositiveSnapshot {
            scanned: 4,
            offender_count: 1,
            max_value: 5,
            offenders: vec![PositiveTabindexOffender {
                selector: "body > main > input".to_owned(),
                tag: "input".to_owned(),
                value: 5,
                raw_value: "5".to_owned(),
                text: "Email".to_owned(),
            }],
        };
        let findings = detect_tabindex_positive_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Warn));
        assert_eq!(findings[0].kind, "tabindex-positive.antipattern");
        assert!(findings[0].detail.contains("max value 5"));
        assert!(findings[0].detail.contains(r#"tabindex="5""#));
        assert!(findings[0].detail.contains("HTML living standard"));
        assert!(findings[0].detail.contains(r#"tabindex="0""#));
    }

    #[test]
    fn max_value_surfaces_across_multiple_offenders() {
        let snap = TabindexPositiveSnapshot {
            scanned: 6,
            offender_count: 3,
            max_value: 99,
            offenders: vec![
                PositiveTabindexOffender {
                    selector: "a".to_owned(),
                    tag: "input".to_owned(),
                    value: 1,
                    raw_value: "1".to_owned(),
                    text: "a".to_owned(),
                },
                PositiveTabindexOffender {
                    selector: "b".to_owned(),
                    tag: "button".to_owned(),
                    value: 50,
                    raw_value: "50".to_owned(),
                    text: "b".to_owned(),
                },
                PositiveTabindexOffender {
                    selector: "c".to_owned(),
                    tag: "a".to_owned(),
                    value: 99,
                    raw_value: "99".to_owned(),
                    text: "c".to_owned(),
                },
            ],
        };
        let findings = detect_tabindex_positive_issues(&snap);
        assert!(findings[0].detail.contains("3 element"));
        assert!(findings[0].detail.contains("max value 99"));
    }

    #[test]
    fn truncated_count_above_array_len_honest() {
        let snap = TabindexPositiveSnapshot {
            scanned: 200,
            offender_count: 73,
            max_value: 12,
            offenders: vec![PositiveTabindexOffender {
                selector: "x".to_owned(),
                tag: "div".to_owned(),
                value: 12,
                raw_value: "12".to_owned(),
                text: "x".to_owned(),
            }],
        };
        let findings = detect_tabindex_positive_issues(&snap);
        assert!(findings[0].detail.contains("73 element"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = TabindexPositiveSnapshot {
            scanned: 5,
            offender_count: 1,
            max_value: 3,
            offenders: vec![PositiveTabindexOffender {
                selector: "body > x".to_owned(),
                tag: "select".to_owned(),
                value: 3,
                raw_value: "3".to_owned(),
                text: "Choose".to_owned(),
            }],
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"scanned\":5"));
        assert!(json.contains("\"maxValue\":3"));
        assert!(json.contains("\"rawValue\":\"3\""));
        let back: TabindexPositiveSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.max_value, 3);
        assert_eq!(back.offenders.len(), 1);
        assert_eq!(back.offenders[0].value, 3);
    }
}
