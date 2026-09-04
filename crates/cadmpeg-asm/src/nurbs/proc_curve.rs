// SPDX-License-Identifier: Apache-2.0
//! Procedural curve embedded types, decoders, resolving-ref walkers, and writer-facing patch layouts.

use crate::nurbs::blend::{
    decode_optional_rolling_ball_surface, decode_rolling_ball_side, decode_surface_ranges,
    optional_rolling_ball_surface, surface_ranges,
};
use crate::nurbs::core::{
    curve_block, decode_curve_block, decode_surface_block, owned_curve_cache_resolving_refs,
    owned_surface_cache_resolving_refs, surface_block,
};
use crate::nurbs::pcurve::{decode_pcurve_block_with_end, pcurve_block_with_end, NurbsPcurve};
use crate::nurbs::proc_surface::{
    decode_nullable_embedded_pcurve, ellipse_to_nurbs, law_formula, nullable_embedded_pcurve,
    EmbeddedLawFormula,
};
use crate::nurbs::reader::{
    normalized, take_bool, take_double_payload, take_f64, take_float_array_payloads,
    take_native_ident, take_native_string, take_native_vec3, take_range_value, take_tagged_int,
    Nullable, LEN_TO_MM,
};
use crate::nurbs::subtypes::{
    find_owned_intcurve_subtype, find_owned_subtype_marker, subtype_span, SubtypeTables,
};
use crate::nurbs::toks::{Cur, SubtypeTable};
use crate::sab::Token;
use cadmpeg_ir::geometry::{NurbsCurve, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

const EPS_PARAMETER_AGREEMENT: f64 = 1.0e-12;

/// Source curve and tail fields decoded from an `offset_int_cur` construction.
pub type VectorOffsetDefinition = (NurbsCurve, [f64; 2], Vector3, [String; 2], [i64; 2]);

/// Parent curve and retained range decoded from a `subset_int_cur` construction.
pub type SubsetDefinition = (NurbsCurve, [f64; 2]);

/// Parameter arrays and child curves decoded from a `comp_int_cur` construction.
pub type CompoundDefinition = (Vec<f64>, Vec<f64>, Vec<NurbsCurve>);

/// Embedded freeform support carriers and tail fields of an `off_int_cur`.
pub struct EmbeddedTwoSidedOffset {
    /// Two ordered embedded support surfaces.
    pub surfaces: [Option<SurfaceGeometry>; 2],
    /// Two ordered embedded NURBS parameter curves.
    pub pcurves: [Option<NurbsPcurve>; 2],
    /// Shared native parameter interval.
    pub parameter_range: [f64; 2],
    /// Three discontinuity arrays.
    pub discontinuities: [Vec<f64>; 3],
    /// The boolean serialized after the discontinuity arrays.
    pub discontinuity_flag: bool,
    /// Signed side offsets in document length units.
    pub offsets: [f64; 2],
}

/// Embedded support carriers and shared fields of an `int_int_cur`.
pub struct EmbeddedIntersection {
    /// Two ordered embedded support surfaces.
    pub surfaces: [Option<SurfaceGeometry>; 2],
    /// Whether each native support slot contains a valid non-null support.
    ///
    /// A procedural support can be valid without a standalone
    /// `SurfaceGeometry` cache, so this is distinct from `surfaces`.
    pub support_present: [bool; 2],
    /// Two ordered embedded NURBS parameter curves.
    pub pcurves: [Option<NurbsPcurve>; 2],
    /// Shared native parameter interval.
    pub parameter_range: [f64; 2],
    /// Three discontinuity arrays.
    pub discontinuities: [Vec<f64>; 3],
}

/// Embedded support context and family-specific cache-first tail of a native
/// surface curve.
pub enum EmbeddedSurfaceCurve {
    /// Blend family.
    Blend {
        /// Embedded support context.
        context: EmbeddedIntersection,
        /// Optional cache-first tail and family flag.
        tail: Option<cadmpeg_ir::geometry::SurfaceCurveCacheFirst<bool>>,
    },
    /// Surface-constrained family.
    SurfaceConstrained {
        /// Embedded support context.
        context: EmbeddedIntersection,
        /// Optional cache-first tail and family flag.
        tail: Option<cadmpeg_ir::geometry::SurfaceCurveCacheFirst<bool>>,
    },
    /// Parametric family with its optional second tail flag.
    Parametric {
        /// Embedded support context.
        context: EmbeddedIntersection,
        /// Optional cache-first tail and parametric-family flags.
        tail: Option<
            cadmpeg_ir::geometry::SurfaceCurveCacheFirst<
                cadmpeg_ir::geometry::ParametricSurfaceCurveFlags,
            >,
        >,
    },
    /// Skin family.
    Skin {
        /// Embedded support context.
        context: EmbeddedIntersection,
        /// Optional cache-first tail and family flag.
        tail: Option<cadmpeg_ir::geometry::SurfaceCurveCacheFirst<bool>>,
    },
}

impl EmbeddedSurfaceCurve {
    fn context(&self) -> &EmbeddedIntersection {
        match self {
            Self::Blend { context, .. }
            | Self::SurfaceConstrained { context, .. }
            | Self::Parametric { context, .. }
            | Self::Skin { context, .. } => context,
        }
    }
}

#[derive(Clone, Copy)]
enum NativeSupportChart {
    Canonical,
    PlaneLengths,
    Cone { axial_scale: f64 },
}

fn cone_axial_scale(sine: f64, cosine: f64, u_scale: f64) -> f64 {
    let direction = if sine * cosine < 0.0 { -1.0 } else { 1.0 };
    direction * cosine * u_scale * LEN_TO_MM
}

/// The native parameter chart of the support serialized at token `position`.
fn native_support_chart(toks: &[Token], position: usize) -> NativeSupportChart {
    let mut cur = Cur::at(toks, position);
    let Some(kind) = cur.take_ident() else {
        return NativeSupportChart::Canonical;
    };
    match kind {
        "plane" => NativeSupportChart::PlaneLengths,
        "cone" => {
            let parsed = (|| {
                cur.take_position()?;
                cur.take_vector3()?;
                cur.take_vector3()?;
                cur.take_f64()?;
                cur.take_bool()?;
                cur.take_bool()?;
                let sine = cur.take_f64()?;
                let cosine = cur.take_f64()?;
                let u_scale = cur.take_f64()?;
                Some(NativeSupportChart::Cone {
                    axial_scale: cone_axial_scale(sine, cosine, u_scale),
                })
            })();
            parsed.unwrap_or(NativeSupportChart::Canonical)
        }
        _ => NativeSupportChart::Canonical,
    }
}

fn normalize_support_pcurve(chart: NativeSupportChart, pcurve: &mut NurbsPcurve) {
    match chart {
        NativeSupportChart::Canonical => {}
        NativeSupportChart::PlaneLengths => {
            for point in &mut pcurve.control_points {
                point.u *= LEN_TO_MM;
                point.v *= -LEN_TO_MM;
            }
        }
        NativeSupportChart::Cone { axial_scale } => {
            for point in &mut pcurve.control_points {
                let native = *point;
                point.u = native.v;
                point.v = native.u * axial_scale;
            }
        }
    }
}

/// Convert a pcurve stored in a standalone analytic surface's native chart to
/// the neutral chart used by [`SurfaceGeometry`].
pub(crate) fn normalize_pcurve_for_surface_record(
    surface_head: &str,
    surface_tokens: &[Token],
    pcurve: &mut NurbsPcurve,
) {
    let chart = match surface_head {
        "plane" => NativeSupportChart::PlaneLengths,
        "cone" => {
            let values = surface_tokens
                .iter()
                .filter_map(|token| match token {
                    Token::Double(value) => Some(*value),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let Some((&sine, &cosine, &u_scale)) = values
                .get(1)
                .zip(values.get(2))
                .zip(values.get(3))
                .map(|((sine, cosine), u_scale)| (sine, cosine, u_scale))
            else {
                return;
            };
            NativeSupportChart::Cone {
                axial_scale: cone_axial_scale(sine, cosine, u_scale),
            }
        }
        _ => return,
    };
    normalize_support_pcurve(chart, pcurve);
}

fn required_support_pair(cur: &mut Cur<'_>) -> Option<([SurfaceGeometry; 2], [NurbsPcurve; 2])> {
    let first_surface_start = cur.pos();
    let first_surface = embedded_surface(cur)?;
    let second_surface_start = cur.pos();
    let second_surface = embedded_surface(cur)?;
    let (mut first_pcurve, first_end) = pcurve_block_with_end(cur.toks(), cur.pos())?;
    cur.set_pos(first_end);
    let (mut second_pcurve, second_end) = pcurve_block_with_end(cur.toks(), cur.pos())?;
    cur.set_pos(second_end);
    normalize_support_pcurve(
        native_support_chart(cur.toks(), first_surface_start),
        &mut first_pcurve,
    );
    normalize_support_pcurve(
        native_support_chart(cur.toks(), second_surface_start),
        &mut second_pcurve,
    );
    Some((
        [first_surface, second_surface],
        [first_pcurve, second_pcurve],
    ))
}

/// Three ordered support carriers and selector of an `sss_int_cur`.
pub struct EmbeddedThreeSurfaceIntersection {
    /// Three ordered embedded support surfaces.
    pub surfaces: [SurfaceGeometry; 3],
    /// Three ordered embedded NURBS parameter curves.
    pub pcurves: [NurbsPcurve; 3],
    /// Shared native parameter interval.
    pub parameter_range: [f64; 2],
    /// Three discontinuity arrays.
    pub discontinuities: [Vec<f64>; 3],
    /// The integer selector serialized after the discontinuity arrays.
    pub selector: i64,
}

/// Embedded support context, source curve, and tail of a `proj_int_cur`.
pub struct EmbeddedProjection {
    /// Two ordered embedded support surfaces.
    pub surfaces: [SurfaceGeometry; 2],
    /// Two ordered embedded NURBS parameter curves.
    pub pcurves: [NurbsPcurve; 2],
    /// Shared native parameter interval.
    pub parameter_range: [f64; 2],
    /// Three discontinuity arrays.
    pub discontinuities: [Vec<f64>; 3],
    /// The boolean serialized after the discontinuity arrays.
    pub discontinuity_flag: bool,
    /// The embedded projected source curve.
    pub source: NurbsCurve,
    /// Neutral tail fields decoded after the source curve.
    pub tail: cadmpeg_ir::geometry::ProjectionTail,
}

/// Shared context and tail fields of a silhouette intcurve.
pub struct EmbeddedSilhouette {
    /// Shared embedded support context.
    pub context: EmbeddedIntersection,
    /// The silhouette family the subtype name selects.
    pub silhouette: cadmpeg_ir::geometry::SilhouetteKind,
    /// The embedded surface the silhouette is cast on.
    pub cast_surface: SurfaceGeometry,
    /// The projection direction of the silhouette light.
    pub light_direction: Vector3,
}

/// Shared context and tail fields of an `off_surf_int_cur`.
pub struct EmbeddedSurfaceOffset {
    /// Shared embedded support context.
    pub context: EmbeddedIntersection,
    /// The boolean serialized after the discontinuity arrays.
    pub discontinuity_flag: bool,
    /// U parameter interval of the base surface.
    pub base_u_range: [f64; 2],
    /// V parameter interval of the base surface.
    pub base_v_range: [f64; 2],
    /// The embedded base curve the offset follows.
    pub base: NurbsCurve,
    /// Native parameter interval of the base curve.
    pub base_range: [f64; 2],
    /// Optional endpoint bounds of the base curve.
    pub base_endpoints: [Option<f64>; 2],
    /// Layout form when the cache precedes the construction.
    pub cache_first: Option<cadmpeg_ir::geometry::CacheFirstCurveForm>,
    /// Signed offset distance in document length units.
    pub distance: f64,
    /// The shift value serialized after the distance.
    pub shift: f64,
    /// The scale value serialized after the shift.
    pub scale: f64,
}

/// One context-first spring support slot.
pub enum EmbeddedSpringSupport {
    /// Embedded support surface.
    Surface(SurfaceGeometry),
    /// U/V ranges stored in place of a null support.
    Ranges([[f64; 2]; 2]),
}

/// First context-first spring pcurve slot.
pub enum EmbeddedSpringPcurve {
    /// Embedded pcurve.
    Pcurve(NurbsPcurve),
    /// Parameter range stored in place of a null pcurve.
    Range([f64; 2]),
}

/// Structurally selected spring layout.
pub enum EmbeddedSpringLayout {
    /// Context-first form with inline replacement ranges.
    ContextFirst {
        /// Ordered support slots.
        supports: [EmbeddedSpringSupport; 2],
        /// First pcurve slot.
        first_pcurve: EmbeddedSpringPcurve,
        /// Nullable second pcurve slot.
        second_pcurve: Option<NurbsPcurve>,
        /// Shared parameter interval.
        parameter_range: [f64; 2],
        /// Three discontinuity arrays.
        discontinuities: [Vec<f64>; 3],
        /// Boolean following the discontinuity arrays.
        discontinuity_flag: bool,
    },
    /// Cache-first form with no inline replacement ranges.
    CacheFirst {
        /// Shared embedded support context.
        context: EmbeddedIntersection,
        /// Cache-first serializer fields.
        form: cadmpeg_ir::geometry::CacheFirstCurveForm,
    },
}

/// Spring construction and direction enum.
pub struct EmbeddedSpring {
    /// Context-first or cache-first payload.
    pub layout: EmbeddedSpringLayout,
    /// The direction enum serialized at the construction tail.
    pub direction: i64,
}

/// Embedded support context and recursive formulas of a `law_int_cur`.
pub struct EmbeddedLawCurve {
    /// Shared embedded support context.
    pub context: EmbeddedIntersection,
    /// Version-stamped serializer form; `None` for the legacy layout.
    pub version: Option<EmbeddedLawVersion>,
    /// The extension enum serialized before the primary formula.
    pub extension: i64,
    /// The law formula that drives the curve.
    pub primary: EmbeddedLawFormula,
    /// Additional law formulas serialized after the primary.
    pub additional: Vec<EmbeddedLawFormula>,
}

/// Version stamp, trailing enum, and unbounded parameter interval of the
/// stamped `law_int_cur` serializer form.
pub struct EmbeddedLawVersion {
    /// The serializer version stamp.
    pub stamp: i64,
    /// The enum serialized after the version stamp.
    pub post_enum: i64,
    /// Optional parameter bounds; `None` marks an unbounded end.
    pub parameter_range: [Option<f64>; 2],
}

/// Mode-discriminated payload of a `defm_int_cur` construction.
pub enum EmbeddedDeformableData {
    /// The vector-field payload: four frame vectors and a parameter-pair list.
    VectorField {
        /// Four ordered frame vectors.
        vectors: [Vector3; 4],
        /// Counted list of parameter pairs.
        parameter_pairs: Vec<[f64; 2]>,
    },
    /// The mode-3 payload: leading frame, trailing frame, and scalar tail.
    Mode3 {
        /// Four ordered leading frame vectors.
        leading_vectors: [Vector3; 4],
        /// The parameter serialized after the leading vectors.
        leading_parameter: f64,
        /// Three booleans serialized after the leading parameter.
        leading_flags: [bool; 3],
        /// The point that anchors the trailing frame.
        trailing_point: Point3,
        /// Two ordered trailing frame vectors.
        trailing_vectors: [Vector3; 2],
        /// The parameter serialized after the trailing vectors.
        frame_parameter: f64,
        /// Two booleans serialized after the frame parameter.
        frame_flags: [bool; 2],
        /// Three scalar parameters of the tail.
        parameters: [f64; 3],
        /// Five booleans serialized after the tail parameters.
        trailing_flags: [bool; 5],
        /// The parameter serialized after the trailing flags.
        trailing_parameter: f64,
        /// The integer serialized at the payload end.
        trailing_value: i64,
    },
}

/// Embedded bend curve and discriminator payload of a `defm_int_cur`.
pub struct EmbeddedDeformable {
    /// Layout form of the cache-first construction.
    pub form: cadmpeg_ir::geometry::CacheFirstCurveForm,
    /// Two ordered embedded support surfaces.
    pub surfaces: [Option<SurfaceGeometry>; 2],
    /// Two ordered embedded NURBS parameter curves.
    pub pcurves: [Option<NurbsPcurve>; 2],
    /// Shared native parameter interval.
    pub parameter_range: [f64; 2],
    /// Three discontinuity arrays.
    pub discontinuities: [Vec<f64>; 3],
    /// The source the deformation bends.
    pub source: EmbeddedDeformableSource,
    /// Optional parameter bounds of the source; `None` marks an unbounded end.
    pub source_parameter_range: [Option<f64>; 2],
    /// The mode-discriminated payload.
    pub data: EmbeddedDeformableData,
}

/// The source a `defm_int_cur` deformation bends.
pub enum EmbeddedDeformableSource {
    /// An embedded source curve.
    Curve(NurbsCurve),
    /// A flag and index referencing a curve serialized elsewhere in the stream.
    NativeReference {
        /// The boolean serialized before the index.
        flag: bool,
        /// The native reference index.
        index: i64,
    },
}

/// A procedural curve cache together with its native subtype and fit contract.
pub struct DecodedProceduralCurve {
    /// The cached B-spline curve (control points scaled centimetre→
    /// millimetre; knots and weights unscaled).
    pub curve: NurbsCurve,
    /// The `intcurve` subtype record name (`exact_int_cur`, `off_int_cur`,
    /// `proj_int_cur`, `int_int_cur`, `helix_int_cur`, `sss_int_cur`, ...).
    pub native_kind: String,
    /// Neutral construction fields decoded from the subtype tail.
    pub definition: Option<cadmpeg_ir::geometry::ProceduralCurveDefinition>,
    /// Source curve and tail fields of an `offset_int_cur` construction.
    pub vector_offset: Option<VectorOffsetDefinition>,
    /// Parent curve and retained range of a `subset_int_cur` construction.
    pub subset: Option<SubsetDefinition>,
    /// Parameter arrays and ordered child curves of a `comp_int_cur` construction.
    pub compound: Option<CompoundDefinition>,
    /// Non-null embedded NURBS support carriers of an `off_int_cur`.
    pub embedded_two_sided_offset: Option<EmbeddedTwoSidedOffset>,
    /// Embedded support context of an `int_int_cur`.
    pub embedded_intersection: Option<(EmbeddedIntersection, bool)>,
    /// Three embedded support pairs of an `sss_int_cur`.
    pub embedded_three_surface_intersection: Option<EmbeddedThreeSurfaceIntersection>,
    /// Prefix-only surface-curve family and support context.
    pub embedded_surface_curve: Option<EmbeddedSurfaceCurve>,
    /// Embedded silhouette support, cast surface, and light vector.
    pub embedded_silhouette: Option<EmbeddedSilhouette>,
    /// Embedded support context and base curve of an `off_surf_int_cur`.
    pub embedded_surface_offset: Option<EmbeddedSurfaceOffset>,
    /// Modern non-null `spring_int_cur` construction.
    pub embedded_spring: Option<EmbeddedSpring>,
    /// Embedded bend curve and discriminator payload of a `defm_int_cur`.
    pub embedded_deformable: Option<EmbeddedDeformable>,
    /// Embedded support context and source of a `proj_int_cur`.
    pub embedded_projection: Option<EmbeddedProjection>,
    /// Embedded support context and recursive formulas of a `law_int_cur`.
    pub embedded_law: Option<EmbeddedLawCurve>,
    /// `surface_fit_tolerance` of the cached B-spline block, if present
    /// ([spec §6.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#65-nubsnurbs-blocks-b-spline-curves-and-surfaces)).
    pub cache_fit_tolerance: Option<f64>,
}

/// Decode a procedural 3D curve cache while following subtype-table references.
pub fn procedural_curve_resolving_refs(
    toks: &[Token],
    table: &SubtypeTable,
) -> Option<DecodedProceduralCurve> {
    procedural_curve_recursive(toks, table, &mut Vec::new())
}

/// Resolve the signed pcurve slot carried by an `intcurve` reference.
///
/// A typed construction owns its pcurves alongside the corresponding support
/// surfaces. A plain generated `intcurve` has one direct UV block, which is
/// the second slot in the ref-form grammar. The selector's sign is handled by
/// the owning PCURVE decoder because it composes with the intcurve sense bit.
pub fn pcurve_for_selector_resolving_refs(
    toks: &[Token],
    selector: i64,
    table: &SubtypeTable,
) -> Option<NurbsPcurve> {
    pcurve_for_selector_with_chart(toks, selector, table).map(|(pcurve, _)| pcurve)
}

/// Decode a selector pcurve and report whether it remains in the standalone
/// face surface's native parameter chart.
pub(crate) fn pcurve_for_selector_with_chart(
    toks: &[Token],
    selector: i64,
    table: &SubtypeTable,
) -> Option<(NurbsPcurve, bool)> {
    let slot = match selector {
        1 | -1 => 0,
        2 | -2 => 1,
        _ => return None,
    };
    pcurve_for_selector_recursive(toks, slot, table, &mut Vec::new())
}

fn pcurve_for_selector_recursive(
    toks: &[Token],
    slot: usize,
    table: &SubtypeTable,
    seen: &mut Vec<usize>,
) -> Option<(NurbsPcurve, bool)> {
    // A record-level intcurve wrapper can carry only a compact `{ref N}`
    // scope. Follow that one construction reference before decoding the
    // wrapper. Typed constructions may contain support references, but their
    // own ordered slots remain authoritative and must not be searched here.
    if let Some(index) = direct_subtype_reference(toks) {
        if !seen.contains(&index) {
            seen.push(index);
            if let Some(target) = table.span(index) {
                if let Some(result) = pcurve_for_selector_recursive(target, slot, table, seen) {
                    return Some(result);
                }
            }
        }
    }
    let has_typed_construction = crate::nurbs::toks::owned_construction_subtype(toks).is_some();
    if let Some(decoded) = procedural_curve_resolving_refs(toks, table) {
        if let Some(pcurve) = selected_pcurve(&decoded, slot) {
            return Some((pcurve, false));
        }
        if decoded.native_kind != "intcurve" {
            // Modern exact curves can carry the same cache-first support
            // context as the surface-related intcurve families. Their
            // construction remains exact, but the pcurve selector still
            // names one of that context's ordered support slots. Parse the
            // context only from the exact-intcurve marker; do not search
            // arbitrary BS2/BS3 blocks in the record.
            if let Some(marker) =
                crate::nurbs::toks::find_owned_intcurve_subtype(toks, "exact_int_cur")
            {
                let mut cur = Cur::at(toks, marker + 2);
                if let Some(context) = cache_first_curve_context(&mut cur, &decoded.curve, table) {
                    if let Some(pcurve) =
                        selected_optional_pcurve(&context.surfaces, &context.pcurves, slot)
                    {
                        return Some((pcurve, false));
                    }
                }
            }
            return None;
        }
    }
    if has_typed_construction {
        return None;
    }
    (slot == 1)
        .then(|| direct_pcurve_after_curve(toks))?
        .map(|pcurve| (pcurve, true))
}

/// Return the sole record-level subtype-table reference of an untyped wrapper.
///
/// A nested reference inside a typed construction is deliberately excluded:
/// its owner has a structural pcurve slot, while an untyped wrapper has no
/// carrier of its own and delegates the complete construction.
fn direct_subtype_reference(toks: &[Token]) -> Option<usize> {
    if crate::nurbs::toks::owned_construction_subtype(toks).is_some() {
        return None;
    }
    let mut depth = 0usize;
    let mut candidate = None;
    for (position, token) in toks.iter().enumerate() {
        match token {
            Token::SubtypeOpen => {
                if depth == 0 {
                    let scope = crate::nurbs::toks::subtype_span(toks, position)?;
                    let index = match scope {
                        [Token::SubtypeOpen, Token::Long(index), Token::SubtypeClose]
                            if *index >= 0 =>
                        {
                            usize::try_from(*index).ok()
                        }
                        [Token::SubtypeOpen, Token::Ident(name), Token::Long(index), Token::SubtypeClose]
                            if name == "ref" && *index >= 0 =>
                        {
                            usize::try_from(*index).ok()
                        }
                        _ => None,
                    }?;
                    if candidate.replace(index).is_some() {
                        return None;
                    }
                }
                depth += 1;
            }
            Token::SubtypeClose => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    candidate
}

fn selected_optional_pcurve(
    surfaces: &[Option<SurfaceGeometry>; 2],
    pcurves: &[Option<NurbsPcurve>; 2],
    slot: usize,
) -> Option<NurbsPcurve> {
    surfaces.get(slot)?.as_ref()?;
    pcurves.get(slot)?.clone()
}

fn selected_pcurve(decoded: &DecodedProceduralCurve, slot: usize) -> Option<NurbsPcurve> {
    if let Some(context) = decoded.embedded_two_sided_offset.as_ref() {
        return selected_optional_pcurve(&context.surfaces, &context.pcurves, slot);
    }
    if let Some((context, _)) = decoded.embedded_intersection.as_ref() {
        // An intersection support may be a procedural surface with no
        // standalone SurfaceGeometry carrier. The native pcurve slot remains
        // a complete parameter-space carrier when its paired native support
        // slot is present. Edge-domain validation runs after this function.
        context
            .support_present
            .get(slot)
            .copied()
            .filter(|present| *present)?;
        return context.pcurves.get(slot)?.clone();
    }
    if let Some(context) = decoded.embedded_three_surface_intersection.as_ref() {
        return context.pcurves.get(slot).cloned();
    }
    if let Some(surface_curve) = decoded.embedded_surface_curve.as_ref() {
        let context = surface_curve.context();
        return selected_optional_pcurve(&context.surfaces, &context.pcurves, slot);
    }
    if let Some(context) = decoded.embedded_silhouette.as_ref() {
        return selected_optional_pcurve(&context.context.surfaces, &context.context.pcurves, slot);
    }
    if let Some(context) = decoded.embedded_surface_offset.as_ref() {
        return selected_optional_pcurve(&context.context.surfaces, &context.context.pcurves, slot);
    }
    if let Some(context) = decoded.embedded_spring.as_ref() {
        return match &context.layout {
            EmbeddedSpringLayout::CacheFirst { context, .. } => {
                selected_optional_pcurve(&context.surfaces, &context.pcurves, slot)
            }
            EmbeddedSpringLayout::ContextFirst {
                supports,
                first_pcurve,
                second_pcurve,
                ..
            } => matches!(supports.get(slot), Some(EmbeddedSpringSupport::Surface(_)))
                .then(|| match slot {
                    0 => match first_pcurve {
                        EmbeddedSpringPcurve::Pcurve(pcurve) => Some(pcurve.clone()),
                        EmbeddedSpringPcurve::Range(_) => None,
                    },
                    1 => second_pcurve.clone(),
                    _ => None,
                })
                .flatten(),
        };
    }
    if let Some(context) = decoded.embedded_deformable.as_ref() {
        return selected_optional_pcurve(&context.surfaces, &context.pcurves, slot);
    }
    if let Some(context) = decoded.embedded_projection.as_ref() {
        return context.pcurves.get(slot).cloned();
    }
    if let Some(context) = decoded.embedded_law.as_ref() {
        return selected_optional_pcurve(&context.context.surfaces, &context.context.pcurves, slot);
    }
    None
}

fn direct_pcurve_after_curve(toks: &[Token]) -> Option<NurbsPcurve> {
    let position = crate::nurbs::toks::owned_marker_positions(toks)
        .into_iter()
        .next()?;
    let (_, end) = curve_block(toks, position)?;
    pcurve_block_with_end(toks, end).map(|(pcurve, _)| pcurve)
}

/// Decode an exact procedural curve construction that has no solved cache.
pub fn cacheless_procedural_curve_resolving_refs(
    toks: &[Token],
    table: &SubtypeTable,
) -> Option<(String, cadmpeg_ir::geometry::ProceduralCurveDefinition)> {
    cacheless_procedural_curve_recursive(toks, table, &mut Vec::new())
}

fn cacheless_procedural_curve_recursive(
    toks: &[Token],
    table: &SubtypeTable,
    seen: &mut Vec<usize>,
) -> Option<(String, cadmpeg_ir::geometry::ProceduralCurveDefinition)> {
    if let Some(definition) = helix_definition(toks) {
        return Some(("helix_int_cur".into(), definition));
    }
    for index in crate::nurbs::toks::subtype_refs(toks) {
        if seen.contains(&index) {
            continue;
        }
        seen.push(index);
        let target = table.span(index)?;
        if let Some(decoded) = cacheless_procedural_curve_recursive(target, table, seen) {
            return Some(decoded);
        }
    }
    None
}

fn procedural_curve_recursive(
    toks: &[Token],
    table: &SubtypeTable,
    seen: &mut Vec<usize>,
) -> Option<DecodedProceduralCurve> {
    let vector_offset = vector_offset_definition(toks);
    let subset = subset_definition(toks);
    let compound = compound_definition(toks);
    // Wrapper constructions serialize their source curves before the record's
    // own cache, so the cache is the last decodable curve block. Every other
    // intcurve opens with its cache — the first block, followed by the fit
    // tolerance; later blocks belong to nested construction machinery
    // (support surfaces, blend spines, progenitors) and are not the carrier.
    let cache_scope = crate::nurbs::toks::owned_cache_scope(toks).unwrap_or(toks);
    let positions = crate::nurbs::toks::owned_marker_positions(cache_scope);
    let solved = if vector_offset.is_some() || subset.is_some() || compound.is_some() {
        positions
            .into_iter()
            .rev()
            .find_map(|position| curve_block(cache_scope, position))
    } else {
        positions
            .into_iter()
            .find_map(|position| curve_block(cache_scope, position))
    };
    if let Some((curve, end)) = solved {
        let cache_fit_tolerance = match cache_scope.get(end) {
            Some(Token::Double(value)) => Some(*value * LEN_TO_MM),
            _ => None,
        };
        let native_kind = crate::nurbs::toks::owned_construction_subtype(toks)
            .unwrap_or_else(|| "intcurve".to_string());
        let definition = if native_kind == "exact_int_cur" {
            Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact)
        } else {
            helix_definition(toks).or_else(|| two_sided_offset(toks))
        };
        let embedded_intersection = embedded_intersection(toks, &curve, table);
        let embedded_surface_curve = embedded_surface_curve(toks, &curve, table);
        let embedded_surface_offset = embedded_surface_offset(toks, &curve, table);
        let embedded_spring = embedded_spring(toks, &curve, table);
        let embedded_deformable = embedded_deformable(toks, &curve, table);
        return Some(DecodedProceduralCurve {
            curve,
            native_kind,
            definition,
            vector_offset,
            subset,
            compound,
            embedded_two_sided_offset: embedded_two_sided_offset(toks),
            embedded_intersection,
            embedded_three_surface_intersection: embedded_three_surface_intersection(toks),
            embedded_surface_curve,
            embedded_silhouette: embedded_silhouette(toks),
            embedded_surface_offset,
            embedded_spring,
            embedded_deformable,
            embedded_projection: embedded_projection(toks),
            embedded_law: embedded_law_curve(toks),
            cache_fit_tolerance,
        });
    }
    for index in crate::nurbs::toks::subtype_refs(toks) {
        if seen.contains(&index) {
            continue;
        }
        seen.push(index);
        let target = table.span(index)?;
        if let Some(decoded) = procedural_curve_recursive(target, table, seen) {
            return Some(decoded);
        }
    }
    None
}

fn embedded_deformable(
    toks: &[Token],
    solved: &NurbsCurve,
    table: &SubtypeTable,
) -> Option<EmbeddedDeformable> {
    let marker = crate::nurbs::toks::find_owned_subtype_marker(toks, &["defm_int_cur"])
        .map(|(marker, _)| marker)?;
    let mut cur = Cur::at(toks, marker + 2);
    let context = cache_first_curve_context(&mut cur, solved, table)?;
    let source_start = cur.pos();
    let source = if let Some(curve) = embedded_base_curve_resolving_refs(&mut cur, table) {
        EmbeddedDeformableSource::Curve(curve)
    } else {
        cur.set_pos(source_start);
        (cur.take_ident()? == "intcurve").then_some(())?;
        let flag = cur.take_bool()?;
        let reference = cur.pos();
        matches!(toks.get(reference), Some(Token::SubtypeOpen)).then_some(())?;
        matches!(toks.get(reference + 1), Some(Token::Ident(name)) if name == "ref")
            .then_some(())?;
        let Some(Token::Long(index)) = toks.get(reference + 2) else {
            return None;
        };
        let reference_span = crate::nurbs::toks::subtype_span(toks, reference)?;
        cur.set_pos(reference + reference_span.len());
        EmbeddedDeformableSource::NativeReference {
            flag,
            index: *index,
        }
    };
    let source_parameter_range = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    let mode = cur.take_long()?;
    let data = match mode {
        8 => {
            let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut vectors {
                let value = cur.take_vector3()?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            let count = cur.take_long()?;
            let count = usize::try_from(count).ok()?;
            let mut parameter_pairs = Vec::with_capacity(count);
            for _ in 0..count {
                parameter_pairs.push([cur.take_f64()?, cur.take_f64()?]);
            }
            EmbeddedDeformableData::VectorField {
                vectors,
                parameter_pairs,
            }
        }
        3 => {
            let mut leading_vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut leading_vectors {
                let value = cur.take_vector3()?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            let leading_parameter = cur.take_f64()?;
            let leading_flags = [cur.take_bool()?, cur.take_bool()?, cur.take_bool()?];
            let value = cur.take_position()?;
            let trailing_point = Point3::new(
                value[0] * LEN_TO_MM,
                value[1] * LEN_TO_MM,
                value[2] * LEN_TO_MM,
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
            EmbeddedDeformableData::Mode3 {
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
            }
        }
        _ => return None,
    };
    matches!(toks.get(cur.pos()), Some(Token::SubtypeClose)).then_some(EmbeddedDeformable {
        form: context.form,
        surfaces: context.surfaces,
        pcurves: context.pcurves,
        parameter_range: context.parameter_range,
        discontinuities: context.discontinuities,
        source,
        source_parameter_range,
        data,
    })
}

/// Decode one law support surface, mapping the `null_surface` sentinel to an
/// absent side.
#[allow(clippy::option_option)] // Outer None is parse failure; inner None is a null carrier.
fn nullable_law_surface(cur: &mut Cur<'_>) -> Option<Option<SurfaceGeometry>> {
    let saved = cur.pos();
    if cur.take_ident() == Some("null_surface") {
        return Some(None);
    }
    cur.set_pos(saved);
    Some(Some(embedded_surface(cur)?))
}

/// Consume one version-form interval bound: a bare `false` unbounded sentinel
/// or a `true`-prefixed double. Any other encoding fails the strict match so
/// the record falls back to verbatim retention.
#[allow(clippy::option_option)] // Outer None is parse failure; inner None is an unbounded bound.
fn law_version_bound(cur: &mut Cur<'_>) -> Option<Option<f64>> {
    match cur.peek()? {
        Token::False => {
            cur.bump();
            Some(None)
        }
        Token::True => {
            cur.bump();
            cur.take_f64().map(Some)
        }
        _ => None,
    }
}

fn embedded_law_curve(toks: &[Token]) -> Option<EmbeddedLawCurve> {
    let marker = crate::nurbs::toks::find_owned_intcurve_subtype(toks, "law_int_cur")?;
    let mut cur = Cur::at(toks, marker + 2);
    // The stamped serializer form opens with an integer stamp and an enum
    // before the solved cache; the legacy form opens directly with the cache
    // marker.
    let stamp_start = cur.pos();
    let stamp = matches!(cur.peek(), Some(Token::Long(_)))
        .then(|| {
            let stamp = cur.take_long()?;
            let post_enum = cur.take_enum()?;
            Some((stamp, post_enum))
        })
        .flatten();
    if stamp.is_none() {
        // Restore any tokens consumed by a partially-matched stamp prefix so
        // the legacy path (and, on its failure, verbatim retention) sees the
        // record from the cache marker rather than mid-prefix.
        cur.set_pos(stamp_start);
    }
    let (solved, solved_end) = curve_block(toks, cur.pos())?;
    cur.set_pos(solved_end);
    cur.take_f64()?;
    let first_surface_start = cur.pos();
    let first_surface = nullable_law_surface(&mut cur)?;
    let second_surface_start = cur.pos();
    let second_surface = nullable_law_surface(&mut cur)?;
    let surfaces = [first_surface, second_surface];
    let mut pcurves = [
        nullable_embedded_pcurve(&mut cur)?,
        nullable_embedded_pcurve(&mut cur)?,
    ];
    for (pcurve, chart) in pcurves.iter_mut().zip([
        native_support_chart(toks, first_surface_start),
        native_support_chart(toks, second_surface_start),
    ]) {
        if let Some(pcurve) = pcurve {
            normalize_support_pcurve(chart, pcurve);
        }
    }
    let (parameter_range, version) = if let Some((stamp, post_enum)) = stamp {
        let bounds = [law_version_bound(&mut cur)?, law_version_bound(&mut cur)?];
        let domain = nurbs_curve_parameter_domain(&solved).unwrap_or([0.0, 0.0]);
        let parameter_range = [
            bounds[0].unwrap_or(domain[0]),
            bounds[1].unwrap_or(domain[1]),
        ];
        (
            parameter_range,
            Some(EmbeddedLawVersion {
                stamp,
                post_enum,
                parameter_range: bounds,
            }),
        )
    } else {
        let parameter_range = [cur.take_range_value()?, cur.take_range_value()?];
        (parameter_range, None)
    };
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let extension = cur.take_long()?;
    let primary = law_formula(&mut cur)?;
    let count = usize::try_from(cur.take_long()?).ok()?;
    if count > 100_000 {
        return None;
    }
    let additional = (0..count)
        .map(|_| law_formula(&mut cur))
        .collect::<Option<Vec<_>>>()?;
    let support_present = [surfaces[0].is_some(), surfaces[1].is_some()];
    Some(EmbeddedLawCurve {
        context: EmbeddedIntersection {
            surfaces,
            support_present,
            pcurves,
            parameter_range,
            discontinuities,
        },
        version,
        extension,
        primary,
        additional,
    })
}

fn embedded_spring(
    toks: &[Token],
    solved: &NurbsCurve,
    table: &SubtypeTable,
) -> Option<EmbeddedSpring> {
    let marker = crate::nurbs::toks::find_owned_intcurve_subtype(toks, "spring_int_cur")?;
    let mut cur = Cur::at(toks, marker + 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        let context = cache_first_curve_context(&mut cur, solved, table)?;
        let direction = cur.take_enum()?;
        return Some(EmbeddedSpring {
            layout: EmbeddedSpringLayout::CacheFirst {
                context: EmbeddedIntersection {
                    support_present: [context.surfaces[0].is_some(), context.surfaces[1].is_some()],
                    surfaces: context.surfaces,
                    pcurves: context.pcurves,
                    parameter_range: context.parameter_range,
                    discontinuities: context.discontinuities,
                },
                form: context.form,
            },
            direction,
        });
    }
    let mut supports = Vec::with_capacity(2);
    let mut surface_charts = [NativeSupportChart::Canonical; 2];
    for side in 0..2 {
        let saved = cur.pos();
        if cur.take_ident() == Some("null_surface") {
            supports.push(EmbeddedSpringSupport::Ranges([
                [cur.take_range_value()?, cur.take_range_value()?],
                [cur.take_range_value()?, cur.take_range_value()?],
            ]));
        } else {
            cur.set_pos(saved);
            surface_charts[side] = native_support_chart(toks, cur.pos());
            supports.push(EmbeddedSpringSupport::Surface(embedded_surface(&mut cur)?));
        }
    }
    let saved = cur.pos();
    let mut first_pcurve = if cur.take_ident() == Some("nullbs") {
        EmbeddedSpringPcurve::Range([cur.take_range_value()?, cur.take_range_value()?])
    } else {
        cur.set_pos(saved);
        let (pcurve, end) = pcurve_block_with_end(toks, cur.pos())?;
        cur.set_pos(end);
        EmbeddedSpringPcurve::Pcurve(pcurve)
    };
    let saved = cur.pos();
    let second_pcurve = if cur.take_ident() == Some("nullbs") {
        None
    } else {
        cur.set_pos(saved);
        let (pcurve, end) = pcurve_block_with_end(toks, cur.pos())?;
        cur.set_pos(end);
        Some(pcurve)
    };
    if let EmbeddedSpringPcurve::Pcurve(pcurve) = &mut first_pcurve {
        normalize_support_pcurve(surface_charts[0], pcurve);
    }
    let mut second_pcurve = second_pcurve;
    if let Some(pcurve) = &mut second_pcurve {
        normalize_support_pcurve(surface_charts[1], pcurve);
    }
    let parameter_range = [cur.take_range_value()?, cur.take_range_value()?];
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
    let direction = cur.take_enum()?;
    Some(EmbeddedSpring {
        layout: EmbeddedSpringLayout::ContextFirst {
            supports: supports.try_into().ok()?,
            first_pcurve,
            second_pcurve,
            parameter_range,
            discontinuities,
            discontinuity_flag,
        },
        direction,
    })
}

/// Writable fields in the shared context tail of a `spring_int_cur` subtype.
pub struct SpringPatchLayout {
    /// Byte offsets of the two parameter-range doubles.
    pub parameter_range: [usize; 2],
    /// Byte offsets of the values in each discontinuity array.
    pub discontinuities: [Vec<usize>; 3],
    /// Byte offset of the boolean after the discontinuity arrays.
    pub discontinuity_flag: usize,
    /// Byte offset of the direction enum.
    pub direction: usize,
}

/// Locate spring context fields by walking the subtype grammar at `int_width`.
pub fn spring_patch_layout(bytes: &[u8], int_width: usize) -> Option<SpringPatchLayout> {
    let (marker, name_len) = find_owned_intcurve_subtype(bytes, b"spring_int_cur", int_width)?;
    let mut position = marker + name_len + 3;
    for _ in 0..2 {
        let saved = position;
        if take_native_ident(bytes, &mut position).as_deref() == Some("null_surface") {
            for _ in 0..4 {
                take_double_payload(bytes, &mut position)?;
            }
        } else {
            position = saved;
            decode_embedded_surface(bytes, &mut position, int_width)?;
        }
    }
    let saved = position;
    if take_native_ident(bytes, &mut position).as_deref() == Some("nullbs") {
        take_double_payload(bytes, &mut position)?;
        take_double_payload(bytes, &mut position)?;
    } else {
        position = decode_pcurve_block_with_end(bytes, saved, int_width)?.1;
    }
    let saved = position;
    if take_native_ident(bytes, &mut position).as_deref() != Some("nullbs") {
        position = decode_pcurve_block_with_end(bytes, saved, int_width)?.1;
    }
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = position;
    take_bool(bytes, &mut position)?;
    let direction = position;
    take_tagged_int(bytes, &mut position, 0x15, int_width)?;
    Some(SpringPatchLayout {
        parameter_range,
        discontinuities,
        discontinuity_flag,
        direction,
    })
}

/// Writable radius-law payloads in a rolling-ball blend surface subtype.
pub struct RollingBallPatchLayout {
    /// Byte offsets of the two radius-law doubles.
    pub radii: [usize; 2],
}

/// Writable leading fields in a translational-extrusion surface subtype.
pub struct ExtrusionPatchLayout {
    /// Byte offsets of the two parameter-interval doubles.
    pub parameter_interval: [usize; 2],
    /// Byte offset of the extrusion direction vector.
    pub direction: usize,
    /// Byte offset of the native position triple.
    pub native_position: usize,
}

/// Writable construction fields in a `helix_int_cur` subtype.
pub struct HelixPatchLayout {
    /// Byte offsets of the two angle-range doubles.
    pub angle_range: [usize; 2],
    /// Byte offsets of the four frame vectors.
    pub frame_vectors: [usize; 4],
    /// Byte offset of the apex factor double.
    pub apex_factor: usize,
    /// Byte offset of the axis vector.
    pub axis: usize,
}

/// Writable fields following the source cache in an `offset_int_cur` subtype.
pub struct VectorOffsetPatchLayout {
    /// Byte offsets of the two parameter-range doubles.
    pub parameter_range: [usize; 2],
    /// Byte offset of the offset vector.
    pub offset: usize,
}

/// Writable parameter range following the parent curve in `subset_int_cur`.
pub struct SubsetPatchLayout {
    /// Byte offsets of the two parameter-range doubles.
    pub parameter_range: [usize; 2],
}

/// Writable parameter arrays in a `comp_int_cur` subtype.
pub struct CompoundPatchLayout {
    /// Byte offsets of the values in the parameter array.
    pub parameters: Vec<usize>,
    /// Byte offsets of the values in the component-parameter array.
    pub component_parameters: Vec<usize>,
}

/// Locate both compound parameter arrays from their native counts.
pub fn compound_patch_layout(bytes: &[u8], int_width: usize) -> Option<CompoundPatchLayout> {
    let name = b"comp_int_cur";
    let marker = find_owned_subtype_marker(bytes, &[name], int_width).map(|(marker, _)| marker)?;
    subtype_span(bytes, marker, int_width)?;
    let mut position = marker + name.len() + 3;
    let parameters = take_float_array_payloads(bytes, &mut position, int_width)?;
    let component_count =
        usize::try_from(take_tagged_int(bytes, &mut position, 0x04, int_width)?).ok()?;
    if component_count == 0 {
        return None;
    }
    let mut component_parameters = Vec::with_capacity(component_count);
    for _ in 0..component_count {
        component_parameters.push(take_double_payload(bytes, &mut position)?);
    }
    Some(CompoundPatchLayout {
        parameters,
        component_parameters,
    })
}

/// Locate the subset range by consuming the subtype-owned parent curve.
pub fn subset_patch_layout(bytes: &[u8], int_width: usize) -> Option<SubsetPatchLayout> {
    let name = b"subset_int_cur";
    let marker = find_owned_subtype_marker(bytes, &[name], int_width).map(|(marker, _)| marker)?;
    subtype_span(bytes, marker, int_width)?;
    let mut position = marker + name.len() + 3;
    position = decode_curve_block(bytes, position, int_width)?.end;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    Some(SubsetPatchLayout { parameter_range })
}

/// Locate vector-offset fields by consuming the wrapper flag and source curve.
pub fn vector_offset_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<VectorOffsetPatchLayout> {
    let name = b"offset_int_cur";
    let marker = find_owned_subtype_marker(bytes, &[name], int_width).map(|(marker, _)| marker)?;
    subtype_span(bytes, marker, int_width)?;
    let mut position = marker + name.len() + 3;
    take_bool(bytes, &mut position)?;
    position = decode_curve_block(bytes, position, int_width)?.end;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let offset = position + 1;
    take_native_vec3(bytes, &mut position, 0x14)?;
    Some(VectorOffsetPatchLayout {
        parameter_range,
        offset,
    })
}

/// Locate helix fields by consuming the subtype prefix grammar.
pub fn helix_patch_layout(bytes: &[u8], int_width: usize) -> Option<HelixPatchLayout> {
    let name = b"helix_int_cur";
    let marker = find_owned_subtype_marker(bytes, &[name], int_width).map(|(marker, _)| marker)?;
    subtype_span(bytes, marker, int_width)?;
    let mut position = marker + name.len() + 3;
    let current_layout = take_optional_helix_revision(bytes, &mut position, int_width)?;
    let take_range_payload = |position: &mut usize| {
        if matches!(bytes.get(*position), Some(0x0a | 0x0b)) {
            *position += 1;
        }
        take_double_payload(bytes, position)
    };
    let angle_range = [
        take_range_payload(&mut position)?,
        take_range_payload(&mut position)?,
    ];
    let mut frame_vectors = [0usize; 4];
    let frame_tags = if current_layout {
        [0x13, 0x14, 0x14, 0x14]
    } else {
        [0x13; 4]
    };
    for (offset, tag) in frame_vectors.iter_mut().zip(frame_tags) {
        *offset = position + 1;
        take_native_vec3(bytes, &mut position, tag)?;
    }
    let apex_factor = take_double_payload(bytes, &mut position)?;
    let axis = position + 1;
    take_native_vec3(bytes, &mut position, 0x14)?;
    Some(HelixPatchLayout {
        angle_range,
        frame_vectors,
        apex_factor,
        axis,
    })
}

/// Locate extrusion fields from the `cyl_spl_sur` subtype header.
pub fn extrusion_patch_layout(bytes: &[u8], int_width: usize) -> Option<ExtrusionPatchLayout> {
    let names: [&[u8]; 2] = [b"cyl_spl_sur", b"cylsur"];
    let (start, name_len) = find_owned_subtype_marker(bytes, &names, int_width)
        .map(|(start, name)| (start, name.len()))?;
    subtype_span(bytes, start, int_width)?;
    let mut position = start + name_len + 3;
    let parameter_interval = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let direction = position + 1;
    take_native_vec3(bytes, &mut position, 0x14)?;
    let native_position = position + 1;
    take_native_vec3(bytes, &mut position, 0x13)?;
    Some(ExtrusionPatchLayout {
        parameter_interval,
        direction,
        native_position,
    })
}

/// Locate the rolling-ball radius pair by walking both supports and the slice curve.
pub fn rolling_ball_patch_layout(bytes: &[u8], int_width: usize) -> Option<RollingBallPatchLayout> {
    let names: [&[u8]; 6] = [
        b"rb_blend_spl_sur",
        b"rbblnsur",
        b"pipe_spl_sur",
        b"pipesur",
        b"sss_blend_spl_sur",
        b"sssblndsur",
    ];
    let (start, name_len) = find_owned_subtype_marker(bytes, &names, int_width)
        .map(|(start, name)| (start, name.len()))?;
    let span = subtype_span(bytes, start, int_width)?;
    let payload_start = name_len + 3;
    let radii = (|| {
        let mut position = payload_start;
        take_tagged_int(span, &mut position, 0x04, int_width)?;
        decode_rolling_ball_side(span, &mut position, int_width, None)?;
        decode_rolling_ball_side(span, &mut position, int_width, None)?;
        position = decode_curve_block(span, position, int_width)?.end;
        Some([
            start + take_double_payload(span, &mut position)?,
            start + take_double_payload(span, &mut position)?,
        ])
    })()
    .or_else(|| {
        let mut position = payload_start;
        for _ in 0..2 {
            take_native_string(span, &mut position, int_width)?;
            let support_kind = take_native_ident(span, &mut position)?;
            if !matches!(support_kind.as_str(), "plane" | "sphere" | "cone" | "torus") {
                return None;
            }
            position = decode_surface_block(span, position, int_width)?.end;
        }
        position = decode_curve_block(span, position, int_width)?.end;
        Some([
            start + take_double_payload(span, &mut position)?,
            start + take_double_payload(span, &mut position)?,
        ])
    })?;
    Some(RollingBallPatchLayout { radii })
}

/// Embedded cache-first base curve: a direct NURBS block, an analytic
/// `straight`, or a referenced `intcurve` resolved to its solved cache.
pub(crate) fn embedded_base_curve_resolving_refs(
    cur: &mut Cur<'_>,
    table: &SubtypeTable,
) -> Option<NurbsCurve> {
    let toks = cur.toks();
    if let Some((curve, end)) = curve_block(toks, cur.pos()) {
        cur.set_pos(end);
        return Some(curve);
    }
    let saved = cur.pos();
    // Some revision-gated owners omit the redundant `intcurve` identifier and
    // store only its sense followed by the compact subtype-table reference.
    if matches!(cur.peek(), Some(Token::True | Token::False)) {
        cur.take_bool()?;
        let reference = cur.pos();
        if matches!(toks.get(reference), Some(Token::SubtypeOpen)) {
            let scope = crate::nurbs::toks::subtype_span(toks, reference)?;
            if let Some(curve) = owned_curve_cache_resolving_refs(scope, table) {
                cur.set_pos(reference + scope.len());
                return Some(curve);
            }
        }
        cur.set_pos(saved);
    }
    match cur.take_ident()? {
        "straight" => {
            let origin = cur.take_position()?;
            let direction = cur.take_vector3()?;
            let start = Point3::new(
                origin[0] * LEN_TO_MM,
                origin[1] * LEN_TO_MM,
                origin[2] * LEN_TO_MM,
            );
            let end = Point3::new(
                (origin[0] + direction[0]) * LEN_TO_MM,
                (origin[1] + direction[1]) * LEN_TO_MM,
                (origin[2] + direction[2]) * LEN_TO_MM,
            );
            Some(NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![start, end],
                weights: None,
                periodic: false,
            })
        }
        "ellipse" => {
            let center = cur.take_position()?;
            let normal = cur.take_vector3()?;
            let major = cur.take_vector3()?;
            let ratio = cur.take_f64()?;
            ellipse_to_nurbs(center, normal, major, ratio)
        }
        "degenerate_curve" => {
            let point = cur.take_position()?;
            let at = Point3::new(
                point[0] * LEN_TO_MM,
                point[1] * LEN_TO_MM,
                point[2] * LEN_TO_MM,
            );
            Some(NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![at, at],
                weights: None,
                periodic: false,
            })
        }
        "intcurve" => {
            cur.take_bool()?;
            let reference = cur.pos();
            let compact_ref = matches!(toks.get(reference), Some(Token::SubtypeOpen))
                && matches!(toks.get(reference + 1), Some(Token::Ident(name)) if name == "ref")
                && matches!(toks.get(reference + 2), Some(Token::Long(_)));
            if !compact_ref {
                if matches!(toks.get(reference), Some(Token::SubtypeOpen)) {
                    // Inline subtype scope: resolve its solved curve cache.
                    let scope = crate::nurbs::toks::subtype_span(toks, reference)?;
                    let curve = owned_curve_cache_resolving_refs(scope, table)?;
                    cur.set_pos(reference + scope.len());
                    return Some(curve);
                }
                cur.set_pos(saved);
                return None;
            }
            let Some(Token::Long(index)) = toks.get(reference + 2) else {
                return None;
            };
            let index = usize::try_from(*index).ok()?;
            let reference_span = crate::nurbs::toks::subtype_span(toks, reference)?;
            cur.set_pos(reference + reference_span.len());
            table
                .span(index)
                .and_then(|target| owned_curve_cache_resolving_refs(target, table))
        }
        _ => {
            cur.set_pos(saved);
            None
        }
    }
}

fn embedded_surface_offset(
    toks: &[Token],
    solved: &NurbsCurve,
    table: &SubtypeTable,
) -> Option<EmbeddedSurfaceOffset> {
    let marker = crate::nurbs::toks::find_owned_intcurve_subtype(toks, "off_surf_int_cur")?;
    let mut cur = Cur::at(toks, marker + 2);
    if matches!(cur.peek(), Some(Token::Long(_))) {
        let context = cache_first_curve_context(&mut cur, solved, table)?;
        let base_u_range = [
            cur.take_optional_range_value()??,
            cur.take_optional_range_value()??,
        ];
        let base_v_range = [
            cur.take_optional_range_value()??,
            cur.take_optional_range_value()??,
        ];
        let base = embedded_base_curve_resolving_refs(&mut cur, table)?;
        let base_endpoints = [
            cur.take_optional_range_value()?,
            cur.take_optional_range_value()?,
        ];
        let base_range = [
            cur.take_optional_range_value()??,
            cur.take_optional_range_value()??,
        ];
        return Some(EmbeddedSurfaceOffset {
            context: EmbeddedIntersection {
                surfaces: context.surfaces,
                support_present: context.support_present,
                pcurves: context.pcurves,
                parameter_range: context.parameter_range,
                discontinuities: context.discontinuities,
            },
            discontinuity_flag: false,
            base_u_range,
            base_v_range,
            base,
            base_range,
            base_endpoints,
            cache_first: Some(context.form),
            distance: cur.take_f64()? * LEN_TO_MM,
            shift: cur.take_f64()?,
            scale: cur.take_f64()?,
        });
    }
    let (surfaces, pcurves) = required_support_pair(&mut cur)?;
    let parameter_range = [cur.take_range_value()?, cur.take_range_value()?];
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
    let base_u_range = [cur.take_range_value()?, cur.take_range_value()?];
    let base_v_range = [cur.take_range_value()?, cur.take_range_value()?];
    let (base, base_end) = curve_block(toks, cur.pos())?;
    cur.set_pos(base_end);
    let base_range = [cur.take_range_value()?, cur.take_range_value()?];
    Some(EmbeddedSurfaceOffset {
        context: EmbeddedIntersection {
            surfaces: surfaces.map(Some),
            support_present: [true, true],
            pcurves: pcurves.map(Some),
            parameter_range,
            discontinuities,
        },
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base,
        base_range,
        base_endpoints: [None, None],
        cache_first: None,
        distance: cur.take_f64()? * LEN_TO_MM,
        shift: cur.take_f64()?,
        scale: cur.take_f64()?,
    })
}

/// Writable scalar fields in an `off_surf_int_cur` subtype.
pub struct SurfaceOffsetPatchLayout {
    /// Byte offsets of the two parameter-range doubles.
    pub parameter_range: [usize; 2],
    /// Byte offsets of the values in each discontinuity array.
    pub discontinuities: [Vec<usize>; 3],
    /// Byte offset of the boolean after the discontinuity arrays.
    pub discontinuity_flag: usize,
    /// Byte offsets of the two base-surface U-range doubles.
    pub base_u_range: [usize; 2],
    /// Byte offsets of the two base-surface V-range doubles.
    pub base_v_range: [usize; 2],
    /// Byte offsets of the two base-curve range doubles.
    pub base_range: [usize; 2],
    /// Byte offset of the offset-distance double.
    pub distance: usize,
    /// Byte offset of the shift double.
    pub shift: usize,
    /// Byte offset of the scale double.
    pub scale: usize,
}

/// Locate surface-offset fields by walking supports and the base curve.
pub fn surface_offset_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<SurfaceOffsetPatchLayout> {
    let (marker, name_len) = find_owned_intcurve_subtype(bytes, b"off_surf_int_cur", int_width)?;
    let mut position = marker + name_len + 3;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = position;
    take_bool(bytes, &mut position)?;
    let base_u_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let base_v_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    position = decode_curve_block(bytes, position, int_width)?.end;
    let base_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let distance = take_double_payload(bytes, &mut position)?;
    let shift = take_double_payload(bytes, &mut position)?;
    let scale = take_double_payload(bytes, &mut position)?;
    Some(SurfaceOffsetPatchLayout {
        parameter_range,
        discontinuities,
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base_range,
        distance,
        shift,
        scale,
    })
}

fn embedded_silhouette(toks: &[Token]) -> Option<EmbeddedSilhouette> {
    use cadmpeg_ir::geometry::SilhouetteKind;
    let names = [
        ("silh_int_cur", SilhouetteKind::Standard),
        ("para_silh_int_cur", SilhouetteKind::Parametric),
        ("parasil", SilhouetteKind::Parametric),
        (
            "taper_silh_int_cur",
            SilhouetteKind::Taper { draft_factor: 0.0 },
        ),
    ];
    let candidates: Vec<&str> = names.iter().map(|(name, _)| *name).collect();
    let (marker, name) = crate::nurbs::toks::find_owned_subtype_marker(toks, &candidates)?;
    let mut silhouette = names
        .iter()
        .find_map(|(candidate, silhouette)| (*candidate == name).then(|| silhouette.clone()))?;
    let mut cur = Cur::at(toks, marker + 2);
    let (surfaces, pcurves) = required_support_pair(&mut cur)?;
    let parameter_range = [cur.take_range_value()?, cur.take_range_value()?];
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let cast_surface = embedded_surface(&mut cur)?;
    let light = cur.take_vector3()?;
    let light_direction = normalized(light)?;
    if matches!(silhouette, SilhouetteKind::Taper { .. }) {
        silhouette = SilhouetteKind::Taper {
            draft_factor: cur.take_f64()?,
        };
    }
    Some(EmbeddedSilhouette {
        context: EmbeddedIntersection {
            surfaces: surfaces.map(Some),
            support_present: [true, true],
            pcurves: pcurves.map(Some),
            parameter_range,
            discontinuities,
        },
        silhouette,
        cast_surface,
        light_direction,
    })
}

/// Writable light and optional taper fields in a silhouette subtype.
pub struct SilhouettePatchLayout {
    /// Byte offset of the light-direction vector.
    pub light_direction: usize,
    /// Byte offset of the draft-factor double, for tapered silhouettes.
    pub draft_factor: Option<usize>,
}

/// Locate silhouette fields by walking its context and cast surface.
pub fn silhouette_patch_layout(
    bytes: &[u8],
    int_width: usize,
    silhouette: &cadmpeg_ir::geometry::SilhouetteKind,
) -> Option<SilhouettePatchLayout> {
    use cadmpeg_ir::geometry::SilhouetteKind;
    let (names, tapered): (&[&[u8]], bool) = match silhouette {
        SilhouetteKind::Standard => (&[b"silh_int_cur"], false),
        SilhouetteKind::Parametric => (&[b"para_silh_int_cur", b"parasil"], false),
        SilhouetteKind::Taper { .. } => (&[b"taper_silh_int_cur"], true),
    };
    let (marker, name) = find_owned_subtype_marker(bytes, names, int_width)?;
    let mut position = marker + name.len() + 3;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    take_double_payload(bytes, &mut position)?;
    take_double_payload(bytes, &mut position)?;
    for _ in 0..3 {
        take_float_array_payloads(bytes, &mut position, int_width)?;
    }
    decode_embedded_surface(bytes, &mut position, int_width)?;
    (*bytes.get(position)? == 0x14).then_some(())?;
    let light_direction = position + 1;
    bytes.get(light_direction..light_direction + 24)?;
    position = light_direction + 24;
    let draft_factor = if tapered {
        Some(take_double_payload(bytes, &mut position)?)
    } else {
        None
    };
    Some(SilhouettePatchLayout {
        light_direction,
        draft_factor,
    })
}

fn embedded_surface_curve(
    toks: &[Token],
    solved: &NurbsCurve,
    table: &SubtypeTable,
) -> Option<EmbeddedSurfaceCurve> {
    use cadmpeg_ir::geometry::SurfaceCurveFamilyKind;
    let names = [
        ("blend_int_cur", SurfaceCurveFamilyKind::Blend),
        ("bldcur", SurfaceCurveFamilyKind::Blend),
        ("surf_int_cur", SurfaceCurveFamilyKind::SurfaceConstrained),
        ("surfcur", SurfaceCurveFamilyKind::SurfaceConstrained),
        ("par_int_cur", SurfaceCurveFamilyKind::Parametric),
        ("parcur", SurfaceCurveFamilyKind::Parametric),
        ("skin_int_cur", SurfaceCurveFamilyKind::Skin),
        ("d5c2_cur", SurfaceCurveFamilyKind::Skin),
    ];
    let candidates: Vec<&str> = names.iter().map(|(name, _)| *name).collect();
    let (marker, name) = crate::nurbs::toks::find_owned_subtype_marker(toks, &candidates)?;
    let family = names
        .iter()
        .find_map(|(candidate, family)| (*candidate == name).then_some(*family))?;
    let position = marker + 2;
    context_first_surface_curve(toks, position, family)
        .or_else(|| cache_first_surface_curve(toks, position, family, solved, table))
}

/// Decode a form-2 `par_int_cur` scope into the curve it denotes.
///
/// Form 2 replaces the solved cache and its fit tolerance with a bool-gated
/// curve interval and a closed-form enum; the members from the supports onward
/// are the shared cache-first context. The construction is therefore the
/// occupied support surface restricted to the parameter curve in the matching
/// slot.
///
/// Only a pcurve that holds one surface parameter constant across the support's
/// whole domain in the other is decoded: that restriction is exactly a NURBS
/// curve of the support's degree over the support's knot vector. Any other
/// pcurve denotes a curve a NURBS cache can only approximate, so it is refused.
pub fn decode_par_int_cur_isoline(
    scope: &[u8],
    int_width: usize,
    reference_context: Option<(&[u8], &SubtypeTables)>,
) -> Option<NurbsCurve> {
    let names: [&[u8]; 2] = [b"par_int_cur", b"parcur"];
    let (start, name) = find_owned_subtype_marker(scope, &names, int_width)?;
    let mut position = start + name.len() + 3;
    (take_tagged_int(scope, &mut position, 0x04, int_width)? > 0).then_some(())?;
    (take_tagged_int(scope, &mut position, 0x15, int_width)? == 2).then_some(())?;
    take_range_value(scope, &mut position)?;
    take_range_value(scope, &mut position)?;
    take_tagged_int(scope, &mut position, 0x15, int_width)?;
    let supports = [
        decode_optional_rolling_ball_surface(scope, &mut position, int_width, reference_context)?.0,
        decode_optional_rolling_ball_surface(scope, &mut position, int_width, reference_context)?.0,
    ];
    let pcurves = [
        decode_nullable_embedded_pcurve(scope, &mut position, int_width)?,
        decode_nullable_embedded_pcurve(scope, &mut position, int_width)?,
    ];
    // The support-slot selector puts the parametric support and its parameter
    // curve in the same slot and nulls the other; a support without its pcurve,
    // or two occupied slots, is not this construction.
    let occupied: Vec<usize> = (0..2)
        .filter(|slot| supports[*slot].is_some() || pcurves[*slot].is_some())
        .collect();
    let [slot] = occupied.as_slice() else {
        return None;
    };
    let (Some(SurfaceGeometry::Nurbs(support)), Some(pcurve)) = (&supports[*slot], &pcurves[*slot])
    else {
        return None;
    };
    surface_isoline_along(support, pcurve)
}

/// Decode a form-2 `par_int_cur` scope into the curve it denotes. Token-space
/// counterpart of [`decode_par_int_cur_isoline`].
pub(crate) fn par_int_cur_isoline(
    scope: &[Token],
    reference_context: Option<&SubtypeTable>,
) -> Option<NurbsCurve> {
    let (start, _) =
        crate::nurbs::toks::find_owned_subtype_marker(scope, &["par_int_cur", "parcur"])?;
    let mut cur = Cur::at(scope, start + 2);
    (cur.take_long()? > 0).then_some(())?;
    (cur.take_enum()? == 2).then_some(())?;
    cur.take_range_value()?;
    cur.take_range_value()?;
    cur.take_enum()?;
    let supports = [
        optional_rolling_ball_surface(&mut cur, reference_context)?.0,
        optional_rolling_ball_surface(&mut cur, reference_context)?.0,
    ];
    let pcurves = [
        nullable_embedded_pcurve(&mut cur)?,
        nullable_embedded_pcurve(&mut cur)?,
    ];
    // The support-slot selector puts the parametric support and its parameter
    // curve in the same slot and nulls the other; a support without its pcurve,
    // or two occupied slots, is not this construction.
    let occupied: Vec<usize> = (0..2)
        .filter(|slot| supports[*slot].is_some() || pcurves[*slot].is_some())
        .collect();
    let [slot] = occupied.as_slice() else {
        return None;
    };
    let (Some(SurfaceGeometry::Nurbs(support)), Some(pcurve)) = (&supports[*slot], &pcurves[*slot])
    else {
        return None;
    };
    surface_isoline_along(support, pcurve)
}

/// The support isoline a uv pcurve selects, or `None` when the pcurve is not an
/// isoline of the support's full domain.
fn surface_isoline_along(
    support: &cadmpeg_ir::geometry::NurbsSurface,
    pcurve: &NurbsPcurve,
) -> Option<NurbsCurve> {
    use cadmpeg_ir::eval::IsolineDirection;
    (pcurve.degree == 1 && pcurve.control_points.len() == 2 && pcurve.weights.is_none())
        .then_some(())?;
    let start = *pcurve.control_points.first()?;
    let end = *pcurve.control_points.last()?;
    let domain = [*pcurve.knots.first()?, *pcurve.knots.last()?];
    let u_domain = [*support.u_knots.first()?, *support.u_knots.last()?];
    let v_domain = [*support.v_knots.first()?, *support.v_knots.last()?];
    let (direction, at, free_domain, travel) = if agree(start.u, end.u, width(u_domain)) {
        (
            IsolineDirection::ConstantU,
            start.u,
            v_domain,
            [start.v, end.v],
        )
    } else if agree(start.v, end.v, width(v_domain)) {
        (
            IsolineDirection::ConstantV,
            start.v,
            u_domain,
            [start.u, end.u],
        )
    } else {
        return None;
    };
    // The pcurve's free coordinate must be the support parameter itself, and
    // must run the support's whole domain: anything shorter is a trim, which the
    // support's own knot vector cannot express.
    let scale = width(free_domain);
    (agree(travel[0], domain[0], scale)
        && agree(travel[1], domain[1], scale)
        && agree(free_domain[0], domain[0], scale)
        && agree(free_domain[1], domain[1], scale))
    .then_some(())?;
    cadmpeg_ir::eval::nurbs_surface_isoline(support, direction, at)
}

/// Span of a parameter domain.
fn width(domain: [f64; 2]) -> f64 {
    (domain[1] - domain[0]).abs()
}

/// Two parameters name the same value at `scale`'s representable precision.
fn agree(left: f64, right: f64, scale: f64) -> bool {
    (left - right).abs() <= EPS_PARAMETER_AGREEMENT * scale
}

/// Shared cache-first intcurve context: revision, enum zero, solved cache and
/// fit tolerance, two bounded supports, two nullable pcurves, two optional
/// solved-interval endpoints, three discontinuity arrays, and one extension.
struct CacheFirstCurveContext {
    form: cadmpeg_ir::geometry::CacheFirstCurveForm,
    surfaces: [Option<SurfaceGeometry>; 2],
    support_present: [bool; 2],
    pcurves: [Option<NurbsPcurve>; 2],
    parameter_range: [f64; 2],
    discontinuities: [Vec<f64>; 3],
}

fn cache_first_curve_context(
    cur: &mut Cur<'_>,
    solved: &NurbsCurve,
    table: &SubtypeTable,
) -> Option<CacheFirstCurveContext> {
    let revision = cur.take_long()?;
    (revision > 0).then_some(())?;
    // The leading enum selects the approximation-cache form. `0` stores the
    // solved curve cache and its fit tolerance; `2` stores neither and instead
    // stores a bool-gated curve interval and a closed-form enum. No other value
    // has a defined grammar, so it fails and the containing record is retained
    // verbatim.
    let cache_enum = cur.take_enum()?;
    let cache = match cache_enum {
        0 => {
            let (_, end) = curve_block(cur.toks(), cur.pos())?;
            cur.set_pos(end);
            cadmpeg_ir::geometry::RevisionCacheForm::SolvedCache {
                fit_tolerance: cur.take_f64()? * LEN_TO_MM,
            }
        }
        2 => cadmpeg_ir::geometry::RevisionCacheForm::Parameterization(
            cadmpeg_ir::geometry::CacheFirstCurveParameterization {
                interval: [
                    cur.take_optional_range_value()?,
                    cur.take_optional_range_value()?,
                ],
                closed_form: cur.take_enum()?,
            },
        ),
        _ => return None,
    };
    let first_surface_start = cur.pos();
    let first_support_present = support_slot_present(cur, table);
    let (first_surface, first_bounds) = optional_embedded_surface_with_bounds(cur, table)?;
    let second_surface_start = cur.pos();
    let second_support_present = support_slot_present(cur, table);
    let (second_surface, second_bounds) = optional_embedded_surface_with_bounds(cur, table)?;
    let mut pcurves = [
        nullable_embedded_pcurve(cur)?,
        nullable_embedded_pcurve(cur)?,
    ];
    if let Some(pcurve) = &mut pcurves[0] {
        normalize_support_pcurve(
            native_support_chart(cur.toks(), first_surface_start),
            pcurve,
        );
    }
    if let Some(pcurve) = &mut pcurves[1] {
        normalize_support_pcurve(
            native_support_chart(cur.toks(), second_surface_start),
            pcurve,
        );
    }
    let solved_range = [
        cur.take_optional_range_value()?,
        cur.take_optional_range_value()?,
    ];
    let domain = nurbs_curve_parameter_domain(solved)?;
    let parameter_range = [
        solved_range[0].unwrap_or(domain[0]),
        solved_range[1].unwrap_or(domain[1]),
    ];
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let extension = cur.take_long()?;
    Some(CacheFirstCurveContext {
        form: cadmpeg_ir::geometry::CacheFirstCurveForm {
            revision,
            cache,
            support_bounds: [first_bounds, second_bounds],
            solved_range,
            extension,
        },
        surfaces: [first_surface, second_surface],
        support_present: [first_support_present, second_support_present],
        pcurves,
        parameter_range,
        discontinuities,
    })
}

fn context_first_surface_curve(
    toks: &[Token],
    position: usize,
    family: cadmpeg_ir::geometry::SurfaceCurveFamilyKind,
) -> Option<EmbeddedSurfaceCurve> {
    let mut cur = Cur::at(toks, position);
    let (surfaces, pcurves) = required_support_pair(&mut cur)?;
    let parameter_range = [cur.take_range_value()?, cur.take_range_value()?];
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    embedded_surface_curve_from_parts(
        family,
        EmbeddedIntersection {
            surfaces: surfaces.map(Some),
            support_present: [true, true],
            pcurves: pcurves.map(Some),
            parameter_range,
            discontinuities,
        },
        None,
    )
}

fn cache_first_surface_curve(
    toks: &[Token],
    position: usize,
    family: cadmpeg_ir::geometry::SurfaceCurveFamilyKind,
    solved: &NurbsCurve,
    table: &SubtypeTable,
) -> Option<EmbeddedSurfaceCurve> {
    let mut cur = Cur::at(toks, position);
    let context = cache_first_curve_context(&mut cur, solved, table)?;
    let flag = cur.take_bool()?;
    let second_flag = matches!(cur.peek(), Some(Token::True | Token::False))
        .then(|| cur.take_bool())
        .flatten();
    let tail = cadmpeg_ir::geometry::SurfaceCurveTail {
        extension: context.form.extension,
        revision: context.form.revision,
        cache: context.form.cache,
        support_bounds: context.form.support_bounds,
        solved_range: context.form.solved_range,
    };
    embedded_surface_curve_from_parts(
        family,
        EmbeddedIntersection {
            surfaces: context.surfaces,
            support_present: context.support_present,
            pcurves: context.pcurves,
            parameter_range: context.parameter_range,
            discontinuities: context.discontinuities,
        },
        Some((tail, flag, second_flag)),
    )
}

fn embedded_surface_curve_from_parts(
    family: cadmpeg_ir::geometry::SurfaceCurveFamilyKind,
    context: EmbeddedIntersection,
    tail: Option<(cadmpeg_ir::geometry::SurfaceCurveTail, bool, Option<bool>)>,
) -> Option<EmbeddedSurfaceCurve> {
    use cadmpeg_ir::geometry::{
        ParametricSurfaceCurveFlags, SurfaceCurveCacheFirst, SurfaceCurveFamilyKind,
    };
    let single_tail = |tail| match tail {
        None => Some(None),
        Some((tail, flag, None)) => Some(Some(SurfaceCurveCacheFirst { tail, flags: flag })),
        Some((_, _, Some(_))) => None,
    };
    match family {
        SurfaceCurveFamilyKind::Blend => Some(EmbeddedSurfaceCurve::Blend {
            context,
            tail: single_tail(tail)?,
        }),
        SurfaceCurveFamilyKind::SurfaceConstrained => {
            Some(EmbeddedSurfaceCurve::SurfaceConstrained {
                context,
                tail: single_tail(tail)?,
            })
        }
        SurfaceCurveFamilyKind::Parametric => Some(EmbeddedSurfaceCurve::Parametric {
            context,
            tail: tail.map(|(tail, flag, second_flag)| SurfaceCurveCacheFirst {
                tail,
                flags: ParametricSurfaceCurveFlags { flag, second_flag },
            }),
        }),
        SurfaceCurveFamilyKind::Skin => Some(EmbeddedSurfaceCurve::Skin {
            context,
            tail: single_tail(tail)?,
        }),
    }
}

/// Writable shared-context fields in a surface-related `intcurve` subtype.
pub struct SurfaceCurvePatchLayout {
    /// Byte offsets of the two parameter-range doubles.
    pub parameter_range: [usize; 2],
    /// Byte offsets of the values in each discontinuity array.
    pub discontinuities: [Vec<usize>; 3],
}

/// Locate a surface-curve context by walking its two ordered support pairs.
pub fn surface_curve_patch_layout(
    bytes: &[u8],
    int_width: usize,
    family: cadmpeg_ir::geometry::SurfaceCurveFamilyKind,
) -> Option<SurfaceCurvePatchLayout> {
    use cadmpeg_ir::geometry::SurfaceCurveFamilyKind;
    let names: &[&[u8]] = match family {
        SurfaceCurveFamilyKind::Blend => &[b"blend_int_cur", b"bldcur"],
        SurfaceCurveFamilyKind::SurfaceConstrained => &[b"surf_int_cur", b"surfcur"],
        SurfaceCurveFamilyKind::Parametric => &[b"par_int_cur", b"parcur"],
        SurfaceCurveFamilyKind::Skin => &[b"skin_int_cur", b"d5c2_cur"],
    };
    let (marker, name) = find_owned_subtype_marker(bytes, names, int_width)?;
    let mut position = marker + name.len() + 3;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    Some(SurfaceCurvePatchLayout {
        parameter_range,
        discontinuities,
    })
}

fn embedded_three_surface_intersection(toks: &[Token]) -> Option<EmbeddedThreeSurfaceIntersection> {
    let marker = crate::nurbs::toks::find_owned_subtype_marker(toks, &["sss_int_cur"])
        .map(|(marker, _)| marker)?;
    let mut cur = Cur::at(toks, marker + 2);
    let ([first, second], [first_pcurve, second_pcurve]) = required_support_pair(&mut cur)?;
    let parameter_range = [cur.take_range_value()?, cur.take_range_value()?];
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let selector = cur.take_long()?;
    let third_surface_start = cur.pos();
    let third = embedded_surface(&mut cur)?;
    let (mut third_pcurve, _) = pcurve_block_with_end(toks, cur.pos())?;
    normalize_support_pcurve(
        native_support_chart(toks, third_surface_start),
        &mut third_pcurve,
    );
    Some(EmbeddedThreeSurfaceIntersection {
        surfaces: [first, second, third],
        pcurves: [first_pcurve, second_pcurve, third_pcurve],
        parameter_range,
        discontinuities,
        selector,
    })
}

/// Writable context fields in an `sss_int_cur` subtype.
pub struct ThreeSurfacePatchLayout {
    /// Byte offsets of the two parameter-range doubles.
    pub parameter_range: [usize; 2],
    /// Byte offsets of the values in each discontinuity array.
    pub discontinuities: [Vec<usize>; 3],
    /// Byte offset of the selector integer.
    pub selector: usize,
}

/// Locate three-surface intersection fields by walking all three support pairs.
pub fn three_surface_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<ThreeSurfacePatchLayout> {
    let (marker, name_len) = find_owned_intcurve_subtype(bytes, b"sss_int_cur", int_width)?;
    let mut position = marker + name_len + 3;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    let selector = position;
    take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_pcurve_block_with_end(bytes, position, int_width)?;
    Some(ThreeSurfacePatchLayout {
        parameter_range,
        discontinuities,
        selector,
    })
}

fn embedded_projection(toks: &[Token]) -> Option<EmbeddedProjection> {
    let marker = crate::nurbs::toks::find_owned_intcurve_subtype(toks, "proj_int_cur")?;
    let mut cur = Cur::at(toks, marker + 2);
    let (surfaces, pcurves) = required_support_pair(&mut cur)?;
    let parameter_range = [cur.take_range_value()?, cur.take_range_value()?];
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
    let (source, source_end) = curve_block(toks, cur.pos())?;
    cur.set_pos(source_end);
    let flag = cur.take_bool()?;
    let tail = if matches!(cur.peek(), Some(Token::SubtypeClose)) {
        cadmpeg_ir::geometry::ProjectionTail::EarlyClose { flag }
    } else {
        cadmpeg_ir::geometry::ProjectionTail::Ranged {
            flag,
            parameter_range: [cur.take_range_value()?, cur.take_range_value()?],
            role: cur.take_str()?.to_string(),
        }
    };
    Some(EmbeddedProjection {
        surfaces,
        pcurves,
        parameter_range,
        discontinuities,
        discontinuity_flag,
        source,
        tail,
    })
}

/// Writable tail shape of a `proj_int_cur` subtype.
pub enum ProjectionTailPatchLayout {
    /// The tail closes directly after the flag.
    EarlyClose {
        /// Byte offset of the tail flag boolean.
        flag: usize,
    },
    /// The tail carries a parameter range and a role identifier.
    Ranged {
        /// Byte offset of the tail flag boolean.
        flag: usize,
        /// Byte offsets of the two tail parameter-range doubles.
        parameter_range: [usize; 2],
        /// Byte range of the role identifier payload.
        role: std::ops::Range<usize>,
    },
}

/// Writable shared-context and tail fields in a `proj_int_cur` subtype.
pub struct ProjectionPatchLayout {
    /// Byte offsets of the two parameter-range doubles.
    pub parameter_range: [usize; 2],
    /// Byte offsets of the values in each discontinuity array.
    pub discontinuities: [Vec<usize>; 3],
    /// Byte offset of the boolean after the discontinuity arrays.
    pub discontinuity_flag: usize,
    /// Writable tail shape.
    pub tail: ProjectionTailPatchLayout,
}

/// Locate projection fields by walking supports, source curve, and selected tail.
pub fn projection_patch_layout(bytes: &[u8], int_width: usize) -> Option<ProjectionPatchLayout> {
    let (marker, name_len) = find_owned_intcurve_subtype(bytes, b"proj_int_cur", int_width)?;
    let mut position = marker + name_len + 3;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = position;
    take_bool(bytes, &mut position)?;
    position = decode_curve_block(bytes, position, int_width)?.end;
    let tail_flag = position;
    take_bool(bytes, &mut position)?;
    let tail = if bytes.get(position) == Some(&0x10) {
        ProjectionTailPatchLayout::EarlyClose { flag: tail_flag }
    } else {
        let parameter_range = [
            take_double_payload(bytes, &mut position)?,
            take_double_payload(bytes, &mut position)?,
        ];
        (*bytes.get(position)? == 0x07).then_some(())?;
        let length = usize::from(*bytes.get(position + 1)?);
        let role = position + 2..position + 2 + length;
        bytes.get(role.clone())?;
        ProjectionTailPatchLayout::Ranged {
            flag: tail_flag,
            parameter_range,
            role,
        }
    };
    Some(ProjectionPatchLayout {
        parameter_range,
        discontinuities,
        discontinuity_flag,
        tail,
    })
}

fn embedded_intersection(
    toks: &[Token],
    solved: &NurbsCurve,
    table: &SubtypeTable,
) -> Option<(EmbeddedIntersection, bool)> {
    let names = ["int_int_cur", "surf_surf_int_cur", "surfintcur"];
    let (marker, _) = crate::nurbs::toks::find_owned_subtype_marker(toks, &names)?;
    let position = marker + 2;
    context_first_intersection(toks, position)
        .or_else(|| cache_first_intersection(toks, position, solved, table))
}

fn context_first_intersection(
    toks: &[Token],
    position: usize,
) -> Option<(EmbeddedIntersection, bool)> {
    let mut cur = Cur::at(toks, position);
    let (surfaces, pcurves) = required_support_pair(&mut cur)?;
    let parameter_range = [cur.take_range_value()?, cur.take_range_value()?];
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
    Some((
        EmbeddedIntersection {
            surfaces: surfaces.map(Some),
            support_present: [true, true],
            pcurves: pcurves.map(Some),
            parameter_range,
            discontinuities,
        },
        discontinuity_flag,
    ))
}

fn cache_first_intersection(
    toks: &[Token],
    position: usize,
    solved: &NurbsCurve,
    table: &SubtypeTable,
) -> Option<(EmbeddedIntersection, bool)> {
    let mut cur = Cur::at(toks, position);
    (cur.take_long()? > 0).then_some(())?;
    (cur.take_enum()? == 0).then_some(())?;
    let (_, cache_end) = curve_block(toks, cur.pos())?;
    cur.set_pos(cache_end);
    cur.take_f64()?;
    let first_surface_start = cur.pos();
    let first_support_present = support_slot_present(&cur, table);
    let first_surface = optional_embedded_surface_resolving_ref(&mut cur, table)?;
    let second_surface_start = cur.pos();
    let second_support_present = support_slot_present(&cur, table);
    let second_surface = optional_embedded_surface_resolving_ref(&mut cur, table)?;
    let surfaces = [first_surface, second_surface];
    let support_present = [first_support_present, second_support_present];
    let mut pcurves = [
        nullable_embedded_pcurve(&mut cur)?,
        nullable_embedded_pcurve(&mut cur)?,
    ];
    if let Some(pcurve) = &mut pcurves[0] {
        normalize_support_pcurve(native_support_chart(toks, first_surface_start), pcurve);
    }
    if let Some(pcurve) = &mut pcurves[1] {
        normalize_support_pcurve(native_support_chart(toks, second_surface_start), pcurve);
    }
    let domain = nurbs_curve_parameter_domain(solved)?;
    let parameter_range = [
        cur.take_optional_range_value()?.unwrap_or(domain[0]),
        cur.take_optional_range_value()?.unwrap_or(domain[1]),
    ];
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_long()? != 0;
    Some((
        EmbeddedIntersection {
            surfaces,
            support_present,
            pcurves,
            parameter_range,
            discontinuities,
        },
        discontinuity_flag,
    ))
}

/// Writable shared-context fields in an `int_int_cur` subtype.
pub struct IntersectionPatchLayout {
    /// Byte offsets of the two parameter-range doubles.
    pub parameter_range: [usize; 2],
    /// Byte offsets of the values in each discontinuity array.
    pub discontinuities: [Vec<usize>; 3],
    /// Byte offset of the boolean after the discontinuity arrays.
    pub discontinuity_flag: usize,
}

/// Locate an intersection context by walking both ordered support pairs.
pub fn intersection_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<IntersectionPatchLayout> {
    let names: [&[u8]; 3] = [b"int_int_cur", b"surf_surf_int_cur", b"surfintcur"];
    let (marker, name) = find_owned_subtype_marker(bytes, &names, int_width)?;
    let mut position = marker + name.len() + 3;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    decode_embedded_surface(bytes, &mut position, int_width)?;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    position = decode_pcurve_block_with_end(bytes, position, int_width)?.1;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = position;
    take_bool(bytes, &mut position)?;
    Some(IntersectionPatchLayout {
        parameter_range,
        discontinuities,
        discontinuity_flag,
    })
}

fn embedded_two_sided_offset(toks: &[Token]) -> Option<EmbeddedTwoSidedOffset> {
    let marker = crate::nurbs::toks::find_owned_intcurve_subtype(toks, "off_int_cur")?;
    let mut cur = Cur::at(toks, marker + 2);
    let first_surface_start = cur.pos();
    let first_surface = optional_embedded_surface(&mut cur)?.value();
    let second_surface_start = cur.pos();
    let second_surface = optional_embedded_surface(&mut cur)?.value();
    let surfaces = [first_surface, second_surface];
    let first_pcurve = optional_pcurve(&mut cur)?.value();
    let second_pcurve = optional_pcurve(&mut cur)?.value();
    let mut pcurves = [first_pcurve, second_pcurve];
    for (pcurve, chart) in pcurves.iter_mut().zip([
        native_support_chart(toks, first_surface_start),
        native_support_chart(toks, second_surface_start),
    ]) {
        if let Some(pcurve) = pcurve {
            normalize_support_pcurve(chart, pcurve);
        }
    }
    let parameter_range = [cur.take_range_value()?, cur.take_range_value()?];
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
    let offsets = [
        cur.take_range_value()? * LEN_TO_MM,
        cur.take_range_value()? * LEN_TO_MM,
    ];
    Some(EmbeddedTwoSidedOffset {
        surfaces,
        pcurves,
        parameter_range,
        discontinuities,
        discontinuity_flag,
        offsets,
    })
}

fn optional_embedded_surface(cur: &mut Cur<'_>) -> Option<Nullable<SurfaceGeometry>> {
    let start = cur.pos();
    if cur.take_ident()? == "null_surface" {
        return Some(Nullable::Null);
    }
    cur.set_pos(start);
    embedded_surface(cur).map(Nullable::Value)
}

fn optional_pcurve(cur: &mut Cur<'_>) -> Option<Nullable<NurbsPcurve>> {
    let start = cur.pos();
    if cur.take_ident()? == "nullbs" {
        return Some(Nullable::Null);
    }
    let (pcurve, end) = pcurve_block_with_end(cur.toks(), start)?;
    cur.set_pos(end);
    Some(Nullable::Value(pcurve))
}

/// Whether the next cache-first support slot contains a valid non-null
/// surface construction.
///
/// The neutral `SurfaceGeometry` field cannot represent a cacheless
/// procedural surface without assigning it a document-level construction ID.
/// Keep that distinction separate: a typed support reference still proves that
/// its paired native pcurve slot is eligible, while `null_surface` does not.
fn support_slot_present(cur: &Cur<'_>, table: &SubtypeTable) -> bool {
    let mut probe = *cur;
    if probe.take_ident() == Some("null_surface") {
        return false;
    }

    let mut parsed = *cur;
    if let Some((Some(_), _)) = optional_embedded_surface_with_bounds(&mut parsed, table) {
        return true;
    }

    let mut probe = *cur;
    if probe.take_ident() != Some("spline") {
        return false;
    }
    if matches!(probe.peek(), Some(Token::True | Token::False)) && probe.take_bool().is_none() {
        return false;
    }
    let Some(Token::SubtypeOpen) = probe.peek() else {
        return false;
    };
    let start = probe.pos();
    let Some(scope) = crate::nurbs::toks::subtype_span(probe.toks(), start) else {
        return false;
    };
    let Some(Token::Ident(name)) = probe.toks().get(start + 1) else {
        return false;
    };
    if name == "ref" {
        let Some(Token::Long(index)) = probe.toks().get(start + 2) else {
            return false;
        };
        let Ok(index) = usize::try_from(*index) else {
            return false;
        };
        return table.span(index).is_some_and(|target| {
            crate::nurbs::core::owned_surface_cache_resolving_refs(target, table).is_some()
                || crate::nurbs::proc_surface::procedural_surface_resolving_refs(target, table)
                    .is_some()
        });
    }
    crate::nurbs::core::owned_surface_cache_resolving_refs(scope, table).is_some()
        || crate::nurbs::proc_surface::procedural_surface_resolving_refs(scope, table).is_some()
}

/// Writable scalar locations in a retained `off_int_cur` construction.
pub struct TwoSidedOffsetPatchLayout {
    /// Byte offsets of the two parameter-range doubles.
    pub parameter_range: [usize; 2],
    /// Byte offsets of the values in each discontinuity array.
    pub discontinuities: [Vec<usize>; 3],
    /// Byte offset of the boolean after the discontinuity arrays.
    pub discontinuity_flag: usize,
    /// Byte offsets of the two side-offset doubles.
    pub offsets: [usize; 2],
}

/// Locates the fixed-width scalar payloads after variable embedded supports.
pub fn two_sided_offset_patch_layout(
    bytes: &[u8],
    int_width: usize,
) -> Option<TwoSidedOffsetPatchLayout> {
    let name = b"off_int_cur";
    let (marker, name_len) = find_owned_intcurve_subtype(bytes, name, int_width)?;
    let mut position = marker + name_len + 3;
    skip_offset_support_surface(bytes, &mut position, int_width)?;
    skip_offset_support_surface(bytes, &mut position, int_width)?;
    skip_offset_support_pcurve(bytes, &mut position, int_width)?;
    skip_offset_support_pcurve(bytes, &mut position, int_width)?;
    let parameter_range = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
        take_float_array_payloads(bytes, &mut position, int_width)?,
    ];
    let discontinuity_flag = position;
    take_bool(bytes, &mut position)?;
    let offsets = [
        take_double_payload(bytes, &mut position)?,
        take_double_payload(bytes, &mut position)?,
    ];
    Some(TwoSidedOffsetPatchLayout {
        parameter_range,
        discontinuities,
        discontinuity_flag,
        offsets,
    })
}

fn skip_offset_support_surface(bytes: &[u8], position: &mut usize, int_width: usize) -> Option<()> {
    let start = *position;
    if take_native_ident(bytes, position)?.as_str() == "null_surface" {
        return Some(());
    }
    *position = start;
    decode_embedded_surface(bytes, position, int_width)?;
    Some(())
}

fn skip_offset_support_pcurve(bytes: &[u8], position: &mut usize, int_width: usize) -> Option<()> {
    let start = *position;
    if take_native_ident(bytes, position)?.as_str() == "nullbs" {
        return Some(());
    }
    *position = decode_pcurve_block_with_end(bytes, start, int_width)?.1;
    Some(())
}

pub(crate) fn decode_embedded_surface(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<SurfaceGeometry> {
    decode_embedded_surface_fields(bytes, position, int_width, false).map(|(surface, _)| surface)
}

/// Decode one embedded analytic or spline support surface. Token-space
/// counterpart of [`decode_embedded_surface`].
pub(crate) fn embedded_surface(cur: &mut Cur<'_>) -> Option<SurfaceGeometry> {
    embedded_surface_fields(cur, false).map(|(surface, _)| surface)
}

/// [`embedded_surface`], preserving the four trailing U/V range fields.
/// Token-space counterpart of [`decode_embedded_surface_with_ranges`].
pub(crate) fn embedded_surface_with_ranges(
    cur: &mut Cur<'_>,
) -> Option<(SurfaceGeometry, [[Option<f64>; 2]; 2])> {
    embedded_surface_fields(cur, true)
}

fn embedded_surface_fields(
    cur: &mut Cur<'_>,
    preserve_ranges: bool,
) -> Option<(SurfaceGeometry, [[Option<f64>; 2]; 2])> {
    let no_ranges = [[None, None], [None, None]];
    let kind = cur.take_ident()?;
    if kind == "spline" {
        let (decoded, end) = surface_block(cur.toks(), cur.pos())?;
        cur.set_pos(end);
        let ranges = if preserve_ranges {
            surface_ranges(cur)?
        } else {
            no_ranges
        };
        return Some((SurfaceGeometry::Nurbs(decoded), ranges));
    }
    let point = cur.take_position()?;
    let point = Point3::new(
        point[0] * LEN_TO_MM,
        point[1] * LEN_TO_MM,
        point[2] * LEN_TO_MM,
    );
    match kind {
        "plane" => {
            let normal = normalized(cur.take_vector3()?)?;
            let u_axis = normalized(cur.take_vector3()?)?;
            cur.take_bool()?;
            let ranges = if preserve_ranges {
                surface_ranges(cur)?
            } else {
                no_ranges
            };
            Some((
                SurfaceGeometry::Plane {
                    origin: point,
                    normal,
                    u_axis,
                },
                ranges,
            ))
        }
        "cone" => {
            let native_axis = normalized(cur.take_vector3()?)?;
            let major = cur.take_vector3()?;
            let radius = (major[0] * major[0] + major[1] * major[1] + major[2] * major[2]).sqrt()
                * LEN_TO_MM;
            let ref_direction = normalized(major)?;
            let ratio = cur.take_f64()?;
            cur.take_bool()?;
            cur.take_bool()?;
            let sine = cur.take_f64()?;
            let cosine = cur.take_f64()?;
            cur.take_f64()?;
            cur.take_bool()?;
            let ranges = if preserve_ranges {
                surface_ranges(cur)?
            } else {
                for _ in 0..4 {
                    cur.take_bool()?;
                }
                no_ranges
            };
            let surface = if sine.abs() <= f64::EPSILON && ratio == 1.0 {
                SurfaceGeometry::Cylinder {
                    origin: point,
                    axis: native_axis,
                    ref_direction,
                    radius,
                }
            } else {
                let axis = if sine * cosine < 0.0 {
                    Vector3::new(-native_axis.x, -native_axis.y, -native_axis.z)
                } else {
                    native_axis
                };
                SurfaceGeometry::Cone {
                    origin: point,
                    axis,
                    ref_direction,
                    radius,
                    ratio,
                    // See `brep/geometry.rs`: `atan2` keeps half-angle recovery
                    // stable across libm implementations.
                    half_angle: sine.abs().atan2(cosine.abs()),
                }
            };
            Some((surface, ranges))
        }
        "sphere" => {
            let radius = cur.take_f64()? * LEN_TO_MM;
            let ref_direction = normalized(cur.take_vector3()?)?;
            let axis = normalized(cur.take_vector3()?)?;
            cur.take_bool()?;
            let ranges = if preserve_ranges {
                surface_ranges(cur)?
            } else {
                for _ in 0..4 {
                    cur.take_bool()?;
                }
                no_ranges
            };
            Some((
                SurfaceGeometry::Sphere {
                    center: point,
                    axis,
                    ref_direction,
                    radius,
                },
                ranges,
            ))
        }
        "torus" => {
            let axis = normalized(cur.take_vector3()?)?;
            let major_radius = cur.take_f64()? * LEN_TO_MM;
            let minor_radius = cur.take_f64()? * LEN_TO_MM;
            let ref_direction = normalized(cur.take_vector3()?)?;
            cur.take_bool()?;
            let ranges = if preserve_ranges {
                surface_ranges(cur)?
            } else {
                for _ in 0..4 {
                    cur.take_bool()?;
                }
                no_ranges
            };
            Some((
                SurfaceGeometry::Torus {
                    center: point,
                    axis,
                    ref_direction,
                    major_radius,
                    minor_radius,
                },
                ranges,
            ))
        }
        _ => None,
    }
}

pub(crate) fn decode_embedded_surface_with_ranges(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<(SurfaceGeometry, [[Option<f64>; 2]; 2])> {
    decode_embedded_surface_fields(bytes, position, int_width, true)
}

fn decode_embedded_surface_fields(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
    preserve_ranges: bool,
) -> Option<(SurfaceGeometry, [[Option<f64>; 2]; 2])> {
    let no_ranges = [[None, None], [None, None]];
    let kind = take_native_ident(bytes, position)?;
    if kind == "spline" {
        let decoded = decode_surface_block(bytes, *position, int_width)?;
        *position = decoded.end;
        let ranges = if preserve_ranges {
            decode_surface_ranges(bytes, position)?
        } else {
            no_ranges
        };
        return Some((SurfaceGeometry::Nurbs(decoded.surface), ranges));
    }
    let point = take_native_vec3(bytes, position, 0x13)?;
    let point = Point3::new(
        point[0] * LEN_TO_MM,
        point[1] * LEN_TO_MM,
        point[2] * LEN_TO_MM,
    );
    match kind.as_str() {
        "plane" => {
            let normal = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            let u_axis = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            take_bool(bytes, position)?;
            let ranges = if preserve_ranges {
                decode_surface_ranges(bytes, position)?
            } else {
                no_ranges
            };
            Some((
                SurfaceGeometry::Plane {
                    origin: point,
                    normal,
                    u_axis,
                },
                ranges,
            ))
        }
        "cone" => {
            let native_axis = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            let major = take_native_vec3(bytes, position, 0x14)?;
            let radius = (major[0] * major[0] + major[1] * major[1] + major[2] * major[2]).sqrt()
                * LEN_TO_MM;
            let ref_direction = normalized(major)?;
            let ratio = take_f64(bytes, position)?;
            take_bool(bytes, position)?;
            take_bool(bytes, position)?;
            let sine = take_f64(bytes, position)?;
            let cosine = take_f64(bytes, position)?;
            take_f64(bytes, position)?;
            take_bool(bytes, position)?;
            let ranges = if preserve_ranges {
                decode_surface_ranges(bytes, position)?
            } else {
                for _ in 0..4 {
                    take_bool(bytes, position)?;
                }
                no_ranges
            };
            let surface = if sine.abs() <= f64::EPSILON && ratio == 1.0 {
                SurfaceGeometry::Cylinder {
                    origin: point,
                    axis: native_axis,
                    ref_direction,
                    radius,
                }
            } else {
                let axis = if sine * cosine < 0.0 {
                    Vector3::new(-native_axis.x, -native_axis.y, -native_axis.z)
                } else {
                    native_axis
                };
                SurfaceGeometry::Cone {
                    origin: point,
                    axis,
                    ref_direction,
                    radius,
                    ratio,
                    // See `brep/geometry.rs`: `atan2` keeps half-angle recovery
                    // stable across libm implementations.
                    half_angle: sine.abs().atan2(cosine.abs()),
                }
            };
            Some((surface, ranges))
        }
        "sphere" => {
            let radius = take_f64(bytes, position)? * LEN_TO_MM;
            let ref_direction = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            let axis = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            take_bool(bytes, position)?;
            let ranges = if preserve_ranges {
                decode_surface_ranges(bytes, position)?
            } else {
                for _ in 0..4 {
                    take_bool(bytes, position)?;
                }
                no_ranges
            };
            Some((
                SurfaceGeometry::Sphere {
                    center: point,
                    axis,
                    ref_direction,
                    radius,
                },
                ranges,
            ))
        }
        "torus" => {
            let axis = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            let major_radius = take_f64(bytes, position)? * LEN_TO_MM;
            let minor_radius = take_f64(bytes, position)? * LEN_TO_MM;
            let ref_direction = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            take_bool(bytes, position)?;
            let ranges = if preserve_ranges {
                decode_surface_ranges(bytes, position)?
            } else {
                for _ in 0..4 {
                    take_bool(bytes, position)?;
                }
                no_ranges
            };
            Some((
                SurfaceGeometry::Torus {
                    center: point,
                    axis,
                    ref_direction,
                    major_radius,
                    minor_radius,
                },
                ranges,
            ))
        }
        _ => None,
    }
}

#[allow(clippy::option_option)] // Outer None is parse failure; inner None is an unresolved ref.
fn optional_embedded_surface_resolving_ref(
    cur: &mut Cur<'_>,
    table: &SubtypeTable,
) -> Option<Option<SurfaceGeometry>> {
    optional_embedded_surface_with_bounds(cur, table).map(|(surface, _)| surface)
}

/// Optional embedded support surface plus its four optional U/V bound fields.
#[allow(clippy::type_complexity)]
pub(crate) fn optional_embedded_surface_with_bounds(
    cur: &mut Cur<'_>,
    table: &SubtypeTable,
) -> Option<(Option<SurfaceGeometry>, [Option<f64>; 4])> {
    let toks = cur.toks();
    let saved = cur.pos();
    let kind = cur.take_ident();
    if kind == Some("null_surface") {
        return Some((None, [None; 4]));
    }
    if kind == Some("spline") {
        if matches!(cur.peek(), Some(Token::True | Token::False)) {
            cur.take_bool()?;
        }
        let reference = cur.pos();
        let compact_ref = matches!(toks.get(reference), Some(Token::SubtypeOpen))
            && matches!(toks.get(reference + 1), Some(Token::Ident(name)) if name == "ref")
            && matches!(toks.get(reference + 2), Some(Token::Long(_)));
        if compact_ref {
            let Some(Token::Long(index)) = toks.get(reference + 2) else {
                return None;
            };
            let index = usize::try_from(*index).ok()?;
            let reference_span = crate::nurbs::toks::subtype_span(toks, reference)?;
            cur.set_pos(reference + reference_span.len());
            let surface = table
                .span(index)
                .and_then(|target| owned_surface_cache_resolving_refs(target, table))
                .map(SurfaceGeometry::Nurbs);
            let mut bounds = [None; 4];
            for bound in &mut bounds {
                *bound = cur.take_optional_range_value()?;
            }
            return Some((surface, bounds));
        }
    }
    cur.set_pos(saved);
    if let Some(surface) = embedded_surface(cur) {
        let mut bounds = [None; 4];
        if kind == Some("plane") || kind == Some("spline") {
            for bound in &mut bounds {
                *bound = cur.take_optional_range_value()?;
            }
        }
        return Some((Some(surface), bounds));
    }
    // Inline `spline { <subtype> }` support scope: resolve a solved surface
    // cache when present, or validate the procedural surface construction when
    // the support is cacheless. A cacheless procedural support has no neutral
    // SurfaceGeometry carrier, but it still makes its paired native pcurve
    // slot eligible.
    cur.set_pos(saved);
    if kind == Some("spline") {
        cur.take_ident()?;
        if matches!(cur.peek(), Some(Token::True | Token::False)) {
            cur.take_bool()?;
        }
        if matches!(cur.peek(), Some(Token::SubtypeOpen)) {
            let scope = crate::nurbs::toks::subtype_span(toks, cur.pos())?;
            let surface = if let Some(surface) = owned_surface_cache_resolving_refs(scope, table) {
                Some(SurfaceGeometry::Nurbs(surface))
            } else if crate::nurbs::proc_surface::procedural_surface_resolving_refs(scope, table)
                .is_some()
            {
                None
            } else {
                return None;
            };
            cur.set_pos(cur.pos() + scope.len());
            let mut bounds = [None; 4];
            for bound in &mut bounds {
                *bound = cur.take_optional_range_value()?;
            }
            return Some((surface, bounds));
        }
    }
    cur.set_pos(saved);
    None
}

fn two_sided_offset(toks: &[Token]) -> Option<cadmpeg_ir::geometry::ProceduralCurveDefinition> {
    use cadmpeg_ir::geometry::{
        IntcurveSupportContext, IntcurveSupportSide, ProceduralCurveDefinition,
    };

    let marker = crate::nurbs::toks::find_owned_intcurve_subtype(toks, "off_int_cur")?;
    let mut cur = Cur::at(toks, marker + 2);
    for expected in ["null_surface", "null_surface", "nullbs", "nullbs"] {
        if cur.take_ident()? != expected {
            return None;
        }
    }
    let parameter_range = [cur.take_range_value()?, cur.take_range_value()?];
    let discontinuities = [
        cur.take_float_array()?,
        cur.take_float_array()?,
        cur.take_float_array()?,
    ];
    let discontinuity_flag = cur.take_bool()?;
    let offsets = [
        cur.take_range_value()? * LEN_TO_MM,
        cur.take_range_value()? * LEN_TO_MM,
    ];
    Some(ProceduralCurveDefinition::TwoSidedOffset {
        context: IntcurveSupportContext {
            sides: [
                IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                    pcurve_parameter_range: None,
                },
                IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                    pcurve_parameter_range: None,
                },
            ],
            parameter_range,
            discontinuities,
        },
        discontinuity_flag,
        offsets,
    })
}

fn compound_definition(toks: &[Token]) -> Option<CompoundDefinition> {
    let marker = crate::nurbs::toks::find_owned_subtype_marker(toks, &["comp_int_cur"])
        .map(|(marker, _)| marker)?;
    let mut cur = Cur::at(toks, marker + 2);
    let parameters = cur.take_float_array()?;
    let count = usize::try_from(cur.take_long()?).ok()?;
    if count == 0 {
        return None;
    }
    let mut component_parameters = Vec::with_capacity(count);
    for _ in 0..count {
        component_parameters.push(cur.take_f64()?);
    }
    if !matches!(cur.peek(), Some(Token::True | Token::False)) {
        return None;
    }
    cur.bump();
    let mut components = Vec::with_capacity(count);
    for _ in 0..count {
        let (curve, end) = curve_block(toks, cur.pos())?;
        components.push(curve);
        cur.set_pos(end);
    }
    Some((parameters, component_parameters, components))
}

fn subset_definition(toks: &[Token]) -> Option<SubsetDefinition> {
    let marker = crate::nurbs::toks::find_owned_intcurve_subtype(toks, "subset_int_cur")?;
    let mut cur = Cur::at(toks, marker + 2);
    let (source, source_end) = curve_block(toks, cur.pos())?;
    cur.set_pos(source_end);
    let range = [cur.take_range_value()?, cur.take_range_value()?];
    Some((source, range))
}

fn vector_offset_definition(toks: &[Token]) -> Option<VectorOffsetDefinition> {
    let marker = crate::nurbs::toks::find_owned_intcurve_subtype(toks, "offset_int_cur")?;
    let mut cur = Cur::at(toks, marker + 2);
    cur.take_bool()?;
    let (source, source_end) = curve_block(toks, cur.pos())?;
    cur.set_pos(source_end);
    if !matches!(toks.get(cur.pos()), Some(Token::Double(_)))
        || !matches!(toks.get(cur.pos() + 1), Some(Token::Double(_)))
    {
        return None;
    }
    let parameter_range = [cur.take_f64()?, cur.take_f64()?];
    let offset = cur.take_vector3()?;
    let first_label = cur.take_str()?.to_string();
    let first_code = cur.take_long()?;
    let second_label = cur.take_str()?.to_string();
    let second_code = cur.take_long()?;
    Some((
        source,
        parameter_range,
        Vector3::new(
            offset[0] * LEN_TO_MM,
            offset[1] * LEN_TO_MM,
            offset[2] * LEN_TO_MM,
        ),
        [first_label, second_label],
        [first_code, second_code],
    ))
}

/// Decode the `helix_int_cur` construction fields. Token-space counterpart of
/// the byte helix walk retained by [`helix_patch_layout`].
pub(crate) fn helix_definition(
    toks: &[Token],
) -> Option<cadmpeg_ir::geometry::ProceduralCurveDefinition> {
    let marker = crate::nurbs::toks::find_owned_subtype_marker(toks, &["helix_int_cur"])
        .map(|(marker, _)| marker)?;
    let mut cur = Cur::at(toks, marker + 2);
    let current_layout = optional_helix_revision(&mut cur)?;
    let lower = cur.take_range_value()?;
    let upper = cur.take_range_value()?;
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
    let axis = cur.take_vector3()?;
    Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
        angle_range: [lower, upper],
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
        axis: Vector3::new(axis[0], axis[1], axis[2]),
    })
}

/// Consume the current helix subtype's ASM release word when present. The
/// earlier form begins directly with an optional range-bound flag or double.
pub(crate) fn take_optional_helix_revision(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<bool> {
    if bytes.get(*position) != Some(&0x04) {
        return Some(false);
    }
    let revision = take_tagged_int(bytes, position, 0x04, int_width)?;
    (20_000..=99_999).contains(&revision).then_some(true)
}

/// Consume the current helix subtype's ASM release word when present. The
/// earlier form begins directly with an optional range-bound flag or double.
/// Token-space counterpart of [`take_optional_helix_revision`].
pub(crate) fn optional_helix_revision(cur: &mut Cur<'_>) -> Option<bool> {
    if !matches!(cur.peek(), Some(Token::Long(_))) {
        return Some(false);
    }
    let revision = cur.take_long()?;
    (20_000..=99_999).contains(&revision).then_some(true)
}

/// Four optional U/V parameter bounds following a surface record's first
/// top-level subtype scope, or `None` when the record stores no bound fields.
/// `toks` is the record's payload tokens.
pub fn record_trailing_surface_bounds(toks: &[Token]) -> Option<[Option<f64>; 4]> {
    // Walk the fixed spline-record header: any leading payload identifiers,
    // attrib ref, history int, geometry ref, sense boolean, then the subtype
    // scope.
    let mut position = 0usize;
    while toks.get(position).is_some_and(Token::is_payload_ident) {
        position += 1;
    }
    if !matches!(toks.get(position), Some(Token::Ref(_))) {
        return None;
    }
    position += 1;
    if !matches!(toks.get(position), Some(Token::Long(_))) {
        return None;
    }
    position += 1;
    if !matches!(toks.get(position), Some(Token::Ref(_))) {
        return None;
    }
    position += 1;
    if !matches!(toks.get(position), Some(Token::True | Token::False)) {
        return None;
    }
    position += 1;
    if !matches!(toks.get(position), Some(Token::SubtypeOpen)) {
        return None;
    }
    let scope = crate::nurbs::toks::subtype_span(toks, position)?;
    position += scope.len();
    if !matches!(toks.get(position), Some(Token::True | Token::False)) {
        return None;
    }
    let mut cur = Cur::at(toks, position);
    let mut bounds = [None; 4];
    for bound in &mut bounds {
        *bound = cur.take_optional_range_value()?;
    }
    Some(bounds)
}

fn nurbs_curve_parameter_domain(curve: &NurbsCurve) -> Option<[f64; 2]> {
    let degree = usize::try_from(curve.degree).ok()?;
    Some([
        *curve.knots.get(degree)?,
        *curve.knots.get(curve.control_points.len())?,
    ])
}

#[cfg(test)]
mod cache_form_tests {
    use super::*;
    use cadmpeg_ir::math::Point2;

    fn linear_pcurve(points: [Point2; 2]) -> NurbsPcurve {
        NurbsPcurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: points.into(),
            weights: None,
            periodic: false,
        }
    }

    #[test]
    fn analytic_support_charts_map_to_neutral_surface_parameters() {
        let mut plane = linear_pcurve([Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)]);
        normalize_support_pcurve(NativeSupportChart::PlaneLengths, &mut plane);
        assert_eq!(
            plane.control_points,
            [Point2::new(10.0, -20.0), Point2::new(30.0, -40.0)]
        );

        let mut cone = linear_pcurve([Point2::new(2.0, 0.5), Point2::new(-3.0, -0.25)]);
        normalize_support_pcurve(NativeSupportChart::Cone { axial_scale: 15.0 }, &mut cone);
        assert_eq!(
            cone.control_points,
            [Point2::new(0.5, 30.0), Point2::new(-0.25, -45.0)]
        );
    }

    #[test]
    fn native_cone_chart_projects_generator_distance_onto_its_axis() {
        let tokens = [
            Token::Ident("cone".into()),
            Token::Position([0.0; 3]),
            Token::Vector3([0.0, 0.0, 1.0]),
            Token::Vector3([1.0, 0.0, 0.0]),
            Token::Double(1.0),
            Token::True,
            Token::False,
            Token::Double(3.0_f64.sqrt() / 2.0),
            Token::Double(0.5),
            Token::Double(1.5),
        ];
        let NativeSupportChart::Cone { axial_scale } = native_support_chart(&tokens, 0) else {
            panic!("cone tokens select a cone parameter chart");
        };
        assert_eq!(axial_scale, 7.5);
    }

    #[test]
    fn standalone_cylinder_pcurve_uses_azimuth_and_axial_distance() {
        let surface = [
            Token::Double(1.0),
            Token::Double(0.0),
            Token::Double(1.0),
            Token::Double(2.0),
        ];
        let mut pcurve = linear_pcurve([Point2::new(-0.5, 1.25), Point2::new(0.75, -2.0)]);
        normalize_pcurve_for_surface_record("cone", &surface, &mut pcurve);
        assert_eq!(
            pcurve.control_points,
            [Point2::new(1.25, -10.0), Point2::new(-2.0, 15.0)]
        );
    }

    /// A tagged integer field.
    fn push_int(bytes: &mut Vec<u8>, tag: u8, value: i64, int_width: usize) {
        bytes.push(tag);
        match int_width {
            8 => bytes.extend_from_slice(&value.to_le_bytes()),
            _ => bytes.extend_from_slice(&(value as i32).to_le_bytes()),
        }
    }

    /// A native identifier.
    fn push_ident(bytes: &mut Vec<u8>, value: &str) {
        bytes.push(0x0d);
        bytes.push(u8::try_from(value.len()).expect("short identifier"));
        bytes.extend_from_slice(value.as_bytes());
    }

    /// The shared cache-first intcurve context after its leading enum: two null
    /// supports, two null pcurves, two absent solved-interval endpoints, three
    /// empty discontinuity arrays, and the ASM extension integer.
    fn push_cache_first_remainder(bytes: &mut Vec<u8>, int_width: usize) {
        push_ident(bytes, "null_surface");
        push_ident(bytes, "null_surface");
        push_ident(bytes, "nullbs");
        push_ident(bytes, "nullbs");
        bytes.extend_from_slice(&[0x0b, 0x0b]);
        for _ in 0..3 {
            push_int(bytes, 0x04, 0, int_width);
        }
        push_int(bytes, 0x04, 7, int_width);
    }

    /// A degree-one solved curve whose parameter domain is `[0, 1]`.
    fn solved_curve() -> NurbsCurve {
        NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            weights: None,
            periodic: false,
        }
    }

    /// Cache form `2` stores no `bs3_curve` and no fit tolerance: the leading
    /// enum is followed by the bool-gated curve interval and the closed-form
    /// enum, and the rest of the context continues unchanged.
    #[test]
    fn parameterized_cache_form_reads_the_interval_and_closed_form_enum() {
        for int_width in [4usize, 8] {
            let mut bytes = Vec::new();
            push_int(&mut bytes, 0x04, 23_100, int_width);
            push_int(&mut bytes, 0x15, 2, int_width);
            bytes.push(0x0a);
            bytes.push(0x06);
            bytes.extend_from_slice(&0.125f64.to_le_bytes());
            bytes.push(0x0b);
            push_int(&mut bytes, 0x15, 1, int_width);
            push_cache_first_remainder(&mut bytes, int_width);

            let solved = solved_curve();
            let toks = crate::nurbs::toks::lex_test_span(&bytes, int_width);
            let table = crate::nurbs::toks::test_table(&bytes, int_width);
            let mut cur = Cur::at(&toks, 0);
            let context =
                cache_first_curve_context(&mut cur, &solved, &table).unwrap_or_else(|| {
                    panic!("parameterized cache-first context at width {int_width}")
                });
            // Every field of the context is read: the walk ends on the last
            // token of the ASM extension integer.
            assert_eq!(cur.pos(), toks.len());
            assert_eq!(context.form.revision, 23_100);
            assert_eq!(context.form.cache.selector(), 2);
            let parameterization = match context.form.cache {
                cadmpeg_ir::geometry::RevisionCacheForm::Parameterization(value) => value,
                cadmpeg_ir::geometry::RevisionCacheForm::SolvedCache { .. } => {
                    panic!("parameterized cache-first context")
                }
            };
            assert_eq!(parameterization.interval, [Some(0.125), None]);
            assert_eq!(parameterization.closed_form, 1);
            assert_eq!(context.form.extension, 7);
            assert_eq!(context.form.solved_range, [None, None]);
            // Absent solved-interval endpoints inherit the solved domain.
            assert_eq!(context.parameter_range, [0.0, 1.0]);
        }
    }

    /// A cache form with no defined grammar fails, so the containing record is
    /// retained verbatim rather than misparsed.
    #[test]
    fn undefined_cache_form_is_rejected_for_verbatim_retention() {
        for int_width in [4usize, 8] {
            let mut bytes = Vec::new();
            push_int(&mut bytes, 0x04, 23_100, int_width);
            push_int(&mut bytes, 0x15, 1, int_width);
            push_cache_first_remainder(&mut bytes, int_width);

            let solved = solved_curve();
            let toks = crate::nurbs::toks::lex_test_span(&bytes, int_width);
            let table = crate::nurbs::toks::test_table(&bytes, int_width);
            let mut cur = Cur::at(&toks, 0);
            assert!(cache_first_curve_context(&mut cur, &solved, &table).is_none());
        }
    }
}
