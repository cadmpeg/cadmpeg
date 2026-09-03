// SPDX-License-Identifier: Apache-2.0
//! Procedural spline-surface embedded types and their `_spl_sur` decoders.

use crate::nurbs::blend::{
    compact_rb_blend_spl_sur, cyl_spl_sur, full_rb_blend_spl_sur, rolling_ball_side,
    var_blend_spl_sur, vertex_blend_spl_sur,
};
use crate::nurbs::core::{curve_block, surface_block};
use crate::nurbs::pcurve::{decode_pcurve_block_with_end, pcurve_block_with_end, NurbsPcurve};
use crate::nurbs::proc_curve::{
    embedded_base_curve_resolving_refs, embedded_surface, embedded_surface_with_ranges,
    optional_embedded_surface_with_bounds, optional_helix_revision,
};
use crate::nurbs::reader::{normalized, take_native_ident, LEN_TO_MM};
use crate::nurbs::toks::{self, Cur, SubtypeTable};
use crate::sab::Token;
use cadmpeg_core::decode::bounded_len;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, NurbsCurve, NurbsSurface, RevisionCacheForm,
    RevisionSurfaceParameterization, SurfaceGeometry, VariableBlendSolvedCache,
};
use cadmpeg_ir::math::{Point3, Vector3};
use std::num::NonZeroI64;

/// A decoded native procedural definition and the fit contract of its solved cache.
pub struct DecodedProceduralSurface {
    /// The native procedural surface construction (blend, sweep, loft, or
    /// taper family) decoded from its subtype-dispatched inline fields.
    pub definition: DecodedProceduralSurfaceDefinition,
    /// `surface_fit_tolerance` of the cached B-spline block, if present.
    /// `0.0` marks fidelity to the procedural surface. Primitive identity uses
    /// a separate value ([spec §6.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#65-nubsnurbs-blocks-b-spline-curves-and-surfaces)).
    pub cache_fit_tolerance: Option<f64>,
}

/// Source-native procedural semantics before embedded geometry is assigned IR ids.
pub enum DecodedProceduralSurfaceDefinition {
    /// Exact NURBS construction and retained native parameter fields.
    Exact {
        /// Legacy ordered ranges or revision-native scalar values.
        parameters: cadmpeg_ir::geometry::SplineSurfaceParameters,
        /// Native ASM extension integer.
        extension: i64,
        /// Revision-gated form fields.
        revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
    },
    /// Native compound surface with ordered scalar/component pairs.
    Compound {
        /// Ordered native parameters.
        parameters: Vec<f64>,
        /// Ordered embedded component surfaces.
        components: Vec<SurfaceGeometry>,
    },
    /// Exact rectangular restriction of an embedded support surface.
    SubSurface {
        /// Embedded support surface.
        support: SurfaceGeometry,
        /// Ordered U and V parameter intervals.
        parameter_ranges: [[f64; 2]; 2],
    },
    /// Native taper family with shared carriers and subtype tail.
    Taper {
        /// Embedded base surface.
        support: SurfaceGeometry,
        /// Embedded reference curve.
        reference: NurbsCurve,
        /// Embedded UV curve, absent for `nullbs`.
        pcurve: Option<NurbsPcurve>,
        /// Native taper parameter.
        parameter: f64,
        /// Subtype-specific tail.
        taper: cadmpeg_ir::geometry::TaperSurfaceKind,
        /// Revision-gated form fields.
        revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
    },
    /// Native loft construction graph with embedded carriers.
    Loft(EmbeddedLoft),
    /// Native compound-loft graph with embedded carriers.
    CompoundLoft(Box<EmbeddedCompoundLoft>),
    /// Revision-gated compound-loft graph with embedded carriers.
    RevisionCompoundLoft(Box<EmbeddedRevisionCompoundLoft>),
    /// Native scaled compound-loft graph with embedded carriers.
    ScaledCompoundLoft(Box<EmbeddedScaledCompoundLoft>),
    /// Native skinned surface graph with embedded carriers.
    Skin(Box<EmbeddedSkinSurface>),
    /// Native recursive law-surface graph.
    Law(Box<EmbeddedLawSurface>),
    /// Native curve-network surface graph with embedded carriers.
    Net(Box<EmbeddedNetSurface>),
    /// Native sweep surface graph with embedded carriers.
    Sweep(Box<EmbeddedSweepSurface>),
    /// Native T-spline wrapper and subtransform program.
    TSpline(Box<cadmpeg_ir::geometry::TSplineSurfaceConstruction>),
    /// Native circular or linear helix surface.
    Helix(Box<cadmpeg_ir::geometry::HelixSurfaceConstruction>),
    /// Native deformable surface with embedded support.
    Deformable(Box<EmbeddedDeformableSurface>),
    /// Native G2 blend construction with embedded carriers.
    G2Blend(Box<EmbeddedG2Blend>),
    /// Revision-gated G2 blend in the variable-blend side layout.
    RevisionG2Blend(Box<EmbeddedRevisionG2Blend>),
    /// Ruled interpolation between two ordered profile curves.
    Ruled {
        /// First embedded profile.
        first: NurbsCurve,
        /// Second embedded profile.
        second: NurbsCurve,
    },
    /// Translational sum of two curves around a stored origin.
    Sum {
        /// First embedded curve.
        first: CurveGeometry,
        /// Second embedded curve.
        second: CurveGeometry,
        /// Native model-space origin.
        basepoint: Vector3,
        /// Revision-gated form fields.
        revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
    },
    /// Revolution of an embedded profile around an axis.
    Revolution {
        /// Embedded profile curve.
        directrix: CurveGeometry,
        /// Point on the axis in model space.
        axis_origin: Point3,
        /// Unit axis direction.
        axis_direction: Vector3,
        /// Angular interval from the solved surface cache.
        angular_interval: [f64; 2],
        /// Native profile parameter interval.
        parameter_interval: [f64; 2],
        /// Revision-gated form fields.
        revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
    },
    /// Signed offset from an embedded support surface.
    Offset {
        /// Embedded support surface.
        support: SurfaceGeometry,
        /// Signed model-space distance.
        distance: f64,
        /// Native U sense enum, absent from the revision-gated layout.
        u_sense: Option<i64>,
        /// Native V sense enum, absent from the revision-gated layout.
        v_sense: Option<i64>,
        /// Ordered conditional ASM flags.
        extension_flags: Vec<bool>,
        /// Revision-gated form fields.
        revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
    },
    /// Translation of an embedded directrix along a length-bearing direction.
    Extrusion {
        /// Embedded directrix cache.
        directrix: NurbsCurve,
        /// Stored directrix parameter interval.
        parameter_interval: [f64; 2],
        /// Length-bearing sweep direction.
        direction: Vector3,
        /// Native model-space position following the direction.
        native_position: Point3,
        /// Revision-gated form fields.
        revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
    },
    /// Rolling-ball blend with embedded support and spine caches.
    Blend {
        /// Embedded support caches in side order.
        supports: Box<[Option<SurfaceGeometry>; 2]>,
        /// Embedded center/spine curve.
        spine: Option<NurbsCurve>,
        /// Signed radius law.
        radius: BlendRadiusLaw,
        /// Blend cross-section family.
        cross_section: BlendCrossSection,
        /// Complete native construction graph when the full layout decoded.
        native: Option<Box<EmbeddedRollingBall>>,
    },
    /// Variable-radius blend with a complete embedded construction graph.
    VariableBlend(Box<EmbeddedVariableBlend>),
    /// Vertex-blend patch with complete embedded boundary graphs.
    VertexBlend(Box<EmbeddedVertexBlend>),
}

/// One embedded support side of a rolling-ball or variable blend.
pub struct EmbeddedRollingBallSide {
    /// The support kind the side's leading identifier selects.
    pub support_kind: cadmpeg_ir::geometry::VariableBlendSupportKind,
    /// The embedded support surface.
    pub surface: Option<SurfaceGeometry>,
    /// Optional UV bounds of the support surface; `None` marks an unbounded end.
    pub surface_ranges: [[Option<f64>; 2]; 2],
    /// The embedded support curve.
    pub curve: Option<CurveGeometry>,
    /// Optional parameter bounds of the support curve.
    pub curve_range: [Option<f64>; 2],
    /// The embedded NURBS parameter curve on the support surface.
    pub pcurve: Option<NurbsPcurve>,
    /// The support location point.
    pub location: Point3,
    /// A second embedded parameter curve, when serialized.
    pub secondary_pcurve: Option<NurbsPcurve>,
    /// The extension integer serialized after the secondary pcurve.
    pub extension: Option<i64>,
    /// A third embedded parameter curve, when serialized.
    pub tertiary_pcurve: Option<NurbsPcurve>,
}

/// Embedded revision-gated G2 blend before stable IR ids are assigned.
pub struct EmbeddedRevisionG2Blend {
    /// The revision integer that gates the layout.
    pub revision: i64,
    /// Two leading parameters serialized before the sides.
    pub leading_parameters: [f64; 2],
    /// Two ordered embedded support sides.
    pub sides: Box<[EmbeddedRollingBallSide; 2]>,
    /// The embedded center curve.
    pub center: CurveGeometry,
    /// Optional parameter bounds of the center curve.
    pub center_range: [Option<f64>; 2],
    /// Two blend radii in document length units.
    pub radii: [f64; 2],
    /// The integer selector serialized after the radii.
    pub radius_selector: i64,
    /// Support-side parameter interval `(T0, T1)`.
    pub u_range: [Option<f64>; 2],
    /// Second interval; `None` marks an unbounded end.
    pub v_range: [Option<f64>; 2],
    /// Approximation-current flag (`1` when the cache is current).
    pub shape_prefix: i64,
    /// Requested fit tolerance.
    pub shape_parameter: f64,
    /// Achieved fit tolerance, at or below `shape_parameter`.
    pub shape_length: f64,
    /// Signed integer immediately before the shared tail's enum, taking the
    /// values `-1` and `1`.
    pub shape_tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm,
    /// Six discontinuity arrays of the shared tail.
    pub discontinuities: [Vec<f64>; 6],
    /// The boolean serialized after the discontinuity arrays.
    pub tail_flag: bool,
    /// Three integers closing the shared tail.
    pub tail_extensions: [i64; 3],
}

/// The optional third support side of a rolling-ball blend.
pub struct EmbeddedRollingBallThirdSide {
    /// The leading identifier of the third side.
    pub label: String,
    /// The embedded support surface.
    pub surface: SurfaceGeometry,
    /// The embedded support curve.
    pub curve: NurbsCurve,
    /// The embedded NURBS parameter curve on the support surface.
    pub pcurve: Option<NurbsPcurve>,
    /// The support direction vector.
    pub direction: Vector3,
    /// A second embedded parameter curve, when serialized.
    pub secondary_pcurve: Option<NurbsPcurve>,
    /// The extension integer serialized after the secondary pcurve.
    pub extension: i64,
    /// A third embedded parameter curve, when serialized.
    pub tertiary_pcurve: Option<NurbsPcurve>,
    /// The boolean closing the third side.
    pub flag: bool,
}

/// Embedded native variable blend before stable IR ids are assigned.
pub struct EmbeddedVariableBlend {
    /// The blend subtype the record name selects.
    pub subtype: cadmpeg_ir::geometry::VariableBlendSurfaceSubtype,
    /// The revision integer that gates the layout.
    pub revision: i64,
    /// Two ordered embedded support sides.
    pub sides: Box<[EmbeddedRollingBallSide; 2]>,
    /// The embedded slice curve.
    pub slice: CurveGeometry,
    /// Optional parameter bounds of the slice curve.
    pub slice_range: [Option<f64>; 2],
    /// Two side offsets in document length units.
    pub offsets: [f64; 2],
    /// The radius-law kind of the blend.
    pub radius_kind: cadmpeg_ir::geometry::VariableBlendRadiusKind,
    /// The first radius-law value.
    pub first_value: cadmpeg_ir::geometry::VariableBlendValue,
    /// The second radius-law value, when serialized.
    pub second_value: Option<cadmpeg_ir::geometry::VariableBlendValue>,
    /// The cross-section law, when serialized.
    pub cross_section: Option<cadmpeg_ir::geometry::VariableBlendCrossSection>,
    /// Support-side parameter interval `(T0, T1)`.
    pub u_range: [Option<f64>; 2],
    /// Second interval `(T lo, F)`: a lower bound with an unbounded-above
    /// marker decoding to `[Some(lo), None]`.
    pub v_range: [Option<f64>; 2],
    /// Approximation-current flag (`1` when the cache is current).
    pub shape_prefix: i64,
    /// Requested fit tolerance.
    pub shape_parameter: f64,
    /// Achieved fit tolerance, at or below `shape_parameter`.
    pub shape_length: f64,
    /// Signed integer immediately before the shared tail's enum, taking the
    /// values `-1` and `1`.
    pub shape_tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm<RevisionSurfaceParameterization, VariableBlendSolvedCache>,
    /// Six discontinuity arrays of the shared tail.
    pub discontinuities: [Vec<f64>; 6],
    /// The boolean serialized after the discontinuity arrays.
    pub tail_flag: bool,
    /// Three integers closing the shared tail.
    pub tail_extensions: [i64; 3],
    /// A second embedded curve serialized after the tail, when present.
    pub secondary_curve: Option<CurveGeometry>,
    /// Optional parameter bounds of the secondary curve.
    pub secondary_range: [Option<f64>; 2],
    /// The convexity enum of the blend.
    pub convexity: cadmpeg_ir::geometry::VariableBlendConvexity,
    /// The render-mode enum of the blend.
    pub render_mode: cadmpeg_ir::geometry::VariableBlendRenderMode,
    /// Optional parameter bounds of the post curve.
    pub post_range: [Option<f64>; 2],
    /// An embedded curve closing the record, when present.
    pub post_curve: Option<NurbsCurve>,
    /// An embedded parameter curve closing the record, when present.
    pub post_pcurve: Option<NurbsPcurve>,
}

/// The geometry form of one vertex-blend boundary.
pub enum EmbeddedVertexBlendBoundaryGeometry {
    /// A circular boundary carried by an embedded curve.
    Circle {
        /// The embedded boundary curve.
        curve: CurveGeometry,
        /// Optional endpoint bounds of the boundary curve.
        curve_endpoints: [Option<f64>; 2],
        /// The form integer serialized after the endpoints.
        form: i64,
        /// Counted list of twist points.
        twists: Vec<Point3>,
        /// Two parameters closing the circle form.
        parameters: [f64; 2],
        /// The sense boolean of the boundary.
        sense: bool,
    },
    /// A degenerate boundary collapsed to one location.
    Degenerate {
        /// The collapsed boundary location.
        location: Point3,
        /// Two boundary normals.
        normals: [Vector3; 2],
    },
    /// A boundary carried by a parameter curve on a support surface.
    Pcurve {
        /// The embedded support surface.
        surface: SurfaceGeometry,
        /// Optional UV bounds of the support surface.
        support_bounds: [Option<f64>; 4],
        /// The embedded NURBS parameter curve.
        pcurve: Option<NurbsPcurve>,
        /// The sense boolean of the boundary.
        sense: bool,
        /// The fit tolerance of the boundary approximation.
        fit_tolerance: f64,
    },
    /// A planar boundary carried by a normal and an embedded curve.
    Plane {
        /// The plane normal.
        normal: Vector3,
        /// Two parameters serialized after the normal.
        parameters: [f64; 2],
        /// The embedded boundary curve.
        curve: CurveGeometry,
        /// Optional endpoint bounds of the boundary curve.
        curve_endpoints: [Option<f64>; 2],
    },
}

/// One boundary of an embedded vertex blend.
pub struct EmbeddedVertexBlendBoundary {
    /// The boundary-type boolean.
    pub boundary_type: bool,
    /// The vector serialized after the boundary type.
    pub magic: Vector3,
    /// The U-smoothing boolean.
    pub u_smoothing: bool,
    /// The V-smoothing boolean.
    pub v_smoothing: bool,
    /// The fullness value of the boundary.
    pub fullness: f64,
    /// The geometry form of the boundary.
    pub geometry: EmbeddedVertexBlendBoundaryGeometry,
}

/// Embedded native vertex blend before stable IR ids are assigned.
pub struct EmbeddedVertexBlend {
    /// The revision integer that gates the layout, when serialized.
    pub revision: Option<i64>,
    /// The embedded boundary graphs, in stream order.
    pub boundaries: Vec<EmbeddedVertexBlendBoundary>,
    /// The approximation grid size.
    pub grid_size: i64,
    /// The fit tolerance of the patch approximation.
    pub fit_tolerance: f64,
}

/// The radius selector of an embedded rolling-ball blend.
pub enum EmbeddedRollingBallRadiusSelector {
    /// No selector value is serialized.
    None,
    /// The serialized selector value.
    Value(f64),
}

/// Embedded native rolling-ball graph before stable IR ids are assigned.
pub struct EmbeddedRollingBall {
    /// The subtype-table index of the record's own definition.
    pub definition_index: i64,
    /// Two ordered embedded support sides.
    pub sides: Box<[EmbeddedRollingBallSide; 2]>,
    /// The embedded slice curve.
    pub slice: CurveGeometry,
    /// Optional parameter bounds of the slice curve.
    pub slice_range: [Option<f64>; 2],
    /// Two side offsets in document length units.
    pub offsets: [f64; 2],
    /// The radius selector of the blend.
    pub radius_selector: EmbeddedRollingBallRadiusSelector,
    /// Support-side parameter interval `(T0, T1)`.
    pub u_range: [Option<f64>; 2],
    /// Second interval; `None` marks an unbounded end.
    pub v_range: [Option<f64>; 2],
    /// Approximation-current flag (`1` when the cache is current).
    pub shape_prefix: i64,
    /// Two parameters serialized after the shape prefix.
    pub parameters: [f64; 2],
    /// The integer closing the shape block.
    pub tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm,
    /// Six discontinuity arrays of the shared tail.
    pub discontinuities: [Vec<f64>; 6],
    /// The boolean serialized after the discontinuity arrays.
    pub tail_flag: bool,
    /// The optional third support side.
    pub third: Option<Box<EmbeddedRollingBallThirdSide>>,
    /// Three integers closing the shared tail.
    pub tail_extensions: [i64; 3],
}

