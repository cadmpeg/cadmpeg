// SPDX-License-Identifier: Apache-2.0
//! Neutral construction-feature taxonomy.

use std::collections::BTreeMap;

use crate::ids::{
    BodyId, CurveId, EdgeId, FaceId, FeatureInputTopologyId, HistoricalBodyId, HistoricalEdgeId,
    HistoricalFaceId, SubdId, VertexId,
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
    /// Evaluated parameter state when this configuration is active.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameter_values: BTreeMap<ParameterId, ParameterValue>,
    /// Evaluated feature operation state when this configuration is active.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub feature_states: BTreeMap<FeatureId, ConfigurationFeatureState>,
    /// Identifier of the full-fidelity record in a native namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Configuration-local evaluation state for one construction feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigurationFeatureState {
    /// Whether evaluation of this feature is disabled in the configuration.
    #[serde(default)]
    pub suppressed: bool,
    /// Earlier features consumed during regeneration in source operand order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<FeatureId>,
    /// Bodies produced or modified in the configuration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<BodyId>,
    /// Evaluated construction semantics in the configuration.
    pub definition: FeatureDefinition,
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
    /// Angular extent in radians.
    Angle,
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
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
        /// Ordered features owned by this node.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        children: Vec<FeatureId>,
        /// Active child, when the source design identifies one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active_child: Option<FeatureId>,
    },
    /// Direct-modeling session represented by its captured result bodies.
    BaseFeature {
        /// Bodies copied into the parametric timeline when the session closed.
        bodies: BodySelection,
    },
    /// Independent bodies introduced by a copy-and-paste operation.
    InsertBodies {
        /// Newly created body copies in source order.
        bodies: BodySelection,
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
        diameter: Option<Length>,
        /// Axial extent of the annotation, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<CosmeticThreadExtent>,
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
    /// Constructed reference-plane family whose model-space frame is unresolved.
    DatumPlaneUnresolved,
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
    /// Standalone model vertex constructed at one point.
    PointGeometry {
        /// Vertex position in the feature's local construction frame.
        position: Point3,
    },
    /// Straight edge between two finite points.
    LineSegment {
        /// Start point.
        start: Point3,
        /// End point.
        end: Point3,
    },
    /// Circular edge over an angular interval.
    CircularArc {
        /// Circle center in the feature's local construction frame.
        center: Point3,
        /// Circle-plane normal.
        normal: Vector3,
        /// Circle radius.
        radius: Length,
        /// Start parameter angle.
        start_angle: Angle,
        /// End parameter angle.
        end_angle: Angle,
    },
    /// Elliptic edge over an angular interval.
    EllipticArc {
        /// Ellipse center in the feature's local construction frame.
        center: Point3,
        /// Circle-plane normal.
        normal: Vector3,
        /// Major-axis direction in the ellipse plane.
        major_axis: Vector3,
        /// Major semiaxis radius.
        major_radius: Length,
        /// Minor semiaxis radius.
        minor_radius: Length,
        /// Start parameter angle.
        start_angle: Angle,
        /// End parameter angle.
        end_angle: Angle,
    },
    /// Ordered straight-edge chain.
    Polyline {
        /// Ordered vertices in the feature's local construction frame.
        points: Vec<Point3>,
        /// Whether the last point connects back to the first.
        closed: bool,
    },
    /// Regular planar polygon centered at the local origin.
    RegularPolygonCurve {
        /// Number of polygon sides.
        sides: u32,
        /// Center-to-vertex distance.
        circumradius: Length,
    },
    /// Rectangular bounded planar face in the local XY plane.
    PlanarPatch {
        /// Length along the local x-axis.
        length: Length,
        /// Width along the local y-axis.
        width: Length,
    },
    /// Faces built from an ordered set of source shapes.
    FaceFromShapes {
        /// Complete ordered source-shape selection.
        sources: BodySelection,
        /// Extensible native face-building algorithm identifier.
        face_maker_class: String,
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
    /// Shapes projected along a direction onto one support surface.
    ProjectOnSurface {
        /// Ordered shapes and subelements projected onto the support.
        sources: PathRef,
        /// Single support face receiving the projection.
        support_face: FaceSelection,
        /// Unit projection direction.
        direction: Vector3,
        /// Result topology retained from the projected shapes.
        mode: SurfaceProjectionMode,
        /// Normal extrusion height used to turn projected faces into solids.
        height: Length,
        /// Normal offset applied to the projected result.
        offset: Length,
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
        /// Angular position at the curve start.
        #[serde(default)]
        start_angle: Angle,
        /// Whether angular travel is clockwise when viewed along the axis.
        clockwise: bool,
        /// Radial growth per revolution for a planar spiral, when non-cylindrical.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radial_growth: Option<Length>,
        /// Cone half-angle for a conical helix, when non-cylindrical.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cone_angle: Option<Angle>,
        /// Number of turns per generated curve subdivision, when requested.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        segment_turns: Option<f64>,
        /// Persisted construction algorithm generation, when selectable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        construction_style: Option<HelixConstructionStyle>,
    },
    /// Circular helix with retained native axis placement.
    HelixNativeAxis {
        /// Source-native record carrying the unresolved construction axis.
        axis_native_ref: String,
        /// Signed total rise along the axis.
        #[serde(alias = "radius")]
        axial_rise: Length,
        /// Signed axial rise per revolution.
        #[serde(alias = "height")]
        pitch: Length,
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
        /// Neutral planar sketch geometry owned by this history node, when resolved.
        /// Neutral sketch geometry owned by this history node, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<crate::sketches::SketchId>,
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
    StoredGeometry,
    /// Geometry copied from an earlier feature without an additional modeling operation.
    DerivedGeometry {
        /// Feature supplying the copied geometry.
        source: FeatureId,
    },
    /// Geometry imported from an external model file.
    ImportedGeometry {
        /// External source path exactly as persisted by the design.
        path: String,
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
        /// Independent draft angle applied to the second side of an extrusion
        /// whose extent has two sides, when present.
        #[serde(
            default,
            alias = "reverse_draft",
            skip_serializing_if = "Option::is_none"
        )]
        second_draft: Option<Angle>,
        /// Persisted source used to resolve the extrusion direction.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction_source: Option<ExtrusionDirectionSource>,
        /// Whether the result is a solid (`true`) or sheet (`false`), when selectable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        solid: Option<bool>,
        /// Native face-building policy used to turn closed wires into faces.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face_maker: Option<ExtrusionFaceMaker>,
        /// Taper orientation used for inner wires, when selectable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inner_wire_taper: Option<InnerWireTaper>,
        /// Signed offset from the first side's terminating geometry.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_offset: Option<Length>,
        /// Signed offset from the second side's terminating geometry.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        second_offset: Option<Length>,
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
        construction: RevolutionConstruction,
        /// Boolean combination with existing bodies.
        op: BooleanOp,
    },
    /// Sweep of a profile along a path.
    Sweep {
        /// Cross-section swept along the path, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<ProfileRef>,
        /// Additional cross-sections after the primary profile, in path order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sections: Vec<ProfileRef>,
        /// Trajectory followed by the profile, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<PathRef>,
        /// Result family and solid Boolean operation.
        mode: SweepMode,
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
        /// End-to-start profile scale ratio, when specified.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<f64>,
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
        #[serde(alias = "profiles")]
        sections: Vec<LoftSection>,
        /// Optional ordered guide trajectories.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        guides: Vec<PathRef>,
        /// Optional centerline to which the loft sections remain normal.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        centerline: Option<PathRef>,
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
        /// Maximum polynomial degree used to interpolate the sections, when constrained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_degree: Option<u32>,
        /// Whether section topology is checked and adjusted for compatibility, when carried.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        check_compatibility: Option<bool>,
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
        profile: ProfileRef,
        /// Finished sheet thickness.
        thickness: Length,
        /// Distribution of thickness relative to the profile plane.
        side: SheetMetalThicknessSide,
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
        /// Whether the dimensional reference side is reversed.
        #[serde(default)]
        flip_direction: bool,
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
        distance: Length,
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
        /// First intersected source shape.
        first: BodySelection,
        /// Second intersected source shape.
        second: BodySelection,
        /// Whether the resulting section edges are approximated.
        approximate: bool,
    },
    /// Reflects one source shape across a model-space plane.
    MirrorShape {
        /// Shape transformed into the mirrored result.
        source: BodySelection,
        /// Point on the persisted resolved mirror plane.
        plane_origin: Point3,
        /// Unit normal of the persisted resolved mirror plane.
        plane_normal: Vector3,
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
        /// Sketch or profile supplying one or more hole locations.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<ProfileRef>,
        /// Geometry families in the profile that generate hole locations.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile_filter: Option<HoleProfileFilter>,
        /// Face the hole is placed on, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<FaceSelection>,
        /// Shared hole entry position when the construction carries one location separately.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<Point3>,
        /// Shared drilling direction when carried independently of complete placements.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
        /// Complete one-or-many hole placements. Empty when placement is unresolved.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        placements: Vec<HolePlacement>,
        /// Structural drilling, entry-treatment, and threading form.
        kind: HoleKind,
        /// Hole diameter, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diameter: Option<Length>,
        /// How deep the hole extends, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<Extent>,
        /// Shape and depth convention at the blind end of the hole.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bottom: Option<HoleBottom>,
        /// Included taper angle for a conical hole, when enabled.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        taper_angle: Option<Angle>,
        /// Standard sizing and thread construction, when specified.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        specification: Option<Box<HoleSpecification>>,
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
        operation: Box<FeatureDefinition>,
        /// Whether redundant splitter boundaries are removed.
        refine: bool,
        /// Boolean-operation tolerance selection carried by the feature family.
        fuzzy_tolerance: FuzzyTolerance,
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
/// One complete spatial placement in a hole operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HolePlacement {
    /// Position and directed drilling vector recorded by the feature definition.
    Directed {
        /// Hole entry position in model space.
        position: Point3,
        /// Directed drilling vector.
        direction: Vector3,
    },
    /// Unoriented geometric axis inferred from a generated cylindrical surface.
    Axis {
        /// Point on the cylinder axis in model space.
        origin: Point3,
        /// Unoriented cylinder-axis vector; its sign has no semantic meaning.
        axis: Vector3,
    },
}

/// One geometric selection repeated or reflected by a pattern operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PatternSeed {
    /// Complete result of a preceding construction-history feature.
    Feature(FeatureId),
    /// Selected faces, including faces in an intermediate regenerated result.
    Faces(FaceSelection),
    /// Selected bodies, including bodies in an intermediate regenerated result.
    Bodies(BodySelection),
}

