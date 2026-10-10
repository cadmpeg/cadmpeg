// SPDX-License-Identifier: Apache-2.0
//! Decode analytic surfaces and 3D curves, select edge pcurves, reverse
//! curve orientation, and recognize procedural carriers as analytic geometry.

use super::records::TolerantCoedgeExtension;
use crate::nurbs::proc_surface::{
    DecodedProceduralSurfaceDefinition, EmbeddedRollingBall, EmbeddedScaledCompoundLoftShape,
};
use crate::nurbs::reader::LEN_TO_MM;
use crate::sab::{Record, Token};
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, EllipseCurve, LineCurve, PlaneSurface,
    SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, NonNegativeLength, NonZeroLength, PositiveLength, PositiveReal};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};
use std::collections::{HashMap, HashSet};

use super::AsmBrep;
const EPS_PCURVE_DOMAIN: f64 = 1.0e-9;
const EPS_EXTRUSION_AXIS_ALIGNMENT: f64 = 1.0e-10;
const EPS_ROLLING_BALL_MATCH: f64 = 1.0e-10;
const EPS_SPINE_COLLINEARITY: f64 = 1.0e-10;
const EPS_RATIONAL_KNOT_MATCH: f64 = 1.0e-12;
const EPS_RATIONAL_CIRCLE_MATCH: f64 = 1.0e-10;
const EPS_DEGREE_REDUCTION: f64 = 1.0e-10;
const EPS_EDGE_DOMAIN_SNAP: f64 = 1.0e-9;

macro_rules! propagate_resource {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => return Some(Err(error.into())),
        }
    };
}

/// Ordered typed values pulled from a carrier record's payload.
pub(in crate::brep) struct Carrier<'ctx> {
    pub(super) positions: Vec<[f64; 3]>,
    pub(super) vectors: Vec<[f64; 3]>,
    doubles: Vec<f64>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(super) fn collect_carrier<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    rec: &Record,
) -> Result<Carrier<'ctx>, cadmpeg_core::CodecError> {
    let mut c = Carrier {
        storage: ctx.reserve_scoped(0, "ASM analytic carrier scratch")?,
        positions: Vec::new(),
        vectors: Vec::new(),
        doubles: Vec::new(),
    };
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(rec.tokens.as_ref());
    while source_values.len() != 0 {
        let Some(t) = ctx.next_charged(&mut source_values, "ASM analytic carrier tokens")? else {
            break;
        };
        match t {
            Token::Position(p) => {
                ctx.push_scoped_vec(
                    &mut c.storage,
                    &mut c.positions,
                    *p,
                    "ASM carrier positions",
                )?;
            }
            Token::Vector3(v) => {
                ctx.push_scoped_vec(&mut c.storage, &mut c.vectors, *v, "ASM carrier vectors")?;
            }
            Token::Double(d) => {
                ctx.push_scoped_vec(&mut c.storage, &mut c.doubles, *d, "ASM carrier doubles")?;
            }
            _ => {}
        }
    }
    Ok(c)
}

pub(super) fn scale_point(p: [f64; 3]) -> Point3 {
    Point3::new(p[0] * LEN_TO_MM, p[1] * LEN_TO_MM, p[2] * LEN_TO_MM)
}

pub(super) fn norm3(v: [f64; 3]) -> f64 {
    Vector3::from(v).norm()
}

/// The unit direction of a stored vector, absent when the vector is not
/// finite or its length is within `f64::EPSILON` of zero.
fn direction(v: [f64; 3]) -> Option<UnitVector3> {
    UnitVector3::normalized(Vector3::from(v))
}

/// The frame of two stored directions, absent when either direction is
/// degenerate or the two are not perpendicular.
fn frame(axis: [f64; 3], reference: [f64; 3]) -> Option<OrthonormalFrame3> {
    OrthonormalFrame3::from_units(direction(axis)?, direction(reference)?)
}

/// Whether a record name heads an analytic surface carrier.
pub(super) fn is_analytic_surface(head: &str) -> bool {
    matches!(head, "plane" | "cone" | "sphere" | "torus")
}

/// Whether a record name heads an analytic curve carrier.
pub(super) fn is_analytic_curve(head: &str) -> bool {
    matches!(head, "straight" | "ellipse" | "degenerate_curve")
}

/// Decode an analytic surface carrier. Signed sphere and torus radii remain in
/// the IR because they are part of the ASM carrier semantics.
pub fn decode_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rec: &Record,
) -> Option<Result<(SolvedSurfaceGeometry, bool), cadmpeg_core::CodecError>> {
    let carrier = match collect_carrier(ctx, rec) {
        Ok(carrier) => carrier,
        Err(error) => return Some(Err(error)),
    };
    decode_surface_carrier(rec, &carrier).map(Ok)
}

fn decode_surface_carrier(rec: &Record, c: &Carrier<'_>) -> Option<(SolvedSurfaceGeometry, bool)> {
    let origin = *c.positions.first()?;
    match rec.head() {
        "plane" => Some((
            SolvedSurfaceGeometry::Plane(PlaneSurface::new(
                FinitePoint3::new(scale_point(origin))?,
                frame(*c.vectors.first()?, *c.vectors.get(1)?)?,
            )),
            false,
        )),
        "cone" => {
            let ratio = *c.doubles.first().unwrap_or(&1.0);
            let axis = direction(*c.vectors.first()?)?;
            let major = *c.vectors.get(1)?;
            // Doubles are (ratio, sine, cosine, u_scale). `ratio` is the
            // minor/major radius ratio. `sine` selects cylinder vs cone. The
            // base radius is the major-axis vector's
            // magnitude; the trailing `u_scale` double is the u-parameter
            // scale, which usually coincides with the radius but diverges on
            // offset-derived surfaces. The signed slope `sine / cosine` is the
            // radius change per unit axis distance, and a negative `cosine`
            // points the surface normal toward the axis.
            let sine = *c.doubles.get(1).unwrap_or(&0.0);
            let cosine = *c.doubles.get(2).unwrap_or(&1.0);
            let radius = norm3(major) * LEN_TO_MM;
            (radius > f64::EPSILON).then_some(())?;
            let ref_direction = direction(major)?;
            if sine.abs() <= f64::EPSILON && ratio == 1.0 {
                Some((
                    SolvedSurfaceGeometry::Cylinder(CylinderSurface::new(
                        FinitePoint3::new(scale_point(origin))?,
                        OrthonormalFrame3::from_units(axis, ref_direction)?,
                        PositiveLength::new(radius)?,
                    )),
                    cosine < 0.0,
                ))
            } else {
                // The IR cone's radius grows along `+axis`; a negative native
                // slope shrinks it, so the axis flips to compensate. The
                // outward normal is invariant under the flip; the inward
                // normal of a negative `cosine` folds into the face sense.
                let axis = if sine * cosine < 0.0 {
                    axis.reversed()
                } else {
                    axis
                };
                Some((
                    SolvedSurfaceGeometry::Cone(ConeSurface::new(
                        FinitePoint3::new(scale_point(origin))?,
                        OrthonormalFrame3::from_units(axis, ref_direction)?,
                        NonNegativeLength::new(radius)?,
                        PositiveReal::new(ratio)?,
                        Angle::new(sine.abs().atan2(cosine.abs()))?,
                    )),
                    cosine < 0.0,
                ))
            }
        }
        "sphere" => {
            let signed = *c.doubles.first()?;
            Some((
                SolvedSurfaceGeometry::Sphere(SphereSurface::new(
                    FinitePoint3::new(scale_point(origin))?,
                    frame(*c.vectors.get(1)?, *c.vectors.first()?)?,
                    NonZeroLength::new(signed * LEN_TO_MM)?,
                )),
                false,
            ))
        }
        "torus" => {
            let axis = *c.vectors.first()?;
            let ref_direction = *c.vectors.get(1)?;
            let major = *c.doubles.first()?;
            let minor = *c.doubles.get(1)?;
            Some((
                SolvedSurfaceGeometry::Torus(TorusSurface::new(
                    FinitePoint3::new(scale_point(origin))?,
                    frame(axis, ref_direction)?,
                    PositiveLength::new(major * LEN_TO_MM)?,
                    NonZeroLength::new(minor * LEN_TO_MM)?,
                )),
                false,
            ))
        }
        _ => None,
    }
}