/// One embedded support side of a G2 blend.
pub struct EmbeddedG2Side {
    /// The leading identifier of the side.
    pub label: String,
    /// The embedded support surface.
    pub surface: SurfaceGeometry,
    /// The embedded support curve.
    pub curve: NurbsCurve,
    /// Two embedded NURBS parameter curves on the support surface.
    pub pcurves: [Option<NurbsPcurve>; 2],
    /// The support direction vector.
    pub direction: Vector3,
}

/// The shape block serialized after a G2 blend's first side.
pub enum EmbeddedG2FirstShape {
    /// The full form: an optional surface cache and tolerance.
    Full {
        /// The embedded shape surface, when serialized.
        surface: Option<NurbsSurface>,
        /// The fit tolerance, when serialized.
        tolerance: Option<f64>,
    },
    /// The reduced form: nine coefficients and a tolerance.
    None {
        /// Nine shape coefficients.
        coefficients: [f64; 9],
        /// The fit tolerance.
        tolerance: f64,
        /// The bridge token serialized after the tolerance, when present.
        extension: Option<cadmpeg_ir::geometry::LoftBridgeToken>,
        /// The embedded parameter curve closing the block, when present.
        pcurve: Option<NurbsPcurve>,
    },
}

/// Embedded native G2 blend graph before stable IR ids are assigned.
pub struct EmbeddedG2Blend {
    /// The first embedded support side.
    pub first: EmbeddedG2Side,
    /// The singularity integer serialized after the first side.
    pub singularity: i64,
    /// The shape block of the first side.
    pub first_shape: EmbeddedG2FirstShape,
    /// The second embedded support side.
    pub second: EmbeddedG2Side,
    /// The exact surface of the second side.
    pub second_exact_surface: NurbsSurface,
    /// The embedded center curve.
    pub center_curve: NurbsCurve,
    /// Two parameters of the center curve.
    pub center_parameters: [f64; 2],
    /// The integer serialized after the center parameters.
    pub center_flag: i64,
    /// Two UV parameter intervals.
    pub parameter_ranges: [[f64; 2]; 2],
    /// Four parameters closing the record body.
    pub trailing_parameters: [f64; 4],
    /// Three discontinuity arrays.
    pub discontinuities: [Vec<f64>; 3],
}

#[allow(clippy::option_option)] // Outer None is parse failure; inner None is native nullbs.
pub(crate) fn decode_nullable_embedded_pcurve(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<Option<NurbsPcurve>> {
    let saved = *position;
    if take_native_ident(bytes, position).as_deref() == Some("nullbs") {
        return Some(None);
    }
    *position = saved;
    let (pcurve, end) = decode_pcurve_block_with_end(bytes, *position, int_width)?;
    *position = end;
    Some(Some(pcurve))
}

/// Decode a `nullbs`-or-2D-block pcurve slot. Token-space counterpart of
/// [`decode_nullable_embedded_pcurve`].
#[allow(clippy::option_option)] // Outer None is parse failure; inner None is native nullbs.
pub(crate) fn nullable_embedded_pcurve(cur: &mut Cur<'_>) -> Option<Option<NurbsPcurve>> {
    let saved = cur.pos();
    if cur.take_ident() == Some("nullbs") {
        return Some(None);
    }
    cur.set_pos(saved);
    let (pcurve, end) = pcurve_block_with_end(cur.toks(), cur.pos())?;
    cur.set_pos(end);
    Some(Some(pcurve))
}

fn g2_side(cur: &mut Cur<'_>) -> Option<EmbeddedG2Side> {
    let label = cur.take_str()?.to_string();
    let surface = embedded_surface(cur)?;
    let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
    cur.set_pos(curve_end);
    let first = nullable_embedded_pcurve(cur)?;
    let direction = cur.take_vector3()?;
    let second = nullable_embedded_pcurve(cur)?;
    Some(EmbeddedG2Side {
        label,
        surface,
        curve,
        pcurves: [first, second],
        direction: Vector3::new(direction[0], direction[1], direction[2]),
    })
}

fn bridge_token(cur: &mut Cur<'_>) -> Option<cadmpeg_ir::geometry::LoftBridgeToken> {
    use cadmpeg_ir::geometry::LoftBridgeToken;
    match cur.peek()? {
        Token::True | Token::False => Some(LoftBridgeToken::Boolean(cur.take_bool()?)),
        Token::Long(_) => Some(LoftBridgeToken::Integer(cur.take_long()?)),
        Token::Double(_) => Some(LoftBridgeToken::Double(cur.take_f64()?)),
        Token::Enum(_) => Some(LoftBridgeToken::Enum(cur.take_enum()?)),
        Token::Str(_) => Some(LoftBridgeToken::Text(cur.take_str()?.to_string())),
        _ => None,
    }
}

#[allow(
    clippy::option_option,
    reason = "outer None rejects malformed trailing fields; inner None is a valid absent tolerance"
)]
fn optional_trailing_cache_tolerance(cur: &mut Cur<'_>) -> Option<Option<f64>> {
    if cur.at_scope_end() {
        Some(None)
    } else {
        let tolerance = cur.take_f64()? * LEN_TO_MM;
        cur.at_scope_end().then_some(Some(tolerance))
    }
}

fn g2_blend_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let names = ["g2_blend_spl_sur", "g2blnsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Revision-gated layout: revision integer, two scalars, two sides in
        // the variable-blend side layout, center curve with endpoints, two
        // radii, radius selector, optional U/V bounds, shape prologue,
        // shared tail, and three trailing integers. The modern name uses this
        // layout.
        (name == "g2_blend_spl_sur").then_some(())?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let leading_parameters = [cur.take_f64()?, cur.take_f64()?];
        let sides = Box::new([
            rolling_ball_side(&mut cur, resolver)?,
            rolling_ball_side(&mut cur, resolver)?,
        ]);
        let table = resolver?;
        let center = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let center_range = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let radii = [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM];
        let radius_selector = cur.take_enum()?;
        let u_range = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let v_range = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let shape_prefix = cur.take_long()?;
        let shape_parameter = cur.take_f64()?;
        let shape_length = cur.take_f64()? * LEN_TO_MM;
        let shape_tail = cur.take_long()?;
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            solved_cache_domains: _,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        let tail_extensions = [cur.take_long()?, cur.take_long()?, cur.take_long()?];
        cur.at_scope_end().then_some(())?;
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::RevisionG2Blend(Box::new(
                EmbeddedRevisionG2Blend {
                    revision,
                    leading_parameters,
                    sides,
                    center: CurveGeometry::Nurbs(center),
                    center_range,
                    radii,
                    radius_selector,
                    u_range,
                    v_range,
                    shape_prefix,
                    shape_parameter,
                    shape_length,
                    shape_tail,
                    cache: revision_cache_form(tail_enum, fit_tolerance, parameterization)?,
                    discontinuities,
                    tail_flag,
                    tail_extensions,
                },
            )),
            cache_fit_tolerance: fit_tolerance,
        });
    }
    let first = g2_side(&mut cur)?;
    let singularity = cur.take_enum()?;
    let first_shape = if cur.peek().is_some_and(Token::is_payload_ident) {
        let saved = cur.pos();
        if cur.take_ident() == Some("nullbs") {
            EmbeddedG2FirstShape::Full {
                surface: None,
                tolerance: None,
            }
        } else {
            cur.set_pos(saved);
            let (surface, surface_end) = surface_block(span, cur.pos())?;
            cur.set_pos(surface_end);
            EmbeddedG2FirstShape::Full {
                surface: Some(surface),
                tolerance: Some(cur.take_f64()? * LEN_TO_MM),
            }
        }
    } else {
        let mut coefficients = [0.0; 9];
        for coefficient in &mut coefficients {
            *coefficient = cur.take_f64()?;
        }
        let tolerance = cur.take_f64()? * LEN_TO_MM;
        let extension = (!matches!(cur.peek(), Some(token)
            if matches!(token, Token::Str(_)) || token.is_payload_ident()))
        .then(|| bridge_token(&mut cur))
        .flatten();
        let pcurve = nullable_embedded_pcurve(&mut cur)?;
        EmbeddedG2FirstShape::None {
            coefficients,
            tolerance,
            extension,
            pcurve,
        }
    };
    let second = g2_side(&mut cur)?;
    let (second_exact, second_exact_end) = surface_block(span, cur.pos())?;
    cur.set_pos(second_exact_end);
    let (center, center_end) = curve_block(span, cur.pos())?;
    cur.set_pos(center_end);
    let center_parameters = [cur.take_f64()?, cur.take_f64()?];
    let center_flag = cur.take_long()?;
    let parameter_ranges = [
        [cur.take_f64()?, cur.take_f64()?],
        [cur.take_f64()?, cur.take_f64()?],
    ];
    let mut trailing_parameters = [0.0; 4];
    for parameter in &mut trailing_parameters {
        *parameter = cur.take_f64()?;
    }
    let (_, cache_end) = surface_block(span, cur.pos())?;
    let cache_fit_tolerance = match span.get(cache_end) {
        Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
        _ => None,
    };
    cur.set_pos(cache_end + usize::from(cache_fit_tolerance.is_some()));
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::G2Blend(Box::new(EmbeddedG2Blend {
            first,
            singularity,
            first_shape,
            second,
            second_exact_surface: second_exact,
            center_curve: center,
            center_parameters,
            center_flag,
            parameter_ranges,
            trailing_parameters,
            discontinuities,
        })),
        cache_fit_tolerance,
    })
}

/// The support data serialized after a loft profile member's curve.
pub struct EmbeddedLoftProfileData {
    /// The embedded support surface, when serialized.
    pub surface: Option<SurfaceGeometry>,
    /// Optional UV bounds of the support surface.
    pub support_bounds: [Option<f64>; 4],
    /// The embedded NURBS parameter curve on the support surface.
    pub pcurve: Option<NurbsPcurve>,
    /// A second embedded parameter curve, when serialized.
    pub secondary_pcurve: Option<NurbsPcurve>,
    /// The boolean serialized before the subdata, when present.
    pub first_flag: Option<bool>,
    /// The ASM extension integer, when serialized.
    pub asm_extension: Option<i64>,
    /// Neutral subdata fields of the member.
    pub subdata: cadmpeg_ir::geometry::LoftSubdata,
    /// The member direction vector, when serialized.
    pub direction: Option<Vector3>,
}

/// One profile member of an embedded loft section.
pub struct EmbeddedLoftProfileMember {
    /// The member type code.
    pub type_code: i64,
    /// The embedded profile curve.
    pub curve: NurbsCurve,
    /// Optional endpoint bounds of the profile curve.
    pub endpoints: Option<[Option<f64>; 2]>,
    /// The support data of the member.
    pub data: EmbeddedLoftProfileData,
}

/// The path block of an embedded loft section.
pub struct EmbeddedLoftPath {
    /// The embedded path curve, when serialized.
    pub curve: Option<NurbsCurve>,
    /// Optional endpoint bounds of the path curve.
    pub endpoints: Option<[Option<f64>; 2]>,
    /// Auxiliary embedded curves, in stream order.
    pub auxiliaries: Vec<NurbsCurve>,
    /// The integer closing the path block.
    pub flag: i64,
}

/// Embedded revision-gated compound loft before stable IR ids are assigned.
pub struct EmbeddedRevisionCompoundLoft {
    /// The revision integer that gates the layout.
    pub revision: i64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm,
    /// Six discontinuity arrays of the shared tail.
    pub discontinuities: [Vec<f64>; 6],
    /// The boolean serialized after the discontinuity arrays.
    pub tail_flag: bool,
    /// The base profile members, in stream order.
    pub base_profile: Vec<EmbeddedLoftProfileMember>,
    /// The base path block.
    pub base_path: EmbeddedLoftPath,
    /// The section entries, in stream order.
    pub entries: Vec<EmbeddedLoftSectionEntry>,
    /// Two booleans serialized after the entries.
    pub flags: [bool; 2],
    /// The kind integer of the loft.
    pub kind: i64,
    /// Two booleans serialized after the kind.
    pub kind_flags: [bool; 2],
    /// The loft direction selected after the kind flags.
    pub direction: EmbeddedCompoundLoftDirection,
    /// Optional parameter bounds; `None` marks an unbounded end.
    pub interval: [Option<f64>; 2],
    /// An embedded curve closing the record, when present.
    pub trailing_curve: Option<NurbsCurve>,
}

/// One section entry of an embedded loft.
pub struct EmbeddedLoftSectionEntry {
    /// The section parameter.
    pub parameter: f64,
    /// The profile members of the section, in stream order.
    pub profile: Vec<EmbeddedLoftProfileMember>,
    /// The path block of the section.
    pub path: EmbeddedLoftPath,
}

/// Embedded native loft graph before its carriers receive stable IR ids.
pub struct EmbeddedLoft {
    /// The two section lists, in stream order.
    pub sections: [Vec<EmbeddedLoftSectionEntry>; 2],
    /// The revision-gated form of the layout, when serialized.
    pub revision_form: Option<cadmpeg_ir::geometry::LoftRevisionForm>,
    /// Neutral surface parameters of the loft.
    pub parameters: cadmpeg_ir::geometry::SplineSurfaceParameters,
    /// Two closure enums.
    pub closures: [i64; 2],
    /// Two singularity enums.
    pub singularities: [i64; 2],
    /// The mode integer of the loft.
    pub mode: i64,
    /// The bridge tokens closing the record, in stream order.
    pub bridge: Vec<cadmpeg_ir::geometry::LoftBridgeToken>,
}

/// One scale block of an embedded compound loft.
pub struct EmbeddedCompoundLoftScale {
    /// The profile members of the block, in stream order.
    pub members: Vec<EmbeddedLoftProfileMember>,
    /// The embedded path curve.
    pub path: NurbsCurve,
    /// Auxiliary embedded curves, in stream order.
    pub auxiliaries: Vec<NurbsCurve>,
    /// Two integers closing the block.
    pub tail: [i64; 2],
}

/// The direction carrier of a compound loft tail.
pub enum EmbeddedCompoundLoftDirection {
    /// A direction vector.
    Vector(Vector3),
    /// An embedded direction curve.
    Curve {
        /// Exact nonzero selector serialized before the curve.
        selector: NonZeroI64,
        /// Embedded direction curve.
        curve: NurbsCurve,
    },
}

/// The kind-discriminated tail of an embedded compound loft.
pub enum EmbeddedCompoundLoftTail {
    /// The kind-6 tail: one scale block and a ranged curve.
    Six {
        /// Two booleans opening the tail.
        flags: [bool; 2],
        /// The scale block of the tail.
        scale: Box<EmbeddedCompoundLoftScale>,
        /// The integer selector serialized after the scale block.
        selector: i64,
        /// The tail direction vector.
        direction: Vector3,
        /// Two parameter-range doubles.
        parameter_range: [f64; 2],
        /// The embedded curve closing the tail.
        curve: NurbsCurve,
    },
    /// The kind-7 tail: two flagged scale blocks.
    Seven {
        /// The boolean before the first scale block.
        first_flag: bool,
        /// The first scale block, when serialized.
        first_scale: Option<Box<EmbeddedCompoundLoftScale>>,
        /// The boolean before the second scale block.
        second_flag: bool,
        /// The second scale block.
        second_scale: Box<EmbeddedCompoundLoftScale>,
        /// The integer selector serialized after the scale blocks.
        selector: i64,
        /// The tail direction vector.
        direction: Vector3,
        /// Two booleans closing the tail.
        trailing_flags: [bool; 2],
    },
    /// The kind-0 tail: a direction carrier without scale blocks.
    Zero {
        /// Two booleans opening the tail.
        flags: [bool; 2],
        /// The integer selector serialized after the flags.
        selector: i64,
        /// The direction carrier of the tail.
        direction: EmbeddedCompoundLoftDirection,
        /// Two booleans closing the tail.
        trailing_flags: [bool; 2],
    },
}

/// Embedded native compound loft before stable IR ids are assigned.
pub struct EmbeddedCompoundLoft {
    /// Four ordered optional scale blocks.
    pub scales: Box<[Option<EmbeddedCompoundLoftScale>; 4]>,
    /// A fifth scale block, when serialized.
    pub fifth_scale: Option<Box<EmbeddedCompoundLoftScale>>,
    /// Two booleans serialized after the scale blocks.
    pub flags: [bool; 2],
    /// The kind-discriminated tail.
    pub tail: EmbeddedCompoundLoftTail,
}

/// The shape block of an embedded scaled compound loft.
pub enum EmbeddedScaledCompoundLoftShape {
    /// The full form carries no extra fields.
    Full,
    /// The reduced form: parameter ranges and arrays in place of a cache.
    None {
        /// Two UV parameter intervals.
        parameter_ranges: [[f64; 2]; 2],
        /// Two parameter arrays.
        parameters: [Vec<f64>; 2],
    },
}

/// The branch-discriminated tail of an embedded scaled compound loft.
pub enum EmbeddedScaledCompoundLoftBranch {
    /// The extended branch closed by a direction vector.
    ExtendedVector {
        /// The first scale block, when serialized.
        first_scale: Option<Box<EmbeddedCompoundLoftScale>>,
        /// The second scale block.
        second_scale: Box<EmbeddedCompoundLoftScale>,
        /// The integer selector serialized after the scale blocks.
        selector: i64,
        /// The branch direction vector.
        direction: Vector3,
    },
    /// The extended branch closed by an embedded curve.
    ExtendedCurve {
        /// The scale block, when serialized.
        scale: Option<Box<EmbeddedCompoundLoftScale>>,
        /// The boolean serialized after the scale block.
        flag: bool,
        /// The singularity integer of the branch.
        singularity: i64,
        /// The embedded curve closing the branch.
        curve: NurbsCurve,
    },
    /// The direct branch: a flag, selector, and direction carrier.
    Direct {
        /// The boolean opening the branch.
        flag: bool,
        /// The integer selector serialized after the flag.
        selector: i64,
        /// The direction carrier of the branch.
        direction: EmbeddedCompoundLoftDirection,
    },
}

