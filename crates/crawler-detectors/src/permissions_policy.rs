//! `permissions_policy` — Permissions-Policy response-header audit.
//! T76 port of `src/permissionsPolicy.ts`.
//!
//! Permissions-Policy (W3C, formerly Feature-Policy) gates which
//! browser APIs the page AND any embedded iframes can use. Without
//! the header — or with overly permissive directives — third-party
//! scripts and iframes inherit ambient permission to access camera,
//! microphone, geolocation, payment, USB, serial, MIDI, motion
//! sensors (keystroke-recovery side channel), display capture, etc.
//!
//! Findings (kinds + severities mirror TS byte-for-byte):
//!
//!   * `permissions-policy.missing`             — warn
//!   * `permissions-policy.allow-all-<feature>` — strict (per high-risk feature)
//!   * `permissions-policy.invalid`             — warn
//!   * `permissions-policy.high-risk-omitted`   — warn
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One parsed `feature=allowlist` directive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ParsedDirective {
    /// Lowercased feature name.
    pub feature: String,
    /// Raw allowlist text inside the parens (or `*` for unparenned star).
    /// Empty string for `()` (deny).
    pub allowlist_raw: String,
    /// True iff the allowlist is `*` — every iframe allowed.
    pub is_allow_all: bool,
    /// True iff the allowlist is `()` — denied for everyone.
    pub is_deny: bool,
    /// True iff `self` appears in the allowlist.
    pub has_self: bool,
    /// Origins beyond `self` listed in the allowlist (unquoted).
    pub origins: Vec<String>,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PermissionsPolicySnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Raw header value, or `None`.
    pub raw: Option<String>,
    /// Parsed directives in declaration order.
    pub directives: Vec<ParsedDirective>,
    /// True iff the header was present but unparseable into ANY directive.
    pub unparseable: bool,
}

/// High-risk features that trigger the strict `allow-all-<feature>`
/// finding when set to `*`. Drawn from the W3C Permissions-Policy
/// spec's privacy-impacting features list + payment + device-access
/// APIs known to enable cross-origin fingerprinting + motion sensors
/// (TouchLogger / AccessLogger keystroke side channel on mobile).
fn high_risk_features() -> BTreeSet<&'static str> {
    [
        "camera",
        "microphone",
        "geolocation",
        "payment",
        "usb",
        "serial",
        "midi",
        "hid",
        "bluetooth",
        "accelerometer",
        "gyroscope",
        "magnetometer",
        "display-capture",
        "screen-wake-lock",
    ]
    .into_iter()
    .collect()
}

fn header_lookup<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

/// Parse one `feature=allowlist` directive.
///
/// Returns `None` on malformed input (no `=`, empty value).
fn parse_directive(raw: &str) -> Option<ParsedDirective> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let eq = trimmed.find('=')?;
    if eq == 0 {
        return None;
    }
    let feature = trimmed[..eq].trim().to_ascii_lowercase();
    if feature.is_empty() {
        return None;
    }
    let value = trimmed[eq + 1..].trim();
    if value.is_empty() {
        return None;
    }

    if value == "*" {
        return Some(ParsedDirective {
            feature,
            allowlist_raw: "*".into(),
            is_allow_all: true,
            is_deny: false,
            has_self: false,
            origins: Vec::new(),
        });
    }
    if !(value.starts_with('(') && value.ends_with(')')) {
        if value.eq_ignore_ascii_case("self") {
            return Some(ParsedDirective {
                feature,
                allowlist_raw: "self".into(),
                is_allow_all: false,
                is_deny: false,
                has_self: true,
                origins: Vec::new(),
            });
        }
        return None;
    }
    let inner = value[1..value.len() - 1].trim();
    if inner.is_empty() {
        return Some(ParsedDirective {
            feature,
            allowlist_raw: String::new(),
            is_allow_all: false,
            is_deny: true,
            has_self: false,
            origins: Vec::new(),
        });
    }
    let mut has_self = false;
    let mut origins = Vec::new();
    let mut allow_all = false;
    for t in inner.split_whitespace() {
        if t == "*" {
            allow_all = true;
        } else if t.eq_ignore_ascii_case("self") {
            has_self = true;
        } else if (t.starts_with('"') && t.ends_with('"'))
            || (t.starts_with('\'') && t.ends_with('\''))
        {
            origins.push(t[1..t.len() - 1].to_owned());
        } else {
            // Bare origin (non-spec but common in the wild).
            origins.push(t.to_owned());
        }
    }
    Some(ParsedDirective {
        feature,
        allowlist_raw: inner.to_owned(),
        is_allow_all: allow_all,
        is_deny: false,
        has_self,
        origins,
    })
}

