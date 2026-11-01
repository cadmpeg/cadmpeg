// SPDX-License-Identifier: Apache-2.0
//! Neutral construction-feature taxonomy.

use std::collections::{BTreeMap, HashSet};

use crate::assets::AssetId;
use crate::ids::ProductDefinitionId;
use crate::ids::{
    BodyId, CurveId, EdgeId, FaceId, FeatureInputTopologyId, FeatureResultTopologyId,
    HistoricalBodyId, HistoricalEdgeId, HistoricalFaceId, HistoricalVertexId, OccurrenceId, SubdId,
    VertexId,
};
use crate::math::{Point2, Point3, Vector3};
use crate::products::JointId;
use crate::scalar::{
    Angle, FiniteReal, Fraction, InteriorAngle, Length, NonNegativeLength, NonZeroLength,
    NonZeroReal, PositiveAngle, PositiveLength, PositiveReal, SlopeAngle,
};
use crate::transform::Transform;
use crate::units::{FinitePoint2, FiniteVector, UnitVector3};
use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

mod expressions;
pub use expressions::{Expression, ExpressionError, ExpressionToken};
mod dimensions;
pub(crate) use dimensions::bound_parameter_ids as bound_dimension_parameters;
pub use dimensions::{
    Dimension, DimensionResolutionError, DimensionValue, FeatureAngularRange, FeatureScalarRange,
    RadialDimension,
};

mod body_selection;
mod decode_clone;
mod helix;
mod membership;
mod selection_overlap;
pub use helix::{HelixEvaluation, HelixLaw};

pub mod edge_treatments;
use edge_treatments::{ChamferGroup, FilletGroup, FullRoundFilletGroup, RadiusSpec};

pub mod holes;
use holes::{HoleBottom, HolePlacement, HoleProfileFilter, HoleShape};

pub mod patterns;
pub use patterns::{CircularPatternDistribution, PatternAngle, PatternDistance};
use patterns::{PatternKind, PatternSeed};

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

    /// The point with three finite coordinates. Every coordinate is finite,
    /// so the point is admitted without a check.
    #[must_use]
    pub const fn from_coordinates(x: FiniteReal, y: FiniteReal, z: FiniteReal) -> Self {
        Self(Point3 {
            x: x.get(),
            y: y.get(),
            z: z.get(),
        })
    }

    /// Reflect through the model origin. Negation keeps every coordinate
    /// finite, so the result stays admitted.
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

    /// The vector with three finite components. Every component is finite,
    /// so the vector is admitted without a check.
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
    /// Carry an admitted unit direction as a finite displacement. Its
    /// components are finite, so no admission can refuse it.
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

const EPS_FEATURE_UNIT_FRAME: f64 = 1.0e-9;

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

const EPS_FEATURE_PLANE_ORTHOGONAL: f64 = 1.0e-9;

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
    points: Vec<FinitePoint3>,
    closed: bool,
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
    parameter: String,
    x_expression: String,
    y_expression: String,
    z_expression: String,
    domain: FeatureScalarRange,
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
    start: Dimension<FiniteReal>,
    /// Inclusive upper parameter bound.
    end: Dimension<FiniteReal>,
}

impl FeatureEquationCurve {
    /// Admit nonblank expressions and finite increasing parameter bounds.
    pub fn new(
        parameter: String,
        x_expression: String,
        y_expression: String,
        z_expression: String,
        start: Dimension<FiniteReal>,
        end: Dimension<FiniteReal>,
    ) -> Option<Self> {
        if [&parameter, &x_expression, &y_expression, &z_expression]
            .into_iter()
            .any(|value| value.trim().is_empty())
        {
            return None;
        }
        let domain = FeatureScalarRange::new(start, end)?;
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
    pub fn domain(&self) -> &FeatureScalarRange {
        &self.domain
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "FeatureCircularArcWire", into = "FeatureCircularArcWire")]
pub struct FeatureCircularArc {
    center: FinitePoint3,
    normal: FeatureDirection3,
    radius: RadialDimension<PositiveLength>,
    angles: FeatureAngularRange,
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
    radius: RadialDimension<PositiveLength>,
    /// Start of the directed angular interval, in radians.
    start_angle: Dimension<Angle>,
    /// End of the directed angular interval, in radians.
    end_angle: Dimension<Angle>,
}

impl FeatureCircularArc {
    /// Admit a finite circle frame and a nonzero angular interval.
    pub fn new(
        center: Point3,
        normal: Vector3,
        radius: RadialDimension<PositiveLength>,
        angles: FeatureAngularRange,
    ) -> Option<Self> {
        Some(Self::from_parts(
            FinitePoint3::new(center)?,
            FeatureDirection3::new(normal)?,
            radius,
            angles,
        ))
    }

    /// Build an arc from checked parts. The argument types state the whole
    /// arc contract, so nothing is checked again.
    #[must_use]
    pub const fn from_parts(
        center: FinitePoint3,
        normal: FeatureDirection3,
        radius: RadialDimension<PositiveLength>,
        angles: FeatureAngularRange,
    ) -> Self {
        Self {
            center,
            normal,
            radius,
            angles,
        }
    }

    /// Return the circle center.
    pub fn center(&self) -> FinitePoint3 {
        self.center
    }

    /// Return the circle-plane normal.
    pub fn normal(&self) -> FeatureDirection3 {
        self.normal
    }

    /// Return the radius.
    pub fn radius(&self) -> &RadialDimension<PositiveLength> {
        &self.radius
    }

    /// Return the directed angular interval in radians.
    pub fn angles(&self) -> &FeatureAngularRange {
        &self.angles
    }
}

impl TryFrom<FeatureCircularArcWire> for FeatureCircularArc {
    type Error = &'static str;
    fn try_from(wire: FeatureCircularArcWire) -> Result<Self, Self::Error> {
        let angles = FeatureAngularRange::new(wire.start_angle, wire.end_angle)
            .ok_or("arc start_angle and end_angle must be finite and distinct")?;
        Self::new(wire.center, wire.normal, wire.radius, angles)
            .ok_or("circular-arc center must be finite and normal must have finite nonzero norm")
    }
}

impl From<FeatureCircularArc> for FeatureCircularArcWire {
    fn from(value: FeatureCircularArc) -> Self {
        let [start_angle, end_angle] = value.angles.into_endpoints();
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "FeatureEllipticArcWire", into = "FeatureEllipticArcWire")]
pub struct FeatureEllipticArc {
    center: FinitePoint3,
    normal: FeatureDirection3,
    major_axis: FeatureDirection3,
    radii: [RadialDimension<PositiveLength>; 2],
    angles: FeatureAngularRange,
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
    major_radius: RadialDimension<PositiveLength>,
    /// Minor semiaxis radius.
    minor_radius: RadialDimension<PositiveLength>,
    /// Start of the directed angular interval, in radians.
    start_angle: Dimension<Angle>,
    /// End of the directed angular interval, in radians.
    end_angle: Dimension<Angle>,
}

impl FeatureEllipticArc {
    /// Admit a finite ellipse frame, major/minor radii, and a directed angular interval.
    pub fn new(
        center: Point3,
        normal: Vector3,
        major_axis: Vector3,
        radii: [RadialDimension<PositiveLength>; 2],
        angles: FeatureAngularRange,
    ) -> Option<Self> {
        let center = FinitePoint3::new(center)?;
        let normal = FeatureDirection3::new(normal)?;
        let major_axis = FeatureDirection3::new(major_axis)?;
        if normal.dot(major_axis.get()).abs() > EPS_FEATURE_ELLIPSE_AXES_ORTHO
            || radii[1]
                .literal_radius()
                .zip(radii[0].literal_radius())
                .is_some_and(|(minor, major)| minor.get() > major.get())
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
    pub fn with_center(self, center: FinitePoint3) -> Self {
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
        let [major, minor] = self.radii;
        let map = |value: PositiveLength| PositiveLength::new(value.get() * scale.get()).ok_or(());
        let radii = [
            major.try_map_literal(map).ok()?,
            minor.try_map_literal(map).ok()?,
        ];
        Self::new(
            self.center.get(),
            self.normal.get(),
            self.major_axis.get(),
            radii,
            self.angles,
        )
    }

    /// Return the ellipse center.
    pub fn center(&self) -> FinitePoint3 {
        self.center
    }

    /// Return the ellipse-plane normal.
    pub fn normal(&self) -> FeatureDirection3 {
        self.normal
    }

    /// Return the major-axis direction.
    pub fn major_axis(&self) -> FeatureDirection3 {
        self.major_axis
    }

    /// Return the major and minor semiaxis radii.
    pub fn radii(&self) -> &[RadialDimension<PositiveLength>; 2] {
        &self.radii
    }

    /// Return the directed angular interval in radians.
    pub fn angles(&self) -> &FeatureAngularRange {
        &self.angles
    }
}

impl TryFrom<FeatureEllipticArcWire> for FeatureEllipticArc {
    type Error = &'static str;
    fn try_from(wire: FeatureEllipticArcWire) -> Result<Self, Self::Error> {
        let angles = FeatureAngularRange::new(wire.start_angle, wire.end_angle)
            .ok_or("arc start_angle and end_angle must be finite and distinct")?;
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
        let [start_angle, end_angle] = value.angles.into_endpoints();
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
#[serde(deny_unknown_fields)]
pub struct DraftPull {
    /// Pull direction used to measure the draft angle.
    pub direction: FeatureDirection3,
    /// Datum-plane feature that supplied the direction, when retained.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_plane"
    )]
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pull"
        )]
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
}

crate::ids::id_type!(
    /// Identifies a neutral construction feature.
    FeatureId, compose, into_string, key
);

crate::ids::id_type!(
    /// Identifies a neutral design configuration.
    ConfigurationId, compose
);

/// A named parametric model variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DesignConfiguration {
    /// Globally unique configuration id.
    pub id: ConfigurationId,
    /// Position in the design configuration list.
    #[serde(default)]
    pub ordinal: u32,
    /// Whether this configuration supplies the document's active model state.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub active: bool,
    /// Format-native configuration slot, when distinct from list order.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_index"
    )]
    pub source_index: Option<u32>,
    /// Source display name, when established.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Material override, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_material"
    )]
    pub material: Option<String>,
    /// Configuration-local named values not otherwise represented.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub properties: BTreeMap<NonBlankString, String>,
    /// Configuration-specific source expressions keyed by the overridden parameter.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub parameter_overrides: BTreeMap<ParameterId, Expression>,
    /// Bodies present when this configuration is active.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_bodies"
    )]
    pub bodies: Option<DistinctMembers<BodyId>>,
    /// Evaluated parameter state when this configuration is active.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub parameter_values: BTreeMap<ParameterId, ParameterValue>,
    /// Evaluated feature operation state when this configuration is active.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub feature_states: BTreeMap<FeatureId, ConfigurationFeatureState>,
    /// Identifier of the full-fidelity record in a native namespace.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

impl DesignConfiguration {
    /// Canonical expression in this configuration, inheriting the base when
    /// no expression override is supplied.
    pub fn parameter_expression<'a>(
        &'a self,
        ir: &'a crate::CadIr,
        id: &ParameterId,
    ) -> Option<&'a Expression> {
        self.parameter_overrides.get(id).or_else(|| {
            ir.model
                .parameters
                .iter()
                .find(|parameter| parameter.id == *id)
                .map(|parameter| &parameter.expression)
        })
    }

    /// Cached parameter value in this configuration. An overridden expression
    /// without an evaluated override has no value; the base cache is not reused.
    pub fn parameter_value<'a>(
        &'a self,
        ir: &'a crate::CadIr,
        id: &ParameterId,
    ) -> Option<&'a ParameterValue> {
        if self.parameter_overrides.contains_key(id) {
            return self.parameter_values.get(id);
        }
        self.parameter_values.get(id).or_else(|| {
            ir.model
                .parameters
                .iter()
                .find(|parameter| parameter.id == *id)
                .and_then(|parameter| parameter.value.as_ref())
        })
    }

    /// Resolve a literal or bound dimension using this configuration's cache.
    pub fn dimension_value<Q: DimensionValue>(
        &self,
        ir: &crate::CadIr,
        dimension: &Dimension<Q>,
    ) -> Option<Q> {
        match dimension {
            Dimension::Literal(value) => Some(value.clone()),
            Dimension::Parameter(id) => self
                .parameter_value(ir, id)
                .and_then(Q::from_parameter_value),
        }
    }

    /// Features suppressed when this configuration is active.
    pub fn suppressed_features(&self) -> impl Iterator<Item = &FeatureId> {
        self.feature_states
            .iter()
            .filter_map(|(feature, state)| state.is_suppressed().then_some(feature))
    }
}

/// A configuration-local difference from the base feature state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfigurationFeatureState {
    /// The feature is suppressed in this configuration.
    Suppressed {},
    /// The feature is active without a structural override.
    Active {},
    /// Construction semantics differ structurally from the base definition.
    Definition {
        /// Structural override; bound dimensional changes use parameter overrides.
        definition: Box<FeatureDefinition>,
    },
}

impl ConfigurationFeatureState {
    /// Whether this configuration suppresses the feature.
    pub const fn is_suppressed(&self) -> bool {
        matches!(self, Self::Suppressed {})
    }
    /// Structural definition override, if one is supplied.
    pub const fn definition(&self) -> Option<&FeatureDefinition> {
        match self {
            Self::Definition { definition } => Some(definition),
            _ => None,
        }
    }
}

crate::ids::id_type!(
    /// Identifies a neutral design parameter.
    ParameterId, compose, key
);

/// A named design expression, optionally owned by a construction feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DesignParameter {
    /// Product definition whose frame contains this parameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_product_definition"
    )]
    pub product_definition: Option<ProductDefinitionId>,
    /// Globally unique parameter id.
    pub id: ParameterId,
    /// Feature that consumes this parameter; absent for a document parameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner"
    )]
    pub owner: Option<FeatureId>,
    /// Position among parameters in the same ownership scope.
    #[serde(default)]
    pub ordinal: u32,
    /// Source parameter name.
    pub name: String,
    /// Canonical expression with identity-bound parameter references.
    pub expression: Expression,
    /// Whether this parameter drives its constrained dimension.
    pub driving: bool,
    /// Geometric display semantics carried by the dimension expression.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_display"
    )]
    pub display: Option<DimensionDisplay>,
    /// Evaluated scalar when available.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_value"
    )]
    pub value: Option<ParameterValue>,
    /// Source parameter properties not represented by another field.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub properties: BTreeMap<NonBlankString, String>,
    /// Product-manufacturing dimension semantics, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pmi"
    )]
    pub pmi: Option<crate::ids::PmiId>,
    /// Identifier of the full-fidelity source parameter record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// Geometric interpretation requested by a dimension display modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum DimensionDisplay {
    /// Displays the dimension as a diameter.
    Diameter,
    /// Displays the dimension as a radius.
    Radius,
}

/// Canonical scalar value of a literal design parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
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

impl ParameterValue {
    /// Render an unbound literal expression using canonical quantity units.
    pub fn literal_expression(&self) -> Expression {
        Expression::literal(self)
    }
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
}

impl<'de> Deserialize<'de> for PolygonSideCount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u32::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("polygon sides must be at least three"))
    }
}

/// A feature definition and the bodies its evaluation produces.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureEvaluation {
    definition: FeatureDefinition,
    outputs: DistinctMembers<BodyId>,
}

impl FeatureEvaluation {
    /// Construct an evaluation from its semantics and produced bodies.
    #[must_use]
    pub const fn new(definition: FeatureDefinition, outputs: DistinctMembers<BodyId>) -> Self {
        Self {
            definition,
            outputs,
        }
    }

    /// Construct an evaluation with no produced bodies.
    #[must_use]
    pub const fn from_definition(definition: FeatureDefinition) -> Self {
        Self {
            definition,
            outputs: DistinctMembers(Vec::new()),
        }
    }

    /// Consume the evaluation and return its semantics and produced bodies.
    pub fn into_parts(self) -> (FeatureDefinition, DistinctMembers<BodyId>) {
        (self.definition, self.outputs)
    }

    /// Return the neutral construction semantics.
    pub const fn definition(&self) -> &FeatureDefinition {
        &self.definition
    }

    /// Return the produced or modified body identities.
    pub const fn outputs(&self) -> &Vec<BodyId> {
        &self.outputs.0
    }

    /// Edit the semantics and the produced bodies together.
    pub fn edit(
        &mut self,
        edit: impl FnOnce(&mut FeatureDefinition, &mut DistinctMembers<BodyId>),
    ) {
        edit(&mut self.definition, &mut self.outputs);
    }

    /// Replace the construction semantics.
    pub fn set_definition(&mut self, definition: FeatureDefinition) {
        self.definition = definition;
    }

    /// Replace the produced body identities.
    pub fn set_outputs(&mut self, outputs: DistinctMembers<BodyId>) {
        self.outputs = outputs;
    }
}

/// An ordered neutral construction feature and its resulting bodies.
///
/// Construction requires an admitted identity and feature evaluation. There is
/// no public [`Default`]: an empty id with an arbitrary definition is illegal.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    /// Product definition whose frame contains this feature.
    pub product_definition: Option<ProductDefinitionId>,
    /// Globally unique feature id.
    pub id: FeatureId,
    /// Stable construction order within the source history.
    pub ordinal: u64,
    /// Source display name.
    pub name: Option<String>,
    /// Whether evaluation of this feature is disabled.
    pub suppressed: Option<bool>,
    /// Whether this feature's stored evaluation may no longer match its inputs.
    pub needs_recompute: bool,
    /// Earlier features consumed during regeneration, in source operand order.
    pub dependencies: DistinctMembers<FeatureId>,
    /// Source operation attributes not consumed by the neutral definition.
    pub source_properties: BTreeMap<NonBlankString, String>,
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

