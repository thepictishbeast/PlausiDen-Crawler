//! `iframe_allow_attribute` — `<iframe allow>` permissions
//! audit.
//!
//! Sibling axis to `iframe_sandbox` (which audits the
//! `sandbox` attribute's allowlist) and `iframe_title` (which
//! audits the accessible name). This detector audits the
//! `allow=` attribute, which delegates browser Permissions
//! Policy features (camera, microphone, geolocation, etc.) into
//! the embedded frame.
//!
//! ## The Permissions Policy delegation contract
//!
//! Per the W3C Permissions Policy spec + HTML living standard,
//! `<iframe allow="…">` specifies which features the embedded
//! document is permitted to use. Common features:
//!
//! * `camera`, `microphone`, `geolocation`
//! * `payment`, `web-share`
//! * `fullscreen`, `autoplay`
//! * `display-capture`, `clipboard-read`, `clipboard-write`
//!
//! Operators routinely either over-delegate (broad `allow=
//! "camera *; microphone *"` on a marketing iframe that has no
//! camera use) or under-delegate (forget to add the feature an
//! embedded widget needs, which silently fails inside the
//! frame).
//!
//! ## Findings
//!
//! * `iframe-allow.over-permissive` warn — `allow=` value
//!   carries one or more high-impact features (camera /
//!   microphone / geolocation / payment / display-capture /
//!   clipboard-read / clipboard-write) AND the iframe's src
//!   appears to be a third-party host. Privacy + security
//!   concern.
//! * `iframe-allow.uses-deprecated-token` strict — `allow=`
//!   value contains tokens that have been deprecated /
//!   removed from the Permissions Policy registry
//!   (`document-domain`, `vibrate`, `sync-xhr`). Per the
//!   spec, deprecated tokens produce no-op or warning console
//!   noise; behaviour is platform-specific and operator likely
//!   does not realise.
//! * `iframe-allow.malformed-token` strict — `allow=` value
//!   contains tokens with invalid syntax (whitespace inside a
//!   token, missing semicolon separator between features).
//!
//! Out of scope:
//!
//! * `sandbox=` attribute — covered by `iframe_sandbox`.
//! * `referrerpolicy=` — covered by future axis.
//! * Cross-checking against the page's outer Permissions-
//!   Policy header — covered by `permissions_policy` axis.
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

/// High-impact Permissions Policy features. Delegating these
/// into a third-party iframe is a privacy / security concern.
const HIGH_IMPACT_FEATURES: &[&str] = &[
    "camera",
    "microphone",
    "geolocation",
    "payment",
    "display-capture",
    "clipboard-read",
    "clipboard-write",
    "screen-wake-lock",
    "usb",
    "serial",
    "midi",
    "hid",
];

/// Deprecated / removed Permissions Policy feature tokens.
const DEPRECATED_FEATURES: &[&str] =
    &["document-domain", "vibrate", "sync-xhr", "speaker"];

/// One captured `<iframe>` element with its allow + src signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct IframeAllowEntry {
    /// CSS-ish selector pointing at the iframe.
    pub selector: String,
    /// Raw `allow=` attribute value (trimmed).
    pub allow_value: String,
    /// Resolved origin of the iframe's `src` (scheme + host
    /// + port). `None` when src is missing or not HTTP(S).
    pub src_origin: Option<String>,
    /// Page's own origin captured at snapshot time.
    pub page_origin: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct IframeAllowAttributeSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every `<iframe allow>` on the page.
    pub entries: Vec<IframeAllowEntry>,
}

