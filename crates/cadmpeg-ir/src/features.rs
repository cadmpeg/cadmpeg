// SPDX-License-Identifier: Apache-2.0
//! Neutral construction-feature taxonomy.

use std::collections::BTreeMap;

use crate::ids::{BodyId, CurveId, EdgeId, FaceId};
use crate::math::{Point3, Vector3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Identifies a neutral construction feature.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct FeatureId(pub String);

impl FeatureId {
    /// Borrow the underlying id string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FeatureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<S: Into<String>> From<S> for FeatureId {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

/// Identifies a neutral design configuration.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct ConfigurationId(pub String);

/// A named parametric model variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignConfiguration {
    /// Globally unique configuration id.
    pub id: ConfigurationId,
    /// Source display name.
    pub name: String,
    /// Material override, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Configuration-local named values not otherwise represented.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    /// Bodies present when this configuration is active.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bodies: Vec<BodyId>,
    /// Identifier of the full-fidelity record in a native namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Identifies a neutral design parameter.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct ParameterId(pub String);

/// A named expression owned by a construction feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignParameter {
    /// Globally unique parameter id.
    pub id: ParameterId,
    /// Feature that consumes this parameter.
    pub owner: FeatureId,
    /// Source parameter name.
    pub name: String,
    /// Literal or expression text used by the source system.
    pub expression: String,
    /// Evaluated scalar when the expression is an unambiguous literal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ParameterValue>,
}

/// Canonical scalar value of a literal design parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ParameterValue {
    /// Length in canonical millimeters.
    Length(Length),
    /// Angle in canonical radians.
    Angle(Angle),
    /// Dimensionless real scalar.
    Real(f64),
    /// Integer scalar.
    Integer(i64),
    /// Boolean scalar.
    Boolean(bool),
}

/// A length in canonical millimeters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Length(pub f64);

/// An angle in canonical radians.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Angle(pub f64);

/// An ordered neutral construction feature and its resulting bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Feature {
    /// Globally unique feature id.
    pub id: FeatureId,
    /// Stable construction order within the source history.
    pub ordinal: u64,
    /// Source display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether evaluation of this feature is disabled.
    #[serde(default)]
    pub suppressed: bool,
    /// Containing or logically preceding feature, when represented by the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<FeatureId>,
    /// Bodies produced or modified by the feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<BodyId>,
    /// Neutral construction semantics.
    pub definition: FeatureDefinition,
    /// Identifier of the full-fidelity record in a native namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Neutral construction semantics, with an explicit native escape hatch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "definition", rename_all = "snake_case")]