#[derive(Serialize)]
pub(crate) struct FeatureWriteWire<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    product_definition: &'a Option<ProductDefinitionId>,
    id: &'a FeatureId,
    ordinal: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: &'a Option<String>,
    suppressed: &'a Option<bool>,
    needs_recompute: bool,
    /// Regeneration predecessor of a feature that has no tree parent.
    #[serde(skip_serializing_if = "Option::is_none")]
    regeneration_parent: Option<&'a FeatureId>,
    #[serde(skip_serializing_if = "DistinctMembers::is_empty")]
    dependencies: &'a DistinctMembers<FeatureId>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    source_properties: &'a BTreeMap<NonBlankString, String>,
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
    pub(crate) fn new(feature: &'a Feature, regeneration_parent: Option<&'a FeatureId>) -> Self {
        Self {
            product_definition: &feature.product_definition,
            id: &feature.id,
            ordinal: feature.ordinal,
            name: &feature.name,
            suppressed: &feature.suppressed,
            needs_recompute: feature.needs_recompute,
            regeneration_parent,
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

    fn standalone(feature: &'a Feature) -> Self {
        Self::new(feature, None)
    }
}

/// One row of the model route's feature list.
///
/// `regeneration_parent` is a model-route datum: the read hands it to
/// [`crate::document::Model`] with the feature, which is the only owner that
/// can place the edge. The standalone route reads [`FeatureReadWire`], which
/// declares no such field, so the difference between the two routes is a
/// field, not a check.
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(crate) struct FeatureRowWire {
    #[serde(default, deserialize_with = "deserialize_product_definition")]
    product_definition: Option<ProductDefinitionId>,
    id: FeatureId,
    ordinal: u64,
    #[serde(default, deserialize_with = "deserialize_name")]
    name: Option<String>,
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    suppressed: Option<bool>,
    needs_recompute: bool,
    #[serde(default, deserialize_with = "deserialize_regeneration_parent")]
    regeneration_parent: Option<FeatureId>,
    #[serde(default, deserialize_with = "deserialize_dependencies")]
    dependencies: DistinctMembers<FeatureId>,
    #[serde(default)]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    source_properties: BTreeMap<NonBlankString, String>,
    #[serde(default, deserialize_with = "deserialize_source_tag")]
    source_tag: Option<String>,
    #[serde(default, deserialize_with = "deserialize_source_text")]
    source_text: Option<String>,
    #[serde(default)]
    source_content: FeatureContent,
    #[serde(default)]
    #[cfg_attr(feature = "schema", schemars(with = "Vec<BodyId>"))]
    outputs: DistinctMembers<BodyId>,
    definition: FeatureDefinition,
    #[serde(default, deserialize_with = "deserialize_native_ref")]
    native_ref: Option<String>,
}

impl FeatureRowWire {
    pub(crate) fn into_parts(self) -> (Feature, Option<FeatureId>) {
        let regeneration_parent = self.regeneration_parent;
        let feature = FeatureReadWire {
            product_definition: self.product_definition,
            id: self.id,
            ordinal: self.ordinal,
            name: self.name,
            suppressed: self.suppressed,
            needs_recompute: self.needs_recompute,
            dependencies: self.dependencies,
            source_properties: self.source_properties,
            source_tag: self.source_tag,
            source_text: self.source_text,
            source_content: self.source_content,
            outputs: self.outputs,
            definition: self.definition,
            native_ref: self.native_ref,
        }
        .into_feature();
        (feature, regeneration_parent)
    }
}

/// The standalone feature wire: one feature outside any model.
///
/// It declares no `regeneration_parent`, so a row of the model route does not
/// read back as a standalone feature and the parent edge cannot arrive without
/// the model that owns it. `deny_unknown_fields` states the refusal.
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct FeatureReadWire {
    #[serde(default, deserialize_with = "deserialize_product_definition")]
    product_definition: Option<ProductDefinitionId>,
    id: FeatureId,
    ordinal: u64,
    #[serde(default, deserialize_with = "deserialize_name")]
    name: Option<String>,
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    suppressed: Option<bool>,
    needs_recompute: bool,
    #[serde(default, deserialize_with = "deserialize_dependencies")]
    dependencies: DistinctMembers<FeatureId>,
    #[serde(default)]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    source_properties: BTreeMap<NonBlankString, String>,
    #[serde(default, deserialize_with = "deserialize_source_tag")]
    source_tag: Option<String>,
    #[serde(default, deserialize_with = "deserialize_source_text")]
    source_text: Option<String>,
    #[serde(default)]
    source_content: FeatureContent,
    #[serde(default)]
    #[cfg_attr(feature = "schema", schemars(with = "Vec<BodyId>"))]
    outputs: DistinctMembers<BodyId>,
    definition: FeatureDefinition,
    #[serde(default, deserialize_with = "deserialize_native_ref")]
    native_ref: Option<String>,
}

impl FeatureReadWire {
    fn into_feature(self) -> Feature {
        Feature {
            product_definition: self.product_definition,
            id: self.id,
            ordinal: self.ordinal,
            name: self.name,
            suppressed: self.suppressed,
            needs_recompute: self.needs_recompute,
            dependencies: self.dependencies,
            source_properties: self.source_properties,
            source_tag: self.source_tag,
            source_text: self.source_text,
            source_content: self.source_content,
            evaluation: FeatureEvaluation::new(self.definition, self.outputs),
            native_ref: self.native_ref,
        }
    }
}

/// Read one feature outside any model.
///
/// The model route reads `FeatureRowWire` and splits the row with `into_parts`.
/// Standalone reconstruction reads the same feature fields without a parent edge.
impl Feature {
    /// Borrow the feature's canonical construction definition.
    pub const fn definition(&self) -> &FeatureDefinition {
        self.evaluation.definition()
    }
}

impl<'de> Deserialize<'de> for Feature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(FeatureReadWire::deserialize(deserializer)?.into_feature())
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
#[serde(deny_unknown_fields)]
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
    /// Sampled geometry of edges present in this state, at most one entry
    /// per edge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_geometry: Vec<HistoricalEdgeGeometry>,
    /// Full-fidelity source state reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// Geometry of one historical edge in a feature's input state.
///
/// Positions and directions are in millimetres in the frame of the body that
/// the feature evaluates, before the feature applies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HistoricalEdgeGeometry {
    /// Edge of the owning input state.
    pub edge: HistoricalEdgeId,
    /// Point on the edge at the midpoint of its carrier parameter interval.
    pub interior: FinitePoint3,
    /// Unit carrier tangent at `interior`, in the carrier's parameter
    /// direction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_geometry_tangent"
    )]
    pub tangent: Option<UnitVector3>,
    /// Position of the start vertex.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_geometry_start"
    )]
    pub start: Option<FinitePoint3>,
    /// Position of the end vertex.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_geometry_end"
    )]
    pub end: Option<FinitePoint3>,
    /// Faces of the owning input state that use the edge.
    #[serde(default, skip_serializing_if = "DistinctMembers::is_empty")]
    pub faces: DistinctMembers<HistoricalFaceId>,
}

/// One member of a feature result state: a body, face, edge, or vertex.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SelectionMember {
    /// A feature-local body identity.
    Body {
        /// Feature-local identity.
        id: NonBlankString,
    },
    /// A feature-local face identity.
    Face {
        /// Feature-local identity.
        id: NonBlankString,
    },
    /// A feature-local edge identity.
    Edge {
        /// Feature-local identity.
        id: NonBlankString,
    },
    /// A feature-local vertex identity.
    Vertex {
        /// Feature-local identity.
        id: NonBlankString,
    },
}

/// The members of a feature result state, sorted into kinds.
///
/// The wire carries one non-empty member list, so "all four lists are empty"
/// has no spelling and no arm refuses it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    try_from = "NonEmptyMembers<SelectionMember>",
    into = "Vec<SelectionMember>"
)]
pub struct FeatureResultMembers {
    bodies: Vec<NonBlankString>,
    faces: Vec<NonBlankString>,
    edges: Vec<NonBlankString>,
    vertices: Vec<NonBlankString>,
}

/// Refusal for a feature result member list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FeatureResultMemberError {
    /// The member list carries no member.
    #[error(transparent)]
    Members(#[from] BodySelectionError),
    /// A feature-local body identity occurs more than once.
    #[error("feature result bodies must be distinct")]
    RepeatedBody,
    /// A feature-local face identity occurs more than once.
    #[error("feature result faces must be distinct")]
    RepeatedFace,
    /// A feature-local edge identity occurs more than once.
    #[error("feature result edges must be distinct")]
    RepeatedEdge,
    /// A feature-local vertex identity occurs more than once.
    #[error("feature result vertices must be distinct")]
    RepeatedVertex,
}

impl TryFrom<NonEmptyMembers<SelectionMember>> for FeatureResultMembers {
    type Error = FeatureResultMemberError;

    fn try_from(members: NonEmptyMembers<SelectionMember>) -> Result<Self, Self::Error> {
        let mut sorted = Self {
            bodies: Vec::new(),
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: Vec::new(),
        };
        for member in members {
            let (target, id) = match member {
                SelectionMember::Body { id } => (&mut sorted.bodies, id),
                SelectionMember::Face { id } => (&mut sorted.faces, id),
                SelectionMember::Edge { id } => (&mut sorted.edges, id),
                SelectionMember::Vertex { id } => (&mut sorted.vertices, id),
            };
            target
                .try_reserve(1)
                .map_err(|_| BodySelectionError::Allocation)?;
            target.push(id);
        }
        sorted.validate()
    }
}

impl FeatureResultMembers {
    /// Admit distinct local members in each arena, retaining their source order.
    pub fn new(
        bodies: Vec<NonBlankString>,
        faces: Vec<NonBlankString>,
        edges: Vec<NonBlankString>,
        vertices: Vec<NonBlankString>,
    ) -> Result<Self, FeatureResultMemberError> {
        Self {
            bodies,
            faces,
            edges,
            vertices,
        }
        .validate()
    }

    fn validate(self) -> Result<Self, FeatureResultMemberError> {
        if self.bodies.is_empty()
            && self.faces.is_empty()
            && self.edges.is_empty()
            && self.vertices.is_empty()
        {
            return Err(BodySelectionError::Empty.into());
        }
        for (error, values) in [
            (FeatureResultMemberError::RepeatedBody, &self.bodies),
            (FeatureResultMemberError::RepeatedFace, &self.faces),
            (FeatureResultMemberError::RepeatedEdge, &self.edges),
            (FeatureResultMemberError::RepeatedVertex, &self.vertices),
        ] {
            if !membership::distinct(values, |_| true) {
                return Err(error);
            }
        }
        Ok(self)
    }
}

/// Persistent feature-local topology identities in one regenerated result.
///
/// These identities describe intermediate history results that need not be
/// members of the saved current model topology. Generated feature selections
/// address members through the producing feature and the corresponding local
/// identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FeatureResultTopology {
    /// Globally unique result-state id.
    pub id: FeatureResultTopologyId,
    /// Feature that produces this state.
    pub output_of: FeatureId,
    /// Feature-local identities in stable source order, tagged by arena.
    #[cfg_attr(feature = "schema", schemars(with = "Vec<SelectionMember>"))]
    members: FeatureResultMembers,
    /// Full-fidelity source state reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

impl FeatureResultTopology {
    /// Construct a result record from checked local members.
    pub fn new(
        id: FeatureResultTopologyId,
        output_of: FeatureId,
        members: FeatureResultMembers,
        native_ref: Option<String>,
    ) -> Self {
        Self {
            id,
            output_of,
            members,
            native_ref,
        }
    }

    /// Feature-local body identities.
    pub fn bodies(&self) -> &[NonBlankString] {
        &self.members.bodies
    }
    /// Feature-local face identities.
    pub fn faces(&self) -> &[NonBlankString] {
        &self.members.faces
    }
    /// Feature-local edge identities.
    pub fn edges(&self) -> &[NonBlankString] {
        &self.members.edges
    }
    /// Feature-local vertex identities.
    pub fn vertices(&self) -> &[NonBlankString] {
        &self.members.vertices
    }
}

/// Ordered source content with distinct parameter and child-feature references.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct FeatureContent(Vec<FeatureSourceContent>);

impl TryFrom<Vec<FeatureSourceContent>> for FeatureContent {
    type Error = FeatureCollectionError;
    fn try_from(value: Vec<FeatureSourceContent>) -> Result<Self, Self::Error> {
        Self::build(value).map_err(FeatureCollectionError::Invalid)
    }
}
impl FeatureContent {
    fn build(value: Vec<FeatureSourceContent>) -> Result<Self, &'static str> {
        if !membership::distinct(&value, |item| {
            !matches!(item, FeatureSourceContent::Text(_))
        }) {
            return Err("source_content repeats a parameter or child-feature reference");
        }
        Ok(Self(value))
    }

    /// Constructs text-only content in source order, retaining repeated text.
    pub fn text(values: impl IntoIterator<Item = String>) -> Self {
        Self(values.into_iter().map(FeatureSourceContent::Text).collect())
    }

    /// Append decoded content without repeating a parameter or child-feature reference.
    pub fn push(&mut self, value: FeatureSourceContent) -> Result<(), FeatureCollectionError> {
        if !matches!(value, FeatureSourceContent::Text(_)) {
            for member in &self.0 {
                if member == &value {
                    return Err(FeatureCollectionError::Invalid(
                        "source_content repeats a parameter or child-feature reference",
                    ));
                }
            }
        }

        self.0.push(value);
        Ok(())
    }

    /// Whether the sequence has no content.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
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
        let values = Vec::<FeatureSourceContent>::deserialize(deserializer)?;
        Self::build(values).map_err(serde::de::Error::custom)
    }
}
/// One item in a source feature's mixed-content sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum FeatureSourceContent {
    /// Literal text between child records.
    Text(String),
    /// Dimension or equation parameter at this position.
    Parameter(ParameterId),
    /// Nested feature record at this position.
    Feature(FeatureId),
}

/// Parametric support of an offset datum plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "reference", rename_all = "snake_case", deny_unknown_fields)]
pub enum DatumPlaneReference {
    /// Another datum-plane feature.
    Feature {
        /// Referenced datum-plane feature.
        feature: FeatureId,
    },
    /// A selected planar face whose geometry defines the support plane.
    Face {
        /// Selected planar face.
        face: FaceSelection,
    },
    /// A resolved support plane without a face identity.
    ResolvedPlane {
        /// Finite support plane with nonzero perpendicular directions.
        frame: FeatureSupportPlaneFrame,
    },
}

/// Sketch point operand resolved by a datum-point construction or retained in
/// native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
        /// Canonical fraction from the path start, in the closed unit interval.
        fraction: Dimension<Fraction>,
    },
}

/// Rule that maps a raster decal onto its selected faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum DecalMapping {
    /// Scale the complete raster to the selected faces' native parameter domain.
    FitToFaces,
}

