//! `link_text` — link-purpose detector.
//!
//! WCAG 2.4.4 (Link Purpose, AA): every link's purpose must be
//! determinable from the link text alone (or text + programmatically
//! associated context). Two violations this detector catches:
//!
//! 1. **Empty link text.** No visible text AND no aria-label / no
//!    aria-labelledby / no title. Screen-reader users hear "link"
//!    with no context. Strict — these are unconditionally broken.
//!
//! 2. **Generic link text.** Phrases like "click here", "read more",
//!    "more", "here", "details", "this link" — text that says
//!    nothing about the destination. Warn — common in marketing
//!    copy, easily confused with "this link is the article", but
//!    out-of-context (e.g. screen reader's link list) it's
//!    meaningless.
//!
//! Out of scope:
//!   * Same anchor text → different href detection (separate axis).
//!   * Link-text-equals-href (raw URLs as link text — readable, just
//!     ugly; not a WCAG violation).
//!   * Visual ambiguity (small text, low contrast — covered by
//!     runtimeContrast).
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on enums.
//! * Pure functions; no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval. Captures every `<a>` with its accessible name
/// computed from textContent + aria-label + aria-labelledby +
/// title (in that priority order).
pub const LINK_TEXT_JS: &str = r##"(() => {
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

    const isVisible = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      const rect = el.getBoundingClientRect();
      // 0×0 means hidden / not laid out.
      if (rect.width === 0 && rect.height === 0) return false;
      return true;
    };

    // Compute accessible name per WAI-ARIA name-computation
    // simplified: aria-labelledby > aria-label > visible text > title.
    const accessibleName = function(el) {
      const labelledby = el.getAttribute('aria-labelledby');
      if (labelledby) {
        const ids = labelledby.split(/\s+/).filter(Boolean);
        const parts = [];
        for (const id of ids) {
          const ref = document.getElementById(id);
          if (ref) parts.push((ref.textContent || '').trim());
        }
        const joined = parts.join(' ').trim();
        if (joined) return joined;
      }
      const aria = el.getAttribute('aria-label');
      if (aria && aria.trim()) return aria.trim();
      const text = (el.textContent || '').trim();
      if (text) return text;
      const title = el.getAttribute('title');
      if (title && title.trim()) return title.trim();
      return '';
    };

    const links = [];
    const anchors = document.querySelectorAll('a[href]');
    for (let i = 0; i < anchors.length; i++) {
      const el = anchors[i];
      if (!isVisible(el)) continue;
      const name = accessibleName(el);
      const href = el.getAttribute('href') || '';
      links.push({
        selector: selectorOf(el),
        href: href,
        name: name.slice(0, 120),
      });
    }

    return {
      pageUrl: window.location.href,
      links: links,
    };
})()"##;

/// One captured link.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedLink {
    /// Best-effort CSS selector.
    pub selector: String,
    /// `href` attribute value.
    pub href: String,
    /// Accessible name (first 120 chars).
    pub name: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkTextSnapshot {
    /// Page URL at capture time.
    pub page_url: String,
    /// Visible anchor links with accessible names.
    pub links: Vec<CapturedLink>,
}

/// Phrases that say nothing about the destination. Compared
/// case-insensitively, with surrounding whitespace trimmed.
/// Matched as the WHOLE accessible name — substring matches are
/// too noisy (a link "Click here for the calendar" is fine).
const GENERIC_PHRASES: &[&str] = &[
    "click here",
    "click",
    "here",
    "read more",
    "more",
    "learn more",
    "details",
    "see more",
    "see details",
    "this",
    "this link",
    "link",
    "go",
    "next",
    "previous",
    "continue",
    ">",
    ">>",
    "..",
    "...",
];