/// Build a snapshot from a captured headers map.
pub fn build_permissions_policy_snapshot(
    page_url: &str,
    headers: &BTreeMap<String, String>,
) -> PermissionsPolicySnapshot {
    let page_is_localhost = is_localhost(page_url);
    let raw = header_lookup(headers, "permissions-policy").cloned();
    let Some(raw_str) = raw.clone() else {
        return PermissionsPolicySnapshot {
            page_url: page_url.to_owned(),
            page_is_localhost,
            raw: None,
            directives: Vec::new(),
            unparseable: false,
        };
    };

    let parts: Vec<&str> = raw_str
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let directives: Vec<ParsedDirective> =
        parts.iter().filter_map(|p| parse_directive(p)).collect();
    let unparseable = !parts.is_empty() && directives.is_empty();

    PermissionsPolicySnapshot {
        page_url: page_url.to_owned(),
        page_is_localhost,
        raw: Some(raw_str),
        directives,
        unparseable,
    }
}

fn slice_for_log(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Pure detector: snapshot → findings. No I/O.
pub fn detect_permissions_policy_issues(snap: &PermissionsPolicySnapshot) -> Vec<AxisFinding> {
    if snap.page_is_localhost {
        return Vec::new();
    }
    let mut out = Vec::new();

    if snap.raw.is_none() {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "permissions-policy.missing".into(),
            detail: "No Permissions-Policy header on this page. Every browser API (camera, microphone, geolocation, payment, USB, serial, MIDI, accelerometer, etc.) defaults to '*' — every embedded iframe inherits ambient permission. Set the header to deny each high-risk feature you don't use, e.g. 'Permissions-Policy: camera=(), microphone=(), geolocation=(), payment=()'.".into(),
        });
        return out;
    }

    if snap.unparseable {
        let raw_snippet = slice_for_log(snap.raw.as_deref().unwrap_or(""), 200);
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "permissions-policy.invalid".into(),
            detail: format!(
                "Permissions-Policy header is set but couldn't be parsed into any valid directive. Browsers ignore unparseable values, so the header has no effect. Header value: '{raw_snippet}'."
            ),
        });
        return out;
    }

    let high_risk = high_risk_features();

    for d in &snap.directives {
        if !high_risk.contains(d.feature.as_str()) {
            continue;
        }
        if d.is_allow_all {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: format!("permissions-policy.allow-all-{}", d.feature),
                detail: format!(
                    "Permissions-Policy directive '{}={}' explicitly allows every embedded iframe to use a high-risk feature. Restrict to '{}=()' (denied) or '{}=(self)' (page-only) unless an embed genuinely needs it.",
                    d.feature, d.allowlist_raw, d.feature, d.feature
                ),
            });
        }
    }

    let declared: BTreeSet<&str> = snap.directives.iter().map(|d| d.feature.as_str()).collect();
    let omitted: Vec<&str> = high_risk
        .iter()
        .filter(|f| !declared.contains(*f))
        .copied()
        .collect();
    if !omitted.is_empty() && !snap.directives.is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "permissions-policy.high-risk-omitted".into(),
            detail: format!(
                "Permissions-Policy declares {} directive(s) but omits {} high-risk feature(s) which therefore default to '*'. Add restrictions for: {}.",
                snap.directives.len(),
                omitted.len(),
                omitted.join(", ")
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> &'static str {
        "https://example.com/"
    }

    fn build(headers: &[(&str, &str)]) -> PermissionsPolicySnapshot {
        let map: BTreeMap<String, String> = headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        build_permissions_policy_snapshot(page(), &map)
    }

    #[test]
    fn localhost_skipped() {
        let map = BTreeMap::new();
        let s = build_permissions_policy_snapshot("http://localhost/", &map);
        assert!(detect_permissions_policy_issues(&s).is_empty());
    }

    #[test]
    fn missing_header_warns() {
        let s = build(&[]);
        let f = detect_permissions_policy_issues(&s);
        assert!(f.iter().any(|x| x.kind == "permissions-policy.missing"));
    }

    #[test]
    fn parses_deny_directive() {
        let s = build(&[("Permissions-Policy", "camera=()")]);
        assert_eq!(s.directives.len(), 1);
        assert!(s.directives[0].is_deny);
    }

    #[test]
    fn parses_self_only_directive() {
        let s = build(&[("Permissions-Policy", "camera=(self)")]);
        assert!(s.directives[0].has_self);
        assert!(!s.directives[0].is_allow_all);
    }

    #[test]
    fn parses_star_directive() {
        let s = build(&[("Permissions-Policy", "camera=*")]);
        assert!(s.directives[0].is_allow_all);
    }

    #[test]
    fn allow_all_high_risk_is_strict() {
        let s = build(&[("Permissions-Policy", "camera=*")]);
        let f = detect_permissions_policy_issues(&s);
        assert!(f.iter().any(|x| {
            x.kind == "permissions-policy.allow-all-camera" && x.severity == AxisSeverity::Strict
        }));
    }

    #[test]
    fn allow_all_low_risk_does_not_fire_strict() {
        // 'fullscreen' is not in the HIGH_RISK set.
        let s = build(&[(
            "Permissions-Policy",
            "fullscreen=*, camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), midi=(), hid=(), bluetooth=(), accelerometer=(), gyroscope=(), magnetometer=(), display-capture=(), screen-wake-lock=()",
        )]);
        let f = detect_permissions_policy_issues(&s);
        assert!(!f
            .iter()
            .any(|x| x.kind.starts_with("permissions-policy.allow-all-")));
    }

    #[test]
    fn unparseable_warns() {
        // Header is non-empty after split, but no = signs → no parseable directives
        let s = build(&[("Permissions-Policy", "garbage, also-garbage")]);
        let f = detect_permissions_policy_issues(&s);
        assert!(f.iter().any(|x| x.kind == "permissions-policy.invalid"));
    }

    #[test]
    fn high_risk_omitted_warns() {
        let s = build(&[("Permissions-Policy", "camera=()")]);
        let f = detect_permissions_policy_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "permissions-policy.high-risk-omitted"));
    }

    #[test]
    fn fully_locked_down_policy_is_clean() {
        let s = build(&[(
            "Permissions-Policy",
            "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), midi=(), hid=(), bluetooth=(), accelerometer=(), gyroscope=(), magnetometer=(), display-capture=(), screen-wake-lock=()",
        )]);
        let f = detect_permissions_policy_issues(&s);
        assert!(
            f.is_empty(),
            "fully-locked policy should be silent, got: {f:#?}"
        );
    }

    #[test]
    fn parses_quoted_origin_in_allowlist() {
        let s = build(&[("Permissions-Policy", "camera=(self \"https://e.com\")")]);
        assert!(s.directives[0].has_self);
        assert_eq!(s.directives[0].origins, vec!["https://e.com".to_string()]);
    }

    #[test]
    fn parses_self_shorthand_without_parens() {
        let s = build(&[("Permissions-Policy", "camera=self")]);
        assert!(s.directives[0].has_self);
    }

    #[test]
    fn header_lookup_case_insensitive() {
        let s = build(&[("PERMISSIONS-POLICY", "camera=()")]);
        assert_eq!(s.directives.len(), 1);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = build(&[("Permissions-Policy", "camera=(), microphone=(self)")]);
        let j = serde_json::to_string(&s).expect("ser");
        let back: PermissionsPolicySnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.directives.len(), s.directives.len());
    }

    #[test]
    fn allow_all_microphone_strict_too() {
        // SECURITY: every high-risk feature must fire individually.
        for feat in [
            "microphone",
            "geolocation",
            "payment",
            "usb",
            "serial",
            "midi",
            "hid",
            "bluetooth",
            "accelerometer",
            "gyroscope",
            "magnetometer",
            "display-capture",
            "screen-wake-lock",
        ] {
            let s = build(&[("Permissions-Policy", &format!("{feat}=*"))]);
            let f = detect_permissions_policy_issues(&s);
            assert!(
                f.iter().any(|x| {
                    x.kind == format!("permissions-policy.allow-all-{feat}")
                        && x.severity == AxisSeverity::Strict
                }),
                "feature {feat} must fire strict allow-all finding"
            );
        }
    }
}
