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
    /// Feature that owns or defines this parameter.
    pub owner: FeatureId,
    /// Position among parameters owned by the feature.
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

/// One item in a source feature's mixed-content sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FeatureSourceContent {
    /// Literal text between child records.
    Text(String),
    /// Dimension or equation parameter referenced at this position. The same
    /// parameter may occur more than once and may be owned by another feature.
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
    /// Rectangular solid primitive constructed from three local dimensions.
    Block {
        /// Ordered local x, y, and z dimensions.
        dimensions: [Length; 3],
        /// Local-to-model placement; absent until the native frame is resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placement: Option<crate::transform::Transform>,
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
    /// Joins ordered sheet or solid bodies along coincident boundaries.
    SewBodies {
        /// Bodies participating in the sew operation.
        bodies: BodySelection,
        /// Maximum boundary gap accepted by the operation, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap_tolerance: Option<Length>,
    },
    /// Surface patch spanning a selected edge boundary.
    FilledSurface {
        /// Closed edge boundary of the generated patch.
        boundary: EdgeSelection,
        /// Adjacent faces supplying tangent or curvature conditions.
        support_faces: FaceSelection,
        /// Continuity imposed against the support faces.
        continuity: SurfaceContinuity,
        /// Whether the generated patch is merged into adjacent surface bodies.
        merge_result: bool,
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
    /// Removes one side of selected bodies using selected surface faces.
    CutWithSurface {
        /// Bodies cut by the operation.
        targets: BodySelection,
        /// Oriented surface faces defining the cut.
        tools: FaceSelection,
        /// Whether the side opposite the default tool orientation is removed.
        reverse: bool,
    },
    /// Removes one side of target bodies using ordered sheet or solid bodies.
    TrimBodies {
        /// Bodies modified by the operation.
        targets: BodySelection,
        /// Sheet or solid bodies defining the trimming boundary.
        tools: BodySelection,
        /// Side retained by the trim.
        keep: BodyTrimSide,
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
        /// Exit-shape family, when the far-side treatment resolves independently.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_kind: Option<HoleKind>,
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

/// Side retained by a body-trim operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BodyTrimSide {
    /// The source records establish a trim without assigning its retained side.
    Unresolved,
    /// Retain the side selected by the tool orientation.
    Forward,
    /// Retain the side opposite the tool orientation.
    Reverse,
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
    /// Beveled entry whose diameter and angle may be resolved independently.
    Chamfer,
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
