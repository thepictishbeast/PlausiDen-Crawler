//! `favicon` — page favicon-link detector.
//!
//! Mirror of `src/favicon.ts`. Findings:
//!
//!   * `favicon.missing-link`   warn   no <link rel="icon"> /
//!                                     "shortcut icon" /
//!                                     "apple-touch-icon" /
//!                                     "mask-icon" in head.
//!
//! warn-only — missing favicon doesn't break the page; it just
//! ships a generic browser-tab glyph and reads as unfinished.
//! Companion (broken icon URL) is already covered by the
//! failed-requests axis.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval — count icon-rel link tags in head and collect
/// their distinct rel values for the report.
pub const FAVICON_JS: &str = r##"(() => {
    const links = document.querySelectorAll('head link[rel]');
    const rels = [];
    let count = 0;
    for (let i = 0; i < links.length; i++) {
      const rel = (links[i].getAttribute('rel') || '').toLowerCase().trim();
      if (!rel) continue;
      const tokens = rel.split(/\s+/).filter(Boolean);
      const isIcon = tokens.some(function(t) {
        return t === 'icon' || t === 'shortcut' || t === 'apple-touch-icon' || t === 'mask-icon';
      });
      if (isIcon) {
        count += 1;
        if (rels.indexOf(rel) < 0) rels.push(rel);
      }
    }
    return { iconLinkCount: count, relValues: rels };
})()"##;

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FaviconSnapshot {
    /// Page URL.
    pub page_url: String,
    /// Number of head link tags with an icon-class rel value.
    pub icon_link_count: u32,
    /// Distinct rel attribute values that matched (e.g.
    /// `["icon", "apple-touch-icon", "mask-icon"]`).
    pub rel_values: Vec<String>,
}

#[must_use]
pub fn detect_favicon_issues(snap: &FaviconSnapshot) -> Vec<crate::AxisFinding> {
    if snap.icon_link_count > 0 {
        return Vec::new();
    }
    vec![crate::AxisFinding {
        severity: crate::AxisSeverity::Warn,
        kind: "favicon.missing-link".to_owned(),
        detail: "Page <head> declares no <link rel=\"icon\"> (or apple-touch-icon / mask-icon / shortcut icon). Browsers fall back to fetching /favicon.ico automatically; if that path 404s, browser tabs / bookmarks / history / PWA-install prompts show a generic glyph and the site reads as unfinished. Add at least <link rel=\"icon\" href=\"/favicon.svg\" type=\"image/svg+xml\">.".to_owned(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(count: u32, rels: Vec<&str>) -> FaviconSnapshot {
        FaviconSnapshot {
            page_url: "http://t/".to_owned(),
            icon_link_count: count,
            rel_values: rels.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn js_brackets_balanced() {
        assert_eq!(FAVICON_JS.matches('(').count(), FAVICON_JS.matches(')').count());
        assert_eq!(FAVICON_JS.matches('{').count(), FAVICON_JS.matches('}').count());
    }

    #[test]
    fn icon_present_no_findings() {
        assert!(detect_favicon_issues(&snap(1, vec!["icon"])).is_empty());
    }

    #[test]
    fn missing_warns() {
        let f = detect_favicon_issues(&snap(0, vec![]));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "favicon.missing-link");
        assert_eq!(f[0].severity, crate::AxisSeverity::Warn);
    }

    #[test]
    fn multiple_icons_clean() {
        let f = detect_favicon_issues(&snap(3, vec!["icon", "apple-touch-icon", "mask-icon"]));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn apple_touch_icon_only_clean() {
        let f = detect_favicon_issues(&snap(1, vec!["apple-touch-icon"]));
        assert!(f.is_empty(), "{:?}", f);
    }
}
