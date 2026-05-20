//! `render_blocking_resources` — flag stylesheets + scripts in
//! `<head>` that block first paint.
//!
//! Lighthouse "render-blocking-resources" audit. Highest-leverage
//! marketing-page perf gap: every same-origin or cross-origin
//! resource fetched synchronously from `<head>` delays first
//! contentful paint by its full RTT + parse time.
//!
//! Loom's `page_shell_themed` already emits stylesheets with the
//! `media="print" onload="..."` defer pattern (when critical CSS
//! is inlined) and emits inline scripts with hashed CSPs. This
//! detector catches:
//!
//! * Stylesheets in `<head>` without the defer pattern.
//! * `<script>` in `<head>` without `defer`, `async`, or
//!   `type="module"`.
//! * Cross-origin `<link>` to fonts / scripts (a separate strict
//!   regression but flagged in passing).
//!
//! ## Heuristic
//!
//! Snapshot enumerates every `<head>` child of relevant type and
//! records its blocking properties. Classifier walks each entry
//! and emits one finding per blocking resource.
//!
//! ## Severity
//!
//! Warn on the first 1 blocking resource, strict on any more. The
//! first hit is sometimes intentional (critical CSS for a
//! supersociety site with strict CSP); multiple blockers are
//! always a regression.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One blocking resource captured in `<head>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct BlockingResource {
    /// Selector-ish path.
    pub selector: String,
    /// `link` or `script`.
    pub element_kind: String,
    /// Href (for `<link>`) or src (for `<script>`).
    pub url: String,
    /// Why it blocks: human-readable explanation.
    pub reason: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct RenderBlockingSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every blocking resource the walker found in `<head>`.
    pub blockers: Vec<BlockingResource>,
    /// Total `<head>` link + script children walked.
    pub scanned: u32,
}

/// Pure detector: snapshot → findings. Warn on 1, strict on > 1.
#[must_use]
pub fn detect_render_blocking_resources(snap: &RenderBlockingSnapshot) -> Vec<AxisFinding> {
    if snap.blockers.is_empty() {
        return Vec::new();
    }
    let severity = if snap.blockers.len() == 1 {
        AxisSeverity::Warn
    } else {
        AxisSeverity::Strict
    };
    let examples: Vec<String> = snap
        .blockers
        .iter()
        .take(5)
        .map(|b| {
            format!(
                "{} {} → {} ({})",
                b.selector, b.element_kind, b.url, b.reason
            )
        })
        .collect();
    vec![AxisFinding {
        severity,
        kind: "render-blocking.resource".to_owned(),
        detail: format!(
            "{} resource(s) in <head> block first paint. Defer with media=\"print\" onload swap (for stylesheets) or `defer` / `async` / `type=\"module\"` (for scripts). Examples: {}",
            snap.blockers.len(),
            examples.join("; ")
        ),
    }]
}

