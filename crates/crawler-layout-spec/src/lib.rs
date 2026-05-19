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
//! DSL + offline runner ship in this crate. The chromiumoxide
//! integration that populates a [`BoundsSnapshot`] from a live
//! page is a downstream concern (the Crawler runner already
//! calls `getBoundingClientRect` per axis — that wiring is
//! mechanical).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// One element's bounding box as captured from a live page.
///
/// Coordinates are CSS pixels relative to the document origin
/// (NOT the viewport — viewport coordinates lose information
/// when the page is scrolled).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BoundingBox {
    /// Left edge (CSS px from document origin).
    pub x: f64,
    /// Top edge (CSS px from document origin).
    pub y: f64,
    /// Width in CSS px.
    pub width: f64,
    /// Height in CSS px.
    pub height: f64,
    /// True iff the element is rendered (non-zero size AND not
    /// `display: none` AND not `visibility: hidden`).
    pub visible: bool,
    /// True iff the element's rect lies fully within the current
    /// viewport (no horizontal scroll required).
    pub in_viewport: bool,
}

impl BoundingBox {
    /// Right edge (x + width).
    pub fn right(&self) -> f64 {
        self.x + self.width
    }
    /// Bottom edge (y + height).
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
    /// Horizontal center.
    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }
    /// Vertical center.
    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }
}

/// A snapshot of every named object's bounding box, keyed by
/// the [`LayoutObject::name`] from the spec.
///
/// Populated by the Crawler runner via `getBoundingClientRect` +
/// `getComputedStyle` evaluation. This crate consumes the
/// snapshot offline — the IO of capturing it is a separate
/// concern, deliberately not coupled here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BoundsSnapshot {
    /// Object name → bounding box.
    pub boxes: HashMap<String, BoundingBox>,
}

impl BoundsSnapshot {
    /// Construct an empty snapshot.
    pub fn new() -> Self {
        Self::default()
    }
    /// Insert a bounding box for a named object.
    pub fn insert(&mut self, name: impl Into<String>, bbox: BoundingBox) {
        self.boxes.insert(name.into(), bbox);
    }
    /// Look up a bounding box by object name.
    pub fn get(&self, name: &str) -> Option<&BoundingBox> {
        self.boxes.get(name)
    }
}

/// Result of evaluating a single assertion against a snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AssertionResult {
    /// Echoed assertion (so the report stands alone without the spec).
    pub assertion: LayoutAssertion,
    /// True iff the assertion held against the snapshot.
    pub passed: bool,
    /// Human-readable detail. On pass, an empty-ish "ok"; on
    /// fail, the measured value vs the expected range/relation.
    pub detail: String,
}

/// Evaluate every assertion in `spec` against `snapshot`.
///
/// Returns one [`AssertionResult`] per assertion, in declared
/// order. If an assertion references an object that's not in the
/// snapshot, the result is `passed: false` with a "missing
/// object" detail — same shape as any other failure so callers
/// don't branch on a separate error.
///
/// This is a pure function: same `(spec, snapshot)` → same
/// `Vec<AssertionResult>`. Safe to call in tests / replay.
pub fn evaluate(spec: &LayoutSpec, snapshot: &BoundsSnapshot) -> Vec<AssertionResult> {
    spec.assertions
        .iter()
        .map(|a| evaluate_assertion(a, snapshot))
        .collect()
}

