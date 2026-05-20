//! `placeholder_alt` — flags `<img alt="…">` set to useless placeholder values.
//!
//! Distinct from `runtime_images` (which flags MISSING alt attribute):
//! this detector flags alt that's syntactically present but
//! semantically empty. Screen readers announce the placeholder
//! literally — "image image" or "alt 1920x1080" — so the user gets
//! noise without information.
//!
//! Common placeholder shapes flagged:
//!   * Whitespace-only (`alt=" "` / `alt="\t"` / `alt="\n"`)
//!   * Generic words: "image", "picture", "photo", "photograph",
//!     "icon", "img", "graphic", "illustration"
//!   * Filename / extension: contains `.jpg` / `.png` / `.jpeg` /
//!     `.gif` / `.svg` / `.webp` / `.avif`
//!   * Dimension string: matches `<int>x<int>` or `<int>×<int>`
//!   * File-path: contains `/` (also covers absolute paths)
//!   * URL: starts with `http://` / `https://` / `data:`
//!
//! `alt=""` is INTENTIONAL — that's the spec-correct decorative-image
//! marker. We do NOT flag empty alt.
//!
//! Severity: warn. These read as filler but the page still functions;
//! they're a copy-quality issue, not a gate-blocking one. Strict
//! mode would promote them, but v1 ships warn-only.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const PLACEHOLDER_ALT_JS: &str = r##"(() => {
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

    const isVisible = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      const op = parseFloat(cs.opacity);
      if (!isNaN(op) && op === 0) return false;
      return true;
    };

    // Generic-word allowlist (lowercased compare).
    const GENERIC_WORDS = [
      'image', 'picture', 'photo', 'photograph', 'icon', 'img',
      'graphic', 'illustration', 'thumbnail', 'logo'
    ];

    // Image-extension allowlist (lowercased compare against suffix).
    const IMAGE_EXTENSIONS = [
      '.jpg', '.jpeg', '.png', '.gif', '.svg', '.webp', '.avif', '.bmp', '.tiff'
    ];

    const classify = function(altRaw) {
      const trimmed = altRaw.trim();
      if (trimmed === '') {
        // alt="" is intentional decorative marker; do NOT flag.
        // Whitespace-only alt that's NON-empty before trim is a bug.
        if (altRaw.length > 0) return 'whitespace';
        return '';
      }
      const lower = trimmed.toLowerCase();
      // URL / data URL.
      if (lower.startsWith('http://') || lower.startsWith('https://')
          || lower.startsWith('data:')) return 'url';
      // File path: contains `/` and looks like a path.
      if (trimmed.indexOf('/') !== -1 && trimmed.length < 200) {
        return 'file-path';
      }
      // Image extension anywhere.
      for (let i = 0; i < IMAGE_EXTENSIONS.length; i++) {
        if (lower.indexOf(IMAGE_EXTENSIONS[i]) !== -1) return 'filename';
      }
      // Dimension string. Match <digits>(x|×)<digits>.
      if (/^\d+\s*[x×]\s*\d+$/i.test(trimmed)) return 'dimension';
      // Single generic word (or just two words like "stock image").
      const words = lower.split(/\s+/).filter(function(w) { return w.length > 0; });
      if (words.length <= 3) {
        // If every word is a generic placeholder, flag it.
        let allGeneric = true;
        for (let i = 0; i < words.length; i++) {
          if (GENERIC_WORDS.indexOf(words[i]) === -1) {
            allGeneric = false;
            break;
          }
        }
        if (allGeneric && words.length >= 1) return 'generic-word';
      }
      return '';
    };

    let scanned = 0;
    const offenders = [];
    const imgs = document.querySelectorAll('img[alt]');
    for (let i = 0; i < imgs.length; i++) {
      const el = imgs[i];
      scanned += 1;
      if (!isVisible(el)) continue;
      const alt = el.getAttribute('alt');
      if (alt === null) continue;
      const kind = classify(alt);
      if (kind === '') continue;
      offenders.push({
        selector: selectorOf(el),
        src: el.getAttribute('src') || '',
        alt: alt.slice(0, 120),
        kind: kind
      });
      if (offenders.length >= 50) break;
    }

    return {
      scanned: scanned,
      offenderCount: offenders.length,
      offenders: offenders
    };
})()"##;

