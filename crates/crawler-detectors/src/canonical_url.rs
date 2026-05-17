//! `canonical_url` — `<link rel="canonical">` validity.
//!
//! The canonical link relationship tells search engines + LLM
//! crawlers which URL is the authoritative version when a page
//! is accessible via multiple paths (query-param permutations,
//! trailing-slash variants, www vs naked, http vs https). Wrong
//! or missing canonicals cause:
//!   * search-rank dilution (signals split across duplicate URLs)
//!   * LLM training-data dedup failures
//!   * URL canonicalization mismatches on the customer's side
//!     (analytics, server logs)
//!
//! Findings:
//!   * `canonical.missing`             warn   no <link rel=canonical>
//!                                            (per RFC 6596 strongly
//!                                            recommended on every
//!                                            indexable page)
//!   * `canonical.multiple`            strict more than one canonical
//!                                            link (search engines
//!                                            ignore all of them)
//!   * `canonical.relative`            warn   relative href (works
//!                                            but spec recommends
//!                                            absolute)
//!   * `canonical.scheme-mismatch`     strict canonical https vs
//!                                            current page http
//!                                            (or vice versa)
//!   * `canonical.points-to-self-loop` strict canonical ≠ current
//!                                            page URL AND target
//!                                            also has canonical
//!                                            back to current
//!                                            (loop)
//!
//! The "loop" check requires a second pass; this detector only
//! flags it when the consumer pre-resolves the target's canonical
//! and provides `target_canonical`.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on snapshot types.
//! * Pure detector function; no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Captured canonical state for one page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CanonicalSnapshot {
    /// Page's own canonical URL (the URL the operator believes
    /// they're at).
    pub page_url: String,
    /// All `<link rel="canonical">` `href` values discovered on
    /// the page.
    pub canonicals: Vec<String>,
    /// When the consumer has pre-resolved the target page's own
    /// canonical (e.g. via a second crawl pass), this is its
    /// value. Used by the loop check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_canonical: Option<String>,
}

/// Page-side eval. Collects every `<link rel="canonical">` href.
pub const CANONICAL_URL_JS: &str = r##"(() => {
    const links = document.querySelectorAll('link[rel~="canonical"]');
    const out = [];
    for (let i = 0; i < links.length; i++) {
        const h = links[i].getAttribute('href');
        if (h) out.push(h);
    }
    return {
        pageUrl: window.location.href,
        canonicals: out,
        targetCanonical: null
    };
})()"##;

/// Extract the URL scheme (`"https"`, `"http"`, etc.) from a URL.
/// Returns `None` for relative / opaque / fragment-only inputs.
fn url_scheme(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut end = 0;
    while end < bytes.len() {
        let c = bytes[end];
        if c == b':' {
            // Scheme must start with a letter (RFC 3986).
            if end == 0 || !bytes[0].is_ascii_alphabetic() {
                return None;
            }
            return s.get(0..end);
        }
        if !(c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.') {
            return None;
        }
        end += 1;
    }
    None
}

fn is_absolute(s: &str) -> bool {
    url_scheme(s).is_some() || s.starts_with("//")
}

