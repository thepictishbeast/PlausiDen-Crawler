//! `cross_page_title` — aggregates-layer detector for duplicate
//! `<title>` tags across pages in a single journey.
//!
//! Mirror of `src/crossPageTitle.ts`. One finding kind:
//!
//!   * `title.cross-page-dup` warn — 2+ pages share the same `<title>`
//!
//! Unlike per-page detectors (which run on each goto and produce
//! findings from one page's snapshot), aggregates detectors run
//! AFTER the journey completes and walk accumulated cross-page
//! state. Maintains an ordered list of (url, title) records; at
//! report time, groups by title and emits one finding per group
//! that spans ≥2 distinct URL pathnames.
//!
//! URL dedup is by `origin + pathname` so the same logical page
//! visited with different query strings (theme switch, density
//! switch) doesn't count as multiple URLs sharing a title.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Ordered list of (url, title) records the crawler accumulates as
/// it walks a journey. Caller pushes each page's trimmed title via
/// [`record_page_title`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CrossPageTitleAccumulator {
    /// One entry per page visit, in journey order. Empty titles
    /// are filtered at record time (the per-page title.empty
    /// detector handles that case better).
    pub entries: Vec<CrossPageTitleEntry>,
}

/// One recorded (URL, title) pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CrossPageTitleEntry {
    /// URL as visited (preserves query string for evidence).
    pub url: String,
    /// Trimmed `<title>` text.
    pub title: String,
}

/// Construct an empty accumulator.
#[must_use]
pub fn new_cross_page_title_accumulator() -> CrossPageTitleAccumulator {
    CrossPageTitleAccumulator::default()
}

/// Record a page's title. Caller should pass the trimmed title.
/// Empty titles are dropped.
pub fn record_page_title(acc: &mut CrossPageTitleAccumulator, url: &str, title: &str) {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return;
    }
    acc.entries.push(CrossPageTitleEntry {
        url: url.to_owned(),
        title: trimmed.to_owned(),
    });
}

