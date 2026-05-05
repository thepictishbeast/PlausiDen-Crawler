//! Crawler — typed journey schema.
//!
//! Phase 3 prep: parse `journeys/*.json` into Rust structs the
//! future chromiumoxide runner consumes. Pure Rust, no browser
//! deps — this crate compiles in <1s and the JSON parser fully
//! tested before the heavy chromiumoxide dep lands.
//!
//! Schema mirrored from the TS `Journey` interface in
//! `src/journey.ts`. Step kinds in use across 15 journey files:
//!
//! | kind             | required fields                    |
//! |------------------|------------------------------------|
//! | goto             | url; optional timeout              |
//! | wait             | ms                                 |
//! | screenshot       | label                              |
//! | click            | selector; optional timeout         |
//! | press            | key; optional selector             |
//! | scroll           | (optional selector + position)     |
//! | waitForSelector  | selector; optional timeout         |
//! | discover         | (semantic — runner-specific)       |
//! | probe            | (semantic — runner-specific)       |
//!
//! Optional journey-level fields drive runner behavior:
//!
//! * `viewport`     — render at `{ w, h }` instead of default.
//! * `throttle`     — `slow-3g | fast-3g | regular-4g | offline`
//!   (T53 — CDP `Network.emulateNetworkConditions`).
//! * `zoom`         — text-resize emulation percentage (T54).
//! * `firstTime`    — wipe storage between navigations (T50).
//! * `screenReader` — inject SR-ergonomics audit script (T51).
//! * `storageState` — Playwright storageState path (auth seed).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};

/// Throttle profile — maps to CDP `Network.emulateNetworkConditions`.
///
/// BUG ASSUMPTION: variant names use explicit `#[serde(rename)]`
/// because `rename_all = "kebab-case"` doesn't insert a hyphen
/// between letters and digits — `Slow3g` would otherwise become
/// `slow3g`, mismatching the TS schema which writes `slow-3g`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Throttle {
    /// 150ms RTT, 50 KB/s up + down.
    #[serde(rename = "slow-3g")]
    Slow3g,
    /// 100ms RTT, 200 KB/s down, 93 KB/s up.
    #[serde(rename = "fast-3g")]
    Fast3g,
    /// 20ms RTT, 500 KB/s down, 375 KB/s up.
    #[serde(rename = "regular-4g")]
    Regular4g,
    /// Network disabled (CDP `offline = true`).
    #[serde(rename = "offline")]
    Offline,
}

/// Viewport dimensions. Defaults are runner-controlled.
///
/// BUG ASSUMPTION: two field-name conventions exist in the
/// shipping journey JSON corpus:
/// * `{ "w": 1280, "h": 800 }`        — modern SkillShots (T39+)
/// * `{ "width": 1280, "height": 800 }` — legacy (css-health
///   fixtures, sacred-vote pre-2026-04).
///
/// We accept both via `#[serde(alias)]` so the runner is liberal
/// in what it parses; serialization always writes the modern
/// short form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ViewportDef {
    /// CSS pixel width.
    #[serde(alias = "width")]
    pub w: u32,
    /// CSS pixel height.
    #[serde(alias = "height")]
    pub h: u32,
}

/// One journey step. The `step` enum is `#[non_exhaustive]` so
/// adding kinds is non-breaking.
///
/// BUG ASSUMPTION: `serde(tag = "kind")` plus `rename_all =
/// "camelCase"` matches the TS schema exactly — `goto`, `wait`,
/// `screenshot`, `click`, `press`, `scroll`, `waitForSelector`,
/// `discover`, `probe`. Adding a new kind requires updating
/// both this enum AND every runner that pattern-matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Step {
    /// Navigate to a URL.
    Goto {
        /// Absolute URL.
        url: String,
        /// Per-step timeout in ms (overrides journey default).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        timeout: Option<u32>,
        /// Operator-friendly label; surfaces in screenshots and reports.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        label: Option<String>,
        /// Inline screenshot shortcut: take a screenshot named
        /// `<screenshot>` immediately after this goto. Used by the
        /// css-health-fixtures journey (legacy shape — modern
        /// journeys use a separate `screenshot` step instead).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        screenshot: Option<String>,
    },
    /// Wait a fixed number of ms.
    Wait {
        /// Wait duration in ms.
        ms: u32,
        /// Optional label.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        label: Option<String>,
    },
    /// Take a screenshot.
    Screenshot {
        /// Filename (without extension).
        label: String,
    },
    /// Click an element matching the selector.
    Click {
        /// CSS selector.
        selector: String,
        /// Optional timeout.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        timeout: Option<u32>,
        /// Optional label.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        label: Option<String>,
    },
    /// Press a keyboard key.
    Press {
        /// Key name (e.g. `"End"`, `"Tab"`, `"Enter"`).
        key: String,
        /// Optional element to focus first.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        selector: Option<String>,
        /// Optional label.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        label: Option<String>,
    },
    /// Scroll the page or an element.
    Scroll {
        /// Optional selector — defaults to `<html>`.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        selector: Option<String>,
        /// "top" / "bottom" / pixel offset.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        position: Option<String>,
        /// Optional label.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        label: Option<String>,
    },
    /// Wait until a selector matches.
    #[serde(rename = "waitForSelector")]
    WaitForSelector {
        /// CSS selector.
        selector: String,
        /// Optional timeout.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        timeout: Option<u32>,
        /// Optional label.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        label: Option<String>,
    },
    /// Runner-specific "discover linked pages" step.
    Discover {
        /// Optional label.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        label: Option<String>,
    },
    /// Runner-specific "probe registered backends" step.
    Probe {
        /// Optional label.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        label: Option<String>,
    },
}

