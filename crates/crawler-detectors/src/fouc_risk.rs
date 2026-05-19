//! `fouc_risk` — flags Flash-Of-Unstyled-Content anti-patterns
//! in the document head.
//!
//! FOUC is the visible interval between HTML parse and full
//! stylesheet/font application. Common causes:
//!
//! 1. **Body stylesheets without preload** — a `<link rel="stylesheet">`
//!    placed inside `<body>` (or after an early `<script>` that blocks
//!    parser progress) defers paint until the CSS resolves.
//! 2. **`@font-face` without `font-display`** — the browser blocks
//!    text paint waiting for the font (default `block` behavior up
//!    to 3 seconds). The user sees no text, then a jarring swap.
//! 3. **JS-loaded stylesheets** — `document.head.appendChild(link)`
//!    fires after first paint. Visitor sees the unstyled page for
//!    one frame minimum, more if the CSS is large.
//! 4. **Missing critical-CSS inline block** — a same-origin
//!    `<link rel="stylesheet">` in head with no inlined critical
//!    CSS will paint nothing until that file lands.
//!
//! Detection is static analysis of the served HTML — lighter
//! weight than runtime timing and runs in offline replays.
//!
//! ## Severity
//!
//! - `strict` — body has a `<link rel="stylesheet">` in `<body>`,
//!   OR an `@font-face` declaration with `font-display: block` (the
//!   browser default; sites should opt into `swap` / `optional` /
//!   `fallback`).
//! - `warn` — no inline critical-CSS `<style>` block in head AND
//!   any same-origin stylesheet link is present; common when
//!   authors forget the critical-CSS extraction step.
//!
//! AVP-2: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offender or signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FoucRiskHit {
    /// Kind of risk: "body-stylesheet" | "font-display-block" |
    /// "js-loaded-stylesheet" | "missing-critical-css".
    pub kind: String,
    /// One-line excerpt for context (capped at 200 chars).
    pub excerpt: String,
}

/// Captured page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FoucRiskSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every captured risk signal.
    pub hits: Vec<FoucRiskHit>,
    /// True iff the head contained a non-empty `<style>` block
    /// (proxy for "inline critical CSS extraction ran").
    pub has_inline_critical_css: bool,
    /// True iff any same-origin stylesheet link was present.
    pub has_external_stylesheet: bool,
}

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_fouc_risk(snap: &FoucRiskSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();

    let (strict_hits, _warn_hits): (Vec<&FoucRiskHit>, Vec<&FoucRiskHit>) =
        snap.hits.iter().partition(|h| {
            matches!(
                h.kind.as_str(),
                "body-stylesheet" | "font-display-block" | "js-loaded-stylesheet"
            )
        });

    if !strict_hits.is_empty() {
        let examples: Vec<String> = strict_hits
            .iter()
            .take(5)
            .map(|h| format!("{}: \"{}\"", h.kind, h.excerpt))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "fouc-risk.anti-pattern".to_owned(),
            detail: format!(
                "{} FOUC-risk anti-pattern(s) in the served HTML. Causes visible flash of unstyled content (body-stylesheet defers paint; font-display-block hides text up to 3s; js-loaded-stylesheet shows unstyled page for one frame minimum). Examples: {}",
                strict_hits.len(),
                examples.join("; ")
            ),
        });
    }

    // Warn: no inline critical CSS but external stylesheet is loaded.
    if !snap.has_inline_critical_css && snap.has_external_stylesheet {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "fouc-risk.no-critical-css".to_owned(),
            detail: "Head has an external stylesheet link but no inline critical-CSS <style> block. Visitors on slow networks see unstyled content until the stylesheet lands. Extract above-the-fold CSS into an inline <style>; ship the rest as a non-blocking <link rel=\"stylesheet\">.".to_owned(),
        });
    }

    out
}

