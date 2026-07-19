// SPDX-License-Identifier: Apache-2.0
//! Neutral construction-feature taxonomy.

use std::collections::BTreeMap;

use crate::ids::{
    BodyId, CurveId, EdgeId, FaceId, FeatureInputTopologyId, HistoricalBodyId, HistoricalEdgeId,
    HistoricalFaceId,
};
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
    /// Position in the design configuration list.
    #[serde(default)]
    pub ordinal: u32,
    /// Whether this configuration supplies the document's active model state.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub active: bool,
    /// Format-native configuration slot, when distinct from list order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_index: Option<u32>,
    /// Source display name.
    pub name: String,
    /// Material override, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Configuration-local named values not otherwise represented.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    /// Configuration-specific source expressions keyed by the overridden parameter.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameter_overrides: BTreeMap<ParameterId, String>,
    /// Features suppressed when this configuration is active.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suppressed_features: Vec<FeatureId>,
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

/// A named design expression, optionally owned by a construction feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<ParameterId>,
    /// Source dimension properties not represented by another field.
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "native_kind", rename_all = "snake_case")]
pub enum PmiDimensionSubtype {
    /// Linear distance.
    Linear,
    /// Diameter.
    Diameter,
    /// Radius.
    Radial,
    /// Source-native family without a neutral equivalent.
    Native(String),
}

/// Geometric interpretation requested by a dimension display modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DimensionDisplay {
    /// Displays the dimension as a diameter.
    Diameter,
    /// Displays the dimension as a radius.
    Radius,
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
    /// Earlier features consumed during regeneration, in source operand order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<FeatureId>,
    /// Source operation attributes not consumed by the neutral definition.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub source_properties: BTreeMap<String, String>,
    /// Source XML element name for the operation record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_tag: Option<String>,
    /// Text payload of a source leaf operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_text: Option<String>,
    /// Ordered source text, parameter, and child-feature content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_content: Vec<FeatureSourceContent>,
    /// Bodies produced or modified by the feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<BodyId>,
    /// Neutral construction semantics.
    pub definition: FeatureDefinition,
    /// Identifier of the full-fidelity record in a native namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Typed topology membership at one feature's evaluation input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputTopology {
    /// Globally unique state id.
    pub id: FeatureInputTopologyId,
    /// Feature evaluated from this state.
    pub input_of: FeatureId,
    /// Bodies present in this state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bodies: Vec<HistoricalBodyId>,
    /// Faces present in this state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<HistoricalFaceId>,
    /// Edges present in this state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<HistoricalEdgeId>,
    /// Full-fidelity source state reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// One item in a source feature's mixed-content sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FeatureSourceContent {
    /// Literal text between child records.
    Text(String),
    /// Dimension or equation parameter at this position.
    Parameter(ParameterId),
    /// Nested feature record at this position.
    Feature(FeatureId),
}

