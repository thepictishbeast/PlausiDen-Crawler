//! `link_target_blank_safety` — anti-tabnabbing check.
//!
//! An `<a target="_blank">` without `rel="noopener"` (or
//! `rel="noreferrer"`) gives the destination page access to the
//! source via `window.opener` — a documented phishing vector
//! known as "tabnabbing." Modern browsers default to noopener for
//! `target="_blank"`, but explicit rel attribution is still the
//! correct posture for older clients + embedded webviews.
//!
//! This detector ALSO checks for cross-origin `target="_blank"`
//! without `rel="noreferrer"` — the referrer leak is a separate
//! privacy issue that the noopener-only fix doesn't cover.
//!
//! Findings:
//!   * `target-blank.no-noopener`     strict   any `_blank` without
//!                                              noopener OR noreferrer
//!   * `target-blank.no-noreferrer`   warn     cross-origin `_blank`
//!                                              with noopener but no
//!                                              noreferrer
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on the snapshot.
//! * Pure detector function; no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One outbound link the page exposes via `target="_blank"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TargetBlankLink {
    /// Stable CSS selector pointing at the `<a>` element.
    pub selector: String,
    /// The `href` value (absolute or relative).
    pub href: String,
    /// Whether the href is cross-origin to the page hosting it.
    pub is_cross_origin: bool,
    /// Tokens parsed from the element's `rel` attribute
    /// (lowercased + whitespace-split).
    pub rel_tokens: Vec<String>,
}

impl TargetBlankLink {
    /// True iff `rel` contains `noopener`.
    pub fn has_noopener(&self) -> bool {
        self.rel_tokens.iter().any(|t| t == "noopener")
    }

    /// True iff `rel` contains `noreferrer`.
    pub fn has_noreferrer(&self) -> bool {
        self.rel_tokens.iter().any(|t| t == "noreferrer")
    }
}

/// Captured set of all `target="_blank"` anchors on the page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TargetBlankSnapshot {
    /// Page URL.
    pub page_url: String,
    /// All `target="_blank"` anchors discovered.
    pub links: Vec<TargetBlankLink>,
}

/// Page-side eval. Collects every `<a target="_blank">` with
/// origin classification.
pub const TARGET_BLANK_JS: &str = r##"(() => {
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
    const anchors = document.querySelectorAll('a[target=_blank]');
    for (let i = 0; i < anchors.length; i++) {
        const a = anchors[i];
        const href = a.getAttribute('href') || '';
        let isCross = false;
        try {
            const u = new URL(href, window.location.href);
            isCross = u.origin !== pageOrigin;
        } catch (e) {
            isCross = false;
        }
        const rel = (a.getAttribute('rel') || '').toLowerCase().trim();
        const tokens = rel.length === 0 ? [] : rel.split(/\s+/);
        out.push({
            selector: selectorOf(a),
            href: href,
            isCrossOrigin: isCross,
            relTokens: tokens
        });
    }
    return { pageUrl: window.location.href, links: out };
})()"##;

/// Run the detector.
pub fn detect_target_blank_issues(snap: &TargetBlankSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    for link in &snap.links {
        // noopener OR noreferrer is sufficient for the tabnabbing
        // protection; missing both is strict.
        if !link.has_noopener() && !link.has_noreferrer() {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "target-blank.no-noopener".into(),
                detail: format!(
                    "<a target=\"_blank\" href=\"{}\"> has neither rel=noopener nor rel=noreferrer; tabnabbing risk ({})",
                    link.href, link.selector
                ),
            });
            continue;
        }
        // If noopener present but noreferrer absent + cross-origin,
        // the referrer still leaks.
        if link.is_cross_origin && link.has_noopener() && !link.has_noreferrer() {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "target-blank.no-noreferrer".into(),
                detail: format!(
                    "cross-origin <a target=\"_blank\" href=\"{}\"> has noopener but no noreferrer; referrer leaks ({})",
                    link.href, link.selector
                ),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(href: &str, cross: bool, rel: &[&str]) -> TargetBlankLink {
        TargetBlankLink {
            selector: format!("a[href={}]", href),
            href: href.to_string(),
            is_cross_origin: cross,
            rel_tokens: rel.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn snap(links: Vec<TargetBlankLink>) -> TargetBlankSnapshot {
        TargetBlankSnapshot {
            page_url: "https://example.com/".into(),
            links,
        }
    }

    #[test]
    fn no_target_blank_links_is_clean() {
        let s = snap(vec![]);
        assert!(detect_target_blank_issues(&s).is_empty());
    }

    #[test]
    fn missing_both_rels_is_strict() {
        let s = snap(vec![link("https://evil.example/", true, &[])]);
        let f = detect_target_blank_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert_eq!(f[0].kind, "target-blank.no-noopener");
    }

    #[test]
    fn noopener_only_cross_origin_warns_about_referrer() {
        let s = snap(vec![link("https://other.example/", true, &["noopener"])]);
        let f = detect_target_blank_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Warn);
        assert_eq!(f[0].kind, "target-blank.no-noreferrer");
    }

    #[test]
    fn noopener_only_same_origin_is_clean() {
        let s = snap(vec![link("/about", false, &["noopener"])]);
        assert!(detect_target_blank_issues(&s).is_empty());
    }

    #[test]
    fn noreferrer_alone_is_sufficient() {
        let s = snap(vec![link("https://other.example/", true, &["noreferrer"])]);
        assert!(detect_target_blank_issues(&s).is_empty());
    }

    #[test]
    fn both_rels_is_clean() {
        let s = snap(vec![link(
            "https://other.example/",
            true,
            &["noopener", "noreferrer"],
        )]);
        assert!(detect_target_blank_issues(&s).is_empty());
    }

    #[test]
    fn multiple_links_collect_multiple_findings() {
        let s = snap(vec![
            link("https://a.example/", true, &[]),           // strict
            link("https://b.example/", true, &["noopener"]), // warn referrer
            link("/internal", false, &[]),                   // strict
        ]);
        let f = detect_target_blank_issues(&s);
        assert_eq!(f.len(), 3);
        assert_eq!(
            f.iter()
                .filter(|x| x.severity == AxisSeverity::Strict)
                .count(),
            2
        );
        assert_eq!(
            f.iter()
                .filter(|x| x.severity == AxisSeverity::Warn)
                .count(),
            1
        );
    }
}
