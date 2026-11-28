// SPDX-License-Identifier: Apache-2.0
//! The `NurbsPatch` schema (spec §14).

use cadmpeg_ir::unknown::part::Cause;

use crate::schema::{index, integer, integers, reals, token, vector, vectors3, Issues};
use crate::stage::{Prim, Resolved};

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
    /// The authored `uRange` and `vRange`.
    pub(crate) ranges: [Option<[f64; 2]>; 2],
    /// Trim loops, each a list of curves.
    pub(crate) trims: Vec<Vec<TrimCurve>>,
    /// The trim attributes do not describe their curves.
    pub(crate) trims_malformed: bool,
}

/// The count that attribute `name` states: undecoded where the prim states
/// none, unreadable where the value is malformed or negative.
fn count(prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Result<usize, Cause> {
    let since = issues.malformed.len();
    let value = integer(prim, name, issues).ok_or_else(|| issues.cause_since(since))?;
    index(value).ok_or(Cause::Unreadable)
}

/// Reads a `NurbsPatch` prim, or returns why it states no surface: it is
/// unreadable where a value is malformed or its counts and arrays disagree,
/// and undecoded where it states no value that the surface needs.
pub(crate) fn read(prim: &Prim<'_>, issues: &mut Issues) -> Result<PatchData, Cause> {
    let u_count = count(prim, "uVertexCount", issues)?;
    let v_count = count(prim, "vVertexCount", issues)?;
    let u_order = count(prim, "uOrder", issues)?;
    let v_order = count(prim, "vOrder", issues)?;
    if !(1..=u_count).contains(&u_order) || !(1..=v_count).contains(&v_order) {
        return Err(Cause::Unreadable);
    }
    let since = issues.malformed.len();
    let points = vectors3(prim, "points", issues).ok_or_else(|| issues.cause_since(since))?;
    if Some(points.len()) != u_count.checked_mul(v_count) {
        return Err(Cause::Unreadable);
    }
    let knots = |name: &str, count: usize, order: usize, issues: &mut Issues| {
        let since = issues.malformed.len();
        let knots = reals(prim, name, 1, issues).ok_or_else(|| issues.cause_since(since))?;
        if knots.len() == count + order {
            Ok(knots)
        } else {
            Err(Cause::Unreadable)
        }
    };
    let u_knots = knots("uKnots", u_count, u_order, issues)?;
    let v_knots = knots("vKnots", v_count, v_order, issues)?;
    let weights = reals(prim, "pointWeights", 1, issues);
    let weights = match weights {
        Some(weights) if weights.len() == points.len() => Some(weights),
        Some(_) => return Err(Cause::Unreadable),
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
    let range = |name: &str, issues: &mut Issues| {
        vector(prim, name, 2, issues).map(|range| [range[0], range[1]])
    };
    let ranges = [range("uRange", issues), range("vRange", issues)];
    let (trims, trims_malformed) = trims(prim, issues);
    let degree = |order: usize| u32::try_from(order - 1).map_err(|_| Cause::Undecoded);
    Ok(PatchData {
        u_degree: degree(u_order)?,
        v_degree: degree(v_order)?,
        u_knots,
        v_knots,
        rows,
        weights,
        left_handed: token(prim, "orientation", issues).as_deref() == Some("leftHanded"),
        ranges,
        trims,
        trims_malformed,
    })
}

/// The trim loops of a patch and whether the trim attributes are malformed.
fn trims(prim: &Prim<'_>, issues: &mut Issues) -> (Vec<Vec<TrimCurve>>, bool) {
    let Some(loop_counts) = integers(prim, "trimCurve:counts", issues) else {
        return (Vec::new(), false);
    };
    let Some(loop_counts) = loop_counts
        .into_iter()
        .map(index)
        .collect::<Option<Vec<usize>>>()
    else {
        return (Vec::new(), true);
    };
    let Some(curve_count) = loop_counts
        .iter()
        .try_fold(0_usize, |sum, count| sum.checked_add(*count))
    else {
        return (Vec::new(), true);
    };
    if curve_count == 0 {
        return (Vec::new(), false);
    }
    let parsed = (|| {
        let orders = integers(prim, "trimCurve:orders", issues)
            .filter(|orders| orders.len() == curve_count)?;
        let vertex_counts = integers(prim, "trimCurve:vertexCounts", issues)
            .filter(|counts| counts.len() == curve_count)?;
        let knots = reals(prim, "trimCurve:knots", 1, issues)?;
        let ranges = match reals(prim, "trimCurve:ranges", 2, issues) {
            Some(ranges) if curve_count.checked_mul(2) == Some(ranges.len()) => Some(ranges),
            Some(_) => return None,
            None if matches!(
                prim.resolve("trimCurve:ranges"),
                Some(Resolved::Default(..) | Resolved::FirstSample(..))
            ) =>
            {
                return None
            }
            None => None,
        };
        let points = vectors3(prim, "trimCurve:points", issues)?;
        let mut curve = 0_usize;
        let mut knot_start = 0_usize;
        let mut point_start = 0_usize;
        let mut loops = Vec::new();
        for loop_count in &loop_counts {
            let mut curves = Vec::new();
            for _ in 0..*loop_count {
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
        (knot_start == knots.len() && point_start == points.len()).then_some(loops)
    })();
    match parsed {
        Some(loops) => (loops, false),
        None => (Vec::new(), true),
    }
}

#[cfg(test)]
mod tests;
