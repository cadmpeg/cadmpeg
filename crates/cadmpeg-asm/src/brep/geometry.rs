// SPDX-License-Identifier: Apache-2.0
//! Decode analytic surfaces and 3D curves, select edge pcurves, reverse
//! curve orientation, and recognize procedural carriers as analytic geometry.

use super::records::TolerantCoedgeExtension;
use crate::nurbs;
use crate::nurbs::proc_surface::{
    DecodedProceduralSurfaceDefinition, EmbeddedRollingBall, EmbeddedScaledCompoundLoftShape,
};
use crate::nurbs::reader::LEN_TO_MM;
use crate::sab::{Record, Token};
use cadmpeg_ir::geometry::{knots_nondecreasing, CurveGeometry, NurbsCurve, SurfaceGeometry};
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Sense;
use std::collections::{HashMap, HashSet};

use super::AsmBrep;
/// Ordered typed values pulled from a carrier record's payload.
pub(crate) struct Carrier {
    pub(crate) positions: Vec<[f64; 3]>,
    pub(crate) vectors: Vec<[f64; 3]>,
    doubles: Vec<f64>,
}

pub(crate) fn collect_carrier(rec: &Record) -> Carrier {
    let mut c = Carrier {
        positions: Vec::new(),
        vectors: Vec::new(),
        doubles: Vec::new(),
    };
    for t in rec.tokens.iter() {
        match t {
            Token::Position(p) => c.positions.push(*p),
            Token::Vector3(v) => c.vectors.push(*v),
            Token::Double(d) => c.doubles.push(*d),
            _ => {}
        }
    }
    c
}

pub(crate) fn scale_point(p: [f64; 3]) -> Point3 {
    Point3::new(p[0] * LEN_TO_MM, p[1] * LEN_TO_MM, p[2] * LEN_TO_MM)
}

pub(crate) fn norm3(v: [f64; 3]) -> f64 {
    Vector3::from(v).norm()
}

/// Return `v` normalized to unit length, or `v` unchanged if it is degenerate
/// (validation flags a degenerate direction rather than this hiding it).
pub(crate) fn unit(v: [f64; 3]) -> Vector3 {
    Vector3::from(v).unit().unwrap_or(Vector3::from(v))
}

/// Whether a record name heads an analytic surface carrier.
pub(crate) fn is_analytic_surface(head: &str) -> bool {
    matches!(head, "plane" | "cone" | "sphere" | "torus")
}

/// Whether a record name heads an analytic curve carrier.
pub(crate) fn is_analytic_curve(head: &str) -> bool {
    matches!(head, "straight" | "ellipse" | "degenerate_curve")
}

