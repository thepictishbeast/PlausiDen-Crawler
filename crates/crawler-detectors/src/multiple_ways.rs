//! `multiple_ways` — flag pages that offer only one way to find
//! their location in a site.
//!
//! WCAG 2.1 SC 2.4.5 (Multiple Ways, Level AA). Each page (except
//! steps in a process) must be reachable via at least two of:
//! site-wide navigation, in-page search, sitemap, table-of-contents,
//! related-links.
//!
//! ## Heuristic
//!
//! Snapshot enumerates which "way to find" affordances are present:
//!
//! * `nav` — at least one `<nav>` element OR
//!   `[aria-label*="primary"|"main"]` / `role="navigation"`.
//! * `search` — at least one `<input type="search">` OR
//!   `role="search"` form.
//! * `sitemap` — at least one `<a>` linking to `/sitemap.xml` /
//!   `/sitemap.html` / text "sitemap" / "site map".
//! * `toc` — `<nav aria-label*="contents"|"toc">`,
//!   `[data-loom-toc]`, OR a `<details>` whose summary text
//!   includes "contents".
//! * `related_links` — a `<section>` / `<aside>` / `<nav>` whose
//!   aria-label or visible heading contains "related", "see also",
//!   "more in this section", "you might like".
//!
//! Count the present affordances. Warn if count < 2 (per-page,
//! Level AA threshold).
//!
//! ## Severity
//!
//! Warn. SC 2.4.5 has process-step exemption — single-step
//! checkouts / signin flows legitimately have one way to navigate
//! and shouldn't fire as strict.
//!
//! Caller opt-out: `data-loom-multiple-ways-exempt="true"` on
//! `<html>` for legitimate single-step pages.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Which affordances the page provides.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MultipleWaysSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Page has a site-wide `<nav>` / role=navigation.
    pub has_nav: bool,
    /// Page has an in-page search input or role=search form.
    pub has_search: bool,
    /// Page links to a sitemap.
    pub has_sitemap_link: bool,
    /// Page has a table-of-contents region.
    pub has_toc: bool,
    /// Page has a related-links / see-also region.
    pub has_related_links: bool,
    /// Caller-side exemption (single-step process pages).
    pub exempt: bool,
}

impl MultipleWaysSnapshot {
    /// Count of present affordances.
    #[must_use]
    pub fn affordance_count(&self) -> u8 {
        u8::from(self.has_nav)
            + u8::from(self.has_search)
            + u8::from(self.has_sitemap_link)
            + u8::from(self.has_toc)
            + u8::from(self.has_related_links)
    }
}

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_multiple_ways(snap: &MultipleWaysSnapshot) -> Vec<AxisFinding> {
    if snap.exempt {
        return Vec::new();
    }
    let count = snap.affordance_count();
    if count >= 2 {
        return Vec::new();
    }
    let mut present: Vec<&'static str> = Vec::new();
    if snap.has_nav {
        present.push("nav");
    }
    if snap.has_search {
        present.push("search");
    }
    if snap.has_sitemap_link {
        present.push("sitemap-link");
    }
    if snap.has_toc {
        present.push("toc");
    }
    if snap.has_related_links {
        present.push("related-links");
    }
    let have = if present.is_empty() {
        "none".to_owned()
    } else {
        present.join(", ")
    };
    vec![AxisFinding {
        severity: AxisSeverity::Warn,
        kind: "multiple-ways.insufficient".to_owned(),
        detail: format!(
            "WCAG 2.4.5 — page {} provides only {count} way(s) to find its location (have: {have}). Level AA requires ≥ 2 of: nav / search / sitemap / toc / related-links. If this is a single-step process page (signin / checkout / etc.) declare `data-loom-multiple-ways-exempt=\"true\"` on `<html>`.",
            snap.page_url
        ),
    }]
}

