//! `crawler-reference-capture` — typed wire shape for headless
//! reference-site captures.
//!
//! Cross-repo mirror of
//! `PlausiDen-Forge::forge-core::reference_capture` (forge-core
//! is the canonical owner). Per the doc comment in that module:
//! "Crawler should import forge-core or duplicate the structs to
//! stay in sync." Cross-repo path/git deps would couple the two
//! workspaces; duplication with a `CaptureSpec::V1` tag keeps
//! the contract pinned and lets either side reject mismatched
//! payloads.
//!
//! Scope (this crate): typed wire shape + on-disk round-trip
//! only. The Crawler runner (chromiumoxide-based) instantiates
//! `ReferenceCapture` + `CaptureManifest` and writes them to
//! `<output-dir>/<site-slug>/manifest.json` alongside the
//! screenshot/html/computed-styles artifacts. The forge-core
//! per-axis extractors read the same shape on the consumer side.
//!
//! Spec changes MUST bump `CaptureSpec`; readers refuse
//! mismatched payloads via `CaptureError::SpecMismatch`.

#![deny(unsafe_code)]
#![deny(missing_docs)]

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Spec version. Bumped when the capture shape changes
/// incompatibly. Mirrors forge-core::reference_capture::CaptureSpec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CaptureSpec {
    /// Initial spec, 2026-05-20.
    #[default]
    V1,
}

impl CaptureSpec {
    /// Stable kebab-case slug.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::V1 => "v1",
        }
    }
}

/// One reference-site capture at one viewport.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ReferenceCapture {
    /// Schema version.
    pub spec: CaptureSpec,
    /// URL captured.
    pub url: String,
    /// ISO-8601 RFC-3339 UTC.
    pub captured_at: String,
    /// Viewport width in CSS pixels.
    pub viewport_px: u32,
    /// Path to the screenshot file (PNG). Relative to the
    /// manifest's directory.
    pub screenshot_path: String,
    /// Path to the captured HTML (post-render snapshot).
    pub html_path: String,
    /// Path to the computed-styles JSON dump (per-element CSS
    /// property/value pairs the extractors consume).
    pub computed_styles_path: String,
    /// Network + resource summary.
    pub network_summary: NetworkSummary,
}

/// Summary of resources loaded during the capture.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct NetworkSummary {
    /// Font families actually used (computed-style font-family
    /// values, normalized).
    #[serde(default)]
    pub fonts_loaded: Vec<String>,
    /// `<img>` + `<picture>` count.
    pub image_count: u32,
    /// `<video>` + iframed-video count.
    pub video_count: u32,
    /// `<script>` count.
    pub script_count: u32,
    /// Third-party origins observed (host strings).
    #[serde(default)]
    pub third_party_origins: Vec<String>,
    /// Total bytes downloaded for the capture (HTML + assets).
    pub total_bytes: u64,
}

/// Manifest listing every (URL, viewport) capture for one
/// reference site. Lives at the root of the per-site capture
/// directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CaptureManifest {
    /// Schema version (mirrors CaptureSpec).
    pub spec: CaptureSpec,
    /// Stable slug identifying the reference site (matches the
    /// `corpora/reference_baseline.json` slug field on the
    /// forge-core side).
    pub site_slug: String,
    /// URL of the site root.
    pub url: String,
    /// ISO-8601 RFC-3339 timestamp when the manifest was last
    /// updated.
    pub updated_at: String,
    /// All captures across all (URL, viewport) combinations.
    pub captures: Vec<ReferenceCapture>,
}

/// Errors capture readers + writers can raise.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CaptureError {
    /// I/O error reading or writing.
    #[error("capture I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON parse / emit error.
    #[error("capture JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Spec version skew between reader and payload.
    #[error("capture spec mismatch: expected {expected:?}, got {actual:?}")]
    SpecMismatch {
        /// Expected spec.
        expected: CaptureSpec,
        /// Spec carried by the loaded payload.
        actual: CaptureSpec,
    },
}

impl ReferenceCapture {
    /// Construct a fresh capture with empty resource paths.
    /// Crawler-side emitters fill paths in after writing the
    /// screenshot/html/computed-styles artifacts.
    #[must_use]
    pub fn new(
        url: impl Into<String>,
        captured_at: impl Into<String>,
        viewport_px: u32,
    ) -> Self {
        Self {
            spec: CaptureSpec::V1,
            url: url.into(),
            captured_at: captured_at.into(),
            viewport_px,
            screenshot_path: String::new(),
            html_path: String::new(),
            computed_styles_path: String::new(),
            network_summary: NetworkSummary::default(),
        }
    }
}

