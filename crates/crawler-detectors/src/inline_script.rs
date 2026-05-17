//! `inline_script` — inline-script + event-handler + javascript: URI
//! per-element DOM audit. T76 port of `src/inlineScript.ts`.
//!
//! Threat model: pages with `script-src 'nonce-<random>'` (modern
//! best-practice CSP) require EVERY inline `<script>` block to carry
//! the matching nonce OR have a sha256 hash pinned in the CSP. Without
//! either, the script is silently dropped under strict CSP, and is a
//! stored-XSS sink under no-CSP.
//!
//! Findings (mirror TS byte-for-byte):
//!
//!   * `inline-script.present-without-nonce`   warn
//!   * `inline-script.event-handler-attribute` warn
//!   * `inline-script.javascript-uri`          warn
//!   * `inline-script.no-csp-but-inline`       warn (composite)
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured inline `<script>` block (no `src=` attribute).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedInlineScript {
    /// Truncated source (first 200 chars).
    pub src: String,
    /// True iff a `nonce` attribute was present.
    pub has_nonce: bool,
    /// Base64 SHA-256 of the FULL inline body. Lets the detector
    /// credit hash-pinned blocks as CSP-covered. Optional because
    /// pre-cycle-54 captures don't include it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// One captured event-handler attribute (`onclick=`, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedEventHandler {
    /// Lowercased tag name.
    pub tag: String,
    /// Attribute name (e.g. `onclick`).
    pub attribute: String,
    /// Truncated value (first 200 chars).
    pub value: String,
}

/// One captured `javascript:` URI attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedJavascriptUri {
    /// Lowercased tag name.
    pub tag: String,
    /// Truncated href/src/action value.
    pub uri: String,
}

/// Captured page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct InlineScriptSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// True iff a CSP header (or meta http-equiv) is present.
    pub has_csp: bool,
    /// Raw value of the CSP `script-src` directive (used for
    /// hash-pinning detection). Empty when no script-src observable.
    pub csp_script_src: String,
    /// Inline `<script>` blocks (no `src=`).
    pub inline_scripts: Vec<CapturedInlineScript>,
    /// Event-handler attributes.
    pub event_handlers: Vec<CapturedEventHandler>,
    /// `javascript:` URIs in href / src / action / formaction.
    pub javascript_uris: Vec<CapturedJavascriptUri>,
}

/// True iff the script's sha256 hash is pinned in the CSP script-src.
fn is_hash_pinned(s: &CapturedInlineScript, script_src_lower: &str) -> bool {
    let Some(hash) = s.sha256.as_deref() else {
        return false;
    };
    let needle = format!("sha256-{hash}");
    script_src_lower.contains(&needle.to_ascii_lowercase())
}

