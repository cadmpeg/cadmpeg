// SPDX-License-Identifier: Apache-2.0
//! Geometry carriers: analytic surfaces, analytic curves, NURBS, and pcurves.
//!
//! Carriers are stored in their own arenas and referenced by id from the
//! topology graph (a face references a [`Surface`], an edge a [`Curve`], a
//! coedge a [`Pcurve`]). One carrier may therefore support several topological
//! entities.

use crate::ids::{CurveId, PcurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
use crate::math::{Point2, Point3, Vector3};
use crate::provenance::SourceObjectAssociation;
use crate::transform::Transform;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A tensor-product NURBS surface.
///
/// Control points use u-major order. `weights == None` denotes a non-rational
/// surface. Validation checks knot, count, control-point, and weight lengths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NurbsSurface {
    /// Degree in the u parametric direction.
    pub u_degree: u32,
    /// Degree in the v parametric direction.
    pub v_degree: u32,
    /// Full knot vector in u.
    pub u_knots: Vec<f64>,
    /// Full knot vector in v.
    pub v_knots: Vec<f64>,
    /// Number of control points along u (poles per row).
    pub u_count: u32,
    /// Number of control points along v (poles per column).
    pub v_count: u32,
    /// Control points, u-major: index `i*v_count + j` is pole `(i, j)`.
    pub control_points: Vec<Point3>,
    /// Per-pole weights in control-point order; `None` denotes non-rational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<Vec<f64>>,
    /// Whether the surface is periodic in u.
    pub u_periodic: bool,
    /// Whether the surface is periodic in v.
    pub v_periodic: bool,
}

/// A NURBS curve knot/pole payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NurbsCurve {
    /// Curve degree.
    pub degree: u32,
    /// Full knot vector.
    pub knots: Vec<f64>,
    /// Control points in parameter order.
    pub control_points: Vec<Point3>,
    /// Per-pole weights; `None` denotes non-rational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<Vec<f64>>,
    /// Whether the curve is periodic.
    pub periodic: bool,
}

/// Analytic, NURBS, or opaque surface geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SurfaceGeometry {
    /// Infinite plane through `origin` with the given `normal`.
    Plane {
        /// A point on the plane.
        origin: Point3,
        /// Plane normal (unit in well-formed IR).
        normal: Vector3,
        /// Positive-u direction in the plane.
        u_axis: Vector3,
    },
    /// Right circular cylinder of the given `radius` about the axis line.
    Cylinder {
        /// A point on the axis.
        origin: Point3,
        /// Axis direction (unit).
        axis: Vector3,
        /// Zero-azimuth direction perpendicular to `axis`.
        ref_direction: Vector3,
        /// Cylinder radius, in the document's length unit.
        radius: f64,
    },
    /// Right elliptical cone. `radius` is the major radius at `origin`;
    /// `ratio` is the minor-to-major radius ratio; `half_angle` is the major
    /// half-angle between the axis and the cone surface, in radians.
    Cone {
        /// Reference point on the axis where `radius` is measured.
        origin: Point3,
        /// Axis direction (unit).
        axis: Vector3,
        /// Zero-azimuth direction perpendicular to `axis`.
        ref_direction: Vector3,
        /// Radius at `origin`.
        radius: f64,
        /// Minor-to-major radius ratio.
        ratio: f64,
        /// Half-angle in radians.
        half_angle: f64,
    },
    /// Sphere.
    Sphere {
        /// Sphere center.
        center: Point3,
        /// Polar axis.
        axis: Vector3,
        /// Zero-azimuth direction perpendicular to `axis`.
        ref_direction: Vector3,
        /// Radius.
        radius: f64,
    },
    /// Torus. `major_radius` is the distance from `center` to the tube center;
    /// `minor_radius` is the tube radius.
    Torus {
        /// Torus center.
        center: Point3,
        /// Axis of revolution (unit).
        axis: Vector3,
        /// Zero-azimuth direction perpendicular to `axis`.
        ref_direction: Vector3,
        /// Major radius.
        major_radius: f64,
        /// Minor (tube) radius.
        minor_radius: f64,
    },
    /// Free-form NURBS surface.
    Nurbs(NurbsSurface),
    /// Exact surface defined by a procedural construction in the same model.
    Procedural {
        /// Construction that produces this carrier.
        construction: ProceduralSurfaceId,
    },
    /// Source-native polygonal surface with an explicit chordal error bound.
    Polygonal {
        /// Ordered model-space vertices.
        vertices: Vec<Point3>,
        /// Zero-based triangle indices into `vertices`.
        triangles: Vec<[u32; 3]>,
        /// Maximum chordal deviation recorded by the source.
        chordal_deflection: f64,
    },
    /// Exact affine placement of an inline basis surface.
    Transformed {
        /// Unplaced basis geometry with unchanged parameterization.
        basis: Box<SurfaceGeometry>,
        /// Affine map from basis coordinates to model coordinates.
        transform: Transform,
    },
    /// Surface geometry that has no typed neutral representation.
    ///
    /// `record` links to retained source bytes when available.
    ///
    /// A [`Surface`] carrying this variant should have entity exactness
    /// [`Exactness::Unknown`](crate::provenance::Exactness::Unknown) in the
    /// document's [`Annotations`](crate::annotations::Annotations): the shape was
    /// not established, so nothing about it is byte-exact or derived.
    Unknown {
        /// Link to the preserved raw record, when the decoder kept the bytes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
}

/// An identified surface carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Surface {
    /// Arena id.
    pub id: SurfaceId,
    /// Surface shape.
    pub geometry: SurfaceGeometry,
    /// Native source-object identity and effective display metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object: Option<SourceObjectAssociation>,
}

/// The analytic or free-form shape of a 3D curve carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveGeometry {
    /// Infinite line.
    Line {
        /// Point on the line.
        origin: Point3,
        /// Unit direction.
        direction: Vector3,
    },
    /// Full circle.
    Circle {
        /// Center.
        center: Point3,
        /// Plane normal.
        axis: Vector3,
        /// Zero-angle direction perpendicular to `axis`.
        ref_direction: Vector3,
        /// Radius.
        radius: f64,
    },
    /// Ellipse.
    Ellipse {
        /// Center.
        center: Point3,
        /// Plane normal.
        axis: Vector3,
        /// Major-axis direction.
        major_direction: Vector3,
        /// Semi-major radius.
        major_radius: f64,
        /// Semi-minor radius.
        minor_radius: f64,
    },
    /// Parabola in STEP conic form.
    Parabola {
        /// Vertex.
        vertex: Point3,
        /// Plane normal.
        axis: Vector3,
        /// Major direction.
        major_direction: Vector3,
        /// Focus distance.
        focal_distance: f64,
    },
    /// Hyperbola in STEP conic form.
    Hyperbola {
        /// Center.
        center: Point3,
        /// Plane normal.
        axis: Vector3,
        /// Transverse-axis direction.
        major_direction: Vector3,
        /// Semi-transverse radius.
        major_radius: f64,
        /// Semi-conjugate radius.
        minor_radius: f64,
    },
    /// A curve collapsed to one model-space point at a topological singularity.
    Degenerate {
        /// The collapsed curve point.
        point: Point3,
    },
    /// Ordered child curves joined into one bounded carrier.
    Composite {
        /// Ordered curve uses and their continuity contracts.
        segments: Vec<CompositeCurveSegment>,
        /// Whether the source classifies the complete curve as self-intersecting.
        self_intersect: Option<bool>,
    },
    /// Free-form NURBS curve.
    Nurbs(NurbsCurve),
    /// Exact curve defined by a procedural construction in the same model.
    Procedural {
        /// Construction that produces this carrier.
        construction: ProceduralCurveId,
    },
    /// Source-native polyline with an explicit chordal error bound.
    Polyline {
        /// Ordered model-space samples.
        points: Vec<Point3>,
        /// Optional source parameters parallel to `points`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameters: Option<Vec<f64>>,
        /// Maximum chordal deviation recorded by the source.
        chordal_deflection: f64,
    },
    /// Exact affine placement of an inline basis curve.
    Transformed {
        /// Unplaced basis geometry with unchanged parameterization.
        basis: Box<CurveGeometry>,
        /// Affine map from basis coordinates to model coordinates.
        transform: Transform,
    },
    /// Native curve carrier whose shape is not decoded.
    Unknown {
        /// Retained native record containing the curve carrier.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
}

/// One directed child use in a composite curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompositeCurveSegment {
    /// Referenced child curve carrier.
    pub curve: CurveId,
    /// Whether the child parameter direction is retained.
    pub same_sense: bool,
    /// Required continuity from the preceding segment to this segment.
    pub transition: CompositeCurveTransition,
}

/// STEP composite-curve transition continuity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompositeCurveTransition {
    /// No positional continuity is asserted.
    Discontinuous,
    /// Positional continuity.
    Continuous,
    /// Positional and tangent continuity.
    ContSameGradient,
    /// Positional, tangent, and curvature continuity.
    ContSameGradientSameCurvature,
}

/// Derive a stable in-plane reference direction from an axis.
///
/// The least-aligned global basis axis is projected onto the plane normal to
/// `axis`, then normalized. Degenerate axes fall back to global x.
pub fn derive_reference_direction(axis: Vector3) -> Vector3 {
    let norm = axis.norm();
    if !norm.is_finite() || norm == 0.0 {
        return Vector3::new(1.0, 0.0, 0.0);
    }
    let axis = Vector3::new(axis.x / norm, axis.y / norm, axis.z / norm);
    let basis = if axis.x.abs() <= axis.y.abs() && axis.x.abs() <= axis.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if axis.y.abs() <= axis.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let dot = axis.x * basis.x + axis.y * basis.y + axis.z * basis.z;
    let projected = Vector3::new(
        basis.x - dot * axis.x,
        basis.y - dot * axis.y,
        basis.z - dot * axis.z,
    );
    let projected_norm = projected.norm();
    Vector3::new(
        projected.x / projected_norm,
        projected.y / projected_norm,
        projected.z / projected_norm,
    )
}

/// A 3D curve carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Curve {
    /// Arena id.
    pub id: CurveId,
    /// Curve shape.
    pub geometry: CurveGeometry,
    /// Native source-object identity and effective display metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object: Option<SourceObjectAssociation>,
}

/// A neutral surface construction linked to the carrier it produces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProceduralSurface {
    /// Stable construction identity.
    pub id: ProceduralSurfaceId,
    /// Surface produced by this construction.
    pub surface: SurfaceId,
    /// Neutral construction definition.
    pub definition: ProceduralSurfaceDefinition,
    /// Fit contract for the solved cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_fit_tolerance: Option<f64>,
    /// Four optional U/V parameter bounds following the record's subtype
    /// scope. `None` when the record stores no bound fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_bounds: Option<[Option<f64>; 4]>,
}