/// Embedded native scaled compound loft before stable IR ids are assigned.
pub struct EmbeddedScaledCompoundLoft {
    /// The singularity integer opening the record body.
    pub singularity: i64,
    /// The shape block of the loft.
    pub shape: EmbeddedScaledCompoundLoftShape,
    /// Six discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// The boolean serialized after the discontinuity arrays.
    pub discontinuity_flag: bool,
    /// Three ordered optional scale blocks.
    pub scales: Box<[Option<EmbeddedCompoundLoftScale>; 3]>,
    /// Two booleans serialized after the scale blocks.
    pub flags: [bool; 2],
    /// The integer selector serialized after the flags.
    pub selector: i64,
    /// The branch-discriminated tail.
    pub branch: EmbeddedScaledCompoundLoftBranch,
    /// Two booleans serialized after the branch.
    pub trailing_flags: [bool; 2],
    /// The kind integer of the closing tail.
    pub tail_kind: i64,
    /// Two direction vectors of the closing tail.
    pub tail_directions: [Vector3; 2],
    /// The singularity integer of the closing tail.
    pub tail_singularity: i64,
    /// The embedded curve closing the record.
    pub tail_curve: NurbsCurve,
}

/// One law-expression operand of an embedded law formula.
pub enum EmbeddedLawExpression {
    /// A null operand.
    Null,
    /// A serializer-preserved textual law expression.
    Text(String),
    /// An integer operand.
    Integer(i64),
    /// A double operand.
    Double(f64),
    /// A point operand.
    Point(Point3),
    /// A vector operand.
    Vector(Vector3),
    /// A transform operand of thirteen scalars and three enums.
    Transform {
        /// Thirteen transform scalars.
        scalars: [f64; 13],
        /// Three transform enums.
        enums: [i64; 3],
    },
    /// A vector-form transform operand.
    TransformVec {
        /// Four transform vectors.
        vectors: [Vector3; 4],
        /// The transform scale.
        scale: f64,
        /// Three transform booleans.
        flags: [bool; 3],
    },
    /// An edge operand carrying an embedded curve.
    Edge {
        /// The embedded edge curve.
        curve: NurbsCurve,
        /// Optional endpoint bounds of the edge curve.
        endpoints: Option<[Option<f64>; 2]>,
        /// Two parameters closing the operand.
        parameters: [f64; 2],
    },
    /// A spline operand carrying raw knot and control arrays.
    Spline {
        /// The native identifier of the spline.
        native_id: i64,
        /// The raw knot array.
        knots: Vec<f64>,
        /// The raw control array.
        controls: Vec<f64>,
        /// The point closing the operand.
        point: Point3,
    },
    /// An algebraic operand applying an operator to nested operands.
    Algebraic {
        /// The operator name.
        operator: String,
        /// The nested operands, in stream order.
        operands: Vec<EmbeddedLawExpression>,
    },
}

/// One law formula: a name and its operand list.
pub struct EmbeddedLawFormula {
    /// The formula name.
    pub name: String,
    /// The formula operands, in stream order.
    pub variables: Vec<EmbeddedLawExpression>,
}

/// Embedded native law surface before stable IR ids are assigned.
pub struct EmbeddedLawSurface {
    /// Two UV parameter intervals, when serialized.
    pub parameter_ranges: Option<[[f64; 2]; 2]>,
    /// The law formula that drives the surface.
    pub primary: EmbeddedLawFormula,
    /// Additional law formulas serialized after the primary.
    pub additional: Vec<EmbeddedLawFormula>,
    /// Neutral tail fields of the record.
    pub tail: cadmpeg_ir::geometry::LawSurfaceTail,
    /// Six discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
}

/// The layout-discriminated body of an embedded skin surface.
pub enum EmbeddedSkinSurfaceLayout {
    /// The profile-list form.
    Profiles {
        /// The profile members, in stream order.
        profiles: Vec<EmbeddedLoftProfileMember>,
        /// The embedded path curve.
        path: NurbsCurve,
        /// Two integers closing the form.
        tail: [i64; 2],
    },
    /// The compact two-curve form.
    Compact {
        /// The first embedded curve.
        curve: NurbsCurve,
        /// Neutral subdata fields of the first curve.
        subdata: cadmpeg_ir::geometry::LoftSubdata,
        /// The integer serialized after the first curve.
        first_tail: i64,
        /// The second embedded curve.
        secondary_curve: NurbsCurve,
        /// The integer serialized after the second curve.
        second_tail: i64,
    },
}

/// Embedded native skin surface before stable IR ids are assigned.
pub struct EmbeddedSkinSurface {
    /// The first integer of the record body.
    pub surface_boolean: i64,
    /// The surface-normal integer.
    pub surface_normal: i64,
    /// The surface-direction integer.
    pub surface_direction: i64,
    /// The profile count.
    pub count: i64,
    /// The parameter serialized after the count.
    pub parameter: f64,
    /// The inner profile count.
    pub inner_count: i64,
    /// The layout-discriminated body.
    pub layout: EmbeddedSkinSurfaceLayout,
    /// The skin direction vector.
    pub direction: Vector3,
    /// The parameter serialized after the direction.
    pub trailing_parameter: f64,
    /// The law formula of the skin.
    pub formula: EmbeddedLawFormula,
    /// The embedded parameter curve of the skin.
    pub parameter_curve: NurbsCurve,
    /// Six discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// The boolean serialized after the discontinuity arrays.
    pub discontinuity_flag: bool,
}

/// Embedded native net surface before stable IR ids are assigned.
pub struct EmbeddedNetSurface {
    /// The two section lists, in stream order.
    pub sections: Box<[Vec<EmbeddedLoftSectionEntry>; 2]>,
    /// Twelve frame parameters.
    pub frame_parameters: [f64; 12],
    /// The integer serialized after the frame parameters.
    pub flag: i64,
    /// Four direction vectors.
    pub directions: [Vector3; 4],
    /// Four law formulas, in stream order.
    pub formulas: Box<[EmbeddedLawFormula; 4]>,
    /// Six discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// The boolean serialized after the discontinuity arrays.
    pub discontinuity_flag: bool,
}

/// The layout-discriminated body of an embedded sweep surface.
pub enum EmbeddedSweepSurfaceLayout {
    /// The profile-first form: profile, spine, and a formula triple.
    ProfileFirst {
        /// The embedded profile curve.
        profile: NurbsCurve,
        /// The embedded spine curve.
        spine: NurbsCurve,
        /// The secondary kind integer.
        secondary_kind: i64,
        /// Five direction vectors.
        directions: [Vector3; 5],
        /// The sweep origin point.
        origin: Point3,
        /// Four parameters closing the form.
        parameters: [f64; 4],
        /// Three law formulas, in stream order.
        formulas: Box<[EmbeddedLawFormula; 3]>,
    },
    /// The explicit form closed by one law formula.
    ExplicitFormula {
        /// The embedded profile curve.
        profile: NurbsCurve,
        /// The mode integer of the sweep.
        mode: i64,
        /// Two parameter bounds of the profile curve.
        profile_range: [f64; 2],
        /// The profile frame point and vector, when serialized.
        profile_frame: Option<(Point3, Vector3)>,
        /// The sweep origin point.
        origin: Point3,
        /// Three direction vectors.
        directions: [Vector3; 3],
        /// The boolean serialized before the path.
        trajectory_flag: bool,
        /// The embedded path curve.
        path: NurbsCurve,
        /// Two parameter bounds of the path curve.
        path_range: [f64; 2],
        /// The parameter serialized after the path range.
        path_parameter: f64,
        /// The boolean serialized before the formula.
        formula_flag: bool,
        /// The law formula closing the form.
        formula: EmbeddedLawFormula,
        /// The boolean closing the form.
        trailing_flag: bool,
    },
    /// The explicit form closed by a guide curve.
    ExplicitGuide {
        /// The embedded profile curve.
        profile: NurbsCurve,
        /// The mode integer of the sweep.
        mode: i64,
        /// Two parameter bounds of the profile curve.
        profile_range: [f64; 2],
        /// The profile frame point and vector, when serialized.
        profile_frame: Option<(Point3, Vector3)>,
        /// The sweep origin point.
        origin: Point3,
        /// Three direction vectors.
        directions: [Vector3; 3],
        /// The boolean serialized before the path.
        trajectory_flag: bool,
        /// The embedded path curve.
        path: NurbsCurve,
        /// Two parameter bounds of the path curve.
        path_range: [f64; 2],
        /// The parameter serialized after the path range.
        path_parameter: f64,
        /// Two booleans serialized before the guide curve.
        guide_flags: [bool; 2],
        /// The embedded guide curve.
        guide_curve: NurbsCurve,
        /// Two parameter bounds of the guide curve.
        guide_range: [f64; 2],
        /// Two mode integers of the guide.
        guide_modes: [i64; 2],
        /// Six parameters of the guide.
        guide_parameters: [f64; 6],
        /// Three booleans closing the form.
        trailing_flags: [bool; 3],
    },
    /// The explicit form closed by a support surface.
    ExplicitSurface {
        /// The embedded profile curve.
        profile: NurbsCurve,
        /// The mode integer of the sweep.
        mode: i64,
        /// Two parameter bounds of the profile curve.
        profile_range: [f64; 2],
        /// The profile frame point and vector, when serialized.
        profile_frame: Option<(Point3, Vector3)>,
        /// The sweep origin point.
        origin: Point3,
        /// Three direction vectors.
        directions: [Vector3; 3],
        /// The boolean serialized before the path.
        trajectory_flag: bool,
        /// The embedded path curve.
        path: NurbsCurve,
        /// Two parameter bounds of the path curve.
        path_range: [f64; 2],
        /// The parameter serialized after the path range.
        path_parameter: f64,
        /// The singularity integer of the form.
        singularity: i64,
        /// The embedded support surface.
        support_surface: SurfaceGeometry,
        /// An auxiliary embedded curve, when serialized.
        auxiliary_curve: Option<NurbsCurve>,
        /// The boolean serialized after the support surface.
        support_flag: bool,
        /// The legacy boolean closing the form, when serialized.
        legacy_flag: Option<bool>,
    },
    /// The law-driven form: two law expressions and one formula.
    LawDriven {
        /// The embedded profile curve.
        profile: NurbsCurve,
        /// The mode integer of the sweep.
        mode: i64,
        /// Two parameter bounds of the profile curve.
        profile_range: [f64; 2],
        /// The profile frame point and vector, when serialized.
        profile_frame: Option<(Point3, Vector3)>,
        /// The sweep origin point.
        origin: Point3,
        /// Three direction vectors.
        directions: [Vector3; 3],
        /// The first law expression.
        first_law: EmbeddedLawExpression,
        /// The mode integer of the first law.
        first_mode: i64,
        /// Two parameter bounds of the first law.
        first_range: [f64; 2],
        /// The law direction vector.
        law_direction: Vector3,
        /// The mode integer serialized before the path.
        path_mode: i64,
        /// The boolean serialized before the path.
        path_flag: bool,
        /// The embedded path curve.
        path: NurbsCurve,
        /// Two parameter bounds of the path curve.
        path_range: [f64; 2],
        /// The parameter serialized after the path range.
        path_parameter: f64,
        /// The boolean serialized before the second law.
        second_law_flag: bool,
        /// The second law expression.
        second_law: EmbeddedLawExpression,
        /// The mode integer serialized before the formula.
        formula_mode: i64,
        /// The law formula closing the form.
        formula: EmbeddedLawFormula,
        /// The boolean closing the form.
        trailing_flag: bool,
    },
}

/// Embedded native sweep surface before stable IR ids are assigned.
pub struct EmbeddedSweepSurface {
    /// The primary kind integer of the record.
    pub primary_kind: i64,
    /// The revision-gated form of the layout, when serialized.
    pub revision_form: Option<cadmpeg_ir::geometry::SweepRevisionForm>,
    /// The layout-discriminated body.
    pub layout: EmbeddedSweepSurfaceLayout,
    /// Six discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// The boolean serialized after the discontinuity arrays.
    pub discontinuity_flag: bool,
}

/// Embedded native deformable surface before stable support ids are assigned.
pub struct EmbeddedDeformableSurface {
    /// The embedded support surface.
    pub support: SurfaceGeometry,
    /// Revision-gated fields surrounding the support and shared surface tail.
    pub revision_form: Option<cadmpeg_ir::geometry::RevisionSurfaceForm>,
    /// The mode-discriminated payload.
    pub data: EmbeddedDeformableSurfaceData,
    /// Six discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// The boolean serialized after the discontinuity arrays.
    pub discontinuity_flag: bool,
}

/// The mode-discriminated payload of an embedded deformable surface.
pub enum EmbeddedDeformableSurfaceData {
    /// A payload fully resolved to neutral IR fields.
    Resolved(cadmpeg_ir::geometry::DeformableSurfaceData),
    /// The surface-curve payload: an embedded surface, curve, and frame.
    SurfaceCurve {
        /// The embedded payload surface.
        surface: SurfaceGeometry,
        /// The native identifier of the payload.
        native_id: i64,
        /// The boolean serialized after the native identifier.
        flag: bool,
        /// The parameter serialized after the flag.
        first_parameter: f64,
        /// The integer selector of the payload.
        selector: i64,
        /// The parameter serialized after the selector.
        second_parameter: f64,
        /// The embedded payload curve.
        curve: NurbsCurve,
        /// Four frame vectors.
        vectors: [Vector3; 4],
        /// The parameter serialized after the frame vectors.
        frame_parameter: f64,
        /// Three booleans serialized after the frame parameter.
        flags: [bool; 3],
        /// Counted list of parameter triples.
        parameter_triples: Vec<[f64; 3]>,
    },
    /// The full payload: a leading frame, surface, curve, and vector frames.
    Full {
        /// Four ordered leading frame vectors.
        leading_vectors: [Vector3; 4],
        /// The parameter serialized after the leading vectors.
        leading_parameter: f64,
        /// Three booleans serialized after the leading parameter.
        leading_flags: [bool; 3],
        /// The integer selector of the payload.
        selector: i64,
        /// The embedded payload surface.
        surface: SurfaceGeometry,
        /// The native identifier of the payload.
        native_id: i64,
        /// The boolean serialized after the native identifier.
        flag: bool,
        /// The parameter serialized after the flag.
        first_parameter: f64,
        /// The version integer, when serialized.
        version_value: Option<i64>,
        /// The parameter serialized after the version value.
        second_parameter: f64,
        /// The embedded payload curve.
        curve: NurbsCurve,
        /// Two deformation vector frames.
        frames: Box<[cadmpeg_ir::geometry::DeformableVectorFrame; 2]>,
        /// The integer closing the payload.
        trailing_value: i64,
    },
}

#[allow(clippy::option_option)] // Outer None is parse failure; inner None is an absent scale slot.
fn compound_loft_scale(cur: &mut Cur<'_>) -> Option<Option<EmbeddedCompoundLoftScale>> {
    if matches!(cur.peek(), Some(Token::True | Token::False)) {
        return Some(None);
    }
    let count = usize::try_from(cur.take_long()?).ok()?;
    if count > 100_000 {
        return None;
    }
    let mut members = Vec::with_capacity(count);
    for _ in 0..count {
        let type_code = cur.take_long()?;
        let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
        cur.set_pos(curve_end);
        let data = loft_profile_data(cur)?;
        members.push(EmbeddedLoftProfileMember {
            type_code,
            curve,
            endpoints: None,
            data,
        });
    }
    let (path, path_end) = curve_block(cur.toks(), cur.pos())?;
    cur.set_pos(path_end);
    let auxiliary_count = usize::try_from(cur.take_long()?).ok()?;
    if auxiliary_count > 100_000 {
        return None;
    }
    let mut auxiliaries = Vec::with_capacity(auxiliary_count);
    for _ in 0..auxiliary_count {
        let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
        cur.set_pos(curve_end);
        auxiliaries.push(curve);
    }
    let tail = [cur.take_long()?, cur.take_long()?];
    Some(Some(EmbeddedCompoundLoftScale {
        members,
        path,
        auxiliaries,
        tail,
    }))
}

/// Exact rational quadratic NURBS of a full native ellipse.
pub(crate) fn ellipse_to_nurbs(
    center: [f64; 3],
    normal: [f64; 3],
    major: [f64; 3],
    ratio: f64,
) -> Option<NurbsCurve> {
    let length = (major[0] * major[0] + major[1] * major[1] + major[2] * major[2]).sqrt();
    (length.is_finite() && length > 0.0).then_some(())?;
    let minor_direction = [
        normal[1] * major[2] - normal[2] * major[1],
        normal[2] * major[0] - normal[0] * major[2],
        normal[0] * major[1] - normal[1] * major[0],
    ];
    let minor_length = (minor_direction[0] * minor_direction[0]
        + minor_direction[1] * minor_direction[1]
        + minor_direction[2] * minor_direction[2])
        .sqrt();
    (minor_length.is_finite() && minor_length > 0.0).then_some(())?;
    let minor_scale = ratio * length / minor_length;
    let minor = [
        minor_direction[0] * minor_scale,
        minor_direction[1] * minor_scale,
        minor_direction[2] * minor_scale,
    ];
    let at = |mj: f64, mn: f64| {
        Point3::new(
            (center[0] + mj * major[0] + mn * minor[0]) * LEN_TO_MM,
            (center[1] + mj * major[1] + mn * minor[1]) * LEN_TO_MM,
            (center[2] + mj * major[2] + mn * minor[2]) * LEN_TO_MM,
        )
    };
    let w = std::f64::consts::FRAC_1_SQRT_2;
    Some(NurbsCurve {
        degree: 2,
        knots: vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ],
        control_points: vec![
            at(1.0, 0.0),
            at(1.0, 1.0),
            at(0.0, 1.0),
            at(-1.0, 1.0),
            at(-1.0, 0.0),
            at(-1.0, -1.0),
            at(0.0, -1.0),
            at(1.0, -1.0),
            at(1.0, 0.0),
        ],
        weights: Some(vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0]),
        periodic: false,
    })
}