impl DatumPointConstruction {
    /// Return construction features referenced by this rule.
    pub fn feature_references(&self) -> impl Iterator<Item = &FeatureId> {
        let references = match self {
            Self::ThreePlaneIntersection { planes } => planes.each_ref().map(|plane| match plane {
                DatumPlaneReference::Feature { feature } => Some(feature),
                DatumPlaneReference::Face { .. } | DatumPlaneReference::ResolvedPlane { .. } => {
                    None
                }
            }),
            Self::EdgePlaneIntersection {
                plane: DatumPlaneReference::Feature { feature },
                ..
            } => [Some(feature), None, None],
            Self::Vertex {
                vertex: VertexSelection::Generated { vertex, .. },
            } => [Some(&vertex.feature), None, None],
            Self::CircleCenter { .. }
            | Self::TwoEdgeIntersection { .. }
            | Self::Vertex { .. }
            | Self::SketchPoint { .. }
            | Self::EdgePlaneIntersection { .. }
            | Self::DistanceOnEdge { .. } => [None; 3],
        };
        references.into_iter().flatten()
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
        BodySelection::ResolvedSet { members } => Some(members.count()),
        BodySelection::Historical { bodies, .. } => Some(bodies.len()),
        BodySelection::HistoricalSet { members, .. } => Some(members.count()),
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
    ($name:ident, $wire:ident, $selection:ty, $first:ident, $second:ident, $arity:expr, $overlap:ident $(, $edit:ident)?) => {
        /// Two admitted operand selections for one feature operation.
        #[derive(Debug, Clone, PartialEq, Serialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        pub struct $name {
            $first: $selection,
            $second: $selection,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $wire {
            $first: $selection,
            $second: $selection,
        }

        impl $name {
            /// Admit the operand arity and disjoint membership.
            pub fn new(
                $first: $selection,
                $second: $selection,
            ) -> Result<Self, &'static str> {
                if !($arity)(&$first) || selection_overlap::$overlap(&$first, &$second) {
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

            $(
            /// Admit both edited selections before replacing either operand.
            pub fn $edit(
                &mut self,
                edit: impl FnOnce(&mut $selection, &mut $selection),
            ) -> Result<(), &'static str> {
                let mut first = self.$first.clone();
                let mut second = self.$second.clone();
                edit(&mut first, &mut second);
                *self = Self::new(first, second)?;
                Ok(())
            }
            )?
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
    |_: &FaceSelection| true,
    face_selections_overlap
);
selection_operands!(
    ReplaceFaceOperands,
    ReplaceFaceOperandsWire,
    FaceSelection,
    targets,
    replacements,
    |_: &FaceSelection| true,
    face_selections_overlap,
    try_edit
);
selection_operands!(
    SectionOperands,
    SectionOperandsWire,
    BodySelection,
    first,
    second,
    |_: &BodySelection| true,
    body_selections_overlap
);
selection_operands!(
    CombineOperands,
    CombineOperandsWire,
    BodySelection,
    target,
    tools,
    |first| known_body_count(first).is_none_or(|count| count == 1),
    body_selections_overlap,
    try_edit
);
selection_operands!(
    TrimBodyOperands,
    TrimBodyOperandsWire,
    BodySelection,
    targets,
    tools,
    |_: &BodySelection| true,
    body_selections_overlap
);

impl CombineOperands {
    /// Move both selections out of this admitted pair.
    pub fn into_parts(self) -> (BodySelection, BodySelection) {
        (self.target, self.tools)
    }
}

impl ReplaceFaceOperands {
    /// Move both selections out of this admitted pair.
    pub fn into_parts(self) -> (FaceSelection, FaceSelection) {
        (self.targets, self.replacements)
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
#[serde(deny_unknown_fields)]
struct TreeChildrenWire {
    #[serde(default)]
    children: Vec<FeatureId>,
    #[serde(default, deserialize_with = "deserialize_active_child")]
    active_child: Option<FeatureId>,
}

impl TreeChildren {
    /// Admit distinct children and an active member that belongs to them.
    pub fn new(
        children: Vec<FeatureId>,
        active_child: Option<FeatureId>,
    ) -> Result<Self, FeatureCollectionError> {
        Self::build(children, active_child).map_err(FeatureCollectionError::Invalid)
    }

    fn build(
        children: Vec<FeatureId>,
        active_child: Option<FeatureId>,
    ) -> Result<Self, &'static str> {
        if active_child
            .as_ref()
            .is_some_and(|active| !children.contains(active))
        {
            return Err("active_child must belong to children");
        }
        if !membership::distinct(&children, |_| true) {
            return Err("members must be distinct");
        }
        Ok(Self {
            children: DistinctMembers(children),
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
        Self::build(wire.children, wire.active_child).map_err(serde::de::Error::custom)
    }
}

/// A feature operation: neutral construction semantics with an explicit native
/// escape hatch, and no post-processing layer of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "definition", rename_all = "snake_case", deny_unknown_fields)]
pub enum FeatureOperation {
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
    /// Independent bodies introduced by a copy-and-paste operation. The copies
    /// are the feature's outputs, so the selection carries only its native
    /// expression.
    InsertBodies {
        /// Native selection expression of the copied bodies.
        bodies: InsertedBodies,
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
    /// Thread attached to a cylindrical face.
    ///
    /// The source may also model the helical form in the body geometry;
    /// `modeled` states whether it does, and absent means unproven. A field
    /// that the source encoding does not prove stays absent. F3D threads set
    /// `standard`, `designation` and `pitch` and stay definition-incomplete
    /// while `modeled` is absent (DR-143); the `nx`, `sldprt`, and `inventor` codecs do not
    /// set the four new fields.
    CosmeticThread {
        /// Cylindrical face carrying the thread.
        face: FaceSelection,
        /// Thread standard family or profile name, as the source spells it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thread_standard"
        )]
        standard: Option<NonBlankString>,
        /// Size designation within the standard, as the source spells it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thread_designation"
        )]
        designation: Option<NonBlankString>,
        /// Nominal thread diameter, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_diameter"
        )]
        diameter: Option<Dimension<PositiveLength>>,
        /// Thread pitch along the axis, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thread_pitch"
        )]
        pitch: Option<Dimension<PositiveLength>>,
        /// Whether the body geometry already contains the helical form.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thread_modeled"
        )]
        modeled: Option<bool>,
        /// Axial extent of the thread, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_extent"
        )]
        extent: Option<CosmeticThreadExtent>,
    },
    /// Raster reference image placed in model space.
    ReferenceImage {
        /// Embedded or external raster resource.
        asset: AssetId,
        /// Whether the raster is visible in the source presentation.
        #[serde(default = "crate::default_true")]
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_opacity"
        )]
        opacity: Option<Dimension<Fraction>>,
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_opacity"
        )]
        opacity: Option<Dimension<Fraction>>,
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_reference"
        )]
        reference: Option<DatumPlaneReference>,
        /// Signed normal offset from the source plane.
        distance: Dimension<Length>,
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_construction"
        )]
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
        sides: Dimension<PolygonSideCount>,
        /// Center-to-vertex distance.
        circumradius: Dimension<PositiveLength>,
    },
    /// Rectangular bounded planar face in the local XY plane.
    PlanarPatch {
        /// Length along the local x-axis.
        length: Dimension<PositiveLength>,
        /// Width along the local y-axis.
        width: Dimension<PositiveLength>,
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
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        dimensions: Option<[Dimension<PositiveLength>; 3]>,
        /// Local-to-model placement, when resolved.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_bidirectional"
        )]
        bidirectional: Option<bool>,
    },
    /// Shapes projected along a direction onto a support face selection.
    ProjectOnSurface {
        /// Ordered shapes and subelements projected onto the support.
        sources: PathRef,
        /// Support face selection receiving the projection. The selection
        /// admits any number of faces.
        support_face: FaceSelection,
        /// Unit projection direction.
        direction: UnitVector3,
        /// Result topology retained from the projected shapes.
        mode: SurfaceProjectionMode,
        /// Normal extrusion height used to turn projected faces into solids.
        height: Dimension<NonNegativeLength>,
        /// Normal offset applied to the projected result.
        offset: Dimension<Length>,
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
        radius: Dimension<PositiveLength>,
        /// Axial or radial construction law.
        shape: HelixShape,
        /// Angular position at the curve start.
        #[serde(default)]
        start_angle: Dimension<Angle>,
        /// Whether angular travel is clockwise when viewed along the axis.
        clockwise: bool,
        /// Number of turns per generated curve subdivision, when requested.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_segment_turns"
        )]
        segment_turns: Option<Dimension<PositiveReal>>,
        /// Persisted construction algorithm generation, when selectable.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_construction_style"
        )]
        construction_style: Option<HelixConstructionStyle>,
    },
    /// Circular helix with retained native axis placement.
    HelixNativeAxis {
        /// Source-native record carrying the unresolved construction axis.
        #[serde(deserialize_with = "deserialize_local_axis_native_ref")]
        axis_native_ref: NonBlankString,
        /// The two actual authored controls of axial travel.
        law: HelixLaw,
        /// Angular position at the curve start.
        start_angle: Dimension<Angle>,
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
        radius: RadialDimension<PositiveLength>,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Solid torus primitive.
    Torus {
        /// Torus center in model space.
        center: FinitePoint3,
        /// Unit normal of the torus center plane.
        axis: UnitVector3,
        /// Positive distance from the center to the tube centerline.
        major_radius: RadialDimension<PositiveLength>,
        /// Positive tube radius.
        minor_radius: RadialDimension<PositiveLength>,
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_sketch"
        )]
        sketch: Option<crate::sketches::SpatialSketchId>,
    },
    /// Reusable planar sketch geometry.
    SketchBlockDefinition {
        /// Neutral sketch geometry owned by the block, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_sketch"
        )]
        sketch: Option<crate::sketches::SketchId>,
    },
    /// Placement of one reusable sketch-block definition.
    SketchBlockInstance {
        /// Referenced block definition, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_block"
        )]
        block: Option<FeatureId>,
        /// Affine placement in the owning sketch space, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_placement"
        )]
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
        /// Local-to-model placement of the primitive's feature frame, when
        /// resolved. Without it the feature frame is unknown.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_primitive_placement"
        )]
        placement: Option<FeatureRigidPlacement>,
        /// Boolean combination with an existing `PartDesign` body.
        op: BooleanOp,
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_path"
        )]
        path: Option<PathRef>,
        /// Rule used to orient cross-sections along the path.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_orientation"
        )]
        orientation: Option<SweepOrientation>,
        /// Corner continuation used where path segments meet.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_transition"
        )]
        transition: Option<SweepTransition>,
        /// Interpolation law used between multiple cross-sections.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_transformation"
        )]
        transformation: Option<SweepTransformation>,
        /// Whether tangent-connected edges are included in the primary path.
        #[serde(default)]
        path_tangent: bool,
        /// Whether linear edges and planar faces are simplified after construction.
        #[serde(default)]
        linearize: bool,
        /// Total profile twist along the path, when specified.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_twist"
        )]
        twist: Option<Dimension<Angle>>,
        /// Fractions of the selected path swept on either side of the profile.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_path_extent"
        )]
        path_extent: Option<SweepPathExtent>,
        /// Guide rail and its independently consumed extent, when present.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_guide_rail"
        )]
        guide_rail: Option<SweepGuideRail>,
        /// Profile taper angle over the swept extent, when specified.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_taper"
        )]
        taper: Option<Dimension<Angle>>,
        /// End-to-start profile scale ratio, when specified.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_scale"
        )]
        scale: Option<Dimension<PositiveReal>>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
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
        thickness: Dimension<PositiveLength>,
        /// Distribution of thickness relative to the profile plane.
        side: SheetMetalThicknessSide,
    },
    /// Flat plate attached to an existing sheet edge.
    /// Replay the plate before its paired `SheetMetalBend`. The bend builds the joining region.
    SheetMetalAttachedPlate {
        /// Closed profile in the attached plate's sketch plane.
        profile: PlanarProfileRef,
        /// Finished sheet thickness.
        thickness: PositiveLength,
        /// Distribution of thickness relative to the profile plane.
        side: SheetMetalThicknessSide,
        /// Selected edges of the existing sheet.
        attachment: EdgeSelection,
    },
    /// Bend region joining a separate flat plate to the selected sheet edge.
    /// Replay after the paired `SheetMetalAttachedPlate`. Bend dimensions belong to this operation.
    SheetMetalBend {
        /// Preceding attached plate that supplies the planar profile and thickness.
        plate: FeatureId,
        /// Selected edges of the existing sheet, in this bend's input topology.
        attachment: EdgeSelection,
        /// Inside bend radius.
        inside_radius: PositiveLength,
        /// Relief form and evaluated length operands.
        relief: SheetMetalRelief,
        /// Transition form and evaluated radius.
        transition: SheetMetalTransition,
    },
    /// Corner evaluation on a bend with an explicit geometric effect.
    SheetMetalCorner {
        /// Bend on which the corner is evaluated.
        bend: FeatureId,
        /// Effect on the input body.
        effect: SheetMetalCornerEffect,
    },
    /// Sheet-metal wall grown from selected edges of an existing sheet body.
    SheetMetalEdgeFlange {
        /// Edges the flange is grown from.
        edges: EdgeSelection,
        /// Height of the flange wall.
        height: SheetMetalFlangeHeight,
        /// Angle between the flange wall and its source face.
        angle: Dimension<Angle>,
        /// Face pair the height is measured from.
        height_datum: SheetMetalHeightDatum,
        /// Placement of the bend region against the selected edge.
        bend_position: SheetMetalBendPosition,
        /// Extent of the flange along the selected edge.
        width: SheetMetalFlangeWidth,
        /// Inside radius of the bend joining the flange to its source face.
        bend_radius: Dimension<PositiveLength>,
    },
    /// Sheet-metal hem grown from one or more selected edges.
    SheetMetalHem {
        /// Edges the hem is grown from.
        edges: EdgeSelection,
        /// Placement of the hem bend against its selected edge.
        bend_position: SheetMetalHemBendPosition,
        /// Dimensional owner layout carried by the source hem.
        form: SheetMetalHemForm,
        /// Direction of the hem fold, when the source carries a proven value.
        direction: SheetMetalHemDirection,
        /// Inside radius of the bend joining the hem to its source face.
        bend_radius: Dimension<PositiveLength>,
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_bodies"
        )]
        bodies: Option<BodySelection>,
        /// Faces removed to open the shell.
        removed_faces: FaceSelection,
        /// Wall thickness left after shelling, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thickness"
        )]
        thickness: Option<Dimension<PositiveLength>>,
        /// Whether the wall is grown outward from the original boundary,
        /// as opposed to inward, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_outward"
        )]
        outward: Option<bool>,
        /// Offset construction used to generate the wall.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_mode"
        )]
        mode: Option<ShellMode>,
        /// Corner continuation law used between offset faces.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_join"
        )]
        join: Option<ShellJoin>,
        /// Whether intersecting offset regions are resolved during construction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_resolve_intersections"
        )]
        resolve_intersections: Option<bool>,
        /// Whether self-intersecting offset regions may be retained.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_self_intersections"
        )]
        allow_self_intersections: Option<bool>,
    },
    /// Offsets an entire source shape without removing opening faces.
    OffsetShape {
        /// Source shape or body to offset.
        source: BodySelection,
        /// Signed normal offset in canonical millimeters.
        distance: Dimension<NonZeroLength>,
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_approximate"
        )]
        approximate: Option<bool>,
    },
    /// Reflects one source shape across a model-space plane.
    MirrorShape {
        /// Shape transformed into the mirrored result.
        source: BodySelection,
        /// Point on the persisted resolved mirror plane.
        plane_origin: FinitePoint3,
        /// Unit normal of the persisted resolved mirror plane.
        plane_normal: UnitVector3,
        /// Native plane, face, or circle reference that supplied the resolved plane.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_plane_reference"
        )]
        plane_reference: Option<FaceSelection>,
    },
    /// Adds material normal to selected faces.
    Thicken {
        /// Faces offset by the operation.
        faces: FaceSelection,
        /// Finished added thickness, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thickness"
        )]
        thickness: Option<Dimension<PositiveLength>>,
        /// Distribution of thickness relative to the selected faces, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_side"
        )]
        side: Option<ThickenSide>,
    },
    /// Surface copied at a signed normal offset from selected support faces.
    OffsetSurface {
        /// Faces supplying the source surface geometry.
        faces: FaceSelection,
        /// Signed normal offset in canonical millimeters.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        distance: Option<Dimension<Length>>,
    },
    /// Joins selected surface bodies along coincident or near-coincident boundaries.
    KnitSurface {
        /// Faces participating in the knit operation.
        faces: FaceSelection,
        /// Whether coincident face and edge entities are merged.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        merge_entities: Option<bool>,
        /// Whether a closed result is converted to a solid body.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        create_solid: Option<bool>,
        /// Maximum boundary gap accepted by the operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_gap_tolerance"
        )]
        gap_tolerance: Option<Dimension<NonNegativeLength>>,
    },
    /// Joins sheet or solid bodies along coincident boundaries.
    SewBodies {
        /// Bodies participating in the sew operation.
        bodies: SewBodySelection,
        /// Maximum accepted boundary gap, when resolved.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        gap_tolerance: Option<Dimension<PositiveLength>>,
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
        continuity: FilledSurfaceContinuityState,
        /// Whether the generated patch is merged into adjacent surface bodies,
        /// when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_merge_result"
        )]
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
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        distance: Option<Dimension<PositiveLength>>,
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Dimension<Angle>>,
        /// Whether the opposite incident face supplies the angle reference.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_alternate_face"
        )]
        alternate_face: Option<bool>,
        /// Boundary-corner construction law.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_corner"
        )]
        corner: Option<RuledSurfaceCorner>,
    },
    /// Taper applied to selected faces about a neutral plane.
    Draft {
        /// Faces whose angle is modified.
        faces: FaceSelection,
        /// Structurally selected anchor and pull frame.
        anchor: DraftAnchor,
        /// Signed draft angle.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        angle: Option<Dimension<SlopeAngle>>,
        /// Whether material is added away from the pull direction.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_reverse"
        )]
        reverse: Option<bool>,
    },
    /// Removes one side of target bodies using ordered tool bodies.
    TrimBodies {
        /// Disjoint operand selections and their operation arity.
        operands: TrimBodyOperands,
        /// Side retained by the trim.
        keep: BodyTrimSide,
    },
    /// Partitions selected bodies with surface faces or a datum plane while
    /// retaining every resulting side.
    SplitBody {
        /// Bodies partitioned by the operation.
        targets: BodySelection,
        /// Geometry that partitions the target bodies.
        tool: SplitBodyTool,
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
        /// Canonical model-space displacement components in millimeters.
        translation: [Dimension<Length>; 3],
        /// Axis-angle rotation applied with the translation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rotation"
        )]
        rotation: Option<AxisAngle>,
        /// Number of transformed copies; zero moves the selected bodies.
        #[serde(default)]
        copies: Dimension<u32>,
    },
    /// Dome grown from selected planar faces.
    Dome {
        /// Faces that bound the dome base.
        faces: FaceSelection,
        /// Dome height measured normal to the base, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_height"
        )]
        height: Option<Dimension<PositiveLength>>,
        /// Whether the profile is elliptical rather than spherical, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_elliptical"
        )]
        elliptical: Option<bool>,
        /// Whether growth opposes the selected-face normal, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_reverse"
        )]
        reverse: Option<bool>,
    },
    /// Deformation of existing geometry about a feature axis.
    Flex {
        /// Flex axis direction in model space, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_axis"
        )]
        axis: Option<FeatureDirection3>,
        /// Applied deformation mode and magnitude.
        mode: FlexMode,
    },
    /// Scales selected bodies about a model-space point.
    Scale {
        /// Bodies transformed by the operation.
        bodies: BodySelection,
        /// Fixed locus of the scale transform.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_center"
        )]
        center: Option<ScaleCenter>,
        /// Uniform, per-axis, or unresolved scale factors.
        factors: ScaleFactors,
    },
    /// Drilled or machined hole.
    Hole {
        /// Sketch or profile supplying one or more hole locations.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_profile"
        )]
        profile: Option<PlanarProfileRef>,
        /// Geometry families in the profile that generate hole locations.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_profile_filter"
        )]
        profile_filter: Option<HoleProfileFilter>,
        /// Face the hole is placed on, when known.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_face"
        )]
        face: Option<FaceSelection>,
        /// Drilling direction carried independently of complete placements.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_direction"
        )]
        direction: Option<FeatureDirection3>,
        /// Complete one-or-many hole placements, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_placements"
        )]
        placements: Option<Vec<HolePlacement>>,
        /// Bore diameter and compatible entry, exit, and thread construction.
        shape: HoleShape,
        /// How deep the hole extends, when resolved. Holes travel on one side
        /// only, so the termination law needs no sidedness wrapper.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_extent"
        )]
        extent: Option<LinearTermination>,
        /// Shape and depth convention at the blind end of the hole.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_bottom"
        )]
        bottom: Option<HoleBottom>,
        /// Included taper angle for a conical hole, when enabled.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_taper_angle"
        )]
        taper_angle: Option<Dimension<InteriorAngle>>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
    /// Repetition or reflection of existing features.
    Pattern {
        /// Geometry being repeated or reflected; empty when the source selection is unresolved.
        seeds: Vec<PatternSeed>,
        /// Spatial transform defining the repetition or reflection.
        pattern: PatternKind,
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
        #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
        parameters: BTreeMap<NonBlankString, Dimension<Expression>>,
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_solid"
        )]
        solid: Option<bool>,
        /// Native face-building policy used to turn closed wires into faces.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_face_maker"
        )]
        face_maker: Option<FaceMaker>,
        /// Taper orientation used for inner wires, when selectable.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_inner_wire_taper"
        )]
        inner_wire_taper: Option<InnerWireTaper>,
        /// Whether stored lengths are measured along the profile normal instead of the sweep axis.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_length_along_profile_normal"
        )]
        length_along_profile_normal: Option<bool>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
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
        #[serde(default = "crate::default_true")]
        solid: bool,
        /// Whether adjacent sections are connected by straight ruled spans.
        #[serde(default)]
        ruled: bool,
        /// Whether linear edges and planar faces are simplified after construction.
        #[serde(default)]
        linearize: bool,
        /// Maximum polynomial degree used to interpolate the sections, when constrained.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_max_degree"
        )]
        max_degree: Option<Dimension<std::num::NonZeroU32>>,
        /// Whether profiles containing multiple faces are accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
}

/// Neutral construction semantics: one operation, optionally under a single
/// post-processing layer.
///
/// A post-processed operation carries a [`FeatureOperation`], which has no
/// post-processing arm, so a post-process inside a post-process has no
/// spelling. Every other operation reaches the wire through the untagged
/// [`FeatureDefinition::Operation`] arm, so the wire stays a single flat
/// `definition`-tagged object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "definition", rename_all = "snake_case", deny_unknown_fields)]
pub enum FeatureDefinition {
    /// Operation followed by source-requested topology cleanup.
    PostProcess {
        /// Underlying construction whose result is post-processed.
        operation: FeatureOperation,
        /// Whether redundant splitter boundaries are removed.
        refine: bool,
        /// Boolean-operation tolerance selection carried by the feature family.
        fuzzy_tolerance: FuzzyTolerance,
    },
    /// Any operation that a post-processing layer also admits.
    #[serde(untagged)]
    Operation(FeatureOperation),
}

/// Operation family of a feature whose construction operands are unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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

impl FeatureOperation {
    /// Family name of an operation whose replay produces body geometry, and `None` for
    /// definitions that do not.
    ///
    /// A feature of one of these families carries current result bodies in its
    /// [`FeatureEvaluation::outputs`] list or intermediate result identities in a
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
            Self::SheetMetalAttachedPlate { .. } => Some("sheet metal attached plate"),
            Self::SheetMetalBend { .. } => Some("sheet metal bend"),
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
}

impl FeatureDefinition {
    /// The operation this definition performs, its post-processing layer aside.
    #[must_use]
    pub const fn operation(&self) -> &FeatureOperation {
        match self {
            Self::PostProcess { operation, .. } | Self::Operation(operation) => operation,
        }
    }

    /// Family name of a definition whose replay produces body geometry, and
    /// `None` for definitions that do not.
    ///
    /// A feature of one of these families carries current result bodies in its
    /// [`FeatureEvaluation::outputs`] list or intermediate result identities in a
    /// [`FeatureResultTopology`]. The returned name is the stable lowercase
    /// label for the family, suitable for grouping such features in a report.
    #[must_use]
    pub fn body_output_family(&self) -> Option<&'static str> {
        match self {
            Self::PostProcess { operation, .. } => operation.body_output_family(),
            Self::Operation(operation) => operation.body_output_family(),
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_source"
        )]
        source: Option<ExtrusionDirectionSource>,
    },
}

impl Default for ExtrudeDirection {
    fn default() -> Self {
        Self::ProfileNormal {}
    }
}

