// SPDX-License-Identifier: Apache-2.0
//! The bounded faces of a `NurbsPatch` prim (spec §14): the trim loops of
//! the patch, or the four sides of its parameter range.

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_ir::math::portable::Portable;

use super::lift::lifted;
use super::nurbs::Nurbs;
use super::{
    Brep, Catalog, EdgeRecord, FaceRecord, Form, LoopRecord, NurbsCurveRecord, NurbsSurfaceRecord,
    OrientedEdgeRecord, PoleRecord, ShellRecord, DEFAULT_TOLERANCE,
};
use crate::schema::patch::{PatchData, TrimCurve};

/// The largest gap between consecutive trim curves of a loop, as a fraction
/// of the larger side of the parameter range.
const LOOP_CLOSURE: f64 = 1e-6;
/// How far a trim curve may leave the parameter range, as a fraction of the
/// larger side of the range.
const RANGE_SLACK: f64 = 1e-9;
/// Samples of each knot span of a trim curve that outline its loop.
const SPAN_SAMPLES: usize = 16;
/// Samples that compare two edge curves.
const SEAM_SAMPLES: usize = 8;
/// The most trim curves of one patch.
// resource-cap: MAX_TRIM_CURVES = 2
const MAX_TRIM_CURVES: usize = 4096;

type Uv = [f64; 2];

/// The faces of one patch.
#[derive(Debug)]
pub(crate) struct Bounded {
    /// One shell; one face for each outer trim loop, or one face bounded by
    /// the sides of the parameter range.
    pub(crate) brep: Brep,
    pub(crate) trims: Trims,
    /// An authored `uRange` or `vRange` does not lie in the knot domain.
    pub(crate) range_ignored: bool,
}

/// What became of the trim curves of a patch.
#[derive(Debug, PartialEq)]
pub(crate) enum Trims {
    Absent,
    /// The trim loops bound the faces. `region_assumed` states that an
    /// authored loop direction disagrees with its nesting depth.
    Read {
        region_assumed: bool,
    },
    /// The trim loops do not bound a face, for the reason given; the face
    /// is bounded by the sides of the parameter range.
    Dropped(String),
}

/// The surface record of `patch`.
fn surface(patch: &PatchData) -> Result<NurbsSurfaceRecord, String> {
    let u_count = patch.rows.len();
    let v_count = patch.rows.first().map_or(0, Vec::len);
    let points: Vec<[f64; 3]> = patch.rows.iter().flatten().copied().collect();
    let weights: Vec<f64> = match &patch.weights {
        Some(rows) => rows.iter().flatten().copied().collect(),
        None => points.iter().map(|_| 1.0).collect(),
    };
    let ordered = |knots: &[f64]| {
        knots.iter().all(|knot| knot.is_finite()) && knots.windows(2).all(|pair| pair[0] <= pair[1])
    };
    if points.len() != u_count * v_count
        || weights.len() != points.len()
        || !points.iter().flatten().all(|value| value.is_finite())
        || !weights
            .iter()
            .all(|weight| weight.is_finite() && *weight > 0.0)
        || !ordered(&patch.u_knots)
        || !ordered(&patch.v_knots)
    {
        return Err("the surface is not finite and ordered".to_owned());
    }
    let order = |degree: u32| usize::try_from(degree).map(|degree| degree + 1);
    Ok(NurbsSurfaceRecord {
        u_order: order(patch.u_degree).map_err(|_| "the u degree overflows")?,
        v_order: order(patch.v_degree).map_err(|_| "the v degree overflows")?,
        u_count,
        v_count,
        u_form: Form::Open,
        v_form: Form::Open,
        points,
        weights,
        u_knots: patch.u_knots.clone(),
        v_knots: patch.v_knots.clone(),
    })
}

/// A straight parameter-space segment from `a` to `b` on `[0, 1]`.
fn segment(a: Uv, b: Uv) -> Nurbs<2> {
    Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![a, b],
        weights: vec![1.0, 1.0],
    }
}

/// The sides of `range`, counterclockwise from `(u0, v0)`.
fn natural_loop([u, v]: [Uv; 2]) -> Vec<Nurbs<2>> {
    let corners = [[u[0], v[0]], [u[1], v[0]], [u[1], v[1]], [u[0], v[1]]];
    (0..4)
        .map(|side| segment(corners[side], corners[(side + 1) % 4]))
        .collect()
}

/// The larger side of `range`.
fn size([u, v]: [Uv; 2]) -> f64 {
    (u[1] - u[0]).max(v[1] - v[0])
}