/// The vertex record's point reference. The modern layout stores the
/// endpoint-index integer at chunk 4 and the point at chunk 5; the
/// save-format 700 layout stores no endpoint index and the point at chunk 4.
/// A modern record always carries the integer, so the legacy branch is
/// unreachable for it.
pub(super) fn vertex_point_ref(record: &Record) -> Option<i64> {
    match record.chunk(4) {
        Some(Token::Long(_)) => record.ref_at(5),
        _ => record.ref_at(4),
    }
}

/// The coedge record's pcurve reference: chunk 10 after the reserved integer
/// in the modern layout, chunk 9 in the save-format 700 layout that stores
/// no reserved integer.
pub(super) fn coedge_pcurve_ref(record: &Record) -> Option<i64> {
    match record.chunk(9) {
        Some(Token::Long(_)) => record.ref_at(10),
        _ => record.ref_at(9),
    }
}

pub(super) fn is_vertex_record(record: &Record) -> bool {
    matches!(record.head(), "vertex" | "tvertex")
}

pub(super) fn is_edge_record(record: &Record) -> bool {
    matches!(record.head(), "edge" | "tedge")
}

pub(super) fn is_coedge_record(record: &Record) -> bool {
    matches!(record.head(), "coedge" | "tcoedge")
}

pub(super) fn tolerant_coedge_extension(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &Record,
) -> Result<Option<TolerantCoedgeExtension>, cadmpeg_core::CodecError> {
    let mut tokens = record.tokens.iter();
    let mut fields = [None; 16];
    for field in &mut fields {
        *field = ctx.find_map(
            &mut tokens,
            |token| Ok((!token.is_payload_ident()).then_some(token)),
            "ASM tolerant coedge fields",
        )?;
        if field.is_none() {
            return Ok(None);
        }
    }
    let target = match fields[13] {
        Some(Token::Ref(target)) => (*target >= 0).then_some(*target),
        _ => return Ok(None),
    };
    match fields[14] {
        Some(Token::Long(0)) if matches!(fields[15], Some(Token::Long(0))) => {
            Ok(Some(TolerantCoedgeExtension::Empty { target }))
        }
        Some(Token::Long(1)) => {
            let curve_reversed = match fields[15] {
                Some(Token::True) => true,
                Some(Token::False) => false,
                _ => return Ok(None),
            };
            let open = ctx.find_map(
                &mut tokens,
                |token| Ok((!token.is_payload_ident()).then_some(token)),
                "ASM tolerant coedge fields",
            )?;
            if !matches!(open, Some(Token::SubtypeOpen)) {
                return Ok(None);
            }
            let mut depth = 1usize;
            let mut payload_token_count = 0usize;
            let close = ctx.find_map(
                &mut tokens,
                |token| {
                    match token {
                        Token::SubtypeOpen => depth += 1,
                        Token::SubtypeClose => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        return Ok(Some(()));
                    }
                    if !token.is_payload_ident() {
                        payload_token_count += 1;
                    }
                    Ok(None)
                },
                "ASM tolerant coedge payload",
            )?;
            if close.is_none() {
                return Ok(None);
            }
            let mut suffix = [None; 6];
            for field in &mut suffix {
                *field = ctx.find_map(
                    &mut tokens,
                    |token| Ok((!token.is_payload_ident()).then_some(token)),
                    "ASM tolerant coedge suffix",
                )?;
                if field.is_none() {
                    break;
                }
            }
            let parameter_range = match suffix {
                [Some(Token::False), Some(Token::False), Some(Token::Long(0)), None, None, None] => {
                    None
                }
                [Some(Token::True), Some(Token::Double(start)), Some(Token::True), Some(Token::Double(end)), Some(Token::Long(0)), None] =>
                {
                    let Some(range) = cadmpeg_ir::units::FiniteVector::new([*start, *end]) else {
                        return Ok(None);
                    };
                    Some(range)
                }
                _ => return Ok(None),
            };
            let Ok(payload_token_count) = u32::try_from(payload_token_count) else {
                return Ok(None);
            };
            Ok(Some(TolerantCoedgeExtension::EmbeddedCurve {
                target,
                curve_reversed,
                payload_token_count,
                parameter_range,
            }))
        }
        _ => Ok(None),
    }
}

/// Whether a record head belongs to the topology or geometry vocabulary.
///
/// Carrier heads remain known even when no active topology references that
/// particular record. Reachability determines transfer; it does not turn an
/// unreferenced carrier into an application/refinement record.
pub(super) fn is_known_record_head(head: &str) -> bool {
    matches!(
        head,
        "body"
            | "region"
            | "lump"
            | "shell"
            | "subshell"
            | "wire"
            | "face"
            | "loop"
            | "point"
            | "asmheader"
    ) || matches!(
        head,
        "coedge" | "tcoedge" | "edge" | "tedge" | "vertex" | "tvertex"
    ) || is_analytic_surface(head)
        || is_analytic_curve(head)
        || matches!(head, "spline" | "intcurve" | "pcurve")
}

pub(super) fn is_asm_stream_delimiter(name: &str) -> bool {
    matches!(name, "Begin-of-ASM-History-Data" | "End-of-ASM-data")
}

pub(super) fn edge_pcurve_parameter_ranges(edge: &Record) -> Option<[[f64; 2]; 2]> {
    let (Some(Token::Double(start)), Some(Token::Double(end))) = (edge.chunk(4), edge.chunk(6))
    else {
        return None;
    };
    let direct = [*start, *end];
    let negated = [-start, -end];
    Some(if matches!(edge.chunk(9), Some(Token::True)) {
        [negated, direct]
    } else {
        [direct, negated]
    })
}

