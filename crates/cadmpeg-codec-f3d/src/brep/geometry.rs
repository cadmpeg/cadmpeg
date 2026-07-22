// SPDX-License-Identifier: Apache-2.0
//! Decode analytic surfaces and 3D curves, select edge pcurves, reverse
//! curve orientation, and recognize procedural carriers as analytic geometry.

use crate::nurbs;
use crate::nurbs::proc_surface::{
    DecodedProceduralSurfaceDefinition, EmbeddedRollingBall, EmbeddedScaledCompoundLoftShape,
};
use crate::nurbs::reader::LEN_TO_MM;
use crate::records::TolerantCoedgeExtension;
use crate::sab::{Record, Token};
use cadmpeg_ir::eval;
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, SurfaceGeometry};
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Sense;
use std::collections::{HashMap, HashSet};

use super::Brep;
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

fn vec3(v: [f64; 3]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}

pub(crate) fn norm3(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Return `v` normalized to unit length, or `v` unchanged if it is degenerate
/// (validation flags a degenerate direction rather than this hiding it).
pub(crate) fn unit(v: [f64; 3]) -> Vector3 {
    let n = norm3(v);
    if n > f64::EPSILON {
        Vector3::new(v[0] / n, v[1] / n, v[2] / n)
    } else {
        vec3(v)
    }
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
pub(crate) fn decode_surface(rec: &Record) -> Option<(SurfaceGeometry, bool)> {
    let c = collect_carrier(rec);
    let origin = *c.positions.first()?;
    match rec.head.as_str() {
        "plane" => {
            let normal = *c.vectors.first()?;
            let normal = unit(normal);
            let u_axis = c
                .vectors
                .get(1)
                .map_or_else(|| deterministic_ref_direction(normal), |axis| unit(*axis));
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
            let major = c.vectors.get(1).copied();
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
            let u_scale = c.doubles.get(3).copied();
            let radius = major
                .map(|vector| norm3(vector) * LEN_TO_MM)
                .filter(|radius| *radius > f64::EPSILON)
                .or_else(|| u_scale.map(|r| r * LEN_TO_MM))?;
            let ref_direction = major.map_or_else(|| deterministic_ref_direction(axis), unit);
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
                        half_angle: sine.abs().asin(),
                    },
                    cosine < 0.0,
                ))
            }
        }
        "sphere" => {
            let signed = *c.doubles.first()?;
            let polar_axis = c.vectors.get(1).or_else(|| c.vectors.first()).copied()?;
            let polar_axis = unit(polar_axis);
            let equator = c
                .vectors
                .first()
                .filter(|_| c.vectors.len() > 1)
                .map_or_else(
                    || deterministic_ref_direction(polar_axis),
                    |direction| unit(*direction),
                );
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
            let ref_direction = c.vectors.get(1).map_or_else(
                || deterministic_ref_direction(axis),
                |direction| unit(*direction),
            );
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

fn deterministic_ref_direction(axis: Vector3) -> Vector3 {
    let candidates = [
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ];
    let basis = candidates
        .into_iter()
        .min_by(|a, b| {
            let a_dot = (a.x * axis.x + a.y * axis.y + a.z * axis.z).abs();
            let b_dot = (b.x * axis.x + b.y * axis.y + b.z * axis.z).abs();
            a_dot.total_cmp(&b_dot)
        })
        .expect("fixed candidate set is non-empty");
    let dot = basis.x * axis.x + basis.y * axis.y + basis.z * axis.z;
    let projected = Vector3::new(
        basis.x - dot * axis.x,
        basis.y - dot * axis.y,
        basis.z - dot * axis.z,
    );
    let length = projected.norm();
    Vector3::new(
        projected.x / length,
        projected.y / length,
        projected.z / length,
    )
}

/// Maximum distance, in millimeters, between a pcurve endpoint mapped through
/// the face surface and the edge's vertex position for the pcurve to count as
/// that surface's parameter-space image.
const PCURVE_ENDPOINT_TOLERANCE_MM: f64 = 0.01;

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
            let mut depth = 0usize;
            let mut close = None;
            for (index, token) in record.tokens.iter().enumerate().skip(16) {
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
            let parameter_range = match record.tokens.get(close + 1..) {
                Some([Token::False, Token::False, Token::Long(0)]) => None,
                Some(
                    [Token::True, Token::Double(start), Token::True, Token::Double(end), Token::Long(0)],
                ) if start.is_finite() && end.is_finite() => Some([*start, *end]),
                _ => return None,
            };
            Some(TolerantCoedgeExtension::EmbeddedCurve {
                target,
                curve_reversed,
                payload_token_count: u32::try_from(close.checked_sub(17)?).ok()?,
                parameter_range,
            })
        }
        _ => None,
    }
}

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
}

pub(crate) fn is_asm_stream_delimiter(name: &str) -> bool {
    matches!(name, "Begin-of-ASM-History-Data" | "End-of-ASM-data")
}

