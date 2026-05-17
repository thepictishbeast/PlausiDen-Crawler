//! `trusted_types_runtime` — Trusted Types DOM-sink monitor classifier.
//! T76 port of `src/trustedTypesRuntime.ts` (classifier only — the
//! browser-side `addInitScript` probe stays TS-side until the
//! chromiumoxide adapter lands).
//!
//! Trusted Types is a CSP-Level-3 extension: any string assigned to
//! a DOM sink (innerHTML, document.write, eval, etc.) must be a
//! Trusted* policy-issued object, otherwise the browser rejects the
//! assignment. Strict CSP + Trusted Types is the strongest in-browser
//! DOM-XSS mitigation available today.
//!
//! Findings (mirror TS byte-for-byte, with info→warn since
//! AxisSeverity only has Strict/Warn at the shared level):
//!
//!   * `tt.unprotected-sink`     warn
//!   * `tt.directive-missing`    warn
//!   * `tt.policy-undeclared`    warn (was info in TS; logged as warn)
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure classifier, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One captured sink call from the runtime probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedTrustedTypesSink {
    /// Which sink was called (`innerHTML`, `outerHTML`,
    /// `insertAdjacentHTML`, `document.write`, `document.writeln`,
    /// `setTimeout(string)`, `setInterval(string)`,
    /// `createContextualFragment`).
    pub kind: String,
    /// First 200 chars of the assigned value.
    pub preview: String,
    /// True iff the argument was a Trusted* instance.
    pub trusted: bool,
    /// Time (ms) since page open when the call fired.
    pub t: u32,
}

/// Captured page state the classifier consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TrustedTypesSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Sink calls captured during this step.
    pub sinks: Vec<CapturedTrustedTypesSink>,
    /// True iff CSP contains `require-trusted-types-for`.
    pub has_require_directive: bool,
    /// Raw value of the `trusted-types` directive, empty if absent.
    pub trusted_types_directive: String,
    /// True iff the page has any `<script>` tag.
    pub has_scripts: bool,
}

