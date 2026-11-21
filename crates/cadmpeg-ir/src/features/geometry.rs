// SPDX-License-Identifier: Apache-2.0
//! Feature points, directions, frames and curves.

use super::{EPS_FEATURE_ELLIPSE_AXES_ORTHO, EPS_FEATURE_PLANE_ORTHOGONAL, EPS_FEATURE_UNIT_FRAME};
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::{FiniteReal, PositiveLength, PositiveReal};
use crate::transform::Transform;
use crate::units::{FinitePoint2, FiniteVector, UnitVector3};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! checked_feature_geometry {
    ($(#[$meta:meta])* $name:ident, $raw:ident, $value:ident, $valid:expr, $error:literal $(, $getter:ident)*) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Serialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[serde(transparent)]
        pub struct $name($raw);
        rewrite_scalar!($name);

        impl $name {
            /// Admit a value that satisfies the feature geometry bounds.
            pub fn new($value: $raw) -> Option<Self> {
                ($valid).then_some(Self($value))
            }

            $(checked_feature_geometry!(@getter $getter, $raw);)*
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
    (@getter get, $raw:ident) => {
        /// Return the geometric value.
        pub const fn get(self) -> $raw { self.0 }
    };
    (@getter as_raw, $raw:ident) => {
        /// Borrow the geometric value.
        pub const fn as_raw(&self) -> &$raw { &self.0 }
    };
}

checked_feature_geometry!(
    /// A model-space point with finite coordinates.
    FinitePoint3, Point3, value,
    value.is_finite(),
    "FinitePoint3 coordinates must be finite", get, as_raw
);

impl FinitePoint3 {
    /// The model origin.
    pub const ZERO: Self = Self(Point3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    });

    /// The point with three finite coordinates. Every coordinate is finite, so the point is admitted without a check.
    #[must_use]
    pub const fn from_coordinates(x: FiniteReal, y: FiniteReal, z: FiniteReal) -> Self {
        Self(Point3 {
            x: x.get(),
            y: y.get(),
            z: z.get(),
        })
    }

    /// Reflect through the model origin. Negation keeps every coordinate finite, so the result stays admitted.
    #[must_use]
    pub fn negated(self) -> Self {
        Self(Point3::new(-self.0.x, -self.0.y, -self.0.z))
    }

    /// The point with every coordinate times `scale`. The product is refused
    /// only when a coordinate overflows.
    #[must_use]
    pub fn scaled(self, scale: PositiveReal) -> Option<Self> {
        let scale = scale.get();
        Self::new(Point3::new(
            self.0.x * scale,
            self.0.y * scale,
            self.0.z * scale,
        ))
    }

    /// Admit every point, or none of them.
    pub(crate) fn array<const N: usize>(values: [Point3; N]) -> Option<[Self; N]> {
        values
            .iter()
            .all(Point3::is_finite)
            .then(|| values.map(Self))
    }

    /// The raw points of the array, for a reader that writes or edits them.
    #[must_use]
    pub fn raw_array<const N: usize>(values: [Self; N]) -> [Point3; N] {
        values.map(Self::get)
    }
}

impl From<FiniteVector<3>> for FinitePoint3 {
    /// Carry three admitted finite coordinates into a model-space point.
    fn from(value: FiniteVector<3>) -> Self {
        let [x, y, z] = value.get();
        Self(Point3::new(x, y, z))
    }
}

checked_feature_geometry!(
    /// A displacement with finite components, including zero.
    FiniteVector3, Vector3, value,
    value.is_finite(),
    "FiniteVector3 components must be finite", get, as_raw
);

impl FiniteVector3 {
    /// The zero displacement.
    pub const ZERO: Self = Self(Vector3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    });

    /// The vector with three finite components. Every component is finite, so the vector is admitted without a check.
    #[must_use]
    pub const fn from_components(x: FiniteReal, y: FiniteReal, z: FiniteReal) -> Self {
        Self(Vector3 {
            x: x.get(),
            y: y.get(),
            z: z.get(),
        })
    }

    /// Use admitted vector coordinates as a finite point.
    #[must_use]
    pub const fn as_point(self) -> FinitePoint3 {
        FinitePoint3(Point3 {
            x: self.0.x,
            y: self.0.y,
            z: self.0.z,
        })
    }

    /// Reverse all components.
    #[must_use]
    pub fn negated(self) -> Self {
        Self(Vector3::new(-self.0.x, -self.0.y, -self.0.z))
    }

    /// Admit every vector, or none of them.
    pub(crate) fn array<const N: usize>(values: [Vector3; N]) -> Option<[Self; N]> {
        values
            .iter()
            .all(Vector3::is_finite)
            .then(|| values.map(Self))
    }

    /// The raw vectors of the array, for a reader that writes or edits them.
    #[must_use]
    pub fn raw_array<const N: usize>(values: [Self; N]) -> [Vector3; N] {
        values.map(Self::get)
    }
}

checked_feature_geometry!(
    /// A direction whose squared norm is finite and nonzero, so its length and
    /// every product of two of its components are representable.
    FeatureDirection3, Vector3, value,
    value.dot(value).is_finite() && value.dot(value) > 0.0,
    "FeatureDirection3 norm must be finite and nonzero", get
);

impl From<UnitVector3> for FiniteVector3 {
    /// Carry an admitted unit direction as a finite displacement. Its components are finite, so no admission can refuse it.
    fn from(value: UnitVector3) -> Self {
        Self(*value.as_raw())
    }
}

impl UnitVector3 {
    /// The cross product with another unit direction. Each component is a
    /// difference of two products of components within rounding of one, so
    /// it is finite and nothing is checked. The route lives beside
    /// [`FiniteVector3`] because only this module constructs one.
    #[must_use]
    pub fn finite_cross(self, other: UnitVector3) -> FiniteVector3 {
        FiniteVector3(self.as_raw().cross(*other.as_raw()))
    }
}

impl Transform {
    /// The translation column. The transform admits only finite entries, so
    /// nothing is checked. The route lives beside [`FiniteVector3`] because
    /// only this module constructs one.
    #[must_use]
    pub fn translation(self) -> FiniteVector3 {
        let rows = self.affine_rows();
        FiniteVector3(Vector3::new(rows[0][3], rows[1][3], rows[2][3]))
    }

    /// The columns of the linear part. The transform admits only finite
    /// entries, so nothing is checked. The route lives beside
    /// [`FiniteVector3`] because only this module constructs one.
    #[must_use]
    pub fn linear_columns(self) -> [FiniteVector3; 3] {
        let rows = self.affine_rows();
        std::array::from_fn(|column| {
            FiniteVector3(Vector3::new(
                rows[0][column],
                rows[1][column],
                rows[2][column],
            ))
        })
    }
}

impl From<FeatureDirection3> for FiniteVector3 {
    /// Carry an admitted direction as a finite displacement. A finite squared
    /// norm states finite components, so no admission can refuse it.
    fn from(value: FeatureDirection3) -> Self {
        Self(value.0)
    }
}

impl From<UnitVector3> for FeatureDirection3 {
    /// Carry an admitted unit direction. Its components are finite and its
    /// squared norm is within rounding of one, so no admission can refuse it.
    fn from(value: UnitVector3) -> Self {
        Self(*value.as_raw())
    }
}

impl FeatureDirection3 {
    /// The unit +z direction.
    pub const Z_AXIS: Self = Self(Vector3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    });

    /// Reverse all components. Each product of two components keeps its
    /// value under negation, so the squared norm is unchanged and the result
    /// stays admitted.
    #[must_use]
    pub fn reversed(self) -> Self {
        Self(Vector3::new(-self.0.x, -self.0.y, -self.0.z))
    }

    /// Canonicalize a unit input by replacing components of magnitude at most
    /// `1e-12` with positive zero. A unit vector has a component above one
    /// half in magnitude, so the result keeps a finite nonzero squared norm.
    #[must_use]
    pub fn from_unit_without_small_components(direction: UnitVector3) -> Self {
        const EPS_CANONICAL_COMPONENT: f64 = 1.0e-12;
        let component = |value: f64| {
            if value.abs() <= EPS_CANONICAL_COMPONENT {
                0.0
            } else {
                value
            }
        };
        let value = direction.as_raw();
        Self(Vector3::new(
            component(value.x),
            component(value.y),
            component(value.z),
        ))
    }
}

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

    /// Replace the translation and keep the linear rows. Rigidity is a
    /// condition on the linear rows alone, so the result keeps the admission
    /// without a check.
    #[must_use]
    pub fn with_translation(self, translation: FiniteVector3) -> Self {
        Self(self.0.with_translation(translation))
    }

    /// Scale the translation while keeping the admitted rigid linear rows.
    #[must_use]
    pub fn scaled_translation(self, scale: PositiveReal) -> Option<Self> {
        Some(Self(self.0.scaled_translation(scale)?))
    }
}

