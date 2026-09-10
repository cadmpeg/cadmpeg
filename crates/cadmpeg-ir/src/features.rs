// SPDX-License-Identifier: Apache-2.0
//! Neutral construction-feature taxonomy.

use std::collections::{BTreeMap, HashSet};

use crate::assets::AssetId;
use crate::ids::{
    BodyId, CurveId, EdgeId, FaceId, FeatureInputTopologyId, FeatureResultTopologyId,
    HistoricalBodyId, HistoricalEdgeId, HistoricalFaceId, HistoricalVertexId, OccurrenceId, SubdId,
    VertexId,
};
use crate::math::{Point2, Point3, Vector3};
use crate::products::{JointId, NonEmptyString};
use crate::scalar::{
    Angle, FiniteReal, Fraction, InteriorAngle, Length, NonNegativeLength, NonZeroLength,
    NonZeroReal, PositiveAngle, PositiveLength, PositiveReal, SlopeAngle,
};
use crate::transform::Transform;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{ser::SerializeStruct, Deserialize, Serialize, Serializer};

macro_rules! checked_feature_geometry {
    ($(#[$meta:meta])* $name:ident, $raw:ident, $value:ident, $valid:expr, $error:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Serialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[serde(transparent)]
        pub struct $name($raw);

        impl $name {
            /// Admit a value that satisfies the feature geometry bounds.
            pub fn new($value: $raw) -> Option<Self> {
                ($valid).then_some(Self($value))
            }

            /// Return the geometric value.
            pub const fn get(self) -> $raw { self.0 }

            /// Borrow the geometric value.
            pub const fn as_raw(&self) -> &$raw { &self.0 }
        }

        impl PartialEq<$raw> for $name {
            fn eq(&self, other: &$raw) -> bool { self.0 == *other }
        }

        impl std::ops::Deref for $name {
            type Target = $raw;
            fn deref(&self) -> &$raw { &self.0 }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = $raw::deserialize(deserializer)?;
                Self::try_from(value).map_err(serde::de::Error::custom)
            }
        }

        impl TryFrom<$raw> for $name {
            type Error = &'static str;
            fn try_from(value: $raw) -> Result<Self, Self::Error> {
                Self::new(value).ok_or($error)
            }
        }

        impl From<$name> for $raw {
            fn from(value: $name) -> Self { value.0 }
        }
    };
}

checked_feature_geometry!(
    /// A model-space point with finite coordinates.
    FinitePoint3, Point3, value,
    [value.x, value.y, value.z].into_iter().all(f64::is_finite),
    "FinitePoint3 coordinates must be finite"
);
checked_feature_geometry!(
    /// A displacement with finite components, including zero.
    FiniteVector3, Vector3, value,
    [value.x, value.y, value.z].into_iter().all(f64::is_finite),
    "FiniteVector3 components must be finite"
);
impl FiniteVector3 {
    /// Reverse all components.
    #[must_use]
    pub fn negated(self) -> Self {
        Self(Vector3::new(-self.0.x, -self.0.y, -self.0.z))
    }
}

checked_feature_geometry!(
    /// A direction with finite nonzero norm.
    FeatureDirection3, Vector3, value,
    value.norm().is_finite() && value.norm() > 0.0,
    "FeatureDirection3 norm must be finite and nonzero"
);

checked_feature_geometry!(
    /// A finite right-handed rigid feature placement.
    FeatureRigidPlacement, Transform, value, value.is_proper_rigid(),
    "FeatureRigidPlacement must be a finite right-handed rigid transform"
);

impl FeatureRigidPlacement {
    /// Return the identity placement.
    pub fn identity() -> Self {
        Self(Transform::identity())
    }
}

const EPS_FEATURE_UNIT_FRAME: f64 = 1.0e-9;

/// A finite origin and two perpendicular unit directions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "FeatureUnitPlaneFrameWire")]
pub struct FeatureUnitPlaneFrame {
    origin: FinitePoint3,
    u_axis: Vector3,
    v_axis: Vector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct FeatureUnitPlaneFrameWire {
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
}

impl FeatureUnitPlaneFrame {
    /// Admit a finite origin and perpendicular unit axes within the feature tolerance.
    pub fn new(origin: Point3, u_axis: Vector3, v_axis: Vector3) -> Option<Self> {
        let origin = FinitePoint3::new(origin)?;
        ((u_axis.norm() - 1.0).abs() <= EPS_FEATURE_UNIT_FRAME
            && (v_axis.norm() - 1.0).abs() <= EPS_FEATURE_UNIT_FRAME
            && u_axis.dot(v_axis).abs() <= EPS_FEATURE_UNIT_FRAME)
            .then_some(Self {
                origin,
                u_axis,
                v_axis,
            })
    }

    /// Return the model-space origin.
    pub fn origin(self) -> Point3 {
        self.origin.get()
    }
    /// Return the first unit direction.
    pub fn u_axis(self) -> Vector3 {
        self.u_axis
    }
    /// Return the second unit direction.
    pub fn v_axis(self) -> Vector3 {
        self.v_axis
    }
}

impl TryFrom<FeatureUnitPlaneFrameWire> for FeatureUnitPlaneFrame {
    type Error = &'static str;
    fn try_from(wire: FeatureUnitPlaneFrameWire) -> Result<Self, Self::Error> {
        Self::new(wire.origin, wire.u_axis, wire.v_axis)
            .ok_or("feature plane requires a finite origin and perpendicular unit axes")
    }
}

/// A finite right-handed coordinate frame with perpendicular unit axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "FeatureCoordinateFrameWire",
    into = "FeatureCoordinateFrameWire"
)]
pub struct FeatureCoordinateFrame {
    plane: FeatureUnitPlaneFrame,
    z_axis: Vector3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct FeatureCoordinateFrameWire {
    origin: Point3,
    x_axis: Vector3,
    y_axis: Vector3,
    z_axis: Vector3,
}

impl FeatureCoordinateFrame {
    /// Admit a finite right-handed frame within the feature unit-axis tolerance.
    pub fn new(origin: Point3, x_axis: Vector3, y_axis: Vector3, z_axis: Vector3) -> Option<Self> {
        let plane = FeatureUnitPlaneFrame::new(origin, x_axis, y_axis)?;
        ((z_axis.norm() - 1.0).abs() <= EPS_FEATURE_UNIT_FRAME
            && x_axis.dot(z_axis).abs() <= EPS_FEATURE_UNIT_FRAME
            && y_axis.dot(z_axis).abs() <= EPS_FEATURE_UNIT_FRAME
            && x_axis.cross(y_axis).dot(z_axis) >= 1.0 - EPS_FEATURE_UNIT_FRAME)
            .then_some(Self { plane, z_axis })
    }
    /// Return the model-space origin.
    pub fn origin(self) -> Point3 {
        self.plane.origin()
    }
    /// Return the x-axis.
    pub fn x_axis(self) -> Vector3 {
        self.plane.u_axis()
    }
    /// Return the y-axis.
    pub fn y_axis(self) -> Vector3 {
        self.plane.v_axis()
    }
    /// Return the z-axis.
    pub fn z_axis(self) -> Vector3 {
        self.z_axis
    }
}

impl TryFrom<FeatureCoordinateFrameWire> for FeatureCoordinateFrame {
    type Error = &'static str;
    fn try_from(wire: FeatureCoordinateFrameWire) -> Result<Self, Self::Error> {
        Self::new(wire.origin, wire.x_axis, wire.y_axis, wire.z_axis)
            .ok_or("feature coordinate frame requires finite origin and right-handed perpendicular unit axes")
    }
}
impl From<FeatureCoordinateFrame> for FeatureCoordinateFrameWire {
    fn from(frame: FeatureCoordinateFrame) -> Self {
        Self {
            origin: frame.origin(),
            x_axis: frame.x_axis(),
            y_axis: frame.y_axis(),
            z_axis: frame.z_axis(),
        }
    }
}

/// Two finite opposite image corners with nonzero extent in both coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "[Point2; 2]", into = "[Point2; 2]")]
pub struct FeatureImageBounds([Point2; 2]);
impl FeatureImageBounds {
    /// Admit finite corners with nonzero width and height, in either order.
    pub fn new(corners: [Point2; 2]) -> Option<Self> {
        let [first, second] = corners;
        ([first.u, first.v, second.u, second.v]
            .into_iter()
            .all(f64::is_finite)
            && first.u != second.u
            && first.v != second.v)
            .then_some(Self(corners))
    }
    /// Return the opposite corners.
    pub fn corners(self) -> [Point2; 2] {
        self.0
    }
}
impl TryFrom<[Point2; 2]> for FeatureImageBounds {
    type Error = &'static str;
    fn try_from(corners: [Point2; 2]) -> Result<Self, Self::Error> {
        Self::new(corners).ok_or("image bounds require finite corners and nonzero width and height")
    }
}
impl From<FeatureImageBounds> for [Point2; 2] {
    fn from(bounds: FeatureImageBounds) -> Self {
        bounds.0
    }
}

const EPS_FEATURE_PLANE_ORTHOGONAL: f64 = 1.0e-9;

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct FeaturePlaneFrameWire {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
}

macro_rules! checked_feature_plane_frame {
    ($(#[$meta:meta])* $name:ident, $normal_length:ident, $u_length:ident, $bound:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[serde(try_from = "FeaturePlaneFrameWire", into = "FeaturePlaneFrameWire")]
        pub struct $name {
            origin: FinitePoint3,
            normal: FeatureDirection3,
            u_axis: FeatureDirection3,
        }
        impl $name {
            /// Admit finite origin and nonzero perpendicular directions without normalizing them.
            pub fn new(origin: Point3, normal: Vector3, u_axis: Vector3) -> Option<Self> {
                let origin = FinitePoint3::new(origin)?;
                let normal = FeatureDirection3::new(normal)?;
                let u_axis = FeatureDirection3::new(u_axis)?;
                let $normal_length = normal.norm();
                let $u_length = u_axis.norm();
                if normal.dot(u_axis.get()).abs() > $bound { return None; }
                Some(Self { origin, normal, u_axis })
            }
            /// Return the model-space origin.
            pub fn origin(self) -> Point3 { self.origin.get() }
            /// Return the plane normal with its original magnitude.
            pub fn normal(self) -> Vector3 { self.normal.get() }
            /// Return the in-plane direction with its original magnitude.
            pub fn u_axis(self) -> Vector3 { self.u_axis.get() }
        }
        impl TryFrom<FeaturePlaneFrameWire> for $name {
            type Error = &'static str;
            fn try_from(wire: FeaturePlaneFrameWire) -> Result<Self, Self::Error> {
                Self::new(wire.origin, wire.normal, wire.u_axis)
                    .ok_or("plane frame requires finite origin and nonzero perpendicular directions")
            }
        }
        impl From<$name> for FeaturePlaneFrameWire {
            fn from(frame: $name) -> Self {
                Self { origin: frame.origin(), normal: frame.normal(), u_axis: frame.u_axis() }
            }
        }
    };
}

checked_feature_plane_frame!(
    /// A datum-plane frame whose orthogonality bound scales with the product of direction norms.
    FeatureDatumPlaneFrame, normal_length, u_length,
    { let scale = normal_length * u_length; if !scale.is_finite() { return None; } EPS_FEATURE_PLANE_ORTHOGONAL * scale }
);
checked_feature_plane_frame!(
    /// A resolved support-plane frame whose relative orthogonality bound scales each norm in order.
    FeatureSupportPlaneFrame, normal_length, u_length,
    EPS_FEATURE_PLANE_ORTHOGONAL * normal_length * u_length
);

/// A straight feature edge with distinct finite endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "FeatureLineSegmentWire")]
pub struct FeatureLineSegment {
    start: FinitePoint3,
    end: FinitePoint3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct FeatureLineSegmentWire {
    start: Point3,
    end: Point3,
}

impl FeatureLineSegment {
    /// Admit distinct finite line endpoints.
    pub fn new(start: Point3, end: Point3) -> Option<Self> {
        let start = FinitePoint3::new(start)?;
        let end = FinitePoint3::new(end)?;
        (start != end).then_some(Self { start, end })
    }

    /// Return the start point.
    pub fn start(self) -> Point3 {
        self.start.get()
    }

    /// Return the end point.
    pub fn end(self) -> Point3 {
        self.end.get()
    }
}

impl TryFrom<FeatureLineSegmentWire> for FeatureLineSegment {
    type Error = &'static str;
    fn try_from(wire: FeatureLineSegmentWire) -> Result<Self, Self::Error> {
        Self::new(wire.start, wire.end).ok_or("line start and end must be finite and distinct")
    }
}

/// A finite feature polyline with distinct adjacent vertices and sufficient vertices for closure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "FeaturePolylineWire")]
pub struct FeaturePolyline {
    points: Vec<FinitePoint3>,
    closed: bool,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct FeaturePolylineWire {
    points: Vec<Point3>,
    closed: bool,
}

impl FeaturePolyline {
    /// Admit a finite chain with at least two points, or three when closed.
    pub fn new(points: Vec<Point3>, closed: bool) -> Option<Self> {
        if points.len() < 2
            || (closed && points.len() < 3)
            || points.windows(2).any(|pair| pair[0] == pair[1])
        {
            return None;
        }
        let points = points
            .into_iter()
            .map(FinitePoint3::new)
            .collect::<Option<Vec<_>>>()?;
        Some(Self { points, closed })
    }

    /// Return the ordered vertices.
    pub fn points(&self) -> &[FinitePoint3] {
        &self.points
    }

    /// Whether the last vertex connects to the first.
    pub fn closed(&self) -> bool {
        self.closed
    }
}

impl TryFrom<FeaturePolylineWire> for FeaturePolyline {
    type Error = &'static str;
    fn try_from(wire: FeaturePolylineWire) -> Result<Self, Self::Error> {
        Self::new(wire.points, wire.closed).ok_or("polyline points must be finite, adjacent-distinct, and sufficient for its closed state")
    }
}

/// Coordinate expressions over a finite increasing feature-curve domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "FeatureEquationCurveWire")]
pub struct FeatureEquationCurve {
    parameter: String,
    x_expression: String,
    y_expression: String,
    z_expression: String,
    start: FiniteReal,
    end: FiniteReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct FeatureEquationCurveWire {
    parameter: String,
    x_expression: String,
    y_expression: String,
    z_expression: String,
    start: f64,
    end: f64,
}

impl FeatureEquationCurve {
    /// Admit nonblank expressions and finite increasing parameter bounds.
    pub fn new(
        parameter: String,
        x_expression: String,
        y_expression: String,
        z_expression: String,
        start: f64,
        end: f64,
    ) -> Option<Self> {
        if [&parameter, &x_expression, &y_expression, &z_expression]
            .into_iter()
            .any(|value| value.trim().is_empty())
            || start >= end
        {
            return None;
        }
        let start = FiniteReal::new(start)?;
        let end = FiniteReal::new(end)?;
        Some(Self {
            parameter,
            x_expression,
            y_expression,
            z_expression,
            start,
            end,
        })
    }

    /// Return the independent parameter symbol.
    pub fn parameter(&self) -> &str {
        &self.parameter
    }

    /// Return the model-space x expression.
    pub fn x_expression(&self) -> &str {
        &self.x_expression
    }

    /// Return the model-space y expression.
    pub fn y_expression(&self) -> &str {
        &self.y_expression
    }

    /// Return the model-space z expression.
    pub fn z_expression(&self) -> &str {
        &self.z_expression
    }

    /// Return the inclusive lower parameter bound.
    pub fn start(&self) -> f64 {
        self.start.get()
    }

    /// Return the inclusive upper parameter bound.
    pub fn end(&self) -> f64 {
        self.end.get()
    }
}

impl TryFrom<FeatureEquationCurveWire> for FeatureEquationCurve {
    type Error = &'static str;
    fn try_from(wire: FeatureEquationCurveWire) -> Result<Self, Self::Error> {
        Self::new(wire.parameter, wire.x_expression, wire.y_expression, wire.z_expression, wire.start, wire.end)
            .ok_or("equation-curve expressions must be nonblank and its start and end finite and increasing")
    }
}

const EPS_FEATURE_ELLIPSE_AXES_ORTHO: f64 = 1.0e-9;

/// A finite circular feature arc with a nonzero angular span.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "FeatureCircularArcWire", into = "FeatureCircularArcWire")]
pub struct FeatureCircularArc {
    center: FinitePoint3,
    normal: FeatureDirection3,
    radius: PositiveLength,
    angles: crate::geometry::DirectedParameterRange,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct FeatureCircularArcWire {
    center: Point3,
    normal: Vector3,
    radius: PositiveLength,
    start_angle: f64,
    end_angle: f64,
}

impl FeatureCircularArc {
    /// Admit a finite circle frame and a nonzero angular interval.
    pub fn new(
        center: Point3,
        normal: Vector3,
        radius: PositiveLength,
        angles: crate::geometry::DirectedParameterRange,
    ) -> Option<Self> {
        Some(Self {
            center: FinitePoint3::new(center)?,
            normal: FeatureDirection3::new(normal)?,
            radius,
            angles,
        })
    }

    /// Return the circle center.
    pub fn center(self) -> Point3 {
        self.center.get()
    }

    /// Return the circle-plane normal.
    pub fn normal(self) -> Vector3 {
        self.normal.get()
    }

    /// Return the radius.
    pub fn radius(self) -> PositiveLength {
        self.radius
    }

    /// Return the directed angular interval in radians.
    pub fn angles(self) -> crate::geometry::DirectedParameterRange {
        self.angles
    }
}

impl TryFrom<FeatureCircularArcWire> for FeatureCircularArc {
    type Error = &'static str;
    fn try_from(wire: FeatureCircularArcWire) -> Result<Self, Self::Error> {
        let angles =
            crate::geometry::DirectedParameterRange::new([wire.start_angle, wire.end_angle])
                .map_err(|_| "arc start_angle and end_angle must be finite and distinct")?;
        Self::new(wire.center, wire.normal, wire.radius, angles)
            .ok_or("circular-arc center must be finite and normal must have finite nonzero norm")
    }
}

impl From<FeatureCircularArc> for FeatureCircularArcWire {
    fn from(value: FeatureCircularArc) -> Self {
        let [start_angle, end_angle] = value.angles.endpoints();
        Self {
            center: value.center.get(),
            normal: value.normal.get(),
            radius: value.radius,
            start_angle,
            end_angle,
        }
    }
}

/// A finite elliptic feature arc with perpendicular axes, ordered radii, and a nonzero angular span.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "FeatureEllipticArcWire", into = "FeatureEllipticArcWire")]
pub struct FeatureEllipticArc {
    center: FinitePoint3,
    normal: FeatureDirection3,
    major_axis: FeatureDirection3,
    radii: [PositiveLength; 2],
    angles: crate::geometry::DirectedParameterRange,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct FeatureEllipticArcWire {
    center: Point3,
    normal: Vector3,
    major_axis: Vector3,
    major_radius: PositiveLength,
    minor_radius: PositiveLength,
    start_angle: f64,
    end_angle: f64,
}

impl FeatureEllipticArc {
    /// Admit a finite ellipse frame, major/minor radii, and a directed angular interval.
    pub fn new(
        center: Point3,
        normal: Vector3,
        major_axis: Vector3,
        radii: [PositiveLength; 2],
        angles: crate::geometry::DirectedParameterRange,
    ) -> Option<Self> {
        let center = FinitePoint3::new(center)?;
        let normal = FeatureDirection3::new(normal)?;
        let major_axis = FeatureDirection3::new(major_axis)?;
        if normal.dot(major_axis.get()).abs() > EPS_FEATURE_ELLIPSE_AXES_ORTHO
            || radii[1].get() > radii[0].get()
        {
            return None;
        }
        Some(Self {
            center,
            normal,
            major_axis,
            radii,
            angles,
        })
    }

    /// Return the ellipse center.
    pub fn center(self) -> Point3 {
        self.center.get()
    }

    /// Return the ellipse-plane normal.
    pub fn normal(self) -> Vector3 {
        self.normal.get()
    }

    /// Return the major-axis direction.
    pub fn major_axis(self) -> Vector3 {
        self.major_axis.get()
    }

    /// Return the major and minor semiaxis radii.
    pub fn radii(self) -> [PositiveLength; 2] {
        self.radii
    }

    /// Return the directed angular interval in radians.
    pub fn angles(self) -> crate::geometry::DirectedParameterRange {
        self.angles
    }
}

impl TryFrom<FeatureEllipticArcWire> for FeatureEllipticArc {
    type Error = &'static str;
    fn try_from(wire: FeatureEllipticArcWire) -> Result<Self, Self::Error> {
        let angles =
            crate::geometry::DirectedParameterRange::new([wire.start_angle, wire.end_angle])
                .map_err(|_| "arc start_angle and end_angle must be finite and distinct")?;
        Self::new(
            wire.center,
            wire.normal,
            wire.major_axis,
            [wire.major_radius, wire.minor_radius],
            angles,
        )
        .ok_or("elliptic-arc center, axes, or major/minor radius order is invalid")
    }
}

impl From<FeatureEllipticArc> for FeatureEllipticArcWire {
    fn from(value: FeatureEllipticArc) -> Self {
        let [start_angle, end_angle] = value.angles.endpoints();
        let [major_radius, minor_radius] = value.radii;
        Self {
            center: value.center.get(),
            normal: value.normal.get(),
            major_axis: value.major_axis.get(),
            major_radius,
            minor_radius,
            start_angle,
            end_angle,
        }
    }
}

/// Resolved pull frame of a draft anchor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DraftPull {
    /// Pull direction used to measure the draft angle.
    pub direction: FeatureDirection3,
    /// Datum-plane feature that supplied the direction, when retained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<FeatureId>,
}

/// Selection form and pull frame of a draft operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DraftAnchor {
    /// A neutral plane remains fixed during the operation.
    NeutralPlane {
        /// Neutral-plane selection.
        plane: FaceSelection,
        /// Explicit pull frame, when the source retains one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pull: Option<DraftPull>,
    },
    /// A parting tool and its pull frame define a parting-line draft.
    PartingLine {
        /// Parting-tool faces.
        tool: FaceSelection,
        /// Required pull frame.
        pull: DraftPull,
    },
}

impl DraftAnchor {
    /// Pull frame retained by this anchor, when available.
    #[must_use]
    pub const fn pull(&self) -> Option<&DraftPull> {
        match self {
            Self::NeutralPlane { pull, .. } => pull.as_ref(),
            Self::PartingLine { pull, .. } => Some(pull),
        }
    }

    /// Mutable pull frame retained by this anchor, when available.
    #[must_use]
    pub fn pull_mut(&mut self) -> Option<&mut DraftPull> {
        match self {
            Self::NeutralPlane { pull, .. } => pull.as_mut(),
            Self::PartingLine { pull, .. } => Some(pull),
        }
    }
}

crate::ids::id_type!(
    /// Identifies a neutral construction feature.
    FeatureId
);

crate::ids::id_type!(
    /// Identifies a neutral design configuration.
    ConfigurationId
);

/// Resolution state of a configuration's source display name.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(untagged)]
pub enum ConfigurationName {
    /// Source display name.
    Resolved(String),
    /// Source configuration exists but its display name is not established.
    #[default]
    Unresolved,
}

impl ConfigurationName {
    /// Return the source display name when resolved.
    pub fn resolved(&self) -> Option<&str> {
        match self {
            Self::Resolved(value) => Some(value),
            Self::Unresolved => None,
        }
    }

    /// Return the mutable source display name when resolved.
    pub fn resolved_mut(&mut self) -> Option<&mut String> {
        match self {
            Self::Resolved(value) => Some(value),
            Self::Unresolved => None,
        }
    }

    /// Whether the source display name remains unresolved.
    pub fn is_unresolved(&self) -> bool {
        matches!(self, Self::Unresolved)
    }
}

impl From<String> for ConfigurationName {
    fn from(value: String) -> Self {
        Self::Resolved(value)
    }
}

impl From<&str> for ConfigurationName {
    fn from(value: &str) -> Self {
        Self::Resolved(value.to_string())
    }
}

impl PartialEq<str> for ConfigurationName {
    fn eq(&self, other: &str) -> bool {
        self.resolved() == Some(other)
    }
}

impl PartialEq<&str> for ConfigurationName {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl PartialEq<String> for ConfigurationName {
    fn eq(&self, other: &String) -> bool {
        self == other.as_str()
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(untagged)]
/// Resolution state of one configuration's complete body membership.
pub enum ConfigurationBodies {
    /// Complete ordered body membership.
    Resolved(DistinctMembers<BodyId>),
    /// Source configuration exists but its body membership is not established.
    #[default]
    Unresolved,
}

impl ConfigurationBodies {
    /// Return complete body membership when resolved.
    pub fn resolved(&self) -> Option<&[BodyId]> {
        match self {
            Self::Resolved(value) => Some(value),
            Self::Unresolved => None,
        }
    }
    /// Whether body membership remains unresolved.
    pub fn is_unresolved(&self) -> bool {
        matches!(self, Self::Unresolved)
    }
    /// Iterate over resolved membership; unresolved membership yields no values.
    pub fn iter(&self) -> std::slice::Iter<'_, BodyId> {
        self.resolved().unwrap_or_default().iter()
    }
    /// Number of resolved members; zero when unresolved.
    pub fn len(&self) -> usize {
        self.resolved().map_or(0, <[BodyId]>::len)
    }
    /// Whether resolved membership is empty; false when unresolved.
    pub fn is_empty(&self) -> bool {
        self.resolved().is_some_and(<[BodyId]>::is_empty)
    }
}

impl PartialEq<Vec<BodyId>> for ConfigurationBodies {
    fn eq(&self, other: &Vec<BodyId>) -> bool {
        self.resolved() == Some(other.as_slice())
    }
}

impl<'a> IntoIterator for &'a ConfigurationBodies {
    type Item = &'a BodyId;
    type IntoIter = std::slice::Iter<'a, BodyId>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A named parametric model variant.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignConfiguration {
    /// Globally unique configuration id.
    pub id: ConfigurationId,
    /// Position in the design configuration list.
    pub ordinal: u32,
    /// Whether this configuration supplies the document's active model state.
    pub active: bool,
    /// Format-native configuration slot, when distinct from list order.
    pub source_index: Option<u32>,
    /// Source display name, when established.
    pub name: ConfigurationName,
    /// Material override, when present.
    pub material: Option<String>,
    /// Configuration-local named values not otherwise represented.
    pub properties: BTreeMap<String, String>,
    /// Configuration-specific source expressions keyed by the overridden parameter.
    pub parameter_overrides: BTreeMap<ParameterId, String>,
    /// Bodies present when this configuration is active.
    pub bodies: ConfigurationBodies,
    /// Evaluated parameter state when this configuration is active.
    pub parameter_values: BTreeMap<ParameterId, ParameterValue>,
    /// Evaluated feature operation state when this configuration is active.
    pub feature_states: BTreeMap<FeatureId, ConfigurationFeatureState>,
    /// Identifier of the full-fidelity record in a native namespace.
    pub native_ref: Option<String>,
}

impl DesignConfiguration {
    /// Features suppressed when this configuration is active.
    pub fn suppressed_features(&self) -> impl Iterator<Item = &FeatureId> {
        self.feature_states
            .iter()
            .filter_map(|(feature, state)| state.evaluation.is_suppressed().then_some(feature))
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct DesignConfigurationReadWire {
    id: ConfigurationId,
    #[serde(default)]
    ordinal: u32,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    source_index: Option<u32>,
    #[serde(default)]
    name: ConfigurationName,
    #[serde(default)]
    material: Option<String>,
    #[serde(default)]
    properties: BTreeMap<String, String>,
    #[serde(default)]
    parameter_overrides: BTreeMap<ParameterId, String>,
    #[serde(default)]
    suppressed_features: Vec<FeatureId>,
    #[serde(default)]
    bodies: ConfigurationBodies,
    #[serde(default)]
    parameter_values: BTreeMap<ParameterId, ParameterValue>,
    #[serde(default)]
    feature_states: BTreeMap<FeatureId, ConfigurationFeatureState>,
    #[serde(default)]
    native_ref: Option<String>,
}

impl DesignConfigurationReadWire {
    pub(crate) fn into_configuration(self) -> Result<DesignConfiguration, String> {
        let mut listed = HashSet::new();
        for feature_id in &self.suppressed_features {
            if !listed.insert(feature_id.clone()) {
                return Err(format!(
                    "configuration repeats suppressed feature `{}`",
                    feature_id.0
                ));
            }
            let Some(state) = self.feature_states.get(feature_id) else {
                return Err(format!(
                    "configuration suppressed feature `{}` has no configuration feature state",
                    feature_id.0
                ));
            };
            if !state.evaluation.is_suppressed() {
                return Err(format!(
                    "configuration suppression disagrees with feature state `{}`",
                    feature_id.0
                ));
            }
        }
        if let Some(feature) = self.feature_states.iter().find_map(|(feature, state)| {
            (state.evaluation.is_suppressed() && !listed.contains(feature)).then_some(feature)
        }) {
            return Err(format!(
                "configuration feature state `{}` is suppressed but absent from suppressed_features",
                feature.0
            ));
        }
        Ok(DesignConfiguration {
            id: self.id,
            ordinal: self.ordinal,
            active: self.active,
            source_index: self.source_index,
            name: self.name,
            material: self.material,
            properties: self.properties,
            parameter_overrides: self.parameter_overrides,
            bodies: self.bodies,
            parameter_values: self.parameter_values,
            feature_states: self.feature_states,
            native_ref: self.native_ref,
        })
    }
}

impl Serialize for DesignConfiguration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let suppressed_features = self.suppressed_features().collect::<Vec<_>>();
        let mut state = serializer.serialize_struct("DesignConfiguration", 13)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("ordinal", &self.ordinal)?;
        if self.active {
            state.serialize_field("active", &self.active)?;
        }
        if let Some(source_index) = self.source_index {
            state.serialize_field("source_index", &source_index)?;
        }
        if !self.name.is_unresolved() {
            state.serialize_field("name", &self.name)?;
        }
        if let Some(material) = &self.material {
            state.serialize_field("material", material)?;
        }
        if !self.properties.is_empty() {
            state.serialize_field("properties", &self.properties)?;
        }
        if !self.parameter_overrides.is_empty() {
            state.serialize_field("parameter_overrides", &self.parameter_overrides)?;
        }
        if !suppressed_features.is_empty() {
            state.serialize_field("suppressed_features", &suppressed_features)?;
        }
        if !self.bodies.is_unresolved() {
            state.serialize_field("bodies", &self.bodies)?;
        }
        if !self.parameter_values.is_empty() {
            state.serialize_field("parameter_values", &self.parameter_values)?;
        }
        if !self.feature_states.is_empty() {
            state.serialize_field("feature_states", &self.feature_states)?;
        }
        if let Some(native_ref) = &self.native_ref {
            state.serialize_field("native_ref", native_ref)?;
        }
        state.end()
    }
}

impl<'de> Deserialize<'de> for DesignConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        DesignConfigurationReadWire::deserialize(deserializer)?
            .into_configuration()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for DesignConfiguration {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DesignConfiguration".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        DesignConfigurationReadWire::json_schema(generator)
    }
}

/// Configuration-local evaluation state for one construction feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ConfigurationFeatureState {
    /// Whether evaluation produced bodies or was suppressed.
    pub evaluation: ConfigurationEvaluation,
    /// Earlier features consumed during regeneration in source operand order.
    #[serde(
        default,
        skip_serializing_if = "DistinctMembers::is_empty",
        deserialize_with = "deserialize_dependencies"
    )]
    pub dependencies: DistinctMembers<FeatureId>,
    /// Evaluated construction semantics in the configuration.
    pub definition: FeatureDefinition,
}

/// Result of evaluating one feature in a configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfigurationEvaluation {
    /// The feature is suppressed for this configuration.
    Suppressed,
    /// The feature is active; the output list may be empty for operations
    /// whose neutral result is carried by the surrounding topology.
    Active {
        /// Bodies produced or modified in the configuration.
        #[serde(default, skip_serializing_if = "DistinctMembers::is_empty")]
        outputs: DistinctMembers<BodyId>,
    },
}

impl<'de> Deserialize<'de> for ConfigurationEvaluation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            Suppressed {},
            Active {
                #[serde(default)]
                outputs: Vec<BodyId>,
            },
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Suppressed {} => Self::Suppressed,
            Wire::Active { outputs } => Self::Active {
                outputs: outputs
                    .try_into()
                    .map_err(|error| serde::de::Error::custom(format!("outputs: {error}")))?,
            },
        })
    }
}

