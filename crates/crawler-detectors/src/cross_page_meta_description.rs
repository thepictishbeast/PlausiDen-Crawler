//! `cross_page_meta_description` — aggregates-layer detector for
//! duplicate `<meta name="description">` content across pages.
//!
//! Mirror of `src/crossPageMetaDescription.ts`. Sister to
//! `cross_page_title`. One finding kind:
//!
//!   * `meta-description.cross-page-dup` warn
//!
//! Real-world impact: Google filters near-duplicate descriptions in
//! search results — affected pages become effectively invisible.
//! Social-share preview cards collapse.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Ordered list of (url, description) records the crawler
/// accumulates as it walks a journey.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CrossPageMetaDescriptionAccumulator {
    /// One entry per page visit, in journey order. Empty
    /// descriptions are filtered at record time.
    pub entries: Vec<CrossPageMetaDescriptionEntry>,
}

/// One recorded (URL, description) pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CrossPageMetaDescriptionEntry {
    /// URL as visited (preserves query string for evidence).
    pub url: String,
    /// Trimmed meta-description content.
    pub description: String,
}

/// Construct an empty accumulator.
#[must_use]
pub fn new_cross_page_meta_description_accumulator() -> CrossPageMetaDescriptionAccumulator {
    CrossPageMetaDescriptionAccumulator::default()
}

/// Record a page's description. Caller passes the raw text;
/// this function trims and drops empty descriptions.
pub fn record_page_meta_description(
    acc: &mut CrossPageMetaDescriptionAccumulator,
    url: &str,
    description: &str,
) {
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return;
    }
    acc.entries.push(CrossPageMetaDescriptionEntry {
        url: url.to_owned(),
        description: trimmed.to_owned(),
    });
}

/// Best-effort `origin + pathname` extraction. Sibling to the same
/// helper in `cross_page_title`; both are kept local until a third
/// aggregates detector lands and earns a shared helper extraction.
fn path_key(u: &str) -> String {
    if let Some(scheme_end) = u.find("://") {
        let after_scheme = &u[scheme_end + 3..];
        if let Some(path_start) = after_scheme.find('/') {
            let path_only = after_scheme[path_start..]
                .split_once('?')
                .map(|(p, _)| p)
                .unwrap_or(&after_scheme[path_start..])
                .split_once('#')
                .map(|(p, _)| p)
                .unwrap_or_else(|| {
                    after_scheme[path_start..]
                        .split_once('?')
                        .map(|(p, _)| p)
                        .unwrap_or(&after_scheme[path_start..])
                });
            let host = &after_scheme[..path_start];
            return format!("{}://{}{}", &u[..scheme_end], host, path_only);
        }
        return format!("{}://{}/", &u[..scheme_end], after_scheme);
    }
    u.to_owned()
}

/// Truncate description preview at 80 chars + ellipsis. Real meta
/// descriptions are commonly 150+ chars so full inclusion bloats
/// the audit detail.
fn preview(description: &str) -> String {
    let s: String = description.chars().take(80).collect();
    if description.chars().count() > 80 {
        format!("{s}…")
    } else {
        s
    }
}

