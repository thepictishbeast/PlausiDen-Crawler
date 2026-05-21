//! `z_index_misuse` — flags two common stacking-context bugs:
//!
//! 1. **Absurd z-index** — values like `9999999` / `2147483647`
//!    that scream "stack on top of everything" — usually a hack
//!    around a stacking-context bug the operator didn't track
//!    down. Once two scripts both pick that strategy they fight
//!    forever; on real pages it's how modals end up rendered
//!    BEHIND a sticky header that "won" the absurd-number race.
//!
//! 2. **No-op z-index** — `z-index` set on an element whose
//!    `position` is `static` (the default). Per the
//!    [CSS spec](https://drafts.csswg.org/css-position/#z-index)
//!    z-index only applies to positioned elements (`relative` /
//!    `absolute` / `fixed` / `sticky`) or flex/grid items. On
//!    a `static` element it's silently ignored — the operator
//!    pasted a stylesheet rule expecting it to work and it
//!    doesn't.
//!
//! Both classes are pure cleanliness signals — they don't
//! always produce a visible defect (the absurd value might
//! actually be solving a real ordering problem; the no-op is
//! invisible by definition). But both indicate code paid for
//! at design time that earns no behaviour — so a future
//! refactor can drop them.
//!
//! ## Heuristic
//!
//! JS walks all elements, captures `(position, z-index)` pairs.
//! Skips elements where z-index is `auto` or `0` (the
//! overwhelmingly common case — no signal). Captures:
//!
//! * `is_absurd: |z| > ABSURD_THRESHOLD` (1,000,000).
//! * `has_no_op_z_index: position == 'static' AND z-index != 'auto'`.
//!
//! Detector splits the captured hits into Strict (absurd) and
//! Warn (no-op) buckets.
//!
//! ## Severity
//!
//! * **Strict** — `is_absurd`. Stack-on-top-of-everything hack.
//! * **Warn** — `has_no_op_z_index`. Dead rule.
//!
//! Honors a `data-z-index-allow="true"` opt-out for legitimate
//! "always on top" elements (a help bubble that genuinely must
//! sit above every possible modal — operator declares the
//! intent).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ZIndexMisuseHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Visible text (capped at 40 chars) for context.
    pub text: String,
    /// Computed `z-index` string verbatim
    /// (`"9999999"`, `"-1"`, etc.).
    pub z_index: String,
    /// Parsed numeric z-index, when the computed value parses
    /// as an integer. `None` for `"auto"`-ish values that
    /// shouldn't reach the snapshot but defensively handled.
    pub z_index_numeric: Option<i64>,
    /// Computed `position` value (`"static"`, `"relative"`,
    /// `"absolute"`, `"fixed"`, `"sticky"`).
    pub position: String,
    /// True iff `|z_index_numeric| > ABSURD_THRESHOLD`.
    pub is_absurd: bool,
    /// True iff `position == "static"` AND z-index is non-auto
    /// (silently ignored by the browser).
    pub has_no_op_z_index: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ZIndexMisuseSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Every element whose computed style declared a non-auto
    /// z-index, with the misuse flags set.
    pub hits: Vec<ZIndexMisuseHit>,
    /// Total elements walked.
    pub scanned_elements: u32,
}

/// Z-index magnitude above which the value is treated as
/// "absurd" (stack-on-top-of-everything hack). 1,000,000 is a
/// generous floor — legitimate ladders never need more than 5
/// layers, so anything over 1M is intent rather than measure.
pub const ABSURD_THRESHOLD: i64 = 1_000_000;