impl CaptureManifest {
    /// Construct an empty manifest for a site.
    #[must_use]
    pub fn new(site_slug: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            spec: CaptureSpec::V1,
            site_slug: site_slug.into(),
            url: url.into(),
            updated_at: String::new(),
            captures: Vec::new(),
        }
    }

    /// Read a manifest JSON file from disk. Refuses spec skew.
    pub fn read(path: &Path) -> Result<Self, CaptureError> {
        let body = fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&body)?;
        if manifest.spec != CaptureSpec::V1 {
            return Err(CaptureError::SpecMismatch {
                expected: CaptureSpec::V1,
                actual: manifest.spec,
            });
        }
        Ok(manifest)
    }

    /// Write a manifest JSON file to disk (pretty-printed).
    /// Creates the parent directory if missing.
    pub fn write(&self, path: &Path) -> Result<(), CaptureError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(self)?;
        fs::write(path, body)?;
        Ok(())
    }

    /// Filter captures to those matching a specific viewport.
    #[must_use]
    pub fn for_viewport(&self, viewport_px: u32) -> Vec<&ReferenceCapture> {
        self.captures
            .iter()
            .filter(|c| c.viewport_px == viewport_px)
            .collect()
    }

    /// Resolve a capture-relative path (screenshot / html /
    /// styles) against the manifest's directory.
    #[must_use]
    pub fn resolve_path(manifest_dir: &Path, relative: &str) -> PathBuf {
        manifest_dir.join(relative)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "crawler-reference-capture-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn capture_spec_slug_is_stable() {
        assert_eq!(CaptureSpec::V1.slug(), "v1");
        assert_eq!(CaptureSpec::default(), CaptureSpec::V1);
    }

    #[test]
    fn reference_capture_new_sets_defaults() {
        let c = ReferenceCapture::new(
            "https://example.com",
            "2026-05-20T00:00:00Z",
            1280,
        );
        assert_eq!(c.url, "https://example.com");
        assert_eq!(c.captured_at, "2026-05-20T00:00:00Z");
        assert_eq!(c.viewport_px, 1280);
        assert!(c.screenshot_path.is_empty());
        assert!(c.html_path.is_empty());
        assert!(c.computed_styles_path.is_empty());
        assert_eq!(c.spec, CaptureSpec::V1);
        assert_eq!(c.network_summary.image_count, 0);
        assert_eq!(c.network_summary.total_bytes, 0);
    }

    #[test]
    fn manifest_round_trip_preserves_captures() {
        let dir = temp_dir("round-trip");
        let mut m = CaptureManifest::new("test-site", "https://test.example");
        m.updated_at = "2026-05-20T13:00:00Z".to_string();
        m.captures.push(ReferenceCapture::new(
            "https://test.example",
            "2026-05-20T13:00:00Z",
            390,
        ));
        m.captures.push(ReferenceCapture::new(
            "https://test.example",
            "2026-05-20T13:00:01Z",
            1280,
        ));
        let path = dir.join("manifest.json");
        m.write(&path).unwrap();
        let read_back = CaptureManifest::read(&path).unwrap();
        assert_eq!(read_back.site_slug, "test-site");
        assert_eq!(read_back.url, "https://test.example");
        assert_eq!(read_back.updated_at, "2026-05-20T13:00:00Z");
        assert_eq!(read_back.captures.len(), 2);
        assert_eq!(read_back.captures[0].viewport_px, 390);
        assert_eq!(read_back.captures[1].viewport_px, 1280);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn for_viewport_filters_correctly() {
        let mut m = CaptureManifest::new("s", "https://x");
        m.captures.push(ReferenceCapture::new("https://x", "t", 390));
        m.captures.push(ReferenceCapture::new("https://x", "t", 768));
        m.captures.push(ReferenceCapture::new("https://x", "t", 1280));
        m.captures.push(ReferenceCapture::new("https://x", "t", 1280));
        assert_eq!(m.for_viewport(390).len(), 1);
        assert_eq!(m.for_viewport(768).len(), 1);
        assert_eq!(m.for_viewport(1280).len(), 2);
        assert_eq!(m.for_viewport(9999).len(), 0);
    }

    #[test]
    fn resolve_path_joins_relative_against_manifest_dir() {
        let dir = Path::new("/tmp/captures/abc");
        let p = CaptureManifest::resolve_path(dir, "1280.png");
        assert_eq!(p, PathBuf::from("/tmp/captures/abc/1280.png"));
    }

    #[test]
    fn write_creates_parent_directory() {
        let dir = temp_dir("create-parent");
        let nested = dir.join("a/b/c");
        let path = nested.join("manifest.json");
        let m = CaptureManifest::new("s", "https://x");
        m.write(&path).unwrap();
        assert!(path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_rejects_unparseable_json() {
        let dir = temp_dir("bad-json");
        let path = dir.join("manifest.json");
        fs::write(&path, b"{not json").unwrap();
        match CaptureManifest::read(&path) {
            Err(CaptureError::Json(_)) => {}
            other => panic!("expected Json error, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_serializes_compactly_in_v1() {
        let m = CaptureManifest::new("alpha", "https://alpha.test");
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"spec\":\"v1\""));
        assert!(json.contains("\"site_slug\":\"alpha\""));
    }
}
