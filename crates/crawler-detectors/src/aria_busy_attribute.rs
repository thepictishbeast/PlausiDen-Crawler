//! `aria_busy_attribute` — `aria-busy` value + lifecycle audit.
//!
//! Sibling to the four IDREF-resolution detectors plus
//! `aria_expanded_state` and `aria_haspopup_attribute`. Audits
//! the `aria-busy` attribute used on live regions and dynamic
//! containers to tell assistive tech that the region is in the
//! middle of an update and incremental DOM changes should NOT
//! be announced piece-by-piece.
//!
//! ## The bug class
//!
//! Per ARIA 1.2, `aria-busy`:
//!
//! * Allowed values: `false` (default) or `true`.
//! * Should be set to `true` BEFORE making batched DOM
//!   changes to a live region, then reset to `false` once the
//!   batch completes. AT then announces the final state once.
//! * If left at `true` permanently, AT never announces the
//!   region — a worse failure than not setting `aria-busy` at
//!   all.
//!
//! Common authoring failures:
//!
//! 1. **Permanent `aria-busy="true"`** — operator sets it on
//!    page load and never clears. The live region silently
//!    drops every update.
//! 2. **Invalid value** — `aria-busy="loading"` / `"yes"` /
//!    `""` — AT silently ignores.
//! 3. **`aria-busy="true"` on a non-live-region host** —
//!    `aria-busy` is only meaningful on elements that are
//!    themselves live regions (`aria-live` set, or a role
//!    that implies a live region — `alert`, `log`, `status`,
//!    `progressbar`). On non-live hosts it's noise.
//!
//! ## Findings
//!
//! * `aria-busy.invalid-value` strict — value not in
//!   `{"true", "false"}`.
//! * `aria-busy.permanent-true` warn — host carries
//!   `aria-busy="true"` and the snapshot captured the page in a
//!   resting state (no DOM mutation observed between two
//!   snapshots). Only emitted when runner supplies the
//!   `was-busy-at-rest` signal.
//! * `aria-busy.on-non-live-region` warn — `aria-busy="true"`
//!   on a host with no `aria-live` attribute AND no implicit-
//!   live role.
//!
//! Out of scope:
//!
//! * `aria-live` value audit — separate future axis.
//! * Polling `aria-busy` over time to detect stuck-busy
//!   states — covered by future runtime-state detector.
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

/// Roles that imply an implicit `aria-live` region per ARIA 1.2.
const IMPLICIT_LIVE_ROLES: &[&str] =
    &["alert", "log", "status", "progressbar", "marquee", "timer"];

/// One captured element with `aria-busy` signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaBusyEntry {
    /// CSS-ish selector pointing at the host element.
    pub selector: String,
    /// `role=` attribute on the host (empty when no explicit
    /// role).
    pub role: String,
    /// Raw `aria-busy` value (trimmed).
    pub aria_busy_value: String,
    /// Whether the host carries an `aria-live` attribute
    /// (any value other than `off`).
    pub has_aria_live: bool,
    /// Optional runner-supplied flag: `Some(true)` means the
    /// runner observed the host remain `aria-busy="true"`
    /// across a stability window. `None` means the runner did
    /// not perform that check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was_busy_at_rest: Option<bool>,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaBusyAttributeSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every element on the page carrying `aria-busy`.
    pub entries: Vec<AriaBusyEntry>,
}

