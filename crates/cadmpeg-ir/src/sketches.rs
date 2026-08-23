// SPDX-License-Identifier: Apache-2.0
//! Neutral planar sketches, solved entities, and geometric constraints.

use crate::features::{Angle, Length, ParameterId};
use crate::math::{Point2, Point3, Vector3};
use crate::transform::Transform;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

macro_rules! string_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                crate::schema::serialize_reference_id(&self.0, serializer)
            }
        }
    };
}

string_id!(SketchId, "Identifies a neutral planar sketch.");
string_id!(SketchEntityId, "Identifies solved geometry in a sketch.");
string_id!(SpatialSketchId, "Identifies a neutral spatial sketch.");
string_id!(
    SpatialSketchEntityId,
    "Identifies solved geometry in a spatial sketch."
);
string_id!(
    SketchConstraintId,
    "Identifies a geometric sketch constraint."
);

/// Horizontal placement of sketch text about its text anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "native_value", rename_all = "snake_case")]
pub enum SketchTextHorizontalAlignment {
    /// Align the text's left edge with its anchor.
    Left,
    /// Center the text about its anchor.
    Center,
    /// Align the text's right edge with its anchor.
    Right,
    /// Source alignment ordinal without an assigned neutral meaning.
    Native(u32),
}

/// Vertical placement of sketch text about its text anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "native_value", rename_all = "snake_case")]
pub enum SketchTextVerticalAlignment {
    /// Align the text's top with its anchor.
    Top,
    /// Center the text about its anchor.
    Middle,
    /// Align the text's bottom with its anchor.
    Bottom,
    /// Source alignment ordinal without an assigned neutral meaning.
    Native(u32),
}

/// Canonical reference axis in neutral sketch coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SketchAxis {
    /// Positive sketch-u direction.
    Horizontal,
    /// Positive sketch-v direction.
    Vertical,
}

/// A planar sketch and its ordered profile loops.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Sketch {
    /// Globally unique sketch id.
    pub id: SketchId,
    /// Source display name, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Source configuration key, when scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    /// Source display visibility, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Placement of sketch coordinates in model space.
    pub placement: SketchPlacement,
    /// Ordered closed or open profile chains.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<Vec<SketchEntityUse>>,
    /// Identifier of the full-fidelity native input lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Placement of a planar sketch's local coordinates in model space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchPlacement {
    /// Local geometry is decoded but its model-space frame is unresolved.
    Unresolved,
    /// Complete model-space sketch frame.
    Resolved {
        /// Sketch-plane origin in model space.
        origin: Point3,
        /// Sketch-plane unit normal.
        normal: Vector3,
        /// Sketch-plane u-axis.
        u_axis: Vector3,
    },
}

impl SketchPlacement {
    /// Return the complete frame when placement is resolved.
    pub fn resolved(self) -> Option<(Point3, Vector3, Vector3)> {
        match self {
            Self::Unresolved => None,
            Self::Resolved {
                origin,
                normal,
                u_axis,
            } => Some((origin, normal, u_axis)),
        }
    }
}

impl Sketch {
    /// Return the complete model-space frame when placement is resolved.
    pub fn resolved_placement(&self) -> Option<(Point3, Vector3, Vector3)> {
        self.placement.resolved()
    }
}

/// Oriented use of one sketch entity in a profile chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchEntityUse {
    /// Referenced sketch entity.
    pub entity: SketchEntityId,
    /// Whether traversal opposes the entity's stored direction.
    #[serde(default)]
    pub reversed: bool,
}

/// Solved geometry belonging to one sketch.
///
/// Prefer [`SketchEntity::new`] for invariant-bearing construction. There is no
/// public [`Default`]: an empty id is illegal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchEntity {
    /// Globally unique entity id.
    pub id: SketchEntityId,
    /// Owning sketch.
    pub sketch: SketchId,
    /// Whether the entity is construction geometry.
    #[serde(default)]
    pub construction: bool,
    /// Source-native geometry record represented by this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
    /// Source-native curve carrier represented by this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry_ref: Option<String>,
    /// Source-native endpoint records in stored entity direction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoint_refs: Vec<String>,
    /// Solved two-dimensional geometry.
    pub geometry: SketchGeometry,
}

impl SketchEntity {
    /// Construct a sketch entity from its id, owning sketch, and geometry.
    pub fn new(id: SketchEntityId, sketch: SketchId, geometry: SketchGeometry) -> Self {
        Self {
            id,
            sketch,
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        }
    }
}