/// The millimeter-space position of an edge-record vertex reference.
fn vertex_position(by_index: &HashMap<i64, &Record>, vertex: i64) -> Option<Point3> {
    let vertex_record = by_index.get(&vertex).filter(|r| is_vertex_record(r))?;
    let point_record = by_index.get(&vertex_record.ref_at(5)?)?;
    collect_carrier(point_record)
        .positions
        .first()
        .map(|p| scale_point(*p))
}

pub(crate) fn distance(a: Point3, b: Point3) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
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

/// Select the candidate 2D block that is the face surface's parameter-space
/// image of the coedge: its endpoints, mapped through the surface, land on the
/// owning edge's vertex positions. On a non-NURBS surface, or when the edge's
/// vertex positions cannot be read, the first candidate passes unverified. An
/// empty result means no candidate is the surface's image of this edge.
pub(crate) fn select_face_pcurve(
    candidates: Vec<nurbs::pcurve::NurbsPcurve>,
    surface: Option<&SurfaceGeometry>,
    exact_procedural_parameterization: bool,
    edge: Option<&Record>,
    by_index: &HashMap<i64, &Record>,
) -> Option<(nurbs::pcurve::NurbsPcurve, [f64; 2])> {
    // Procedural surfaces retain an evaluated NURBS cache, but their pcurves
    // are expressed on the exact construction's parameterization. Evaluating
    // those UVs on the cache can drift between knots and is not a valid
    // candidate test. Candidate discovery orders unambiguous BS2 carriers
    // before ambiguous 3D interpretations, so the first candidate is the
    // authoritative exact-space carrier in this case.
    if exact_procedural_parameterization {
        let candidate = candidates.into_iter().next()?;
        let range = pcurve_ranges_on_domain(&candidate, edge)?[0];
        return Some((candidate, range));
    }
    let Some(SurfaceGeometry::Nurbs(surface)) = surface else {
        let candidate = candidates.into_iter().next()?;
        let range = pcurve_ranges_on_domain(&candidate, edge)?[0];
        return Some((candidate, range));
    };
    let vertex_pair = edge.and_then(|edge| {
        Some((
            vertex_position(by_index, edge.ref_at(3)?)?,
            vertex_position(by_index, edge.ref_at(5)?)?,
        ))
    });
    let Some((start, end)) = vertex_pair else {
        let candidate = candidates.into_iter().next()?;
        let range = pcurve_ranges_on_domain(&candidate, edge)?[0];
        return Some((candidate, range));
    };
    let mut best: Option<(f64, nurbs::pcurve::NurbsPcurve, [f64; 2])> = None;
    for candidate in candidates {
        let uv_at = |t: f64| {
            eval::nurbs_pcurve_uv(
                candidate.degree,
                &candidate.knots,
                &candidate.control_points,
                candidate.weights.as_deref(),
                t,
            )
        };
        let Some(parameter_ranges) = pcurve_ranges_on_domain(&candidate, edge) else {
            continue;
        };
        let Some((mismatch, range)) = parameter_ranges
            .into_iter()
            .filter_map(|[first, second]| {
                let (uv0, uv1) = (uv_at(first)?, uv_at(second)?);
                let (p0, p1) = (
                    eval::nurbs_surface_point(surface, uv0.u, uv0.v)?,
                    eval::nurbs_surface_point(surface, uv1.u, uv1.v)?,
                );
                // Coedge sense can reverse traversal independently of the
                // edge carrier, so accept either endpoint assignment.
                let forward = distance(p0, start).max(distance(p1, end));
                let reversed = distance(p0, end).max(distance(p1, start));
                Some((forward.min(reversed), [first, second]))
            })
            .min_by(|(left, _), (right, _)| left.total_cmp(right))
        else {
            continue;
        };
        if mismatch <= PCURVE_ENDPOINT_TOLERANCE_MM
            && best
                .as_ref()
                .is_none_or(|(current, _, _)| mismatch < *current)
        {
            best = Some((mismatch, candidate, range));
        }
    }
    best.map(|(_, candidate, range)| (candidate, range))
}