/// Decode an analytic surface carrier. Signed sphere and torus radii remain in
/// the IR because they are part of the ASM carrier semantics.
pub fn decode_surface(rec: &Record) -> Option<(SurfaceGeometry, bool)> {
    let c = collect_carrier(rec);
    let origin = *c.positions.first()?;
    match rec.head.as_str() {
        "plane" => {
            let normal = *c.vectors.first()?;
            let normal = unit(normal);
            let u_axis = unit(*c.vectors.get(1)?);
            Some((
                SurfaceGeometry::Plane {
                    origin: scale_point(origin),
                    normal,
                    u_axis,
                },
                false,
            ))
        }
        "cone" => {
            let ratio = *c.doubles.first().unwrap_or(&1.0);
            let axis = *c.vectors.first()?;
            let axis = unit(axis);
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
            let ref_direction = unit(major);
            if sine.abs() <= f64::EPSILON && ratio == 1.0 {
                Some((
                    SurfaceGeometry::Cylinder {
                        origin: scale_point(origin),
                        axis,
                        ref_direction,
                        radius,
                    },
                    cosine < 0.0,
                ))
            } else {
                // The IR cone's radius grows along `+axis`; a negative native
                // slope shrinks it, so the axis flips to compensate. The
                // outward normal is invariant under the flip; the inward
                // normal of a negative `cosine` folds into the face sense.
                let axis = if sine * cosine < 0.0 {
                    Vector3::new(-axis.x, -axis.y, -axis.z)
                } else {
                    axis
                };
                Some((
                    SurfaceGeometry::Cone {
                        origin: scale_point(origin),
                        axis,
                        ref_direction,
                        radius,
                        ratio,
                        // Recover from the stored (sine, cosine) pair. `asin(|sine|)`
                        // alone differs by 1 ULP across libm for common values such as
                        // 1/2; `atan2` matches the writer round-trip on every platform.
                        half_angle: sine.abs().atan2(cosine.abs()),
                    },
                    cosine < 0.0,
                ))
            }
        }
        "sphere" => {
            let signed = *c.doubles.first()?;
            let equator = unit(*c.vectors.first()?);
            let polar_axis = unit(*c.vectors.get(1)?);
            Some((
                SurfaceGeometry::Sphere {
                    center: scale_point(origin),
                    axis: polar_axis,
                    ref_direction: equator,
                    radius: signed * LEN_TO_MM,
                },
                false,
            ))
        }
        "torus" => {
            let axis = *c.vectors.first()?;
            let axis = unit(axis);
            let ref_direction = unit(*c.vectors.get(1)?);
            let major = *c.doubles.first()?;
            let minor = *c.doubles.get(1)?;
            Some((
                SurfaceGeometry::Torus {
                    center: scale_point(origin),
                    axis,
                    ref_direction,
                    major_radius: major * LEN_TO_MM,
                    minor_radius: minor * LEN_TO_MM,
                },
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
pub(crate) fn vertex_point_ref(record: &Record) -> Option<i64> {
    match record.chunk(4) {
        Some(Token::Long(_)) => record.ref_at(5),
        _ => record.ref_at(4),
    }
}

/// The coedge record's pcurve reference: chunk 10 after the reserved integer
/// in the modern layout, chunk 9 in the save-format 700 layout that stores
/// no reserved integer.
pub(crate) fn coedge_pcurve_ref(record: &Record) -> Option<i64> {
    match record.chunk(9) {
        Some(Token::Long(_)) => record.ref_at(10),
        _ => record.ref_at(9),
    }
}

pub(crate) fn is_vertex_record(record: &Record) -> bool {
    matches!(record.head.as_str(), "vertex" | "tvertex")
}

pub(crate) fn is_edge_record(record: &Record) -> bool {
    matches!(record.head.as_str(), "edge" | "tedge")
}

pub(crate) fn is_coedge_record(record: &Record) -> bool {
    matches!(record.head.as_str(), "coedge" | "tcoedge")
}

pub(crate) fn tolerant_coedge_extension(record: &Record) -> Option<TolerantCoedgeExtension> {
    let target = match record.chunk(13)? {
        Token::Ref(target) => (*target >= 0).then_some(*target),
        _ => return None,
    };
    match record.chunk(14)? {
        Token::Long(0) if matches!(record.chunk(15), Some(Token::Long(0))) => {
            Some(TolerantCoedgeExtension::Empty { target })
        }
        Token::Long(1) => {
            let curve_reversed = match record.chunk(15)? {
                Token::True => true,
                Token::False => false,
                _ => return None,
            };
            if !matches!(record.chunk(16), Some(Token::SubtypeOpen)) {
                return None;
            }
            // Raw index of chunk 16: the payload identifiers inside the
            // embedded scope are not chunks, and the serialized token count
            // below is defined over the value tokens alone.
            let open = record
                .tokens
                .iter()
                .enumerate()
                .filter(|(_, token)| !token.is_payload_ident())
                .nth(16)
                .map(|(index, _)| index)?;
            let mut depth = 0usize;
            let mut close = None;
            for (index, token) in record.tokens.iter().enumerate().skip(open) {
                match token {
                    Token::SubtypeOpen => depth += 1,
                    Token::SubtypeClose => {
                        depth = depth.checked_sub(1)?;
                        if depth == 0 {
                            close = Some(index);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let close = close?;
            // Suffix in chunk space: a record can end with payload
            // identifiers (`null_curve` placeholders for absent curve slots),
            // which are not fields of the extension.
            let suffix: Vec<&Token> = record
                .tokens
                .get(close + 1..)?
                .iter()
                .filter(|token| !token.is_payload_ident())
                .collect();
            let parameter_range = match suffix.as_slice() {
                [Token::False, Token::False, Token::Long(0)] => None,
                [Token::True, Token::Double(start), Token::True, Token::Double(end), Token::Long(0)]
                    if start.is_finite() && end.is_finite() =>
                {
                    Some([*start, *end])
                }
                _ => return None,
            };
            let payload_token_count = record
                .tokens
                .get(open + 1..close)?
                .iter()
                .filter(|token| !token.is_payload_ident())
                .count();
            Some(TolerantCoedgeExtension::EmbeddedCurve {
                target,
                curve_reversed,
                payload_token_count: u32::try_from(payload_token_count).ok()?,
                parameter_range,
            })
        }
        _ => None,
    }
}

/// Whether a record head belongs to the topology or geometry vocabulary.
///
/// Carrier heads remain known even when no active topology references that
/// particular record. Reachability determines transfer; it does not turn an
/// unreferenced carrier into an application/refinement record.
pub(crate) fn is_known_record_head(head: &str) -> bool {
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

pub(crate) fn is_asm_stream_delimiter(name: &str) -> bool {
    matches!(name, "Begin-of-ASM-History-Data" | "End-of-ASM-data")
}

pub(crate) fn edge_pcurve_parameter_ranges(edge: &Record) -> Option<[[f64; 2]; 2]> {
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
/// carrier's knot domain. The full knot domain is the final fallback.
pub(crate) fn pcurve_ranges_on_domain(
    candidate: &nurbs::pcurve::NurbsPcurve,
    edge: Option<&Record>,
) -> Option<Vec<[f64; 2]>> {
    let (&first, &last) = (candidate.knots.first()?, candidate.knots.last()?);
    let tolerance = 1.0e-9 * (last - first).abs().max(1.0);
    let mut ranges = edge
        .and_then(edge_pcurve_parameter_ranges)
        .into_iter()
        .flatten()
        .filter_map(|mut range| {
            if range
                .iter()
                .all(|value| *value >= first - tolerance && *value <= last + tolerance)
            {
                for value in &mut range {
                    if *value < first {
                        *value = first;
                    } else if *value > last {
                        *value = last;
                    }
                }
                Some(range)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if !ranges.contains(&[first, last]) {
        ranges.push([first, last]);
    }
    Some(ranges)
}

/// Decode an analytic curve carrier.
pub fn decode_curve(rec: &Record) -> Option<CurveGeometry> {
    let carrier = collect_carrier(rec);
    let base = *carrier.positions.first()?;
    match rec.head.as_str() {
        "straight" => Some(CurveGeometry::Line {
            origin: scale_point(base),
            direction: unit(*carrier.vectors.first()?),
        }),
        "ellipse" => {
            let axis = *carrier.vectors.first()?;
            let reference = *carrier.vectors.get(1)?;
            let ratio = *carrier.doubles.first()?;
            let major_radius = norm3(reference) * LEN_TO_MM;
            if (ratio.abs() - 1.0).abs() <= f64::EPSILON {
                Some(CurveGeometry::Circle {
                    center: scale_point(base),
                    axis: unit(axis),
                    ref_direction: unit(reference),
                    radius: major_radius,
                })
            } else {
                Some(CurveGeometry::Ellipse {
                    center: scale_point(base),
                    axis: unit(axis),
                    major_direction: unit(reference),
                    major_radius,
                    minor_radius: major_radius * ratio.abs(),
                })
            }
        }
        "degenerate_curve" => Some(CurveGeometry::Degenerate {
            point: scale_point(base),
        }),
        _ => None,
    }
}

pub(crate) fn sense_at(rec: &Record, i: usize) -> Sense {
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
pub(crate) fn record_reversed(rec: &Record) -> bool {
    // Adjacency in chunk space: a freestanding payload identifier (e.g. the
    // embedded curve's type name) can sit between value tokens without
    // separating the sense bit from the scope it precedes.
    let chunks: Vec<&Token> = rec.chunks().collect();
    chunks
        .windows(2)
        .find_map(|tokens| {
            matches!(tokens[1], Token::SubtypeOpen).then(|| match tokens[0] {
                Token::True => true,
                Token::False => false,
                _ => false,
            })
        })
        .or_else(|| {
            // A plain `intcurve` companion has no subtype scope after its
            // base header; its sense remains the fourth payload value.
            (rec.head == "intcurve").then(|| matches!(rec.chunk(3), Some(Token::True)))
        })
        .unwrap_or(false)
}

/// Reparameterize a cached B-spline to its record's reversed sense,
/// `C'(t) = C(-t)`, by reversing poles and weights and negating reversed knots.
pub fn reverse_nurbs_curve(curve: &mut NurbsCurve) {
    curve.control_points.reverse();
    if let Some(weights) = curve.weights.as_mut() {
        weights.reverse();
    }
    curve.knots.reverse();
    for knot in &mut curve.knots {
        *knot = -*knot;
    }
}

/// Reparameterize a referenced pcurve to its opposite orientation, preserving
/// its UV chart while negating the parameterization.
pub(crate) fn reverse_nurbs_pcurve(curve: &mut nurbs::pcurve::NurbsPcurve) {
    curve.control_points.reverse();
    if let Some(weights) = curve.weights.as_mut() {
        weights.reverse();
    }
    curve.knots.reverse();
    for knot in &mut curve.knots {
        *knot = -*knot;
    }
}

/// Reverse a curve carrier to its opposite orientation, `C'(t) = C(-t)`.
/// Lines negate their direction, conics negate their plane normal (flipping
/// the angular sweep while keeping the zero-angle direction), and B-splines
/// reverse poles and knots. Carriers without an orientation pass through.
pub(crate) fn reverse_curve_geometry(geometry: &mut CurveGeometry) {
    match geometry {
        CurveGeometry::Line { direction, .. } => {
            *direction = Vector3::new(-direction.x, -direction.y, -direction.z);
        }
        CurveGeometry::Circle { axis, .. } | CurveGeometry::Ellipse { axis, .. } => {
            *axis = Vector3::new(-axis.x, -axis.y, -axis.z);
        }
        CurveGeometry::Nurbs(curve) => reverse_nurbs_curve(curve),
        _ => {}
    }
}

pub(crate) fn reverse_procedural_curve_definition(
    definition: &mut cadmpeg_ir::geometry::ProceduralCurveDefinition,
) {
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
        angle_range,
        minor,
        pitch,
        apex_factor,
        ..
    } = definition
    {
        *angle_range = [-angle_range[1], -angle_range[0]];
        *minor = Vector3::new(-minor.x, -minor.y, -minor.z);
        *pitch = Vector3::new(-pitch.x, -pitch.y, -pitch.z);
        *apex_factor = -*apex_factor;
    }
}

pub(crate) fn double_at(rec: &Record, i: usize) -> Option<f64> {
    match rec.chunk(i) {
        Some(Token::Double(d)) => Some(*d),
        _ => None,
    }
}

pub(crate) fn pcurve_parameter_range(rec: &Record) -> Option<[f64; 2]> {
    // The final two value tokens; a record may end with payload identifiers
    // (e.g. `null_curve` placeholders), which are not fields.
    let mut values = rec.chunks().rev();
    match (values.next(), values.next()) {
        (Some(Token::Double(end)), Some(Token::Double(start))) => Some([*start, *end]),
        _ => None,
    }
}

pub(crate) fn pcurve_inline_tail_flags(rec: &Record) -> Option<[bool; 4]> {
    if !matches!(rec.chunk(3), Some(Token::Long(0))) {
        return None;
    }
    // End-relative in chunk space: the four booleans precede the final two
    // value tokens, and trailing payload identifiers are not fields.
    let chunks: Vec<&Token> = rec.chunks().collect();
    let end = chunks.len().checked_sub(2)?;
    let flags = chunks.get(end.checked_sub(4)?..end)?;
    flags
        .iter()
        .map(|token| match token {
            Token::True => Some(true),
            Token::False => Some(false),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

pub(crate) fn procedural_surface_definition_is_exact_carrier(
    definition: &DecodedProceduralSurfaceDefinition,
) -> bool {
    match definition {
        DecodedProceduralSurfaceDefinition::Extrusion { .. }
        | DecodedProceduralSurfaceDefinition::Helix(_)
        | DecodedProceduralSurfaceDefinition::Ruled { .. }
        | DecodedProceduralSurfaceDefinition::Sum { .. }
        | DecodedProceduralSurfaceDefinition::VertexBlend(_)
        | DecodedProceduralSurfaceDefinition::SubSurface { .. } => true,
        DecodedProceduralSurfaceDefinition::Sweep(construction) => construction
            .revision_form
            .as_ref()
            .is_some_and(|form| form.tail_enum == 2),
        DecodedProceduralSurfaceDefinition::Law(construction) => !matches!(
            construction.tail,
            cadmpeg_ir::geometry::LawSurfaceTail::Full
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
        } => construction.tail_enum == 2,
        DecodedProceduralSurfaceDefinition::VariableBlend(construction) => {
            construction.tail_enum == 2 || construction.shape_prefix == 0
        }
        _ => false,
    }
}

pub(crate) fn analytic_procedural_surface(
    definition: &DecodedProceduralSurfaceDefinition,
) -> Option<SurfaceGeometry> {
    match definition {
        DecodedProceduralSurfaceDefinition::Extrusion {
            directrix,
            direction,
            ..
        } => {
            let (center, normal, ref_direction, radius) = rational_four_arc_circle(directrix)?;
            let axis = direction.unit()?;
            if 1.0 - axis.dot(normal).abs() > 1.0e-10 {
                return None;
            }
            Some(SurfaceGeometry::Cylinder {
                origin: center,
                axis,
                ref_direction,
                radius,
            })
        }
        DecodedProceduralSurfaceDefinition::Blend {
            supports,
            spine: Some(spine),
            radius: cadmpeg_ir::geometry::BlendRadiusLaw::Constant { signed_radius },
            cross_section: cadmpeg_ir::geometry::BlendCrossSection::Circular,
            native,
        } => analytic_rolling_ball_surface(supports, native.as_deref(), spine, *signed_radius),
        _ => None,
    }
}

fn analytic_rolling_ball_surface(
    supports: &[Option<SurfaceGeometry>; 2],
    native: Option<&EmbeddedRollingBall>,
    spine: &cadmpeg_ir::geometry::NurbsCurve,
    signed_radius: f64,
) -> Option<SurfaceGeometry> {
    let radius = signed_radius.abs();
    if !radius.is_finite() || radius <= f64::EPSILON {
        return None;
    }
    let support = |index: usize| {
        supports[index]
            .as_ref()
            .or_else(|| native.and_then(|native| native.sides[index].surface.as_ref()))
    };
    let first = support(0)?;
    let second = support(1)?;

    if let (
        SurfaceGeometry::Plane {
            origin: first_origin,
            normal: first_normal,
            ..
        },
        SurfaceGeometry::Plane {
            origin: second_origin,
            normal: second_normal,
            ..
        },
    ) = (first, second)
    {
        let (origin, axis) = linear_nurbs_spine(spine)?;
        let tolerance = 1.0e-10
            * radius
                .max(point_vector(*first_origin, *second_origin).norm())
                .max(1.0);
        let first_normal = first_normal.unit()?;
        let second_normal = second_normal.unit()?;
        let support_intersection = first_normal.cross(second_normal);
        let support_intersection_norm = support_intersection.norm();
        if support_intersection_norm <= 1.0e-10
            || 1.0
                - axis
                    .dot(support_intersection.scale(1.0 / support_intersection_norm))
                    .abs()
                > 1.0e-10
        {
            return None;
        }
        for (plane_origin, plane_normal) in [
            (*first_origin, first_normal),
            (*second_origin, second_normal),
        ] {
            if axis.dot(plane_normal).abs() > 1.0e-10
                || (point_vector(plane_origin, origin).dot(plane_normal).abs() - radius).abs()
                    > tolerance
            {
                return None;
            }
        }
        return Some(SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis),
            radius,
        });
    }

    let ((plane @ SurfaceGeometry::Plane { .. }, cylinder @ SurfaceGeometry::Cylinder { .. })
    | (cylinder @ SurfaceGeometry::Cylinder { .. }, plane @ SurfaceGeometry::Plane { .. })) =
        (first, second)
    else {
        return None;
    };
    let (center, axis, ref_direction, major_radius) = rational_four_arc_circle(spine)?;
    let SurfaceGeometry::Plane {
        origin: plane_origin,
        normal: plane_normal,
        ..
    } = plane
    else {
        unreachable!()
    };
    let SurfaceGeometry::Cylinder {
        origin: cylinder_origin,
        axis: cylinder_axis,
        radius: cylinder_radius,
        ..
    } = cylinder
    else {
        unreachable!()
    };
    let plane_normal = plane_normal.unit()?;
    let cylinder_axis = cylinder_axis.unit()?;
    let scale = major_radius.max(radius).max(cylinder_radius.abs()).max(1.0);
    let tolerance = 1.0e-10 * scale;
    let center_offset = point_vector(*cylinder_origin, center);
    let axial_offset = center_offset.dot(cylinder_axis);
    let radial_offset = center_offset - cylinder_axis.scale(axial_offset);
    if 1.0 - axis.dot(plane_normal).abs() > 1.0e-10
        || 1.0 - axis.dot(cylinder_axis).abs() > 1.0e-10
        || (point_vector(*plane_origin, center).dot(plane_normal).abs() - radius).abs() > tolerance
        || radial_offset.norm() > tolerance
        || ((major_radius - cylinder_radius.abs()).abs() - radius).abs() > tolerance
    {
        return None;
    }
    Some(SurfaceGeometry::Torus {
        center,
        axis,
        ref_direction,
        major_radius,
        minor_radius: signed_radius,
    })
}

fn linear_nurbs_spine(curve: &cadmpeg_ir::geometry::NurbsCurve) -> Option<(Point3, Vector3)> {
    if curve.degree == 0
        || curve.periodic
        || curve.control_points.len() <= curve.degree as usize
        || curve.knots.len() != curve.control_points.len() + curve.degree as usize + 1
        || curve.knots.iter().any(|knot| !knot.is_finite())
        || !knots_nondecreasing(&curve.knots)
        || curve
            .control_points
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
    {
        return None;
    }
    if let Some(weights) = curve.weights.as_deref() {
        let first_sign = weights.first()?.signum();
        if first_sign == 0.0
            || weights.len() != curve.control_points.len()
            || weights
                .iter()
                .any(|weight| !weight.is_finite() || weight.signum() != first_sign)
        {
            return None;
        }
    }
    let origin = curve.control_points[0];
    let (_, farthest) = curve
        .control_points
        .iter()
        .copied()
        .map(|point| (point_vector(origin, point).norm(), point))
        .max_by(|left, right| left.0.total_cmp(&right.0))?;
    let extent = point_vector(origin, farthest).norm();
    if !extent.is_finite() || extent <= f64::EPSILON {
        return None;
    }
    let axis = point_vector(origin, farthest).unit()?;
    let tolerance = 1.0e-10 * extent.max(1.0);
    if curve
        .control_points
        .iter()
        .any(|point| axis.cross(point_vector(origin, *point)).norm() > tolerance)
    {
        return None;
    }
    Some((origin, axis))
}

pub(crate) fn rational_four_arc_circle(
    curve: &cadmpeg_ir::geometry::NurbsCurve,
) -> Option<(Point3, Vector3, Vector3, f64)> {
    let weights = curve.weights.as_deref()?;
    let degree = curve.degree as usize;
    if degree < 2
        || curve.periodic
        || curve.control_points.len() != 4 * degree + 1
        || weights.len() != curve.control_points.len()
        || curve.knots.len() != curve.control_points.len() + degree + 1
        || curve.knots.iter().any(|knot| !knot.is_finite())
    {
        return None;
    }
    let knot_tolerance = 1.0e-12
        * (curve.knots[curve.knots.len() - 1] - curve.knots[0])
            .abs()
            .max(1.0);
    let spans = [
        curve.knots[0],
        curve.knots[degree + 1],
        curve.knots[2 * degree + 1],
        curve.knots[3 * degree + 1],
        curve.knots[4 * degree + 1],
    ];
    if spans
        .windows(2)
        .any(|pair| !pair[0].is_finite() || pair[1] - pair[0] <= knot_tolerance)
        || (0..5).any(|span| {
            let range = if span == 0 {
                0..degree + 1
            } else if span == 4 {
                4 * degree + 1..curve.knots.len()
            } else {
                span * degree + 1..(span + 1) * degree + 1
            };
            curve.knots[range]
                .iter()
                .any(|value| (*value - spans[span]).abs() > knot_tolerance)
        })
    {
        return None;
    }
    let homogeneous = curve
        .control_points
        .iter()
        .zip(weights)
        .map(|(point, weight)| {
            let homogeneous = [
                point.x * weight,
                point.y * weight,
                point.z * weight,
                *weight,
            ];
            (point.x.is_finite()
                && point.y.is_finite()
                && point.z.is_finite()
                && weight.is_finite()
                && *weight != 0.0
                && homogeneous.iter().all(|value| value.is_finite()))
            .then_some(homogeneous)
        })
        .collect::<Option<Vec<_>>>()?;
    let quadratics = (0..4)
        .map(|span| {
            reduce_homogeneous_bezier_to_quadratic(
                homogeneous[span * degree..=span * degree + degree].to_vec(),
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let base_weight = quadratics[0][0][3];
    let weight_scale = base_weight.abs().max(1.0);
    let weight_tolerance = 1.0e-10 * weight_scale;
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
    let quadratic_points = quadratics
        .iter()
        .map(|span| {
            span.map(|point| {
                Point3::new(
                    point[0] / point[3],
                    point[1] / point[3],
                    point[2] / point[3],
                )
            })
        })
        .collect::<Vec<_>>();
    let point_distance = |left: Point3, right: Point3| point_vector(left, right).norm();
    let scale = quadratic_points
        .iter()
        .flat_map(|span| span.windows(2))
        .map(|pair| point_distance(pair[0], pair[1]))
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let tolerance = 1.0e-10 * scale;
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
        if (radial.norm() - radius).abs() > tolerance || radial.dot(next).abs() > tolerance * radius
        {
            return None;
        }
        let span_normal = radial.cross(next);
        let span_normal_norm = span_normal.norm();
        if span_normal_norm <= tolerance * radius {
            return None;
        }
        let span_normal = span_normal.scale(1.0 / span_normal_norm);
        if normal.is_some_and(|normal: Vector3| normal.dot(span_normal) < 1.0 - 1.0e-10) {
            return None;
        }
        normal.get_or_insert(span_normal);
    }
    Some((
        first_center,
        normal?,
        first_radial.scale(1.0 / radius),
        radius,
    ))
}

fn reduce_homogeneous_bezier_to_quadratic(mut control: Vec<[f64; 4]>) -> Option<[[f64; 4]; 3]> {
    while control.len() > 3 {
        let degree = control.len() - 1;
        let mut reduced = Vec::with_capacity(degree);
        reduced.push(control[0]);
        for index in 1..degree {
            let alpha = index as f64 / degree as f64;
            let denominator = 1.0 - alpha;
            reduced.push(std::array::from_fn(|coordinate| {
                (control[index][coordinate] - alpha * reduced[index - 1][coordinate]) / denominator
            }));
        }
        if reduced.iter().flatten().any(|value| !value.is_finite()) {
            return None;
        }
        let scale = control
            .iter()
            .flatten()
            .fold(1.0_f64, |scale, value| scale.max(value.abs()));
        if (0..4).any(|coordinate| {
            (reduced[degree - 1][coordinate] - control[degree][coordinate]).abs() > 1.0e-10 * scale
        }) {
            return None;
        }
        control = reduced;
    }
    control.try_into().ok()
}

pub(crate) fn point_vector(origin: Point3, point: Point3) -> Vector3 {
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
pub(crate) fn clamp_edge_ranges_to_carrier_domains(out: &mut AsmBrep) {
    let domains: HashMap<&str, [f64; 2]> = out
        .curves
        .iter()
        .filter_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => {
                let (first, last) = (nurbs.knots.first()?, nurbs.knots.last()?);
                Some((curve.id.0.as_str(), [*first, *last]))
            }
            _ => None,
        })
        .collect();
    for edge in &mut out.edges {
        let Some([start, end]) = edge.param_range.as_mut() else {
            continue;
        };
        let Some([first, last]) = edge
            .curve
            .as_ref()
            .and_then(|curve| domains.get(curve.0.as_str()))
        else {
            continue;
        };
        let tolerance = 1.0e-9 * (last - first).abs().max(1.0);
        if *start < *first && *first - *start <= tolerance {
            *start = *first;
        }
        if *end > *last && *end - *last <= tolerance {
            *end = *last;
        }
    }
}

pub(crate) fn classify_body_kinds(out: &mut AsmBrep) {
    let mut shell_bodies = HashMap::new();
    for region in &out.regions {
        for shell in &region.shells {
            shell_bodies.insert(shell.clone(), region.body.clone());
        }
    }
    let mut body_has_faces = HashSet::new();
    let mut body_has_wires = HashSet::new();
    let mut face_bodies = HashMap::new();
    for shell in &out.shells {
        let Some(body) = shell_bodies.get(&shell.id) else {
            continue;
        };
        if !shell.wire_edges.is_empty() || !shell.free_vertices.is_empty() {
            body_has_wires.insert(body.clone());
        }
        if !shell.faces.is_empty() {
            body_has_faces.insert(body.clone());
        }
        for face in &shell.faces {
            face_bodies.insert(face.clone(), body.clone());
        }
    }
    let mut loop_bodies = HashMap::new();
    for face in &out.faces {
        let Some(body) = face_bodies.get(&face.id) else {
            continue;
        };
        for loop_id in &face.loops {
            loop_bodies.insert(loop_id.clone(), body.clone());
        }
    }
    let mut coedge_bodies = HashMap::new();
    for loop_ in &out.loops {
        let Some(body) = loop_bodies.get(&loop_.id) else {
            continue;
        };
        for coedge in &loop_.coedges {
            coedge_bodies.insert(coedge.clone(), body.clone());
        }
    }
    let mut edge_use_counts = HashMap::<_, HashMap<EdgeId, usize>>::new();
    for coedge in &out.coedges {
        if let Some(body) = coedge_bodies.get(&coedge.id) {
            *edge_use_counts
                .entry(body.clone())
                .or_default()
                .entry(coedge.edge.clone())
                .or_default() += 1;
        }
    }
    for body in &mut out.bodies {
        if !body_has_faces.contains(&body.id) {
            body.kind = cadmpeg_ir::topology::BodyKind::Wire;
            continue;
        }
        if body_has_wires.contains(&body.id) {
            body.kind = cadmpeg_ir::topology::BodyKind::General;
            continue;
        }
        let counts = edge_use_counts.get(&body.id);
        body.kind = if counts
            .is_some_and(|counts| !counts.is_empty() && counts.values().all(|count| *count == 2))
        {
            cadmpeg_ir::topology::BodyKind::Solid
        } else {
            cadmpeg_ir::topology::BodyKind::Sheet
        };
    }
}

#[cfg(test)]
mod analytic_surface_tests {
    use super::*;
    use std::sync::Arc;

    fn surface_record(head: &str, tokens: Vec<Token>) -> Record {
        Record {
            index: 1,
            name: format!("{head}-surface"),
            head: head.into(),
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
            assert!(decode_surface(&record).is_none());
        }
    }
}
