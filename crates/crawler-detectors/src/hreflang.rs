//! `hreflang` — alternate-language link audit.
//!
//! Per RFC 4647 + Google's hreflang documentation: pages
//! available in multiple locales SHOULD declare
//! `<link rel="alternate" hreflang="…">` for every variant
//! including the page itself.
//!
//! Common authoring failures the detector flags:
//!
//!   * a hreflang without a reciprocal back-link on the target
//!   * incorrect language code (lowercase, valid BCP 47)
//!   * missing `x-default` for sites with international audiences
//!   * the page's own URL not declared (self-reference required)
//!
//! Findings:
//!   * `hreflang.invalid-tag`       strict   tag fails BCP 47
//!                                            shape check
//!   * `hreflang.self-missing`      strict   alternates declared
//!                                            but the current page
//!                                            URL is NOT among them
//!   * `hreflang.no-x-default`      warn     ≥ 2 alternates without
//!                                            an x-default
//!   * `hreflang.duplicate`         strict   same tag declared
//!                                            twice with different
//!                                            hrefs
//!
//! Reciprocal-back-link checks require cross-page state and live
//! in the runner; this detector only handles single-page
//! invariants.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on snapshot types.
//! * Pure detector function; no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured hreflang alternate entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HreflangEntry {
    /// `hreflang=` attribute value (lowercased — `"en"`, `"en-us"`,
    /// `"x-default"`, etc.).
    pub tag: String,
    /// `href=` attribute value.
    pub href: String,
}

/// Captured hreflang set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HreflangSnapshot {
    /// Current page URL (absolute).
    pub page_url: String,
    /// All `<link rel="alternate" hreflang="…">` entries.
    pub entries: Vec<HreflangEntry>,
}

/// Page-side eval.
pub const HREFLANG_JS: &str = r##"(() => {
    const out = [];
    const links = document.querySelectorAll('link[rel~="alternate"][hreflang]');
    for (let i = 0; i < links.length; i++) {
        const tag = (links[i].getAttribute('hreflang') || '').toLowerCase();
        const href = links[i].getAttribute('href') || '';
        if (tag.length > 0 && href.length > 0) out.push({ tag: tag, href: href });
    }
    return { pageUrl: window.location.href, entries: out };
})()"##;

/// Validate a hreflang tag shape. Accepts:
///   * `"x-default"` (the only `x-` form recognised)
///   * 2-3 letter lowercase language tag
///   * optional `-` + 2-letter region (uppercase OR lowercase
///     accepted — engines normalise) OR 3-digit region
///   * total length ≤ 12
fn is_valid_hreflang(tag: &str) -> bool {
    if tag == "x-default" {
        return true;
    }
    if tag.is_empty() || tag.len() > 12 {
        return false;
    }
    let parts: Vec<&str> = tag.split('-').collect();
    let primary = parts[0];
    if primary.len() < 2 || primary.len() > 3 || !primary.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if parts.len() >= 2 {
        let region = parts[1];
        let region_ok = (region.len() == 2 && region.chars().all(|c| c.is_ascii_alphabetic()))
            || (region.len() == 3 && region.chars().all(|c| c.is_ascii_digit()));
        if !region_ok {
            return false;
        }
    }
    if parts.len() > 2 {
        return false;
    }
    true
}