/// Solved two-dimensional sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchGeometry {
    /// Isolated point.
    Point {
        /// Solved point position.
        position: Point2,
    },
    /// Bounded line segment.
    Line {
        /// Segment start.
        start: Point2,
        /// Segment end.
        end: Point2,
    },
    /// Unbounded construction or reference line.
    ReferenceLine {
        /// Point on the line.
        origin: Point2,
        /// Non-zero direction in sketch coordinates.
        direction: Point2,
    },
    /// Full circle.
    Circle {
        /// Circle center.
        center: Point2,
        /// Circle radius.
        radius: Length,
    },
    /// Circular arc with angles in radians.
    Arc {
        /// Arc center.
        center: Point2,
        /// Arc radius.
        radius: Length,
        /// Start angle.
        start_angle: Angle,
        /// End angle.
        end_angle: Angle,
    },
    /// Full or bounded ellipse.
    Ellipse {
        /// Ellipse center.
        center: Point2,
        /// Major-axis angle in sketch coordinates.
        major_angle: Angle,
        /// Semi-major radius.
        major_radius: Length,
        /// Semi-minor radius.
        minor_radius: Length,
        /// Start parameter for a bounded arc; absent for a full ellipse.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start_angle: Option<Angle>,
        /// End parameter for a bounded arc; absent for a full ellipse.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_angle: Option<Angle>,
    },
    /// Full or bounded hyperbola.
    Hyperbola {
        /// Hyperbola center.
        center: Point2,
        /// Major-axis angle in sketch coordinates.
        major_angle: Angle,
        /// Semi-major radius.
        major_radius: Length,
        /// Semi-minor radius.
        minor_radius: Length,
        /// Start parameter for a bounded branch; absent for the full curve.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start_parameter: Option<f64>,
        /// End parameter for a bounded branch; absent for the full curve.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_parameter: Option<f64>,
    },
    /// Full or bounded parabola.
    Parabola {
        /// Parabola vertex.
        vertex: Point2,
        /// Symmetry-axis angle in sketch coordinates.
        axis_angle: Angle,
        /// Distance from the vertex to the focus.
        focal_length: Length,
        /// Start parameter for a bounded branch; absent for the full curve.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start_parameter: Option<f64>,
        /// End parameter for a bounded branch; absent for the full curve.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_parameter: Option<f64>,
    },
    /// NURBS curve in sketch coordinates.
    Nurbs {
        /// Curve degree.
        degree: u32,
        /// Full knot vector.
        knots: Vec<f64>,
        /// Control points in parameter order.
        control_points: Vec<Point2>,
        /// Per-pole weights; absent for non-rational curves.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weights: Option<Vec<f64>>,
        /// Whether the curve is periodic.
        #[serde(default)]
        periodic: bool,
    },
    /// Text placed in sketch coordinates.
    Text {
        /// Unicode text content.
        text: String,
        /// Source font-family name.
        font_family: String,
        /// Numeric font weight from the source text style.
        font_weight: i32,
        /// Nominal character height.
        height: Length,
        /// Horizontal scale relative to the nominal font width, absent when the
        /// source stores none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width_factor: Option<f64>,
        /// Point the text is placed and rotated about, in sketch coordinates,
        /// absent when the source stores no placement.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<Point2>,
        /// Rotation about the anchor, counterclockwise from the sketch u axis,
        /// absent when the source stores no placement.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rotation: Option<Angle>,
        /// Horizontal placement about the text anchor, when the source class
        /// carries an alignment enum.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        horizontal_alignment: Option<SketchTextHorizontalAlignment>,
        /// Vertical placement about the text anchor, when the source class
        /// carries an alignment enum.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        vertical_alignment: Option<SketchTextVerticalAlignment>,
    },
    /// Geometry referenced from another object when no solved sketch-space carrier is stored.
    ExternalReference {
        /// External document identity, absent for a reference within the current document.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        document: Option<String>,
        /// Referenced object identity.
        object: String,
        /// Ordered source subelement selectors.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        subelements: Vec<String>,
    },
    /// Source-native geometry not yet reduced to a neutral family.
    Native {
        /// Source geometry family.
        native_kind: String,
    },
}

/// A sketch whose solved geometry is expressed directly in model space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpatialSketch {
    /// Globally unique spatial-sketch id.
    pub id: SpatialSketchId,
    /// Source display name, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Source configuration key, when scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    /// Source display visibility, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Ordered closed profile loops with profile-local planes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<SpatialSketchProfile>,
    /// Identifier of the full-fidelity native input lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// One closed spatial-sketch profile and its model-space plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpatialSketchProfile {
    /// Profile-plane origin in model space.
    pub origin: Point3,
    /// Profile-plane unit normal, oriented by boundary traversal.
    pub normal: Vector3,
    /// Profile-plane unit u-axis.
    pub u_axis: Vector3,
    /// Ordered oriented boundary uses.
    pub boundary: Vec<SpatialSketchEntityUse>,
}