impl ConfigurationEvaluation {
    /// Whether evaluation of the feature is suppressed.
    #[must_use]
    pub const fn is_suppressed(&self) -> bool {
        matches!(self, Self::Suppressed)
    }

    /// Bodies produced or modified by an active feature.
    #[must_use]
    pub fn outputs(&self) -> &[BodyId] {
        match self {
            Self::Suppressed => &[],
            Self::Active { outputs } => outputs,
        }
    }
}

crate::ids::id_type!(
    /// Identifies a neutral design parameter.
    ParameterId
);

/// A named design expression, optionally owned by a construction feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DesignParameter {
    /// Globally unique parameter id.
    pub id: ParameterId,
    /// Feature that consumes this parameter; absent for a document parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<FeatureId>,
    /// Position among parameters in the same ownership scope.
    #[serde(default)]
    pub ordinal: u32,
    /// Source parameter name.
    pub name: String,
    /// Literal or expression text used by the source system.
    pub expression: String,
    /// Geometric display semantics carried by the dimension expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<DimensionDisplay>,
    /// Evaluated scalar when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ParameterValue>,
    /// Parameters referenced by `expression`, in source expression order.
    #[serde(
        default,
        skip_serializing_if = "DistinctMembers::is_empty",
        deserialize_with = "deserialize_dependencies"
    )]
    pub dependencies: DistinctMembers<ParameterId>,
    /// Source parameter properties not represented by another field.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    /// Product-manufacturing dimension semantics, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pmi: Option<ParameterPmi>,
    /// Identifier of the full-fidelity source parameter record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Product-manufacturing semantics attached to a design parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ParameterPmi {
    /// Semantic dimension family.
    pub subtype: PmiDimensionSubtype,
    /// Display precision carried by the semantic annotation.
    pub precision: i64,
    /// Native formatted dimension text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_text: Option<String>,
    /// Basic-dimension flag.
    pub basic: bool,
    /// Inspection-dimension flag.
    pub inspection: bool,
    /// Reference-only flag.
    pub reference_only: bool,
    /// Identifier of the full-fidelity semantic record.
    pub native_ref: String,
}

/// Semantic PMI dimension family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "native_kind", rename_all = "snake_case")]
pub enum PmiDimensionSubtype {
    /// Linear distance.
    Linear,
    /// Angular extent in radians.
    Angle,
    /// Diameter.
    Diameter,
    /// Radius.
    Radial,
    /// Linear coordinate measured from an ordinate origin.
    Ordinate,
    /// Dimensionless integral instance count.
    Count,
    /// Source-native family without a neutral equivalent.
    Native(String),
}

/// Geometric interpretation requested by a dimension display modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DimensionDisplay {
    /// Displays the dimension as a diameter.
    Diameter,
    /// Displays the dimension as a radius.
    Radius,
}

/// Canonical scalar value of a literal design parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ParameterValue {
    /// Length in canonical millimeters.
    Length(Length),
    /// Angle in canonical radians.
    Angle(Angle),
    /// Dimensionless real scalar.
    Real(#[serde(deserialize_with = "deserialize_parameter_real")] FiniteReal),
    /// Integer scalar.
    Integer(i64),
    /// Boolean scalar.
    Boolean(bool),
    /// Literal text value.
    String(String),
}

crate::units::named_field!(deserialize_parameter_real, FiniteReal, "value");

fn deserialize_dependencies<'de, D, T>(deserializer: D) -> Result<DistinctMembers<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Eq + std::hash::Hash,
{
    crate::units::deserialize_named(deserializer, "dependencies")
}

/// A polygon side count of at least three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct PolygonSideCount(u32);

impl PolygonSideCount {
    /// Admits a side count of at least three.
    pub fn new(value: u32) -> Option<Self> {
        (value >= 3).then_some(Self(value))
    }

    /// Returns the number of sides.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for PolygonSideCount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u32::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("polygon sides must be at least three"))
    }
}

/// A feature definition and its compatible output-body list.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureEvaluation {
    definition: FeatureDefinition,
    outputs: Vec<BodyId>,
}

impl FeatureEvaluation {
    /// Admit outputs that equal resolved inserted-body membership.
    pub fn new(definition: FeatureDefinition, outputs: Vec<BodyId>) -> Result<Self, &'static str> {
        if Self::inserted_bodies(&definition).is_some_and(|bodies| *bodies != outputs) {
            return Err("outputs must equal the resolved InsertBodies selection");
        }
        Ok(Self {
            definition,
            outputs,
        })
    }

    fn inserted_bodies(definition: &FeatureDefinition) -> Option<&Vec<BodyId>> {
        let definition = match definition {
            FeatureDefinition::PostProcess { operation, .. } => operation.as_ref(),
            definition => definition,
        };
        match definition {
            FeatureDefinition::InsertBodies {
                bodies: BodySelection::Resolved { bodies, .. },
            } => Some(bodies),
            _ => None,
        }
    }

    /// Construct an evaluation with the definition's inserted bodies or no outputs.
    pub fn from_definition(definition: FeatureDefinition) -> Self {
        let outputs = Self::inserted_bodies(&definition)
            .cloned()
            .unwrap_or_default();
        Self {
            definition,
            outputs,
        }
    }

    /// Return the neutral construction semantics.
    pub fn definition(&self) -> &FeatureDefinition {
        &self.definition
    }

    /// Return the produced or modified body identities.
    pub fn outputs(&self) -> &Vec<BodyId> {
        &self.outputs
    }

    /// Admit edited semantics and outputs before replacing either value.
    pub fn try_edit(
        &mut self,
        edit: impl FnOnce(&mut FeatureDefinition, &mut Vec<BodyId>),
    ) -> Result<(), &'static str> {
        let mut definition = self.definition.clone();
        let mut outputs = self.outputs.clone();
        edit(&mut definition, &mut outputs);
        *self = Self::new(definition, outputs)?;
        Ok(())
    }

    /// Replace semantics while preserving compatible output bodies.
    pub fn set_definition(&mut self, definition: FeatureDefinition) -> Result<(), &'static str> {
        if Self::inserted_bodies(&definition).is_some_and(|bodies| *bodies != self.outputs) {
            return Err("outputs must equal the resolved InsertBodies selection");
        }
        self.definition = definition;
        Ok(())
    }

    /// Replace output bodies when compatible with the definition.
    pub fn set_outputs(&mut self, outputs: Vec<BodyId>) -> Result<(), &'static str> {
        if Self::inserted_bodies(&self.definition).is_some_and(|bodies| *bodies != outputs) {
            return Err("outputs must equal the resolved InsertBodies selection");
        }
        self.outputs = outputs;
        Ok(())
    }
}

/// An ordered neutral construction feature and its resulting bodies.
///
/// Prefer [`Feature::new`] for invariant-bearing construction. There is no
/// public [`Default`]: an empty id with an arbitrary definition is illegal.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    /// Globally unique feature id.
    pub id: FeatureId,
    /// Stable construction order within the source history.
    pub ordinal: u64,
    /// Source display name.
    pub name: Option<String>,
    /// Whether evaluation of this feature is disabled.
    pub suppressed: Option<bool>,
    /// Earlier features consumed during regeneration, in source operand order.
    pub dependencies: DistinctMembers<FeatureId>,
    /// Source operation attributes not consumed by the neutral definition.
    pub source_properties: BTreeMap<String, String>,
    /// Source XML element name for the operation record.
    pub source_tag: Option<String>,
    /// Text payload of a source leaf operation.
    pub source_text: Option<String>,
    /// Ordered source text, parameter, and child-feature content.
    pub source_content: FeatureContent,
    /// Construction semantics and their admitted resulting bodies.
    pub evaluation: FeatureEvaluation,
    /// Identifier of the full-fidelity record in a native namespace.
    pub native_ref: Option<String>,
}

impl Feature {
    /// Construct a feature from its identity, construction order, and definition.
    pub fn new(id: FeatureId, ordinal: u64, definition: FeatureDefinition) -> Self {
        Self {
            id,
            ordinal,
            name: None,
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(definition),
            native_ref: None,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct FeatureWriteWire<'a> {
    id: &'a FeatureId,
    ordinal: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: &'a Option<String>,
    suppressed: &'a Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<&'a FeatureId>,
    #[serde(skip_serializing_if = "DistinctMembers::is_empty")]
    dependencies: &'a DistinctMembers<FeatureId>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    source_properties: &'a BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_tag: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_text: &'a Option<String>,
    #[serde(skip_serializing_if = "FeatureContent::is_empty")]
    source_content: &'a FeatureContent,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    outputs: &'a Vec<BodyId>,
    definition: &'a FeatureDefinition,
    #[serde(skip_serializing_if = "Option::is_none")]
    native_ref: &'a Option<String>,
}

impl<'a> FeatureWriteWire<'a> {
    pub(crate) fn new(feature: &'a Feature, parent: Option<&'a FeatureId>) -> Self {
        Self {
            id: &feature.id,
            ordinal: feature.ordinal,
            name: &feature.name,
            suppressed: &feature.suppressed,
            parent,
            dependencies: &feature.dependencies,
            source_properties: &feature.source_properties,
            source_tag: &feature.source_tag,
            source_text: &feature.source_text,
            source_content: &feature.source_content,
            outputs: feature.evaluation.outputs(),
            definition: feature.evaluation.definition(),
            native_ref: &feature.native_ref,
        }
    }

    pub(crate) fn standalone(feature: &'a Feature) -> Self {
        Self::new(feature, None)
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct FeatureReadWire {
    id: FeatureId,
    ordinal: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    suppressed: Option<bool>,
    #[serde(default)]
    parent: Option<FeatureId>,
    #[serde(default, deserialize_with = "deserialize_dependencies")]
    dependencies: DistinctMembers<FeatureId>,
    #[serde(default)]
    source_properties: BTreeMap<String, String>,
    #[serde(default)]
    source_tag: Option<String>,
    #[serde(default)]
    source_text: Option<String>,
    #[serde(default)]
    source_content: FeatureContent,
    #[serde(default)]
    outputs: Vec<BodyId>,
    definition: FeatureDefinition,
    #[serde(default)]
    native_ref: Option<String>,
}

impl FeatureReadWire {
    pub(crate) fn into_parts(self) -> Result<(Feature, Option<FeatureId>), &'static str> {
        Ok((
            Feature {
                id: self.id,
                ordinal: self.ordinal,
                name: self.name,
                suppressed: self.suppressed,
                dependencies: self.dependencies,
                source_properties: self.source_properties,
                source_tag: self.source_tag,
                source_text: self.source_text,
                source_content: self.source_content,
                evaluation: FeatureEvaluation::new(self.definition, self.outputs)?,
                native_ref: self.native_ref,
            },
            self.parent,
        ))
    }
}

impl Serialize for Feature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        FeatureWriteWire::standalone(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Feature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (feature, parent) = FeatureReadWire::deserialize(deserializer)?
            .into_parts()
            .map_err(serde::de::Error::custom)?;
        if parent.is_some() {
            return Err(serde::de::Error::custom(
                "a feature parent requires its owning model",
            ));
        }
        Ok(feature)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for Feature {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Feature".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        FeatureReadWire::json_schema(generator)
    }
}

/// Typed topology membership at one feature's evaluation input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct FeatureInputTopology {
    /// Globally unique state id.
    pub id: FeatureInputTopologyId,
    /// Feature evaluated from this state.
    pub input_of: FeatureId,
    /// Bodies present in this state.
    #[serde(default, skip_serializing_if = "DistinctMembers::is_empty")]
    pub bodies: DistinctMembers<HistoricalBodyId>,
    /// Faces present in this state.
    #[serde(default, skip_serializing_if = "DistinctMembers::is_empty")]
    pub faces: DistinctMembers<HistoricalFaceId>,
    /// Edges present in this state.
    #[serde(default, skip_serializing_if = "DistinctMembers::is_empty")]
    pub edges: DistinctMembers<HistoricalEdgeId>,
    /// Vertices present in this state.
    #[serde(default, skip_serializing_if = "DistinctMembers::is_empty")]
    pub vertices: DistinctMembers<HistoricalVertexId>,
    /// Full-fidelity source state reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Persistent feature-local topology identities in one regenerated result.
///
/// These identities describe intermediate history results that need not be
/// members of the saved current model topology. Generated feature selections
/// address members through the producing feature and the corresponding local
/// identity.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct FeatureResultTopology {
    /// Globally unique result-state id.
    pub id: FeatureResultTopologyId,
    /// Feature that produces this state.
    pub output_of: FeatureId,
    /// Feature-local body identities in stable source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bodies: Vec<String>,
    /// Feature-local face identities in stable source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    faces: Vec<String>,
    /// Feature-local edge identities in stable source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    edges: Vec<String>,
    /// Feature-local vertex identities in stable source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vertices: Vec<String>,
    /// Full-fidelity source state reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

#[derive(Deserialize)]
struct FeatureResultTopologyWire {
    id: FeatureResultTopologyId,
    output_of: FeatureId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bodies: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    faces: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    edges: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vertices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_ref: Option<String>,
}

impl FeatureResultTopology {
    /// A nonempty result with distinct nonblank local identities in each arena.
    pub fn new(
        id: FeatureResultTopologyId,
        output_of: FeatureId,
        bodies: Vec<String>,
        faces: Vec<String>,
        edges: Vec<String>,
        vertices: Vec<String>,
        native_ref: Option<String>,
    ) -> Result<Self, &'static str> {
        if bodies.is_empty() && faces.is_empty() && edges.is_empty() && vertices.is_empty() {
            return Err("feature result topology members must not be empty");
        }
        for (error, members) in [
            ("bodies must be nonblank and distinct", &bodies),
            ("faces must be nonblank and distinct", &faces),
            ("edges must be nonblank and distinct", &edges),
            ("vertices must be nonblank and distinct", &vertices),
        ] {
            if members.iter().any(|name| name.trim().is_empty())
                || members.iter().collect::<HashSet<_>>().len() != members.len()
            {
                return Err(error);
            }
        }
        Ok(Self {
            id,
            output_of,
            bodies,
            faces,
            edges,
            vertices,
            native_ref,
        })
    }

    /// Feature-local body identities.
    pub fn bodies(&self) -> &[String] {
        &self.bodies
    }
    /// Feature-local face identities.
    pub fn faces(&self) -> &[String] {
        &self.faces
    }
    /// Feature-local edge identities.
    pub fn edges(&self) -> &[String] {
        &self.edges
    }
    /// Feature-local vertex identities.
    pub fn vertices(&self) -> &[String] {
        &self.vertices
    }
}

impl<'de> Deserialize<'de> for FeatureResultTopology {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = FeatureResultTopologyWire::deserialize(deserializer)?;
        Self::new(
            wire.id,
            wire.output_of,
            wire.bodies,
            wire.faces,
            wire.edges,
            wire.vertices,
            wire.native_ref,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Ordered source content with distinct parameter and child-feature references.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct FeatureContent(Vec<FeatureSourceContent>);

impl TryFrom<Vec<FeatureSourceContent>> for FeatureContent {
    type Error = &'static str;
    fn try_from(value: Vec<FeatureSourceContent>) -> Result<Self, Self::Error> {
        let mut seen = HashSet::new();
        if value
            .iter()
            .filter(|item| !matches!(item, FeatureSourceContent::Text(_)))
            .any(|item| !seen.insert(item))
        {
            return Err("source_content repeats a parameter or child-feature reference");
        }
        Ok(Self(value))
    }
}

impl FeatureContent {
    /// Constructs text-only content in source order, retaining repeated text.
    pub fn text(values: impl IntoIterator<Item = String>) -> Self {
        Self(values.into_iter().map(FeatureSourceContent::Text).collect())
    }

    /// Appends content without repeating a parameter or child-feature reference.
    pub fn push(&mut self, value: FeatureSourceContent) -> Result<(), &'static str> {
        if !matches!(value, FeatureSourceContent::Text(_)) && self.0.contains(&value) {
            return Err("source_content repeats a parameter or child-feature reference");
        }
        self.0.push(value);
        Ok(())
    }

    /// Whether the sequence has no content.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The source content in order.
    pub fn as_slice(&self) -> &[FeatureSourceContent] {
        &self.0
    }
}

impl std::ops::Deref for FeatureContent {
    type Target = [FeatureSourceContent];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> IntoIterator for &'a FeatureContent {
    type Item = &'a FeatureSourceContent;
    type IntoIter = std::slice::Iter<'a, FeatureSourceContent>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de> Deserialize<'de> for FeatureContent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Vec::<FeatureSourceContent>::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

/// One item in a source feature's mixed-content sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FeatureSourceContent {
    /// Literal text between child records.
    Text(String),
    /// Dimension or equation parameter at this position.
    Parameter(ParameterId),
    /// Nested feature record at this position.
    Feature(FeatureId),
}

/// Parametric support of an offset datum plane.
///
/// The untagged representation retains the legacy feature-id string while face
/// selections use their existing tagged object representation.
#[derive(Debug, Clone, PartialEq)]
pub enum DatumPlaneReference {
    /// Another datum-plane feature.
    Feature(FeatureId),
    /// A selected planar face whose geometry defines the support plane.
    Face(FaceSelection),
    /// A resolved support plane without a face identity.
    ResolvedPlane {
        /// Finite support plane with nonzero perpendicular directions.
        frame: FeatureSupportPlaneFrame,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
enum DatumPlaneReferenceWireRef<'a> {
    Feature(&'a FeatureId),
    Face {
        face: &'a FaceSelection,
    },
    ResolvedPlane {
        face: FaceSelection,
        origin: Point3,
        normal: Vector3,
        u_axis: Vector3,
    },
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(untagged)]
enum DatumPlaneReferenceWire {
    Feature(FeatureId),
    LegacyFace(DatumPlaneLegacyFaceWire),
    Face(DatumPlaneFaceWire),
    ResolvedPlane(DatumResolvedPlaneWire),
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct DatumPlaneLegacyFaceWire {
    face: FaceSelection,
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct DatumPlaneFaceWire {
    face: FaceSelection,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct DatumResolvedPlaneWire {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
}

impl Serialize for DatumPlaneReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Feature(feature) => DatumPlaneReferenceWireRef::Feature(feature),
            Self::Face(face) => DatumPlaneReferenceWireRef::Face { face },
            Self::ResolvedPlane { frame } => DatumPlaneReferenceWireRef::ResolvedPlane {
                face: FaceSelection::Unresolved,
                origin: frame.origin(),
                normal: frame.normal(),
                u_axis: frame.u_axis(),
            },
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DatumPlaneReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match DatumPlaneReferenceWire::deserialize(deserializer)? {
            DatumPlaneReferenceWire::Feature(feature) => Self::Feature(feature),
            DatumPlaneReferenceWire::LegacyFace(DatumPlaneLegacyFaceWire {
                face: FaceSelection::Unresolved,
                origin,
                normal,
                u_axis,
            })
            | DatumPlaneReferenceWire::ResolvedPlane(DatumResolvedPlaneWire {
                origin,
                normal,
                u_axis,
            }) => Self::ResolvedPlane {
                frame: FeatureSupportPlaneFrame::new(origin, normal, u_axis)
                    .ok_or_else(|| serde::de::Error::custom("resolved plane requires finite origin and nonzero perpendicular directions"))?,
            },
            DatumPlaneReferenceWire::LegacyFace(DatumPlaneLegacyFaceWire { face, .. })
            | DatumPlaneReferenceWire::Face(DatumPlaneFaceWire { face }) => Self::Face(face),
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for DatumPlaneReference {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DatumPlaneReference".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        DatumPlaneReferenceWire::json_schema(generator)
    }
}

/// Sketch point operand resolved by a datum-point construction or retained in
/// native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SketchPointSelection {
    /// Selection exists semantically but its sketch entity is not resolved.
    Unresolved,
    /// Point in a planar sketch.
    Planar {
        /// Owning planar sketch.
        sketch: crate::sketches::SketchId,
        /// Selected point entity.
        point: crate::sketches::SketchEntityId,
        /// Format-native persistent selection reference.
        native: String,
    },
    /// Point in a model-space sketch.
    Spatial {
        /// Owning model-space sketch.
        sketch: crate::sketches::SpatialSketchId,
        /// Selected point entity.
        point: crate::sketches::SpatialSketchEntityId,
        /// Format-native persistent selection reference.
        native: String,
    },
    /// Format-native selection reference.
    Native(String),
}

/// Construction rule used to derive one datum point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DatumPointConstruction {
    /// Center of one selected circular edge.
    CircleCenter {
        /// Selected circular edge.
        edge: EdgeSelection,
    },
    /// Intersection of two selected edges.
    TwoEdgeIntersection {
        /// Selected edges in source order.
        edges: [EdgeSelection; 2],
    },
    /// Intersection of three selected planes.
    ThreePlaneIntersection {
        /// Selected planes in source order.
        planes: Box<[DatumPlaneReference; 3]>,
    },
    /// One selected topological vertex.
    Vertex {
        /// Selected vertex.
        vertex: VertexSelection,
    },
    /// One selected point from a planar or model-space sketch.
    SketchPoint {
        /// Selected sketch point.
        point: SketchPointSelection,
    },
    /// Intersection of one selected edge and one selected plane.
    EdgePlaneIntersection {
        /// Selected edge.
        edge: EdgeSelection,
        /// Selected plane.
        plane: DatumPlaneReference,
    },
    /// Point at a normalized position along one selected edge.
    DistanceOnEdge {
        /// Selected edge.
        edge: EdgeSelection,
        /// Fraction from the path start in the closed interval from zero through one.
        fraction: Fraction,
    },
}

/// Rule that maps a raster decal onto its selected faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DecalMapping {
    /// Scale the complete raster to the selected faces' native parameter domain.
    FitToFaces,
}

impl DatumPointConstruction {
    /// Return construction features referenced by this rule.
    pub fn feature_references(&self) -> Vec<&FeatureId> {
        match self {
            Self::ThreePlaneIntersection { planes } => planes
                .iter()
                .filter_map(|plane| match plane {
                    DatumPlaneReference::Feature(feature) => Some(feature),
                    DatumPlaneReference::Face(_) | DatumPlaneReference::ResolvedPlane { .. } => {
                        None
                    }
                })
                .collect(),
            Self::EdgePlaneIntersection {
                plane: DatumPlaneReference::Feature(feature),
                ..
            } => vec![feature],
            Self::Vertex {
                vertex: VertexSelection::Generated { vertex, .. },
            } => vec![&vertex.feature],
            Self::CircleCenter { .. }
            | Self::TwoEdgeIntersection { .. }
            | Self::Vertex { .. }
            | Self::SketchPoint { .. }
            | Self::EdgePlaneIntersection { .. }
            | Self::DistanceOnEdge { .. } => Vec::new(),
        }
    }
}

/// Source-native feature family retained without neutral construction semantics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeFeatureKind {
    /// Fusion reference-image operation.
    Canvas,
    /// Fusion decal operation.
    Decal,
    /// Fusion draft operation.
    Draft,
    /// Fusion edge-fillet operation.
    Fillet,
    /// Fusion chamfer operation.
    Chamfer,
    /// Fusion extrusion operation.
    Extrude,
    /// Fusion direct face deletion.
    DeleteFace,
    /// Fusion surface face deletion.
    SurfaceDeleteFace,
    /// Source-native family without typed neutral handling.
    Other(String),
}

impl NativeFeatureKind {
    /// Stable source spelling carried on the CADIR wire.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Canvas => "Canvas",
            Self::Decal => "Decal",
            Self::Draft => "Draft",
            Self::Fillet => "Fillet",
            Self::Chamfer => "Chamfer",
            Self::Extrude => "Extrude",
            Self::DeleteFace => "DeleteFace",
            Self::SurfaceDeleteFace => "SurfaceDeleteFace",
            Self::Other(value) => value,
        }
    }
}

impl From<String> for NativeFeatureKind {
    fn from(value: String) -> Self {
        match value.as_str() {
            "Canvas" => Self::Canvas,
            "Decal" => Self::Decal,
            "Draft" => Self::Draft,
            "Fillet" => Self::Fillet,
            "Chamfer" => Self::Chamfer,
            "Extrude" => Self::Extrude,
            "DeleteFace" => Self::DeleteFace,
            "SurfaceDeleteFace" => Self::SurfaceDeleteFace,
            _ => Self::Other(value),
        }
    }
}

impl From<&str> for NativeFeatureKind {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

impl std::fmt::Display for NativeFeatureKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for NativeFeatureKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NativeFeatureKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(String::deserialize(deserializer)?.into())
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for NativeFeatureKind {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "NativeFeatureKind".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(generator)
    }
}

/// Guide semantics of a loft operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "path",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LoftGuidance {
    /// Ordered guide trajectories; an empty vector means unguided.
    Guides(Vec<PathRef>),
    /// Centerline to which loft sections remain normal.
    Centerline(PathRef),
}

impl Default for LoftGuidance {
    fn default() -> Self {
        Self::Guides(Vec::new())
    }
}

fn known_body_count(selection: &BodySelection) -> Option<usize> {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            Some(bodies.len())
        }
        BodySelection::Local { bodies, .. } => Some(bodies.len()),
        BodySelection::ResolvedSet { members } => Some(members.len()),
        BodySelection::Historical { bodies, .. } => Some(bodies.len()),
        BodySelection::HistoricalSet { members, .. } => Some(members.len()),
        BodySelection::HistoricalUnorderedSet { selection, .. } => Some(selection.len()),
        BodySelection::Generated { bodies, .. } => Some(bodies.len()),
        BodySelection::Unresolved | BodySelection::Native(_) | BodySelection::NativeSet(_) => None,
    }
}