/// External model format consumed by an imported-geometry feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum GeometryImportFormat {
    /// ISO 10303 STEP model data.
    Step,
    /// IGES model data.
    Iges,
    /// Native boundary-representation model data.
    Brep,
}

/// Selection policy for Boolean-operation fuzzy tolerance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FuzzyTolerance {
    /// Let the modeling kernel use its default tolerance.
    KernelDefault,
    /// Determine a suitable tolerance from the participating shapes.
    Automatic,
    /// Use the supplied positive model-unit tolerance.
    Explicit(Dimension<PositiveLength>),
}

/// Geometric offset construction used by a thin-wall shell operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub enum RuledCurveOrientation {
    /// Select curve traversal automatically from endpoint proximity.
    Automatic,
    /// Retain both persisted curve traversal directions.
    Forward,
    /// Reverse the second curve relative to the first.
    Reversed,
}

/// The refusal of primitive dimensions that define no solid.
const INVALID_PRIMITIVE_DIMENSIONS: &str = "primitive dimensions are invalid";

/// An analytic solid primitive with valid dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PrimitiveSolidKind", into = "PrimitiveSolidKind")]
pub struct PrimitiveSolid(PrimitiveSolidKind);

/// Which condition refuses a scaled primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveSolidScaleError {
    /// A scaled dimension overflows.
    NonFinite,
    /// A positive dimension or strict extent collapses under rounding.
    Admission(&'static str),
}

impl PrimitiveSolid {
    /// Admit literal dimensions that define a solid primitive. Bound dimensions
    /// are checked against their current parameters by document validation.
    pub fn new(kind: PrimitiveSolidKind) -> Result<Self, &'static str> {
        let positive =
            |value: &Dimension<Length>| value.literal().is_none_or(|value| value.get() > 0.0);
        let nonnegative =
            |value: &Dimension<Length>| value.literal().is_none_or(|value| value.get() >= 0.0);
        let ordered_length = |low: &Dimension<Length>, high: &Dimension<Length>, strict: bool| {
            low.literal().zip(high.literal()).is_none_or(|(low, high)| {
                if strict {
                    low.get() < high.get()
                } else {
                    low.get() <= high.get()
                }
            })
        };
        let ordered_angle = |low: &Dimension<Angle>, high: &Dimension<Angle>| {
            low.literal()
                .zip(high.literal())
                .is_none_or(|(low, high)| low.get() < high.get())
        };
        let valid = match &kind {
            PrimitiveSolidKind::Box {
                length,
                width,
                height,
            } => positive(length) && positive(width) && positive(height),
            PrimitiveSolidKind::Cylinder { radius, height, .. } => {
                positive(radius.dimension()) && positive(height)
            }
            PrimitiveSolidKind::Cone {
                radius1,
                radius2,
                height,
                ..
            } => {
                nonnegative(radius1.dimension())
                    && nonnegative(radius2.dimension())
                    && (radius1.dimension().literal().is_none()
                        || radius2.dimension().literal().is_none()
                        || positive(radius1.dimension())
                        || positive(radius2.dimension()))
                    && positive(height)
            }
            PrimitiveSolidKind::Sphere {
                radius,
                latitude1,
                latitude2,
                ..
            } => positive(radius.dimension()) && ordered_angle(latitude1, latitude2),
            PrimitiveSolidKind::Ellipsoid {
                x_radius,
                y_radius,
                z_radius,
                latitude1,
                latitude2,
                ..
            } => {
                positive(x_radius.dimension())
                    && positive(y_radius.dimension())
                    && positive(z_radius.dimension())
                    && ordered_angle(latitude1, latitude2)
            }
            PrimitiveSolidKind::Torus {
                major_radius,
                minor_radius,
                latitude1,
                latitude2,
                ..
            } => {
                positive(major_radius.dimension())
                    && positive(minor_radius.dimension())
                    && ordered_angle(latitude1, latitude2)
            }
            PrimitiveSolidKind::Prism {
                sides,
                circumradius,
                height,
            } => {
                sides.literal().is_none_or(|sides| *sides >= 3)
                    && positive(circumradius.dimension())
                    && positive(height)
            }
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
                ordered_length(xmin, xmax, true)
                    && ordered_length(ymin, ymax, true)
                    && ordered_length(zmin, zmax, true)
                    && ordered_length(x2min, x2max, false)
                    && ordered_length(z2min, z2max, false)
            }
        };
        valid
            .then_some(Self(kind))
            .ok_or(INVALID_PRIMITIVE_DIMENSIONS)
    }

    /// Scale unbound lengths, preserving bindings to separately scaled parameters.
    pub fn scaled(&self, scale: PositiveReal) -> Result<Self, PrimitiveSolidScaleError> {
        let scaled = |value: Length| {
            Length::new(value.get() * scale.get()).ok_or(PrimitiveSolidScaleError::NonFinite)
        };
        let mut kind = self.0.clone();
        macro_rules! scale_fields { ($($field:ident),+ $(,)?) => { $( *$field = $field.clone().try_map_literal(&scaled)?; )+ }; }
        match &mut kind {
            PrimitiveSolidKind::Box {
                length,
                width,
                height,
            } => {
                scale_fields!(length, width, height);
            }
            PrimitiveSolidKind::Cylinder { radius, height, .. } => {
                scale_fields!(radius, height);
            }
            PrimitiveSolidKind::Cone {
                radius1,
                radius2,
                height,
                ..
            } => {
                scale_fields!(radius1, radius2, height);
            }
            PrimitiveSolidKind::Sphere { radius, .. } => {
                scale_fields!(radius);
            }
            PrimitiveSolidKind::Ellipsoid {
                x_radius,
                y_radius,
                z_radius,
                ..
            } => {
                scale_fields!(x_radius, y_radius, z_radius);
            }
            PrimitiveSolidKind::Torus {
                major_radius,
                minor_radius,
                ..
            } => {
                scale_fields!(major_radius, minor_radius);
            }
            PrimitiveSolidKind::Prism {
                circumradius,
                height,
                ..
            } => {
                scale_fields!(circumradius, height);
            }
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
                scale_fields!(xmin, ymin, zmin, x2min, z2min, xmax, ymax, zmax, x2max, z2max);
            }
        }
        Self::new(kind).map_err(PrimitiveSolidScaleError::Admission)
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
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrimitiveSolidKind {
    /// Rectangular solid aligned to its feature frame.
    Box {
        /// Size along the local x-axis.
        length: Dimension<Length>,
        /// Size along the local y-axis.
        width: Dimension<Length>,
        /// Size along the local z-axis.
        height: Dimension<Length>,
    },
    /// Circular cylinder aligned to its feature-frame z-axis.
    Cylinder {
        /// Circular base radius.
        radius: RadialDimension<Length>,
        /// Axial height.
        height: Dimension<Length>,
        /// Angular sweep around the axis.
        angle: Dimension<Angle>,
    },
    /// Circular cone or frustum aligned to its feature-frame z-axis.
    Cone {
        /// Radius at the local-frame origin.
        radius1: RadialDimension<Length>,
        /// Radius at the opposite end.
        radius2: RadialDimension<Length>,
        /// Axial height.
        height: Dimension<Length>,
        /// Angular sweep around the axis.
        angle: Dimension<Angle>,
    },
    /// Spherical segment.
    Sphere {
        /// Sphere radius.
        radius: RadialDimension<Length>,
        /// Lower latitude bound.
        latitude1: Dimension<Angle>,
        /// Upper latitude bound.
        latitude2: Dimension<Angle>,
        /// Longitudinal sweep.
        longitude: Dimension<Angle>,
    },
    /// Ellipsoidal segment aligned to its feature frame.
    Ellipsoid {
        /// Radius along local x.
        x_radius: RadialDimension<Length>,
        /// Radius along local y.
        y_radius: RadialDimension<Length>,
        /// Radius along local z.
        z_radius: RadialDimension<Length>,
        /// Lower latitude bound.
        latitude1: Dimension<Angle>,
        /// Upper latitude bound.
        latitude2: Dimension<Angle>,
        /// Longitudinal sweep.
        longitude: Dimension<Angle>,
    },
    /// Toroidal segment aligned to its feature frame.
    Torus {
        /// Distance from the axis to the tube center.
        major_radius: RadialDimension<Length>,
        /// Tube radius.
        minor_radius: RadialDimension<Length>,
        /// Lower tube-angle bound.
        latitude1: Dimension<Angle>,
        /// Upper tube-angle bound.
        latitude2: Dimension<Angle>,
        /// Sweep around the torus axis.
        longitude: Dimension<Angle>,
    },
    /// Regular polygonal prism aligned to its feature frame.
    Prism {
        /// Number of polygon sides.
        sides: Dimension<u32>,
        /// Distance from polygon center to each vertex.
        circumradius: RadialDimension<Length>,
        /// Axial height.
        height: Dimension<Length>,
    },
    /// General wedge defined by two x-z profiles across a y interval.
    Wedge {
        /// Lower x coordinate of the profile at `ymin`.
        xmin: Dimension<Length>,
        /// Y coordinate of the first profile.
        ymin: Dimension<Length>,
        /// Lower z coordinate of the profile at `ymin`.
        zmin: Dimension<Length>,
        /// Lower x coordinate of the profile at `ymax`.
        x2min: Dimension<Length>,
        /// Lower z coordinate of the profile at `ymax`.
        z2min: Dimension<Length>,
        /// Upper x coordinate of the profile at `ymin`.
        xmax: Dimension<Length>,
        /// Y coordinate of the second profile.
        ymax: Dimension<Length>,
        /// Upper z coordinate of the profile at `ymin`.
        zmax: Dimension<Length>,
        /// Upper x coordinate of the profile at `ymax`.
        x2max: Dimension<Length>,
        /// Upper z coordinate of the profile at `ymax`.
        z2max: Dimension<Length>,
    },
}

/// Resolution state and inputs of a profile revolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RevolveConstruction {
    /// The first absent required construction input names the partial state.
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_solid"
        )]
        solid: Option<bool>,
        /// Face-building algorithm used for a standalone solid revolution.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_face_maker_class"
        )]
        #[serde(rename = "face_maker_class")]
        #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
        face_maker: Option<FaceMaker>,
        /// Compatibility ordering for fusing a `PartDesign` revolution into its body.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fuse_order"
        )]
        fuse_order: Option<RevolutionFuseOrder>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
}

/// Incomplete inputs of a profile revolution, named by the first absent operand
/// in the order profile, axis, extent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "missing", rename_all = "snake_case", deny_unknown_fields)]
pub enum PartialRevolveConstruction {
    /// The profile is absent.
    Profile {
        /// Placed revolution axis, when decoded.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_partial_revolve_construction_axis"
        )]
        axis: Option<RevolutionAxis>,
        /// Angular extent, when decoded.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_partial_revolve_construction_extent"
        )]
        extent: Option<RevolveExtent>,
        /// Whether a standalone revolution creates a solid rather than a sheet.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_solid"
        )]
        solid: Option<bool>,
        /// Face-building algorithm used for a standalone solid revolution.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_face_maker_class"
        )]
        #[serde(rename = "face_maker_class")]
        #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
        face_maker: Option<FaceMaker>,
        /// Compatibility ordering for fusing a `PartDesign` revolution into its body.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fuse_order"
        )]
        fuse_order: Option<RevolutionFuseOrder>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
    /// The profile is present and the axis is absent.
    Axis {
        /// Profile revolved about the axis.
        profile: PlanarProfileRef,
        /// Angular extent, when decoded.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_partial_revolve_construction_extent"
        )]
        extent: Option<RevolveExtent>,
        /// Whether a standalone revolution creates a solid rather than a sheet.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_solid"
        )]
        solid: Option<bool>,
        /// Face-building algorithm used for a standalone solid revolution.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_face_maker_class"
        )]
        #[serde(rename = "face_maker_class")]
        #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
        face_maker: Option<FaceMaker>,
        /// Compatibility ordering for fusing a `PartDesign` revolution into its body.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fuse_order"
        )]
        fuse_order: Option<RevolutionFuseOrder>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
    /// The profile and the axis are present and the extent is absent.
    Extent {
        /// Profile revolved about the axis.
        profile: PlanarProfileRef,
        /// Placed revolution axis and its optional native source.
        axis: RevolutionAxis,
        /// Whether a standalone revolution creates a solid rather than a sheet.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_solid"
        )]
        solid: Option<bool>,
        /// Face-building algorithm used for a standalone solid revolution.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_face_maker_class"
        )]
        #[serde(rename = "face_maker_class")]
        #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
        face_maker: Option<FaceMaker>,
        /// Compatibility ordering for fusing a `PartDesign` revolution into its body.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fuse_order"
        )]
        fuse_order: Option<RevolutionFuseOrder>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
}

/// The four optional selections a revolution carries in every resolution state.
struct RevolveSelections {
    solid: Option<bool>,
    face_maker: Option<FaceMaker>,
    fuse_order: Option<RevolutionFuseOrder>,
    allow_multi_profile_faces: Option<bool>,
}

impl RevolveConstruction {
    fn selections(&self) -> RevolveSelections {
        let (solid, face_maker, fuse_order, allow_multi_profile_faces) = match self {
            Self::Resolved {
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
                ..
            }
            | Self::Unresolved(
                PartialRevolveConstruction::Profile {
                    solid,
                    face_maker,
                    fuse_order,
                    allow_multi_profile_faces,
                    ..
                }
                | PartialRevolveConstruction::Axis {
                    solid,
                    face_maker,
                    fuse_order,
                    allow_multi_profile_faces,
                    ..
                }
                | PartialRevolveConstruction::Extent {
                    solid,
                    face_maker,
                    fuse_order,
                    allow_multi_profile_faces,
                    ..
                },
            ) => (solid, face_maker, fuse_order, allow_multi_profile_faces),
        };
        RevolveSelections {
            solid: *solid,
            face_maker: face_maker.clone(),
            fuse_order: *fuse_order,
            allow_multi_profile_faces: *allow_multi_profile_faces,
        }
    }

    /// Returns whether all required construction inputs are present.
    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved { .. })
    }

    /// Returns the profile, when decoded.
    pub const fn profile(&self) -> Option<&PlanarProfileRef> {
        match self {
            Self::Unresolved(PartialRevolveConstruction::Profile { .. }) => None,
            Self::Unresolved(
                PartialRevolveConstruction::Axis { profile, .. }
                | PartialRevolveConstruction::Extent { profile, .. },
            )
            | Self::Resolved { profile, .. } => Some(profile),
        }
    }

    /// Returns the mutable profile, when decoded.
    pub const fn profile_mut(&mut self) -> Option<&mut PlanarProfileRef> {
        match self {
            Self::Unresolved(PartialRevolveConstruction::Profile { .. }) => None,
            Self::Unresolved(
                PartialRevolveConstruction::Axis { profile, .. }
                | PartialRevolveConstruction::Extent { profile, .. },
            )
            | Self::Resolved { profile, .. } => Some(profile),
        }
    }

    /// Replaces the decoded profile, naming the resolution state the new
    /// operand set produces.
    pub fn set_profile(&mut self, profile: Option<PlanarProfileRef>) {
        let RevolveSelections {
            solid,
            face_maker,
            fuse_order,
            allow_multi_profile_faces,
        } = self.selections();
        let axis = self.axis().cloned();
        let extent = self.extent().cloned();
        *self = match (profile, axis, extent) {
            (None, axis, extent) => Self::Unresolved(PartialRevolveConstruction::Profile {
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            }),
            (Some(profile), None, extent) => Self::Unresolved(PartialRevolveConstruction::Axis {
                profile,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            }),
            (Some(profile), Some(axis), None) => {
                Self::Unresolved(PartialRevolveConstruction::Extent {
                    profile,
                    axis,
                    solid,
                    face_maker,
                    fuse_order,
                    allow_multi_profile_faces,
                })
            }
            (Some(profile), Some(axis), Some(extent)) => Self::Resolved {
                profile,
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            },
        };
    }

    /// Returns the placed axis, when decoded.
    pub const fn axis(&self) -> Option<&RevolutionAxis> {
        match self {
            Self::Unresolved(
                PartialRevolveConstruction::Profile { axis: None, .. }
                | PartialRevolveConstruction::Axis { .. },
            ) => None,
            Self::Unresolved(
                PartialRevolveConstruction::Profile {
                    axis: Some(axis), ..
                }
                | PartialRevolveConstruction::Extent { axis, .. },
            )
            | Self::Resolved { axis, .. } => Some(axis),
        }
    }

    /// Returns the mutable placed axis, when decoded.
    pub const fn axis_mut(&mut self) -> Option<&mut RevolutionAxis> {
        match self {
            Self::Unresolved(
                PartialRevolveConstruction::Profile { axis: None, .. }
                | PartialRevolveConstruction::Axis { .. },
            ) => None,
            Self::Unresolved(
                PartialRevolveConstruction::Profile {
                    axis: Some(axis), ..
                }
                | PartialRevolveConstruction::Extent { axis, .. },
            )
            | Self::Resolved { axis, .. } => Some(axis),
        }
    }

    /// Replaces the placed axis, naming the resolution state the new operand
    /// set produces.
    pub fn set_axis(&mut self, axis: Option<RevolutionAxis>) {
        let old = std::mem::replace(
            self,
            Self::Unresolved(PartialRevolveConstruction::Profile {
                axis: None,
                extent: None,
                solid: None,
                face_maker: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            }),
        );
        let (profile, extent, solid, face_maker, fuse_order, allow_multi_profile_faces) = match old
        {
            Self::Unresolved(PartialRevolveConstruction::Profile {
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
                ..
            }) => (
                None,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            ),
            Self::Unresolved(PartialRevolveConstruction::Axis {
                profile,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            }) => (
                Some(profile),
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            ),
            Self::Unresolved(PartialRevolveConstruction::Extent {
                profile,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
                ..
            }) => (
                Some(profile),
                None,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            ),
            Self::Resolved {
                profile,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
                ..
            } => (
                Some(profile),
                Some(extent),
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            ),
        };
        *self = match (profile, axis, extent) {
            (None, axis, extent) => Self::Unresolved(PartialRevolveConstruction::Profile {
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            }),
            (Some(profile), None, extent) => Self::Unresolved(PartialRevolveConstruction::Axis {
                profile,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            }),
            (Some(profile), Some(axis), None) => {
                Self::Unresolved(PartialRevolveConstruction::Extent {
                    profile,
                    axis,
                    solid,
                    face_maker,
                    fuse_order,
                    allow_multi_profile_faces,
                })
            }
            (Some(profile), Some(axis), Some(extent)) => Self::Resolved {
                profile,
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            },
        };
    }

    /// Returns the angular extent, when decoded.
    pub const fn extent(&self) -> Option<&RevolveExtent> {
        match self {
            Self::Unresolved(
                PartialRevolveConstruction::Profile { extent: None, .. }
                | PartialRevolveConstruction::Axis { extent: None, .. }
                | PartialRevolveConstruction::Extent { .. },
            ) => None,
            Self::Unresolved(
                PartialRevolveConstruction::Profile {
                    extent: Some(extent),
                    ..
                }
                | PartialRevolveConstruction::Axis {
                    extent: Some(extent),
                    ..
                },
            )
            | Self::Resolved { extent, .. } => Some(extent),
        }
    }

    /// Returns the mutable angular extent, when decoded.
    pub const fn extent_mut(&mut self) -> Option<&mut RevolveExtent> {
        match self {
            Self::Unresolved(
                PartialRevolveConstruction::Profile { extent: None, .. }
                | PartialRevolveConstruction::Axis { extent: None, .. }
                | PartialRevolveConstruction::Extent { .. },
            ) => None,
            Self::Unresolved(
                PartialRevolveConstruction::Profile {
                    extent: Some(extent),
                    ..
                }
                | PartialRevolveConstruction::Axis {
                    extent: Some(extent),
                    ..
                },
            )
            | Self::Resolved { extent, .. } => Some(extent),
        }
    }

    /// Returns the standalone solid selection, when carried.
    pub const fn solid(&self) -> Option<bool> {
        match self {
            Self::Resolved { solid, .. }
            | Self::Unresolved(
                PartialRevolveConstruction::Profile { solid, .. }
                | PartialRevolveConstruction::Axis { solid, .. }
                | PartialRevolveConstruction::Extent { solid, .. },
            ) => *solid,
        }
    }

    /// Returns the standalone face-maker selection, when carried.
    pub const fn face_maker(&self) -> Option<&FaceMaker> {
        match self {
            Self::Resolved { face_maker, .. }
            | Self::Unresolved(
                PartialRevolveConstruction::Profile { face_maker, .. }
                | PartialRevolveConstruction::Axis { face_maker, .. }
                | PartialRevolveConstruction::Extent { face_maker, .. },
            ) => face_maker.as_ref(),
        }
    }

    /// Returns the `PartDesign` fuse ordering, when carried.
    pub const fn fuse_order(&self) -> Option<RevolutionFuseOrder> {
        match self {
            Self::Resolved { fuse_order, .. }
            | Self::Unresolved(
                PartialRevolveConstruction::Profile { fuse_order, .. }
                | PartialRevolveConstruction::Axis { fuse_order, .. }
                | PartialRevolveConstruction::Extent { fuse_order, .. },
            ) => *fuse_order,
        }
    }

    /// Returns the multi-profile-face selection, when carried.
    pub const fn allow_multi_profile_faces(&self) -> Option<bool> {
        match self {
            Self::Resolved {
                allow_multi_profile_faces,
                ..
            }
            | Self::Unresolved(
                PartialRevolveConstruction::Profile {
                    allow_multi_profile_faces,
                    ..
                }
                | PartialRevolveConstruction::Axis {
                    allow_multi_profile_faces,
                    ..
                }
                | PartialRevolveConstruction::Extent {
                    allow_multi_profile_faces,
                    ..
                },
            ) => *allow_multi_profile_faces,
        }
    }
}