/// External model format consumed by an imported-geometry feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FuzzyTolerance {
    /// Let the modeling kernel use its default tolerance.
    KernelDefault,
    /// Determine a suitable tolerance from the participating shapes.
    Automatic,
    /// Use the supplied positive model-unit tolerance.
    Explicit(f64),
}

const fn default_true() -> bool {
    true
}

/// Geometric offset construction used by a thin-wall shell operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuledCurveOrientation {
    /// Select curve traversal automatically from endpoint proximity.
    Automatic,
    /// Retain both persisted curve traversal directions.
    Forward,
    /// Reverse the second curve relative to the first.
    Reversed,
}

/// Canonical dimensions of an analytic solid primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PrimitiveSolid {
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

/// Independently decoded inputs of a profile revolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RevolutionConstruction {
    /// Profile revolved about the axis, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileRef>,
    /// Placed revolution axis, when resolved from an axis-bearing selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<RevolutionAxis>,
    /// Angular extent, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<Extent>,
    /// Native edge, datum, or sketch-axis selection used to resolve the axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis_reference: Option<PathRef>,
    /// Whether a standalone revolution creates a solid rather than a sheet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid: Option<bool>,
    /// Face-building algorithm used for a standalone solid revolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_maker_class: Option<String>,
    /// Compatibility ordering for fusing a `PartDesign` revolution into its body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuse_order: Option<RevolutionFuseOrder>,
    /// Whether a profile containing multiple faces is accepted as one operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_multi_profile_faces: Option<bool>,
}