/// Browser-side capture.
pub const MULTIPLE_WAYS_DOM_CAPTURE_JS: &str = r#"
(() => {
    const exempt = document.documentElement.getAttribute('data-loom-multiple-ways-exempt') === 'true';

    // nav — explicit element OR role/aria-label.
    const navEl = document.querySelector('nav, [role="navigation"], [aria-label*="Primary" i], [aria-label*="Main" i]');
    const hasNav = !!navEl;

    // search — input type=search OR role=search.
    const searchEl = document.querySelector('input[type="search"], [role="search"]');
    const hasSearch = !!searchEl;

    // sitemap link.
    let hasSitemapLink = false;
    const anchors = document.querySelectorAll('a[href]');
    for (let i = 0; i < anchors.length; i++) {
      const a = anchors[i];
      const href = (a.getAttribute('href') || '').toLowerCase();
      const text = (a.textContent || '').toLowerCase();
      if (href.indexOf('sitemap') !== -1
          || text.indexOf('sitemap') !== -1
          || text.indexOf('site map') !== -1) {
        hasSitemapLink = true;
        break;
      }
    }

    // toc — labelled nav / loom-toc data attr / details with contents summary.
    let hasToc = false;
    if (document.querySelector('[data-loom-toc], nav[aria-label*="contents" i], nav[aria-label*="toc" i]')) {
      hasToc = true;
    } else {
      const detailsList = document.querySelectorAll('details > summary');
      for (let i = 0; i < detailsList.length; i++) {
        const s = (detailsList[i].textContent || '').toLowerCase();
        if (s.indexOf('contents') !== -1 || s.indexOf('on this page') !== -1) {
          hasToc = true;
          break;
        }
      }
    }

    // related-links — aside/section/nav with related-ish label or heading.
    let hasRelatedLinks = false;
    const containers = document.querySelectorAll('aside, section, nav');
    const relatedPatterns = ['related', 'see also', 'more in', 'you might like', 'further reading'];
    outer: for (let i = 0; i < containers.length; i++) {
      const c = containers[i];
      const label = (c.getAttribute('aria-label') || '').toLowerCase();
      for (let j = 0; j < relatedPatterns.length; j++) {
        if (label.indexOf(relatedPatterns[j]) !== -1) {
          hasRelatedLinks = true;
          break outer;
        }
      }
      const heading = c.querySelector('h2, h3, h4');
      if (heading) {
        const ht = (heading.textContent || '').toLowerCase();
        for (let j = 0; j < relatedPatterns.length; j++) {
          if (ht.indexOf(relatedPatterns[j]) !== -1) {
            hasRelatedLinks = true;
            break outer;
          }
        }
      }
    }

    return {
      pageUrl: window.location.href,
      hasNav: hasNav,
      hasSearch: hasSearch,
      hasSitemapLink: hasSitemapLink,
      hasToc: hasToc,
      hasRelatedLinks: hasRelatedLinks,
      exempt: exempt,
    };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_snap() -> MultipleWaysSnapshot {
        MultipleWaysSnapshot {
            page_url: "https://dev.plausiden.com/".to_owned(),
            has_nav: false,
            has_search: false,
            has_sitemap_link: false,
            has_toc: false,
            has_related_links: false,
            exempt: false,
        }
    }

    #[test]
    fn no_affordances_warns() {
        let s = empty_snap();
        let f = detect_multiple_ways(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Warn);
        assert!(f[0].detail.contains("only 0 way"));
        assert!(f[0].detail.contains("have: none"));
    }

    #[test]
    fn one_affordance_warns() {
        let mut s = empty_snap();
        s.has_nav = true;
        let f = detect_multiple_ways(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("only 1 way"));
        assert!(f[0].detail.contains("nav"));
    }

    #[test]
    fn two_affordances_pass() {
        let mut s = empty_snap();
        s.has_nav = true;
        s.has_search = true;
        assert!(detect_multiple_ways(&s).is_empty());
    }

    #[test]
    fn five_affordances_pass() {
        let mut s = empty_snap();
        s.has_nav = true;
        s.has_search = true;
        s.has_sitemap_link = true;
        s.has_toc = true;
        s.has_related_links = true;
        assert!(detect_multiple_ways(&s).is_empty());
    }

    #[test]
    fn exempt_page_does_not_warn_even_with_zero() {
        let mut s = empty_snap();
        s.exempt = true;
        assert!(detect_multiple_ways(&s).is_empty());
    }

    #[test]
    fn affordance_count_correct() {
        let mut s = empty_snap();
        assert_eq!(s.affordance_count(), 0);
        s.has_nav = true;
        assert_eq!(s.affordance_count(), 1);
        s.has_search = true;
        s.has_toc = true;
        assert_eq!(s.affordance_count(), 3);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let mut s = empty_snap();
        s.has_nav = true;
        s.has_toc = true;
        let j = serde_json::to_string(&s).expect("ser");
        let back: MultipleWaysSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.affordance_count(), 2);
    }

    #[test]
    fn js_brackets_balanced() {
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        let mut bracket: i32 = 0;
        for c in MULTIPLE_WAYS_DOM_CAPTURE_JS.chars() {
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
    fn js_includes_exempt_marker() {
        assert!(
            MULTIPLE_WAYS_DOM_CAPTURE_JS.contains("data-loom-multiple-ways-exempt"),
            "capture JS missing the caller-side exemption marker"
        );
    }
}
