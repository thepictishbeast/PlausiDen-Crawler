//! `link_rel_attribute` — `<link rel>` attribute value-set audit.
//!
//! Sibling to `link_target_blank_safety`,
//! `link_internal_external_indication`, `pdf_link_indicator`,
//! `download_attribute_audit`, `mailto_link_audit`, and
//! `tel_link_audit`. This detector audits the `rel=` attribute
//! on `<link>` elements (NOT `<a rel>`, which is its own
//! surface).
//!
//! ## The contract
//!
//! Per the HTML Living Standard + WHATWG rel registry, each
//! `<link rel="...">` value declares a relationship between the
//! current document and a linked resource. Common authoring
//! failures:
//!
//! 1. **Invalid tokens** — `rel="canonincal"` (typo for
//!    `canonical`), `rel="prefetch-as-script"` (made-up), or
//!    legacy values long removed (`rel="shortcut icon"` — the
//!    `shortcut` keyword is non-standard).
//! 2. **Duplicate `rel="canonical"`** — two or more `<link
//!    rel="canonical">` tags on the same page; per the spec
//!    only the first canonical applies and the rest are
//!    silently ignored.
//! 3. **`rel="stylesheet"` with no `href`** — common
//!    copy-paste-then-delete-href accident; the link does
//!    nothing.
//!
//! ## Findings
//!
//! * `link-rel.invalid-token` strict — any token in `rel=`
//!   that isn't in the registry.
//! * `link-rel.duplicate-canonical` strict — 2+ `rel="canonical"`
//!   on the page.
//! * `link-rel.stylesheet-missing-href` strict — `rel=
//!   "stylesheet"` with no `href` attribute.
//!
//! Out of scope:
//!
//! * `<a rel>` — different surface, covered by
//!   `link_target_blank_safety` etc.
//! * `<link rel="alternate">` content variants — covered by
//!   `hreflang` + `hreflang_mutual_reference`.
//! * `<link rel="preload">` `as=` matching — separate axis.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; the JS const is the only side-
//!   effect channel.
//! * MAX_EXAMPLES = 5 for any per-bucket finding list.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

const MAX_EXAMPLES: usize = 5;

/// Tokens accepted by current browsers per the WHATWG rel
/// registry for `<link>` elements. Not exhaustive — the
/// registry adds entries periodically — but covers every value
/// in active production use plus the most common typo
/// candidates' correct forms.
const VALID_LINK_REL_TOKENS: &[&str] = &[
    "alternate",
    "author",
    "canonical",
    "dns-prefetch",
    "expect",
    "help",
    "icon",
    "license",
    "manifest",
    "modulepreload",
    "next",
    "pingback",
    "preconnect",
    "prefetch",
    "preload",
    "prerender",
    "prev",
    "search",
    "stylesheet",
    "me",
    "shortlink",
    "publisher",
    "apple-touch-icon",
    "apple-touch-icon-precomposed",
    "mask-icon",
    "fluid-icon",
    "edituri",
    "amphtml",
    "openid.delegate",
    "openid.server",
    "openid2.local_id",
    "openid2.provider",
    "preconnect-on-load",
    "privacy-policy",
    "terms-of-service",
];

/// One captured `<link>` element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LinkRelEntry {
    /// CSS-ish selector pointing at the `<link>` (typically
    /// based on `:nth-of-type` since `<link>` elements rarely
    /// have ids).
    pub selector: String,
    /// Raw `rel=` attribute value (trimmed).
    pub rel_value: String,
    /// Whether the link carries an `href` attribute.
    pub has_href: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LinkRelAttributeSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every `<link>` element with a `rel=` attribute on the
    /// page.
    pub entries: Vec<LinkRelEntry>,
}

/// Detector.
#[must_use]
pub fn detect_link_rel_attribute(snap: &LinkRelAttributeSnapshot) -> Vec<AxisFinding> {
    let mut invalid: Vec<String> = Vec::new();
    let mut stylesheet_no_href: Vec<&str> = Vec::new();
    let mut canonical_count = 0;
    let mut canonical_selectors: Vec<&str> = Vec::new();

    for entry in &snap.entries {
        let raw = entry.rel_value.trim();
        if raw.is_empty() {
            continue;
        }
        let tokens: Vec<String> = raw
            .split_whitespace()
            .map(|s| s.to_ascii_lowercase())
            .collect();

        // Invalid-token check per token.
        let bad_tokens: Vec<&str> = tokens
            .iter()
            .filter(|t| {
                !VALID_LINK_REL_TOKENS
                    .iter()
                    .any(|v| v.eq_ignore_ascii_case(t))
            })
            .map(String::as_str)
            .collect();
        if !bad_tokens.is_empty() {
            invalid.push(format!(
                "{} (invalid tokens=[{}])",
                entry.selector,
                bad_tokens.join(",")
            ));
        }

        // Canonical count.
        if tokens.iter().any(|t| t == "canonical") {
            canonical_count += 1;
            canonical_selectors.push(entry.selector.as_str());
        }

        // Stylesheet-without-href check.
        if tokens.iter().any(|t| t == "stylesheet") && !entry.has_href {
            stylesheet_no_href.push(entry.selector.as_str());
        }
    }

    let mut findings = Vec::new();
    let total = snap.entries.len();

    if !invalid.is_empty() {
        let preview =
            preview_examples(&invalid.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "link-rel.invalid-token".to_owned(),
            detail: format!(
                "{} of {} <link rel> attribute(s) contain tokens not in the WHATWG rel registry. Examples: {}",
                invalid.len(),
                total,
                preview
            ),
        });
    }

    if !stylesheet_no_href.is_empty() {
        let preview = preview_examples(&stylesheet_no_href);
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "link-rel.stylesheet-missing-href".to_owned(),
            detail: format!(
                "{} of {} <link rel=\"stylesheet\"> tag(s) have no href attribute; the stylesheet load is a no-op. Examples: {}",
                stylesheet_no_href.len(),
                total,
                preview
            ),
        });
    }

    if canonical_count >= 2 {
        let preview = preview_examples(&canonical_selectors);
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "link-rel.duplicate-canonical".to_owned(),
            detail: format!(
                "{} <link rel=\"canonical\"> tags on the page; per spec only the first applies, the rest are silently ignored. Examples: {}",
                canonical_count, preview
            ),
        });
    }

    findings
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

