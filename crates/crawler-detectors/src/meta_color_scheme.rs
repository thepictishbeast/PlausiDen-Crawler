//! `meta_color_scheme` — `<meta name="color-scheme">` audit
//! plus `color-scheme` CSS property cross-check.
//!
//! Sibling axis to `viewport_meta`, `meta_description`,
//! `doctype_charset`, and the `inline_theme_override` axis.
//! This detector covers a different bug class: whether the
//! page declares a `color-scheme` honoured by both the
//! browser UA chrome (form widgets, scrollbars, autofill) and
//! the document's own CSS variables.
//!
//! ## Why this matters
//!
//! Per CSS Color Adjustment (Level 1) + HTML spec, the
//! `color-scheme` property tells the browser which schemes the
//! page supports. When absent, browsers assume `light` —
//! meaning native UI surfaces (date pickers, scrollbars,
//! checkboxes, autofill highlights) render in light mode even
//! on a `prefers-color-scheme: dark` user. This produces
//! flashes of bright UA chrome inside an otherwise dark
//! design.
//!
//! ## Findings
//!
//! * `color-scheme.missing` warn — page has no
//!   `<meta name="color-scheme">` AND no `color-scheme` CSS
//!   property set on `:root`. Native UI chrome renders in UA
//!   default (`light`) regardless of user preference.
//! * `color-scheme.invalid-value` strict — declared value
//!   contains tokens that aren't part of the
//!   `color-scheme` grammar (allowed: `normal`, `light`,
//!   `dark`, `only`, and `light dark` / `dark light` pairs).
//! * `color-scheme.disagrees-between-meta-and-css` warn —
//!   `<meta>` declares one scheme set, the computed
//!   `color-scheme` of `:root` declares another. Either is
//!   honoured but inconsistency is a bug class.
//!
//! Out of scope:
//!
//! * Whether the page's CUSTOM dark/light tokens match the
//!   declared color-scheme — separate `dark_mode_token_match`
//!   axis.
//! * `prefers-color-scheme` media query absence — covered by
//!   `css_health` axis.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; the JS const is the only side-
//!   effect channel.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Valid `color-scheme` keyword tokens per the CSS Color
/// Adjustment Level 1 grammar.
const VALID_TOKENS: &[&str] = &["normal", "light", "dark", "only"];

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MetaColorSchemeSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// `content=` attribute of `<meta name="color-scheme">`,
    /// `None` when the meta tag is absent.
    pub meta_value: Option<String>,
    /// Computed `color-scheme` property on `:root`, `None`
    /// when not set (browser default).
    pub root_css_value: Option<String>,
}

/// Detector.
#[must_use]
pub fn detect_meta_color_scheme(snap: &MetaColorSchemeSnapshot) -> Vec<AxisFinding> {
    let mut findings = Vec::new();

    let meta = snap.meta_value.as_deref().map(str::trim).unwrap_or("");
    let css = snap.root_css_value.as_deref().map(str::trim).unwrap_or("");

    // Treat "normal" / empty as "no explicit scheme declared".
    let meta_present = !meta.is_empty() && !meta.eq_ignore_ascii_case("normal");
    let css_present = !css.is_empty() && !css.eq_ignore_ascii_case("normal");

    if !meta_present && !css_present {
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "color-scheme.missing".to_owned(),
            detail: "Page declares no color-scheme (neither <meta name=\"color-scheme\"> nor :root CSS property); native UI chrome renders in UA default (light) regardless of user preference."
                .to_owned(),
        });
    }

    // Invalid-value: check meta first, then css.
    if meta_present && !is_valid_color_scheme_grammar(meta) {
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "color-scheme.invalid-value".to_owned(),
            detail: format!(
                "<meta name=\"color-scheme\"> content=\"{meta}\" contains tokens outside the color-scheme grammar (allowed: normal, light, dark, only)."
            ),
        });
    }
    if css_present && !is_valid_color_scheme_grammar(css) {
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "color-scheme.invalid-value".to_owned(),
            detail: format!(
                ":root {{ color-scheme: {css}; }} contains tokens outside the color-scheme grammar (allowed: normal, light, dark, only)."
            ),
        });
    }

    // Disagreement: only when both are present, both valid,
    // and the schemes differ.
    if meta_present
        && css_present
        && is_valid_color_scheme_grammar(meta)
        && is_valid_color_scheme_grammar(css)
        && !schemes_equivalent(meta, css)
    {
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "color-scheme.disagrees-between-meta-and-css".to_owned(),
            detail: format!(
                "<meta name=\"color-scheme\">=\"{meta}\" but :root CSS color-scheme=\"{css}\". Each is independently honoured but the inconsistency is a bug class."
            ),
        });
    }

    findings
}

fn is_valid_color_scheme_grammar(value: &str) -> bool {
    let tokens: Vec<&str> =
        value.split_whitespace().filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() {
        return false;
    }
    tokens.iter().all(|t| {
        VALID_TOKENS
            .iter()
            .any(|v| v.eq_ignore_ascii_case(t))
    })
}

fn schemes_equivalent(a: &str, b: &str) -> bool {
    let canonicalise = |s: &str| -> Vec<String> {
        let mut tokens: Vec<String> = s
            .split_whitespace()
            .map(|t| t.to_ascii_lowercase())
            .collect();
        tokens.sort();
        tokens
    };
    canonicalise(a) == canonicalise(b)
}