/// Pure detector: snapshot → findings.
pub fn detect_inline_script_issues(snap: &InlineScriptSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    let script_src_lower = snap.csp_script_src.to_ascii_lowercase();

    let uncovered: Vec<&CapturedInlineScript> = snap
        .inline_scripts
        .iter()
        .filter(|s| !s.has_nonce && !is_hash_pinned(s, &script_src_lower))
        .collect();

    if !uncovered.is_empty() {
        let examples: Vec<String> = uncovered
            .iter()
            .take(5)
            .map(|s| format!("<script> '{}'", s.src))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "inline-script.present-without-nonce".into(),
            detail: format!(
                "{} inline <script> block(s) without a 'nonce' attribute OR a matching sha256 hash in CSP `script-src`. Under a strict CSP, these are silently dropped. Without a CSP, they're stored-XSS sinks: any HTML-injection vulnerability that writes a <script> tag executes immediately. Migrate to external <script src> with SRI, add a per-page nonce, OR pin the hash via `script-src 'sha256-<b64>'`. Examples: {}",
                uncovered.len(),
                examples.join("; ")
            ),
        });
    }

    if !snap.event_handlers.is_empty() {
        let examples: Vec<String> = snap
            .event_handlers
            .iter()
            .take(5)
            .map(|h| format!("<{} {}=\"{}\">", h.tag, h.attribute, h.value))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "inline-script.event-handler-attribute".into(),
            detail: format!(
                "{} element(s) carry an event-handler attribute (onclick, onload, onmouseover, etc.). CSP cannot nonce these — they require 'unsafe-inline' or 'unsafe-hashes' to work, both of which weaken protection. Migrate to addEventListener in an external script. Examples: {}",
                snap.event_handlers.len(),
                examples.join("; ")
            ),
        });
    }

    if !snap.javascript_uris.is_empty() {
        let examples: Vec<String> = snap
            .javascript_uris
            .iter()
            .take(5)
            .map(|u| format!("<{} src/href=\"{}\">", u.tag, u.uri))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "inline-script.javascript-uri".into(),
            detail: format!(
                "{} element(s) use a 'javascript:' URI. Old-school JS-execution sink; CSP cannot block without 'unsafe-inline'. Replace with addEventListener-bound handlers. Examples: {}",
                snap.javascript_uris.len(),
                examples.join("; ")
            ),
        });
    }

    if !snap.has_csp
        && (!uncovered.is_empty()
            || !snap.event_handlers.is_empty()
            || !snap.javascript_uris.is_empty())
    {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "inline-script.no-csp-but-inline".into(),
            detail: "Page has inline scripts/handlers AND no Content-Security-Policy header. Nothing stops a stored-XSS injection from executing. The cspPolicy detector also fires 'csp.missing' on this; the additional finding here calls out that the inline scripts make the missing CSP particularly dangerous. Add a strict CSP first (script-src 'self' 'nonce-<random>'), then migrate inline scripts to external src + nonce.".into(),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        inline: Vec<CapturedInlineScript>,
        handlers: Vec<CapturedEventHandler>,
        uris: Vec<CapturedJavascriptUri>,
        has_csp: bool,
        csp_script_src: &str,
    ) -> InlineScriptSnapshot {
        InlineScriptSnapshot {
            page_url: "https://example.com/".into(),
            has_csp,
            csp_script_src: csp_script_src.into(),
            inline_scripts: inline,
            event_handlers: handlers,
            javascript_uris: uris,
        }
    }

    fn inline(src: &str, has_nonce: bool, sha256: Option<&str>) -> CapturedInlineScript {
        CapturedInlineScript {
            src: src.into(),
            has_nonce,
            sha256: sha256.map(String::from),
        }
    }

    #[test]
    fn empty_snapshot_no_findings() {
        let s = snap(vec![], vec![], vec![], true, "");
        assert!(detect_inline_script_issues(&s).is_empty());
    }

    #[test]
    fn inline_without_nonce_warns() {
        let s = snap(
            vec![inline("alert(1)", false, None)],
            vec![],
            vec![],
            true,
            "",
        );
        let f = detect_inline_script_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "inline-script.present-without-nonce"));
    }

    #[test]
    fn inline_with_nonce_clean() {
        let s = snap(
            vec![inline("alert(1)", true, None)],
            vec![],
            vec![],
            true,
            "",
        );
        let f = detect_inline_script_issues(&s);
        assert!(!f
            .iter()
            .any(|x| x.kind == "inline-script.present-without-nonce"));
    }

    #[test]
    fn inline_hash_pinned_clean() {
        let s = snap(
            vec![inline("alert(1)", false, Some("ABCDEF"))],
            vec![],
            vec![],
            true,
            "'self' 'sha256-ABCDEF'",
        );
        let f = detect_inline_script_issues(&s);
        assert!(!f
            .iter()
            .any(|x| x.kind == "inline-script.present-without-nonce"));
    }

    #[test]
    fn hash_pin_case_insensitive() {
        let s = snap(
            vec![inline("alert(1)", false, Some("aBcDeF"))],
            vec![],
            vec![],
            true,
            "'self' 'SHA256-AbCdEf'",
        );
        let f = detect_inline_script_issues(&s);
        assert!(!f
            .iter()
            .any(|x| x.kind == "inline-script.present-without-nonce"));
    }

    #[test]
    fn event_handler_warns() {
        let s = snap(
            vec![],
            vec![CapturedEventHandler {
                tag: "button".into(),
                attribute: "onclick".into(),
                value: "go()".into(),
            }],
            vec![],
            true,
            "",
        );
        let f = detect_inline_script_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "inline-script.event-handler-attribute"));
    }

    #[test]
    fn javascript_uri_warns() {
        let s = snap(
            vec![],
            vec![],
            vec![CapturedJavascriptUri {
                tag: "a".into(),
                uri: "javascript:alert(1)".into(),
            }],
            true,
            "",
        );
        let f = detect_inline_script_issues(&s);
        assert!(f.iter().any(|x| x.kind == "inline-script.javascript-uri"));
    }

    #[test]
    fn composite_no_csp_but_inline_fires() {
        let s = snap(
            vec![inline("alert(1)", false, None)],
            vec![],
            vec![],
            false,
            "",
        );
        let f = detect_inline_script_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "inline-script.no-csp-but-inline"));
    }

    #[test]
    fn composite_silent_when_csp_present() {
        let s = snap(
            vec![inline("alert(1)", false, None)],
            vec![],
            vec![],
            true,
            "",
        );
        let f = detect_inline_script_issues(&s);
        assert!(!f
            .iter()
            .any(|x| x.kind == "inline-script.no-csp-but-inline"));
    }

    #[test]
    fn composite_silent_when_no_inline() {
        let s = snap(vec![], vec![], vec![], false, "");
        let f = detect_inline_script_issues(&s);
        assert!(!f
            .iter()
            .any(|x| x.kind == "inline-script.no-csp-but-inline"));
    }

    #[test]
    fn examples_capped_at_5() {
        let mut inlines = Vec::new();
        for i in 0..10 {
            inlines.push(inline(&format!("alert({i})"), false, None));
        }
        let s = snap(inlines, vec![], vec![], true, "");
        let f = detect_inline_script_issues(&s);
        let p = f
            .iter()
            .find(|x| x.kind == "inline-script.present-without-nonce")
            .unwrap();
        assert!(p.detail.contains("10 inline"));
        let count = p.detail.matches("<script> '").count();
        assert_eq!(count, 5);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap(
            vec![inline("alert(1)", true, Some("ABC"))],
            vec![CapturedEventHandler {
                tag: "div".into(),
                attribute: "onload".into(),
                value: "x()".into(),
            }],
            vec![],
            true,
            "'self' 'sha256-ABC'",
        );
        let j = serde_json::to_string(&s).expect("ser");
        let back: InlineScriptSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.inline_scripts.len(), s.inline_scripts.len());
        assert_eq!(back.event_handlers.len(), s.event_handlers.len());
    }
}
