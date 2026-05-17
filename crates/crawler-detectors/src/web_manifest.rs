//! `web_manifest` — Web App Manifest declared correctness.
//!
//! Per the W3C Web App Manifest specification: a site that
//! declares `<link rel="manifest" href="…">` SHOULD provide a
//! complete manifest JSON with at minimum:
//!   * `name` (the human-readable application name)
//!   * `short_name` (a short variant for constrained surfaces)
//!   * `start_url` (the URL when launched from a home screen)
//!   * `display` (`"standalone"` | `"fullscreen"` | `"minimal-ui"`
//!     | `"browser"`)
//!   * `icons[]` with at least one 192x192 and one 512x512 PNG
//!     (Chromium + iOS install-prompt requirement)
//!   * `theme_color`
//!   * `background_color`
//!
//! Without these, an Add-to-Home-Screen + native install prompt
//! fails silently. Beyond install: search engines + assistive
//! tech use the manifest for app-name disambiguation on shared
//! domains.
//!
//! Findings:
//!   * `web-manifest.missing`        warn   <link rel=manifest>
//!                                            absent on
//!                                            indexable HTML page
//!   * `web-manifest.bad-json`       strict referenced manifest
//!                                            failed to parse
//!   * `web-manifest.required-field` strict required field absent
//!                                            (name, start_url,
//!                                            display)
//!   * `web-manifest.icon-missing-size` strict no icon at 192x192
//!                                              or 512x512 PNG
//!   * `web-manifest.no-theme-color` warn   theme_color absent
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on snapshot types.
//! * Pure detector function; no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One manifest icon entry as captured by the runner. The runner
/// fetches + parses the manifest JSON; this detector consumes the
/// typed result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ManifestIcon {
    /// `src` field from the manifest icon entry.
    pub src: String,
    /// `sizes` field, e.g. `"192x192"` or `"any"`.
    pub sizes: String,
    /// `type` field, e.g. `"image/png"`.
    pub mime: String,
}

/// Captured manifest state for one page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct WebManifestSnapshot {
    /// Page URL.
    pub page_url: String,
    /// Whether a `<link rel="manifest">` was declared.
    pub link_present: bool,
    /// Whether the runner successfully fetched + parsed the
    /// referenced JSON.
    pub parse_ok: bool,
    /// Parsed manifest fields (only populated when parse_ok).
    pub name: Option<String>,
    /// Short name (≤ 12 chars recommended).
    pub short_name: Option<String>,
    /// `start_url` field.
    pub start_url: Option<String>,
    /// `display` field.
    pub display: Option<String>,
    /// `theme_color` field.
    pub theme_color: Option<String>,
    /// `background_color` field.
    pub background_color: Option<String>,
    /// Parsed icons array.
    pub icons: Vec<ManifestIcon>,
}

/// Page-side eval — only the link presence. Manifest JSON fetch
/// + parse lives in the runner harness (the JSON is on a separate
/// URL).
pub const WEB_MANIFEST_LINK_JS: &str = r##"(() => {
    const l = document.querySelector('link[rel~="manifest"]');
    return {
        pageUrl: window.location.href,
        linkPresent: l !== null,
        manifestHref: l ? (l.getAttribute('href') || '') : ''
    };
})()"##;

fn icon_has_size(icons: &[ManifestIcon], target: &str) -> bool {
    icons.iter().any(|i| {
        i.sizes
            .split_ascii_whitespace()
            .any(|t| t == target || t == "any")
            && i.mime == "image/png"
    })
}

