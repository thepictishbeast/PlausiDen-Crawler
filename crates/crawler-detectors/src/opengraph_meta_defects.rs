//! `opengraph_meta_defects` — flags broken Open Graph meta tags.
//!
//! Mirror of [`crate::twitter_meta_defects`] for the
//! complementary canonical-meta vocabulary. Open Graph uses
//! `<meta property="og:…">` — NOT `<meta name="og:…">` like
//! Twitter Cards. The canonical operator copy-paste bug:
//! using `name=` for both. Result: Facebook / LinkedIn /
//! Slack / Discord previews fall back to whatever they can
//! resolve from generic defaults — usually wrong title,
//! wrong image.
//!
//! Three defect classes:
//!
//! 1. **Wrong attribute** (Strict). `<meta name="og:X">`.
//!    Parsers expecting `property=` ignore it.
//!
//! 2. **Empty content** (Warn). `<meta property="og:X"
//!    content="">`. Valid syntax but no signal; wasted bytes.
//!
//! 3. **Missing required field for declared og:type** (Strict).
//!    Open Graph requires `og:title`, `og:type`, `og:image`,
//!    and `og:url` at minimum. When `og:type` is set but any
//!    of the other three is missing, the preview will be
//!    incomplete.
//!
//! Skip cases that don't carry the burden:
//!
//! * No `og:*` tags at all — page isn't trying to opt into
//!   social-card previews. Out of scope here; the
//!   meta_description axis covers the generic case.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending Open Graph meta tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct OpenGraphMetaDefectHit {
    /// CSS-ish path of the offending `<meta>`. For the
    /// missing-required-field defect this points at the
    /// declared `og:type` tag (the trigger) — the missing
    /// key appears in `key`.
    pub selector: String,
    /// The `og:*` key (e.g. `"og:title"`, `"og:image"`).
    pub key: String,
    /// `content` value, capped at 120 chars.
    pub content: String,
    /// Defect kind — one of `"wrong-attribute"`,
    /// `"empty-content"`, `"missing-required-field"`.
    pub defect_kind: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct OpenGraphMetaDefectSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Offending Open Graph meta tags.
    pub hits: Vec<OpenGraphMetaDefectHit>,
    /// Total `<meta>` elements walked.
    pub scanned_meta: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// The Open Graph required-field set when `og:type` is set.