/// Detector.
#[must_use]
pub fn detect_aria_busy_attribute(
    snap: &AriaBusyAttributeSnapshot,
) -> Vec<AxisFinding> {
    let mut invalid: Vec<String> = Vec::new();
    let mut permanent_true: Vec<&str> = Vec::new();
    let mut on_non_live: Vec<&str> = Vec::new();

    for entry in &snap.entries {
        let raw = entry.aria_busy_value.trim();
        if raw.is_empty() {
            continue;
        }
        let lower = raw.to_ascii_lowercase();
        let is_valid = lower == "true" || lower == "false";
        if !is_valid {
            invalid.push(format!("{} (value=\"{}\")", entry.selector, raw));
            continue;
        }
        if lower != "true" {
            // false / default — nothing further to check.
            continue;
        }

        // For aria-busy="true":
        let is_live_region = entry.has_aria_live
            || IMPLICIT_LIVE_ROLES
                .iter()
                .any(|r| r.eq_ignore_ascii_case(&entry.role));
        if !is_live_region {
            on_non_live.push(entry.selector.as_str());
        }

        if let Some(true) = entry.was_busy_at_rest {
            permanent_true.push(entry.selector.as_str());
        }
    }

    let mut findings = Vec::new();
    let total = snap.entries.len();

    if !invalid.is_empty() {
        let preview =
            preview_examples(&invalid.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "aria-busy.invalid-value".to_owned(),
            detail: format!(
                "{} of {} aria-busy value(s) are not in {{\"true\", \"false\"}}. Examples: {}",
                invalid.len(),
                total,
                preview
            ),
        });
    }

    if !permanent_true.is_empty() {
        let preview = preview_examples(&permanent_true);
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-busy.permanent-true".to_owned(),
            detail: format!(
                "{} of {} aria-busy=\"true\" host(s) remained busy across the runner's stability window; live region updates are silently dropped. Examples: {}",
                permanent_true.len(),
                total,
                preview
            ),
        });
    }

    if !on_non_live.is_empty() {
        let preview = preview_examples(&on_non_live);
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-busy.on-non-live-region".to_owned(),
            detail: format!(
                "{} of {} aria-busy=\"true\" host(s) carry no aria-live attribute AND no implicit-live role; the attribute has no effect. Examples: {}",
                on_non_live.len(),
                total,
                preview
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

/// Page-side eval. Walks every element with `aria-busy`. The
/// `wasBusyAtRest` signal is left null by this evaluator —
/// the runner is expected to enrich it by polling the host
/// after a stability delay.
pub const ARIA_BUSY_ATTRIBUTE_JS: &str = r##"(() => {
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

    const hosts = Array.from(document.querySelectorAll('[aria-busy]'));
    const entries = hosts.map(function(el) {
      const live = (el.getAttribute('aria-live') || '').trim().toLowerCase();
      const hasLive = live.length > 0 && live !== 'off';
      return {
        selector: selectorOf(el),
        role: (el.getAttribute('role') || '').toLowerCase(),
        ariaBusyValue: el.getAttribute('aria-busy') || '',
        hasAriaLive: hasLive,
        wasBusyAtRest: null
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
        role: &str,
        ab: &str,
        has_live: bool,
        at_rest: Option<bool>,
    ) -> AriaBusyEntry {
        AriaBusyEntry {
            selector: sel.to_owned(),
            role: role.to_owned(),
            aria_busy_value: ab.to_owned(),
            has_aria_live: has_live,
            was_busy_at_rest: at_rest,
        }
    }

    fn snap(entries: Vec<AriaBusyEntry>) -> AriaBusyAttributeSnapshot {
        AriaBusyAttributeSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_aria_busy_attribute(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn aria_busy_false_on_anything_is_clean() {
        let f = detect_aria_busy_attribute(&snap(vec![entry(
            "div", "", "false", false, None,
        )]));
        assert!(f.is_empty(), "false is the default + benign: {f:?}");
    }

    #[test]
    fn aria_busy_true_on_live_region_is_clean() {
        let f = detect_aria_busy_attribute(&snap(vec![entry(
            "div", "", "true", true, None,
        )]));
        assert!(f.is_empty(), "live-region host is fine: {f:?}");
    }

    #[test]
    fn aria_busy_true_on_implicit_live_role_is_clean() {
        for role in ["alert", "log", "status", "progressbar", "marquee", "timer"] {
            let f = detect_aria_busy_attribute(&snap(vec![entry(
                "div", role, "true", false, None,
            )]));
            assert!(
                f.is_empty(),
                "implicit-live role {role} should pass; got {f:?}"
            );
        }
    }

    #[test]
    fn invalid_value_loading_is_strict() {
        let f = detect_aria_busy_attribute(&snap(vec![entry(
            "div", "", "loading", false, None,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-busy.invalid-value")
            .expect("invalid-value expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("loading"));
    }

    #[test]
    fn each_invalid_value_flags() {
        for v in ["loading", "yes", "no", "1", "0", "indeterminate"] {
            let f = detect_aria_busy_attribute(&snap(vec![entry(
                "div", "", v, false, None,
            )]));
            assert!(
                f.iter().any(|x| x.kind == "aria-busy.invalid-value"),
                "invalid value {v} should flag"
            );
        }
    }

    #[test]
    fn aria_busy_true_on_non_live_region_is_warn() {
        let f = detect_aria_busy_attribute(&snap(vec![entry(
            "div#stale",
            "",
            "true",
            false,
            None,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-busy.on-non-live-region")
            .expect("on-non-live expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn permanent_true_flagged_when_runner_observed_at_rest() {
        let f = detect_aria_busy_attribute(&snap(vec![entry(
            "div#stuck",
            "log",
            "true",
            false,
            Some(true),
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-busy.permanent-true")
            .expect("permanent-true expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn permanent_true_not_flagged_when_runner_did_not_observe() {
        let f = detect_aria_busy_attribute(&snap(vec![entry(
            "div", "log", "true", false, None,
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "aria-busy.permanent-true"),
            "no observation should not flag: {f:?}"
        );
    }

    #[test]
    fn invalid_value_suppresses_on_non_live_finding() {
        let f = detect_aria_busy_attribute(&snap(vec![entry(
            "div", "", "loading", false, None,
        )]));
        assert!(f.iter().any(|x| x.kind == "aria-busy.invalid-value"));
        assert!(!f.iter().any(|x| x.kind == "aria-busy.on-non-live-region"));
    }

    #[test]
    fn case_insensitive_value_match() {
        let f = detect_aria_busy_attribute(&snap(vec![entry(
            "div", "log", "TRUE", false, None,
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "aria-busy.invalid-value"),
            "TRUE should be valid case-insensitively"
        );
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| entry(&format!("div#x{i}"), "", "loading", false, None))
            .collect();
        let f = detect_aria_busy_attribute(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-busy.invalid-value")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_aria_busy_hosts() {
        assert!(ARIA_BUSY_ATTRIBUTE_JS.starts_with("(() => {"));
        assert!(ARIA_BUSY_ATTRIBUTE_JS.ends_with(")()"));
        assert!(ARIA_BUSY_ATTRIBUTE_JS.contains("[aria-busy]"));
        assert!(ARIA_BUSY_ATTRIBUTE_JS.contains("aria-live"));
    }
}