/// A body selection with at least two members when membership is resolved.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SewBodySelection(BodySelection);

impl TryFrom<BodySelection> for SewBodySelection {
    type Error = &'static str;
    fn try_from(bodies: BodySelection) -> Result<Self, Self::Error> {
        if known_body_count(&bodies).is_some_and(|count| count < 2) {
            return Err("bodies must select at least two bodies for sewing");
        }
        Ok(Self(bodies))
    }
}

impl std::ops::Deref for SewBodySelection {
    type Target = BodySelection;
    fn deref(&self) -> &BodySelection {
        &self.0
    }
}

impl AsRef<BodySelection> for SewBodySelection {
    fn as_ref(&self) -> &BodySelection {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SewBodySelection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(BodySelection::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

macro_rules! selection_operands {
    ($name:ident, $wire:ident, $selection:ty, $first:ident, $second:ident, $valid:expr) => {
        /// Two admitted operand selections for one feature operation.
        #[derive(Debug, Clone, PartialEq, Serialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        pub struct $name {
            $first: $selection,
            $second: $selection,
        }

        #[derive(Deserialize)]
        struct $wire {
            $first: $selection,
            $second: $selection,
        }

        impl $name {
            /// Admit the operand arity and disjoint membership.
            pub fn new($first: $selection, $second: $selection) -> Result<Self, &'static str> {
                if !($valid)(&$first, &$second) {
                    return Err(concat!(
                        stringify!($first),
                        " and ",
                        stringify!($second),
                        " must have valid arity and disjoint membership"
                    ));
                }
                Ok(Self { $first, $second })
            }

            /// Return the first operand selection.
            pub fn $first(&self) -> &$selection {
                &self.$first
            }

            /// Return the second operand selection.
            pub fn $second(&self) -> &$selection {
                &self.$second
            }

            /// Admit both edited selections before replacing either operand.
            pub fn try_edit(
                &mut self,
                edit: impl FnOnce(&mut $selection, &mut $selection),
            ) -> Result<(), &'static str> {
                let mut first = self.$first.clone();
                let mut second = self.$second.clone();
                edit(&mut first, &mut second);
                *self = Self::new(first, second)?;
                Ok(())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let wire = $wire::deserialize(deserializer)?;
                Self::new(wire.$first, wire.$second).map_err(serde::de::Error::custom)
            }
        }
    };
}

selection_operands!(
    FaceBlendOperands,
    FaceBlendOperandsWire,
    FaceSelection,
    first_faces,
    second_faces,
    |first, second| !face_selections_overlap(first, second)
);
selection_operands!(
    ReplaceFaceOperands,
    ReplaceFaceOperandsWire,
    FaceSelection,
    targets,
    replacements,
    |first, second| !face_selections_overlap(first, second)
);
selection_operands!(
    SectionOperands,
    SectionOperandsWire,
    BodySelection,
    first,
    second,
    |first, second| !body_selections_overlap(first, second)
);
selection_operands!(
    CombineOperands,
    CombineOperandsWire,
    BodySelection,
    target,
    tools,
    |first, second| known_body_count(first).is_none_or(|count| count == 1)
        && !body_selections_overlap(first, second)
);
selection_operands!(
    TrimBodyOperands,
    TrimBodyOperandsWire,
    BodySelection,
    targets,
    tools,
    |first, second| !body_selections_overlap(first, second)
);

fn face_selections_overlap(first: &FaceSelection, second: &FaceSelection) -> bool {
    fn direct(selection: &FaceSelection) -> Option<&[crate::ids::FaceId]> {
        match selection {
            FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => {
                Some(faces.as_slice())
            }
            _ => None,
        }
    }
    fn historical(
        selection: &FaceSelection,
    ) -> Option<(
        &crate::ids::FeatureInputTopologyId,
        &[crate::ids::HistoricalFaceId],
    )> {
        match selection {
            FaceSelection::Historical { state, faces, .. } => Some((state, faces.as_slice())),
            FaceSelection::HistoricalPartial { state, faces, .. } => {
                Some((state, faces.as_slice()))
            }
            _ => None,
        }
    }
    if let Some((first, second)) = direct(first).zip(direct(second)) {
        return first.iter().any(|face| second.contains(face));
    }
    if let Some(((first_state, first), (second_state, second))) =
        historical(first).zip(historical(second))
    {
        return first_state == second_state && first.iter().any(|face| second.contains(face));
    }
    match (first, second) {
        (
            FaceSelection::Generated { faces: first, .. },
            FaceSelection::Generated { faces: second, .. },
        ) => first.iter().any(|face| second.contains(face)),
        _ => false,
    }
}

fn body_selections_overlap(first: &BodySelection, second: &BodySelection) -> bool {
    fn direct(selection: &BodySelection) -> Option<Vec<&crate::ids::BodyId>> {
        match selection {
            BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
                Some(bodies.iter().collect())
            }
            BodySelection::ResolvedSet { members } => Some(members.bodies().collect()),
            _ => None,
        }
    }
    fn historical(
        selection: &BodySelection,
    ) -> Option<(
        &crate::ids::FeatureInputTopologyId,
        Vec<&crate::ids::HistoricalBodyId>,
    )> {
        match selection {
            BodySelection::Historical { state, bodies, .. } => {
                Some((state, bodies.iter().collect()))
            }
            BodySelection::HistoricalSet { state, members } => {
                Some((state, members.bodies().collect()))
            }
            BodySelection::HistoricalUnorderedSet { state, selection } => {
                Some((state, selection.bodies().iter().collect()))
            }
            _ => None,
        }
    }
    if let Some((first, second)) = direct(first).zip(direct(second)) {
        return first.iter().any(|body| second.contains(body));
    }
    if let Some(((first_state, first), (second_state, second))) =
        historical(first).zip(historical(second))
    {
        return first_state == second_state && first.iter().any(|body| second.contains(body));
    }
    match (first, second) {
        (
            BodySelection::Generated { bodies: first, .. },
            BodySelection::Generated { bodies: second, .. },
        ) => first.iter().any(|body| second.contains(body)),
        (
            BodySelection::Local { bodies: first, .. },
            BodySelection::Local { bodies: second, .. },
        ) => first.iter().any(|body| second.contains(body)),
        _ => false,
    }
}

/// Distinct tree children and an optional active member.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TreeChildren {
    #[serde(default, skip_serializing_if = "DistinctMembers::is_empty")]
    children: DistinctMembers<FeatureId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_child: Option<FeatureId>,
}

#[derive(Deserialize)]
struct TreeChildrenWire {
    #[serde(default)]
    children: Vec<FeatureId>,
    #[serde(default)]
    active_child: Option<FeatureId>,
}

impl TreeChildren {
    /// Admit distinct children and an active identity from those children.
    pub fn new(
        children: Vec<FeatureId>,
        active_child: Option<FeatureId>,
    ) -> Result<Self, &'static str> {
        if active_child
            .as_ref()
            .is_some_and(|active| !children.contains(active))
        {
            return Err("active_child must belong to children");
        }
        Ok(Self {
            children: children
                .try_into()
                .map_err(|_| "children must be distinct")?,
            active_child,
        })
    }

    /// Whether the node has no children.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Return the active child identity.
    pub fn active_child(&self) -> &Option<FeatureId> {
        &self.active_child
    }

    /// Add a child unless it is already a member.
    pub fn insert(&mut self, child: FeatureId) {
        self.children.insert(child);
    }

    /// Select an active child from the current members.
    pub fn set_active_child(
        &mut self,
        active_child: Option<FeatureId>,
    ) -> Result<(), &'static str> {
        if active_child
            .as_ref()
            .is_some_and(|active| !self.children.contains(active))
        {
            return Err("active_child must belong to children");
        }
        self.active_child = active_child;
        Ok(())
    }
}

impl std::ops::Deref for TreeChildren {
    type Target = [FeatureId];
    fn deref(&self) -> &[FeatureId] {
        &self.children
    }
}

impl<'a> IntoIterator for &'a TreeChildren {
    type Item = &'a FeatureId;
    type IntoIter = std::slice::Iter<'a, FeatureId>;
    fn into_iter(self) -> Self::IntoIter {
        self.children.iter()
    }
}

impl<'de> Deserialize<'de> for TreeChildren {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = TreeChildrenWire::deserialize(deserializer)?;
        Self::new(wire.children, wire.active_child).map_err(serde::de::Error::custom)
    }
}

/// Neutral construction semantics, with an explicit native escape hatch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "definition", rename_all = "snake_case", deny_unknown_fields)]
pub enum FeatureDefinition {
    /// Non-modeling node retained in the ordered feature tree.
    TreeNode {
        /// Structural or presentation role of the node.
        role: FeatureTreeNodeRole,
        /// Ordered child membership and its optional active child.
        #[serde(default, skip_serializing_if = "TreeChildren::is_empty")]
        children: TreeChildren,
    },
    /// Direct-modeling session represented by its captured result bodies.
    BaseFeature {
        /// Bodies copied into the parametric timeline when the session closed.
        bodies: BodySelection,
    },
    /// Mesh geometry imported into the parametric timeline.
    MeshImport {
        /// Tessellation identities supplied by the mesh-body records.
        #[serde(deserialize_with = "deserialize_local_tessellations")]
        tessellations: SelectionMembers<String>,
    },
    /// Independent bodies introduced by a copy-and-paste operation.
    InsertBodies {
        /// Newly created body copies in source order.
        bodies: BodySelection,
    },
    /// External component occurrence introduced into an assembly timeline.
    InsertComponent {
        /// Placed occurrence created by this history operation.
        occurrence: OccurrenceId,
    },
    /// Assembly constraint introduced into the product structure.
    AssemblyJoint {
        /// Joint created by this history operation.
        joint: JointId,
    },
    /// Freeform modeling session represented by its final subdivision cages.
    Form {
        /// Ordered control cages committed by the session.
        cages: Vec<SubdId>,
    },
    /// Non-geometric thread annotation attached to a cylindrical face.
    CosmeticThread {
        /// Cylindrical face carrying the annotation.
        face: FaceSelection,
        /// Nominal thread diameter, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diameter: Option<PositiveLength>,
        /// Axial extent of the annotation, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<CosmeticThreadExtent>,
    },
    /// Raster reference image placed in model space.
    ReferenceImage {
        /// Embedded or external raster resource.
        asset: AssetId,
        /// Whether the raster is visible in the source presentation.
        #[serde(default = "default_true")]
        visible: bool,
        /// Whether image u increases toward decreasing plane-local u.
        #[serde(default)]
        mirror_u: bool,
        /// Whether image v increases toward decreasing plane-local v.
        #[serde(default)]
        mirror_v: bool,
        /// Finite image plane with perpendicular unit directions.
        frame: FeatureUnitPlaneFrame,
        /// Opposite corners with nonzero extent in both coordinates.
        bounds: FeatureImageBounds,
        /// Normalized image opacity.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        opacity: Option<Fraction>,
    },
    /// Raster image applied to model faces.
    Decal {
        /// Embedded or external raster resource.
        asset: AssetId,
        /// Faces receiving the raster image.
        faces: FaceSelection,
        /// Rule relating image coordinates to the selected faces.
        mapping: DecalMapping,
        /// Normalized image opacity, or the source format's default when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        opacity: Option<Fraction>,
    },
    /// Built-in world-origin reference plane.
    DatumPrincipalPlane {
        /// Canonical principal-plane role.
        plane: PrincipalPlane,
    },
    /// Constructed reference plane.
    DatumPlane {
        /// Finite plane with nonzero perpendicular directions.
        frame: FeatureDatumPlaneFrame,
    },
    /// Reference plane constructed through three selected vertices.
    DatumThreePointPlane {
        /// Finite plane with nonzero perpendicular directions.
        frame: FeatureDatumPlaneFrame,
        /// Construction vertices in source order.
        points: ThreePointSelection,
    },
    /// Reference plane offset from another datum plane.
    DatumOffsetPlane {
        /// Source plane or planar face, when its identity is available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<DatumPlaneReference>,
        /// Signed normal offset from the source plane.
        distance: Length,
    },
    /// Constructed reference axis.
    DatumAxis {
        /// Point on the axis in model space.
        origin: FinitePoint3,
        /// Axis direction.
        direction: FeatureDirection3,
    },
    /// Constructed reference point.
    DatumPoint {
        /// Point position in model space.
        position: FinitePoint3,
        /// Rule that derives the point from preceding construction geometry.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        construction: Option<Box<DatumPointConstruction>>,
    },
    /// Standalone model vertex constructed at one point.
    PointGeometry {
        /// Vertex position in the feature's local construction frame.
        position: FinitePoint3,
    },
    /// Straight edge between two finite points.
    LineSegment {
        /// Finite distinct endpoints.
        segment: FeatureLineSegment,
    },
    /// Circular edge over an angular interval.
    CircularArc {
        /// Finite arc geometry and directed angular span.
        arc: FeatureCircularArc,
    },
    /// Elliptic edge over an angular interval.
    EllipticArc {
        /// Finite arc geometry and directed angular span.
        arc: FeatureEllipticArc,
    },
    /// Ordered straight-edge chain.
    Polyline {
        /// Finite ordered chain and closure.
        chain: FeaturePolyline,
    },
    /// Regular planar polygon centered at the local origin.
    RegularPolygonCurve {
        /// Number of polygon sides.
        sides: PolygonSideCount,
        /// Center-to-vertex distance.
        circumradius: PositiveLength,
    },
    /// Rectangular bounded planar face in the local XY plane.
    PlanarPatch {
        /// Length along the local x-axis.
        length: PositiveLength,
        /// Width along the local y-axis.
        width: PositiveLength,
    },
    /// Faces built from an ordered set of source shapes.
    FaceFromShapes {
        /// Complete ordered source-shape selection.
        sources: BodySelection,
        /// Native face-building algorithm.
        #[serde(rename = "face_maker_class")]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
        face_maker: FaceMaker,
    },
    /// Constructed model-space coordinate system.
    DatumCoordinateSystem {
        /// Finite right-handed coordinate frame.
        frame: FeatureCoordinateFrame,
    },
    /// Rectangular solid primitive.
    Block {
        /// Ordered local x, y, and z dimensions, when resolved.
        dimensions: Option<[PositiveLength; 3]>,
        /// Local-to-model placement, when resolved.
        placement: Option<FeatureRigidPlacement>,
        /// Whether the primitive creates or combines material.
        op: BooleanOp,
    },
    /// Parametric model-space curve defined by coordinate expressions.
    EquationCurve {
        /// Coordinate expressions and parameter domain.
        curve: FeatureEquationCurve,
    },
    /// Curve produced by projecting a source path onto target faces.
    ProjectedCurve {
        /// Sketch or model-space path being projected.
        source: PathRef,
        /// Faces receiving the projected curve.
        target_faces: FaceSelection,
        /// Direction law used by the projection.
        #[serde(default)]
        direction: CurveProjectionDirection,
        /// Whether projection proceeds in both directions, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bidirectional: Option<bool>,
    },
    /// Shapes projected along a direction onto one support surface.
    ProjectOnSurface {
        /// Ordered shapes and subelements projected onto the support.
        sources: PathRef,
        /// Single support face receiving the projection.
        support_face: FaceSelection,
        /// Unit projection direction.
        direction: FeatureDirection3,
        /// Result topology retained from the projected shapes.
        mode: SurfaceProjectionMode,
        /// Normal extrusion height used to turn projected faces into solids.
        height: NonNegativeLength,
        /// Normal offset applied to the projected result.
        offset: Length,
    },
    /// Ordered chain of source paths exposed as one construction curve.
    CompositeCurve {
        /// Source segments in traversal order.
        #[serde(deserialize_with = "deserialize_local_segments")]
        segments: NonEmptyMembers<PathRef>,
        /// Whether the final segment joins the first.
        #[serde(default)]
        closed: bool,
    },
    /// Circular helix or planar spiral constructed around an axis.
    Helix {
        /// Point on the construction axis at the curve start.
        axis_origin: FinitePoint3,
        /// Construction-axis direction.
        axis_direction: FeatureDirection3,
        /// Initial radial distance from the axis.
        radius: PositiveLength,
        /// Axial or radial construction law.
        shape: HelixShape,
        /// Positive number of revolutions.
        revolutions: PositiveReal,
        /// Angular position at the curve start.
        #[serde(default)]
        start_angle: Angle,
        /// Whether angular travel is clockwise when viewed along the axis.
        clockwise: bool,
        /// Number of turns per generated curve subdivision, when requested.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        segment_turns: Option<PositiveReal>,
        /// Persisted construction algorithm generation, when selectable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        construction_style: Option<HelixConstructionStyle>,
    },
    /// Circular helix with retained native axis placement.
    HelixNativeAxis {
        /// Source-native record carrying the unresolved construction axis.
        #[serde(deserialize_with = "deserialize_local_axis_native_ref")]
        axis_native_ref: NonEmptyString,
        /// Signed total rise along the axis.
        #[serde(alias = "radius")]
        axial_rise: Length,
        /// Signed axial rise per revolution.
        #[serde(alias = "height")]
        pitch: Length,
        /// Positive number of revolutions.
        revolutions: PositiveReal,
        /// Angular position at the curve start.
        start_angle: Angle,
        /// Whether angular travel is clockwise when viewed along the axis.
        clockwise: bool,
    },
    /// Solid primitive formed by sweeping a generated section along a helix or spiral.
    Coil {
        /// Complete geometric and parametric construction definition.
        construction: CoilConstruction,
        /// Result-body semantics.
        result: CoilResult,
    },
    /// Solid sphere primitive.
    Sphere {
        /// Sphere center in model space.
        center: FinitePoint3,
        /// Positive sphere radius.
        radius: PositiveLength,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Solid torus primitive.
    Torus {
        /// Torus center in model space.
        center: FinitePoint3,
        /// Unit normal of the torus center plane.
        axis: FeatureDirection3,
        /// Positive distance from the center to the tube centerline.
        major_radius: PositiveLength,
        /// Positive tube radius.
        minor_radius: PositiveLength,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Profile mapped onto a target face.
    Wrap {
        /// Sketch or face profile mapped onto the target.
        profile: PlanarProfileRef,
        /// Face receiving the mapped profile.
        face: FaceSelection,
        /// Material or imprint operation performed by the mapping.
        mode: WrapMode,
    },
    /// Solved sketch node in the construction history.
    Sketch {
        /// Source-declared sketch space and optional decoded geometry.
        sketch: SketchFeatureBinding,
    },
    /// Solved spatial-sketch node in the construction history.
    SpatialSketch {
        /// Neutral model-space sketch geometry owned by this history node, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<crate::sketches::SpatialSketchId>,
    },
    /// Reusable planar sketch geometry.
    SketchBlockDefinition {
        /// Neutral sketch geometry owned by the block, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<crate::sketches::SketchId>,
    },
    /// Placement of one reusable sketch-block definition.
    SketchBlockInstance {
        /// Referenced block definition, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block: Option<FeatureId>,
        /// Affine placement in the owning sketch space, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placement: Option<crate::transform::Transform>,
    },
    /// Directly stored geometry with no replayable parametric construction.
    ///
    /// The feature's `outputs` identify the retained bodies when geometry is present.
    StoredGeometry {},
    /// Body geometry copied from existing bodies.
    ExtractBody {
        /// Bodies supplying the copied geometry.
        source: BodySelection,
    },
    /// Geometry copied from an earlier feature without an additional modeling operation.
    DerivedGeometry {
        /// Feature supplying the copied geometry.
        source: FeatureId,
    },
    /// Geometry imported from an external model file.
    ImportedGeometry {
        /// External source path exactly as persisted by the design.
        path: GeometryImportPath,
        /// Model format read from the external file.
        format: GeometryImportFormat,
    },
    /// Parametric analytic solid primitive.
    Primitive {
        /// Primitive dimensions and angular bounds.
        solid: PrimitiveSolid,
        /// Boolean combination with an existing `PartDesign` body.
        op: BooleanOp,
    },
    /// Linear extrusion of a profile.
    Extrude {
        /// Profile swept along `direction`.
        profile: ProfileRef,
        /// Direction in which the profile is swept and its optional persisted source.
        #[serde(default)]
        direction: ExtrudeDirection,
        /// Plane or face from which the extrusion begins.
        #[serde(default)]
        start: ExtrudeStart,
        /// How far the extrusion travels on each of its sides.
        extent: ExtrudeExtent,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
        /// Whether the result is a solid (`true`) or sheet (`false`), when selectable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        solid: Option<bool>,
        /// Native face-building policy used to turn closed wires into faces.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(with = "optional_extrusion_face_maker")]
        #[cfg_attr(feature = "schema", schemars(with = "Option<ExtrusionFaceMakerWire>"))]
        face_maker: Option<FaceMaker>,
        /// Taper orientation used for inner wires, when selectable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inner_wire_taper: Option<InnerWireTaper>,
        /// Whether stored lengths are measured along the profile normal instead of the sweep axis.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length_along_profile_normal: Option<bool>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allow_multi_profile_faces: Option<bool>,
    },
    /// Revolution of a profile around an axis.
    Revolve {
        /// Independently resolved construction inputs.
        construction: RevolveConstruction,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Sweep of a referenced or generated cross-section along a path.
    Sweep {
        /// Cross-sections and their compatible result mode.
        shape: SweepShape,
        /// Trajectory followed by the profile, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<PathRef>,
        /// Rule used to orient cross-sections along the path.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        orientation: Option<SweepOrientation>,
        /// Corner continuation used where path segments meet.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transition: Option<SweepTransition>,
        /// Interpolation law used between multiple cross-sections.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transformation: Option<SweepTransformation>,
        /// Whether tangent-connected edges are included in the primary path.
        #[serde(default)]
        path_tangent: bool,
        /// Whether linear edges and planar faces are simplified after construction.
        #[serde(default)]
        linearize: bool,
        /// Total profile twist along the path, when specified.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        twist: Option<Angle>,
        /// Fractions of the selected path swept on either side of the profile.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path_extent: Option<SweepPathExtent>,
        /// Guide rail and its independently consumed extent, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guide_rail: Option<SweepGuideRail>,
        /// Profile taper angle over the swept extent, when specified.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        taper: Option<Angle>,
        /// End-to-start profile scale ratio, when specified.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<PositiveReal>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allow_multi_profile_faces: Option<bool>,
    },
    /// Solid sweep of a profile along a parametrically defined helix or spiral.
    HelicalSweep {
        /// Complete helix path and profile construction.
        construction: HelicalSweepConstruction,
        /// Boolean combination with the existing body.
        op: BooleanOp,
    },
    /// Live or frozen reference geometry imported from other design features.
    Binder {
        /// Ordered source objects and selected subelements.
        sources: Vec<BinderSource>,
        /// Binding and derived-shape construction semantics.
        construction: BinderConstruction,
    },
    /// Loft through an ordered sequence of profile or point sections.
    Loft {
        /// Ordered cross-sections from the loft start to end.
        sections: Vec<LoftSection>,
        /// Mutually exclusive guide trajectories or centerline.
        guidance: LoftGuidance,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
        /// Whether the loft closes from the last section to the first.
        #[serde(default)]
        closed: bool,
        /// Whether the sections bound a solid instead of a sheet body.
        #[serde(default = "default_true")]
        solid: bool,
        /// Whether adjacent sections are connected by straight ruled spans.
        #[serde(default)]
        ruled: bool,
        /// Whether linear edges and planar faces are simplified after construction.
        #[serde(default)]
        linearize: bool,
        /// Maximum polynomial degree used to interpolate the sections, when constrained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_degree: Option<std::num::NonZeroU32>,
        /// Whether profiles containing multiple faces are accepted as one operation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allow_multi_profile_faces: Option<bool>,
    },
    /// Thin rib grown from a profile.
    Rib {
        /// Independently resolved construction inputs.
        construction: RibConstruction,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Planar sheet-metal body created from a closed profile.
    SheetMetalBaseFlange {
        /// Closed profile defining the planar sheet boundary.
        profile: PlanarProfileRef,
        /// Finished sheet thickness.
        thickness: PositiveLength,
        /// Distribution of thickness relative to the profile plane.
        side: SheetMetalThicknessSide,
    },
    /// Sheet-metal wall grown from selected edges of an existing sheet body.
    SheetMetalEdgeFlange {
        /// Edges the flange is grown from.
        edges: EdgeSelection,
        /// Height of the flange wall.
        height: SheetMetalFlangeHeight,
        /// Angle between the flange wall and its source face.
        angle: Angle,
        /// Face pair the height is measured from.
        height_datum: SheetMetalHeightDatum,
        /// Placement of the bend region against the selected edge.
        bend_position: SheetMetalBendPosition,
        /// Extent of the flange along the selected edge.
        width: SheetMetalFlangeWidth,
        /// Inside radius of the bend joining the flange to its source face.
        bend_radius: PositiveLength,
    },
    /// Sheet-metal hem grown from one or more selected edges.
    SheetMetalHem {
        /// Edges the hem is grown from.
        edges: EdgeSelection,
        /// Dimensional owner layout carried by the source hem.
        form: SheetMetalHemForm,
        /// Direction of the hem fold, when the source carries a proven value.
        direction: SheetMetalHemDirection,
        /// Inside radius of the bend joining the hem to its source face.
        bend_radius: PositiveLength,
    },
    /// Edge fillet.
    Fillet {
        /// Ordered edge groups and their radius laws.
        #[serde(deserialize_with = "deserialize_local_groups")]
        groups: NonEmptyMembers<FilletGroup>,
    },
    /// Full-round fillet built from a center-face selection and two side-face sets.
    FullRoundFillet {
        /// Ordered full-round face groups.
        #[serde(deserialize_with = "deserialize_local_groups")]
        groups: NonEmptyMembers<FullRoundFilletGroup>,
    },
    /// Blend constructed between two face sets.
    FaceBlend {
        /// Disjoint operand selections and their operation arity.
        operands: FaceBlendOperands,
        /// Radius law along the face intersection.
        radius: RadiusSpec,
    },
    /// Edge chamfer.
    Chamfer {
        /// Ordered edge groups and their dimensional specifications.
        #[serde(deserialize_with = "deserialize_local_groups")]
        groups: NonEmptyMembers<ChamferGroup>,
        /// Whether the dimensional reference side is reversed.
        #[serde(default)]
        flip_direction: bool,
    },
    /// Thin-wall shell operation.
    Shell {
        /// Bodies explicitly selected as shell inputs, when the source names them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bodies: Option<BodySelection>,
        /// Faces removed to open the shell.
        removed_faces: FaceSelection,
        /// Wall thickness left after shelling, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thickness: Option<PositiveLength>,
        /// Whether the wall is grown outward from the original boundary,
        /// as opposed to inward, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outward: Option<bool>,
        /// Offset construction used to generate the wall.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<ShellMode>,
        /// Corner continuation law used between offset faces.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        join: Option<ShellJoin>,
        /// Whether intersecting offset regions are resolved during construction.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolve_intersections: Option<bool>,
        /// Whether self-intersecting offset regions may be retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allow_self_intersections: Option<bool>,
    },
    /// Offsets an entire source shape without removing opening faces.
    OffsetShape {
        /// Source shape or body to offset.
        source: BodySelection,
        /// Signed normal offset in canonical millimeters.
        distance: NonZeroLength,
        /// Offset construction mode.
        mode: ShellMode,
        /// Corner continuation law.
        join: ShellJoin,
        /// Whether intersecting regions are resolved.
        resolve_intersections: bool,
        /// Whether self-intersecting regions may be retained.
        allow_self_intersections: bool,
        /// Whether open offset boundaries are filled.
        fill: bool,
        /// Whether planar two-dimensional offset rules are used.
        planar: bool,
    },
    /// Builds one compound topology node from ordered source shapes.
    Compound {
        /// Ordered source members retained as a native or resolved selection.
        members: BodySelection,
    },
    /// Removes redundant splitter topology from a source shape.
    RefineShape {
        /// Source shape whose coincident boundaries are simplified.
        source: BodySelection,
    },
    /// Reverses the topological orientation of a source shape.
    ReverseShape {
        /// Source shape whose complete orientation is reversed.
        source: BodySelection,
    },
    /// Ruled sheet connecting two ordered boundary curves.
    RuledBetweenCurves {
        /// First source boundary.
        first: PathRef,
        /// Second source boundary.
        second: PathRef,
        /// Traversal relationship between the two boundaries.
        orientation: RuledCurveOrientation,
    },
    /// Intersection curves produced where two source shapes meet.
    SectionShape {
        /// Disjoint operand selections and their operation arity.
        operands: SectionOperands,
        /// Whether the resulting section edges are approximated, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approximate: Option<bool>,
    },
    /// Reflects one source shape across a model-space plane.
    MirrorShape {
        /// Shape transformed into the mirrored result.
        source: BodySelection,
        /// Point on the persisted resolved mirror plane.
        plane_origin: FinitePoint3,
        /// Unit normal of the persisted resolved mirror plane.
        plane_normal: FeatureDirection3,
        /// Native plane, face, or circle reference that supplied the resolved plane.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plane_reference: Option<FaceSelection>,
    },
    /// Adds material normal to selected faces.
    Thicken {
        /// Faces offset by the operation.
        faces: FaceSelection,
        /// Finished added thickness, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thickness: Option<PositiveLength>,
        /// Distribution of thickness relative to the selected faces, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        side: Option<ThickenSide>,
    },
    /// Surface copied at a signed normal offset from selected support faces.
    OffsetSurface {
        /// Faces supplying the source surface geometry.
        faces: FaceSelection,
        /// Signed normal offset in canonical millimeters.
        distance: Option<Length>,
    },
    /// Joins selected surface bodies along coincident or near-coincident boundaries.
    KnitSurface {
        /// Faces participating in the knit operation.
        faces: FaceSelection,
        /// Whether coincident face and edge entities are merged.
        merge_entities: Option<bool>,
        /// Whether a closed result is converted to a solid body.
        create_solid: Option<bool>,
        /// Maximum boundary gap accepted by the operation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap_tolerance: Option<NonNegativeLength>,
    },
    /// Joins sheet or solid bodies along coincident boundaries.
    SewBodies {
        /// Bodies participating in the sew operation.
        bodies: SewBodySelection,
        /// Maximum accepted boundary gap, when resolved.
        gap_tolerance: Option<PositiveLength>,
    },
    /// Surface patch spanning a selected edge boundary.
    FilledSurface {
        /// Closed boundary of the generated patch.
        boundary: SurfaceBoundary,
        /// Adjacent faces supplying tangent or curvature conditions.
        support_faces: FaceSelection,
        /// Uniform, component-specific, or unresolved boundary continuity.
        #[serde(
            default = "FilledSurfaceContinuityState::unresolved",
            skip_serializing_if = "FilledSurfaceContinuityState::is_unresolved"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "FilledSurfaceContinuityWire"))]
        continuity: FilledSurfaceContinuityState,
        /// Whether the generated patch is merged into adjacent surface bodies,
        /// when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        merge_result: Option<bool>,
    },
    /// Restricts selected surface faces to one side of a trimming path.
    TrimSurface {
        /// Surface faces modified by the operation.
        faces: FaceSelection,
        /// Sketch or model-space path defining the trim boundary.
        tool: PathRef,
        /// Region or explicit partition-cell set retained after trimming.
        keep: TrimRegion,
    },
    /// Extends selected surface boundaries by a fixed distance.
    ExtendSurface {
        /// Surface faces whose boundaries are extended.
        faces: FaceSelection,
        /// Positive extension distance in canonical millimeters.
        distance: Option<PositiveLength>,
        /// Geometric continuation law.
        method: SurfaceExtension,
    },
    /// Ruled surface grown from selected boundary edges.
    RuledSurface {
        /// Boundary edges from which the surface is generated.
        edges: EdgeSelection,
        /// Adjacent faces supplying normal or tangent context.
        support_faces: FaceSelection,
        /// Direction law and extension distance.
        mode: RuledSurfaceMode,
        /// Signed rotation from the selected ruled-surface law.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<Angle>,
        /// Whether the opposite incident face supplies the angle reference.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alternate_face: Option<bool>,
        /// Boundary-corner construction law.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        corner: Option<RuledSurfaceCorner>,
    },
    /// Taper applied to selected faces about a neutral plane.
    Draft {
        /// Faces whose angle is modified.
        faces: FaceSelection,
        /// Structurally selected anchor and pull frame.
        anchor: DraftAnchor,
        /// Signed draft angle.
        angle: Option<SlopeAngle>,
        /// Whether material is added away from the pull direction.
        outward: Option<bool>,
    },
    /// Boolean operation between existing bodies.
    Combine {
        /// Disjoint operand selections and their operation arity.
        operands: CombineOperands,
        /// Join, cut, or intersection operation.
        op: BooleanKind,
        /// Whether tool bodies remain present after the Boolean result is created.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        keep_tools: bool,
    },
    /// Creates solid bodies from selected cells enclosed by boundary bodies.
    BoundaryFill {
        /// Bodies whose faces partition space into candidate cells.
        tools: BodySelection,
        /// Enclosed cells retained as result bodies, in source order.
        #[serde(deserialize_with = "deserialize_local_cells")]
        cells: NonEmptyMembers<BodySelection>,
    },
    /// Removes one side of selected bodies using selected surface faces.
    CutWithSurface {
        /// Bodies cut by the operation.
        targets: BodySelection,
        /// Oriented surface faces defining the cut.
        tools: FaceSelection,
        /// Whether the side opposite the default tool orientation is removed,
        /// when the native side flag is present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reverse: Option<bool>,
    },
    /// Removes one side of target bodies using ordered tool bodies.
    TrimBodies {
        /// Disjoint operand selections and their operation arity.
        operands: TrimBodyOperands,
        /// Side retained by the trim.
        keep: BodyTrimSide,
    },
    /// Partitions selected bodies with selected surface faces while retaining
    /// every resulting side.
    SplitBody {
        /// Bodies partitioned by the operation.
        targets: BodySelection,
        /// Surface faces extended as necessary to partition the targets.
        tools: FaceSelection,
    },
    /// Partitions selected faces along selected sketch curves while retaining
    /// every resulting face region.
    SplitFace {
        /// Faces partitioned by the operation.
        targets: FaceSelection,
        /// Geometry or datum plane that partitions the target faces.
        tool: SplitFaceTool,
    },
    /// Deletes bodies directly or retains only the selected bodies.
    DeleteBody {
        /// Bodies selected by the operation.
        bodies: BodySelection,
        /// Whether selected bodies are deleted or retained.
        mode: BodyRetentionMode,
    },
    /// Removal of selected faces from an existing body.
    DeleteFace {
        /// Faces removed by the operation.
        faces: FaceSelection,
        /// Whether adjacent faces extend to heal the resulting boundary.
        heal: bool,
    },
    /// Replaces selected faces with another face set.
    ReplaceFace {
        /// Disjoint operand selections and their operation arity.
        operands: ReplaceFaceOperands,
    },
    /// Direct motion of selected faces.
    MoveFace {
        /// Faces modified by the operation.
        faces: FaceSelection,
        /// Motion applied to the selected faces.
        motion: FaceMotion,
    },
    /// Rigid translation or rotation of selected bodies, optionally creating copies.
    MoveBody {
        /// Bodies transformed by the operation.
        bodies: BodySelection,
        /// Model-space translation vector in canonical millimeters.
        translation: FiniteVector3,
        /// Axis-angle rotation applied with the translation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rotation: Option<AxisAngle>,
        /// Number of transformed copies; zero moves the selected bodies.
        #[serde(default)]
        copies: u32,
    },
    /// Dome grown from selected planar faces.
    Dome {
        /// Faces that bound the dome base.
        faces: FaceSelection,
        /// Dome height measured normal to the base, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<PositiveLength>,
        /// Whether the profile is elliptical rather than spherical, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        elliptical: Option<bool>,
        /// Whether growth opposes the selected-face normal, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reverse: Option<bool>,
    },
    /// Deformation of existing geometry about a feature axis.
    Flex {
        /// Flex axis direction in model space, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        axis: Option<FeatureDirection3>,
        /// Applied deformation mode and magnitude.
        #[cfg_attr(feature = "schema", schemars(with = "FlexModeWire"))]
        mode: FlexMode,
    },
    /// Scales selected bodies about a model-space point.
    Scale {
        /// Bodies transformed by the operation.
        bodies: BodySelection,
        /// Fixed locus of the scale transform.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        center: Option<ScaleCenter>,
        /// Uniform, per-axis, or unresolved scale factors.
        #[cfg_attr(feature = "schema", schemars(with = "ScaleFactorsWire"))]
        factors: ScaleFactors,
    },
    /// Drilled or machined hole.
    Hole {
        /// Sketch or profile supplying one or more hole locations.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<PlanarProfileRef>,
        /// Geometry families in the profile that generate hole locations.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile_filter: Option<HoleProfileFilter>,
        /// Face the hole is placed on, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<FaceSelection>,
        /// Drilling direction carried independently of complete placements.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
        /// Complete one-or-many hole placements, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placements: Option<Vec<HolePlacement>>,
        /// Bore diameter and compatible entry, exit, and thread construction.
        shape: HoleShape,
        /// How deep the hole extends, when resolved. Holes travel on one side
        /// only, so the termination law needs no sidedness wrapper.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<LinearTermination>,
        /// Shape and depth convention at the blind end of the hole.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bottom: Option<HoleBottom>,
        /// Included taper angle for a conical hole, when enabled.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        taper_angle: Option<InteriorAngle>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allow_multi_profile_faces: Option<bool>,
    },
    /// Repetition or reflection of existing features.
    Pattern {
        /// Geometry being repeated or reflected; empty when the source selection is unresolved.
        seeds: Vec<PatternSeed>,
        /// Spatial transform defining the repetition or reflection.
        pattern: PatternKind,
    },
    /// Operation followed by source-requested topology cleanup.
    PostProcess {
        /// Underlying construction whose result is post-processed.
        operation: UnprocessedFeature,
        /// Whether redundant splitter boundaries are removed.
        refine: bool,
        /// Boolean-operation tolerance selection carried by the feature family.
        fuzzy_tolerance: FuzzyTolerance,
    },
    /// Operation family established without its construction operands.
    Unresolved {
        /// Operation family of the unresolved feature.
        family: UnresolvedFamily,
    },
    /// Source-native operation without neutral semantics.
    Native {
        /// Native feature-type tag (e.g. `"Extrude"`, `"Fillet"`).
        kind: NativeFeatureKind,
        /// Source parametric input values keyed by parameter name.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        parameters: BTreeMap<String, String>,
    },
}

/// Operation family of a feature whose construction operands are unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum UnresolvedFamily {
    /// The `datum_plane` operation family.
    DatumPlane,
    /// The `datum_axis` operation family.
    DatumAxis,
    /// The `datum_point` operation family.
    DatumPoint,
    /// The `datum_coordinate_system` operation family.
    DatumCoordinateSystem,
    /// The `bridge_curve` operation family.
    BridgeCurve,
    /// The `brep` operation family.
    Brep,
    /// The `cylinder` operation family.
    Cylinder,
    /// The `cone` operation family.
    Cone,
    /// The `sphere` operation family.
    Sphere,
    /// The `thread` operation family.
    Thread,
    /// The `detailed_thread` operation family.
    DetailedThread,
    /// The `loft` operation family.
    Loft,
    /// The `through_curve_mesh` operation family.
    ThroughCurveMesh,
    /// The `freeform_surface` operation family.
    FreeformSurface,
    /// The `extrude` operation family.
    Extrude,
    /// The `revolve` operation family.
    Revolve,
    /// The `fillet` operation family.
    Fillet,
    /// The `extract_face` operation family.
    ExtractFace,
    /// The `copy_face` operation family.
    CopyFace,
    /// The `linked_face` operation family.
    LinkedFace,
    /// The `fill_hole` operation family.
    FillHole,
    /// The `boundary_surface` operation family.
    BoundarySurface,
    /// The `draft` operation family.
    Draft,
    /// The `delete_face` operation family.
    DeleteFace,
    /// The `move_face` operation family.
    MoveFace,
    /// The `mirror_face` operation family.
    MirrorFace,
    /// The `subdivision_body` operation family.
    SubdivisionBody,
    /// The `topology_optimization` operation family.
    TopologyOptimization,
    /// The `move_object` operation family.
    MoveObject,
}