fn evaluate_assertion(a: &LayoutAssertion, snap: &BoundsSnapshot) -> AssertionResult {
    let (passed, detail) = match a {
        LayoutAssertion::Width { object, range } => match snap.get(object) {
            Some(b) => check_range_f64("width", b.width, *range),
            None => missing(object),
        },
        LayoutAssertion::Height { object, range } => match snap.get(object) {
            Some(b) => check_range_f64("height", b.height, *range),
            None => missing(object),
        },
        LayoutAssertion::Inside {
            inner,
            outer,
            edge,
            offset,
        } => match (snap.get(inner), snap.get(outer)) {
            (Some(i), Some(o)) => check_inside(i, o, *edge, *offset),
            (None, _) => missing(inner),
            (_, None) => missing(outer),
        },
        LayoutAssertion::LeftOf { left, right, gap } => match (snap.get(left), snap.get(right)) {
            (Some(l), Some(r)) => check_horizontal_gap(l, r, *gap),
            (None, _) => missing(left),
            (_, None) => missing(right),
        },
        LayoutAssertion::RightOf { right, left, gap } => match (snap.get(left), snap.get(right)) {
            (Some(l), Some(r)) => check_horizontal_gap(l, r, *gap),
            (None, _) => missing(left),
            (_, None) => missing(right),
        },
        LayoutAssertion::Above { top, bottom, gap } => match (snap.get(top), snap.get(bottom)) {
            (Some(t), Some(b)) => check_vertical_gap(t, b, *gap),
            (None, _) => missing(top),
            (_, None) => missing(bottom),
        },
        LayoutAssertion::Below { bottom, top, gap } => match (snap.get(top), snap.get(bottom)) {
            (Some(t), Some(b)) => check_vertical_gap(t, b, *gap),
            (None, _) => missing(top),
            (_, None) => missing(bottom),
        },
        LayoutAssertion::AlignedTo {
            a: an,
            b: bn,
            axis,
            tolerance,
        } => match (snap.get(an), snap.get(bn)) {
            (Some(a), Some(b)) => check_aligned(a, b, *axis, *tolerance),
            (None, _) => missing(an),
            (_, None) => missing(bn),
        },
        LayoutAssertion::Visible { object } => match snap.get(object) {
            Some(b) if b.visible => (true, "ok".to_owned()),
            Some(_) => (false, format!("{object} is not visible")),
            None => return missing_result(a, object),
        },
        LayoutAssertion::InViewport { object } => match snap.get(object) {
            Some(b) if b.in_viewport => (true, "ok".to_owned()),
            Some(_) => (false, format!("{object} is outside the viewport")),
            None => return missing_result(a, object),
        },
    };
    AssertionResult {
        assertion: a.clone(),
        passed,
        detail,
    }
}

fn check_range_f64(label: &str, value: f64, range: PxRange) -> (bool, String) {
    let v = value.round() as i64;
    let min = range.min as i64;
    let max = range.max as i64;
    if v >= min && v <= max {
        (true, format!("{label}={v}px in [{min}, {max}]"))
    } else {
        (false, format!("{label}={v}px outside [{min}, {max}]"))
    }
}

fn check_inside(
    inner: &BoundingBox,
    outer: &BoundingBox,
    edge: InsideEdge,
    offset: Option<PxRange>,
) -> (bool, String) {
    let contained = inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom();
    if !contained {
        return (
            false,
            format!(
                "inner box ({},{},{}x{}) not contained in outer ({},{},{}x{})",
                inner.x as i64,
                inner.y as i64,
                inner.width as i64,
                inner.height as i64,
                outer.x as i64,
                outer.y as i64,
                outer.width as i64,
                outer.height as i64
            ),
        );
    }
    if let Some(r) = offset {
        let perpendicular_offset: f64 = match edge {
            InsideEdge::Any => return (true, "ok (contained, edge=any)".to_owned()),
            InsideEdge::Top => inner.y - outer.y,
            InsideEdge::Bottom => outer.bottom() - inner.bottom(),
            InsideEdge::Left => inner.x - outer.x,
            InsideEdge::Right => outer.right() - inner.right(),
            InsideEdge::TopLeft => (inner.y - outer.y).max(inner.x - outer.x),
            InsideEdge::TopRight => (inner.y - outer.y).max(outer.right() - inner.right()),
            InsideEdge::BottomLeft => (outer.bottom() - inner.bottom()).max(inner.x - outer.x),
            InsideEdge::BottomRight => {
                (outer.bottom() - inner.bottom()).max(outer.right() - inner.right())
            }
        };
        let v = perpendicular_offset.round() as i64;
        if r.contains(v.max(0) as u32) {
            (true, format!("ok (offset={v}px, edge={edge:?})"))
        } else {
            (
                false,
                format!("offset={v}px outside [{},{}] (edge={edge:?})", r.min, r.max),
            )
        }
    } else {
        (true, "ok (contained)".to_owned())
    }
}

