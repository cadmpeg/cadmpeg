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
}

/// Neutral geometric and dimensional sketch relations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchConstraintDefinition {
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
    /// A point locus lies at the midpoint of a bounded entity.
    Midpoint {
        /// Point constrained to the midpoint.
        point: SketchLocus,
        /// Bounded entity whose midpoint is used.
        entity: SketchEntityId,
    },
    /// Circular or elliptical entities share a center.
    Concentric {
        /// First centered entity.
        first: SketchEntityId,
        /// Second centered entity.
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
    /// Source-native relation not yet reduced to a neutral family.
    Native {
        /// Source constraint family.
        native_kind: String,
        /// Referenced entities.
        entities: Vec<SketchEntityId>,
        /// Driving or driven parameter attached to the relation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter: Option<ParameterId>,
        /// Solved distance reported by the source relation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        measured_distance: Option<Length>,
        /// Native operands whose neutral loci are unresolved.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        operands: Vec<SketchNativeOperand>,
    },
}
