//! `gradient_text_clip` — flags text elements using the
//! `background-clip: text` gradient-fill technique and warns
//! when the rendered bounding box is too narrow to safely fit
//! the text without mid-character clipping.
//!
//! The technique:
//!
//! ```css
//! .heading {
//!   background: linear-gradient(135deg, #4338CA 0%, #E07A5F 100%);
//!   -webkit-background-clip: text;
//!   background-clip: text;
//!   color: transparent;          /* or -webkit-text-fill-color */
//! }
//! ```
//!
//! Real-world failure modes the detector catches:
//!
//! 1. **Mid-character clipping** — the gradient is clipped to the
//!    text shape, but if the container's `overflow: hidden` (or a
//!    too-narrow flex item) cuts the bounding box mid-letter, the
//!    last character renders as a partial glyph. Tier-1 SaaS sites
//!    ship this regularly when display fonts at 6rem+ outgrow a
//!    mobile container.
//!
//! 2. **Missing fill-color fallback** — browsers that don't support
//!    `background-clip: text` (older Firefox / niche viewers) render
//!    the element as `color: transparent` (invisible text) when the
//!    author forgot a `@supports not (background-clip: text)`
//!    fallback. Detector flags when the element uses the technique
//!    AND has no visible color cascade.
//!
//! 3. **Per super-society stack** (`feedback_super_society_tech_stack`):
//!    gradient-clipped marketing text is the prototypical "fragile +
//!    decoration over function" pattern. Every hit scores against
//!    `reliable` and `robust`.
//!
//! ## Heuristic
//!
//! For each element with `background-clip: text` or
//! `-webkit-background-clip: text` in its computed style:
//!
//! * `narrow_ratio = rect.width / (char_count * font_size * 0.55)`
//!   * `< 1.0` ⇒ container narrower than expected text width
//!     ⇒ `strict` (likely clipping).
//!   * `>= 1.0` and the element has no transparent-text-color
//!     declaration ⇒ `warn` (working today, will break on browsers
//!     without background-clip support).
//!   * Otherwise ⇒ `warn` (mere usage of the fragile technique).
//!
//! The 0.55 constant is an empirical average character width
//! relative to font-size for proportional Latin display fonts; it
//! over-estimates monospace and under-estimates condensed fonts,
//! both biases acceptable for a warn-level signal.
//!
//! ## Severity
//!
//! * `strict` — `narrow_ratio < 1.0` (probable mid-character clip).
//! * `warn` — usage without container-narrowness signal (the
//!   technique itself is fragile; surfaces in audits as a soft
//!   warn rather than a hard reject).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offender — a text element using the gradient-text
/// fill technique.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct GradientTextClipHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Visible text (capped at 80 chars) for context.
    pub text: String,
    /// Character count of the visible text.
    pub char_count: u32,
    /// Bounding-box width in CSS px.
    pub rect_width: u32,
    /// Computed font-size in CSS px.
    pub font_size: u32,
    /// `rect_width / (char_count * font_size * 0.55)` —
    /// see module docs. < 1.0 ⇒ likely clipping.
    pub narrow_ratio: f32,
    /// True iff the element's computed `color` is `transparent`
    /// (or rgba with alpha 0) AND it's relying on background-clip
    /// for the visible color.
    pub uses_transparent_color: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct GradientTextClipSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Every element that uses background-clip: text.
    pub hits: Vec<GradientTextClipHit>,
    /// Total elements walked.
    pub scanned_elements: u32,
}

/// Pure detector: snapshot → findings. Splits into one strict
/// finding (per-hit narrow_ratio < 1.0) and one warn finding
/// (per-hit usage without clipping risk). Examples capped at 5.
#[must_use]
pub fn detect_gradient_text_clip(snap: &GradientTextClipSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }

    let (likely_clipped, decorative_only): (Vec<&GradientTextClipHit>, Vec<&GradientTextClipHit>) =
        snap.hits.iter().partition(|h| h.narrow_ratio < 1.0);

    let mut out = Vec::new();
    if !likely_clipped.is_empty() {
        let examples: Vec<String> = likely_clipped
            .iter()
            .take(5)
            .map(|h| {
                format!(
                    "{} (\"{}\", {}c, {}px wide, ratio {:.2})",
                    h.selector, h.text, h.char_count, h.rect_width, h.narrow_ratio
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "gradient-text-clip.narrow".to_owned(),
            detail: format!(
                "{} element(s) with background-clip:text in a container narrower than the text needs at viewport {}px — likely mid-character clipping. Either widen the container, reduce font-size, or replace gradient text with solid color. Examples: {}",
                likely_clipped.len(),
                snap.viewport_width,
                examples.join("; ")
            ),
        });
    }

    if !decorative_only.is_empty() {
        let examples: Vec<String> = decorative_only
            .iter()
            .take(5)
            .map(|h| format!("{} (\"{}\")", h.selector, h.text))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "gradient-text-clip.usage".to_owned(),
            detail: format!(
                "{} element(s) use background-clip:text — fragile under browsers without support and a common source of mid-character clipping at narrow viewports. Consider a solid-color fallback (`@supports not (background-clip: text)`). Examples: {}",
                decorative_only.len(),
                examples.join("; ")
            ),
        });
    }

    out
}