/// Page-side eval. Walks every `<link rel>` element in `<head>`
/// (and elsewhere — `<link>` is allowed in body per HTML
/// living standard).
pub const LINK_REL_ATTRIBUTE_JS: &str = r##"(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const sameTag = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (sameTag.length > 1) {
            const idx = sameTag.indexOf(node) + 1;
            parts.unshift(tag + ':nth-of-type(' + idx + ')');
          } else { parts.unshift(tag); }
        } else { parts.unshift(tag); }
        node = parent;
        depth += 1;
      }
      return parts.join(' > ') || 'body';
    };

    const links = Array.from(document.querySelectorAll('link[rel]'));
    const entries = links.map(function(l) {
      return {
        selector: selectorOf(l),
        relValue: l.getAttribute('rel') || '',
        hasHref: l.hasAttribute('href')
      };
    });

    return {
      pageUrl: location.href,
      entries: entries
    };
  })()"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sel: &str, rel: &str, has_href: bool) -> LinkRelEntry {
        LinkRelEntry {
            selector: sel.to_owned(),
            rel_value: rel.to_owned(),
            has_href,
        }
    }

    fn snap(entries: Vec<LinkRelEntry>) -> LinkRelAttributeSnapshot {
        LinkRelAttributeSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_link_rel_attribute(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn valid_canonical_is_clean() {
        let f = detect_link_rel_attribute(&snap(vec![entry(
            "head > link",
            "canonical",
            true,
        )]));
        assert!(f.is_empty(), "valid canonical should pass: {f:?}");
    }

    #[test]
    fn valid_stylesheet_with_href_is_clean() {
        let f = detect_link_rel_attribute(&snap(vec![entry(
            "head > link",
            "stylesheet",
            true,
        )]));
        assert!(f.is_empty(), "valid stylesheet should pass: {f:?}");
    }

    #[test]
    fn multi_token_rel_with_all_valid_is_clean() {
        let f = detect_link_rel_attribute(&snap(vec![entry(
            "head > link",
            "preconnect dns-prefetch",
            true,
        )]));
        assert!(f.is_empty(), "multi-valid should pass: {f:?}");
    }

    #[test]
    fn typo_canonical_is_strict_invalid() {
        let f = detect_link_rel_attribute(&snap(vec![entry(
            "head > link",
            "canonincal",
            true,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "link-rel.invalid-token")
            .expect("invalid-token expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("canonincal"));
    }

    #[test]
    fn shortcut_icon_is_strict_invalid() {
        // `shortcut` is non-standard (only `icon` is registered).
        let f = detect_link_rel_attribute(&snap(vec![entry(
            "head > link",
            "shortcut icon",
            true,
        )]));
        assert!(f
            .iter()
            .any(|x| x.kind == "link-rel.invalid-token"));
    }

    #[test]
    fn stylesheet_without_href_is_strict() {
        let f = detect_link_rel_attribute(&snap(vec![entry(
            "head > link",
            "stylesheet",
            false,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "link-rel.stylesheet-missing-href")
            .expect("stylesheet-missing-href expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
    }

    #[test]
    fn duplicate_canonical_is_strict() {
        let f = detect_link_rel_attribute(&snap(vec![
            entry("head > link:nth-of-type(1)", "canonical", true),
            entry("head > link:nth-of-type(2)", "canonical", true),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "link-rel.duplicate-canonical")
            .expect("duplicate-canonical expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("2 <link rel=\"canonical\">"));
    }

    #[test]
    fn single_canonical_is_clean() {
        let f = detect_link_rel_attribute(&snap(vec![entry(
            "head > link",
            "canonical",
            true,
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "link-rel.duplicate-canonical"),
            "single canonical should not flag dup: {f:?}"
        );
    }

    #[test]
    fn case_insensitive_token_match() {
        let f = detect_link_rel_attribute(&snap(vec![entry(
            "head > link",
            "CANONICAL",
            true,
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "link-rel.invalid-token"),
            "CANONICAL should be case-insensitive valid: {f:?}"
        );
    }

    #[test]
    fn multi_token_mixed_valid_and_invalid_flags_invalid_part() {
        let f = detect_link_rel_attribute(&snap(vec![entry(
            "head > link",
            "stylesheet preload nonsense",
            true,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "link-rel.invalid-token")
            .expect("invalid-token expected");
        assert!(hit.detail.contains("nonsense"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| {
                entry(
                    &format!("head > link:nth-of-type({})", i + 1),
                    "bogus",
                    true,
                )
            })
            .collect();
        let f = detect_link_rel_attribute(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "link-rel.invalid-token")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_link_rel() {
        assert!(LINK_REL_ATTRIBUTE_JS.starts_with("(() => {"));
        assert!(LINK_REL_ATTRIBUTE_JS.ends_with(")()"));
        assert!(LINK_REL_ATTRIBUTE_JS.contains("link[rel]"));
        assert!(LINK_REL_ATTRIBUTE_JS.contains("hasAttribute"));
    }
}