/// Candidate edge-use intervals whose endpoints lie on this pcurve carrier.
/// Edge sense orders the two signs, but it cannot move a NURBS use outside the
/// carrier's active knot domain. That active domain is the final fallback.
pub(super) fn pcurve_ranges_on_domain(
    candidate: &cadmpeg_ir::geometry::pcurve::PcurveNurbs,
    edge: Option<&Record>,
) -> Option<impl Iterator<Item = [f64; 2]>> {
    let first = *candidate
        .knots()
        .get(usize::try_from(candidate.degree()).ok()?)?;
    let last = *candidate.knots().get(candidate.pole_rows().count())?;
    (first < last).then_some(())?;
    let mut ranges = [None; 3];
    let mut count = 0;
    for range in edge
        .and_then(edge_pcurve_parameter_ranges)
        .into_iter()
        .flatten()
    {
        if let Some(range) = (|| {
            range
                .iter()
                .all(|value| {
                    cadmpeg_ir::math::parameter_in_domain(*value, [first, last], EPS_PCURVE_DOMAIN)
                })
                .then_some(())?;
            let range = range.map(|value| value.clamp(first, last));
            (range[0] != range[1]).then_some(range)
        })() {
            ranges[count] = Some(range);
            count += 1;
        }
    }
    if !ranges.contains(&Some([first, last])) {
        ranges[count] = Some([first, last]);
    }
    Some(ranges.into_iter().flatten())
}

/// Decode an analytic curve carrier.
pub fn decode_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rec: &Record,
) -> Option<Result<CurveGeometry, cadmpeg_core::CodecError>> {
    let carrier = match collect_carrier(ctx, rec) {
        Ok(carrier) => carrier,
        Err(error) => return Some(Err(error)),
    };
    decode_curve_carrier(rec, &carrier).map(Ok)
}

fn decode_curve_carrier(rec: &Record, carrier: &Carrier<'_>) -> Option<CurveGeometry> {
    let base = *carrier.positions.first()?;
    match rec.head() {
        "straight" => Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::new(
                FinitePoint3::new(scale_point(base))?,
                direction(*carrier.vectors.first()?)?,
            ),
        ))),
        "ellipse" => {
            let axis = *carrier.vectors.first()?;
            let reference = *carrier.vectors.get(1)?;
            let ratio = *carrier.doubles.first()?;
            let major_radius = norm3(reference) * LEN_TO_MM;
            let center = FinitePoint3::new(scale_point(base))?;
            let conic_frame = frame(axis, reference)?;
            if (ratio.abs() - 1.0).abs() <= f64::EPSILON {
                Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    CircleCurve::new(center, conic_frame, PositiveLength::new(major_radius)?),
                )))
            } else {
                Some(CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                    EllipseCurve::try_from_parts(
                        center,
                        conic_frame,
                        PositiveLength::new(major_radius)?,
                        PositiveLength::new(major_radius * ratio.abs())?,
                    )
                    .ok()?,
                )))
            }
        }
        "degenerate_curve" => Some(CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
            cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(scale_point(base)).ok()?,
        ))),
        _ => None,
    }
}

pub(super) fn sense_at(rec: &Record, i: usize) -> Sense {
    match rec.chunk(i) {
        Some(Token::True) => Sense::Reversed,
        _ => Sense::Forward,
    }
}

/// The record-level sense bit of an `intcurve` or `spline` carrier: the boolean
/// token immediately before the record's subtype scope ([spec §6.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#66-intcurve-and-spline-subtypes)). `true`
/// marks geometry as the reverse of its cached definition. A reversed intcurve
/// negates the cache parameterization (`C(t) = cache(-t)`), and a reversed
/// spline surface flips the cache normal.
pub(super) fn record_reversed(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rec: &Record,
) -> Result<bool, cadmpeg_core::CodecError> {
    // Payload identifiers do not separate the sense bit from its scope.
    let mut previous: Option<&Token> = None;
    let mut header_fields = 0;
    let mut header_reversed = false;
    let scoped = ctx.find_map(
        rec.tokens.as_ref(),
        |token| {
            if token.is_payload_ident() {
                return Ok(None);
            }
            if header_fields < 4 {
                if header_fields == 3 {
                    header_reversed = matches!(token, Token::True);
                }
                header_fields += 1;
            }
            let reversed = if matches!(token, Token::SubtypeOpen) {
                match previous {
                    Some(Token::True) => Some(true),
                    Some(Token::False) => Some(false),
                    _ => None,
                }
            } else {
                None
            };
            previous = Some(token);
            Ok(reversed)
        },
        "ASM record sense tokens",
    )?;
    Ok(scoped.unwrap_or_else(|| rec.head() == "intcurve" && header_reversed))
}

/// Reverse a curve carrier to its opposite orientation, `C'(t) = C(-t)`.
/// Lines negate their direction, conics negate their plane normal (flipping
/// the angular sweep while keeping the zero-angle direction), and B-splines
/// reverse poles and knots. Carriers without an orientation pass through.
pub(super) fn reverse_curve_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &mut CurveGeometry,
) -> Result<(), cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            line_curve.reverse_parameterization();
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            circle_curve.reverse_parameterization();
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            ellipse_curve.reverse_parameterization();
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) => {
            curve.reverse_parameterization(ctx)?;
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn reverse_procedural_curve_definition(
    definition: &mut cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), &'static str> {
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix(helix) = definition {
        helix.try_reverse_parameterization()?;
    }
    Ok(())
}

pub(super) fn double_at(rec: &Record, i: usize) -> Option<f64> {
    match rec.chunk(i) {
        Some(Token::Double(d)) => Some(*d),
        _ => None,
    }
}

#[derive(Default)]
pub(super) struct PcurveTailMetadata {
    pub(super) flags: Option<[bool; 4]>,
    pub(super) parameter_range: Option<[f64; 2]>,
}

pub(super) fn pcurve_tail_metadata(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rec: &Record,
) -> Result<PcurveTailMetadata, cadmpeg_core::CodecError> {
    let mut metadata = PcurveTailMetadata::default();
    let mut values = rec.tokens.iter().rev();
    let mut tail = [None; 6];
    for slot in &mut tail[..2] {
        *slot = ctx.find_map(
            &mut values,
            |token| Ok((!token.is_payload_ident()).then_some(token)),
            "ASM pcurve parameter tail",
        )?;
        if slot.is_none() {
            return Ok(metadata);
        }
    }
    metadata.parameter_range = match (tail[0], tail[1]) {
        (Some(Token::Double(end)), Some(Token::Double(start))) => Some([*start, *end]),
        _ => None,
    };
    let mut prefix = rec.tokens.iter();
    let mut field = None;
    for _ in 0..4 {
        field = ctx.find_map(
            &mut prefix,
            |token| Ok((!token.is_payload_ident()).then_some(token)),
            "ASM pcurve wrapper fields",
        )?;
        if field.is_none() {
            return Ok(metadata);
        }
    }
    if !matches!(field, Some(Token::Long(0))) {
        return Ok(metadata);
    }
    for slot in &mut tail[2..] {
        *slot = ctx.find_map(
            &mut values,
            |token| Ok((!token.is_payload_ident()).then_some(token)),
            "ASM pcurve boolean tail",
        )?;
        if slot.is_none() {
            return Ok(metadata);
        }
    }
    let mut flags = [false; 4];
    for (slot, token) in flags.iter_mut().zip(tail[2..].iter().rev()) {
        *slot = match token {
            Some(Token::True) => true,
            Some(Token::False) => false,
            _ => return Ok(metadata),
        };
    }
    metadata.flags = Some(flags);
    Ok(metadata)
}

