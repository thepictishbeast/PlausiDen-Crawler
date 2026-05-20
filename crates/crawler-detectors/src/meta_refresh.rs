//! `meta_refresh` — `<meta http-equiv="refresh">` detector.
//!
//! `<meta http-equiv="refresh" content="N; url=...">` instructs
//! the browser to auto-refresh or auto-redirect the page after
//! `N` seconds. The construct is a WCAG 2.2.1 failure (Timing
//! Adjustable) AND a back-button trap — when a user clicks Back
//! the redirect fires again, leaving them stuck on the source
//! page. It's also bad for SEO; Google guidance prefers HTTP-
//! level 301/302 redirects.
//!
//! This detector flags every `<meta http-equiv="refresh">` in the
//! captured `<head>`. The severity depends on the shape:
//!
//! * `meta-refresh.auto-redirect` — `content="N; url=..."` with
//!   `N >= 1`. Strict. Back-button trap. Use HTTP 301/302.
//! * `meta-refresh.instant-redirect` — `content="0; url=..."`
//!   (or `0;url=...`). Strict. Still a back-button trap (the
//!   instant redirect happens client-side, breaking history).
//!   Use HTTP 301/302.
//! * `meta-refresh.auto-reload` — `content="N"` (no `url`). The
//!   page reloads itself every N seconds. Strict. Refusing to
//!   stay still is a hostile UX pattern for screen-reader users
//!   and people who type slowly.
//! * `meta-refresh.malformed` — `<meta http-equiv="refresh">` with
//!   no `content` attribute, or content that doesn't match either
//!   `N` or `N; url=...` shape. Warn. Old browsers may still try
//!   to interpret it.
//!
//! Spec reference: <https://html.spec.whatwg.org/#attr-meta-http-equiv-refresh>
//!
//! WCAG SC 2.2.1 (Timing Adjustable): users must be able to turn
//! off, adjust, or extend any time limit set by the content. A
//! `<meta refresh>` provides no in-page control.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public snapshot / entry struct.
//! * Pure detector function; the JS const is the only side-effect
//!   channel.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured `<meta http-equiv="refresh">` element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MetaRefreshEntry {
    /// CSS-ish selector pointing at the offending element.
    pub selector: String,
    /// Raw `content` attribute value (e.g. `"5; url=/next"`).
    /// `None` means the attribute was absent entirely — malformed.
    pub content: Option<String>,
    /// Parsed timeout in seconds. `None` if the content couldn't
    /// be parsed or the attribute was absent.
    pub timeout_seconds: Option<u32>,
    /// Parsed target URL. `None` if no `url=…` clause was present
    /// (i.e. self-reload).
    pub target_url: Option<String>,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MetaRefreshSnapshot {
    /// Top-level page URL (for diagnostic context).
    pub page_url: String,
    /// Every captured offender. Empty = no findings.
    pub entries: Vec<MetaRefreshEntry>,
}

/// Page-side eval. Collects every `<meta http-equiv="refresh">`
/// element regardless of position; HTML spec only honors the FIRST
/// one but downstream tools should still flag stragglers as a
/// content hygiene issue.
pub const META_REFRESH_JS: &str = r##"(() => {
    const entries = [];
    const metas = document.querySelectorAll('meta[http-equiv]');
    for (let i = 0; i < metas.length; i++) {
      const el = metas[i];
      const equiv = (el.getAttribute('http-equiv') || '').trim().toLowerCase();
      if (equiv !== 'refresh') continue;
      const rawContent = el.getAttribute('content');
      let timeoutSeconds = null;
      let targetUrl = null;
      if (rawContent !== null && rawContent.length > 0) {
        const trimmed = rawContent.trim();
        const semiIdx = trimmed.indexOf(';');
        const timePart = semiIdx >= 0 ? trimmed.slice(0, semiIdx).trim() : trimmed;
        const restPart = semiIdx >= 0 ? trimmed.slice(semiIdx + 1).trim() : '';
        const n = parseInt(timePart, 10);
        if (Number.isFinite(n) && n >= 0) {
          timeoutSeconds = n;
        }
        if (restPart.length > 0) {
          const eqIdx = restPart.indexOf('=');
          const key = eqIdx >= 0 ? restPart.slice(0, eqIdx).trim().toLowerCase() : '';
          if (key === 'url' && eqIdx >= 0) {
            let urlRaw = restPart.slice(eqIdx + 1).trim();
            if ((urlRaw.startsWith('"') && urlRaw.endsWith('"')) ||
                (urlRaw.startsWith("'") && urlRaw.endsWith("'"))) {
              urlRaw = urlRaw.slice(1, -1);
            }
            if (urlRaw.length > 0) {
              targetUrl = urlRaw;
            }
          }
        }
      }
      entries.push({
        selector: 'meta[http-equiv="refresh"]',
        content: rawContent,
        timeoutSeconds: timeoutSeconds,
        targetUrl: targetUrl
      });
    }
    return {
      pageUrl: location.href,
      entries: entries
    };
  })()"##;

