//! `font_face_no_fallback` — @font-face + computed-style fallback
//! ladder detector.
//!
//! Sibling axis to `font_loading` (which audits `font-display`
//! strategies + FOIT/FOUT exposure on `@font-face` rules) and to
//! `webfont_preload_audit` (which audits whether key webfont
//! files have `<link rel="preload">` ahead of CSS request).
//! This axis covers the OTHER end of the chain: whether the
//! computed `font-family` of visible text nodes lists generic-
//! family fallbacks at all.
//!
//! ## The bug class
//!
//! `font-family: "GT America"` with no `, system-ui, sans-serif`
//! fallback is a routine ship-blocker. When the webfont fails to
//! load (offline, blocked tracker, CDN outage, Tor circuit
//! refused, network captive portal), the browser walks the
//! family list to find a usable family. With no fallback the
//! family list collapses to the UA default for the element,
//! which is often a serif on a sans-styled site — visible
//! typographic regression for every reader.
//!
//! The detector receives a snapshot of:
//! 1. Every `@font-face` rule loaded into the document (CSSOM
//!    walk). Captures the declared `font-family` and source URLs.
//! 2. The aggregated computed `font-family` strings of every
//!    visible text-bearing element. Counted by appearance.
//!
//! ## Findings
//!
//! * `font-face.declared-family-not-fallback-only` strict —
//!   a computed `font-family` references ONLY an
//!   `@font-face`-declared family with no generic-family token
//!   (`serif` / `sans-serif` / `monospace` / `system-ui` /
//!   `cursive` / `fantasy` / `ui-*`) appended. When the webfont
//!   fails to load there is no fallback at all.
//! * `font-face.generic-family-missing` strict — computed
//!   `font-family` has multiple families listed but NONE are
//!   generic. Operators sometimes chain three webfonts
//!   (`"Display", "Body", "Mono"`) hoping at least one loads;
//!   if none do, the UA default is the only fallback.
//! * `font-face.unused-declared-family` warn — `@font-face`
//!   declared a family the computed-style aggregate never
//!   references. Wasted bytes + cache pressure.
//!
//! Out of scope:
//!
//! * `font-display: optional|fallback|swap|block|auto` — covered
//!   by the existing `font_loading` axis.
//! * `unicode-range` correctness — covered by future
//!   `font_subset_coverage` axis.
//! * Generic family RENAMING (`font-family: "Helvetica
//!   Neue", Helvetica, Arial, sans-serif`) — only the presence
//!   of *any* generic at chain tail is required.
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

/// CSS generic-family keywords. Order doesn't matter; presence
/// of any token in the family list is sufficient to call the
/// chain "has a generic fallback".
const GENERIC_FAMILIES: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
    "fangsong",
];

/// One declared `@font-face` rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DeclaredFontFace {
    /// Declared family token from the `@font-face { font-family: ... }`
    /// rule (unquoted, normalised).
    pub family: String,
    /// Captured source URL(s). Empty when the rule has no `src:`
    /// (rare; treated as no-op fallback).
    pub src_urls: Vec<String>,
}

/// One computed `font-family` cluster — the family-list string
/// (e.g. `"GT America", system-ui, sans-serif`) plus the count
/// of visible text-bearing elements observed with this exact
/// computed value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ComputedFamilyCluster {
    /// Raw computed `font-family` value (browser-normalised).
    pub family_list: String,
    /// Count of visible text-bearing elements with this exact
    /// computed value.
    pub element_count: u32,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FontFaceNoFallbackSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every `@font-face` rule walked from the document's
    /// stylesheets.
    pub declared: Vec<DeclaredFontFace>,
    /// Aggregated computed `font-family` clusters over visible
    /// text-bearing elements.
    pub computed: Vec<ComputedFamilyCluster>,
}

