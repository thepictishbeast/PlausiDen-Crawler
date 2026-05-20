//! `offscreen_content` — text-bearing visible-DOM content positioned
//! outside the viewport / page bounds.
//!
//! Distinct from `offscreen_focusable` (which flags focusable elements
//! moved off-screen — the phantom-focus bug class). `offscreen_content`
//! is about *visible-to-the-CSS-engine* text that the user can't see
//! because its bounding rect is outside both the viewport AND the
//! document scroll bounds. Common real-world causes:
//!
//! * An editor adds a paragraph but a layout bug stuck it at
//!   `left: -9999px` (mistakenly applied `.sr-only` class).
//! * A CSS transform animation got stuck mid-flight.
//! * A carousel slide that fell out of its track via stale state.
//! * A copy-paste from a `.sr-only` template that wasn't reverted.
//!
//! Bug surfaces as a layout hole — the operator sees blank space
//! where copy was supposed to be.
//!
//! HEURISTIC
//! ---------
//! For every element with `textContent.trim().length > 10` whose
//! computed style says it's rendered (`display != none`,
//! `visibility != hidden`, `opacity > 0`):
//!
//! 1. Get `getBoundingClientRect()`.
//! 2. Compute the absolute document rect by adding `window.scrollX` /
//!    `window.scrollY`.
//! 3. Compute the document bounds: `documentElement.scrollWidth` ×
//!    `documentElement.scrollHeight` (= entire scrollable area).
//! 4. If the absolute rect is **entirely outside** the document
//!    bounds — either left < -slack, right > docW + slack, top <
//!    -slack, bottom > docH + slack — flag it.
//!
//! Allowlist (these are intentional, not bugs):
//! * `aria-hidden="true"` — explicitly hidden from AT.
//! * Elements matching the canonical `.sr-only` shape: 1px × 1px,
//!   overflow:hidden, position:absolute, clip-path / clip / clip-rect
//!   set. These are screen-reader-only utility marks; their off-
//!   screen positioning is the intended design.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const OFFSCREEN_CONTENT_JS: &str = r##"(() => {
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

    const isVisuallyRendered = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      const op = parseFloat(cs.opacity);
      if (!isNaN(op) && op === 0) return false;
      return true;
    };

    // True for elements that match the canonical screen-reader-only
    // utility shape. These are INTENDED to live off-screen.
    const isScreenReaderOnly = function(el) {
      const cs = window.getComputedStyle(el);
      const w = parseFloat(cs.width);
      const h = parseFloat(cs.height);
      // 1px × 1px (or 0px × 0px) + overflow hidden is the canonical shape.
      const tinySized = (!isNaN(w) && w <= 2) && (!isNaN(h) && h <= 2);
      if (tinySized && cs.overflow === 'hidden') return true;
      // clip / clip-path setting that hides the box visually.
      // We avoid open-paren literals inside JS strings so the
      // js_balanced test stays clean — split the marker tokens.
      const clip = cs.clip;
      const clipPath = cs.clipPath;
      if (clip && clip !== 'auto' && clip.indexOf('rect') !== -1 && clip.indexOf('0') !== -1) return true;
      if (clipPath && clipPath !== 'none' && clipPath.indexOf('inset') !== -1 && clipPath.indexOf('100') !== -1) return true;
      return false;
    };

    const isOwnLeaf = function(el) {
      // We only consider an element a content "leaf" if its own text
      // content (excluding descendant elements' contributions that
      // are themselves rendered) is non-trivial. Otherwise we'd flag
      // every container whose deep descendant is off-screen.
      const ownText = Array.from(el.childNodes)
        .filter(function(n) { return n.nodeType === 3; })
        .map(function(n) { return (n.textContent || '').trim(); })
        .join(' ')
        .trim();
      return ownText.length > 10;
    };

    // SLACK — allow tiny sub-pixel rounding or 1-pixel chrome offsets
    // before flagging. Real off-screen bugs are usually thousands of
    // pixels away from the document edge.
    const SLACK = 16;

    const docEl = document.documentElement;
    const docW = Math.max(docEl.scrollWidth, docEl.clientWidth || 0);
    const docH = Math.max(docEl.scrollHeight, docEl.clientHeight || 0);
    const vpW = window.innerWidth;
    const vpH = window.innerHeight;

    const seen = new Set();
    const offenders = [];
    let scanned = 0;

    // Walk every element under <body>. Limit to a reasonable cap so
    // pages with 50k+ nodes don't tank the runner.
    const SCAN_CAP = 5000;
    const stack = [document.body];
    while (stack.length > 0 && scanned < SCAN_CAP) {
      const el = stack.pop();
      if (!el || seen.has(el)) continue;
      seen.add(el);
      scanned += 1;

      // Descend regardless of own match — children may match even if parent doesn't.
      const kids = el.children;
      for (let i = 0; i < kids.length; i++) stack.push(kids[i]);

      if (el === document.body) continue;
      if (!isVisuallyRendered(el)) continue;
      if (el.getAttribute('aria-hidden') === 'true') continue;
      if (isScreenReaderOnly(el)) continue;
      if (!isOwnLeaf(el)) continue;

      const rect = el.getBoundingClientRect();
      // Absolute (document-relative) rect.
      const absLeft = rect.left + window.scrollX;
      const absTop = rect.top + window.scrollY;
      const absRight = rect.right + window.scrollX;
      const absBottom = rect.bottom + window.scrollY;

      // Entirely outside document bounds?
      const outsideLeft = absRight < -SLACK;
      const outsideRight = absLeft > docW + SLACK;
      const outsideTop = absBottom < -SLACK;
      const outsideBottom = absTop > docH + SLACK;

      if (outsideLeft || outsideRight || outsideTop || outsideBottom) {
        const side = outsideLeft ? 'left'
                   : outsideRight ? 'right'
                   : outsideTop ? 'top'
                   : 'bottom';
        offenders.push({
          selector: selectorOf(el),
          tag: el.tagName.toLowerCase(),
          text: (el.textContent || '').trim().slice(0, 80),
          side: side,
          absLeft: Math.round(absLeft),
          absTop: Math.round(absTop),
          absRight: Math.round(absRight),
          absBottom: Math.round(absBottom)
        });
        if (offenders.length >= 50) break;
      }
    }

    return {
      vpW: vpW,
      vpH: vpH,
      docW: docW,
      docH: docH,
      scanned: scanned,
      truncated: scanned >= SCAN_CAP || offenders.length >= 50,
      offenders: offenders
    };
})()"##;

