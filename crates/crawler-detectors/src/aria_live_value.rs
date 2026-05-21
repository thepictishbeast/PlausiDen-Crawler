//! `aria_live_value` — `aria-live` value-grammar audit.
//!
//! Sibling to the four IDREF-resolution detectors plus
//! `aria_expanded_state`, `aria_haspopup_attribute`,
//! `aria_busy_attribute`, and `tab_panel_reciprocal`. Audits
//! the `aria-live` attribute used on live regions to tell AT
//! how to announce DOM changes.
//!
//! ## The contract
//!
//! Per ARIA 1.2, `aria-live`:
//!
//! * Allowed values: `"off"` (default), `"polite"`, `"assertive"`.
//! * Live region updates announce per the value:
//!   - `off` — never announce (the default; usually equivalent to
//!     no `aria-live` attribute at all)
//!   - `polite` — announce at the next idle moment
//!   - `assertive` — interrupt the current announcement
//!
//! Common authoring failures:
//!
//! 1. **Invalid value** — operators ship `aria-live="true"` /
//!    `"on"` / `"alert"` / `"live"`. AT silently ignores.
//! 2. **Assertive everywhere** — every live region declared
//!    `assertive`, drowning out the user with constant
//!    interruptions. Most updates should be `polite`.
//! 3. **`aria-live="off"` AND `aria-atomic` / `aria-relevant`
//!    set** — `off` means "ignore this region"; the
//!    accompanying attributes are dead code.
//!
//! ## Findings
//!
//! * `aria-live.invalid-value` strict — value not in
//!   `{off, polite, assertive}`.
//! * `aria-live.assertive-everywhere` warn — page has 3+ live
//!   regions AND all of them use `aria-live="assertive"`. Most
//!   should be `polite`.
//! * `aria-live.off-with-companion-attrs` warn — `aria-live=
//!   "off"` AND host carries `aria-atomic` or `aria-relevant`.
//!   The companion attributes have no effect when the region
//!   is off.
//!
//! Out of scope:
//!
//! * `aria-busy` value audit — covered by `aria_busy_attribute`.
//! * Implicit-live roles (alert/log/status/etc.) — those are
//!   their own surface; only explicitly-declared `aria-live`
//!   hosts are audited here.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; the JS const is the only side-
//!   effect channel.
//! * MAX_EXAMPLES = 5 for any per-bucket finding list.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

const MAX_EXAMPLES: usize = 5;

/// Threshold below which `assertive-everywhere` is not flagged
/// (1-2 assertive regions is plausibly intentional).
const ASSERTIVE_ALL_THRESHOLD: usize = 3;

/// One captured `aria-live` host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaLiveEntry {
    /// CSS-ish selector pointing at the host element.
    pub selector: String,
    /// Raw `aria-live` value (trimmed).
    pub aria_live_value: String,
    /// Whether the host carries `aria-atomic`.
    pub has_aria_atomic: bool,
    /// Whether the host carries `aria-relevant`.
    pub has_aria_relevant: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaLiveValueSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every element on the page carrying `aria-live`.
    pub entries: Vec<AriaLiveEntry>,
}