/// A finite origin and two perpendicular unit directions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "FeatureUnitPlaneFrameWire")]
pub struct FeatureUnitPlaneFrame {
    origin: FinitePoint3,
    u_axis: UnitVector3,
    v_axis: UnitVector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct FeatureUnitPlaneFrameWire {
    /// Model-space origin.
    origin: Point3,
    /// First unit direction.
    u_axis: Vector3,
    /// Second unit direction.
    v_axis: Vector3,
}

impl FeatureUnitPlaneFrame {
    /// Admit a finite origin and perpendicular unit axes within the feature tolerance.
    pub fn new(origin: Point3, u_axis: Vector3, v_axis: Vector3) -> Option<Self> {
        let origin = FinitePoint3::new(origin)?;
        let u_axis = UnitVector3::new(u_axis)?;
        let v_axis = UnitVector3::new(v_axis)?;
        Self::from_parts(origin, u_axis, v_axis)
    }

    /// Build a plane frame from checked parts; only perpendicularity remains to check.
    pub fn from_parts(
        origin: FinitePoint3,
        u_axis: UnitVector3,
        v_axis: UnitVector3,
    ) -> Option<Self> {
        (u_axis.as_raw().dot(*v_axis.as_raw()).abs() <= EPS_FEATURE_UNIT_FRAME).then_some(Self {
            origin,
            u_axis,
            v_axis,
        })
    }

