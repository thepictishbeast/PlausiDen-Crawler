//! `dom_size` — total DOM node count.
//!
//! Excessive DOM size is one of the most reliable predictors of
//! poor rendering performance. Chromium's Lighthouse recommends:
//!
//!   * `< 1500` nodes        — healthy
//!   * `1500..=3000` nodes   — warn (rendering cost climbs)
//!   * `> 3000` nodes        — strict (significant CLS/INP risk)
//!
//! Two secondary checks the detector also runs:
//!
//!   * `dom-size.max-depth`  — the deepest descendant chain. > 32
//!                              fires `warn` (matches Lighthouse).
//!   * `dom-size.max-children` — single element with > 60
//!                                children fires `warn` (long lists
//!                                without virtualization).
//!
//! Findings:
//!   * `dom-size.too-large`    strict   total > 3000
//!   * `dom-size.large`        warn     1500..=3000
//!   * `dom-size.deep`         warn     max-depth > 32
//!   * `dom-size.wide`         warn     max-children > 60
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * Pure detector function; no I/O.
//! * Snapshot is pre-collected; this module does the math + finding emission.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Healthy DOM size ceiling (Lighthouse default).
pub const DOM_SIZE_WARN: u32 = 1500;

/// Strict DOM size ceiling.
pub const DOM_SIZE_STRICT: u32 = 3000;

/// Max-depth threshold (Lighthouse default).
pub const DOM_DEPTH_WARN: u32 = 32;

/// Max-children threshold per single element.
pub const DOM_WIDTH_WARN: u32 = 60;

/// Page-side eval. Walks the document and reports the three
/// counters the detector needs. Returns:
///
/// ```json
/// { "total": 1234, "maxDepth": 12, "maxChildren": 18 }
/// ```
pub const DOM_SIZE_JS: &str = r##"(() => {
    let total = 0;
    let maxDepth = 0;
    let maxChildren = 0;

    const walk = function(node, depth) {
        total += 1;
        if (depth > maxDepth) maxDepth = depth;
        const childCount = node.children ? node.children.length : 0;
        if (childCount > maxChildren) maxChildren = childCount;
        if (node.children) {
            for (let i = 0; i < node.children.length; i++) {
                walk(node.children[i], depth + 1);
            }
        }
    };
    walk(document.documentElement, 1);
    return { total, maxDepth, maxChildren };
})()"##;

/// Captured DOM-size counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DomSizeSnapshot {
    /// Total element count.
    pub total: u32,
    /// Deepest descendant chain in nodes.
    pub max_depth: u32,
    /// Largest sibling group under a single element.
    pub max_children: u32,
}

/// Build a snapshot from raw counters (e.g. parsed from JS eval
/// output).
pub fn build_dom_size_snapshot(total: u32, max_depth: u32, max_children: u32) -> DomSizeSnapshot {
    DomSizeSnapshot {
        total,
        max_depth,
        max_children,
    }
}

/// Run the detector against a snapshot.
pub fn detect_dom_size_issues(snap: &DomSizeSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    if snap.total > DOM_SIZE_STRICT {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "dom-size.too-large".into(),
            detail: format!(
                "DOM has {} elements (> {} cap); rendering + interaction perf at risk",
                snap.total, DOM_SIZE_STRICT
            ),
        });
    } else if snap.total > DOM_SIZE_WARN {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "dom-size.large".into(),
            detail: format!(
                "DOM has {} elements (> {} recommended)",
                snap.total, DOM_SIZE_WARN
            ),
        });
    }
    if snap.max_depth > DOM_DEPTH_WARN {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "dom-size.deep".into(),
            detail: format!(
                "deepest DOM chain is {} levels (> {} threshold)",
                snap.max_depth, DOM_DEPTH_WARN
            ),
        });
    }
    if snap.max_children > DOM_WIDTH_WARN {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "dom-size.wide".into(),
            detail: format!(
                "largest sibling group is {} children (> {} threshold; consider virtualization)",
                snap.max_children, DOM_WIDTH_WARN
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_dom_emits_nothing() {
        let s = build_dom_size_snapshot(800, 12, 18);
        assert!(detect_dom_size_issues(&s).is_empty());
    }

    #[test]
    fn warn_at_1500_to_3000() {
        let s = build_dom_size_snapshot(2000, 10, 10);
        let findings = detect_dom_size_issues(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "dom-size.large");
    }

    #[test]
    fn strict_above_3000() {
        let s = build_dom_size_snapshot(5000, 10, 10);
        let findings = detect_dom_size_issues(&s);
        assert!(findings.iter().any(|f| f.kind == "dom-size.too-large"));
        assert!(findings.iter().any(|f| f.severity == AxisSeverity::Strict));
    }

    #[test]
    fn deep_dom_warns() {
        let s = build_dom_size_snapshot(500, 40, 10);
        let findings = detect_dom_size_issues(&s);
        assert!(findings.iter().any(|f| f.kind == "dom-size.deep"));
    }

    #[test]
    fn wide_dom_warns() {
        let s = build_dom_size_snapshot(500, 10, 200);
        let findings = detect_dom_size_issues(&s);
        assert!(findings.iter().any(|f| f.kind == "dom-size.wide"));
    }

    #[test]
    fn multiple_thresholds_compose() {
        let s = build_dom_size_snapshot(4000, 40, 200);
        let findings = detect_dom_size_issues(&s);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"dom-size.too-large"));
        assert!(kinds.contains(&"dom-size.deep"));
        assert!(kinds.contains(&"dom-size.wide"));
    }
}