impl FeatureDefinition {
    /// Family name of a definition whose replay produces body geometry, and `None` for
    /// definitions that do not.
    ///
    /// A feature of one of these families carries current result bodies in its
    /// [`Feature::outputs`] list or intermediate result identities in a
    /// [`FeatureResultTopology`]. The returned name is the stable lowercase label for the
    /// family, suitable for grouping such features in a report.
    pub fn body_output_family(&self) -> Option<&'static str> {
        match self {
            Self::BaseFeature { .. } => Some("base feature"),
            Self::Unresolved { family } => match family {
                UnresolvedFamily::Brep => Some("brep"),
                UnresolvedFamily::ThroughCurveMesh => Some("through curve mesh"),
                UnresolvedFamily::ExtractFace => Some("extract face"),
                UnresolvedFamily::CopyFace => Some("copy face"),
                UnresolvedFamily::LinkedFace => Some("linked face"),
                UnresolvedFamily::FillHole => Some("fill hole"),
                UnresolvedFamily::MoveFace => Some("move face"),
                UnresolvedFamily::MirrorFace => Some("mirror face"),
                UnresolvedFamily::SubdivisionBody => Some("subdivision body"),
                UnresolvedFamily::TopologyOptimization => Some("topology optimization"),
                UnresolvedFamily::MoveObject => Some("move object"),
                UnresolvedFamily::Cylinder => Some("cylinder"),
                UnresolvedFamily::Cone => Some("cone"),
                UnresolvedFamily::Sphere => Some("sphere"),
                UnresolvedFamily::Thread => Some("thread"),
                UnresolvedFamily::DetailedThread => Some("detailed thread"),
                UnresolvedFamily::Extrude => Some("extrude"),
                UnresolvedFamily::Revolve => Some("revolve"),
                UnresolvedFamily::Fillet => Some("fillet"),
                UnresolvedFamily::DeleteFace => Some("delete face"),
                UnresolvedFamily::DatumPlane
                | UnresolvedFamily::DatumAxis
                | UnresolvedFamily::DatumPoint
                | UnresolvedFamily::DatumCoordinateSystem
                | UnresolvedFamily::BridgeCurve
                | UnresolvedFamily::Loft
                | UnresolvedFamily::FreeformSurface
                | UnresolvedFamily::BoundarySurface
                | UnresolvedFamily::Draft => None,
            },
            Self::Block { .. } => Some("block"),
            Self::Sphere { .. } => Some("sphere"),
            Self::ExtractBody { .. } => Some("extract body"),
            Self::Loft { .. } => Some("loft"),
            Self::TrimSurface { .. } => Some("trim surface"),
            Self::ExtendSurface { .. } => Some("extend surface"),
            Self::RuledSurface { .. } => Some("ruled surface"),
            Self::Hole { .. } => Some("hole"),
            Self::Rib { .. } => Some("rib"),
            Self::Chamfer { .. } => Some("chamfer"),
            Self::Fillet { .. } => Some("fillet"),
            Self::FullRoundFillet { .. } => Some("fillet"),
            Self::FaceBlend { .. } => Some("face blend"),
            Self::Shell { .. } => Some("shell"),
            Self::SewBodies { .. } => Some("sew bodies"),
            Self::TrimBodies { .. } => Some("trim bodies"),
            Self::Extrude { .. } => Some("extrude"),
            Self::Revolve { .. } => Some("revolve"),
            Self::Sweep { .. } => Some("sweep"),
            Self::OffsetSurface { .. } => Some("offset surface"),
            Self::Thicken { .. } => Some("thicken"),
            Self::Draft { .. } => Some("draft"),
            Self::Pattern { .. } => Some("pattern"),
            Self::Combine { .. } => Some("body combine"),
            Self::ReplaceFace { .. } => Some("replace face"),
            Self::DeleteFace { .. } => Some("delete face"),
            _ => None,
        }
    }

    /// Operation family of an unresolved definition, and `None` when it is resolved.
    pub fn unresolved_family(&self) -> Option<UnresolvedFamily> {
        match self {
            Self::Unresolved { family } => Some(*family),
            _ => None,
        }
    }
}

/// Direction in which an extrusion sweeps its profile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtrudeDirection {
    /// Native direction selection is present structurally but unresolved.
    Unresolved {},
    /// Sweep along the profile's positive normal.
    ProfileNormal {},
    /// Sweep opposite the profile's positive normal.
    ReversedProfileNormal {},
    /// Sweep along an explicit model-space vector.
    Explicit {
        /// Directed model-space sweep vector.
        vector: FeatureDirection3,
        /// Persisted selection or rule used to resolve the vector, when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<ExtrusionDirectionSource>,
    },
}

impl Default for ExtrudeDirection {
    fn default() -> Self {
        Self::ProfileNormal {}
    }
}

/// One complete spatial placement in a hole operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HolePlacement {
    /// Position and directed drilling vector recorded by the feature definition.
    Directed {
        /// Hole entry position in model space.
        position: FinitePoint3,
        /// Directed drilling vector.
        direction: FeatureDirection3,
    },
    /// Unoriented geometric axis inferred from a generated cylindrical surface.
    Axis {
        /// Point on the cylinder axis in model space.
        origin: FinitePoint3,
        /// Unoriented cylinder-axis vector; its sign has no semantic meaning.
        axis: FeatureDirection3,
    },
}

/// One geometric selection repeated or reflected by a pattern operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PatternSeed {
    /// Complete result of a preceding construction-history feature.
    Feature(FeatureId),
    /// Selected faces, including faces in an intermediate regenerated result.
    Faces(FaceSelection),
    /// Selected bodies, including bodies in an intermediate regenerated result.
    Bodies(BodySelection),
    /// Selected placed component occurrences.
    Occurrences(
        #[serde(deserialize_with = "deserialize_local_occurrences")] SelectionMembers<OccurrenceId>,
    ),
}

/// External model format consumed by an imported-geometry feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum GeometryImportFormat {
    /// ISO 10303 STEP model data.
    Step,
    /// IGES model data.
    Iges,
    /// Native boundary-representation model data.
    Brep,
}

/// Selection policy for Boolean-operation fuzzy tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FuzzyTolerance {
    /// Let the modeling kernel use its default tolerance.
    KernelDefault,
    /// Determine a suitable tolerance from the participating shapes.
    Automatic,
    /// Use the supplied positive model-unit tolerance.
    Explicit(PositiveLength),
}

const fn default_true() -> bool {
    true
}

/// Geometric offset construction used by a thin-wall shell operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ShellMode {
    /// Offsets the selected boundary as a skin.
    Skin,
    /// Extends the offset along boundary edges as a pipe-like wall.
    Pipe,
    /// Builds wall material on both sides of the original boundary.
    BothSides,
}

/// Corner continuation law for adjacent shell offset faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ShellJoin {
    /// Continues corners with rounded arcs.
    Arc,
    /// Extends adjacent faces tangentially to meet.
    Tangent,
    /// Intersects adjacent offset faces to form sharp corners.
    Intersection,
}

/// Traversal relationship between ruled-surface boundary curves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RuledCurveOrientation {
    /// Select curve traversal automatically from endpoint proximity.
    Automatic,
    /// Retain both persisted curve traversal directions.
    Forward,
    /// Reverse the second curve relative to the first.
    Reversed,
}

/// An analytic solid primitive with valid dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PrimitiveSolidKind", into = "PrimitiveSolidKind")]
pub struct PrimitiveSolid(PrimitiveSolidKind);

impl PrimitiveSolid {
    /// Admits dimensions that define a solid primitive.
    pub fn new(kind: PrimitiveSolidKind) -> Result<Self, &'static str> {
        let positive = |value: Length| value.get() > 0.0;
        let valid = match &kind {
            PrimitiveSolidKind::Box {
                length,
                width,
                height,
            } => positive(*length) && positive(*width) && positive(*height),
            PrimitiveSolidKind::Cylinder { radius, height, .. } => {
                positive(*radius) && positive(*height)
            }
            PrimitiveSolidKind::Cone {
                radius1,
                radius2,
                height,
                ..
            } => {
                radius1.get() >= 0.0
                    && radius2.get() >= 0.0
                    && (positive(*radius1) || positive(*radius2))
                    && positive(*height)
            }
            PrimitiveSolidKind::Sphere {
                radius,
                latitude1,
                latitude2,
                ..
            } => positive(*radius) && latitude1.get() < latitude2.get(),
            PrimitiveSolidKind::Ellipsoid {
                x_radius,
                y_radius,
                z_radius,
                latitude1,
                latitude2,
                ..
            } => {
                positive(*x_radius)
                    && positive(*y_radius)
                    && positive(*z_radius)
                    && latitude1.get() < latitude2.get()
            }
            PrimitiveSolidKind::Torus {
                major_radius,
                minor_radius,
                latitude1,
                latitude2,
                ..
            } => {
                positive(*major_radius)
                    && positive(*minor_radius)
                    && latitude1.get() < latitude2.get()
            }
            PrimitiveSolidKind::Prism {
                sides,
                circumradius,
                height,
            } => *sides >= 3 && positive(*circumradius) && positive(*height),
            PrimitiveSolidKind::Wedge {
                xmin,
                ymin,
                zmin,
                x2min,
                z2min,
                xmax,
                ymax,
                zmax,
                x2max,
                z2max,
            } => {
                xmax.get() > xmin.get()
                    && ymax.get() > ymin.get()
                    && zmax.get() > zmin.get()
                    && x2max.get() >= x2min.get()
                    && z2max.get() >= z2min.get()
            }
        };
        valid
            .then_some(Self(kind))
            .ok_or("primitive dimensions are invalid")
    }

    /// Returns the primitive form and dimensions.
    pub fn kind(&self) -> &PrimitiveSolidKind {
        &self.0
    }
}

impl TryFrom<PrimitiveSolidKind> for PrimitiveSolid {
    type Error = &'static str;

    fn try_from(kind: PrimitiveSolidKind) -> Result<Self, Self::Error> {
        Self::new(kind)
    }
}

impl From<PrimitiveSolid> for PrimitiveSolidKind {
    fn from(solid: PrimitiveSolid) -> Self {
        solid.0
    }
}

/// Canonical dimensions of an analytic solid primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PrimitiveSolidKind {
    /// Rectangular solid aligned to its feature frame.
    Box {
        /// Size along the local x-axis.
        length: Length,
        /// Size along the local y-axis.
        width: Length,
        /// Size along the local z-axis.
        height: Length,
    },
    /// Circular cylinder aligned to its feature-frame z-axis.
    Cylinder {
        /// Circular base radius.
        radius: Length,
        /// Axial height.
        height: Length,
        /// Angular sweep around the axis.
        angle: Angle,
    },
    /// Circular cone or frustum aligned to its feature-frame z-axis.
    Cone {
        /// Radius at the local-frame origin.
        radius1: Length,
        /// Radius at the opposite end.
        radius2: Length,
        /// Axial height.
        height: Length,
        /// Angular sweep around the axis.
        angle: Angle,
    },
    /// Spherical segment.
    Sphere {
        /// Sphere radius.
        radius: Length,
        /// Lower latitude bound.
        latitude1: Angle,
        /// Upper latitude bound.
        latitude2: Angle,
        /// Longitudinal sweep.
        longitude: Angle,
    },
    /// Ellipsoidal segment aligned to its feature frame.
    Ellipsoid {
        /// Radius along local x.
        x_radius: Length,
        /// Radius along local y.
        y_radius: Length,
        /// Radius along local z.
        z_radius: Length,
        /// Lower latitude bound.
        latitude1: Angle,
        /// Upper latitude bound.
        latitude2: Angle,
        /// Longitudinal sweep.
        longitude: Angle,
    },
    /// Toroidal segment aligned to its feature frame.
    Torus {
        /// Distance from the axis to the tube center.
        major_radius: Length,
        /// Tube radius.
        minor_radius: Length,
        /// Lower tube-angle bound.
        latitude1: Angle,
        /// Upper tube-angle bound.
        latitude2: Angle,
        /// Sweep around the torus axis.
        longitude: Angle,
    },
    /// Regular polygonal prism aligned to its feature frame.
    Prism {
        /// Number of polygon sides.
        sides: u32,
        /// Distance from polygon center to each vertex.
        circumradius: Length,
        /// Axial height.
        height: Length,
    },
    /// General wedge defined by two x-z profiles across a y interval.
    Wedge {
        /// Lower x bound.
        xmin: Length,
        /// Lower y bound.
        ymin: Length,
        /// Lower z bound.
        zmin: Length,
        /// Inner x coordinate on the lower-y profile.
        x2min: Length,
        /// Inner z coordinate on the lower-y profile.
        z2min: Length,
        /// Upper x bound.
        xmax: Length,
        /// Upper y bound.
        ymax: Length,
        /// Upper z bound.
        zmax: Length,
        /// Inner x coordinate on the upper-y profile.
        x2max: Length,
        /// Inner z coordinate on the upper-y profile.
        z2max: Length,
    },
}