/// Operand ordering used to fuse a `PartDesign` revolution result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RevolutionFuseOrder {
    /// Existing body is the first fuse operand.
    BaseFirst,
    /// Newly revolved feature is the first fuse operand.
    FeatureFirst,
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CosmeticThreadExtent {
    /// Fixed thread length along the cylindrical face.
    Blind {
        /// Positive axial thread length.
        length: Length,
    },
    /// Thread annotation spans the complete cylindrical face.
    Through,
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

/// Distribution of sheet thickness relative to its construction plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SheetMetalThicknessSide {
    /// Thickness lies along the profile plane's positive normal.
    Forward,
    /// Thickness lies opposite the profile plane's positive normal.
    Reverse,
    /// Thickness is split equally across both sides of the profile plane.
    Symmetric,
}

/// Edge operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    /// Edges in intermediate regenerated feature results, paired with the
    /// format-native selection required for rewrite.
    Generated {
        /// Feature-local edge identities.
        edges: Vec<GeneratedEdgeRef>,
        /// Format-native persistent selection reference.
        native: String,
    },
    /// Format-native selection reference.
    Native(String),
}

/// Persistent identity of an edge in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GeneratedEdgeRef {
    /// Feature whose regenerated result owns the edge.
    pub feature: FeatureId,
    /// Feature-local persistent edge identity.
    pub local_id: String,
}

