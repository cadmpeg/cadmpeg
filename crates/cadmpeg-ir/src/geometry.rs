// SPDX-License-Identifier: Apache-2.0
//! Geometry carriers: analytic surfaces, analytic curves, NURBS, and pcurves.
//!
//! Carriers are stored in their own arenas and referenced by id from the
//! topology graph (a face references a [`Surface`], an edge a [`Curve`], a
//! coedge a [`Pcurve`]). This mirrors the ACIS/ASM model where geometry is
//! shared by reference — the f3d spec notes ~35% of faces share a surface
//! entity by reference id (see the f3d program notes on sequential-ID surface
//! sharing).

use crate::ids::{CurveId, PcurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
use crate::math::{Point2, Point3, Vector3};
use crate::provenance::EntityMeta;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A NURBS surface or curve knot/pole payload.
///
/// Fields follow the ASM `nubs`/`nurbs` block grammar: degrees, full (clamped)
/// knot vectors per parametric direction, a flat control-point list in
/// row-major (u-major) order, and optional per-pole weights (absent ⇒
/// non-rational). Pole counts equal `knots.len() - degree - 1` per direction.
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
    /// Per-pole weights (same order as `control_points`); `None` ⇒ non-rational.
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
    /// Full (clamped) knot vector.
    pub knots: Vec<f64>,
    /// Control points in parameter order.
    pub control_points: Vec<Point3>,
    /// Per-pole weights; `None` ⇒ non-rational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<Vec<f64>>,
    /// Whether the curve is periodic.
    pub periodic: bool,
}

/// The analytic or free-form shape of a surface carrier.
///
/// The analytic variants (plane…torus) correspond to the ASM analytic surface
/// carriers; [`SurfaceGeometry::Nurbs`] covers spline surfaces and reduced
/// blends. Feature-specific blend subtypes are decoded into NURBS rather than
/// modeled as distinct IR variants in v0.
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u_axis: Option<Vector3>,
    },
    /// Right circular cylinder of the given `radius` about the axis line.
    Cylinder {
        /// A point on the axis.
        origin: Point3,
        /// Axis direction (unit).
        axis: Vector3,
        /// Zero-azimuth direction perpendicular to `axis`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_direction: Option<Vector3>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_direction: Option<Vector3>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        axis: Option<Vector3>,
        /// Zero-azimuth direction perpendicular to `axis`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_direction: Option<Vector3>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_direction: Option<Vector3>,
        /// Major radius.
        major_radius: f64,
        /// Minor (tube) radius.
        minor_radius: f64,
    },
    /// Free-form NURBS surface.
    Nurbs(NurbsSurface),
    /// The face's topology is known but its surface geometry was not decoded
    /// into a typed carrier (e.g. a spline or procedural surface the decoder
    /// recognizes as a record but cannot yet interpret). The face keeps its
    /// loops and trims; only the underlying shape is opaque.
    ///
    /// `record` links to the preserved raw bytes in the [`crate::unknown`]
    /// arena so a re-encode path can recover the original record. It is
    /// `Option` because a surface can be known-unknown even when the decoder
    /// did not (or could not) retain the bytes.
    ///
    /// A [`Surface`] carrying this variant should set its
    /// [`EntityMeta`](crate::provenance::EntityMeta) exactness to
    /// [`Exactness::Unknown`](crate::provenance::Exactness::Unknown): the shape
    /// was not established, so nothing about it is byte-exact or derived.
    Unknown {
        /// Link to the preserved raw record, when the decoder kept the bytes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
}

/// A surface carrier: geometry plus id and provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Surface {
    /// Arena id.
    pub id: SurfaceId,
    /// Surface shape.
    pub geometry: SurfaceGeometry,
    /// Provenance/exactness metadata.
    pub meta: EntityMeta,
}

/// Source surface parameter frame. `u_reference` defines zero azimuth or the
/// positive u axis; `v_reference` defines the positive v direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceParameterization {
    /// Surface this parameter frame applies to.
    pub surface: SurfaceId,
    /// World-space point at parameter `(0, 0)`.
    pub origin: Point3,
    /// Zero-azimuth / positive-u direction, in document length units.
    pub u_reference: Vector3,
    /// Positive-v direction, in document length units.
    pub v_reference: Vector3,
    /// Provenance metadata for this parameterization record.
    pub meta: EntityMeta,
}

/// Native construction semantics for a solved surface carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProceduralSurface {
    /// Solved surface produced by this construction.
    pub surface: SurfaceId,
    /// Native operation retained independently of its solved cache.
    pub definition: ProceduralSurfaceDefinition,
    /// Fit contract for the stored solved cache, when supplied by the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_fit_tolerance: Option<f64>,
    /// Byte provenance of the native construction record.
    pub meta: EntityMeta,
}

/// A source-native procedural surface definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProceduralSurfaceDefinition {
    /// Translational extrusion `S(u,v) = C(u) + vD` (`cyl_spl_sur`).
    TranslationalExtrusion {
        /// Directrix recovered from the native construction record.
        directrix: NurbsCurve,
        /// Length-bearing derivative `D`, in document length units.
        direction: Vector3,
        /// Native directrix parameter interval.
        u_range: [f64; 2],
        /// Native extrusion parameter interval.
        v_range: [f64; 2],
    },
    /// Rolling-ball envelope between two native support surfaces.
    RollingBallBlend {
        /// Native support families in side order.
        supports: BlendSupports,
        /// Stored center/spine curve, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        center_curve: Option<NurbsCurve>,
        /// Signed offset-radius law. Signs select the support offsets.
        radius: BlendRadiusLaw,
    },
}

