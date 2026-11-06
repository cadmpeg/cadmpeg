// SPDX-License-Identifier: Apache-2.0
//! Geometry of the exact edge revisions selected by matching attributes.

use std::collections::HashMap;

use cadmpeg_asm::{
    brep::geometry::record_reversed,
    nurbs::{
        proc_curve::{procedural_curve_resolving_refs, ProceduralCurveConstruction},
        toks::SubtypeTable,
    },
    sab::{Record, Token},
};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    eval::{
        decode::{curve_tangent, nurbs_curve_point_at},
        finite_or_refusal, nurbs_curve_parameter_domain,
    },
    features::FinitePoint3,
    geometry::{CurveGeometry, SolvedCurveGeometry},
    math::{Point3, Vector3},
    units::UnitVector3,
};

use super::{generated, matched, EdgeName, Selector};

const CM_TO_MM: f64 = 10.0;
const EPS_EDGE_REVISION_POINT_MM: f64 = 1.0e-5;
/// Unit length of the ellipse axis and its orthogonality to the major axis.
const EPS_ELLIPSE_FRAME: f64 = 1.0e-6;
/// Slack in radians when an edge interval is tested against the stored
/// parameter bounds of its ellipse.
const EPS_ELLIPSE_RANGE: f64 = 1.0e-9;

#[derive(Debug, Clone, PartialEq)]
pub(in crate::feature) struct EdgeGeometry {
    pub(in crate::feature) start: FinitePoint3,
    pub(in crate::feature) end: FinitePoint3,
    pub(in crate::feature) interior: FinitePoint3,
    pub(in crate::feature) tangent: UnitVector3,
}

fn position(record: &Record) -> Option<FinitePoint3> {
    if record.head() != "point" {
        return None;
    }
    let Token::Position([x, y, z]) = record.chunk(3)? else {
        return None;
    };
    FinitePoint3::new(Point3::new(x * CM_TO_MM, y * CM_TO_MM, z * CM_TO_MM))
}

fn endpoints(
    edge: &Record,
    records: &HashMap<i64, &Record>,
) -> Option<(FinitePoint3, FinitePoint3)> {
    if !super::is_edge(edge) {
        return None;
    }
    let endpoint = |slot| {
        let vertex = records.get(&edge.ref_at(slot)?)?;
        if !matches!(vertex.head(), "vertex" | "tvertex") {
            return None;
        }
        position(records.get(&vertex.ref_at(5)?)?)
    };
    Some((endpoint(3)?, endpoint(5)?))
}

/// Distance in millimetres within which a carrier endpoint equals the vertex
/// at edge slot `slot`: the kernel linear tolerance, widened to the stored
/// tolerance of a tolerant vertex. That tolerance is the vertex's last
/// tolerance slot, in centimetres, when it is set (positive).
fn vertex_tolerance(edge: &Record, records: &HashMap<i64, &Record>, slot: usize) -> f64 {
    let stored = (|| {
        let vertex = records.get(&edge.ref_at(slot)?)?;
        if vertex.head() != "tvertex" {
            return None;
        }
        let Token::Double(tolerance) = vertex.chunk(8)? else {
            return None;
        };
        (tolerance.is_finite() && *tolerance > 0.0).then_some(tolerance * CM_TO_MM)
    })();
    stored.map_or(EPS_EDGE_REVISION_POINT_MM, |tolerance| {
        tolerance.max(EPS_EDGE_REVISION_POINT_MM)
    })
}

/// Geometry of a stored edge revision on a straight, elliptical, or spline
/// carrier. Only a decode-session resource refusal is an error.
pub(super) fn revision(
    ctx: &DecodeContext<'_>,
    edge: &Record,
    records: &HashMap<i64, &Record>,
    subtypes: &SubtypeTable,
) -> Result<Option<EdgeGeometry>, CodecError> {
    let Some(carrier) = edge.ref_at(8).and_then(|index| records.get(&index)) else {
        return Ok(None);
    };
    match carrier.head() {
        "straight" => Ok(line(edge, records)),
        "ellipse" => Ok(ellipse(edge, records)),
        "intcurve" => spline(ctx, edge, records, subtypes),
        _ => Ok(None),
    }
}