pub enum FeatureDefinition {
    /// Constructed reference plane.
    DatumPlane {
        /// Plane origin in model space.
        origin: Point3,
        /// Plane normal.
        normal: Vector3,
        /// In-plane u-axis.
        u_axis: Vector3,
    },
    /// Solved sketch node in the construction history.
    Sketch {
        /// Neutral sketch geometry owned by this history node, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<crate::sketches::SketchId>,
    },
    /// Linear extrusion of a profile.
    Extrude {
        /// Profile swept along `direction` (or the profile's own normal, when
        /// `direction` is `None`).
        profile: ProfileRef,
        /// Extrusion direction, when the source recorded one explicit of the
        /// profile plane; `None` to extrude along the profile's own normal.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
        /// How far the extrusion travels.
        extent: Extent,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
        /// Draft angle applied to the extruded side walls, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        draft: Option<Angle>,
    },
    /// Revolution of a profile around an axis.
    Revolve {
        /// Profile revolved about the axis.
        profile: ProfileRef,
        /// A point on the revolution axis.
        axis_origin: Point3,
        /// Unit direction of the revolution axis.
        axis_dir: Vector3,
        /// Angular extent of the revolution.
        angle: Extent,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Sweep of a profile along a path.
    Sweep {
        /// Cross-section swept along the path.
        profile: ProfileRef,
        /// Trajectory followed by the profile.
        path: PathRef,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
        /// Total profile twist along the path, when specified.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        twist: Option<Angle>,
        /// End-to-start profile scale ratio, when specified.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<f64>,
    },
    /// Loft through an ordered sequence of section profiles.
    Loft {
        /// Ordered section profiles.
        profiles: Vec<ProfileRef>,
        /// Optional ordered guide trajectories.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        guides: Vec<PathRef>,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
        /// Whether the loft closes from the last section to the first.
        #[serde(default)]
        closed: bool,
    },
    /// Thin rib grown from a profile.
    Rib {
        /// Rib centerline or open profile.
        profile: ProfileRef,
        /// Rib growth direction.
        direction: Vector3,
        /// Finished rib thickness.
        thickness: Length,
        /// Whether thickness is split equally around the profile.
        #[serde(default)]
        both_sides: bool,
        /// Draft angle applied to rib walls, when specified.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        draft: Option<Angle>,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Edge fillet.
    Fillet {
        /// Edges the fillet is applied to.
        edges: EdgeSelection,
        /// Fillet radius assignment along the edges.
        radius: RadiusSpec,
    },
    /// Edge chamfer.
    Chamfer {
        /// Edges the chamfer is applied to.
        edges: EdgeSelection,
        /// Dimensional definition of the chamfer.
        spec: ChamferSpec,
    },
    /// Thin-wall shell operation.
    Shell {
        /// Faces removed to open the shell.
        removed_faces: FaceSelection,
        /// Wall thickness left after shelling.
        thickness: Length,
        /// Whether the wall is grown outward from the original boundary,
        /// as opposed to inward.
        outward: bool,
    },
    /// Taper applied to selected faces about a neutral plane.
    Draft {
        /// Faces whose angle is modified.
        faces: FaceSelection,
        /// Neutral plane that remains fixed during the operation.
        neutral_plane: FaceSelection,
        /// Pull direction used to measure the draft angle.
        pull_direction: Vector3,
        /// Signed draft angle.
        angle: Angle,
        /// Whether material is added away from the pull direction.
        outward: bool,
    },
    /// Boolean operation between existing bodies.
    Combine {
        /// Body modified by the operation.
        target: BodySelection,
        /// Bodies consumed as Boolean tools.
        tools: BodySelection,
        /// Join, cut, or intersection operation.
        op: BooleanOp,
    },
    /// Removal of selected faces from an existing body.
    DeleteFace {
        /// Faces removed by the operation.
        faces: FaceSelection,
        /// Whether adjacent faces extend to heal the resulting boundary.
        heal: bool,
    },
    /// Direct motion of selected faces.
    MoveFace {
        /// Faces modified by the operation.
        faces: FaceSelection,
        /// Motion applied to the selected faces.
        motion: FaceMotion,
    },
    /// Dome grown from selected planar faces.
    Dome {
        /// Faces that bound the dome base.
        faces: FaceSelection,
        /// Dome height measured normal to the base.
        height: Length,
        /// Whether the profile is elliptical rather than spherical.
        elliptical: bool,
        /// Whether growth opposes the selected-face normal.
        reverse: bool,
    },
    /// Drilled or machined hole.
    Hole {
        /// Face the hole is placed on, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<FaceSelection>,
        /// Hole placement position, when recorded independently of `face`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<Point3>,
        /// Entry-shape family of the hole.
        kind: HoleKind,
        /// Hole diameter.
        diameter: Length,
        /// How deep the hole extends.
        extent: Extent,
    },
    /// Repetition or reflection of existing features.
    Pattern {
        /// Features being repeated or reflected.
        seeds: Vec<FeatureId>,
        /// Spatial transform defining the repetition or reflection.
        pattern: PatternKind,
    },
    /// Source-native operation without neutral semantics.
    Native {
        /// Native feature-type tag (e.g. `"Extrude"`, `"Fillet"`).
        kind: String,
        /// Source parametric input values keyed by parameter name.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        parameters: BTreeMap<String, String>,
    },
}

/// Edge operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum EdgeSelection {
    /// Resolved topological edges.
    Edges(Vec<EdgeId>),
    /// Format-native selection reference.
    Native(String),
}

/// Face operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FaceSelection {
    /// Resolved topological faces; empty for no selected faces.
    Faces(Vec<FaceId>),
    /// Format-native selection reference.
    Native(String),
}

/// Body operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum BodySelection {
    /// Resolved topological bodies.
    Bodies(Vec<BodyId>),
    /// Format-native selection expression.
    Native(String),
}

/// Direct face-motion law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
        direction: Vector3,
        /// Signed translation distance.
        distance: Length,
    },
    /// Rotation about an axis.
    Rotate {
        /// Point on the rotation axis.
        axis_origin: Point3,
        /// Rotation-axis direction.
        axis_dir: Vector3,
        /// Signed rotation angle.
        angle: Angle,
    },
}