/// Detector.
#[must_use]
pub fn detect_iframe_allow_attribute(
    snap: &IframeAllowAttributeSnapshot,
) -> Vec<AxisFinding> {
    let mut over_permissive: Vec<String> = Vec::new();
    let mut deprecated: Vec<String> = Vec::new();
    let mut malformed: Vec<String> = Vec::new();

    for entry in &snap.entries {
        let raw = entry.allow_value.trim();
        if raw.is_empty() {
            continue;
        }

        // Parse into tokens: features separated by `;`, each
        // feature is "name [allowlist...]".
        let mut feature_names: Vec<String> = Vec::new();
        let mut found_malformed = false;
        for feat in raw.split(';') {
            let feat = feat.trim();
            if feat.is_empty() {
                continue;
            }
            let name = feat.split_whitespace().next().unwrap_or("");
            if name.is_empty() {
                found_malformed = true;
                continue;
            }
            // Token must be alphanumeric + dashes.
            if !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
            {
                found_malformed = true;
                continue;
            }
            feature_names.push(name.to_ascii_lowercase());
        }

        if found_malformed {
            malformed.push(format!("{} (allow={})", entry.selector, raw));
        }

        let used_deprecated: Vec<&str> = feature_names
            .iter()
            .filter(|n| {
                DEPRECATED_FEATURES
                    .iter()
                    .any(|d| d.eq_ignore_ascii_case(n))
            })
            .map(String::as_str)
            .collect();
        if !used_deprecated.is_empty() {
            deprecated.push(format!(
                "{} (deprecated tokens={})",
                entry.selector,
                used_deprecated.join(",")
            ));
        }

        // Over-permissive: only flag if cross-origin AND
        // contains high-impact features.
        let cross_origin = match (&entry.src_origin, entry.page_origin.is_empty()) {
            (Some(src), false) => !origins_equal(src, &entry.page_origin),
            _ => false,
        };
        if cross_origin {
            let high_impact: Vec<&str> = feature_names
                .iter()
                .filter(|n| {
                    HIGH_IMPACT_FEATURES
                        .iter()
                        .any(|f| f.eq_ignore_ascii_case(n))
                })
                .map(String::as_str)
                .collect();
            if !high_impact.is_empty() {
                over_permissive.push(format!(
                    "{} (src_origin={}, high_impact={})",
                    entry.selector,
                    entry.src_origin.as_deref().unwrap_or("?"),
                    high_impact.join(",")
                ));
            }
        }
    }

    let mut findings = Vec::new();
    let total = snap.entries.len();

    if !deprecated.is_empty() {
        let preview = preview_examples(
            &deprecated.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "iframe-allow.uses-deprecated-token".to_owned(),
            detail: format!(
                "{} of {} <iframe allow> attribute(s) use deprecated Permissions Policy tokens. Examples: {}",
                deprecated.len(),
                total,
                preview
            ),
        });
    }

    if !malformed.is_empty() {
        let preview = preview_examples(
            &malformed.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "iframe-allow.malformed-token".to_owned(),
            detail: format!(
                "{} of {} <iframe allow> attribute(s) contain malformed tokens. Examples: {}",
                malformed.len(),
                total,
                preview
            ),
        });
    }

    if !over_permissive.is_empty() {
        let preview = preview_examples(
            &over_permissive
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "iframe-allow.over-permissive".to_owned(),
            detail: format!(
                "{} of {} <iframe allow> attribute(s) delegate high-impact features to a cross-origin frame. Examples: {}",
                over_permissive.len(),
                total,
                preview
            ),
        });
    }

    findings
}