/// Operand ordering used to fuse a `PartDesign` revolution result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum RevolutionFuseOrder {
    /// Existing body is the first fuse operand.
    BaseFirst,
    /// Newly revolved feature is the first fuse operand.
    FeatureFirst,
}

/// Complete line placement used as a revolution axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RevolutionAxis {
    /// A point on the axis.
    pub origin: FinitePoint3,
    /// Axis direction with a finite nonzero norm.
    pub direction: FeatureDirection3,
    /// Native edge, datum, or sketch-axis selection used to resolve the axis.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_revolution_axis_reference"
    )]
    pub reference: Option<PathRef>,
}

/// Independently decoded inputs of a thin rib operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RibConstruction {
    /// Rib centerline or open profile, when resolved.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_profile"
    )]
    pub profile: Option<PlanarProfileRef>,
    /// Rib growth direction, when resolved.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rib_construction_direction"
    )]
    pub direction: Option<FeatureDirection3>,
    /// Finished rib thickness, when resolved.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thickness"
    )]
    pub thickness: Option<Dimension<PositiveLength>>,
    /// Distribution of thickness around the profile, when resolved.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rib_construction_side"
    )]
    pub side: Option<RibSide>,
    /// Draft state applied to the rib walls.
    #[serde(default)]
    pub draft: RibDraft,
}

/// Distribution of rib thickness around its profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum RibSide {
    /// Thickness lies on one side of the profile.
    OneSided,
    /// Thickness is split equally around the profile.
    Centered,
}

/// Draft state of a rib construction.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "angle", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum RibDraft {
    /// Draft semantics are present but unresolved.
    #[default]
    Unresolved,
    /// Rib walls have no draft.
    None,
    /// Rib walls use the specified draft angle.
    Angle(Dimension<SlopeAngle>),
}

/// Canonical role of a non-modeling feature-tree node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CosmeticThreadExtent {
    /// Fixed thread length along the cylindrical face.
    Blind {
        /// Positive axial thread length.
        length: Dimension<PositiveLength>,
    },
    /// Thread annotation spans the complete cylindrical face.
    Through {},
}

/// Canonical role of a built-in reference plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PrincipalPlane {
    /// Front plane through the model origin.
    Front,
    /// Top plane through the model origin.
    Top,
    /// Right plane through the model origin.
    Right,
}

/// Known sketch space and its optional resolved planar geometry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(from = "SketchFeatureBindingWire", into = "SketchFeatureBindingWire")]
#[serde(deny_unknown_fields)]
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
    /// The feature's sketch space is unresolved.
    Unresolved {},
    /// The source declares a planar sketch, with optional decoded geometry.
    Planar {
        /// Resolved planar sketch identity, when available.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_feature_operation_sketch"
        )]
        sketch: Option<crate::sketches::SketchId>,
    },
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub enum BodyRetentionMode {
    /// The operation family is known but the selected retention mode is unavailable.
    Unresolved,
    /// Delete the selected bodies.
    DeleteSelected,
    /// Delete every body except the selected bodies.
    KeepSelected,
}

/// Material effect of a wrapped profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum WrapMode {
    /// Add material above the target face.
    Emboss {
        /// Positive normal offset above the target face.
        depth: Dimension<PositiveLength>,
    },
    /// Remove material below the target face.
    Deboss {
        /// Positive normal offset below the target face.
        depth: Dimension<PositiveLength>,
    },
    /// Imprint the profile without adding or removing material.
    Scribe,
}

/// Continuity order imposed at a generated surface boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SurfaceContinuity {
    /// Positional continuity only.
    Contact,
    /// First-derivative continuity.
    Tangent,
    /// Second-derivative continuity.
    Curvature,
}

cadmpeg_core::named_optional_field!(deserialize_filled_surface_weight, PositiveReal, "weight");

/// One filled-surface boundary condition and its continuity weight.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FilledSurfaceBoundaryCondition {
    /// Positional continuity, which has no weight.
    Contact {},
    /// First-derivative continuity with an optional resolved weight.
    Tangent {
        /// Positive dimensionless weight; absent when the source weight is unresolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_filled_surface_weight"
        )]
        weight: Option<PositiveReal>,
    },
    /// Second-derivative continuity with an optional resolved weight.
    Curvature {
        /// Positive dimensionless weight; absent when the source weight is unresolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_filled_surface_weight"
        )]
        weight: Option<PositiveReal>,
    },
}

impl FilledSurfaceBoundaryCondition {
    /// The continuity order of this condition.
    #[must_use]
    pub const fn continuity(self) -> SurfaceContinuity {
        match self {
            Self::Contact {} => SurfaceContinuity::Contact,
            Self::Tangent { .. } => SurfaceContinuity::Tangent,
            Self::Curvature { .. } => SurfaceContinuity::Curvature,
        }
    }

    /// The resolved weight, absent for positional continuity.
    #[must_use]
    pub const fn weight(self) -> Option<PositiveReal> {
        match self {
            Self::Contact {} => None,
            Self::Tangent { weight } | Self::Curvature { weight } => weight,
        }
    }
}

impl From<SurfaceContinuity> for FilledSurfaceBoundaryCondition {
    fn from(continuity: SurfaceContinuity) -> Self {
        match continuity {
            SurfaceContinuity::Contact => Self::Contact {},
            SurfaceContinuity::Tangent => Self::Tangent { weight: None },
            SurfaceContinuity::Curvature => Self::Curvature { weight: None },
        }
    }
}

/// Resolved continuity conditions for a filled-surface boundary.
///
/// One condition per boundary component, in source order. A boundary whose
/// components all impose the same condition is uniform, which [`Self::uniform`]
/// reports; a uniform boundary has no second spelling to disagree with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct FilledSurfaceContinuity {
    /// Conditions of the boundary components, in source order.
    pub conditions: NonEmptyMembers<FilledSurfaceBoundaryCondition>,
}

impl FilledSurfaceContinuity {
    /// Returns the aggregate condition when every component uses one value.
    #[must_use]
    pub fn uniform(&self) -> Option<FilledSurfaceBoundaryCondition> {
        let mut components = self.conditions.iter();
        let first = *components.next()?;
        components
            .all(|continuity| continuity == &first)
            .then_some(first)
    }
}

/// Optional filled-surface continuity.
///
/// The wire carries the component conditions and nothing else, so an aggregate
/// condition that disagrees with them is unrepresentable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct FilledSurfaceContinuityState(
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    Option<FilledSurfaceContinuity>,
);

impl FilledSurfaceContinuityState {
    /// Creates an unresolved continuity state.
    #[must_use]
    pub const fn unresolved() -> Self {
        Self(None)
    }

    /// Creates one condition for the complete boundary.
    #[must_use]
    pub fn uniform(continuity: SurfaceContinuity) -> Self {
        Self(Some(FilledSurfaceContinuity {
            conditions: NonEmptyMembers::one(continuity.into()),
        }))
    }

    /// Creates component-specific conditions.
    #[must_use]
    pub const fn per_boundary(conditions: NonEmptyMembers<FilledSurfaceBoundaryCondition>) -> Self {
        Self(Some(FilledSurfaceContinuity { conditions }))
    }

    /// Returns the resolved continuity form.
    #[must_use]
    pub const fn resolved(&self) -> Option<&FilledSurfaceContinuity> {
        self.0.as_ref()
    }

    /// Returns the continuity order when every component has the same order.
    #[must_use]
    pub fn uniform_value(&self) -> Option<SurfaceContinuity> {
        let mut conditions = self.0.as_ref()?.conditions.iter();
        let first = conditions.next()?.continuity();
        conditions
            .all(|condition| condition.continuity() == first)
            .then_some(first)
    }

    /// Whether no continuity condition was resolved.
    #[must_use]
    pub const fn is_unresolved(&self) -> bool {
        self.0.is_none()
    }
}

/// Boundary input accepted by a filled-surface operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct TrimCellSelectionWire {
    /// One-based ordinals of cells removed by the operation.
    removed: Vec<u64>,
    /// Number of cells in the operation's partition.
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub enum RuledSurfaceMode {
    /// Extend normal to the support faces.
    Normal {
        /// Positive extension distance.
        distance: Dimension<PositiveLength>,
    },
    /// Extend tangent to the support faces.
    Tangent {
        /// Positive extension distance.
        distance: Dimension<PositiveLength>,
    },
    /// Extend along one explicit model-space direction.
    Direction {
        /// Extension direction.
        direction: FeatureDirection3,
        /// Positive extension distance.
        distance: Dimension<PositiveLength>,
    },
}

/// Corner construction law for a ruled-surface operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ScaleFactors {
    /// No complete scale factor is available.
    Unresolved {},
    /// One factor applies on all three axes.
    Uniform {
        /// The one factor.
        factor: Dimension<NonZeroReal>,
    },
    /// Independent factors apply on the model-space axes.
    PerAxis {
        /// Factors on the model-space axes, in order.
        factors: [Dimension<NonZeroReal>; 3],
    },
}

impl ScaleFactors {
    /// Resolve model-space factors from canonical dimensions.
    pub fn resolved(&self, ir: &crate::CadIr) -> Option<Vector3> {
        match self {
            Self::Unresolved {} => None,
            Self::Uniform { factor } => {
                let factor = ir.dimension_value(factor)?.get();
                Some(Vector3::new(factor, factor, factor))
            }
            Self::PerAxis { factors } => Some(Vector3::new(
                ir.dimension_value(&factors[0])?.get(),
                ir.dimension_value(&factors[1])?.get(),
                ir.dimension_value(&factors[2])?.get(),
            )),
        }
    }
}

/// Direction in which a thicken feature adds material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ThickenSide {
    /// Add material along the selected-face normal.
    Forward,
    /// Add material opposite the selected-face normal.
    Reverse,
    /// Split the thickness equally across both sides.
    Both,
}

/// Relief form selected by the document's sheet-metal rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SheetMetalReliefForm {
    /// Use the active document rule's bend-relief form.
    StyleDefault,
}

/// Bend-relief operands with lengths in millimetres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SheetMetalRelief {
    /// Construction form.
    pub form: SheetMetalReliefForm,
    /// Evaluated relief width.
    pub width: PositiveLength,
    /// Evaluated relief depth.
    pub depth: PositiveLength,
    /// Minimum material remnant.
    pub minimum_remnant: crate::scalar::NonNegativeLength,
    /// Gap between adjacent sheet edges.
    pub seam_gap: crate::scalar::NonNegativeLength,
}

/// Transition form selected by the document's sheet-metal rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SheetMetalTransitionForm {
    /// Use the active document rule's transition form.
    StyleDefault,
}

/// Sheet-metal transition operands with lengths in millimetres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SheetMetalTransition {
    /// Construction form.
    pub form: SheetMetalTransitionForm,
    /// Evaluated transition radius.
    pub radius: crate::scalar::NonNegativeLength,
}

/// Face pair a sheet-metal flange height is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalHeightDatum {
    /// The height is measured from the inner faces of the sheet.
    InnerFaces,
    /// The height is measured from the outer faces of the sheet.
    OuterFaces,
}

/// Geometric effect of a sheet-metal corner evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalCornerEffect {
    /// The output body equals the input body, including its topology.
    Unchanged,
}

/// Placement of a sheet-metal bend region against its selected edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
/// transition projects that layout to [`Self::Flat`] or [`Self::Open`];
/// [`Self::GapLength`] retains the dimensional owners when the transition does
/// not prove either semantic form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalHemForm {
    /// Flat hem with its developed length.
    Flat {
        /// Developed length of the hem.
        length: Dimension<PositiveLength>,
    },
    /// Open hem with a gap and a developed length.
    Open {
        /// Gap between the folded wall and its source.
        gap: Dimension<NonNegativeLength>,
        /// Developed length of the hem.
        length: Dimension<PositiveLength>,
    },
    /// Unresolved flat-or-open form with a gap and a length owner.
    GapLength {
        /// Gap between the folded wall and its source.
        gap: Dimension<NonNegativeLength>,
        /// Developed length of the hem.
        length: Dimension<PositiveLength>,
    },
    /// Rolled source form with radius and included-angle owners.
    Rolled {
        /// Rolled section radius.
        radius: Dimension<PositiveLength>,
        /// Included angle of the rolled section.
        angle: Dimension<Angle>,
    },
    /// Teardrop source form with gap, length, and radius owners.
    Teardrop {
        /// Gap between the folded wall and its source.
        gap: Dimension<NonNegativeLength>,
        /// Developed length of the hem.
        length: Dimension<PositiveLength>,
        /// Teardrop section radius.
        radius: Dimension<PositiveLength>,
    },
}

/// Placement of a sheet-metal hem bend against its selected edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalHemBendPosition {
    /// The bend starts at the selected edge.
    Adjacent,
    /// The bend is tangent to the source sheet face.
    Tangent,
}

/// Direction of a sheet-metal hem fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub enum SheetMetalFlangeHeight {
    /// Fixed distance from the operation's height datum.
    Distance(Dimension<PositiveLength>),
    /// Signed offset from a selected construction entity.
    ToObject {
        /// Construction entity that supplies the height reference.
        target: SheetMetalFlangeHeightTarget,
        /// Signed distance from the target entity.
        offset: Dimension<Length>,
    },
}

/// Extent of a sheet-metal flange along its selected edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalFlangeWidth {
    /// The flange spans the complete selected edge.
    FullEdge,
    /// The flange is centred on the edge with one width.
    Symmetric {
        /// Total width centred on the edge.
        width: Dimension<PositiveLength>,
    },
    /// The flange is measured inward from each end of the edge.
    TwoSides {
        /// Distance measured from the edge's first end.
        first: Dimension<PositiveLength>,
        /// Distance measured from the edge's second end.
        second: Dimension<PositiveLength>,
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
#[serde(deny_unknown_fields)]
pub struct SheetMetalFlangeTwoSidedWidth {
    /// Distance measured from the edge's first end.
    #[serde(deserialize_with = "deserialize_flange_first")]
    pub first: Dimension<PositiveLength>,
    /// Distance measured from the edge's second end.
    #[serde(deserialize_with = "deserialize_flange_second")]
    pub second: Dimension<PositiveLength>,
}

/// Distribution of sheet thickness relative to its construction plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalThicknessSide {
    /// Thickness lies along the profile plane's positive normal.
    Forward,
    /// Thickness lies along the profile plane's negative normal.
    Reverse,
    /// Thickness is split equally across both sides of the profile plane.
    Symmetric,
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
selection_field_deserializer!(deserialize_local_value, "value");

/// Edge operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
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
        native: NonBlankString,
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
        native: NonBlankString,
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
        native: NonBlankString,
    },
    /// Format-native selection reference.
    Native(#[serde(deserialize_with = "deserialize_selection_native")] SelectionReference),
}

/// Face operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
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
        native: NonBlankString,
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
        native: NonBlankString,
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
///
/// The members stay contiguous: the sequence is read through slice patterns
/// and slice methods. The mint refuses an empty list, `push` only grows, and
/// `DerefMut` hands out `&mut [T]`, which cannot shorten the sequence, so no
/// reader has to ask whether a member is there.
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

    /// Append a member while retaining the nonempty witness.
    pub fn push(&mut self, member: T) {
        self.0.push(member);
    }

    /// Map the members in source order while retaining the nonempty witness.
    pub fn map<U>(self, map: impl FnMut(T) -> U) -> NonEmptyMembers<U> {
        NonEmptyMembers(self.0.into_iter().map(map).collect())
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

impl<T> IntoIterator for NonEmptyMembers<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
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
    /// Admit a generated vertex and its native reference.
    pub fn generated(
        vertex: GeneratedVertexRef,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        SelectionReference::new(native).map(|native| Self::Generated { vertex, native })
    }
    /// Admit a historical vertex and its native reference.
    pub fn historical(
        state: FeatureInputTopologyId,
        vertex: HistoricalVertexId,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        NonBlankString::new(native)
            .ok_or(BodySelectionError::BlankNativeMember)
            .map(|native| Self::Historical {
                state,
                vertex,
                native,
            })
    }
    /// Admit a native vertex reference.
    pub fn native(native: String) -> Result<Self, BodySelectionError> {
        SelectionReference::new(native).map(Self::Native)
    }
}

impl EdgeSelection {
    /// Admit nonempty distinct historical members and their native reference.
    pub fn historical(
        state: FeatureInputTopologyId,
        edges: Vec<HistoricalEdgeId>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let edges = SelectionMembers::try_from(edges)?;
        let native = NonBlankString::new(native).ok_or(BodySelectionError::BlankNativeMember)?;
        Ok(Self::Historical {
            state,
            edges,
            native,
        })
    }