/// Payload form of a revision-gated loft profile member, selected by the
/// member's type integer.
#[derive(Clone, Copy)]
enum RevisionLoftMemberForm {
    /// Nonzero type: bounded support surface, one nullable BS2 pcurve, and the
    /// first flag.
    Support,
    /// Zero type: two nullable BS2 pcurve slots and no first flag.
    PcurvePair,
}

impl RevisionLoftMemberForm {
    /// The form the member's type integer selects.
    fn of(type_code: i64) -> Self {
        if type_code == 0 {
            Self::PcurvePair
        } else {
            Self::Support
        }
    }
}

/// The highest stream save format version whose revision-gated loft profile
/// members omit the ASM integer between the member payload and its constraint
/// subdata. Save format 22300 through 22600 streams omit it; save format 23200
/// streams carry it.
const LOFT_ASM_EXTENSION_ABSENT_THROUGH: u32 = 22600;

/// Whether a revision-gated loft profile member in this stream carries the ASM
/// integer. The stream save format selects this gate. The record serializer
/// revision remains a separate field; one revision can carry the integer in a
/// later stream and omit it in an earlier stream.
fn revision_loft_carries_asm_extension(table: &SubtypeTable) -> bool {
    table
        .save_format_version()
        .is_none_or(|version| version > LOFT_ASM_EXTENSION_ABSENT_THROUGH)
}

/// Revision-gated loft profile data: the type-selected member payload, an
/// optional ASM integer, and constraint subdata with trailing row pairs.
fn revision_loft_profile_data(
    cur: &mut Cur<'_>,
    table: &SubtypeTable,
    form: RevisionLoftMemberForm,
    asm_extension_present: bool,
) -> Option<EmbeddedLoftProfileData> {
    let (surface, support_bounds, pcurve, secondary_pcurve, first_flag) = match form {
        RevisionLoftMemberForm::Support => {
            let (surface, support_bounds) = optional_embedded_surface_with_bounds(cur, table)?;
            let pcurve = nullable_embedded_pcurve(cur)?;
            let first_flag = cur.take_bool()?;
            (surface, support_bounds, pcurve, None, Some(first_flag))
        }
        RevisionLoftMemberForm::PcurvePair => {
            let pcurve = nullable_embedded_pcurve(cur)?;
            let secondary_pcurve = nullable_embedded_pcurve(cur)?;
            (None, [None; 4], pcurve, secondary_pcurve, None)
        }
    };
    let asm_extension = if asm_extension_present {
        Some(cur.take_long()?)
    } else {
        None
    };
    let subdata = loft_subdata_form(cur, true)?;
    let direction = if cur.take_bool()? {
        let value = cur.take_vector3()?;
        Some(Vector3::new(value[0], value[1], value[2]))
    } else {
        None
    };
    Some(EmbeddedLoftProfileData {
        surface,
        support_bounds,
        pcurve,
        secondary_pcurve,
        first_flag,
        asm_extension,
        subdata,
        direction,
    })
}

fn revision_loft_section(
    cur: &mut Cur<'_>,
    table: &SubtypeTable,
    asm_extension_present: bool,
) -> Option<Vec<EmbeddedLoftSectionEntry>> {
    let count = usize::try_from(cur.take_long()?).ok()?;
    // Each entry consumes at least one double token for its parameter.
    let count = bounded_len(count as u64, 1, cur.rest().len())?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let parameter = cur.take_f64()?;
        let member_count = usize::try_from(cur.take_long()?).ok()?;
        // Each member consumes at least its type-code token.
        let member_count = bounded_len(member_count as u64, 1, cur.rest().len())?;
        let mut profile = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            let type_code = cur.take_long()?;
            let curve = embedded_base_curve_resolving_refs(cur, table)?;
            let endpoints = [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ];
            let data = revision_loft_profile_data(
                cur,
                table,
                RevisionLoftMemberForm::of(type_code),
                asm_extension_present,
            )?;
            profile.push(EmbeddedLoftProfileMember {
                type_code,
                curve,
                endpoints: Some(endpoints),
                data,
            });
        }
        let saved = cur.pos();
        let (path_curve, path_endpoints) = if cur.take_ident() == Some("null_curve") {
            (None, None)
        } else {
            cur.set_pos(saved);
            let curve = embedded_base_curve_resolving_refs(cur, table)?;
            let endpoints = [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ];
            (Some(curve), Some(endpoints))
        };
        let auxiliary_count = usize::try_from(cur.take_long()?).ok()?;
        // Each auxiliary consumes at least its curve-block marker token.
        let auxiliary_count = bounded_len(auxiliary_count as u64, 1, cur.rest().len())?;
        let mut auxiliaries = Vec::with_capacity(auxiliary_count);
        for _ in 0..auxiliary_count {
            let (auxiliary, auxiliary_end) = curve_block(cur.toks(), cur.pos())?;
            cur.set_pos(auxiliary_end);
            auxiliaries.push(auxiliary);
        }
        let flag = cur.take_long()?;
        entries.push(EmbeddedLoftSectionEntry {
            parameter,
            profile,
            path: EmbeddedLoftPath {
                curve: path_curve,
                endpoints: path_endpoints,
                auxiliaries,
                flag,
            },
        });
    }
    Some(entries)
}

fn loft_subdata(cur: &mut Cur<'_>) -> Option<cadmpeg_ir::geometry::LoftSubdata> {
    loft_subdata_form(cur, false)
}

fn loft_subdata_form(
    cur: &mut Cur<'_>,
    revision: bool,
) -> Option<cadmpeg_ir::geometry::LoftSubdata> {
    use cadmpeg_ir::geometry::{LoftSubdata, LoftSubdataRow};
    let type_code = cur.take_long()?;
    let row_count = cur.take_long()?;
    let column_count = cur.take_long()?;
    let rows_to_read = if type_code == 211 {
        1
    } else {
        usize::try_from(row_count).ok()?
    };
    let columns_to_read = usize::try_from(column_count).ok()?;
    // Each row consumes two double tokens for its parameters.
    let rows_to_read = bounded_len(rows_to_read as u64, 2, cur.rest().len())?;
    let mut rows = Vec::with_capacity(rows_to_read);
    for _ in 0..rows_to_read {
        let parameters = [cur.take_f64()?, cur.take_f64()?];
        let mut columns = Vec::new();
        if type_code != 211 {
            columns.reserve(columns_to_read);
            for _ in 0..columns_to_read {
                columns.push([cur.take_f64()?, cur.take_f64()?]);
            }
        }
        let extra = if revision && type_code != 211 {
            Some([cur.take_f64()?, cur.take_f64()?])
        } else {
            None
        };
        rows.push(LoftSubdataRow {
            parameters,
            columns,
            extra,
        });
    }
    Some(LoftSubdata {
        type_code,
        row_count,
        column_count,
        rows,
    })
}

fn loft_profile_data(cur: &mut Cur<'_>) -> Option<EmbeddedLoftProfileData> {
    let surface = embedded_surface(cur)?;
    let saved = cur.pos();
    let pcurve = if cur.take_ident() == Some("nullbs") {
        None
    } else {
        cur.set_pos(saved);
        let (pcurve, end) = pcurve_block_with_end(cur.toks(), cur.pos())?;
        cur.set_pos(end);
        Some(pcurve)
    };
    let first_flag = cur.take_bool()?;
    let asm_extension = cur.take_long()?;
    let subdata = loft_subdata(cur)?;
    let direction = if cur.take_bool()? {
        let value = cur.take_vector3()?;
        Some(Vector3::new(value[0], value[1], value[2]))
    } else {
        None
    };
    Some(EmbeddedLoftProfileData {
        surface: Some(surface),
        support_bounds: [None; 4],
        pcurve,
        secondary_pcurve: None,
        first_flag: Some(first_flag),
        asm_extension: Some(asm_extension),
        subdata,
        direction,
    })
}

fn loft_section(cur: &mut Cur<'_>) -> Option<Vec<EmbeddedLoftSectionEntry>> {
    let count = usize::try_from(cur.take_long()?).ok()?;
    // Each entry consumes at least one double token for its parameter.
    let count = bounded_len(count as u64, 1, cur.rest().len())?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let parameter = cur.take_f64()?;
        let member_count = usize::try_from(cur.take_long()?).ok()?;
        // Each member consumes at least its type-code token.
        let member_count = bounded_len(member_count as u64, 1, cur.rest().len())?;
        let mut profile = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            let type_code = cur.take_long()?;
            let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
            cur.set_pos(curve_end);
            let data = loft_profile_data(cur)?;
            profile.push(EmbeddedLoftProfileMember {
                type_code,
                curve,
                endpoints: None,
                data,
            });
        }
        let (curve, curve_end) = curve_block(cur.toks(), cur.pos())?;
        cur.set_pos(curve_end);
        let auxiliary_count = usize::try_from(cur.take_long()?).ok()?;
        // Each auxiliary consumes at least its curve-block marker token.
        let auxiliary_count = bounded_len(auxiliary_count as u64, 1, cur.rest().len())?;
        let mut auxiliaries = Vec::with_capacity(auxiliary_count);
        for _ in 0..auxiliary_count {
            let (auxiliary, auxiliary_end) = curve_block(cur.toks(), cur.pos())?;
            cur.set_pos(auxiliary_end);
            auxiliaries.push(auxiliary);
        }
        let flag = cur.take_long()?;
        entries.push(EmbeddedLoftSectionEntry {
            parameter,
            profile,
            path: EmbeddedLoftPath {
                curve: Some(curve),
                endpoints: None,
                auxiliaries,
                flag,
            },
        });
    }
    Some(entries)
}

fn revision_loft(
    span: &[Token],
    position: usize,
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let table = resolver?;
    let mut cur = Cur::at(span, position);
    let revision = cur.take_long()?;
    (revision > 0).then_some(())?;
    let asm_extension_present = revision_loft_carries_asm_extension(table);
    let sections = [
        revision_loft_section(&mut cur, table, asm_extension_present)?,
        revision_loft_section(&mut cur, table, asm_extension_present)?,
    ];
    let wrap_ranges = [
        [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ],
        [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ],
    ];
    let mut flags = [false; 4];
    for flag in &mut flags {
        *flag = cur.take_bool()?;
    }
    let ints = [cur.take_long()?, cur.take_long()?];
    let RevisionSurfaceTail {
        enumeration: tail_enum,
        fit_tolerance,
        solved_cache_domains: _,
        parameterization,
        discontinuities,
        tail_flag,
    } = revision_surface_tail(&mut cur)?;
    cur.at_scope_end().then_some(())?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Loft(EmbeddedLoft {
            sections,
            revision_form: Some(cadmpeg_ir::geometry::LoftRevisionForm {
                revision,
                flags,
                ints,
                cache: revision_cache_form(tail_enum, fit_tolerance, parameterization)?,
                discontinuities,
                tail_flag,
            }),
            parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::RevisionRanges {
                intervals: wrap_ranges,
            },
            closures: [0, 0],
            singularities: [0, 0],
            mode: 0,
            bridge: Vec::new(),
        }),
        cache_fit_tolerance: fit_tolerance,
    })
}

fn loft_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::LoftBridgeToken;
    let names = ["loft_spl_sur", "loftsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    // The modern name uses the revision-gated layout.
    if matches!(cur.peek(), Some(Token::Long(_))) && name == "loft_spl_sur" {
        if let Some(decoded) = revision_loft(span, cur.pos(), resolver) {
            return Some(decoded);
        }
    }
    let sections = [loft_section(&mut cur)?, loft_section(&mut cur)?];
    let parameter_ranges = [
        [cur.take_f64()?, cur.take_f64()?],
        [cur.take_f64()?, cur.take_f64()?],
    ];
    let closures = [cur.take_enum()?, cur.take_enum()?];
    let singularities = [cur.take_enum()?, cur.take_enum()?];
    let mode = cur.take_long()?;
    let mut bridge = Vec::new();
    while toks::marker_at(span, cur.pos()).is_none() {
        match cur.peek()? {
            Token::True | Token::False => {
                bridge.push(LoftBridgeToken::Boolean(cur.take_bool()?));
            }
            Token::Long(_) => bridge.push(LoftBridgeToken::Integer(cur.take_long()?)),
            Token::Double(_) => bridge.push(LoftBridgeToken::Double(cur.take_f64()?)),
            Token::Enum(_) => bridge.push(LoftBridgeToken::Enum(cur.take_enum()?)),
            Token::Str(_) => bridge.push(LoftBridgeToken::Text(cur.take_str()?.to_string())),
            _ => return None,
        }
    }
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = optional_trailing_cache_tolerance(&mut cur)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Loft(EmbeddedLoft {
            sections,
            revision_form: None,
            parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::OrderedRanges {
                ranges: parameter_ranges,
            },
            closures,
            singularities,
            mode,
            bridge,
        }),
        cache_fit_tolerance,
    })
}

/// One revision-gated compound-loft scale block: counted profile members,
/// nullable path curve with optional endpoints, counted auxiliary BS3
/// curves, and one tail integer.
fn revision_cl_scale(
    cur: &mut Cur<'_>,
    table: &SubtypeTable,
    asm_extension_present: bool,
) -> Option<(Vec<EmbeddedLoftProfileMember>, EmbeddedLoftPath)> {
    let member_count = usize::try_from(cur.take_long()?).ok()?;
    // Each member consumes at least its type-code token.
    let member_count = bounded_len(member_count as u64, 1, cur.rest().len())?;
    let mut profile = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        let type_code = cur.take_long()?;
        let curve = embedded_base_curve_resolving_refs(cur, table)?;
        let endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let data = revision_loft_profile_data(
            cur,
            table,
            RevisionLoftMemberForm::of(type_code),
            asm_extension_present,
        )?;
        profile.push(EmbeddedLoftProfileMember {
            type_code,
            curve,
            endpoints: Some(endpoints),
            data,
        });
    }
    let saved = cur.pos();
    let (path_curve, path_endpoints) = if cur.take_ident() == Some("null_curve") {
        (None, None)
    } else {
        cur.set_pos(saved);
        let curve = embedded_base_curve_resolving_refs(cur, table)?;
        let endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        (Some(curve), Some(endpoints))
    };
    let auxiliary_count = usize::try_from(cur.take_long()?).ok()?;
    // Each auxiliary consumes at least its curve-block marker token.
    let auxiliary_count = bounded_len(auxiliary_count as u64, 1, cur.rest().len())?;
    let mut auxiliaries = Vec::with_capacity(auxiliary_count);
    for _ in 0..auxiliary_count {
        let (auxiliary, auxiliary_end) = curve_block(cur.toks(), cur.pos())?;
        cur.set_pos(auxiliary_end);
        auxiliaries.push(auxiliary);
    }
    let flag = cur.take_long()?;
    Some((
        profile,
        EmbeddedLoftPath {
            curve: path_curve,
            endpoints: path_endpoints,
            auxiliaries,
            flag,
        },
    ))
}

fn revision_compound_loft(
    span: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let table = resolver?;
    let mut cur = Cur::at(span, 2);
    let revision = cur.take_long()?;
    (revision > 0).then_some(())?;
    let RevisionSurfaceTail {
        enumeration: tail_enum,
        fit_tolerance,
        solved_cache_domains: _,
        parameterization,
        discontinuities,
        tail_flag,
    } = revision_surface_tail(&mut cur)?;
    let asm_extension_present = revision_loft_carries_asm_extension(table);
    let (base_profile, base_path) = revision_cl_scale(&mut cur, table, asm_extension_present)?;
    let entry_count = usize::try_from(cur.take_long()?).ok()?;
    // Each entry consumes at least its member-count token.
    let entry_count = bounded_len(entry_count as u64, 1, cur.rest().len())?;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let (profile, path) = revision_cl_scale(&mut cur, table, asm_extension_present)?;
        let parameter = cur.take_f64()?;
        entries.push(EmbeddedLoftSectionEntry {
            parameter,
            profile,
            path,
        });
    }
    let flags = [cur.take_bool()?, cur.take_bool()?];
    let kind = cur.take_long()?;
    // The revision layout defines the kind-zero payload.
    (kind == 0).then_some(())?;
    let kind_flags = [cur.take_bool()?, cur.take_bool()?];
    let selector = cur.take_long()?;
    let direction = if selector == 0 {
        let value = cur.take_vector3()?;
        EmbeddedCompoundLoftDirection::Vector(Vector3::new(value[0], value[1], value[2]))
    } else {
        let (curve, curve_end) = curve_block(span, cur.pos())?;
        cur.set_pos(curve_end);
        EmbeddedCompoundLoftDirection::Curve {
            selector: NonZeroI64::new(selector)?,
            curve,
        }
    };
    let interval = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    // Both parameter values select a trailing curve. The stream has no separate
    // marker; the parameter pair selects it.
    let trailing_curve = if interval.iter().all(Option::is_some) {
        let (curve, curve_end) = curve_block(span, cur.pos())?;
        cur.set_pos(curve_end);
        Some(curve)
    } else {
        None
    };
    cur.at_scope_end().then_some(())?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::RevisionCompoundLoft(Box::new(
            EmbeddedRevisionCompoundLoft {
                revision,
                cache: revision_cache_form(tail_enum, fit_tolerance, parameterization)?,
                discontinuities,
                tail_flag,
                base_profile,
                base_path,
                entries,
                flags,
                kind,
                kind_flags,
                direction,
                interval,
                trailing_curve,
            },
        )),
        cache_fit_tolerance: fit_tolerance,
    })
}

