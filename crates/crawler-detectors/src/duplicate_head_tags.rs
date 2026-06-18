//! `duplicate_head_tags` — flags `<head>` tags that should be
//! singletons but appear more than once.
//!
//! HTML allows multiple of these tags syntactically, but each
//! has "first one wins" semantics. The presence of duplicates
//! indicates a build-system bug — usually a page-template +
//! layout-template double-emission that nobody noticed because
//! browsers/crawlers silently use the first match.
//!
//! Three classes:
//!
//! 1. **`<title>`** (Strict). Each page should have exactly
//!    one `<title>`. Multiple breaks SEO tooling, social-share
//!    expectations, and screen reader landmark semantics.
//!
//! 2. **`<link rel="canonical">`** (Strict). Per Google
//!    Search Central: "If we find more than one canonical link
//!    element for a given page, we ignore all of them." So
//!    duplicates silently break canonicalization → could
//!    cause duplicate-content indexing.
//!
//! 3. **`<meta name="description">`** (Warn). Google takes
//!    the first one. Less severe than canonical because the
//!    description only affects search-snippet display, not
//!    canonicalization.
//!
//! Out of scope: multiple `<link rel="stylesheet">` (legit
//! pattern), multiple `<meta property="og:image">` (Open
//! Graph permits multiples for fallback), multiple `<script>`
//! (obviously legit).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured duplicated tag kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DuplicateHeadTagHit {
    /// Tag kind — one of `"title"`, `"meta-description"`,
    /// `"link-canonical"`.
    pub tag_kind: String,
    /// Total count of tags found (always ≥ 2 to reach here).
    pub count: u32,
    /// First few content values (capped at 5, each 120 chars)
    /// for diff-style context — when duplicates carry the
    /// same value the operator's bug is "template emitted
    /// twice"; when values differ, it's "intentional emission
    /// + accidental emission stepping on each other."
    pub values: Vec<String>,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DuplicateHeadTagsSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Duplicate hits (already pre-filtered to count ≥ 2).
    pub hits: Vec<DuplicateHeadTagHit>,
    /// Total `<head>` children walked.
    pub scanned_head_children: u32,
}

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_duplicate_head_tags(snap: &DuplicateHeadTagsSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for h in &snap.hits {
        let values_str = h.values.join(" / ");
        let (severity, kind, detail) = match h.tag_kind.as_str() {
            "title" => (
                AxisSeverity::Strict,
                "duplicate-head-tags.title".to_owned(),
                format!(
                    "Found {} `<title>` elements in <head>. Each page should have exactly one. Browsers + SEO tooling use the first; the others confuse landmark semantics. Values: {}",
                    h.count, values_str
                ),
            ),
            "link-canonical" => (
                AxisSeverity::Strict,
                "duplicate-head-tags.link-canonical".to_owned(),
                format!(
                    "Found {} `<link rel=\"canonical\">` elements. Per Google Search Central: 'If we find more than one canonical link element for a given page, we ignore all of them.' Duplicates silently break canonicalization. Values: {}",
                    h.count, values_str
                ),
            ),
            "meta-description" => (
                AxisSeverity::Warn,
                "duplicate-head-tags.meta-description".to_owned(),
                format!(
                    "Found {} `<meta name=\"description\">` elements. Google takes the first; the others waste bytes + obscure build-system bugs. Values: {}",
                    h.count, values_str
                ),
            ),
            _ => continue, // defensive: unknown kinds dropped
        };
        out.push(AxisFinding {
            severity,
            kind,
            detail,
        });
    }
    out
}