    /// Admit distinct historical members, unresolved native members and their native reference.
    pub fn historical_partial(
        state: FeatureInputTopologyId,
        edges: Vec<HistoricalEdgeId>,
        unresolved: Vec<String>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let edges =
            DistinctMembers::try_from(edges).map_err(|_| BodySelectionError::RepeatedBody)?;
        let unresolved = NativeSelections::try_from(unresolved)?;
        let native = NonBlankString::new(native).ok_or(BodySelectionError::BlankNativeMember)?;
        Ok(Self::HistoricalPartial {
            state,
            edges,
            unresolved,
            native,
        })
    }

    /// Admits generated members and their native reference.
    pub fn generated(
        edges: Vec<GeneratedEdgeRef>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let edges = edges.try_into()?;
        SelectionReference::new(native).map(|native| Self::Generated { edges, native })
    }
}

impl FaceSelection {
    /// Admit nonempty distinct historical members and their native reference.
    pub fn historical(
        state: FeatureInputTopologyId,
        faces: Vec<HistoricalFaceId>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let faces = SelectionMembers::try_from(faces)?;
        let native = NonBlankString::new(native).ok_or(BodySelectionError::BlankNativeMember)?;
        Ok(Self::Historical {
            state,
            faces,
            native,
        })
    }

    /// Admit distinct historical members, unresolved native members and their native reference.
    pub fn historical_partial(
        state: FeatureInputTopologyId,
        faces: Vec<HistoricalFaceId>,
        unresolved: Vec<String>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let faces =
            DistinctMembers::try_from(faces).map_err(|_| BodySelectionError::RepeatedBody)?;
        let unresolved = NativeSelections::try_from(unresolved)?;
        let native = NonBlankString::new(native).ok_or(BodySelectionError::BlankNativeMember)?;
        Ok(Self::HistoricalPartial {
            state,
            faces,
            unresolved,
            native,
        })
    }

    /// Admits generated members and their native reference.
    pub fn generated(
        faces: Vec<GeneratedFaceRef>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let faces = faces.try_into()?;
        SelectionReference::new(native).map(|native| Self::Generated { faces, native })
    }
}

impl GeneratedEdgeRef {
    /// Admit a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        SelectionReference::new(local_id).map(|local_id| Self { feature, local_id })
    }
}

impl GeneratedFaceRef {
    /// Admit a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        SelectionReference::new(local_id).map(|local_id| Self { feature, local_id })
    }
}

impl GeneratedVertexRef {
    /// Admit a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        SelectionReference::new(local_id).map(|local_id| Self { feature, local_id })
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
    /// Admit a nonblank persistent reference.
    pub fn new(value: String) -> Result<Self, BodySelectionError> {
        Self::try_from(value)
    }

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

/// Invalid collection membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FeatureCollectionError {
    /// The collection violates its member constraint.
    #[error("{0}")]
    Invalid(&'static str),
}

impl From<FeatureCollectionError> for cadmpeg_core::CodecError {
    fn from(error: FeatureCollectionError) -> Self {
        match error {
            FeatureCollectionError::Invalid(message) => Self::malformed(message),
        }
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
    type Error = FeatureCollectionError;
    /// Admit members that are pairwise distinct, in source order.
    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        if !membership::distinct(&value, |_| true) {
            return Err(FeatureCollectionError::Invalid("members must be distinct"));
        }
        Ok(Self(value))
    }
}

impl<T: PartialEq> DistinctMembers<T> {
    /// Append a member unless it is already present, and return whether it was appended.
    pub fn insert(&mut self, value: T) -> bool {
        membership::insert(&mut self.0, value)
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
            membership::insert(&mut self.0, member);
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
        let values = Vec::<T>::deserialize(deserializer)?;
        Self::try_from(values).map_err(serde::de::Error::custom)
    }
}
/// Nonempty distinct selection members in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SelectionMembers<T>(Vec<T>);

impl<T: Eq + std::hash::Hash> TryFrom<Vec<T>> for SelectionMembers<T> {
    type Error = BodySelectionError;
    /// Admit nonempty pairwise distinct members, in source order.
    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(BodySelectionError::Empty);
        }
        if !membership::distinct(&value, |_| true) {
            return Err(BodySelectionError::RepeatedBody);
        }
        Ok(Self(value))
    }
}

impl<T> SelectionMembers<T> {
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
        let values = Vec::<T>::deserialize(deserializer)?;
        Self::try_from(values).map_err(serde::de::Error::custom)
    }
}
/// Nonempty distinct nonblank native selection names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct NativeSelections(Vec<String>);

impl TryFrom<Vec<String>> for NativeSelections {
    type Error = BodySelectionError;
    /// Admit nonempty, nonblank, pairwise distinct native names, in source order.
    fn try_from(value: Vec<String>) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(BodySelectionError::Empty);
        }
        if value.iter().any(|name| name.trim().is_empty()) {
            return Err(BodySelectionError::BlankNativeMember);
        }
        if !membership::distinct(&value, |_| true) {
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
        let values = Vec::<String>::deserialize(deserializer)?;
        Self::try_from(values).map_err(serde::de::Error::custom)
    }
}
/// Failure to construct a body-selection member set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BodySelectionError {
    /// A selection set contains no members.
    #[error("body selection set must not be empty")]
    Empty,
    /// A body occurs more than once in the set.
    #[error("body selection set repeats a body")]
    RepeatedBody,
    /// A native selection member is empty or contains only whitespace.
    #[error("body selection member must not be blank")]
    BlankNativeMember,
    /// A native member occurs more than once in the set.
    #[error("body selection set repeats a native member")]
    RepeatedNativeMember,
    /// The standard allocator refused context-free selection storage.
    #[error("selection member allocation failed")]
    Allocation,
}

/// Native selection of the bodies a copy-and-paste operation introduces. The
/// copied bodies themselves are the feature's outputs, so no variant restates
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InsertedBodies {
    /// The copied bodies are not resolved to the neutral model.
    Native(String),
    /// The copied bodies are resolved and listed in the feature's outputs.
    Resolved {
        /// Format-native selection expression.
        native: String,
    },
}

impl InsertedBodies {
    /// Whether the copied bodies are resolved to the neutral model.
    #[must_use]
    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved { .. })
    }
}

/// Body operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BodyMember<B> {
    body: B,
    native: cadmpeg_core::text::NonBlankString,
}

impl<B> BodyMember<B> {
    /// Construct one body/native selection row.
    ///
    /// The native member is non-blank by type; the caller mints it at the
    /// boundary where the source states it.
    pub const fn new(body: B, native: cadmpeg_core::text::NonBlankString) -> Self {
        Self { body, native }
    }

    /// Body identity in this row.
    #[must_use]
    pub const fn body(&self) -> &B {
        &self.body
    }

    /// Consume the row and return its body identity and native member.
    #[must_use]
    pub fn into_parts(self) -> (B, String) {
        (self.body, self.native.into_string())
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
        #[serde(deny_unknown_fields)]
        struct Wire<B> {
            body: B,
            native: String,
        }
        let wire = Wire::deserialize(deserializer)?;
        let native = cadmpeg_core::text::NonBlankString::new(wire.native).ok_or_else(|| {
            serde::de::Error::custom(BodySelectionError::BlankNativeMember.to_string())
        })?;
        Ok(Self::new(wire.body, native))
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
        let mut bodies = std::collections::HashSet::new();
        let mut native = std::collections::HashSet::new();
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

    /// Count paired selection rows.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.0.len()
    }

    /// Iterate over paired selection rows in source order.
    pub fn iter(&self) -> std::slice::Iter<'_, BodyMember<B>> {
        self.0.iter()
    }

    /// Borrow the body identities in source order.
    pub fn bodies(&self) -> impl Iterator<Item = &B> {
        self.0.iter().map(BodyMember::body)
    }
}

impl<'a, B> IntoIterator for &'a BodyMembers<B> {
    type Item = &'a BodyMember<B>;
    type IntoIter = std::slice::Iter<'a, BodyMember<B>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
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

/// Body operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum BodySelection {
    /// Selection exists semantically but its operands are not resolved.
    Unresolved,
    /// Resolved topological bodies.
    Bodies(#[cfg_attr(feature = "schema", schemars(with = "Vec<BodyId>"))] DistinctMembers<BodyId>),
    /// Resolved bodies paired with the format-native selection required for rewrite.
    Resolved {
        /// Resolved topological bodies.
        #[cfg_attr(feature = "schema", schemars(with = "Vec<BodyId>"))]
        bodies: DistinctMembers<BodyId>,
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

/// Persistent identity of a body in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GeneratedBodyRef {
    /// Feature whose regenerated result owns the body.
    pub feature: FeatureId,
    /// Feature-local persistent body identity.
    pub local_id: SelectionReference,
}

impl GeneratedBodyRef {
    /// Admit a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        SelectionReference::new(local_id).map(|local_id| Self { feature, local_id })
    }
}

/// Direct face-motion law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum FaceMotion {
    /// Offset along each face normal.
    Offset {
        /// Signed offset distance.
        distance: Dimension<Length>,
    },
    /// Translation along one direction.
    Translate {
        /// Translation direction.
        direction: FeatureDirection3,
        /// Signed translation distance.
        distance: Dimension<Length>,
    },
    /// Rotation about an axis.
    Rotate {
        /// Point on the rotation axis.
        axis_origin: FinitePoint3,
        /// Rotation-axis direction.
        axis_dir: FeatureDirection3,
        /// Signed rotation angle.
        angle: Dimension<Angle>,
    },
}

/// Model-space axis-angle rotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AxisAngle {
    /// Point on the rotation axis.
    pub origin: FinitePoint3,
    /// Rotation-axis direction.
    pub direction: FeatureDirection3,
    /// Signed rotation angle.
    pub angle: Dimension<Angle>,
}

/// Start condition of a linear extrusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtrudeStart {
    /// Native start condition is present structurally but unresolved.
    Unresolved {},
    /// Begin on the profile's own plane.
    ProfilePlane {},
    /// Begin on a plane parallel to the profile plane at a signed offset.
    OffsetProfilePlane {
        /// Signed offset along the profile normal in canonical millimeters.
        offset: Dimension<Length>,
    },
    /// Begin on a selected face, optionally displaced along the extrusion direction.
    FromFace {
        /// Face defining the start plane.
        face: FaceSelection,
        /// Signed displacement from the selected face.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Dimension<Length>>,
    },
    /// Begin on a selected plane that has no face identity.
    FromPlane {
        /// Model-space frame of the start plane.
        plane: FeatureSupportPlaneFrame,
        /// Native record that stores the plane.
        native: NonBlankString,
    },
}

/// One-sided termination law of a linear sweep. Sidedness around the profile
/// plane is stated by the owning feature's extent type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinearTermination {
    /// Native termination is present structurally but unresolved.
    Unresolved {},
    /// Fixed travel distance.
    Blind {
        /// Fixed travel distance.
        length: Dimension<NonZeroLength>,
    },
    /// Extends through all material.
    ThroughAll {},
    /// Extends until it exits the next material region.
    ThroughNext {},
    /// Extends until the first encountered model face.
    ToFirst {},
    /// Extends until the last encountered model face.
    ToLast {},
    /// Extends until it reaches a target face.
    ToFace {
        /// Face terminating the operation.
        face: FaceSelection,
        /// Signed displacement from the terminating face.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Dimension<Length>>,
    },
    /// Extends until it reaches a target vertex.
    ToVertex {
        /// Vertex terminating the operation.
        vertex: VertexSelection,
    },
    /// Extends until it reaches a target plane that has no face identity.
    ToPlane {
        /// Model-space frame of the terminating plane.
        plane: FeatureSupportPlaneFrame,
        /// Native record that stores the plane.
        native: NonBlankString,
    },
    /// Extends to a fixed offset from a target face.
    OffsetFromFace {
        /// Face the termination is measured from.
        face: FaceSelection,
        /// Offset distance from the face.
        offset: Dimension<PositiveLength>,
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
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AngularTermination {
    /// Native termination is present structurally but unresolved.
    Unresolved {},
    /// Extends through all material.
    ThroughAll {},
    /// Extends until it exits the next material region.
    ThroughNext {},
    /// Extends until the first encountered model face.
    ToFirst {},
    /// Extends until the last encountered model face.
    ToLast {},
    /// Extends until it reaches a target face.
    ToFace {
        /// Face terminating the operation.
        face: FaceSelection,
        /// Signed displacement from the terminating face.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Dimension<Length>>,
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
        offset: Dimension<PositiveLength>,
    },
    /// Extends until one of the faces in a selected target shape.
    ToShape {
        /// Native or resolved target shape selection.
        target: FaceSelection,
    },
    /// Fixed angular travel.
    Angle {
        /// Angular travel.
        angle: Dimension<PositiveAngle>,
    },
}

/// One side of an extrusion: its termination law and side-local modifiers.
/// Drafts are measured from the profile plane outward along the side's
/// travel; an absent draft leaves the side walls parallel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ExtrudeSide {
    /// Where this side's travel terminates.
    pub termination: LinearTermination,
    /// Draft angle applied to this side's walls, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_draft"
    )]
    pub draft: Option<Dimension<SlopeAngle>>,
}

impl Default for ExtrudeStart {
    fn default() -> Self {
        Self::ProfilePlane {}
    }
}

/// Extrusion sidedness around the profile plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
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

/// Revolution sides use the right-hand rule about the stated axis direction.
/// The source profile is at zero radians. One-sided travel runs from zero to
/// the positive angle. Two-sided travel runs from zero to the first angle and
/// from zero to the negative second angle. Symmetric travel runs from minus
/// half the total angle to plus half the total angle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
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

impl RevolveExtent {
    /// Whether each fixed angle and their combined travel fit within one turn.
    #[must_use]
    pub fn angles_within_turn(&self) -> bool {
        let angle = |termination: &AngularTermination| match termination {
            AngularTermination::Angle { angle } => angle.literal().map_or(0.0, |angle| angle.get()),
            _ => 0.0,
        };
        let total = match self {
            Self::OneSided { termination } | Self::Symmetric { termination } => angle(termination),
            Self::TwoSided { first, second } => angle(first) + angle(second),
        };
        total <= std::f64::consts::TAU
    }
}

/// Persisted source of a resolved linear-extrusion direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtrusionDirectionSource {
    /// Direction comes from the persisted direction vector.
    Custom {},
    /// Direction comes from a selected straight edge.
    Edge {
        /// Native edge selection used as the direction axis.
        reference: PathRef,
    },
    /// Direction comes from the source profile's plane normal.
    ProfileNormal {},
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
    Other(NonBlankString),
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
            _ => Self::Other(NonBlankString::new(class)?),
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

/// Relationship between outer-wire and inner-wire taper directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub enum HelixConstructionStyle {
    /// Historical construction retained for document compatibility.
    Legacy,
    /// Corrected construction used by newly created features.
    Corrected,
}

/// Axial or radial construction law of a helix feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HelixShape {
    /// Constant-radius helix with signed axial rise per revolution.
    Cylindrical {
        /// The two actual authored controls of axial travel.
        law: HelixLaw,
    },
    /// Conical helix with signed axial rise and cone half-angle.
    Conical {
        /// The two actual authored controls of axial travel.
        law: HelixLaw,
        /// Cone half-angle.
        cone_angle: Dimension<SlopeAngle>,
    },
    /// Planar spiral with signed radial growth per revolution.
    Spiral {
        /// Signed radial growth per revolution.
        radial_growth: Dimension<Length>,
        /// Canonical positive number of revolutions.
        revolutions: Dimension<PositiveReal>,
    },
}

/// Result topology retained by a projection-on-surface operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

impl From<SolidSweepOperation> for BooleanOp {
    fn from(value: SolidSweepOperation) -> Self {
        match value {
            SolidSweepOperation::NewBody => Self::NewBody,
            SolidSweepOperation::Join => Self::Join,
            SolidSweepOperation::Cut => Self::Cut,
            SolidSweepOperation::Intersect => Self::Intersect,
        }
    }
}

impl TryFrom<BooleanOp> for SolidSweepOperation {
    type Error = BooleanOp;

    fn try_from(value: BooleanOp) -> Result<Self, Self::Error> {
        match value {
            BooleanOp::NewBody => Ok(Self::NewBody),
            BooleanOp::Join => Ok(Self::Join),
            BooleanOp::Cut => Ok(Self::Cut),
            BooleanOp::Intersect => Ok(Self::Intersect),
            BooleanOp::Unresolved => Err(value),
        }
    }
}

/// Placement and parameterization of a solid Coil primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CoilConstruction {
    /// Axis frame and angular origin.
    pub placement: CoilPlacement,
    /// Diameter of the reference trajectory at its start.
    pub diameter: Dimension<PositiveLength>,
    /// Independent driving dimensions retained from the source feature.
    pub extent: CoilExtent,
    /// Generated section swept along the trajectory.
    pub section: CoilSection,
    /// Radial position of the section relative to the reference trajectory.
    pub section_placement: CoilSectionPlacement,
    /// Angular travel direction when viewed from the axis origin along the positive axis.
    pub clockwise: bool,
    /// Signed cone half-angle of an axial coil; zero produces a cylindrical helix.
    pub taper: Dimension<Angle>,
}

/// Geometric placement of a Coil trajectory.
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
#[serde(deny_unknown_fields)]
enum CoilPlacementWire {
    /// Complete model-space axis and radial frame.
    Explicit {
        /// Model-space origin of the trajectory frame.
        origin: Point3,
        /// Positive trajectory axis direction.
        axis: Vector3,
        /// Angular-zero radial direction.
        radial: Vector3,
    },
    /// Placement retained in one source-native construction aggregate.
    Native {
        /// Nonblank native record or scope containing the placement semantics.
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
/// Independent driving dimensions of a Coil trajectory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CoilExtent {
    /// Axial coil driven by revolution count and total signed height.
    RevolutionsHeight {
        /// Positive angular-turn count.
        revolutions: Dimension<PositiveReal>,
        /// Signed axial travel.
        height: Dimension<Length>,
    },
    /// Axial coil driven by revolution count and signed pitch per revolution.
    RevolutionsPitch {
        /// Positive angular-turn count.
        revolutions: Dimension<PositiveReal>,
        /// Signed axial travel per revolution.
        pitch: Dimension<NonZeroLength>,
    },
    /// Axial coil driven by total signed height and signed pitch per revolution.
    HeightPitch {
        /// Signed axial travel.
        height: Dimension<NonZeroLength>,
        /// Signed axial travel per revolution.
        pitch: Dimension<NonZeroLength>,
    },
    /// Planar spiral driven by revolution count and signed radial pitch.
    Spiral {
        /// Positive angular-turn count.
        revolutions: Dimension<PositiveReal>,
        /// Signed radial growth per revolution.
        radial_pitch: Dimension<NonZeroLength>,
    },
}

/// Generated cross-section of a Coil primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CoilSection {
    /// Circular section whose size is its diameter.
    Circular {
        /// Circle diameter.
        diameter: Dimension<PositiveLength>,
    },
    /// Square section whose size is its edge length.
    Square {
        /// Edge length.
        size: Dimension<PositiveLength>,
    },
    /// Equilateral triangle pointing radially away from the axis.
    ExternalTriangle {
        /// Edge length.
        size: Dimension<PositiveLength>,
    },
    /// Equilateral triangle pointing radially toward the axis.
    InternalTriangle {
        /// Edge length.
        size: Dimension<PositiveLength>,
    },
}