/// Per [the spec](https://ogp.me/): every page declaring
/// `og:type` SHOULD also declare `og:title`, `og:image`,
/// `og:url`. (`og:description` is optional but conventional.)
pub const OG_REQUIRED_KEYS_WHEN_TYPE_SET: &[&str] =
    &["og:title", "og:image", "og:url"];

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_opengraph_meta_defects(snap: &OpenGraphMetaDefectSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut wrong_attr: Vec<&OpenGraphMetaDefectHit> = Vec::new();
    let mut empty_content: Vec<&OpenGraphMetaDefectHit> = Vec::new();
    let mut missing_required: Vec<&OpenGraphMetaDefectHit> = Vec::new();
    for h in &snap.hits {
        match h.defect_kind.as_str() {
            "wrong-attribute" => wrong_attr.push(h),
            "empty-content" => empty_content.push(h),
            "missing-required-field" => missing_required.push(h),
            _ => {} // defensive: unknown kinds dropped
        }
    }

    let format_example = |h: &OpenGraphMetaDefectHit| -> String {
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
            kind: "opengraph-meta.wrong-attribute".to_owned(),
            detail: format!(
                "{} Open Graph meta tag(s) using `name=` instead of `property=` — Facebook / LinkedIn / Slack / Discord parsers ignore these. Replace `<meta name=\"og:X\">` with `<meta property=\"og:X\">`. Twitter Cards use `name=` — Open Graph does not. Examples: {}",
                wrong_attr.len(),
                examples.join("; ")
            ),
        });
    }
    if !missing_required.is_empty() {
        let examples: Vec<String> = missing_required
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "opengraph-meta.missing-required-field".to_owned(),
            detail: format!(
                "{} page(s) declare `og:type` but are missing one of the required fields (`og:title` / `og:image` / `og:url`). Previews will be incomplete on Facebook / LinkedIn / Slack / Discord. Add the missing tags. Examples: {}",
                missing_required.len(),
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
            kind: "opengraph-meta.empty-content".to_owned(),
            detail: format!(
                "{} `og:*` meta tag(s) with empty `content=` — wasted bytes that provide no signal. Either populate the field or remove the tag. Examples: {}",
                empty_content.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks `<meta>` elements,
/// captures `og:*` tags written with the wrong attribute +
/// correct-attribute tags with empty content + the missing-
/// required-field defect when `og:type` is set.
pub const OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS: &str = r#"
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

    const REQUIRED_KEYS = ['og:title', 'og:image', 'og:url'];

    const hits = [];
    let scanned = 0;
    const allMeta = document.querySelectorAll('head meta');
    // Track presence of correct-attribute og:type + required keys.
    let typeTagSelector = null;
    let typeTagContent = '';
    const presentRequired = { 'og:title': false, 'og:image': false, 'og:url': false };

    for (const m of allMeta) {
      scanned += 1;
      const propAttr = (m.getAttribute('property') || '').trim().toLowerCase();
      const nameAttr = (m.getAttribute('name') || '').trim().toLowerCase();
      const content = (m.getAttribute('content') || '').trim();

      // Wrong attribute: name="og:…".
      if (nameAttr.startsWith('og:')) {
        hits.push({
          selector: selectorOf(m),
          key: nameAttr,
          content: content.substring(0, 120),
          defectKind: 'wrong-attribute'
        });
        continue;
      }

      // Correct-attribute og:*.
      if (propAttr.startsWith('og:')) {
        if (content === '') {
          hits.push({
            selector: selectorOf(m),
            key: propAttr,
            content: '',
            defectKind: 'empty-content'
          });
          continue;
        }
        if (propAttr === 'og:type') {
          typeTagSelector = selectorOf(m);
          typeTagContent = content;
        }
        if (REQUIRED_KEYS.indexOf(propAttr) !== -1) {
          presentRequired[propAttr] = true;
        }
      }
    }

    // Cross-check: og:type set but any required field missing.
    if (typeTagSelector !== null) {
      for (const k of REQUIRED_KEYS) {
        if (!presentRequired[k]) {
          hits.push({
            selector: typeTagSelector,
            key: k,
            content: typeTagContent.substring(0, 120),
            defectKind: 'missing-required-field'
          });
        }
      }
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

    fn hit(selector: &str, key: &str, content: &str, defect_kind: &str) -> OpenGraphMetaDefectHit {
        OpenGraphMetaDefectHit {
            selector: selector.into(),
            key: key.into(),
            content: content.into(),
            defect_kind: defect_kind.into(),
        }
    }

    fn snap(hits: Vec<OpenGraphMetaDefectHit>) -> OpenGraphMetaDefectSnapshot {
        OpenGraphMetaDefectSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_meta: 20,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_opengraph_meta_defects(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn wrong_attribute_is_strict() {
        let s = snap(vec![hit(
            "head > meta:nth-of-type(2)",
            "og:title",
            "Welcome",
            "wrong-attribute",
        )]);
        let findings = detect_opengraph_meta_defects(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "opengraph-meta.wrong-attribute");
        assert!(findings[0].detail.contains("key=`og:title`"));
        assert!(findings[0].detail.contains("ignore these"));
    }

    #[test]
    fn empty_content_is_warn() {
        let s = snap(vec![hit(
            "head > meta:nth-of-type(4)",
            "og:description",
            "",
            "empty-content",
        )]);
        let findings = detect_opengraph_meta_defects(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "opengraph-meta.empty-content");
    }

    #[test]
    fn missing_required_field_is_strict() {
        let s = snap(vec![hit(
            "head > meta:nth-of-type(1)",
            "og:image",
            "article",
            "missing-required-field",
        )]);
        let findings = detect_opengraph_meta_defects(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(
            findings[0].kind,
            "opengraph-meta.missing-required-field"
        );
        assert!(findings[0].detail.contains("Previews will be incomplete"));
    }

    #[test]
    fn all_three_defects_emit_three_findings() {
        let s = snap(vec![
            hit(".a", "og:title", "X", "wrong-attribute"),
            hit(".b", "og:description", "", "empty-content"),
            hit(".c", "og:image", "article", "missing-required-field"),
        ]);
        let findings = detect_opengraph_meta_defects(&s);
        assert_eq!(findings.len(), 3);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"opengraph-meta.wrong-attribute"));
        assert!(kinds.contains(&"opengraph-meta.empty-content"));
        assert!(kinds.contains(&"opengraph-meta.missing-required-field"));
    }

    #[test]
    fn unknown_defect_kind_ignored_defensively() {
        let s = snap(vec![hit(".x", "og:type", "article", "future-defect")]);
        let findings = detect_opengraph_meta_defects(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!(".meta-{i}"),
                "og:title",
                "X",
                "wrong-attribute",
            ));
        }
        let s = snap(hits);
        let findings = detect_opengraph_meta_defects(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 Open Graph meta tag(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn og_required_keys_constant_matches_spec() {
        // Sanity: ogp.me lists og:title + og:type + og:image +
        // og:url as the minimum required. We track 3 (title +
        // image + url) and trigger only when og:type itself is
        // set — that's how the JS captures it.
        assert_eq!(
            OG_REQUIRED_KEYS_WHEN_TYPE_SET,
            &["og:title", "og:image", "og:url"]
        );
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + cross-check contract.
        assert!(OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS.contains("hits"));
        assert!(OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS.contains("scannedMeta"));
        assert!(OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS.contains("defectKind"));
        // All three defect-kind strings.
        assert!(OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS.contains("'wrong-attribute'"));
        assert!(OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS.contains("'empty-content'"));
        assert!(OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS.contains("'missing-required-field'"));
        // Required-keys contract — 3 keys.
        assert!(OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS.contains("'og:title'"));
        assert!(OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS.contains("'og:image'"));
        assert!(OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS.contains("'og:url'"));
        assert!(OPENGRAPH_META_DEFECTS_DOM_CAPTURE_JS.contains("'og:type'"));
    }
}