fn compound_loft_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let (start, _) = toks::find_owned_subtype_marker(toks, &["cl_loft_spl_sur"])?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        return revision_compound_loft(span, resolver);
    }
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
    let scales = Box::new([
        compound_loft_scale(&mut cur)?,
        compound_loft_scale(&mut cur)?,
        compound_loft_scale(&mut cur)?,
        compound_loft_scale(&mut cur)?,
    ]);
    let fifth_scale = if matches!(cur.peek(), Some(Token::Long(_))) {
        compound_loft_scale(&mut cur)?.map(Box::new)
    } else {
        None
    };
    let flags = [cur.take_bool()?, cur.take_bool()?];
    let kind = cur.take_long()?;
    let tail = match kind {
        6 => {
            let tail_flags = [cur.take_bool()?, cur.take_bool()?];
            let scale = Box::new(compound_loft_scale(&mut cur)??);
            let selector = cur.take_long()?;
            let direction = cur.take_vector3()?;
            let parameter_range = [cur.take_range_value()?, cur.take_range_value()?];
            let (curve, _) = curve_block(span, cur.pos())?;
            EmbeddedCompoundLoftTail::Six {
                flags: tail_flags,
                scale,
                selector,
                direction: Vector3::new(direction[0], direction[1], direction[2]),
                parameter_range,
                curve,
            }
        }
        7 => {
            let first_flag = cur.take_bool()?;
            let first_scale = compound_loft_scale(&mut cur)?.map(Box::new);
            let second_flag = cur.take_bool()?;
            let second_scale = Box::new(compound_loft_scale(&mut cur)??);
            let selector = cur.take_long()?;
            let direction = cur.take_vector3()?;
            let trailing_flags = [cur.take_bool()?, cur.take_bool()?];
            EmbeddedCompoundLoftTail::Seven {
                first_flag,
                first_scale,
                second_flag,
                second_scale,
                selector,
                direction: Vector3::new(direction[0], direction[1], direction[2]),
                trailing_flags,
            }
        }
        0 => {
            let tail_flags = [cur.take_bool()?, cur.take_bool()?];
            let selector = cur.take_long()?;
            let direction = if selector == 0 {
                let value = cur.take_vector3()?;
                EmbeddedCompoundLoftDirection::Vector(Vector3::new(value[0], value[1], value[2]))
            } else {
                let (curve, curve_end) = curve_block(span, cur.pos())?;
                cur.set_pos(curve_end);
                EmbeddedCompoundLoftDirection::Curve {
                    selector: NonZeroI64::new(selector)?,
                    curve,
                }
            };
            let trailing_flags = [cur.take_bool()?, cur.take_bool()?];
            EmbeddedCompoundLoftTail::Zero {
                flags: tail_flags,
                selector,
                direction,
                trailing_flags,
            }
        }
        _ => return None,
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::CompoundLoft(Box::new(
            EmbeddedCompoundLoft {
                scales,
                fifth_scale,
                flags,
                tail,
            },
        )),
        cache_fit_tolerance,
    })
}

fn scaled_compound_loft_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["scaled_cloft_spl_sur", "sclclftsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let singularity = cur.take_enum()?;
    let (shape, cache_fit_tolerance) = if cur.peek().is_some_and(Token::is_payload_ident) {
        let (_, cache_end) = surface_block(span, cur.pos())?;
        cur.set_pos(cache_end);
        let tolerance = cur.take_f64()? * LEN_TO_MM;
        (EmbeddedScaledCompoundLoftShape::Full, Some(tolerance))
    } else {
        let parameter_ranges = [
            [cur.take_range_value()?, cur.take_range_value()?],
            [cur.take_range_value()?, cur.take_range_value()?],
        ];
        let parameters = [cur.take_float_array()?, cur.take_float_array()?];
        (
            EmbeddedScaledCompoundLoftShape::None {
                parameter_ranges,
                parameters,
            },
            None,
        )
    };
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
    let scales = Box::new([
        compound_loft_scale(&mut cur)?,
        compound_loft_scale(&mut cur)?,
        compound_loft_scale(&mut cur)?,
    ]);
    let flags = [cur.take_bool()?, cur.take_bool()?];
    let selector = cur.take_long()?;
    let extended = cur.take_bool()?;
    let branch = if extended {
        let first_scale = compound_loft_scale(&mut cur)?.map(Box::new);
        if cur.take_bool()? {
            let second_scale = Box::new(compound_loft_scale(&mut cur)??);
            let selector = cur.take_long()?;
            let direction = cur.take_vector3()?;
            EmbeddedScaledCompoundLoftBranch::ExtendedVector {
                first_scale,
                second_scale,
                selector,
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            }
        } else {
            let flag = cur.take_bool()?;
            let singularity = cur.take_enum()?;
            let (curve, curve_end) = curve_block(span, cur.pos())?;
            cur.set_pos(curve_end);
            EmbeddedScaledCompoundLoftBranch::ExtendedCurve {
                scale: first_scale,
                flag,
                singularity,
                curve,
            }
        }
    } else {
        let flag = cur.take_bool()?;
        let selector = cur.take_long()?;
        let direction = if selector == 0 {
            let direction = cur.take_vector3()?;
            EmbeddedCompoundLoftDirection::Vector(Vector3::new(
                direction[0],
                direction[1],
                direction[2],
            ))
        } else {
            let (curve, curve_end) = curve_block(span, cur.pos())?;
            cur.set_pos(curve_end);
            EmbeddedCompoundLoftDirection::Curve {
                selector: NonZeroI64::new(selector)?,
                curve,
            }
        };
        EmbeddedScaledCompoundLoftBranch::Direct {
            flag,
            selector,
            direction,
        }
    };
    let trailing_flags = [cur.take_bool()?, cur.take_bool()?];
    let tail_kind = cur.take_long()?;
    let first = cur.take_vector3()?;
    let second = cur.take_vector3()?;
    let tail_singularity = cur.take_enum()?;
    let (tail_curve, _) = curve_block(span, cur.pos())?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::ScaledCompoundLoft(Box::new(
            EmbeddedScaledCompoundLoft {
                singularity,
                shape,
                discontinuities,
                discontinuity_flag,
                scales,
                flags,
                selector,
                branch,
                trailing_flags,
                tail_kind,
                tail_directions: [
                    Vector3::new(first[0], first[1], first[2]),
                    Vector3::new(second[0], second[1], second[2]),
                ],
                tail_singularity,
                tail_curve,
            },
        )),
        cache_fit_tolerance,
    })
}

/// Decode one recursive law expression.
pub(crate) fn law_expression(cur: &mut Cur<'_>, depth: usize) -> Option<EmbeddedLawExpression> {
    law_expression_resolving(cur, depth, None)
}

/// Decode a law slot of the sweep layout. Older sweep records use the
/// recursive law grammar, while revision-gated records may store the whole
/// expression as one serializer string. The text form is scoped to sweep
/// law slots so an unknown operator in another law grammar remains a refusal.
fn sweep_law_expression(cur: &mut Cur<'_>) -> Option<EmbeddedLawExpression> {
    if matches!(cur.peek(), Some(Token::Str(_))) {
        return Some(EmbeddedLawExpression::Text(cur.take_str()?.to_string()));
    }
    law_expression(cur, 0)
}

fn law_expression_resolving(
    cur: &mut Cur<'_>,
    depth: usize,
    resolver: Option<&SubtypeTable>,
) -> Option<EmbeddedLawExpression> {
    if depth > 64 {
        return None;
    }
    match cur.peek()? {
        Token::Long(_) => {
            return Some(EmbeddedLawExpression::Integer(cur.take_long()?));
        }
        Token::Double(_) => return Some(EmbeddedLawExpression::Double(cur.take_f64()?)),
        Token::Position(_) => {
            let value = cur.take_position()?;
            return Some(EmbeddedLawExpression::Point(Point3::new(
                value[0] * LEN_TO_MM,
                value[1] * LEN_TO_MM,
                value[2] * LEN_TO_MM,
            )));
        }
        Token::Vector3(_) => {
            let value = cur.take_vector3()?;
            return Some(EmbeddedLawExpression::Vector(Vector3::new(
                value[0], value[1], value[2],
            )));
        }
        _ => {}
    }
    let operator = cur.take_str()?.to_string();
    match operator.as_str() {
        "null_law" => Some(EmbeddedLawExpression::Null),
        "TRANS" => {
            if matches!(cur.peek(), Some(Token::Vector3(_))) {
                let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
                for vector in &mut vectors {
                    let value = cur.take_vector3()?;
                    *vector = Vector3::new(value[0], value[1], value[2]);
                }
                let scale = cur.take_f64()?;
                let flags = [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?];
                return Some(EmbeddedLawExpression::TransformVec {
                    vectors,
                    scale,
                    flags,
                });
            }
            let mut scalars = [0.0; 13];
            for scalar in &mut scalars {
                *scalar = cur.take_f64()?;
            }
            let enums = [cur.take_enum()?, cur.take_enum()?, cur.take_enum()?];
            Some(EmbeddedLawExpression::Transform { scalars, enums })
        }
        "EDGE" => {
            let (curve, endpoints) = if let Some((curve, end)) = curve_block(cur.toks(), cur.pos())
            {
                cur.set_pos(end);
                let endpoints = matches!(cur.peek(), Some(Token::True | Token::False))
                    .then(|| {
                        Some([
                            cur.take_optional_range_value()?,
                            cur.take_optional_range_value()?,
                        ])
                    })
                    .flatten();
                (curve, endpoints)
            } else {
                let table = resolver?;
                let curve = embedded_base_curve_resolving_refs(cur, table)?;
                let endpoints = Some([
                    cur.take_optional_range_value()?,
                    cur.take_optional_range_value()?,
                ]);
                (curve, endpoints)
            };
            let parameters = [cur.take_f64()?, cur.take_f64()?];
            Some(EmbeddedLawExpression::Edge {
                curve,
                endpoints,
                parameters,
            })
        }
        "SPLINE_LAW" => {
            let native_id = cur.take_long()?;
            let knots = cur.take_float_array()?;
            let controls = cur.take_float_array()?;
            let point = cur.take_position()?;
            Some(EmbeddedLawExpression::Spline {
                native_id,
                knots,
                controls,
                point: Point3::new(
                    point[0] * LEN_TO_MM,
                    point[1] * LEN_TO_MM,
                    point[2] * LEN_TO_MM,
                ),
            })
        }
        _ => {
            let arity = match operator.as_str() {
                "COS" | "SIN" | "TAN" | "COT" | "SEC" | "CSC" | "COSH" | "SINH" | "TANH"
                | "COTH" | "SECH" | "CSCH" | "ARCCOS" | "ARCSIN" | "ARCTAN" | "ARCOT"
                | "ARCSEC" | "ARCCSC" | "ARCCOSH" | "ARCSINH" | "ARCTANH" | "ARCOTH"
                | "ARCSECH" | "ARCCSCH" | "ABS" | "EXP" | "LN" | "LOG" | "SIGN" | "SIZE"
                | "SET" | "SQRT" | "NORM" | "NOT" => 1,
                "CROSS" | "DOT" | "DCUR" | "O" | "ROTATE" | "TERM" => 2,
                "VEC" | "DSURF" => 3,
                _ => return None,
            };
            let operands = (0..arity)
                .map(|_| law_expression_resolving(cur, depth + 1, resolver))
                .collect::<Option<Vec<_>>>()?;
            Some(EmbeddedLawExpression::Algebraic { operator, operands })
        }
    }
}

/// Decode one named law formula and its counted variables.
pub(crate) fn law_formula(cur: &mut Cur<'_>) -> Option<EmbeddedLawFormula> {
    law_formula_resolving(cur, None)
}

fn law_formula_resolving(
    cur: &mut Cur<'_>,
    resolver: Option<&SubtypeTable>,
) -> Option<EmbeddedLawFormula> {
    let name = cur.take_str()?.to_string();
    if name == "null_law" {
        return Some(EmbeddedLawFormula {
            name,
            variables: Vec::new(),
        });
    }
    let count = usize::try_from(cur.take_long()?).ok()?;
    if count > 100_000 {
        return None;
    }
    let variables = (0..count)
        .map(|_| law_expression_resolving(cur, 0, resolver))
        .collect::<Option<Vec<_>>>()?;
    Some(EmbeddedLawFormula { name, variables })
}

fn skin_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["skin_spl_sur", "skinsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let surface_boolean = cur.take_enum()?;
    let surface_normal = cur.take_enum()?;
    let surface_direction = cur.take_enum()?;
    let count = cur.take_long()?;
    let parameter = cur.take_f64()?;
    let inner_count = cur.take_long()?;
    let layout = if cur.peek().is_some_and(Token::is_payload_ident) {
        let (curve, curve_end) = curve_block(span, cur.pos())?;
        cur.set_pos(curve_end);
        let subdata = loft_subdata(&mut cur)?;
        let first_tail = cur.take_long()?;
        let (secondary_curve, secondary_end) = curve_block(span, cur.pos())?;
        cur.set_pos(secondary_end);
        let second_tail = cur.take_long()?;
        EmbeddedSkinSurfaceLayout::Compact {
            curve,
            subdata,
            first_tail,
            secondary_curve,
            second_tail,
        }
    } else {
        let profile_count = usize::try_from(inner_count).ok()?;
        if profile_count > 100_000 {
            return None;
        }
        let mut profiles = Vec::with_capacity(profile_count);
        for _ in 0..profile_count {
            let type_code = cur.take_long()?;
            let (curve, curve_end) = curve_block(span, cur.pos())?;
            cur.set_pos(curve_end);
            let data = loft_profile_data(&mut cur)?;
            profiles.push(EmbeddedLoftProfileMember {
                type_code,
                curve,
                endpoints: None,
                data,
            });
        }
        let (path, path_end) = curve_block(span, cur.pos())?;
        cur.set_pos(path_end);
        let tail = [cur.take_long()?, cur.take_long()?];
        EmbeddedSkinSurfaceLayout::Profiles {
            profiles,
            path,
            tail,
        }
    };
    let direction = cur.take_vector3()?;
    let trailing_parameter = cur.take_f64()?;
    let formula = law_formula(&mut cur)?;
    let (parameter_curve, parameter_curve_end) = curve_block(span, cur.pos())?;
    cur.set_pos(parameter_curve_end);
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Skin(Box::new(EmbeddedSkinSurface {
            surface_boolean,
            surface_normal,
            surface_direction,
            count,
            parameter,
            inner_count,
            layout,
            direction: Vector3::new(direction[0], direction[1], direction[2]),
            trailing_parameter,
            formula,
            parameter_curve,
            discontinuities,
            discontinuity_flag,
        })),
        cache_fit_tolerance,
    })
}

pub(crate) fn law_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["law_spl_sur", "lawsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let parameter_ranges = if matches!(cur.peek(), Some(Token::Double(_))) {
        Some([
            [cur.take_f64()?, cur.take_f64()?],
            [cur.take_f64()?, cur.take_f64()?],
        ])
    } else {
        None
    };
    let primary = law_formula(&mut cur)?;
    let count = usize::try_from(cur.take_long()?).ok()?;
    if count > 100_000 {
        return None;
    }
    let additional = (0..count)
        .map(|_| law_formula(&mut cur))
        .collect::<Option<Vec<_>>>()?;
    let selector = if parameter_ranges.is_some()
        && toks::marker_at(span, cur.pos()) == Some(toks::BsplineMarker::Nubs)
    {
        0
    } else {
        cur.take_enum()?
    };
    let (tail, cache_fit_tolerance) = match selector {
        0 => {
            let (_, cache_end) = surface_block(span, cur.pos())?;
            cur.set_pos(cache_end);
            (
                cadmpeg_ir::geometry::LawSurfaceTail::Full,
                Some(cur.take_f64()? * LEN_TO_MM),
            )
        }
        1 => {
            let parameters = [cur.take_float_array()?, cur.take_float_array()?];
            let fit_tolerance = cur.take_f64()? * LEN_TO_MM;
            let closures = [cur.take_enum()?, cur.take_enum()?];
            let singularities = [cur.take_enum()?, cur.take_enum()?];
            (
                cadmpeg_ir::geometry::LawSurfaceTail::Summary {
                    parameters,
                    fit_tolerance,
                    closures,
                    singularities,
                },
                None,
            )
        }
        2 => {
            let parameter_ranges = [
                [cur.take_f64()?, cur.take_f64()?],
                [cur.take_f64()?, cur.take_f64()?],
            ];
            let closures = [cur.take_enum()?, cur.take_enum()?];
            let singularities = [cur.take_enum()?, cur.take_enum()?];
            (
                cadmpeg_ir::geometry::LawSurfaceTail::None {
                    parameter_ranges,
                    closures,
                    singularities,
                },
                None,
            )
        }
        3 => (cadmpeg_ir::geometry::LawSurfaceTail::Historical, None),
        4 => (cadmpeg_ir::geometry::LawSurfaceTail::Optimal, None),
        _ => return None,
    };
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Law(Box::new(EmbeddedLawSurface {
            parameter_ranges,
            primary,
            additional,
            tail,
            discontinuities,
        })),
        cache_fit_tolerance,
    })
}

pub(crate) fn sub_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["sub_spl_sur", "subsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let parameter_ranges = [
        [cur.take_f64()?, cur.take_f64()?],
        [cur.take_f64()?, cur.take_f64()?],
    ];
    let support = embedded_surface(&mut cur)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::SubSurface {
            support,
            parameter_ranges,
        },
        cache_fit_tolerance: None,
    })
}

fn net_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["net_spl_sur", "netsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let sections = Box::new([loft_section(&mut cur)?, loft_section(&mut cur)?]);
    let mut frame_parameters = [0.0; 12];
    for parameter in &mut frame_parameters {
        *parameter = cur.take_f64()?;
    }
    let flag = cur.take_long()?;
    let mut directions = [Vector3::new(0.0, 0.0, 0.0); 4];
    for direction in &mut directions {
        let value = cur.take_vector3()?;
        *direction = Vector3::new(value[0], value[1], value[2]);
    }
    let formulas = (0..4)
        .map(|_| law_formula(&mut cur))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Net(Box::new(EmbeddedNetSurface {
            sections,
            frame_parameters,
            flag,
            directions,
            formulas: Box::new(formulas),
            discontinuities,
            discontinuity_flag,
        })),
        cache_fit_tolerance,
    })
}