/// Oriented use of one spatial-sketch entity in a profile boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpatialSketchEntityUse {
    /// Referenced spatial-sketch entity.
    pub entity: SpatialSketchEntityId,
    /// Whether traversal opposes the entity's stored direction.
    #[serde(default)]
    pub reversed: bool,
}

/// Solved model-space geometry belonging to one spatial sketch.
///
/// Prefer [`SpatialSketchEntity::new`] for invariant-bearing construction. There
/// is no public [`Default`]: an empty id is illegal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpatialSketchEntity {
    /// Globally unique spatial entity id.
    pub id: SpatialSketchEntityId,
    /// Owning spatial sketch.
    pub sketch: SpatialSketchId,
    /// Whether the entity is construction geometry.
    #[serde(default)]
    pub construction: bool,
    /// Source-native geometry record represented by this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
    /// Source-native curve carrier represented by this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry_ref: Option<String>,
    /// Source-native endpoint records in stored entity direction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoint_refs: Vec<String>,
    /// Solved model-space geometry.
    pub geometry: SpatialSketchGeometry,
}

impl SpatialSketchEntity {
    /// Construct a spatial sketch entity from its id, owning sketch, and geometry.
    pub fn new(
        id: SpatialSketchEntityId,
        sketch: SpatialSketchId,
        geometry: SpatialSketchGeometry,
    ) -> Self {
        Self {
            id,
            sketch,
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        }
    }
}

/// One geometric relation owned by a spatial sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpatialSketchConstraint {
    /// Globally unique constraint id.
    pub id: SketchConstraintId,
    /// Owning spatial sketch.
    pub sketch: SpatialSketchId,
    /// Neutral relation semantics.
    pub definition: SpatialSketchConstraintDefinition,
    /// Source-native relation represented by this constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// One unordered entity pair in a repeated model-space sketch relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpatialSketchEntityPair {
    /// First member in source discovery order.
    pub first: SpatialSketchEntityId,
    /// Second member in source discovery order.
    pub second: SpatialSketchEntityId,
}