/// Detector.
#[must_use]
pub fn detect_aria_live_value(snap: &AriaLiveValueSnapshot) -> Vec<AxisFinding> {
    let mut invalid: Vec<String> = Vec::new();
    let mut off_with_companions: Vec<&str> = Vec::new();

    let mut assertive_count = 0;
    let mut total_valid_non_off = 0;

    for entry in &snap.entries {
        let raw = entry.aria_live_value.trim();
        if raw.is_empty() {
            continue;
        }
        let lower = raw.to_ascii_lowercase();
        let is_valid =
            matches!(lower.as_str(), "off" | "polite" | "assertive");
        if !is_valid {
            invalid.push(format!("{} (value=\"{}\")", entry.selector, raw));
            continue;
        }

        if lower == "off"
            && (entry.has_aria_atomic || entry.has_aria_relevant)
        {
            off_with_companions.push(entry.selector.as_str());
            continue;
        }

        if lower != "off" {
            total_valid_non_off += 1;
            if lower == "assertive" {
                assertive_count += 1;
            }
        }
    }

    let assertive_everywhere = total_valid_non_off >= ASSERTIVE_ALL_THRESHOLD
        && assertive_count == total_valid_non_off;

    let mut findings = Vec::new();
    let total = snap.entries.len();

    if !invalid.is_empty() {
        let preview =
            preview_examples(&invalid.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "aria-live.invalid-value".to_owned(),
            detail: format!(
                "{} of {} aria-live value(s) are not in {{off, polite, assertive}}. Examples: {}",
                invalid.len(),
                total,
                preview
            ),
        });
    }

    if !off_with_companions.is_empty() {
        let preview = preview_examples(&off_with_companions);
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-live.off-with-companion-attrs".to_owned(),
            detail: format!(
                "{} of {} aria-live=\"off\" host(s) carry aria-atomic and/or aria-relevant; the companion attributes have no effect when the region is off. Examples: {}",
                off_with_companions.len(),
                total,
                preview
            ),
        });
    }

    if assertive_everywhere {
        let preview = preview_examples(
            &snap
                .entries
                .iter()
                .filter(|e| {
                    e.aria_live_value
                        .trim()
                        .eq_ignore_ascii_case("assertive")
                })
                .map(|e| e.selector.as_str())
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-live.assertive-everywhere".to_owned(),
            detail: format!(
                "All {} non-off aria-live region(s) use \"assertive\"; users get a wall of interruptions. Most updates should be polite. Examples: {}",
                total_valid_non_off, preview
            ),
        });
    }

    findings
}

fn preview_examples(examples: &[&str]) -> String {
    let mut buf = String::new();
    let n = examples.len().min(MAX_EXAMPLES);
    for (i, sel) in examples.iter().take(n).enumerate() {
        if i > 0 {
            buf.push_str(" | ");
        }
        buf.push_str(sel);
    }
    if examples.len() > MAX_EXAMPLES {
        buf.push_str(&format!(" (+{} more)", examples.len() - MAX_EXAMPLES));
    }
    buf
}

