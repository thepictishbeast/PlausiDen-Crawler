//! Crawler — Galen-style layout-assertion DSL.
//!
//! Operator-authored specs that describe WHERE elements should
//! sit on the page: "the logo is inside the header at 10px from
//! top-left", "the nav is right of the logo with a gap of 20-40
//! px", "every primary CTA is at least 44px tall".
//!
//! Distinct from `crawler-detectors`, which are auto-fire
//! heuristics that produce findings from a generic catalogue.
//! Layout specs are intent-shaped: the author declares the
//! intended layout, the runner checks whether the live page
//! matches that intent.
//!
//! ## Shape
//!
//! A `LayoutSpec` has three sections:
//!
//! - `objects` — named references to elements via CSS selector.
//! - `assertions` — typed claims about object positions / sizes
//!   / relations.
//! - `metadata` — spec author + version + applied URL pattern.
//!
//! The runner (follow-up; not in this crate) consumes a
//! `LayoutSpec` + a snapshot of every object's bounding box +
//! returns a list of pass/fail results.
//!
//! ## Why a typed DSL
//!
//! Galen-style layout testing in TypeScript is string-typed:
//! `width 200 to 300px`, `inside header 10px top left`. Any
//! typo silently passes. The Rust port refuses that: every
//! assertion variant is a closed enum, every offset is a
//! `Range<u32>`, every selector is non-empty by deserialize-time
//! check.
//!
//! ## Status
//!
//! Closes the DSL half of #116. Runner half — consume a spec
//! + DOM-bounds capture + emit pass/fail — is a follow-up.
//! Shipping the typed surface first lets authors write specs
//! before the runner is finished.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use serde::{Deserialize, Serialize};

/// A complete layout specification.
///
/// Holds named objects + assertions referring to those objects.
/// The runner deserializes a `LayoutSpec` from JSON / YAML, then
/// evaluates each assertion against a DOM-bounds snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LayoutSpec {
    /// Spec name — used in reports. Kebab-case.
    pub name: String,
    /// Spec author identifier (free-form, e.g. an email or team slug).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Spec version — bump when semantic intent changes.
    pub version: String,
    /// URL pattern this spec applies to (glob, e.g. `/blog/**`).
    /// If `None`, the spec applies wherever the runner aims it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applies_to: Option<String>,
    /// Named object references — `name` → CSS selector.
    pub objects: Vec<LayoutObject>,
    /// Assertions about objects' positions / sizes / relations.
    pub assertions: Vec<LayoutAssertion>,
}

/// One named element reference.
///
/// Keeps the spec self-documenting: assertions refer to objects
/// by name, not raw selector. Authors can rename the selector in
/// one place when the page changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LayoutObject {
    /// Stable in-spec name (kebab-case slug).
    pub name: String,
    /// CSS selector that locates this object in the live DOM.
    pub selector: String,
}

/// Inclusive integer range used for size / offset assertions.
///
/// Stored as `[min, max]` in JSON. The runner treats both ends
/// as inclusive — a value `v` passes when `min <= v <= max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PxRange {
    /// Lower bound, inclusive, in CSS pixels.
    pub min: u32,
    /// Upper bound, inclusive, in CSS pixels.
    pub max: u32,
}

impl PxRange {
    /// Construct a range. Panics in debug builds if `min > max`.
    pub const fn new(min: u32, max: u32) -> Self {
        debug_assert!(min <= max, "PxRange: min must be <= max");
        Self { min, max }
    }
    /// True iff `value` is in `[min, max]`.
    pub const fn contains(self, value: u32) -> bool {
        value >= self.min && value <= self.max
    }
}

/// Which edge of an enclosing object an inner object is anchored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsideEdge {
    /// No specific edge — the inner object is anywhere inside.
    Any,
    /// Anchored to the top edge.
    Top,
    /// Anchored to the right edge.
    Right,
    /// Anchored to the bottom edge.
    Bottom,
    /// Anchored to the left edge.
    Left,
    /// Anchored to the top-left corner.
    TopLeft,
    /// Anchored to the top-right corner.
    TopRight,
    /// Anchored to the bottom-left corner.
    BottomLeft,
    /// Anchored to the bottom-right corner.
    BottomRight,
}

/// Axis along which two objects are aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlignAxis {
    /// Same left edge (x).
    Left,
    /// Same right edge (x).
    Right,
    /// Same top edge (y).
    Top,
    /// Same bottom edge (y).
    Bottom,
    /// Same horizontal center (x).
    CenterX,
    /// Same vertical center (y).
    CenterY,
}