/// Run the detector.
pub fn detect_hreflang_issues(snap: &HreflangSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    if snap.entries.is_empty() {
        return out;
    }

    // Invalid-tag detection.
    for e in &snap.entries {
        if !is_valid_hreflang(&e.tag) {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "hreflang.invalid-tag".into(),
                detail: format!(
                    "hreflang tag {:?} fails BCP 47 shape check (entry href: {})",
                    e.tag, e.href
                ),
            });
        }
    }

    // Duplicate-tag check.
    let mut by_tag: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    for e in &snap.entries {
        by_tag.entry(e.tag.as_str()).or_default().push(&e.href);
    }
    for (tag, hrefs) in &by_tag {
        if hrefs.len() > 1 {
            // Multiple entries with the same tag but DIFFERENT
            // hrefs is the failing case; identical-href duplicates
            // are just authoring redundancy (still warn? we treat
            // as strict because engines may take only the first).
            let mut unique = std::collections::HashSet::new();
            for h in hrefs {
                unique.insert(*h);
            }
            if unique.len() > 1 {
                out.push(AxisFinding {
                    severity: AxisSeverity::Strict,
                    kind: "hreflang.duplicate".into(),
                    detail: format!(
                        "hreflang tag {:?} declared {} times with different hrefs ({:?})",
                        tag,
                        hrefs.len(),
                        unique
                    ),
                });
            }
        }
    }

    // Self-reference check.
    let has_self = snap
        .entries
        .iter()
        .any(|e| e.href == snap.page_url && e.tag != "x-default");
    if !has_self {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "hreflang.self-missing".into(),
            detail: format!(
                "page {} has hreflang alternates but no self-reference among them",
                snap.page_url
            ),
        });
    }

    // x-default check (only when ≥ 2 alternates).
    if snap.entries.len() >= 2 && !snap.entries.iter().any(|e| e.tag == "x-default") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "hreflang.no-x-default".into(),
            detail: format!(
                "page declares {} hreflang alternates without an x-default fallback",
                snap.entries.len()
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(tag: &str, href: &str) -> HreflangEntry {
        HreflangEntry {
            tag: tag.to_string(),
            href: href.to_string(),
        }
    }

    fn snap(page: &str, entries: Vec<HreflangEntry>) -> HreflangSnapshot {
        HreflangSnapshot {
            page_url: page.into(),
            entries,
        }
    }

    #[test]
    fn no_alternates_is_clean() {
        let s = snap("https://example.com/", vec![]);
        assert!(detect_hreflang_issues(&s).is_empty());
    }

    #[test]
    fn valid_self_referential_with_x_default_is_clean() {
        let s = snap(
            "https://example.com/",
            vec![
                entry("en", "https://example.com/"),
                entry("ja", "https://example.com/ja/"),
                entry("x-default", "https://example.com/"),
            ],
        );
        assert!(detect_hreflang_issues(&s).is_empty());
    }

    #[test]
    fn invalid_tag_is_strict() {
        let s = snap(
            "https://example.com/",
            vec![entry("english", "https://example.com/")],
        );
        let f = detect_hreflang_issues(&s);
        assert!(f.iter().any(|x| x.kind == "hreflang.invalid-tag"));
    }

    #[test]
    fn duplicate_tag_with_different_hrefs_is_strict() {
        let s = snap(
            "https://example.com/",
            vec![
                entry("en", "https://example.com/"),
                entry("en", "https://example.com/v2/"),
                entry("x-default", "https://example.com/"),
            ],
        );
        let f = detect_hreflang_issues(&s);
        assert!(f.iter().any(|x| x.kind == "hreflang.duplicate"));
    }

    #[test]
    fn missing_self_reference_is_strict() {
        let s = snap(
            "https://example.com/",
            vec![
                entry("ja", "https://example.com/ja/"),
                entry("de", "https://example.com/de/"),
                entry("x-default", "https://example.com/de/"),
            ],
        );
        let f = detect_hreflang_issues(&s);
        assert!(f.iter().any(|x| x.kind == "hreflang.self-missing"));
    }

    #[test]
    fn multiple_alternates_without_x_default_warns() {
        let s = snap(
            "https://example.com/",
            vec![
                entry("en", "https://example.com/"),
                entry("ja", "https://example.com/ja/"),
            ],
        );
        let f = detect_hreflang_issues(&s);
        assert!(f.iter().any(|x| x.kind == "hreflang.no-x-default"));
    }

    #[test]
    fn single_alternate_no_x_default_is_clean_for_that_check() {
        let s = snap(
            "https://example.com/",
            vec![entry("en", "https://example.com/")],
        );
        let f = detect_hreflang_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "hreflang.no-x-default"));
    }

    #[test]
    fn hreflang_tag_shape_validator_accepts_common_forms() {
        assert!(is_valid_hreflang("en"));
        assert!(is_valid_hreflang("en-us"));
        assert!(is_valid_hreflang("en-US"));
        assert!(is_valid_hreflang("zh-cn"));
        assert!(is_valid_hreflang("x-default"));
        assert!(is_valid_hreflang("fr-150")); // 3-digit region
    }

    #[test]
    fn hreflang_tag_shape_validator_rejects_bad_forms() {
        assert!(!is_valid_hreflang(""));
        assert!(!is_valid_hreflang("english"));
        assert!(!is_valid_hreflang("en-USA")); // 3-letter region not allowed here
        assert!(!is_valid_hreflang("en-us-extra"));
        assert!(!is_valid_hreflang("x-custom")); // only x-default is x-
    }
}
