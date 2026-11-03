// SPDX-License-Identifier: Apache-2.0
//! The `NurbsPatch` schema (spec §14).

use crate::schema::{index, integer, integers, reals, token, vector, vectors3, Issues};
use crate::stage::Prim;

/// One trim curve in the parameter space of its patch (§14.1).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrimCurve {
    pub(crate) degree: u32,
    pub(crate) knots: Vec<f64>,
    /// Parameter-space positions `(u, v)`.
    pub(crate) points: Vec<[f64; 2]>,
    pub(crate) weights: Vec<f64>,
    pub(crate) range: Option<[f64; 2]>,
}

/// A patch read in scene units, its grid in rows of constant `u` index.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PatchData {
    pub(crate) u_degree: u32,
    pub(crate) v_degree: u32,
    pub(crate) u_knots: Vec<f64>,
    pub(crate) v_knots: Vec<f64>,
    /// `rows[i][j]` is the control point at U index `i` and V index `j`.
    pub(crate) rows: Vec<Vec<[f64; 3]>>,
    pub(crate) weights: Option<Vec<Vec<f64>>>,
    pub(crate) left_handed: bool,
    /// An authored range is narrower than the knot domain.
    pub(crate) range_narrowed: bool,
    /// Trim loops, each a list of curves.
    pub(crate) trims: Vec<Vec<TrimCurve>>,
    /// The trim attributes do not describe their curves.
    pub(crate) trims_malformed: bool,
}

fn count(prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Option<usize> {
    integer(prim, name, issues).and_then(index)
}

/// Reads a `NurbsPatch` prim, or returns `None` when its counts and arrays
/// disagree.
pub(crate) fn read(prim: &Prim<'_>, issues: &mut Issues) -> Option<PatchData> {
    let u_count = count(prim, "uVertexCount", issues)?;
    let v_count = count(prim, "vVertexCount", issues)?;
    let u_order = count(prim, "uOrder", issues).filter(|order| *order >= 1 && *order <= u_count)?;
    let v_order = count(prim, "vOrder", issues).filter(|order| *order >= 1 && *order <= v_count)?;
    let points = vectors3(prim, "points", issues)?;
    if points.len() != u_count.checked_mul(v_count)? {
        return None;
    }
    let u_knots =
        reals(prim, "uKnots", 1, issues).filter(|knots| knots.len() == u_count + u_order)?;
    let v_knots =
        reals(prim, "vKnots", 1, issues).filter(|knots| knots.len() == v_count + v_order)?;
    let weights = reals(prim, "pointWeights", 1, issues);
    let weights = match weights {
        Some(weights) if weights.len() == points.len() => Some(weights),
        Some(_) => return None,
        None => None,
    };
    let rows: Vec<Vec<[f64; 3]>> = (0..u_count)
        .map(|u| (0..v_count).map(|v| points[v * u_count + u]).collect())
        .collect();
    let weights = weights.map(|weights| {
        (0..u_count)
            .map(|u| (0..v_count).map(|v| weights[v * u_count + u]).collect())
            .collect()
    });
    let mut range_narrowed = false;
    for (name, knots, order, vertex_count) in [
        ("uRange", &u_knots, u_order, u_count),
        ("vRange", &v_knots, v_order, v_count),
    ] {
        if let Some(range) = vector(prim, name, 2, issues) {
            let domain = [knots[order - 1], knots[vertex_count]];
            range_narrowed |= range.as_slice() != domain.as_slice();
        }
    }
    let (trims, trims_malformed) = trims(prim, issues);
    Some(PatchData {
        u_degree: u32::try_from(u_order - 1).ok()?,
        v_degree: u32::try_from(v_order - 1).ok()?,
        u_knots,
        v_knots,
        rows,
        weights,
        left_handed: token(prim, "orientation", issues).as_deref() == Some("leftHanded"),
        range_narrowed,
        trims,
        trims_malformed,
    })
}

/// The trim loops of a patch and whether the trim attributes are malformed.
fn trims(prim: &Prim<'_>, issues: &mut Issues) -> (Vec<Vec<TrimCurve>>, bool) {
    let Some(loop_counts) = integers(prim, "trimCurve:counts", issues) else {
        return (Vec::new(), false);
    };
    let parsed = (|| {
        let orders = integers(prim, "trimCurve:orders", issues)?;
        let vertex_counts = integers(prim, "trimCurve:vertexCounts", issues)?;
        let knots = reals(prim, "trimCurve:knots", 1, issues)?;
        let ranges = reals(prim, "trimCurve:ranges", 2, issues);
        let points = vectors3(prim, "trimCurve:points", issues)?;
        let mut curve = 0_usize;
        let mut knot_start = 0_usize;
        let mut point_start = 0_usize;
        let mut loops = Vec::new();
        for loop_count in &loop_counts {
            let mut curves = Vec::new();
            for _ in 0..index(*loop_count)? {
                let order = orders.get(curve).copied().and_then(index)?;
                let count = vertex_counts.get(curve).copied().and_then(index)?;
                if order == 0 || order > count {
                    return None;
                }
                let knot_end = knot_start.checked_add(count + order)?;
                let point_end = point_start.checked_add(count)?;
                let homogeneous = points.get(point_start..point_end)?;
                let mut positions = Vec::new();
                let mut weights = Vec::new();
                for [x, y, w] in homogeneous {
                    if *w == 0.0 {
                        return None;
                    }
                    positions.push([x / w, y / w]);
                    weights.push(*w);
                }
                let curve_knots = knots.get(knot_start..knot_end)?.to_vec();
                let range = ranges
                    .as_ref()
                    .and_then(|ranges| ranges.get(2 * curve..2 * curve + 2))
                    .map(|range| [range[0], range[1]]);
                curves.push(TrimCurve {
                    degree: u32::try_from(order - 1).ok()?,
                    knots: curve_knots,
                    points: positions,
                    weights,
                    range,
                });
                curve += 1;
                knot_start = knot_end;
                point_start = point_end;
            }
            loops.push(curves);
        }
        Some(loops)
    })();
    match parsed {
        Some(loops) => (loops, false),
        None => (Vec::new(), true),
    }
}

#[cfg(test)]
mod tests;
