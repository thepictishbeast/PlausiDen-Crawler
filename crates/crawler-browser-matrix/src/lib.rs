//! `crawler-browser-matrix` — typed cross-browser × device
//! matrix for Crawler journeys.
//!
//! Real-world site quality requires checking the same journey
//! against multiple browser engines AND multiple device profiles.
//! A journey that passes Chromium Desktop can fail WebKit iPad
//! Mini (different touch-target rounding, different
//! viewport-meta interpretation, different scrollbar widths).
//!
//! ### What this crate ships
//!
//! Typed surface only — the **runner** instantiates real engines
//! via `chromiumoxide` / `playwright` / WebDriver-BiDi. This
//! crate is the cross-runner contract so a single matrix config
//! produces a single comparable result grid regardless of which
//! runner is in use.
//!
//!   * [`BrowserEngine`]    — closed enum: Chromium / Firefox /
//!                             WebKit
//!   * [`BrowserChannel`]   — closed enum: Stable / Beta / Canary
//!   * [`DeviceProfile`]    — desktop / tablet / mobile + custom
//!                             dimensions
//!   * [`MatrixConfig`]     — set of engines × set of devices to
//!                             test against
//!   * [`MatrixCell`]       — one (engine, device) coordinate
//!   * [`CellOutcome`]      — typed pass/fail per cell
//!   * [`MatrixReport`]     — grid of cell outcomes for a journey
//!
//! ### Per `crawler_stays_general_purpose`
//!
//! No PlausiDen-specific coupling. Sacred.Vote can adopt the
//! same matrix shape unchanged.

#![deny(unsafe_code)]
#![deny(missing_docs)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Closed enum of browser engine families. Stable across
/// Crawler runner implementations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrowserEngine {
    /// Blink-based: Chrome, Edge, Brave, Opera, Arc.
    Chromium,
    /// Gecko: Firefox, Tor Browser, LibreWolf.
    Firefox,
    /// WebKit: Safari macOS / iOS, Epiphany.
    Webkit,
}

impl BrowserEngine {
    /// Stable kebab-case slug.
    pub fn slug(&self) -> &'static str {
        match self {
            Self::Chromium => "chromium",
            Self::Firefox => "firefox",
            Self::Webkit => "webkit",
        }
    }

    /// All engines in stable order.
    pub const ALL: &'static [Self] = &[Self::Chromium, Self::Firefox, Self::Webkit];
}

/// Closed enum of browser release channels.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "kebab-case")]
pub enum BrowserChannel {
    /// Stable release (default).
    #[default]
    Stable,
    /// Beta channel.
    Beta,
    /// Canary / Dev / Nightly.
    Canary,
}

impl BrowserChannel {
    /// Stable kebab-case slug.
    pub fn slug(&self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Canary => "canary",
        }
    }
}

/// Closed enum of device profile categories with typed
/// dimensions. Custom dimensions allowed via [`DeviceProfile::Custom`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum DeviceProfile {
    /// Desktop 1440×900, no touch, 1x DPR.
    Desktop,
    /// Wide desktop 1920×1080, no touch, 1x DPR.
    DesktopWide,
    /// 12.9" tablet — 1024×1366 portrait, touch, 2x DPR.
    TabletLarge,
    /// 10.9" tablet — 820×1180 portrait, touch, 2x DPR.
    TabletMedium,
    /// "Pixel-class" mobile — 412×915, touch, 2.625x DPR.
    MobileAndroid,
    /// "iPhone 14"-class — 390×844, touch, 3x DPR.
    MobileIos,
    /// "iPhone SE"-class small mobile — 320×568, touch, 2x DPR.
    MobileSmall,
    /// Operator-defined dimensions.
    Custom {
        /// Display name shown in reports.
        name: String,
        /// Logical viewport width in CSS px.
        width: u32,
        /// Logical viewport height in CSS px.
        height: u32,
        /// Whether the device reports touch capability.
        touch: bool,
        /// Device pixel ratio (e.g. 1.0, 2.0, 3.0).
        device_pixel_ratio: u32, // stored ×100 to keep Eq
    },
}

impl DeviceProfile {
    /// Logical viewport width.
    pub fn width(&self) -> u32 {
        match self {
            Self::Desktop => 1440,
            Self::DesktopWide => 1920,
            Self::TabletLarge => 1024,
            Self::TabletMedium => 820,
            Self::MobileAndroid => 412,
            Self::MobileIos => 390,
            Self::MobileSmall => 320,
            Self::Custom { width, .. } => *width,
        }
    }