/// Parameter fields carried by exact and loft spline-surface constructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SplineSurfaceParameters {
    /// Ordered semantic U and V intervals in the legacy layout.
    OrderedRanges {
        /// Ordered U and V intervals.
        ranges: [[f64; 2]; 2],
    },
    /// Four optional native scalar fields in a revision-gated layout.
    RevisionValues {
        /// Values in serialized field order; `None` is a false presence flag.
        values: [Option<f64>; 4],
    },
}

/// Neutral semantics for a procedural surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProceduralSurfaceDefinition {
    /// Exact native NURBS surface with retained parameter fields.
    Exact {
        /// Legacy ordered ranges or revision-native scalar values.
        parameters: SplineSurfaceParameters,
        /// Native ASM extension integer following the intervals.
        extension: i64,
        /// Revision-gated form fields; absent from the pre-revision layout.
        /// The revision layout stores the shared tail before `parameters`
        /// and the extension as an enum.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_form: Option<RevisionSurfaceForm>,
    },
    /// Ordered native compound of a solved surface and component surfaces.
    Compound {
        /// One native scalar paired with each component surface.
        parameters: Vec<f64>,
        /// Ordered component surfaces.
        components: Vec<SurfaceId>,
    },
    /// Exact rectangular restriction of an embedded support surface.
    SubSurface {
        /// Embedded support surface whose parameterization is retained.
        support: SurfaceId,
        /// Ordered U and V parameter intervals.
        parameter_ranges: [[f64; 2]; 2],
    },
    /// Taper of a support surface around a reference curve.
    Taper {
        /// Base surface being tapered.
        support: SurfaceId,
        /// Reference curve on the support.
        reference: CurveId,
        /// UV curve on the support, absent for `nullbs`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pcurve: Option<PcurveGeometry>,
        /// Native taper parameter or draft magnitude.
        parameter: f64,
        /// Subtype-specific taper tail.
        taper: TaperSurfaceKind,
        /// Revision-gated form fields; absent from the pre-revision layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_form: Option<RevisionSurfaceForm>,
    },
    /// Native loft defined by two section graphs and closure contracts.
    Loft {
        /// Two ordered loft sections.
        sections: [LoftSection; 2],
        /// Legacy ordered ranges or revision-native scalar values.
        parameters: SplineSurfaceParameters,
        /// Two ordered native closure enums.
        closures: [i64; 2],
        /// Two ordered native singularity enums.
        singularities: [i64; 2],
        /// Native loft mode integer.
        mode: i64,
        /// Variable native tokens between the mode and solved cache.
        bridge: Vec<LoftBridgeToken>,
        /// Revision-gated form fields; absent from the pre-revision layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_form: Option<LoftRevisionForm>,
    },
    /// Native compound-loft construction.
    CompoundLoft {
        /// Complete native compound-loft graph.
        construction: Box<CompoundLoftConstruction>,
    },
    /// Revision-gated compound-loft construction.
    RevisionCompoundLoft {
        /// Complete native revision-gated compound-loft graph.
        construction: Box<RevisionCompoundLoftConstruction>,
    },
    /// Native scaled compound-loft construction.
    ScaledCompoundLoft {
        /// Complete native scaled compound-loft graph.
        construction: Box<ScaledCompoundLoftConstruction>,
    },
    /// Native skinned spline surface.
    Skin {
        /// Complete native skin construction graph.
        construction: Box<SkinSurfaceConstruction>,
    },
    /// Native surface defined by recursive law formulas.
    Law {
        /// Complete native law-surface construction graph.
        construction: Box<LawSurfaceConstruction>,
    },
    /// Native curve-network spline surface.
    Net {
        /// Complete native net construction graph.
        construction: Box<NetSurfaceConstruction>,
    },
    /// Native curvature-continuous two-sided blend.
    G2Blend {
        /// Complete native G2 construction graph.
        construction: Box<G2BlendConstruction>,
    },
    /// Revision-gated curvature-continuous blend in the variable-blend side
    /// layout.
    RevisionG2Blend {
        /// Complete native revision-gated G2 construction graph.
        construction: Box<RevisionG2BlendConstruction>,
    },
    /// Native variable-radius two-sided blend.
    VariableBlend {
        /// Complete native variable-blend construction graph.
        construction: Box<VariableBlendConstruction>,
    },
    /// Native vertex-blend patch.
    VertexBlend {
        /// Complete native vertex-blend construction graph.
        construction: Box<VertexBlendConstruction>,
    },
    /// Translation of a directrix along a direction.
    Extrusion {
        /// Curve swept along `direction` to form the surface.
        directrix: CurveId,
        /// Stored directrix parameter interval, when carried by the source.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter_interval: Option<[f64; 2]>,
        /// Length-bearing sweep direction, in document length units.
        direction: Vector3,
        /// Native model-space position following the sweep direction, when carried.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_position: Option<Point3>,
    },
    /// Unbounded linear sweep of a directrix.
    LinearSweep {
        /// Curve swept along `direction`.
        directrix: CurveId,
        /// Length-bearing sweep vector.
        direction: Vector3,
    },
    /// Revolution of a directrix about an axis.
    Revolution {
        /// Curve revolved about the axis to form the surface.
        directrix: CurveId,
        /// A point on the revolution axis.
        axis_origin: Point3,
        /// Unit direction of the revolution axis.
        axis_direction: Vector3,
        /// Angular start and end parameters, in radians.
        angular_interval: [f64; 2],
        /// Directrix surface-parameter start and end values, when carried by
        /// the source representation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter_interval: Option<[f64; 2]>,
        /// Whether the source parameter directions are transposed.
        transposed: bool,
        /// Revision-gated form fields; absent from the pre-revision layout.
        /// The profile curve's optional endpoints are `reference_endpoints`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_form: Option<RevisionSurfaceForm>,
    },
    /// Full revolution of a directrix about an axis.
    AxisRevolution {
        /// Curve revolved about the axis.
        directrix: CurveId,
        /// Point on the revolution axis.
        axis_origin: Point3,
        /// Unit revolution-axis direction.
        axis_direction: Vector3,
    },
    /// Sum of two ordered curves from a base point.
    Sum {
        /// First curve, varying in the first surface parameter.
        first: CurveId,
        /// Second curve, varying in the second surface parameter.
        second: CurveId,
        /// Surface base point.
        basepoint: Vector3,
        /// Revision-gated form fields; absent from the pre-revision layout.
        /// The first curve's optional endpoints are `reference_endpoints`
        /// and the second curve's are `second_endpoints`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_form: Option<RevisionSurfaceForm>,
    },
    /// Sweep of a profile along a spine.
    Sweep {
        /// Cross-section curve carried along `spine`.
        profile: CurveId,
        /// Path curve the profile is swept along.
        spine: CurveId,
        /// Complete native sweep graph when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native: Option<Box<SweepSurfaceConstruction>>,
    },
    /// T-spline face with its shared subtransform program.
    TSpline {
        /// Complete native T-spline wrapper construction.
        construction: Box<TSplineSurfaceConstruction>,
    },
    /// Surface generated along an inline circular or linear helix path.
    Helix {
        /// Complete native helix-surface construction.
        construction: Box<HelixSurfaceConstruction>,
    },
    /// Native deformable spline surface.
    Deformable {
        /// Complete decoded deformable construction.
        construction: Box<DeformableSurfaceConstruction>,
    },
    /// Offset from a support surface.
    Offset {
        /// Surface this surface is offset from.
        support: SurfaceId,
        /// Signed offset distance, in document length units.
        distance: f64,
        /// Native U parameter-direction sense enum, when carried.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u_sense: Option<i64>,
        /// Native V parameter-direction sense enum, when carried.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        v_sense: Option<i64>,
        /// Ordered conditional ASM extension flags.
        extension_flags: Vec<bool>,
        /// Revision-gated form fields; absent from the pre-revision layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_form: Option<RevisionSurfaceForm>,
    },
    /// Rectangular parameter sub-range of a support surface.
    Subset {
        /// Surface being restricted.
        support: SurfaceId,
        /// Ordered U and V parameter intervals.
        parameter_ranges: [[f64; 2]; 2],
    },
    /// Parallel offset from a support surface.
    ParallelOffset {
        /// Surface being offset.
        support: SurfaceId,
        /// Signed offset distance.
        distance: f64,
        /// Whether the source classifies the result as self-intersecting.
        self_intersect: Option<bool>,
    },
    /// Self-intersecting torus with an explicitly selected outer or inner sheet.
    DegenerateTorus {
        /// Whether the outer sheet is selected at the self-intersection.
        select_outer: bool,
    },
    /// Surface domain bounded by ordered curves on a supporting surface.
    CurveBounded {
        /// Supporting surface whose parameterization defines the domain.
        support: SurfaceId,
        /// Boundary curves on the support.
        boundaries: Vec<CurveId>,
        /// Whether the support's natural outer boundary is implicit.
        implicit_outer: bool,
    },
    /// Ruled surface joining two directrices.
    Ruled {
        /// First bounding curve of the ruled surface.
        first: CurveId,
        /// Second bounding curve of the ruled surface.
        second: CurveId,
    },
    /// Rolling-ball or law-driven blend between two support surfaces.
    Blend {
        /// The two blend support sides, in side order; `None` when a side was
        /// not resolved.
        supports: [Option<BlendSupport>; 2],
        /// Stored center/spine curve, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spine: Option<CurveId>,
        /// Signed offset-radius law along the spine.
        radius: BlendRadiusLaw,
        /// Cross-section family of the blend.
        cross_section: BlendCrossSection,
        /// Complete byte-backed rolling-ball context when available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native: Option<Box<RollingBallConstruction>>,
    },
    /// Preserved construction without a neutral interpretation.
    Unknown {
        /// Reference to the preserved raw source record, when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
}

