//! `pixel_diff` — visual-regression detector via per-pixel
//! comparison.
//!
//! BackstopJS-equivalent in pure Rust. The detector itself is
//! pure: takes two PNG buffers (baseline + current screenshot)
//! and emits diff metrics. The runner integration (baseline
//! storage keyed by url+viewport+theme, screenshot capture)
//! lands in `crawler-runner` when wired into the journey
//! pipeline — separate iteration.
//!
//! ## Heuristic
//!
//! Both images MUST be the same dimensions. The walker compares
//! pixels at the same (x, y). A pixel "changed" if any of its
//! 4 channels (R/G/B/A) deviates by more than the per-channel
//! tolerance (default 5/255).
//!
//! Three metrics surface:
//!
//! * `changed_pixel_count` — absolute count of changed pixels.
//! * `total_pixels` — width × height (denominator for the ratio).
//! * `changed_ratio` — changed / total. Range [0.0, 1.0].
//! * `max_channel_delta` — largest single-channel deviation seen
//!   anywhere in the image. 0 = identical, 255 = full inversion.
//!
//! ## Severity
//!
//! Configurable via caller-side thresholds. The classifier emits:
//!
//! * `pixel-diff.dimension-mismatch` (strict) — baseline +
//!   current have different dimensions. A baseline taken at
//!   1280×800 compared to a current at 1280×1024 is a layout
//!   change that pixel-diff can't compare meaningfully — flag
//!   and bail.
//! * `pixel-diff.major` (strict) — `changed_ratio` ≥ 0.05
//!   (5% of pixels differ).
//! * `pixel-diff.minor` (warn) — `changed_ratio` ≥ 0.005
//!   (0.5% of pixels differ).
//! * Below 0.5% → silent (anti-aliasing noise, font-hinting
//!   variation, etc. fall in this bucket).
//!
//! Both thresholds are configurable on `PixelDiffConfig`.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Per-axis configuration for the pixel-diff detector.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PixelDiffConfig {
    /// Per-channel tolerance. A channel-delta strictly greater
    /// than this counts as a pixel change. 5/255 (~2%) is a
    /// good default that masks anti-aliasing noise.
    pub channel_tolerance: u8,
    /// Strict threshold on `changed_ratio`. Default 0.05 (5%).
    pub strict_threshold: f32,
    /// Warn threshold on `changed_ratio`. Default 0.005 (0.5%).
    pub warn_threshold: f32,
}

impl Default for PixelDiffConfig {
    fn default() -> Self {
        Self {
            channel_tolerance: 5,
            strict_threshold: 0.05,
            warn_threshold: 0.005,
        }
    }
}

/// Result of comparing two screenshots.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PixelDiffMetrics {
    /// Number of pixels whose channel-delta exceeded the
    /// tolerance.
    pub changed_pixel_count: u64,
    /// width × height of the compared images.
    pub total_pixels: u64,
    /// `changed_pixel_count / total_pixels`. Range [0.0, 1.0].
    pub changed_ratio: f32,
    /// Largest single-channel delta observed (0..=255).
    pub max_channel_delta: u8,
}

