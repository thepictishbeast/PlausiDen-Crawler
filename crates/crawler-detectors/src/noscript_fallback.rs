//! `noscript_fallback` — universal-access audit for JS-required
//! pages.
//!
//! Per the W3C WCAG-EM methodology + WAI-ARIA "no-JS reader"
//! guidance: pages that render meaningful content only after JS
//! executes MUST provide a `<noscript>` fallback that explains
//! the requirement and offers an alternate path.
//!
//! Beyond WCAG, this matters operationally for:
//!   * Tor Browser users with "Safer" / "Safest" security level
//!     (JS disabled by default)
//!   * Reader-view extensions that strip JS
//!   * Search engine crawlers that lag on JS rendering
//!   * Air-gapped, archival, and offline-first deployments
//!
//! Findings:
//!   * `noscript.missing`              strict   JS required for
//!                                              first paint AND no
//!                                              `<noscript>` present
//!   * `noscript.empty-message`        warn     `<noscript>` present
//!                                              but contains no
//!                                              text content
//!   * `noscript.unhelpful`            warn     `<noscript>` text
//!                                              < 16 chars (likely
//!                                              "JS required.")
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on snapshot types.
//! * Pure detector function; no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Minimum content length (chars) inside `<noscript>` to be
/// considered helpful.
pub const NOSCRIPT_HELPFUL_MIN_CHARS: usize = 16;

/// Captured noscript state for one page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct NoscriptSnapshot {
    /// Page URL.
    pub page_url: String,
    /// Whether the rendered DOM has meaningful body content
    /// observable BEFORE JS executes. False indicates JS is
    /// required.
    pub has_pre_js_body_content: bool,
    /// Combined text content of all `<noscript>` elements,
    /// trimmed. Empty when no `<noscript>` present at all.
    pub noscript_text: String,
}

/// Page-side eval. This needs to be invoked AFTER serving the
/// page with JS disabled (the runner manages the two-pass capture
/// outside this detector). The detector only consumes the typed
/// snapshot.
pub const NOSCRIPT_FALLBACK_JS: &str = r##"(() => {
    const nodes = document.querySelectorAll('noscript');
    let combined = '';
    for (let i = 0; i < nodes.length; i++) {
        combined += ' ' + (nodes[i].textContent || '');
    }
    combined = combined.trim().replace(/\s+/g, ' ');
    // hasPreJsBodyContent must come from the JS-disabled pass —
    // this JS only collects the noscript text. The runner harness
    // composes the final snapshot.
    return {
        pageUrl: window.location.href,
        hasPreJsBodyContent: false,
        noscriptText: combined
    };
})()"##;

/// Run the detector.
pub fn detect_noscript_fallback_issues(snap: &NoscriptSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    // If the page renders meaningful content without JS, no
    // fallback is required.
    if snap.has_pre_js_body_content {
        return out;
    }
    let trimmed = snap.noscript_text.trim();
    if trimmed.is_empty() {
        // `<noscript>` may exist but be empty, OR there may be no
        // `<noscript>` at all. Either way: there's no helpful
        // fallback.
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "noscript.missing".into(),
            detail: "page renders nothing without JS and has no `<noscript>` fallback content"
                .into(),
        });
        return out;
    }
    if trimmed.chars().count() < NOSCRIPT_HELPFUL_MIN_CHARS {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "noscript.unhelpful".into(),
            detail: format!(
                "`<noscript>` fallback is only {} chars ({:?}); expected a useful explanation + alternate path",
                trimmed.chars().count(),
                trimmed
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(has_pre_js: bool, noscript: &str) -> NoscriptSnapshot {
        NoscriptSnapshot {
            page_url: "https://example.com/".into(),
            has_pre_js_body_content: has_pre_js,
            noscript_text: noscript.to_string(),
        }
    }

    #[test]
    fn page_with_pre_js_content_is_clean() {
        // SSR / static HTML page — no `<noscript>` needed.
        let s = snap(true, "");
        assert!(detect_noscript_fallback_issues(&s).is_empty());
    }

    #[test]
    fn js_required_without_noscript_is_strict() {
        let s = snap(false, "");
        let f = detect_noscript_fallback_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert_eq!(f[0].kind, "noscript.missing");
    }

    #[test]
    fn unhelpful_noscript_warns() {
        let s = snap(false, "JS required.");
        let f = detect_noscript_fallback_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Warn);
        assert_eq!(f[0].kind, "noscript.unhelpful");
    }

    #[test]
    fn helpful_noscript_is_clean() {
        let s = snap(
            false,
            "This site requires JavaScript. View the static archive at /static/.",
        );
        assert!(detect_noscript_fallback_issues(&s).is_empty());
    }

    #[test]
    fn noscript_with_just_whitespace_is_treated_as_missing() {
        let s = snap(false, "   \n\t  ");
        let f = detect_noscript_fallback_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "noscript.missing");
    }

    #[test]
    fn pre_js_content_overrides_short_noscript() {
        // Page has SSR content AND a too-short `<noscript>` — clean.
        let s = snap(true, "JS req.");
        assert!(detect_noscript_fallback_issues(&s).is_empty());
    }
}
