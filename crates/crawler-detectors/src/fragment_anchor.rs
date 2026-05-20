//! `fragment_anchor` — broken in-page anchor link detector.
//!
//! Walks every `<a href="#…">` (same-page fragment links) and
//! flags anchors whose target id doesn't exist on the page.
//! Cross-references the `unique_id` detector — even if no
//! duplicate ids exist, missing targets are still a real bug class.
//!
//! Three findings emitted:
//!
//! * `fragment-anchor.missing-target` — `href="#x"` where no element
//!   has `id="x"`. Strict — clicking the anchor does nothing.
//! * `fragment-anchor.empty-href` — `href="#"` (just the hash, no
//!   target). Warn — usually an anti-pattern; the element should be
//!   `<button>` if it's JS-driven, or carry a real `href`.
//! * `fragment-anchor.duplicate-target` — `href="#x"` where multiple
//!   elements have `id="x"`. Strict — browsers scroll to the FIRST
//!   match; the operator's intent is ambiguous.
//!
//! Out of scope: cross-page fragments (`href="other.html#x"`) —
//! resolving them needs the other page; defer to a future
//! cross-page crawl detector. The `#x`-only fragment is the common
//! same-page anchor pattern (skip-link, ToC, in-doc cross-ref).
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const FRAGMENT_ANCHOR_JS: &str = r##"(() => {
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

    // Build id → count map once. Walking the DOM for every anchor
    // would be O(n*m); this is O(n + m).
    const idCounts = {};
    const allIded = document.querySelectorAll('[id]');
    for (let i = 0; i < allIded.length; i++) {
      const id = allIded[i].getAttribute('id') || '';
      if (id === '') continue;
      idCounts[id] = (idCounts[id] || 0) + 1;
    }

    // Special-case the spec-blessed skip-link target. The HTML spec
    // treats `href="#top"` and `href="#"` as scrolling to top
    // even without a matching id. We exempt `#top` from the missing-
    // target check but still flag `#` as empty-href (anti-pattern).
    const SPEC_TOP_TARGETS = ['top'];

    let scanned = 0;
    const missing = [];
    const empty = [];
    const duplicate = [];

    const anchors = document.querySelectorAll('a[href]');
    for (let i = 0; i < anchors.length; i++) {
      const el = anchors[i];
      const href = el.getAttribute('href') || '';
      if (href.length === 0) continue;
      if (href[0] !== '#') continue;
      scanned += 1;
      // Strip the leading '#'. Empty after that = empty-href.
      const fragment = href.slice(1);
      if (fragment === '') {
        empty.push({
          selector: selectorOf(el),
          text: (el.textContent || '').trim().slice(0, 60),
          href: href
        });
        continue;
      }
      // Spec-blessed top targets — skip the missing-target check.
      if (SPEC_TOP_TARGETS.indexOf(fragment) !== -1) continue;
      const count = idCounts[fragment] || 0;
      if (count === 0) {
        missing.push({
          selector: selectorOf(el),
          text: (el.textContent || '').trim().slice(0, 60),
          fragment: fragment
        });
      } else if (count > 1) {
        duplicate.push({
          selector: selectorOf(el),
          text: (el.textContent || '').trim().slice(0, 60),
          fragment: fragment,
          count: count
        });
      }
    }

    return {
      scanned: scanned,
      missing: missing.slice(0, 50),
      missingCount: missing.length,
      empty: empty.slice(0, 50),
      emptyCount: empty.length,
      duplicate: duplicate.slice(0, 50),
      duplicateCount: duplicate.length
    };
})()"##;

/// One broken-anchor row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct MissingAnchor {
    /// CSS selector for the anchor.
    pub selector: String,
    /// First 60 chars of the anchor's text.
    pub text: String,
    /// Fragment id the anchor targets (without the leading `#`).
    pub fragment: String,
}

/// One empty-href row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct EmptyHrefAnchor {
    /// CSS selector for the anchor.
    pub selector: String,
    /// First 60 chars of the anchor's text.
    pub text: String,
    /// The raw href (always `"#"`).
    pub href: String,
}