/// Termination of a linear or angular feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Extent {
    /// Fixed depth or angle in one direction.
    Blind {
        /// Fixed travel distance.
        length: Length,
    },
    /// Fixed depth or angle split evenly on both sides of the profile.
    Symmetric {
        /// Total travel distance split around the profile plane.
        length: Length,
    },
    /// Independent depths or angles on each side of the profile.
    TwoSided {
        /// Extent on the first side.
        first: Length,
        /// Extent on the second side.
        second: Length,
    },
    /// Extends through all material.
    ThroughAll,
    /// Extends until it reaches a target face.
    ToFace {
        /// Face terminating the operation.
        face: FaceId,
    },
    /// Fixed angular extent.
    Angle {
        /// Angular travel.
        angle: Angle,
    },
}

/// Boolean effect of a solid-producing feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BooleanOp {
    /// Union with existing bodies.
    Join,
    /// Subtraction from existing bodies.
    Cut,
    /// Intersection with existing bodies.
    Intersect,
    /// Creates an independent new body without combining.
    NewBody,
}

/// Profile consumed by an extrude or revolve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ProfileRef {
    /// Opaque reference into a native feature-input record; no neutral geometry given.
    Native(String),
    /// Solved neutral sketch profile.
    Sketch(crate::sketches::SketchId),
    /// Profile given directly as a set of solved B-rep faces.
    Faces(Vec<FaceId>),
}

/// Trajectory consumed by a sweep or path-driven operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PathRef {
    /// Opaque reference into a native path record.
    Native(String),
    /// Ordered geometry from a neutral sketch.
    Sketch(crate::sketches::SketchId),
    /// Path resolved as ordered topological edges.
    Edges(Vec<EdgeId>),
    /// Path resolved as ordered geometric curves.
    Curves(Vec<CurveId>),
}

/// Radius assignment along filleted edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RadiusSpec {
    /// Same radius along the whole edge chain.
    Constant {
        /// The fillet radius.
        radius: Length,
    },
    /// Radius varying along the edge chain per explicit control points.
    Variable {
        /// Radius samples along the edge chain, in chain-parameter order.
        points: Vec<VariableRadius>,
    },
}

/// Radius at a normalized position along a filleted edge chain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VariableRadius {
    /// Position in `[0, 1]` along the edge chain.
    pub parameter: f64,
    /// Fillet radius at this position.
    pub radius: Length,
}

/// Dimensional definition of an edge chamfer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChamferSpec {
    /// Equal setback distance on both faces meeting the edge.
    Distance {
        /// Setback distance from the edge.
        distance: Length,
    },
    /// Independent setback distances on each face meeting the edge.
    TwoDistances {
        /// Setback distance on the first face.
        first: Length,
        /// Setback distance on the second face.
        second: Length,
    },
    /// A setback distance on one face plus an angle from it to the other.
    DistanceAngle {
        /// Setback distance on the reference face.
        distance: Length,
        /// Chamfer angle measured from the reference face.
        angle: Angle,
    },
}

/// Shape at the entry of a hole.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HoleKind {
    /// Plain cylindrical hole with no entry feature.
    Simple,
    /// Hole with a wider, flat-bottomed counterbore at the entry.
    Counterbore {
        /// Counterbore diameter, wider than the hole diameter.
        diameter: Length,
        /// Counterbore depth.
        depth: Length,
    },
    /// Hole with a conical countersink at the entry.
    Countersink {
        /// Countersink diameter at the surface, wider than the hole diameter.
        diameter: Length,
        /// Countersink included angle.
        angle: Angle,
    },
}

/// Spatial transform used to repeat or reflect seed features.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatternKind {
    /// Repeats seeds evenly along a straight direction.
    Linear {
        /// Repetition direction.
        direction: Vector3,
        /// Distance between consecutive instances.
        spacing: Length,
        /// Total number of instances, including the original.
        count: u32,
    },
    /// Repeats seeds evenly around an axis.
    Circular {
        /// A point on the pattern axis.
        axis_origin: Point3,
        /// Unit direction of the pattern axis.
        axis_dir: Vector3,
        /// Angular span covered by the pattern.
        angle: Angle,
        /// Total number of instances, including the original.
        count: u32,
    },
    /// Reflects seeds across a plane.
    Mirror {
        /// A point on the mirror plane.
        plane_origin: Point3,
        /// Unit normal of the mirror plane.
        plane_normal: Vector3,
    },
}