pub(super) fn procedural_surface_definition_is_exact_carrier(
    definition: &DecodedProceduralSurfaceDefinition,
) -> bool {
    match definition {
        DecodedProceduralSurfaceDefinition::Extrusion { .. }
        | DecodedProceduralSurfaceDefinition::Sum { .. }
        | DecodedProceduralSurfaceDefinition::Helix(_)
        | DecodedProceduralSurfaceDefinition::Ruled { .. }
        | DecodedProceduralSurfaceDefinition::VertexBlend(_)
        | DecodedProceduralSurfaceDefinition::SubSurface { .. } => true,
        DecodedProceduralSurfaceDefinition::Sweep(construction) => matches!(
            &construction.layout,
            crate::nurbs::proc_surface::EmbeddedSweepSurfaceLayout::Revision { form, .. }
                if matches!(form.cache, cadmpeg_ir::geometry::RevisionCacheForm::Parameterization(_))
        ),
        DecodedProceduralSurfaceDefinition::Law(construction) => !matches!(
            construction.tail,
            cadmpeg_ir::geometry::LawSurfaceTail::Full { .. }
        ),
        DecodedProceduralSurfaceDefinition::ScaledCompoundLoft(construction) => matches!(
            construction.shape,
            EmbeddedScaledCompoundLoftShape::None { .. }
        ),
        // Tail form `2` stores no solved cache, so every surface block a blend
        // record holds is a support of its construction.
        DecodedProceduralSurfaceDefinition::Blend {
            native: Some(construction),
            ..
        } => matches!(
            construction.cache,
            cadmpeg_ir::geometry::RevisionCacheForm::Parameterization(_)
        ),
        DecodedProceduralSurfaceDefinition::VariableBlend(construction) => {
            matches!(
                construction.cache,
                cadmpeg_ir::geometry::VariableBlendCache::Parameterization { .. }
                    | cadmpeg_ir::geometry::VariableBlendCache::Stale {}
            )
        }
        _ => false,
    }
}

pub(super) fn analytic_procedural_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &DecodedProceduralSurfaceDefinition,
) -> Option<Result<SurfaceGeometry, cadmpeg_core::CodecError>> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Some(Err(refusal.into()));
    }
    match definition {
        DecodedProceduralSurfaceDefinition::Extrusion {
            directrix,
            direction,
            ..
        } => {
            let (center, normal, ref_direction, radius) =
                propagate_resource!(rational_four_arc_circle(ctx, directrix)?);
            let axis = UnitVector3::normalized(*direction)?;
            if 1.0 - axis.as_raw().dot(normal).abs() > EPS_EXTRUSION_AXIS_ALIGNMENT {
                return None;
            }
            Some(Ok(SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(CylinderSurface::new(
                    FinitePoint3::new(center)?,
                    OrthonormalFrame3::from_units(axis, UnitVector3::new(ref_direction)?)?,
                    PositiveLength::new(radius)?,
                )),
            )))
        }
        DecodedProceduralSurfaceDefinition::Blend {
            supports,
            spine: Some(spine),
            radius_offsets: [signed_radius, end_offset],
            cross_section: cadmpeg_ir::geometry::BlendCrossSection::Circular,
            native,
        } if signed_radius == end_offset => {
            analytic_rolling_ball_surface(ctx, supports, native.as_deref(), spine, *signed_radius)
        }
        _ => None,
    }
}

fn analytic_rolling_ball_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    supports: &[Option<SurfaceGeometry>; 2],
    native: Option<&EmbeddedRollingBall>,
    spine: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    signed_radius: f64,
) -> Option<Result<SurfaceGeometry, cadmpeg_core::CodecError>> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Some(Err(refusal.into()));
    }
    let radius = signed_radius.abs();
    if !radius.is_finite() || radius <= f64::EPSILON {
        return None;
    }
    let support = |index: usize| {
        supports[index].as_ref().or_else(|| {
            native.and_then(|native| {
                native.sides[index]
                    .surface
                    .as_ref()
                    .map(|support| &support.surface)
            })
        })
    };
    let first = support(0)?;
    let second = support(1)?;

    if let (
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface_2)),
    ) = (first, second)
    {
        let first_origin = plane_surface.origin();
        let first_normal = *plane_surface.frame().axis().as_raw();
        let second_origin = plane_surface_2.origin();
        let second_normal = *plane_surface_2.frame().axis().as_raw();
        let (origin, axis) = propagate_resource!(linear_nurbs_spine(ctx, spine)?);
        let tolerance = EPS_ROLLING_BALL_MATCH * radius;
        let support_intersection = first_normal.cross(second_normal);
        let support_intersection_norm = support_intersection.norm();
        if support_intersection_norm <= EPS_ROLLING_BALL_MATCH
            || 1.0
                - axis
                    .as_raw()
                    .dot(support_intersection.scale(1.0 / support_intersection_norm))
                    .abs()
                > EPS_ROLLING_BALL_MATCH
        {
            return None;
        }
        for (plane_origin, plane_normal) in [
            (*first_origin, first_normal),
            (*second_origin, second_normal),
        ] {
            if axis.as_raw().dot(plane_normal).abs() > EPS_ROLLING_BALL_MATCH
                || (point_vector(plane_origin, origin).dot(plane_normal).abs() - radius).abs()
                    > tolerance
            {
                return None;
            }
        }
        return Some(Ok(SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Cylinder(CylinderSurface::new(
                FinitePoint3::new(origin)?,
                OrthonormalFrame3::from_units(
                    axis,
                    UnitVector3::new(cadmpeg_ir::geometry::derive_reference_direction(
                        *axis.as_raw(),
                    ))?,
                )?,
                PositiveLength::new(radius)?,
            )),
        )));
    }

    let (plane_origin, plane_normal, cylinder_origin, cylinder_axis, cylinder_radius) =
        match (first, second) {
            (
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)),
            ) => {
                let plane_origin = plane_surface.origin().get();
                let plane_normal = *plane_surface.frame().axis().as_raw();
                let cylinder_origin = cylinder_surface.origin().get();
                let cylinder_axis = *cylinder_surface.frame().axis().as_raw();
                let cylinder_radius = cylinder_surface.radius().get();
                (
                    plane_origin,
                    plane_normal,
                    cylinder_origin,
                    cylinder_axis,
                    cylinder_radius,
                )
            }
            (
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface_2)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface_2)),
            ) => {
                let cylinder_origin = cylinder_surface_2.origin().get();
                let cylinder_axis = *cylinder_surface_2.frame().axis().as_raw();
                let cylinder_radius = cylinder_surface_2.radius().get();
                let plane_origin = plane_surface_2.origin().get();
                let plane_normal = *plane_surface_2.frame().axis().as_raw();
                (
                    plane_origin,
                    plane_normal,
                    cylinder_origin,
                    cylinder_axis,
                    cylinder_radius,
                )
            }
            _ => {
                return None;
            }
        };
    let (center, axis, ref_direction, major_radius) =
        propagate_resource!(rational_four_arc_circle(ctx, spine)?);
    let scale = major_radius.max(radius).max(cylinder_radius);
    let tolerance = EPS_ROLLING_BALL_MATCH * scale;
    let center_offset = point_vector(cylinder_origin, center);
    let axial_offset = center_offset.dot(cylinder_axis);
    let radial_offset = center_offset - cylinder_axis.scale(axial_offset);
    if 1.0 - axis.dot(plane_normal).abs() > EPS_ROLLING_BALL_MATCH
        || 1.0 - axis.dot(cylinder_axis).abs() > EPS_ROLLING_BALL_MATCH
        || (point_vector(plane_origin, center).dot(plane_normal).abs() - radius).abs() > tolerance
        || radial_offset.norm() > tolerance
        || ((major_radius - cylinder_radius).abs() - radius).abs() > tolerance
    {
        return None;
    }
    Some(Ok(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        TorusSurface::try_new(center, axis, ref_direction, major_radius, signed_radius).ok()?,
    ))))
}

