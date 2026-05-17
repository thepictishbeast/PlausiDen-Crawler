//! `font_loading` — `@font-face` `font-display` detector.
//!
//! Mirror of `src/fontLoading.ts`. Findings:
//!
//!   * `font-loading.no-display`    warn — `@font-face` has no
//!                                          `font-display` declaration
//!   * `font-loading.display-block` warn — explicit `block`/`auto`
//!
//! The browser default `font-display: block` causes FOIT (Flash of
//! Invisible Text) — text in the web font is hidden until the font
//! finishes loading. `swap`, `fallback`, or `optional` avoid this.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One `@font-face` rule the browser exposed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedFontFace {
    /// Family name from `font-family: "..."`. Quotes stripped.
    pub family: String,
    /// Raw `font-display` value (lowercase, trimmed). Empty if absent.
    pub font_display: String,
    /// Stylesheet href the rule came from. Empty for inline `<style>`.
    pub sheet_href: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FontLoadingSnapshot {
    /// Page URL (carried for evidence).
    pub page_url: String,
    /// Count of stylesheets the browser refused to expose
    /// (`SecurityError` on `cssRules` access — cross-origin sheets
    /// without CORS). Tracked so a near-empty `faces` vec can be
    /// distinguished from genuinely-clean.
    pub inaccessible_sheet_count: u32,
    /// Every `@font-face` rule the browser exposed.
    pub faces: Vec<CapturedFontFace>,
}

/// Acceptable `font-display` values per spec.
const SAFE_FONT_DISPLAY: &[&str] = &["swap", "fallback", "optional"];
/// Explicit FOIT-producing values.
const FOIT_FONT_DISPLAY: &[&str] = &["block", "auto"];

/// Pure detector: snapshot → findings. No I/O.
pub fn detect_font_loading_issues(snap: &FontLoadingSnapshot) -> Vec<AxisFinding> {
    let mut no_display: Vec<&CapturedFontFace> = Vec::new();
    let mut block_display: Vec<&CapturedFontFace> = Vec::new();

    for face in &snap.faces {
        let d = face.font_display.as_str();
        if d.is_empty() {
            no_display.push(face);
        } else if FOIT_FONT_DISPLAY.contains(&d) {
            block_display.push(face);
        } else if !SAFE_FONT_DISPLAY.contains(&d) {
            // Unknown / typo — treat as no-display (effectively block).
            no_display.push(face);
        }
    }

    let mut out = Vec::new();

    if !no_display.is_empty() {
        let examples = format_no_display_examples(&no_display, &snap.page_url);
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "font-loading.no-display".into(),
            detail: format!(
                "{} @font-face rule(s) have no 'font-display' declaration. Browser default is 'block' → text rendered in this font is INVISIBLE until the font file loads (Flash of Invisible Text / FOIT). On slow connections this can hide content for several seconds. Add 'font-display: swap' to each rule. Examples: {}",
                no_display.len(),
                examples
            ),
        });
    }

    if !block_display.is_empty() {
        let examples = format_block_examples(&block_display);
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "font-loading.display-block".into(),
            detail: format!(
                "{} @font-face rule(s) explicitly set 'font-display: block' or 'auto' — text is INVISIBLE until the font loads (FOIT). 'auto' resolves to 'block' in most browsers. Switch to 'swap'. Examples: {}",
                block_display.len(),
                examples
            ),
        });
    }

    out
}