/// Persistent identity of a face in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GeneratedFaceRef {
    /// Feature whose regenerated result owns the face.
    pub feature: FeatureId,
    /// Feature-local persistent face identity.
    pub local_id: String,
}

/// Persistent identity of a vertex in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GeneratedVertexRef {
    /// Feature whose regenerated result owns the vertex.
    pub feature: FeatureId,
    /// Feature-local persistent vertex identity.
    pub local_id: String,
}

/// Vertex operand resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    /// Faces in an intermediate regenerated feature result, paired with the
    /// format-native selection required for rewrite.
    Generated {
        /// Feature-local face identities.
        faces: Vec<GeneratedFaceRef>,
        /// Format-native persistent selection reference.
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
    /// Bodies in intermediate regenerated feature results, paired with the
    /// format-native selection required for rewrite.
    Generated {
        /// Feature-local body identities.
        bodies: Vec<GeneratedBodyRef>,
        /// Format-native persistent selection reference.
        native: String,
    },
    /// Persistent bodies in the consuming feature's regeneration input state.
    Local {
        /// Ordered feature-input-local body identities.
        bodies: Vec<String>,
        /// Format-native persistent selection reference.
        native: String,
    },
    /// Format-native selection expression.
    Native(String),
}

/// Persistent identity of a body in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GeneratedBodyRef {
    /// Feature whose regenerated result owns the body.
    pub feature: FeatureId,
    /// Feature-local persistent body identity.
    pub local_id: String,
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
    /// Native extent is present structurally but its termination is unresolved.
    Unresolved,
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
    /// Independent termination laws on the two oriented sides of the profile.
    TwoSidedExtents {
        /// Termination on the oriented first side.
        first: Box<Extent>,
        /// Termination on the opposite second side.
        second: Box<Extent>,
    },
    /// One termination law mirrored across the profile plane.
    SymmetricExtent {
        /// First-side termination whose result is mirrored.
        extent: Box<Extent>,
    },
    /// Extends through all material.
    ThroughAll,
    /// Extends through all material on both sides of the profile.
    ThroughAllBoth,
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
        offset: Length,
    },
    /// Extends until one of the faces in a selected target shape.
    ToShape {
        /// Native or resolved target shape selection.
        target: FaceSelection,
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

/// Persisted source of a resolved linear-extrusion direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

/// Native face-building policy for a solid linear extrusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExtrusionFaceMaker {
    /// Runtime face-maker class, retained as an extensible semantic identifier.
    pub class: String,
    /// Persisted enumeration value corresponding to the class, when carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
}