/// Snapshot the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PixelDiffSnapshot {
    /// Page URL that was screenshotted (informational; the
    /// metrics come from the image comparison, not the URL).
    pub page_url: String,
    /// Viewport identifier (e.g. "390x844-portrait", "1280-desktop").
    pub viewport: String,
    /// Theme name applied during capture.
    pub theme: String,
    /// True if baseline and current have the same dimensions.
    /// False short-circuits the detector to `dimension-mismatch`.
    pub dimensions_match: bool,
    /// Metrics from the per-pixel walk. Always present even
    /// when dimensions don't match — represents partial info.
    pub metrics: PixelDiffMetrics,
}

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_pixel_diff(
    snap: &PixelDiffSnapshot,
    cfg: &PixelDiffConfig,
) -> Vec<AxisFinding> {
    if !snap.dimensions_match {
        return vec![AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "pixel-diff.dimension-mismatch".to_owned(),
            detail: format!(
                "Baseline + current screenshots have different dimensions at viewport {} (theme {}). Layout changed enough that pixel-diff cannot compare; refresh the baseline OR check for an intentional layout-shape change at {}.",
                snap.viewport, snap.theme, snap.page_url
            ),
        }];
    }
    let r = snap.metrics.changed_ratio;
    let max_delta = snap.metrics.max_channel_delta;
    if r >= cfg.strict_threshold {
        return vec![AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "pixel-diff.major".to_owned(),
            detail: format!(
                "Pixel-diff at viewport {} (theme {}): {:.2}% of pixels changed (≥{:.2}% strict threshold), max channel delta {max_delta}. Significant visual regression vs baseline at {}.",
                snap.viewport,
                snap.theme,
                r * 100.0,
                cfg.strict_threshold * 100.0,
                snap.page_url
            ),
        }];
    }
    if r >= cfg.warn_threshold {
        return vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "pixel-diff.minor".to_owned(),
            detail: format!(
                "Pixel-diff at viewport {} (theme {}): {:.2}% of pixels changed (≥{:.2}% warn threshold), max channel delta {max_delta}. Minor visual regression vs baseline at {}.",
                snap.viewport,
                snap.theme,
                r * 100.0,
                cfg.warn_threshold * 100.0,
                snap.page_url
            ),
        }];
    }
    Vec::new()
}