/// Neutral geometric relations between model-space sketch entities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpatialSketchConstraintDefinition {
    /// Source-native spatial relation without complete neutral semantics.
    Native {
        /// Source relation family.
        native_kind: String,
        /// Source relation state or subtype discriminator, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_state: Option<u64>,
        /// Neutral parameter driving the relation, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter: Option<crate::features::ParameterId>,
        /// Full-fidelity source operands in field order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        operands: Vec<SketchNativeOperand>,
    },
    /// Two model-space sketch points occupy the same solved position.
    Coincident {
        /// First coincident point.
        first: SpatialSketchEntityId,
        /// Second coincident point.
        second: SpatialSketchEntityId,
    },
    /// Two model-space sketch points are mirror images across a model-space line.
    Symmetric {
        /// First symmetric point.
        first: SpatialSketchEntityId,
        /// Second symmetric point.
        second: SpatialSketchEntityId,
        /// Bounded line whose infinite carrier is the reflection axis.
        axis: SpatialSketchEntityId,
    },
    /// A model-space point lies on a model-space surface.
    PointOnSurface {
        /// Point constrained to the surface.
        point: SpatialSketchEntityId,
        /// Surface containing the point.
        surface: SpatialSketchEntityId,
    },
    /// A model-space point lies at the midpoint of a bounded line.
    Midpoint {
        /// Point constrained to the midpoint.
        point: SpatialSketchEntityId,
        /// Bounded line whose midpoint is used.
        entity: SpatialSketchEntityId,
    },
    /// Two model-space curves are tangent.
    Tangent {
        /// First tangent curve.
        first: SpatialSketchEntityId,
        /// Second tangent curve.
        second: SpatialSketchEntityId,
    },
    /// Euclidean distance between two model-space sketch points.
    PointDistance {
        /// First measured point.
        first: SpatialSketchEntityId,
        /// Second measured point.
        second: SpatialSketchEntityId,
        /// Driving distance parameter.
        parameter: crate::features::ParameterId,
    },
    /// Distance from a model-space point to an infinite model-space line.
    PointLineDistance {
        /// Measured point.
        point: SpatialSketchEntityId,
        /// Bounded entity supplying the infinite line carrier.
        line: SpatialSketchEntityId,
        /// Driving distance parameter.
        parameter: crate::features::ParameterId,
    },
    /// Endpoint-to-endpoint length of one bounded model-space sketch line.
    LineLength {
        /// Measured line.
        entity: SpatialSketchEntityId,
        /// Driving length parameter.
        parameter: crate::features::ParameterId,
    },
    /// Endpoint-to-endpoint lengths of multiple bounded model-space sketch lines.
    RepeatedLineLength {
        /// Distinct measured lines in spatial-sketch entity order.
        entities: Vec<SpatialSketchEntityId>,
        /// Shared driving length parameter.
        parameter: crate::features::ParameterId,
    },
    /// Minimum separation between two parallel model-space sketch lines.
    ParallelLineDistance {
        /// First measured line.
        first: SpatialSketchEntityId,
        /// Second measured line.
        second: SpatialSketchEntityId,
        /// Driving distance parameter.
        parameter: crate::features::ParameterId,
    },
    /// Repeated separation between disjoint pairs of parallel lines.
    RepeatedParallelLineDistance {
        /// Distinct line pairs in profile traversal order.
        pairs: Vec<SpatialSketchEntityPair>,
        /// Shared driving distance parameter.
        parameter: crate::features::ParameterId,
    },
    /// Minimum separation between two parallel collinear model-space line sets.
    ParallelLineSetDistance {
        /// Collinear entities forming the first line carrier.
        first: Vec<SpatialSketchEntityId>,
        /// Collinear entities forming the second line carrier.
        second: Vec<SpatialSketchEntityId>,
        /// Driving distance parameter.
        parameter: crate::features::ParameterId,
    },
    /// A model-space curve set generated at one offset distance from a source set.
    Offset {
        /// Source curves in native relation order.
        sources: Vec<SpatialSketchEntityId>,
        /// Generated curves in native relation order.
        ///
        /// Position does not imply pairwise geometric correspondence with
        /// `sources`; an offset operation can change curve carriers or topology.
        results: Vec<SpatialSketchEntityId>,
        /// Unit normal of the result curve set's common plane.
        normal: Vector3,
        /// Strictly positive operation-level offset magnitude.
        distance: crate::features::Length,
        /// Driving offset-distance parameter, when dimensional.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter: Option<crate::features::ParameterId>,
        /// Multiplier from the driving parameter value to `distance`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter_factor: Option<f64>,
    },
    /// A model-space line is parallel to one fixed model-space direction.
    ParallelToDirection {
        /// Line constrained to the direction.
        entity: SpatialSketchEntityId,
        /// Unit model-space direction; either sign denotes the same axis.
        direction: Vector3,
    },
    /// A spline's defining model-space entities grouped by one native relation.
    SplineGroup {
        /// Ordered spline-group members.
        entities: Vec<SpatialSketchEntityId>,
    },
}

/// Solved model-space spatial-sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpatialSketchGeometry {
    /// Model-space point.
    Point {
        /// Point position in model coordinates.
        position: Point3,
    },
    /// Bounded model-space line segment.
    Line {
        /// Segment start in model coordinates.
        start: Point3,
        /// Segment end in model coordinates.
        end: Point3,
    },
    /// Oriented full model-space circle.
    Circle {
        /// Circle center in model coordinates.
        center: Point3,
        /// Unit normal defining positive angular travel.
        normal: Vector3,
        /// Unit radial direction at parameter zero.
        reference_direction: Vector3,
        /// Circle radius.
        radius: Length,
    },
    /// Oriented bounded model-space circular arc.
    Arc {
        /// Arc center in model coordinates.
        center: Point3,
        /// Unit normal defining positive angular travel.
        normal: Vector3,
        /// Unit radial direction at parameter zero.
        reference_direction: Vector3,
        /// Arc radius.
        radius: Length,
        /// Inclusive start parameter in radians.
        start_angle: Angle,
        /// Inclusive end parameter in radians.
        end_angle: Angle,
    },
    /// Model-space NURBS curve.
    Nurbs {
        /// Curve degree.
        degree: u32,
        /// Full knot vector.
        knots: Vec<f64>,
        /// Control points in parameter order.
        control_points: Vec<Point3>,
        /// Per-pole weights; absent for non-rational curves.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weights: Option<Vec<f64>>,
        /// Whether the curve is periodic.
        #[serde(default)]
        periodic: bool,
    },
    /// Tensor-product NURBS surface embedded in model space.
    NurbsSurface {
        /// Degree in the first parameter.
        u_degree: u32,
        /// Degree in the second parameter.
        v_degree: u32,
        /// Full knot vector in the first parameter.
        u_knots: Vec<f64>,
        /// Full knot vector in the second parameter.
        v_knots: Vec<f64>,
        /// Rectangular control grid in first-parameter-major order.
        control_points: Vec<Vec<Point3>>,
    },
    /// Source-native spatial geometry not yet reduced to a neutral family.
    Native {
        /// Source geometry family.
        native_kind: String,
    },
}