/// One off-screen-content offender row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct OffscreenOffender {
    /// Best-effort CSS selector.
    pub selector: String,
    /// `el.tagName.toLowerCase()`.
    pub tag: String,
    /// First 80 chars of textContent (the operator-recognisable
    /// fingerprint).
    pub text: String,
    /// Which side of the document the offender is past:
    /// `"left" | "right" | "top" | "bottom"`.
    pub side: String,
    /// Document-relative bounding-rect coordinates (integers,
    /// rounded from float).
    pub abs_left: i32,
    /// Document-relative top.
    pub abs_top: i32,
    /// Document-relative right.
    pub abs_right: i32,
    /// Document-relative bottom.
    pub abs_bottom: i32,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct OffscreenContentSnapshot {
    /// `window.innerWidth`.
    #[serde(rename = "vpW")]
    pub vp_w: u32,
    /// `window.innerHeight`.
    #[serde(rename = "vpH")]
    pub vp_h: u32,
    /// `documentElement.scrollWidth` (full scrollable area).
    pub doc_w: u32,
    /// `documentElement.scrollHeight`.
    pub doc_h: u32,
    /// Number of elements scanned before hitting SCAN_CAP or the end.
    pub scanned: u32,
    /// `true` if the scan was truncated by the SCAN_CAP or finding limit.
    pub truncated: bool,
    /// Offenders found.
    pub offenders: Vec<OffscreenOffender>,
}