fn linear_nurbs_spine(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Option<Result<(Point3, UnitVector3), cadmpeg_core::CodecError>> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Some(Err(refusal.into()));
    }
    if curve.degree() == 0 || curve.periodic() {
        return None;
    }
    match curve.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
            linear_spine_points(ctx, points, |point| *point)
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
            let first_sign = points.first()?.weight.get().signum();
            if propagate_resource!(ctx.any_by(
                points,
                |pole| { Ok(pole.weight.get().signum() != first_sign) },
                "ASM spine weight signs"
            )) {
                return None;
            }
            linear_spine_points(ctx, points, |pole| pole.point)
        }
    }
}

fn linear_spine_points<T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    points: &[T],
    point: impl Fn(&T) -> FinitePoint3,
) -> Option<Result<(Point3, UnitVector3), cadmpeg_core::CodecError>> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Some(Err(refusal.into()));
    }
    let origin = point(points.first()?);
    let farthest = propagate_resource!(ctx.max_by_key(
        points,
        |pole| Ok(point_vector(origin.get(), point(pole).get()).norm()),
        |left, right| Ok(left.total_cmp(right)),
        "ASM spine farthest control point",
    ))?;
    let extent = point_vector(origin.get(), point(farthest).get()).norm();
    if !extent.is_finite() || extent <= f64::EPSILON {
        return None;
    }
    let axis = UnitVector3::normalized(point_vector(origin.get(), point(farthest).get()))?;
    // This admits an analytic replacement, not a model-length approximation.
    if propagate_resource!(ctx.any_by(
        points,
        |pole| {
            let relative = point_vector(origin.get(), point(pole).get());
            let relative = Vector3::new(
                relative.x / extent,
                relative.y / extent,
                relative.z / extent,
            );
            Ok(axis.as_raw().cross(relative).norm() > EPS_SPINE_COLLINEARITY)
        },
        "ASM spine collinearity"
    )) {
        return None;
    }
    Some(Ok((origin.get(), axis)))
}

pub(super) fn rational_four_arc_circle(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Option<Result<(Point3, Vector3, Vector3, f64), cadmpeg_core::CodecError>> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Some(Err(refusal.into()));
    }
    let cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } = curve.pole_rows() else {
        return None;
    };
    let degree = usize::try_from(curve.degree()).ok()?;
    if degree < 2 || curve.periodic() || points.len() != 4 * degree + 1 {
        return None;
    }
    let knot_tolerance = EPS_RATIONAL_KNOT_MATCH
        * (curve.knots()[curve.knots().len() - 1] * 0.5 - curve.knots()[0] * 0.5).abs()
        * 2.0;
    let spans = [
        curve.knots()[0],
        curve.knots()[degree + 1],
        curve.knots()[2 * degree + 1],
        curve.knots()[3 * degree + 1],
        curve.knots()[4 * degree + 1],
    ];
    if spans
        .windows(2)
        .any(|pair| pair[1] - pair[0] <= knot_tolerance)
    {
        return None;
    }
    for (span, knot) in spans.iter().enumerate() {
        let range = if span == 0 {
            0..degree + 1
        } else if span == 4 {
            4 * degree + 1..curve.knots().len()
        } else {
            span * degree + 1..(span + 1) * degree + 1
        };
        if propagate_resource!(ctx.any_by(
            &curve.knots()[range],
            |value| Ok((*value - knot).abs() > knot_tolerance),
            "ASM rational span knots"
        )) {
            return None;
        }
    }
    let weight_scale = propagate_resource!(ctx.fold(
        points,
        0.0_f64,
        |scale, pole| Ok(scale.max(pole.weight.get().abs())),
        "ASM rational weight scale"
    ));
    let _homogeneous_storage;
    let (mut homogeneous, result_homogeneous_storage) = propagate_resource!(
        ctx.temporary_vec(points.len(), "ASM rational four-arc homogeneous poles")
    );
    _homogeneous_storage = result_homogeneous_storage;
    if let Some(refusal) = ctx.resource_refusal() {
        return Some(Err(refusal.into()));
    }
    let mut source_values = IntoIterator::into_iter(points);
    while source_values.len() != 0 {
        let Some(pole) = propagate_resource!(ctx.next_charged(&mut source_values, "ASM rational homogeneous pole pass")) else {
            break;
        };
        let point = pole.point;
        let weight = pole.weight.get() / weight_scale;
        let homogeneous_pole = [point.x * weight, point.y * weight, point.z * weight, weight];
        if !(weight.is_finite()
            && weight != 0.0
            && homogeneous_pole.iter().all(|value| value.is_finite()))
        {
            return None;
        }
        homogeneous.push(homogeneous_pole);
    }
    let mut quadratics = [None; 4];
    for (span, quadratic) in quadratics.iter_mut().enumerate() {
        *quadratic = Some(propagate_resource!(reduce_homogeneous_bezier_to_quadratic(
            ctx,
            &homogeneous[span * degree..=span * degree + degree],
        )?));
    }
    let quadratics = [
        quadratics[0]?,
        quadratics[1]?,
        quadratics[2]?,
        quadratics[3]?,
    ];
    let base_weight = quadratics[0][0][3];
    let weight_scale = base_weight.abs();
    let weight_tolerance = EPS_RATIONAL_CIRCLE_MATCH * weight_scale;
    if !base_weight.is_finite()
        || base_weight == 0.0
        || quadratics.iter().any(|span| {
            (span[0][3] - base_weight).abs() > weight_tolerance
                || (span[2][3] - base_weight).abs() > weight_tolerance
                || (span[1][3] - base_weight * std::f64::consts::FRAC_1_SQRT_2).abs()
                    > weight_tolerance
        })
    {
        return None;
    }
    let quadratic_points = quadratics.map(|span| {
        span.map(|point| {
            Point3::new(
                point[0] / point[3],
                point[1] / point[3],
                point[2] / point[3],
            )
        })
    });
    let point_distance = |left: Point3, right: Point3| point_vector(left, right).norm();
    let scale = quadratic_points
        .iter()
        .flat_map(|span| span.windows(2))
        .map(|pair| point_distance(pair[0], pair[1]))
        .fold(0.0_f64, f64::max);
    let tolerance = EPS_RATIONAL_CIRCLE_MATCH * scale;
    if point_distance(quadratic_points[0][0], quadratic_points[3][2]) > tolerance
        || quadratic_points
            .windows(2)
            .any(|pair| point_distance(pair[0][2], pair[1][0]) > tolerance)
    {
        return None;
    }
    let first_center = point_sum_difference(
        quadratic_points[0][0],
        quadratic_points[0][2],
        quadratic_points[0][1],
    );
    for span in &quadratic_points {
        let [start, control, end] = *span;
        let center = point_sum_difference(start, end, control);
        if point_distance(center, first_center) > tolerance {
            return None;
        }
    }
    let first_radial = point_vector(first_center, quadratic_points[0][0]);
    let radius = first_radial.norm();
    if !radius.is_finite() || radius <= tolerance {
        return None;
    }
    let mut normal = None;
    for span in &quadratic_points {
        let radial = point_vector(first_center, span[0]);
        let next = point_vector(first_center, span[2]);
        let radial_unit = FiniteVector3::new(radial)?.unit_nonzero()?;
        let next_unit = FiniteVector3::new(next)?.unit_nonzero()?;
        if (radial.norm() - radius).abs() > tolerance
            || radial_unit.dot(next_unit).abs() > EPS_RATIONAL_CIRCLE_MATCH
        {
            return None;
        }
        let span_normal = FiniteVector3::new(radial_unit.cross(next_unit))?.unit_nonzero()?;
        if normal.is_some_and(|normal: Vector3| {
            normal.dot(span_normal) < 1.0 - EPS_RATIONAL_CIRCLE_MATCH
        }) {
            return None;
        }
        normal.get_or_insert(span_normal);
    }
    Some(Ok((
        first_center,
        normal?,
        FiniteVector3::new(first_radial)?.unit_nonzero()?,
        radius,
    )))
}

