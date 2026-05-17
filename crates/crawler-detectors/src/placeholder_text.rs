//! `placeholder_text` — sentinel-text detector. T16 port of `src/placeholderText.ts`.
//!
//! Catches paste-a-template-and-forgot signals in the rendered DOM:
//! developer copy-deck markers (TODO / FIXME / XXX / HACK), Lorem
//! ipsum filler, and explicit template instructions ("delete me",
//! "sample text", "your text here") that ship a placeholder to
//! production.
//!
//! Categories + severities mirror the TS source byte-for-byte:
//!
//!   * `placeholder.lorem-ipsum` — strict
//!   * `placeholder.template`    — strict
//!   * `placeholder.dev-marker`  — strict
//!   * `placeholder.coming-soon` — warn (intentional sometimes)
//!
//! Split-of-concerns mirrors the TS file: a browser-side capture JS
//! const (kept identical so Playwright and chromiumoxide produce
//! identical snapshots) and a pure-Rust classifier consuming the
//! snapshot.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Pattern category. Mirrors the TS string-literal union.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlaceholderCategory {
    /// Lorem ipsum filler text in rendered DOM.
    LoremIpsum,
    /// Explicit template instructions ("delete me", "sample text", etc.).
    Template,
    /// Developer-only markers (TODO / FIXME / XXX / HACK).
    DevMarker,
    /// "Coming soon" / "TBD" — sometimes intentional, sometimes not.
    ComingSoon,
}

impl PlaceholderCategory {
    fn kind_str(self) -> &'static str {
        match self {
            Self::LoremIpsum => "placeholder.lorem-ipsum",
            Self::Template => "placeholder.template",
            Self::DevMarker => "placeholder.dev-marker",
            Self::ComingSoon => "placeholder.coming-soon",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::LoremIpsum => {
                "Lorem ipsum filler text reached the rendered DOM. Replace with real copy before shipping."
            }
            Self::Template => {
                "Template instructions (\"delete me\", \"sample text\", \"your text here\", etc.) leaked into the rendered DOM."
            }
            Self::DevMarker => {
                "Developer-only marker (TODO / FIXME / XXX / HACK) is visible to end users."
            }
            Self::ComingSoon => {
                "\"Coming soon\" / \"TBD\" placeholder copy in the rendered DOM. Acceptable when intentional, but flag for review."
            }
        }
    }

    fn severity(self) -> AxisSeverity {
        match self {
            Self::LoremIpsum | Self::Template | Self::DevMarker => AxisSeverity::Strict,
            Self::ComingSoon => AxisSeverity::Warn,
        }
    }
}

/// One captured hit — a text node that matched one of the patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PlaceholderHit {
    /// CSS-ish path of the enclosing element.
    pub selector: String,
    /// Matched category.
    pub category: PlaceholderCategory,
    /// Substring that matched (TS caps at 80 chars).
    pub match_text: String,
    /// Surrounding text for context (TS caps at 160 chars).
    pub context: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PlaceholderTextSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every text-node hit the walker found.
    pub hits: Vec<PlaceholderHit>,
    /// Total visible text-node chars walked — noise-floor signal.
    pub scanned_chars: usize,
}