impl Step {
    /// Step kind name (matches the JSON `"kind"` value). Useful
    /// for log prefixes and test assertions.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Goto { .. } => "goto",
            Self::Wait { .. } => "wait",
            Self::Screenshot { .. } => "screenshot",
            Self::Click { .. } => "click",
            Self::Press { .. } => "press",
            Self::Scroll { .. } => "scroll",
            Self::WaitForSelector { .. } => "waitForSelector",
            Self::Discover { .. } => "discover",
            Self::Probe { .. } => "probe",
        }
    }

    /// Optional step label, common to every variant.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Goto { label, .. }
            | Self::Wait { label, .. }
            | Self::Click { label, .. }
            | Self::Press { label, .. }
            | Self::Scroll { label, .. }
            | Self::WaitForSelector { label, .. }
            | Self::Discover { label, .. }
            | Self::Probe { label, .. } => label.as_deref(),
            Self::Screenshot { label } => Some(label.as_str()),
        }
    }
}

/// Top-level journey definition. Wire-compat with the TS
/// `Journey` interface in `src/journey.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct Journey {
    /// Journey name. Used as run-output directory prefix.
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Default `goto` URL when a step omits its own.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub base_url: Option<String>,
    /// Render viewport.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub viewport: Option<ViewportDef>,
    /// Network throttle profile.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub throttle: Option<Throttle>,
    /// Text-resize emulation percentage (T54).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub zoom: Option<u32>,
    /// Wipe storage between navigations (T50).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub first_time: Option<bool>,
    /// Inject SR-ergonomics audit (T51).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub screen_reader: Option<bool>,
    /// Playwright storageState path (auth seed).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub storage_state: Option<String>,
    /// Steps in run order.
    pub steps: Vec<Step>,
}