fn sweep_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let names = ["sweep_spl_sur", "sweep_sur", "sweepsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // The revision-gated layout belongs to `sweep_sur`.
        (name == "sweep_sur").then_some(())?;
        return revision_sweep_sur(span, cur.pos(), resolver?);
    }
    let primary_kind = cur.take_enum()?;
    let layout = if cur.peek().is_some_and(Token::is_payload_ident) {
        let (profile, profile_end) = curve_block(span, cur.pos())?;
        cur.set_pos(profile_end);
        let (spine, spine_end) = curve_block(span, cur.pos())?;
        cur.set_pos(spine_end);
        let secondary_kind = cur.take_enum()?;
        let mut directions = [Vector3::new(0.0, 0.0, 0.0); 5];
        for direction in &mut directions {
            let value = cur.take_vector3()?;
            *direction = Vector3::new(value[0], value[1], value[2]);
        }
        let origin = cur.take_position()?;
        let mut parameters = [0.0; 4];
        for parameter in &mut parameters {
            *parameter = cur.take_f64()?;
        }
        let formulas = (0..3)
            .map(|_| law_formula(&mut cur))
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()?;
        EmbeddedSweepSurfaceLayout::ProfileFirst {
            profile,
            spine,
            secondary_kind,
            directions,
            origin: Point3::new(
                origin[0] * LEN_TO_MM,
                origin[1] * LEN_TO_MM,
                origin[2] * LEN_TO_MM,
            ),
            parameters,
            formulas: Box::new(formulas),
        }
    } else {
        let mode = cur.take_long()?;
        let (profile, profile_end) = curve_block(span, cur.pos())?;
        cur.set_pos(profile_end);
        let profile_range = [cur.take_f64()?, cur.take_f64()?];
        let profile_frame = if cur.take_bool()? {
            let point = cur.take_position()?;
            let vector = cur.take_vector3()?;
            Some((
                Point3::new(
                    point[0] * LEN_TO_MM,
                    point[1] * LEN_TO_MM,
                    point[2] * LEN_TO_MM,
                ),
                Vector3::new(vector[0], vector[1], vector[2]),
            ))
        } else {
            None
        };
        let point = cur.take_position()?;
        let origin = Point3::new(
            point[0] * LEN_TO_MM,
            point[1] * LEN_TO_MM,
            point[2] * LEN_TO_MM,
        );
        let mut directions = [Vector3::new(0.0, 0.0, 0.0); 3];
        for direction in &mut directions {
            let value = cur.take_vector3()?;
            *direction = Vector3::new(value[0], value[1], value[2]);
        }
        if matches!(cur.peek(), Some(Token::Long(_))) {
            let branch = cur.take_long()?;
            let trajectory_flag = cur.take_bool()?;
            let (path, path_end) = curve_block(span, cur.pos())?;
            cur.set_pos(path_end);
            let path_range = [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM];
            let path_parameter = cur.take_f64()?;
            match branch {
                1 => {
                    let formula_flag = cur.take_bool()?;
                    let formula = law_formula(&mut cur)?;
                    let trailing_flag = cur.take_bool()?;
                    EmbeddedSweepSurfaceLayout::ExplicitFormula {
                        profile,
                        mode,
                        profile_range,
                        profile_frame,
                        origin,
                        directions,
                        trajectory_flag,
                        path,
                        path_range,
                        path_parameter,
                        formula_flag,
                        formula,
                        trailing_flag,
                    }
                }
                2 => {
                    let guide_flags = [cur.take_bool()?, cur.take_bool()?];
                    let (guide_curve, guide_end) = curve_block(span, cur.pos())?;
                    cur.set_pos(guide_end);
                    let guide_range = [cur.take_f64()?, cur.take_f64()?];
                    let guide_modes = [cur.take_long()?, cur.take_long()?];
                    let mut guide_parameters = [0.0; 6];
                    for parameter in &mut guide_parameters {
                        *parameter = cur.take_f64()?;
                    }
                    let trailing_flags = [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?];
                    EmbeddedSweepSurfaceLayout::ExplicitGuide {
                        profile,
                        mode,
                        profile_range,
                        profile_frame,
                        origin,
                        directions,
                        trajectory_flag,
                        path,
                        path_range,
                        path_parameter,
                        guide_flags,
                        guide_curve,
                        guide_range,
                        guide_modes,
                        guide_parameters,
                        trailing_flags,
                    }
                }
                3 => {
                    let singularity = cur.take_enum()?;
                    let support_surface = embedded_surface(&mut cur)?;
                    let auxiliary_curve = if cur.take_bool()? {
                        let (curve, curve_end) = curve_block(span, cur.pos())?;
                        cur.set_pos(curve_end);
                        Some(curve)
                    } else {
                        None
                    };
                    let support_flag = cur.take_bool()?;
                    let legacy_flag = matches!(cur.peek(), Some(Token::True | Token::False))
                        .then(|| cur.take_bool())
                        .flatten();
                    EmbeddedSweepSurfaceLayout::ExplicitSurface {
                        profile,
                        mode,
                        profile_range,
                        profile_frame,
                        origin,
                        directions,
                        trajectory_flag,
                        path,
                        path_range,
                        path_parameter,
                        singularity,
                        support_surface,
                        auxiliary_curve,
                        support_flag,
                        legacy_flag,
                    }
                }
                _ => return None,
            }
        } else {
            let first_law = sweep_law_expression(&mut cur)?;
            let first_mode = cur.take_long()?;
            let first_range = [cur.take_f64()?, cur.take_f64()?];
            let vector = cur.take_vector3()?;
            let law_direction = Vector3::new(vector[0], vector[1], vector[2]);
            let path_mode = cur.take_long()?;
            let path_flag = cur.take_bool()?;
            let (path, path_end) = curve_block(span, cur.pos())?;
            cur.set_pos(path_end);
            let path_range = [cur.take_f64()?, cur.take_f64()?];
            let path_parameter = cur.take_f64()?;
            let second_law_flag = cur.take_bool()?;
            let second_law = sweep_law_expression(&mut cur)?;
            let formula_mode = cur.take_long()?;
            let formula = law_formula(&mut cur)?;
            let trailing_flag = cur.take_bool()?;
            EmbeddedSweepSurfaceLayout::LawDriven {
                profile,
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                first_law,
                first_mode,
                first_range,
                law_direction,
                path_mode,
                path_flag,
                path,
                path_range,
                path_parameter,
                second_law_flag,
                second_law,
                formula_mode,
                formula,
                trailing_flag,
            }
        }
    };
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Sweep(Box::new(EmbeddedSweepSurface {
            primary_kind,
            revision_form: None,
            layout,
            discontinuities,
            discontinuity_flag,
        })),
        cache_fit_tolerance,
    })
}

/// Revision-gated `sweep_sur` layouts.
fn revision_sweep_sur(
    span: &[Token],
    position: usize,
    table: &SubtypeTable,
) -> Option<DecodedProceduralSurface> {
    let mut cur = Cur::at(span, position);
    let revision = cur.take_long()?;
    (revision > 0).then_some(())?;
    let primary_flag = cur.take_bool()?;
    let mode = cur.take_long()?;
    let profile = embedded_base_curve_resolving_refs(&mut cur, table)?;
    let profile_endpoints = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    let profile_range = [
        cur.take_optional_range_value()??,
        cur.take_optional_range_value()??,
    ];
    let profile_frame = if cur.take_bool()? {
        let point = cur.take_position()?;
        let vector = cur.take_vector3()?;
        Some((
            Point3::new(
                point[0] * LEN_TO_MM,
                point[1] * LEN_TO_MM,
                point[2] * LEN_TO_MM,
            ),
            Vector3::new(vector[0], vector[1], vector[2]),
        ))
    } else {
        None
    };
    let point = cur.take_position()?;
    let origin = Point3::new(
        point[0] * LEN_TO_MM,
        point[1] * LEN_TO_MM,
        point[2] * LEN_TO_MM,
    );
    let mut directions = [Vector3::new(0.0, 0.0, 0.0); 3];
    for direction in &mut directions {
        let value = cur.take_vector3()?;
        *direction = Vector3::new(value[0], value[1], value[2]);
    }
    let (layout, path_endpoints) = if matches!(cur.peek(), Some(Token::Str(_))) {
        let first_law = sweep_law_expression(&mut cur)?;
        let first_mode = cur.take_long()?;
        let first_range = [
            cur.take_optional_range_value()??,
            cur.take_optional_range_value()??,
        ];
        let law_direction = cur.take_vector3()?;
        let path_mode = cur.take_long()?;
        let path_flag = cur.take_bool()?;
        let path = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let path_endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let path_range = [
            cur.take_optional_range_value()?? * LEN_TO_MM,
            cur.take_optional_range_value()?? * LEN_TO_MM,
        ];
        let path_parameter = cur.take_f64()?;
        let second_law_flag = cur.take_bool()?;
        let second_law = sweep_law_expression(&mut cur)?;
        let formula_mode = cur.take_long()?;
        let formula = law_formula_resolving(&mut cur, Some(table))?;
        let trailing_flag = cur.take_bool()?;
        let law_direction = Vector3::new(law_direction[0], law_direction[1], law_direction[2]);
        (
            EmbeddedSweepSurfaceLayout::LawDriven {
                profile,
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                first_law,
                first_mode,
                first_range,
                law_direction,
                path_mode,
                path_flag,
                path,
                path_range,
                path_parameter,
                second_law_flag,
                second_law,
                formula_mode,
                formula,
                trailing_flag,
            },
            path_endpoints,
        )
    } else {
        (cur.take_long()? == 1).then_some(())?;
        let trajectory_flag = cur.take_bool()?;
        let path = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let path_endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let path_range = [
            cur.take_optional_range_value()?? * LEN_TO_MM,
            cur.take_optional_range_value()?? * LEN_TO_MM,
        ];
        let path_parameter = cur.take_f64()?;
        let formula_flag = cur.take_bool()?;
        let formula = law_formula_resolving(&mut cur, Some(table))?;
        let trailing_flag = cur.take_bool()?;
        (
            EmbeddedSweepSurfaceLayout::ExplicitFormula {
                profile,
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                trajectory_flag,
                path,
                path_range,
                path_parameter,
                formula_flag,
                formula,
                trailing_flag,
            },
            path_endpoints,
        )
    };
    let RevisionSurfaceTail {
        enumeration: tail_enum,
        fit_tolerance: cache_fit_tolerance,
        solved_cache_domains: _,
        parameterization: tail_parameterization,
        discontinuities,
        tail_flag: discontinuity_flag,
    } = revision_surface_tail(&mut cur)?;
    cur.at_scope_end().then_some(())?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Sweep(Box::new(EmbeddedSweepSurface {
            primary_kind: 0,
            revision_form: Some(cadmpeg_ir::geometry::SweepRevisionForm {
                revision,
                primary_flag,
                profile_endpoints,
                path_endpoints,
                cache: revision_cache_form(tail_enum, cache_fit_tolerance, tail_parameterization)?,
            }),
            layout,
            discontinuities,
            discontinuity_flag,
        })),
        cache_fit_tolerance,
    })
}

fn taper_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::TaperSurfaceKind;
    let names: &[(&str, u8)] = &[
        ("taper_spl_sur", 0),
        ("ortho_spl_sur", 1),
        ("orthosur", 1),
        ("edge_tpr_spl_sur", 2),
        ("shadow_tpr_spl_sur", 3),
        ("shadowtapersur", 3),
        ("ruled_tpr_spl_sur", 4),
        ("ruledtapersur", 4),
        ("swept_tpr_spl_sur", 5),
        ("swepttapersur", 5),
    ];
    let candidates: Vec<&str> = names.iter().map(|(name, _)| *name).collect();
    let (start, name) = toks::find_owned_subtype_marker(toks, &candidates)?;
    let kind = names
        .iter()
        .find_map(|(candidate, kind)| (*candidate == name).then_some(*kind))?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Revision-gated form, stored by the orthogonal subtype's modern name.
        (name == "ortho_spl_sur").then_some(())?;
        let table = resolver?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let (support, support_bounds) = optional_embedded_surface_with_bounds(&mut cur, table)?;
        let support = support?;
        let reference = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let reference_endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let pcurve = nullable_embedded_pcurve(&mut cur)?;
        let parameter = cur.take_f64()?;
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            solved_cache_domains: _,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        // The single trailing logical after the shared tail is the record's own
        // orthogonal-sense field, positionally matching the text form's single
        // boolean. `tail_flag` above is the shared-tail illegal-region flag.
        let sense = cur.take_bool()?;
        cur.at_scope_end().then_some(())?;
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Taper {
                support,
                reference,
                pcurve,
                parameter,
                taper: TaperSurfaceKind::Orthogonal { sense },
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds,
                    reference_endpoints,
                    second_endpoints: [None; 2],
                    flags: Vec::new(),
                    cache: revision_cache_form(tail_enum, fit_tolerance, parameterization)?,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: fit_tolerance,
        });
    }
    let support = embedded_surface(&mut cur)?;
    let (reference, reference_end) = curve_block(span, cur.pos())?;
    cur.set_pos(reference_end);
    let saved = cur.pos();
    let pcurve = if cur.take_ident() == Some("nullbs") {
        None
    } else {
        cur.set_pos(saved);
        let (pcurve, end) = pcurve_block_with_end(span, cur.pos())?;
        cur.set_pos(end);
        Some(pcurve)
    };
    let parameter = cur.take_f64()?;
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = if matches!(cur.peek(), Some(Token::Double(_))) {
        Some(cur.take_f64()? * LEN_TO_MM)
    } else {
        None
    };
    let take_draft = |cur: &mut Cur<'_>| {
        let draft = cur.take_vector3()?;
        Some(Vector3::new(draft[0], draft[1], draft[2]))
    };
    let taper = match kind {
        0 => TaperSurfaceKind::Standard,
        1 => TaperSurfaceKind::Orthogonal {
            sense: cur.take_bool()?,
        },
        2 => TaperSurfaceKind::Edge {
            draft: take_draft(&mut cur)?,
        },
        3 => TaperSurfaceKind::Shadow {
            draft: take_draft(&mut cur)?,
            sine: cur.take_f64()?,
            cosine: cur.take_f64()?,
        },
        4 => TaperSurfaceKind::Ruled {
            draft: take_draft(&mut cur)?,
            sine: cur.take_f64()?,
            cosine: cur.take_f64()?,
            factor: cur.take_f64()?,
        },
        5 => TaperSurfaceKind::Swept {
            draft: take_draft(&mut cur)?,
            sine: cur.take_f64()?,
            cosine: cur.take_f64()?,
        },
        _ => return None,
    };
    cur.at_scope_end().then_some(())?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Taper {
            support,
            reference,
            pcurve,
            parameter,
            taper,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn comp_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let (start, _) = toks::find_owned_subtype_marker(toks, &["comp_spl_sur"])?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = if matches!(cur.peek(), Some(Token::Double(_))) {
        Some(cur.take_f64()? * LEN_TO_MM)
    } else {
        None
    };
    let parameters = cur.take_float_array()?;
    let mut components = Vec::with_capacity(parameters.len());
    for _ in 0..parameters.len() {
        components.push(embedded_surface(&mut cur)?);
    }
    cur.at_scope_end().then_some(())?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Compound {
            parameters,
            components,
        },
        cache_fit_tolerance,
    })
}

/// The shared revision-gated surface tail, decoded.
pub struct RevisionSurfaceTail {
    /// Enum opening the tail, selecting the approximation-cache form.
    pub enumeration: i64,
    /// Fit tolerance of the solved cache. Carried by form `0` only.
    pub fit_tolerance: Option<f64>,
    /// U and V knot domains of the solved cache. Carried by form `0` only.
    pub solved_cache_domains: Option<[[f64; 2]; 2]>,
    /// Parameter intervals and closure/singularity enums. Carried by form `2`
    /// only.
    pub parameterization: Option<cadmpeg_ir::geometry::RevisionSurfaceParameterization>,
    /// Six ordered discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the tail.
    pub tail_flag: bool,
}

/// Parse the shared revision-gated surface tail (GC-08).
///
/// Form `0` carries the solved NURBS surface and fit tolerance. Form `2` carries
/// U/V intervals in the optional bool-gated form, four closure and singularity
/// enums, six discontinuity arrays, and a terminating boolean. The decoder
/// retains the containing record in native form for other values.
pub fn revision_surface_tail(cur: &mut Cur<'_>) -> Option<RevisionSurfaceTail> {
    let enumeration = cur.take_enum()?;
    let (fit_tolerance, solved_cache_domains, parameterization) = match enumeration {
        0 => {
            let (cache, cache_end) = surface_block(cur.toks(), cur.pos())?;
            cur.set_pos(cache_end);
            let domains = [
                [*cache.u_knots.first()?, *cache.u_knots.last()?],
                [*cache.v_knots.first()?, *cache.v_knots.last()?],
            ];
            (Some(cur.take_f64()? * LEN_TO_MM), Some(domains), None)
        }
        2 => {
            let u_interval = [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ];
            let v_interval = [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ];
            (
                None,
                None,
                Some(cadmpeg_ir::geometry::RevisionSurfaceParameterization {
                    u_interval,
                    v_interval,
                    u_closure: cur.take_enum()?,
                    v_closure: cur.take_enum()?,
                    u_singularity: cur.take_enum()?,
                    v_singularity: cur.take_enum()?,
                }),
            )
        }
        _ => return None,
    };
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let tail_flag = cur.take_bool()?;
    Some(RevisionSurfaceTail {
        enumeration,
        fit_tolerance,
        solved_cache_domains,
        parameterization,
        discontinuities,
        tail_flag,
    })
}

pub(crate) fn revision_cache_form(
    selector: i64,
    fit_tolerance: Option<f64>,
    parameterization: Option<RevisionSurfaceParameterization>,
) -> Option<RevisionCacheForm> {
    match (selector, fit_tolerance, parameterization) {
        (0, Some(fit_tolerance), None) => Some(RevisionCacheForm::SolvedCache { fit_tolerance }),
        (2, None, Some(parameterization)) => {
            Some(RevisionCacheForm::Parameterization(parameterization))
        }
        _ => None,
    }
}