/// Run the detector.
pub fn detect_canonical_url_issues(snap: &CanonicalSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();

    match snap.canonicals.len() {
        0 => {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "canonical.missing".into(),
                detail: format!(
                    "page {} has no <link rel=\"canonical\">; search-rank dilution risk",
                    snap.page_url
                ),
            });
            return out;
        }
        1 => {}
        n => {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "canonical.multiple".into(),
                detail: format!(
                    "page {} declares {} <link rel=\"canonical\"> elements; engines ignore all of them",
                    snap.page_url, n
                ),
            });
        }
    }

    let primary = snap.canonicals[0].as_str();

    if !is_absolute(primary) {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "canonical.relative".into(),
            detail: format!(
                "canonical href {:?} is relative; absolute URL recommended by the canonical-link relation spec",
                primary
            ),
        });
    }

    // Scheme mismatch check — only meaningful when both are absolute.
    if let (Some(page_scheme), Some(canon_scheme)) =
        (url_scheme(&snap.page_url), url_scheme(primary))
    {
        if page_scheme != canon_scheme {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "canonical.scheme-mismatch".into(),
                detail: format!(
                    "page is {} but canonical declares {} scheme ({:?} → {:?})",
                    page_scheme, canon_scheme, snap.page_url, primary
                ),
            });
        }
    }

    // Loop check — primary points elsewhere AND elsewhere points back.
    if primary != snap.page_url {
        if let Some(target_canon) = &snap.target_canonical {
            if target_canon == &snap.page_url {
                out.push(AxisFinding {
                    severity: AxisSeverity::Strict,
                    kind: "canonical.points-to-self-loop".into(),
                    detail: format!(
                        "canonical of {} → {} but {}'s canonical points back ({} ↔ {}); engines ignore loops",
                        snap.page_url, primary, primary, snap.page_url, primary
                    ),
                });
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(page: &str, canonicals: &[&str]) -> CanonicalSnapshot {
        CanonicalSnapshot {
            page_url: page.into(),
            canonicals: canonicals.iter().map(|s| (*s).to_string()).collect(),
            target_canonical: None,
        }
    }

    #[test]
    fn missing_canonical_warns() {
        let s = snap("https://example.com/p", &[]);
        let f = detect_canonical_url_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Warn);
        assert_eq!(f[0].kind, "canonical.missing");
    }

    #[test]
    fn single_self_referential_canonical_is_clean() {
        let s = snap("https://example.com/p", &["https://example.com/p"]);
        assert!(detect_canonical_url_issues(&s).is_empty());
    }

    #[test]
    fn multiple_canonicals_is_strict() {
        let s = snap(
            "https://example.com/p",
            &["https://example.com/p", "https://example.com/p?utm=x"],
        );
        let f = detect_canonical_url_issues(&s);
        assert!(f.iter().any(|x| x.kind == "canonical.multiple"));
    }

    #[test]
    fn relative_canonical_warns() {
        let s = snap("https://example.com/p", &["/p"]);
        let f = detect_canonical_url_issues(&s);
        assert!(f.iter().any(|x| x.kind == "canonical.relative"));
    }

    #[test]
    fn protocol_relative_canonical_does_not_warn_relative() {
        // // is absolute (protocol-relative).
        let s = snap("https://example.com/p", &["//example.com/p"]);
        let f = detect_canonical_url_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "canonical.relative"));
    }

    #[test]
    fn scheme_mismatch_is_strict() {
        let s = snap("https://example.com/p", &["http://example.com/p"]);
        let f = detect_canonical_url_issues(&s);
        assert!(f.iter().any(|x| x.kind == "canonical.scheme-mismatch"));
    }

    #[test]
    fn loop_detected_when_target_canonical_points_back() {
        let mut s = snap("https://example.com/p", &["https://example.com/q"]);
        s.target_canonical = Some("https://example.com/p".into());
        let f = detect_canonical_url_issues(&s);
        assert!(f.iter().any(|x| x.kind == "canonical.points-to-self-loop"));
    }

    #[test]
    fn loop_not_flagged_when_target_canonical_unknown() {
        let s = snap("https://example.com/p", &["https://example.com/q"]);
        let f = detect_canonical_url_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "canonical.points-to-self-loop"));
    }

    #[test]
    fn url_scheme_recognizes_common_schemes() {
        assert_eq!(url_scheme("https://x"), Some("https"));
        assert_eq!(url_scheme("http://x"), Some("http"));
        assert_eq!(url_scheme("ftp://x"), Some("ftp"));
        assert_eq!(url_scheme("/relative"), None);
        assert_eq!(url_scheme("//protocol-relative"), None);
        assert_eq!(url_scheme(""), None);
        assert_eq!(url_scheme("1bad://x"), None); // starts with digit
    }
}