/// Classify a single entry into the appropriate finding kind.
/// Pure; tested in isolation below.
#[must_use]
fn classify(entry: &MetaRefreshEntry) -> (AxisSeverity, &'static str, String) {
    match (entry.content.as_deref(), entry.timeout_seconds, entry.target_url.as_deref()) {
        // No content attribute at all → malformed (warn).
        (None, _, _) => (
            AxisSeverity::Warn,
            "meta-refresh.malformed",
            "<meta http-equiv=\"refresh\"> with no content attribute".to_owned(),
        ),
        // Content present but couldn't parse a timeout → malformed (warn).
        (Some(raw), None, _) => (
            AxisSeverity::Warn,
            "meta-refresh.malformed",
            format!("<meta http-equiv=\"refresh\"> with unparseable content=\"{raw}\""),
        ),
        // timeout=0 + url → instant client-side redirect (strict).
        (Some(_), Some(0), Some(u)) => (
            AxisSeverity::Strict,
            "meta-refresh.instant-redirect",
            format!(
                "client-side instant redirect to {u}; back-button trap. Use an HTTP 301/302 instead."
            ),
        ),
        // timeout>=1 + url → delayed auto-redirect (strict).
        (Some(_), Some(n), Some(u)) => (
            AxisSeverity::Strict,
            "meta-refresh.auto-redirect",
            format!(
                "auto-redirects to {u} after {n}s; WCAG 2.2.1 + back-button trap. Use an HTTP 301/302 instead."
            ),
        ),
        // No url → page auto-reloads itself (strict regardless of N).
        (Some(_), Some(n), None) => (
            AxisSeverity::Strict,
            "meta-refresh.auto-reload",
            format!(
                "page auto-reloads every {n}s; WCAG 2.2.1 fails. Server-push or polling-fetch is the right pattern."
            ),
        ),
    }
}

