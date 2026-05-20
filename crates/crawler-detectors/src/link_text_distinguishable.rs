//! `link_text_distinguishable` — same-text-different-href detector.
//!
//! Forward step on rolling task #117 (Crawler axes). Pairs with
//! `link_text`, `link_color_only`, `link_underline`, `link_target_blank_safety`
//! — the link-quality detector family.
//!
//! ## The bug class
//!
//! WCAG 2.4.4 (Link Purpose, in Context): the purpose of each
//! link should be determinable from the link text alone, OR from
//! the link text together with its programmatically-determined
//! context. The most common failure: a page has multiple "Read
//! more" or "Click here" links, each going to a DIFFERENT URL,
//! with no aria-label / aria-describedby distinguishing them.
//!
//! Screen-reader users typically navigate by link list — they
//! pull up "list of links" and hear "Read more, Read more, Read
//! more, Read more" with no way to tell which is which.
//!
//! ## Findings
//!
//! * `link-text.duplicate-text-different-href` strict — N links
//!   share identical visible text (trimmed + lowercased) BUT
//!   point to different hrefs. Each group emits one finding
//!   listing the involved URLs.
//! * `link-text.vague-cluster`                  warn  — N >= 2
//!   links with one of the well-known vague phrases ("read more",
//!   "click here", "learn more", "more info", "details") in their
//!   visible text. Distinct from the duplicate-different-href
//!   case: even when all targets are the SAME URL, a page full
//!   of "click here" links signals weak editorial.
//!
//! Out of scope:
//!
//! * `<a>` with empty / whitespace-only text + no aria-label —
//!   that's `empty_button`/`link_text`'s domain.
//! * `aria-describedby` is read for the analysis (a link with
//!   distinct describedby is treated as distinguished even with
//!   duplicate visible text).
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; the JS const is the only side-
//!   effect channel.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One captured `<a>` element with its accessible-link-text
/// signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LinkEntry {
    /// CSS-ish selector pointing at the link.
    pub selector: String,
    /// Visible text content (truncated to 200 chars; trimmed by
    /// the JS capture).
    pub text: String,
    /// href attribute as-authored.
    pub href: String,
    /// aria-label attribute value if present.
    pub aria_label: Option<String>,
    /// aria-describedby attribute value if present. Distinct
    /// describedby on otherwise-duplicate links counts as
    /// distinguishing context.
    pub aria_describedby: Option<String>,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LinkTextDistinguishableSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every visible `<a href>` on the page.
    pub entries: Vec<LinkEntry>,
}

/// Vague link-text phrases the warn-cluster check flags.
/// Lowercased + trimmed before match. Phrase list is small +
/// intentional — only the canonical SaaS-vague set; per-tenant
/// extensions belong in the operator's own slop dictionary
/// (per memory [[forge-substrate-flexible-product-opinionated]]
/// + [[crawler-stays-general-purpose]]).
const VAGUE_PHRASES: &[&str] = &[
    "read more",
    "click here",
    "learn more",
    "more info",
    "details",
    "see more",
    "more",
];