fn check_horizontal_gap(left: &BoundingBox, right: &BoundingBox, gap: PxRange) -> (bool, String) {
    let g = (right.x - left.right()).round() as i64;
    if g < gap.min as i64 || g > gap.max as i64 {
        (false, format!("gap={g}px outside [{},{}]", gap.min, gap.max))
    } else {
        (true, format!("gap={g}px in [{},{}]", gap.min, gap.max))
    }
}

fn check_vertical_gap(top: &BoundingBox, bottom: &BoundingBox, gap: PxRange) -> (bool, String) {
    let g = (bottom.y - top.bottom()).round() as i64;
    if g < gap.min as i64 || g > gap.max as i64 {
        (false, format!("gap={g}px outside [{},{}]", gap.min, gap.max))
    } else {
        (true, format!("gap={g}px in [{},{}]", gap.min, gap.max))
    }
}

fn check_aligned(
    a: &BoundingBox,
    b: &BoundingBox,
    axis: AlignAxis,
    tolerance: u32,
) -> (bool, String) {
    let delta = match axis {
        AlignAxis::Left => (a.x - b.x).abs(),
        AlignAxis::Right => (a.right() - b.right()).abs(),
        AlignAxis::Top => (a.y - b.y).abs(),
        AlignAxis::Bottom => (a.bottom() - b.bottom()).abs(),
        AlignAxis::CenterX => (a.center_x() - b.center_x()).abs(),
        AlignAxis::CenterY => (a.center_y() - b.center_y()).abs(),
    };
    let d = delta.round() as u64;
    if d <= tolerance as u64 {
        (true, format!("delta={d}px within tol={tolerance}"))
    } else {
        (false, format!("delta={d}px exceeds tol={tolerance}"))
    }
}

fn missing(name: &str) -> (bool, String) {
    (false, format!("missing object in snapshot: {name}"))
}

fn missing_result(assertion: &LayoutAssertion, name: &str) -> AssertionResult {
    AssertionResult {
        assertion: assertion.clone(),
        passed: false,
        detail: format!("missing object in snapshot: {name}"),
    }
}

/// Convenience: return the count of failed assertions.
pub fn count_failures(results: &[AssertionResult]) -> usize {
    results.iter().filter(|r| !r.passed).count()
}