    /// Logical viewport height.
    pub fn height(&self) -> u32 {
        match self {
            Self::Desktop => 900,
            Self::DesktopWide => 1080,
            Self::TabletLarge => 1366,
            Self::TabletMedium => 1180,
            Self::MobileAndroid => 915,
            Self::MobileIos => 844,
            Self::MobileSmall => 568,
            Self::Custom { height, .. } => *height,
        }
    }

    /// Whether the device reports touch capability.
    pub fn is_touch(&self) -> bool {
        match self {
            Self::Desktop | Self::DesktopWide => false,
            Self::TabletLarge
            | Self::TabletMedium
            | Self::MobileAndroid
            | Self::MobileIos
            | Self::MobileSmall => true,
            Self::Custom { touch, .. } => *touch,
        }
    }

    /// Display name for the device profile.
    pub fn display_name(&self) -> String {
        match self {
            Self::Desktop => "Desktop 1440×900".to_string(),
            Self::DesktopWide => "Desktop 1920×1080".to_string(),
            Self::TabletLarge => "Tablet 12.9″ (1024×1366)".to_string(),
            Self::TabletMedium => "Tablet 10.9″ (820×1180)".to_string(),
            Self::MobileAndroid => "Mobile Android (412×915)".to_string(),
            Self::MobileIos => "Mobile iOS (390×844)".to_string(),
            Self::MobileSmall => "Mobile small (320×568)".to_string(),
            Self::Custom { name, .. } => name.clone(),
        }
    }

    /// Built-in profiles (ordered desktop → mobile-small).
    pub fn presets() -> Vec<DeviceProfile> {
        vec![
            Self::Desktop,
            Self::DesktopWide,
            Self::TabletLarge,
            Self::TabletMedium,
            Self::MobileAndroid,
            Self::MobileIos,
            Self::MobileSmall,
        ]
    }
}

/// Configuration for the matrix to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MatrixConfig {
    /// Engines to test. Empty defaults to [`BrowserEngine::ALL`]
    /// at resolution time.
    #[serde(default)]
    pub engines: Vec<BrowserEngine>,
    /// Browser channel for engines that distinguish. Default
    /// Stable.
    #[serde(default)]
    pub channel: BrowserChannel,
    /// Device profiles to test against. Empty defaults to
    /// [`DeviceProfile::presets`] at resolution time.
    #[serde(default)]
    pub devices: Vec<DeviceProfile>,
    /// Whether journey failure in ANY cell fails the overall run.
    /// When false, the report records per-cell outcomes but the
    /// overall journey is considered passing if ≥ 1 cell passes.
    #[serde(default = "default_true")]
    pub all_cells_must_pass: bool,
}

fn default_true() -> bool {
    true
}

impl Default for MatrixConfig {
    fn default() -> Self {
        Self {
            engines: vec![],
            channel: BrowserChannel::Stable,
            devices: vec![],
            all_cells_must_pass: true,
        }
    }
}

impl MatrixConfig {
    /// Resolve to a concrete grid of (engine, device) cells,
    /// applying defaults where empty.
    pub fn resolve_cells(&self) -> Vec<MatrixCell> {
        let engines = if self.engines.is_empty() {
            BrowserEngine::ALL.to_vec()
        } else {
            self.engines.clone()
        };
        let devices = if self.devices.is_empty() {
            DeviceProfile::presets()
        } else {
            self.devices.clone()
        };
        let mut out = Vec::with_capacity(engines.len() * devices.len());
        for e in &engines {
            for d in &devices {
                out.push(MatrixCell {
                    engine: *e,
                    channel: self.channel,
                    device: d.clone(),
                });
            }
        }
        out
    }
}

/// One coordinate in the matrix grid — engine + channel + device.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MatrixCell {
    /// Engine to use.
    pub engine: BrowserEngine,
    /// Release channel.
    pub channel: BrowserChannel,
    /// Device profile to emulate.
    pub device: DeviceProfile,
}

impl MatrixCell {
    /// Stable slug suitable for filenames + admin-UI keys.
    pub fn slug(&self) -> String {
        let dev_slug = match &self.device {
            DeviceProfile::Desktop => "desktop".into(),
            DeviceProfile::DesktopWide => "desktop-wide".into(),
            DeviceProfile::TabletLarge => "tablet-large".into(),
            DeviceProfile::TabletMedium => "tablet-medium".into(),
            DeviceProfile::MobileAndroid => "mobile-android".into(),
            DeviceProfile::MobileIos => "mobile-ios".into(),
            DeviceProfile::MobileSmall => "mobile-small".into(),
            DeviceProfile::Custom { name, .. } => name.to_lowercase().replace(' ', "-"),
        };
        format!(
            "{}-{}-{}",
            self.engine.slug(),
            self.channel.slug(),
            dev_slug
        )
    }
}