/// Resolution state and inputs of a profile revolution.
#[derive(Debug, Clone, PartialEq)]
pub enum RevolveConstruction {
    /// One or more required construction inputs are absent.
    Unresolved(PartialRevolveConstruction),
    /// All required construction inputs are present.
    Resolved {
        /// Profile revolved about the axis.
        profile: PlanarProfileRef,
        /// Placed revolution axis and its optional native source.
        axis: RevolutionAxis,
        /// Angular extent.
        extent: RevolveExtent,
        /// Whether a standalone revolution creates a solid rather than a sheet.
        solid: Option<bool>,
        /// Face-building algorithm used for a standalone solid revolution.
        face_maker: Option<FaceMaker>,
        /// Compatibility ordering for fusing a `PartDesign` revolution into its body.
        fuse_order: Option<RevolutionFuseOrder>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        allow_multi_profile_faces: Option<bool>,
    },
}

/// Incomplete inputs of a profile revolution.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialRevolveConstruction {
    profile: Option<PlanarProfileRef>,
    axis: Option<RevolutionAxis>,
    extent: Option<RevolveExtent>,
    solid: Option<bool>,
    face_maker: Option<FaceMaker>,
    fuse_order: Option<RevolutionFuseOrder>,
    allow_multi_profile_faces: Option<bool>,
}

#[derive(Clone)]
struct RevolveConstructionComponents {
    profile: Option<PlanarProfileRef>,
    axis: Option<RevolutionAxis>,
    extent: Option<RevolveExtent>,
    solid: Option<bool>,
    face_maker: Option<FaceMaker>,
    fuse_order: Option<RevolutionFuseOrder>,
    allow_multi_profile_faces: Option<bool>,
}

impl RevolveConstruction {
    /// Constructs the resolved or partial form from independently decoded inputs.
    #[allow(
        clippy::too_many_arguments,
        reason = "the arguments are the complete legacy revolution record"
    )]
    pub fn new(
        profile: Option<PlanarProfileRef>,
        axis: Option<RevolutionAxis>,
        extent: Option<RevolveExtent>,
        solid: Option<bool>,
        face_maker: Option<FaceMaker>,
        fuse_order: Option<RevolutionFuseOrder>,
        allow_multi_profile_faces: Option<bool>,
    ) -> Self {
        Self::from_components(RevolveConstructionComponents {
            profile,
            axis,
            extent,
            solid,
            face_maker,
            fuse_order,
            allow_multi_profile_faces,
        })
    }

    fn from_components(components: RevolveConstructionComponents) -> Self {
        let RevolveConstructionComponents {
            profile,
            axis,
            extent,
            solid,
            face_maker,
            fuse_order,
            allow_multi_profile_faces,
        } = components;
        match (profile, axis, extent) {
            (Some(profile), Some(axis), Some(extent)) => Self::Resolved {
                profile,
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            },
            (profile, axis, extent) => Self::Unresolved(PartialRevolveConstruction {
                profile,
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            }),
        }
    }

    fn components(&self) -> RevolveConstructionComponents {
        match self {
            Self::Unresolved(partial) => RevolveConstructionComponents {
                profile: partial.profile.clone(),
                axis: partial.axis.clone(),
                extent: partial.extent.clone(),
                solid: partial.solid,
                face_maker: partial.face_maker.clone(),
                fuse_order: partial.fuse_order,
                allow_multi_profile_faces: partial.allow_multi_profile_faces,
            },
            Self::Resolved {
                profile,
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            } => RevolveConstructionComponents {
                profile: Some(profile.clone()),
                axis: Some(axis.clone()),
                extent: Some(extent.clone()),
                solid: *solid,
                face_maker: face_maker.clone(),
                fuse_order: *fuse_order,
                allow_multi_profile_faces: *allow_multi_profile_faces,
            },
        }
    }

    fn update(&mut self, update: impl FnOnce(&mut RevolveConstructionComponents)) {
        let mut components = self.components();
        update(&mut components);
        *self = Self::from_components(components);
    }

    /// Returns whether all required construction inputs are present.
    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved { .. })
    }

    /// Returns the profile, when decoded.
    pub fn profile(&self) -> Option<&PlanarProfileRef> {
        match self {
            Self::Unresolved(partial) => partial.profile.as_ref(),
            Self::Resolved { profile, .. } => Some(profile),
        }
    }

    /// Returns the mutable profile, when decoded.
    pub fn profile_mut(&mut self) -> Option<&mut PlanarProfileRef> {
        match self {
            Self::Unresolved(partial) => partial.profile.as_mut(),
            Self::Resolved { profile, .. } => Some(profile),
        }
    }

    /// Replaces the decoded profile and updates the resolution state.
    pub fn set_profile(&mut self, profile: Option<PlanarProfileRef>) {
        self.update(|components| components.profile = profile);
    }

    /// Returns the placed axis, when decoded.
    pub fn axis(&self) -> Option<&RevolutionAxis> {
        match self {
            Self::Unresolved(partial) => partial.axis.as_ref(),
            Self::Resolved { axis, .. } => Some(axis),
        }
    }

    /// Returns the mutable placed axis, when decoded.
    pub fn axis_mut(&mut self) -> Option<&mut RevolutionAxis> {
        match self {
            Self::Unresolved(partial) => partial.axis.as_mut(),
            Self::Resolved { axis, .. } => Some(axis),
        }
    }

    /// Replaces the placed axis and updates the resolution state.
    pub fn set_axis(&mut self, axis: Option<RevolutionAxis>) {
        self.update(|components| components.axis = axis);
    }

    /// Returns the angular extent, when decoded.
    pub fn extent(&self) -> Option<&RevolveExtent> {
        match self {
            Self::Unresolved(partial) => partial.extent.as_ref(),
            Self::Resolved { extent, .. } => Some(extent),
        }
    }

    /// Returns the mutable angular extent, when decoded.
    pub fn extent_mut(&mut self) -> Option<&mut RevolveExtent> {
        match self {
            Self::Unresolved(partial) => partial.extent.as_mut(),
            Self::Resolved { extent, .. } => Some(extent),
        }
    }

    /// Replaces the angular extent and updates the resolution state.
    pub fn set_extent(&mut self, extent: Option<RevolveExtent>) {
        self.update(|components| components.extent = extent);
    }

    /// Returns the standalone solid selection, when carried.
    pub const fn solid(&self) -> Option<bool> {
        match self {
            Self::Unresolved(partial) => partial.solid,
            Self::Resolved { solid, .. } => *solid,
        }
    }

    /// Replaces the standalone solid selection.
    pub fn set_solid(&mut self, solid: Option<bool>) {
        self.update(|components| components.solid = solid);
    }

    /// Returns the standalone face-maker selection, when carried.
    pub fn face_maker(&self) -> Option<&FaceMaker> {
        match self {
            Self::Unresolved(partial) => partial.face_maker.as_ref(),
            Self::Resolved { face_maker, .. } => face_maker.as_ref(),
        }
    }

    /// Returns the `PartDesign` fuse ordering, when carried.
    pub const fn fuse_order(&self) -> Option<RevolutionFuseOrder> {
        match self {
            Self::Unresolved(partial) => partial.fuse_order,
            Self::Resolved { fuse_order, .. } => *fuse_order,
        }
    }

    /// Returns the multi-profile-face selection, when carried.
    pub const fn allow_multi_profile_faces(&self) -> Option<bool> {
        match self {
            Self::Unresolved(partial) => partial.allow_multi_profile_faces,
            Self::Resolved {
                allow_multi_profile_faces,
                ..
            } => *allow_multi_profile_faces,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct RevolveConstructionWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<PlanarProfileRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    axis: Option<RevolutionAxis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extent: Option<RevolveExtent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    axis_reference: Option<PathRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    solid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "face_maker_class")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    face_maker: Option<FaceMaker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fuse_order: Option<RevolutionFuseOrder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allow_multi_profile_faces: Option<bool>,
}

impl Serialize for RevolveConstruction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let components = self.components();
        let axis_reference = components
            .axis
            .as_ref()
            .and_then(|axis| axis.reference.clone());
        RevolveConstructionWire {
            profile: components.profile,
            axis: components.axis,
            extent: components.extent,
            axis_reference,
            solid: components.solid,
            face_maker: components.face_maker,
            fuse_order: components.fuse_order,
            allow_multi_profile_faces: components.allow_multi_profile_faces,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RevolveConstruction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let RevolveConstructionWire {
            profile,
            mut axis,
            extent,
            axis_reference,
            solid,
            face_maker,
            fuse_order,
            allow_multi_profile_faces,
        } = RevolveConstructionWire::deserialize(deserializer)?;
        if let Some(reference) = axis_reference {
            let Some(axis) = axis.as_mut() else {
                return Err(serde::de::Error::custom(
                    "axis_reference requires a revolution axis",
                ));
            };
            axis.reference = Some(reference);
        }
        Ok(Self::new(
            profile,
            axis,
            extent,
            solid,
            face_maker,
            fuse_order,
            allow_multi_profile_faces,
        ))
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for RevolveConstruction {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "RevolveConstruction".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        RevolveConstructionWire::json_schema(generator)
    }
}

/// Operand ordering used to fuse a `PartDesign` revolution result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RevolutionFuseOrder {
    /// Existing body is the first fuse operand.
    BaseFirst,
    /// Newly revolved feature is the first fuse operand.
    FeatureFirst,
}

/// Complete line placement used as a revolution axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RevolutionAxis {
    /// A point on the axis.
    pub origin: FinitePoint3,
    /// Unit axis direction.
    pub direction: FeatureDirection3,
    /// Native edge, datum, or sketch-axis selection used to resolve the axis.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub reference: Option<PathRef>,
}

/// Independently decoded inputs of a thin rib operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RibConstruction {
    /// Rib centerline or open profile, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<PlanarProfileRef>,
    /// Rib growth direction, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<FeatureDirection3>,
    /// Finished rib thickness, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<PositiveLength>,
    /// Distribution of thickness around the profile, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<RibSide>,
    /// Draft state applied to the rib walls.
    #[serde(default)]
    pub draft: RibDraft,
}

/// Distribution of rib thickness around its profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RibSide {
    /// Thickness lies on one side of the profile.
    OneSided,
    /// Thickness is split equally around the profile.
    Centered,
}

/// Draft state of a rib construction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "angle", rename_all = "snake_case")]
pub enum RibDraft {
    /// Draft semantics are present but unresolved.
    #[default]
    Unresolved,
    /// Rib walls have no draft.
    None,
    /// Rib walls use the specified draft angle.
    Angle(SlopeAngle),
}

/// Canonical role of a non-modeling feature-tree node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FeatureTreeNodeRole {
    /// Annotation container.
    Annotations,
    /// Ambient scene light.
    AmbientLight,
    /// Comment container.
    Comments,
    /// Cross-section container.
    CrossSections,
    /// Design-binder container.
    DesignBinder,
    /// Detail-item container.
    Details,
    /// Profile-selection handle generated from a dissectable sketch.
    DissectedProfile,
    /// Directional scene light.
    DirectionalLight,
    /// Equation container.
    Equations,
    /// Exploded-view container.
    ExplodedViews,
    /// Favorites container.
    Favorites,
    /// User-created feature folder.
    FeatureFolder,
    /// Generic history folder.
    History,
    /// Lights, cameras, and scene container.
    LightsAndCameras,
    /// Markup container.
    Markups,
    /// Built-in model origin node.
    ModelOrigin,
    /// Point scene light.
    PointLight,
    /// Material container or assignment node.
    Materials,
    /// Note container.
    Notes,
    /// Selection-set container.
    SelectionSets,
    /// Sensor container.
    Sensors,
    /// Built-in sheet-metal state root.
    SheetMetal,
    /// Solid-body container.
    SolidBodies,
    /// Spot scene light.
    SpotLight,
    /// Surface-body container.
    SurfaceBodies,
    /// Table container.
    Tables,
}

/// Axial termination of a cosmetic-thread annotation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CosmeticThreadExtent {
    /// Fixed thread length along the cylindrical face.
    Blind {
        /// Positive axial thread length.
        length: PositiveLength,
    },
    /// Thread annotation spans the complete cylindrical face.
    Through,
}

/// Canonical role of a built-in reference plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PrincipalPlane {
    /// Front plane through the model origin.
    Front,
    /// Top plane through the model origin.
    Top,
    /// Right plane through the model origin.
    Right,
}

/// Known sketch space and its optional resolved planar geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(from = "SketchFeatureBindingWire", into = "SketchFeatureBindingWire")]
pub enum SketchFeatureBinding {
    /// The feature's sketch space is unresolved.
    Unresolved,
    /// The source declares a planar sketch, with optional decoded geometry.
    Planar(Option<crate::sketches::SketchId>),
}

impl SketchFeatureBinding {
    /// Resolved planar sketch identity, when available.
    pub fn id(&self) -> Option<&crate::sketches::SketchId> {
        match self {
            Self::Unresolved => None,
            Self::Planar(sketch) => sketch.as_ref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "space", rename_all = "snake_case", deny_unknown_fields)]
enum SketchFeatureBindingWire {
    Unresolved {},
    Planar {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<crate::sketches::SketchId>,
    },
}

impl From<SketchFeatureBinding> for SketchFeatureBindingWire {
    fn from(value: SketchFeatureBinding) -> Self {
        match value {
            SketchFeatureBinding::Unresolved => Self::Unresolved {},
            SketchFeatureBinding::Planar(sketch) => Self::Planar { sketch },
        }
    }
}

impl From<SketchFeatureBindingWire> for SketchFeatureBinding {
    fn from(value: SketchFeatureBindingWire) -> Self {
        match value {
            SketchFeatureBindingWire::Unresolved {} => Self::Unresolved,
            SketchFeatureBindingWire::Planar { sketch } => Self::Planar(sketch),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
/// Side retained by a body-trim operation.
pub enum BodyTrimSide {
    /// Retained side is unresolved.
    Unresolved,
    /// Retain the side selected by tool orientation.
    Forward,
    /// Retain the side opposite tool orientation.
    Reverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(untagged)]
/// Direction law for projected curves.
pub enum CurveProjectionDirection {
    /// One explicit model-space vector.
    Vector(FeatureDirection3),
    /// Direction state without one explicit vector.
    State(CurveProjectionDirectionState),
}

impl Default for CurveProjectionDirection {
    fn default() -> Self {
        Self::State(CurveProjectionDirectionState::TargetNormal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
/// Direction state for projection without an explicit vector.
pub enum CurveProjectionDirectionState {
    /// Projection direction remains unresolved.
    Unresolved,
    /// Project along each target face's normal.
    TargetNormal,
}

/// Selection interpretation for a delete/keep-body operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BodyRetentionMode {
    /// The operation family is known but the selected retention mode is unavailable.
    Unresolved,
    /// Delete the selected bodies.
    DeleteSelected,
    /// Delete every body except the selected bodies.
    KeepSelected,
}

/// Material effect of a wrapped profile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum WrapMode {
    /// Add material above the target face.
    Emboss {
        /// Positive normal offset above the target face.
        depth: Length,
    },
    /// Remove material below the target face.
    Deboss {
        /// Positive normal offset below the target face.
        depth: Length,
    },
    /// Imprint the profile without adding or removing material.
    Scribe,
}

/// Continuity order imposed at a generated surface boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SurfaceContinuity {
    /// Positional continuity only.
    Contact,
    /// First-derivative continuity.
    Tangent,
    /// Second-derivative continuity.
    Curvature,
}

/// Resolved continuity conditions for a filled-surface boundary.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum FilledSurfaceContinuity {
    /// One condition applies to the complete boundary.
    Uniform(SurfaceContinuity),
    /// Conditions apply to individual boundary components in source order.
    PerBoundary {
        /// Condition of the first component; its presence makes the sequence non-empty.
        first: SurfaceContinuity,
        /// Conditions of the remaining components.
        rest: Vec<SurfaceContinuity>,
    },
}

impl FilledSurfaceContinuity {
    /// Creates a non-empty component-specific condition sequence.
    #[must_use]
    pub fn per_boundary(conditions: Vec<SurfaceContinuity>) -> Option<Self> {
        let mut conditions = conditions.into_iter();
        Some(Self::PerBoundary {
            first: conditions.next()?,
            rest: conditions.collect(),
        })
    }

    /// Returns the aggregate condition when every component uses one value.
    #[must_use]
    pub fn uniform(&self) -> Option<SurfaceContinuity> {
        match self {
            Self::Uniform(continuity) => Some(*continuity),
            Self::PerBoundary { first, rest }
                if rest.iter().all(|continuity| continuity == first) =>
            {
                Some(*first)
            }
            Self::PerBoundary { .. } => None,
        }
    }
}

/// Optional filled-surface continuity with checked flat-wire deserialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "FilledSurfaceContinuityWire",
    into = "FilledSurfaceContinuityWire"
)]
pub struct FilledSurfaceContinuityState(Option<FilledSurfaceContinuity>);

impl FilledSurfaceContinuityState {
    /// Creates an unresolved continuity state.
    #[must_use]
    pub const fn unresolved() -> Self {
        Self(None)
    }

    /// Creates one condition for the complete boundary.
    #[must_use]
    pub const fn uniform(continuity: SurfaceContinuity) -> Self {
        Self(Some(FilledSurfaceContinuity::Uniform(continuity)))
    }

    /// Creates component-specific conditions, or unresolved state for no conditions.
    #[must_use]
    pub fn per_boundary(conditions: Vec<SurfaceContinuity>) -> Self {
        Self(FilledSurfaceContinuity::per_boundary(conditions))
    }

    /// Returns the resolved continuity form.
    #[must_use]
    pub const fn resolved(&self) -> Option<&FilledSurfaceContinuity> {
        self.0.as_ref()
    }

    /// Returns the aggregate condition when every component uses one value.
    #[must_use]
    pub fn uniform_value(&self) -> Option<SurfaceContinuity> {
        self.0.as_ref().and_then(FilledSurfaceContinuity::uniform)
    }

    /// Whether no continuity condition was resolved.
    #[must_use]
    pub const fn is_unresolved(&self) -> bool {
        self.0.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct FilledSurfaceContinuityWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    continuity: Option<SurfaceContinuity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    boundary_continuities: Vec<SurfaceContinuity>,
}

impl From<FilledSurfaceContinuityState> for FilledSurfaceContinuityWire {
    fn from(value: FilledSurfaceContinuityState) -> Self {
        match value.0 {
            None => Self {
                continuity: None,
                boundary_continuities: Vec::new(),
            },
            Some(FilledSurfaceContinuity::Uniform(continuity)) => Self {
                continuity: Some(continuity),
                boundary_continuities: Vec::new(),
            },
            Some(FilledSurfaceContinuity::PerBoundary { first, rest }) => {
                let continuity = rest
                    .iter()
                    .all(|candidate| candidate == &first)
                    .then_some(first);
                let mut boundary_continuities = Vec::with_capacity(rest.len() + 1);
                boundary_continuities.push(first);
                boundary_continuities.extend(rest);
                Self {
                    continuity,
                    boundary_continuities,
                }
            }
        }
    }
}

impl TryFrom<FilledSurfaceContinuityWire> for FilledSurfaceContinuityState {
    type Error = String;

    fn try_from(value: FilledSurfaceContinuityWire) -> Result<Self, Self::Error> {
        let FilledSurfaceContinuityWire {
            continuity,
            boundary_continuities,
        } = value;
        let Some(per_boundary) = FilledSurfaceContinuity::per_boundary(boundary_continuities)
        else {
            return Ok(continuity.map_or_else(Self::unresolved, Self::uniform));
        };
        if continuity.is_some_and(|continuity| per_boundary.uniform() != Some(continuity)) {
            return Err(
                "filled-surface continuity disagrees with boundary_continuities".to_string(),
            );
        }
        Ok(Self(Some(per_boundary)))
    }
}

/// Boundary input accepted by a filled-surface operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SurfaceBoundary {
    /// Boundary selected as topological edges.
    Edges(EdgeSelection),
    /// Boundary selected as a sketch, curve, or mixed path collection.
    Path(PathRef),
}

/// Region retained by a trim-surface operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TrimRegion {
    /// Source trim exists but the retained region is unresolved.
    Unresolved,
    /// Retain the region enclosed by the trimming path.
    Inside,
    /// Retain the region outside the trimming path.
    Outside,
    /// Remove an explicit set of partition cells.
    Cells(TrimCellSelection),
}

/// Cells removed by a trim operation from its partition of the target faces.
///
/// Cell ordinals are one-based within the operation's source partition. The
/// total is part of the semantic value because the complement is the retained
/// result and a selected set can contain neither all nor only the first cells.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TrimCellSelectionWire")]
pub struct TrimCellSelection {
    /// One-based ordinals of cells removed by the operation.
    removed: Vec<u64>,
    /// Number of cells in the operation's partition.
    total: u64,
}

impl TrimCellSelection {
    /// Creates a nonempty selection whose unique ordinals are within the partition.
    #[must_use]
    pub fn new(removed: Vec<u64>, total: u64) -> Option<Self> {
        let mut seen = HashSet::with_capacity(removed.len());
        (total > 0
            && !removed.is_empty()
            && removed
                .iter()
                .all(|ordinal| *ordinal > 0 && *ordinal <= total)
            && removed.iter().all(|ordinal| seen.insert(*ordinal)))
        .then_some(Self { removed, total })
    }

    /// Returns the one-based removed-cell ordinals.
    #[must_use]
    pub fn removed(&self) -> &[u64] {
        &self.removed
    }

    /// Returns the number of cells in the source partition.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct TrimCellSelectionWire {
    removed: Vec<u64>,
    total: u64,
}

impl TryFrom<TrimCellSelectionWire> for TrimCellSelection {
    type Error = &'static str;

    fn try_from(wire: TrimCellSelectionWire) -> Result<Self, Self::Error> {
        Self::new(wire.removed, wire.total)
            .ok_or("trim cell selection removed must be nonempty, unique, and within total")
    }
}
/// Geometric law used to extend a surface boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SurfaceExtension {
    /// Extension law is unresolved.
    Unresolved,
    /// Continue the source surface parameterization.
    Natural,
    /// Extend boundary tangents as ruled linear strips.
    Linear,
    /// Extend boundary faces perpendicular to the source faces.
    Perpendicular,
}

/// Direction law for a ruled-surface operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuledSurfaceMode {
    /// Extend normal to the support faces.
    Normal {
        /// Positive extension distance.
        distance: PositiveLength,
    },
    /// Extend tangent to the support faces.
    Tangent {
        /// Positive extension distance.
        distance: PositiveLength,
    },
    /// Extend along one explicit model-space direction.
    Direction {
        /// Extension direction.
        direction: FeatureDirection3,
        /// Positive extension distance.
        distance: PositiveLength,
    },
}

/// Corner construction law for a ruled-surface operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RuledSurfaceCorner {
    /// Join adjacent ruled strips with rounded corners.
    Rounded,
    /// Intersect adjacent ruled strips to form mitered corners.
    Mitered,
}

/// Fixed locus of a body-scale transform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ScaleCenter {
    /// Combined centroid of the selected bodies.
    Centroid,
    /// Model coordinate-system origin.
    ModelOrigin,
    /// Explicit model-space point.
    Point(FinitePoint3),
    /// Format-native coordinate-system or reference identifier.
    Native(String),
}

/// Factors of a body-scale transform.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "ScaleFactorsWire", into = "ScaleFactorsWire")]
pub enum ScaleFactors {
    /// No complete scale factor is available.
    Unresolved,
    /// One factor applies on all three axes.
    Uniform(NonZeroReal),
    /// Independent factors apply on the model-space axes.
    PerAxis([NonZeroReal; 3]),
}

impl ScaleFactors {
    /// Resolve the effective model-space factors when construction is complete.
    #[must_use]
    pub fn resolved(self) -> Option<Vector3> {
        match self {
            Self::Unresolved => None,
            Self::Uniform(factor) => Some(Vector3::new(factor.get(), factor.get(), factor.get())),
            Self::PerAxis(factors) => Some(Vector3::new(
                factors[0].get(),
                factors[1].get(),
                factors[2].get(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ScaleFactorsWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uniform: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    z: Option<f64>,
}

impl From<ScaleFactors> for ScaleFactorsWire {
    fn from(value: ScaleFactors) -> Self {
        match value {
            ScaleFactors::Unresolved => Self {
                uniform: None,
                x: None,
                y: None,
                z: None,
            },
            ScaleFactors::Uniform(factor) => Self {
                uniform: Some(factor.get()),
                x: None,
                y: None,
                z: None,
            },
            ScaleFactors::PerAxis(factors) => Self {
                uniform: None,
                x: Some(factors[0].get()),
                y: Some(factors[1].get()),
                z: Some(factors[2].get()),
            },
        }
    }
}

impl TryFrom<ScaleFactorsWire> for ScaleFactors {
    type Error = String;

    fn try_from(value: ScaleFactorsWire) -> Result<Self, Self::Error> {
        match (value.uniform, value.x, value.y, value.z) {
            (None, None, None, None) => Ok(Self::Unresolved),
            (Some(factor), None, None, None) => Ok(Self::Uniform(
                NonZeroReal::try_from(factor)
                    .map_err(|_| "scale uniform factor must be finite and nonzero")?,
            )),
            (None, Some(x), Some(y), Some(z)) => Ok(Self::PerAxis([
                NonZeroReal::try_from(x)
                    .map_err(|_| "scale x factor must be finite and nonzero")?,
                NonZeroReal::try_from(y)
                    .map_err(|_| "scale y factor must be finite and nonzero")?,
                NonZeroReal::try_from(z)
                    .map_err(|_| "scale z factor must be finite and nonzero")?,
            ])),
            _ => Err(
                "scale factors must be uniformly resolved, resolved on all axes, or absent"
                    .to_string(),
            ),
        }
    }
}

/// Direction in which a thicken feature adds material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ThickenSide {
    /// Add material along the selected-face normal.
    Forward,
    /// Add material opposite the selected-face normal.
    Reverse,
    /// Split the thickness equally across both sides.
    Both,
}

/// Face pair a sheet-metal flange height is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SheetMetalHeightDatum {
    /// The height is measured from the inner faces of the sheet.
    InnerFaces,
    /// The height is measured from the outer faces of the sheet.
    OuterFaces,
}

/// Placement of a sheet-metal bend region against its selected edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SheetMetalBendPosition {
    /// The bend lies outside the selected edge.
    Outside,
    /// The bend lies inside the selected edge.
    Inside,
    /// The bend starts at the selected edge.
    Adjacent,
    /// The bend is tangent to the side reference plane.
    TangentToSide,
}

/// Dimensional owner layout carried by a sheet-metal hem.
///
/// The source uses one owner layout for flat and open hems. A resolved
/// transition projects that layout to [`Flat`] or [`Open`]; [`GapLength`]
/// retains the dimensional owners when the transition does not prove either
/// semantic form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SheetMetalHemForm {
    /// Flat hem with its developed length.
    Flat {
        /// Developed length of the hem.
        length: PositiveLength,
    },
    /// Open hem with a gap and a developed length.
    Open {
        /// Gap between the folded wall and its source.
        gap: NonNegativeLength,
        /// Developed length of the hem.
        length: PositiveLength,
    },
    /// Unresolved flat-or-open form with a gap and a length owner.
    GapLength {
        /// Gap between the folded wall and its source.
        gap: NonNegativeLength,
        /// Developed length of the hem.
        length: PositiveLength,
    },
    /// Rolled source form with radius and included-angle owners.
    Rolled {
        /// Rolled section radius.
        radius: PositiveLength,
        /// Included angle of the rolled section.
        angle: Angle,
    },
    /// Teardrop source form with gap, length, and radius owners.
    Teardrop {
        /// Gap between the folded wall and its source.
        gap: NonNegativeLength,
        /// Developed length of the hem.
        length: PositiveLength,
        /// Teardrop section radius.
        radius: PositiveLength,
    },
}

/// Direction of a sheet-metal hem fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SheetMetalHemDirection {
    /// Fold in the source operation's forward direction.
    Forward,
    /// Fold opposite the source operation's forward direction.
    Reverse,
    /// Source direction carrier is not resolved.
    Unresolved,
}