/// Radial placement of a generated Coil section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub enum CoilResult {
    /// Create an independent body.
    NewBody {},
    /// Combine the swept volume with selected existing bodies.
    Boolean {
        /// Join, cut, or intersection operation.
        operation: BooleanKind,
        /// Existing bodies participating in the operation.
        targets: BodySelection,
    },
}

/// Result semantics of a swept profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum SweepMode {
    /// Native sweep family is known but its result subtype is unresolved.
    Unresolved {},
    /// Sweep creates or modifies a solid body.
    Solid {
        /// Independent-body creation or boolean combination with existing bodies.
        op: SolidSweepOperation,
    },
    /// Sweep creates a sheet body.
    Surface {},
}

/// How a solid sweep result meets the existing bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SolidSweepOperation {
    /// Create an independent body.
    NewBody,
    /// Add the swept volume to existing bodies.
    Join,
    /// Remove the swept volume from existing bodies.
    Cut,
    /// Keep the intersection with existing bodies.
    Intersect,
}

/// Directed fractions of a sweep path consumed from the profile location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SweepPathExtent {
    /// Fraction consumed in the path's forward traversal direction.
    pub along_fraction: Dimension<Fraction>,
    /// Fraction consumed in the path's reverse traversal direction.
    pub against_fraction: Dimension<Fraction>,
}

/// Guide rail controlling a sweep, with its directed consumed extent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SweepGuideRail {
    /// Ordered guide trajectory.
    pub path: PathRef,
    /// Fractions consumed on either side of the profile location.
    pub extent: SweepPathExtent,
}

/// The generated cross-section of a result that generates none.
///
/// A sheet sweep owns no geometry of its own, so this type has no value and
/// [`SweepSection::Generated`] is unreachable for a sheet section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum NoGeneratedSection {}

/// A cross-section of a result that generates no geometry of its own.
pub type SheetSweepSection = SweepSection<NoGeneratedSection>;

/// Cross-section owned or referenced by a sweep construction.
///
/// `G` names the geometry the result may generate. A solid result generates a
/// circular region; a sheet result generates nothing, and its `G` is
/// uninhabited.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SweepSection<G = GeneratedSweepSection> {
    /// The source requires a cross-section, but its carrier is unresolved.
    Unresolved(#[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")] Option<String>),
    /// Cross-section supplied by referenced profile geometry.
    Profile(PlanarProfileRef),
    /// Cross-section generated by the sweep construction itself.
    Generated(G),
}

impl<G> SweepSection<G> {
    /// Returns the referenced profile when this section does not own its geometry.
    pub const fn referenced_profile(&self) -> Option<&PlanarProfileRef> {
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

    /// Whether the source requires a cross-section it did not resolve.
    pub const fn is_unresolved(&self) -> bool {
        matches!(self, Self::Unresolved(_))
    }

    /// The native carrier named by an unresolved cross-section.
    pub fn unresolved_native(&self) -> Option<&str> {
        match self {
            Self::Unresolved(native) => native.as_deref(),
            Self::Profile(_) | Self::Generated(_) => None,
        }
    }
}

/// A sweep circular section with an optional wall thinner than its radius.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SweepCircularRegion {
    outer_radius: RadialDimension<PositiveLength>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wall_thickness: Option<Dimension<PositiveLength>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SweepCircularRegionWire {
    outer_radius: RadialDimension<PositiveLength>,
    #[serde(default, deserialize_with = "deserialize_wall_thickness")]
    wall_thickness: Option<Dimension<PositiveLength>>,
}

impl SweepCircularRegion {
    /// Admit a disk or a wall thinner than the outer radius where literals decide.
    pub fn new(
        outer_radius: RadialDimension<PositiveLength>,
        wall_thickness: Option<Dimension<PositiveLength>>,
    ) -> Result<Self, &'static str> {
        if outer_radius
            .literal_radius()
            .zip(wall_thickness.as_ref().and_then(Dimension::literal))
            .is_some_and(|(outer, wall)| wall.get() >= outer.get())
        {
            return Err("wall_thickness must be less than outer_radius");
        }
        Ok(Self {
            outer_radius,
            wall_thickness,
        })
    }
    /// The canonical outer radius dimension.
    pub const fn outer_radius(&self) -> &RadialDimension<PositiveLength> {
        &self.outer_radius
    }
    /// The canonical wall-thickness dimension, if supplied.
    pub const fn wall_thickness(&self) -> Option<&Dimension<PositiveLength>> {
        self.wall_thickness.as_ref()
    }
}

impl<'de> Deserialize<'de> for SweepCircularRegion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = SweepCircularRegionWire::deserialize(deserializer)?;
        Self::new(wire.outer_radius, wire.wall_thickness).map_err(serde::de::Error::custom)
    }
}

impl From<SheetSweepSection> for SweepSection {
    fn from(section: SheetSweepSection) -> Self {
        match section {
            SweepSection::Unresolved(native) => Self::Unresolved(native),
            SweepSection::Profile(profile) => Self::Profile(profile),
            SweepSection::Generated(generated) => match generated {},
        }
    }
}

/// Sweep cross-sections nested under the result mode they belong to.
///
/// A sheet result generates no geometry, so its sections carry an uninhabited
/// generated arm and a generated circular region under a sheet result has no
/// spelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "mode", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SweepShape {
    /// Native sweep family is known but its result subtype is unresolved.
    Unresolved {
        /// Primary cross-section.
        section: SheetSweepSection,
        /// Additional cross-sections in path order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sections: Vec<SheetSweepSection>,
    },
    /// Sweep creates or modifies a solid body.
    Solid {
        /// Independent-body creation or boolean combination with existing bodies.
        op: SolidSweepOperation,
        /// Primary cross-section.
        section: SweepSection,
        /// Additional cross-sections in path order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sections: Vec<SweepSection>,
    },
    /// Sweep creates a sheet body.
    Surface {
        /// Primary cross-section.
        section: SheetSweepSection,
        /// Additional cross-sections in path order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sections: Vec<SheetSweepSection>,
    },
}

impl SweepShape {
    /// Construct an unresolved sweep section and result mode.
    pub fn unresolved(native: Option<String>) -> Self {
        Self::Unresolved {
            section: SweepSection::Unresolved(native),
            sections: Vec::new(),
        }
    }

    /// A sweep under `mode` whose cross-sections generate no geometry.
    ///
    /// Referenced and unresolved cross-sections are admissible under every
    /// result, so a caller that generated nothing may name any mode.
    pub fn sheet_sections(
        mode: SweepMode,
        section: SheetSweepSection,
        sections: Vec<SheetSweepSection>,
    ) -> Self {
        match mode {
            SweepMode::Unresolved {} => Self::Unresolved { section, sections },
            SweepMode::Surface {} => Self::Surface { section, sections },
            SweepMode::Solid { op } => Self::Solid {
                op,
                section: section.into(),
                sections: sections.into_iter().map(Into::into).collect(),
            },
        }
    }

    /// Return the result mode.
    pub const fn mode(&self) -> SweepMode {
        match self {
            Self::Unresolved { .. } => SweepMode::Unresolved {},
            Self::Solid { op, .. } => SweepMode::Solid { op: *op },
            Self::Surface { .. } => SweepMode::Surface {},
        }
    }

    /// Whether the primary cross-section is unresolved.
    pub const fn section_is_unresolved(&self) -> bool {
        match self {
            Self::Unresolved { section, .. } | Self::Surface { section, .. } => {
                section.is_unresolved()
            }
            Self::Solid { section, .. } => section.is_unresolved(),
        }
    }

    /// Whether any cross-section is unresolved.
    pub fn any_section_is_unresolved(&self) -> bool {
        match self {
            Self::Unresolved { section, sections } | Self::Surface { section, sections } => {
                section.is_unresolved() || sections.iter().any(SweepSection::is_unresolved)
            }
            Self::Solid {
                section, sections, ..
            } => section.is_unresolved() || sections.iter().any(SweepSection::is_unresolved),
        }
    }

    /// Whether any cross-section is unresolved and names a native carrier.
    pub fn any_section_names_an_unresolved_carrier(&self) -> bool {
        match self {
            Self::Unresolved { section, sections } | Self::Surface { section, sections } => {
                std::iter::once(section)
                    .chain(sections)
                    .any(|section| section.unresolved_native().is_some())
            }
            Self::Solid {
                section, sections, ..
            } => std::iter::once(section)
                .chain(sections)
                .any(|section| section.unresolved_native().is_some()),
        }
    }

    /// Number of additional cross-sections beyond the primary one.
    pub fn additional_section_count(&self) -> usize {
        match self {
            Self::Unresolved { sections, .. } | Self::Surface { sections, .. } => sections.len(),
            Self::Solid { sections, .. } => sections.len(),
        }
    }

    /// Referenced profile of the primary cross-section, when it references one.
    pub const fn referenced_profile(&self) -> Option<&PlanarProfileRef> {
        match self {
            Self::Unresolved { section, .. } | Self::Surface { section, .. } => {
                section.referenced_profile()
            }
            Self::Solid { section, .. } => section.referenced_profile(),
        }
    }

    /// Mutable referenced profile of the primary cross-section.
    pub fn referenced_profile_mut(&mut self) -> Option<&mut PlanarProfileRef> {
        match self {
            Self::Unresolved { section, .. } | Self::Surface { section, .. } => {
                section.referenced_profile_mut()
            }
            Self::Solid { section, .. } => section.referenced_profile_mut(),
        }
    }

    /// Every referenced profile, the primary cross-section first.
    pub fn referenced_profiles(&self) -> impl Iterator<Item = &PlanarProfileRef> + Clone {
        let (primary, sheet_sections, solid_sections): (
            Option<&PlanarProfileRef>,
            &[SheetSweepSection],
            &[SweepSection],
        ) = match self {
            Self::Unresolved { section, sections } | Self::Surface { section, sections } => {
                (section.referenced_profile(), sections, &[])
            }
            Self::Solid {
                section, sections, ..
            } => (section.referenced_profile(), &[], sections),
        };
        primary
            .into_iter()
            .chain(
                sheet_sections
                    .iter()
                    .filter_map(SweepSection::referenced_profile),
            )
            .chain(
                solid_sections
                    .iter()
                    .filter_map(SweepSection::referenced_profile),
            )
    }

    /// Generated cross-sections, mutable, the primary cross-section first.
    ///
    /// Only a solid result generates geometry.
    pub fn generated_sections_mut(&mut self) -> impl Iterator<Item = &mut GeneratedSweepSection> {
        let (section, sections) = match self {
            Self::Unresolved { .. } | Self::Surface { .. } => (None, &mut [][..]),
            Self::Solid {
                section, sections, ..
            } => (Some(section), sections.as_mut_slice()),
        };
        section
            .into_iter()
            .chain(sections.iter_mut())
            .filter_map(|section| match section {
                SweepSection::Generated(generated) => Some(generated),
                SweepSection::Unresolved(_) | SweepSection::Profile(_) => None,
            })
    }

    /// The generated region of the primary cross-section, when the sweep owns it.
    pub const fn generated_section(&self) -> Option<&GeneratedSweepSection> {
        match self {
            Self::Unresolved { .. } | Self::Surface { .. } => None,
            Self::Solid { section, .. } => match section {
                SweepSection::Generated(generated) => Some(generated),
                SweepSection::Unresolved(_) | SweepSection::Profile(_) => None,
            },
        }
    }

    /// A solid sweep under `op` with the given cross-sections.
    #[must_use]
    pub fn solid_sections(
        op: SolidSweepOperation,
        section: SweepSection,
        sections: Vec<SweepSection>,
    ) -> Self {
        Self::Solid {
            op,
            section,
            sections,
        }
    }

    /// Take the cross-sections out of the shape, in path order.
    #[must_use]
    pub fn into_sections(self) -> (SweepSection, Vec<SweepSection>) {
        match self {
            Self::Unresolved { section, sections } | Self::Surface { section, sections } => (
                section.into(),
                sections.into_iter().map(Into::into).collect(),
            ),
            Self::Solid {
                section, sections, ..
            } => (section, sections),
        }
    }

    /// Replaces the primary cross-section with referenced profile geometry.
    pub fn set_referenced_profile(&mut self, profile: PlanarProfileRef) {
        match self {
            Self::Unresolved { section, .. } | Self::Surface { section, .. } => {
                *section = SweepSection::Profile(profile);
            }
            Self::Solid { section, .. } => *section = SweepSection::Profile(profile),
        }
    }
}

/// Cross-section geometry generated and owned by a sweep construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "shape", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum GeneratedSweepSection {
    /// Filled or hollow circular region centered on the sweep path.
    CircularRegion {
        /// Outer radius and optional smaller radial wall thickness.
        region: SweepCircularRegion,
    },
}

/// One directed use of a solved sketch curve in an arrangement boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
struct SketchProfileLoopsWire {
    /// The exterior-loop index.
    outer: u32,
    /// The hole-loop indices in source order.
    #[serde(default)]
    holes: Vec<u32>,
}

impl SketchProfileLoops {
    /// Admit a whole-loop region with distinct holes that exclude its outer loop.
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
    type Error = FeatureCollectionError;
    fn try_from(wire: SketchProfileLoopsWire) -> Result<Self, Self::Error> {
        let mut seen = std::collections::HashSet::new();
        seen.try_reserve(wire.holes.len())
            .map_err(|_| FeatureCollectionError::Invalid("hole member allocation failed"))?;
        for hole in &wire.holes {
            if !seen.insert(hole) {
                return Err(FeatureCollectionError::Invalid("holes must be distinct"));
            }
        }
        if seen.contains(&wire.outer) {
            return Err(FeatureCollectionError::Invalid(
                "holes must not contain outer",
            ));
        }
        Ok(Self {
            outer: wire.outer,
            holes: DistinctMembers(wire.holes),
        })
    }
}

/// One connected planar region bounded by solved sketch curves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "region", rename_all = "snake_case", deny_unknown_fields)]
pub enum SketchProfileRegion {
    /// Exterior and holes are complete entries in the sketch profile table.
    Loops {
        /// Whole-loop exterior and hole indices.
        #[serde(flatten)]
        loops: SketchProfileLoops,
    },
    /// Boundary rings switch source curves at arrangement intersections.
    Trimmed {
        /// Directed exterior boundary ring.
        outer_boundary: NonEmptyMembers<SketchProfileBoundaryUse>,
        /// Directed hole boundary rings.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        hole_boundaries: Vec<NonEmptyMembers<SketchProfileBoundaryUse>>,
    },
}

impl SketchProfileRegion {
    /// Admit a whole-loop region.
    pub fn loops(outer: u32, holes: Vec<u32>) -> Result<Self, &'static str> {
        SketchProfileLoops::new(outer, holes).map(|loops| Self::Loops { loops })
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
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SweepOrientation {
    /// Rotation-minimizing corrected-Frenet frame.
    CorrectedFrenet {},
    /// Fixed section frame.
    Fixed {},
    /// Exact Frenet frame from path derivatives.
    Frenet {},
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
        direction: UnitVector3,
    },
}

/// Corner continuation used by a sweep path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

/// Signed axial travel and radial growth, named by which components move.
///
/// Each arm carries only the components that move, and each is a
/// [`NonZeroLength`], which refuses zero and every non-finite value. A helix
/// that neither rises nor grows has no spelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum HelicalSweepTravel {
    /// The helix rises along its axis at a constant radius.
    Axial {
        /// Total signed axial travel.
        height: Dimension<NonZeroLength>,
    },
    /// The helix grows radially in its base plane.
    Radial {
        /// Signed radial change per turn.
        radial_growth: Dimension<NonZeroLength>,
    },
    /// The helix rises and grows.
    Conical {
        /// Total signed axial travel.
        height: Dimension<NonZeroLength>,
        /// Signed radial change per turn.
        radial_growth: Dimension<NonZeroLength>,
    },
}

impl HelicalSweepTravel {
    /// Admit signed travel with at least one potentially nonzero component.
    pub fn new(height: Dimension<Length>, radial_growth: Dimension<Length>) -> Option<Self> {
        let nonzero = |dimension: Dimension<Length>| match dimension {
            Dimension::Literal(value) => {
                NonZeroLength::try_from(value).ok().map(Dimension::Literal)
            }
            Dimension::Parameter(parameter) => Some(Dimension::Parameter(parameter)),
        };
        match (nonzero(height), nonzero(radial_growth)) {
            (None, None) => None,
            (Some(height), None) => Some(Self::Axial { height }),
            (None, Some(radial_growth)) => Some(Self::Radial { radial_growth }),
            (Some(height), Some(radial_growth)) => Some(Self::Conical {
                height,
                radial_growth,
            }),
        }
    }
    /// Return the canonical signed axial travel or an unbound zero.
    pub fn height(&self) -> Dimension<Length> {
        match self {
            Self::Radial { .. } => Dimension::Literal(Length::ZERO),
            Self::Axial { height } | Self::Conical { height, .. } => {
                height.clone().map_literal(Into::into)
            }
        }
    }
    /// Return the canonical signed radial growth or an unbound zero.
    pub fn radial_growth(&self) -> Dimension<Length> {
        match self {
            Self::Axial { .. } => Dimension::Literal(Length::ZERO),
            Self::Radial { radial_growth } | Self::Conical { radial_growth, .. } => {
                radial_growth.clone().map_literal(Into::into)
            }
        }
    }
}

/// Complete construction of a solid helical sweep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HelicalSweepConstruction {
    /// Profile swept along the helical path.
    pub profile: PlanarProfileRef,
    /// Point at the start of the helix axis.
    pub axis_origin: FinitePoint3,
    /// Unit direction of positive axial travel.
    pub axis_direction: UnitVector3,
    /// Persisted authoring law identifying the independent parameters.
    pub law: HelicalSweepLaw,
    /// Nonnegative axial advance per turn; zero gives a planar spiral.
    pub pitch: Dimension<NonNegativeLength>,
    /// Signed axial travel and radial change per turn.
    pub travel: HelicalSweepTravel,
    /// Positive number of turns.
    pub turns: Dimension<PositiveReal>,
    /// Cone half-angle corresponding to radial growth.
    pub cone_angle: Dimension<Angle>,
    /// Whether angular travel is left-handed along the positive axis.
    pub left_handed: bool,
    /// Whether path travel runs opposite the declared axis direction.
    pub reversed: bool,
    /// Relative tolerance used while joining the generated sweep, when persisted.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tolerance"
    )]
    pub tolerance: Option<Dimension<PositiveReal>>,
    /// Whether a profile containing multiple faces is accepted as one operation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_allow_multi_profile_faces"
    )]
    pub allow_multi_profile_faces: Option<bool>,
}

/// Independent-parameter law used to author a helical sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct BinderSource {
    /// Bound object identity.
    pub target: BinderTarget,
    /// Ordered native subelement selectors; empty selects the complete object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[serde(deserialize_with = "deserialize_local_subelements")]
    pub subelements: Vec<NonBlankString>,
}