/// Walk the accumulator. For each description appearing on ≥2
/// distinct URL pathnames, emit one warn finding.
#[must_use]
pub fn detect_cross_page_meta_description_duplicates(
    acc: &CrossPageMetaDescriptionAccumulator,
) -> Vec<AxisFinding> {
    let mut groups: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for entry in &acc.entries {
        let urls = groups.entry(entry.description.clone()).or_default();
        let k = path_key(&entry.url);
        urls.entry(k).or_insert_with(|| entry.url.clone());
    }

    let mut out = Vec::new();
    for (description, urls) in groups {
        if urls.len() < 2 {
            continue;
        }
        let url_list: Vec<String> = urls.into_values().collect();
        let url_preview: Vec<&str> = url_list.iter().take(5).map(String::as_str).collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "meta-description.cross-page-dup".into(),
            detail: format!(
                "{} distinct URL(s) share the same <meta name=\"description\"> content '{}'. Google filters near-duplicate descriptions in search results — affected pages become effectively invisible. Social-share preview cards collapse. Set a unique, page-specific description per route. URLs: {}",
                url_list.len(),
                preview(&description),
                url_preview.join("; ")
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_accumulator_no_findings() {
        let acc = new_cross_page_meta_description_accumulator();
        assert!(detect_cross_page_meta_description_duplicates(&acc).is_empty());
    }

    #[test]
    fn distinct_descriptions_no_findings() {
        let mut acc = new_cross_page_meta_description_accumulator();
        record_page_meta_description(&mut acc, "https://example.com/", "Home page");
        record_page_meta_description(&mut acc, "https://example.com/about", "About page");
        assert!(detect_cross_page_meta_description_duplicates(&acc).is_empty());
    }

    #[test]
    fn duplicate_descriptions_flag() {
        let mut acc = new_cross_page_meta_description_accumulator();
        record_page_meta_description(&mut acc, "https://example.com/", "Lorem ipsum");
        record_page_meta_description(&mut acc, "https://example.com/about", "Lorem ipsum");
        let f = detect_cross_page_meta_description_duplicates(&acc);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "meta-description.cross-page-dup");
        assert!(f[0].detail.contains("Lorem ipsum"));
    }

    #[test]
    fn query_string_variants_dedupe() {
        let mut acc = new_cross_page_meta_description_accumulator();
        record_page_meta_description(&mut acc, "https://example.com/?theme=dark", "Same page");
        record_page_meta_description(&mut acc, "https://example.com/?theme=light", "Same page");
        assert!(detect_cross_page_meta_description_duplicates(&acc).is_empty());
    }

    #[test]
    fn empty_descriptions_dropped_at_record_time() {
        let mut acc = new_cross_page_meta_description_accumulator();
        record_page_meta_description(&mut acc, "https://example.com/a", "");
        record_page_meta_description(&mut acc, "https://example.com/b", "   ");
        assert_eq!(acc.entries.len(), 0);
    }

    #[test]
    fn description_is_trimmed() {
        let mut acc = new_cross_page_meta_description_accumulator();
        record_page_meta_description(&mut acc, "https://example.com/", "  Lots of whitespace  ");
        assert_eq!(acc.entries[0].description, "Lots of whitespace");
    }

    #[test]
    fn long_description_truncated_in_preview() {
        let long = "a".repeat(200);
        let mut acc = new_cross_page_meta_description_accumulator();
        record_page_meta_description(&mut acc, "https://example.com/x", &long);
        record_page_meta_description(&mut acc, "https://example.com/y", &long);
        let f = detect_cross_page_meta_description_duplicates(&acc);
        // 80 'a's + ellipsis appears in detail; the full 200 doesn't
        assert!(f[0].detail.contains("…"));
        assert!(!f[0].detail.contains(&long));
    }

    #[test]
    fn multiple_duplicate_groups_each_get_own_finding() {
        let mut acc = new_cross_page_meta_description_accumulator();
        record_page_meta_description(&mut acc, "https://example.com/a", "Group A");
        record_page_meta_description(&mut acc, "https://example.com/b", "Group A");
        record_page_meta_description(&mut acc, "https://example.com/c", "Group B");
        record_page_meta_description(&mut acc, "https://example.com/d", "Group B");
        let f = detect_cross_page_meta_description_duplicates(&acc);
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn count_reflects_distinct_urls() {
        let mut acc = new_cross_page_meta_description_accumulator();
        for p in ["a", "b", "c"] {
            record_page_meta_description(&mut acc, &format!("https://example.com/{p}"), "Same");
        }
        let f = detect_cross_page_meta_description_duplicates(&acc);
        assert!(f[0].detail.contains("3 distinct URL(s)"));
    }

    #[test]
    fn unparseable_url_falls_back_to_raw_string() {
        let mut acc = new_cross_page_meta_description_accumulator();
        record_page_meta_description(&mut acc, "raw-string-1", "Same");
        record_page_meta_description(&mut acc, "raw-string-2", "Same");
        let f = detect_cross_page_meta_description_duplicates(&acc);
        assert_eq!(f.len(), 1);
    }
}