/// Construction feature used as the reference for a sheet-metal flange height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SheetMetalFlangeHeightTarget {
    /// A neutral construction feature in the same design history.
    Feature(FeatureId),
    /// A source selection whose neutral construction identity is not resolved.
    Native(String),
}

/// Height law for a sheet-metal edge flange.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SheetMetalFlangeHeight {
    /// Fixed distance from the operation's height datum.
    Distance(PositiveLength),
    /// Signed offset from a selected construction entity.
    ToObject {
        /// Construction entity that supplies the height reference.
        target: SheetMetalFlangeHeightTarget,
        /// Signed distance from the target entity.
        offset: Length,
    },
}

/// Extent of a sheet-metal flange along its selected edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SheetMetalFlangeWidth {
    /// The flange spans the complete selected edge.
    FullEdge,
    /// The flange is centred on the edge with one width.
    Symmetric {
        /// Total width centred on the edge.
        width: PositiveLength,
    },
    /// The flange is measured inward from each end of the edge.
    TwoSides {
        /// Distance measured from the edge's first end.
        first: PositiveLength,
        /// Distance measured from the edge's second end.
        second: PositiveLength,
    },
    /// Independent two-sided extents for each selected edge.
    ///
    /// The entries are in the same order as the operation's selected-edge
    /// groups. Each pair is local to its edge and is not an operation-wide
    /// pair shared by all edges.
    TwoSidesPerEdge {
        /// One first-end/second-end pair for each selected edge.
        widths: SheetMetalFlangeEdgeWidths,
    },
}

/// Nonempty source-ordered per-edge flange widths.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SheetMetalFlangeEdgeWidths(Vec<SheetMetalFlangeTwoSidedWidth>);

impl SheetMetalFlangeEdgeWidths {
    /// Construct widths for at least one selected source edge group.
    pub fn new(widths: Vec<SheetMetalFlangeTwoSidedWidth>) -> Result<Self, &'static str> {
        if widths.is_empty() {
            Err("sheet-metal flange widths must contain at least one pair")
        } else {
            Ok(Self(widths))
        }
    }

    /// Width pairs in selected source edge-group order.
    pub fn as_slice(&self) -> &[SheetMetalFlangeTwoSidedWidth] {
        &self.0
    }

    /// Mutable width values. The number of selected groups cannot change.
    pub fn as_mut_slice(&mut self) -> &mut [SheetMetalFlangeTwoSidedWidth] {
        &mut self.0
    }
}

impl<'de> Deserialize<'de> for SheetMetalFlangeEdgeWidths {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Two-sided extent assigned to one selected sheet-metal flange edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SheetMetalFlangeTwoSidedWidth {
    /// Distance measured from the edge's first end.
    #[serde(deserialize_with = "deserialize_flange_first")]
    pub first: PositiveLength,
    /// Distance measured from the edge's second end.
    #[serde(deserialize_with = "deserialize_flange_second")]
    pub second: PositiveLength,
}

/// Distribution of sheet thickness relative to its construction plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SheetMetalThicknessSide {
    /// Thickness lies along the profile plane's positive normal.
    Forward,
    /// Thickness is split equally across both sides of the profile plane.
    Symmetric,
}

macro_rules! selection_field_deserializer {
    ($name:ident, $field:literal) => {
        fn $name<'de, D, T>(deserializer: D) -> Result<T, D::Error>
        where
            D: serde::Deserializer<'de>,
            T: Deserialize<'de>,
        {
            T::deserialize(deserializer)
                .map_err(|error| serde::de::Error::custom(format!("{}: {error}", $field)))
        }
    };
}

selection_field_deserializer!(deserialize_selection_native, "native");
selection_field_deserializer!(deserialize_selection_local_id, "local_id");
selection_field_deserializer!(deserialize_selection_edges, "edges");
selection_field_deserializer!(deserialize_selection_faces, "faces");
selection_field_deserializer!(deserialize_selection_unresolved, "unresolved");
selection_field_deserializer!(deserialize_flange_first, "first");
selection_field_deserializer!(deserialize_flange_second, "second");
selection_field_deserializer!(deserialize_selection_profiles, "profiles");
selection_field_deserializer!(deserialize_selection_entities, "entities");
selection_field_deserializer!(deserialize_selection_selections, "selections");
selection_field_deserializer!(deserialize_selection_curves, "curves");
selection_field_deserializer!(deserialize_profile_parameter_range, "parameter_range");
selection_field_deserializer!(deserialize_local_tessellations, "tessellations");
selection_field_deserializer!(deserialize_local_segments, "segments");
selection_field_deserializer!(deserialize_local_axis_native_ref, "axis_native_ref");
selection_field_deserializer!(deserialize_local_groups, "groups");
selection_field_deserializer!(deserialize_local_cells, "cells");
selection_field_deserializer!(deserialize_local_document, "document");
selection_field_deserializer!(deserialize_local_object, "object");
selection_field_deserializer!(deserialize_local_reference, "reference");
selection_field_deserializer!(deserialize_local_subelements, "subelements");
selection_field_deserializer!(deserialize_local_standard, "standard");
selection_field_deserializer!(deserialize_local_occurrences, "occurrences");
selection_field_deserializer!(deserialize_local_value, "value");

/// Edge operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum EdgeSelection {
    /// Selection exists semantically but its operands are not resolved.
    Unresolved,
    /// Every edge of the operation's input body.
    All,
    /// Resolved topological edges.
    Edges(Vec<EdgeId>),
    /// Resolved edges paired with the format-native selection required for rewrite.
    Resolved {
        /// Resolved topological edges.
        edges: Vec<EdgeId>,
        /// Format-native selection reference.
        native: String,
    },
    /// Edges resolved in the containing feature's input topology.
    Historical {
        /// Input topology containing every selected edge.
        state: FeatureInputTopologyId,
        /// State-local edge identities in operand order.
        #[serde(deserialize_with = "deserialize_selection_edges")]
        edges: SelectionMembers<HistoricalEdgeId>,
        /// Format-native selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NonEmptyString,
    },
    /// Proven historical edges plus source operands whose edge identity is unresolved.
    /// `edges` is empty when the input state is known but no member identity resolves.
    HistoricalPartial {
        /// Input topology containing every resolved edge.
        state: FeatureInputTopologyId,
        /// Proven state-local edge identities in source operand order.
        #[serde(deserialize_with = "deserialize_selection_edges")]
        edges: DistinctMembers<HistoricalEdgeId>,
        /// Stable native identities of unresolved source operands.
        #[serde(deserialize_with = "deserialize_selection_unresolved")]
        unresolved: NativeSelections,
        /// Format-native group selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NonEmptyString,
    },
    /// Edges in intermediate regenerated feature results, paired with the
    /// format-native selection required for rewrite.
    Generated {
        /// Feature-local edge identities.
        #[serde(deserialize_with = "deserialize_selection_edges")]
        edges: NonEmptyMembers<GeneratedEdgeRef>,
        /// Format-native persistent selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: SelectionReference,
    },
    /// Format-native selection reference.
    Native(String),
}

/// Persistent identity of an edge in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GeneratedEdgeRef {
    /// Feature whose regenerated result owns the edge.
    pub feature: FeatureId,
    /// Feature-local persistent edge identity.
    #[serde(deserialize_with = "deserialize_selection_local_id")]
    pub local_id: SelectionReference,
}

/// Persistent identity of a face in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GeneratedFaceRef {
    /// Feature whose regenerated result owns the face.
    pub feature: FeatureId,
    /// Feature-local persistent face identity.
    #[serde(deserialize_with = "deserialize_selection_local_id")]
    pub local_id: SelectionReference,
}

/// Persistent identity of a vertex in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GeneratedVertexRef {
    /// Feature whose regenerated result owns the vertex.
    pub feature: FeatureId,
    /// Feature-local persistent vertex identity.
    #[serde(deserialize_with = "deserialize_selection_local_id")]
    pub local_id: SelectionReference,
}

/// Vertex operand resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum VertexSelection {
    /// Selection exists semantically but its operand is not resolved.
    Unresolved,
    /// Vertex in an intermediate regenerated feature result, paired with the
    /// format-native selection required for rewrite.
    Generated {
        /// Feature-local vertex identity.
        vertex: GeneratedVertexRef,
        /// Format-native persistent selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: SelectionReference,
    },
    /// Vertex resolved in the containing feature's input topology.
    Historical {
        /// Input topology containing the selected vertex.
        state: FeatureInputTopologyId,
        /// State-local vertex identity.
        vertex: HistoricalVertexId,
        /// Format-native persistent selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NonEmptyString,
    },
    /// Format-native selection reference.
    Native(#[serde(deserialize_with = "deserialize_selection_native")] SelectionReference),
}

/// Face operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FaceSelection {
    /// Selection exists semantically but its operands are not resolved.
    Unresolved,
    /// Resolved topological faces; empty for no selected faces.
    Faces(Vec<FaceId>),
    /// Resolved faces paired with the format-native selection required for rewrite.
    Resolved {
        /// Resolved topological faces.
        faces: Vec<FaceId>,
        /// Format-native selection reference.
        native: String,
    },
    /// Faces resolved in the containing feature's input topology.
    Historical {
        /// Input topology containing every selected face.
        state: FeatureInputTopologyId,
        /// State-local face identities in operand order.
        #[serde(deserialize_with = "deserialize_selection_faces")]
        faces: SelectionMembers<HistoricalFaceId>,
        /// Format-native selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NonEmptyString,
    },
    /// Historical faces proven for part of a native selection.
    HistoricalPartial {
        /// Input topology containing every resolved face.
        state: FeatureInputTopologyId,
        /// Proven state-local face identities in source operand order.
        #[serde(deserialize_with = "deserialize_selection_faces")]
        faces: DistinctMembers<HistoricalFaceId>,
        /// Stable native identities of unresolved source operands.
        #[serde(deserialize_with = "deserialize_selection_unresolved")]
        unresolved: NativeSelections,
        /// Format-native selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NonEmptyString,
    },
    /// Faces in an intermediate regenerated feature result, paired with the
    /// format-native selection required for rewrite.
    Generated {
        /// Feature-local face identities.
        #[serde(deserialize_with = "deserialize_selection_faces")]
        faces: NonEmptyMembers<GeneratedFaceRef>,
        /// Format-native persistent selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: SelectionReference,
    },
    /// Format-native selection reference.
    Native(String),
}

/// A nonempty sequence of members in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct NonEmptyMembers<T>(Vec<T>);

impl<T> TryFrom<Vec<T>> for NonEmptyMembers<T> {
    type Error = BodySelectionError;
    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(BodySelectionError::Empty);
        }
        Ok(Self(value))
    }
}

impl<T> NonEmptyMembers<T> {
    /// Construct a sequence with one member.
    pub fn one(member: T) -> Self {
        Self(vec![member])
    }

    /// The members in source order.
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<T> std::ops::Deref for NonEmptyMembers<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<'a, T> IntoIterator for &'a NonEmptyMembers<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for NonEmptyMembers<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Vec::<T>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl<T> std::ops::DerefMut for NonEmptyMembers<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}

impl<'a, T> IntoIterator for &'a mut NonEmptyMembers<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

impl VertexSelection {
    /// Admits a generated vertex and its native reference.
    pub fn generated(
        vertex: GeneratedVertexRef,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::Generated {
            vertex,
            native: native.try_into()?,
        })
    }

    /// Admits a historical vertex and its native reference.
    pub fn historical(
        state: FeatureInputTopologyId,
        vertex: HistoricalVertexId,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::Historical {
            state,
            vertex,
            native: NonEmptyString::new(native).ok_or(BodySelectionError::BlankNativeMember)?,
        })
    }

    /// Admits a native vertex reference.
    pub fn native(native: String) -> Result<Self, BodySelectionError> {
        Ok(Self::Native(native.try_into()?))
    }
}

impl EdgeSelection {
    /// Admits historical members and their native reference.
    pub fn historical(
        state: FeatureInputTopologyId,
        edges: Vec<HistoricalEdgeId>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::Historical {
            state,
            edges: edges.try_into()?,
            native: NonEmptyString::new(native).ok_or(BodySelectionError::BlankNativeMember)?,
        })
    }

    /// Admits partial historical members and unresolved native operands.
    pub fn historical_partial(
        state: FeatureInputTopologyId,
        edges: Vec<HistoricalEdgeId>,
        unresolved: Vec<String>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::HistoricalPartial {
            state,
            edges: edges
                .try_into()
                .map_err(|_| BodySelectionError::RepeatedBody)?,
            unresolved: unresolved.try_into()?,
            native: NonEmptyString::new(native).ok_or(BodySelectionError::BlankNativeMember)?,
        })
    }

    /// Admits generated members and their native reference.
    pub fn generated(
        edges: Vec<GeneratedEdgeRef>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::Generated {
            edges: edges.try_into()?,
            native: native.try_into()?,
        })
    }
}

impl FaceSelection {
    /// Admits historical members and their native reference.
    pub fn historical(
        state: FeatureInputTopologyId,
        faces: Vec<HistoricalFaceId>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::Historical {
            state,
            faces: faces.try_into()?,
            native: NonEmptyString::new(native).ok_or(BodySelectionError::BlankNativeMember)?,
        })
    }

    /// Admits partial historical members and unresolved native operands.
    pub fn historical_partial(
        state: FeatureInputTopologyId,
        faces: Vec<HistoricalFaceId>,
        unresolved: Vec<String>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::HistoricalPartial {
            state,
            faces: faces
                .try_into()
                .map_err(|_| BodySelectionError::RepeatedBody)?,
            unresolved: unresolved.try_into()?,
            native: NonEmptyString::new(native).ok_or(BodySelectionError::BlankNativeMember)?,
        })
    }

    /// Admits generated members and their native reference.
    pub fn generated(
        faces: Vec<GeneratedFaceRef>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::Generated {
            faces: faces.try_into()?,
            native: native.try_into()?,
        })
    }
}

impl GeneratedEdgeRef {
    /// Admits a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        Ok(Self {
            feature,
            local_id: local_id.try_into()?,
        })
    }
}

impl GeneratedFaceRef {
    /// Admits a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        Ok(Self {
            feature,
            local_id: local_id.try_into()?,
        })
    }
}

impl GeneratedVertexRef {
    /// Admits a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        Ok(Self {
            feature,
            local_id: local_id.try_into()?,
        })
    }
}

/// A nonblank persistent selection reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SelectionReference(String);

impl TryFrom<String> for SelectionReference {
    type Error = BodySelectionError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err(BodySelectionError::BlankNativeMember);
        }
        Ok(Self(value))
    }
}

impl SelectionReference {
    /// The retained reference text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for SelectionReference {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for SelectionReference {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for SelectionReference {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl<'de> Deserialize<'de> for SelectionReference {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Distinct members in source order, including an empty sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct DistinctMembers<T>(Vec<T>);

impl<T> Default for DistinctMembers<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T: Eq + std::hash::Hash> TryFrom<Vec<T>> for DistinctMembers<T> {
    type Error = &'static str;
    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        if value.iter().collect::<HashSet<_>>().len() != value.len() {
            return Err("members must be distinct");
        }
        Ok(Self(value))
    }
}

impl<T: PartialEq> DistinctMembers<T> {
    /// Inserts a member unless it is already present, and returns whether it was added.
    pub fn insert(&mut self, value: T) -> bool {
        if self.0.contains(&value) {
            return false;
        }
        self.0.push(value);
        true
    }
}

impl<T> DistinctMembers<T> {
    /// Removes all members.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Retains members that satisfy the predicate without changing their order.
    pub fn retain(&mut self, predicate: impl FnMut(&T) -> bool) {
        self.0.retain(predicate);
    }

    /// Whether the sequence contains no members.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The members in source order.
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<T: PartialEq> Extend<T> for DistinctMembers<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for member in iter {
            self.insert(member);
        }
    }
}

impl<T: PartialEq> FromIterator<T> for DistinctMembers<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut members = Self::default();
        members.extend(iter);
        members
    }
}

impl<T> std::ops::Deref for DistinctMembers<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<'a, T> IntoIterator for &'a DistinctMembers<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de, T: Deserialize<'de> + Eq + std::hash::Hash> Deserialize<'de> for DistinctMembers<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Vec::<T>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Nonempty distinct selection members in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SelectionMembers<T>(Vec<T>);

impl<T: Eq + std::hash::Hash> TryFrom<Vec<T>> for SelectionMembers<T> {
    type Error = BodySelectionError;
    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(BodySelectionError::Empty);
        }
        if value.iter().collect::<HashSet<_>>().len() != value.len() {
            return Err(BodySelectionError::RepeatedBody);
        }
        Ok(Self(value))
    }
}

impl<T> SelectionMembers<T> {
    /// Construct a selection containing one member.
    pub fn one(member: T) -> Self {
        Self(vec![member])
    }

    /// The selected members in source order.
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<T> IntoIterator for SelectionMembers<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T> std::ops::Deref for SelectionMembers<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<'a, T> IntoIterator for &'a SelectionMembers<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de, T: Deserialize<'de> + Eq + std::hash::Hash> Deserialize<'de> for SelectionMembers<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Vec::<T>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Nonempty distinct nonblank native selection names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct NativeSelections(Vec<String>);

impl TryFrom<Vec<String>> for NativeSelections {
    type Error = BodySelectionError;
    fn try_from(value: Vec<String>) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(BodySelectionError::Empty);
        }
        if value.iter().any(|name| name.trim().is_empty()) {
            return Err(BodySelectionError::BlankNativeMember);
        }
        if value.iter().collect::<HashSet<_>>().len() != value.len() {
            return Err(BodySelectionError::RepeatedNativeMember);
        }
        Ok(Self(value))
    }
}

impl NativeSelections {
    /// The native names in source order.
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

impl IntoIterator for NativeSelections {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl std::ops::Deref for NativeSelections {
    type Target = [String];
    fn deref(&self) -> &[String] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for NativeSelections {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Vec::<String>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Failure to construct a body-selection member set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BodySelectionError {
    /// A selection set contains no members.
    #[error("body selection set must not be empty")]
    Empty,
    /// Parallel inputs have different member counts.
    #[error("body selection rows have mismatched lengths")]
    MismatchedLengths,
    /// A body occurs more than once in the set.
    #[error("body selection set repeats a body")]
    RepeatedBody,
    /// A native selection member is empty or contains only whitespace.
    #[error("body selection member must not be blank")]
    BlankNativeMember,
    /// A native member occurs more than once in the set.
    #[error("body selection set repeats a native member")]
    RepeatedNativeMember,
}

/// Body operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BodyMember<B> {
    body: B,
    native: String,
}

impl<B> BodyMember<B> {
    /// Construct one body/native selection row.
    pub fn new(body: B, native: String) -> Result<Self, BodySelectionError> {
        if native.trim().is_empty() {
            return Err(BodySelectionError::BlankNativeMember);
        }
        Ok(Self { body, native })
    }

    /// Body identity in this row.
    #[must_use]
    pub const fn body(&self) -> &B {
        &self.body
    }

    /// Native selection member in this row.
    #[must_use]
    pub fn native(&self) -> &str {
        &self.native
    }
}

impl<'de, B> Deserialize<'de> for BodyMember<B>
where
    B: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire<B> {
            body: B,
            native: String,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.body, wire.native).map_err(serde::de::Error::custom)
    }
}

/// Immutable, checked rows pairing a body identity with its native member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct BodyMembers<B>(Vec<BodyMember<B>>);

impl<B> BodyMembers<B> {
    /// Construct checked rows from body/native pairs.
    pub fn try_from_rows(rows: Vec<BodyMember<B>>) -> Result<Self, BodySelectionError>
    where
        B: Eq + std::hash::Hash,
    {
        if rows.is_empty() {
            return Err(BodySelectionError::Empty);
        }
        let mut bodies = HashSet::with_capacity(rows.len());
        let mut native = HashSet::with_capacity(rows.len());
        for row in &rows {
            if !bodies.insert(&row.body) {
                return Err(BodySelectionError::RepeatedBody);
            }
            if !native.insert(&row.native) {
                return Err(BodySelectionError::RepeatedNativeMember);
            }
        }
        Ok(Self(rows))
    }

    /// Construct checked rows from parallel body and native-member vectors.
    pub fn try_from_parts(bodies: Vec<B>, native: Vec<String>) -> Result<Self, BodySelectionError>
    where
        B: Eq + std::hash::Hash,
    {
        if bodies.len() != native.len() {
            return Err(BodySelectionError::MismatchedLengths);
        }
        let rows = bodies
            .into_iter()
            .zip(native)
            .map(|(body, native)| BodyMember::new(body, native))
            .collect::<Result<Vec<_>, _>>()?;
        Self::try_from_rows(rows)
    }

    /// Number of paired selection rows.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether this selection has no rows.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Borrow the checked rows in source order.
    pub fn iter(&self) -> std::slice::Iter<'_, BodyMember<B>> {
        self.0.iter()
    }

    /// Borrow the body identities in source order.
    pub fn bodies(&self) -> impl Iterator<Item = &B> {
        self.0.iter().map(BodyMember::body)
    }

    /// Borrow the native members in source order.
    pub fn native(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(BodyMember::native)
    }
}

impl<'a, B> IntoIterator for &'a BodyMembers<B> {
    type Item = &'a BodyMember<B>;
    type IntoIter = std::slice::Iter<'a, BodyMember<B>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de, B> Deserialize<'de> for BodyMembers<B>
where
    B: Deserialize<'de> + Eq + std::hash::Hash,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let rows = Vec::<BodyMember<B>>::deserialize(deserializer)?;
        Self::try_from_rows(rows).map_err(serde::de::Error::custom)
    }
}

/// Checked aggregate for historical selections whose native members have no
/// established body-to-member correspondence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct HistoricalUnorderedBodySelection {
    bodies: Vec<HistoricalBodyId>,
    native: Vec<String>,
}

impl HistoricalUnorderedBodySelection {
    /// Construct a checked unordered selection while preserving both orders.
    pub fn try_from_parts(
        bodies: Vec<HistoricalBodyId>,
        native: Vec<String>,
    ) -> Result<Self, BodySelectionError> {
        if bodies.is_empty() {
            return Err(BodySelectionError::Empty);
        }
        if bodies.len() != native.len() {
            return Err(BodySelectionError::MismatchedLengths);
        }
        if bodies.iter().collect::<HashSet<_>>().len() != bodies.len() {
            return Err(BodySelectionError::RepeatedBody);
        }
        if native.iter().any(|member| member.trim().is_empty()) {
            return Err(BodySelectionError::BlankNativeMember);
        }
        if native.iter().collect::<HashSet<_>>().len() != native.len() {
            return Err(BodySelectionError::RepeatedNativeMember);
        }
        Ok(Self { bodies, native })
    }

    /// Historical body identities in deterministic source order.
    #[must_use]
    pub fn bodies(&self) -> &[HistoricalBodyId] {
        &self.bodies
    }

    /// Native members in their retained source order.
    #[must_use]
    pub fn native(&self) -> &[String] {
        &self.native
    }

    /// Number of retained bodies and native members.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.bodies.len()
    }

    /// Whether the checked aggregate is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }
}

impl<'de> Deserialize<'de> for HistoricalUnorderedBodySelection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            bodies: Vec<HistoricalBodyId>,
            native: Vec<String>,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::try_from_parts(wire.bodies, wire.native).map_err(serde::de::Error::custom)
    }
}

/// Body operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum BodySelection {
    /// Selection exists semantically but its operands are not resolved.
    Unresolved,
    /// Resolved topological bodies.
    Bodies(Vec<BodyId>),
    /// Resolved bodies paired with the format-native selection required for rewrite.
    Resolved {
        /// Resolved topological bodies.
        bodies: Vec<BodyId>,
        /// Format-native selection expression.
        native: String,
    },
    /// Resolved bodies paired with independently retained native selection members.
    ResolvedSet {
        /// Checked rows in native member order.
        members: BodyMembers<BodyId>,
    },
    /// Bodies resolved in the containing feature's input topology.
    Historical {
        /// Input topology containing every selected body.
        state: FeatureInputTopologyId,
        /// State-local body identities in operand order.
        bodies: SelectionMembers<HistoricalBodyId>,
        /// Format-native selection expression.
        native: SelectionReference,
    },
    /// Bodies resolved in the containing feature's input topology from
    /// independently retained native selection members.
    HistoricalSet {
        /// Input topology containing every selected body.
        state: FeatureInputTopologyId,
        /// Checked rows in native member order.
        members: BodyMembers<HistoricalBodyId>,
    },
    /// Bodies resolved collectively in the containing feature's input topology
    /// when no body-to-native-member correspondence is established.
    HistoricalUnorderedSet {
        /// Input topology containing every selected body.
        state: FeatureInputTopologyId,
        /// Checked aggregate retaining both independent source orders.
        selection: HistoricalUnorderedBodySelection,
    },
    /// Bodies in intermediate regenerated feature results, paired with the
    /// format-native selection required for rewrite.
    Generated {
        /// Feature-local body identities.
        bodies: SelectionMembers<GeneratedBodyRef>,
        /// Format-native persistent selection reference.
        native: SelectionReference,
    },
    /// Persistent bodies in the consuming feature's regeneration input state.
    Local {
        /// Ordered feature-input-local body identities.
        bodies: NativeSelections,
        /// Format-native persistent selection reference.
        native: SelectionReference,
    },
    /// Format-native selection expression.
    Native(String),
    /// Ordered format-native selection members that have no enclosing native
    /// group record.
    NativeSet(NativeSelections),
}

impl BodySelection {
    /// Checked local body operands with their native reference.
    pub fn local(bodies: Vec<String>, native: String) -> Result<Self, BodySelectionError> {
        Ok(Self::Local {
            bodies: bodies.try_into()?,
            native: native.try_into()?,
        })
    }

    /// Checked historical body operands with their native reference.
    pub fn historical(
        state: FeatureInputTopologyId,
        bodies: Vec<HistoricalBodyId>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::Historical {
            state,
            bodies: bodies.try_into()?,
            native: native.try_into()?,
        })
    }

    /// Checked generated body operands with their native reference.
    pub fn generated(
        bodies: Vec<GeneratedBodyRef>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::Generated {
            bodies: bodies.try_into()?,
            native: native.try_into()?,
        })
    }
}

/// Persistent identity of a body in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GeneratedBodyRef {
    /// Feature whose regenerated result owns the body.
    pub feature: FeatureId,
    /// Feature-local persistent body identity.
    pub local_id: SelectionReference,
}

impl GeneratedBodyRef {
    /// A feature-local body identity with a nonblank local name.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        Ok(Self {
            feature,
            local_id: local_id.try_into()?,
        })
    }
}

/// Direct face-motion law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FaceMotion {
    /// Offset along each face normal.
    Offset {
        /// Signed offset distance.
        distance: Length,
    },
    /// Translation along one direction.
    Translate {
        /// Translation direction.
        direction: FeatureDirection3,
        /// Signed translation distance.
        distance: Length,
    },
    /// Rotation about an axis.
    Rotate {
        /// Point on the rotation axis.
        axis_origin: FinitePoint3,
        /// Rotation-axis direction.
        axis_dir: FeatureDirection3,
        /// Signed rotation angle.
        angle: Angle,
    },
}