fn reduce_homogeneous_bezier_to_quadratic(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    input: &[[f64; 4]],
) -> Option<Result<[[f64; 4]; 3], cadmpeg_core::CodecError>> {
    (|| -> Result<Option<[[f64; 4]; 3]>, cadmpeg_core::CodecError> {
        let mut control_storage;
        let (mut control, result_control_storage) =
            ctx.copy_temporary_slice(input, "ASM rational four-arc control copy")?;
        control_storage = result_control_storage;
        while control.len() > 3 {
            ctx.charge_work(1, "ASM rational four-arc reduction work")?;
            let degree = control.len() - 1;
            let reduced_storage;
            let (mut reduced, result_reduced_storage) =
                ctx.temporary_vec(degree, "ASM rational four-arc degree reduction")?;
            reduced_storage = result_reduced_storage;
            reduced.push(control[0]);
            if let Some(refusal) = ctx.resource_refusal() {
                return Err(refusal.into());
            }
            let mut source_values = IntoIterator::into_iter(1..degree);
            while source_values.len() != 0 {
                let Some(index) = ctx.next_charged(&mut source_values, "ASM rational reduction poles")? else {
                    break;
                };
                let (Some(index_value), Some(degree_value)) = (
                    cadmpeg_core::convert::f64_from_index(index),
                    cadmpeg_core::convert::f64_from_index(degree),
                ) else {
                    return Ok(None);
                };
                let alpha = index_value / degree_value;
                let denominator = 1.0 - alpha;
                reduced.push(std::array::from_fn(|coordinate| {
                    (control[index][coordinate] - alpha * reduced[index - 1][coordinate])
                        / denominator
                }));
            }
            if ctx.any_by(
                &reduced,
                |point| Ok(point.iter().any(|value| !value.is_finite())),
                "ASM rational reduced pole finiteness",
            )? {
                return Ok(None);
            }
            for coordinate in 0..4 {
                let scale = ctx.fold(
                    &control,
                    0.0_f64,
                    |scale, point| Ok(scale.max(point[coordinate].abs())),
                    "ASM rational reduction scale",
                )?;
                if (reduced[degree - 1][coordinate] - control[degree][coordinate]).abs()
                    > EPS_DEGREE_REDUCTION * scale
                {
                    return Ok(None);
                }
            }
            control = reduced;
            control_storage = reduced_storage;
        }
        let quadratic = control.try_into().ok();
        drop(control_storage);
        Ok(quadratic)
    })()
    .transpose()
}

pub(super) fn point_vector(origin: Point3, point: Point3) -> Vector3 {
    Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z)
}

fn point_sum_difference(first: Point3, second: Point3, subtract: Point3) -> Point3 {
    Point3::new(
        first.x + second.x - subtract.x,
        first.y + second.y - subtract.y,
        first.z + second.z - subtract.z,
    )
}

/// Snap edge parameter ranges that overshoot their B-spline carrier's knot
/// domain by floating-point noise back onto the domain boundary. Native edge
/// ranges and cache knot vectors are stored independently and can disagree in
/// their last few bits; a genuine domain violation is left for validation.
pub(super) fn clamp_edge_ranges_to_carrier_domains(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "ASM edge domain storage")?;
    let domains: HashMap<&str, [f64; 2]> = storage.with_storage(|| {
        ctx.collect_hash_map(
            ctx.admit_iter(&out.curves, "ASM edge domain curves")?
                .filter_map(|curve| match &curve.geometry {
                    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                        let first = nurbs.knots().get(usize::try_from(nurbs.degree()).ok()?)?;
                        let last = nurbs.knots().get(nurbs.pole_count())?;
                        Some((curve.id.as_str(), [*first, *last]))
                    }
                    _ => None,
                }),
            "ASM edge carrier domains",
        )
    })?;
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(&mut out.edges);
    while source_values.len() != 0 {
        let Some(edge) = ctx.next_charged(&mut source_values, "ASM edge range clamp")? else {
            break;
        };
        let cadmpeg_ir::topology::EdgeCarrier::Bounded(curve, interval) = &mut edge.carrier else {
            continue;
        };
        let [mut start, mut end] = interval.endpoints();
        let Some([first, last]) =
            ctx.get_hash_map(&domains, curve.as_str(), "ASM edge domain lookup")?
        else {
            continue;
        };
        let tolerance = (last * 0.5 - first * 0.5).abs() * (2.0 * EPS_EDGE_DOMAIN_SNAP);
        if start < *first && *first - start <= tolerance {
            start = *first;
        }
        if end > *last && end - *last <= tolerance {
            end = *last;
        }
        *interval = cadmpeg_ir::topology::ParameterInterval::new([start, end])
            .map_err(|_| cadmpeg_core::CodecError::malformed(
                "edge param_range must be finite and ordered"))?;
    }
    Ok(())
}

