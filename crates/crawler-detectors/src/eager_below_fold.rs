//! `eager_below_fold` — inverse of `lazy_above_fold`. Flags
//! `<img>` elements that are below the initial viewport AND
//! lack `loading="lazy"`. Wastes bandwidth fetching pixels the
//! user will never see; slows LCP because the browser
//! competes with eager below-fold images for connection slots.
//!
//! Findings:
//!
//!   * `eager_below_fold.unnecessary-eager` warn — `<img>` whose
//!     top edge is BELOW the initial viewport AND has no
//!     `loading="lazy"` attribute. Add the attribute or set
//!     `loading="lazy"` to defer the load.
//!
//! warn-only — runtime works, just slower + heavier. Companion
//! to `lazy_above_fold` (strict on the LCP-hurting inverse).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval — walks every `<img>` element, collects
/// `getBoundingClientRect().top` + the `loading` attribute, plus
/// the initial viewport height for the fold cutoff.
pub const EAGER_BELOW_FOLD_JS: &str = r##"(() => {
    const fold = window.innerHeight || 0;
    const imgs = [];
    const all = document.querySelectorAll('img');
    for (let i = 0; i < all.length; i++) {
        const el = all[i];
        const rect = el.getBoundingClientRect();
        imgs.push({
            top: Math.round(rect.top),
            loading: (el.getAttribute('loading') || '').toLowerCase(),
            src: el.getAttribute('src') || '',
            alt: (el.getAttribute('alt') || '').slice(0, 60),
        });
    }
    return { fold_y: fold, images: imgs };
})()"##;

/// One captured `<img>` element.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct ImageEntry {
    /// `getBoundingClientRect().top`, in CSS px. Negative when
    /// the element has scrolled off the top.
    pub top: i32,
    /// `loading` attribute value (lowercased). Empty if absent.
    pub loading: String,
    /// `src` attribute (raw URL).
    pub src: String,
    /// First ~60 chars of `alt` for diagnostic detail.
    pub alt: String,
}

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub struct EagerBelowFoldSnapshot {
    /// Initial viewport height — the cutoff below which images
    /// are treated as below-the-fold.
    pub fold_y: i32,
    /// All `<img>` elements on the page.
    pub images: Vec<ImageEntry>,
}

/// Pure detector.
#[must_use]
pub fn detect_eager_below_fold(
    snap: &EagerBelowFoldSnapshot,
) -> Vec<crate::AxisFinding> {
    let mut out = Vec::new();
    for img in &snap.images {
        // Below-fold: the image's top edge is strictly below the
        // initial viewport height.
        if img.top <= snap.fold_y {
            continue;
        }
        // Already lazy-loaded → fine.
        if img.loading == "lazy" {
            continue;
        }
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "eager_below_fold.unnecessary-eager".to_owned(),
            detail: format!(
                "<img src=\"{}\" alt=\"{}\"> is below the fold (top={}, fold={}) without loading=\"lazy\". Browser fetches eagerly, wasting bandwidth + competing with above-fold LCP for connection slots. Add loading=\"lazy\" to defer until the user scrolls near it.",
                img.src, img.alt, img.top, snap.fold_y
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(top: i32, loading: &str, src: &str) -> ImageEntry {
        ImageEntry {
            top,
            loading: loading.to_owned(),
            src: src.to_owned(),
            alt: String::new(),
        }
    }

    fn snap(fold: i32, images: Vec<ImageEntry>) -> EagerBelowFoldSnapshot {
        EagerBelowFoldSnapshot {
            fold_y: fold,
            images,
        }
    }

    #[test]
    fn below_fold_eager_flags() {
        let f = detect_eager_below_fold(&snap(800, vec![img(1200, "", "/a.jpg")]));
        assert!(f
            .iter()
            .any(|x| x.kind == "eager_below_fold.unnecessary-eager"));
    }

    #[test]
    fn below_fold_lazy_does_not_flag() {
        let f = detect_eager_below_fold(&snap(800, vec![img(1200, "lazy", "/a.jpg")]));
        assert!(f.is_empty());
    }

    #[test]
    fn above_fold_eager_does_not_flag() {
        let f = detect_eager_below_fold(&snap(800, vec![img(100, "", "/a.jpg")]));
        assert!(f.is_empty());
    }

    #[test]
    fn at_fold_boundary_does_not_flag() {
        // top == fold means the top edge is exactly at the
        // bottom of the viewport — at least 1px is visible,
        // treat as above-fold.
        let f = detect_eager_below_fold(&snap(800, vec![img(800, "", "/a.jpg")]));
        assert!(f.is_empty());
    }

    #[test]
    fn multiple_below_fold_emit_independent_findings() {
        let f = detect_eager_below_fold(&snap(
            800,
            vec![
                img(1000, "", "/a.jpg"),
                img(1500, "", "/b.jpg"),
                img(2000, "lazy", "/c.jpg"),
            ],
        ));
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn eager_attribute_value_still_flags() {
        // loading="eager" is the same as "no attribute" for the
        // purposes of bandwidth waste below the fold.
        let f = detect_eager_below_fold(&snap(800, vec![img(1200, "eager", "/a.jpg")]));
        assert!(f
            .iter()
            .any(|x| x.kind == "eager_below_fold.unnecessary-eager"));
    }

    #[test]
    fn no_images_no_findings() {
        let f = detect_eager_below_fold(&snap(800, vec![]));
        assert!(f.is_empty());
    }
}