fn origins_equal(a: &str, b: &str) -> bool {
    a.trim_end_matches('/')
        .eq_ignore_ascii_case(b.trim_end_matches('/'))
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

/// Page-side eval. Walks every `<iframe allow>` and captures
/// the allow value + resolved src origin.
pub const IFRAME_ALLOW_ATTRIBUTE_JS: &str = r##"(() => {
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

    const pageOrigin = location.origin;
    const entries = Array.from(document.querySelectorAll('iframe[allow]')).map(function(f) {
      const allow = (f.getAttribute('allow') || '').trim();
      const src = f.getAttribute('src') || '';
      let srcOrigin = null;
      if (src) {
        try {
          const u = new URL(src, location.href);
          if (u.protocol === 'http:' || u.protocol === 'https:') {
            srcOrigin = u.origin;
          }
        } catch (_) { /* unparseable */ }
      }
      return {
        selector: selectorOf(f),
        allowValue: allow,
        srcOrigin: srcOrigin,
        pageOrigin: pageOrigin
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

    fn entry(
        sel: &str,
        allow: &str,
        src_origin: Option<&str>,
        page_origin: &str,
    ) -> IframeAllowEntry {
        IframeAllowEntry {
            selector: sel.to_owned(),
            allow_value: allow.to_owned(),
            src_origin: src_origin.map(str::to_owned),
            page_origin: page_origin.to_owned(),
        }
    }

    fn snap(entries: Vec<IframeAllowEntry>) -> IframeAllowAttributeSnapshot {
        IframeAllowAttributeSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_iframe_allow_attribute(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn same_origin_high_impact_is_clean() {
        // Same-origin frame can hold high-impact features
        // without flagging — operator owns the embedded page.
        let f = detect_iframe_allow_attribute(&snap(vec![entry(
            "iframe#own",
            "camera; microphone",
            Some("https://example.test"),
            "https://example.test",
        )]));
        assert!(f.is_empty(), "same-origin should pass: {f:?}");
    }

    #[test]
    fn cross_origin_high_impact_is_warn() {
        let f = detect_iframe_allow_attribute(&snap(vec![entry(
            "iframe.embed",
            "camera; microphone",
            Some("https://widget.other.net"),
            "https://example.test",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "iframe-allow.over-permissive")
            .expect("over-permissive expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("camera"));
        assert!(hit.detail.contains("widget.other.net"));
    }

    #[test]
    fn deprecated_token_is_strict() {
        let f = detect_iframe_allow_attribute(&snap(vec![entry(
            "iframe#legacy",
            "document-domain; sync-xhr",
            Some("https://example.test"),
            "https://example.test",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "iframe-allow.uses-deprecated-token")
            .expect("deprecated expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("document-domain"));
        assert!(hit.detail.contains("sync-xhr"));
    }

    #[test]
    fn malformed_token_is_strict() {
        // Token starting with a digit / containing `@` is
        // malformed.
        let f = detect_iframe_allow_attribute(&snap(vec![entry(
            "iframe#bad",
            "camera@host; microphone",
            Some("https://example.test"),
            "https://example.test",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "iframe-allow.malformed-token")
            .expect("malformed expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
    }

    #[test]
    fn low_impact_features_dont_flag_over_permissive() {
        // fullscreen / autoplay are common embed permissions
        // and not on the high-impact list.
        let f = detect_iframe_allow_attribute(&snap(vec![entry(
            "iframe#video",
            "fullscreen; autoplay",
            Some("https://video.other.net"),
            "https://example.test",
        )]));
        assert!(
            !f.iter()
                .any(|x| x.kind == "iframe-allow.over-permissive"),
            "low-impact features should not flag: {f:?}"
        );
    }

    #[test]
    fn each_high_impact_feature_flags_cross_origin() {
        let features = [
            "camera",
            "microphone",
            "geolocation",
            "payment",
            "display-capture",
            "clipboard-read",
            "clipboard-write",
            "screen-wake-lock",
            "usb",
            "serial",
            "midi",
            "hid",
        ];
        for feat in features {
            let f = detect_iframe_allow_attribute(&snap(vec![entry(
                "iframe#x",
                feat,
                Some("https://other.net"),
                "https://example.test",
            )]));
            assert!(
                f.iter().any(|x| x.kind == "iframe-allow.over-permissive"),
                "feature {feat} should flag cross-origin"
            );
        }
    }

    #[test]
    fn case_insensitive_feature_match() {
        let f = detect_iframe_allow_attribute(&snap(vec![entry(
            "iframe",
            "CAMERA",
            Some("https://other.net"),
            "https://example.test",
        )]));
        assert!(f
            .iter()
            .any(|x| x.kind == "iframe-allow.over-permissive"));
    }

    #[test]
    fn empty_allow_value_is_ignored() {
        let f = detect_iframe_allow_attribute(&snap(vec![entry(
            "iframe", "  ", Some("https://other.net"), "https://example.test",
        )]));
        assert!(f.is_empty(), "whitespace-only allow value should ignore: {f:?}");
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| {
                entry(
                    &format!("iframe#x{i}"),
                    "camera",
                    Some("https://other.net"),
                    "https://example.test",
                )
            })
            .collect();
        let f = detect_iframe_allow_attribute(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "iframe-allow.over-permissive")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_iframes() {
        assert!(IFRAME_ALLOW_ATTRIBUTE_JS.starts_with("(() => {"));
        assert!(IFRAME_ALLOW_ATTRIBUTE_JS.ends_with(")()"));
        assert!(IFRAME_ALLOW_ATTRIBUTE_JS.contains("iframe[allow]"));
        assert!(IFRAME_ALLOW_ATTRIBUTE_JS.contains("new URL"));
        assert!(IFRAME_ALLOW_ATTRIBUTE_JS.contains("location.origin"));
    }
}