/// Maximum examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_z_index_misuse(snap: &ZIndexMisuseSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut absurd: Vec<&ZIndexMisuseHit> = Vec::new();
    let mut no_op: Vec<&ZIndexMisuseHit> = Vec::new();
    for h in &snap.hits {
        if h.is_absurd {
            absurd.push(h);
        } else if h.has_no_op_z_index {
            no_op.push(h);
        }
    }

    let mut out = Vec::new();
    if !absurd.is_empty() {
        let examples: Vec<String> = absurd
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| {
                let label = if h.text.is_empty() {
                    String::new()
                } else {
                    format!(" \"{}\"", h.text)
                };
                format!(
                    "{}{} (z-index={}, position={})",
                    h.selector, label, h.z_index, h.position
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "z-index-misuse.absurd-value".to_owned(),
            detail: format!(
                "{} element(s) using |z-index| > {} — stack-on-top-of-everything hack that fights with any other code following the same strategy. Find the underlying stacking-context bug, then collapse to a 0-5 ladder. Opt out with `data-z-index-allow=\"true\"` when the value is genuinely intentional. Examples: {}",
                absurd.len(),
                ABSURD_THRESHOLD,
                examples.join("; ")
            ),
        });
    }
    if !no_op.is_empty() {
        let examples: Vec<String> = no_op
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| {
                let label = if h.text.is_empty() {
                    String::new()
                } else {
                    format!(" \"{}\"", h.text)
                };
                format!(
                    "{}{} (z-index={} on position=static — silently ignored)",
                    h.selector, label, h.z_index
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "z-index-misuse.no-op".to_owned(),
            detail: format!(
                "{} element(s) carry z-index on position:static — silently ignored by the browser. Either remove the dead rule or add `position: relative` to make it effective. Examples: {}",
                no_op.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every element, picks
/// out positions with a non-`auto` z-index, computes the misuse
/// flags.
///
/// Mirror any change in this file's `ZIndexMisuseHit` /
/// `ZIndexMisuseSnapshot` field set.
pub const Z_INDEX_MISUSE_DOM_CAPTURE_JS: &str = r#"
(() => {
    const ABSURD_THRESHOLD = 1000000;

    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      if (el.id) return '#' + el.id;
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

    const hits = [];
    let scanned = 0;
    const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT, null);
    let node = walk.currentNode;
    while (node) {
      if (node.nodeType === 1) {
        scanned += 1;
        const allow = node.getAttribute && node.getAttribute('data-z-index-allow') === 'true';
        if (!allow) {
          const cs = window.getComputedStyle(node);
          const z = (cs.zIndex || 'auto').trim();
          // 'auto' (default) + numeric '0' add no signal; skip.
          if (z !== 'auto' && z !== '0') {
            const parsed = parseInt(z, 10);
            const num = Number.isFinite(parsed) ? parsed : null;
            const pos = (cs.position || 'static').trim();
            const isAbsurd = num != null && Math.abs(num) > ABSURD_THRESHOLD;
            const hasNoOp = pos === 'static';
            if (isAbsurd || hasNoOp) {
              const text = (node.textContent || '').trim().substring(0, 40);
              hits.push({
                selector: selectorOf(node),
                text: text,
                zIndex: z,
                zIndexNumeric: num,
                position: pos,
                isAbsurd: isAbsurd,
                hasNoOpZIndex: hasNoOp
              });
            }
          }
        }
      }
      node = walk.nextNode();
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedElements: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        selector: &str,
        z_index: &str,
        position: &str,
        is_absurd: bool,
        has_no_op: bool,
    ) -> ZIndexMisuseHit {
        let num = z_index.parse::<i64>().ok();
        ZIndexMisuseHit {
            selector: selector.into(),
            text: String::new(),
            z_index: z_index.into(),
            z_index_numeric: num,
            position: position.into(),
            is_absurd,
            has_no_op_z_index: has_no_op,
        }
    }

    fn snap(hits: Vec<ZIndexMisuseHit>) -> ZIndexMisuseSnapshot {
        ZIndexMisuseSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_elements: 100,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_z_index_misuse(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn absurd_value_is_strict() {
        let s = snap(vec![hit(".tooltip", "9999999", "fixed", true, false)]);
        let findings = detect_z_index_misuse(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "z-index-misuse.absurd-value");
        assert!(findings[0].detail.contains(".tooltip"));
        assert!(findings[0].detail.contains("9999999"));
    }

    #[test]
    fn no_op_z_index_on_static_position_is_warn() {
        let s = snap(vec![hit(".card", "5", "static", false, true)]);
        let findings = detect_z_index_misuse(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "z-index-misuse.no-op");
        assert!(findings[0].detail.contains("silently ignored"));
    }

    #[test]
    fn mixed_severity_emits_two_findings() {
        let s = snap(vec![
            hit(".a", "2147483647", "fixed", true, false),
            hit(".b", "10", "static", false, true),
            hit(".c", "3", "relative", false, false), // clean — won't push
        ]);
        let findings = detect_z_index_misuse(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"z-index-misuse.absurd-value"));
        assert!(kinds.contains(&"z-index-misuse.no-op"));
    }

    #[test]
    fn negative_absurd_value_also_strict() {
        // Sub-zero stack-on-bottom-of-everything is the mirror
        // hack of stack-on-top-of-everything — flag the same.
        let s = snap(vec![hit(".sub", "-9999999", "absolute", true, false)]);
        let findings = detect_z_index_misuse(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn moderate_z_index_on_positioned_element_no_finding() {
        let s = snap(vec![hit(".header", "10", "sticky", false, false)]);
        let findings = detect_z_index_misuse(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!(".bad-{i}"),
                "9999999",
                "fixed",
                true,
                false,
            ));
        }
        let s = snap(hits);
        let findings = detect_z_index_misuse(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 element(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape.
        assert!(Z_INDEX_MISUSE_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(Z_INDEX_MISUSE_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(Z_INDEX_MISUSE_DOM_CAPTURE_JS.contains("hits"));
        assert!(Z_INDEX_MISUSE_DOM_CAPTURE_JS.contains("scannedElements"));
        assert!(Z_INDEX_MISUSE_DOM_CAPTURE_JS.contains("zIndex"));
        assert!(Z_INDEX_MISUSE_DOM_CAPTURE_JS.contains("zIndexNumeric"));
        assert!(Z_INDEX_MISUSE_DOM_CAPTURE_JS.contains("isAbsurd"));
        assert!(Z_INDEX_MISUSE_DOM_CAPTURE_JS.contains("hasNoOpZIndex"));
        // ABSURD_THRESHOLD constant present.
        assert!(Z_INDEX_MISUSE_DOM_CAPTURE_JS.contains("1000000"));
        // Skip-list contract — `'auto'` and `'0'`.
        assert!(Z_INDEX_MISUSE_DOM_CAPTURE_JS.contains("'auto'"));
        assert!(Z_INDEX_MISUSE_DOM_CAPTURE_JS.contains("'0'"));
        // data-z-index-allow opt-out contract.
        assert!(Z_INDEX_MISUSE_DOM_CAPTURE_JS.contains("data-z-index-allow"));
    }
}
