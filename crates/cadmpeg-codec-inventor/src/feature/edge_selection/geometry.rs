// SPDX-License-Identifier: Apache-2.0
//! Geometry of the exact edge revisions selected by matching attributes.

use std::collections::HashMap;

use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_ir::{
    features::FinitePoint3,
    math::{Point3, Vector3},
    units::UnitVector3,
};

use super::{generated, matched, EdgeName, Selector};

const CM_TO_MM: f64 = 10.0;
const EPS_EDGE_REVISION_POINT_MM: f64 = 1.0e-5;
/// Unit length of the ellipse axis and its orthogonality to the major axis.
const EPS_ELLIPSE_FRAME: f64 = 1.0e-6;

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

/// Geometry of a stored edge revision on a straight or elliptical carrier.
pub(super) fn revision(edge: &Record, records: &HashMap<i64, &Record>) -> Option<EdgeGeometry> {
    match records.get(&edge.ref_at(8)?)?.head() {
        "straight" => line(edge, records),
        "ellipse" => ellipse(edge, records),
        _ => None,
    }
}

/// Geometry of an edge revision on an ellipse carrier. The carrier point at
/// parameter `u` is `center + major * cos(u) + ratio * (normal x major) * sin(u)`;
/// the edge sense negates the stored edge parameters. Each parameter endpoint
/// must equal its stored vertex. A closed edge on a reversed carrier is
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
    match (curve.chunk(7), curve.chunk(8), curve.chunk_len()) {
        (None, None, 7) | (Some(Token::False), Some(Token::False), 9) => {}
        _ => return None,
    }
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
    let at = |t: f64| {
        let u = sign * t;
        let offset = major.scale(u.cos()) + minor.scale(u.sin());
        FinitePoint3::new(Point3::new(
            (center.x + offset.x) * CM_TO_MM,
            (center.y + offset.y) * CM_TO_MM,
            (center.z + offset.z) * CM_TO_MM,
        ))
    };
    let near = |a: FinitePoint3, b: FinitePoint3| {
        a.as_raw().distance(*b.as_raw()) <= EPS_EDGE_REVISION_POINT_MM
    };
    if !near(start, at(*first)?) || !near(end, at(*last)?) {
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
    let near = |a: FinitePoint3, b: FinitePoint3| {
        a.as_raw().distance(*b.as_raw()) <= EPS_EDGE_REVISION_POINT_MM
    };
    if !near(start, at(*first)?) || !near(end, at(*last)?) {
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
