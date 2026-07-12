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
    /// Right circular cone. `radius` is measured at `origin`; `half_angle` is
    /// the half-angle between the axis and the cone surface, in radians.
    Cone {
        /// Reference point on the axis where `radius` is measured.
        origin: Point3,
        /// Axis direction (unit).
        axis: Vector3,
        /// Zero-azimuth direction perpendicular to `axis`.
        ref_direction: Vector3,
        /// Radius at `origin`.
        radius: f64,
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
    /// Free-form NURBS curve.
    Nurbs(NurbsCurve),
    /// Native curve carrier whose shape is not decoded.
    Unknown {
        /// Retained native record containing the curve carrier.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
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

/// A neutral surface construction linked to its solved carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProceduralSurface {
    /// Stable construction identity.
    pub id: ProceduralSurfaceId,
    /// Solved surface produced by this construction.
    pub surface: SurfaceId,
    /// Neutral construction definition.
    pub definition: ProceduralSurfaceDefinition,
    /// Fit contract for the solved cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_fit_tolerance: Option<f64>,
}

/// Neutral semantics for a procedural surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProceduralSurfaceDefinition {
    /// Exact native NURBS surface with retained parameter intervals.
    Exact {
        /// Ordered native U and V intervals.
        parameter_ranges: [[f64; 2]; 2],
        /// Native ASM extension integer following the intervals.
        extension: i64,
    },
    /// Translation of a directrix along a direction.
    Extrusion {
        /// Curve swept along `direction` to form the surface.
        directrix: CurveId,
        /// Length-bearing sweep direction, in document length units.
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
        /// Directrix surface-parameter start and end values.
        parameter_interval: [f64; 2],
        /// Whether the source parameter directions are transposed.
        transposed: bool,
    },
    /// Sum of two ordered curves from a base point.
    Sum {
        /// First curve, varying in the first surface parameter.
        first: CurveId,
        /// Second curve, varying in the second surface parameter.
        second: CurveId,
        /// Surface base point.
        basepoint: Vector3,
    },
    /// Sweep of a profile along a spine.
    Sweep {
        /// Cross-section curve carried along `spine`.
        profile: CurveId,
        /// Path curve the profile is swept along.
        spine: CurveId,
    },
    /// Offset from a support surface.
    Offset {
        /// Surface this surface is offset from.
        support: SurfaceId,
        /// Signed offset distance, in document length units.
        distance: f64,
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
    },
    /// Preserved construction without a neutral interpretation.
    Unknown {
        /// Reference to the preserved raw source record, when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
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
        /// Native U interval on the base surface.
        base_u_range: [f64; 2],
        /// Native V interval on the base surface.
        base_v_range: [f64; 2],
        /// Embedded base curve.
        base: CurveId,
        /// Native interval on `base`.
        base_range: [f64; 2],
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
        /// Surface the offset is measured within, when the offset is constrained
        /// to a support surface; `None` for a free-space offset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        support: Option<SurfaceId>,
    },
    /// Intersection of two surfaces after applying independent signed offsets.
    TwoSidedOffset {
        /// Shared surfaces, UV curves, interval, and discontinuity metadata.
        context: IntcurveSupportContext,
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
        /// Reference to the preserved raw source record, when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
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
    /// Native parameter interval on which this pcurve is evaluated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_range: Option<[f64; 2]>,
    /// Parameter-space fit tolerance following the solved UV cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_tolerance: Option<f64>,
}