/// A single typed assertion about layout.
///
/// Each variant references objects by their `name` from the
/// spec's `objects` list. The runner resolves the name to the
/// object's selector at evaluation time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LayoutAssertion {
    /// `object` width (CSS px) is within `range`.
    Width {
        /// Name of the object being measured.
        object: String,
        /// Inclusive pixel range.
        range: PxRange,
    },
    /// `object` height (CSS px) is within `range`.
    Height {
        /// Name of the object being measured.
        object: String,
        /// Inclusive pixel range.
        range: PxRange,
    },
    /// `inner` is geometrically contained in `outer`, optionally
    /// anchored to a specific edge of `outer` with a positional
    /// offset range.
    Inside {
        /// Name of the inner object.
        inner: String,
        /// Name of the outer (containing) object.
        outer: String,
        /// Which edge / corner of `outer` `inner` is anchored to.
        #[serde(default = "default_inside_edge")]
        edge: InsideEdge,
        /// If `Some`, the perpendicular-to-edge offset must be in
        /// this px range. Ignored when `edge` is `Any`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<PxRange>,
    },
    /// `left.right_edge` <= `right.left_edge` with a horizontal
    /// gap in `gap` px range (negative gaps disallowed — use
    /// `Overlaps` if overlap is intentional).
    LeftOf {
        /// Object on the left.
        left: String,
        /// Object on the right.
        right: String,
        /// Inclusive horizontal-gap range, in CSS px.
        gap: PxRange,
    },
    /// `right.left_edge` >= `left.right_edge` with a horizontal
    /// gap in `gap` px range. Sugar for `LeftOf` with roles
    /// swapped — kept as its own variant so specs read naturally
    /// in author intent ("logo is RIGHT of menu icon").
    RightOf {
        /// Object on the right.
        right: String,
        /// Object on the left.
        left: String,
        /// Inclusive horizontal-gap range, in CSS px.
        gap: PxRange,
    },
    /// `top.bottom_edge` <= `bottom.top_edge` with a vertical gap.
    Above {
        /// Object on top.
        top: String,
        /// Object below.
        bottom: String,
        /// Inclusive vertical-gap range, in CSS px.
        gap: PxRange,
    },
    /// `bottom.top_edge` >= `top.bottom_edge` with a vertical gap.
    Below {
        /// Object below.
        bottom: String,
        /// Object on top.
        top: String,
        /// Inclusive vertical-gap range, in CSS px.
        gap: PxRange,
    },
    /// `a` and `b` are aligned along `axis` within an inclusive
    /// pixel tolerance (sub-pixel rendering can produce 0-1px drift).
    AlignedTo {
        /// First object.
        a: String,
        /// Second object.
        b: String,
        /// Axis along which alignment is required.
        axis: AlignAxis,
        /// Maximum delta in CSS px (default 1).
        #[serde(default = "default_align_tolerance")]
        tolerance: u32,
    },
    /// `object` is visible (non-zero width AND non-zero height AND
    /// not display:none). Operator intent: "this CTA must be on
    /// the page".
    Visible {
        /// Name of the object.
        object: String,
    },
    /// `object` is fully contained within the viewport (no
    /// horizontal scroll required to see it).
    InViewport {
        /// Name of the object.
        object: String,
    },
}

const fn default_inside_edge() -> InsideEdge {
    InsideEdge::Any
}

const fn default_align_tolerance() -> u32 {
    1
}

impl LayoutSpec {
    /// Validate the spec's internal references.
    ///
    /// Checks (in stable order):
    /// 1. `name` and `version` are non-empty.
    /// 2. No two `objects` share a `name`.
    /// 3. Every object's `selector` is non-empty.
    /// 4. Every assertion's referenced object name appears in
    ///    `objects` (catches typos at validation time, not at
    ///    runner time).
    /// 5. Every `PxRange` has `min <= max`.
    pub fn validate(&self) -> Vec<LayoutSpecError> {
        let mut errors = Vec::new();
        if self.name.is_empty() {
            errors.push(LayoutSpecError::EmptyName);
        }
        if self.version.is_empty() {
            errors.push(LayoutSpecError::EmptyVersion);
        }
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for o in &self.objects {
            if !seen.insert(o.name.clone()) {
                errors.push(LayoutSpecError::DuplicateObjectName(o.name.clone()));
            }
            if o.selector.is_empty() {
                errors.push(LayoutSpecError::EmptySelector(o.name.clone()));
            }
        }
        for a in &self.assertions {
            for needed in a.referenced_object_names() {
                if !seen.contains(needed) {
                    errors.push(LayoutSpecError::UnknownObjectRef(needed.to_owned()));
                }
            }
            for r in a.ranges() {
                if r.min > r.max {
                    errors.push(LayoutSpecError::InvertedRange {
                        min: r.min,
                        max: r.max,
                    });
                }
            }
        }
        errors
    }