/// Relationship between outer-wire and inner-wire taper directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InnerWireTaper {
    /// Inner wires taper opposite to outer wires.
    Inverted,
    /// Inner wires taper in the same direction as outer wires.
    SameAsOuter,
}

/// Persisted construction algorithm used for a parametric helix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HelixConstructionStyle {
    /// Historical construction retained for document compatibility.
    Legacy,
    /// Corrected construction used by newly created features.
    Corrected,
}

/// Result topology retained by a projection-on-surface operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// Cross-section orientation law along a sweep path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    /// Frame constrained by a fixed binormal direction.
    Binormal {
        /// Unit binormal direction.
        direction: Vector3,
    },
}

/// Corner continuation used by a sweep path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// Complete construction of a solid helical sweep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HelicalSweepConstruction {
    /// Profile swept along the helical path.
    pub profile: ProfileRef,
    /// Point at the start of the helix axis.
    pub axis_origin: Point3,
    /// Unit direction of positive axial travel.
    pub axis_direction: Vector3,
    /// Persisted authoring law identifying the independent parameters.
    pub law: HelicalSweepLaw,
    /// Positive axial advance per turn; zero is permitted for a planar spiral.
    pub pitch: Length,
    /// Signed total axial travel.
    pub height: Length,
    /// Positive number of turns.
    pub turns: f64,
    /// Signed radial change per turn.
    pub radial_growth: Length,
    /// Cone half-angle corresponding to radial growth.
    pub cone_angle: Angle,
    /// Whether angular travel is left-handed along the positive axis.
    pub left_handed: bool,
    /// Whether path travel runs opposite the declared axis direction.
    pub reversed: bool,
    /// Relative tolerance used while joining the generated sweep.
    pub tolerance: f64,
    /// Whether a profile containing multiple faces is accepted as one operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_multi_profile_faces: Option<bool>,
}

/// Independent-parameter law used to author a helical sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BinderSource {
    /// Bound object identity.
    pub target: BinderTarget,
    /// Ordered native subelement selectors; empty selects the complete object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subelements: Vec<String>,
}

/// Resolved or externally scoped binder target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
        document: String,
        /// Object identity within the source document.
        object: String,
    },
    /// Source-native target identity that cannot be resolved further.
    Native {
        /// Opaque source-native target identity.
        reference: String,
    },
}

/// Binding behavior and optional derived-shape construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BinderPlacement {
    /// Interpret source placement relative to the binder context.
    Relative,
    /// Preserve source placement in global coordinates.
    Global,
}

/// Copy-on-change state of a subshape binder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BinderOffset {
    /// Signed offset distance.
    pub distance: Length,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BinderOffsetJoin {
    /// Circular corner arcs.
    Arcs,
    /// Tangent continuation.
    Tangent,
    /// Sharp line-line intersections.
    Intersection,
}

/// Profile consumed by a profile-driven feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    /// Specific solved profile loops within one neutral spatial sketch.
    SpatialSketchProfiles {
        /// Spatial sketch containing the selected loops.
        sketch: crate::sketches::SpatialSketchId,
        /// Zero-based indices into [`crate::sketches::SpatialSketch::profiles`].
        profiles: Vec<u32>,
    },
    /// Source-native selection within a known neutral spatial sketch.
    SpatialSketchSelection {
        /// Spatial sketch containing the unresolved selected geometry.
        sketch: crate::sketches::SpatialSketchId,
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
    /// Complete curve result of an earlier construction-history feature.
    Feature(FeatureId),
    /// Curves in an intermediate regenerated feature result, paired with the
    /// format-native persistent reference required for rewrite.
    Generated {
        /// Persistent feature-local curve identities.
        curves: Vec<GeneratedCurveRef>,
        /// Format-native persistent profile reference.
        native: String,
    },
    /// Profile given directly as a set of solved B-rep faces.
    Faces(Vec<FaceId>),
}