/// Model-space axis-angle rotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AxisAngle {
    /// Point on the rotation axis.
    pub origin: FinitePoint3,
    /// Rotation-axis direction.
    pub direction: FeatureDirection3,
    /// Signed rotation angle.
    pub angle: Angle,
}

/// Start condition of a linear extrusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExtrudeStart {
    /// Native start condition is present structurally but unresolved.
    Unresolved,
    /// Begin on the profile's own plane.
    #[default]
    ProfilePlane,
    /// Begin on a plane parallel to the profile plane at a signed offset.
    OffsetProfilePlane {
        /// Signed offset along the profile normal in canonical millimeters.
        offset: Length,
    },
    /// Begin on a selected face, optionally displaced along the extrusion direction.
    FromFace {
        /// Face defining the start plane.
        face: FaceSelection,
        /// Signed displacement from the selected face.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<Length>,
    },
}

/// One-sided termination law of a linear sweep. Sidedness around the profile
/// plane is stated by the owning feature's extent type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinearTermination {
    /// Native termination is present structurally but unresolved.
    Unresolved,
    /// Fixed travel distance.
    Blind {
        /// Fixed travel distance.
        length: NonZeroLength,
    },
    /// Extends through all material.
    ThroughAll,
    /// Extends until it exits the next material region.
    ThroughNext,
    /// Extends until the first encountered model face.
    ToFirst,
    /// Extends until the last encountered model face.
    ToLast,
    /// Extends until it reaches a target face.
    ToFace {
        /// Face terminating the operation.
        face: FaceSelection,
        /// Signed displacement from the terminating face.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<Length>,
    },
    /// Extends until it reaches a target vertex.
    ToVertex {
        /// Vertex terminating the operation.
        vertex: VertexSelection,
    },
    /// Extends to a fixed offset from a target face.
    OffsetFromFace {
        /// Face the termination is measured from.
        face: FaceSelection,
        /// Offset distance from the face.
        offset: PositiveLength,
    },
    /// Extends until one of the faces in a selected target shape.
    ToShape {
        /// Native or resolved target shape selection.
        target: FaceSelection,
    },
}

/// One-sided termination law of an angular sweep. Sidedness around the profile
/// plane is stated by the owning revolution extent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AngularTermination {
    /// Native termination is present structurally but unresolved.
    Unresolved,
    /// Extends through all material.
    ThroughAll,
    /// Extends until it exits the next material region.
    ThroughNext,
    /// Extends until the first encountered model face.
    ToFirst,
    /// Extends until the last encountered model face.
    ToLast,
    /// Extends until it reaches a target face.
    ToFace {
        /// Face terminating the operation.
        face: FaceSelection,
        /// Signed displacement from the terminating face.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<Length>,
    },
    /// Extends until it reaches a target vertex.
    ToVertex {
        /// Vertex terminating the operation.
        vertex: VertexSelection,
    },
    /// Extends to a fixed offset from a target face.
    OffsetFromFace {
        /// Face the termination is measured from.
        face: FaceSelection,
        /// Offset distance from the face.
        offset: PositiveLength,
    },
    /// Extends until one of the faces in a selected target shape.
    ToShape {
        /// Native or resolved target shape selection.
        target: FaceSelection,
    },
    /// Fixed angular travel.
    Angle {
        /// Angular travel.
        angle: PositiveAngle,
    },
}

/// One side of an extrusion: its termination law and side-local modifiers.
/// Drafts are measured from the profile plane outward along the side's
/// travel; an absent draft leaves the side walls parallel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ExtrudeSide {
    /// Where this side's travel terminates.
    pub termination: LinearTermination,
    /// Draft angle applied to this side's walls, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<SlopeAngle>,
}

/// Extrusion sidedness around the profile plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExtrudeExtent {
    /// Travel on the oriented side only.
    OneSided {
        /// The single traveled side.
        side: ExtrudeSide,
    },
    /// Independent sides on each side of the profile plane.
    TwoSided {
        /// Side along the extrusion direction.
        first: ExtrudeSide,
        /// Side opposite the extrusion direction.
        second: ExtrudeSide,
    },
    /// One side mirrored across the profile plane. A blind length states the
    /// total travel split evenly around the plane.
    Symmetric {
        /// The mirrored side.
        side: ExtrudeSide,
    },
}

/// Revolution sidedness around the profile plane. Revolution sides carry no
/// side-local modifiers, only their termination laws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RevolveExtent {
    /// Travel on the oriented side only.
    OneSided {
        /// The single traveled side's termination.
        termination: AngularTermination,
    },
    /// Independent terminations on each side of the profile plane.
    TwoSided {
        /// Termination along the revolution direction.
        first: AngularTermination,
        /// Termination opposite the revolution direction.
        second: AngularTermination,
    },
    /// One termination mirrored across the profile plane. An angular travel
    /// states the total travel split evenly around the plane.
    Symmetric {
        /// The mirrored side's termination.
        termination: AngularTermination,
    },
}

/// Persisted source of a resolved linear-extrusion direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExtrusionDirectionSource {
    /// Direction comes from the persisted direction vector.
    Custom,
    /// Direction comes from a selected straight edge.
    Edge {
        /// Native edge selection used as the direction axis.
        reference: PathRef,
    },
    /// Direction comes from the source profile's plane normal.
    ProfileNormal,
}

/// Native algorithm used to construct faces from wires.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FaceMaker {
    /// Builds each wire independently.
    Simple,
    /// Builds one face with non-nested holes.
    Cheese,
    /// Builds the extrusion-compatible face form.
    Extrusion,
    /// Builds planar faces with nested islands.
    Bullseye,
    /// Builds overlapping, nested, or non-planar faces.
    Unified,
    /// Retains an extension face-maker class.
    Other(NonEmptyString),
}

impl FaceMaker {
    /// Parses a non-empty runtime class name.
    pub fn new(class: impl Into<String>) -> Option<Self> {
        let class = class.into();
        Some(match class.as_str() {
            "Part::FaceMakerSimple" => Self::Simple,
            "Part::FaceMakerCheese" => Self::Cheese,
            "Part::FaceMakerExtrusion" => Self::Extrusion,
            "Part::FaceMakerBullseye" => Self::Bullseye,
            "Part::FaceMakerUnified" => Self::Unified,
            _ => Self::Other(NonEmptyString::new(class)?),
        })
    }

    /// Returns the `FreeCAD` runtime class name.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Simple => "Part::FaceMakerSimple",
            Self::Cheese => "Part::FaceMakerCheese",
            Self::Extrusion => "Part::FaceMakerExtrusion",
            Self::Bullseye => "Part::FaceMakerBullseye",
            Self::Unified => "Part::FaceMakerUnified",
            Self::Other(class) => class.as_str(),
        }
    }

    /// Returns the persisted `FreeCAD` extrusion enumeration value.
    pub const fn mode(&self) -> u32 {
        match self {
            Self::Simple => 0,
            Self::Cheese => 1,
            Self::Extrusion => 2,
            Self::Bullseye => 3,
            Self::Unified | Self::Other(_) => 4,
        }
    }
}

impl Serialize for FaceMaker {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_str().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FaceMaker {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("face maker class must not be empty"))
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for FaceMaker {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FaceMaker".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(generator)
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ExtrusionFaceMakerWire {
    class: FaceMaker,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<u32>,
}

mod optional_extrusion_face_maker {
    use super::{ExtrusionFaceMakerWire, FaceMaker};
    use serde::{Deserialize, Serialize};

    // Serde passes the borrowed field to this adapter.
    #[allow(clippy::ref_option)]
    pub(super) fn serialize<S>(value: &Option<FaceMaker>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        value
            .as_ref()
            .map(|maker| ExtrusionFaceMakerWire {
                class: maker.clone(),
                mode: Some(maker.mode()),
            })
            .serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Option<FaceMaker>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let Some(wire) = Option::<ExtrusionFaceMakerWire>::deserialize(deserializer)? else {
            return Ok(None);
        };
        if wire.mode.is_some_and(|mode| mode != wire.class.mode()) {
            return Err(serde::de::Error::custom(
                "face_maker.mode does not match face_maker.class",
            ));
        }
        Ok(Some(wire.class))
    }
}

/// Relationship between outer-wire and inner-wire taper directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum InnerWireTaper {
    /// Inner wires taper opposite to outer wires.
    Inverted,
    /// Inner wires taper in the same direction as outer wires.
    SameAsOuter,
}

/// Persisted construction algorithm used for a parametric helix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HelixConstructionStyle {
    /// Historical construction retained for document compatibility.
    Legacy,
    /// Corrected construction used by newly created features.
    Corrected,
}

/// Axial or radial construction law of a helix feature.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HelixShape {
    /// Constant-radius helix with signed axial rise per revolution.
    Cylindrical {
        /// Signed axial rise per revolution.
        pitch: NonZeroLength,
    },
    /// Conical helix with signed axial rise and cone half-angle.
    Conical {
        /// Signed axial rise per revolution.
        pitch: NonZeroLength,
        /// Cone half-angle.
        cone_angle: SlopeAngle,
    },
    /// Planar spiral with signed radial growth per revolution.
    Spiral {
        /// Signed radial growth per revolution.
        radial_growth: Length,
    },
}

/// Result topology retained by a projection-on-surface operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SurfaceProjectionMode {
    /// Retain all projected result shapes.
    All,
    /// Retain projected faces only.
    Faces,
    /// Retain projected edges only.
    Edges,
}

/// Boolean effect of a solid-producing feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BooleanOp {
    /// Source operation is retained but not semantically resolved.
    Unresolved,
    /// Union with existing bodies.
    Join,
    /// Subtraction from existing bodies.
    Cut,
    /// Intersection with existing bodies.
    Intersect,
    /// Creates an independent new body without combining.
    NewBody,
}

/// Boolean operation that consumes at least one existing target body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BooleanKind {
    /// Union with existing bodies.
    Join,
    /// Subtraction from existing bodies.
    Cut,
    /// Intersection with existing bodies.
    Intersect,
}

impl From<BooleanKind> for BooleanOp {
    fn from(value: BooleanKind) -> Self {
        match value {
            BooleanKind::Join => Self::Join,
            BooleanKind::Cut => Self::Cut,
            BooleanKind::Intersect => Self::Intersect,
        }
    }
}

impl TryFrom<BooleanOp> for BooleanKind {
    type Error = BooleanOp;

    fn try_from(value: BooleanOp) -> Result<Self, Self::Error> {
        match value {
            BooleanOp::Join => Ok(Self::Join),
            BooleanOp::Cut => Ok(Self::Cut),
            BooleanOp::Intersect => Ok(Self::Intersect),
            BooleanOp::Unresolved | BooleanOp::NewBody => Err(value),
        }
    }
}

/// Placement and parameterization of a solid Coil primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CoilConstruction {
    /// Axis frame and angular origin.
    pub placement: CoilPlacement,
    /// Diameter of the reference trajectory at its start.
    pub diameter: PositiveLength,
    /// Independent driving dimensions retained from the source feature.
    pub extent: CoilExtent,
    /// Generated section swept along the trajectory.
    pub section: CoilSection,
    /// Radial position of the section relative to the reference trajectory.
    pub section_placement: CoilSectionPlacement,
    /// Angular travel direction when viewed from the axis origin along the positive axis.
    pub clockwise: bool,
    /// Signed cone half-angle of an axial coil; zero produces a cylindrical helix.
    pub taper: Angle,
}

/// Geometric placement of a Coil trajectory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CoilPlacementWire", into = "CoilPlacementWire")]
pub enum CoilPlacement {
    /// Complete model-space axis and radial frame.
    Explicit {
        /// Origin, positive trajectory axis, and angular-zero radial direction.
        frame: FeatureUnitPlaneFrame,
    },
    /// Placement retained in one source-native construction aggregate.
    Native {
        /// Nonblank native record or scope containing the placement semantics.
        native_ref: SelectionReference,
    },
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CoilPlacementWire {
    Explicit {
        origin: Point3,
        axis: Vector3,
        radial: Vector3,
    },
    Native {
        native_ref: SelectionReference,
    },
}
impl TryFrom<CoilPlacementWire> for CoilPlacement {
    type Error = &'static str;
    fn try_from(wire: CoilPlacementWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            CoilPlacementWire::Explicit {
                origin,
                axis,
                radial,
            } => Self::Explicit {
                frame: FeatureUnitPlaneFrame::new(origin, axis, radial).ok_or(
                    "coil placement requires finite origin and perpendicular unit directions",
                )?,
            },
            CoilPlacementWire::Native { native_ref } => Self::Native { native_ref },
        })
    }
}
impl From<CoilPlacement> for CoilPlacementWire {
    fn from(placement: CoilPlacement) -> Self {
        match placement {
            CoilPlacement::Explicit { frame } => Self::Explicit {
                origin: frame.origin(),
                axis: frame.u_axis(),
                radial: frame.v_axis(),
            },
            CoilPlacement::Native { native_ref } => Self::Native { native_ref },
        }
    }
}

/// Independent driving dimensions of a Coil trajectory.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoilExtent {
    /// Axial coil driven by revolution count and total signed height.
    RevolutionsHeight {
        /// Positive angular-turn count.
        revolutions: PositiveReal,
        /// Signed axial travel.
        height: Length,
    },
    /// Axial coil driven by revolution count and signed pitch per revolution.
    RevolutionsPitch {
        /// Positive angular-turn count.
        revolutions: PositiveReal,
        /// Signed axial travel per revolution.
        pitch: NonZeroLength,
    },
    /// Axial coil driven by total signed height and signed pitch per revolution.
    HeightPitch {
        /// Signed axial travel.
        height: NonZeroLength,
        /// Signed axial travel per revolution.
        pitch: NonZeroLength,
    },
    /// Planar spiral driven by revolution count and signed radial pitch.
    Spiral {
        /// Positive angular-turn count.
        revolutions: PositiveReal,
        /// Signed radial growth per revolution.
        radial_pitch: NonZeroLength,
    },
}

/// Generated cross-section of a Coil primitive.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoilSection {
    /// Circular section whose size is its diameter.
    Circular {
        /// Circle diameter.
        diameter: PositiveLength,
    },
    /// Square section whose size is its edge length.
    Square {
        /// Edge length.
        size: PositiveLength,
    },
    /// Equilateral triangle pointing radially away from the axis.
    ExternalTriangle {
        /// Edge length.
        size: PositiveLength,
    },
    /// Equilateral triangle pointing radially toward the axis.
    InternalTriangle {
        /// Edge length.
        size: PositiveLength,
    },
}

/// Radial placement of a generated Coil section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoilSectionPlacement {
    /// Section lies inside the reference trajectory.
    Inside,
    /// Section centroid lies on the reference trajectory.
    Center,
    /// Section lies outside the reference trajectory.
    Outside,
}

/// Result semantics of a solid Coil primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoilResult {
    /// Create an independent body.
    NewBody,
    /// Combine the swept volume with selected existing bodies.
    Boolean {
        /// Join, cut, or intersection operation.
        operation: BooleanKind,
        /// Existing bodies participating in the operation.
        targets: BodySelection,
    },
}

/// Result semantics of a swept profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepMode {
    /// Native sweep family is known but its result subtype is unresolved.
    Unresolved,
    /// Sweep creates an independent solid body.
    NewBody,
    /// Sweep creates or modifies a solid body.
    Solid {
        /// Boolean combination with existing bodies.
        op: BooleanKind,
    },
    /// Sweep creates a sheet body.
    Surface,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "mode", rename_all = "snake_case")]
enum SweepModeWire {
    Unresolved,
    Solid { op: SolidSweepOperation },
    Surface,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
enum SolidSweepOperation {
    Join,
    Cut,
    Intersect,
    NewBody,
}

impl From<SweepMode> for SweepModeWire {
    fn from(value: SweepMode) -> Self {
        match value {
            SweepMode::Unresolved => Self::Unresolved,
            SweepMode::NewBody => Self::Solid {
                op: SolidSweepOperation::NewBody,
            },
            SweepMode::Solid { op } => Self::Solid {
                op: match op {
                    BooleanKind::Join => SolidSweepOperation::Join,
                    BooleanKind::Cut => SolidSweepOperation::Cut,
                    BooleanKind::Intersect => SolidSweepOperation::Intersect,
                },
            },
            SweepMode::Surface => Self::Surface,
        }
    }
}

impl From<SweepModeWire> for SweepMode {
    fn from(value: SweepModeWire) -> Self {
        match value {
            SweepModeWire::Unresolved => Self::Unresolved,
            SweepModeWire::Solid {
                op: SolidSweepOperation::NewBody,
            } => Self::NewBody,
            SweepModeWire::Solid {
                op: SolidSweepOperation::Join,
            } => Self::Solid {
                op: BooleanKind::Join,
            },
            SweepModeWire::Solid {
                op: SolidSweepOperation::Cut,
            } => Self::Solid {
                op: BooleanKind::Cut,
            },
            SweepModeWire::Solid {
                op: SolidSweepOperation::Intersect,
            } => Self::Solid {
                op: BooleanKind::Intersect,
            },
            SweepModeWire::Surface => Self::Surface,
        }
    }
}

impl Serialize for SweepMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        SweepModeWire::from(*self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SweepMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(SweepModeWire::deserialize(deserializer)?.into())
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for SweepMode {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SweepMode".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        SweepModeWire::json_schema(generator)
    }
}

/// Directed fractions of a sweep path consumed from the profile location.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SweepPathExtent {
    /// Fraction consumed in the path's forward traversal direction.
    pub along_fraction: Fraction,
    /// Fraction consumed in the path's reverse traversal direction.
    pub against_fraction: Fraction,
}

/// Guide rail controlling a sweep, with its directed consumed extent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SweepGuideRail {
    /// Ordered guide trajectory.
    pub path: PathRef,
    /// Fractions consumed on either side of the profile location.
    pub extent: SweepPathExtent,
}

/// Cross-section owned or referenced by a sweep construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SweepSection {
    /// The source requires a cross-section, but its carrier is unresolved.
    Unresolved(Option<String>),
    /// Cross-section supplied by referenced profile geometry.
    Profile(PlanarProfileRef),
    /// Cross-section generated by the sweep construction itself.
    Generated(GeneratedSweepSection),
}

impl SweepSection {
    /// Returns the referenced profile when this section does not own its geometry.
    pub fn referenced_profile(&self) -> Option<&ProfileRef> {
        match self {
            Self::Profile(profile) => Some(profile),
            Self::Unresolved(_) | Self::Generated(_) => None,
        }
    }

    /// Returns the mutable referenced profile when this section does not own its geometry.
    pub fn referenced_profile_mut(&mut self) -> Option<&mut PlanarProfileRef> {
        match self {
            Self::Profile(profile) => Some(profile),
            Self::Unresolved(_) | Self::Generated(_) => None,
        }
    }
}

/// A sweep circular section with an optional wall thinner than its radius.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SweepCircularRegion {
    outer_radius: PositiveLength,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wall_thickness: Option<PositiveLength>,
}

#[derive(Deserialize)]
struct SweepCircularRegionWire {
    outer_radius: PositiveLength,
    #[serde(default)]
    wall_thickness: Option<PositiveLength>,
}

impl SweepCircularRegion {
    /// Admit a disk or a wall thinner than the outer radius.
    pub fn new(
        outer_radius: PositiveLength,
        wall_thickness: Option<PositiveLength>,
    ) -> Result<Self, &'static str> {
        if wall_thickness.is_some_and(|wall| wall.get() >= outer_radius.get()) {
            return Err("wall_thickness must be less than outer_radius");
        }
        Ok(Self {
            outer_radius,
            wall_thickness,
        })
    }

    /// Return the outer radius.
    pub const fn outer_radius(self) -> PositiveLength {
        self.outer_radius
    }

    /// Return the inward radial wall thickness.
    pub const fn wall_thickness(self) -> Option<PositiveLength> {
        self.wall_thickness
    }
}

impl<'de> Deserialize<'de> for SweepCircularRegion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = SweepCircularRegionWire::deserialize(deserializer)?;
        Self::new(wire.outer_radius, wire.wall_thickness).map_err(serde::de::Error::custom)
    }
}

/// Sweep cross-sections and their compatible result mode.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SweepShape {
    section: SweepSection,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sections: Vec<SweepSection>,
    mode: SweepMode,
}

#[derive(Deserialize)]
struct SweepShapeWire {
    section: SweepSection,
    #[serde(default)]
    sections: Vec<SweepSection>,
    mode: SweepMode,
}

impl SweepShape {
    /// Construct an unresolved sweep section and result mode.
    pub fn unresolved(native: Option<String>) -> Self {
        Self {
            section: SweepSection::Unresolved(native),
            sections: Vec::new(),
            mode: SweepMode::Unresolved,
        }
    }

    /// Admit sections whose generated circular regions use a solid result mode.
    pub fn new(
        section: SweepSection,
        sections: Vec<SweepSection>,
        mode: SweepMode,
    ) -> Result<Self, &'static str> {
        if !matches!(mode, SweepMode::NewBody | SweepMode::Solid { .. })
            && std::iter::once(&section).chain(&sections).any(|section| {
                matches!(
                    section,
                    SweepSection::Generated(GeneratedSweepSection::CircularRegion { .. })
                )
            })
        {
            return Err(
                "section and sections with generated circular regions require a solid mode",
            );
        }
        Ok(Self {
            section,
            sections,
            mode,
        })
    }

    /// Return the primary cross-section.
    pub fn section(&self) -> &SweepSection {
        &self.section
    }

    /// Return the additional cross-sections in path order.
    pub fn sections(&self) -> &[SweepSection] {
        &self.sections
    }

    /// Return the result mode.
    pub const fn mode(&self) -> SweepMode {
        self.mode
    }

    /// Admit edited sections and result mode before replacing the sweep shape.
    pub fn try_edit(
        &mut self,
        edit: impl FnOnce(&mut SweepSection, &mut Vec<SweepSection>, &mut SweepMode),
    ) -> Result<(), &'static str> {
        let mut section = self.section.clone();
        let mut sections = self.sections.clone();
        let mut mode = self.mode;
        edit(&mut section, &mut sections, &mut mode);
        *self = Self::new(section, sections, mode)?;
        Ok(())
    }
}

impl<'de> Deserialize<'de> for SweepShape {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = SweepShapeWire::deserialize(deserializer)?;
        Self::new(wire.section, wire.sections, wire.mode).map_err(serde::de::Error::custom)
    }
}

/// Cross-section geometry generated and owned by a sweep construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum GeneratedSweepSection {
    /// Filled or hollow circular region centered on the sweep path.
    CircularRegion {
        /// Outer radius and optional smaller radial wall thickness.
        #[serde(flatten)]
        region: SweepCircularRegion,
    },
}

/// One directed use of a solved sketch curve in an arrangement boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchProfileBoundaryUse {
    /// Sketch entity supplying the curve geometry.
    pub entity: crate::sketches::SketchEntityId,
    /// Parameter endpoints on the source curve, ordered in the entity's stored direction.
    #[serde(deserialize_with = "deserialize_profile_parameter_range")]
    pub parameter_range: crate::geometry::DirectedParameterRange,
    /// Whether boundary traversal opposes the interval's stored direction.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reversed: bool,
}

/// Whole-loop region with distinct holes that exclude its outer loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SketchProfileLoopsWire")]
pub struct SketchProfileLoops {
    outer: u32,
    #[serde(skip_serializing_if = "DistinctMembers::is_empty")]
    holes: DistinctMembers<u32>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchProfileLoopsWire {
    outer: u32,
    #[serde(default)]
    holes: Vec<u32>,
}

impl SketchProfileLoops {
    /// Admits an outer loop and distinct hole loops that exclude it.
    pub fn new(outer: u32, holes: Vec<u32>) -> Result<Self, &'static str> {
        let holes = DistinctMembers::try_from(holes).map_err(|_| "holes must be distinct")?;
        if holes.contains(&outer) {
            return Err("holes must not contain outer");
        }
        Ok(Self { outer, holes })
    }

    /// The exterior-loop index.
    pub fn outer(&self) -> u32 {
        self.outer
    }

    /// The hole-loop indices in source order.
    pub fn holes(&self) -> &[u32] {
        self.holes.as_slice()
    }
}

impl TryFrom<SketchProfileLoopsWire> for SketchProfileLoops {
    type Error = &'static str;
    fn try_from(wire: SketchProfileLoopsWire) -> Result<Self, Self::Error> {
        Self::new(wire.outer, wire.holes)
    }
}

/// One connected planar region bounded by solved sketch curves.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(untagged)]
pub enum SketchProfileRegion {
    /// Exterior and holes are complete entries in the sketch profile table.
    Loops(SketchProfileLoops),
    /// Boundary rings switch source curves at arrangement intersections.
    Trimmed {
        /// Directed exterior boundary ring.
        outer_boundary: NonEmptyMembers<SketchProfileBoundaryUse>,
        /// Directed hole boundary rings.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        hole_boundaries: Vec<NonEmptyMembers<SketchProfileBoundaryUse>>,
    },
}

#[derive(Deserialize)]
struct SketchProfileRegionReadWire {
    #[serde(default)]
    outer: Option<u32>,
    #[serde(default)]
    holes: Vec<u32>,
    #[serde(default)]
    outer_boundary: Option<Vec<SketchProfileBoundaryUse>>,
    #[serde(default)]
    hole_boundaries: Vec<Vec<SketchProfileBoundaryUse>>,
}

impl SketchProfileRegion {
    /// Admits a whole-loop region with distinct holes that exclude the exterior loop.
    pub fn loops(outer: u32, holes: Vec<u32>) -> Result<Self, &'static str> {
        SketchProfileLoops::new(outer, holes).map(Self::Loops)
    }

    /// Admits a trimmed region whose exterior and hole rings are nonempty.
    pub fn trimmed(
        outer_boundary: Vec<SketchProfileBoundaryUse>,
        hole_boundaries: Vec<Vec<SketchProfileBoundaryUse>>,
    ) -> Result<Self, &'static str> {
        Ok(Self::Trimmed {
            outer_boundary: outer_boundary
                .try_into()
                .map_err(|_| "outer_boundary must not be empty")?,
            hole_boundaries: hole_boundaries
                .into_iter()
                .map(|ring| {
                    ring.try_into()
                        .map_err(|_| "hole_boundaries must not contain an empty ring")
                })
                .collect::<Result<_, _>>()?,
        })
    }
}

impl<'de> Deserialize<'de> for SketchProfileRegion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = SketchProfileRegionReadWire::deserialize(deserializer)?;
        if let Some(outer) = wire.outer {
            Self::loops(outer, wire.holes).map_err(serde::de::Error::custom)
        } else if let Some(outer_boundary) = wire.outer_boundary {
            Self::trimmed(outer_boundary, wire.hole_boundaries).map_err(serde::de::Error::custom)
        } else {
            Err(serde::de::Error::custom(
                "region requires outer or outer_boundary",
            ))
        }
    }
}

/// Nonempty distinct profile regions in source selection order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SketchProfileRegions(Vec<SketchProfileRegion>);

impl TryFrom<Vec<SketchProfileRegion>> for SketchProfileRegions {
    type Error = &'static str;
    fn try_from(regions: Vec<SketchProfileRegion>) -> Result<Self, Self::Error> {
        if regions.is_empty() {
            return Err("regions must not be empty");
        }
        if regions
            .iter()
            .enumerate()
            .any(|(index, region)| regions[..index].contains(region))
        {
            return Err("regions must be distinct");
        }
        Ok(Self(regions))
    }
}

impl SketchProfileRegions {
    /// The selected regions in source order.
    pub fn as_slice(&self) -> &[SketchProfileRegion] {
        &self.0
    }
}