/// Pure detector: snapshot → findings. One finding per offending
/// entry — the audit phase can re-aggregate if it wants to, but
/// each offending meta-refresh is a distinct content authoring
/// decision and deserves its own row in the report.
#[must_use]
pub fn detect_meta_refresh(snap: &MetaRefreshSnapshot) -> Vec<AxisFinding> {
    snap.entries
        .iter()
        .map(|entry| {
            let (severity, kind, detail) = classify(entry);
            AxisFinding {
                severity,
                kind: kind.to_owned(),
                detail,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(entries: Vec<MetaRefreshEntry>) -> MetaRefreshSnapshot {
        MetaRefreshSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    fn entry(content: Option<&str>, timeout: Option<u32>, url: Option<&str>) -> MetaRefreshEntry {
        MetaRefreshEntry {
            selector: "meta[http-equiv=\"refresh\"]".to_owned(),
            content: content.map(str::to_owned),
            timeout_seconds: timeout,
            target_url: url.map(str::to_owned),
        }
    }

    #[test]
    fn empty_snapshot_emits_no_findings() {
        let findings = detect_meta_refresh(&snap(Vec::new()));
        assert!(findings.is_empty());
    }

    #[test]
    fn instant_redirect_is_strict() {
        let findings = detect_meta_refresh(&snap(vec![entry(
            Some("0; url=/next"),
            Some(0),
            Some("/next"),
        )]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "meta-refresh.instant-redirect");
        assert!(findings[0].detail.contains("/next"));
    }

    #[test]
    fn delayed_redirect_is_strict_with_seconds_in_detail() {
        let findings = detect_meta_refresh(&snap(vec![entry(
            Some("5; url=/somewhere"),
            Some(5),
            Some("/somewhere"),
        )]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "meta-refresh.auto-redirect");
        assert!(findings[0].detail.contains("after 5s"));
        assert!(findings[0].detail.contains("/somewhere"));
        assert!(findings[0].detail.contains("WCAG 2.2.1"));
    }

    #[test]
    fn auto_reload_without_url_is_strict() {
        let findings = detect_meta_refresh(&snap(vec![entry(Some("30"), Some(30), None)]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "meta-refresh.auto-reload");
        assert!(findings[0].detail.contains("every 30s"));
        assert!(findings[0].detail.contains("WCAG 2.2.1"));
    }

    #[test]
    fn missing_content_attribute_is_warn_malformed() {
        let findings = detect_meta_refresh(&snap(vec![entry(None, None, None)]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "meta-refresh.malformed");
        assert!(findings[0].detail.contains("no content attribute"));
    }

    #[test]
    fn unparseable_content_is_warn_malformed() {
        let findings = detect_meta_refresh(&snap(vec![entry(Some("xyz"), None, None)]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "meta-refresh.malformed");
        assert!(findings[0].detail.contains("xyz"));
    }

    #[test]
    fn multiple_entries_emit_one_finding_each() {
        // Each meta-refresh is a distinct authoring decision —
        // don't aggregate. The audit phase can group if it wants.
        let findings = detect_meta_refresh(&snap(vec![
            entry(Some("0; url=/a"), Some(0), Some("/a")),
            entry(Some("60"), Some(60), None),
        ]));
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].kind, "meta-refresh.instant-redirect");
        assert_eq!(findings[1].kind, "meta-refresh.auto-reload");
    }

    #[test]
    fn snapshot_serde_round_trip() {
        let s = snap(vec![entry(Some("3; url=/next"), Some(3), Some("/next"))]);
        let j = serde_json::to_string(&s).unwrap();
        // camelCase wire shape — pageUrl + targetUrl + timeoutSeconds.
        assert!(j.contains("\"pageUrl\""));
        assert!(j.contains("\"targetUrl\""));
        assert!(j.contains("\"timeoutSeconds\""));
        let back: MetaRefreshSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back.entries.len(), 1);
        assert_eq!(back.entries[0].target_url.as_deref(), Some("/next"));
        assert_eq!(back.entries[0].timeout_seconds, Some(3));
    }

    #[test]
    fn js_eval_string_includes_expected_selector_patterns() {
        // Sanity: the JS const collects from meta[http-equiv]
        // and filters on equiv=='refresh' inside the loop.
        assert!(META_REFRESH_JS.contains("meta[http-equiv]"));
        assert!(META_REFRESH_JS.contains("'refresh'"));
        assert!(META_REFRESH_JS.contains("targetUrl"));
        assert!(META_REFRESH_JS.contains("timeoutSeconds"));
    }

    #[test]
    fn classify_handles_zero_timeout_no_url_as_auto_reload() {
        // Edge case: content="0" with no url. The browser would
        // reload immediately + repeatedly — worst variant of
        // auto-reload. We still classify as auto-reload (not
        // instant-redirect) because there's no URL.
        let (sev, kind, _detail) = classify(&entry(Some("0"), Some(0), None));
        assert_eq!(sev, AxisSeverity::Strict);
        assert_eq!(kind, "meta-refresh.auto-reload");
    }
}