/// Neutral construction semantics, with an explicit native escape hatch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "definition", rename_all = "snake_case")]
pub enum FeatureDefinition {
    /// Non-modeling node retained in the ordered feature tree.
    TreeNode {
        /// Structural or presentation role of the node.
        role: FeatureTreeNodeRole,
    },
    /// Direct-modeling session represented by its captured result bodies.
    BaseFeature {
        /// Bodies copied into the parametric timeline when the session closed.
        bodies: BodySelection,
    },
    /// Built-in world-origin reference plane.
    DatumPrincipalPlane {
        /// Canonical principal-plane role.
        plane: PrincipalPlane,
    },
    /// Constructed reference plane.
    DatumPlane {
        /// Plane origin in model space.
        origin: Point3,
        /// Plane normal.
        normal: Vector3,
        /// In-plane u-axis.
        u_axis: Vector3,
    },
    /// Reference plane offset from another datum plane.
    DatumOffsetPlane {
        /// Source plane, when its feature reference is available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<FeatureId>,
        /// Signed normal offset from the source plane.
        distance: Length,
    },
    /// Constructed reference axis.
    DatumAxis {
        /// Point on the axis in model space.
        origin: Point3,
        /// Axis direction.
        direction: Vector3,
    },
    /// Constructed reference point.
    DatumPoint {
        /// Point position in model space.
        position: Point3,
    },
    /// Constructed model-space coordinate system.
    DatumCoordinateSystem {
        /// Frame origin.
        origin: Point3,
        /// Unit x-axis.
        x_axis: Vector3,
        /// Unit y-axis.
        y_axis: Vector3,
        /// Unit z-axis.
        z_axis: Vector3,
    },
    /// Parametric model-space curve defined by coordinate expressions.
    EquationCurve {
        /// Independent parameter symbol used by the coordinate expressions.
        parameter: String,
        /// Model-space x-coordinate expression.
        x_expression: String,
        /// Model-space y-coordinate expression.
        y_expression: String,
        /// Model-space z-coordinate expression.
        z_expression: String,
        /// Inclusive lower parameter bound.
        start: f64,
        /// Inclusive upper parameter bound.
        end: f64,
    },
    /// Curve produced by projecting a source path onto target faces.
    ProjectedCurve {
        /// Sketch or model-space path being projected.
        source: PathRef,
        /// Faces receiving the projected curve.
        target_faces: FaceSelection,
        /// Explicit projection direction; absent for target-normal projection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
        /// Whether projection proceeds in both directions.
        #[serde(default)]
        bidirectional: bool,
    },
    /// Ordered chain of source paths exposed as one construction curve.
    CompositeCurve {
        /// Source segments in traversal order.
        segments: Vec<PathRef>,
        /// Whether the final segment joins the first.
        #[serde(default)]
        closed: bool,
    },
    /// Circular helix or planar spiral constructed around an axis.
    Helix {
        /// Point on the construction axis at the curve start.
        axis_origin: Point3,
        /// Construction-axis direction.
        axis_direction: Vector3,
        /// Initial radial distance from the axis.
        radius: Length,
        /// Signed axial rise per revolution; zero produces a planar spiral.
        pitch: Length,
        /// Positive number of revolutions.
        revolutions: f64,
        /// Whether angular travel is clockwise when viewed along the axis.
        clockwise: bool,
    },
    /// Circular helix with retained native axis placement.
    HelixNativeAxis {
        /// Source-native record carrying the unresolved construction axis.
        axis_native_ref: String,
        /// Initial radial distance from the axis.
        radius: Length,
        /// Signed total rise along the axis.
        height: Length,
        /// Positive number of revolutions.
        revolutions: f64,
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
        center: Point3,
        /// Positive sphere radius.
        radius: Length,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Solid torus primitive.
    Torus {
        /// Torus center in model space.
        center: Point3,
        /// Unit normal of the torus center plane.
        axis: Vector3,
        /// Positive distance from the center to the tube centerline.
        major_radius: Length,
        /// Positive tube radius.
        minor_radius: Length,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Profile mapped onto a target face.
    Wrap {
        /// Sketch or face profile mapped onto the target.
        profile: ProfileRef,
        /// Face receiving the mapped profile.
        face: FaceSelection,
        /// Material or imprint operation performed by the mapping.
        mode: WrapMode,
        /// Normal offset for emboss and deboss operations.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        depth: Option<Length>,
    },
    /// Solved sketch node in the construction history.
    Sketch {
        /// Coordinate space containing the sketch geometry.
        #[serde(default)]
        space: SketchSpace,
        /// Neutral sketch geometry owned by this history node, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<crate::sketches::SketchId>,
    },
    /// Linear extrusion of a profile.
    Extrude {
        /// Profile swept along `direction`.
        profile: ProfileRef,
        /// Direction in which the profile is swept.
        #[serde(default, skip_serializing_if = "ExtrudeDirection::is_profile_normal")]
        direction: ExtrudeDirection,
        /// Plane or face from which the extrusion begins.
        #[serde(default)]
        start: ExtrudeStart,
        /// How far the extrusion travels.
        extent: Extent,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
        /// Draft angle applied to the extruded side walls, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        draft: Option<Angle>,
        /// Independent draft angle applied to the opposite side of a two-sided
        /// extrusion, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        second_draft: Option<Angle>,
    },
    /// Revolution of a profile around an axis.
    Revolve {
        /// Independently resolved construction inputs.
        construction: RevolutionConstruction,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Sweep of a profile along a path.
    Sweep {
        /// Cross-section swept along the path, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<ProfileRef>,
        /// Trajectory followed by the profile, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<PathRef>,
        /// Result family and solid Boolean operation.
        mode: SweepMode,
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
        /// Independently resolved construction inputs.
        construction: RibConstruction,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Edge fillet.
    Fillet {
        /// Ordered edge groups and their radius laws.
        groups: Vec<FilletGroup>,
    },
    /// Edge chamfer.
    Chamfer {
        /// Ordered edge groups and their dimensional specifications.
        groups: Vec<ChamferGroup>,
    },
    /// Thin-wall shell operation.
    Shell {
        /// Faces removed to open the shell.
        removed_faces: FaceSelection,
        /// Wall thickness left after shelling, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thickness: Option<Length>,
        /// Whether the wall is grown outward from the original boundary,
        /// as opposed to inward, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outward: Option<bool>,
    },
    /// Adds material normal to selected faces.
    Thicken {
        /// Faces offset by the operation.
        faces: FaceSelection,
        /// Finished added thickness, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thickness: Option<Length>,
        /// Distribution of thickness relative to the selected faces, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        side: Option<ThickenSide>,
    },
    /// Surface copied at a signed normal offset from selected support faces.
    OffsetSurface {
        /// Faces supplying the source surface geometry.
        faces: FaceSelection,
        /// Signed normal offset in canonical millimeters.
        distance: Length,
    },
    /// Joins selected surface bodies along coincident or near-coincident boundaries.
    KnitSurface {
        /// Faces participating in the knit operation.
        faces: FaceSelection,
        /// Whether coincident face and edge entities are merged.
        merge_entities: bool,
        /// Whether a closed result is converted to a solid body.
        create_solid: bool,
        /// Maximum boundary gap accepted by the operation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap_tolerance: Option<Length>,
    },
    /// Surface patch spanning a selected edge boundary.
    FilledSurface {
        /// Closed boundary of the generated patch.
        boundary: SurfaceBoundary,
        /// Adjacent faces supplying tangent or curvature conditions.
        support_faces: FaceSelection,
        /// Continuity imposed against the support faces, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        continuity: Option<SurfaceContinuity>,
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
        /// Region retained after trimming.
        keep: TrimRegion,
    },
    /// Extends selected surface boundaries by a fixed distance.
    ExtendSurface {
        /// Surface faces whose boundaries are extended.
        faces: FaceSelection,
        /// Positive extension distance in canonical millimeters.
        distance: Length,
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
    /// Creates solid bodies from selected cells enclosed by boundary bodies.
    BoundaryFill {
        /// Bodies whose faces partition space into candidate cells.
        tools: BodySelection,
        /// Enclosed cells retained as result bodies, in source order.
        cells: Vec<BodySelection>,
    },
    /// Removes one side of selected bodies using selected surface faces.
    CutWithSurface {
        /// Bodies cut by the operation.
        targets: BodySelection,
        /// Oriented surface faces defining the cut.
        tools: FaceSelection,
        /// Whether the side opposite the default tool orientation is removed.
        reverse: bool,
    },
    /// Partitions selected bodies with selected surface faces while retaining
    /// every resulting side.
    SplitBody {
        /// Bodies partitioned by the operation.
        targets: BodySelection,
        /// Surface faces extended as necessary to partition the targets.
        tools: FaceSelection,
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
        /// Faces removed from the target body.
        targets: FaceSelection,
        /// Faces whose underlying geometry supplies the replacement.
        replacements: FaceSelection,
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
        translation: Vector3,
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
        height: Option<Length>,
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
        axis: Option<Vector3>,
        /// Applied deformation mode and magnitude.
        mode: FlexMode,
    },
    /// Scales selected bodies about a model-space point.
    Scale {
        /// Bodies transformed by the operation.
        bodies: BodySelection,
        /// Fixed locus of the scale transform.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        center: Option<ScaleCenter>,
        /// Independently decoded uniform and axis scale factors.
        factors: ScaleFactors,
    },
    /// Drilled or machined hole.
    Hole {
        /// Face the hole is placed on, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<FaceSelection>,
        /// Hole placement position, when recorded independently of `face`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<Point3>,
        /// Drilling direction, when recorded independently of the placement face.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
        /// Entry-shape family of the hole.
        kind: HoleKind,
        /// Hole diameter, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diameter: Option<Length>,
        /// How deep the hole extends, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<Extent>,
    },
    /// Repetition or reflection of existing features.
    Pattern {
        /// Features being repeated or reflected; empty when the source selection is unresolved.
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
        /// Source operation attributes that are not dimensional parameters.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        properties: BTreeMap<String, String>,
    },
}

/// Direction in which an extrusion sweeps its profile.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ExtrudeDirection {
    /// Sweep along the profile's positive normal.
    #[default]
    ProfileNormal,
    /// Sweep opposite the profile's positive normal.
    ReversedProfileNormal,
    /// Sweep along an explicit model-space vector.
    Explicit(Vector3),
}

impl ExtrudeDirection {
    /// Whether this is the default positive profile-normal direction.
    pub fn is_profile_normal(&self) -> bool {
        matches!(self, Self::ProfileNormal)
    }
}

/// Independently decoded inputs of a profile revolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RevolutionConstruction {
    /// Profile revolved about the axis, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileRef>,
    /// Placed revolution axis, when resolved as a complete line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<RevolutionAxis>,
    /// Angular extent, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<Extent>,
}

/// Complete line placement used as a revolution axis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RevolutionAxis {
    /// A point on the axis.
    pub origin: Point3,
    /// Unit axis direction.
    pub direction: Vector3,
}

/// Independently decoded inputs of a thin rib operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RibConstruction {
    /// Rib centerline or open profile, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileRef>,
    /// Rib growth direction, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Vector3>,
    /// Finished rib thickness, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Length>,
    /// Distribution of thickness around the profile, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<RibSide>,
    /// Draft state applied to the rib walls.
    #[serde(default)]
    pub draft: RibDraft,
}

/// Distribution of rib thickness around its profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RibSide {
    /// Thickness lies on one side of the profile.
    OneSided,
    /// Thickness is split equally around the profile.
    Centered,
}