/// One relation constraining solved sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchConstraint {
    /// Globally unique constraint id.
    pub id: SketchConstraintId,
    /// Owning sketch.
    pub sketch: SketchId,
    /// Constraint semantics.
    pub definition: SketchConstraintDefinition,
    /// User-visible constraint name, when assigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether this dimensional relation drives geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driving: Option<bool>,
    /// Whether the solver currently applies this relation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    /// Whether the relation belongs to virtual sketch space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_space: Option<bool>,
    /// Whether the relation is displayed in the sketch UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Source orientation bit field, when the relation carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<u32>,
    /// Persisted label offset from the constrained geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_distance: Option<f64>,
    /// Persisted position along the dimension label path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_position: Option<f64>,
    /// Application metadata text attached to this relation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    /// Source-native relation record when decoded from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// A geometric locus on a sketch entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "entity", rename_all = "snake_case")]
pub enum SketchLocus {
    /// The complete entity.
    Entity(SketchEntityId),
    /// Stored start point of a bounded entity.
    Start(SketchEntityId),
    /// Stored end point of a bounded entity.
    End(SketchEntityId),
    /// Center of a circle, arc, or ellipse.
    Center(SketchEntityId),
}

/// Coordinate axis selected by a sketch relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SketchCoordinateAxis {
    /// First coordinate in sketch space.
    U,
    /// Second coordinate in sketch space.
    V,
}

/// One ordered operand retained from a native sketch relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchNativeOperand {
    /// Source-native operand family.
    pub native_kind: String,
    /// Source-native field containing this operand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_field: Option<String>,
    /// Source-native role code, when the field carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_role: Option<u32>,
    /// Source-native object index.
    pub object_index: u32,
    /// Resolved source-native operand record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// One progenitor/result pair in a sketch offset relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchOffsetPair {
    /// Source entity whose stored direction defines the signed offset normal.
    pub source: SketchEntityId,
    /// Entity produced at the shared signed offset distance.
    pub result: SketchEntityId,
    /// Reverse the source's stored traversal before selecting its left normal.
    #[serde(default)]
    pub source_reversed: bool,
}

/// One axis of a rectangular sketch pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchPatternDirection {
    /// Unit direction in sketch coordinates.
    pub direction: [f64; 2],
    /// Adjacent-instance spacing along `direction`.
    pub spacing: Length,
    /// Number of instances along this axis, including the seed instance.
    pub count: u32,
    /// Driving adjacent-spacing parameter, when the source exposes it as a neutral parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing_parameter: Option<ParameterId>,
    /// Driving seed-to-final-span parameter, when the source exposes span
    /// rather than adjacent spacing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_parameter: Option<ParameterId>,
    /// Driving instance-count parameter, when the source exposes it as a neutral parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count_parameter: Option<ParameterId>,
}

/// One resolved rectangular-pattern instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchPatternInstance {
    /// Zero-based indices along the two pattern directions.
    pub indices: [u32; 2],
    /// Entities in fixed seed-entity order.
    pub entities: Vec<SketchEntityId>,
}

/// One resolved circular-pattern instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchCircularPatternInstance {
    /// Zero-based position in pattern order; zero is the seed instance.
    pub index: u32,
    /// Signed rotation from the seed instance in radians.
    pub angle: Angle,
    /// Entities in fixed seed-entity order.
    pub entities: Vec<SketchEntityId>,
}

/// One independently measured pair within a repeated linear dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchDistanceMeasurement {
    /// Euclidean separation between two loci.
    Distance {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
    },
    /// Horizontal separation between two loci.
    Horizontal {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
    },
    /// Vertical separation between two loci.
    Vertical {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
    },
}

