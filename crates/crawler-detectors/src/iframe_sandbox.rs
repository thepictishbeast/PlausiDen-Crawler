//! `iframe_sandbox` — `<iframe>` sandbox attribution audit.
//!
//! Per HTML Living Standard + OWASP guidance: any `<iframe>`
//! embedding cross-origin content SHOULD declare a `sandbox`
//! attribute, restricting the embedded document's capabilities to
//! the explicit grants the operator listed.
//!
//! A particularly dangerous configuration is `sandbox` containing
//! BOTH `allow-scripts` AND `allow-same-origin` for an iframe
//! whose source IS the parent's origin — that combination grants
//! the embedded document the same authority as the parent (it
//! can mutate the iframe's `sandbox` attribute via `parent`
//! reference + remove all restrictions). Spec note: see
//! <https://html.spec.whatwg.org/#attr-iframe-sandbox>.
//!
//! Findings:
//!   * `iframe-sandbox.absent`              strict   no sandbox attr
//!   * `iframe-sandbox.scripts-and-same-origin`  strict  defeated sandbox
//!   * `iframe-sandbox.allow-top-navigation` warn    rare legitimate use
//!
//! Local-fs and `about:blank` iframes are exempt — they don't
//! pose cross-origin risk.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on snapshot types.
//! * Pure detector function; no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured iframe element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct IframeEntry {
    /// CSS selector pointing at the iframe.
    pub selector: String,
    /// `src` attribute (empty for `about:blank` / `srcdoc`).
    pub src: String,
    /// True iff `src` is cross-origin to the page hosting it.
    pub is_cross_origin: bool,
    /// True iff the iframe uses `srcdoc=` instead of `src=`.
    pub is_srcdoc: bool,
    /// Whether the `sandbox` attribute is present at all.
    pub has_sandbox: bool,
    /// Lowercased + whitespace-split tokens from the sandbox
    /// attribute. Empty when has_sandbox is false; also empty
    /// when sandbox=""  (which is the maximum-restriction value).
    pub sandbox_tokens: Vec<String>,
}

/// Captured iframe set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct IframeSandboxSnapshot {
    /// Page URL.
    pub page_url: String,
    /// All `<iframe>` elements discovered.
    pub iframes: Vec<IframeEntry>,
}

/// Page-side eval. Collects iframes + sandbox attribution.
pub const IFRAME_SANDBOX_JS: &str = r##"(() => {
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
      return 'body > ' + parts.join(' > ');
    };

    const pageOrigin = window.location.origin;
    const out = [];
    const frames = document.querySelectorAll('iframe');
    for (let i = 0; i < frames.length; i++) {
        const fr = frames[i];
        const src = fr.getAttribute('src') || '';
        const isSrcdoc = fr.hasAttribute('srcdoc');
        let isCross = false;
        if (src.length > 0 && !isSrcdoc) {
            try {
                const u = new URL(src, window.location.href);
                isCross = u.origin !== pageOrigin && u.protocol !== 'about:';
            } catch (e) { isCross = false; }
        }
        const sb = fr.getAttribute('sandbox');
        const hasSandbox = sb !== null;
        const tokens = hasSandbox && sb.length > 0
            ? sb.toLowerCase().trim().split(/\s+/).filter(function(t) { return t.length > 0; })
            : [];
        out.push({
            selector: selectorOf(fr),
            src: src,
            isCrossOrigin: isCross,
            isSrcdoc: isSrcdoc,
            hasSandbox: hasSandbox,
            sandboxTokens: tokens
        });
    }
    return { pageUrl: window.location.href, iframes: out };
})()"##;

