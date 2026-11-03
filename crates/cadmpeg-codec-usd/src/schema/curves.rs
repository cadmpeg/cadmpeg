// SPDX-License-Identifier: Apache-2.0
//! The `BasisCurves` and `NurbsCurves` schemas (spec §13) as NURBS curves.
//!
//! Every basis curve has an exact NURBS form. A linear curve is degree 1. A
//! cubic Bezier curve is degree 3 with triple interior knots. A cubic
//! B-spline curve is a uniform B-spline. A Catmull-Rom segment is the Bezier
//! segment with the same end points and end tangents. A periodic curve is
//! unrolled into an open curve that repeats its first points; a pinned curve
//! carries its phantom points.

use crate::schema::{index, integers, reals, token, vectors3, Issues};
use crate::stage::Prim;

/// One curve as NURBS data, in scene units.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurveData {
    pub(crate) degree: u32,
    pub(crate) knots: Vec<f64>,
    pub(crate) points: Vec<[f64; 3]>,
    pub(crate) weights: Option<Vec<f64>>,
    /// The parameter range, when narrower than the knot domain.
    pub(crate) range: Option<[f64; 2]>,
}

/// The curves of a batch and the number of curves that were rejected.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Curves {
    pub(crate) curves: Vec<CurveData>,
    pub(crate) rejected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Basis {
    Bezier,
    Bspline,
    CatmullRom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wrap {
    Nonperiodic,
    Periodic,
    Pinned,
}

/// Uniform knots `0, 1, …, count - 1`.
fn uniform_knots(count: usize) -> Vec<f64> {
    (0..count)
        .map(|knot| cadmpeg_core::convert::f64_from_index(knot).unwrap_or(f64::NAN))
        .collect()
}

/// A clamped knot vector for `segments` segments of `degree`, with knots of
/// multiplicity `degree` at each interior segment joint.
fn bezier_knots(segments: usize, degree: usize) -> Vec<f64> {
    let mut knots = Vec::new();
    for segment in 0..=segments {
        let value = cadmpeg_core::convert::f64_from_index(segment).unwrap_or(f64::NAN);
        let multiplicity = if segment == 0 || segment == segments {
            degree + 1
        } else {
            degree
        };
        knots.extend((0..multiplicity).map(|_| value));
    }
    knots
}

fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn sub(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale(vector: [f64; 3], factor: f64) -> [f64; 3] {
    vector.map(|component| component * factor)
}

/// `points` with the pinned phantom points (§13.1).
fn pinned(points: &[[f64; 3]]) -> Option<Vec<[f64; 3]>> {
    let first = *points.first()?;
    let second = *points.get(1)?;
    let last = *points.last()?;
    let before = *points.get(points.len().checked_sub(2)?)?;
    let mut out = vec![sub(scale(first, 2.0), second)];
    out.extend_from_slice(points);
    out.push(sub(scale(last, 2.0), before));
    Some(out)
}

/// One basis curve as NURBS data, or `None` when its vertex count does not
/// satisfy its type and wrap.
fn basis_curve(points: &[[f64; 3]], cubic: Option<Basis>, wrap: Wrap) -> Option<CurveData> {
    let count = points.len();
    let Some(basis) = cubic else {
        let mut points = points.to_vec();
        match wrap {
            Wrap::Periodic if count >= 3 => points.push(points[0]),
            Wrap::Nonperiodic | Wrap::Pinned if count >= 2 => {}
            _ => return None,
        }
        let mut knots = vec![0.0];
        knots.extend(uniform_knots(points.len()));
        knots.push(*knots.last()?);
        return Some(CurveData {
            degree: 1,
            knots,
            points,
            weights: None,
            range: None,
        });
    };
    let wrap = if basis == Basis::Bezier && wrap == Wrap::Pinned {
        Wrap::Nonperiodic
    } else {
        wrap
    };
    let points: Vec<[f64; 3]> = match wrap {
        Wrap::Nonperiodic => points.to_vec(),
        Wrap::Pinned => pinned(points)?,
        Wrap::Periodic => {
            let repeat = if basis == Basis::Bezier { 1 } else { 3 };
            let mut out = points.to_vec();
            out.extend(points.iter().cycle().take(repeat).copied());
            out
        }
    };
    match basis {
        Basis::Bezier => {
            let count = points.len();
            if count < 4 || !(count - 1).is_multiple_of(3) {
                return None;
            }
            let knots = bezier_knots((count - 1) / 3, 3);
            Some(CurveData {
                degree: 3,
                knots,
                points,
                weights: None,
                range: None,
            })
        }
        Basis::Bspline => {
            if points.len() < 4 {
                return None;
            }
            let knots = uniform_knots(points.len() + 4);
            Some(CurveData {
                degree: 3,
                knots,
                points,
                weights: None,
                range: None,
            })
        }
        Basis::CatmullRom => {
            if points.len() < 4 {
                return None;
            }
            let mut out = vec![points[1]];
            for window in points.windows(4) {
                let [p0, p1, p2, p3] = [window[0], window[1], window[2], window[3]];
                out.push(add(p1, scale(sub(p2, p0), 1.0 / 6.0)));
                out.push(sub(p2, scale(sub(p3, p1), 1.0 / 6.0)));
                out.push(p2);
            }
            let segments = points.len() - 3;
            Some(CurveData {
                degree: 3,
                knots: bezier_knots(segments, 3),
                points: out,
                weights: None,
                range: None,
            })
        }
    }
}

/// The vertex counts and points of a curve batch, split per curve.
fn split<'p>(
    prim: &Prim<'_>,
    points: &'p [[f64; 3]],
    issues: &mut Issues,
) -> Option<Vec<Option<&'p [[f64; 3]]>>> {
    let counts = integers(prim, "curveVertexCounts", issues)?;
    let mut start = 0_usize;
    Some(
        counts
            .iter()
            .map(|count| {
                let end = start.checked_add(index(*count)?)?;
                let slice = points.get(start..end);
                start = end;
                slice
            })
            .collect(),
    )
}

