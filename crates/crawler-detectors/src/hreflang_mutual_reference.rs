//! `hreflang_mutual_reference` — cross-page hreflang reciprocity
//! detector.
//!
//! Sibling axis to `hreflang` (which covers single-page invariants:
//! invalid tags, missing self-reference, duplicate tags, missing
//! `x-default`). This detector covers what `hreflang`'s own module
//! comment explicitly defers to the runner: **reciprocal-back-link
//! checks**.
//!
//! ## The bug class
//!
//! Per Google Search Central + the original W3C i18n note: when
//! page A declares `<link rel="alternate" hreflang="fr" href="B">`,
//! page B SHOULD declare a reciprocal `<link rel="alternate"
//! hreflang="…" href="A">`. Without the back-link the search
//! engine treats the hreflang declaration as one-sided
//! self-promotion and ignores it. Operators ship redesigns where
//! the canonical English page lists every locale variant but the
//! locale variants only list themselves + the canonical page —
//! breaking the matrix.
//!
//! Snapshot shape: the runner walks every page it has captured
//! for a journey + lifts each page's `<link rel="alternate"
//! hreflang>` entries. The detector receives the resulting
//! page-set + per-page hreflang lists and emits findings on every
//! one-way reference that lacks a reciprocal.
//!
//! ## Findings
//!
//! * `hreflang-mutual.one-way-reference` strict — page A links
//!   to page B via hreflang, but page B (which IS in the
//!   snapshot) does not link back to page A.
//! * `hreflang-mutual.unknown-target` warn — page A links via
//!   hreflang to a URL that is NOT present in the captured
//!   page-set. Detector can't determine reciprocity; surfaces the
//!   gap to the operator so they can either include the target
//!   in the journey or remove the reference.
//! * `hreflang-mutual.tag-disagreement` strict — pages A and B
//!   each reference the other, BUT the tag attached to A's link
//!   to B disagrees with B's self-declared language. Operator
//!   error (typo in one direction). Confuses search engines and
//!   AT.
//!
//! Out of scope:
//!
//! * Self-reference and per-page invariants — covered by the
//!   existing `hreflang` detector.
//! * Canonical-vs-hreflang consistency — covered by the
//!   `canonical_url` detector.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; cross-page input is the only
//!   data the detector consumes.
//! * MAX_EXAMPLES = 5 for any per-bucket finding list.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const MAX_EXAMPLES: usize = 5;

/// Per-page record consumed by the detector. The runner builds
/// one of these for each captured page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PageHreflangRecord {
    /// Canonical absolute URL of this page.
    pub page_url: String,
    /// This page's declared language (from `<html lang>` or
    /// `<meta http-equiv="content-language">`; `None` when neither
    /// is present). Used by `tag-disagreement` only.
    pub self_lang: Option<String>,
    /// Every `<link rel="alternate" hreflang="…">` declared on
    /// this page, lifted exactly as captured.
    pub hreflang_links: Vec<HreflangLink>,
}

/// One outbound hreflang link from a page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HreflangLink {
    /// `hreflang=` attribute value (lowercased — `"en"`,
    /// `"en-us"`, `"x-default"`).
    pub tag: String,
    /// `href=` attribute value (absolute URL, runner-resolved).
    pub href: String,
}

/// Cross-page snapshot consumed by the detector.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HreflangMutualReferenceSnapshot {
    /// Journey identifier (URL of the entry point, or operator-
    /// supplied label).
    pub journey_id: String,
    /// Every page the runner captured for this journey + its
    /// hreflang declarations.
    pub pages: Vec<PageHreflangRecord>,
}