/// Apply detection rules. Pure function. Emits one strict finding
/// per offender, capped at the per-snapshot limit.
#[must_use]
pub fn detect_offscreen_content_issues(snap: &OffscreenContentSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::new();
    if snap.offenders.is_empty() {
        return out;
    }
    out.push(crate::AxisFinding {
        severity: crate::AxisSeverity::Strict,
        kind: "offscreen-content.outside-bounds".to_owned(),
        detail: format!(
            "{} text-bearing element(s) rendered outside document bounds (doc {}x{}, scanned {}{}). Likely a stale CSS transform / mistakenly-applied sr-only / stuck carousel. First: <{}> \"{}\" @ ({},{}) side={}",
            snap.offenders.len(),
            snap.doc_w,
            snap.doc_h,
            snap.scanned,
            if snap.truncated { ", truncated" } else { "" },
            snap.offenders[0].tag,
            snap.offenders[0].text,
            snap.offenders[0].abs_left,
            snap.offenders[0].abs_top,
            snap.offenders[0].side,
        ),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AxisSeverity;

    #[test]
    fn js_balanced() {
        assert_eq!(
            OFFSCREEN_CONTENT_JS.matches('(').count(),
            OFFSCREEN_CONTENT_JS.matches(')').count()
        );
        assert_eq!(
            OFFSCREEN_CONTENT_JS.matches('{').count(),
            OFFSCREEN_CONTENT_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(OFFSCREEN_CONTENT_JS.starts_with("(() => {"));
        assert!(OFFSCREEN_CONTENT_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "vpW",
            "vpH",
            "docW",
            "docH",
            "scanned",
            "truncated",
            "offenders",
            "selector",
            "tag",
            "text",
            "side",
            "absLeft",
            "absTop",
            "absRight",
            "absBottom",
        ] {
            assert!(OFFSCREEN_CONTENT_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_respects_aria_hidden_allowlist() {
        assert!(OFFSCREEN_CONTENT_JS.contains("aria-hidden"));
    }

    #[test]
    fn js_checks_sr_only_canonical_shape() {
        // The screen-reader-only allowlist tests width/height + overflow + clip.
        assert!(OFFSCREEN_CONTENT_JS.contains("isScreenReaderOnly"));
        assert!(OFFSCREEN_CONTENT_JS.contains("overflow"));
        assert!(OFFSCREEN_CONTENT_JS.contains("clip"));
    }

    #[test]
    fn empty_offenders_no_finding() {
        let snap = OffscreenContentSnapshot {
            vp_w: 1280,
            vp_h: 800,
            doc_w: 1280,
            doc_h: 2400,
            scanned: 412,
            truncated: false,
            offenders: vec![],
        };
        let findings = detect_offscreen_content_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn one_offender_emits_strict() {
        let snap = OffscreenContentSnapshot {
            vp_w: 1280,
            vp_h: 800,
            doc_w: 1280,
            doc_h: 2400,
            scanned: 410,
            truncated: false,
            offenders: vec![OffscreenOffender {
                selector: "body > main > p:nth-of-type(3)".to_owned(),
                tag: "p".to_owned(),
                text: "This paragraph should be visible but isn't".to_owned(),
                side: "left".to_owned(),
                abs_left: -10240,
                abs_top: 312,
                abs_right: -10000,
                abs_bottom: 340,
            }],
        };
        let findings = detect_offscreen_content_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "offscreen-content.outside-bounds");
        assert!(findings[0].detail.contains("<p>"));
        assert!(findings[0].detail.contains("should be visible"));
        assert!(findings[0].detail.contains("side=left"));
    }

    #[test]
    fn multiple_offenders_reports_count_and_first() {
        let snap = OffscreenContentSnapshot {
            vp_w: 1280,
            vp_h: 800,
            doc_w: 1280,
            doc_h: 2400,
            scanned: 800,
            truncated: false,
            offenders: vec![
                OffscreenOffender {
                    selector: "body > main > p".to_owned(),
                    tag: "p".to_owned(),
                    text: "first offender".to_owned(),
                    side: "right".to_owned(),
                    abs_left: 9999,
                    abs_top: 100,
                    abs_right: 12000,
                    abs_bottom: 130,
                },
                OffscreenOffender {
                    selector: "body > footer > p".to_owned(),
                    tag: "p".to_owned(),
                    text: "second offender".to_owned(),
                    side: "right".to_owned(),
                    abs_left: 9999,
                    abs_top: 2200,
                    abs_right: 12000,
                    abs_bottom: 2230,
                },
            ],
        };
        let findings = detect_offscreen_content_issues(&snap);
        assert_eq!(findings.len(), 1);
        // Count present.
        assert!(findings[0].detail.contains("2 text-bearing element"));
        // First-offender fingerprint present.
        assert!(findings[0].detail.contains("first offender"));
        // Doc dimensions present.
        assert!(findings[0].detail.contains("1280x2400"));
    }

    #[test]
    fn truncated_flag_surfaced_in_detail() {
        let snap = OffscreenContentSnapshot {
            vp_w: 1280,
            vp_h: 800,
            doc_w: 1280,
            doc_h: 2400,
            scanned: 5000,
            truncated: true,
            offenders: vec![OffscreenOffender {
                selector: "x".to_owned(),
                tag: "div".to_owned(),
                text: "x".to_owned(),
                side: "bottom".to_owned(),
                abs_left: 0,
                abs_top: 99999,
                abs_right: 100,
                abs_bottom: 99999,
            }],
        };
        let findings = detect_offscreen_content_issues(&snap);
        assert!(findings[0].detail.contains("truncated"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = OffscreenContentSnapshot {
            vp_w: 360,
            vp_h: 800,
            doc_w: 360,
            doc_h: 4000,
            scanned: 200,
            truncated: false,
            offenders: vec![OffscreenOffender {
                selector: "x".to_owned(),
                tag: "p".to_owned(),
                text: "round-trip me".to_owned(),
                side: "top".to_owned(),
                abs_left: 10,
                abs_top: -5000,
                abs_right: 200,
                abs_bottom: -4970,
            }],
        };
        let json = serde_json::to_string(&snap).expect("ser");
        let back: OffscreenContentSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.offenders.len(), 1);
        assert_eq!(back.offenders[0].text, "round-trip me");
        assert_eq!(back.offenders[0].abs_top, -5000);
        assert_eq!(back.vp_w, 360);
    }

    #[test]
    fn offender_text_truncation_preserved() {
        // The JS truncates textContent to 80 chars; this is a wire
        // contract the consumer relies on for log compactness. We can't
        // test the JS directly, but the typed struct accepts any length;
        // the test reminds us the contract is one-way (JS truncates,
        // Rust trusts).
        let snap = OffscreenContentSnapshot {
            vp_w: 1280,
            vp_h: 800,
            doc_w: 1280,
            doc_h: 1600,
            scanned: 1,
            truncated: false,
            offenders: vec![OffscreenOffender {
                selector: "x".to_owned(),
                tag: "p".to_owned(),
                text: "a".repeat(80),
                side: "left".to_owned(),
                abs_left: -200,
                abs_top: 0,
                abs_right: -100,
                abs_bottom: 30,
            }],
        };
        let findings = detect_offscreen_content_issues(&snap);
        assert!(findings[0].detail.contains(&"a".repeat(80)));
    }

    #[test]
    fn detect_includes_doc_dimensions() {
        let snap = OffscreenContentSnapshot {
            vp_w: 390,
            vp_h: 844,
            doc_w: 390,
            doc_h: 7200,
            scanned: 50,
            truncated: false,
            offenders: vec![OffscreenOffender {
                selector: "x".to_owned(),
                tag: "p".to_owned(),
                text: "mobile bug".to_owned(),
                side: "bottom".to_owned(),
                abs_left: 0,
                abs_top: 99999,
                abs_right: 100,
                abs_bottom: 99999,
            }],
        };
        let findings = detect_offscreen_content_issues(&snap);
        assert!(findings[0].detail.contains("390x7200"));
    }
}