/// One ordered cross-section consumed by a loft operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum LoftSection {
    /// Planar or face-backed section profile.
    Profile(ProfileRef),
    /// Point-like terminal section.
    Point(LoftPointSection),
}

/// Point-like cross-section consumed by a loft operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LoftPointSection {
    /// Source-native point-selection record whose position is not resolved.
    #[serde(rename = "native_point")]
    Native(String),
    /// Solved model-space point section.
    Point(Point3),
    /// Solved B-rep vertex section.
    Vertex(VertexId),
}

/// Persistent identity of a curve in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GeneratedCurveRef {
    /// Feature whose regenerated result owns the curve.
    pub feature: FeatureId,
    /// Complete ordered feature-local component identity.
    pub local_id: String,
}

/// Trajectory consumed by a sweep or path-driven operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PathRef {
    /// Opaque reference into a native path record.
    Native(String),
    /// Ordered geometry from a neutral sketch.
    Sketch(crate::sketches::SketchId),
    /// Source-native curve selection within a known neutral spatial sketch.
    SpatialSketchSelection {
        /// Spatial sketch containing the selected curves.
        sketch: crate::sketches::SpatialSketchId,
        /// Full-fidelity native selection records in path order.
        selections: Vec<String>,
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
        edges: Vec<HistoricalEdgeId>,
        /// Full-fidelity source path selection.
        native: String,
    },
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
    /// Constant transverse chord length across the fillet surface.
    Chordal {
        /// Distance between the fillet's two support boundaries.
        chord_length: Length,
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
    /// Constant transverse chord length defines the fillet width.
    Chordal,
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

/// Structural drilling, entry-treatment, and threading form of a hole.
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
    /// Plain cylindrical hole terminating in a conical drill point.
    SimpleDrilled {
        /// Included angle of the conical drill point.
        drill_point_angle: Angle,
    },
    /// Hole with a wider, flat-bottomed counterbore at the entry.
    Counterbore {
        /// Counterbore diameter, wider than the hole diameter.
        diameter: Length,
        /// Counterbore depth.
        depth: Length,
    },
    /// Counterbored hole terminating in a conical drill point.
    CounterboreDrilled {
        /// Counterbore diameter, wider than the hole diameter.
        diameter: Length,
        /// Axial depth of the counterbore.
        depth: Length,
        /// Included angle of the conical drill point.
        drill_point_angle: Angle,
    },
    /// Hole with a conical countersink at the entry.
    Countersink {
        /// Countersink diameter at the surface, wider than the hole diameter.
        diameter: Length,
        /// Countersink included angle.
        angle: Angle,
    },
    /// Internally threaded hole terminating in a conical drill point.
    Threaded {
        /// Nominal major diameter of the internal thread.
        major_diameter: Length,
        /// Axial length over which the thread is cut.
        thread_depth: Length,
        /// Thread pitch, when carried independently of the nominal designation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pitch: Option<Length>,
        /// Included angle of the conical drill point.
        drill_point_angle: Angle,
    },
    /// Hole with a conical entry followed by a wider cylindrical recess.
    Counterdrill {
        /// Entry-recess diameter.
        diameter: Length,
        /// Cylindrical recess depth.
        depth: Length,
        /// Included conical entry angle.
        angle: Angle,
    },
}

/// Profile geometry families accepted as hole-location generators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HoleProfileFilter {
    /// Profile points generate holes.
    pub points: bool,
    /// Profile circles generate holes at their centers.
    pub circles: bool,
    /// Profile circular arcs generate holes at their centers.
    pub arcs: bool,
}

/// Blind-end construction of a drilled hole.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HoleBottom {
    /// Flat-bottomed cylindrical end.
    Flat,
    /// Conical drill point.
    Angled {
        /// Included drill-point angle.
        included_angle: Angle,
        /// Whether the declared blind depth reaches the tip instead of the shoulder.
        depth_to_tip: bool,
    },
}