/// Detector. Walks the cross-page set, builds a URL → record map,
/// then for each outbound hreflang link checks reciprocity and
/// tag agreement.
#[must_use]
pub fn detect_hreflang_mutual_reference(
    snap: &HreflangMutualReferenceSnapshot,
) -> Vec<AxisFinding> {
    // Map every captured page url → record (normalised).
    let page_index: HashMap<String, &PageHreflangRecord> = snap
        .pages
        .iter()
        .map(|p| (normalise_url(&p.page_url), p))
        .collect();

    let mut one_way: Vec<String> = Vec::new();
    let mut unknown_target: Vec<String> = Vec::new();
    let mut tag_disagreement: Vec<String> = Vec::new();

    for src in &snap.pages {
        for link in &src.hreflang_links {
            // Skip x-default — it is a fallback signal, not a
            // bidirectional cross-language link, so reciprocity
            // doesn't apply.
            if link.tag.eq_ignore_ascii_case("x-default") {
                continue;
            }
            let target_norm = normalise_url(&link.href);
            // Self-references are out of scope (covered by per-
            // page detector).
            if target_norm == normalise_url(&src.page_url) {
                continue;
            }
            let Some(target_record) = page_index.get(&target_norm) else {
                unknown_target.push(format!(
                    "{} -> {} (tag={})",
                    src.page_url, link.href, link.tag
                ));
                continue;
            };
            // Reciprocity: does the target list a back-link to
            // src.page_url?
            let src_norm = normalise_url(&src.page_url);
            let has_back = target_record.hreflang_links.iter().any(|back| {
                normalise_url(&back.href) == src_norm
            });
            if !has_back {
                one_way.push(format!(
                    "{} -> {} (tag={}; back-link missing on target)",
                    src.page_url, link.href, link.tag
                ));
                continue;
            }
            // Tag-disagreement: src says target is tag=link.tag;
            // target's self_lang (if known) should match.
            if let Some(declared) = target_record.self_lang.as_deref() {
                if !lang_tags_compatible(&link.tag, declared) {
                    tag_disagreement.push(format!(
                        "{} -> {} (src claims tag={}, target self-declares lang={})",
                        src.page_url, link.href, link.tag, declared
                    ));
                }
            }
        }
    }

    let mut findings = Vec::new();
    let total_pages = snap.pages.len();

    if !one_way.is_empty() {
        let preview = preview_examples(&one_way.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "hreflang-mutual.one-way-reference".to_owned(),
            detail: format!(
                "{} hreflang link(s) across {} captured page(s) lack a reciprocal back-link on the target. Examples: {}",
                one_way.len(),
                total_pages,
                preview
            ),
        });
    }

    if !tag_disagreement.is_empty() {
        let preview = preview_examples(
            &tag_disagreement.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "hreflang-mutual.tag-disagreement".to_owned(),
            detail: format!(
                "{} hreflang link(s) disagree with the target page's self-declared language. Examples: {}",
                tag_disagreement.len(),
                preview
            ),
        });
    }

    if !unknown_target.is_empty() {
        let preview = preview_examples(
            &unknown_target.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "hreflang-mutual.unknown-target".to_owned(),
            detail: format!(
                "{} hreflang link(s) point at URLs not present in the captured page-set; reciprocity cannot be checked. Examples: {}",
                unknown_target.len(),
                preview
            ),
        });
    }

    findings
}

fn normalise_url(s: &str) -> String {
    // Lowercase + strip trailing slash (other than root). Good
    // enough for the same-host journeys this runner produces; the
    // runner is responsible for upstream canonicalisation.
    let lower = s.trim().to_ascii_lowercase();
    let trimmed = lower.trim_end_matches('/');
    if trimmed.is_empty() {
        lower
    } else {
        trimmed.to_owned()
    }
}