/// Browser-side DOM-capture script. Counts singletons-that-
/// should-be-singletons and emits hits when count ≥ 2.
pub const DUPLICATE_HEAD_TAGS_DOM_CAPTURE_JS: &str = r#"
(() => {
    const hits = [];
    const head = document.head;
    const headChildren = head ? head.children.length : 0;

    const collect = function(tagKind, selector, valueAttr) {
      const nodes = head ? head.querySelectorAll(selector) : [];
      if (nodes.length < 2) return;
      const values = [];
      let i = 0;
      for (const n of nodes) {
        if (i >= 5) break;
        if (valueAttr === '__text__') {
          values.push((n.textContent || '').trim().substring(0, 120));
        } else {
          values.push((n.getAttribute(valueAttr) || '').trim().substring(0, 120));
        }
        i += 1;
      }
      hits.push({
        tagKind: tagKind,
        count: nodes.length,
        values: values
      });
    };

    collect('title', 'title', '__text__');
    collect('link-canonical', 'link[rel="canonical"]', 'href');
    collect('meta-description', 'meta[name="description"]', 'content');

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedHeadChildren: headChildren
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(tag_kind: &str, count: u32, values: Vec<&str>) -> DuplicateHeadTagHit {
        DuplicateHeadTagHit {
            tag_kind: tag_kind.into(),
            count,
            values: values.iter().map(|v| (*v).into()).collect(),
        }
    }

    fn snap(hits: Vec<DuplicateHeadTagHit>) -> DuplicateHeadTagsSnapshot {
        DuplicateHeadTagsSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_head_children: 20,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_duplicate_head_tags(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn duplicate_title_is_strict() {
        let s = snap(vec![hit("title", 2, vec!["Page A", "Page A — site"])]);
        let findings = detect_duplicate_head_tags(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "duplicate-head-tags.title");
        assert!(findings[0].detail.contains("2"));
        assert!(findings[0].detail.contains("Page A"));
    }

    #[test]
    fn duplicate_link_canonical_is_strict() {
        let s = snap(vec![hit(
            "link-canonical",
            3,
            vec!["/a", "/b", "/c"],
        )]);
        let findings = detect_duplicate_head_tags(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "duplicate-head-tags.link-canonical");
        assert!(findings[0].detail.contains("Google Search Central"));
        assert!(findings[0].detail.contains("/a / /b / /c"));
    }

    #[test]
    fn duplicate_meta_description_is_warn() {
        let s = snap(vec![hit(
            "meta-description",
            2,
            vec!["First desc", "Second desc"],
        )]);
        let findings = detect_duplicate_head_tags(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "duplicate-head-tags.meta-description");
        assert!(findings[0].detail.contains("Google takes the first"));
    }

    #[test]
    fn multiple_kinds_emit_separate_findings() {
        let s = snap(vec![
            hit("title", 2, vec!["A", "B"]),
            hit("link-canonical", 2, vec!["/x", "/y"]),
            hit("meta-description", 2, vec!["d1", "d2"]),
        ]);
        let findings = detect_duplicate_head_tags(&s);
        assert_eq!(findings.len(), 3);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"duplicate-head-tags.title"));
        assert!(kinds.contains(&"duplicate-head-tags.link-canonical"));
        assert!(kinds.contains(&"duplicate-head-tags.meta-description"));
    }

    #[test]
    fn unknown_tag_kind_ignored_defensively() {
        let s = snap(vec![hit("future-tag", 2, vec!["x", "y"])]);
        let findings = detect_duplicate_head_tags(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(DUPLICATE_HEAD_TAGS_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(DUPLICATE_HEAD_TAGS_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(DUPLICATE_HEAD_TAGS_DOM_CAPTURE_JS.contains("hits"));
        assert!(DUPLICATE_HEAD_TAGS_DOM_CAPTURE_JS.contains("scannedHeadChildren"));
        assert!(DUPLICATE_HEAD_TAGS_DOM_CAPTURE_JS.contains("tagKind"));
        // Three tag-kind strings.
        assert!(DUPLICATE_HEAD_TAGS_DOM_CAPTURE_JS.contains("'title'"));
        assert!(DUPLICATE_HEAD_TAGS_DOM_CAPTURE_JS.contains("'link-canonical'"));
        assert!(DUPLICATE_HEAD_TAGS_DOM_CAPTURE_JS.contains("'meta-description'"));
        // Selector contracts.
        assert!(DUPLICATE_HEAD_TAGS_DOM_CAPTURE_JS.contains("'link[rel=\"canonical\"]'"));
        assert!(DUPLICATE_HEAD_TAGS_DOM_CAPTURE_JS.contains("'meta[name=\"description\"]'"));
        // textContent marker for titles.
        assert!(DUPLICATE_HEAD_TAGS_DOM_CAPTURE_JS.contains("'__text__'"));
    }
}
