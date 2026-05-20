//! `cross_origin_form_post` — cross-origin `<form action>` detector.
//!
//! Forward step on rolling Crawler axes (#117). Pairs with
//! `inline_script` / `mixed_content` / `iframe_sandbox` —
//! security-shaped detector family.
//!
//! ## The bug class
//!
//! `<form action="https://different-origin.com/submit">` posts
//! form data to an origin DIFFERENT from the page origin. Three
//! ways this is bad:
//!
//! 1. **Phishing / exfiltration vector.** A compromised CMS that
//!    serves clean HTML but with a tampered form action sends
//!    user-typed credentials / payment info / personal data to
//!    an attacker-controlled origin. The page LOOKS legit, the
//!    form LOOKS native, but the submission lands elsewhere.
//! 2. **CORS / cookie semantics surprise.** Cross-origin POSTs
//!    don't carry the page's session cookies (without explicit
//!    cross-origin CORS setup) — so what looks like a form
//!    submission actually fails silently OR succeeds against an
//!    anonymous endpoint, both subtle bugs.
//! 3. **CSP `form-action` policy violation.** Strict CSPs set
//!    `form-action 'self'` so the browser refuses cross-origin
//!    submissions; pages that ship cross-origin actions silently
//!    break user flows when CSP eventually tightens.
//!
//! ## Findings
//!
//! * `cross-origin-form.action`     strict — `<form action>` URL
//!   resolves to a different origin than the page origin.
//! * `cross-origin-form.formaction` strict — same but the
//!   override is on a submit `<button formaction>` or `<input
//!   formaction>`. HTML5 lets these override the parent form's
//!   action; same risks apply.
//! * `cross-origin-form.unsafe-scheme` warn — action URL uses a
//!   scheme the browser doesn't post to (mailto:, javascript:,
//!   tel:, etc.). The form silently fails / opens email client
//!   instead of submitting. Different bug class but worth
//!   flagging adjacent.
//!
//! Out of scope:
//!
//! * Same-origin actions with hostile paths (catch via existing
//!   `inline_script` for javascript: URIs, this detector targets
//!   ORIGIN mismatch only)
//! * Forms without a method attribute (defaults to GET) —
//!   cross-origin GET is less sensitive (no body posted) but
//!   the detector still flags the action URL for visibility.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; JS const is the only side-effect channel.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured form (or submitter element with formaction) entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CrossOriginFormEntry {
    /// CSS-ish selector pointing at the element.
    pub selector: String,
    /// Element tag — "form" / "button" / "input".
    pub tag: String,
    /// The action / formaction URL as-authored.
    pub action_url: String,
    /// Whether the URL is a "submitter-override" (formaction on
    /// button/input) rather than the form's own action.
    pub is_submitter_override: bool,
    /// Resolved origin of the action URL ("scheme://host:port"),
    /// or None if the URL doesn't have an origin (mailto:,
    /// data:, etc.).
    pub action_origin: Option<String>,
    /// Whether the URL uses a non-submission-safe scheme
    /// (mailto / tel / javascript / data). The JS captures
    /// this flag; the detector classifies separately.
    pub uses_unsafe_scheme: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CrossOriginFormSnapshot {
    /// Top-level page URL (used to derive page_origin).
    pub page_url: String,
    /// Page origin ("scheme://host:port"). Derived browser-side
    /// from window.location to match the browser's same-origin
    /// computation exactly.
    pub page_origin: String,
    /// Every captured form / submitter override.
    pub entries: Vec<CrossOriginFormEntry>,
}