impl std::ops::Deref for SketchProfileRegions {
    type Target = [SketchProfileRegion];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SketchProfileRegions {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Vec::<SketchProfileRegion>::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

/// Cross-section orientation law along a sweep path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SweepOrientation {
    /// Rotation-minimizing corrected-Frenet frame.
    CorrectedFrenet,
    /// Fixed section frame.
    Fixed,
    /// Exact Frenet frame from path derivatives.
    Frenet,
    /// Frame constrained by a secondary path.
    Auxiliary {
        /// Secondary orientation path.
        path: PathRef,
        /// Whether tangent-connected edges extend the secondary path.
        tangent: bool,
        /// Whether corresponding points use curvilinear rather than parameter distance.
        curvilinear: bool,
    },
    /// Frame constrained to remain normal to selected guide faces.
    GuideSurface {
        /// Ordered guide faces controlling the section frame.
        faces: FaceSelection,
    },
    /// Frame constrained by a fixed binormal direction.
    Binormal {
        /// Unit binormal direction.
        direction: FeatureDirection3,
    },
}

/// Corner continuation used by a sweep path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SweepTransition {
    /// Transform the section continuously across the corner.
    Transformed,
    /// Form a sharp right-corner intersection.
    RightCorner,
    /// Insert a rounded corner transition.
    RoundCorner,
}

/// Cross-section interpolation law for a multi-section sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SweepTransformation {
    /// Keep one constant section along the path.
    Constant,
    /// Interpolate through explicit ordered sections.
    MultiSection,
    /// Apply linear section interpolation.
    Linear,
    /// Apply an S-shaped interpolation law.
    SShape,
    /// Apply the native smooth interpolation law.
    Interpolation,
}

/// Signed axial travel and radial growth with at least one nonzero component.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelicalSweepTravelWire")]
pub struct HelicalSweepTravel {
    height: Length,
    radial_growth: Length,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HelicalSweepTravelWire {
    height: Length,
    radial_growth: Length,
}

impl HelicalSweepTravel {
    /// Admit finite signed travel with nonzero height or radial growth.
    pub fn new(height: Length, radial_growth: Length) -> Option<Self> {
        (height.get() != 0.0 || radial_growth.get() != 0.0).then_some(Self {
            height,
            radial_growth,
        })
    }

    /// Return the total axial travel.
    pub fn height(self) -> Length {
        self.height
    }

    /// Return the radial change per turn.
    pub fn radial_growth(self) -> Length {
        self.radial_growth
    }
}

impl TryFrom<HelicalSweepTravelWire> for HelicalSweepTravel {
    type Error = &'static str;
    fn try_from(wire: HelicalSweepTravelWire) -> Result<Self, Self::Error> {
        Self::new(wire.height, wire.radial_growth)
            .ok_or("helical-sweep height and radial_growth cannot both be zero")
    }
}

/// Complete construction of a solid helical sweep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct HelicalSweepConstruction {
    /// Profile swept along the helical path.
    pub profile: PlanarProfileRef,
    /// Point at the start of the helix axis.
    pub axis_origin: FinitePoint3,
    /// Unit direction of positive axial travel.
    pub axis_direction: FeatureDirection3,
    /// Persisted authoring law identifying the independent parameters.
    pub law: HelicalSweepLaw,
    /// Positive axial advance per turn; zero is permitted for a planar spiral.
    pub pitch: NonNegativeLength,
    /// Signed axial travel and radial change per turn.
    #[serde(flatten)]
    pub travel: HelicalSweepTravel,
    /// Positive number of turns.
    pub turns: PositiveReal,
    /// Cone half-angle corresponding to radial growth.
    pub cone_angle: Angle,
    /// Whether angular travel is left-handed along the positive axis.
    pub left_handed: bool,
    /// Whether path travel runs opposite the declared axis direction.
    pub reversed: bool,
    /// Relative tolerance used while joining the generated sweep, when persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<PositiveReal>,
    /// Whether a profile containing multiple faces is accepted as one operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_multi_profile_faces: Option<bool>,
}

/// Independent-parameter law used to author a helical sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HelicalSweepLaw {
    /// Pitch, height, and cone angle are independent.
    PitchHeightAngle,
    /// Pitch, turn count, and cone angle are independent.
    PitchTurnsAngle,
    /// Height, turn count, and cone angle are independent.
    HeightTurnsAngle,
    /// Height, turn count, and radial growth are independent.
    HeightTurnsGrowth,
}

/// One object or subelement selection consumed by a design binder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BinderSource {
    /// Bound object identity.
    pub target: BinderTarget,
    /// Ordered native subelement selectors; empty selects the complete object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[serde(deserialize_with = "deserialize_local_subelements")]
    pub subelements: Vec<NonEmptyString>,
}

/// Resolved or externally scoped binder target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BinderTarget {
    /// Feature in this CADIR document.
    Feature {
        /// Target feature identity.
        feature: FeatureId,
    },
    /// Object in another source document.
    External {
        /// Source document identity.
        #[serde(deserialize_with = "deserialize_local_document")]
        document: NonEmptyString,
        /// Object identity within the source document.
        #[serde(deserialize_with = "deserialize_local_object")]
        object: NonEmptyString,
    },
    /// Source-native target identity that cannot be resolved further.
    Native {
        /// Opaque source-native target identity.
        #[serde(deserialize_with = "deserialize_local_reference")]
        reference: NonEmptyString,
    },
}

/// Binding behavior and optional derived-shape construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BinderConstruction {
    /// Simple binder over one support object.
    Shape {
        /// Transform support geometry between its container and the binder container.
        trace_support: bool,
    },
    /// Multi-object subshape binder.
    SubShape {
        /// Live-update lifecycle.
        lifecycle: BinderLifecycle,
        /// Placement interpretation for linked subobjects.
        placement: BinderPlacement,
        /// Copy-on-change state.
        copy_on_change: BinderCopyOnChange,
        /// Whether linked objects are claimed as children in the tree.
        claim_children: bool,
        /// Whether multiple resulting solids are fused.
        fuse: bool,
        /// Whether bound wires are promoted to faces.
        make_face: bool,
        /// Whether external documents may remain partially loaded.
        partial_load: bool,
        /// Whether redundant edges are removed from the result.
        refine: bool,
        /// Optional two-dimensional offset construction.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<BinderOffset>,
        /// Context object used to interpret relative placement.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context: Option<BinderTarget>,
    },
}

/// Update lifecycle of a subshape binder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BinderLifecycle {
    /// Automatically tracks changes to its sources.
    Synchronized,
    /// Retains links but updates only when explicitly requested.
    Frozen,
    /// Stores a copied shape and no longer retains live binding behavior.
    Detached,
}

/// Placement interpretation for bound subobjects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BinderPlacement {
    /// Interpret source placement relative to the binder context.
    Relative,
    /// Preserve source placement in global coordinates.
    Global,
}

/// Copy-on-change state of a subshape binder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BinderCopyOnChange {
    /// Do not clone configurable source properties.
    Disabled,
    /// Clone configurable source properties when they change.
    Enabled,
    /// A private source copy has already been mutated.
    Mutated,
}

/// Two-dimensional offset applied to bound faces or wires.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BinderOffset {
    /// Signed offset distance.
    pub distance: NonZeroLength,
    /// Join law at offset corners.
    pub join: BinderOffsetJoin,
    /// Whether to fill between original and offset wires.
    pub fill: bool,
    /// Whether open input wires produce open offset results.
    pub open_result: bool,
    /// Whether child-wire intersections are resolved together.
    pub intersection: bool,
}

/// Corner join law of a binder offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BinderOffsetJoin {
    /// Circular corner arcs.
    Arcs,
    /// Tangent continuation.
    Tangent,
    /// Sharp line-line intersections.
    Intersection,
}

/// A profile reference excluding spatial-sketch profile forms.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct PlanarProfileRef(ProfileRef);

impl TryFrom<ProfileRef> for PlanarProfileRef {
    type Error = &'static str;
    fn try_from(profile: ProfileRef) -> Result<Self, Self::Error> {
        if matches!(
            profile,
            ProfileRef::SpatialSketchProfiles { .. } | ProfileRef::SpatialSketchSelection { .. }
        ) {
            return Err("profile must not select spatial-sketch profiles");
        }
        Ok(Self(profile))
    }
}

impl std::ops::Deref for PlanarProfileRef {
    type Target = ProfileRef;
    fn deref(&self) -> &ProfileRef {
        &self.0
    }
}

impl AsRef<ProfileRef> for PlanarProfileRef {
    fn as_ref(&self) -> &ProfileRef {
        &self.0
    }
}

impl From<crate::sketches::SketchId> for PlanarProfileRef {
    fn from(sketch: crate::sketches::SketchId) -> Self {
        Self(ProfileRef::Sketch(sketch))
    }
}

impl PlanarProfileRef {
    /// Retain a native profile reference.
    pub fn native(reference: String) -> Self {
        Self(ProfileRef::Native(reference))
    }

    /// Admit a profile edit before replacing the reference.
    pub fn try_edit(&mut self, edit: impl FnOnce(&mut ProfileRef)) -> Result<(), &'static str> {
        let mut profile = self.0.clone();
        edit(&mut profile);
        *self = Self::try_from(profile)?;
        Ok(())
    }
}

impl<'de> Deserialize<'de> for PlanarProfileRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(ProfileRef::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// A feature operation eligible for a single post-processing layer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct UnprocessedFeature(Box<FeatureDefinition>);

impl TryFrom<FeatureDefinition> for UnprocessedFeature {
    type Error = &'static str;
    fn try_from(operation: FeatureDefinition) -> Result<Self, Self::Error> {
        let spatial = |profile: &ProfileRef| {
            matches!(
                profile,
                ProfileRef::SpatialSketchProfiles { .. }
                    | ProfileRef::SpatialSketchSelection { .. }
            )
        };
        match &operation {
            FeatureDefinition::PostProcess { .. } => {
                return Err("operation must not be a PostProcess")
            }
            FeatureDefinition::Extrude { profile, .. } if spatial(profile) => {
                return Err("operation in PostProcess must not use spatial-sketch profiles")
            }
            FeatureDefinition::Loft { sections, .. }
                if sections.iter().any(
                    |section| matches!(section, LoftSection::Profile(profile) if spatial(profile)),
                ) =>
            {
                return Err("operation in PostProcess must not use spatial-sketch profiles")
            }
            _ => {}
        }
        Ok(Self(Box::new(operation)))
    }
}

impl AsRef<FeatureDefinition> for UnprocessedFeature {
    fn as_ref(&self) -> &FeatureDefinition {
        &self.0
    }
}

impl std::ops::Deref for UnprocessedFeature {
    type Target = FeatureDefinition;
    fn deref(&self) -> &FeatureDefinition {
        &self.0
    }
}

impl UnprocessedFeature {
    /// Admit an operation edit before replacing the underlying operation.
    pub fn try_edit(
        &mut self,
        edit: impl FnOnce(&mut FeatureDefinition),
    ) -> Result<(), &'static str> {
        let mut operation = (*self.0).clone();
        edit(&mut operation);
        *self = Self::try_from(operation)?;
        Ok(())
    }
}

impl<'de> Deserialize<'de> for UnprocessedFeature {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(FeatureDefinition::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

/// Profile consumed by a profile-driven feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ProfileRef {
    /// A profile is required by the identified native owner but its carrier is unresolved.
    Unresolved(String),
    /// Opaque reference into a native feature-input record; no neutral geometry given.
    Native(String),
    /// Solved neutral sketch profile.
    Sketch(crate::sketches::SketchId),
    /// Specific solved profile loops within one neutral sketch.
    SketchProfiles {
        /// Sketch containing the selected loops.
        sketch: crate::sketches::SketchId,
        /// Zero-based indices into [`crate::sketches::Sketch::profiles`].
        #[serde(deserialize_with = "deserialize_selection_profiles")]
        profiles: SelectionMembers<u32>,
    },
    /// Exact union of bounded atomic regions within one neutral sketch.
    SketchRegions {
        /// Sketch containing every referenced boundary loop.
        sketch: crate::sketches::SketchId,
        /// Connected regions in source selection order.
        regions: SketchProfileRegions,
    },
    /// Exact ordered sketch entities forming an open or closed profile.
    SketchEntities {
        /// Sketch containing every selected entity.
        sketch: crate::sketches::SketchId,
        /// Selected entities in source order.
        #[serde(deserialize_with = "deserialize_selection_entities")]
        entities: SelectionMembers<crate::sketches::SketchEntityId>,
    },
    /// Source-native selection within a known neutral sketch.
    SketchSelection {
        /// Sketch containing the unresolved selected geometry.
        sketch: crate::sketches::SketchId,
        /// Full-fidelity native selection records in source order.
        #[serde(deserialize_with = "deserialize_selection_selections")]
        selections: NativeSelections,
    },
    /// Specific solved profile loops within one neutral spatial sketch.
    SpatialSketchProfiles {
        /// Spatial sketch containing the selected loops.
        sketch: crate::sketches::SpatialSketchId,
        /// Zero-based indices into [`crate::sketches::SpatialSketch::profiles`].
        #[serde(deserialize_with = "deserialize_selection_profiles")]
        profiles: SelectionMembers<u32>,
    },
    /// Source-native selection within a known neutral spatial sketch.
    SpatialSketchSelection {
        /// Spatial sketch containing the unresolved selected geometry.
        sketch: crate::sketches::SpatialSketchId,
        /// Full-fidelity native selection records in source order.
        #[serde(deserialize_with = "deserialize_selection_selections")]
        selections: NativeSelections,
    },
    /// Profile given by faces in the consuming feature's input topology.
    HistoricalFaces {
        /// Input topology containing every selected face.
        state: FeatureInputTopologyId,
        /// State-local face identities in source selection order.
        #[serde(deserialize_with = "deserialize_selection_faces")]
        faces: SelectionMembers<HistoricalFaceId>,
        /// Full-fidelity source selection groups in source order.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NativeSelections,
    },
    /// Complete curve result of an earlier construction-history feature.
    Feature(FeatureId),
    /// Curves in an intermediate regenerated feature result, paired with the
    /// format-native persistent reference required for rewrite.
    Generated {
        /// Persistent feature-local curve identities.
        #[serde(deserialize_with = "deserialize_selection_curves")]
        curves: NonEmptyMembers<GeneratedCurveRef>,
        /// Format-native persistent profile reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: SelectionReference,
    },
    /// Profile given directly as a set of solved B-rep faces.
    Faces(Vec<FaceId>),
}

/// One ordered cross-section consumed by a loft operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(untagged)]
pub enum LoftSection {
    /// Planar or face-backed section profile.
    Profile(ProfileRef),
    /// Point-like terminal section.
    Point(LoftPointSection),
}

/// Point-like cross-section consumed by a loft operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LoftPointSection {
    /// Source-native point-selection record whose position is not resolved.
    #[serde(rename = "native_point")]
    Native(#[serde(deserialize_with = "deserialize_local_value")] NonEmptyString),
    /// Solved model-space point section.
    Point(FinitePoint3),
    /// Solved B-rep vertex section.
    Vertex(VertexId),
}

/// Persistent identity of a curve in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GeneratedCurveRef {
    /// Feature whose regenerated result owns the curve.
    pub feature: FeatureId,
    /// Complete ordered feature-local component identity.
    #[serde(deserialize_with = "deserialize_selection_local_id")]
    pub local_id: SelectionReference,
}

/// Trajectory consumed by a sweep or path-driven operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PathRef {
    /// Source path exists but its neutral members remain unresolved.
    Unresolved(String),
    /// Opaque reference into a native path record.
    Native(String),
    /// Ordered geometry from a neutral sketch.
    Sketch(crate::sketches::SketchId),
    /// Ordered selected curves from one neutral planar sketch.
    SketchCurves {
        /// Sketch containing every selected curve.
        sketch: crate::sketches::SketchId,
        /// Selected curve identities in source order.
        #[serde(deserialize_with = "deserialize_selection_curves")]
        curves: SelectionMembers<crate::sketches::SketchEntityId>,
    },
    /// Source-native curve selection within a known neutral spatial sketch.
    SpatialSketchSelection {
        /// Spatial sketch containing the selected curves.
        sketch: crate::sketches::SpatialSketchId,
        /// Full-fidelity native selection records in path order.
        #[serde(deserialize_with = "deserialize_selection_selections")]
        selections: NativeSelections,
    },
    /// Ordered selected curves from one neutral spatial sketch.
    SpatialSketchCurves {
        /// Spatial sketch containing every selected curve.
        sketch: crate::sketches::SpatialSketchId,
        /// Selected curve identities in source order.
        #[serde(deserialize_with = "deserialize_selection_curves")]
        curves: SelectionMembers<crate::sketches::SpatialSketchEntityId>,
    },
    /// Path resolved as ordered topological edges.
    Edges(Vec<EdgeId>),
    /// Path resolved as ordered geometric curves.
    Curves(Vec<CurveId>),
    /// Path resolved as ordered edges in the consuming feature's input topology.
    HistoricalEdges {
        /// Input topology containing every path edge.
        state: FeatureInputTopologyId,
        /// State-local edge identities in path order.
        #[serde(deserialize_with = "deserialize_selection_edges")]
        edges: SelectionMembers<HistoricalEdgeId>,
        /// Full-fidelity source path selection.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NonEmptyString,
    },
}

impl ProfileRef {
    /// Admits nonempty distinct profile regions in one sketch.
    pub fn sketch_regions(
        sketch: crate::sketches::SketchId,
        regions: Vec<SketchProfileRegion>,
    ) -> Result<Self, &'static str> {
        Ok(Self::SketchRegions {
            sketch,
            regions: regions.try_into()?,
        })
    }

    /// Admits distinct profile indices in one planar sketch.
    pub fn sketch_profiles(
        sketch: crate::sketches::SketchId,
        profiles: Vec<u32>,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::SketchProfiles {
            sketch,
            profiles: profiles.try_into()?,
        })
    }

    /// Admits distinct profile indices in one spatial sketch.
    pub fn spatial_sketch_profiles(
        sketch: crate::sketches::SpatialSketchId,
        profiles: Vec<u32>,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::SpatialSketchProfiles {
            sketch,
            profiles: profiles.try_into()?,
        })
    }

    /// Admits distinct profile entities in one sketch.
    pub fn sketch_entities(
        sketch: crate::sketches::SketchId,
        entities: Vec<crate::sketches::SketchEntityId>,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::SketchEntities {
            sketch,
            entities: entities.try_into()?,
        })
    }

    /// Admits native profile selections in one sketch.
    pub fn sketch_selection(
        sketch: crate::sketches::SketchId,
        selections: Vec<String>,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::SketchSelection {
            sketch,
            selections: selections.try_into()?,
        })
    }

    /// Admits native profile selections in one spatial sketch.
    pub fn spatial_sketch_selection(
        sketch: crate::sketches::SpatialSketchId,
        selections: Vec<String>,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::SpatialSketchSelection {
            sketch,
            selections: selections.try_into()?,
        })
    }

    /// Admits historical profile faces and native selection groups.
    pub fn historical_faces(
        state: FeatureInputTopologyId,
        faces: Vec<HistoricalFaceId>,
        native: Vec<String>,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::HistoricalFaces {
            state,
            faces: faces.try_into()?,
            native: native.try_into()?,
        })
    }

    /// Admits generated profile curves and their native reference.
    pub fn generated(
        curves: Vec<GeneratedCurveRef>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::Generated {
            curves: curves.try_into()?,
            native: native.try_into()?,
        })
    }
}

impl PathRef {
    /// Admits distinct path curves in one planar sketch.
    pub fn sketch_curves(
        sketch: crate::sketches::SketchId,
        curves: Vec<crate::sketches::SketchEntityId>,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::SketchCurves {
            sketch,
            curves: curves.try_into()?,
        })
    }

    /// Admits distinct path curves in one spatial sketch.
    pub fn spatial_sketch_curves(
        sketch: crate::sketches::SpatialSketchId,
        curves: Vec<crate::sketches::SpatialSketchEntityId>,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::SpatialSketchCurves {
            sketch,
            curves: curves.try_into()?,
        })
    }

    /// Admits native path selections in one spatial sketch.
    pub fn spatial_sketch_selection(
        sketch: crate::sketches::SpatialSketchId,
        selections: Vec<String>,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::SpatialSketchSelection {
            sketch,
            selections: selections.try_into()?,
        })
    }

    /// Admits historical path edges and their native reference.
    pub fn historical_edges(
        state: FeatureInputTopologyId,
        edges: Vec<HistoricalEdgeId>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        Ok(Self::HistoricalEdges {
            state,
            edges: edges.try_into()?,
            native: NonEmptyString::new(native).ok_or(BodySelectionError::BlankNativeMember)?,
        })
    }
}

impl GeneratedCurveRef {
    /// Admits a feature-local persistent curve identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        Ok(Self {
            feature,
            local_id: local_id.try_into()?,
        })
    }
}

/// Three distinct vertex targets with a common historical input state.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct ThreePointSelection(Box<[VertexSelection; 3]>);

impl TryFrom<Box<[VertexSelection; 3]>> for ThreePointSelection {
    type Error = &'static str;
    fn try_from(points: Box<[VertexSelection; 3]>) -> Result<Self, Self::Error> {
        if same_vertex_target(&points[0], &points[1])
            || same_vertex_target(&points[0], &points[2])
            || same_vertex_target(&points[1], &points[2])
        {
            return Err("points must select three distinct vertex targets");
        }
        let mut states = points.iter().filter_map(|point| match point {
            VertexSelection::Historical { state, .. } => Some(state),
            _ => None,
        });
        if let Some(state) = states.next() {
            if states.any(|candidate| candidate != state) {
                return Err("points must use the same input topology");
            }
        }
        Ok(Self(points))
    }
}

impl std::ops::Deref for ThreePointSelection {
    type Target = [VertexSelection; 3];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ThreePointSelection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Box::<[VertexSelection; 3]>::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

/// At least two distinct datum-plane feature identities in source order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SplitFacePlanes(SelectionMembers<FeatureId>);

impl TryFrom<Vec<FeatureId>> for SplitFacePlanes {
    type Error = &'static str;
    fn try_from(planes: Vec<FeatureId>) -> Result<Self, Self::Error> {
        if planes.len() < 2 {
            return Err("planes must contain at least two planes");
        }
        Ok(Self(
            planes.try_into().map_err(|_| "planes must be distinct")?,
        ))
    }
}

impl std::ops::Deref for SplitFacePlanes {
    type Target = [FeatureId];
    fn deref(&self) -> &[FeatureId] {
        &self.0
    }
}

impl<'a> IntoIterator for &'a SplitFacePlanes {
    type Item = &'a FeatureId;
    type IntoIter = std::slice::Iter<'a, FeatureId>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de> Deserialize<'de> for SplitFacePlanes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Vec::<FeatureId>::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

/// A nonempty source path without NUL characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct GeometryImportPath(String);

impl TryFrom<String> for GeometryImportPath {
    type Error = &'static str;
    fn try_from(path: String) -> Result<Self, Self::Error> {
        if path.is_empty() || path.contains('\0') {
            return Err("path must be nonempty and contain no NUL");
        }
        Ok(Self(path))
    }
}

impl std::ops::Deref for GeometryImportPath {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for GeometryImportPath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

fn same_vertex_target(
    first: &crate::features::VertexSelection,
    second: &crate::features::VertexSelection,
) -> bool {
    use crate::features::VertexSelection;

    match (first, second) {
        (
            VertexSelection::Generated { vertex: first, .. },
            VertexSelection::Generated { vertex: second, .. },
        ) => first == second,
        (
            VertexSelection::Historical {
                state: first_state,
                vertex: first_vertex,
                ..
            },
            VertexSelection::Historical {
                state: second_state,
                vertex: second_vertex,
                ..
            },
        ) => first_state == second_state && first_vertex == second_vertex,
        (VertexSelection::Native(first), VertexSelection::Native(second)) => first == second,
        (VertexSelection::Unresolved, VertexSelection::Unresolved) => true,
        _ => false,
    }
}

/// Geometry used to partition faces in a `SplitFace` operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SplitFaceTool {
    /// Sketch or model-space path projected onto the target faces.
    Path(PathRef),
    /// Datum-plane feature extended through the target faces.
    Plane {
        /// Earlier datum-plane feature supplying the splitting plane.
        plane: FeatureId,
    },
    /// Two or more datum-plane features extended through the target faces.
    Planes {
        /// Unique earlier datum-plane features in operation order.
        planes: SplitFacePlanes,
    },
}

mod edge_treatments;
pub use edge_treatments::{
    ChamferGroup, ChamferSpec, FilletGroup, FullRoundFilletGroup, FullRoundSideSelection,
    RadiusSpec, VariableRadii, VariableRadius,
};

mod holes;
pub use holes::{
    split, CounterdrillDiameters, HoleBottom, HoleConstruction, HoleForm, HoleKind,
    HoleProfileFilter, HoleShape, HoleSpecification, HoleThreadDepth, PartialPair, Split,
    ThreadHand,
};

/// Deformation applied by a flex feature.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "FlexModeWire", into = "FlexModeWire")]
pub enum FlexMode {
    /// Deformation family whose required magnitude remains unresolved.
    Unresolved(Option<FlexForm>),
    /// Bend through a signed angle.
    Bending {
        /// Total bend angle.
        angle: Angle,
    },
    /// Twist through a signed angle.
    Twisting {
        /// Total twist angle.
        angle: Angle,
    },
    /// Scale transverse sections by a dimensionless factor.
    Tapering {
        /// End-to-start transverse scale ratio.
        factor: PositiveReal,
    },
    /// Extend or contract along the flex axis.
    Stretching {
        /// Signed change in length.
        distance: Length,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
enum FlexModeWire {
    Unresolved {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<FlexForm>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<Angle>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        factor: Option<PositiveReal>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<Length>,
    },
    Bending {
        angle: Angle,
    },
    Twisting {
        angle: Angle,
    },
    Tapering {
        factor: PositiveReal,
    },
    Stretching {
        distance: Length,
    },
}

impl From<FlexMode> for FlexModeWire {
    fn from(value: FlexMode) -> Self {
        match value {
            FlexMode::Unresolved(form) => Self::Unresolved {
                form,
                angle: None,
                factor: None,
                distance: None,
            },
            FlexMode::Bending { angle } => Self::Bending { angle },
            FlexMode::Twisting { angle } => Self::Twisting { angle },
            FlexMode::Tapering { factor } => Self::Tapering { factor },
            FlexMode::Stretching { distance } => Self::Stretching { distance },
        }
    }
}

impl TryFrom<FlexModeWire> for FlexMode {
    type Error = String;

    fn try_from(value: FlexModeWire) -> Result<Self, Self::Error> {
        Ok(match value {
            FlexModeWire::Unresolved {
                form,
                angle: None,
                factor: None,
                distance: None,
            } => Self::Unresolved(form),
            FlexModeWire::Unresolved { .. } => {
                return Err("unresolved flex magnitude fields must be absent".to_string());
            }
            FlexModeWire::Bending { angle } => Self::Bending { angle },
            FlexModeWire::Twisting { angle } => Self::Twisting { angle },
            FlexModeWire::Tapering { factor } => Self::Tapering { factor },
            FlexModeWire::Stretching { distance } => Self::Stretching { distance },
        })
    }
}

/// Structural form of a flex deformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FlexForm {
    /// Angular bending.
    Bending,
    /// Angular twisting.
    Twisting,
    /// Transverse tapering.
    Tapering,
    /// Axial stretching.
    Stretching,
}

mod patterns;
pub use patterns::{
    LinearPatternDirection, PatternKind, PatternScaleCenter, PatternStage, PatternStageCombination,
    PatternTransform,
};

#[cfg(test)]
mod tests;