/// The trim curve `curve` restricted to its range.
fn trim_curve(curve: &TrimCurve, range: [Uv; 2]) -> Result<Nurbs<2>, String> {
    let degree = usize::try_from(curve.degree).map_err(|_| "a trim degree overflows")?;
    if degree == 0 {
        return Err("a trim curve has degree zero".to_owned());
    }
    if !curve.knots.iter().all(|knot| knot.is_finite())
        || !curve.knots.windows(2).all(|pair| pair[0] <= pair[1])
    {
        return Err("the knots of a trim curve are not finite and ordered".to_owned());
    }
    if !curve.points.iter().flatten().all(|value| value.is_finite())
        || !curve
            .weights
            .iter()
            .all(|weight| weight.is_finite() && *weight > 0.0)
    {
        return Err(
            "a trim curve has a point or weight that is not finite and positive".to_owned(),
        );
    }
    let nurbs = Nurbs {
        degree,
        knots: curve.knots.clone(),
        points: curve.points.clone(),
        weights: curve.weights.clone(),
    };
    let domain = nurbs
        .domain()
        .filter(|[low, high]| low < high)
        .ok_or("a trim curve has an empty domain")?;
    let nurbs = match curve.range {
        None => nurbs,
        Some(range) if range == domain => nurbs,
        Some([low, high]) => {
            let slack = RANGE_SLACK * (domain[1] - domain[0]);
            if !(low < high && domain[0] - slack <= low && high <= domain[1] + slack) {
                return Err("a trim range does not lie in the domain of its curve".to_owned());
            }
            nurbs
                .segment(low.max(domain[0]), high.min(domain[1]))
                .ok_or("a trim range does not select a part of its curve")?
        }
    };
    let slack = RANGE_SLACK * size(range);
    let inside = samples(&nurbs).iter().all(|point| {
        (0..2).all(|axis| {
            range[axis][0] - slack <= point[axis] && point[axis] <= range[axis][1] + slack
        })
    });
    if !inside {
        return Err("a trim curve leaves the parameter range of the patch".to_owned());
    }
    Ok(nurbs)
}

/// Points along `curve` from its start, excluding its end.
fn samples(curve: &Nurbs<2>) -> Vec<Uv> {
    let Some([low, high]) = curve.domain() else {
        return Vec::new();
    };
    let Some(count) = curve
        .points
        .len()
        .checked_sub(curve.degree)
        .map(|spans| spans.max(1))
        .and_then(|spans| spans.checked_mul(SPAN_SAMPLES))
    else {
        return Vec::new();
    };
    let Some(count_value) = f64_from_index(count) else {
        return Vec::new();
    };
    (0..count)
        .filter_map(|index| {
            let t = low + (high - low) * f64_from_index(index)? / count_value;
            curve.point_at(t)
        })
        .collect()
}

fn start(curve: &Nurbs<2>) -> Option<Uv> {
    curve.point_at(curve.domain()?[0])
}

fn end(curve: &Nurbs<2>) -> Option<Uv> {
    curve.point_at(curve.domain()?[1])
}

/// The trim loops of `patch`, each a closed chain of curves.
fn trim_loops(patch: &PatchData, range: [Uv; 2]) -> Result<Vec<Vec<Nurbs<2>>>, String> {
    if patch.trims.iter().map(Vec::len).sum::<usize>() > MAX_TRIM_CURVES {
        return Err(format!(
            "the patch has more than {MAX_TRIM_CURVES} trim curves"
        ));
    }
    let closure = LOOP_CLOSURE * size(range);
    let mut loops = Vec::new();
    for (index, curves) in patch.trims.iter().enumerate() {
        let curves = curves
            .iter()
            .map(|curve| trim_curve(curve, range))
            .collect::<Result<Vec<_>, _>>()?;
        if curves.is_empty() {
            continue;
        }
        for (position, curve) in curves.iter().enumerate() {
            let next = &curves[(position + 1) % curves.len()];
            let (Some(a), Some(b)) = (end(curve), start(next)) else {
                return Err(format!("trim loop {index} has a curve with no ends"));
            };
            if (a[0] - b[0]).portable_hypot(a[1] - b[1]) > closure {
                return Err(format!("trim loop {index} does not close"));
            }
        }
        loops.push(curves);
    }
    if loops.is_empty() {
        return Err("the trim loops hold no curve".to_owned());
    }
    Ok(loops)
}

