//! `viewport_meta` — viewport meta tag detector.
//!
//! Three findings — all strict (each is a real, blocking
//! mobile/a11y defect):
//!
//!   * `viewport.missing`           no `<meta name="viewport">`
//!   * `viewport.no-device-width`   tag present but no `width=device-width`
//!   * `viewport.zoom-disabled`     `user-scalable=no|0` or
//!                                  `maximum-scale ≤ 1`
//!
//! Mirrors `src/viewportMeta.ts` — JS string + finding kinds +
//! severities byte-equivalent.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * Pure detector function; no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval. Returns the first viewport meta tag's content
/// attribute (or `present=false` if no such tag).
pub const VIEWPORT_META_JS: &str = r##"(() => {
    const m = document.querySelector('head meta[name="viewport"]');
    if (!m) return { present: false, content: '' };
    const c = m.getAttribute('content') || '';
    return { present: true, content: c };
})()"##;

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ViewportMetaSnapshot {
    /// Page URL at capture time.
    pub page_url: String,
    /// True iff at least one `<meta name="viewport">` exists in head.
    pub present: bool,
    /// Raw `content` attribute of the first viewport meta tag, empty
    /// if `present == false`.
    pub content: String,
}

/// Parse a viewport `content` string into a key→value map.
///
/// `width=device-width, initial-scale=1, user-scalable=no` →
///   `{"width": "device-width", "initial-scale": "1", "user-scalable": "no"}`
///
/// Tolerant of whitespace, trailing separators, and flag-style
/// tokens (a key with no `=` maps to empty-string value). Keys are
/// lowercased per spec; values are case-preserved (most are numeric).
#[must_use]
pub fn parse_viewport_content(content: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for segment in content.split([',', ';']) {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(eq) = trimmed.find('=') {
            let k = trimmed[..eq].trim().to_lowercase();
            let v = trimmed[eq + 1..].trim().to_owned();
            if !k.is_empty() {
                out.insert(k, v);
            }
        } else {
            out.insert(trimmed.to_lowercase(), String::new());
        }
    }
    out
}

/// Apply detection rules. Pure function. Mirror of
/// `detectViewportMetaIssues` in `src/viewportMeta.ts`.
#[must_use]
pub fn detect_viewport_meta_issues(snap: &ViewportMetaSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::<crate::AxisFinding>::new();

    if !snap.present {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "viewport.missing".to_owned(),
            detail: "No <meta name=\"viewport\"> in the page <head>. Mobile browsers will render at the legacy 980px default viewport then scale down — text is unreadable, tap targets are unreachable, and WCAG 1.4.10 (Reflow, AA) fails. Add <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">.".to_owned(),
        });
        return out;
    }

    let parts = parse_viewport_content(&snap.content);

    let width_ok = parts
        .get("width")
        .map(|v| v.eq_ignore_ascii_case("device-width"))
        .unwrap_or(false);
    if !width_ok {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "viewport.no-device-width".to_owned(),
            detail: format!(
                "<meta name=\"viewport\"> is present but content lacks 'width=device-width'. Mobile browsers fall back to the legacy 980px default. Got: '{}'. Fix: 'width=device-width, initial-scale=1'.",
                snap.content
            ),
        });
    }

    let user_scalable = parts.get("user-scalable").map(|s| s.to_lowercase()).unwrap_or_default();
    let max_scale_str = parts.get("maximum-scale").cloned().unwrap_or_default();
    let max_scale: Option<f64> = max_scale_str.parse().ok();
    let zoom_disabled = user_scalable == "no"
        || user_scalable == "0"
        || max_scale.map(|v| v <= 1.0).unwrap_or(false);

    if zoom_disabled {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "viewport.zoom-disabled".to_owned(),
            detail: format!(
                "<meta name=\"viewport\"> disables user pinch-zoom (user-scalable='{user_scalable}', maximum-scale='{max_scale_str}'). WCAG 1.4.4 (Resize text, AA): users must be able to scale text up to 200%. Hostile to low-vision users. Remove user-scalable=no and any maximum-scale ≤ 1."
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap_with(content: &str) -> ViewportMetaSnapshot {
        ViewportMetaSnapshot {
            page_url: "http://t/".to_owned(),
            present: true,
            content: content.to_owned(),
        }
    }

    fn snap_missing() -> ViewportMetaSnapshot {
        ViewportMetaSnapshot {
            page_url: "http://t/".to_owned(),
            present: false,
            content: String::new(),
        }
    }

    #[test]
    fn js_brackets_balanced() {
        assert_eq!(VIEWPORT_META_JS.matches('(').count(), VIEWPORT_META_JS.matches(')').count());
        assert_eq!(VIEWPORT_META_JS.matches('{').count(), VIEWPORT_META_JS.matches('}').count());
    }

    #[test]
    fn clean_no_findings() {
        let f = detect_viewport_meta_issues(&snap_with("width=device-width, initial-scale=1"));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn missing_tag_fires_strict() {
        let f = detect_viewport_meta_issues(&snap_missing());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "viewport.missing");
        assert_eq!(f[0].severity, crate::AxisSeverity::Strict);
    }

    #[test]
    fn no_device_width_strict() {
        let f = detect_viewport_meta_issues(&snap_with("initial-scale=1"));
        assert!(f.iter().any(|x| x.kind == "viewport.no-device-width"));
    }

    #[test]
    fn user_scalable_no_strict() {
        let f = detect_viewport_meta_issues(&snap_with(
            "width=device-width, user-scalable=no",
        ));
        assert!(f.iter().any(|x| x.kind == "viewport.zoom-disabled"));
    }

    #[test]
    fn user_scalable_zero_strict() {
        let f = detect_viewport_meta_issues(&snap_with(
            "width=device-width, user-scalable=0",
        ));
        assert!(f.iter().any(|x| x.kind == "viewport.zoom-disabled"));
    }

    #[test]
    fn max_scale_one_strict() {
        let f = detect_viewport_meta_issues(&snap_with(
            "width=device-width, maximum-scale=1",
        ));
        assert!(f.iter().any(|x| x.kind == "viewport.zoom-disabled"));
    }

    #[test]
    fn max_scale_below_one_strict() {
        let f = detect_viewport_meta_issues(&snap_with(
            "width=device-width, maximum-scale=0.9",
        ));
        assert!(f.iter().any(|x| x.kind == "viewport.zoom-disabled"));
    }

    #[test]
    fn max_scale_two_passes() {
        let f = detect_viewport_meta_issues(&snap_with(
            "width=device-width, maximum-scale=2",
        ));
        assert!(!f.iter().any(|x| x.kind == "viewport.zoom-disabled"));
    }

    #[test]
    fn whitespace_tolerated() {
        let f = detect_viewport_meta_issues(&snap_with(
            "  width = device-width ,  initial-scale = 1 , ",
        ));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn keys_case_insensitive() {
        let f = detect_viewport_meta_issues(&snap_with("WIDTH=device-width"));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn parser_flag_token_no_eq() {
        let p = parse_viewport_content("foo, width=device-width");
        assert_eq!(p.get("foo").map(String::as_str), Some(""));
        assert_eq!(p.get("width").map(String::as_str), Some("device-width"));
    }

    #[test]
    fn combined_breakage_two_findings() {
        let f = detect_viewport_meta_issues(&snap_with(
            "initial-scale=1, user-scalable=no",
        ));
        assert!(f.iter().any(|x| x.kind == "viewport.no-device-width"));
        assert!(f.iter().any(|x| x.kind == "viewport.zoom-disabled"));
    }
}