/// One duplicate-target row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct DuplicateAnchor {
    /// CSS selector for the anchor.
    pub selector: String,
    /// First 60 chars of the anchor's text.
    pub text: String,
    /// Fragment id the anchor targets.
    pub fragment: String,
    /// Number of elements on the page sharing this id.
    pub count: u32,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct FragmentAnchorSnapshot {
    /// Total in-page fragment anchors walked (excludes cross-page hrefs).
    pub scanned: u32,
    /// Anchors whose target id does not exist on the page.
    pub missing: Vec<MissingAnchor>,
    /// Total missing-target anchors (may exceed `missing.len()`).
    pub missing_count: u32,
    /// Anchors with `href="#"` (no target).
    pub empty: Vec<EmptyHrefAnchor>,
    /// Total empty-href anchors.
    pub empty_count: u32,
    /// Anchors whose target id appears multiple times on the page.
    pub duplicate: Vec<DuplicateAnchor>,
    /// Total duplicate-target anchors.
    pub duplicate_count: u32,
}

/// Apply detection rules. Pure function.
///
/// Emits up to three findings — one per category that contains
/// offenders. Severity:
/// * missing-target → strict
/// * duplicate-target → strict
/// * empty-href → warn
#[must_use]
pub fn detect_fragment_anchor_issues(snap: &FragmentAnchorSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::new();
    if !snap.missing.is_empty() {
        let first = &snap.missing[0];
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "fragment-anchor.missing-target".to_owned(),
            detail: format!(
                "{} in-page anchor(s) target a fragment id that does not exist. Clicking does nothing. First: <a href=\"#{}\"> \"{}\" @ {}. Either remove the broken anchor, add an `id=\"{}\"` to the intended target, or fix the typo.",
                snap.missing_count,
                first.fragment,
                first.text,
                first.selector,
                first.fragment,
            ),
        });
    }
    if !snap.duplicate.is_empty() {
        let first = &snap.duplicate[0];
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "fragment-anchor.duplicate-target".to_owned(),
            detail: format!(
                "{} in-page anchor(s) target a fragment id that appears {}× on the page. Browsers scroll to the FIRST match — the operator's intent is ambiguous. First: <a href=\"#{}\"> \"{}\" @ {}. Resolve via the unique_id detector + this anchor's intended destination.",
                snap.duplicate_count,
                first.count,
                first.fragment,
                first.text,
                first.selector,
            ),
        });
    }
    if !snap.empty.is_empty() {
        let first = &snap.empty[0];
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "fragment-anchor.empty-href".to_owned(),
            detail: format!(
                "{} anchor(s) have `href=\"#\"` (no target). Usually an anti-pattern — JS-driven anchors should be `<button>` so keyboard activation + screen-reader announcement work correctly. First: \"{}\" @ {}.",
                snap.empty_count,
                first.text,
                first.selector,
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AxisSeverity;

    #[test]
    fn js_balanced() {
        assert_eq!(
            FRAGMENT_ANCHOR_JS.matches('(').count(),
            FRAGMENT_ANCHOR_JS.matches(')').count()
        );
        assert_eq!(
            FRAGMENT_ANCHOR_JS.matches('{').count(),
            FRAGMENT_ANCHOR_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(FRAGMENT_ANCHOR_JS.starts_with("(() => {"));
        assert!(FRAGMENT_ANCHOR_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "scanned",
            "missing",
            "missingCount",
            "empty",
            "emptyCount",
            "duplicate",
            "duplicateCount",
            "selector",
            "text",
            "fragment",
            "href",
            "count",
        ] {
            assert!(FRAGMENT_ANCHOR_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_skips_spec_top_target() {
        // href="#top" is spec-blessed and should NOT count as missing
        // even when no element has id="top".
        assert!(FRAGMENT_ANCHOR_JS.contains("SPEC_TOP_TARGETS"));
        assert!(FRAGMENT_ANCHOR_JS.contains("'top'"));
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = FragmentAnchorSnapshot {
            scanned: 5,
            missing: vec![],
            missing_count: 0,
            empty: vec![],
            empty_count: 0,
            duplicate: vec![],
            duplicate_count: 0,
        };
        let findings = detect_fragment_anchor_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn missing_target_emits_strict() {
        let snap = FragmentAnchorSnapshot {
            scanned: 3,
            missing: vec![MissingAnchor {
                selector: "body > nav > a".to_owned(),
                text: "Pricing".to_owned(),
                fragment: "pricing".to_owned(),
            }],
            missing_count: 1,
            empty: vec![],
            empty_count: 0,
            duplicate: vec![],
            duplicate_count: 0,
        };
        let findings = detect_fragment_anchor_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "fragment-anchor.missing-target");
        assert!(findings[0].detail.contains(r##"href="#pricing""##));
        assert!(findings[0].detail.contains(r##"id="pricing""##));
        assert!(findings[0].detail.contains("Pricing"));
    }

    #[test]
    fn empty_href_emits_warn() {
        let snap = FragmentAnchorSnapshot {
            scanned: 2,
            missing: vec![],
            missing_count: 0,
            empty: vec![EmptyHrefAnchor {
                selector: "body > div > a".to_owned(),
                text: "Open menu".to_owned(),
                href: "#".to_owned(),
            }],
            empty_count: 1,
            duplicate: vec![],
            duplicate_count: 0,
        };
        let findings = detect_fragment_anchor_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Warn));
        assert_eq!(findings[0].kind, "fragment-anchor.empty-href");
        assert!(findings[0].detail.contains("<button>"));
        assert!(findings[0].detail.contains("Open menu"));
    }

    #[test]
    fn duplicate_target_emits_strict() {
        let snap = FragmentAnchorSnapshot {
            scanned: 2,
            missing: vec![],
            missing_count: 0,
            empty: vec![],
            empty_count: 0,
            duplicate: vec![DuplicateAnchor {
                selector: "body > nav > a".to_owned(),
                text: "Section".to_owned(),
                fragment: "main".to_owned(),
                count: 3,
            }],
            duplicate_count: 1,
        };
        let findings = detect_fragment_anchor_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "fragment-anchor.duplicate-target");
        assert!(findings[0].detail.contains("3×"));
        assert!(findings[0].detail.contains("unique_id"));
    }

    #[test]
    fn all_three_categories_emit_three_findings() {
        let snap = FragmentAnchorSnapshot {
            scanned: 6,
            missing: vec![MissingAnchor {
                selector: "x".to_owned(),
                text: "x".to_owned(),
                fragment: "x".to_owned(),
            }],
            missing_count: 1,
            empty: vec![EmptyHrefAnchor {
                selector: "y".to_owned(),
                text: "y".to_owned(),
                href: "#".to_owned(),
            }],
            empty_count: 1,
            duplicate: vec![DuplicateAnchor {
                selector: "z".to_owned(),
                text: "z".to_owned(),
                fragment: "z".to_owned(),
                count: 2,
            }],
            duplicate_count: 1,
        };
        let findings = detect_fragment_anchor_issues(&snap);
        assert_eq!(findings.len(), 3);
        // Order: missing (strict), duplicate (strict), empty (warn).
        assert_eq!(findings[0].kind, "fragment-anchor.missing-target");
        assert_eq!(findings[1].kind, "fragment-anchor.duplicate-target");
        assert_eq!(findings[2].kind, "fragment-anchor.empty-href");
    }

    #[test]
    fn truncated_counts_above_array_len_surface_correctly() {
        let snap = FragmentAnchorSnapshot {
            scanned: 200,
            missing: vec![MissingAnchor {
                selector: "a".to_owned(),
                text: "a".to_owned(),
                fragment: "a".to_owned(),
            }],
            missing_count: 73,
            empty: vec![],
            empty_count: 0,
            duplicate: vec![],
            duplicate_count: 0,
        };
        let findings = detect_fragment_anchor_issues(&snap);
        assert!(findings[0].detail.contains("73 in-page anchor"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = FragmentAnchorSnapshot {
            scanned: 4,
            missing: vec![MissingAnchor {
                selector: "x".to_owned(),
                text: "RT".to_owned(),
                fragment: "rt".to_owned(),
            }],
            missing_count: 1,
            empty: vec![],
            empty_count: 0,
            duplicate: vec![DuplicateAnchor {
                selector: "y".to_owned(),
                text: "Y".to_owned(),
                fragment: "main".to_owned(),
                count: 2,
            }],
            duplicate_count: 1,
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"scanned\":4"));
        assert!(json.contains("\"missingCount\":1"));
        let back: FragmentAnchorSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.missing.len(), 1);
        assert_eq!(back.duplicate[0].count, 2);
    }
}