/// Run the detector.
pub fn detect_iframe_sandbox_issues(snap: &IframeSandboxSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    for frame in &snap.iframes {
        // about:blank, srcdoc, and empty-src iframes are exempt —
        // no remote content loaded.
        if frame.is_srcdoc || frame.src.is_empty() || frame.src.starts_with("about:") {
            continue;
        }
        // Only audit cross-origin iframes; same-origin embeds are
        // assumed trusted by the operator.
        if !frame.is_cross_origin {
            continue;
        }
        if !frame.has_sandbox {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "iframe-sandbox.absent".into(),
                detail: format!(
                    "cross-origin <iframe src=\"{}\"> has no sandbox attribute ({})",
                    frame.src, frame.selector
                ),
            });
            continue;
        }
        let has_scripts = frame.sandbox_tokens.iter().any(|t| t == "allow-scripts");
        let has_same_origin = frame
            .sandbox_tokens
            .iter()
            .any(|t| t == "allow-same-origin");
        if has_scripts && has_same_origin {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "iframe-sandbox.scripts-and-same-origin".into(),
                detail: format!(
                    "<iframe src=\"{}\"> sandbox allows BOTH allow-scripts AND allow-same-origin; defeats sandboxing ({})",
                    frame.src, frame.selector
                ),
            });
        }
        if frame
            .sandbox_tokens
            .iter()
            .any(|t| t == "allow-top-navigation")
        {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "iframe-sandbox.allow-top-navigation".into(),
                detail: format!(
                    "<iframe src=\"{}\"> sandbox grants allow-top-navigation; rare legitimate use ({})",
                    frame.src, frame.selector
                ),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(src: &str, cross: bool, srcdoc: bool, sandbox: Option<Vec<&str>>) -> IframeEntry {
        IframeEntry {
            selector: format!("iframe[src={}]", src),
            src: src.to_string(),
            is_cross_origin: cross,
            is_srcdoc: srcdoc,
            has_sandbox: sandbox.is_some(),
            sandbox_tokens: sandbox
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    fn snap(iframes: Vec<IframeEntry>) -> IframeSandboxSnapshot {
        IframeSandboxSnapshot {
            page_url: "https://example.com/".into(),
            iframes,
        }
    }

    #[test]
    fn no_iframes_is_clean() {
        let s = snap(vec![]);
        assert!(detect_iframe_sandbox_issues(&s).is_empty());
    }

    #[test]
    fn cross_origin_no_sandbox_is_strict() {
        let s = snap(vec![frame("https://evil.example/", true, false, None)]);
        let f = detect_iframe_sandbox_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert_eq!(f[0].kind, "iframe-sandbox.absent");
    }

    #[test]
    fn scripts_plus_same_origin_is_strict() {
        let s = snap(vec![frame(
            "https://other.example/",
            true,
            false,
            Some(vec!["allow-scripts", "allow-same-origin"]),
        )]);
        let f = detect_iframe_sandbox_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "iframe-sandbox.scripts-and-same-origin"));
    }

    #[test]
    fn top_navigation_grant_warns() {
        let s = snap(vec![frame(
            "https://other.example/",
            true,
            false,
            Some(vec!["allow-scripts", "allow-top-navigation"]),
        )]);
        let f = detect_iframe_sandbox_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "iframe-sandbox.allow-top-navigation"));
    }

    #[test]
    fn empty_sandbox_with_cross_origin_is_clean() {
        // sandbox="" is the most restrictive — no findings.
        let s = snap(vec![frame(
            "https://other.example/",
            true,
            false,
            Some(vec![]),
        )]);
        assert!(detect_iframe_sandbox_issues(&s).is_empty());
    }

    #[test]
    fn same_origin_iframe_is_exempt() {
        let s = snap(vec![frame("/embed", false, false, None)]);
        assert!(detect_iframe_sandbox_issues(&s).is_empty());
    }

    #[test]
    fn srcdoc_iframe_is_exempt() {
        let s = snap(vec![frame("", false, true, None)]);
        assert!(detect_iframe_sandbox_issues(&s).is_empty());
    }

    #[test]
    fn about_blank_iframe_is_exempt() {
        let s = snap(vec![frame("about:blank", false, false, None)]);
        assert!(detect_iframe_sandbox_issues(&s).is_empty());
    }

    #[test]
    fn safe_sandboxed_cross_origin_is_clean() {
        let s = snap(vec![frame(
            "https://other.example/",
            true,
            false,
            Some(vec!["allow-scripts"]),
        )]);
        // allow-scripts ALONE (without allow-same-origin) is the
        // intended embed mode — clean.
        assert!(detect_iframe_sandbox_issues(&s).is_empty());
    }
}