/// Detector. Buckets findings by defect kind so consumers see at
/// most one finding per kind with up to `MAX_EXAMPLES` previews
/// in `detail`.
#[must_use]
pub fn detect_font_face_no_fallback(snap: &FontFaceNoFallbackSnapshot) -> Vec<AxisFinding> {
    let declared_families: Vec<String> = snap
        .declared
        .iter()
        .map(|d| normalise_token(&d.family))
        .collect();

    let mut declared_family_not_fallback_only: Vec<String> = Vec::new();
    let mut generic_family_missing: Vec<String> = Vec::new();

    for cluster in &snap.computed {
        let tokens = split_family_list(&cluster.family_list);
        let has_generic = tokens.iter().any(|t| is_generic(t));
        if has_generic {
            continue;
        }
        // No generic at all in this chain. Subclassify.
        let only_one = tokens.len() == 1;
        let only_token_is_declared = only_one
            && declared_families
                .iter()
                .any(|d| d.eq_ignore_ascii_case(&tokens[0]));
        if only_token_is_declared {
            declared_family_not_fallback_only.push(format_cluster_preview(cluster));
        } else {
            generic_family_missing.push(format_cluster_preview(cluster));
        }
    }

    let mut unused_declared: Vec<&str> = Vec::new();
    for declared in &snap.declared {
        let needle = normalise_token(&declared.family);
        let referenced = snap.computed.iter().any(|c| {
            split_family_list(&c.family_list)
                .iter()
                .any(|t| t.eq_ignore_ascii_case(&needle))
        });
        if !referenced {
            unused_declared.push(declared.family.as_str());
        }
    }

    let mut findings = Vec::new();
    let total_clusters = snap.computed.len();
    let total_declared = snap.declared.len();

    if !declared_family_not_fallback_only.is_empty() {
        let preview = preview_examples(
            &declared_family_not_fallback_only
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "font-face.declared-family-not-fallback-only".to_owned(),
            detail: format!(
                "{} of {} computed font-family cluster(s) reference ONLY an @font-face-declared family with no generic fallback. Examples: {}",
                declared_family_not_fallback_only.len(),
                total_clusters,
                preview
            ),
        });
    }

    if !generic_family_missing.is_empty() {
        let preview = preview_examples(
            &generic_family_missing
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "font-face.generic-family-missing".to_owned(),
            detail: format!(
                "{} of {} computed font-family cluster(s) chain multiple non-generic families without a generic tail. Examples: {}",
                generic_family_missing.len(),
                total_clusters,
                preview
            ),
        });
    }

    if !unused_declared.is_empty() {
        let preview = preview_examples(&unused_declared);
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "font-face.unused-declared-family".to_owned(),
            detail: format!(
                "{} of {} @font-face declared families are never referenced by visible text. Examples: {}",
                unused_declared.len(),
                total_declared,
                preview
            ),
        });
    }

    findings
}

fn split_family_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|t| t.trim().trim_matches(|c| c == '"' || c == '\'').to_owned())
        .filter(|t| !t.is_empty())
        .collect()
}

fn normalise_token(s: &str) -> String {
    s.trim().trim_matches(|c| c == '"' || c == '\'').to_owned()
}

fn is_generic(token: &str) -> bool {
    GENERIC_FAMILIES
        .iter()
        .any(|g| token.eq_ignore_ascii_case(g))
}