/// One placeholder-alt offender row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct PlaceholderAltOffender {
    /// CSS selector.
    pub selector: String,
    /// `src` attribute (may be empty / data URL).
    pub src: String,
    /// Raw alt text (truncated to 120 chars).
    pub alt: String,
    /// Category: `whitespace` / `url` / `file-path` / `filename` /
    /// `dimension` / `generic-word`.
    pub kind: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct PlaceholderAltSnapshot {
    /// Total `<img alt=…>` elements walked.
    pub scanned: u32,
    /// Number of offenders (may be capped at 50; count is honest).
    pub offender_count: u32,
    /// Per-offender row, capped at 50.
    pub offenders: Vec<PlaceholderAltOffender>,
}

/// Apply detection rules. Pure function.
///
/// Emits one `warn` finding per snapshot that contained any offender.
/// Surface includes per-category breakdown so the operator sees
/// which kind of placeholder dominates.
#[must_use]
pub fn detect_placeholder_alt_issues(snap: &PlaceholderAltSnapshot) -> Vec<crate::AxisFinding> {
    if snap.offenders.is_empty() {
        return Vec::new();
    }
    // Bucket offenders by kind.
    let mut by_kind: std::collections::BTreeMap<String, u32> =
        std::collections::BTreeMap::new();
    for o in &snap.offenders {
        *by_kind.entry(o.kind.clone()).or_insert(0) += 1;
    }
    let breakdown: Vec<String> = by_kind
        .iter()
        .map(|(k, n)| format!("{k}={n}"))
        .collect();
    let first = &snap.offenders[0];
    let mut out = Vec::with_capacity(1);
    out.push(crate::AxisFinding {
        severity: crate::AxisSeverity::Warn,
        kind: "placeholder-alt.useless-value".to_owned(),
        detail: format!(
            "{} <img> element(s) have alt= set to a useless placeholder value (categories: {}). First: alt=\"{}\" kind={} src={} @ {}. Replace with a concrete description of the image content, or set `alt=\"\"` (empty) if the image is purely decorative.",
            snap.offender_count,
            breakdown.join(", "),
            first.alt,
            first.kind,
            first.src,
            first.selector,
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
            PLACEHOLDER_ALT_JS.matches('(').count(),
            PLACEHOLDER_ALT_JS.matches(')').count()
        );
        assert_eq!(
            PLACEHOLDER_ALT_JS.matches('{').count(),
            PLACEHOLDER_ALT_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(PLACEHOLDER_ALT_JS.starts_with("(() => {"));
        assert!(PLACEHOLDER_ALT_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in ["scanned", "offenderCount", "offenders", "selector", "src", "alt", "kind"] {
            assert!(PLACEHOLDER_ALT_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_lists_all_categories() {
        for cat in ["whitespace", "url", "file-path", "filename", "dimension", "generic-word"] {
            assert!(
                PLACEHOLDER_ALT_JS.contains(&format!("'{cat}'")),
                "missing category in JS: {cat}"
            );
        }
    }

    #[test]
    fn js_lists_image_extensions() {
        for ext in [".jpg", ".jpeg", ".png", ".gif", ".svg", ".webp", ".avif"] {
            assert!(
                PLACEHOLDER_ALT_JS.contains(&format!("'{ext}'")),
                "missing extension in JS: {ext}"
            );
        }
    }

    #[test]
    fn js_lists_generic_words() {
        for word in ["image", "picture", "photo", "icon", "img"] {
            assert!(
                PLACEHOLDER_ALT_JS.contains(&format!("'{word}'")),
                "missing generic word: {word}"
            );
        }
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = PlaceholderAltSnapshot {
            scanned: 4,
            offender_count: 0,
            offenders: vec![],
        };
        let findings = detect_placeholder_alt_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn one_offender_emits_warn() {
        let snap = PlaceholderAltSnapshot {
            scanned: 2,
            offender_count: 1,
            offenders: vec![PlaceholderAltOffender {
                selector: "body > img".to_owned(),
                src: "/assets/hero.jpg".to_owned(),
                alt: "image".to_owned(),
                kind: "generic-word".to_owned(),
            }],
        };
        let findings = detect_placeholder_alt_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Warn));
        assert_eq!(findings[0].kind, "placeholder-alt.useless-value");
        assert!(findings[0].detail.contains(r#"alt="image""#));
        assert!(findings[0].detail.contains("kind=generic-word"));
        assert!(findings[0].detail.contains("generic-word=1"));
    }

    #[test]
    fn multiple_offenders_buckets_by_kind() {
        let snap = PlaceholderAltSnapshot {
            scanned: 5,
            offender_count: 4,
            offenders: vec![
                PlaceholderAltOffender {
                    selector: "x".to_owned(),
                    src: "y".to_owned(),
                    alt: "hero.png".to_owned(),
                    kind: "filename".to_owned(),
                },
                PlaceholderAltOffender {
                    selector: "x".to_owned(),
                    src: "y".to_owned(),
                    alt: "thumbnail.jpg".to_owned(),
                    kind: "filename".to_owned(),
                },
                PlaceholderAltOffender {
                    selector: "x".to_owned(),
                    src: "y".to_owned(),
                    alt: "300x200".to_owned(),
                    kind: "dimension".to_owned(),
                },
                PlaceholderAltOffender {
                    selector: "x".to_owned(),
                    src: "y".to_owned(),
                    alt: "image".to_owned(),
                    kind: "generic-word".to_owned(),
                },
            ],
        };
        let findings = detect_placeholder_alt_issues(&snap);
        let detail = &findings[0].detail;
        assert!(detail.contains("filename=2"));
        assert!(detail.contains("dimension=1"));
        assert!(detail.contains("generic-word=1"));
    }

    #[test]
    fn detail_recommends_empty_alt_for_decorative() {
        let snap = PlaceholderAltSnapshot {
            scanned: 1,
            offender_count: 1,
            offenders: vec![PlaceholderAltOffender {
                selector: "x".to_owned(),
                src: "y".to_owned(),
                alt: "img".to_owned(),
                kind: "generic-word".to_owned(),
            }],
        };
        let findings = detect_placeholder_alt_issues(&snap);
        assert!(findings[0].detail.contains(r#"`alt=""`"#));
        assert!(findings[0].detail.contains("decorative"));
    }

    #[test]
    fn truncated_count_matches_offender_count_field() {
        // When JS truncates at 50 but counts higher, offender_count
        // is what should display, not offenders.len().
        let snap = PlaceholderAltSnapshot {
            scanned: 200,
            offender_count: 73,
            offenders: vec![PlaceholderAltOffender {
                selector: "x".to_owned(),
                src: "y".to_owned(),
                alt: "image".to_owned(),
                kind: "generic-word".to_owned(),
            }],
        };
        let findings = detect_placeholder_alt_issues(&snap);
        assert!(findings[0].detail.contains("73 <img>"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = PlaceholderAltSnapshot {
            scanned: 3,
            offender_count: 1,
            offenders: vec![PlaceholderAltOffender {
                selector: "body > img".to_owned(),
                src: "/x.png".to_owned(),
                alt: "rt".to_owned(),
                kind: "filename".to_owned(),
            }],
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"scanned\":3"));
        assert!(json.contains("\"offenderCount\":1"));
        let back: PlaceholderAltSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.offenders.len(), 1);
        assert_eq!(back.offenders[0].kind, "filename");
    }
}