/// Draft state of a rib construction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "angle", rename_all = "snake_case")]
pub enum RibDraft {
    /// Draft semantics are present but unresolved.
    #[default]
    Unresolved,
    /// Rib walls have no draft.
    None,
    /// Rib walls use the specified draft angle.
    Angle(Angle),
}

/// Canonical role of a non-modeling feature-tree node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeatureTreeNodeRole {
    /// Annotation container.
    Annotations,
    /// Ambient scene light.
    AmbientLight,
    /// Comment container.
    Comments,
    /// Design-binder container.
    DesignBinder,
    /// Directional scene light.
    DirectionalLight,
    /// Equation container.
    Equations,
    /// Exploded-view container.
    ExplodedViews,
    /// Favorites container.
    Favorites,
    /// Generic history folder.
    History,
    /// Lights, cameras, and scene container.
    LightsAndCameras,
    /// Markup container.
    Markups,
    /// Material container or assignment node.
    Materials,
    /// Note container.
    Notes,
    /// Selection-set container.
    SelectionSets,
    /// Sensor container.
    Sensors,
    /// Solid-body container.
    SolidBodies,
    /// Surface-body container.
    SurfaceBodies,
}

/// Canonical role of a built-in reference plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalPlane {
    /// Front plane through the model origin.
    Front,
    /// Top plane through the model origin.
    Top,
    /// Right plane through the model origin.
    Right,
}