/// Structurally selected deformable-surface payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeformableSurfaceData {
    /// Mode-6 full embedded deformation payload.
    Full {
        /// Four leading deformation vectors.
        leading_vectors: [Vector3; 4],
        /// Leading deformation scalar.
        leading_parameter: f64,
        /// Three leading flags.
        leading_flags: [bool; 3],
        /// Native selector before the secondary support.
        selector: i64,
        /// Secondary embedded support surface.
        surface: SurfaceId,
        /// Native long after the support.
        native_id: i64,
        /// Native support-side flag.
        flag: bool,
        /// First scalar after the flag.
        first_parameter: f64,
        /// Version-gated ASM long when present.
        version_value: Option<i64>,
        /// Second scalar after the optional long.
        second_parameter: f64,
        /// Embedded deformation curve.
        curve: CurveId,
        /// Two ordered full vector frames.
        frames: Box<[DeformableVectorFrame; 2]>,
        /// Native trailing long.
        trailing_value: i64,
    },
    /// Mode-5 surface-and-curve deformation payload.
    SurfaceCurve {
        /// Secondary embedded support surface.
        surface: SurfaceId,
        /// Native long identifier.
        native_id: i64,
        /// Native leading flag.
        flag: bool,
        /// First native scalar.
        first_parameter: f64,
        /// Native selector integer.
        selector: i64,
        /// Second native scalar.
        second_parameter: f64,
        /// Embedded deformation curve.
        curve: CurveId,
        /// Four ordered deformation vectors.
        vectors: [Vector3; 4],
        /// Frame scalar after the vectors.
        frame_parameter: f64,
        /// Three frame flags.
        flags: [bool; 3],
        /// Counted ordered scalar triples.
        parameter_triples: Vec<[f64; 3]>,
    },
    /// Mode-1 deformation frame with counted parameter triples.
    Plain {
        /// Shared full deformation frame.
        frame: Box<DeformableSurfaceFrame>,
        /// Ordered native scalar triples.
        parameter_triples: Vec<[f64; 3]>,
    },
    /// Mode-3 deformation frame with a guide scalar.
    Guided {
        /// Shared full deformation frame.
        frame: Box<DeformableSurfaceFrame>,
        /// Native guide selector.
        selector: i64,
        /// Native guide scalar.
        guide_parameter: f64,
    },
    /// Mode-8 minimal four-vector scaffold.
    Minimal {
        /// Four ordered deformation vectors.
        vectors: [Vector3; 4],
        /// Native trailing selector.
        selector: i64,
    },
}

/// Four-vector frame used by full deformable surfaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeformableVectorFrame {
    /// Four ordered vectors.
    pub vectors: [Vector3; 4],
    /// Frame scalar.
    pub parameter: f64,
    /// Three ordered flags.
    pub flags: [bool; 3],
}

/// Shared frame payload of deformable-surface modes 1 and 3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeformableSurfaceFrame {
    /// Four leading deformation vectors.
    pub leading_vectors: [Vector3; 4],
    /// Leading frame scalar.
    pub leading_parameter: f64,
    /// Three leading frame flags.
    pub leading_flags: [bool; 3],
    /// Three secondary deformation vectors.
    pub secondary_vectors: [Vector3; 3],
    /// Secondary frame scalar.
    pub secondary_parameter: f64,
    /// Two secondary frame flags.
    pub secondary_flags: [bool; 2],
    /// Native model-space frame point.
    pub point: Point3,
    /// Five trailing frame flags.
    pub trailing_flags: [bool; 5],
}

/// Complete native deformable-surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeformableSurfaceConstruction {
    /// Surface being deformed.
    pub support: SurfaceId,
    /// Discriminator-selected deformation data.
    pub data: DeformableSurfaceData,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Inline path shared by helix curves and helix surfaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HelixPathConstruction {
    /// Native angular path interval.
    pub angle_range: [f64; 2],
    /// Axis origin at the path start.
    pub center: Point3,
    /// Major profile-radius vector.
    pub major: Vector3,
    /// Minor profile-radius vector.
    pub minor: Vector3,
    /// Axial rise vector per revolution.
    pub pitch: Vector3,
    /// Linear radial growth factor.
    pub apex_factor: f64,
    /// Unit helix axis direction.
    pub axis: Vector3,
}

/// Profile-specific tail of a helix surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HelixSurfaceProfile {
    /// Circular profile swept along the helix.
    Circle {
        /// Native length preceding the inline path.
        length: f64,
        /// Circular profile radius.
        radius: f64,
    },
    /// Linear profile swept along a direction.
    Line {
        /// Native model-space profile direction.
        direction: Vector3,
    },
}

/// Complete native helix-surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HelixSurfaceConstruction {
    /// Native surface angular interval.
    pub angle_range: [f64; 2],
    /// Native secondary interval.
    pub dimension_range: [f64; 2],
    /// Inline helix path.
    pub path: HelixPathConstruction,
    /// Circular or linear profile tail.
    pub profile: HelixSurfaceProfile,
}

/// Native T-spline subtransform storage form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TSplineSubtransform {
    /// Inline line-oriented T-spline program and companion values.
    Inline {
        /// Line-oriented topology and geometry program.
        program: String,
        /// Optional native separator boolean.
        separator: Option<bool>,
        /// Companion values program.
        values: String,
    },
    /// Reference to an earlier subtype-table entry.
    Reference {
        /// Native subtype-table index.
        index: i64,
        /// Resolved shared program when the table target is available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved: Option<Box<TSplineSubtransform>>,
    },
}

/// Complete native `t_spl_sur` wrapper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TSplineSurfaceConstruction {
    /// Ordered U and V native parameter intervals.
    pub parameter_ranges: [[f64; 2]; 2],
    /// Native T-spline type integer.
    pub type_code: i64,
    /// Inline or referenced shared subtransform object.
    pub subtransform: TSplineSubtransform,
    /// Parsed semantic index of the inline program, absent for references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_graph: Option<TSplineProgram>,
    /// Parsed semantic index of the companion values program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values_graph: Option<TSplineProgram>,
    /// Native trailing integer.
    pub trailing_value: i64,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
    /// Revision-gated form fields; absent from the pre-revision layout. The
    /// revision layout stores the shared tail first, then four optional
    /// parameter values (`support_bounds`), the type code as an enum, the
    /// nested subtransform scope, and the trailing integer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_form: Option<RevisionSurfaceForm>,
}

/// Parsed line-oriented T-spline subtransform program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TSplineProgram {
    /// Ordered recognized header declarations.
    pub headers: Vec<TSplineProgramLine>,
    /// Ordered recognized topology, geometry, and constraint records.
    pub records: Vec<TSplineProgramLine>,
    /// Non-comment lines outside the defined vocabulary.
    pub unparsed_lines: Vec<String>,
}

/// One tokenized T-spline program line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TSplineProgramLine {
    /// Leading record or header token.
    pub kind: String,
    /// Ordered remaining fields without interpretation loss.
    pub fields: Vec<String>,
}

impl TSplineProgram {
    /// Parse the defined line vocabulary while retaining every other line.
    #[must_use]
    pub fn parse(program: &str) -> Self {
        const HEADERS: &[&str] = &[
            "degree",
            "cap_type",
            "units",
            "end_conditions",
            "star_knot_rule",
            "star_smoothness",
            "tol",
            "ver",
            "behavior_version",
            "geom_tol",
            "compat_version",
        ];
        const RECORDS: &[&str] = &[
            "f",
            "e",
            "v",
            "l",
            "ec",
            "0m",
            "0g",
            "100edges",
            "100verts",
            "105sym",
            "105plane",
            "105a",
            "106ek",
            "50000grip",
        ];
        let mut parsed = Self {
            headers: Vec::new(),
            records: Vec::new(),
            unparsed_lines: Vec::new(),
        };
        for line in program.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split_whitespace();
            let Some(kind) = fields.next() else { continue };
            let parsed_line = TSplineProgramLine {
                kind: kind.into(),
                fields: fields.map(String::from).collect(),
            };
            if HEADERS.contains(&kind) {
                parsed.headers.push(parsed_line);
            } else if RECORDS.contains(&kind) {
                parsed.records.push(parsed_line);
            } else {
                parsed.unparsed_lines.push(line.into());
            }
        }
        parsed
    }
}

/// One oriented support of a procedural blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BlendSupport {
    /// The support surface.
    pub surface: SurfaceId,
    /// Selects the opposite surface-normal side when true.
    #[serde(default)]
    pub reversed: bool,
}

/// Cross-section family of a procedural blend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlendCrossSection {
    /// Constant-radius circular cross-section.
    Circular,
    /// Conic (non-circular quadric) cross-section.
    Conic,
    /// Free-form polynomial cross-section.
    Polynomial,
}

/// Shared fields of a revision-gated spline-surface form: the revision
/// integer, optional support bounds and reference-curve endpoints, a
/// carrier-specific boolean run, and the shared tail enum, discontinuity
/// arrays, tail boolean, and post-tail boolean run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct RevisionSurfaceForm {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Optional U/V bound fields following the support surface.
    #[serde(default)]
    pub support_bounds: [Option<f64>; 4],
    /// Optional parameter endpoints following the embedded reference curve.
    #[serde(default)]
    pub reference_endpoints: [Option<f64>; 2],
    /// Optional parameter endpoints following a second embedded curve, used
    /// by two-curve carriers such as `sum_spl_sur`.
    #[serde(default)]
    pub second_endpoints: [Option<f64>; 2],
    /// Carrier-specific boolean run preceding the shared tail.
    #[serde(default)]
    pub flags: Vec<bool>,
    /// Enum opening the shared revision-gated surface tail.
    pub tail_enum: i64,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
    /// Boolean run following the shared tail.
    #[serde(default)]
    pub trailing_flags: Vec<bool>,
}

/// Subtype-specific tail of a native taper spline surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaperSurfaceKind {
    /// Standard taper without a subtype-specific tail.
    Standard,
    /// Orthogonal taper with a native sense flag.
    Orthogonal {
        /// Native orientation sense.
        sense: bool,
    },
    /// Edge taper with a model-space draft vector.
    Edge {
        /// Native draft vector.
        draft: Vector3,
    },
    /// Shadow taper with a pre-factored draft angle.
    Shadow {
        /// Native draft vector.
        draft: Vector3,
        /// Stored draft-angle sine.
        sine: f64,
        /// Stored draft-angle cosine.
        cosine: f64,
    },
    /// Ruled taper with a pre-factored angle and factor.
    Ruled {
        /// Native draft vector.
        draft: Vector3,
        /// Stored draft-angle sine.
        sine: f64,
        /// Stored draft-angle cosine.
        cosine: f64,
        /// Native ruled-taper factor.
        factor: f64,
    },
    /// Swept taper with a pre-factored draft angle.
    Swept {
        /// Native draft vector.
        draft: Vector3,
        /// Stored draft-angle sine.
        sine: f64,
        /// Stored draft-angle cosine.
        cosine: f64,
    },
}

/// One scalar row in native loft subdata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoftSubdataRow {
    /// Leading ordered scalar pair.
    pub parameters: [f64; 2],
    /// Ordered per-column scalar pairs; empty for subdata type 211.
    pub columns: Vec<[f64; 2]>,
    /// Trailing scalar pair stored by the revision-gated row encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<[f64; 2]>,
}