/// Meaning of an internal sketch alignment helper relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SketchInternalAlignment {
    /// Major diameter helper for an ellipse.
    EllipseMajorDiameter,
    /// Minor diameter helper for an ellipse.
    EllipseMinorDiameter,
    /// First ellipse focus helper.
    EllipseFocus1,
    /// Second ellipse focus helper.
    EllipseFocus2,
    /// Hyperbola major-axis helper.
    HyperbolaMajor,
    /// Hyperbola minor-axis helper.
    HyperbolaMinor,
    /// Hyperbola focus helper.
    HyperbolaFocus,
    /// Parabola focus helper.
    ParabolaFocus,
    /// B-spline control-point helper.
    BsplineControlPoint,
    /// B-spline knot-point helper.
    BsplineKnotPoint,
    /// Parabola focal-axis helper.
    ParabolaFocalAxis,
}

/// Neutral geometric and dimensional sketch relations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchConstraintDefinition {
    /// Persisted no-op relation slot.
    Disabled,
    /// Two entity loci coincide.
    Coincident {
        /// Coincident entity loci.
        entities: Vec<SketchEntityId>,
    },
    /// Entities participate in one native polygon relation.
    Polygon {
        /// Ordered polygon members.
        entities: Vec<SketchEntityId>,
    },
    /// A spline's defining entities grouped by one native spline relation.
    SplineGroup {
        /// Ordered spline-group members: the spline's defining entities and
        /// its curve entity.
        entities: Vec<SketchEntityId>,
    },
    /// A complete two-axis rectangular pattern with resolved instances.
    RectangularPattern {
        /// Ordered pattern directions.
        directions: [SketchPatternDirection; 2],
        /// Instances in source order; `[0, 0]` is the seed instance.
        instances: Vec<SketchPatternInstance>,
    },
    /// A parameter-driven circular pattern with geometrically resolved instances.
    CircularPattern {
        /// Point entity defining the center of rotation.
        center: SketchEntityId,
        /// Evaluated angular span stored by the native pattern.
        angle: Angle,
        /// Number of instances, including the seed instance.
        count: u32,
        /// Driving angular-span parameter, when the source exposes it as a neutral parameter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle_parameter: Option<ParameterId>,
        /// Driving instance-count parameter, when the source exposes it as a neutral parameter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count_parameter: Option<ParameterId>,
        /// Instances in source order; index zero is the seed instance.
        instances: Vec<SketchCircularPatternInstance>,
    },
    /// Text entity bounded by ordered frame curves.
    TextFrame {
        /// Text entity owning the frame.
        text: SketchEntityId,
        /// Ordered frame curves.
        frame: Vec<SketchEntityId>,
    },
    /// Text entity laid out along a path curve.
    TextPath {
        /// Text entity placed along the path.
        text: SketchEntityId,
        /// Path curve.
        path: SketchEntityId,
        /// Character placements in text order, expressed in sketch coordinates.
        glyph_transforms: Vec<Transform>,
    },
    /// Two or more explicit entity loci coincide.
    CoincidentLoci {
        /// Coincident endpoints, centers, or complete entities.
        loci: Vec<SketchLocus>,
    },
    /// Two loci share one sketch-space coordinate.
    SameCoordinate {
        /// First aligned locus.
        first: SketchLocus,
        /// Second aligned locus.
        second: SketchLocus,
        /// Shared sketch coordinate.
        axis: SketchCoordinateAxis,
    },
    /// A point locus lies on another sketch entity.
    PointOnObject {
        /// Point constrained to the supporting entity.
        point: SketchLocus,
        /// Entity on which the point lies.
        entity: SketchEntityId,
    },
    /// A point locus lies at the midpoint of a bounded entity.
    Midpoint {
        /// Point constrained to the midpoint.
        point: SketchLocus,
        /// Bounded entity whose midpoint is used.
        entity: SketchEntityId,
    },
    /// One or more entities offset from their progenitors by one signed distance.
    Offset {
        /// Ordered progenitor/result pairs.
        pairs: Vec<SketchOffsetPair>,
        /// Strictly positive common offset magnitude, measured along each
        /// oriented source entity's left normal.
        distance: Length,
        /// Driving offset-distance parameter, when the source relation is dimensional.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter: Option<ParameterId>,
        /// Multiplier from the driving parameter value to `distance`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter_factor: Option<f64>,
    },
    /// A regular profile entity copied from a projected reference entity.
    ProjectedCopy {
        /// Projected reference entity that supplies the geometry.
        source: SketchEntityId,
        /// Regular entity used by the profile.
        result: SketchEntityId,
    },
    /// A point locus lies at the intersection of two entities.
    AtIntersection {
        /// Point constrained to the intersection.
        point: SketchLocus,
        /// First intersecting entity.
        first: SketchEntityId,
        /// Second intersecting entity.
        second: SketchEntityId,
    },
    /// Circular or elliptical entities share a center.
    Concentric {
        /// First centered entity.
        first: SketchEntityId,
        /// Second centered entity.
        second: SketchEntityId,
    },
    /// Two circular entities share a center and radius.
    Coradial {
        /// First circular entity.
        first: SketchEntityId,
        /// Second circular entity.
        second: SketchEntityId,
    },
    /// Two line entities lie on one infinite line.
    Collinear {
        /// First line.
        first: SketchEntityId,
        /// Second line.
        second: SketchEntityId,
    },
    /// Two loci are symmetric about a line entity.
    Symmetric {
        /// First symmetric locus.
        first: SketchLocus,
        /// Second symmetric locus.
        second: SketchLocus,
        /// Symmetry axis.
        axis: SketchEntityId,
    },
    /// Two loci are centrally symmetric about a point.
    PointSymmetric {
        /// First symmetric locus.
        first: SketchLocus,
        /// Second symmetric locus.
        second: SketchLocus,
        /// Center of symmetry.
        center: SketchLocus,
    },
    /// Line is horizontal in sketch coordinates.
    Horizontal {
        /// Constrained entity.
        entity: SketchEntityId,
    },
    /// Two loci have equal vertical sketch coordinate.
    HorizontalLoci {
        /// First constrained locus.
        first: SketchLocus,
        /// Second constrained locus.
        second: SketchLocus,
    },
    /// Line is vertical in sketch coordinates.
    Vertical {
        /// Constrained entity.
        entity: SketchEntityId,
    },
    /// Two loci have equal horizontal sketch coordinate.
    VerticalLoci {
        /// First constrained locus.
        first: SketchLocus,
        /// Second constrained locus.
        second: SketchLocus,
    },
    /// Two explicit loci have equal horizontal sketch coordinates.
    HorizontalPoints {
        /// First aligned locus.
        first: SketchLocus,
        /// Second aligned locus.
        second: SketchLocus,
    },
    /// Two explicit loci have equal vertical sketch coordinates.
    VerticalPoints {
        /// First aligned locus.
        first: SketchLocus,
        /// Second aligned locus.
        second: SketchLocus,
    },
    /// Two entities are parallel.
    Parallel {
        /// First entity.
        first: SketchEntityId,
        /// Second entity.
        second: SketchEntityId,
    },
    /// Two entities are perpendicular.
    Perpendicular {
        /// First entity.
        first: SketchEntityId,
        /// Second entity.
        second: SketchEntityId,
    },
    /// Two entities are tangent.
    Tangent {
        /// First entity.
        first: SketchEntityId,
        /// Second entity.
        second: SketchEntityId,
    },
    /// Two bounded entities are tangent at explicit loci.
    TangentLoci {
        /// Tangency locus on the first entity.
        first: SketchLocus,
        /// Tangency locus on the second entity.
        second: SketchLocus,
    },
    /// Two entities have equal tangent direction and curvature at contact.
    Curvature {
        /// First entity.
        first: SketchEntityId,
        /// Second entity.
        second: SketchEntityId,
    },
    /// Two entities have equal size.
    Equal {
        /// First entity.
        first: SketchEntityId,
        /// Second entity.
        second: SketchEntityId,
    },
    /// Entity is fixed in sketch coordinates.
    Fixed {
        /// Fixed entity.
        entity: SketchEntityId,
    },
    /// Circular arc angle fixed by the relation kind.
    ArcAngle {
        /// Constrained circular arc.
        entity: SketchEntityId,
        /// Fixed positive arc angle in radians.
        angle: Angle,
    },
    /// Bounded ellipse parameter sweep fixed by the relation kind.
    EllipseAngle {
        /// Constrained bounded ellipse.
        entity: SketchEntityId,
        /// Fixed positive parameter sweep in radians.
        angle: Angle,
    },
    /// Distance controlled by a design parameter.
    Distance {
        /// Measured entities.
        entities: Vec<SketchEntityId>,
        /// Driving distance parameter.
        parameter: ParameterId,
    },
    /// Euclidean distance between two explicit loci.
    DistanceLoci {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
        /// Driving distance parameter.
        parameter: ParameterId,
    },
    /// Horizontal separation between two explicit loci.
    HorizontalDistance {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
        /// Driving horizontal-distance parameter.
        parameter: ParameterId,
    },
    /// Vertical separation between two explicit loci.
    VerticalDistance {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
        /// Driving vertical-distance parameter.
        parameter: ParameterId,
    },
    /// Multiple disjoint locus pairs controlled by one linear parameter.
    RepeatedDistance {
        /// Ordered independent measurements.
        measurements: Vec<SketchDistanceMeasurement>,
        /// Shared driving distance parameter.
        parameter: ParameterId,
    },
    /// Equal-length line entities controlled by one linear parameter.
    RepeatedLength {
        /// Distinct line entities sharing the driven length.
        entities: Vec<SketchEntityId>,
        /// Shared driving length parameter.
        parameter: ParameterId,
    },
    /// Distance between two parallel collinear line-entity sets.
    ParallelLineSetDistance {
        /// Collinear entities forming the first line carrier.
        first: Vec<SketchEntityId>,
        /// Collinear entities forming the second line carrier.
        second: Vec<SketchEntityId>,
        /// Driving distance parameter.
        parameter: ParameterId,
    },
    /// Angle controlled by a design parameter.
    Angle {
        /// First angular entity.
        first: SketchEntityId,
        /// Second angular entity.
        second: SketchEntityId,
        /// Driving angle parameter.
        parameter: ParameterId,
    },
    /// Angle from a canonical sketch axis to one line entity.
    AngleToAxis {
        /// Measured line entity.
        entity: SketchEntityId,
        /// Canonical sketch reference axis.
        axis: SketchAxis,
        /// Driving angle parameter.
        parameter: ParameterId,
    },
    /// Radius controlled by a design parameter.
    Radius {
        /// Circular or elliptical entity.
        entity: SketchEntityId,
        /// Driving radius parameter.
        parameter: ParameterId,
    },
    /// Equal-radius circular entities controlled by one design parameter.
    RepeatedRadius {
        /// Distinct circular entities sharing the driven radius.
        entities: Vec<SketchEntityId>,
        /// Shared driving radius parameter.
        parameter: ParameterId,
    },
    /// Diameter controlled by a design parameter.
    Diameter {
        /// Circular entity.
        entity: SketchEntityId,
        /// Driving diameter parameter.
        parameter: ParameterId,
    },
    /// Equal-diameter circular entities controlled by one design parameter.
    RepeatedDiameter {
        /// Distinct circular entities sharing the driven diameter.
        entities: Vec<SketchEntityId>,
        /// Shared driving diameter parameter.
        parameter: ParameterId,
    },
    /// Refraction relation between two curve loci and their interface.
    SnellsLaw {
        /// Incident curve locus.
        incident: SketchLocus,
        /// Refracted curve locus.
        refracted: SketchLocus,
        /// Interface entity carrying the surface normal in sketch space.
        interface: SketchEntityId,
        /// Dimensionless refractive-index ratio.
        parameter: ParameterId,
    },
    /// Rational spline weight controlled by a dimensionless parameter.
    Weight {
        /// Weighted spline entity.
        entity: SketchEntityId,
        /// Dimensionless weight parameter.
        parameter: ParameterId,
    },
    /// Relation between generated helper geometry and its parent conic or spline.
    InternalAlignment {
        /// Generated helper geometry.
        helper: SketchEntityId,
        /// Parent geometry receiving the alignment.
        parent: SketchEntityId,
        /// Exact helper relation family.
        alignment: SketchInternalAlignment,
        /// Control-point or knot index when carried by the family.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
    },
    /// Ordered geometry grouped under a sketch construction handle.
    Group {
        /// Group handle followed by its ordered member loci.
        elements: Vec<SketchLocus>,
    },
    /// Text constructed from an ordered set of sketch geometry.
    Text {
        /// Text handle followed by its ordered construction loci.
        elements: Vec<SketchLocus>,
        /// Displayed text.
        text: String,
        /// Font family or source font token, when carried.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font: Option<String>,
        /// Whether the construction dimension controls text height rather than width.
        is_text_height: bool,
    },
    /// Source-native relation not yet reduced to a neutral family.
    Native {
        /// Source constraint family.
        native_kind: String,
        /// Source-native constraint-state mask, when the format carries one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_state: Option<u64>,
        /// Source-native constraint flags, when distinct from constraint state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_flags: Option<u64>,
        /// Exact source-native scalar properties not represented by common state or flags.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        native_properties: BTreeMap<String, String>,
        /// Referenced entities.
        entities: Vec<SketchEntityId>,
        /// Driving or driven parameter attached to the relation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter: Option<ParameterId>,
        /// Native operands whose neutral loci are unresolved.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        operands: Vec<SketchNativeOperand>,
    },
}

#[cfg(test)]
mod tests;
