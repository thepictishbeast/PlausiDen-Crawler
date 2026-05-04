//! `runtime_contrast` — WCAG 2.1 contrast detector.
//!
//! The eval string runs inside the page and walks every visible
//! text node, computing effective foreground vs effective
//! background and emitting offenders below WCAG AA thresholds:
//!
//! * body text  ≥ 4.5:1
//! * large text ≥ 3.0:1 (≥ 18pt = 24px, OR ≥ 14pt + bold = 18.66px)
//!
//! Effective background walks the parent chain stacking translucent
//! layers via the alpha-over operator. Bails (`null` background)
//! when any ancestor has a `background-image` (gradient or image)
//! we can't reduce to a single color — better to skip than to
//! report a false positive against an unrelated parent panel.
//!
//! BUG ASSUMPTION: the eval function is a string-eval (not a
//! function passed by reference to `page.evaluate`) so that named
//! function declarations (`__name` wrappers under tsx) don't break
//! at runtime. Same workaround pattern as `ui_overflow` /
//! `runtime_images` / `runtime_focus`. Don't switch to function-
//! reference form without verifying named-fn handling first.

use serde::{Deserialize, Serialize};

/// The page-side eval string. Runs in the page's window context;
/// returns a JSON-serialisable `RuntimeContrastSnapshot`.
///
/// **Source of truth for both Crawler runtimes** — the TypeScript
/// Crawler currently has a duplicate of this string in
/// `src/runtimeContrast.ts`. Phase 2 of the port replaces that
/// with a build-time generation step that imports from this crate.
///
/// REGRESSION-GUARD: comments containing backticks were a real
/// bug — backticks inside the prior TS template literal terminated
/// the string. The rule "keep comments backtick-free" is preserved
/// here because the same string lands in JS land regardless of how
/// it was authored.
pub const RUNTIME_CONTRAST_JS: &str = r##"(() => {
    const parseColor = function(c) {
      // Returns [r,g,b,a] in 0-255 / 0-1.
      const m = /^rgba?\(\s*([0-9.]+)\s*,\s*([0-9.]+)\s*,\s*([0-9.]+)\s*(?:,\s*([0-9.]+)\s*)?\)$/i.exec(c);
      if (!m) return null;
      return [parseFloat(m[1]), parseFloat(m[2]), parseFloat(m[3]), m[4] === undefined ? 1 : parseFloat(m[4])];
    };
    const sRgb = function(c) { c = c / 255; return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
    const luminance = function(rgba) { return 0.2126 * sRgb(rgba[0]) + 0.7152 * sRgb(rgba[1]) + 0.0722 * sRgb(rgba[2]); };
    const contrast = function(a, b) {
      const l1 = luminance(a), l2 = luminance(b);
      const lo = Math.min(l1, l2), hi = Math.max(l1, l2);
      return (hi + 0.05) / (lo + 0.05);
    };
    const blendOver = function(top, under) {
      // Composite top (rgba) over under (rgba) - alpha-over operator.
      const a = top[3];
      return [
        top[0] * a + under[0] * (1 - a),
        top[1] * a + under[1] * (1 - a),
        top[2] * a + under[2] * (1 - a),
        1
      ];
    };
    const effectiveBg = function(el) {
      let stack = [];
      let node = el;
      while (node && node.nodeType === 1) {
        const cs = window.getComputedStyle(node);
        if (cs.backgroundImage && cs.backgroundImage !== 'none') {
          return null;
        }
        const bg = parseColor(cs.backgroundColor);
        if (bg && bg[3] > 0) {
          stack.unshift(bg);
          if (bg[3] === 1) break;
        }
        node = node.parentElement;
      }
      let cur = [255, 255, 255, 1];
      for (const layer of stack) cur = blendOver(layer, cur);
      return cur;
    };
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

    let textNodesScanned = 0;
    let totalContrastPairs = 0;
    const failingOffenders = [];

    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null);
    let n;
    while ((n = walker.nextNode())) {
      const t = (n.nodeValue || '').trim();
      if (t.length < 2) continue;
      const parent = n.parentElement;
      if (!parent) continue;
      const cs = window.getComputedStyle(parent);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') continue;
      const rect = parent.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      textNodesScanned += 1;

      const fg = parseColor(cs.color);
      if (!fg || fg[3] === 0) continue;
      const bg = effectiveBg(parent);
      if (!bg) continue;

      const fgOpaque = bg ? blendOver(fg, bg) : fg;
      const ratio = contrast(fgOpaque, bg);
      const fontSizePx = parseFloat(cs.fontSize) || 16;
      const fontWeight = parseInt(cs.fontWeight, 10) || 400;
      const isLarge = fontSizePx >= 24 || (fontSizePx >= 18.66 && fontWeight >= 700);
      const required = isLarge ? 3 : 4.5;
      totalContrastPairs += 1;
      if (ratio < required) {
        failingOffenders.push({
          selector: selectorOf(parent),
          fg: cs.color,
          bg: 'rgb(' + Math.round(bg[0]) + ',' + Math.round(bg[1]) + ',' + Math.round(bg[2]) + ')',
          ratio: Math.round(ratio * 100) / 100,
          required: required,
          fontSizePx: Math.round(fontSizePx * 10) / 10,
          isLarge: isLarge,
          text: t.slice(0, 60)
        });
      }
    }

    return {
      vpW: window.innerWidth,
      vpH: window.innerHeight,
      textNodesScanned: textNodesScanned,
      totalContrastPairs: totalContrastPairs,
      failingOffenders: failingOffenders
    };
})()"##;