    /// Replace the origin and keep the admitted axes. The axes alone satisfy
    /// the frame contract, so the result needs no new admission.
    #[must_use]
    pub fn with_origin(self, origin: FinitePoint3) -> Self {
        Self { origin, ..self }
    }

    /// Return the model-space origin.
    pub fn origin(self) -> FinitePoint3 {
        self.origin
    }
    /// Return the first unit direction.
    pub fn u_axis(self) -> UnitVector3 {
        self.u_axis
    }
    /// Return the second unit direction.
    pub fn v_axis(self) -> UnitVector3 {
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
    z_axis: UnitVector3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct FeatureCoordinateFrameWire {
    /// Model-space origin.
    origin: Point3,
    /// First unit axis of the frame.
    x_axis: Vector3,
    /// Second unit axis, perpendicular to `x_axis`.
    y_axis: Vector3,
    /// Third unit axis, completing a right-handed frame.
    z_axis: Vector3,
}

impl FeatureCoordinateFrame {
    /// Admit a finite right-handed frame within the feature unit-axis tolerance.
    pub fn new(origin: Point3, x_axis: Vector3, y_axis: Vector3, z_axis: Vector3) -> Option<Self> {
        let plane = FeatureUnitPlaneFrame::new(origin, x_axis, y_axis)?;
        let z_axis = UnitVector3::new(z_axis)?;
        Self::from_parts(plane, z_axis)
    }

    /// Build a frame from admitted axes and an admitted plane origin.
    pub fn from_parts(plane: FeatureUnitPlaneFrame, z_axis: UnitVector3) -> Option<Self> {
        let x_axis = *plane.u_axis().as_raw();
        let y_axis = *plane.v_axis().as_raw();
        let z = *z_axis.as_raw();
        (x_axis.dot(z).abs() <= EPS_FEATURE_UNIT_FRAME
            && y_axis.dot(z).abs() <= EPS_FEATURE_UNIT_FRAME
            && x_axis.cross(y_axis).dot(z) >= 1.0 - EPS_FEATURE_UNIT_FRAME)
            .then_some(Self { plane, z_axis })
    }
    /// Replace the origin and keep the admitted axes. The axes alone satisfy
    /// the frame contract, so the result needs no new admission.
    #[must_use]
    pub fn with_origin(self, origin: FinitePoint3) -> Self {
        Self {
            plane: self.plane.with_origin(origin),
            ..self
        }
    }
    /// Return the model-space origin.
    pub fn origin(self) -> FinitePoint3 {
        self.plane.origin()
    }
    /// Return the x-axis.
    pub fn x_axis(self) -> UnitVector3 {
        self.plane.u_axis()
    }
    /// Return the y-axis.
    pub fn y_axis(self) -> UnitVector3 {
        self.plane.v_axis()
    }
    /// Return the z-axis.
    pub fn z_axis(self) -> UnitVector3 {
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
            origin: frame.origin().get(),
            x_axis: frame.x_axis().into(),
            y_axis: frame.y_axis().into(),
            z_axis: frame.z_axis().into(),
        }
    }
}

/// Two finite opposite image corners with nonzero extent in both coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "[Point2; 2]", into = "[Point2; 2]")]
pub struct FeatureImageBounds([FinitePoint2; 2]);

impl FeatureImageBounds {
    /// Admit finite corners with nonzero width and height, in either order.
    pub fn new(corners: [Point2; 2]) -> Option<Self> {
        let [first, second] = corners;
        let (Some(first), Some(second)) = (FinitePoint2::new(first), FinitePoint2::new(second))
        else {
            return None;
        };
        (first.u != second.u && first.v != second.v).then_some(Self([first, second]))
    }
    /// Return the opposite corners.
    pub fn corners(self) -> [FinitePoint2; 2] {
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
        bounds.0.map(FinitePoint2::get)
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        rewrite_scalar!($name);
        impl $name {
            /// Admit finite origin and nonzero perpendicular directions without normalizing them.
            pub fn new(origin: Point3, normal: Vector3, u_axis: Vector3) -> Option<Self> {
                let origin = FinitePoint3::new(origin)?;
                let normal = FeatureDirection3::new(normal)?;
                let u_axis = FeatureDirection3::new(u_axis)?;
                Self::from_parts(origin, normal, u_axis)
            }
            /// Build a plane frame from admitted parts; only perpendicularity remains to check.
            pub fn from_parts(origin: FinitePoint3, normal: FeatureDirection3, u_axis: FeatureDirection3) -> Option<Self> {
                let $normal_length = normal.norm();
                let $u_length = u_axis.norm();
                if normal.dot(u_axis.get()).abs() > $bound { return None; }
                Some(Self { origin, normal, u_axis })
            }
            /// Replace the origin and keep the admitted directions. The
            /// orthogonality bound depends only on the directions, so the
            /// result needs no new admission.
            #[must_use]
            pub fn with_origin(self, origin: FinitePoint3) -> Self { Self { origin, ..self } }
            /// Return the model-space origin.
            pub fn origin(self) -> FinitePoint3 { self.origin }
            /// Return the plane normal with its original magnitude.
            pub fn normal(self) -> FeatureDirection3 { self.normal }
            /// Return the in-plane direction with its original magnitude.
            pub fn u_axis(self) -> FeatureDirection3 { self.u_axis }
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
                Self {
                    origin: frame.origin().get(),
                    normal: frame.normal().get(),
                    u_axis: frame.u_axis().get(),
                }
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
#[serde(deny_unknown_fields)]
struct FeatureLineSegmentWire {
    /// Start point.
    start: Point3,
    /// End point.
    end: Point3,
}

impl FeatureLineSegment {
    /// Admit distinct finite line endpoints.
    pub fn new(start: Point3, end: Point3) -> Option<Self> {
        let start = FinitePoint3::new(start)?;
        let end = FinitePoint3::new(end)?;
        Self::from_parts(start, end)
    }

    /// Build a line from admitted endpoints, checking only that they differ.
    pub fn from_parts(start: FinitePoint3, end: FinitePoint3) -> Option<Self> {
        (start != end).then_some(Self { start, end })
    }

    /// Return the start point.
    pub fn start(self) -> FinitePoint3 {
        self.start
    }

    /// Return the end point.
    pub fn end(self) -> FinitePoint3 {
        self.end
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
    pub(super) points: Vec<FinitePoint3>,
    pub(super) closed: bool,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct FeaturePolylineWire {
    /// Ordered vertices.
    points: Vec<Point3>,
    /// Whether the last vertex connects to the first.
    closed: bool,
}

impl FeaturePolyline {
    /// Admit a finite chain with at least two points, or three when closed.
    pub fn new(points: Vec<Point3>, closed: bool) -> Option<Self> {
        if points.len() < 2 || (closed && points.len() < 3) {
            return None;
        }
        let points = points
            .into_iter()
            .map(FinitePoint3::new)
            .collect::<Option<Vec<_>>>()?;
        Self::from_parts(points, closed)
    }

    /// Build a polyline from finite points if its chain is admissible.
    pub fn from_parts(points: Vec<FinitePoint3>, closed: bool) -> Option<Self> {
        if points.len() < 2
            || (closed && points.len() < 3)
            || points.windows(2).any(|pair| pair[0] == pair[1])
        {
            return None;
        }
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "FeatureEquationCurveWire",
    into = "FeatureEquationCurveWire"
)]
pub struct FeatureEquationCurve {
    pub(super) parameter: String,
    pub(super) x_expression: String,
    pub(super) y_expression: String,
    pub(super) z_expression: String,
    pub(super) domain: crate::topology::IncreasingParameterInterval,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct FeatureEquationCurveWire {
    /// Independent parameter symbol.
    parameter: String,
    /// Model-space x expression.
    x_expression: String,
    /// Model-space y expression.
    y_expression: String,
    /// Model-space z expression.
    z_expression: String,
    /// Inclusive lower parameter bound.
    start: f64,
    /// Inclusive upper parameter bound.
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
        {
            return None;
        }
        let domain = crate::topology::IncreasingParameterInterval::new([start, end])?;
        Some(Self {
            parameter,
            x_expression,
            y_expression,
            z_expression,
            domain,
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

    /// Return the admitted parameter domain.
    pub fn domain(&self) -> crate::topology::IncreasingParameterInterval {
        self.domain
    }
}

impl TryFrom<FeatureEquationCurveWire> for FeatureEquationCurve {
    type Error = &'static str;
    fn try_from(wire: FeatureEquationCurveWire) -> Result<Self, Self::Error> {
        Self::new(wire.parameter, wire.x_expression, wire.y_expression, wire.z_expression, wire.start, wire.end)
            .ok_or("equation-curve expressions must be nonblank and its start and end finite and increasing")
    }
}

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
#[serde(deny_unknown_fields)]
struct FeatureCircularArcWire {
    /// Circle center.
    center: Point3,
    /// Circle-plane normal.
    normal: Vector3,
    /// Circle radius.
    radius: PositiveLength,
    /// Start of the directed angular interval, in radians.
    start_angle: f64,
    /// End of the directed angular interval, in radians.
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
        Some(Self::from_parts(
            FinitePoint3::new(center)?,
            FeatureDirection3::new(normal)?,
            radius,
            angles,
        ))
    }

    /// Build an arc from checked parts. The argument types state the whole arc contract, so nothing is checked again.
    #[must_use]
    pub const fn from_parts(
        center: FinitePoint3,
        normal: FeatureDirection3,
        radius: PositiveLength,
        angles: crate::geometry::DirectedParameterRange,
    ) -> Self {
        Self {
            center,
            normal,
            radius,
            angles,
        }
    }

    /// Return the circle center.
    pub fn center(self) -> FinitePoint3 {
        self.center
    }

    /// Return the circle-plane normal.
    pub fn normal(self) -> FeatureDirection3 {
        self.normal
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
#[serde(deny_unknown_fields)]
struct FeatureEllipticArcWire {
    /// Ellipse center.
    center: Point3,
    /// Ellipse-plane normal.
    normal: Vector3,
    /// Major-axis direction.
    major_axis: Vector3,
    /// Major semiaxis radius.
    major_radius: PositiveLength,
    /// Minor semiaxis radius.
    minor_radius: PositiveLength,
    /// Start of the directed angular interval, in radians.
    start_angle: f64,
    /// End of the directed angular interval, in radians.
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

    /// Replace the center and keep the admitted radii, directions and
    /// interval. The center takes part in no other condition, so nothing is
    /// checked.
    #[must_use]
    pub const fn with_center(self, center: FinitePoint3) -> Self {
        Self { center, ..self }
    }

    /// The arc with both radii times `scale`, keeping the center, directions
    /// and interval.
    ///
    /// Rounding is monotone, so the scaled radii keep their order; they can
    /// round to one value, which the order admits. The scaled major radius is
    /// refused when it overflows or rounds to zero, and the scaled minor
    /// radius, which is not above it, when it rounds to zero.
    #[must_use]
    pub fn with_scaled_radii(self, scale: PositiveReal) -> Option<Self> {
        let radii =
            PositiveLength::scale_ordered_pair(self.radii.map(PositiveLength::get), scale).ok()?;
        Some(Self { radii, ..self })
    }

    /// Return the ellipse center.
    pub fn center(self) -> FinitePoint3 {
        self.center
    }

    /// Return the ellipse-plane normal.
    pub fn normal(self) -> FeatureDirection3 {
        self.normal
    }

    /// Return the major-axis direction.
    pub fn major_axis(self) -> FeatureDirection3 {
        self.major_axis
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