/// Native loft constraint table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoftSubdata {
    /// Native table type discriminator.
    pub type_code: i64,
    /// Serialized row count.
    pub row_count: i64,
    /// Serialized per-row column count.
    pub column_count: i64,
    /// Ordered decoded rows.
    pub rows: Vec<LoftSubdataRow>,
}

/// Surface-side constraint attached to one loft profile curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoftProfileData {
    /// Constraint support surface, absent for the native `null_surface`
    /// sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<SurfaceId>,
    /// Optional U/V bound fields following the support surface in the
    /// revision-gated encoding.
    #[serde(default)]
    pub support_bounds: [Option<f64>; 4],
    /// UV curve on the support, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcurve: Option<PcurveGeometry>,
    /// First native constraint flag.
    pub first_flag: bool,
    /// ASM extension integer following the first flag.
    pub asm_extension: i64,
    /// Native constraint table.
    pub subdata: LoftSubdata,
    /// Optional direction selected by the second native flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Vector3>,
}

/// One curve member of a loft profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoftProfileMember {
    /// Native member type discriminator.
    pub type_code: i64,
    /// Profile curve.
    pub curve: CurveId,
    /// Optional parameter endpoints following the curve in the revision-gated
    /// encoding; absent from the pre-revision encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<[Option<f64>; 2]>,
    /// Surface-side constraint data.
    pub data: LoftProfileData,
}

/// Native path data attached to one loft section entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoftPath {
    /// Primary path curve, absent for the native `null_curve` sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<CurveId>,
    /// Optional parameter endpoints following a present curve in the
    /// revision-gated encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<[Option<f64>; 2]>,
    /// Ordered auxiliary BS3 curves.
    pub auxiliaries: Vec<CurveId>,
    /// Native path tail integer.
    pub flag: i64,
}

/// One parameterized entry in a native loft section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoftSectionEntry {
    /// Native section parameter.
    pub parameter: f64,
    /// Ordered profile members.
    pub profile: Vec<LoftProfileMember>,
    /// Native path data.
    pub path: LoftPath,
}

/// Revision-gated `loft_spl_sur` form fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct LoftRevisionForm {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Four booleans following the parameter intervals.
    #[serde(default)]
    pub flags: [bool; 4],
    /// Two integers preceding the shared tail.
    #[serde(default)]
    pub ints: [i64; 2],
    /// Enum opening the shared revision-gated surface tail.
    pub tail_enum: i64,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
}

/// Ordered native loft section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoftSection {
    /// Ordered entries in the section.
    pub entries: Vec<LoftSectionEntry>,
}

/// Token retained from the variable bridge preceding a loft solved cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LoftBridgeToken {
    /// Native boolean token.
    Boolean(bool),
    /// Native integer token.
    Integer(i64),
    /// Native double token.
    Double(f64),
    /// Native string token.
    Text(String),
    /// Native enum token.
    Enum(i64),
}

/// Common carrier fields of one G2 blend side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct G2BlendSide {
    /// Native side label.
    pub label: String,
    /// Primary support surface.
    pub surface: SurfaceId,
    /// Primary side curve.
    pub curve: CurveId,
    /// First and second ordered BS2 pcurves; each may be `nullbs`.
    pub pcurves: [Option<PcurveGeometry>; 2],
    /// Native side direction.
    pub direction: Vector3,
}

/// Singularity-specific payload of the first G2 blend side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum G2BlendFirstShape {
    /// Full singularity with an optional BS3 support surface.
    Full {
        /// Optional exact BS3 support surface.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        surface: Option<SurfaceId>,
        /// Fit tolerance present exactly when `surface` is present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tolerance: Option<f64>,
    },
    /// Non-singular nine-scalar frame and tertiary pcurve.
    None {
        /// Ordered native frame scalars.
        coefficients: [f64; 9],
        /// Native fit tolerance.
        tolerance: f64,
        /// Optional intervening native token.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extension: Option<LoftBridgeToken>,
        /// Tertiary BS2 pcurve, absent for `nullbs`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pcurve: Option<PcurveGeometry>,
    },
}

/// Full native G2 blend construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct G2BlendConstruction {
    /// First side common fields.
    pub first: G2BlendSide,
    /// Native first-side singularity enum.
    pub singularity: i64,
    /// First-side singularity payload.
    pub first_shape: G2BlendFirstShape,
    /// Second side common fields.
    pub second: G2BlendSide,
    /// Exact second-side spline support.
    pub second_exact_surface: SurfaceId,
    /// Center or transition curve.
    pub center_curve: CurveId,
    /// Ordered center-curve scalars.
    pub center_parameters: [f64; 2],
    /// Native center tail integer.
    pub center_flag: i64,
    /// Native U and V intervals.
    pub parameter_ranges: [[f64; 2]; 2],
    /// Four ordered trailing scalars.
    pub trailing_parameters: [f64; 4],
    /// Three ordered ASM discontinuity arrays.
    pub discontinuities: [Vec<f64>; 3],
}

/// One complete native rolling-ball support side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RollingBallSide {
    /// Geometry role selected by the support-side discriminator.
    pub support_kind: VariableBlendSupportKind,
    /// Primary support surface, absent for `null_surface`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<SurfaceId>,
    /// Optional native support-surface U and V endpoints.
    #[serde(default)]
    pub surface_ranges: [[Option<f64>; 2]; 2],
    /// Side curve, absent for `null_curve`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<CurveId>,
    /// Optional native side-curve parameter endpoints.
    #[serde(default)]
    pub curve_range: [Option<f64>; 2],
    /// Primary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcurve: Option<PcurveGeometry>,
    /// Native model-space side location.
    pub location: Point3,
    /// ASM secondary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_pcurve: Option<PcurveGeometry>,
    /// Native extension integer between the secondary and tertiary pcurves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<i64>,
    /// ASM tertiary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tertiary_pcurve: Option<PcurveGeometry>,
}

/// Third support graph appended by `sss_blend_spl_sur`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RollingBallThirdSide {
    /// Native side label.
    pub label: String,
    /// Third support surface.
    pub surface: SurfaceId,
    /// Third side curve.
    pub curve: CurveId,
    /// Primary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcurve: Option<PcurveGeometry>,
    /// Native side vector.
    pub direction: Vector3,
    /// ASM secondary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_pcurve: Option<PcurveGeometry>,
    /// Native ASM integer following the secondary pcurve.
    pub extension: i64,
    /// ASM tertiary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tertiary_pcurve: Option<PcurveGeometry>,
    /// Final ASM flag.
    pub flag: bool,
}

/// Native optional-radius selector in a rolling-ball construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RollingBallRadiusSelector {
    /// Native `-1` no-radius sentinel.
    None,
    /// Explicit native selector scalar.
    Value {
        /// Stored scalar value.
        value: f64,
    },
}

/// Complete byte-backed rolling-ball or three-surface blend context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RollingBallConstruction {
    /// Native subtype definition-table index.
    pub definition_index: i64,
    /// Two ordered primary support sides.
    pub sides: Box<[RollingBallSide; 2]>,
    /// Stored slice or center curve.
    pub slice: CurveId,
    /// Optional native slice-curve parameter endpoints.
    #[serde(default)]
    pub slice_range: [Option<f64>; 2],
    /// Two signed support offsets in document length units.
    pub offsets: [f64; 2],
    /// Optional-radius selector field.
    pub radius_selector: RollingBallRadiusSelector,
    /// Native optional U interval endpoints.
    pub u_range: [Option<f64>; 2],
    /// Native optional V interval endpoints.
    pub v_range: [Option<f64>; 2],
    /// Native integer preceding the trailing scalars.
    pub shape_prefix: i64,
    /// Two ordered trailing scalars.
    pub parameters: [f64; 2],
    /// Native long following the trailing scalars.
    pub tail: i64,
    /// Native selector preceding the solved surface cache.
    pub cache_selector: i64,
    /// Three ordered ASM discontinuity arrays.
    pub discontinuities: [Vec<f64>; 3],
    /// Third side present only for `sss_blend_spl_sur`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub third: Option<Box<RollingBallThirdSide>>,
}

/// Geometry role selected by a variable-blend support-side discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VariableBlendSupportKind {
    /// Support defined by a cosine curve.
    CosineCurve,
    /// Support defined by a general curve.
    Curve,
    /// Support defined by a point curve.
    PointCurve,
    /// Support defined by a surface.
    Surface,
    /// Support defined by a zero curve.
    ZeroCurve,
}

/// Convexity selected for a variable-radius blend surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VariableBlendConvexity {
    /// The blend bends toward the support intersection.
    Convex,
    /// The blend bends away from the support intersection.
    Concave,
}

/// Solved-surface representation selected for a variable-radius blend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VariableBlendRenderMode {
    /// The solved surface is the rolling-ball envelope.
    RollingBallEnvelope,
    /// The solved surface is a rolling-ball snapshot.
    RollingBallSnapshot,
}

/// Number of independently controlled radii in a variable blend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VariableBlendRadiusKind {
    /// One radius law controls both support sides.
    SingleRadius,
    /// Each support side has an independent radius law.
    TwoRadii,
}

/// One interpolation control point in a variable blend-value law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VariableBlendInterpolationPoint {
    /// Law parameter.
    pub parameter: f64,
    /// Radius in document length units.
    pub radius: f64,
    /// Two ordered tangent scalars.
    pub tangents: [f64; 2],
    /// Model-space control location.
    pub location: Point3,
    /// Control normal.
    pub normal: Vector3,
}

/// Complete recursive native `getBlendValues` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VariableBlendValue {
    /// Native blend-value type name.
    pub name: String,
    /// Native Boolean following the calibrated enum.
    pub modern_flag: bool,
    /// Native sub-discriminator preceding the calibrated enum.
    pub discriminator: i64,
    /// Native calibrated enum.
    pub calibrated: i64,
    /// Type-specific payload.
    pub payload: VariableBlendValuePayload,
}