fn lang_tags_compatible(a: &str, b: &str) -> bool {
    // Primary-language token agreement is what search engines
    // actually check; region suffix can vary
    // (`hreflang="en-us"` ↔ `<html lang="en">` is fine).
    let primary = |s: &str| -> String {
        s.split(|c| c == '-' || c == '_')
            .next()
            .unwrap_or(s)
            .to_ascii_lowercase()
    };
    let pa = primary(a);
    let pb = primary(b);
    !pa.is_empty() && !pb.is_empty() && pa == pb
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

/// Sanity-check helper for the runner: returns the set of URLs
/// referenced by hreflang across the page-set. Useful for the
/// runner to decide which pages to add to a journey so unknown-
/// target findings collapse.
#[must_use]
pub fn referenced_urls(snap: &HreflangMutualReferenceSnapshot) -> HashSet<String> {
    let mut out = HashSet::new();
    for p in &snap.pages {
        for l in &p.hreflang_links {
            out.insert(l.href.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(tag: &str, href: &str) -> HreflangLink {
        HreflangLink {
            tag: tag.to_owned(),
            href: href.to_owned(),
        }
    }

    fn page(
        url: &str,
        lang: Option<&str>,
        links: Vec<HreflangLink>,
    ) -> PageHreflangRecord {
        PageHreflangRecord {
            page_url: url.to_owned(),
            self_lang: lang.map(str::to_owned),
            hreflang_links: links,
        }
    }

    fn snap(pages: Vec<PageHreflangRecord>) -> HreflangMutualReferenceSnapshot {
        HreflangMutualReferenceSnapshot {
            journey_id: "test-journey".to_owned(),
            pages,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_hreflang_mutual_reference(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn fully_reciprocal_pair_is_clean() {
        let en = page(
            "https://example.com/en/",
            Some("en"),
            vec![
                link("en", "https://example.com/en/"),
                link("fr", "https://example.com/fr/"),
            ],
        );
        let fr = page(
            "https://example.com/fr/",
            Some("fr"),
            vec![
                link("en", "https://example.com/en/"),
                link("fr", "https://example.com/fr/"),
            ],
        );
        let f = detect_hreflang_mutual_reference(&snap(vec![en, fr]));
        assert!(f.is_empty(), "expected clean, got {f:?}");
    }

    #[test]
    fn one_way_reference_is_strict() {
        // EN points at FR; FR only lists itself.
        let en = page(
            "https://example.com/en/",
            Some("en"),
            vec![
                link("en", "https://example.com/en/"),
                link("fr", "https://example.com/fr/"),
            ],
        );
        let fr = page(
            "https://example.com/fr/",
            Some("fr"),
            vec![link("fr", "https://example.com/fr/")],
        );
        let f = detect_hreflang_mutual_reference(&snap(vec![en, fr]));
        let hit = f
            .iter()
            .find(|x| x.kind == "hreflang-mutual.one-way-reference")
            .expect("one-way finding expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("https://example.com/en/"));
        assert!(hit.detail.contains("https://example.com/fr/"));
        assert!(hit.detail.contains("back-link missing"));
    }

    #[test]
    fn unknown_target_is_warn() {
        // EN points at DE; DE not in page-set.
        let en = page(
            "https://example.com/en/",
            Some("en"),
            vec![
                link("en", "https://example.com/en/"),
                link("de", "https://example.com/de/"),
            ],
        );
        let f = detect_hreflang_mutual_reference(&snap(vec![en]));
        let hit = f
            .iter()
            .find(|x| x.kind == "hreflang-mutual.unknown-target")
            .expect("unknown-target finding expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("https://example.com/de/"));
    }

    #[test]
    fn tag_disagreement_is_strict() {
        // EN claims FR target is "de", but FR self-declares "fr".
        let en = page(
            "https://example.com/en/",
            Some("en"),
            vec![
                link("en", "https://example.com/en/"),
                link("de", "https://example.com/fr/"),
            ],
        );
        let fr = page(
            "https://example.com/fr/",
            Some("fr"),
            vec![
                link("en", "https://example.com/en/"),
                link("de", "https://example.com/fr/"),
            ],
        );
        let f = detect_hreflang_mutual_reference(&snap(vec![en, fr]));
        let hit = f
            .iter()
            .find(|x| x.kind == "hreflang-mutual.tag-disagreement")
            .expect("tag-disagreement finding expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("tag=de"));
        assert!(hit.detail.contains("lang=fr"));
    }

    #[test]
    fn region_variant_does_not_count_as_tag_disagreement() {
        // EN claims target is "en-us"; target self-declares "en".
        // Primary-language token matches — should NOT flag.
        let canonical = page(
            "https://example.com/",
            Some("en"),
            vec![
                link("en-us", "https://example.com/us/"),
                link("en", "https://example.com/"),
            ],
        );
        let us = page(
            "https://example.com/us/",
            Some("en"),
            vec![
                link("en-us", "https://example.com/us/"),
                link("en", "https://example.com/"),
            ],
        );
        let f = detect_hreflang_mutual_reference(&snap(vec![canonical, us]));
        assert!(
            !f.iter()
                .any(|x| x.kind == "hreflang-mutual.tag-disagreement"),
            "region variant should NOT trigger tag-disagreement: {f:?}"
        );
    }

    #[test]
    fn x_default_does_not_require_reciprocity() {
        let en = page(
            "https://example.com/en/",
            Some("en"),
            vec![
                link("en", "https://example.com/en/"),
                link("x-default", "https://example.com/"),
            ],
        );
        let root = page(
            "https://example.com/",
            Some("en"),
            vec![link("en", "https://example.com/en/")],
        );
        let f = detect_hreflang_mutual_reference(&snap(vec![en, root]));
        // x-default link is exempt; one direct hreflang reference
        // EN -> root has no back-link. Root -> EN exists. So no
        // one-way violation.
        assert!(f.is_empty(), "expected clean, got {f:?}");
    }

    #[test]
    fn url_normalisation_handles_trailing_slash() {
        let a = page(
            "https://example.com/en",
            Some("en"),
            vec![
                link("en", "https://example.com/en"),
                link("fr", "https://example.com/fr/"),
            ],
        );
        let b = page(
            "https://example.com/fr/",
            Some("fr"),
            vec![
                // Note: missing trailing slash on the back-link.
                link("en", "https://example.com/en/"),
                link("fr", "https://example.com/fr"),
            ],
        );
        let f = detect_hreflang_mutual_reference(&snap(vec![a, b]));
        assert!(f.is_empty(), "trailing-slash variants should normalise: {f:?}");
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let mut pages = Vec::new();
        for i in 0..8 {
            pages.push(page(
                &format!("https://example.com/p{i}/"),
                Some("en"),
                vec![
                    link("en", &format!("https://example.com/p{i}/")),
                    link("fr", "https://example.com/no-back-link/"),
                ],
            ));
        }
        let f = detect_hreflang_mutual_reference(&snap(pages));
        let hit = f
            .iter()
            .find(|x| x.kind == "hreflang-mutual.unknown-target")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn referenced_urls_returns_all_hrefs() {
        let p = page(
            "https://example.com/en/",
            Some("en"),
            vec![
                link("fr", "https://example.com/fr/"),
                link("x-default", "https://example.com/"),
            ],
        );
        let urls = referenced_urls(&snap(vec![p]));
        assert!(urls.contains("https://example.com/fr/"));
        assert!(urls.contains("https://example.com/"));
        assert_eq!(urls.len(), 2);
    }
}