/// Render the first 5 no-display examples as a `; `-joined string.
fn format_no_display_examples(faces: &[&CapturedFontFace], page_url: &str) -> String {
    faces
        .iter()
        .take(5)
        .map(|f| {
            let fam = if f.family.is_empty() {
                "(unnamed)"
            } else {
                f.family.as_str()
            };
            let sheet = if f.sheet_href.is_empty() {
                "(inline)".to_owned()
            } else {
                resolve_sheet_path(&f.sheet_href, page_url)
            };
            format!("font-family='{fam}' from {sheet}")
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Render the first 5 block examples as a `; `-joined string.
fn format_block_examples(faces: &[&CapturedFontFace]) -> String {
    faces
        .iter()
        .take(5)
        .map(|f| {
            let fam = if f.family.is_empty() {
                "(unnamed)"
            } else {
                f.family.as_str()
            };
            format!("font-family='{}' font-display='{}'", fam, f.font_display)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Extract the path portion of an absolute URL string, falling
/// back to the raw href. Avoids pulling in the `url` crate for a
/// one-line display helper. Examples:
///   `https://cdn.example.com/font.css` → `/font.css`
///   `/local.css` → `/local.css`
///   `data:text/css,...` → `data:text/css,...`
fn resolve_sheet_path(href: &str, _page_url: &str) -> String {
    if let Some(idx) = href.find("://") {
        let after_scheme = &href[idx + 3..];
        if let Some(path_start) = after_scheme.find('/') {
            return after_scheme[path_start..].to_owned();
        }
        return "/".to_owned();
    }
    href.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(family: &str, font_display: &str, sheet_href: &str) -> CapturedFontFace {
        CapturedFontFace {
            family: family.to_owned(),
            font_display: font_display.to_owned(),
            sheet_href: sheet_href.to_owned(),
        }
    }

    fn snap(faces: Vec<CapturedFontFace>) -> FontLoadingSnapshot {
        FontLoadingSnapshot {
            page_url: "https://example.com/".into(),
            inaccessible_sheet_count: 0,
            faces,
        }
    }

    #[test]
    fn no_faces_clean() {
        assert!(detect_font_loading_issues(&snap(vec![])).is_empty());
    }

    #[test]
    fn swap_is_safe() {
        let s = snap(vec![face("Inter", "swap", "/style.css")]);
        assert!(detect_font_loading_issues(&s).is_empty());
    }

    #[test]
    fn fallback_is_safe() {
        let s = snap(vec![face("Inter", "fallback", "/style.css")]);
        assert!(detect_font_loading_issues(&s).is_empty());
    }

    #[test]
    fn optional_is_safe() {
        let s = snap(vec![face("Inter", "optional", "/style.css")]);
        assert!(detect_font_loading_issues(&s).is_empty());
    }

    #[test]
    fn empty_font_display_warns_no_display() {
        let s = snap(vec![face("Inter", "", "/style.css")]);
        let f = detect_font_loading_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "font-loading.no-display");
        assert!(f[0].detail.contains("Inter"));
    }

    #[test]
    fn explicit_block_warns_display_block() {
        let s = snap(vec![face("Inter", "block", "/style.css")]);
        let f = detect_font_loading_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "font-loading.display-block");
    }

    #[test]
    fn auto_warns_display_block() {
        let s = snap(vec![face("Inter", "auto", "/style.css")]);
        let f = detect_font_loading_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "font-loading.display-block");
    }

    #[test]
    fn unknown_value_counted_as_no_display() {
        let s = snap(vec![face("Inter", "wibble", "/style.css")]);
        let f = detect_font_loading_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "font-loading.no-display");
    }

    #[test]
    fn empty_family_renders_as_unnamed() {
        let s = snap(vec![face("", "", "/style.css")]);
        let f = detect_font_loading_issues(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("(unnamed)"));
    }

    #[test]
    fn inline_sheet_href_renders_as_inline_label() {
        let s = snap(vec![face("Inter", "", "")]);
        let f = detect_font_loading_issues(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("(inline)"));
    }

    #[test]
    fn mixed_findings_split_by_kind() {
        let s = snap(vec![
            face("Inter", "", "/a.css"),
            face("JetBrains", "block", "/b.css"),
        ]);
        let f = detect_font_loading_issues(&s);
        assert_eq!(f.len(), 2);
        let kinds: Vec<&str> = f.iter().map(|x| x.kind.as_str()).collect();
        assert!(kinds.contains(&"font-loading.no-display"));
        assert!(kinds.contains(&"font-loading.display-block"));
    }

    #[test]
    fn examples_capped_at_5() {
        let faces = (0..10)
            .map(|i| face(&format!("Font{i}"), "", "/x.css"))
            .collect();
        let s = snap(faces);
        let f = detect_font_loading_issues(&s);
        // count says 10 but examples list shows 5
        assert!(f[0].detail.contains("10 @font-face"));
        // Examples list separator count = 4 (between 5 items)
        let example_count = f[0].detail.matches("font-family='Font").count();
        assert_eq!(example_count, 5);
    }
}