/// Resolution state of the two support sides of a rolling-ball blend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", content = "supports", rename_all = "snake_case")]
pub enum BlendSupports {
    /// Both support sides were resolved to a known kind.
    Complete([ProceduralSupportKind; 2]),
    /// Only some support sides were resolved; length is 0, 1, or unresolved-partial.
    Partial(Vec<ProceduralSupportKind>),
}

/// Native support family used by a procedural construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralSupportKind {
    /// Planar support surface.
    Plane,
    /// Conical (or cylindrical) support surface.
    Cone,
    /// Spherical support surface.
    Sphere,
    /// Toroidal support surface.
    Torus,
    /// Free-form spline support surface.
    Spline,
    /// Translational-extrusion support surface.
    TranslationalExtrusion,
}

/// A rolling-ball blend's byte-stored signed radius law.
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
}

/// The analytic or free-form shape of a 3D curve carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveGeometry {
    /// Infinite line through `origin` with `direction`.
    Line {
        /// A point on the line.
        origin: Point3,
        /// Line direction (unit).
        direction: Vector3,
    },
    /// Full circle.
    Circle {
        /// Circle center.
        center: Point3,
        /// Circle plane normal / axis (unit).
        axis: Vector3,
        /// Radius.
        radius: f64,
    },
    /// Ellipse. `major_direction` is the in-plane direction of the major axis.
    Ellipse {
        /// Ellipse center.
        center: Point3,
        /// Ellipse plane normal (unit).
        axis: Vector3,
        /// In-plane major-axis direction (unit).
        major_direction: Vector3,
        /// Semi-major radius.
        major_radius: f64,
        /// Semi-minor radius.
        minor_radius: f64,
    },
    /// Parabola in STEP conic form.
    Parabola {
        /// Parabola vertex.
        vertex: Point3,
        /// Plane normal (unit).
        axis: Vector3,
        /// In-plane direction from the vertex toward increasing axial parameter.
        major_direction: Vector3,
        /// Distance from the vertex to the focus.
        focal_distance: f64,
    },
    /// Hyperbola in STEP conic form.
    Hyperbola {
        /// Hyperbola center.
        center: Point3,
        /// Plane normal (unit).
        axis: Vector3,
        /// In-plane direction of the transverse axis.
        major_direction: Vector3,
        /// Semi-transverse radius.
        major_radius: f64,
        /// Semi-conjugate radius.
        minor_radius: f64,
    },
    /// Free-form NURBS curve.
    Nurbs(NurbsCurve),
}

/// A 3D curve carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Curve {
    /// Arena id.
    pub id: CurveId,
    /// Curve shape.
    pub geometry: CurveGeometry,
    /// Provenance/exactness metadata.
    pub meta: EntityMeta,
}

/// Native construction metadata for a solved procedural curve cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProceduralCurve {
    /// Solved curve produced by this construction.
    pub curve: CurveId,
    /// Native construction subtype name, retained independently of its solved cache.
    pub native_kind: String,
    /// Fit contract for the stored solved cache, when supplied by the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_fit_tolerance: Option<f64>,
    /// Byte provenance of the native construction record.
    pub meta: EntityMeta,
}

/// A v1 source-native surface construction retained beside its solved carrier.
///
/// This type is parallel to [`ProceduralSurface`]; the v0 carrier remains
/// unchanged during the additive migration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProceduralSurfaceV1 {
    /// Stable construction identity.
    pub id: ProceduralSurfaceId,
    /// Solved surface produced by this construction.
    pub surface: SurfaceId,
    /// Neutral construction definition.
    pub definition: ProceduralSurfaceDefinitionV1,
    /// Fit contract for the solved cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_fit_tolerance: Option<f64>,
}

/// Neutral v1 taxonomy for source-native surface constructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProceduralSurfaceDefinitionV1 {
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
        radius: BlendRadiusLawV1,
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

/// Radius law for a v1 procedural blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlendRadiusLawV1 {
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

/// A v1 source-native curve construction retained beside its solved carrier.
///
/// This type is parallel to [`ProceduralCurve`]; the v0 carrier remains
/// unchanged during the additive migration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProceduralCurveV1 {
    /// Stable construction identity.
    pub id: ProceduralCurveId,
    /// Solved curve produced by this construction.
    pub curve: CurveId,
    /// Neutral construction definition.
    pub definition: ProceduralCurveDefinitionV1,
    /// Fit contract for the solved cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_fit_tolerance: Option<f64>,
}

/// Neutral v1 taxonomy for source-native curve constructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProceduralCurveDefinitionV1 {
    /// Intersection of two support surfaces.
    Intersection {
        /// The two intersecting surfaces; `None` when a side was not resolved.
        supports: [Option<SurfaceId>; 2],
    },
    /// Projection of a source curve onto a support surface.
    Projection {
        /// Curve being projected.
        source: CurveId,
        /// Surface the source curve is projected onto.
        support: SurfaceId,
        /// Projection direction, when the source recorded one; `None` for a
        /// normal (closest-point) projection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
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
        /// Per-pole weights; `None` ⇒ non-rational.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weights: Option<Vec<f64>>,
    },
}

/// A pcurve carrier: the 2D image of a coedge in its face's surface parameter
/// space. Referenced by a coedge; the owning surface establishes whether a
/// parameter dimension is a length (relevant to unit scaling, see f3d spec §6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Pcurve {
    /// Arena id.
    pub id: PcurveId,
    /// Parameter-space shape.
    pub geometry: PcurveGeometry,
    /// Provenance/exactness metadata.
    pub meta: EntityMeta,
}