/// Type-specific payload of a variable blend value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VariableBlendValuePayload {
    /// Two endpoint parameters and radii.
    TwoEnds {
        /// Endpoint parameters.
        parameters: [f64; 2],
        /// Endpoint radii in document length units.
        radii: [f64; 2],
    },
    /// Fixed-width branch: two endpoint parameters and one native width
    /// scalar, stored unscaled.
    FixedWidth {
        /// Endpoint parameters.
        parameters: [f64; 2],
        /// Native width scalar.
        width: f64,
    },
    /// Edge-offset branch.
    EdgeOffset {
        /// Ordered native scalar payload.
        scalars: Vec<f64>,
        /// Ordered length payload in document units.
        lengths: Vec<f64>,
    },
    /// Functional radius law carried by a BS2 pcurve.
    Functional {
        /// Leading scalar.
        parameter: f64,
        /// Leading length in document units.
        radius: f64,
        /// Parametric `(u, radius)` function.
        function: PcurveGeometry,
        /// Numeric or symbolic terminal value.
        terminal: LoftBridgeToken,
    },
    /// Constant law followed by a recursive chamfer value.
    Constant {
        /// Ordered native scalars.
        parameters: [f64; 2],
        /// Radius in document length units.
        radius: f64,
        /// Native variable-chamfer enum.
        variable_chamfer: i64,
        /// Native chamfer-type enum.
        chamfer_type: i64,
        /// Recursively nested blend value.
        nested: Box<VariableBlendValue>,
    },
    /// Interpolated radius law.
    Interpolated {
        /// Leading scalar.
        parameter: f64,
        /// Leading radius in document length units.
        radius: f64,
        /// Parametric support curve.
        function: PcurveGeometry,
        /// Native interpolation enum count.
        enum_count: i64,
        /// Whether the enum count and tail flag are stored as `0x15` enum
        /// tokens (revision-gated streams) rather than `0x04` integers.
        #[serde(default)]
        enum_tagged: bool,
        /// Ordered interpolation controls.
        points: Vec<VariableBlendInterpolationPoint>,
        /// Optional two-scalar tail selected by a nonzero flag.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tail: Option<[f64; 2]>,
    },
}

/// Optional single-radius tail selected by the native radius-kind branch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VariableBlendSingleRadiusTail {
    /// Native symbolic or numeric selector.
    pub selector: LoftBridgeToken,
    /// Two ordered scalars following the selector.
    pub parameters: [f64; 2],
}

/// Variable-blend chamfer form selected by the two-radii branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VariableBlendChamferKind {
    /// A rounded transition controlled by a third blend-value law.
    Rounded,
}

/// Optional rounded-chamfer branch following two radius laws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VariableBlendChamfer {
    /// Variable-chamfer form.
    pub kind: VariableBlendChamferKind,
    /// Native chamfer-type enum.
    pub chamfer_type: i64,
    /// Chamfer blend-value payload.
    pub value: VariableBlendValue,
}

/// Complete native variable-radius blend construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VariableBlendConstruction {
    /// Native serializer-revision integer following the subtype name.
    #[serde(alias = "definition_index")]
    pub revision: i64,
    /// Two ordered support-side graphs in the rolling-ball side layout.
    pub sides: Box<[RollingBallSide; 2]>,
    /// Stored slice curve.
    pub slice: CurveId,
    /// Optional native slice-curve parameter endpoints.
    #[serde(default)]
    pub slice_range: [Option<f64>; 2],
    /// Two signed support offsets in document length units.
    pub offsets: [f64; 2],
    /// Radius-control cardinality.
    pub radius_kind: VariableBlendRadiusKind,
    /// First radius-control payload.
    pub first_value: VariableBlendValue,
    /// Second radius-control payload for a two-radii construction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_value: Option<VariableBlendValue>,
    /// Chamfer-selector enum stored after the second radius value of a
    /// two-radii blend. `0` selects no chamfer, `3` selects the rounded
    /// chamfer carried in `chamfer`. `None` when the source stored no
    /// selector token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamfer_selector: Option<i64>,
    /// Optional rounded-chamfer payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamfer: Option<Box<VariableBlendChamfer>>,
    /// Selector enum stored after the radius value of a single-radius blend.
    /// `0` selects no further fields; `1` and `7` select the two scalars in
    /// `single_radius_tail`. `None` when the source stored no selector token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_radius_selector: Option<i64>,
    /// Optional single-radius selector tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_radius_tail: Option<VariableBlendSingleRadiusTail>,
    /// Native optional U interval endpoints.
    pub u_range: [Option<f64>; 2],
    /// Native optional V interval endpoints.
    pub v_range: [Option<f64>; 2],
    /// Native integer before the solved shape.
    pub shape_prefix: i64,
    /// Native scalar before the solved shape.
    pub shape_parameter: f64,
    /// Native length before the solved shape, in document units.
    pub shape_length: f64,
    /// Native integer immediately before the cache selector.
    pub shape_tail: i64,
    /// Native selector preceding the solved surface cache.
    pub cache_selector: i64,
    /// Six ordered ASM discontinuity arrays following the fit tolerance.
    pub discontinuities: [Vec<f64>; 6],
    /// Native Boolean following the discontinuity arrays.
    pub tail_flag: bool,
    /// Three ASM integers following the tail Boolean.
    pub tail_extensions: [i64; 3],
    /// Secondary curve following the tail extensions, absent for `null_curve`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_curve: Option<CurveId>,
    /// Optional native secondary-curve parameter endpoints.
    #[serde(default)]
    pub secondary_range: [Option<f64>; 2],
    /// Blend convexity.
    pub convexity: VariableBlendConvexity,
    /// Solved-surface representation.
    pub render_mode: VariableBlendRenderMode,
    /// Native optional post-shape interval endpoints.
    pub post_range: [Option<f64>; 2],
    /// Native post-shape BS3 curve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_curve: Option<CurveId>,
    /// Native post-shape BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_pcurve: Option<PcurveGeometry>,
}

/// Complete native revision-gated `g2_blend_spl_sur` construction. The
/// revision layout stores the two support sides in the variable-blend side
/// layout and ends with the shared revision-gated surface tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RevisionG2BlendConstruction {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Two native scalars following the revision integer.
    pub leading_parameters: [f64; 2],
    /// Two ordered support-side graphs in the variable-blend side layout.
    pub sides: Box<[RollingBallSide; 2]>,
    /// Stored center curve.
    pub center: CurveId,
    /// Optional native center-curve parameter endpoints.
    #[serde(default)]
    pub center_range: [Option<f64>; 2],
    /// Two signed blend radii in document length units.
    pub radii: [f64; 2],
    /// Radius-selector enum following the radii; `-1` selects the
    /// absent-radius branch.
    pub radius_selector: i64,
    /// Native optional U interval endpoints.
    pub u_range: [Option<f64>; 2],
    /// Native optional V interval endpoints.
    pub v_range: [Option<f64>; 2],
    /// Native integer before the solved shape.
    pub shape_prefix: i64,
    /// Native scalar before the solved shape.
    pub shape_parameter: f64,
    /// Native length before the solved shape, in document units.
    pub shape_length: f64,
    /// Native integer immediately before the shared tail.
    pub shape_tail: i64,
    /// Enum opening the shared revision-gated surface tail.
    pub tail_enum: i64,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
    /// Three ASM integers following the shared tail.
    pub tail_extensions: [i64; 3],
}

/// Complete native revision-gated `cl_loft_spl_sur` construction. The
/// revision layout is cache-first: the revision integer and shared
/// revision-gated surface tail precede the construction fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RevisionCompoundLoftConstruction {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Enum opening the shared revision-gated surface tail.
    pub tail_enum: i64,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
    /// Leading unparameterized scale block: ordered profile members and path.
    pub base_profile: Vec<LoftProfileMember>,
    /// Path data of the leading scale block.
    pub base_path: LoftPath,
    /// Counted parameterized entries; the native parameter trails each
    /// entry's fields.
    pub entries: Vec<LoftSectionEntry>,
    /// Two flags following the entries.
    pub flags: [bool; 2],
    /// Tail-kind integer following the two flags.
    pub kind: i64,
    /// Two flags opening the kind-zero payload.
    pub kind_flags: [bool; 2],
    /// Kind-zero direction selector.
    pub selector: i64,
    /// Selector-zero direction vector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Vector3>,
    /// Selector-nonzero BS3 direction curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction_curve: Option<CurveId>,
    /// Two optional trailing parameter values.
    #[serde(default)]
    pub interval: [Option<f64>; 2],
    /// Optional trailing BS3 curve closing the payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trailing_curve: Option<CurveId>,
}

/// One boundary record in a native vertex-blend patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VertexBlendBoundary {
    /// Native boundary type enum.
    pub boundary_type: i64,
    /// Native model-space magic location.
    pub magic: Point3,
    /// Native U-smoothing enum.
    pub u_smoothing: i64,
    /// Native V-smoothing enum.
    pub v_smoothing: i64,
    /// Native fullness scalar.
    pub fullness: f64,
    /// Structurally selected boundary geometry.
    pub geometry: VertexBlendBoundaryGeometry,
}

/// Type-specific geometry of a vertex-blend boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VertexBlendBoundaryGeometry {
    /// Curve boundary with a circle/ellipse/unknown twist form.
    Circle {
        /// Boundary curve.
        curve: CurveId,
        /// Optional native curve parameter endpoints stored by the
        /// revision-gated layout.
        #[serde(default)]
        curve_endpoints: [Option<f64>; 2],
        /// Native circle-form enum.
        form: i64,
        /// Zero, one, or two twist entries selected by `form`. Pre-revision
        /// layouts store model-space locations; the revision-gated layout
        /// stores unscaled twist vectors.
        twists: Vec<Point3>,
        /// Two ordered curve parameters.
        parameters: [f64; 2],
        /// Native sense enum.
        sense: i64,
    },
    /// Degenerate boundary at a model-space location.
    Degenerate {
        /// Degenerate location.
        location: Point3,
        /// Two ordered boundary normals.
        normals: [Vector3; 2],
    },
    /// Surface pcurve boundary.
    Pcurve {
        /// Support surface.
        surface: SurfaceId,
        /// Optional U/V bound fields stored after the support by the
        /// revision-gated layout.
        #[serde(default)]
        support_bounds: [Option<f64>; 4],
        /// Native BS2 pcurve, absent for `nullbs`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pcurve: Option<PcurveGeometry>,
        /// Native sense enum.
        sense: i64,
        /// Parameter-space fit tolerance.
        fit_tolerance: f64,
    },
    /// Planar boundary described by a normal and curve.
    Plane {
        /// Plane normal.
        normal: Vector3,
        /// Two ordered plane parameters.
        parameters: [f64; 2],
        /// Boundary curve.
        curve: CurveId,
        /// Optional native curve parameter endpoints stored by the
        /// revision-gated layout.
        #[serde(default)]
        curve_endpoints: [Option<f64>; 2],
    },
}

/// Complete native vertex-blend surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VertexBlendConstruction {
    /// Positive serializer-revision integer selecting the revision-gated
    /// layout; absent from the pre-revision layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    /// Ordered boundary records.
    pub boundaries: Vec<VertexBlendBoundary>,
    /// Native grid-size integer.
    pub grid_size: i64,
    /// Native model-space fit tolerance.
    pub fit_tolerance: f64,
}