/// Best-effort `origin + pathname` extraction. Used for URL dedup
/// so theme/density query-string variants count as the same page.
/// Falls back to the raw URL on parse failure.
fn path_key(u: &str) -> String {
    if let Some(scheme_end) = u.find("://") {
        let after_scheme = &u[scheme_end + 3..];
        // Find the path start (first '/' after host).
        if let Some(path_start) = after_scheme.find('/') {
            // Strip query + fragment.
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
        // No path — origin only, pathname is implicitly "/"
        return format!("{}://{}/", &u[..scheme_end], after_scheme);
    }
    u.to_owned()
}

/// Walk the accumulator. For each title appearing on ≥2 distinct
/// URL pathnames, emit one warn finding listing up to 5 URLs.
#[must_use]
pub fn detect_cross_page_title_duplicates(acc: &CrossPageTitleAccumulator) -> Vec<AxisFinding> {
    // Group URLs by title. BTreeMap inside BTreeMap → stable
    // iteration order = deterministic output.
    let mut groups: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for entry in &acc.entries {
        let urls = groups.entry(entry.title.clone()).or_default();
        let k = path_key(&entry.url);
        urls.entry(k).or_insert_with(|| entry.url.clone());
    }

    let mut out = Vec::new();
    for (title, urls) in groups {
        if urls.len() < 2 {
            continue;
        }
        let url_list: Vec<String> = urls.into_values().collect();
        let preview: Vec<&str> = url_list.iter().take(5).map(String::as_str).collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "title.cross-page-dup".into(),
            detail: format!(
                "{} distinct URL(s) share the same <title> '{}'. SEO suffers (Google often filters duplicate-title results) and users can't tell open tabs / bookmarks / history entries apart. Set a unique, page-specific title per route. URLs: {}",
                url_list.len(),
                title,
                preview.join("; ")
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
        let acc = new_cross_page_title_accumulator();
        assert!(detect_cross_page_title_duplicates(&acc).is_empty());
    }

    #[test]
    fn distinct_titles_no_findings() {
        let mut acc = new_cross_page_title_accumulator();
        record_page_title(&mut acc, "https://example.com/", "Home");
        record_page_title(&mut acc, "https://example.com/about", "About");
        record_page_title(&mut acc, "https://example.com/pricing", "Pricing");
        assert!(detect_cross_page_title_duplicates(&acc).is_empty());
    }

    #[test]
    fn duplicate_title_across_distinct_paths_flags() {
        let mut acc = new_cross_page_title_accumulator();
        record_page_title(&mut acc, "https://example.com/", "Untitled");
        record_page_title(&mut acc, "https://example.com/about", "Untitled");
        let f = detect_cross_page_title_duplicates(&acc);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "title.cross-page-dup");
        assert!(f[0].detail.contains("Untitled"));
    }

    #[test]
    fn same_path_with_different_query_strings_dedupes() {
        // Theme switch / density switch on the same page → ONE
        // logical URL, not multiple sharing a title.
        let mut acc = new_cross_page_title_accumulator();
        record_page_title(&mut acc, "https://example.com/?theme=dark", "Home");
        record_page_title(&mut acc, "https://example.com/?theme=light", "Home");
        record_page_title(&mut acc, "https://example.com/?density=compact", "Home");
        assert!(detect_cross_page_title_duplicates(&acc).is_empty());
    }

    #[test]
    fn empty_titles_dropped_at_record_time() {
        let mut acc = new_cross_page_title_accumulator();
        record_page_title(&mut acc, "https://example.com/a", "");
        record_page_title(&mut acc, "https://example.com/b", "   ");
        record_page_title(&mut acc, "https://example.com/c", "\t\n");
        assert_eq!(acc.entries.len(), 0);
    }

    #[test]
    fn title_is_trimmed_at_record_time() {
        let mut acc = new_cross_page_title_accumulator();
        record_page_title(&mut acc, "https://example.com/a", "  Hello  ");
        assert_eq!(acc.entries[0].title, "Hello");
    }

    #[test]
    fn multiple_duplicate_groups_each_get_own_finding() {
        let mut acc = new_cross_page_title_accumulator();
        record_page_title(&mut acc, "https://example.com/a", "Group A");
        record_page_title(&mut acc, "https://example.com/b", "Group A");
        record_page_title(&mut acc, "https://example.com/c", "Group B");
        record_page_title(&mut acc, "https://example.com/d", "Group B");
        let f = detect_cross_page_title_duplicates(&acc);
        assert_eq!(f.len(), 2);
        assert!(f.iter().any(|x| x.detail.contains("Group A")));
        assert!(f.iter().any(|x| x.detail.contains("Group B")));
    }

    #[test]
    fn unparseable_url_still_groups_by_raw_string() {
        let mut acc = new_cross_page_title_accumulator();
        record_page_title(&mut acc, "weird-url-no-scheme", "Same");
        record_page_title(&mut acc, "another-weird-url", "Same");
        let f = detect_cross_page_title_duplicates(&acc);
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn three_or_more_urls_show_up_in_count() {
        let mut acc = new_cross_page_title_accumulator();
        for p in ["a", "b", "c", "d", "e", "f"] {
            record_page_title(&mut acc, &format!("https://example.com/{p}"), "Shared");
        }
        let f = detect_cross_page_title_duplicates(&acc);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("6 distinct URL(s)"));
    }

    #[test]
    fn cross_origin_same_title_counts_as_distinct() {
        // Same title across origins should NOT dedupe even if
        // the pathname matches — different origins = different sites.
        let mut acc = new_cross_page_title_accumulator();
        record_page_title(&mut acc, "https://example.com/", "Home");
        record_page_title(&mut acc, "https://other.com/", "Home");
        let f = detect_cross_page_title_duplicates(&acc);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("2 distinct URL(s)"));
    }
}