/// Standard sizing and optional physical-thread construction for a hole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HoleSpecification {
    /// Named thread or fastener standard family.
    pub standard: String,
    /// Nominal size designation within the standard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub designation: Option<String>,
    /// Tolerance or thread class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// Clearance-hole fit class when the hole is not threaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<String>,
    /// Whether the hole is internally threaded rather than a clearance hole.
    pub threaded: bool,
    /// Whether exact helical thread geometry is modeled.
    pub modeled: bool,
    /// Whether cosmetic thread presentation is requested.
    pub cosmetic: bool,
    /// Thread pitch in canonical millimeters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<Length>,
    /// Nominal major thread diameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub major_diameter: Option<Length>,
    /// Thread handedness.
    pub hand: ThreadHand,
    /// Axial thread-depth construction.
    pub depth: HoleThreadDepth,
    /// Additional radial thread clearance used for modeled geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<Length>,
}

/// Thread handedness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThreadHand {
    /// Right-hand thread.
    Right,
    /// Left-hand thread.
    Left,
}

/// Axial extent rule for a hole thread.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HoleThreadDepth {
    /// Thread follows the complete hole depth.
    HoleDepth,
    /// Explicit thread length.
    Blind {
        /// Explicit axial thread length.
        depth: Length,
    },
    /// Standard tapped-hole runout is subtracted from the hole depth.
    TappedStandard,
}

/// Structural form of a hole entry treatment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HoleForm {
    /// Wider, flat-bottomed entry.
    Counterbore,
    /// Conical entry followed by a cylindrical recess.
    Counterdrill,
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
        /// Optional complete second translation direction.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        second: Option<LinearPatternDirection>,
    },
    /// Repeats seeds at explicitly located distances along a straight direction.
    LinearOffsets {
        /// Repetition direction, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
        /// Cumulative distances from the original instance, beginning with zero.
        offsets: Vec<Length>,
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
    /// Repeats seeds at explicitly located angles around an axis.
    CircularAngles {
        /// A point on the pattern axis.
        axis_origin: Point3,
        /// Unit direction of the pattern axis.
        axis_dir: Vector3,
        /// Cumulative angles from the original instance, beginning with zero.
        angles: Vec<Angle>,
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
    /// Repeats seeds using progressive uniform scales.
    Scale {
        /// Fixed locus used by every scale transform.
        center: PatternScaleCenter,
        /// Scale factor of the final instance relative to the original.
        final_factor: f64,
        /// Total number of instances, including the original.
        count: u32,
    },
    /// Applies an ordered sequence of pattern stages.
    Composite {
        /// Stages in application order.
        stages: Vec<PatternStage>,
    },
}

/// Fixed locus for a progressive pattern scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PatternScaleCenter {
    /// Volume centroid of the first seed feature.
    FirstSeedCentroid,
    /// Explicit model-space point.
    Point(Point3),
    /// Format-native center reference.
    Native(String),
}

/// One stage of an ordered composite pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PatternStage {
    /// Pattern transform sequence contributed by this stage.
    pub pattern: Box<PatternKind>,
    /// Rule used to combine this stage with preceding stages.
    pub combination: PatternStageCombination,
}

/// Combination rule for a composite-pattern stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PatternStageCombination {
    /// Establishes the initial transform sequence.
    Initialize,
    /// Applies each new transform to every preceding transform.
    CartesianProduct,
    /// Aligns transforms with equally sized slices of preceding occurrences.
    AlignedSlices,
}

/// Complete secondary direction of a two-direction linear pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LinearPatternDirection {
    /// Unit translation direction.
    pub direction: Vector3,
    /// Distance between consecutive instances.
    pub spacing: Length,
    /// Total number of instances, including the original.
    pub count: u32,
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
    /// Progressive uniform scaling.
    Scale,
    /// Ordered composition of multiple pattern forms.
    Composite,
}