/// Pure detector: snapshot → findings, one finding per category that
/// fired (multiple hits in the same category collapse to one finding
/// with up to 5 examples, matching TS).
pub fn detect_placeholder_text_issues(snap: &PlaceholderTextSnapshot) -> Vec<AxisFinding> {
    let mut buckets: BTreeMap<PlaceholderCategory, Vec<&PlaceholderHit>> = BTreeMap::new();
    for h in &snap.hits {
        buckets.entry(h.category).or_default().push(h);
    }

    let mut out = Vec::new();
    for (category, hits) in &buckets {
        let examples: Vec<String> = hits
            .iter()
            .take(5)
            .map(|h| format!("{} → \"{}\"", h.selector, h.match_text))
            .collect();
        out.push(AxisFinding {
            severity: category.severity(),
            kind: category.kind_str().to_owned(),
            detail: format!(
                "{} hit(s): {} Examples: {}",
                hits.len(),
                category.description(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Pinned char-for-char to
/// `src/placeholderText.ts`'s `evalFn` template literal so the
/// future chromiumoxide path produces identical snapshots to the
/// current Playwright path.
///
/// REGRESSION-GUARD: the patterns inside this JS are mirrored by the
/// classifier's `PlaceholderCategory` enum. Adding or renaming a
/// category requires the same edit in both places + a new test that
/// exercises the new path end-to-end.
pub const PLACEHOLDER_TEXT_DOM_CAPTURE_JS: &str = r#"
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
      return 'body > ' + parts.join(' > ');
    };

    const isHidden = function(el) {
      if (!el || el.nodeType !== 1) return false;
      if (el.getAttribute && el.getAttribute('aria-hidden') === 'true') return true;
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return true;
      return false;
    };

    const ancestorHidden = function(node) {
      let cur = node;
      while (cur && cur !== document.body) {
        if (cur.nodeType === 1 && isHidden(cur)) return true;
        cur = cur.parentElement;
      }
      return false;
    };

    const patterns = [
      { category: 'lorem-ipsum',  re: /lorem ipsum/i },
      { category: 'template',     re: /\b(delete me|remove me|replace this|sample text|placeholder text|your text here|insert .{1,20} here)\b/i },
      { category: 'dev-marker',   re: /\b(TODO|FIXME|XXX|HACK)\b/ },
      { category: 'coming-soon',  re: /\b(coming soon|tbd|to be (?:announced|determined))\b/i },
    ];

    const hits = [];
    let scannedChars = 0;
    const walker = document.createTreeWalker(
      document.body,
      NodeFilter.SHOW_TEXT,
      {
        acceptNode: function(node) {
          const p = node.parentElement;
          if (!p) return NodeFilter.FILTER_REJECT;
          const tag = p.tagName;
          if (tag === 'SCRIPT' || tag === 'STYLE' || tag === 'NOSCRIPT' || tag === 'TEMPLATE') {
            return NodeFilter.FILTER_REJECT;
          }
          if (p.closest && p.closest('.forge-overlay, .loom-skip-link, [data-forge-overlay]')) {
            return NodeFilter.FILTER_REJECT;
          }
          if (ancestorHidden(p)) return NodeFilter.FILTER_REJECT;
          return NodeFilter.FILTER_ACCEPT;
        }
      }
    );

    let node;
    while ((node = walker.nextNode())) {
      const raw = (node.nodeValue || '').trim();
      if (!raw) continue;
      scannedChars += raw.length;
      for (const pat of patterns) {
        const m = raw.match(pat.re);
        if (!m) continue;
        const matchText = (m[0] || '').slice(0, 80);
        const idx = m.index !== undefined ? m.index : raw.indexOf(m[0]);
        const start = Math.max(0, idx - 40);
        const end = Math.min(raw.length, idx + matchText.length + 40);
        const ctx = raw.slice(start, end);
        hits.push({
          selector: selectorOf(node.parentElement),
          category: pat.category,
          match: matchText,
          context: ctx,
        });
        break;
      }
    }
    return { hits: hits, scannedChars: scannedChars };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(category: PlaceholderCategory, selector: &str, m: &str) -> PlaceholderHit {
        PlaceholderHit {
            selector: selector.to_owned(),
            category,
            match_text: m.to_owned(),
            context: m.to_owned(),
        }
    }

    fn snap(hits: Vec<PlaceholderHit>) -> PlaceholderTextSnapshot {
        PlaceholderTextSnapshot {
            page_url: "https://example.com/".to_owned(),
            scanned_chars: 100,
            hits,
        }
    }

    #[test]
    fn empty_snapshot_no_findings() {
        let s = snap(Vec::new());
        assert!(detect_placeholder_text_issues(&s).is_empty());
    }

    #[test]
    fn lorem_ipsum_is_strict() {
        let s = snap(vec![hit(
            PlaceholderCategory::LoremIpsum,
            "body > p",
            "Lorem ipsum",
        )]);
        let f = detect_placeholder_text_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "placeholder.lorem-ipsum");
        assert_eq!(f[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn template_is_strict() {
        let s = snap(vec![hit(
            PlaceholderCategory::Template,
            "body > h1",
            "Your text here",
        )]);
        let f = detect_placeholder_text_issues(&s);
        assert_eq!(f[0].kind, "placeholder.template");
        assert_eq!(f[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn dev_marker_is_strict() {
        let s = snap(vec![hit(
            PlaceholderCategory::DevMarker,
            "body > p",
            "TODO",
        )]);
        let f = detect_placeholder_text_issues(&s);
        assert_eq!(f[0].kind, "placeholder.dev-marker");
        assert_eq!(f[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn coming_soon_is_warn() {
        let s = snap(vec![hit(
            PlaceholderCategory::ComingSoon,
            "body > section",
            "coming soon",
        )]);
        let f = detect_placeholder_text_issues(&s);
        assert_eq!(f[0].kind, "placeholder.coming-soon");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn multiple_categories_one_finding_each() {
        let s = snap(vec![
            hit(
                PlaceholderCategory::DevMarker,
                "body > p:nth-of-type(1)",
                "TODO",
            ),
            hit(
                PlaceholderCategory::LoremIpsum,
                "body > p:nth-of-type(2)",
                "Lorem ipsum",
            ),
            hit(PlaceholderCategory::ComingSoon, "body > div", "coming soon"),
        ]);
        let f = detect_placeholder_text_issues(&s);
        assert_eq!(f.len(), 3);
        // BTreeMap-ordered (declaration order in enum)
        let kinds: Vec<&str> = f.iter().map(|x| x.kind.as_str()).collect();
        assert!(kinds.contains(&"placeholder.lorem-ipsum"));
        assert!(kinds.contains(&"placeholder.template") == false);
        assert!(kinds.contains(&"placeholder.dev-marker"));
        assert!(kinds.contains(&"placeholder.coming-soon"));
    }

    #[test]
    fn same_category_multiple_hits_aggregate_into_one_finding() {
        let s = snap(vec![
            hit(
                PlaceholderCategory::DevMarker,
                "body > p:nth-of-type(1)",
                "TODO",
            ),
            hit(
                PlaceholderCategory::DevMarker,
                "body > p:nth-of-type(2)",
                "FIXME",
            ),
            hit(
                PlaceholderCategory::DevMarker,
                "body > p:nth-of-type(3)",
                "HACK",
            ),
        ]);
        let f = detect_placeholder_text_issues(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("3 hit"));
    }

    #[test]
    fn examples_capped_at_5() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                PlaceholderCategory::DevMarker,
                &format!("body > p:nth-of-type({})", i + 1),
                "TODO",
            ));
        }
        let s = snap(hits);
        let f = detect_placeholder_text_issues(&s);
        assert!(f[0].detail.contains("10 hit"));
        // 5 example arrows
        let arrows = f[0].detail.matches(" → \"").count();
        assert_eq!(arrows, 5);
    }

    #[test]
    fn finding_detail_includes_selector_and_match() {
        let s = snap(vec![hit(
            PlaceholderCategory::Template,
            "body > main > h1:nth-of-type(2)",
            "delete me",
        )]);
        let f = detect_placeholder_text_issues(&s);
        assert!(f[0].detail.contains("body > main > h1:nth-of-type(2)"));
        assert!(f[0].detail.contains("delete me"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap(vec![
            hit(PlaceholderCategory::DevMarker, "body > p", "TODO"),
            hit(PlaceholderCategory::ComingSoon, "body > footer", "TBD"),
        ]);
        let j = serde_json::to_string(&s).expect("ser");
        let back: PlaceholderTextSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.hits.len(), s.hits.len());
        assert_eq!(back.hits[0].category, PlaceholderCategory::DevMarker);
    }

    #[test]
    fn category_kebab_case_serde() {
        let j = serde_json::to_string(&PlaceholderCategory::LoremIpsum).unwrap();
        assert_eq!(j, "\"lorem-ipsum\"");
        let j = serde_json::to_string(&PlaceholderCategory::DevMarker).unwrap();
        assert_eq!(j, "\"dev-marker\"");
        let j = serde_json::to_string(&PlaceholderCategory::ComingSoon).unwrap();
        assert_eq!(j, "\"coming-soon\"");
    }

    #[test]
    fn js_brackets_balanced() {
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        let mut bracket: i32 = 0;
        for c in PLACEHOLDER_TEXT_DOM_CAPTURE_JS.chars() {
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
    fn js_includes_all_four_pattern_categories() {
        // Catches accidental edits to the capture JS that drop a category.
        for cat in [
            "'lorem-ipsum'",
            "'template'",
            "'dev-marker'",
            "'coming-soon'",
        ] {
            assert!(
                PLACEHOLDER_TEXT_DOM_CAPTURE_JS.contains(cat),
                "capture JS missing category literal {cat}"
            );
        }
    }
}
