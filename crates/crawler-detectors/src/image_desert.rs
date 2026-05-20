//! `image_desert` — flags pages with dense text and almost no
//! visual texture (images / icons / illustrations / SVG glyphs).
//!
//! The pathology: long-form pages without any visual anchor read
//! as walls of monospace gray. Sighted users skim past; bounce
//! rate climbs. The signal that catches it is purely statistical:
//! ratio of "image carriers" to "content sections."
//!
//! Forge has a build-time aesthetic_distinctiveness phase covering
//! the same concept, but it only runs on Forge-authored cms/*.json.
//! This runtime detector catches the same shape on third-party
//! sites the Crawler audits.
//!
//! ## Heuristic
//!
//! - Count "image carriers" in the live DOM: `<img>` (visible,
//!   non-empty src), `<picture>` (with any `<source>` child),
//!   `<svg>` (with non-empty viewBox AND visible bbox), and
//!   elements whose computed `background-image` is non-`none`.
//! - Count "content sections": top-level children of `<main>`, OR
//!   `<article>` / `<section>` elements with at least 200 chars of
//!   visible text.
//! - `desert_ratio = image_carriers / max(1, content_sections)`.
//!
//! Severity:
//!
//! - `strict` — `content_sections >= 5` AND `desert_ratio < 0.2`
//!   AND total page text > 2000 chars (mostly-prose page with
//!   essentially no images).
//! - `warn` — `content_sections >= 3` AND `desert_ratio < 0.5`
//!   AND total page text > 1000 chars (sparse but not extreme).
//!
//! AVP-2: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Captured page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ImageDesertSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture (CSS px).
    pub viewport_width: u32,
    /// Count of visible `<img>` elements.
    pub img_count: u32,
    /// Count of `<picture>` elements with `<source>` children.
    pub picture_count: u32,
    /// Count of visible `<svg>` elements (with viewBox + bbox).
    pub svg_count: u32,
    /// Count of elements with a non-`none` `background-image`.
    pub bg_image_count: u32,
    /// Count of content-bearing sections (per heuristic).
    pub content_section_count: u32,
    /// Total visible text length (chars).
    pub total_text_chars: u32,
}

impl ImageDesertSnapshot {
    /// Sum of all image-carrier counts.
    pub fn image_carriers(&self) -> u32 {
        self.img_count + self.picture_count + self.svg_count + self.bg_image_count
    }

    /// `image_carriers / max(1, content_section_count)`.
    pub fn desert_ratio(&self) -> f32 {
        self.image_carriers() as f32 / self.content_section_count.max(1) as f32
    }
}

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_image_desert(snap: &ImageDesertSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    let ratio = snap.desert_ratio();
    let carriers = snap.image_carriers();

    if snap.content_section_count >= 5 && ratio < 0.2 && snap.total_text_chars > 2000 {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "image-desert.severe".to_owned(),
            detail: format!(
                "{} content section(s), {} char(s) of text, {} image carrier(s) total — desert ratio {:.2}. Page reads as a wall of text. Consider adding a hero image, illustrative figure per section, or icon row to anchor each section visually.",
                snap.content_section_count, snap.total_text_chars, carriers, ratio
            ),
        });
    } else if snap.content_section_count >= 3 && ratio < 0.5 && snap.total_text_chars > 1000 {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "image-desert.sparse".to_owned(),
            detail: format!(
                "{} content section(s), {} char(s) of text, {} image carrier(s) — desert ratio {:.2}. Sparse visual texture; consider an illustrative figure or icon row in 1-2 sections.",
                snap.content_section_count, snap.total_text_chars, carriers, ratio
            ),
        });
    }

    out
}