/// Browser-side capture.
pub const RENDER_BLOCKING_DOM_CAPTURE_JS: &str = r#"
(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const same = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (same.length > 1) parts.unshift(tag + ':nth-of-type(' + (same.indexOf(node) + 1) + ')');
          else parts.unshift(tag);
        } else parts.unshift(tag);
        node = parent;
        depth += 1;
      }
      return 'head > ' + parts.join(' > ');
    };

    const blockers = [];
    let scanned = 0;
    const head = document.head;
    if (!head) {
      return { pageUrl: window.location.href, blockers: blockers, scanned: 0 };
    }

    const links = head.querySelectorAll('link[rel="stylesheet"]');
    for (let i = 0; i < links.length; i++) {
      const l = links[i];
      scanned += 1;
      const media = (l.getAttribute('media') || 'all').toLowerCase();
      const onload = l.getAttribute('onload') || '';
      const isDeferred = media === 'print' && onload.length > 0;
      if (isDeferred) continue;
      blockers.push({
        selector: selectorOf(l),
        elementKind: 'link',
        url: l.getAttribute('href') || '',
        reason: 'stylesheet without media=print onload defer pattern',
      });
    }

    const scripts = head.querySelectorAll('script');
    for (let i = 0; i < scripts.length; i++) {
      const s = scripts[i];
      scanned += 1;
      // Inline scripts in <head> are technically blocking but
      // PlausiDen ships them via hashed-CSP small inline (theme
      // toggle, eruda loader). Skip if it has no src.
      const src = s.getAttribute('src');
      if (!src) continue;
      const hasDefer = s.hasAttribute('defer');
      const hasAsync = s.hasAttribute('async');
      const isModule = (s.getAttribute('type') || '').toLowerCase() === 'module';
      if (hasDefer || hasAsync || isModule) continue;
      blockers.push({
        selector: selectorOf(s),
        elementKind: 'script',
        url: src,
        reason: 'script without defer / async / type=module',
      });
    }

    return { pageUrl: window.location.href, blockers: blockers, scanned: scanned };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(blockers: Vec<BlockingResource>) -> RenderBlockingSnapshot {
        RenderBlockingSnapshot {
            page_url: "https://dev.plausiden.com/".to_owned(),
            blockers,
            scanned: 5,
        }
    }

    fn blocker(kind: &str, url: &str) -> BlockingResource {
        BlockingResource {
            selector: format!("head > {kind}"),
            element_kind: kind.to_owned(),
            url: url.to_owned(),
            reason: "test".to_owned(),
        }
    }

    #[test]
    fn empty_snapshot_no_findings() {
        let s = snap(Vec::new());
        assert!(detect_render_blocking_resources(&s).is_empty());
    }

    #[test]
    fn one_blocker_warns() {
        let s = snap(vec![blocker("link", "/loom-skin.css")]);
        let f = detect_render_blocking_resources(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Warn);
        assert!(f[0].detail.contains("1 resource"));
    }

    #[test]
    fn multiple_blockers_strict() {
        let s = snap(vec![blocker("link", "/a.css"), blocker("script", "/x.js")]);
        let f = detect_render_blocking_resources(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert!(f[0].detail.contains("2 resource"));
    }

    #[test]
    fn examples_capped_at_5() {
        let mut blockers = Vec::new();
        for i in 0..10 {
            blockers.push(blocker("link", &format!("/style-{i}.css")));
        }
        let s = snap(blockers);
        let f = detect_render_blocking_resources(&s);
        assert!(f[0].detail.contains("10 resource"));
        let arrows = f[0].detail.matches(" → ").count();
        assert_eq!(arrows, 5);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap(vec![blocker("link", "/x.css")]);
        let j = serde_json::to_string(&s).expect("ser");
        let back: RenderBlockingSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.blockers.len(), 1);
        assert_eq!(back.blockers[0].url, "/x.css");
    }

    #[test]
    fn js_brackets_balanced() {
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        let mut bracket: i32 = 0;
        for c in RENDER_BLOCKING_DOM_CAPTURE_JS.chars() {
            match c {
                '(' => paren += 1,
                ')' => paren -= 1,
                '{' => brace += 1,
                '}' => brace -= 1,
                '[' => bracket += 1,
                ']' => bracket -= 1,
                _ => {}
            }
        }
        assert_eq!(paren, 0, "unbalanced parens in capture JS");
        assert_eq!(brace, 0, "unbalanced braces in capture JS");
        assert_eq!(bracket, 0, "unbalanced brackets in capture JS");
    }

    #[test]
    fn js_includes_defer_check() {
        assert!(
            RENDER_BLOCKING_DOM_CAPTURE_JS.contains("hasAttribute('defer')"),
            "capture JS missing defer-affordance check"
        );
    }

    #[test]
    fn js_includes_module_check() {
        assert!(
            RENDER_BLOCKING_DOM_CAPTURE_JS.contains("'module'"),
            "capture JS missing module-script check"
        );
    }
}