/// Run the detector.
pub fn detect_web_manifest_issues(snap: &WebManifestSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();

    if !snap.link_present {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "web-manifest.missing".into(),
            detail: format!(
                "page {} has no <link rel=\"manifest\">; PWA install + app-name disambiguation degraded",
                snap.page_url
            ),
        });
        return out;
    }

    if !snap.parse_ok {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "web-manifest.bad-json".into(),
            detail: "manifest referenced but failed to parse as JSON".into(),
        });
        return out;
    }

    if snap.name.as_deref().unwrap_or("").is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "web-manifest.required-field".into(),
            detail: "manifest missing required `name` field".into(),
        });
    }
    if snap.start_url.as_deref().unwrap_or("").is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "web-manifest.required-field".into(),
            detail: "manifest missing required `start_url` field".into(),
        });
    }
    if snap.display.as_deref().unwrap_or("").is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "web-manifest.required-field".into(),
            detail: "manifest missing required `display` field".into(),
        });
    }

    if !icon_has_size(&snap.icons, "192x192") {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "web-manifest.icon-missing-size".into(),
            detail:
                "manifest icons do not include a 192x192 PNG (Chromium install-prompt requirement)"
                    .into(),
        });
    }
    if !icon_has_size(&snap.icons, "512x512") {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "web-manifest.icon-missing-size".into(),
            detail:
                "manifest icons do not include a 512x512 PNG (Chromium + iOS install requirement)"
                    .into(),
        });
    }

    if snap.theme_color.as_deref().unwrap_or("").is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "web-manifest.no-theme-color".into(),
            detail: "manifest has no theme_color; browser chrome accent + status bar default"
                .into(),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_snap() -> WebManifestSnapshot {
        WebManifestSnapshot {
            page_url: "https://example.com/".into(),
            link_present: true,
            parse_ok: true,
            name: Some("Example".into()),
            short_name: Some("Ex".into()),
            start_url: Some("/".into()),
            display: Some("standalone".into()),
            theme_color: Some("#1d3a8a".into()),
            background_color: Some("#ffffff".into()),
            icons: vec![
                ManifestIcon {
                    src: "/icon-192.png".into(),
                    sizes: "192x192".into(),
                    mime: "image/png".into(),
                },
                ManifestIcon {
                    src: "/icon-512.png".into(),
                    sizes: "512x512".into(),
                    mime: "image/png".into(),
                },
            ],
        }
    }

    #[test]
    fn complete_manifest_is_clean() {
        let s = ok_snap();
        assert!(detect_web_manifest_issues(&s).is_empty());
    }

    #[test]
    fn link_absent_warns() {
        let mut s = ok_snap();
        s.link_present = false;
        let f = detect_web_manifest_issues(&s);
        assert!(f.iter().any(|x| x.kind == "web-manifest.missing"));
    }

    #[test]
    fn bad_json_is_strict() {
        let mut s = ok_snap();
        s.parse_ok = false;
        let f = detect_web_manifest_issues(&s);
        assert!(f.iter().any(|x| x.kind == "web-manifest.bad-json"));
    }

    #[test]
    fn missing_name_is_strict() {
        let mut s = ok_snap();
        s.name = None;
        let f = detect_web_manifest_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "web-manifest.required-field" && x.detail.contains("name")));
    }

    #[test]
    fn missing_start_url_is_strict() {
        let mut s = ok_snap();
        s.start_url = Some(String::new());
        let f = detect_web_manifest_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "web-manifest.required-field" && x.detail.contains("start_url")));
    }

    #[test]
    fn missing_display_is_strict() {
        let mut s = ok_snap();
        s.display = None;
        let f = detect_web_manifest_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "web-manifest.required-field" && x.detail.contains("display")));
    }

    #[test]
    fn missing_192_icon_is_strict() {
        let mut s = ok_snap();
        s.icons.retain(|i| i.sizes != "192x192");
        let f = detect_web_manifest_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "web-manifest.icon-missing-size" && x.detail.contains("192x192")));
    }

    #[test]
    fn missing_512_icon_is_strict() {
        let mut s = ok_snap();
        s.icons.retain(|i| i.sizes != "512x512");
        let f = detect_web_manifest_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "web-manifest.icon-missing-size" && x.detail.contains("512x512")));
    }

    #[test]
    fn icon_with_sizes_any_satisfies_size_requirement() {
        let mut s = ok_snap();
        s.icons = vec![ManifestIcon {
            src: "/svg.svg".into(),
            // an SVG icon with sizes="any" covers all sizes.
            // But it's not a PNG, so it doesn't satisfy our PNG
            // requirement.
            sizes: "any".into(),
            mime: "image/svg+xml".into(),
        }];
        let f = detect_web_manifest_issues(&s);
        // SVG-only is acceptable in spec, but Chromium + iOS
        // still require PNG fallback; we flag for both sizes.
        assert!(f.iter().any(|x| x.kind == "web-manifest.icon-missing-size"));
    }

    #[test]
    fn theme_color_absent_warns() {
        let mut s = ok_snap();
        s.theme_color = None;
        let f = detect_web_manifest_issues(&s);
        assert!(f.iter().any(|x| x.kind == "web-manifest.no-theme-color"));
    }
}