/// Coordinate space of a sketch history node.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SketchSpace {
    /// Geometry lies on one plane and may resolve into the planar sketch arena.
    #[default]
    Planar,
    /// Geometry is spatial and cannot resolve into the planar sketch arena.
    Spatial,
}

/// Selection interpretation for a delete/keep-body operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WrapMode {
    /// Add material above the target face.
    Emboss,
    /// Remove material below the target face.
    Deboss,
    /// Imprint the profile without adding or removing material.
    Scribe,
}

/// Continuity order imposed at a generated surface boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceContinuity {
    /// Positional continuity only.
    Contact,
    /// First-derivative continuity.
    Tangent,
    /// Second-derivative continuity.
    Curvature,
}

/// Boundary input accepted by a filled-surface operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SurfaceBoundary {
    /// Boundary selected as topological edges.
    Edges(EdgeSelection),
    /// Boundary selected as a sketch, curve, or mixed path collection.
    Path(PathRef),
}

/// Region retained by a trim-surface operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrimRegion {
    /// Retain the region enclosed by the trimming path.
    Inside,
    /// Retain the region outside the trimming path.
    Outside,
}

/// Geometric law used to extend a surface boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceExtension {
    /// Continue the source surface parameterization.
    Natural,
    /// Extend boundary tangents as ruled linear strips.
    Linear,
}