/// Browser-side capture script.
pub const FOUC_RISK_DOM_CAPTURE_JS: &str = r#"
(() => {
    const hits = [];

    // 1. Body stylesheets.
    const bodyLinks = document.body
      ? document.body.querySelectorAll('link[rel="stylesheet"]')
      : [];
    for (let i = 0; i < bodyLinks.length; i++) {
      const href = bodyLinks[i].getAttribute('href') || '';
      hits.push({
        kind: 'body-stylesheet',
        excerpt: '<link rel="stylesheet" href="' + href.slice(0, 100) + '">',
      });
    }

    // 2. font-display in @font-face. Walk style sheets — accept a
    //    bare absence-of-font-display as risky too, but only flag
    //    explicit `font-display: block` to avoid noise on inline
    //    system-font declarations.
    try {
      for (let s = 0; s < document.styleSheets.length; s++) {
        const sheet = document.styleSheets[s];
        let rules = [];
        try { rules = sheet.cssRules || []; } catch (e) { continue; }
        for (let r = 0; r < rules.length; r++) {
          const rule = rules[r];
          if (!rule || rule.type !== 5) continue; // CSSRule.FONT_FACE_RULE
          const txt = rule.cssText || '';
          if (txt.indexOf('font-display:block') !== -1 ||
              txt.indexOf('font-display: block') !== -1) {
            hits.push({
              kind: 'font-display-block',
              excerpt: txt.slice(0, 200),
            });
          }
        }
      }
    } catch (_) { /* same-origin restrictions; skip */ }

    // 3. JS-loaded stylesheets — anything that wasn't in the
    //    initial HTML parse. Use sourceURL of the resource.
    const headLinks = document.head.querySelectorAll('link[rel="stylesheet"]');
    // We can't reliably tell JS-injected from HTML-parsed without
    // a separate timing observer. Approximation: any link with
    // data-loaded-by-js, OR any link without an href starting with
    // the same origin AND with `disabled` falsy. Best-effort.
    for (let i = 0; i < headLinks.length; i++) {
      if (headLinks[i].hasAttribute('data-loaded-by-js')) {
        hits.push({
          kind: 'js-loaded-stylesheet',
          excerpt: (headLinks[i].getAttribute('href') || '').slice(0, 200),
        });
      }
    }

    const hasInlineCriticalCss = !!document.head.querySelector('style')
      && (document.head.querySelector('style').textContent || '').trim().length > 0;
    const hasExternalStylesheet = headLinks.length > 0;

    return {
      pageUrl: window.location.href,
      hits: hits,
      hasInlineCriticalCss: hasInlineCriticalCss,
      hasExternalStylesheet: hasExternalStylesheet,
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        hits: Vec<FoucRiskHit>,
        has_inline: bool,
        has_ext: bool,
    ) -> FoucRiskSnapshot {
        FoucRiskSnapshot {
            page_url: "https://x".into(),
            hits,
            has_inline_critical_css: has_inline,
            has_external_stylesheet: has_ext,
        }
    }

    #[test]
    fn clean_page_produces_no_findings() {
        assert!(detect_fouc_risk(&snap(vec![], true, true)).is_empty());
        assert!(detect_fouc_risk(&snap(vec![], true, false)).is_empty());
        assert!(detect_fouc_risk(&snap(vec![], false, false)).is_empty());
    }

    #[test]
    fn body_stylesheet_produces_strict_finding() {
        let findings = detect_fouc_risk(&snap(
            vec![FoucRiskHit {
                kind: "body-stylesheet".into(),
                excerpt: "<link rel=\"stylesheet\" href=\"/extra.css\">".into(),
            }],
            true,
            true,
        ));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "fouc-risk.anti-pattern");
    }

    #[test]
    fn font_display_block_produces_strict_finding() {
        let findings = detect_fouc_risk(&snap(
            vec![FoucRiskHit {
                kind: "font-display-block".into(),
                excerpt: "@font-face { font-family: x; font-display: block; }".into(),
            }],
            true,
            true,
        ));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn missing_critical_css_produces_warn_finding() {
        let findings = detect_fouc_risk(&snap(vec![], false, true));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "fouc-risk.no-critical-css");
    }

    #[test]
    fn js_loaded_stylesheet_produces_strict_finding() {
        let findings = detect_fouc_risk(&snap(
            vec![FoucRiskHit {
                kind: "js-loaded-stylesheet".into(),
                excerpt: "/theme.css".into(),
            }],
            true,
            true,
        ));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn mixed_hits_produce_two_findings() {
        let findings = detect_fouc_risk(&snap(
            vec![FoucRiskHit {
                kind: "body-stylesheet".into(),
                excerpt: "x".into(),
            }],
            false,
            true,
        ));
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().any(|f| f.severity == AxisSeverity::Strict));
        assert!(findings.iter().any(|f| f.severity == AxisSeverity::Warn));
    }

    #[test]
    fn js_capture_constant_is_sensible() {
        assert!(FOUC_RISK_DOM_CAPTURE_JS.contains("hasInlineCriticalCss"));
        assert!(FOUC_RISK_DOM_CAPTURE_JS.contains("font-display"));
    }
}