/// Page-side eval. Walks every element with `aria-live`.
pub const ARIA_LIVE_VALUE_JS: &str = r##"(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const sameTag = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (sameTag.length > 1) {
            const idx = sameTag.indexOf(node) + 1;
            parts.unshift(tag + ':nth-of-type(' + idx + ')');
          } else { parts.unshift(tag); }
        } else { parts.unshift(tag); }
        node = parent;
        depth += 1;
      }
      return parts.join(' > ') || 'body';
    };

    const hosts = Array.from(document.querySelectorAll('[aria-live]'));
    const entries = hosts.map(function(el) {
      return {
        selector: selectorOf(el),
        ariaLiveValue: el.getAttribute('aria-live') || '',
        hasAriaAtomic: el.hasAttribute('aria-atomic'),
        hasAriaRelevant: el.hasAttribute('aria-relevant')
      };
    });

    return {
      pageUrl: location.href,
      entries: entries
    };
  })()"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        sel: &str,
        live: &str,
        atomic: bool,
        relevant: bool,
    ) -> AriaLiveEntry {
        AriaLiveEntry {
            selector: sel.to_owned(),
            aria_live_value: live.to_owned(),
            has_aria_atomic: atomic,
            has_aria_relevant: relevant,
        }
    }

    fn snap(entries: Vec<AriaLiveEntry>) -> AriaLiveValueSnapshot {
        AriaLiveValueSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_aria_live_value(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn polite_region_is_clean() {
        let f = detect_aria_live_value(&snap(vec![entry(
            "div#status", "polite", false, false,
        )]));
        assert!(f.is_empty(), "polite should pass: {f:?}");
    }

    #[test]
    fn assertive_region_alone_is_clean() {
        let f = detect_aria_live_value(&snap(vec![entry(
            "div#alert", "assertive", false, false,
        )]));
        assert!(f.is_empty(), "single assertive should pass: {f:?}");
    }

    #[test]
    fn each_invalid_value_is_strict() {
        for v in ["true", "on", "alert", "live", "1", "yes"] {
            let f = detect_aria_live_value(&snap(vec![entry(
                "div", v, false, false,
            )]));
            assert!(
                f.iter().any(|x| x.kind == "aria-live.invalid-value"),
                "value {v} should flag invalid"
            );
        }
    }

    #[test]
    fn case_insensitive_value_match() {
        let f = detect_aria_live_value(&snap(vec![entry(
            "div", "POLITE", false, false,
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "aria-live.invalid-value"),
            "POLITE should be valid"
        );
    }

    #[test]
    fn three_assertive_everywhere_is_warn() {
        let f = detect_aria_live_value(&snap(vec![
            entry("div#a", "assertive", false, false),
            entry("div#b", "assertive", false, false),
            entry("div#c", "assertive", false, false),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-live.assertive-everywhere")
            .expect("assertive-everywhere expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("All 3"));
    }

    #[test]
    fn two_assertive_below_threshold_not_flagged() {
        let f = detect_aria_live_value(&snap(vec![
            entry("div#a", "assertive", false, false),
            entry("div#b", "assertive", false, false),
        ]));
        assert!(
            !f.iter()
                .any(|x| x.kind == "aria-live.assertive-everywhere"),
            "below threshold should not flag: {f:?}"
        );
    }

    #[test]
    fn mixed_polite_and_assertive_not_flagged_as_everywhere() {
        let f = detect_aria_live_value(&snap(vec![
            entry("div#a", "polite", false, false),
            entry("div#b", "assertive", false, false),
            entry("div#c", "polite", false, false),
        ]));
        assert!(
            !f.iter()
                .any(|x| x.kind == "aria-live.assertive-everywhere"),
            "mixed should not flag: {f:?}"
        );
    }

    #[test]
    fn off_regions_excluded_from_assertive_everywhere_count() {
        // Off regions don't count toward the assertive-everywhere
        // denominator.
        let f = detect_aria_live_value(&snap(vec![
            entry("div#a", "assertive", false, false),
            entry("div#b", "assertive", false, false),
            entry("div#c", "off", false, false),
        ]));
        // Only 2 non-off + 2 assertive = 100%, but below
        // threshold (need 3).
        assert!(!f
            .iter()
            .any(|x| x.kind == "aria-live.assertive-everywhere"));
    }

    #[test]
    fn off_with_aria_atomic_is_warn() {
        let f = detect_aria_live_value(&snap(vec![entry(
            "div#dead", "off", true, false,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-live.off-with-companion-attrs")
            .expect("off-with-companions expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn off_with_aria_relevant_is_warn() {
        let f = detect_aria_live_value(&snap(vec![entry(
            "div#dead", "off", false, true,
        )]));
        assert!(f
            .iter()
            .any(|x| x.kind == "aria-live.off-with-companion-attrs"));
    }

    #[test]
    fn off_without_companions_is_clean() {
        let f = detect_aria_live_value(&snap(vec![entry(
            "div", "off", false, false,
        )]));
        assert!(f.is_empty(), "off alone should pass: {f:?}");
    }

    #[test]
    fn invalid_value_does_not_count_toward_assertive_everywhere() {
        let f = detect_aria_live_value(&snap(vec![
            entry("div#a", "assertive", false, false),
            entry("div#b", "assertive", false, false),
            entry("div#c", "alert", false, false),
        ]));
        // 2 assertive of 2 valid non-off — below threshold.
        assert!(
            !f.iter()
                .any(|x| x.kind == "aria-live.assertive-everywhere"),
            "should not count invalid against assertive-everywhere: {f:?}"
        );
        assert!(f.iter().any(|x| x.kind == "aria-live.invalid-value"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| entry(&format!("div#x{i}"), "alert", false, false))
            .collect();
        let f = detect_aria_live_value(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-live.invalid-value")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_aria_live_hosts() {
        assert!(ARIA_LIVE_VALUE_JS.starts_with("(() => {"));
        assert!(ARIA_LIVE_VALUE_JS.ends_with(")()"));
        assert!(ARIA_LIVE_VALUE_JS.contains("[aria-live]"));
        assert!(ARIA_LIVE_VALUE_JS.contains("aria-atomic"));
        assert!(ARIA_LIVE_VALUE_JS.contains("aria-relevant"));
    }
}