/// Browser-side DOM-capture script. Pinned for the future
/// chromiumoxide path; mirror any change in this file's
/// `GradientTextClipHit` + snapshot fields.
pub const GRADIENT_TEXT_CLIP_DOM_CAPTURE_JS: &str = r#"
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
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return true;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return true;
      return false;
    };

    const usesBackgroundClipText = function(cs) {
      const bc = cs.backgroundClip || '';
      const wbc = cs.webkitBackgroundClip || '';
      return bc === 'text' || wbc === 'text';
    };

    const colorIsTransparent = function(cs) {
      const c = (cs.color || '').replace(/\s+/g, '');
      if (c === 'transparent') return true;
      const rgbaZeroAlpha = c.match(/^rgba?\(\s*\d+\s*,\s*\d+\s*,\s*\d+\s*,\s*0(?:\.0+)?\s*\)$/);
      if (rgbaZeroAlpha) return true;
      const fillColor = cs.webkitTextFillColor || '';
      if (fillColor === 'transparent') return true;
      return false;
    };

    const hits = [];
    let scannedElements = 0;
    const candidates = document.querySelectorAll('body *');
    for (let i = 0; i < candidates.length; i++) {
      const el = candidates[i];
      if (isHidden(el)) continue;
      const cs = window.getComputedStyle(el);
      if (!usesBackgroundClipText(cs)) continue;
      scannedElements += 1;
      const text = (el.textContent || '').trim();
      if (text.length === 0) continue;
      const rect = el.getBoundingClientRect();
      const fontSizePx = parseFloat(cs.fontSize) || 16;
      const expectedWidth = text.length * fontSizePx * 0.55;
      const narrowRatio = expectedWidth > 0 ? (rect.width / expectedWidth) : 1.0;
      hits.push({
        selector: selectorOf(el),
        text: text.slice(0, 80),
        charCount: text.length,
        rectWidth: Math.round(rect.width),
        fontSize: Math.round(fontSizePx),
        narrowRatio: Math.round(narrowRatio * 1000) / 1000,
        usesTransparentColor: colorIsTransparent(cs),
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedElements: scannedElements,
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        selector: &str,
        char_count: u32,
        rect_width: u32,
        font_size: u32,
    ) -> GradientTextClipHit {
        let expected = char_count as f32 * font_size as f32 * 0.55;
        GradientTextClipHit {
            selector: selector.into(),
            text: "sample".into(),
            char_count,
            rect_width,
            font_size,
            narrow_ratio: if expected > 0.0 {
                rect_width as f32 / expected
            } else {
                1.0
            },
            uses_transparent_color: true,
        }
    }

    #[test]
    fn empty_snapshot_produces_no_findings() {
        let snap = GradientTextClipSnapshot {
            page_url: "https://x".into(),
            viewport_width: 390,
            hits: vec![],
            scanned_elements: 0,
        };
        assert!(detect_gradient_text_clip(&snap).is_empty());
    }

    #[test]
    fn narrow_container_produces_strict_finding() {
        // 20 chars × 16px × 0.55 = 176px expected. Rect at 80px ⇒ ratio 0.45.
        let snap = GradientTextClipSnapshot {
            page_url: "https://x".into(),
            viewport_width: 390,
            hits: vec![hit("h1", 20, 80, 16)],
            scanned_elements: 1,
        };
        let findings = detect_gradient_text_clip(&snap);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "gradient-text-clip.narrow");
    }

    #[test]
    fn wide_container_produces_warn_finding() {
        // 10 chars × 16px × 0.55 = 88px. Rect at 600px ⇒ ratio 6.8.
        let snap = GradientTextClipSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits: vec![hit("span", 10, 600, 16)],
            scanned_elements: 1,
        };
        let findings = detect_gradient_text_clip(&snap);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "gradient-text-clip.usage");
    }

    #[test]
    fn mixed_hits_produce_two_findings() {
        let snap = GradientTextClipSnapshot {
            page_url: "https://x".into(),
            viewport_width: 390,
            hits: vec![hit("h1.narrow", 20, 80, 16), hit("h2.wide", 10, 600, 16)],
            scanned_elements: 2,
        };
        let findings = detect_gradient_text_clip(&snap);
        assert_eq!(findings.len(), 2);
        let strict = findings.iter().find(|f| f.severity == AxisSeverity::Strict);
        let warn = findings.iter().find(|f| f.severity == AxisSeverity::Warn);
        assert!(strict.is_some());
        assert!(warn.is_some());
    }

    #[test]
    fn js_capture_constant_is_non_empty() {
        assert!(GRADIENT_TEXT_CLIP_DOM_CAPTURE_JS.contains("backgroundClip"));
        assert!(GRADIENT_TEXT_CLIP_DOM_CAPTURE_JS.contains("narrowRatio"));
    }
}