/// Compute pixel-diff metrics from two PNG byte buffers.
/// Returns `None` if either buffer fails to decode as PNG OR
/// the decoded images have different dimensions (caller then
/// surfaces `pixel-diff.dimension-mismatch`).
#[must_use]
pub fn compute_metrics(
    baseline_png: &[u8],
    current_png: &[u8],
    cfg: &PixelDiffConfig,
) -> Option<PixelDiffMetrics> {
    let baseline = image::load_from_memory_with_format(
        baseline_png,
        image::ImageFormat::Png,
    )
    .ok()?
    .to_rgba8();
    let current = image::load_from_memory_with_format(
        current_png,
        image::ImageFormat::Png,
    )
    .ok()?
    .to_rgba8();
    if baseline.dimensions() != current.dimensions() {
        return None;
    }
    let mut changed: u64 = 0;
    let mut max_delta: u8 = 0;
    let total = u64::from(baseline.width()) * u64::from(baseline.height());
    let b_raw = baseline.as_raw();
    let c_raw = current.as_raw();
    for (b, c) in b_raw.chunks_exact(4).zip(c_raw.chunks_exact(4)) {
        let mut pixel_changed = false;
        for ch in 0..4 {
            let d = b[ch].abs_diff(c[ch]);
            if d > max_delta {
                max_delta = d;
            }
            if d > cfg.channel_tolerance {
                pixel_changed = true;
            }
        }
        if pixel_changed {
            changed += 1;
        }
    }
    let ratio = if total == 0 {
        0.0
    } else {
        changed as f32 / total as f32
    };
    Some(PixelDiffMetrics {
        changed_pixel_count: changed,
        total_pixels: total,
        changed_ratio: ratio,
        max_channel_delta: max_delta,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn make_png(w: u32, h: u32, fill: [u8; 4]) -> Vec<u8> {
        let img = ImageBuffer::from_pixel(w, h, Rgba(fill));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("encode png");
        out
    }

    fn snap_with(metrics: PixelDiffMetrics, dimensions_match: bool) -> PixelDiffSnapshot {
        PixelDiffSnapshot {
            page_url: "https://dev.plausiden.com/".to_owned(),
            viewport: "1280".to_owned(),
            theme: "dark".to_owned(),
            dimensions_match,
            metrics,
        }
    }

    fn metrics(changed: u64, total: u64, max_delta: u8) -> PixelDiffMetrics {
        PixelDiffMetrics {
            changed_pixel_count: changed,
            total_pixels: total,
            changed_ratio: if total == 0 {
                0.0
            } else {
                changed as f32 / total as f32
            },
            max_channel_delta: max_delta,
        }
    }

    #[test]
    fn identical_images_no_findings() {
        let cfg = PixelDiffConfig::default();
        let s = snap_with(metrics(0, 1_000_000, 0), true);
        assert!(detect_pixel_diff(&s, &cfg).is_empty());
    }

    #[test]
    fn below_warn_threshold_silent() {
        let cfg = PixelDiffConfig::default();
        let s = snap_with(metrics(100, 1_000_000, 6), true);
        assert!(detect_pixel_diff(&s, &cfg).is_empty());
    }

    #[test]
    fn above_warn_threshold_warns() {
        let cfg = PixelDiffConfig::default();
        let s = snap_with(metrics(10_000, 1_000_000, 50), true);
        let f = detect_pixel_diff(&s, &cfg);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Warn);
        assert_eq!(f[0].kind, "pixel-diff.minor");
    }

    #[test]
    fn above_strict_threshold_strict() {
        let cfg = PixelDiffConfig::default();
        let s = snap_with(metrics(100_000, 1_000_000, 200), true);
        let f = detect_pixel_diff(&s, &cfg);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert_eq!(f[0].kind, "pixel-diff.major");
    }

    #[test]
    fn dimension_mismatch_strict_regardless_of_ratio() {
        let cfg = PixelDiffConfig::default();
        let s = snap_with(metrics(0, 1_000_000, 0), false);
        let f = detect_pixel_diff(&s, &cfg);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "pixel-diff.dimension-mismatch");
    }

    #[test]
    fn compute_metrics_identical_images_no_changes() {
        let cfg = PixelDiffConfig::default();
        let a = make_png(64, 64, [200, 200, 200, 255]);
        let b = a.clone();
        let m = compute_metrics(&a, &b, &cfg).expect("decode");
        assert_eq!(m.changed_pixel_count, 0);
        assert_eq!(m.total_pixels, 64 * 64);
        assert_eq!(m.max_channel_delta, 0);
    }

    #[test]
    fn compute_metrics_full_inversion_all_changed() {
        let cfg = PixelDiffConfig::default();
        let a = make_png(32, 32, [0, 0, 0, 255]);
        let b = make_png(32, 32, [255, 255, 255, 255]);
        let m = compute_metrics(&a, &b, &cfg).expect("decode");
        assert_eq!(m.changed_pixel_count, 32 * 32);
        assert_eq!(m.changed_ratio, 1.0);
        assert_eq!(m.max_channel_delta, 255);
    }

    #[test]
    fn compute_metrics_below_tolerance_no_changes() {
        // Channel delta of 3 is below the default tolerance of 5.
        let cfg = PixelDiffConfig::default();
        let a = make_png(16, 16, [100, 100, 100, 255]);
        let b = make_png(16, 16, [103, 103, 103, 255]);
        let m = compute_metrics(&a, &b, &cfg).expect("decode");
        assert_eq!(m.changed_pixel_count, 0);
        assert_eq!(m.max_channel_delta, 3);
    }

    #[test]
    fn compute_metrics_size_mismatch_returns_none() {
        let cfg = PixelDiffConfig::default();
        let a = make_png(16, 16, [0; 4]);
        let b = make_png(32, 32, [0; 4]);
        assert!(compute_metrics(&a, &b, &cfg).is_none());
    }

    #[test]
    fn compute_metrics_garbage_input_returns_none() {
        let cfg = PixelDiffConfig::default();
        let m = compute_metrics(b"not a png", b"also not a png", &cfg);
        assert!(m.is_none());
    }

    #[test]
    fn config_default_thresholds_are_sane() {
        let c = PixelDiffConfig::default();
        assert!(c.warn_threshold < c.strict_threshold);
        assert!(c.channel_tolerance < 32);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap_with(metrics(10, 100, 10), true);
        let j = serde_json::to_string(&s).expect("ser");
        let back: PixelDiffSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.metrics.changed_pixel_count, 10);
    }
}