fn format_cluster_preview(cluster: &ComputedFamilyCluster) -> String {
    format!(
        "[count={}] {}",
        cluster.element_count, cluster.family_list
    )
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

/// Page-side eval. Walks every `CSSStyleSheet.cssRules` for
/// `@font-face` rules; clusters every visible text-bearing
/// element's computed `font-family`.
pub const FONT_FACE_NO_FALLBACK_JS: &str = r##"(() => {
    const declared = [];
    try {
      const sheets = Array.from(document.styleSheets || []);
      sheets.forEach(function(sheet) {
        let rules = null;
        try { rules = sheet.cssRules; } catch (e) { return; }
        if (!rules) return;
        Array.from(rules).forEach(function(rule) {
          if (rule.type !== CSSRule.FONT_FACE_RULE) return;
          const family = (rule.style.getPropertyValue('font-family') || '').trim();
          if (!family) return;
          const srcRaw = rule.style.getPropertyValue('src') || '';
          const srcUrls = [];
          const re = /url\(\s*['"]?([^'")]+)['"]?\s*\)/g;
          let m;
          while ((m = re.exec(srcRaw)) !== null) { srcUrls.push(m[1]); }
          declared.push({ family: family.replace(/['"]/g, ''), srcUrls: srcUrls });
        });
      });
    } catch (e) { /* CSSOM exposure varies; surface zero declared and let detector mark unused as empty */ }

    const counts = new Map();
    const all = document.querySelectorAll('body *');
    for (const el of all) {
      if (!el || el.nodeType !== 1) continue;
      // Skip elements with no own text content.
      const hasOwnText = Array.from(el.childNodes).some(function(n) {
        return n.nodeType === 3 && (n.textContent || '').trim().length > 0;
      });
      if (!hasOwnText) continue;
      const cs = getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') continue;
      const fam = (cs.fontFamily || '').trim();
      if (!fam) continue;
      counts.set(fam, (counts.get(fam) || 0) + 1);
    }

    const computed = [];
    counts.forEach(function(count, fam) {
      computed.push({ familyList: fam, elementCount: count });
    });

    return {
      pageUrl: location.href,
      declared: declared,
      computed: computed
    };
  })()"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(family: &str) -> DeclaredFontFace {
        DeclaredFontFace {
            family: family.to_owned(),
            src_urls: vec![format!("/fonts/{family}.woff2")],
        }
    }

    fn cluster(family_list: &str, count: u32) -> ComputedFamilyCluster {
        ComputedFamilyCluster {
            family_list: family_list.to_owned(),
            element_count: count,
        }
    }

    fn snap(
        declared: Vec<DeclaredFontFace>,
        computed: Vec<ComputedFamilyCluster>,
    ) -> FontFaceNoFallbackSnapshot {
        FontFaceNoFallbackSnapshot {
            page_url: "https://example.test/".to_owned(),
            declared,
            computed,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_font_face_no_fallback(&snap(vec![], vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn well_formed_chain_with_generic_tail_is_clean() {
        let f = detect_font_face_no_fallback(&snap(
            vec![declared("GT America")],
            vec![cluster("\"GT America\", system-ui, sans-serif", 42)],
        ));
        assert!(f.is_empty(), "expected clean, got {f:?}");
    }

    #[test]
    fn declared_family_with_no_fallback_is_strict() {
        let f = detect_font_face_no_fallback(&snap(
            vec![declared("Display")],
            vec![cluster("\"Display\"", 17)],
        ));
        let hit = f
            .iter()
            .find(|x| x.kind == "font-face.declared-family-not-fallback-only")
            .expect("declared-only finding expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("Display"), "{}", hit.detail);
        assert!(hit.detail.contains("count=17"));
    }

    #[test]
    fn multiple_non_generic_families_without_generic_tail_is_strict() {
        let f = detect_font_face_no_fallback(&snap(
            vec![declared("Display"), declared("Body"), declared("Mono")],
            vec![cluster("\"Display\", \"Body\", \"Mono\"", 9)],
        ));
        let hit = f
            .iter()
            .find(|x| x.kind == "font-face.generic-family-missing")
            .expect("generic-missing finding expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("count=9"));
        // NOT also flagged as declared-only — only one undeclared
        // chain shape per cluster.
        assert!(!f
            .iter()
            .any(|x| x.kind == "font-face.declared-family-not-fallback-only"));
    }

    #[test]
    fn unused_declared_family_is_warn() {
        let f = detect_font_face_no_fallback(&snap(
            vec![declared("Display"), declared("Unused")],
            vec![cluster("\"Display\", system-ui, sans-serif", 30)],
        ));
        let hit = f
            .iter()
            .find(|x| x.kind == "font-face.unused-declared-family")
            .expect("unused-declared finding expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("Unused"));
        assert!(!hit.detail.contains("Display"));
    }

    #[test]
    fn quoted_and_unquoted_tokens_normalise_equivalently() {
        // Declared as "GT America" (with quotes) — computed-style
        // string normalises to either quoted or unquoted depending
        // on browser; detector treats them as equal.
        let f = detect_font_face_no_fallback(&snap(
            vec![declared("GT America")],
            vec![cluster("GT America, sans-serif", 12)],
        ));
        assert!(f.is_empty(), "expected clean (unquoted match), got {f:?}");
    }

    #[test]
    fn case_insensitive_match_on_generic_tail() {
        let f = detect_font_face_no_fallback(&snap(
            vec![declared("Body")],
            vec![cluster("\"Body\", SANS-SERIF", 5)],
        ));
        assert!(f.is_empty(), "case-insensitive generic should be ok");
    }

    #[test]
    fn computed_family_referencing_only_undeclared_token_is_generic_missing() {
        // Body text references a name that's not @font-face declared
        // and not a generic — almost certainly a typo. Detector
        // can't distinguish typo from "I rely on system having
        // Helvetica installed"; it just flags missing generic.
        let f = detect_font_face_no_fallback(&snap(
            vec![],
            vec![cluster("Helvetica", 3)],
        ));
        let hit = f
            .iter()
            .find(|x| x.kind == "font-face.generic-family-missing")
            .expect("generic-missing finding expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("Helvetica"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let clusters: Vec<ComputedFamilyCluster> =
            (0..8).map(|i| cluster(&format!("\"Display{i}\""), 1)).collect();
        let f = detect_font_face_no_fallback(&snap(vec![], clusters));
        let hit = f
            .iter()
            .find(|x| x.kind == "font-face.generic-family-missing")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
        for i in 0..5 {
            assert!(hit.detail.contains(&format!("Display{i}")));
        }
        for i in 5..8 {
            assert!(
                !hit.detail.contains(&format!("Display{i}")),
                "Display{i} should be hidden"
            );
        }
    }

    #[test]
    fn js_const_is_iife_and_walks_font_face_rules() {
        assert!(FONT_FACE_NO_FALLBACK_JS.starts_with("(() => {"));
        assert!(FONT_FACE_NO_FALLBACK_JS.ends_with(")()"));
        assert!(FONT_FACE_NO_FALLBACK_JS.contains("FONT_FACE_RULE"));
        assert!(FONT_FACE_NO_FALLBACK_JS.contains("getComputedStyle"));
        assert!(FONT_FACE_NO_FALLBACK_JS.contains("fontFamily"));
    }
}