/// Errors from journey-file parsing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum JourneyError {
    /// JSON couldn't be parsed.
    #[error("parse {path}: {source}")]
    Parse {
        /// File path for context.
        path: String,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },
    /// File system read failed.
    #[error("io {path}: {source}")]
    Io {
        /// File path for context.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

/// Read + parse a journey file from disk.
pub fn load(path: &std::path::Path) -> Result<Journey, JourneyError> {
    let bytes = std::fs::read(path).map_err(|e| JourneyError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    serde_json::from_slice(&bytes).map_err(|e| JourneyError::Parse {
        path: path.display().to_string(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skillshots_smoke_journey() {
        let json = r#"{
            "name": "skillshots-poc",
            "description": "smoke",
            "baseUrl": "http://127.0.0.1:8123/",
            "steps": [
                { "kind": "goto", "url": "http://127.0.0.1:8123/", "timeout": 10000, "label": "feed" },
                { "kind": "wait", "ms": 1500, "label": "settle" },
                { "kind": "screenshot", "label": "01-feed-top" },
                { "kind": "press", "key": "End", "label": "scroll-bottom" }
            ]
        }"#;
        let j: Journey = serde_json::from_str(json).expect("parse");
        assert_eq!(j.name, "skillshots-poc");
        assert_eq!(j.steps.len(), 4);
        assert_eq!(j.steps[0].kind(), "goto");
        assert_eq!(j.steps[1].kind(), "wait");
        assert_eq!(j.steps[2].kind(), "screenshot");
        assert_eq!(j.steps[3].kind(), "press");
    }

    #[test]
    fn parse_throttled_journey() {
        let json = r#"{
            "name": "skillshots-poc-throttled",
            "throttle": "slow-3g",
            "steps": [{ "kind": "goto", "url": "http://x/", "label": "h" }]
        }"#;
        let j: Journey = serde_json::from_str(json).expect("parse");
        assert_eq!(j.throttle, Some(Throttle::Slow3g));
    }

    #[test]
    fn parse_zoom_journey() {
        let json = r#"{
            "name": "z",
            "zoom": 200,
            "steps": []
        }"#;
        let j: Journey = serde_json::from_str(json).expect("parse");
        assert_eq!(j.zoom, Some(200));
    }

    #[test]
    fn parse_first_time_journey() {
        let json = r#"{
            "name": "ft",
            "firstTime": true,
            "steps": []
        }"#;
        let j: Journey = serde_json::from_str(json).expect("parse");
        assert_eq!(j.first_time, Some(true));
    }

    #[test]
    fn parse_screen_reader_journey() {
        let json = r#"{
            "name": "sr",
            "screenReader": true,
            "steps": []
        }"#;
        let j: Journey = serde_json::from_str(json).expect("parse");
        assert_eq!(j.screen_reader, Some(true));
    }

    #[test]
    fn parse_wait_for_selector_step() {
        // `r##"..."##` because the JSON body contains `"#main"`
        // which closes a single-hash raw string. Same pattern as
        // crawler-detectors test fixes (T100 series).
        let json = r##"{ "kind": "waitForSelector", "selector": "#main", "timeout": 5000 }"##;
        let s: Step = serde_json::from_str(json).expect("parse");
        match s {
            Step::WaitForSelector {
                selector, timeout, ..
            } => {
                assert_eq!(selector, "#main");
                assert_eq!(timeout, Some(5000));
            }
            other => unreachable!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn step_kind_names_match_ts() {
        // Round-trip every variant through (de)serialization to
        // confirm the JSON tag matches what the TS Crawler writes.
        let cases: &[(&str, Step)] = &[
            (
                "goto",
                Step::Goto {
                    url: "x".into(),
                    timeout: None,
                    label: None,
                    screenshot: None,
                },
            ),
            (
                "wait",
                Step::Wait {
                    ms: 100,
                    label: None,
                },
            ),
            ("screenshot", Step::Screenshot { label: "x".into() }),
            (
                "click",
                Step::Click {
                    selector: "x".into(),
                    timeout: None,
                    label: None,
                },
            ),
            (
                "press",
                Step::Press {
                    key: "Tab".into(),
                    selector: None,
                    label: None,
                },
            ),
            (
                "scroll",
                Step::Scroll {
                    selector: None,
                    position: None,
                    label: None,
                },
            ),
            (
                "waitForSelector",
                Step::WaitForSelector {
                    selector: "x".into(),
                    timeout: None,
                    label: None,
                },
            ),
            ("discover", Step::Discover { label: None }),
            ("probe", Step::Probe { label: None }),
        ];
        for (expected_kind, step) in cases {
            assert_eq!(step.kind(), *expected_kind);
            let json = serde_json::to_string(step).expect("ser");
            assert!(
                json.contains(&format!(r#""kind":"{expected_kind}""#)),
                "kind tag wrong for {expected_kind}: {json}"
            );
            let back: Step = serde_json::from_str(&json).expect("de");
            assert_eq!(step, &back, "round-trip mismatch for {expected_kind}");
        }
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let json = r#"{ "kind": "made-up-step" }"#;
        let r: Result<Step, _> = serde_json::from_str(json);
        assert!(r.is_err(), "unknown step kind must reject");
    }

    #[test]
    fn label_helper_works_across_variants() {
        let s = Step::Goto {
            url: "x".into(),
            timeout: None,
            label: Some("home".into()),
            screenshot: None,
        };
        assert_eq!(s.label(), Some("home"));
        let s = Step::Wait {
            ms: 100,
            label: None,
        };
        assert_eq!(s.label(), None);
        let s = Step::Screenshot {
            label: "shot".into(),
        };
        assert_eq!(s.label(), Some("shot"));
    }

    #[test]
    fn full_journey_round_trips_with_optional_fields() {
        let j = Journey {
            name: "test".into(),
            description: "x".into(),
            base_url: Some("http://x/".into()),
            viewport: Some(ViewportDef { w: 1280, h: 800 }),
            throttle: Some(Throttle::Fast3g),
            zoom: Some(150),
            first_time: Some(true),
            screen_reader: Some(false),
            storage_state: None,
            steps: vec![Step::Goto {
                url: "http://x/".into(),
                timeout: Some(5000),
                label: Some("h".into()),
                screenshot: None,
            }],
        };
        let json = serde_json::to_string(&j).expect("ser");
        let back: Journey = serde_json::from_str(&json).expect("de");
        assert_eq!(j, back);
    }
}