/// Page-side eval. Walks every `<form>` + every `<button[formaction]>`
/// + every `<input[type="submit"][formaction]>`, captures the
/// action URL + resolved origin.
pub const CROSS_ORIGIN_FORM_POST_JS: &str = r##"(() => {
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
      return 'body > ' + parts.join(' > ');
    };
    const UNSAFE_SCHEMES = ['mailto:', 'tel:', 'sms:', 'javascript:', 'data:'];
    const resolveOrigin = function(raw) {
      if (raw === null || raw === undefined || raw.length === 0) return null;
      try {
        const u = new URL(raw, location.href);
        return u.origin;
      } catch (e) {
        return null;
      }
    };
    const isUnsafeScheme = function(raw) {
      if (!raw) return false;
      const lower = raw.trim().toLowerCase();
      for (let i = 0; i < UNSAFE_SCHEMES.length; i++) {
        if (lower.startsWith(UNSAFE_SCHEMES[i])) return true;
      }
      return false;
    };
    const entries = [];
    const forms = document.querySelectorAll('form[action]');
    for (let i = 0; i < forms.length; i++) {
      const el = forms[i];
      const raw = el.getAttribute('action') || '';
      entries.push({
        selector: selectorOf(el),
        tag: 'form',
        actionUrl: raw,
        isSubmitterOverride: false,
        actionOrigin: resolveOrigin(raw),
        usesUnsafeScheme: isUnsafeScheme(raw)
      });
    }
    const submitters = document.querySelectorAll('button[formaction], input[formaction]');
    for (let i = 0; i < submitters.length; i++) {
      const el = submitters[i];
      const raw = el.getAttribute('formaction') || '';
      entries.push({
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase(),
        actionUrl: raw,
        isSubmitterOverride: true,
        actionOrigin: resolveOrigin(raw),
        usesUnsafeScheme: isUnsafeScheme(raw)
      });
    }
    return {
      pageUrl: location.href,
      pageOrigin: location.origin,
      entries: entries
    };
  })()"##;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_cross_origin_form_post(snap: &CrossOriginFormSnapshot) -> Vec<AxisFinding> {
    let mut findings = Vec::new();
    for entry in &snap.entries {
        if entry.uses_unsafe_scheme {
            let attr = if entry.is_submitter_override {
                "formaction"
            } else {
                "action"
            };
            findings.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "cross-origin-form.unsafe-scheme".to_owned(),
                detail: format!(
                    "{} <{}> {attr}=\"{}\" uses an unsubmittable scheme (mailto/tel/javascript/data); the form silently fails to submit or opens an external app. Use http(s) action OR replace the form with an <a href> link if a mailto link is what's intended.",
                    entry.selector, entry.tag, entry.action_url
                ),
            });
            continue;
        }
        let Some(action_origin) = entry.action_origin.as_deref() else {
            // No origin resolved AND not unsafe-scheme → likely
            // relative path that didn't resolve in the JS layer
            // for some reason. Don't fire — false-positive risk.
            continue;
        };
        if action_origin == snap.page_origin {
            continue;
        }
        // Cross-origin action.
        let attr = if entry.is_submitter_override {
            "formaction"
        } else {
            "action"
        };
        let kind = if entry.is_submitter_override {
            "cross-origin-form.formaction"
        } else {
            "cross-origin-form.action"
        };
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: kind.to_owned(),
            detail: format!(
                "{} <{}> {attr}=\"{}\" POSTs to origin `{}` but the page is served from `{}` — cross-origin form action is a phishing/exfil vector AND silently breaks under `Content-Security-Policy: form-action 'self'`. Move the endpoint to a same-origin path (with a reverse-proxy if the actual handler is elsewhere).",
                entry.selector, entry.tag, entry.action_url, action_origin, snap.page_origin
            ),
        });
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        tag: &str,
        action_url: &str,
        is_submitter_override: bool,
        action_origin: Option<&str>,
        uses_unsafe_scheme: bool,
    ) -> CrossOriginFormEntry {
        CrossOriginFormEntry {
            selector: format!("body > {tag}"),
            tag: tag.to_owned(),
            action_url: action_url.to_owned(),
            is_submitter_override,
            action_origin: action_origin.map(str::to_owned),
            uses_unsafe_scheme,
        }
    }

    fn snap(page_origin: &str, entries: Vec<CrossOriginFormEntry>) -> CrossOriginFormSnapshot {
        CrossOriginFormSnapshot {
            page_url: format!("{page_origin}/index.html"),
            page_origin: page_origin.to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_entries_no_findings() {
        assert!(detect_cross_origin_form_post(&snap("https://example.com", vec![])).is_empty());
    }

    #[test]
    fn same_origin_form_action_is_fine() {
        let findings = detect_cross_origin_form_post(&snap(
            "https://example.com",
            vec![entry(
                "form",
                "/submit",
                false,
                Some("https://example.com"),
                false,
            )],
        ));
        assert!(findings.is_empty());
    }

    #[test]
    fn cross_origin_form_action_is_strict() {
        let findings = detect_cross_origin_form_post(&snap(
            "https://example.com",
            vec![entry(
                "form",
                "https://attacker.com/exfil",
                false,
                Some("https://attacker.com"),
                false,
            )],
        ));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "cross-origin-form.action");
        assert!(findings[0].detail.contains("https://attacker.com"));
        assert!(findings[0].detail.contains("https://example.com"));
        assert!(findings[0].detail.contains("phishing/exfil"));
    }

    #[test]
    fn cross_origin_formaction_override_is_distinct_kind() {
        let findings = detect_cross_origin_form_post(&snap(
            "https://example.com",
            vec![entry(
                "button",
                "https://attacker.com/exfil",
                true,
                Some("https://attacker.com"),
                false,
            )],
        ));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "cross-origin-form.formaction");
        assert!(findings[0].detail.contains("formaction"));
    }

    #[test]
    fn mailto_action_is_warn_unsafe_scheme() {
        let findings = detect_cross_origin_form_post(&snap(
            "https://example.com",
            vec![entry(
                "form",
                "mailto:contact@example.com",
                false,
                None,
                true,
            )],
        ));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "cross-origin-form.unsafe-scheme");
        assert!(findings[0].detail.contains("mailto"));
    }

    #[test]
    fn javascript_action_is_warn_unsafe_scheme() {
        let findings = detect_cross_origin_form_post(&snap(
            "https://example.com",
            vec![entry("form", "javascript:doSubmit()", false, None, true)],
        ));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, "cross-origin-form.unsafe-scheme");
    }

    #[test]
    fn unresolved_origin_silent_to_avoid_false_positive() {
        // Action with no resolved origin AND not unsafe-scheme.
        // Could be a malformed URL or a relative path that the JS
        // capture couldn't resolve. Detector silently skips rather
        // than false-firing.
        let findings = detect_cross_origin_form_post(&snap(
            "https://example.com",
            vec![entry("form", "weird-url", false, None, false)],
        ));
        assert!(findings.is_empty());
    }

    #[test]
    fn multiple_entries_emit_one_finding_each() {
        let findings = detect_cross_origin_form_post(&snap(
            "https://example.com",
            vec![
                entry("form", "/safe", false, Some("https://example.com"), false),
                entry(
                    "form",
                    "https://attacker.com/a",
                    false,
                    Some("https://attacker.com"),
                    false,
                ),
                entry(
                    "button",
                    "https://attacker.com/b",
                    true,
                    Some("https://attacker.com"),
                    false,
                ),
                entry("form", "mailto:x@y.com", false, None, true),
            ],
        ));
        // Strict for the 2 cross-origin + warn for the mailto = 3 total
        // (same-origin is silent).
        assert_eq!(findings.len(), 3);
        assert_eq!(findings[0].kind, "cross-origin-form.action");
        assert_eq!(findings[1].kind, "cross-origin-form.formaction");
        assert_eq!(findings[2].kind, "cross-origin-form.unsafe-scheme");
    }

    #[test]
    fn different_port_is_cross_origin() {
        // Same host, different port = cross-origin per the browser's
        // same-origin policy. Origin includes the port.
        let findings = detect_cross_origin_form_post(&snap(
            "https://example.com",
            vec![entry(
                "form",
                "https://example.com:8443/submit",
                false,
                Some("https://example.com:8443"),
                false,
            )],
        ));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn snapshot_serde_camel_case() {
        let s = snap(
            "https://example.com",
            vec![entry(
                "form",
                "/x",
                false,
                Some("https://example.com"),
                false,
            )],
        );
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"pageOrigin\""));
        assert!(j.contains("\"actionUrl\""));
        assert!(j.contains("\"actionOrigin\""));
        assert!(j.contains("\"isSubmitterOverride\""));
        assert!(j.contains("\"usesUnsafeScheme\""));
        let back: CrossOriginFormSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back.entries.len(), 1);
    }

    #[test]
    fn js_eval_const_walks_forms_and_submitters() {
        assert!(CROSS_ORIGIN_FORM_POST_JS.contains("querySelectorAll('form[action]')"));
        assert!(CROSS_ORIGIN_FORM_POST_JS
            .contains("querySelectorAll('button[formaction], input[formaction]')"));
        assert!(CROSS_ORIGIN_FORM_POST_JS.contains("location.origin"));
        assert!(CROSS_ORIGIN_FORM_POST_JS.contains("UNSAFE_SCHEMES"));
        // The unsafe schemes list
        for scheme in ["mailto:", "javascript:", "data:"] {
            assert!(
                CROSS_ORIGIN_FORM_POST_JS.contains(scheme),
                "JS const missing {scheme} in UNSAFE_SCHEMES"
            );
        }
    }
}