/// The optional parameter bounds of an `ellipse` carrier. After the ratio the
/// carrier stores two optional range endpoints, each a false Boolean (absent)
/// or a true Boolean followed by a double. A carrier that stores nothing after
/// the ratio has no bounds. Stored bounds are ordered.
fn ellipse_range(curve: &Record) -> Option<(Option<f64>, Option<f64>)> {
    let mut slot = 7;
    let mut ends = [None, None];
    if curve.chunk_len() == slot {
        return Some((None, None));
    }
    for end in &mut ends {
        match curve.chunk(slot)? {
            Token::False => slot += 1,
            Token::True => {
                let Token::Double(value) = curve.chunk(slot + 1)? else {
                    return None;
                };
                if !value.is_finite() {
                    return None;
                }
                *end = Some(*value);
                slot += 2;
            }
            _ => return None,
        }
    }
    if slot != curve.chunk_len() || matches!(ends, [Some(low), Some(high)] if low > high) {
        return None;
    }
    Some((ends[0], ends[1]))
}

/// Geometry of an edge revision on an `intcurve` carrier whose stored
/// B-spline is exact. The carrier point at edge parameter `t` is the stored
/// B-spline (polynomial or rational) at `s * t`, where `s` is -1 when exactly
/// one of the carrier sense and the edge sense is reversed and +1 otherwise.
/// The scaled first and last parameters must lie in the knot domain, and each
/// carrier point must equal its stored vertex. A carrier with a procedural
/// construction other than the exact cache, a nonzero fit tolerance, a
/// periodic spline, and an edge whose vertices coincide are refused: the
/// vertices of a closed edge do not fix the sense of its parameters.
fn spline(
    ctx: &DecodeContext<'_>,
    edge: &Record,
    records: &HashMap<i64, &Record>,
    subtypes: &SubtypeTable,
) -> Result<Option<EdgeGeometry>, CodecError> {
    let Some(((start, end), carrier)) =
        endpoints(edge, records).zip(edge.ref_at(8).and_then(|index| records.get(&index)))
    else {
        return Ok(None);
    };
    let near = |a: FinitePoint3, b: FinitePoint3, tolerance: f64| {
        a.as_raw().distance(*b.as_raw()) <= tolerance
    };
    if near(start, end, EPS_EDGE_REVISION_POINT_MM) || carrier.head() != "intcurve" {
        return Ok(None);
    }
    let decoded = match procedural_curve_resolving_refs(ctx, &carrier.tokens, subtypes) {
        Some(Ok(decoded)) => decoded,
        Some(Err(error @ CodecError::ResourceLimit(_))) => return Err(error),
        Some(Err(_)) | None => return Ok(None),
    };
    if !matches!(decoded.construction, ProceduralCurveConstruction::Exact)
        || decoded.cache_fit_tolerance.is_some_and(|fit| fit != 0.0)
        || decoded.curve.periodic()
    {
        return Ok(None);
    }
    let edge_reversed = match edge.chunk(9) {
        Some(Token::False) => false,
        Some(Token::True) => true,
        _ => return Ok(None),
    };
    let sign = if record_reversed(carrier) == edge_reversed {
        1.0
    } else {
        -1.0
    };
    let (Some(Token::Double(first)), Some(Token::Double(last))) = (edge.chunk(4), edge.chunk(6))
    else {
        return Ok(None);
    };
    if !(first.is_finite() && last.is_finite() && first < last) {
        return Ok(None);
    }
    let Some([low, high]) = nurbs_curve_parameter_domain(&decoded.curve)
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        return Ok(None);
    };
    let within = |u: f64| (low..=high).contains(&u);
    let (u_first, u_last) = (sign * first, sign * last);
    if !(within(u_first) && within(u_last)) {
        return Ok(None);
    }
    let at = |u: f64| finite_or_refusal(nurbs_curve_point_at(ctx, &decoded.curve, u));
    let (Some(first_point), Some(last_point)) = (at(u_first)?, at(u_last)?) else {
        return Ok(None);
    };
    if !near(start, first_point, vertex_tolerance(edge, records, 3))
        || !near(end, last_point, vertex_tolerance(edge, records, 5))
    {
        return Ok(None);
    }
    let u_middle = sign * first.midpoint(*last);
    let carrier_geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(decoded.curve.clone()));
    let (Some(interior), Some(tangent)) = (
        at(u_middle)?,
        finite_or_refusal(curve_tangent(ctx, &carrier_geometry, u_middle))?,
    ) else {
        return Ok(None);
    };
    let direction = tangent.as_raw();
    Ok(UnitVector3::normalized(Vector3::new(
        sign * direction.x,
        sign * direction.y,
        sign * direction.z,
    ))
    .map(|tangent| EdgeGeometry {
        start,
        end,
        interior,
        tangent,
    }))
}