/// Outcome per cell — pass / fail with optional finding count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CellOutcome {
    /// Cell coordinate.
    pub cell: MatrixCell,
    /// True iff this cell's journey passed all gates.
    pub passed: bool,
    /// Number of strict findings emitted in this cell.
    #[serde(default)]
    pub strict_count: u32,
    /// Number of warn findings emitted in this cell.
    #[serde(default)]
    pub warn_count: u32,
    /// Wall-clock duration in milliseconds for the cell's run.
    #[serde(default)]
    pub duration_ms: u32,
    /// Optional human-readable detail when failed (top failing
    /// rule kind).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_failure_kind: Option<String>,
}

/// Aggregate report — grid of cell outcomes for one journey run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MatrixReport {
    /// Journey identifier this report covers.
    pub journey_id: String,
    /// Config that produced the matrix.
    pub config: MatrixConfig,
    /// Per-cell outcomes in cell-resolution order.
    pub outcomes: Vec<CellOutcome>,
}

impl MatrixReport {
    /// Overall pass — depends on `config.all_cells_must_pass`.
    pub fn passed(&self) -> bool {
        if self.outcomes.is_empty() {
            return false;
        }
        if self.config.all_cells_must_pass {
            self.outcomes.iter().all(|o| o.passed)
        } else {
            self.outcomes.iter().any(|o| o.passed)
        }
    }

    /// Cells that failed (always reported, regardless of policy).
    pub fn failures(&self) -> Vec<&CellOutcome> {
        self.outcomes.iter().filter(|o| !o.passed).collect()
    }

    /// Per-engine pass-rate summary.
    pub fn by_engine(&self) -> BTreeMap<BrowserEngine, EnginePassRate> {
        let mut out: BTreeMap<BrowserEngine, EnginePassRate> = BTreeMap::new();
        for o in &self.outcomes {
            let e = out.entry(o.cell.engine).or_default();
            e.total += 1;
            if o.passed {
                e.passed += 1;
            }
        }
        out
    }
}

/// Pass-rate accumulator per engine for [`MatrixReport::by_engine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct EnginePassRate {
    /// Cells run on this engine.
    pub total: u32,
    /// Cells that passed on this engine.
    pub passed: u32,
}

