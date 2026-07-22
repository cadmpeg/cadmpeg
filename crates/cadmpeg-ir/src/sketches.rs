// SPDX-License-Identifier: Apache-2.0
//! Neutral planar sketches, solved entities, and geometric constraints.

use crate::features::{Angle, Length, ParameterId};
use crate::math::{Point2, Point3, Vector3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(pub String);
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

/// A planar sketch and its ordered profile loops.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sketch {
    /// Globally unique sketch id.
    pub id: SketchId,
    /// Source display name, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Source configuration key, when scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    /// Sketch-plane origin in model space.
    pub origin: Point3,
    /// Sketch-plane unit normal.
    pub normal: Vector3,
    /// Sketch-plane u-axis.
    pub u_axis: Vector3,
    /// Ordered closed or open profile chains.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<Vec<SketchEntityUse>>,
    /// Identifier of the full-fidelity native input lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Oriented use of one sketch entity in a profile chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchEntityUse {
    /// Referenced sketch entity.
    pub entity: SketchEntityId,
    /// Whether traversal opposes the entity's stored direction.
    #[serde(default)]
    pub reversed: bool,
}

/// Solved geometry belonging to one sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

/// A spatial sketch and its ordered 3D entities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpatialSketch {
    /// Globally unique spatial-sketch id.
    pub id: SpatialSketchId,
    /// Source display name, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Source configuration key, when scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    /// Ordered solved entities owned by this sketch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<SpatialSketchEntityId>,
    /// Identifier of the full-fidelity native input lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Solved geometry belonging to one spatial sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpatialSketchEntity {
    /// Globally unique entity id.
    pub id: SpatialSketchEntityId,
    /// Owning spatial sketch.
    pub sketch: SpatialSketchId,
    /// Whether the entity is construction geometry.
    #[serde(default)]
    pub construction: bool,
    /// Source-native geometry record represented by this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
    /// Solved three-dimensional geometry.
    pub geometry: SpatialSketchGeometry,
}

/// Solved three-dimensional spatial-sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpatialSketchGeometry {
    /// Isolated point.
    Point {
        /// Solved model-space position.
        position: Point3,
    },
    /// Bounded line segment.
    Line {
        /// Stored segment start.
        start: Point3,
        /// Stored segment end.
        end: Point3,
    },
    /// Full circle in an oriented plane.
    Circle {
        /// Circle center in model coordinates.
        center: Point3,
        /// Unit plane normal.
        normal: Vector3,
        /// Positive circle radius.
        radius: Length,
    },
    /// Circular arc in an oriented plane.
    Arc {
        /// Arc center in model coordinates.
        center: Point3,
        /// Unit plane normal.
        normal: Vector3,
        /// Unit radial axis for zero angle.
        u_axis: Vector3,
        /// Positive arc radius.
        radius: Length,
        /// Start angle about `normal` from `u_axis`.
        start_angle: Angle,
        /// End angle about `normal` from `u_axis`.
        end_angle: Angle,
    },
    /// NURBS curve in model coordinates.
    Nurbs {
        /// Polynomial degree.
        degree: u32,
        /// Full nondecreasing knot vector.
        knots: Vec<f64>,
        /// Model-space control points in parameter order.
        control_points: Vec<Point3>,
        /// Positive per-pole weights for a rational curve.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weights: Option<Vec<f64>>,
        /// Whether the curve is periodic.
        #[serde(default)]
        periodic: bool,
    },
    /// Source-native geometry not yet reduced to a neutral family.
    Native {
        /// Source geometry family.
        native_kind: String,
    },
}

/// Solved two-dimensional sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    /// Source-native geometry not yet reduced to a neutral family.
    Native {
        /// Source geometry family.
        native_kind: String,
    },
}

/// One relation constraining solved sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// One unresolved operand retained from a native sketch relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SketchNativeOperand {
    /// Source-native operand family.
    pub native_kind: String,
    /// Source-native object index.
    pub object_index: u32,
    /// Resolved source-native operand record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Meaning of an internal sketch alignment helper relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchConstraintDefinition {
    /// Persisted no-op relation slot.
    Disabled,
    /// Two entity loci coincide.
    Coincident {
        /// Coincident entity loci.
        entities: Vec<SketchEntityId>,
    },
    /// Two or more explicit entity loci coincide.
    CoincidentLoci {
        /// Coincident endpoints, centers, or complete entities.
        loci: Vec<SketchLocus>,
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
    /// Line is horizontal in sketch coordinates.
    Horizontal {
        /// Constrained entity.
        entity: SketchEntityId,
    },
    /// Line is vertical in sketch coordinates.
    Vertical {
        /// Constrained entity.
        entity: SketchEntityId,
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
    /// Angle controlled by a design parameter.
    Angle {
        /// First angular entity.
        first: SketchEntityId,
        /// Second angular entity.
        second: SketchEntityId,
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
    /// Diameter controlled by a design parameter.
    Diameter {
        /// Circular entity.
        entity: SketchEntityId,
        /// Driving diameter parameter.
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