/// Geometry of an edge revision on an ellipse carrier. The carrier point at
/// parameter `u` is `center + major * cos(u) + ratio * (normal x major) * sin(u)`;
/// the edge sense negates the stored edge parameters. Each parameter endpoint
/// must equal its stored vertex. The edge interval, on the carrier
/// parameter, lies within the carrier bounds. A closed edge on a reversed carrier is
/// refused: its vertices do not fix the sense of its parameters.
fn ellipse(edge: &Record, records: &HashMap<i64, &Record>) -> Option<EdgeGeometry> {
    let (start, end) = endpoints(edge, records)?;
    let curve = records.get(&edge.ref_at(8)?)?;
    let (Token::Position([cx, cy, cz]), Token::Vector3([nx, ny, nz]), Token::Vector3([ax, ay, az])) =
        (curve.chunk(3)?, curve.chunk(4)?, curve.chunk(5)?)
    else {
        return None;
    };
    let Token::Double(ratio) = curve.chunk(6)? else {
        return None;
    };
    let (low, high) = ellipse_range(curve)?;
    let center = Vector3::new(*cx, *cy, *cz);
    let normal = Vector3::new(*nx, *ny, *nz);
    let major = Vector3::new(*ax, *ay, *az);
    let major_length = major.norm();
    if !ratio.is_finite()
        || *ratio == 0.0
        || !(major_length.is_finite() && major_length > 0.0)
        || (normal.norm() - 1.0).abs() > EPS_ELLIPSE_FRAME
        || (normal.dot(major) / major_length).abs() > EPS_ELLIPSE_FRAME
    {
        return None;
    }
    let minor = normal.cross(major).scale(*ratio);
    let sign = match edge.chunk(9)? {
        Token::False => 1.0,
        Token::True => -1.0,
        _ => return None,
    };
    let (Token::Double(first), Token::Double(last)) = (edge.chunk(4)?, edge.chunk(6)?) else {
        return None;
    };
    let span = last - first;
    if !(first.is_finite() && last.is_finite())
        || span <= 0.0
        || span > std::f64::consts::TAU + EPS_ELLIPSE_FRAME
    {
        return None;
    }
    let closed = start.as_raw().distance(*end.as_raw()) <= EPS_EDGE_REVISION_POINT_MM;
    if closed && sign < 0.0 {
        return None;
    }
    let (u_low, u_high) = if sign < 0.0 {
        (-last, -first)
    } else {
        (*first, *last)
    };
    if low.is_some_and(|low| u_low < low - EPS_ELLIPSE_RANGE)
        || high.is_some_and(|high| u_high > high + EPS_ELLIPSE_RANGE)
    {
        return None;
    }
    let at = |t: f64| {
        let u = sign * t;
        let offset = major.scale(u.cos()) + minor.scale(u.sin());
        FinitePoint3::new(Point3::new(
            (center.x + offset.x) * CM_TO_MM,
            (center.y + offset.y) * CM_TO_MM,
            (center.z + offset.z) * CM_TO_MM,
        ))
    };
    let near = |a: FinitePoint3, b: FinitePoint3, tolerance: f64| {
        a.as_raw().distance(*b.as_raw()) <= tolerance
    };
    if !near(start, at(*first)?, vertex_tolerance(edge, records, 3))
        || !near(end, at(*last)?, vertex_tolerance(edge, records, 5))
    {
        return None;
    }
    let middle = first.midpoint(*last);
    let u = sign * middle;
    let tangent = (major.scale(-u.sin()) + minor.scale(u.cos())).scale(sign);
    Some(EdgeGeometry {
        start,
        end,
        interior: at(middle)?,
        tangent: UnitVector3::normalized(tangent)?,
    })
}

