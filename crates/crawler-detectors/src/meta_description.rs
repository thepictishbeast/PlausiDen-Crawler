//! `meta_description` — page `<meta name="description">` detector.
//!
//! Mirror of `src/metaDescription.ts`. Findings:
//!
//!   * `meta-description.missing`     warn   no <meta name=description>
//!   * `meta-description.empty`       warn   present but content empty
//!   * `meta-description.too-short`   warn   trimmed < 50 chars
//!   * `meta-description.too-long`    warn   trimmed > 160 chars
//!
//! All warn — a missing description doesn't break the page; it
//! just suboptimizes search-result + social-share previews.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const META_DESCRIPTION_JS: &str = r##"(() => {
    const m = document.querySelector('head meta[name="description"]');
    if (!m) return { present: false, raw: '' };
    return { present: true, raw: m.getAttribute('content') || '' };
})()"##;

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MetaDescriptionSnapshot {
    /// Page URL.
    pub page_url: String,
    /// True iff a `<meta name="description">` exists in head.
    pub present: bool,
    /// Content attribute, untrimmed.
    pub raw: String,
}

const TOO_SHORT_MAX: usize = 50;
const TOO_LONG_MIN: usize = 160;

#[must_use]
pub fn detect_meta_description_issues(snap: &MetaDescriptionSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::<crate::AxisFinding>::new();

    if !snap.present {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "meta-description.missing".to_owned(),
            detail: "Page has no <meta name=\"description\"> in head. Search engines synthesize one from page text (usually poorly); social-share previews lose a useful summary. Add a 50-160 char description that reflects the page's actual content.".to_owned(),
        });
        return out;
    }

    let trimmed = snap.raw.trim();
    if trimmed.is_empty() {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "meta-description.empty".to_owned(),
            detail: "<meta name=\"description\"> is present but content is empty or whitespace-only. Same effect as missing.".to_owned(),
        });
        return out;
    }

    let length = trimmed.chars().count();
    if length < TOO_SHORT_MAX {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "meta-description.too-short".to_owned(),
            detail: format!(
                "Meta description is only {length} characters ('{trimmed}'). Search-result snippets and social-share previews need ~50-160 chars of useful summary."
            ),
        });
    }
    if length > TOO_LONG_MIN {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "meta-description.too-long".to_owned(),
            detail: format!(
                "Meta description is {length} characters — Google + most engines truncate around 155-160 chars in results, so the tail is invisible. Trim to <= 160."
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap_with(raw: &str) -> MetaDescriptionSnapshot {
        MetaDescriptionSnapshot {
            page_url: "http://t/".to_owned(),
            present: true,
            raw: raw.to_owned(),
        }
    }

    fn snap_missing() -> MetaDescriptionSnapshot {
        MetaDescriptionSnapshot {
            page_url: "http://t/".to_owned(),
            present: false,
            raw: String::new(),
        }
    }

    #[test]
    fn js_brackets_balanced() {
        assert_eq!(
            META_DESCRIPTION_JS.matches('(').count(),
            META_DESCRIPTION_JS.matches(')').count()
        );
        assert_eq!(
            META_DESCRIPTION_JS.matches('{').count(),
            META_DESCRIPTION_JS.matches('}').count()
        );
    }

    #[test]
    fn clean_no_findings() {
        let f = detect_meta_description_issues(&snap_with(
            "A clear, accurate page summary that fits the search-result preview window.",
        ));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn missing_warn() {
        let f = detect_meta_description_issues(&snap_missing());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "meta-description.missing");
        assert_eq!(f[0].severity, crate::AxisSeverity::Warn);
    }

    #[test]
    fn empty_warn_short_circuits() {
        let f = detect_meta_description_issues(&snap_with(""));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "meta-description.empty");
    }

    #[test]
    fn whitespace_treated_empty() {
        let f = detect_meta_description_issues(&snap_with("   \t  "));
        assert!(f.iter().any(|x| x.kind == "meta-description.empty"));
    }

    #[test]
    fn forty_nine_too_short() {
        let f = detect_meta_description_issues(&snap_with(&"X".repeat(49)));
        assert!(f.iter().any(|x| x.kind == "meta-description.too-short"));
    }

    #[test]
    fn fifty_passes() {
        let f = detect_meta_description_issues(&snap_with(&"X".repeat(50)));
        assert!(!f.iter().any(|x| x.kind == "meta-description.too-short"));
    }

    #[test]
    fn one_seventy_too_long() {
        let f = detect_meta_description_issues(&snap_with(&"X".repeat(170)));
        assert!(f.iter().any(|x| x.kind == "meta-description.too-long"));
    }

    #[test]
    fn one_sixty_passes() {
        let f = detect_meta_description_issues(&snap_with(&"X".repeat(160)));
        assert!(!f.iter().any(|x| x.kind == "meta-description.too-long"));
    }

    #[test]
    fn trim_removes_whitespace() {
        let raw = format!("   {}   ", "X".repeat(60));
        let f = detect_meta_description_issues(&snap_with(&raw));
        assert!(f.is_empty(), "{:?}", f);
    }
}