/// Direction law for a ruled-surface operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuledSurfaceMode {
    /// Extend normal to the support faces.
    Normal {
        /// Positive extension distance.
        distance: Length,
    },
    /// Extend tangent to the support faces.
    Tangent {
        /// Positive extension distance.
        distance: Length,
    },
    /// Extend along one explicit model-space direction.
    Direction {
        /// Extension direction.
        direction: Vector3,
        /// Positive extension distance.
        distance: Length,
    },
}

/// Fixed locus of a body-scale transform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ScaleCenter {
    /// Combined centroid of the selected bodies.
    Centroid,
    /// Model coordinate-system origin.
    ModelOrigin,
    /// Explicit model-space point.
    Point(Point3),
    /// Format-native coordinate-system or reference identifier.
    Native(String),
}

/// Independently decoded factors of a body-scale transform.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScaleFactors {
    /// Uniform factor, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uniform: Option<f64>,
    /// Model-space x factor, when resolved independently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// Model-space y factor, when resolved independently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// Model-space z factor, when resolved independently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z: Option<f64>,
}

impl ScaleFactors {
    /// Resolve the effective model-space factors when construction is complete.
    #[must_use]
    pub fn resolved(self) -> Option<Vector3> {
        if let Some(factor) = self.uniform {
            return Some(Vector3::new(factor, factor, factor));
        }
        Some(Vector3::new(self.x?, self.y?, self.z?))
    }
}

/// Direction in which a thicken feature adds material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThickenSide {
    /// Add material along the selected-face normal.
    Forward,
    /// Add material opposite the selected-face normal.
    Reverse,
    /// Split the thickness equally across both sides.
    Both,
}

/// Edge operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum EdgeSelection {
    /// Selection exists semantically but its operands are not resolved.
    Unresolved,
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
        edges: Vec<HistoricalEdgeId>,
        /// Format-native selection reference.
        native: String,
    },
    /// Proven historical edges plus source operands whose edge identity is unresolved.
    /// `edges` is empty when the input state is known but no member identity resolves.
    HistoricalPartial {
        /// Input topology containing every resolved edge.
        state: FeatureInputTopologyId,
        /// Proven state-local edge identities in source operand order.
        edges: Vec<HistoricalEdgeId>,
        /// Stable native identities of unresolved source operands.
        unresolved: Vec<String>,
        /// Format-native group selection reference.
        native: String,
    },
    /// Format-native selection reference.
    Native(String),
}

/// Face operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
        faces: Vec<HistoricalFaceId>,
        /// Format-native selection reference.
        native: String,
    },
    /// Historical faces proven for part of a native selection.
    HistoricalPartial {
        /// Input topology containing every resolved face.
        state: FeatureInputTopologyId,
        /// Proven state-local face identities in source operand order.
        faces: Vec<HistoricalFaceId>,
        /// Stable native identities of unresolved source operands.
        unresolved: Vec<String>,
        /// Format-native selection reference.
        native: String,
    },
    /// Format-native selection reference.
    Native(String),
}

/// Body operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    /// Bodies resolved in the containing feature's input topology.
    Historical {
        /// Input topology containing every selected body.
        state: FeatureInputTopologyId,
        /// State-local body identities in operand order.
        bodies: Vec<HistoricalBodyId>,
        /// Format-native selection expression.
        native: String,
    },
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

/// Model-space axis-angle rotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AxisAngle {
    /// Point on the rotation axis.
    pub origin: Point3,
    /// Rotation-axis direction.
    pub direction: Vector3,
    /// Signed rotation angle.
    pub angle: Angle,
}

/// Start condition of a linear extrusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExtrudeStart {
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
        face: FaceSelection,
        /// Signed displacement from the terminating face.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<Length>,
    },
    /// Fixed angular extent.
    Angle {
        /// Angular travel.
        angle: Angle,
    },
    /// Angular travel split equally around the profile plane.
    SymmetricAngle {
        /// Total angular travel.
        angle: Angle,
    },
    /// Independent angular travel on each side of the profile plane.
    TwoSidedAngles {
        /// Angular travel on the first side.
        first: Angle,
        /// Angular travel on the second side.
        second: Angle,
    },
}