    /// True iff `validate()` returns no errors.
    pub fn is_valid(&self) -> bool {
        self.validate().is_empty()
    }
}

impl LayoutAssertion {
    /// Names of objects this assertion refers to. Used by
    /// `LayoutSpec::validate` to confirm every reference resolves.
    pub fn referenced_object_names(&self) -> Vec<&str> {
        match self {
            Self::Width { object, .. } => vec![object],
            Self::Height { object, .. } => vec![object],
            Self::Inside { inner, outer, .. } => vec![inner, outer],
            Self::LeftOf { left, right, .. } => vec![left, right],
            Self::RightOf { right, left, .. } => vec![right, left],
            Self::Above { top, bottom, .. } => vec![top, bottom],
            Self::Below { bottom, top, .. } => vec![bottom, top],
            Self::AlignedTo { a, b, .. } => vec![a, b],
            Self::Visible { object } => vec![object],
            Self::InViewport { object } => vec![object],
        }
    }

    /// Px ranges this assertion carries (for inversion checking).
    pub fn ranges(&self) -> Vec<PxRange> {
        match self {
            Self::Width { range, .. } | Self::Height { range, .. } => vec![*range],
            Self::Inside { offset, .. } => offset.map(|r| vec![r]).unwrap_or_default(),
            Self::LeftOf { gap, .. }
            | Self::RightOf { gap, .. }
            | Self::Above { gap, .. }
            | Self::Below { gap, .. } => vec![*gap],
            Self::AlignedTo { .. } | Self::Visible { .. } | Self::InViewport { .. } => Vec::new(),
        }
    }
}

/// Validation errors returned by [`LayoutSpec::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutSpecError {
    /// `spec.name` is empty.
    EmptyName,
    /// `spec.version` is empty.
    EmptyVersion,
    /// Two objects share this name.
    DuplicateObjectName(String),
    /// An object has an empty selector.
    EmptySelector(String),
    /// An assertion references this name but no object declares it.
    UnknownObjectRef(String),
    /// A `PxRange` has `min > max`.
    InvertedRange {
        /// The lower bound found.
        min: u32,
        /// The (smaller) upper bound found.
        max: u32,
    },
}

impl std::fmt::Display for LayoutSpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => write!(f, "spec.name is empty"),
            Self::EmptyVersion => write!(f, "spec.version is empty"),
            Self::DuplicateObjectName(n) => write!(f, "duplicate object name: {n}"),
            Self::EmptySelector(n) => write!(f, "object {n} has empty selector"),
            Self::UnknownObjectRef(n) => write!(f, "assertion references unknown object: {n}"),
            Self::InvertedRange { min, max } => {
                write!(f, "inverted PxRange: min={min} > max={max}")
            }
        }
    }
}

