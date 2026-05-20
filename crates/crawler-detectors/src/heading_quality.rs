//! `heading_quality` — flags low-quality heading text on the
//! rendered page. Companion to `heading_order` (which checks
//! structural level/skip rules).
//!
//! Findings:
//!
//!   * `heading_quality.single-word` warn — heading text is a
//!     single word (Features / About / Pricing / etc). Single-
//!     word headings are SaaS-trope placeholder content; they
//!     read as filler rather than substance.
//!   * `heading_quality.saas-cliche` warn — heading text matches
//!     a known SaaS-marketing cliche phrase (Get Started, Learn
//!     More, Built for Speed, Powered by AI, Numbers that
//!     compose). Flags scannable-bait shapes the editorial
//!     substrate is built to refuse.
//!
//! warn-only — runtime is fine; the editorial layer just looks
//! consumer-shaped. Companion to Forge's aesthetic_distinctiveness
//! and editorial_purity_gate (build-time equivalents).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval — collects every heading's text + level.
pub const HEADING_QUALITY_JS: &str = r##"(() => {
    const out = [];
    const els = document.querySelectorAll('h1, h2, h3, h4, h5, h6');
    for (let i = 0; i < els.length; i++) {
        const level = parseInt(els[i].tagName.substring(1), 10);
        const text = (els[i].textContent || '').trim();
        out.push({ level, text });
    }
    return { headings: out };
})()"##;

/// One captured heading.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct Heading {
    /// 1-6.
    pub level: u32,
    /// Trimmed textContent.
    pub text: String,
}

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub struct HeadingQualitySnapshot {
    /// Page URL.
    pub page_url: String,
    /// All headings, in document order.
    pub headings: Vec<Heading>,
}

/// SaaS-marketing cliche list. Matched case-insensitively as
/// the WHOLE heading text (after trimming + collapsing
/// whitespace). Picked deliberately conservative — these
/// phrases are unambiguous filler, not borderline editorial
/// choices.
const SAAS_CLICHES: &[&str] = &[
    "get started",
    "learn more",
    "built for speed",
    "built for scale",
    "powered by ai",
    "numbers that compose",
    "trusted by leaders",
    "trusted by teams",
    "join the waitlist",
    "see how it works",
    "ready to get started",
    "ready to build",
    "level up",
    "supercharge your workflow",
    "unlock the power",
    "redefine the way",
    "future of work",
    "modern stack",
    // Cycle 2026-05-20 list expansion — keep in sync with
    // forge-phases::slop_dictionary::SAAS_CLICHES.
    "ai for everyone",
    "everything you need",
    "all-in-one platform",
    "your all-in-one",
    "the way you work",
    "where teams come together",
    "trusted by thousands",
    "ship faster",
    "move fast",
    "ship with confidence",
    "the modern way",
    "the new standard",
    "designed for developers",
    "developer-first",
    "ai-native",
    "your competitive advantage",
];

/// Pure detector.
#[must_use]
pub fn detect_heading_quality_issues(
    snap: &HeadingQualitySnapshot,
) -> Vec<crate::AxisFinding> {
    let mut out = Vec::new();
    for h in &snap.headings {
        let normalized = h.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            continue;
        }
        // Single-word check: word count == 1 AND not in a
        // common-allowed list (numerals, footnote markers).
        let word_count = normalized.split_whitespace().count();
        if word_count == 1 && !is_single_word_allowed(&normalized) {
            out.push(crate::AxisFinding {
                severity: crate::AxisSeverity::Warn,
                kind: "heading_quality.single-word".to_owned(),
                detail: format!(
                    "<h{}> text {:?} is a single word — SaaS-trope placeholder shape. Expand to an editorial-voice sentence, OR move this to a kicker / eyebrow if it's a category label, not a heading.",
                    h.level, h.text
                ),
            });
        }
        let lower = normalized.to_ascii_lowercase();
        if SAAS_CLICHES.iter().any(|c| *c == lower) {
            out.push(crate::AxisFinding {
                severity: crate::AxisSeverity::Warn,
                kind: "heading_quality.saas-cliche".to_owned(),
                detail: format!(
                    "<h{}> text {:?} is a known SaaS-marketing cliche. The editorial substrate refuses this shape by design — rewrite as something the writer would actually say, or drop the heading entirely.",
                    h.level, h.text
                ),
            });
        }
    }
    out
}

/// Single-word headings that the gate intentionally permits:
/// numerals (chapter / step numbers), legal-doc section labels
/// ("Privacy", "Terms", "Contact" used as anchor-id text).
fn is_single_word_allowed(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_digit())
        || matches!(
            s.to_ascii_lowercase().as_str(),
            "appendix" | "footnotes" | "references" | "bibliography" | "index" | "glossary"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(headings: &[(u32, &str)]) -> HeadingQualitySnapshot {
        HeadingQualitySnapshot {
            page_url: "http://t/".to_owned(),
            headings: headings
                .iter()
                .map(|(l, t)| Heading {
                    level: *l,
                    text: (*t).to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn single_word_flags() {
        let f = detect_heading_quality_issues(&snap(&[(2, "Features")]));
        assert!(f.iter().any(|x| x.kind == "heading_quality.single-word"));
    }

    #[test]
    fn editorial_sentence_does_not_flag() {
        let f = detect_heading_quality_issues(&snap(&[
            (2, "Why insurance is essential for financial well-being."),
        ]));
        assert!(f.is_empty());
    }

    #[test]
    fn saas_cliche_flags() {
        let f = detect_heading_quality_issues(&snap(&[(2, "Get Started")]));
        assert!(f.iter().any(|x| x.kind == "heading_quality.saas-cliche"));
    }

    #[test]
    fn saas_cliche_case_insensitive() {
        let f = detect_heading_quality_issues(&snap(&[(2, "POWERED BY AI")]));
        assert!(f.iter().any(|x| x.kind == "heading_quality.saas-cliche"));
    }

    #[test]
    fn numerals_are_allowed_single_words() {
        let f = detect_heading_quality_issues(&snap(&[(3, "1"), (3, "42")]));
        assert!(f.is_empty());
    }

    #[test]
    fn appendix_style_labels_are_allowed_single_words() {
        let f = detect_heading_quality_issues(&snap(&[
            (2, "Appendix"),
            (2, "References"),
            (2, "Footnotes"),
        ]));
        assert!(f.is_empty());
    }

    #[test]
    fn empty_heading_text_does_not_flag() {
        let f = detect_heading_quality_issues(&snap(&[(2, "")]));
        assert!(f.is_empty());
    }

    #[test]
    fn whitespace_collapsing_normalizes_text() {
        // "  Get   Started  " normalizes to "get started" → cliche match.
        let f = detect_heading_quality_issues(&snap(&[(2, "  Get   Started  ")]));
        assert!(f.iter().any(|x| x.kind == "heading_quality.saas-cliche"));
    }
}