/// One contrast offender — a text node whose computed
/// foreground/background contrast falls below the WCAG AA
/// threshold for its font-size class.
///
/// BUG ASSUMPTION: `selector` is a best-effort path; not
/// guaranteed unique, may include `:nth-of-type` segments at
/// each level. Treat as a HUMAN-READABLE locator, not a
/// machine-stable id.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct ContrastOffender {
    /// Best-effort CSS selector path.
    pub selector: String,
    /// Computed-style `color` value, verbatim (e.g. `"rgb(0, 0, 0)"`).
    pub fg: String,
    /// Effective background after stacking translucent layers,
    /// formatted `"rgb(R,G,B)"`.
    pub bg: String,
    /// WCAG ratio rounded to 2 decimal places.
    pub ratio: f64,
    /// Minimum required for the text size class (4.5 normal, 3.0 large).
    pub required: f64,
    /// Font-size in CSS pixels.
    pub font_size_px: f64,
    /// True if WCAG-large (≥ 24px, OR ≥ 18.66px + weight ≥ 700).
    pub is_large: bool,
    /// First 60 characters of the offending text.
    pub text: String,
}

/// Return shape of the eval. The viewport is included so
/// findings are reproducible at the same render size.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct RuntimeContrastSnapshot {
    /// Viewport width in CSS px (`window.innerWidth`).
    #[serde(rename = "vpW")]
    pub vp_w: u32,
    /// Viewport height in CSS px.
    #[serde(rename = "vpH")]
    pub vp_h: u32,
    /// Number of text nodes the walker visited (including ones
    /// that ultimately weren't measured).
    pub text_nodes_scanned: u32,
    /// Number of (fg, bg) pairs successfully evaluated. The
    /// difference between `text_nodes_scanned` and this is the
    /// number of nodes skipped because of gradient bg / 0-size
    /// rect / transparent fg.
    pub total_contrast_pairs: u32,
    /// Offenders below their threshold.
    pub failing_offenders: Vec<ContrastOffender>,
}

/// Severity label a finding maps to. Keep aligned with the
/// `forge_core::Severity` shape the Forge side uses; once the
/// Phase 2 `crawler-report` crate lands, this re-uses that type
/// directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// AA-blocking. Fails the build gate.
    Strict,
    /// Below large-text threshold but not body-text. Advisory.
    Warn,
}

/// Human-friendly label string. Useful for terminal output.
///
/// BUG ASSUMPTION: `Severity` is `#[non_exhaustive]` for the
/// downstream-crate compatibility property, but within THIS crate
/// the match is exhaustive (no `_` arm — Rust would warn it
/// unreachable). When a future variant is added, this match
/// will rightly fail to compile until updated.
#[must_use]
pub fn severity_label(s: Severity) -> &'static str {
    match s {
        Severity::Strict => "STRICT",
        Severity::Warn => "warn",
    }
}