/// Apply detection rules to a link-text snapshot. Pure function.
#[must_use]
pub fn detect_link_text_issues(snap: &LinkTextSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::<crate::AxisFinding>::new();

    let mut empty_count = 0u32;
    let mut empty_examples = Vec::<String>::new();
    let mut generic_count = 0u32;
    let mut generic_examples = Vec::<String>::new();

    for link in &snap.links {
        let name = link.name.trim();
        if name.is_empty() {
            empty_count += 1;
            if empty_examples.len() < 5 {
                empty_examples.push(format!(
                    "{} → href={}",
                    link.selector, link.href
                ));
            }
            continue;
        }
        let lower = name.to_lowercase();
        if GENERIC_PHRASES.iter().any(|p| *p == lower.as_str()) {
            generic_count += 1;
            if generic_examples.len() < 5 {
                generic_examples.push(format!(
                    "'{name}' → href={}",
                    link.href
                ));
            }
        }
    }

    if empty_count > 0 {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "link.empty-text".to_owned(),
            detail: format!(
                "{empty_count} visible link(s) have no accessible name (no textContent, aria-label, aria-labelledby, or title). Screen-reader users hear 'link' with no destination context. Examples: {}",
                empty_examples.join("; ")
            ),
        });
    }
    if generic_count > 0 {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "link.generic-text".to_owned(),
            detail: format!(
                "{generic_count} link(s) have generic text (e.g. 'click here', 'read more') that doesn't convey destination. WCAG 2.4.4 — link purpose must be determinable from the link text. Examples: {}",
                generic_examples.join("; ")
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_balanced() {
        assert_eq!(
            LINK_TEXT_JS.matches('(').count(),
            LINK_TEXT_JS.matches(')').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(LINK_TEXT_JS.starts_with("(() => {"));
        assert!(LINK_TEXT_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in ["pageUrl", "links"] {
            assert!(LINK_TEXT_JS.contains(k), "missing key: {k}");
        }
    }

    fn link(selector: &str, href: &str, name: &str) -> CapturedLink {
        CapturedLink {
            selector: selector.to_owned(),
            href: href.to_owned(),
            name: name.to_owned(),
        }
    }

    fn snap(links: Vec<CapturedLink>) -> LinkTextSnapshot {
        LinkTextSnapshot {
            page_url: "http://t/".to_owned(),
            links,
        }
    }

    #[test]
    fn descriptive_links_pass() {
        let s = snap(vec![
            link("body > a", "/leaderboard", "Top earners this week"),
            link("body > a", "/post-skill", "Post a skill clip"),
            link("body > a", "/profile/dax", "@court_dax profile"),
        ]);
        assert!(detect_link_text_issues(&s).is_empty());
    }

    #[test]
    fn empty_link_fires_strict() {
        let s = snap(vec![link("body > a", "/x", "")]);
        let f = detect_link_text_issues(&s);
        let empty = f.iter().find(|x| x.kind == "link.empty-text");
        assert!(empty.is_some());
        assert!(empty.unwrap().severity == crate::AxisSeverity::Strict);
        assert!(empty.unwrap().detail.contains("/x"));
    }

    #[test]
    fn whitespace_only_link_text_counts_as_empty() {
        let s = snap(vec![link("body > a", "/x", "  \n  \t ")]);
        let f = detect_link_text_issues(&s);
        assert!(f.iter().any(|x| x.kind == "link.empty-text"));
    }

    #[test]
    fn click_here_fires_warn() {
        let s = snap(vec![link("body > a", "/article", "click here")]);
        let f = detect_link_text_issues(&s);
        let g = f.iter().find(|x| x.kind == "link.generic-text");
        assert!(g.is_some());
        assert!(g.unwrap().severity == crate::AxisSeverity::Warn);
    }

    #[test]
    fn read_more_fires_warn() {
        let s = snap(vec![
            link("body > a", "/article-1", "Read more"),
            link("body > a", "/article-2", "Read more"),
        ]);
        let f = detect_link_text_issues(&s);
        let g = f.iter().find(|x| x.kind == "link.generic-text");
        assert!(g.is_some());
        assert!(g.unwrap().detail.contains("2 link"));
    }

    #[test]
    fn case_insensitive_generic_match() {
        let s = snap(vec![
            link("body > a", "/x", "CLICK HERE"),
            link("body > a", "/y", "Click Here"),
            link("body > a", "/z", "click here"),
        ]);
        let f = detect_link_text_issues(&s);
        assert!(f.iter().any(|x| x.detail.contains("3 link")));
    }

    #[test]
    fn substring_doesnt_match_generic() {
        // "Click here for the calendar" contains 'click here' but
        // is not the WHOLE link text — should pass.
        let s = snap(vec![
            link("body > a", "/cal", "Click here for the calendar"),
        ]);
        assert!(detect_link_text_issues(&s).is_empty());
    }

    #[test]
    fn descriptive_more_text_passes() {
        // "More options for staff" passes because it's not just "more".
        let s = snap(vec![link("body > a", "/staff", "More options for staff")]);
        assert!(detect_link_text_issues(&s).is_empty());
    }

    #[test]
    fn empty_and_generic_combine() {
        let s = snap(vec![
            link("body > a:1", "/a", ""),
            link("body > a:2", "/b", "click here"),
            link("body > a:3", "/c", "Read full article on stair-takes"),
        ]);
        let f = detect_link_text_issues(&s);
        assert_eq!(f.len(), 2);
        assert!(f.iter().any(|x| x.kind == "link.empty-text"));
        assert!(f.iter().any(|x| x.kind == "link.generic-text"));
    }

    #[test]
    fn empty_link_examples_capped_at_5() {
        let mut links = Vec::new();
        for i in 0..10 {
            links.push(link(&format!("body > a:{i}"), &format!("/x{i}"), ""));
        }
        let s = snap(links);
        let f = detect_link_text_issues(&s);
        let empty = f.iter().find(|x| x.kind == "link.empty-text").unwrap();
        // Detail mentions the 10 count but only ~5 example selectors
        // (we slice at 5 in the impl).
        assert!(empty.detail.contains("10 visible"));
    }

    #[test]
    fn snapshot_round_trips() {
        let s = snap(vec![link("body > a", "/x", "Hello")]);
        let json = serde_json::to_string(&s).expect("ser");
        let back: LinkTextSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.links.len(), 1);
        assert_eq!(back.links[0].name, "Hello");
    }
}