/// One member of a compound-loft scale block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompoundLoftScaleMember {
    /// Native member integer.
    pub type_code: i64,
    /// Member curve.
    pub curve: CurveId,
    /// Native loft constraint data.
    pub data: LoftProfileData,
}

/// Complete `_readScaleClLoft` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompoundLoftScale {
    /// Ordered scale members.
    pub members: Vec<CompoundLoftScaleMember>,
    /// Scale path curve.
    pub path: CurveId,
    /// Ordered BS3 auxiliary curves.
    pub auxiliaries: Vec<CurveId>,
    /// Two native trailing integers.
    pub tail: [i64; 2],
}

/// Direction carrier in the zero-kind compound-loft tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompoundLoftDirection {
    /// Inline direction vector when the selector is zero.
    Vector {
        /// Stored direction.
        value: Vector3,
    },
    /// BS3 direction curve when the selector is nonzero.
    Curve {
        /// Stored curve.
        curve: CurveId,
    },
}

/// Structurally selected tail of `cl_loft_spl_sur`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompoundLoftTail {
    /// Native kind `6` tail.
    Six {
        /// Two leading flags.
        flags: [bool; 2],
        /// Required scale block.
        scale: Box<CompoundLoftScale>,
        /// Native integer following the scale.
        selector: i64,
        /// Stored direction.
        direction: Vector3,
        /// Native parameter interval.
        parameter_range: [f64; 2],
        /// BS3 tail curve.
        curve: CurveId,
    },
    /// Native kind `7` tail.
    Seven {
        /// First flag.
        first_flag: bool,
        /// First optional scale block.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_scale: Option<Box<CompoundLoftScale>>,
        /// Second flag.
        second_flag: bool,
        /// Required second scale block.
        second_scale: Box<CompoundLoftScale>,
        /// Native selector integer.
        selector: i64,
        /// Stored direction.
        direction: Vector3,
        /// Two trailing flags.
        trailing_flags: [bool; 2],
    },
    /// Native kind `0` tail.
    Zero {
        /// Two leading flags.
        flags: [bool; 2],
        /// Native direction selector.
        selector: i64,
        /// Vector or BS3 curve selected structurally.
        direction: CompoundLoftDirection,
        /// Two trailing flags.
        trailing_flags: [bool; 2],
    },
}

/// Complete native compound-loft construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompoundLoftConstruction {
    /// Four mandatory scale slots; a boolean token encodes an absent slot.
    pub scales: Box<[Option<CompoundLoftScale>; 4]>,
    /// Optional fifth leading scale slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fifth_scale: Option<Box<CompoundLoftScale>>,
    /// Two flags before the tail kind.
    pub flags: [bool; 2],
    /// Kind-specific trailing graph.
    pub tail: CompoundLoftTail,
}

/// Initial solved-shape branch of a scaled compound loft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScaledCompoundLoftShape {
    /// A solved NURBS cache follows the singularity enum.
    Full,
    /// The cache is replaced by two intervals and two scalar arrays.
    None {
        /// Two ordered native intervals.
        parameter_ranges: [[f64; 2]; 2],
        /// Two ordered native scalar arrays.
        parameters: [Vec<f64>; 2],
    },
}

/// Structurally selected middle branch of a scaled compound loft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScaledCompoundLoftBranch {
    /// Extended branch ending in a direction vector.
    ExtendedVector {
        /// Optional first scale block.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_scale: Option<Box<CompoundLoftScale>>,
        /// Required second scale block.
        second_scale: Box<CompoundLoftScale>,
        /// Native selector integer.
        selector: i64,
        /// Stored direction vector.
        direction: Vector3,
    },
    /// Extended branch ending in a singularity and curve.
    ExtendedCurve {
        /// Optional scale block.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<Box<CompoundLoftScale>>,
        /// Native branch flag.
        flag: bool,
        /// Native singularity enum.
        singularity: i64,
        /// Stored BS3 curve.
        curve: CurveId,
    },
    /// Direct vector-or-curve branch.
    Direct {
        /// Native branch flag.
        flag: bool,
        /// Native direction selector.
        selector: i64,
        /// Vector or BS3 curve selected structurally.
        direction: CompoundLoftDirection,
    },
}

/// Complete native scaled compound-loft construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScaledCompoundLoftConstruction {
    /// Native leading singularity enum.
    pub singularity: i64,
    /// Singularity-selected solved-shape payload.
    pub shape: ScaledCompoundLoftShape,
    /// Six ordered discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
    /// Three leading scale slots; absent slots leave the following boolean in place.
    pub scales: Box<[Option<CompoundLoftScale>; 3]>,
    /// Two native flags preceding the selector.
    pub flags: [bool; 2],
    /// Native integer preceding the middle branch.
    pub selector: i64,
    /// Structurally selected middle branch.
    pub branch: ScaledCompoundLoftBranch,
    /// Two trailing branch flags.
    pub trailing_flags: [bool; 2],
    /// Native trailing kind integer.
    pub tail_kind: i64,
    /// Two native trailing vectors.
    pub tail_directions: [Vector3; 2],
    /// Native trailing singularity enum.
    pub tail_singularity: i64,
    /// Native trailing BS3 curve.
    pub tail_curve: CurveId,
}

/// One recursively framed native law formula.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LawFormula {
    /// Native formula name, or `null_law` for the sentinel.
    pub name: String,
    /// Ordered recursive variables; empty for `null_law`.
    pub variables: Vec<LawExpression>,
}

/// Complete recursive construction stored by a native law spline surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LawSurfaceConstruction {
    /// Legacy U and V parameter intervals; absent from modern layouts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_ranges: Option<[[f64; 2]; 2]>,
    /// Primary recursive surface law.
    pub primary: LawFormula,
    /// Ordered counted auxiliary laws referenced by the primary law.
    pub additional: Vec<LawFormula>,
    /// Standard surface-tail mode and its mode-specific fields.
    pub tail: LawSurfaceTail,
    /// Six ordered discontinuity arrays from the standard surface tail.
    pub discontinuities: [Vec<f64>; 6],
}

/// Mode-specific payload of a native law surface's standard surface tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LawSurfaceTail {
    /// Selector 0; the surface record carries a solved NURBS cache.
    Full,
    /// Selector 1; compact parameter summaries replace the solved cache.
    Summary {
        /// Ordered U and V parameter summaries.
        parameters: [Vec<f64>; 2],
        /// Native model-space fit tolerance.
        fit_tolerance: f64,
        /// Ordered U and V closure enums.
        closures: [i64; 2],
        /// Ordered U and V singularity enums.
        singularities: [i64; 2],
    },
    /// Selector 2; exact parameter intervals and boundary classifications.
    None {
        /// Ordered U and V parameter intervals.
        parameter_ranges: [[f64; 2]; 2],
        /// Ordered U and V closure enums.
        closures: [i64; 2],
        /// Ordered U and V singularity enums.
        singularities: [i64; 2],
    },
    /// Selector 3; no mode-specific payload.
    Historical,
    /// Selector 4; no mode-specific payload.
    Optimal,
}

/// One native law-expression node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LawExpression {
    /// Zero-payload `null_law` sentinel.
    Null,
    /// Tagged integer constant.
    Integer {
        /// Stored integer value.
        value: i64,
    },
    /// Tagged double constant.
    Double {
        /// Stored scalar value.
        value: f64,
    },
    /// Tagged model-space point constant.
    Point {
        /// Stored point value.
        value: Point3,
    },
    /// Tagged direction-vector constant.
    Vector {
        /// Stored vector value.
        value: Vector3,
    },
    /// Inline transform-law payload.
    Transform {
        /// Thirteen ordered transform scalars.
        scalars: [f64; 13],
        /// Three ordered transform enums.
        enums: [i64; 3],
    },
    /// Curve-backed edge law.
    Edge {
        /// Embedded curve carrier.
        curve: CurveId,
        /// Two optional parameter endpoints following the curve in the
        /// revision-gated encoding; absent from the pre-revision encoding.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        endpoints: Option<[Option<f64>; 2]>,
        /// Two native curve parameters.
        parameters: [f64; 2],
    },
    /// Spline-law payload.
    Spline {
        /// Native spline-law integer.
        native_id: i64,
        /// Ordered spline-law knots.
        knots: Vec<f64>,
        /// Ordered spline-law controls.
        controls: Vec<f64>,
        /// Native model-space point.
        point: Point3,
    },
    /// Algebraic operator and its recursively framed operands.
    Algebraic {
        /// Native operator token.
        operator: String,
        /// Ordered operands.
        operands: Vec<LawExpression>,
    },
}

/// One profile entry in the expanded skin layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkinSurfaceProfile {
    /// Native profile type integer.
    pub type_code: i64,
    /// Profile curve.
    pub curve: CurveId,
    /// Native loft constraint data.
    pub data: LoftProfileData,
}

/// Structurally selected native skin payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkinSurfaceLayout {
    /// Expanded sequence of profile curves and loft constraints.
    Profiles {
        /// Ordered profile entries.
        profiles: Vec<SkinSurfaceProfile>,
        /// Trailing path curve.
        path: CurveId,
        /// Two native trailing integers.
        tail: [i64; 2],
    },
    /// Compact curve/subdata form.
    Compact {
        /// Primary curve.
        curve: CurveId,
        /// Native loft subdata.
        subdata: LoftSubdata,
        /// Integer after the subdata.
        first_tail: i64,
        /// Secondary curve.
        secondary_curve: CurveId,
        /// Final compact-layout integer.
        second_tail: i64,
    },
}

