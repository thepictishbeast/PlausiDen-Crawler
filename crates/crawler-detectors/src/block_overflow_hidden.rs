//! `block_overflow_hidden` — flags elements with `overflow:
//! hidden` (or `overflow-y: hidden`) whose content overflows
//! in the BLOCK direction (vertical for LTR pages), silently
//! truncating text below the visible boundary.
//!
//! Companion to [`crate::ui_overflow`] which flags HORIZONTAL
//! overflow (text + container width mismatch). This axis
//! catches the opposite: vertical clipping where text just
//! disappears off the bottom of a fixed-height container.
//!
//! Defect class: a card / sidebar / preview pane is given a
//! `height: 200px; overflow: hidden;` to stay visually
//! consistent across the grid. Long content gets silently
//! truncated. Visual designers don't notice because the
//! bottom is below the fold; users with screen readers also
//! miss it (most screen readers respect `overflow: hidden`
//! and stop reading at the clip point).
//!
//! Skip cases:
//!
//! * `text-overflow: ellipsis` set — operator declared
//!   intentional truncation; visible ellipsis informs the
//!   user content was cut. Out of scope.
//! * Form controls (`<input>` / `<textarea>` / `<select>`) —
//!   their intrinsic overflow is by design.
//! * `data-overflow-allow="true"` opt-out for measured
//!   exceptions (e.g. a chart area where overflow:hidden
//!   prevents visual bleed but content is non-textual).
//!
//! Differences from related axes:
//!
//! * `cpl_collapse` — flags too-narrow text columns
//!   (horizontal). Different axis: container shape.
//! * `ui_overflow` — flags content that overflows
//!   HORIZONTALLY producing scroll. Different direction.
//! * `text_wrap_collapse` — flags single-line nowrap
//!   overflow. Different mechanism.
//!
//! ## Severity
//!
//! * **Strict** — `clipped_text_count > 200` chars.
//!   Substantial content silently lost — readers + screen
//!   readers will miss meaningful text.
//! * **Warn** — `clipped_text_count > 0`. Any vertical clip
//!   without ellipsis. May be intentional (operator chose
//!   the height) but worth surfacing.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending container.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct BlockOverflowHiddenHit {
    /// CSS-ish path of the offending container.
    pub selector: String,
    /// First 60 chars of the container's visible text content.
    pub text_sample: String,
    /// `scrollHeight` in CSS px — total content height.
    pub scroll_height_px: u32,
    /// `clientHeight` in CSS px — visible content height.
    pub client_height_px: u32,
    /// `overflow-y` computed value (`"hidden"`, `"clip"`).
    pub overflow_y_value: String,
    /// Approximate visible character count (textContent length
    /// proportional to client_height / scroll_height).
    pub visible_text_count: u32,
    /// Approximate clipped character count
    /// (textContent length - visible).
    pub clipped_text_count: u32,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct BlockOverflowHiddenSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Offending containers (pre-filtered to those with
    /// non-zero clipped text count).
    pub hits: Vec<BlockOverflowHiddenHit>,
    /// Total elements walked.
    pub scanned_elements: u32,
}