/// Decode an analytic curve carrier.
pub(crate) fn decode_curve(rec: &Record) -> Option<CurveGeometry> {
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
/// token immediately before the record's subtype scope ([spec §7.6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#76-intcurve-and-spline-subtypes)). `true`
/// marks geometry as the reverse of its cached definition. A reversed intcurve
/// negates the cache parameterization (`C(t) = cache(-t)`), and a reversed
/// spline surface flips the cache normal.
pub(crate) fn record_reversed(rec: &Record) -> bool {
    rec.tokens
        .windows(2)
        .find_map(|tokens| {
            matches!(tokens[1], Token::SubtypeOpen).then(|| match tokens[0] {
                Token::True => true,
                Token::False => false,
                _ => false,
            })
        })
        .unwrap_or(false)
}

/// Reparameterize a cached B-spline to its record's reversed sense,
/// `C'(t) = C(-t)`, by reversing poles and weights and negating reversed knots.
pub(crate) fn reverse_nurbs_curve(curve: &mut NurbsCurve) {
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
    match &*rec.tokens {
        [.., Token::Double(start), Token::Double(end)] => Some([*start, *end]),
        _ => None,
    }
}

pub(crate) fn pcurve_inline_tail_flags(rec: &Record) -> Option<[bool; 4]> {
    if !matches!(rec.chunk(3), Some(Token::Long(0))) {
        return None;
    }
    let end = rec.tokens.len().checked_sub(2)?;
    let flags = rec.tokens.get(end.checked_sub(4)?..end)?;
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
        DecodedProceduralSurfaceDefinition::Law(construction) => !matches!(
            construction.tail,
            cadmpeg_ir::geometry::LawSurfaceTail::Full
        ),
        DecodedProceduralSurfaceDefinition::ScaledCompoundLoft(construction) => matches!(
            construction.shape,
            EmbeddedScaledCompoundLoftShape::None { .. }
        ),
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
            let axis = normalized_vector(*direction)?;
            if 1.0 - dot_vector(axis, normal).abs() > 1.0e-10 {
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
        let first_normal = normalized_vector(*first_normal)?;
        let second_normal = normalized_vector(*second_normal)?;
        let support_intersection = cross_vector(first_normal, second_normal);
        let support_intersection_norm = support_intersection.norm();
        if support_intersection_norm <= 1.0e-10
            || 1.0
                - dot_vector(
                    axis,
                    Vector3::new(
                        support_intersection.x / support_intersection_norm,
                        support_intersection.y / support_intersection_norm,
                        support_intersection.z / support_intersection_norm,
                    ),
                )
                .abs()
                > 1.0e-10
        {
            return None;
        }
        for (plane_origin, plane_normal) in [
            (*first_origin, first_normal),
            (*second_origin, second_normal),
        ] {
            if dot_vector(axis, plane_normal).abs() > 1.0e-10
                || (dot_vector(point_vector(plane_origin, origin), plane_normal).abs() - radius)
                    .abs()
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
    let plane_normal = normalized_vector(*plane_normal)?;
    let cylinder_axis = normalized_vector(*cylinder_axis)?;
    let scale = major_radius.max(radius).max(cylinder_radius.abs()).max(1.0);
    let tolerance = 1.0e-10 * scale;
    let center_offset = point_vector(*cylinder_origin, center);
    let axial_offset = dot_vector(center_offset, cylinder_axis);
    let radial_offset = Vector3::new(
        center_offset.x - axial_offset * cylinder_axis.x,
        center_offset.y - axial_offset * cylinder_axis.y,
        center_offset.z - axial_offset * cylinder_axis.z,
    );
    if 1.0 - dot_vector(axis, plane_normal).abs() > 1.0e-10
        || 1.0 - dot_vector(axis, cylinder_axis).abs() > 1.0e-10
        || (dot_vector(point_vector(*plane_origin, center), plane_normal).abs() - radius).abs()
            > tolerance
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
        || curve.knots.windows(2).any(|pair| pair[0] > pair[1])
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
    let axis = normalized_vector(point_vector(origin, farthest))?;
    let tolerance = 1.0e-10 * extent.max(1.0);
    if curve
        .control_points
        .iter()
        .any(|point| cross_vector(axis, point_vector(origin, *point)).norm() > tolerance)
    {
        return None;
    }
    Some((origin, axis))
}

fn normalized_vector(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    if !norm.is_finite() || norm <= f64::EPSILON {
        return None;
    }
    Some(Vector3::new(
        vector.x / norm,
        vector.y / norm,
        vector.z / norm,
    ))
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
        if (radial.norm() - radius).abs() > tolerance
            || dot_vector(radial, next).abs() > tolerance * radius
        {
            return None;
        }
        let span_normal = cross_vector(radial, next);
        let span_normal_norm = span_normal.norm();
        if span_normal_norm <= tolerance * radius {
            return None;
        }
        let span_normal = Vector3::new(
            span_normal.x / span_normal_norm,
            span_normal.y / span_normal_norm,
            span_normal.z / span_normal_norm,
        );
        if normal.is_some_and(|normal: Vector3| dot_vector(normal, span_normal) < 1.0 - 1.0e-10) {
            return None;
        }
        normal.get_or_insert(span_normal);
    }
    Some((
        first_center,
        normal?,
        Vector3::new(
            first_radial.x / radius,
            first_radial.y / radius,
            first_radial.z / radius,
        ),
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

fn dot_vector(first: Vector3, second: Vector3) -> f64 {
    first.x * second.x + first.y * second.y + first.z * second.z
}

fn cross_vector(first: Vector3, second: Vector3) -> Vector3 {
    Vector3::new(
        first.y * second.z - first.z * second.y,
        first.z * second.x - first.x * second.z,
        first.x * second.y - first.y * second.x,
    )
}

/// Snap edge parameter ranges that overshoot their B-spline carrier's knot
/// domain by floating-point noise back onto the domain boundary. Native edge
/// ranges and cache knot vectors are stored independently and can disagree in
/// their last few bits; a genuine domain violation is left for validation.
pub(crate) fn clamp_edge_ranges_to_carrier_domains(out: &mut Brep) {
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

pub(crate) fn classify_body_kinds(out: &mut Brep) {
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