/// Map an offender to its severity per WCAG. Body text below
/// 4.5:1 is strict; large text below 3:1 is also strict; large
/// text between 3.0 and 4.5 is warn (it's intentionally large
/// so the lower threshold applies, but we surface as warn for
/// any further audit).
#[must_use]
pub fn classify(offender: &ContrastOffender) -> Severity {
    if offender.ratio < offender.required {
        // Anything below required = strict per WCAG AA.
        Severity::Strict
    } else {
        // Above required (shouldn't be in offenders list normally).
        Severity::Warn
    }
}

/// Apply detection rules to a contrast snapshot. Pure function.
/// Mirrors the TS `detectRuntimeContrastIssues` exactly: body-text
/// (NOT large) below 4.5:1 → strict; large-text below 3:1 → warn.
#[must_use]
pub fn detect_runtime_contrast_issues(snap: &RuntimeContrastSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::new();
    let strict_count = snap
        .failing_offenders
        .iter()
        .filter(|o| !o.is_large)
        .count();
    let warn_count = snap.failing_offenders.iter().filter(|o| o.is_large).count();
    if strict_count > 0 {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "contrast.body-text-below-aa".to_owned(),
            detail: format!("{strict_count} body-text element(s) below WCAG AA 4.5:1 contrast."),
        });
    }
    if warn_count > 0 {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "contrast.large-text-below-aa".to_owned(),
            detail: format!("{warn_count} large-text element(s) below WCAG AA 3:1 contrast."),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_constant_is_balanced() {
        // Sanity: equal counts of `(` and `)` in the eval string.
        // A mismatch would crash V8 immediately on inject.
        let opens = RUNTIME_CONTRAST_JS.matches('(').count();
        let closes = RUNTIME_CONTRAST_JS.matches(')').count();
        assert_eq!(opens, closes, "JS eval has unbalanced parens");
    }

    #[test]
    fn js_constant_is_iife() {
        assert!(RUNTIME_CONTRAST_JS.starts_with("(() => {"));
        assert!(RUNTIME_CONTRAST_JS.ends_with("})()"));
    }

    #[test]
    fn js_constant_returns_required_keys() {
        // The eval body returns an object with these keys; if any
        // are removed accidentally, deserialisation into
        // RuntimeContrastSnapshot will fail at runtime — fail
        // here at compile/test time instead.
        for key in [
            "vpW",
            "vpH",
            "textNodesScanned",
            "totalContrastPairs",
            "failingOffenders",
        ] {
            assert!(
                RUNTIME_CONTRAST_JS.contains(key),
                "JS eval missing expected return key: {key}"
            );
        }
    }

    #[test]
    fn snapshot_round_trips_through_json() {
        let snap = RuntimeContrastSnapshot {
            vp_w: 1280,
            vp_h: 800,
            text_nodes_scanned: 42,
            total_contrast_pairs: 30,
            failing_offenders: vec![ContrastOffender {
                selector: "body > h1".to_owned(),
                fg: "rgb(60, 60, 60)".to_owned(),
                bg: "rgb(255, 255, 255)".to_owned(),
                ratio: 4.20,
                required: 4.5,
                font_size_px: 16.0,
                is_large: false,
                text: "Hello world".to_owned(),
            }],
        };
        let json = serde_json::to_string(&snap).expect("serialise");
        let back: RuntimeContrastSnapshot = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back.vp_w, 1280);
        assert_eq!(back.failing_offenders.len(), 1);
        assert_eq!(back.failing_offenders[0].ratio, 4.20);
    }

    #[test]
    fn classify_strict_below_required() {
        let o = ContrastOffender {
            selector: "x".to_owned(),
            fg: "x".to_owned(),
            bg: "x".to_owned(),
            ratio: 3.5,
            required: 4.5,
            font_size_px: 16.0,
            is_large: false,
            text: "x".to_owned(),
        };
        assert_eq!(classify(&o), Severity::Strict);
    }

    #[test]
    fn severity_label_strict() {
        assert_eq!(severity_label(Severity::Strict), "STRICT");
        assert_eq!(severity_label(Severity::Warn), "warn");
    }
}