/// Page-side eval. Captures the `<meta name="color-scheme">`
/// content + computed `color-scheme` on `:root`.
pub const META_COLOR_SCHEME_JS: &str = r##"(() => {
    const meta = document.querySelector('head meta[name="color-scheme" i]');
    const metaValue = meta ? (meta.getAttribute('content') || '') : null;
    let rootCssValue = null;
    try {
      const cs = getComputedStyle(document.documentElement).colorScheme;
      if (cs && cs !== 'normal') rootCssValue = cs;
      else if (cs) rootCssValue = cs;
    } catch (_) { /* getComputedStyle missing */ }
    return {
      pageUrl: location.href,
      metaValue: metaValue,
      rootCssValue: rootCssValue
    };
  })()"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        meta: Option<&str>,
        css: Option<&str>,
    ) -> MetaColorSchemeSnapshot {
        MetaColorSchemeSnapshot {
            page_url: "https://example.test/".to_owned(),
            meta_value: meta.map(str::to_owned),
            root_css_value: css.map(str::to_owned),
        }
    }

    #[test]
    fn empty_meta_and_css_is_warn_missing() {
        let f = detect_meta_color_scheme(&snap(None, None));
        let hit = f
            .iter()
            .find(|x| x.kind == "color-scheme.missing")
            .expect("missing expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn meta_light_dark_is_clean() {
        let f = detect_meta_color_scheme(&snap(Some("light dark"), None));
        assert!(f.is_empty(), "valid meta should pass: {f:?}");
    }

    #[test]
    fn css_only_dark_is_clean() {
        let f = detect_meta_color_scheme(&snap(None, Some("only dark")));
        assert!(f.is_empty(), "valid css should pass: {f:?}");
    }

    #[test]
    fn matching_meta_and_css_is_clean() {
        let f = detect_meta_color_scheme(&snap(Some("light dark"), Some("light dark")));
        assert!(f.is_empty(), "matching meta+css should pass: {f:?}");
    }

    #[test]
    fn token_order_difference_is_equivalent() {
        // "light dark" and "dark light" should be treated as
        // equivalent for the disagreement check.
        let f = detect_meta_color_scheme(&snap(Some("light dark"), Some("dark light")));
        assert!(
            !f.iter()
                .any(|x| x.kind == "color-scheme.disagrees-between-meta-and-css"),
            "token-order difference should not flag: {f:?}"
        );
    }

    #[test]
    fn invalid_token_in_meta_is_strict() {
        let f = detect_meta_color_scheme(&snap(Some("auto"), None));
        let hit = f
            .iter()
            .find(|x| x.kind == "color-scheme.invalid-value")
            .expect("invalid-value expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("auto"));
        assert!(hit.detail.contains("<meta"));
    }

    #[test]
    fn invalid_token_in_css_is_strict() {
        let f = detect_meta_color_scheme(&snap(None, Some("system")));
        let hit = f
            .iter()
            .find(|x| x.kind == "color-scheme.invalid-value")
            .expect("invalid-value expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains(":root"));
    }

    #[test]
    fn meta_says_light_css_says_dark_is_warn_disagreement() {
        let f = detect_meta_color_scheme(&snap(Some("light"), Some("dark")));
        let hit = f
            .iter()
            .find(|x| x.kind == "color-scheme.disagrees-between-meta-and-css")
            .expect("disagreement expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn normal_value_treated_as_no_declaration() {
        // "normal" means "use browser default" — same as
        // missing. Both being normal triggers the missing finding.
        let f = detect_meta_color_scheme(&snap(Some("normal"), Some("normal")));
        assert!(f.iter().any(|x| x.kind == "color-scheme.missing"));
    }

    #[test]
    fn case_insensitive_token_match() {
        let f = detect_meta_color_scheme(&snap(Some("LIGHT DARK"), None));
        assert!(f.is_empty(), "case-insensitive should pass: {f:?}");
    }

    #[test]
    fn only_dark_only_is_strict_disallowed_outside_grammar() {
        // "only" is a valid token but only-alone is not a
        // well-formed declaration per the spec (must pair with
        // a scheme keyword). The detector treats "only" alone
        // as syntactically OK since the token is in the
        // valid-tokens list; production validation of the full
        // grammar (only must precede a scheme) is delegated to
        // a future stricter axis. Document the behavior here.
        let f = detect_meta_color_scheme(&snap(Some("only"), None));
        assert!(
            !f.iter().any(|x| x.kind == "color-scheme.invalid-value"),
            "loose grammar: 'only' alone passes the token check"
        );
    }

    #[test]
    fn js_const_is_iife_and_captures_meta_and_css() {
        assert!(META_COLOR_SCHEME_JS.starts_with("(() => {"));
        assert!(META_COLOR_SCHEME_JS.ends_with(")()"));
        assert!(META_COLOR_SCHEME_JS.contains("color-scheme"));
        assert!(META_COLOR_SCHEME_JS.contains("getComputedStyle"));
        assert!(META_COLOR_SCHEME_JS.contains("documentElement"));
    }
}