fn off_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let names = ["off_spl_sur", "offsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let modern = name == "off_spl_sur";
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // The modern name uses the revision-gated layout.
        modern.then_some(())?;
        let table = resolver?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let (support, support_bounds) = optional_embedded_surface_with_bounds(&mut cur, table)?;
        let support = support?;
        let distance = cur.take_f64()? * LEN_TO_MM;
        // Four booleans carry the record orientation pair and the ASM extension
        // pair. The first repeats the support sense and orients the offset
        // displacement. The second leaves the point set unchanged. The revision
        // form reads these positions where the earlier form reads U/V sense
        // enums.
        let mut flags = Vec::with_capacity(4);
        for _ in 0..4 {
            flags.push(cur.take_bool()?);
        }
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            solved_cache_domains: _,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        cur.at_scope_end().then_some(())?;
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Offset {
                support,
                distance,
                u_sense: None,
                v_sense: None,
                extension_flags: Vec::new(),
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds,
                    reference_endpoints: [None; 2],
                    second_endpoints: [None; 2],
                    flags,
                    cache: revision_cache_form(tail_enum, fit_tolerance, parameterization)?,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: fit_tolerance,
        });
    }
    let support = embedded_surface(&mut cur)?;
    let distance = cur.take_f64()? * LEN_TO_MM;
    let u_sense = Some(cur.take_enum()?);
    let v_sense = Some(cur.take_enum()?);
    let mut extension_flags = Vec::new();
    if modern {
        let first = cur.take_bool()?;
        extension_flags.push(first);
        if first {
            extension_flags.push(cur.take_bool()?);
            if matches!(cur.peek(), Some(Token::True | Token::False)) {
                extension_flags.push(cur.take_bool()?);
            }
        }
    }
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = optional_trailing_cache_tolerance(&mut cur)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Offset {
            support,
            distance,
            u_sense,
            v_sense,
            extension_flags,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn rot_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let names = ["rot_spl_sur", "rotsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Revision-gated layout: revision integer, profile curve with two
        // optional endpoints, axis origin and direction, shared tail. The
        // modern name uses this layout.
        (name == "rot_spl_sur").then_some(())?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let table = resolver?;
        let profile = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let profile_endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let origin = cur.take_position()?;
        let axis = cur.take_vector3()?;
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            solved_cache_domains,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        cur.at_scope_end().then_some(())?;
        let angular_interval = solved_cache_domains?[1];
        let parameter_interval = [
            profile_endpoints[0].unwrap_or(*profile.knots.first()?),
            profile_endpoints[1].unwrap_or(*profile.knots.last()?),
        ];
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Revolution {
                directrix: CurveGeometry::Nurbs(profile),
                axis_origin: Point3::new(
                    origin[0] * LEN_TO_MM,
                    origin[1] * LEN_TO_MM,
                    origin[2] * LEN_TO_MM,
                ),
                axis_direction: normalized(axis)?,
                angular_interval,
                parameter_interval,
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds: [None; 4],
                    reference_endpoints: profile_endpoints,
                    second_endpoints: [None; 2],
                    flags: Vec::new(),
                    cache: revision_cache_form(tail_enum, fit_tolerance, parameterization)?,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: fit_tolerance,
        });
    }
    let (directrix, directrix_end) = curve_block(span, cur.pos())?;
    cur.set_pos(directrix_end);
    let parameter_interval = [*directrix.knots.first()?, *directrix.knots.last()?];
    let origin = cur.take_position()?;
    let axis_origin = Point3::new(
        origin[0] * LEN_TO_MM,
        origin[1] * LEN_TO_MM,
        origin[2] * LEN_TO_MM,
    );
    let axis = cur.take_vector3()?;
    let axis_direction = normalized(axis)?;
    let (cache, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let angular_interval = [*cache.v_knots.first()?, *cache.v_knots.last()?];
    let cache_fit_tolerance = optional_trailing_cache_tolerance(&mut cur)?;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Revolution {
            directrix: CurveGeometry::Nurbs(directrix),
            axis_origin,
            axis_direction,
            angular_interval,
            parameter_interval,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn sum_spl_sur(
    toks: &[Token],
    resolver: Option<&SubtypeTable>,
) -> Option<DecodedProceduralSurface> {
    let names = ["sum_spl_sur", "sumsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Revision-gated layout: revision integer, two curves each with two
        // optional endpoints, model-space origin, shared tail. The modern name
        // uses this layout.
        (name == "sum_spl_sur").then_some(())?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let table = resolver?;
        let first = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let first_endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let second = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let second_endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let origin = cur.take_position()?;
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            solved_cache_domains: _,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        cur.at_scope_end().then_some(())?;
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Sum {
                first: CurveGeometry::Nurbs(first),
                second: CurveGeometry::Nurbs(second),
                basepoint: Vector3::new(
                    origin[0] * LEN_TO_MM,
                    origin[1] * LEN_TO_MM,
                    origin[2] * LEN_TO_MM,
                ),
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds: [None; 4],
                    reference_endpoints: first_endpoints,
                    second_endpoints,
                    flags: Vec::new(),
                    cache: revision_cache_form(tail_enum, fit_tolerance, parameterization)?,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: fit_tolerance,
        });
    }
    let (first, first_end) = curve_block(span, cur.pos())?;
    cur.set_pos(first_end);
    let (second, second_end) = curve_block(span, cur.pos())?;
    cur.set_pos(second_end);
    let origin = cur.take_position()?;
    let basepoint = Vector3::new(
        origin[0] * LEN_TO_MM,
        origin[1] * LEN_TO_MM,
        origin[2] * LEN_TO_MM,
    );
    let cache_fit_tolerance = if cur.at_scope_end() {
        None
    } else {
        let (_, cache_end) = surface_block(span, cur.pos())?;
        cur.set_pos(cache_end);
        optional_trailing_cache_tolerance(&mut cur)?
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Sum {
            first: CurveGeometry::Nurbs(first),
            second: CurveGeometry::Nurbs(second),
            basepoint,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn ruled_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["rule_sur", "rulesur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let (first, first_end) = curve_block(span, cur.pos())?;
    cur.set_pos(first_end);
    let (second, second_end) = curve_block(span, cur.pos())?;
    cur.set_pos(second_end);
    let cache_fit_tolerance = if cur.at_scope_end() {
        None
    } else {
        let (_, cache_end) = surface_block(span, cur.pos())?;
        cur.set_pos(cache_end);
        optional_trailing_cache_tolerance(&mut cur)?
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Ruled { first, second },
        cache_fit_tolerance,
    })
}

fn exact_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    let names = ["exact_spl_sur", "exactsur"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Revision-gated layout: revision integer, shared tail, four optional
        // parameter values, and the extension as an enum. The modern name uses
        // this layout.
        (name == "exact_spl_sur").then_some(())?;
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            solved_cache_domains: _,
            parameterization,
            discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        // The two unextended parameter intervals, each an ordered [lo, hi] pair
        // of optional bounds. This subtype serializes them U-then-V; loft wrap
        // ranges sharing `RevisionRanges` serialize V-then-U. Store the
        // intervals by position and use the specification's labels.
        let unextended_ranges = [
            [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ],
            [
                cur.take_optional_range_value()?,
                cur.take_optional_range_value()?,
            ],
        ];
        let extension = cur.take_enum()?;
        cur.at_scope_end().then_some(())?;
        return Some(DecodedProceduralSurface {
            definition: DecodedProceduralSurfaceDefinition::Exact {
                parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::RevisionRanges {
                    intervals: unextended_ranges,
                },
                extension,
                revision_form: Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds: [None; 4],
                    reference_endpoints: [None; 2],
                    second_endpoints: [None; 2],
                    flags: Vec::new(),
                    cache: revision_cache_form(tail_enum, fit_tolerance, parameterization)?,
                    discontinuities,
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
            },
            cache_fit_tolerance: fit_tolerance,
        });
    }
    let (_, cache_end) = surface_block(span, cur.pos())?;
    cur.set_pos(cache_end);
    let cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
    let parameter_ranges = [
        [cur.take_range_value()?, cur.take_range_value()?],
        [cur.take_range_value()?, cur.take_range_value()?],
    ];
    let extension = cur.take_long()?;
    cur.at_scope_end().then_some(())?;
    let _ = name;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Exact {
            parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::OrderedRanges {
                ranges: parameter_ranges,
            },
            extension,
            revision_form: None,
        },
        cache_fit_tolerance,
    })
}

fn t_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::{TSplineSubtransform, TSplineSurfaceConstruction};

    let (start, _) = toks::find_owned_subtype_marker(toks, &["t_spl_sur"])?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let (
        cache_fit_tolerance,
        discontinuities,
        discontinuity_flag,
        parameter_ranges,
        type_code,
        revision_form,
    );
    if matches!(cur.peek(), Some(Token::Long(_))) {
        // Revision-gated layout: revision integer, shared tail, four optional
        // parameter values, the type code as an enum, then the nested
        // subtransform scope and trailing integer.
        let revision = cur.take_long()?;
        (revision > 0).then_some(())?;
        let RevisionSurfaceTail {
            enumeration: tail_enum,
            fit_tolerance,
            solved_cache_domains: _,
            parameterization,
            discontinuities: tail_discontinuities,
            tail_flag,
        } = revision_surface_tail(&mut cur)?;
        let mut bounds = [None; 4];
        for bound in &mut bounds {
            *bound = cur.take_optional_range_value()?;
        }
        cache_fit_tolerance = fit_tolerance;
        discontinuities = tail_discontinuities.clone();
        discontinuity_flag = tail_flag;
        parameter_ranges = [
            [bounds[0].unwrap_or(0.0), bounds[1].unwrap_or(0.0)],
            [bounds[2].unwrap_or(0.0), bounds[3].unwrap_or(0.0)],
        ];
        type_code = cur.take_enum()?;
        revision_form = Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
            revision,
            support_bounds: bounds,
            reference_endpoints: [None; 2],
            second_endpoints: [None; 2],
            flags: Vec::new(),
            cache: revision_cache_form(tail_enum, fit_tolerance, parameterization)?,
            discontinuities: tail_discontinuities,
            tail_flag,
            trailing_flags: Vec::new(),
        });
    } else {
        let (_, cache_end) = surface_block(span, cur.pos())?;
        cur.set_pos(cache_end);
        cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
        discontinuities = [
            cur.take_float_array()?,
            cur.take_float_array()?,
            cur.take_float_array()?,
            cur.take_float_array()?,
            cur.take_float_array()?,
            cur.take_float_array()?,
        ];
        discontinuity_flag = cur.take_bool()?;
        parameter_ranges = [
            [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM],
            [cur.take_f64()? * LEN_TO_MM, cur.take_f64()? * LEN_TO_MM],
        ];
        type_code = cur.take_long()?;
        revision_form = None;
    }
    if !matches!(cur.peek(), Some(Token::SubtypeOpen)) {
        return None;
    }
    cur.bump();
    let source_kind = cur.take_ident()?;
    let subtransform = match source_kind {
        "t_spl_subtrans_object" => {
            let program = cur.take_str()?.to_string();
            let separator = if matches!(cur.peek(), Some(Token::Str(_))) {
                None
            } else {
                Some(cur.take_bool()?)
            };
            let values = cur.take_str()?.to_string();
            TSplineSubtransform::Inline {
                program,
                separator,
                values,
            }
        }
        "ref" => TSplineSubtransform::Reference {
            index: cur.take_long()?,
            resolved: None,
        },
        _ => return None,
    };
    if !matches!(cur.peek(), Some(Token::SubtypeClose)) {
        return None;
    }
    cur.bump();
    let trailing_value = cur.take_long()?;
    cur.at_scope_end().then_some(())?;
    let program_graph = match &subtransform {
        TSplineSubtransform::Inline { program, .. } => {
            Some(cadmpeg_ir::geometry::TSplineProgram::parse(program))
        }
        TSplineSubtransform::Reference { .. } => None,
    };
    let values_graph = match &subtransform {
        TSplineSubtransform::Inline { values, .. } => {
            Some(cadmpeg_ir::geometry::TSplineProgram::parse(values))
        }
        TSplineSubtransform::Reference { .. } => None,
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::TSpline(Box::new(
            TSplineSurfaceConstruction {
                parameter_ranges,
                type_code,
                subtransform,
                program_graph,
                values_graph,
                trailing_value,
                discontinuities,
                discontinuity_flag,
                revision_form,
            },
        )),
        cache_fit_tolerance,
    })
}

fn deformable_surface_frame(
    cur: &mut Cur<'_>,
) -> Option<cadmpeg_ir::geometry::DeformableSurfaceFrame> {
    let mut leading_vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
    for vector in &mut leading_vectors {
        let value = cur.take_vector3()?;
        *vector = Vector3::new(value[0], value[1], value[2]);
    }
    let leading_parameter = cur.take_f64()?;
    let leading_flags = [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?];
    let mut secondary_vectors = [Vector3::new(0.0, 0.0, 0.0); 3];
    for vector in &mut secondary_vectors {
        let value = cur.take_vector3()?;
        *vector = Vector3::new(value[0], value[1], value[2]);
    }
    let secondary_parameter = cur.take_f64()?;
    let secondary_flags = [cur.take_bool()?, cur.take_bool()?];
    let point = cur.take_position()?;
    let trailing_flags = [
        cur.take_bool()?,
        cur.take_bool()?,
        cur.take_bool()?,
        cur.take_bool()?,
        cur.take_bool()?,
    ];
    Some(cadmpeg_ir::geometry::DeformableSurfaceFrame {
        leading_vectors,
        leading_parameter,
        leading_flags,
        secondary_vectors,
        secondary_parameter,
        secondary_flags,
        point: Point3::new(
            point[0] * LEN_TO_MM,
            point[1] * LEN_TO_MM,
            point[2] * LEN_TO_MM,
        ),
        trailing_flags,
    })
}

fn deformable_vector_frame(
    cur: &mut Cur<'_>,
) -> Option<cadmpeg_ir::geometry::DeformableVectorFrame> {
    let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
    for vector in &mut vectors {
        let value = cur.take_vector3()?;
        *vector = Vector3::new(value[0], value[1], value[2]);
    }
    Some(cadmpeg_ir::geometry::DeformableVectorFrame {
        vectors,
        parameter: cur.take_f64()?,
        flags: [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?],
    })
}

fn revision_deformable_mode3(
    cur: &mut Cur<'_>,
) -> Option<cadmpeg_ir::geometry::DeformableSurfaceData> {
    let mut leading_vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
    for vector in &mut leading_vectors {
        let value = cur.take_vector3()?;
        *vector = Vector3::new(value[0], value[1], value[2]);
    }
    let leading_parameter = cur.take_f64()?;
    let leading_flags = [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?];
    let point = cur.take_position()?;
    let trailing_point = Point3::new(
        point[0] * LEN_TO_MM,
        point[1] * LEN_TO_MM,
        point[2] * LEN_TO_MM,
    );
    let mut trailing_vectors = [Vector3::new(0.0, 0.0, 0.0); 2];
    for vector in &mut trailing_vectors {
        let value = cur.take_vector3()?;
        *vector = Vector3::new(value[0], value[1], value[2]);
    }
    let frame_parameter = cur.take_f64()?;
    let frame_flags = [cur.take_bool()?, cur.take_bool()?];
    let parameters = [cur.take_f64()?, cur.take_f64()?, cur.take_f64()?];
    let trailing_flags = [
        cur.take_bool()?,
        cur.take_bool()?,
        cur.take_bool()?,
        cur.take_bool()?,
        cur.take_bool()?,
    ];
    let trailing_parameter = cur.take_f64()?;
    let trailing_value = cur.take_long()?;
    Some(cadmpeg_ir::geometry::DeformableSurfaceData::RevisionMode3 {
        leading_vectors,
        leading_parameter,
        leading_flags,
        trailing_point,
        trailing_vectors,
        frame_parameter,
        frame_flags,
        parameters,
        trailing_flags,
        trailing_parameter,
        trailing_value,
    })
}