/// Reads a `BasisCurves` prim.
pub(crate) fn basis_curves(prim: &Prim<'_>, issues: &mut Issues) -> Option<Curves> {
    let points = vectors3(prim, "points", issues)?;
    let cubic = match token(prim, "type", issues).as_deref() {
        Some("linear") => None,
        None | Some("cubic") => Some(match token(prim, "basis", issues).as_deref() {
            None | Some("bezier") => Basis::Bezier,
            Some("bspline") => Basis::Bspline,
            Some("catmullRom") => Basis::CatmullRom,
            Some(_) => {
                return Some(Curves {
                    curves: Vec::new(),
                    rejected: 1,
                })
            }
        }),
        Some(_) => {
            return Some(Curves {
                curves: Vec::new(),
                rejected: 1,
            })
        }
    };
    let wrap = match token(prim, "wrap", issues).as_deref() {
        None | Some("nonperiodic") => Wrap::Nonperiodic,
        Some("periodic") => Wrap::Periodic,
        Some("pinned") => Wrap::Pinned,
        Some(_) => {
            return Some(Curves {
                curves: Vec::new(),
                rejected: 1,
            })
        }
    };
    let mut out = Curves::default();
    for slice in split(prim, &points, issues)? {
        match slice.and_then(|points| basis_curve(points, cubic, wrap)) {
            Some(curve) => out.curves.push(curve),
            None => out.rejected += 1,
        }
    }
    Some(out)
}

/// Reads a `NurbsCurves` prim.
pub(crate) fn nurbs_curves(prim: &Prim<'_>, issues: &mut Issues) -> Option<Curves> {
    let points = vectors3(prim, "points", issues)?;
    let orders = integers(prim, "order", issues)?;
    let knots = reals(prim, "knots", 1, issues)?;
    let ranges = reals(prim, "ranges", 2, issues);
    let weights =
        reals(prim, "pointWeights", 1, issues).filter(|weights| weights.len() == points.len());
    let mut out = Curves::default();
    let mut point_start = 0_usize;
    let mut knot_start = 0_usize;
    for (curve, slice) in split(prim, &points, issues)?.into_iter().enumerate() {
        let Some(slice) = slice else {
            out.rejected += 1;
            continue;
        };
        let count = slice.len();
        let order = orders.get(curve).copied().and_then(index);
        let knot_end = order.and_then(|order| knot_start.checked_add(count.checked_add(order)?));
        let parsed = order
            .filter(|order| *order >= 1 && *order <= count)
            .zip(knot_end)
            .and_then(|(order, knot_end)| {
                let curve_knots = knots.get(knot_start..knot_end)?.to_vec();
                let curve_weights = weights
                    .as_ref()
                    .and_then(|weights| weights.get(point_start..point_start.checked_add(count)?))
                    .map(<[f64]>::to_vec);
                let domain = [*curve_knots.get(order - 1)?, *curve_knots.get(count)?];
                let range = ranges
                    .as_ref()
                    .and_then(|ranges| ranges.get(2 * curve..2 * curve + 2))
                    .map(|range| [range[0], range[1]])
                    .filter(|range| *range != domain);
                Some(CurveData {
                    degree: u32::try_from(order - 1).ok()?,
                    knots: curve_knots,
                    points: slice.to_vec(),
                    weights: curve_weights,
                    range,
                })
            });
        point_start += count;
        match knot_end {
            Some(end) => knot_start = end,
            None => {
                out.rejected += 1;
                break;
            }
        }
        match parsed {
            Some(curve) => out.curves.push(curve),
            None => out.rejected += 1,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests;