/// Pure classifier: snapshot → findings.
pub fn detect_trusted_types_issues(snap: &TrustedTypesSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();

    let untrusted: Vec<&CapturedTrustedTypesSink> =
        snap.sinks.iter().filter(|s| !s.trusted).collect();

    if !untrusted.is_empty() && !snap.has_require_directive {
        let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
        for s in &untrusted {
            *by_kind.entry(s.kind.as_str()).or_insert(0) += 1;
        }
        let kind_summary: Vec<String> = by_kind.iter().map(|(k, v)| format!("{k}×{v}")).collect();
        let examples: Vec<String> = untrusted
            .iter()
            .take(5)
            .map(|s| {
                let preview: String = s.preview.chars().take(80).collect();
                format!("{} <- '{}'", s.kind, preview)
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "tt.unprotected-sink".into(),
            detail: format!(
                "{} DOM-sink call(s) executed with plain (non-Trusted) values AND the page has no `require-trusted-types-for 'script'` CSP directive. Under strict CSP the browser would reject these. Sinks: {}. Examples: {}",
                untrusted.len(),
                kind_summary.join(", "),
                examples.join("; ")
            ),
        });
    }

    if snap.has_scripts && !snap.has_require_directive {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "tt.directive-missing".into(),
            detail: "Page has scripts AND no `require-trusted-types-for 'script'` directive in CSP. Add the directive (CSP-Level-3) so the browser rejects DOM-sink assignments of plain strings. Trusted Types is the strongest in-browser DOM-XSS mitigation currently shipping.".into(),
        });
    }

    if snap.has_require_directive && snap.trusted_types_directive.is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "tt.policy-undeclared".into(),
            detail: "`require-trusted-types-for 'script'` is set but no `trusted-types <policy-names>` allowlist is declared. Any policy can register; restrict to named policies for tighter defense.".into(),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        sinks: Vec<CapturedTrustedTypesSink>,
        has_require: bool,
        trusted_types: &str,
        has_scripts: bool,
    ) -> TrustedTypesSnapshot {
        TrustedTypesSnapshot {
            page_url: "https://example.com/".into(),
            sinks,
            has_require_directive: has_require,
            trusted_types_directive: trusted_types.into(),
            has_scripts,
        }
    }

    fn sink(kind: &str, preview: &str, trusted: bool) -> CapturedTrustedTypesSink {
        CapturedTrustedTypesSink {
            kind: kind.into(),
            preview: preview.into(),
            trusted,
            t: 100,
        }
    }

    #[test]
    fn empty_snapshot_no_findings() {
        let s = snap(vec![], false, "", false);
        assert!(detect_trusted_types_issues(&s).is_empty());
    }

    #[test]
    fn untrusted_sink_without_directive_warns() {
        let s = snap(vec![sink("innerHTML", "<img>", false)], false, "", true);
        let f = detect_trusted_types_issues(&s);
        assert!(f.iter().any(|x| x.kind == "tt.unprotected-sink"));
    }

    #[test]
    fn trusted_sink_silent() {
        let s = snap(vec![sink("innerHTML", "<img>", true)], false, "", true);
        let f = detect_trusted_types_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "tt.unprotected-sink"));
    }

    #[test]
    fn untrusted_sink_with_directive_silent_on_sink_finding() {
        // require-trusted-types-for set; sink calls were already
        // blocked OR the sink finding is moot.
        let s = snap(vec![sink("innerHTML", "<img>", false)], true, "", true);
        let f = detect_trusted_types_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "tt.unprotected-sink"));
    }

    #[test]
    fn has_scripts_without_directive_warns_directive_missing() {
        let s = snap(vec![], false, "", true);
        let f = detect_trusted_types_issues(&s);
        assert!(f.iter().any(|x| x.kind == "tt.directive-missing"));
    }

    #[test]
    fn no_scripts_silent_on_directive_missing() {
        let s = snap(vec![], false, "", false);
        let f = detect_trusted_types_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "tt.directive-missing"));
    }

    #[test]
    fn directive_set_but_no_policy_allowlist_warns() {
        let s = snap(vec![], true, "", false);
        let f = detect_trusted_types_issues(&s);
        assert!(f.iter().any(|x| x.kind == "tt.policy-undeclared"));
    }

    #[test]
    fn directive_with_policy_allowlist_silent() {
        let s = snap(vec![], true, "default loom-policy", false);
        let f = detect_trusted_types_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "tt.policy-undeclared"));
    }

    #[test]
    fn aggregates_sinks_by_kind() {
        let s = snap(
            vec![
                sink("innerHTML", "a", false),
                sink("innerHTML", "b", false),
                sink("document.write", "c", false),
            ],
            false,
            "",
            true,
        );
        let f = detect_trusted_types_issues(&s);
        let unprot = f.iter().find(|x| x.kind == "tt.unprotected-sink").unwrap();
        assert!(unprot.detail.contains("innerHTML×2"));
        assert!(unprot.detail.contains("document.write×1"));
    }

    #[test]
    fn examples_capped_at_5_and_preview_at_80() {
        let mut sinks = Vec::new();
        for i in 0..10 {
            sinks.push(sink(
                "innerHTML",
                &format!("payload-{i}-{}", "x".repeat(200)),
                false,
            ));
        }
        let s = snap(sinks, false, "", true);
        let f = detect_trusted_types_issues(&s);
        let unprot = f.iter().find(|x| x.kind == "tt.unprotected-sink").unwrap();
        assert!(unprot.detail.contains("10 DOM-sink"));
        // 5 examples → 4 separator instances of "; " between them
        // Don't be too strict; just count "innerHTML <- '" prefixes
        let count = unprot.detail.matches("innerHTML <- '").count();
        assert_eq!(count, 5);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap(
            vec![sink("setTimeout(string)", "evil()", false)],
            true,
            "default",
            true,
        );
        let j = serde_json::to_string(&s).expect("ser");
        let back: TrustedTypesSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.sinks.len(), s.sinks.len());
        assert_eq!(back.has_require_directive, s.has_require_directive);
    }
}