impl std::error::Error for LayoutSpecError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_spec() -> LayoutSpec {
        LayoutSpec {
            name: "site-header".into(),
            author: Some("paul".into()),
            version: "0.1".into(),
            applies_to: Some("/**".into()),
            objects: vec![
                LayoutObject {
                    name: "header".into(),
                    selector: "header.site-header".into(),
                },
                LayoutObject {
                    name: "logo".into(),
                    selector: "header.site-header .logo".into(),
                },
                LayoutObject {
                    name: "nav".into(),
                    selector: "header.site-header nav".into(),
                },
            ],
            assertions: vec![
                LayoutAssertion::Height {
                    object: "header".into(),
                    range: PxRange::new(60, 120),
                },
                LayoutAssertion::Inside {
                    inner: "logo".into(),
                    outer: "header".into(),
                    edge: InsideEdge::TopLeft,
                    offset: Some(PxRange::new(0, 40)),
                },
                LayoutAssertion::LeftOf {
                    left: "logo".into(),
                    right: "nav".into(),
                    gap: PxRange::new(20, 200),
                },
                LayoutAssertion::AlignedTo {
                    a: "logo".into(),
                    b: "nav".into(),
                    axis: AlignAxis::CenterY,
                    tolerance: 2,
                },
                LayoutAssertion::Visible {
                    object: "logo".into(),
                },
                LayoutAssertion::InViewport {
                    object: "header".into(),
                },
            ],
        }
    }

    #[test]
    fn header_spec_roundtrips_json() {
        let spec = header_spec();
        let j = serde_json::to_string(&spec).expect("serializes");
        let back: LayoutSpec = serde_json::from_str(&j).expect("deserializes");
        assert_eq!(spec, back);
    }

    #[test]
    fn header_spec_validates_clean() {
        assert!(header_spec().is_valid());
    }

    #[test]
    fn empty_name_fails_validation() {
        let mut s = header_spec();
        s.name = String::new();
        assert!(s.validate().iter().any(|e| matches!(e, LayoutSpecError::EmptyName)));
    }

    #[test]
    fn duplicate_object_name_fails_validation() {
        let mut s = header_spec();
        s.objects.push(LayoutObject {
            name: "logo".into(),
            selector: "#other".into(),
        });
        assert!(s
            .validate()
            .iter()
            .any(|e| matches!(e, LayoutSpecError::DuplicateObjectName(n) if n == "logo")));
    }

    #[test]
    fn empty_selector_fails_validation() {
        let mut s = header_spec();
        s.objects[0].selector = String::new();
        assert!(s
            .validate()
            .iter()
            .any(|e| matches!(e, LayoutSpecError::EmptySelector(n) if n == "header")));
    }

    #[test]
    fn unknown_object_ref_fails_validation() {
        let mut s = header_spec();
        s.assertions.push(LayoutAssertion::Visible {
            object: "missing-element".into(),
        });
        assert!(s
            .validate()
            .iter()
            .any(|e| matches!(e, LayoutSpecError::UnknownObjectRef(n) if n == "missing-element")));
    }

    #[test]
    fn pxrange_contains_endpoints() {
        let r = PxRange::new(10, 20);
        assert!(r.contains(10));
        assert!(r.contains(15));
        assert!(r.contains(20));
        assert!(!r.contains(9));
        assert!(!r.contains(21));
    }

    #[test]
    fn all_assertion_kinds_round_trip() {
        let kinds = vec![
            LayoutAssertion::Width {
                object: "a".into(),
                range: PxRange::new(1, 2),
            },
            LayoutAssertion::Height {
                object: "a".into(),
                range: PxRange::new(1, 2),
            },
            LayoutAssertion::Inside {
                inner: "a".into(),
                outer: "b".into(),
                edge: InsideEdge::Top,
                offset: Some(PxRange::new(0, 5)),
            },
            LayoutAssertion::LeftOf {
                left: "a".into(),
                right: "b".into(),
                gap: PxRange::new(0, 5),
            },
            LayoutAssertion::RightOf {
                right: "a".into(),
                left: "b".into(),
                gap: PxRange::new(0, 5),
            },
            LayoutAssertion::Above {
                top: "a".into(),
                bottom: "b".into(),
                gap: PxRange::new(0, 5),
            },
            LayoutAssertion::Below {
                bottom: "a".into(),
                top: "b".into(),
                gap: PxRange::new(0, 5),
            },
            LayoutAssertion::AlignedTo {
                a: "a".into(),
                b: "b".into(),
                axis: AlignAxis::Left,
                tolerance: 2,
            },
            LayoutAssertion::Visible {
                object: "a".into(),
            },
            LayoutAssertion::InViewport {
                object: "a".into(),
            },
        ];
        for k in kinds {
            let j = serde_json::to_string(&k).expect("serializes");
            let back: LayoutAssertion = serde_json::from_str(&j).expect("deserializes");
            assert_eq!(k, back);
        }
    }

    #[test]
    fn referenced_object_names_includes_all() {
        let a = LayoutAssertion::Inside {
            inner: "logo".into(),
            outer: "header".into(),
            edge: InsideEdge::Any,
            offset: None,
        };
        let names = a.referenced_object_names();
        assert!(names.contains(&"logo"));
        assert!(names.contains(&"header"));
    }

    #[test]
    fn json_shape_is_tagged_kind() {
        let a = LayoutAssertion::Visible {
            object: "logo".into(),
        };
        let j = serde_json::to_string(&a).unwrap();
        assert!(j.contains("\"kind\":\"visible\""));
        assert!(j.contains("\"object\":\"logo\""));
    }
}