/// Page-side eval. Walks every visible `<a href>`, captures the
/// signals above.
pub const LINK_TEXT_DISTINGUISHABLE_JS: &str = r##"(() => {
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
    const isVisible = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return false;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return false;
      return true;
    };
    const truncate = function(s) { return (s || '').replace(/\s+/g, ' ').trim().slice(0, 200); };
    const entries = [];
    const links = document.querySelectorAll('a[href]');
    for (let i = 0; i < links.length; i++) {
      const el = links[i];
      if (!isVisible(el)) continue;
      const text = truncate(el.textContent || '');
      if (text.length === 0 && !el.getAttribute('aria-label')) continue;
      entries.push({
        selector: selectorOf(el),
        text: text,
        href: el.getAttribute('href') || '',
        ariaLabel: el.getAttribute('aria-label'),
        ariaDescribedby: el.getAttribute('aria-describedby')
      });
    }
    return {
      pageUrl: location.href,
      entries: entries
    };
  })()"##;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_link_text_distinguishable(
    snap: &LinkTextDistinguishableSnapshot,
) -> Vec<AxisFinding> {
    let mut findings = Vec::new();
    // Group by accessible-link-text (lowercased + trimmed).
    // aria-label takes precedence over visible text per ARIA
    // name-computation; if aria-label is set, the LIBRARY name
    // for distinguishability is the aria-label.
    let mut groups: BTreeMap<String, Vec<&LinkEntry>> = BTreeMap::new();
    for entry in &snap.entries {
        let accessible_text = entry
            .aria_label
            .as_deref()
            .unwrap_or(&entry.text)
            .trim()
            .to_lowercase();
        if accessible_text.is_empty() {
            continue;
        }
        groups
            .entry(accessible_text)
            .or_default()
            .push(entry);
    }
    for (text, members) in &groups {
        if members.len() < 2 {
            continue;
        }
        // Group of >= 2 links with the same accessible text.
        // CASE A: all distinguished by aria-describedby — fine
        // (assistive tech reads the descriptor + the link text).
        let all_have_distinct_describedby = members.iter().all(|m| {
            m.aria_describedby
                .as_deref()
                .is_some_and(|d| !d.trim().is_empty())
        }) && {
            let mut seen = std::collections::BTreeSet::new();
            members
                .iter()
                .all(|m| seen.insert(m.aria_describedby.as_deref().unwrap_or("")))
        };
        if all_have_distinct_describedby {
            continue;
        }
        // CASE B: hrefs split (different targets) — strict.
        let mut hrefs: std::collections::BTreeSet<&str> =
            std::collections::BTreeSet::new();
        for m in members {
            hrefs.insert(m.href.as_str());
        }
        if hrefs.len() > 1 {
            let href_list: Vec<&str> = hrefs.iter().copied().collect();
            findings.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "link-text.duplicate-text-different-href".to_owned(),
                detail: format!(
                    "{} links on this page all say \"{}\" but point to different URLs: [{}]. Screen-reader users navigating by link list cannot tell them apart. Add aria-label or aria-describedby with distinct values, or rewrite the visible text.",
                    members.len(),
                    text,
                    href_list.join(", ")
                ),
            });
            continue;
        }
        // CASE C: same href + same text + no distinguishing context.
        // Less severe — the link "purpose" is identical (same target),
        // so a screen-reader user choosing any one of them lands on
        // the same place. But a page full of "click here" still
        // reads as weak editorial. Apply the vague-cluster warn.
        if VAGUE_PHRASES.iter().any(|p| text.contains(p)) {
            findings.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "link-text.vague-cluster".to_owned(),
                detail: format!(
                    "{} links on this page say \"{}\" — vague link text appearing in a cluster. Even when targets are the same, replace with descriptive text that names what each link goes to.",
                    members.len(),
                    text
                ),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(
        text: &str,
        href: &str,
        aria_label: Option<&str>,
        aria_describedby: Option<&str>,
    ) -> LinkEntry {
        LinkEntry {
            selector: format!("body > a[href=\"{href}\"]"),
            text: text.to_owned(),
            href: href.to_owned(),
            aria_label: aria_label.map(str::to_owned),
            aria_describedby: aria_describedby.map(str::to_owned),
        }
    }

    fn snap(entries: Vec<LinkEntry>) -> LinkTextDistinguishableSnapshot {
        LinkTextDistinguishableSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_entries_no_findings() {
        assert!(detect_link_text_distinguishable(&snap(Vec::new())).is_empty());
    }

    #[test]
    fn single_link_no_findings() {
        assert!(detect_link_text_distinguishable(&snap(vec![link(
            "Read more", "/a", None, None
        )]))
        .is_empty());
    }

    #[test]
    fn duplicate_text_different_hrefs_is_strict() {
        let findings = detect_link_text_distinguishable(&snap(vec![
            link("Read more", "/posts/1", None, None),
            link("Read more", "/posts/2", None, None),
            link("Read more", "/posts/3", None, None),
        ]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "link-text.duplicate-text-different-href");
        assert!(findings[0].detail.contains("3 links"));
        assert!(findings[0].detail.contains("/posts/1"));
        assert!(findings[0].detail.contains("/posts/2"));
        assert!(findings[0].detail.contains("/posts/3"));
    }

    #[test]
    fn distinct_aria_labels_distinguish_duplicate_text() {
        // Visible text says "Read more" but aria-label varies →
        // accessible-name varies → no finding.
        let findings = detect_link_text_distinguishable(&snap(vec![
            link("Read more", "/posts/1", Some("Read more about cats"), None),
            link("Read more", "/posts/2", Some("Read more about dogs"), None),
        ]));
        assert!(findings.is_empty());
    }

    #[test]
    fn distinct_aria_describedby_distinguishes_duplicate_text() {
        // Same visible text + same aria-label (none) but distinct
        // describedby refs → fine.
        let findings = detect_link_text_distinguishable(&snap(vec![
            link("Read more", "/posts/1", None, Some("desc-1")),
            link("Read more", "/posts/2", None, Some("desc-2")),
        ]));
        assert!(findings.is_empty());
    }

    #[test]
    fn duplicate_describedby_does_NOT_distinguish() {
        // Both links carry the SAME aria-describedby — that's
        // not distinguishing context.
        let findings = detect_link_text_distinguishable(&snap(vec![
            link("Read more", "/posts/1", None, Some("desc-same")),
            link("Read more", "/posts/2", None, Some("desc-same")),
        ]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn same_text_same_href_with_vague_phrase_is_warn_cluster() {
        // Click-here cluster where ALL targets are the same URL.
        // Not a screen-reader ambiguity (same destination), but
        // weak editorial — vague-cluster warn.
        let findings = detect_link_text_distinguishable(&snap(vec![
            link("Click here", "/contact", None, None),
            link("Click here", "/contact", None, None),
            link("Click here", "/contact", None, None),
        ]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "link-text.vague-cluster");
    }

    #[test]
    fn same_text_same_href_non_vague_is_silent() {
        // Two links with the same descriptive text + same target.
        // Not vague + not ambiguous → silent.
        let findings = detect_link_text_distinguishable(&snap(vec![
            link("Documentation", "/docs", None, None),
            link("Documentation", "/docs", None, None),
        ]));
        assert!(findings.is_empty());
    }

    #[test]
    fn case_insensitive_grouping() {
        // "Read More" / "READ MORE" / "read more" should all
        // collapse into one group.
        let findings = detect_link_text_distinguishable(&snap(vec![
            link("Read More", "/a", None, None),
            link("READ MORE", "/b", None, None),
            link("read more", "/c", None, None),
        ]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, "link-text.duplicate-text-different-href");
        assert!(findings[0].detail.contains("3 links"));
    }

    #[test]
    fn multiple_independent_groups_emit_separate_findings() {
        let findings = detect_link_text_distinguishable(&snap(vec![
            link("Read more", "/a", None, None),
            link("Read more", "/b", None, None),
            link("Learn more", "/c", None, None),
            link("Learn more", "/d", None, None),
        ]));
        // Two strict findings, one per group.
        assert_eq!(findings.len(), 2);
        for f in &findings {
            assert_eq!(f.severity, AxisSeverity::Strict);
            assert_eq!(f.kind, "link-text.duplicate-text-different-href");
        }
    }

    #[test]
    fn snapshot_serde_camel_case() {
        let s = snap(vec![link("x", "/x", Some("y"), Some("z"))]);
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"pageUrl\""));
        assert!(j.contains("\"ariaLabel\""));
        assert!(j.contains("\"ariaDescribedby\""));
        let back: LinkTextDistinguishableSnapshot =
            serde_json::from_str(&j).unwrap();
        assert_eq!(back.entries.len(), 1);
    }

    #[test]
    fn vague_phrases_list_covers_known_offenders() {
        for needle in ["read more", "click here", "learn more"] {
            assert!(
                VAGUE_PHRASES.contains(&needle),
                "VAGUE_PHRASES missing {needle}"
            );
        }
    }

    #[test]
    fn js_eval_const_walks_a_href() {
        assert!(LINK_TEXT_DISTINGUISHABLE_JS.contains("querySelectorAll('a[href]')"));
        assert!(LINK_TEXT_DISTINGUISHABLE_JS.contains("ariaLabel"));
        assert!(LINK_TEXT_DISTINGUISHABLE_JS.contains("ariaDescribedby"));
        assert!(LINK_TEXT_DISTINGUISHABLE_JS.contains("isVisible"));
    }
}