pub(super) fn classify_body_kinds(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "ASM body classification storage")?;
    let mut shell_bodies = HashMap::new();
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(&out.regions);
    while source_values.len() != 0 {
        let Some(region) = ctx.next_charged(&mut source_values, "ASM body classification regions")? else {
            break;
        };
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut source_values = IntoIterator::into_iter(&region.shells);
        while source_values.len() != 0 {
            let Some(shell) = ctx.next_charged(&mut source_values, "ASM region shells")? else {
                break;
            };
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut shell_bodies, shell, &region.body, "ASM shell bodies")
            })?;
        }
    }
    let mut body_has_faces = HashSet::new();
    let mut body_has_wires = HashSet::new();
    let mut face_bodies = HashMap::new();
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(&out.shells);
    while source_values.len() != 0 {
        let Some(shell) = ctx.next_charged(&mut source_values, "ASM body classification shells")? else {
            break;
        };
        let Some(body) = ctx
            .get_hash_map(&shell_bodies, &&shell.id, "ASM body owner lookup")?
            .copied()
        else {
            continue;
        };
        if !shell.wire_edges().is_empty() || !shell.free_vertices().is_empty() {
            storage.with_storage(|| {
                ctx.insert_hash_set(&mut body_has_wires, body, "ASM bodies with wires")
            })?;
        }
        if !shell.faces().is_empty() {
            storage.with_storage(|| {
                ctx.insert_hash_set(&mut body_has_faces, body, "ASM bodies with faces")
            })?;
        }
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut source_values = IntoIterator::into_iter(shell.faces());
        while source_values.len() != 0 {
            let Some(face) = ctx.next_charged(&mut source_values, "ASM shell faces")? else {
                break;
            };
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut face_bodies, face, body, "ASM face bodies")
            })?;
        }
    }
    let mut loop_bodies = HashMap::new();
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(&out.faces);
    while source_values.len() != 0 {
        let Some(face) = ctx.next_charged(&mut source_values, "ASM body classification faces")? else {
            break;
        };
        let Some(body) = ctx
            .get_hash_map(&face_bodies, &&face.id, "ASM body owner lookup")?
            .copied()
        else {
            continue;
        };
        let (outer, inner) = match &face.loops {
            cadmpeg_ir::topology::FaceLoops::Unspecified { loops } => (None, loops),
            cadmpeg_ir::topology::FaceLoops::Classified { outer, inner } => (Some(outer), inner),
        };
        if let Some(outer) = outer {
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut loop_bodies, outer, body, "ASM loop bodies")
            })?;
        }
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut source_values = IntoIterator::into_iter(inner);
        while source_values.len() != 0 {
            let Some(loop_id) = ctx.next_charged(&mut source_values, "ASM face loops")? else {
                break;
            };
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut loop_bodies, loop_id, body, "ASM loop bodies")
            })?;
        }
    }
    let mut coedge_bodies = HashMap::new();
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(&out.loops);
    while source_values.len() != 0 {
        let Some(loop_) = ctx.next_charged(&mut source_values, "ASM body classification loops")? else {
            break;
        };
        let Some(body) = ctx
            .get_hash_map(&loop_bodies, &&loop_.id, "ASM body owner lookup")?
            .copied()
        else {
            continue;
        };
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut source_values = IntoIterator::into_iter(loop_.coedges());
        while source_values.len() != 0 {
            let Some(coedge) = ctx.next_charged(&mut source_values, "ASM loop coedges")? else {
                break;
            };
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut coedge_bodies, coedge, body, "ASM coedge bodies")
            })?;
        }
    }
    let mut edge_use_counts = HashMap::<_, std::collections::BTreeMap<&EdgeId, usize>>::new();
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(&out.coedges);
    while source_values.len() != 0 {
        let Some(coedge) = ctx.next_charged(&mut source_values, "ASM body classification coedges")? else {
            break;
        };
        if let Some(body) = ctx
            .get_hash_map(&coedge_bodies, &&coedge.id, "ASM body owner lookup")?
            .copied()
        {
            storage.with_storage(|| {
                ctx.admit_hash_map_entry(&mut edge_use_counts, &body, "ASM body edge use counts")
            })?;
            let counts = edge_use_counts.entry(body).or_default();
            storage.with_storage(|| {
                ctx.admit_btree_entry(counts, &&coedge.edge, "ASM edge use counts")
            })?;
            *counts.entry(&coedge.edge).or_default() += 1;
        }
    }
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(&mut out.bodies);
    while source_values.len() != 0 {
        let Some(body) = ctx.next_charged(&mut source_values, "ASM body classification bodies")? else {
            break;
        };
        if !ctx.contains_hash_set(&body_has_faces, &&body.id, "ASM body face membership")? {
            body.kind = cadmpeg_ir::topology::BodyKind::Wire;
            continue;
        }
        if ctx.contains_hash_set(&body_has_wires, &&body.id, "ASM body wire membership")? {
            body.kind = cadmpeg_ir::topology::BodyKind::General;
            continue;
        }
        let counts = ctx.get_hash_map(&edge_use_counts, &&body.id, "ASM body edge count lookup")?;
        body.kind = if match counts {
            Some(counts) => {
                !counts.is_empty()
                    && ctx.all_by(
                        counts,
                        |(_, count)| Ok(*count == 2),
                        "ASM closed body edge counts",
                    )?
            }
            None => false,
        } {
            cadmpeg_ir::topology::BodyKind::Solid
        } else {
            cadmpeg_ir::topology::BodyKind::Sheet
        };
    }
    Ok(())
}

#[cfg(test)]
mod analytic_surface_tests {
    use super::{collect_carrier, decode_surface};
    use crate::sab::{Record, Token};
    use std::sync::Arc;

    fn assert_carrier_limit(token: Token, operation: &str) {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let record = surface_record("plane", vec![token]);
        let error = collect_carrier(&ctx, &record)
            .err()
            .expect("resource refusal");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected resource refusal: {error:?}")
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(limit.operation, operation);
    }

    #[test]
    fn carrier_positions_refuse_collection_limit() {
        assert_carrier_limit(Token::Position([0.0; 3]), "ASM carrier positions");
    }

    #[test]
    fn carrier_vectors_refuse_collection_limit() {
        assert_carrier_limit(Token::Vector3([1.0, 0.0, 0.0]), "ASM carrier vectors");
    }

    #[test]
    fn carrier_doubles_refuse_collection_limit() {
        assert_carrier_limit(Token::Double(1.0), "ASM carrier doubles");
    }

    fn surface_record(head: &str, tokens: Vec<Token>) -> Record {
        Record {
            index: 1,
            name: format!("{head}-surface"),

            tokens: Arc::from(tokens),
            offset: 0,
            len: 0,
        }
    }