/// Browser-side capture script.
pub const IMAGE_DESERT_DOM_CAPTURE_JS: &str = r#"
(() => {
    const isVisible = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return false;
      const rect = el.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0;
    };

    // 1. <img> with non-empty src.
    let imgCount = 0;
    const imgs = document.querySelectorAll('img');
    for (let i = 0; i < imgs.length; i++) {
      if (!isVisible(imgs[i])) continue;
      const src = imgs[i].getAttribute('src') || imgs[i].getAttribute('srcset') || '';
      if (src.length > 0) imgCount += 1;
    }

    // 2. <picture> with <source> children.
    let pictureCount = 0;
    const pictures = document.querySelectorAll('picture');
    for (let i = 0; i < pictures.length; i++) {
      if (!isVisible(pictures[i])) continue;
      if (pictures[i].querySelector('source')) pictureCount += 1;
    }

    // 3. <svg> with viewBox + non-zero bbox.
    let svgCount = 0;
    const svgs = document.querySelectorAll('svg');
    for (let i = 0; i < svgs.length; i++) {
      if (!isVisible(svgs[i])) continue;
      if (svgs[i].getAttribute('viewBox')) svgCount += 1;
    }

    // 4. background-image: not 'none'.
    let bgImageCount = 0;
    const all = document.querySelectorAll('body *');
    for (let i = 0; i < all.length; i++) {
      if (!isVisible(all[i])) continue;
      const bg = window.getComputedStyle(all[i]).backgroundImage || 'none';
      if (bg !== 'none' && bg.indexOf('url(') !== -1) bgImageCount += 1;
    }

    // 5. Content sections. Prefer <main> children, fall back to
    //    <article>/<section>/<div role="region"> with >=200 char text.
    let contentSectionCount = 0;
    const main = document.querySelector('main');
    if (main) {
      for (let i = 0; i < main.children.length; i++) {
        const child = main.children[i];
        if (!isVisible(child)) continue;
        const text = (child.textContent || '').trim();
        if (text.length >= 200) contentSectionCount += 1;
      }
    } else {
      const sections = document.querySelectorAll('article, section, [role="region"]');
      for (let i = 0; i < sections.length; i++) {
        if (!isVisible(sections[i])) continue;
        const text = (sections[i].textContent || '').trim();
        if (text.length >= 200) contentSectionCount += 1;
      }
    }

    const totalTextChars = (document.body.textContent || '').trim().length;

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      imgCount: imgCount,
      pictureCount: pictureCount,
      svgCount: svgCount,
      bgImageCount: bgImageCount,
      contentSectionCount: contentSectionCount,
      totalTextChars: totalTextChars,
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        img: u32,
        pic: u32,
        svg: u32,
        bg: u32,
        sections: u32,
        chars: u32,
    ) -> ImageDesertSnapshot {
        ImageDesertSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            img_count: img,
            picture_count: pic,
            svg_count: svg,
            bg_image_count: bg,
            content_section_count: sections,
            total_text_chars: chars,
        }
    }

    #[test]
    fn well_illustrated_page_produces_no_findings() {
        // 6 sections, 8 carriers, 5000 chars → ratio 1.33, fine.
        assert!(detect_image_desert(&snap(6, 1, 1, 0, 6, 5000)).is_empty());
    }

    #[test]
    fn severe_desert_produces_strict() {
        // 7 sections, 1 carrier, 4000 chars → ratio 0.14, strict.
        let findings = detect_image_desert(&snap(1, 0, 0, 0, 7, 4000));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "image-desert.severe");
    }

    #[test]
    fn sparse_desert_produces_warn() {
        // 4 sections, 1 carrier, 1500 chars → ratio 0.25, > 0.2 but < 0.5,
        // 4 >= 3 but < 5, > 1000 chars.
        let findings = detect_image_desert(&snap(1, 0, 0, 0, 4, 1500));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "image-desert.sparse");
    }

    #[test]
    fn small_pages_under_threshold_do_not_fire() {
        // Only 2 sections — below either threshold.
        assert!(detect_image_desert(&snap(0, 0, 0, 0, 2, 800)).is_empty());
        // 3 sections, 600 chars — text too short.
        assert!(detect_image_desert(&snap(0, 0, 0, 0, 3, 600)).is_empty());
    }

    #[test]
    fn image_carriers_sums_all_kinds() {
        assert_eq!(snap(3, 2, 1, 4, 0, 0).image_carriers(), 10);
    }

    #[test]
    fn desert_ratio_zero_sections_safe() {
        // Should not panic on division.
        let s = snap(2, 0, 0, 0, 0, 0);
        assert_eq!(s.desert_ratio(), 2.0);
    }

    #[test]
    fn js_capture_constant_is_sensible() {
        assert!(IMAGE_DESERT_DOM_CAPTURE_JS.contains("contentSectionCount"));
        assert!(IMAGE_DESERT_DOM_CAPTURE_JS.contains("backgroundImage"));
    }
}