/// Errors from the matrix surface.
#[derive(Debug, thiserror::Error)]
pub enum MatrixError {
    /// JSON parse / serialize failure.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_all_has_three_entries() {
        assert_eq!(BrowserEngine::ALL.len(), 3);
    }

    #[test]
    fn engine_slugs_are_stable() {
        assert_eq!(BrowserEngine::Chromium.slug(), "chromium");
        assert_eq!(BrowserEngine::Firefox.slug(), "firefox");
        assert_eq!(BrowserEngine::Webkit.slug(), "webkit");
    }

    #[test]
    fn channel_default_is_stable() {
        assert_eq!(BrowserChannel::default(), BrowserChannel::Stable);
    }

    #[test]
    fn device_presets_have_seven_entries() {
        let p = DeviceProfile::presets();
        assert_eq!(p.len(), 7);
    }

    #[test]
    fn device_dimensions_are_correct_for_known_presets() {
        assert_eq!(DeviceProfile::Desktop.width(), 1440);
        assert_eq!(DeviceProfile::Desktop.height(), 900);
        assert_eq!(DeviceProfile::MobileIos.width(), 390);
        assert_eq!(DeviceProfile::MobileIos.height(), 844);
        assert!(DeviceProfile::MobileIos.is_touch());
        assert!(!DeviceProfile::Desktop.is_touch());
    }

    #[test]
    fn custom_device_dimensions_honored() {
        let d = DeviceProfile::Custom {
            name: "Wide Mobile".into(),
            width: 540,
            height: 1100,
            touch: true,
            device_pixel_ratio: 250,
        };
        assert_eq!(d.width(), 540);
        assert_eq!(d.height(), 1100);
        assert!(d.is_touch());
        assert_eq!(d.display_name(), "Wide Mobile");
    }

    #[test]
    fn matrix_config_resolves_to_full_cross_product() {
        let cfg = MatrixConfig::default();
        let cells = cfg.resolve_cells();
        // 3 engines × 7 device presets = 21 cells.
        assert_eq!(cells.len(), 21);
    }

    #[test]
    fn matrix_config_respects_explicit_engines_and_devices() {
        let cfg = MatrixConfig {
            engines: vec![BrowserEngine::Firefox, BrowserEngine::Webkit],
            channel: BrowserChannel::Stable,
            devices: vec![DeviceProfile::Desktop, DeviceProfile::MobileIos],
            all_cells_must_pass: true,
        };
        let cells = cfg.resolve_cells();
        // 2 × 2 = 4 cells.
        assert_eq!(cells.len(), 4);
    }

    #[test]
    fn matrix_cell_slug_is_stable() {
        let c = MatrixCell {
            engine: BrowserEngine::Webkit,
            channel: BrowserChannel::Stable,
            device: DeviceProfile::MobileIos,
        };
        assert_eq!(c.slug(), "webkit-stable-mobile-ios");
    }

    #[test]
    fn report_passed_strict_policy_requires_every_cell() {
        let cell = MatrixCell {
            engine: BrowserEngine::Chromium,
            channel: BrowserChannel::Stable,
            device: DeviceProfile::Desktop,
        };
        let r = MatrixReport {
            journey_id: "x".into(),
            config: MatrixConfig {
                all_cells_must_pass: true,
                ..Default::default()
            },
            outcomes: vec![
                CellOutcome {
                    cell: cell.clone(),
                    passed: true,
                    strict_count: 0,
                    warn_count: 0,
                    duration_ms: 100,
                    top_failure_kind: None,
                },
                CellOutcome {
                    cell,
                    passed: false,
                    strict_count: 1,
                    warn_count: 0,
                    duration_ms: 200,
                    top_failure_kind: Some("tap.too-small".into()),
                },
            ],
        };
        assert!(!r.passed());
        assert_eq!(r.failures().len(), 1);
    }

    #[test]
    fn report_passed_lax_policy_only_needs_one_cell() {
        let cell = MatrixCell {
            engine: BrowserEngine::Chromium,
            channel: BrowserChannel::Stable,
            device: DeviceProfile::Desktop,
        };
        let r = MatrixReport {
            journey_id: "x".into(),
            config: MatrixConfig {
                all_cells_must_pass: false,
                ..Default::default()
            },
            outcomes: vec![
                CellOutcome {
                    cell: cell.clone(),
                    passed: true,
                    strict_count: 0,
                    warn_count: 0,
                    duration_ms: 100,
                    top_failure_kind: None,
                },
                CellOutcome {
                    cell,
                    passed: false,
                    strict_count: 1,
                    warn_count: 0,
                    duration_ms: 200,
                    top_failure_kind: Some("x".into()),
                },
            ],
        };
        assert!(r.passed());
    }

    #[test]
    fn by_engine_groups_outcomes_correctly() {
        let mk = |engine, passed| CellOutcome {
            cell: MatrixCell {
                engine,
                channel: BrowserChannel::Stable,
                device: DeviceProfile::Desktop,
            },
            passed,
            strict_count: 0,
            warn_count: 0,
            duration_ms: 100,
            top_failure_kind: None,
        };
        let r = MatrixReport {
            journey_id: "x".into(),
            config: MatrixConfig::default(),
            outcomes: vec![
                mk(BrowserEngine::Chromium, true),
                mk(BrowserEngine::Chromium, true),
                mk(BrowserEngine::Webkit, false),
                mk(BrowserEngine::Firefox, true),
            ],
        };
        let by = r.by_engine();
        assert_eq!(by[&BrowserEngine::Chromium].total, 2);
        assert_eq!(by[&BrowserEngine::Chromium].passed, 2);
        assert_eq!(by[&BrowserEngine::Webkit].total, 1);
        assert_eq!(by[&BrowserEngine::Webkit].passed, 0);
        assert_eq!(by[&BrowserEngine::Firefox].total, 1);
    }

    #[test]
    fn matrix_config_serde_round_trips() {
        let cfg = MatrixConfig {
            engines: vec![BrowserEngine::Webkit],
            channel: BrowserChannel::Beta,
            devices: vec![DeviceProfile::MobileIos],
            all_cells_must_pass: false,
        };
        let s = serde_json::to_string(&cfg).unwrap();
        let back: MatrixConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn matrix_config_rejects_unknown_field() {
        let bad =
            r#"{"engines":[],"channel":"stable","devices":[],"all-cells-must-pass":true,"ahem":1}"#;
        let r: Result<MatrixConfig, _> = serde_json::from_str(bad);
        assert!(r.is_err());
    }
}