fn defm_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::DeformableSurfaceData;
    let names = ["defm_spl_sur", "defmsur"];
    let (start, _) = toks::find_owned_subtype_marker(toks, &names)?;
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let (support, revision_form_head) = if matches!(cur.peek(), Some(Token::Long(_))) {
        let revision = cur.take_long()?;
        (revision == 22_506).then_some(())?;
        let (support, ranges) = embedded_surface_with_ranges(&mut cur)?;
        let support_bounds = [ranges[0][0], ranges[0][1], ranges[1][0], ranges[1][1]];
        (support, Some((revision, support_bounds)))
    } else {
        (embedded_surface(&mut cur)?, None)
    };
    let mode = cur.take_long()?;
    let data = match mode {
        1 => {
            let frame = Box::new(deformable_surface_frame(&mut cur)?);
            let count = usize::try_from(cur.take_long()?).ok()?;
            let parameter_triples = (0..count)
                .map(|_| Some([cur.take_f64()?, cur.take_f64()?, cur.take_f64()?]))
                .collect::<Option<Vec<_>>>()?;
            EmbeddedDeformableSurfaceData::Resolved(DeformableSurfaceData::Plain {
                frame,
                parameter_triples,
            })
        }
        3 if revision_form_head.is_some() => {
            EmbeddedDeformableSurfaceData::Resolved(revision_deformable_mode3(&mut cur)?)
        }
        3 => EmbeddedDeformableSurfaceData::Resolved(DeformableSurfaceData::Guided {
            frame: Box::new(deformable_surface_frame(&mut cur)?),
            selector: cur.take_long()?,
            guide_parameter: cur.take_f64()?,
        }),
        5 => {
            let surface = embedded_surface(&mut cur)?;
            let native_id = cur.take_long()?;
            let flag = cur.take_bool()?;
            let first_parameter = cur.take_f64()?;
            let selector = cur.take_long()?;
            let second_parameter = cur.take_f64()?;
            let (curve, curve_end) = curve_block(span, cur.pos())?;
            cur.set_pos(curve_end);
            let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut vectors {
                let value = cur.take_vector3()?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            let frame_parameter = cur.take_f64()?;
            let flags = [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?];
            let count = usize::try_from(cur.take_long()?).ok()?;
            let parameter_triples = (0..count)
                .map(|_| Some([cur.take_f64()?, cur.take_f64()?, cur.take_f64()?]))
                .collect::<Option<Vec<_>>>()?;
            EmbeddedDeformableSurfaceData::SurfaceCurve {
                surface,
                native_id,
                flag,
                first_parameter,
                selector,
                second_parameter,
                curve,
                vectors,
                frame_parameter,
                flags,
                parameter_triples,
            }
        }
        6 => {
            let mut leading_vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut leading_vectors {
                let value = cur.take_vector3()?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            let leading_parameter = cur.take_f64()?;
            let leading_flags = [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?];
            let selector = cur.take_long()?;
            let surface = embedded_surface(&mut cur)?;
            let native_id = cur.take_long()?;
            let flag = cur.take_bool()?;
            let first_parameter = cur.take_f64()?;
            let version_value = matches!(cur.peek(), Some(Token::Long(_)))
                .then(|| cur.take_long())
                .flatten();
            let second_parameter = cur.take_f64()?;
            let (curve, curve_end) = curve_block(span, cur.pos())?;
            cur.set_pos(curve_end);
            let frames = Box::new([
                deformable_vector_frame(&mut cur)?,
                deformable_vector_frame(&mut cur)?,
            ]);
            EmbeddedDeformableSurfaceData::Full {
                leading_vectors,
                leading_parameter,
                leading_flags,
                selector,
                surface,
                native_id,
                flag,
                first_parameter,
                version_value,
                second_parameter,
                curve,
                frames,
                trailing_value: cur.take_long()?,
            }
        }
        8 => {
            let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut vectors {
                let value = cur.take_vector3()?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            EmbeddedDeformableSurfaceData::Resolved(DeformableSurfaceData::Minimal {
                vectors,
                selector: cur.take_long()?,
            })
        }
        _ => return None,
    };
    let (revision_form, cache_fit_tolerance, discontinuities, discontinuity_flag) =
        if let Some((revision, support_bounds)) = revision_form_head {
            let RevisionSurfaceTail {
                enumeration: tail_enum,
                fit_tolerance,
                solved_cache_domains: _,
                parameterization: tail_parameterization,
                discontinuities,
                tail_flag,
            } = revision_surface_tail(&mut cur)?;
            (
                Some(cadmpeg_ir::geometry::RevisionSurfaceForm {
                    revision,
                    support_bounds,
                    reference_endpoints: [None; 2],
                    second_endpoints: [None; 2],
                    flags: Vec::new(),
                    cache: revision_cache_form(tail_enum, fit_tolerance, tail_parameterization)?,
                    discontinuities: discontinuities.clone(),
                    tail_flag,
                    trailing_flags: Vec::new(),
                }),
                fit_tolerance,
                discontinuities,
                tail_flag,
            )
        } else {
            let (_, cache_end) = surface_block(span, cur.pos())?;
            cur.set_pos(cache_end);
            let cache_fit_tolerance = Some(cur.take_f64()? * LEN_TO_MM);
            let discontinuities = [
                cur.take_float_array()?,
                cur.take_float_array()?,
                cur.take_float_array()?,
                cur.take_float_array()?,
                cur.take_float_array()?,
                cur.take_float_array()?,
            ];
            let discontinuity_flag = cur.take_bool()?;
            (
                None,
                cache_fit_tolerance,
                discontinuities,
                discontinuity_flag,
            )
        };
    if revision_form.is_some() {
        cur.at_scope_end().then_some(())?;
    }
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Deformable(Box::new(
            EmbeddedDeformableSurface {
                support,
                revision_form,
                data,
                discontinuities,
                discontinuity_flag,
            },
        )),
        cache_fit_tolerance,
    })
}

pub(crate) fn helix_spl_sur(toks: &[Token]) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::{
        HelixPathConstruction, HelixSurfaceConstruction, HelixSurfaceProfile,
    };

    let names = ["helix_spl_circ", "helix_spl_line"];
    let (start, name) = toks::find_owned_subtype_marker(toks, &names)?;
    let circular = name == "helix_spl_circ";
    let span = toks::subtype_span(toks, start)?;
    let mut cur = Cur::at(span, 2);
    let current_layout = optional_helix_revision(&mut cur)?;
    let angle_range = [cur.take_range_value()?, cur.take_range_value()?];
    let dimension_scale = if circular { LEN_TO_MM } else { 1.0 };
    let dimension_range = [
        cur.take_range_value()? * dimension_scale,
        cur.take_range_value()? * dimension_scale,
    ];
    let length = circular
        .then(|| cur.take_f64().map(|v| v * LEN_TO_MM))
        .flatten();
    let path_angle_range = [cur.take_range_value()?, cur.take_range_value()?];
    let center = cur.take_position()?;
    let take_frame_vector = |cur: &mut Cur<'_>| {
        if current_layout {
            cur.take_vector3()
        } else {
            cur.take_position()
        }
    };
    let major = take_frame_vector(&mut cur)?;
    let minor = take_frame_vector(&mut cur)?;
    let pitch = take_frame_vector(&mut cur)?;
    let apex_factor = cur.take_f64()?;
    let axis = normalized(cur.take_vector3()?)?;
    for sentinel in ["null_surface", "null_surface", "nullbs", "nullbs"] {
        if cur.take_ident()? != sentinel {
            return None;
        }
    }
    let path = HelixPathConstruction {
        angle_range: path_angle_range,
        center: Point3::new(
            center[0] * LEN_TO_MM,
            center[1] * LEN_TO_MM,
            center[2] * LEN_TO_MM,
        ),
        major: Vector3::new(
            major[0] * LEN_TO_MM,
            major[1] * LEN_TO_MM,
            major[2] * LEN_TO_MM,
        ),
        minor: Vector3::new(
            minor[0] * LEN_TO_MM,
            minor[1] * LEN_TO_MM,
            minor[2] * LEN_TO_MM,
        ),
        pitch: Vector3::new(
            pitch[0] * LEN_TO_MM,
            pitch[1] * LEN_TO_MM,
            pitch[2] * LEN_TO_MM,
        ),
        apex_factor,
        axis,
    };
    let profile = if let Some(length) = length {
        HelixSurfaceProfile::Circle {
            length,
            radius: cur.take_f64()? * LEN_TO_MM,
        }
    } else {
        let direction = take_frame_vector(&mut cur)?;
        HelixSurfaceProfile::Line {
            direction: Vector3::new(
                direction[0] * LEN_TO_MM,
                direction[1] * LEN_TO_MM,
                direction[2] * LEN_TO_MM,
            ),
        }
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Helix(Box::new(HelixSurfaceConstruction {
            angle_range,
            dimension_range,
            path,
            profile,
        })),
        cache_fit_tolerance: None,
    })
}

fn t_spline_subtransform(span: &[Token]) -> Option<cadmpeg_ir::geometry::TSplineSubtransform> {
    use cadmpeg_ir::geometry::TSplineSubtransform;

    let start = usize::from(matches!(span.first(), Some(Token::SubtypeOpen)));
    let mut cur = Cur::at(span, start);
    match cur.take_ident()? {
        "t_spl_subtrans_object" => {
            let program = cur.take_str()?.to_string();
            let separator = if matches!(cur.peek(), Some(Token::Str(_))) {
                None
            } else {
                Some(cur.take_bool()?)
            };
            let values = cur.take_str()?.to_string();
            Some(TSplineSubtransform::Inline {
                program,
                separator,
                values,
            })
        }
        "ref" => Some(TSplineSubtransform::Reference {
            index: cur.take_long()?,
            resolved: None,
        }),
        _ => None,
    }
}

fn resolve_t_spline_subtransform(
    index: usize,
    table: &SubtypeTable,
    seen: &mut Vec<usize>,
) -> Option<cadmpeg_ir::geometry::TSplineSubtransform> {
    use cadmpeg_ir::geometry::TSplineSubtransform;

    if seen.contains(&index) {
        return None;
    }
    seen.push(index);
    let decoded = t_spline_subtransform(table.span(index)?)?;
    match decoded {
        inline @ TSplineSubtransform::Inline { .. } => Some(inline),
        TSplineSubtransform::Reference { index, .. } => {
            resolve_t_spline_subtransform(usize::try_from(index).ok()?, table, seen)
        }
    }
}

/// Decode a native procedural definition, following nested subtype-table references.
pub fn procedural_surface_resolving_refs(
    toks: &[Token],
    table: &SubtypeTable,
) -> Option<DecodedProceduralSurface> {
    procedural_resolving_refs(toks, table, &mut Vec::new())
}

fn procedural_resolving_refs(
    toks: &[Token],
    table: &SubtypeTable,
    seen: &mut Vec<usize>,
) -> Option<DecodedProceduralSurface> {
    if let Some(mut decoded) = defm_spl_sur(toks)
        .or_else(|| helix_spl_sur(toks))
        .or_else(|| t_spl_sur(toks))
        .or_else(|| exact_spl_sur(toks))
        .or_else(|| comp_spl_sur(toks))
        .or_else(|| taper_spl_sur(toks, Some(table)))
        .or_else(|| loft_spl_sur(toks, Some(table)))
        .or_else(|| compound_loft_spl_sur(toks, Some(table)))
        .or_else(|| scaled_compound_loft_spl_sur(toks))
        .or_else(|| sub_spl_sur(toks))
        .or_else(|| law_spl_sur(toks))
        .or_else(|| skin_spl_sur(toks))
        .or_else(|| net_spl_sur(toks))
        .or_else(|| sweep_spl_sur(toks, Some(table)))
        .or_else(|| g2_blend_spl_sur(toks, Some(table)))
        .or_else(|| ruled_spl_sur(toks))
        .or_else(|| sum_spl_sur(toks, Some(table)))
        .or_else(|| rot_spl_sur(toks, Some(table)))
        .or_else(|| off_spl_sur(toks, Some(table)))
        .or_else(|| cyl_spl_sur(toks, Some(table)))
        .or_else(|| var_blend_spl_sur(toks, Some(table)))
        .or_else(|| vertex_blend_spl_sur(toks, Some(table)))
        .or_else(|| full_rb_blend_spl_sur(toks, table))
        .or_else(|| compact_rb_blend_spl_sur(toks))
    {
        if let DecodedProceduralSurfaceDefinition::TSpline(construction) = &mut decoded.definition {
            if let cadmpeg_ir::geometry::TSplineSubtransform::Reference { index, resolved } =
                &mut construction.subtransform
            {
                let inline = resolve_t_spline_subtransform(
                    usize::try_from(*index).ok()?,
                    table,
                    &mut Vec::new(),
                )?;
                let program = match &inline {
                    cadmpeg_ir::geometry::TSplineSubtransform::Inline { program, .. } => program,
                    cadmpeg_ir::geometry::TSplineSubtransform::Reference { .. } => return None,
                };
                construction.program_graph =
                    Some(cadmpeg_ir::geometry::TSplineProgram::parse(program));
                let values = match &inline {
                    cadmpeg_ir::geometry::TSplineSubtransform::Inline { values, .. } => values,
                    cadmpeg_ir::geometry::TSplineSubtransform::Reference { .. } => return None,
                };
                construction.values_graph =
                    Some(cadmpeg_ir::geometry::TSplineProgram::parse(values));
                *resolved = Some(Box::new(inline));
            }
        }
        return Some(decoded);
    }
    // Follow references for records whose own construction is absent. A record
    // with an undecoded construction keeps its native data; its references
    // belong to that construction's supports.
    if toks::owned_subtype_defs(toks)
        .iter()
        .any(|(_, name)| *name != "ref")
    {
        return None;
    }
    for index in toks::subtype_refs(toks) {
        if seen.contains(&index) {
            continue;
        }
        let target = table.span(index)?;
        seen.push(index);
        if let Some(decoded) = procedural_resolving_refs(target, table, seen) {
            return Some(decoded);
        }
    }
    None
}

#[cfg(test)]
mod sweep_law_tests {
    use super::*;

    #[test]
    fn sweep_text_law_consumes_one_serializer_token() {
        let tokens = [Token::Str("0.008726867790758789*X".into()), Token::Long(21)];
        let mut cur = Cur::at(&tokens, 0);

        let law = sweep_law_expression(&mut cur).expect("text law");

        let EmbeddedLawExpression::Text(value) = law else {
            panic!("expected text law");
        };
        assert_eq!(value, "0.008726867790758789*X");
        assert_eq!(cur.take_long(), Some(21));
        assert_eq!(cur.pos(), tokens.len());
    }

    #[test]
    fn composition_law_consumes_two_recursive_operands() {
        let tokens = [
            Token::Str("O".into()),
            Token::Str("ABS".into()),
            Token::Double(-2.5),
            Token::Str("SIN".into()),
            Token::Double(0.25),
        ];
        let mut cur = Cur::at(&tokens, 0);

        let law = law_expression(&mut cur, 0).expect("composition law");

        assert!(matches!(
            law,
            EmbeddedLawExpression::Algebraic { operator, operands }
                if operator == "O"
                    && matches!(operands.as_slice(), [
                        EmbeddedLawExpression::Algebraic { operator: left, operands: left_operands },
                        EmbeddedLawExpression::Algebraic { operator: right, operands: right_operands },
                    ] if left == "ABS"
                        && matches!(left_operands.as_slice(), [EmbeddedLawExpression::Double(value)] if *value == -2.5)
                        && right == "SIN"
                        && matches!(right_operands.as_slice(), [EmbeddedLawExpression::Double(value)] if *value == 0.25))
        ));
        assert_eq!(cur.pos(), tokens.len());
    }

    #[test]
    fn rail_formula_decodes_counted_vector_transform_binding() {
        let vectors = [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [2.0, 3.0, 4.0],
        ];
        let mut tokens = vec![
            Token::Str("ROTATE(DOMAIN(VEC(1,0,0),0,0.8),TRANS1)".into()),
            Token::Long(1),
            Token::Str("TRANS".into()),
        ];
        tokens.extend(vectors.into_iter().map(Token::Vector3));
        tokens.extend([Token::Double(1.5), Token::True, Token::False, Token::True]);
        let mut cur = Cur::at(&tokens, 0);

        let formula = law_formula_resolving(&mut cur, None).expect("rail formula");

        assert_eq!(formula.name, "ROTATE(DOMAIN(VEC(1,0,0),0,0.8),TRANS1)");
        let [EmbeddedLawExpression::TransformVec {
            vectors: actual_vectors,
            scale,
            flags,
        }] = formula.variables.as_slice()
        else {
            panic!("expected one vector transform binding");
        };
        assert_eq!(
            *actual_vectors,
            vectors.map(|value| Vector3::new(value[0], value[1], value[2]))
        );
        assert_eq!(*scale, 1.5);
        assert_eq!(*flags, [true, false, true]);
        assert_eq!(cur.pos(), tokens.len());
    }
}

#[cfg(test)]
mod tail_selector_tests {
    use super::*;

    /// A four-byte enum field.
    fn push_enum(span: &mut Vec<u8>, value: i32) {
        span.push(0x15);
        span.extend_from_slice(&value.to_le_bytes());
    }

    /// A four-byte counted float array of `count` zeros.
    fn push_float_array(span: &mut Vec<u8>, count: i32) {
        span.push(0x04);
        span.extend_from_slice(&count.to_le_bytes());
        for _ in 0..count {
            span.push(0x06);
            span.extend_from_slice(&0.0f64.to_le_bytes());
        }
    }

    /// A shared revision-gated surface tail whose opening enum selects an
    /// undefined cache form. The helper rejects the form and retains the
    /// containing record verbatim.
    #[test]
    fn undefined_tail_form_is_rejected_for_verbatim_retention() {
        // Enum with value 1, followed by a value that could otherwise open a
        // solved cache block's fields.
        let mut span = Vec::new();
        push_enum(&mut span, 1);
        span.push(0x06);
        span.extend_from_slice(&0.0f64.to_le_bytes());
        let toks = toks::lex_test_span(&span, 4);
        let mut cur = Cur::at(&toks, 0);
        assert!(revision_surface_tail(&mut cur).is_none());
    }

    /// Tail form `2` stores no cache and no fit tolerance: the U parameter
    /// interval, the V parameter interval, then U closure, V closure, U
    /// singularity, and V singularity.
    #[test]
    fn parameterized_tail_form_decodes_intervals_then_closure_enums() {
        let mut span = Vec::new();
        push_enum(&mut span, 2);
        // U interval: present lower bound, absent upper bound.
        span.push(0x0a);
        span.push(0x06);
        span.extend_from_slice(&0.25f64.to_le_bytes());
        span.push(0x0b);
        // V interval: both bounds present.
        for value in [(-1.5f64), 3.5] {
            span.push(0x0a);
            span.push(0x06);
            span.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1, 0, 2, 3] {
            push_enum(&mut span, value);
        }
        push_float_array(&mut span, 1);
        for _ in 0..5 {
            push_float_array(&mut span, 0);
        }
        span.push(0x0b);

        let toks = toks::lex_test_span(&span, 4);
        let mut cur = Cur::at(&toks, 0);
        let tail = revision_surface_tail(&mut cur).expect("parameterized tail");
        assert_eq!(cur.pos(), toks.len());
        assert_eq!(tail.enumeration, 2);
        assert_eq!(tail.fit_tolerance, None);
        assert_eq!(tail.solved_cache_domains, None);
        let parameterization = tail.parameterization.expect("parameterization");
        assert_eq!(parameterization.u_interval, [Some(0.25), None]);
        assert_eq!(parameterization.v_interval, [Some(-1.5), Some(3.5)]);
        assert_eq!(parameterization.u_closure, 1);
        assert_eq!(parameterization.v_closure, 0);
        assert_eq!(parameterization.u_singularity, 2);
        assert_eq!(parameterization.v_singularity, 3);
        assert_eq!(tail.discontinuities[0], [0.0]);
        assert!(tail.discontinuities[1..].iter().all(Vec::is_empty));
        assert!(!tail.tail_flag);
    }
}
