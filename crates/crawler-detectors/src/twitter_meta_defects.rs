//! `twitter_meta_defects` — flags broken Twitter Card meta tags.
//!
//! Twitter Cards use `<meta name="twitter:…">` not `<meta
//! property="twitter:…">`. Open Graph uses `property=`, and a
//! common operator copy-paste bug is using `property=` for
//! both. The result: Twitter silently ignores the tag and the
//! preview falls back to whatever Open Graph / generic
//! defaults can resolve, usually with the wrong image / wrong
//! card type / missing summary.
//!
//! Three defect classes:
//!
//! 1. **Wrong attribute** (Strict). `<meta property="twitter:X"
//!    content="…">`. Twitter ignores; defect silently breaks
//!    the preview. THE most common bug in this space.
//!
//! 2. **Empty content** (Warn). `<meta name="twitter:X"
//!    content="">`. Valid syntax but no signal; the empty
//!    string just costs bytes.
//!
//! 3. **Card type without required fields** (Warn).
//!    `<meta name="twitter:card" content="summary_large_image">`
//!    without a paired `twitter:image`. Twitter falls back to
//!    `summary` card; the operator probably wanted the rich
//!    preview.
//!
//! Out of scope: validating that `twitter:image` URLs resolve
//! (needs network) — flag the structural defect only.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending Twitter meta tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TwitterMetaDefectHit {
    /// CSS-ish path of the offending `<meta>`.
    pub selector: String,
    /// The `twitter:*` key (e.g. `"twitter:card"`,
    /// `"twitter:image"`, `"twitter:title"`).
    pub key: String,
    /// The `content` value, capped at 120 chars.
    pub content: String,
    /// Defect kind — one of `"wrong-attribute"`,
    /// `"empty-content"`, `"large-image-card-without-image"`.
    pub defect_kind: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TwitterMetaDefectSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Offending Twitter meta tags.
    pub hits: Vec<TwitterMetaDefectHit>,
    /// Total `<meta>` elements walked.
    pub scanned_meta: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_twitter_meta_defects(snap: &TwitterMetaDefectSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut wrong_attr: Vec<&TwitterMetaDefectHit> = Vec::new();
    let mut empty_content: Vec<&TwitterMetaDefectHit> = Vec::new();
    let mut large_no_image: Vec<&TwitterMetaDefectHit> = Vec::new();
    for h in &snap.hits {
        match h.defect_kind.as_str() {
            "wrong-attribute" => wrong_attr.push(h),
            "empty-content" => empty_content.push(h),
            "large-image-card-without-image" => large_no_image.push(h),
            _ => {} // defensive: unknown kinds dropped
        }
    }

    let format_example = |h: &TwitterMetaDefectHit| -> String {
        format!("{} key=`{}` content=`{}`", h.selector, h.key, h.content)
    };

    let mut out = Vec::new();
    if !wrong_attr.is_empty() {
        let examples: Vec<String> = wrong_attr
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "twitter-meta.wrong-attribute".to_owned(),
            detail: format!(
                "{} Twitter Card meta tag(s) using `property=` instead of `name=` — Twitter silently ignores these. Replace `<meta property=\"twitter:X\">` with `<meta name=\"twitter:X\">`. Open Graph uses `property=` — Twitter Cards do not. Examples: {}",
                wrong_attr.len(),
                examples.join("; ")
            ),
        });
    }
    if !large_no_image.is_empty() {
        let examples: Vec<String> = large_no_image
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "twitter-meta.large-image-card-without-image".to_owned(),
            detail: format!(
                "{} page(s) declare `twitter:card=\"summary_large_image\"` but no `twitter:image` — Twitter falls back to plain `summary` card. Add `<meta name=\"twitter:image\" content=\"…\">` or downgrade the card declaration. Examples: {}",
                large_no_image.len(),
                examples.join("; ")
            ),
        });
    }
    if !empty_content.is_empty() {
        let examples: Vec<String> = empty_content
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "twitter-meta.empty-content".to_owned(),
            detail: format!(
                "{} `twitter:*` meta tag(s) with empty `content=` — wasted bytes that provide no signal. Either populate the field or remove the tag. Examples: {}",
                empty_content.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks `<meta>` elements,
/// captures `twitter:*` tags written with the wrong attribute
/// + correct-attribute tags with empty content + the missing-
/// image-for-large-card defect.
pub const TWITTER_META_DEFECTS_DOM_CAPTURE_JS: &str = r#"
(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.head && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const same = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (same.length > 1) parts.unshift(tag + ':nth-of-type(' + (same.indexOf(node) + 1) + ')');
          else parts.unshift(tag);
        } else parts.unshift(tag);
        node = parent;
        depth += 1;
      }
      return 'head > ' + parts.join(' > ');
    };

    const hits = [];
    let scanned = 0;
    const allMeta = document.querySelectorAll('head meta');
    // Gather correct-attribute twitter:* tags for the large-image
    // cross-check + a quick `twitter:image` presence flag.
    let hasTwitterImage = false;
    let largeCardSelector = null;
    let largeCardContent = '';
    for (const m of allMeta) {
      scanned += 1;
      const propAttr = (m.getAttribute('property') || '').trim().toLowerCase();
      const nameAttr = (m.getAttribute('name') || '').trim().toLowerCase();
      const content = (m.getAttribute('content') || '').trim();

      // Wrong attribute: property="twitter:…".
      if (propAttr.startsWith('twitter:')) {
        hits.push({
          selector: selectorOf(m),
          key: propAttr,
          content: content.substring(0, 120),
          defectKind: 'wrong-attribute'
        });
        continue;
      }

      // Correct-attribute twitter:*.
      if (nameAttr.startsWith('twitter:')) {
        // Empty content — Warn.
        if (content === '') {
          hits.push({
            selector: selectorOf(m),
            key: nameAttr,
            content: '',
            defectKind: 'empty-content'
          });
          continue;
        }
        if (nameAttr === 'twitter:image') {
          hasTwitterImage = true;
        }
        if (nameAttr === 'twitter:card' && content.toLowerCase() === 'summary_large_image') {
          largeCardSelector = selectorOf(m);
          largeCardContent = content;
        }
      }
    }
    // Cross-check: large image card declared without an image.
    if (largeCardSelector !== null && !hasTwitterImage) {
      hits.push({
        selector: largeCardSelector,
        key: 'twitter:card',
        content: largeCardContent.substring(0, 120),
        defectKind: 'large-image-card-without-image'
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedMeta: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(selector: &str, key: &str, content: &str, defect_kind: &str) -> TwitterMetaDefectHit {
        TwitterMetaDefectHit {
            selector: selector.into(),
            key: key.into(),
            content: content.into(),
            defect_kind: defect_kind.into(),
        }
    }

    fn snap(hits: Vec<TwitterMetaDefectHit>) -> TwitterMetaDefectSnapshot {
        TwitterMetaDefectSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_meta: 20,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_twitter_meta_defects(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn wrong_attribute_is_strict() {
        let s = snap(vec![hit(
            "head > meta:nth-of-type(3)",
            "twitter:card",
            "summary_large_image",
            "wrong-attribute",
        )]);
        let findings = detect_twitter_meta_defects(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "twitter-meta.wrong-attribute");
        assert!(findings[0].detail.contains("property="));
        assert!(findings[0].detail.contains("twitter:card"));
    }

    #[test]
    fn empty_content_is_warn() {
        let s = snap(vec![hit(
            "head > meta:nth-of-type(5)",
            "twitter:description",
            "",
            "empty-content",
        )]);
        let findings = detect_twitter_meta_defects(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "twitter-meta.empty-content");
    }

    #[test]
    fn large_card_without_image_is_warn() {
        let s = snap(vec![hit(
            "head > meta:nth-of-type(2)",
            "twitter:card",
            "summary_large_image",
            "large-image-card-without-image",
        )]);
        let findings = detect_twitter_meta_defects(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(
            findings[0].kind,
            "twitter-meta.large-image-card-without-image"
        );
        assert!(findings[0].detail.contains("falls back to plain `summary`"));
    }

    #[test]
    fn all_three_defects_emit_three_findings() {
        let s = snap(vec![
            hit(".a", "twitter:card", "summary_large_image", "wrong-attribute"),
            hit(".b", "twitter:description", "", "empty-content"),
            hit(".c", "twitter:card", "summary_large_image", "large-image-card-without-image"),
        ]);
        let findings = detect_twitter_meta_defects(&s);
        assert_eq!(findings.len(), 3);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"twitter-meta.wrong-attribute"));
        assert!(kinds.contains(&"twitter-meta.empty-content"));
        assert!(kinds.contains(&"twitter-meta.large-image-card-without-image"));
    }

    #[test]
    fn unknown_defect_kind_ignored_defensively() {
        let s = snap(vec![hit(".x", "twitter:card", "summary", "future-defect")]);
        let findings = detect_twitter_meta_defects(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!(".meta-{i}"),
                "twitter:card",
                "summary_large_image",
                "wrong-attribute",
            ));
        }
        let s = snap(hits);
        let findings = detect_twitter_meta_defects(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 Twitter Card meta tag(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(TWITTER_META_DEFECTS_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(TWITTER_META_DEFECTS_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(TWITTER_META_DEFECTS_DOM_CAPTURE_JS.contains("hits"));
        assert!(TWITTER_META_DEFECTS_DOM_CAPTURE_JS.contains("scannedMeta"));
        assert!(TWITTER_META_DEFECTS_DOM_CAPTURE_JS.contains("defectKind"));
        // All three defect-kind strings.
        assert!(TWITTER_META_DEFECTS_DOM_CAPTURE_JS.contains("'wrong-attribute'"));
        assert!(TWITTER_META_DEFECTS_DOM_CAPTURE_JS.contains("'empty-content'"));
        assert!(TWITTER_META_DEFECTS_DOM_CAPTURE_JS.contains("'large-image-card-without-image'"));
        // Selector contract — head meta + the cross-check key names.
        assert!(TWITTER_META_DEFECTS_DOM_CAPTURE_JS.contains("'head meta'"));
        assert!(TWITTER_META_DEFECTS_DOM_CAPTURE_JS.contains("'twitter:image'"));
        assert!(TWITTER_META_DEFECTS_DOM_CAPTURE_JS.contains("'twitter:card'"));
        assert!(TWITTER_META_DEFECTS_DOM_CAPTURE_JS.contains("'summary_large_image'"));
    }
}