/// Boolean effect of a solid-producing feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// Placement and parameterization of a solid Coil primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CoilConstruction {
    /// Axis frame and angular origin.
    pub placement: CoilPlacement,
    /// Diameter of the reference trajectory at its start.
    pub diameter: Length,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoilPlacement {
    /// Complete right-handed model-space frame.
    Explicit {
        /// Center of the trajectory on its base plane.
        origin: Point3,
        /// Positive trajectory-axis direction.
        axis: Vector3,
        /// Direction from `origin` to angular position zero.
        radial: Vector3,
    },
    /// Complete placement retained in one source-native construction aggregate.
    Native {
        /// Native record or scope containing the placement semantics.
        native_ref: String,
    },
}

/// Independent driving dimensions of a Coil trajectory.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoilExtent {
    /// Axial coil driven by revolution count and total signed height.
    RevolutionsHeight {
        /// Positive angular-turn count.
        revolutions: f64,
        /// Signed axial travel.
        height: Length,
    },
    /// Axial coil driven by revolution count and signed pitch per revolution.
    RevolutionsPitch {
        /// Positive angular-turn count.
        revolutions: f64,
        /// Signed axial travel per revolution.
        pitch: Length,
    },
    /// Axial coil driven by total signed height and signed pitch per revolution.
    HeightPitch {
        /// Signed axial travel.
        height: Length,
        /// Signed axial travel per revolution.
        pitch: Length,
    },
    /// Planar spiral driven by revolution count and signed radial pitch.
    Spiral {
        /// Positive angular-turn count.
        revolutions: f64,
        /// Signed radial growth per revolution.
        radial_pitch: Length,
    },
}

/// Generated cross-section of a Coil primitive.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoilSection {
    /// Circular section whose size is its diameter.
    Circular {
        /// Circle diameter.
        diameter: Length,
    },
    /// Square section whose size is its edge length.
    Square {
        /// Edge length.
        size: Length,
    },
    /// Equilateral triangle pointing radially away from the axis.
    ExternalTriangle {
        /// Edge length.
        size: Length,
    },
    /// Equilateral triangle pointing radially toward the axis.
    InternalTriangle {
        /// Edge length.
        size: Length,
    },
}

/// Radial placement of a generated Coil section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoilResult {
    /// Create an independent body.
    NewBody,
    /// Combine the swept volume with selected existing bodies.
    Boolean {
        /// Join, cut, or intersection operation.
        operation: BooleanOp,
        /// Existing bodies participating in the operation.
        targets: BodySelection,
    },
}

/// Result semantics of a swept profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SweepMode {
    /// Native sweep family is known but its result subtype is unresolved.
    Unresolved,
    /// Sweep creates or modifies a solid body.
    Solid {
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Sweep creates a sheet body.
    Surface,
}

/// One directed use of a solved sketch curve in an arrangement boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchProfileBoundaryUse {
    /// Sketch entity supplying the curve geometry.
    pub entity: crate::sketches::SketchEntityId,
    /// Parameter endpoints on the source curve, ordered in the entity's stored direction.
    pub parameter_range: [f64; 2],
    /// Whether boundary traversal opposes the interval's stored direction.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reversed: bool,
}

/// One connected planar region bounded by solved sketch curves.
///
/// Whole-loop regions retain compact profile indices. Arrangement regions
/// carry exact trimmed curve uses when their boundary switches source loops at
/// intersections. The untagged representation preserves the established JSON
/// shape of whole-loop regions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SketchProfileRegion {
    /// Exterior and holes are complete entries in the sketch profile table.
    Loops {
        /// Exterior-loop index in the referenced sketch's profile-loop table.
        outer: u32,
        /// Immediate child loops removed from the exterior interior.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        holes: Vec<u32>,
    },
    /// Boundary rings switch source curves at arrangement intersections.
    Trimmed {
        /// Directed exterior boundary ring.
        outer_boundary: Vec<SketchProfileBoundaryUse>,
        /// Directed hole boundary rings.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        hole_boundaries: Vec<Vec<SketchProfileBoundaryUse>>,
    },
}