/// Clipped-character count above which the finding is Strict.
pub const STRICT_CLIP_CHARS: u32 = 200;

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_block_overflow_hidden(snap: &BlockOverflowHiddenSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut strict: Vec<&BlockOverflowHiddenHit> = Vec::new();
    let mut warn: Vec<&BlockOverflowHiddenHit> = Vec::new();
    for h in &snap.hits {
        if h.clipped_text_count == 0 {
            continue;
        }
        if h.clipped_text_count > STRICT_CLIP_CHARS {
            strict.push(h);
        } else {
            warn.push(h);
        }
    }

    let format_example = |h: &BlockOverflowHiddenHit| -> String {
        let sample = if h.text_sample.is_empty() {
            String::new()
        } else {
            format!(" \"{}…\"", h.text_sample)
        };
        format!(
            "{}{} ({}px visible / {}px content · ~{} chars clipped)",
            h.selector, sample, h.client_height_px, h.scroll_height_px, h.clipped_text_count
        )
    };

    let mut out = Vec::new();
    if !strict.is_empty() {
        let examples: Vec<String> = strict
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "block-overflow-hidden.substantial-clip".to_owned(),
            detail: format!(
                "{} container(s) silently clip > {} chars of body content via `overflow: hidden` with no `text-overflow: ellipsis`. Substantial content invisible to readers + screen-reader users. Either remove the fixed height, switch to `text-overflow: ellipsis` (which signals truncation visually), or restructure the content. Examples: {}",
                strict.len(),
                STRICT_CLIP_CHARS,
                examples.join("; ")
            ),
        });
    }
    if !warn.is_empty() {
        let examples: Vec<String> = warn
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "block-overflow-hidden.minor-clip".to_owned(),
            detail: format!(
                "{} container(s) clip some body content vertically. May be intentional; surface for audit. Examples: {}",
                warn.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks visible elements
/// with `overflow-y` set to `hidden` / `clip` AND `scrollHeight
/// > clientHeight + 4`. Skips form controls + elements with
/// `text-overflow: ellipsis` set + opt-out attribute.
pub const BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS: &str = r#"
(() => {
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

    const SKIP_TAGS = new Set(['input', 'textarea', 'select', 'option', 'pre', 'code', 'kbd']);

    const hits = [];
    let scanned = 0;
    const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT, null);
    let node = walk.currentNode;
    while (node) {
      if (node.nodeType === 1) {
        const tag = node.tagName ? node.tagName.toLowerCase() : '';
        if (SKIP_TAGS.has(tag)) {
          node = walk.nextNode();
          continue;
        }
        if (node.getAttribute && node.getAttribute('data-overflow-allow') === 'true') {
          node = walk.nextNode();
          continue;
        }
        scanned += 1;
        const cs = window.getComputedStyle(node);
        // text-overflow: ellipsis → operator declared
        // intentional truncation. Out of scope.
        if ((cs.textOverflow || '').trim() === 'ellipsis') {
          node = walk.nextNode();
          continue;
        }
        const overflowY = (cs.overflowY || cs.overflow || 'visible').trim();
        if (overflowY !== 'hidden' && overflowY !== 'clip') {
          node = walk.nextNode();
          continue;
        }
        const scrollH = node.scrollHeight;
        const clientH = node.clientHeight;
        // +4 fuzz for sub-pixel rounding.
        if (scrollH <= clientH + 4) {
          node = walk.nextNode();
          continue;
        }
        const text = (node.textContent || '').trim();
        if (text.length === 0) {
          node = walk.nextNode();
          continue;
        }
        // Approximate visible char count by scaling text length
        // proportionally to client/scroll height.
        const visibleRatio = clientH / Math.max(scrollH, 1);
        const visibleChars = Math.round(text.length * visibleRatio);
        const clippedChars = text.length - visibleChars;
        if (clippedChars <= 0) {
          node = walk.nextNode();
          continue;
        }
        hits.push({
          selector: selectorOf(node),
          textSample: text.substring(0, 60),
          scrollHeightPx: scrollH,
          clientHeightPx: clientH,
          overflowYValue: overflowY,
          visibleTextCount: visibleChars,
          clippedTextCount: clippedChars
        });
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
        text_sample: &str,
        scroll_h: u32,
        client_h: u32,
        visible: u32,
        clipped: u32,
    ) -> BlockOverflowHiddenHit {
        BlockOverflowHiddenHit {
            selector: selector.into(),
            text_sample: text_sample.into(),
            scroll_height_px: scroll_h,
            client_height_px: client_h,
            overflow_y_value: "hidden".into(),
            visible_text_count: visible,
            clipped_text_count: clipped,
        }
    }

    fn snap(hits: Vec<BlockOverflowHiddenHit>) -> BlockOverflowHiddenSnapshot {
        BlockOverflowHiddenSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_elements: 100,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_block_overflow_hidden(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn zero_clipped_chars_skipped() {
        // hit with clipped=0 should be skipped entirely.
        let s = snap(vec![hit(".x", "Body", 100, 100, 100, 0)]);
        let findings = detect_block_overflow_hidden(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn substantial_clip_is_strict() {
        let s = snap(vec![hit(
            ".card",
            "Long body content that exceeds the fixed",
            600,
            200,
            150,
            450,
        )]);
        let findings = detect_block_overflow_hidden(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(
            findings[0].kind,
            "block-overflow-hidden.substantial-clip"
        );
        assert!(findings[0].detail.contains(".card"));
        assert!(findings[0].detail.contains("450 chars clipped"));
    }

    #[test]
    fn minor_clip_is_warn() {
        let s = snap(vec![hit(".sidebar", "Quick note", 220, 200, 195, 25)]);
        let findings = detect_block_overflow_hidden(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "block-overflow-hidden.minor-clip");
    }

    #[test]
    fn mixed_severity_emits_two_findings() {
        let s = snap(vec![
            hit(".big", "X", 600, 200, 150, 450),
            hit(".small", "Y", 220, 200, 195, 25),
        ]);
        let findings = detect_block_overflow_hidden(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"block-overflow-hidden.substantial-clip"));
        assert!(kinds.contains(&"block-overflow-hidden.minor-clip"));
    }

    #[test]
    fn text_sample_appears_in_example_when_present() {
        let s = snap(vec![hit(
            ".x",
            "Sample text",
            500,
            200,
            100,
            300,
        )]);
        let findings = detect_block_overflow_hidden(&s);
        assert!(findings[0].detail.contains("Sample text"));
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!(".c-{i}"),
                "X",
                500,
                200,
                100,
                300,
            ));
        }
        let s = snap(hits);
        let findings = detect_block_overflow_hidden(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 container(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("hits"));
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("scannedElements"));
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("scrollHeightPx"));
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("clientHeightPx"));
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("overflowYValue"));
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("clippedTextCount"));
        // Overflow values handled.
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("'hidden'"));
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("'clip'"));
        // Skip-list contracts.
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("'input'"));
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("'textarea'"));
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("'ellipsis'"));
        assert!(BLOCK_OVERFLOW_HIDDEN_DOM_CAPTURE_JS.contains("data-overflow-allow"));
    }
}