/// Complete native `skin_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkinSurfaceConstruction {
    /// Native `SURF_BOOL` enum.
    pub surface_boolean: i64,
    /// Native `SURF_NORM` enum.
    pub surface_normal: i64,
    /// Native `SURF_DIR` enum.
    pub surface_direction: i64,
    /// Native leading count.
    pub count: i64,
    /// Native leading scalar.
    pub parameter: f64,
    /// Native inner count.
    pub inner_count: i64,
    /// Structurally selected skin payload.
    pub layout: SkinSurfaceLayout,
    /// Stored direction vector.
    pub direction: Vector3,
    /// Native scalar before the formula.
    pub trailing_parameter: f64,
    /// Recursive parametric law.
    pub formula: LawFormula,
    /// Trailing curve after the formula.
    pub parameter_curve: CurveId,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Complete native `net_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NetSurfaceConstruction {
    /// Two ordered loft-section graphs.
    pub sections: Box<[LoftSection; 2]>,
    /// Twelve ordered frame scalars.
    pub frame_parameters: [f64; 12],
    /// Native frame integer.
    pub flag: i64,
    /// Four ordered frame directions.
    pub directions: [Vector3; 4],
    /// Four ordered parameter laws.
    pub formulas: Box<[LawFormula; 4]>,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Structurally selected native sweep payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SweepSurfaceLayout {
    /// Profile-first modern ASM sweep layout.
    ProfileFirst {
        /// Second native sweep enum.
        secondary_kind: i64,
        /// Five ordered frame directions.
        directions: [Vector3; 5],
        /// Native model-space frame origin.
        origin: Point3,
        /// Four ordered native frame scalars.
        parameters: [f64; 4],
        /// Three ordered parametric laws.
        formulas: Box<[LawFormula; 3]>,
    },
    /// Explicit sweep layout whose trajectory is controlled by a formula.
    ExplicitFormula {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [f64; 2],
        /// Optional explicit profile frame.
        profile_frame: Option<(Point3, Vector3)>,
        /// Sweep frame origin.
        origin: Point3,
        /// Three ordered sweep frame directions.
        directions: [Vector3; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [f64; 2],
        /// Native trajectory scalar.
        path_parameter: f64,
        /// Native formula-side boolean.
        formula_flag: bool,
        /// Parametric trajectory formula.
        formula: LawFormula,
        /// Native trailing boolean.
        trailing_flag: bool,
    },
    /// Explicit sweep layout controlled by an auxiliary guide curve.
    ExplicitGuide {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [f64; 2],
        /// Optional explicit profile frame.
        profile_frame: Option<(Point3, Vector3)>,
        /// Sweep frame origin.
        origin: Point3,
        /// Three ordered sweep frame directions.
        directions: [Vector3; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [f64; 2],
        /// Native trajectory scalar.
        path_parameter: f64,
        /// Two guide-side booleans.
        guide_flags: [bool; 2],
        /// Auxiliary guide curve.
        guide_curve: CurveId,
        /// Guide parameter interval.
        guide_range: [f64; 2],
        /// Two native guide integers.
        guide_modes: [i64; 2],
        /// Six ordered guide scalars.
        guide_parameters: [f64; 6],
        /// Three trailing guide booleans.
        trailing_flags: [bool; 3],
    },
    /// Explicit sweep layout controlled by a support surface.
    ExplicitSurface {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [f64; 2],
        /// Optional explicit profile frame.
        profile_frame: Option<(Point3, Vector3)>,
        /// Sweep frame origin.
        origin: Point3,
        /// Three ordered sweep frame directions.
        directions: [Vector3; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [f64; 2],
        /// Native trajectory scalar.
        path_parameter: f64,
        /// Native singularity enum.
        singularity: i64,
        /// Support surface controlling the sweep.
        support_surface: SurfaceId,
        /// Optional auxiliary curve.
        auxiliary_curve: Option<CurveId>,
        /// Native support-side boolean.
        support_flag: bool,
        /// Legacy pre-219 trailing boolean when present.
        legacy_flag: Option<bool>,
    },
    /// Explicit-prefix sweep layout controlled by recursive laws.
    LawDriven {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [f64; 2],
        /// Optional explicit profile frame.
        profile_frame: Option<(Point3, Vector3)>,
        /// Sweep frame origin.
        origin: Point3,
        /// Three ordered sweep frame directions.
        directions: [Vector3; 3],
        /// Leading recursive sweep law.
        first_law: Box<LawExpression>,
        /// Native integer after the leading law.
        first_mode: i64,
        /// First law parameter interval.
        first_range: [f64; 2],
        /// Native law direction.
        law_direction: Vector3,
        /// Native path integer.
        path_mode: i64,
        /// Native path boolean.
        path_flag: bool,
        /// Path parameter interval.
        path_range: [f64; 2],
        /// Native path scalar.
        path_parameter: f64,
        /// Native second-law boolean.
        second_law_flag: bool,
        /// Trailing recursive sweep law.
        second_law: Box<LawExpression>,
        /// Native integer before the formula.
        formula_mode: i64,
        /// Parametric trajectory formula.
        formula: LawFormula,
        /// Native trailing boolean.
        trailing_flag: bool,
    },
}

/// Revision-gated `sweep_sur` form fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct SweepRevisionForm {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Boolean replacing the pre-revision primary enum.
    pub primary_flag: bool,
    /// Optional parameter endpoints following the embedded profile curve.
    #[serde(default)]
    pub profile_endpoints: [Option<f64>; 2],
    /// Optional parameter endpoints following the embedded path curve.
    #[serde(default)]
    pub path_endpoints: [Option<f64>; 2],
    /// Enum opening the shared revision-gated surface tail.
    pub tail_enum: i64,
}

/// Complete native `sweep_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SweepSurfaceConstruction {
    /// Leading native sweep enum.
    pub primary_kind: i64,
    /// Revision-gated form fields; absent from the pre-revision layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_form: Option<SweepRevisionForm>,
    /// Structurally selected sweep layout.
    pub layout: SweepSurfaceLayout,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Radius law for a procedural blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlendRadiusLaw {
    /// Constant blend radius along the whole spine.
    Constant {
        /// Signed radius, in document length units; sign selects the support offset side.
        signed_radius: f64,
    },
    /// Radius varying linearly from `start` to `end` along the spine.
    Linear {
        /// Signed radius at the spine start, in document length units.
        start: f64,
        /// Signed radius at the spine end, in document length units.
        end: f64,
    },
    /// Radius varying along the spine per an explicit law curve.
    Law {
        /// Curve whose parameterization gives the signed radius along the spine.
        curve: NurbsCurve,
    },
}

/// A neutral curve construction linked to its solved carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProceduralCurve {
    /// Stable construction identity.
    pub id: ProceduralCurveId,
    /// Solved curve produced by this construction.
    pub curve: CurveId,
    /// Neutral construction definition.
    pub definition: ProceduralCurveDefinition,
    /// Fit contract for the solved cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_fit_tolerance: Option<f64>,
}

/// One paired surface and parameter-space curve in an intcurve construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IntcurveSupportSide {
    /// Supporting surface, absent for the native `null_surface` sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<SurfaceId>,
    /// UV curve on `surface`, absent for the native `nullbs` sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcurve: Option<PcurveGeometry>,
}

/// Shared prefix carried by surface-related native intcurve constructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IntcurveSupportContext {
    /// Two ordered `(surface, pcurve)` support sides.
    pub sides: [IntcurveSupportSide; 2],
    /// Native parameter interval for the solved curve.
    pub parameter_range: [f64; 2],
    /// Three ordered native discontinuity arrays.
    pub discontinuities: [Vec<f64>; 3],
}

/// Cache-first shared-context fields absent from the context-first layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct CacheFirstCurveForm {
    /// Positive serializer-revision integer selecting the cache-first layout.
    pub revision: i64,
    /// Optional U/V bound fields following each ordered support surface.
    #[serde(default)]
    pub support_bounds: [[Option<f64>; 4]; 2],
    /// Optional solved-curve interval endpoints; absent endpoints inherit the
    /// solved NURBS domain.
    #[serde(default)]
    pub solved_range: [Option<f64>; 2],
    /// Native integer ASM extension following the discontinuity arrays.
    pub extension: i64,
}

/// Tail fields carried by the cache-first surface-curve layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceCurveTail {
    /// Native integer following the discontinuity arrays.
    pub extension: i64,
    /// Native boolean terminating the subtype payload.
    pub flag: bool,
    /// Second terminating boolean stored by `par_int_cur`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_flag: Option<bool>,
    /// Positive serializer-revision integer opening the cache-first layout.
    #[serde(default)]
    pub revision: i64,
    /// Optional U/V bound fields following each ordered support surface.
    #[serde(default)]
    pub support_bounds: [[Option<f64>; 4]; 2],
    /// Optional solved-curve interval endpoints; absent endpoints inherit the
    /// solved NURBS domain.
    #[serde(default)]
    pub solved_range: [Option<f64>; 2],
}

/// Mutually exclusive tail forms of a native projected intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectionTail {
    /// The ASM flag is followed immediately by the subtype close.
    EarlyClose {
        /// Native ASM projection flag.
        flag: bool,
    },
    /// The ASM flag is followed by a retained source interval and role text.
    Ranged {
        /// Native ASM projection flag.
        flag: bool,
        /// Native parameter interval on the projected source curve.
        parameter_range: [f64; 2],
        /// Projection role, such as `surf1` or `surf2`.
        role: String,
    },
}

/// Native prefix-only surface-curve construction family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceCurveFamily {
    /// Blend edge curve whose construction details live on its blend support.
    Blend,
    /// Curve constrained to a support surface.
    SurfaceConstrained,
    /// Parametric curve on a support surface.
    Parametric,
    /// Skin curve on a support surface.
    Skin,
}

/// Native silhouette construction family and its exclusive tail fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SilhouetteKind {
    /// Standard implicit silhouette.
    Standard,
    /// Parametric silhouette.
    Parametric,
    /// Draft/taper silhouette with an explicit factor.
    Taper {
        /// Native unscaled draft factor.
        draft_factor: f64,
    },
}

/// Discriminator-specific payload of a deformable native intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeformableCurveData {
    /// Mode 8 vector field followed by ordered scalar pairs.
    VectorField {
        /// Four ordered native vectors.
        vectors: [Vector3; 4],
        /// Ordered pairs from the mode-8 scalar table.
        parameter_pairs: Vec<[f64; 2]>,
    },
    /// Mode 5 supporting surface.
    Surface {
        /// Embedded deformation support surface.
        surface: SurfaceId,
    },
}