/// Profile consumed by a profile-driven feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ProfileRef {
    /// Opaque reference into a native feature-input record; no neutral geometry given.
    Native(String),
    /// Solved neutral sketch profile.
    Sketch(crate::sketches::SketchId),
    /// Specific solved profile loops within one neutral sketch.
    SketchProfiles {
        /// Sketch containing the selected loops.
        sketch: crate::sketches::SketchId,
        /// Zero-based indices into [`crate::sketches::Sketch::profiles`].
        profiles: Vec<u32>,
    },
    /// Exact union of bounded atomic regions within one neutral sketch.
    SketchRegions {
        /// Sketch containing every referenced boundary loop.
        sketch: crate::sketches::SketchId,
        /// Connected regions in source selection order.
        regions: Vec<SketchProfileRegion>,
    },
    /// Source-native selection within a known neutral sketch.
    SketchSelection {
        /// Sketch containing the unresolved selected geometry.
        sketch: crate::sketches::SketchId,
        /// Full-fidelity native selection records in source order.
        selections: Vec<String>,
    },
    /// Profile given by faces in the consuming feature's input topology.
    HistoricalFaces {
        /// Input topology containing every selected face.
        state: FeatureInputTopologyId,
        /// State-local face identities in source selection order.
        faces: Vec<HistoricalFaceId>,
        /// Full-fidelity source selection groups in source order.
        native: Vec<String>,
    },
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
    /// Radius law is retained but not fully resolved.
    Unresolved {
        /// Structural law form, when independently identified.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<RadiusForm>,
    },
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

/// One independently dimensioned group of filleted edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FilletGroup {
    /// Edges sharing this radius law.
    pub edges: EdgeSelection,
    /// Radius assignment along the edges.
    pub radius: RadiusSpec,
    /// Dimensionless tangency weight, when specified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangency_weight: Option<f64>,
}

/// One independently dimensioned group of chamfered edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChamferGroup {
    /// Edges sharing this dimensional specification.
    pub edges: EdgeSelection,
    /// Dimensional definition applied to the edges.
    pub spec: ChamferSpec,
}

/// Structural form of a fillet radius law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RadiusForm {
    /// One radius applies to the entire edge chain.
    Constant,
    /// Radius varies along the edge chain.
    Variable,
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
    /// Dimensional specification is retained but not fully resolved.
    Unresolved {
        /// Structural specification form, when independently identified.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<ChamferForm>,
    },
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

/// Structural form of a chamfer dimensional specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChamferForm {
    /// One equal setback distance.
    Distance,
    /// Independent setback distances on each adjacent face.
    TwoDistances,
    /// One setback distance and one angle.
    DistanceAngle,
}

/// Shape at the entry of a hole.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HoleKind {
    /// Entry treatment fields whose complete form is unresolved.
    Unresolved {
        /// Entry-treatment family, when established.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<HoleForm>,
        /// Resolved counterbore diameter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        counterbore_diameter: Option<Length>,
        /// Resolved counterbore depth.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        counterbore_depth: Option<Length>,
        /// Resolved countersink diameter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        countersink_diameter: Option<Length>,
        /// Resolved countersink included angle.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        countersink_angle: Option<Angle>,
    },
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

/// Structural form of a hole entry treatment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HoleForm {
    /// Wider, flat-bottomed entry.
    Counterbore,
    /// Conical entry.
    Countersink,
}

/// Deformation applied by a flex feature.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlexMode {
    /// Mode fields whose complete deformation is unresolved.
    Unresolved {
        /// Deformation family, when established.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<FlexForm>,
        /// Resolved bending or twisting magnitude.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<Angle>,
        /// Resolved taper factor.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        factor: Option<f64>,
        /// Resolved stretching distance.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<Length>,
    },
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
        factor: f64,
    },
    /// Extend or contract along the flex axis.
    Stretching {
        /// Signed change in length.
        distance: Length,
    },
}

/// Structural form of a flex deformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// Spatial transform used to repeat or reflect seed features.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatternKind {
    /// Pattern construction whose form or required operands are unresolved.
    Unresolved {
        /// Native pattern form, when identified independently of its operands.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<PatternForm>,
    },
    /// Repeats seeds evenly along a straight direction.
    Linear {
        /// Repetition direction, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
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
    /// Repeats seeds at fixed arc-length spacing along a curve.
    CurveDriven {
        /// Pattern path, when its native reference is available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<PathRef>,
        /// Arc-length spacing between consecutive instances.
        spacing: Length,
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

/// Structural form of a repeated or reflected feature operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PatternForm {
    /// Translation along a straight direction.
    Linear,
    /// Rotation around an axis.
    Circular,
    /// Translation along a curve.
    CurveDriven,
    /// Reflection across a plane.
    Mirror,
}