/// Convenience: true iff every assertion in `results` passed.
pub fn all_passed(results: &[AssertionResult]) -> bool {
    results.iter().all(|r| r.passed)
}

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

    fn bbox(x: f64, y: f64, w: f64, h: f64) -> BoundingBox {
        BoundingBox {
            x,
            y,
            width: w,
            height: h,
            visible: true,
            in_viewport: true,
        }
    }

    #[test]
    fn evaluate_width_height_pass_and_fail() {
        let spec = LayoutSpec {
            name: "n".into(),
            author: None,
            version: "1".into(),
            applies_to: None,
            objects: vec![LayoutObject {
                name: "h".into(),
                selector: "header".into(),
            }],
            assertions: vec![
                LayoutAssertion::Width {
                    object: "h".into(),
                    range: PxRange::new(1000, 1300),
                },
                LayoutAssertion::Height {
                    object: "h".into(),
                    range: PxRange::new(60, 120),
                },
                LayoutAssertion::Height {
                    object: "h".into(),
                    range: PxRange::new(200, 300),
                },
            ],
        };
        let mut snap = BoundsSnapshot::new();
        snap.insert("h", bbox(0.0, 0.0, 1200.0, 80.0));
        let results = evaluate(&spec, &snap);
        assert!(results[0].passed, "width should pass");
        assert!(results[1].passed, "height should pass");
        assert!(!results[2].passed, "height 200-300 should fail at 80");
        assert_eq!(count_failures(&results), 1);
        assert!(!all_passed(&results));
    }

    #[test]
    fn evaluate_inside_with_top_left_offset() {
        let spec = LayoutSpec {
            name: "n".into(),
            author: None,
            version: "1".into(),
            applies_to: None,
            objects: vec![
                LayoutObject {
                    name: "outer".into(),
                    selector: "header".into(),
                },
                LayoutObject {
                    name: "inner".into(),
                    selector: ".logo".into(),
                },
            ],
            assertions: vec![LayoutAssertion::Inside {
                inner: "inner".into(),
                outer: "outer".into(),
                edge: InsideEdge::TopLeft,
                offset: Some(PxRange::new(0, 20)),
            }],
        };
        let mut snap = BoundsSnapshot::new();
        snap.insert("outer", bbox(0.0, 0.0, 1200.0, 80.0));
        snap.insert("inner", bbox(10.0, 10.0, 100.0, 40.0));
        let results = evaluate(&spec, &snap);
        assert!(results[0].passed);
    }

    #[test]
    fn evaluate_left_of_right_of_gap() {
        let spec = LayoutSpec {
            name: "n".into(),
            author: None,
            version: "1".into(),
            applies_to: None,
            objects: vec![
                LayoutObject {
                    name: "a".into(),
                    selector: "#a".into(),
                },
                LayoutObject {
                    name: "b".into(),
                    selector: "#b".into(),
                },
            ],
            assertions: vec![
                LayoutAssertion::LeftOf {
                    left: "a".into(),
                    right: "b".into(),
                    gap: PxRange::new(10, 50),
                },
                LayoutAssertion::LeftOf {
                    left: "a".into(),
                    right: "b".into(),
                    gap: PxRange::new(100, 200),
                },
            ],
        };
        let mut snap = BoundsSnapshot::new();
        snap.insert("a", bbox(0.0, 0.0, 100.0, 40.0));
        snap.insert("b", bbox(120.0, 0.0, 100.0, 40.0));
        let results = evaluate(&spec, &snap);
        assert!(results[0].passed, "20px gap in [10,50]");
        assert!(!results[1].passed, "20px gap NOT in [100,200]");
    }

    #[test]
    fn evaluate_aligned_to_with_tolerance() {
        let spec = LayoutSpec {
            name: "n".into(),
            author: None,
            version: "1".into(),
            applies_to: None,
            objects: vec![
                LayoutObject {
                    name: "a".into(),
                    selector: "#a".into(),
                },
                LayoutObject {
                    name: "b".into(),
                    selector: "#b".into(),
                },
            ],
            assertions: vec![LayoutAssertion::AlignedTo {
                a: "a".into(),
                b: "b".into(),
                axis: AlignAxis::CenterY,
                tolerance: 2,
            }],
        };
        let mut snap = BoundsSnapshot::new();
        snap.insert("a", bbox(0.0, 0.0, 100.0, 40.0));
        // center_y of a = 20, center_y of b = 21 — within tol 2
        snap.insert("b", bbox(200.0, 1.0, 100.0, 40.0));
        let results = evaluate(&spec, &snap);
        assert!(results[0].passed);
    }

    #[test]
    fn evaluate_visible_and_in_viewport() {
        let spec = LayoutSpec {
            name: "n".into(),
            author: None,
            version: "1".into(),
            applies_to: None,
            objects: vec![LayoutObject {
                name: "x".into(),
                selector: "#x".into(),
            }],
            assertions: vec![
                LayoutAssertion::Visible {
                    object: "x".into(),
                },
                LayoutAssertion::InViewport {
                    object: "x".into(),
                },
            ],
        };
        let mut snap = BoundsSnapshot::new();
        let mut hidden = bbox(0.0, 0.0, 0.0, 0.0);
        hidden.visible = false;
        hidden.in_viewport = false;
        snap.insert("x", hidden);
        let results = evaluate(&spec, &snap);
        assert!(!results[0].passed);
        assert!(!results[1].passed);
    }

    #[test]
    fn evaluate_missing_object_reports_failure_not_panic() {
        let spec = LayoutSpec {
            name: "n".into(),
            author: None,
            version: "1".into(),
            applies_to: None,
            objects: vec![LayoutObject {
                name: "ghost".into(),
                selector: "#missing".into(),
            }],
            assertions: vec![LayoutAssertion::Visible {
                object: "ghost".into(),
            }],
        };
        let snap = BoundsSnapshot::new();
        let results = evaluate(&spec, &snap);
        assert!(!results[0].passed);
        assert!(results[0].detail.contains("missing"));
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