/// Neutral semantics for a procedural curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProceduralCurveDefinition {
    /// An exact native intcurve whose solved NURBS cache is authoritative.
    Exact,
    /// Curve defined by recursive native law formulas.
    Law {
        /// Shared support surfaces, UV curves, interval, and discontinuities.
        context: IntcurveSupportContext,
        /// Native ASM extension integer.
        extension: i64,
        /// Primary recursive law formula.
        primary: LawFormula,
        /// Counted additional recursive law formulas.
        additional: Vec<LawFormula>,
    },
    /// Ordered compound of native child curves with construction parameters.
    Compound {
        /// Leading native parameter array.
        parameters: Vec<f64>,
        /// One native scalar paired with each child curve.
        component_parameters: Vec<f64>,
        /// Ordered child curves forming the compound construction.
        components: Vec<CurveId>,
    },
    /// Circular or conical helix around an axis.
    Helix {
        /// Native angular parameter interval.
        angle_range: [f64; 2],
        /// Axis origin at the start of the helix.
        center: Point3,
        /// Major profile-radius vector.
        major: Vector3,
        /// Minor profile-radius vector; its orientation records handedness.
        minor: Vector3,
        /// Axial rise vector per full revolution.
        pitch: Vector3,
        /// Linear radial growth per revolution fraction; zero is cylindrical.
        apex_factor: f64,
        /// Unit helix axis direction.
        axis: Vector3,
    },
    /// Intersection of two support surfaces.
    Intersection {
        /// Shared surfaces, UV curves, interval, and discontinuity metadata.
        context: IntcurveSupportContext,
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
    },
    /// Intersection constrained by a third ordered support surface.
    ThreeSurfaceIntersection {
        /// Shared first two surfaces, UV curves, interval, and discontinuities.
        context: IntcurveSupportContext,
        /// Native selector preceding the third support pair.
        selector: i64,
        /// Third `(surface, pcurve)` support pair.
        third: IntcurveSupportSide,
    },
    /// Surface-related curve whose native subtype has no tail beyond the shared prefix.
    SurfaceCurve {
        /// Native prefix-only family.
        family: SurfaceCurveFamily,
        /// Shared surfaces, UV curves, interval, and discontinuities.
        context: IntcurveSupportContext,
        /// Cache-first subtype tail, absent from the prefix-first layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tail: Option<SurfaceCurveTail>,
    },
    /// Silhouette of a cast surface in a light direction.
    Silhouette {
        /// Shared first two support pairs.
        context: IntcurveSupportContext,
        /// Standard, parametric, or taper silhouette semantics.
        silhouette: SilhouetteKind,
        /// Surface whose silhouette is constructed.
        cast_surface: SurfaceId,
        /// Native model-space light direction.
        light_direction: Vector3,
    },
    /// Curve offset relative to a surface parameterization.
    SurfaceOffset {
        /// Shared first two support pairs.
        context: IntcurveSupportContext,
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
        /// Native U interval on the base surface.
        base_u_range: [f64; 2],
        /// Native V interval on the base surface.
        base_v_range: [f64; 2],
        /// Embedded base curve.
        base: CurveId,
        /// Native interval on `base`.
        base_range: [f64; 2],
        /// Optional parameter endpoints following the embedded base curve in
        /// the cache-first layout.
        #[serde(default)]
        base_endpoints: [Option<f64>; 2],
        /// Cache-first shared-context fields; absent from the context-first
        /// layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_first: Option<CacheFirstCurveForm>,
        /// Signed model-space offset distance.
        distance: f64,
        /// Native unscaled parameter shift.
        shift: f64,
        /// Native unscaled parameter scale.
        scale: f64,
    },
    /// Blend spring guide between two support sides.
    Spring {
        /// Ordered support surfaces, UV curves, interval, and discontinuities.
        context: IntcurveSupportContext,
        /// Conditional U/V intervals, present exactly when the corresponding
        /// support surface is the native `null_surface` sentinel.
        surface_parameter_ranges: [Option<[[f64; 2]; 2]>; 2],
        /// Conditional interval present exactly when the first support pcurve
        /// is the native `nullbs` sentinel.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_pcurve_parameter_range: Option<[f64; 2]>,
        /// Native boolean following the discontinuity arrays; unused by the
        /// cache-first layout.
        discontinuity_flag: bool,
        /// Cache-first shared-context fields; absent from the context-first
        /// layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_first: Option<CacheFirstCurveForm>,
        /// Native `CURV_DIR` enum value.
        direction: i64,
    },
    /// Deformation of an embedded bend curve.
    Deformable {
        /// Native ASM extension integer preceding the bend curve.
        extension: i64,
        /// Embedded bend curve.
        bend: CurveId,
        /// Mode 8 vector field or mode 5 support surface.
        data: DeformableCurveData,
    },
    /// Projection of a source curve onto a support surface.
    Projection {
        /// Shared surfaces, UV curves, interval, and discontinuity metadata.
        context: IntcurveSupportContext,
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
        /// Curve being projected.
        source: CurveId,
        /// Native post-source tail form.
        tail: ProjectionTail,
    },
    /// Offset from a source curve.
    Offset {
        /// Curve this curve is offset from.
        source: CurveId,
        /// Signed offset distance, in document length units.
        distance: f64,
        /// Fixed plane-normal direction defining the offset side, when carried
        /// by the source representation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
        /// Surface the offset is measured within, when the offset is constrained
        /// to a support surface; `None` for a free-space offset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        support: Option<SurfaceId>,
        /// Unit normal defining the positive offset side for planar offsets.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        normal: Option<Vector3>,
        /// Parameter interval on the source curve used by the offset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter_range: Option<[f64; 2]>,
        /// Variable distance law; absent when `distance` is uniform.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance_law: Option<CurveOffsetDistanceLaw>,
    },
    /// Free-space 3D offset using a reference direction.
    SpatialOffset {
        /// Curve being offset.
        source: CurveId,
        /// Signed offset distance.
        distance: f64,
        /// Reference direction controlling the offset frame.
        reference_direction: Vector3,
        /// Whether the source classifies the result as self-intersecting.
        self_intersect: Option<bool>,
    },
    /// Intersection of two surfaces after applying independent signed offsets.
    TwoSidedOffset {
        /// Shared surfaces, UV curves, interval, and discontinuity metadata.
        context: IntcurveSupportContext,
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
        /// Signed offset distance for each support side, in document length units.
        offsets: [f64; 2],
    },
    /// Free-space vector offset of a source curve over a parameter interval.
    VectorOffset {
        /// Curve being offset.
        source: CurveId,
        /// Native parameter interval on the source curve.
        parameter_range: [f64; 2],
        /// Model-space offset vector.
        offset: Vector3,
        /// Native role labels following the offset vector.
        labels: [String; 2],
        /// Native integer role codes paired with `labels`.
        codes: [i64; 2],
    },
    /// A parameter sub-range of a parent curve.
    Subset {
        /// Parent curve being restricted.
        source: CurveId,
        /// Native parameter interval retained from the parent.
        parameter_range: [f64; 2],
    },
    /// Spine or center curve of a blend surface.
    BlendSpine {
        /// The blend surface this curve is the spine of, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blend_surface: Option<SurfaceId>,
    },
    /// Preserved construction without a neutral interpretation.
    Unknown {
        /// Source construction-family discriminator, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_kind: Option<String>,
        /// Reference to the preserved raw source record, when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
}

/// Independent variable used by a curve-offset distance law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CurveOffsetLawBasis {
    /// Distance measured along the source curve from the offset interval start.
    ArcLength,
    /// Native source-curve parameter.
    Parameter,
}

/// Variable signed distance law for a planar curve offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveOffsetDistanceLaw {
    /// Linear interpolation between two distance controls.
    Linear {
        /// Independent-variable interpretation.
        basis: CurveOffsetLawBasis,
        /// Ordered signed distances in document length units.
        distances: [f64; 2],
        /// Ordered arc-length or neutral carrier-parameter controls.
        control_range: [f64; 2],
    },
    /// One coordinate of another curve defines the signed distance.
    Coordinate {
        /// Curve carrying the distance function.
        function: CurveId,
        /// One-based coordinate number on `function`.
        coordinate: u8,
        /// Independent-variable interpretation.
        basis: CurveOffsetLawBasis,
        /// Function parameter at zero source parameter or arc length.
        function_parameter_offset: f64,
        /// Function-parameter change per neutral source parameter or length unit.
        function_parameter_scale: f64,
    },
}

/// The shape of a parameter-space (u, v) curve on a surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PcurveGeometry {
    /// A straight line in parameter space.
    Line {
        /// A parameter-space point on the line.
        origin: Point2,
        /// Parameter-space direction.
        direction: Point2,
    },
    /// Full circle in parameter space.
    Circle {
        /// Circle center.
        center: Point2,
        /// Zero-angle unit direction.
        x_axis: Point2,
        /// Positive-angle unit direction.
        y_axis: Point2,
        /// Circle radius.
        radius: f64,
    },
    /// Full ellipse in parameter space.
    Ellipse {
        /// Ellipse center.
        center: Point2,
        /// Major-axis unit direction.
        x_axis: Point2,
        /// Minor-axis unit direction.
        y_axis: Point2,
        /// Semi-major radius.
        major_radius: f64,
        /// Semi-minor radius.
        minor_radius: f64,
    },
    /// Parabola in parameter space.
    Parabola {
        /// Parabola vertex.
        vertex: Point2,
        /// Axis unit direction.
        x_axis: Point2,
        /// Positive transverse unit direction.
        y_axis: Point2,
        /// Focus distance.
        focal_distance: f64,
    },
    /// Hyperbola in parameter space.
    Hyperbola {
        /// Hyperbola center.
        center: Point2,
        /// Transverse-axis unit direction.
        x_axis: Point2,
        /// Conjugate-axis unit direction.
        y_axis: Point2,
        /// Semi-transverse radius.
        major_radius: f64,
        /// Semi-conjugate radius.
        minor_radius: f64,
    },
    /// A free-form NURBS curve in parameter space (control points are (u, v)).
    Nurbs {
        /// Curve degree.
        degree: u32,
        /// Full knot vector.
        knots: Vec<f64>,
        /// Control points in (u, v) parameter space.
        control_points: Vec<Point2>,
        /// Per-pole weights; `None` denotes non-rational.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weights: Option<Vec<f64>>,
        /// Whether the parameter-space curve is periodic.
        #[serde(default)]
        periodic: bool,
    },
    /// Parameter restriction of an exact basis pcurve.
    Trimmed {
        /// Native parameter interval retained from the basis.
        parameter_range: [f64; 2],
        /// Exact basis geometry.
        basis: Box<PcurveGeometry>,
    },
    /// Signed planar offset of an exact basis pcurve.
    Offset {
        /// Signed parameter-space distance.
        distance: f64,
        /// Exact basis geometry.
        basis: Box<PcurveGeometry>,
    },
}

/// A pcurve carrier: the 2D image of a coedge in its face's surface parameter
/// space. Referenced by a coedge; the owning surface establishes whether a
/// parameter dimension is a length (relevant to unit scaling, see [F3D spec §6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#6-topology-records)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Pcurve {
    /// Arena id.
    pub id: PcurveId,
    /// Parameter-space shape.
    pub geometry: PcurveGeometry,
    /// Inline `exp_par_cur` parameterization reversal; absent on ref-form pcurves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapper_reversed: Option<bool>,
    /// Four native booleans following the inline subtype scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_tail_flags: Option<[bool; 4]>,
    /// Native parameter interval on which this pcurve is evaluated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_range: Option<[f64; 2]>,
    /// Parameter-space fit tolerance following the solved UV cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_tolerance: Option<f64>,
}