    #[test]
    fn analytic_surfaces_require_complete_serialized_frames() {
        let origin = Token::Position([0.0, 0.0, 0.0]);
        let axis = Token::Vector3([0.0, 0.0, 1.0]);

        let plane = surface_record("plane", vec![origin.clone(), axis.clone()]);
        let cone_without_major = surface_record(
            "cone",
            vec![
                origin.clone(),
                axis.clone(),
                Token::Double(1.0),
                Token::Double(0.0),
                Token::Double(1.0),
                Token::Double(7.0),
            ],
        );
        let cone_with_zero_major = surface_record(
            "cone",
            vec![
                origin.clone(),
                axis.clone(),
                Token::Vector3([0.0, 0.0, 0.0]),
                Token::Double(1.0),
            ],
        );
        let sphere = surface_record(
            "sphere",
            vec![origin.clone(), Token::Double(2.0), axis.clone()],
        );
        let torus = surface_record(
            "torus",
            vec![origin, axis, Token::Double(3.0), Token::Double(1.0)],
        );

        for record in [
            plane,
            cone_without_major,
            cone_with_zero_major,
            sphere,
            torus,
        ] {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let policy = cadmpeg_core::decode::DecodePolicy::service();
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(decode_surface(&ctx, &record).transpose().unwrap().is_none());
        }
    }
}

#[cfg(test)]
mod sense_tests {
    use super::record_reversed;
    use crate::sab::{Record, Token};

    #[test]
    fn intcurve_sense_falls_back_only_when_the_scope_has_no_boolean() {
        for (header, before_scope, expected) in [
            (Token::True, Token::Long(7), true),
            (Token::False, Token::Long(7), false),
            (Token::True, Token::False, false),
            (Token::False, Token::True, true),
        ] {
            let record = Record {
                index: 0,
                name: "intcurve".into(),
                tokens: vec![
                    Token::Ref(-1),
                    Token::Long(-1),
                    Token::Ref(-1),
                    header,
                    before_scope,
                    Token::SubtypeOpen,
                    Token::SubtypeClose,
                ]
                .into(),
                offset: 0,
                len: 0,
            };
            assert_eq!(
                record_reversed(&cadmpeg_test_support::service_decode_context(), &record).unwrap(),
                expected
            );
        }
    }
}

#[cfg(test)]
mod tests {
    mod budget;
    mod entry_refusal;
    mod source_visits;
    mod quadratic_storage;
    mod numerical_ranges;
    mod edge_clamping;
    use super::Point3;
    const SMALL_CURVED_SPINE_EXTENT: f64 = 1.0e-10;

    #[test]
    fn rational_circle_degree_reduction_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let poles = [[0.0, 0.0, 0.0, 1.0]; 4];
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "ASM rational four-arc degree reduction",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                super::reduce_homogeneous_bezier_to_quadratic(&ctx, &poles)
                    .expect("degree-four input")
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected collection refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    }
    #[test]
    fn rational_circle_degree_reduction_refuses_work() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let poles = [[0.0, 0.0, 0.0, 1.0]; 4];
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "ASM rational four-arc reduction work",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                super::reduce_homogeneous_bezier_to_quadratic(&ctx, &poles)
                    .expect("degree-four input")
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected work refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "ASM rational four-arc reduction work");
    }

    #[test]
    fn numerical_seventh_analytic_spine_rejects_relative_curvature_at_small_scale() {
        let ctx = cadmpeg_test_support::service_decode_context();
        for scale in [1.0, SMALL_CURVED_SPINE_EXTENT, 1.0e100] {
            let spine = |height| {
                cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                    &cadmpeg_test_support::service_decode_context(),
                    2,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(0.5 * scale, height * scale, 0.0),
                        Point3::new(scale, 0.0, 0.0),
                    ],
                    None,
                    false,
                )
                .expect("fixture constructor admission")
                .unwrap()
            };
            assert!(super::linear_nurbs_spine(&ctx, &spine(0.4))
                .transpose()
                .unwrap()
                .is_none());
            assert!(super::linear_nurbs_spine(&ctx, &spine(0.0))
                .transpose()
                .unwrap()
                .is_some());
        }
    }
    #[test]
    fn audit_regression_circle_recognition_ignores_knot_units() {
        use cadmpeg_ir::geometry::nurbs::NurbsCurve;

        let resource_arena = cadmpeg_core::decode::DecodeArena::new();
        let (resource_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &resource_arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .expect("test decode context");
        for scale in [1.0, 1e-13] {
            let knots = [0., 0., 0., 1., 1., 2., 2., 3., 3., 4., 4., 4.]
                .map(|knot| knot * scale)
                .to_vec();
            let poles = [
                (1., 0.),
                (1., 1.),
                (0., 1.),
                (-1., 1.),
                (-1., 0.),
                (-1., -1.),
                (0., -1.),
                (1., -1.),
                (1., 0.),
            ]
            .into_iter()
            .map(|(x, y)| Point3::new(x, y, 0.))
            .collect();
            let weights = (0..9)
                .map(|i| {
                    if i % 2 == 0 {
                        1.
                    } else {
                        std::f64::consts::FRAC_1_SQRT_2
                    }
                })
                .collect();
            let circle = NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                2,
                knots,
                poles,
                Some(weights),
                false,
            )
            .expect("fixture constructor admission")
            .unwrap();
            let (_, _, _, radius) = super::rational_four_arc_circle(&resource_ctx, &circle)
                .transpose()
                .expect("resource allocation")
                .unwrap();
            assert!((radius - 1.0).abs() <= 8.0 * f64::EPSILON);
        }
    }

    #[test]
    fn audit_regression_edge_clamping_preserves_real_domain_violation() {
        use cadmpeg_ir::geometry::nurbs::NurbsCurve;
        use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
        use cadmpeg_ir::ids::{CurveId, EdgeId, VertexId};
        use cadmpeg_ir::topology::{Edge, EdgeCarrier};

        let resource_arena = cadmpeg_core::decode::DecodeArena::new();
        let (resource_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &resource_arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .expect("test decode context");
        let id = CurveId::mint("sat:audit:curve#domain").unwrap();
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0., 0., 1e-12, 1e-12],
            vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .unwrap();
        let edge = Edge {
            id: EdgeId::mint("sat:audit:edge#domain").unwrap(),
            carrier: EdgeCarrier::new(Some(id.clone()), Some([-1e-10, 5e-13])).unwrap(),
            start: VertexId::mint("sat:audit:vertex#a").unwrap(),
            end: VertexId::mint("sat:audit:vertex#b").unwrap(),
            tolerance: None,
        };
        let mut out = super::AsmBrep {
            curves: vec![Curve {
                id,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            }],
            edges: vec![edge],
            ..Default::default()
        };
        super::clamp_edge_ranges_to_carrier_domains(&resource_ctx, &mut out).unwrap();
        assert_eq!(
            out.edges[0]
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::get),
            Some([-1e-10, 5e-13])
        );
        out.edges[0].set_param_range(Some(
            cadmpeg_ir::topology::ParameterInterval::new([-1e-23, 5e-13]).unwrap(),
        ));
        super::clamp_edge_ranges_to_carrier_domains(&resource_ctx, &mut out).unwrap();
        assert_eq!(
            out.edges[0]
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::get),
            Some([0., 5e-13])
        );
    }
}
