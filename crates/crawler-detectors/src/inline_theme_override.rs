//! `inline_theme_override` — flag inline `style="color: ..."` /
//! `style="background[-color]: ..."` declarations that bypass
//! the page's theme cascade.
//!
//! Findings:
//!
//!   * `inline_theme_override.color`      warn   element has
//!     `style` attribute setting `color:`. Hard-codes a foreground
//!     color outside the theme system; the same element can't
//!     render correctly in light + dark + AMOLED variants.
//!   * `inline_theme_override.background` warn   element has
//!     `style` attribute setting `background` or `background-color`.
//!     Same problem on the surface side: a hard-coded background
//!     fights the theme cascade and shows up as a visible
//!     mismatch on any non-default theme.
//!
//! warn-only — runtime doesn't break, but the design system does.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval — walks every element with a `style` attribute,
/// returns the set of {tag, color?, background?} hits.
pub const INLINE_THEME_OVERRIDE_JS: &str = r##"(() => {
    const hits = [];
    const els = document.querySelectorAll('[style]');
    for (let i = 0; i < els.length; i++) {
        const el = els[i];
        const style = (el.getAttribute('style') || '').toLowerCase();
        if (!style.trim()) continue;
        // Tokenize on ';' then strip leading whitespace; check
        // each declaration's property name.
        const decls = style.split(';');
        let hasColor = false;
        let hasBg = false;
        for (let j = 0; j < decls.length; j++) {
            const colonIdx = decls[j].indexOf(':');
            if (colonIdx < 0) continue;
            const prop = decls[j].slice(0, colonIdx).trim();
            // Exact `color` only — `border-color`, `outline-color`,
            // `caret-color`, `accent-color` are less impactful and
            // can legitimately differ per element.
            if (prop === 'color') hasColor = true;
            if (prop === 'background' || prop === 'background-color') hasBg = true;
        }
        if (!hasColor && !hasBg) continue;
        const tag = el.tagName.toLowerCase();
        // Skip elements where inline color/bg IS the convention:
        // SVG fills, MathML, math-related attrs.
        if (tag === 'svg' || tag === 'path' || tag === 'g' ||
            tag === 'rect' || tag === 'circle' || tag === 'polyline' ||
            tag === 'polygon' || tag === 'ellipse' || tag === 'line' ||
            tag === 'use' || tag === 'defs' || tag === 'linearGradient' ||
            tag === 'stop') continue;
        hits.push({
            tag,
            color: hasColor,
            background: hasBg,
            sample_style: (el.getAttribute('style') || '').slice(0, 120),
        });
    }
    return { hits };
})()"##;

/// One inline-theme-override hit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct Hit {
    /// Element tag (lowercase).
    pub tag: String,
    /// True if a `color:` declaration was present.
    pub color: bool,
    /// True if a `background` or `background-color:` declaration
    /// was present.
    pub background: bool,
    /// First ~120 chars of the offending style attribute (for
    /// operator-readable diagnostics in the finding detail).
    pub sample_style: String,
}

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub struct InlineThemeOverrideSnapshot {
    /// Page URL.
    pub page_url: String,
    /// Hits — one per offending element.
    pub hits: Vec<Hit>,
}

/// Pure detector.
#[must_use]
pub fn detect_inline_theme_overrides(
    snap: &InlineThemeOverrideSnapshot,
) -> Vec<crate::AxisFinding> {
    let mut out = Vec::new();
    for hit in &snap.hits {
        if hit.color {
            out.push(crate::AxisFinding {
                severity: crate::AxisSeverity::Warn,
                kind: "inline_theme_override.color".to_owned(),
                detail: format!(
                    "<{}> has inline style=\"...; color: ...; ...\" (sample: {:?}) — hard-codes a foreground color outside the theme system. The same element can't render correctly in light + dark + AMOLED variants. Move the color into a CSS custom property or a token-aware class.",
                    hit.tag, hit.sample_style
                ),
            });
        }
        if hit.background {
            out.push(crate::AxisFinding {
                severity: crate::AxisSeverity::Warn,
                kind: "inline_theme_override.background".to_owned(),
                detail: format!(
                    "<{}> has inline style=\"...; background[-color]: ...; ...\" (sample: {:?}) — hard-codes a surface color outside the theme system. Shows up as a visible theme mismatch the moment the user switches modes. Move to a token-aware class.",
                    hit.tag, hit.sample_style
                ),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(hits: Vec<Hit>) -> InlineThemeOverrideSnapshot {
        InlineThemeOverrideSnapshot {
            page_url: "http://t/".to_owned(),
            hits,
        }
    }

    fn hit(tag: &str, color: bool, background: bool) -> Hit {
        Hit {
            tag: tag.to_owned(),
            color,
            background,
            sample_style: "color:#abc;".to_owned(),
        }
    }

    #[test]
    fn color_only_flags_one_finding() {
        let f = detect_inline_theme_overrides(&snap(vec![hit("p", true, false)]));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "inline_theme_override.color");
    }

    #[test]
    fn background_only_flags_one_finding() {
        let f = detect_inline_theme_overrides(&snap(vec![hit("div", false, true)]));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "inline_theme_override.background");
    }

    #[test]
    fn both_color_and_background_flag_independently() {
        let f = detect_inline_theme_overrides(&snap(vec![hit("span", true, true)]));
        assert_eq!(f.len(), 2);
        assert!(f.iter().any(|x| x.kind == "inline_theme_override.color"));
        assert!(f
            .iter()
            .any(|x| x.kind == "inline_theme_override.background"));
    }

    #[test]
    fn no_hits_no_findings() {
        let f = detect_inline_theme_overrides(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn multiple_hits_emit_independent_findings() {
        let f = detect_inline_theme_overrides(&snap(vec![
            hit("p", true, false),
            hit("div", false, true),
            hit("span", true, false),
        ]));
        assert_eq!(f.len(), 3);
    }
}