/// Twice the signed area of a closed polygon.
fn area(polygon: &[Uv]) -> f64 {
    (0..polygon.len())
        .map(|index| {
            let [a, b] = [polygon[index], polygon[(index + 1) % polygon.len()]];
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// Whether `point` is inside `polygon` by the even-odd rule.
fn inside(polygon: &[Uv], point: Uv) -> bool {
    let mut odd = false;
    for index in 0..polygon.len() {
        let [a, b] = [polygon[index], polygon[(index + 1) % polygon.len()]];
        if (a[1] > point[1]) != (b[1] > point[1]) {
            let x = a[0] + (point[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if point[0] < x {
                odd = !odd;
            }
        }
    }
    odd
}

/// Twice the signed area that the closed loop `curves` encloses in
/// parameter space: positive when the loop runs counterclockwise.
pub(crate) fn winding(curves: &[Nurbs<2>]) -> f64 {
    let polygon: Vec<Uv> = curves.iter().flat_map(samples).collect();
    area(&polygon)
}

/// `curves` traversed the other way.
pub(crate) fn reversed_loop(curves: &[Nurbs<2>]) -> Option<Vec<Nurbs<2>>> {
    curves.iter().rev().map(Nurbs::reversed).collect()
}

/// The loops of one face: the outer loop, then the inner loops.
type FaceLoops = Vec<Vec<Nurbs<2>>>;

/// The faces that `loops` bound by the even-odd rule: each loop at an even
/// nesting depth is the outer loop of a face, and each loop at an odd depth
/// an inner loop of the face of the loop that holds it. Each loop is
/// directed counterclockwise about the face normal when outer and clockwise
/// when inner. Returns whether an authored direction disagrees.
fn faces(loops: Vec<Vec<Nurbs<2>>>, same_sense: bool) -> Result<(Vec<FaceLoops>, bool), String> {
    let polygons: Vec<Vec<Uv>> = loops
        .iter()
        .map(|curves| curves.iter().flat_map(samples).collect())
        .collect();
    let areas: Vec<f64> = polygons.iter().map(|polygon| area(polygon)).collect();
    if let Some(index) = areas
        .iter()
        .position(|area| area.abs().partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater))
    {
        return Err(format!("trim loop {index} encloses no area"));
    }
    let probes: Vec<Uv> = polygons
        .iter()
        .map(|polygon| polygon[SPAN_SAMPLES / 2 % polygon.len()])
        .collect();
    let holders: Vec<Vec<usize>> = (0..loops.len())
        .map(|index| {
            (0..loops.len())
                .filter(|other| *other != index && inside(&polygons[*other], probes[index]))
                .collect()
        })
        .collect();
    let depth = |index: usize| holders[index].len();
    let mut assumed = false;
    let mut oriented = Vec::with_capacity(loops.len());
    for (index, curves) in loops.into_iter().enumerate() {
        let outer = depth(index) % 2 == 0;
        assumed |= (areas[index] > 0.0) != outer;
        let positive = outer == same_sense;
        oriented.push(if (areas[index] > 0.0) == positive {
            curves
        } else {
            reversed_loop(&curves).ok_or("a trim curve does not reverse")?
        });
    }
    let mut faces: Vec<(usize, FaceLoops)> = Vec::new();
    for (index, curves) in oriented.iter().enumerate() {
        if depth(index) % 2 == 0 {
            faces.push((index, vec![curves.clone()]));
        }
    }
    for index in 0..oriented.len() {
        if depth(index) % 2 == 1 {
            let parent = holders[index]
                .iter()
                .find(|holder| depth(**holder) + 1 == depth(index))
                .ok_or_else(|| format!("trim loop {index} has no holding loop"))?;
            if let Some((_, face)) = faces.iter_mut().find(|(outer, _)| outer == parent) {
                face.push(oriented[index].clone());
            }
        }
    }
    Ok((faces.into_iter().map(|(_, face)| face).collect(), assumed))
}

/// Builds the B-rep records of the faces of one surface.
struct Builder<'a> {
    surface: &'a NurbsSurfaceRecord,
    brep: Brep,
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3)
        .map(|axis| (a[axis] - b[axis]).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn midpoint(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|axis| f64::midpoint(a[axis], b[axis]))
}

fn helper(record: &NurbsCurveRecord<3>) -> Option<Nurbs<3>> {
    Some(Nurbs {
        degree: record.order.checked_sub(1)?,
        knots: record.knots.clone(),
        points: record.points.clone(),
        weights: record.weights.clone(),
    })
}

fn record<const N: usize>(curve: &Nurbs<N>) -> NurbsCurveRecord<N> {
    NurbsCurveRecord {
        order: curve.degree + 1,
        points: curve.points.clone(),
        weights: curve.weights.clone(),
        knots: curve.knots.clone(),
        form: Form::Open,
    }
}

fn first_point(curve: &NurbsCurveRecord<3>) -> Option<[f64; 3]> {
    curve.points.first().copied()
}

fn last_point(curve: &NurbsCurveRecord<3>) -> Option<[f64; 3]> {
    curve.points.last().copied()
}

impl Builder<'_> {
    fn vertex(&mut self, point: [f64; 3]) -> usize {
        let tolerance = self.brep.tolerance;
        if let Some(found) = self
            .brep
            .vertices
            .iter()
            .position(|known| distance(*known, point) <= tolerance)
        {
            return found;
        }
        self.brep.vertices.push(point);
        self.brep.vertices.len() - 1
    }

    /// Whether every control point of `curve` lies within the tolerance of
    /// its first.
    fn collapsed(&self, curve: &NurbsCurveRecord<3>) -> bool {
        curve
            .points
            .iter()
            .all(|point| distance(*point, curve.points[0]) <= self.brep.tolerance)
    }

    /// Whether edge curve `known` runs along `curve` the other way: each
    /// interior sample of either curve lies within the tolerance of the
    /// other, and the nearest parameters on `known` of the samples of
    /// `curve` decrease.
    fn opposite(&self, known: &NurbsCurveRecord<3>, curve: &NurbsCurveRecord<3>) -> bool {
        let (Some(known), Some(curve)) = (helper(known), helper(curve)) else {
            return false;
        };
        let near = |from: &Nurbs<3>, onto: &Nurbs<3>| -> Option<Vec<f64>> {
            let [low, high] = from.domain()?;
            let count = f64_from_index(SEAM_SAMPLES)?;
            (1..SEAM_SAMPLES)
                .map(|index| {
                    let t = low + (high - low) * f64_from_index(index)? / count;
                    let point = from.point_at(t)?;
                    let nearest = onto.nearest(point)?;
                    (distance(onto.point_at(nearest)?, point) <= self.brep.tolerance)
                        .then_some(nearest)
                })
                .collect()
        };
        let (Some(along), Some(_)) = (near(&curve, &known), near(&known, &curve)) else {
            return false;
        };
        along.windows(2).all(|pair| pair[1] < pair[0])
    }

    /// Adds the loop of parameter-space curves `curves` and returns its
    /// index. `edges` holds the edges of the face so far.
    fn add_loop(&mut self, curves: &[Nurbs<2>], edges: &mut Vec<usize>) -> Result<usize, String> {
        let spaces = curves
            .iter()
            .map(|curve| lifted(self.surface, curve))
            .collect::<Option<Vec<_>>>()
            .ok_or("a boundary curve has no exact space curve within the degree limit")?;
        let count = spaces.len();
        let mut joints = Vec::with_capacity(count);
        let mut gaps = Vec::with_capacity(count);
        for index in 0..count {
            let before = last_point(&spaces[(index + count - 1) % count]).ok_or("no end")?;
            let after = first_point(&spaces[index]).ok_or("no start")?;
            joints.push(self.vertex(midpoint(before, after)));
            gaps.push(distance(before, after));
        }
        let mut oriented = Vec::new();
        let mut poles: Vec<(Option<usize>, usize, usize)> = Vec::new();
        for (index, (curve, space)) in curves.iter().zip(&spaces).enumerate() {
            self.brep.nurbs_pcurves.push(record(curve));
            let pcurve = (Catalog::Nurbs, self.brep.nurbs_pcurves.len() - 1);
            let [from, to] = [joints[index], joints[(index + 1) % count]];
            if self.collapsed(space) {
                poles.push((oriented.len().checked_sub(1), from, pcurve.1));
                continue;
            }
            let seam = edges.iter().copied().find(|edge| {
                let record = &self.brep.edges[*edge];
                record.vertices == [to, from]
                    && record.curve.is_some_and(|(_, known)| {
                        self.opposite(&self.brep.nurbs_curves[known], space)
                    })
            });
            let edge = match seam {
                Some(edge) => edge,
                None => {
                    let [low, high] = [
                        *space.knots.get(space.order - 1).ok_or("no domain")?,
                        *space.knots.get(space.points.len()).ok_or("no domain")?,
                    ];
                    let gap = gaps[index].max(gaps[(index + 1) % count]) / 2.0;
                    self.brep.nurbs_curves.push(space.clone());
                    self.brep.edges.push(EdgeRecord {
                        vertices: [from, to],
                        curve: Some((Catalog::Nurbs, self.brep.nurbs_curves.len() - 1)),
                        range: [low, high],
                        same_sense: true,
                        tolerance: (gap > self.brep.tolerance).then_some(gap),
                    });
                    edges.push(self.brep.edges.len() - 1);
                    self.brep.edges.len() - 1
                }
            };
            self.brep.oriented_edges.push(OrientedEdgeRecord {
                edge,
                orientation: seam.is_none(),
                pcurve: Some(pcurve),
            });
            oriented.push(self.brep.oriented_edges.len() - 1);
        }
        let loop_record = if oriented.is_empty() {
            LoopRecord {
                oriented_edges: Vec::new(),
                vertex: poles.first().map(|(_, vertex, _)| *vertex),
                poles: Vec::new(),
            }
        } else {
            let last = oriented.len() - 1;
            LoopRecord {
                poles: poles
                    .into_iter()
                    .map(|(after, vertex, pcurve)| PoleRecord {
                        after: after.unwrap_or(last),
                        vertex,
                        pcurve: Some((Catalog::Nurbs, pcurve)),
                    })
                    .collect(),
                oriented_edges: oriented,
                vertex: None,
            }
        };
        self.brep.loops.push(loop_record);
        Ok(self.brep.loops.len() - 1)
    }
}

/// The B-rep of the faces `faces` on `surface`.
fn topology(
    surface: &NurbsSurfaceRecord,
    faces: &[FaceLoops],
    same_sense: bool,
) -> Result<Brep, String> {
    let mut builder = Builder {
        surface,
        brep: Brep {
            nurbs_surfaces: vec![surface.clone()],
            tolerance: DEFAULT_TOLERANCE,
            angle_scale: 1.0,
            valid: true,
            ..Brep::default()
        },
    };
    for (index, loops) in faces.iter().enumerate() {
        let mut edges = Vec::new();
        let records = loops
            .iter()
            .map(|curves| builder.add_loop(curves, &mut edges))
            .collect::<Result<Vec<_>, _>>()?;
        builder.brep.faces.push(FaceRecord {
            loops: records,
            surface: (Catalog::Nurbs, 0),
            same_sense,
            id: i64::try_from(index).map_err(|_| "too many faces")?,
        });
    }
    builder.brep.shells.push(ShellRecord {
        faces: (0..builder.brep.faces.len()).collect(),
        closed: false,
    });
    Ok(builder.brep)
}

/// The parameter range of `patch` and whether an authored range was set
/// aside: the authored `uRange` and `vRange` where they lie in the knot
/// domain, else the knot domain.
fn parameter_range(
    patch: &PatchData,
    surface: &NurbsSurfaceRecord,
) -> Result<([Uv; 2], bool), String> {
    let domain = super::convert::surface_domain(surface)
        .filter(|[u, v]| u[0] < u[1] && v[0] < v[1])
        .ok_or("the knot domain is empty")?;
    let mut ignored = false;
    let mut range = domain;
    for axis in 0..2 {
        let Some([low, high]) = patch.ranges[axis] else {
            continue;
        };
        if low < high && domain[axis][0] <= low && high <= domain[axis][1] {
            range[axis] = [low, high];
        } else {
            ignored = true;
        }
    }
    Ok((range, ignored))
}

/// The bounded faces of `patch`, in scene units.
pub(crate) fn bounded(patch: &PatchData) -> Result<Bounded, String> {
    let surface = surface(patch)?;
    let (range, range_ignored) = parameter_range(patch, &surface)?;
    let same_sense = !patch.left_handed;
    let natural = || -> Result<Brep, String> {
        let (faces, _) = faces(vec![natural_loop(range)], same_sense)?;
        topology(&surface, &faces, same_sense)
    };
    let trims = if patch.trims_malformed {
        Trims::Dropped("the trim attributes do not describe their curves".to_owned())
    } else if patch.trims.is_empty() {
        Trims::Absent
    } else {
        let trimmed = trim_loops(patch, range).and_then(|loops| {
            let (faces, assumed) = faces(loops, same_sense)?;
            Ok((topology(&surface, &faces, same_sense)?, assumed))
        });
        match trimmed {
            Ok((brep, region_assumed)) => {
                return Ok(Bounded {
                    brep,
                    trims: Trims::Read { region_assumed },
                    range_ignored,
                })
            }
            Err(reason) => Trims::Dropped(reason),
        }
    };
    Ok(Bounded {
        brep: natural()?,
        trims,
        range_ignored,
    })
}

#[cfg(test)]
mod tests;