pub(super) fn line(edge: &Record, records: &HashMap<i64, &Record>) -> Option<EdgeGeometry> {
    let (start, end) = endpoints(edge, records)?;
    let curve = records.get(&edge.ref_at(8)?)?;
    if curve.head() != "straight" {
        return None;
    }
    let Token::Position([x, y, z]) = curve.chunk(3)? else {
        return None;
    };
    let Token::Vector3([dx, dy, dz]) = curve.chunk(4)? else {
        return None;
    };
    let direction = Vector3::new(*dx, *dy, *dz);
    let sign = match edge.chunk(9)? {
        Token::False => 1.0,
        Token::True => -1.0,
        _ => return None,
    };
    let (Token::Double(first), Token::Double(last)) = (edge.chunk(4)?, edge.chunk(6)?) else {
        return None;
    };
    if first == last {
        return None;
    }
    let at = |t: f64| {
        FinitePoint3::new(Point3::new(
            (x + sign * t * dx) * CM_TO_MM,
            (y + sign * t * dy) * CM_TO_MM,
            (z + sign * t * dz) * CM_TO_MM,
        ))
    };
    let near = |a: FinitePoint3, b: FinitePoint3, tolerance: f64| {
        a.as_raw().distance(*b.as_raw()) <= tolerance
    };
    if !near(start, at(*first)?, vertex_tolerance(edge, records, 3))
        || !near(end, at(*last)?, vertex_tolerance(edge, records, 5))
    {
        return None;
    }
    Some(EdgeGeometry {
        start,
        end,
        interior: at(first.midpoint(*last))?,
        tangent: UnitVector3::normalized(direction.scale(sign))?,
    })
}

pub(super) fn selected(
    records: &[Record],
    selectors: &[Selector],
    names: &HashMap<u32, Option<EdgeName>>,
    schema: u32,
) -> HashMap<u32, EdgeGeometry> {
    let by_index = records
        .iter()
        .filter_map(|r| Some((i64::try_from(r.index).ok()?, r)))
        .collect::<HashMap<_, _>>();
    let mut matches = HashMap::<(u32, u32), std::collections::HashSet<i64>>::new();
    let mut generated_names = HashMap::<i64, std::collections::HashSet<EdgeName>>::new();
    for record in records {
        if let Some((source, key, owner)) = matched(record, schema) {
            matches.entry((source, key)).or_default().insert(owner);
        }
        if let Some((owner, name)) = generated(record, schema) {
            generated_names.entry(owner).or_default().insert(name);
        }
    }
    let mut out = HashMap::new();
    for selector in selectors {
        let Some(Some(name)) = names.get(&selector.source) else {
            continue;
        };
        let Some(owners) = matches.get(&(selector.source, selector.key)) else {
            continue;
        };
        let samples = owners
            .iter()
            .map(|owner| {
                let generated = generated_names.get(owner)?;
                if generated.len() != 1 || !generated.contains(name) {
                    return None;
                }
                line(by_index.get(owner)?, &by_index)
            })
            .collect::<Option<Vec<_>>>();
        let Some(samples) = samples else {
            continue;
        };
        if let Some(first) = samples.first() {
            if samples.iter().all(|sample| sample == first) {
                out.insert(selector.source, first.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