/// Resolved or externally scoped binder target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
        document: NonBlankString,
        /// Object identity within the source document.
        #[serde(deserialize_with = "deserialize_local_object")]
        object: NonBlankString,
    },
    /// Source-native target identity that cannot be resolved further.
    Native {
        /// Opaque source-native target identity.
        #[serde(deserialize_with = "deserialize_local_reference")]
        reference: NonBlankString,
    },
}

/// Binding behavior and optional derived-shape construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_binder_construction_offset"
        )]
        offset: Option<BinderOffset>,
        /// Context object used to interpret relative placement.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_context"
        )]
        context: Option<BinderTarget>,
    },
}

/// Update lifecycle of a subshape binder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub enum BinderCopyOnChange {
    /// Do not clone configurable source properties.
    Disabled,
    /// Clone configurable source properties when they change.
    Enabled,
    /// A private source copy has already been mutated.
    Mutated,
}

/// Two-dimensional offset applied to bound faces or wires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BinderOffset {
    /// Signed offset distance.
    pub distance: Dimension<NonZeroLength>,
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
#[serde(deny_unknown_fields)]
pub enum BinderOffsetJoin {
    /// Circular corner arcs.
    Arcs,
    /// Tangent continuation.
    Tangent,
    /// Sharp line-line intersections.
    Intersection,
}

/// Profile consumed by a profile-driven feature that admits no spatial-sketch
/// form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PlanarProfileRef {
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

/// Profile consumed by a profile-driven feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProfileRef {
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
    /// Any profile form that does not select spatial-sketch geometry.
    #[serde(untagged)]
    Planar(PlanarProfileRef),
}

impl From<crate::sketches::SketchId> for PlanarProfileRef {
    fn from(sketch: crate::sketches::SketchId) -> Self {
        Self::Sketch(sketch)
    }
}

impl PlanarProfileRef {
    /// Retain a native profile reference.
    #[must_use]
    pub const fn native(reference: String) -> Self {
        Self::Native(reference)
    }
}

impl ProfileRef {
    /// Planar form of this profile, when it selects no spatial-sketch geometry.
    #[must_use]
    pub const fn planar(&self) -> Option<&PlanarProfileRef> {
        match self {
            Self::Planar(profile) => Some(profile),
            _ => None,
        }
    }
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
#[serde(deny_unknown_fields)]
pub enum LoftPointSection {
    /// Source-native point-selection record whose position is not resolved.
    #[serde(rename = "native_point")]
    Native(#[serde(deserialize_with = "deserialize_local_value")] NonBlankString),
    /// Solved model-space point section.
    Point(FinitePoint3),
    /// Solved B-rep vertex section.
    Vertex(VertexId),
}

/// Persistent identity of a curve in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
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
        native: NonBlankString,
    },
}

impl PlanarProfileRef {
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

    /// Admit nonempty distinct members.
    pub fn sketch_profiles(
        sketch: crate::sketches::SketchId,
        profiles: Vec<u32>,
    ) -> Result<Self, BodySelectionError> {
        let profiles = SelectionMembers::try_from(profiles)?;
        Ok(Self::SketchProfiles { sketch, profiles })
    }

    /// Admit nonempty distinct members.
    pub fn sketch_entities(
        sketch: crate::sketches::SketchId,
        entities: Vec<crate::sketches::SketchEntityId>,
    ) -> Result<Self, BodySelectionError> {
        let entities = SelectionMembers::try_from(entities)?;
        Ok(Self::SketchEntities { sketch, entities })
    }

    /// Admit nonempty distinct members.
    pub fn sketch_selection(
        sketch: crate::sketches::SketchId,
        selections: Vec<String>,
    ) -> Result<Self, BodySelectionError> {
        let selections = NativeSelections::try_from(selections)?;
        Ok(Self::SketchSelection { sketch, selections })
    }

    /// Admit nonempty distinct members.
    pub fn historical_faces(
        state: FeatureInputTopologyId,
        faces: Vec<HistoricalFaceId>,
        native: Vec<String>,
    ) -> Result<Self, BodySelectionError> {
        let faces = SelectionMembers::try_from(faces)?;
        let native = NativeSelections::try_from(native)?;
        Ok(Self::HistoricalFaces {
            state,
            faces,
            native,
        })
    }
    /// Admits generated profile curves and their native reference.
    pub fn generated(
        curves: Vec<GeneratedCurveRef>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let curves = curves.try_into()?;
        SelectionReference::new(native).map(|native| Self::Generated { curves, native })
    }
}

impl ProfileRef {
    /// Admit nonempty distinct members.
    pub fn spatial_sketch_profiles(
        sketch: crate::sketches::SpatialSketchId,
        profiles: Vec<u32>,
    ) -> Result<Self, BodySelectionError> {
        let profiles = SelectionMembers::try_from(profiles)?;
        Ok(Self::SpatialSketchProfiles { sketch, profiles })
    }

    /// Admit nonempty distinct members.
    pub fn spatial_sketch_selection(
        sketch: crate::sketches::SpatialSketchId,
        selections: Vec<String>,
    ) -> Result<Self, BodySelectionError> {
        let selections = NativeSelections::try_from(selections)?;
        Ok(Self::SpatialSketchSelection { sketch, selections })
    }
}

impl PathRef {
    /// Admit nonempty distinct members.
    pub fn sketch_curves(
        sketch: crate::sketches::SketchId,
        curves: Vec<crate::sketches::SketchEntityId>,
    ) -> Result<Self, BodySelectionError> {
        let curves = SelectionMembers::try_from(curves)?;
        Ok(Self::SketchCurves { sketch, curves })
    }

    /// Admit nonempty distinct members.
    pub fn spatial_sketch_curves(
        sketch: crate::sketches::SpatialSketchId,
        curves: Vec<crate::sketches::SpatialSketchEntityId>,
    ) -> Result<Self, BodySelectionError> {
        let curves = SelectionMembers::try_from(curves)?;
        Ok(Self::SpatialSketchCurves { sketch, curves })
    }

    /// Admit nonempty distinct members.
    pub fn historical_edges(
        state: FeatureInputTopologyId,
        edges: Vec<HistoricalEdgeId>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let edges = SelectionMembers::try_from(edges)?;

        let native = NonBlankString::new(native).ok_or(BodySelectionError::BlankNativeMember)?;
        Ok(Self::HistoricalEdges {
            state,
            edges,
            native,
        })
    }
}

impl GeneratedCurveRef {
    /// Admit a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        SelectionReference::new(local_id).map(|local_id| Self { feature, local_id })
    }
}

/// Three distinct vertex targets with a common historical input state.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct ThreePointSelection(Box<[VertexSelection; 3]>);

impl TryFrom<Box<[VertexSelection; 3]>> for ThreePointSelection {
    type Error = &'static str;
    /// Admit three distinct vertex targets that use one historical input state.
    fn try_from(points: Box<[VertexSelection; 3]>) -> Result<Self, Self::Error> {
        for (first, second) in [(0, 1), (0, 2), (1, 2)] {
            if selection_overlap::vertex_targets_equal(&points[first], &points[second]) {
                return Err("points must select three distinct vertex targets");
            }
        }
        let mut state: Option<&FeatureInputTopologyId> = None;
        for point in points.iter() {
            if let VertexSelection::Historical {
                state: candidate, ..
            } = point
            {
                match state {
                    Some(state) if state.as_str() != candidate.as_str() => {
                        return Err("points must use the same input topology");
                    }
                    Some(_) => {}
                    None => state = Some(candidate),
                }
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

/// Geometry used to partition bodies in a `SplitBody` operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SplitBodyTool {
    /// Surface faces extended as necessary to partition the targets.
    Faces(FaceSelection),
    /// Datum-plane feature extended through the target bodies.
    Plane {
        /// Earlier datum-plane feature supplying the splitting plane.
        plane: FeatureId,
    },
}

/// Geometry used to partition faces in a `SplitFace` operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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

/// Deformation applied by a flex feature.
///
/// Each resolved arm carries the one magnitude its family needs, and the
/// unresolved arm carries no magnitude at all, so a magnitude beside an
/// unresolved family has no spelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum FlexMode {
    /// Deformation family whose required magnitude remains unresolved.
    Unresolved {
        /// Deformation family, when the source named one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_form"
        )]
        form: Option<FlexForm>,
    },
    /// Bend through a signed angle.
    Bending {
        /// Total bend angle.
        angle: Dimension<Angle>,
    },
    /// Twist through a signed angle.
    Twisting {
        /// Total twist angle.
        angle: Dimension<Angle>,
    },
    /// Scale transverse sections by a dimensionless factor.
    Tapering {
        /// End-to-start transverse scale ratio.
        factor: Dimension<PositiveReal>,
    },
    /// Extend or contract along the flex axis.
    Stretching {
        /// Signed change in length.
        distance: Dimension<Length>,
    },
}

/// Structural form of a flex deformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_plane, FeatureId, "plane");
cadmpeg_core::named_optional_field!(deserialize_pull, DraftPull, "pull");
cadmpeg_core::named_optional_field!(deserialize_edge_geometry_tangent, UnitVector3, "tangent");
cadmpeg_core::named_optional_field!(deserialize_edge_geometry_start, FinitePoint3, "start");
cadmpeg_core::named_optional_field!(deserialize_edge_geometry_end, FinitePoint3, "end");
cadmpeg_core::named_optional_field!(deserialize_source_index, u32, "source_index");
cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_material, String, "material");
cadmpeg_core::named_optional_field!(deserialize_bodies, DistinctMembers<BodyId>, "bodies");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_owner, FeatureId, "owner");
cadmpeg_core::named_optional_field!(deserialize_display, DimensionDisplay, "display");
cadmpeg_core::named_optional_field!(deserialize_value, ParameterValue, "value");
cadmpeg_core::named_optional_field!(deserialize_pmi, crate::ids::PmiId, "pmi");
cadmpeg_core::named_optional_field!(deserialize_display_text, String, "display_text");
cadmpeg_core::named_optional_field!(
    deserialize_regeneration_parent,
    FeatureId,
    "regeneration_parent"
);
cadmpeg_core::named_optional_field!(deserialize_source_tag, String, "source_tag");
cadmpeg_core::named_optional_field!(deserialize_source_text, String, "source_text");
cadmpeg_core::named_optional_field!(deserialize_active_child, FeatureId, "active_child");
cadmpeg_core::named_optional_field!(deserialize_diameter<Q>, Q, "diameter");
cadmpeg_core::named_optional_field!(deserialize_extent, CosmeticThreadExtent, "extent");
cadmpeg_core::named_optional_field!(deserialize_opacity<Q>, Q, "opacity");
cadmpeg_core::named_optional_field!(deserialize_thread_standard, NonBlankString, "standard");
cadmpeg_core::named_optional_field!(
    deserialize_thread_designation,
    NonBlankString,
    "designation"
);
cadmpeg_core::named_optional_field!(deserialize_thread_pitch<Q>, Q, "pitch");
cadmpeg_core::named_optional_field!(deserialize_thread_modeled, bool, "modeled");
cadmpeg_core::named_optional_field!(deserialize_reference, DatumPlaneReference, "reference");
cadmpeg_core::named_optional_field!(
    deserialize_construction,
    Box<DatumPointConstruction>,
    "construction"
);
cadmpeg_core::named_optional_field!(deserialize_bidirectional, bool, "bidirectional");
cadmpeg_core::named_optional_field!(
    deserialize_primitive_placement,
    FeatureRigidPlacement,
    "placement"
);
cadmpeg_core::named_optional_field!(deserialize_segment_turns<Q>, Q, "segment_turns");
cadmpeg_core::named_optional_field!(
    deserialize_construction_style,
    HelixConstructionStyle,
    "construction_style"
);
cadmpeg_core::named_optional_field!(
    deserialize_sketch,
    crate::sketches::SpatialSketchId,
    "sketch"
);
cadmpeg_core::named_optional_field!(
    deserialize_feature_operation_sketch,
    crate::sketches::SketchId,
    "sketch"
);
cadmpeg_core::named_optional_field!(deserialize_block, FeatureId, "block");
cadmpeg_core::named_optional_field!(
    deserialize_placement,
    crate::transform::Transform,
    "placement"
);
cadmpeg_core::named_optional_field!(deserialize_path, PathRef, "path");
cadmpeg_core::named_optional_field!(deserialize_orientation, SweepOrientation, "orientation");
cadmpeg_core::named_optional_field!(deserialize_transition, SweepTransition, "transition");
cadmpeg_core::named_optional_field!(
    deserialize_transformation,
    SweepTransformation,
    "transformation"
);
cadmpeg_core::named_optional_field!(deserialize_twist<Q>, Q, "twist");
cadmpeg_core::named_optional_field!(deserialize_path_extent, SweepPathExtent, "path_extent");
cadmpeg_core::named_optional_field!(deserialize_guide_rail, SweepGuideRail, "guide_rail");
cadmpeg_core::named_optional_field!(deserialize_taper<Q>, Q, "taper");
cadmpeg_core::named_optional_field!(deserialize_scale<Q>, Q, "scale");
cadmpeg_core::named_optional_field!(
    deserialize_allow_multi_profile_faces,
    bool,
    "allow_multi_profile_faces"
);
cadmpeg_core::named_optional_field!(
    deserialize_feature_operation_bodies,
    BodySelection,
    "bodies"
);
cadmpeg_core::named_optional_field!(deserialize_thickness<Q>, Q, "thickness");
cadmpeg_core::named_optional_field!(deserialize_outward, bool, "outward");
cadmpeg_core::named_optional_field!(deserialize_mode, ShellMode, "mode");
cadmpeg_core::named_optional_field!(deserialize_join, ShellJoin, "join");
cadmpeg_core::named_optional_field!(
    deserialize_resolve_intersections,
    bool,
    "resolve_intersections"
);
cadmpeg_core::named_optional_field!(
    deserialize_allow_self_intersections,
    bool,
    "allow_self_intersections"
);
cadmpeg_core::named_optional_field!(deserialize_approximate, bool, "approximate");
cadmpeg_core::named_optional_field!(
    deserialize_plane_reference,
    FaceSelection,
    "plane_reference"
);
cadmpeg_core::named_optional_field!(deserialize_side, ThickenSide, "side");
cadmpeg_core::named_optional_field!(deserialize_gap_tolerance<Q>, Q, "gap_tolerance");
cadmpeg_core::named_optional_field!(deserialize_merge_result, bool, "merge_result");
cadmpeg_core::named_optional_field!(deserialize_angle<Q>, Q, "angle");
cadmpeg_core::named_optional_field!(deserialize_alternate_face, bool, "alternate_face");
cadmpeg_core::named_optional_field!(deserialize_corner, RuledSurfaceCorner, "corner");
cadmpeg_core::named_optional_field!(deserialize_reverse, bool, "reverse");
cadmpeg_core::named_optional_field!(deserialize_rotation, AxisAngle, "rotation");
cadmpeg_core::named_optional_field!(deserialize_height<Q>, Q, "height");
cadmpeg_core::named_optional_field!(deserialize_elliptical, bool, "elliptical");
cadmpeg_core::named_optional_field!(deserialize_axis, FeatureDirection3, "axis");
cadmpeg_core::named_optional_field!(deserialize_center, ScaleCenter, "center");
cadmpeg_core::named_optional_field!(deserialize_profile, PlanarProfileRef, "profile");
cadmpeg_core::named_optional_field!(
    deserialize_profile_filter,
    HoleProfileFilter,
    "profile_filter"
);
cadmpeg_core::named_optional_field!(deserialize_face, FaceSelection, "face");
cadmpeg_core::named_optional_field!(deserialize_direction, FeatureDirection3, "direction");
cadmpeg_core::named_optional_field!(deserialize_placements, Vec<HolePlacement>, "placements");
cadmpeg_core::named_optional_field!(
    deserialize_feature_operation_extent,
    LinearTermination,
    "extent"
);
cadmpeg_core::named_optional_field!(deserialize_bottom, HoleBottom, "bottom");
cadmpeg_core::named_optional_field!(deserialize_taper_angle<Q>, Q, "taper_angle");
cadmpeg_core::named_optional_field!(deserialize_solid, bool, "solid");
cadmpeg_core::named_optional_field!(deserialize_face_maker, FaceMaker, "face_maker");
cadmpeg_core::named_optional_field!(
    deserialize_inner_wire_taper,
    InnerWireTaper,
    "inner_wire_taper"
);
cadmpeg_core::named_optional_field!(
    deserialize_length_along_profile_normal,
    bool,
    "length_along_profile_normal"
);
cadmpeg_core::named_optional_field!(
    deserialize_max_degree,
    Dimension<std::num::NonZeroU32>,
    "max_degree"
);
cadmpeg_core::named_optional_field!(deserialize_source, ExtrusionDirectionSource, "source");
cadmpeg_core::named_optional_field!(deserialize_face_maker_class, FaceMaker, "face_maker_class");
cadmpeg_core::named_optional_field!(deserialize_fuse_order, RevolutionFuseOrder, "fuse_order");
cadmpeg_core::named_optional_field!(
    deserialize_partial_revolve_construction_axis,
    RevolutionAxis,
    "axis"
);
cadmpeg_core::named_optional_field!(
    deserialize_partial_revolve_construction_extent,
    RevolveExtent,
    "extent"
);
cadmpeg_core::named_optional_field!(deserialize_revolution_axis_reference, PathRef, "reference");
cadmpeg_core::named_optional_field!(
    deserialize_rib_construction_direction,
    FeatureDirection3,
    "direction"
);
cadmpeg_core::named_optional_field!(deserialize_rib_construction_side, RibSide, "side");
cadmpeg_core::named_optional_field!(deserialize_offset<Q>, Q, "offset");
cadmpeg_core::named_optional_field!(deserialize_draft<Q>, Q, "draft");
cadmpeg_core::named_optional_field!(deserialize_wall_thickness<Q>, Q, "wall_thickness");
cadmpeg_core::named_optional_field!(deserialize_tolerance<Q>, Q, "tolerance");
cadmpeg_core::named_optional_field!(
    deserialize_binder_construction_offset,
    BinderOffset,
    "offset"
);
cadmpeg_core::named_optional_field!(deserialize_context, BinderTarget, "context");
cadmpeg_core::named_optional_field!(deserialize_form, FlexForm, "form");

mod identity_rewrite;

mod serialization;

impl FeatureDefinition {
    /// Canonical parameter identities bound to this definition's dimensions.
    pub fn dimension_parameters(&self) -> Result<Vec<ParameterId>, DimensionResolutionError> {
        dimensions::bound_parameter_ids(self)
    }
}

impl<T: Copy> NonEmptyMembers<T> {
    /// Copy every admitted member with fallible caller working storage.
    pub fn try_clone_working(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut values = Vec::new();
        ctx.reserve_working(&mut values, self.0.len(), "copy admitted nonempty members")?;
        values.extend_from_slice(&self.0);
        Ok(Self(values))
    }
}

cadmpeg_core::named_optional_field!(
    deserialize_product_definition,
    ProductDefinitionId,
    "product_definition"
);
