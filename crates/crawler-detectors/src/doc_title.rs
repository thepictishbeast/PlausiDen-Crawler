//! `doc_title` — document title quality detector.
//!
//! Mirror of `src/docTitle.ts`. Findings:
//!
//!   * `title.missing`     strict   no <title>
//!   * `title.empty`       strict   <title></title> / whitespace
//!   * `title.generic`     warn     "Document" / "Untitled" / etc.
//!   * `title.too-short`   warn     ≤ 2 chars
//!   * `title.too-long`    warn     ≥ 70 chars
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const DOC_TITLE_JS: &str = r##"(() => {
    const t = document.querySelector('head > title');
    if (!t) return { present: false, raw: '' };
    return { present: true, raw: t.textContent || '' };
})()"##;

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DocTitleSnapshot {
    /// Page URL.
    pub page_url: String,
    /// True iff a <title> element exists in head.
    pub present: bool,
    /// Inner text, untrimmed.
    pub raw: String,
}

const GENERIC_TITLES: &[&str] = &[
    "document",
    "untitled",
    "untitled document",
    "untitled page",
    "untitled-1",
    "new document",
    "new page",
    "new tab",
    "page",
    "home",
    "index",
    "welcome",
    "title",
    "about:blank",
];

const TITLE_TOO_SHORT_MAX: usize = 2;
const TITLE_TOO_LONG_MIN: usize = 70;

#[must_use]
pub fn detect_doc_title_issues(snap: &DocTitleSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::<crate::AxisFinding>::new();

    if !snap.present {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "title.missing".to_owned(),
            detail: "Page has no <title> element in <head>. Browsers fall back to the URL; screen readers announce 'untitled document'. Add a unique, descriptive <title>.".to_owned(),
        });
        return out;
    }

    let trimmed = snap.raw.trim();
    if trimmed.is_empty() {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "title.empty".to_owned(),
            detail: "Page <title> is empty or whitespace-only. Same effect as missing — the URL becomes the fallback title.".to_owned(),
        });
        return out;
    }

    let lower = trimmed.to_lowercase();
    if GENERIC_TITLES.contains(&lower.as_str()) {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "title.generic".to_owned(),
            detail: format!(
                "Page <title> is a generic default ('{trimmed}') — almost always copy-paste leftover from a template / IDE. Replace with a descriptive page-specific title."
            ),
        });
    }

    if trimmed.chars().count() <= TITLE_TOO_SHORT_MAX {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "title.too-short".to_owned(),
            detail: format!(
                "Page <title> is only {} characters ('{trimmed}'). Search-result previews and screen-reader announcements need more context.",
                trimmed.chars().count()
            ),
        });
    }

    if trimmed.chars().count() >= TITLE_TOO_LONG_MIN {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "title.too-long".to_owned(),
            detail: format!(
                "Page <title> is {} characters — Google truncates around 60-70 chars in search results. Trim or move detail into <meta name=\"description\">.",
                trimmed.chars().count()
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap_with(raw: &str) -> DocTitleSnapshot {
        DocTitleSnapshot {
            page_url: "http://t/".to_owned(),
            present: true,
            raw: raw.to_owned(),
        }
    }

    fn snap_missing() -> DocTitleSnapshot {
        DocTitleSnapshot {
            page_url: "http://t/".to_owned(),
            present: false,
            raw: String::new(),
        }
    }

    #[test]
    fn js_brackets_balanced() {
        assert_eq!(DOC_TITLE_JS.matches('(').count(), DOC_TITLE_JS.matches(')').count());
        assert_eq!(DOC_TITLE_JS.matches('{').count(), DOC_TITLE_JS.matches('}').count());
    }

    #[test]
    fn clean_no_findings() {
        let f = detect_doc_title_issues(&snap_with("Acme — Pricing"));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn missing_strict() {
        let f = detect_doc_title_issues(&snap_missing());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "title.missing");
    }

    #[test]
    fn empty_strict_only() {
        let f = detect_doc_title_issues(&snap_with(""));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "title.empty");
    }

    #[test]
    fn whitespace_treated_as_empty() {
        let f = detect_doc_title_issues(&snap_with("   \t\n  "));
        assert!(f.iter().any(|x| x.kind == "title.empty"));
    }

    #[test]
    fn untitled_generic_warn() {
        let f = detect_doc_title_issues(&snap_with("Untitled"));
        assert!(f.iter().any(|x| x.kind == "title.generic"));
    }

    #[test]
    fn uppercase_document_caught() {
        let f = detect_doc_title_issues(&snap_with("DOCUMENT"));
        assert!(f.iter().any(|x| x.kind == "title.generic"));
    }

    #[test]
    fn composite_title_not_generic() {
        let f = detect_doc_title_issues(&snap_with("Document - Acme"));
        assert!(!f.iter().any(|x| x.kind == "title.generic"));
    }

    #[test]
    fn two_char_short() {
        let f = detect_doc_title_issues(&snap_with("OK"));
        assert!(f.iter().any(|x| x.kind == "title.too-short"));
    }

    #[test]
    fn three_char_acceptable() {
        let f = detect_doc_title_issues(&snap_with("FAQ"));
        assert!(!f.iter().any(|x| x.kind == "title.too-short"));
    }

    #[test]
    fn long_title_warn() {
        let long = "X".repeat(80);
        let f = detect_doc_title_issues(&snap_with(&long));
        assert!(f.iter().any(|x| x.kind == "title.too-long"));
    }

    #[test]
    fn sixty_nine_char_passes() {
        let exact69 = "X".repeat(69);
        let f = detect_doc_title_issues(&snap_with(&exact69));
        assert!(!f.iter().any(|x| x.kind == "title.too-long"));
    }
}
