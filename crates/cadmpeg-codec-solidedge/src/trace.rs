// SPDX-License-Identifier: Apache-2.0
//! Refinement of a native intersection chart onto its two support surfaces.
//!
//! A chart is a polyline cache of the exact intersection of the supports. Its
//! points depart from the supports by up to the chart's stated fit bound. The
//! refinement moves each chart point onto both supports and bisects each
//! segment until the chord stays within a target of the supports.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::legacy_cache::LegacyCache;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3};

use crate::chart::Chart;
use crate::geometry::{self, Nodes};

/// Alternating projections applied to one point before the point counts as
/// off the intersection.
const MAX_SNAP_ITERATIONS: usize = 64;

/// Points one refined chart may hold, counting the points of the segments not
/// yet refined. A chart that reaches the limit keeps the segments it has, and
/// the stated departure covers the rest. A chart that holds the limit before
/// refinement is not subdivided.
const MAX_TRACE_POINTS: usize = 16_384;

/// Midpoint bisections applied to one chart segment.
const MAX_TRACE_DEPTH: u32 = 16;

/// Largest distance, millimetres, from a point on one support to the other
/// support at which the point counts as on the intersection.
const EPS_SNAP_MM: f64 = 1.0e-8;

/// Multiple of a chart's stated fit bound that a refined point may move.
const REACH_PER_FIT: f64 = 4.0;

/// Distance, millimetres, a refined point may move when the chart states no
/// fit bound.
const REACH_UNSTATED_MM: f64 = 1.0;

/// A chart curve moved onto its supports.
pub(crate) struct Traced {
    pub(crate) curve: NurbsCurve,
    /// Largest distance, millimetres, from a vertex or segment midpoint of the
    /// curve to either support.
    pub(crate) departure: f64,
    /// Parameter interval of the curve.
    pub(crate) range: [f64; 2],
}

/// The point of `surface` that `point` projects to. Absent when the surface
/// carrier is not one the evaluators project onto or the projection is not
/// finite.
fn project(
    ctx: &DecodeContext<'_>,
    surface: &SolvedSurfaceGeometry,
    point: Point3,
    seed: &mut Option<Point2>,
) -> Result<Option<Point3>, CodecError> {
    match surface {
        SolvedSurfaceGeometry::Plane(plane) => {
            let normal = *plane.frame().axis().as_raw();
            let height = point.vector_from(plane.origin().get()).dot(normal);
            Ok(Some(point.translated(normal, -height)))
        }
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            let Some(uv) =
                cadmpeg_ir::eval::nurbs_surface_parameter_near_point(ctx, nurbs, point, *seed)?
            else {
                return Ok(None);
            };
            let uv = uv.get();
            *seed = Some(uv);
            Ok(
                cadmpeg_ir::eval::finite_or_refusal(
                    cadmpeg_ir::eval::decode::nurbs_surface_point(ctx, nurbs, uv.u, uv.v),
                )?
                .map(cadmpeg_ir::features::geometry::FinitePoint3::get),
            )
        }
        SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_)
        | SolvedSurfaceGeometry::Torus(_) => {
            let Some(uv) = cadmpeg_ir::eval::analytic_surface_parameters_solved(surface, point)
            else {
                return Ok(None);
            };
            let uv = uv.get();
            Ok(cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::decode::surface_point_solved(ctx, surface, uv.u, uv.v),
            )?
            .map(cadmpeg_ir::features::geometry::FinitePoint3::get))
        }
        _ => Ok(None),
    }
}

/// Seeds of the two projections.
type Seeds = [Option<Point2>; 2];

/// The point on both supports that alternating projection reaches from
/// `point`, when it does so within `reach` of `point`.
fn snap(
    ctx: &DecodeContext<'_>,
    supports: [&SolvedSurfaceGeometry; 2],
    point: Point3,
    reach: f64,
    seeds: &mut Seeds,
) -> Result<Option<Point3>, CodecError> {
    let mut current = point;
    for _ in 0..MAX_SNAP_ITERATIONS {
        let Some(first) = project(ctx, supports[0], current, &mut seeds[0])? else {
            return Ok(None);
        };
        let Some(second) = project(ctx, supports[1], first, &mut seeds[1])? else {
            return Ok(None);
        };
        current = second;
        if first.distance(second) <= EPS_SNAP_MM {
            return Ok((current.distance(point) <= reach).then_some(current));
        }
        if current.distance(point) > reach {
            return Ok(None);
        }
    }
    Ok(None)
}

/// The curve's points and the parameters its knots state for them.
fn points_and_parameters(curve: &NurbsCurve) -> Option<(Vec<Point3>, Vec<f64>)> {
    if curve.degree() != 1 || curve.weights().is_some() {
        return None;
    }
    let points: Vec<Point3> = curve
        .control_points()
        .into_iter()
        .map(cadmpeg_ir::features::geometry::FinitePoint3::get)
        .collect();
    let knots = curve.knots().as_slice();
    let parameters = knots.get(1..=points.len())?.to_vec();
    (points.len() >= 2 && parameters.windows(2).all(|pair| pair[0] < pair[1]))
        .then_some((points, parameters))
}

fn rebuild(curve: &NurbsCurve, points: Vec<Point3>, parameters: &[f64]) -> Option<NurbsCurve> {
    let first = *parameters.first()?;
    let last = *parameters.last()?;
    let mut knots = Vec::with_capacity(parameters.len() + 2);
    knots.push(first);
    knots.extend_from_slice(parameters);
    knots.push(last);
    let closed = curve.periodic() && points.first() == points.last();
    NurbsCurve::from_lanes(1, knots, points, None, closed).ok()
}

/// Moves the points of the degree-one `chart` onto both `supports` and
/// bisects each segment until its chord midpoint lies within `target`
/// millimetres of the supports. Every point stays within `reach` of its chart
/// position; parameters of chart points are kept, and a new point takes the
/// mean parameter of its segment. A chart that stops short of an edge vertex
/// in `ends` continues along the supports to that vertex at the chart's
/// parameter gauge. The end points of the result are the chart's own end
/// points, or the vertex positions where the chart continues. Absent when the
/// chart is not a polyline, a point does not reach both supports, or a new
/// point leaves its segment.
pub(crate) fn refine(
    ctx: &DecodeContext<'_>,
    supports: [&SolvedSurfaceGeometry; 2],
    chart: &NurbsCurve,
    ends: Option<[Point3; 2]>,
    target: f64,
    reach: f64,
) -> Result<Option<Traced>, CodecError> {
    let Some((points, parameters)) = points_and_parameters(chart) else {
        return Ok(None);
    };
    let last_index = points.len() - 1;
    let ring = points.first() == points.last();
    let mut seeds: Seeds = [None, None];
    let mut snapped = Vec::with_capacity(points.len() + 2);
    for (index, point) in points.iter().enumerate() {
        if index == last_index && ring {
            snapped.push(snapped[0]);
            continue;
        }
        let Some(moved) = snap(ctx, supports, *point, reach, &mut seeds)? else {
            return Ok(None);
        };
        snapped.push(moved);
    }
    let mut working_parameters = parameters.clone();
    let mut restore = [points[0], points[last_index]];
    let mut vertex_gap = 0.0_f64;
    let length: f64 = points
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum();
    if let (Some(vertices), false, true) = (ends, ring, length > 0.0) {
        let gauge = (parameters[last_index] - parameters[0]) / length;
        for vertex in vertices {
            let Some(moved) = snap(ctx, supports, vertex, reach, &mut seeds)? else {
                continue;
            };
            let count = snapped.len();
            if snapped.iter().any(|point| point.distance(moved) <= target) {
                continue;
            }
            // The chord whose interior holds the vertex, nearest first.
            let mut best: Option<(usize, f64, f64)> = None;
            for index in 0..count - 1 {
                let span = snapped[index + 1].vector_from(snapped[index]);
                let square = span.dot(span);
                let along = moved.vector_from(snapped[index]).dot(span);
                if square > 0.0 && along > 0.0 && along < square {
                    let fraction = along / square;
                    let foot = snapped[index].translated(span, fraction);
                    let miss = foot.distance(moved);
                    if best.is_none_or(|(_, _, nearest)| miss < nearest) {
                        best = Some((index, fraction, miss));
                    }
                }
            }
            if let Some((index, fraction, _)) = best.filter(|(_, _, miss)| *miss <= reach) {
                let parameter = working_parameters[index]
                    + fraction * (working_parameters[index + 1] - working_parameters[index]);
                working_parameters.insert(index + 1, parameter);
                snapped.insert(index + 1, moved);
                vertex_gap = vertex_gap.max(vertex.distance(moved));
                continue;
            }
            // A vertex beyond a chart end continues the chart to it.
            let beyond = |end: usize, inner: usize| {
                moved
                    .vector_from(snapped[end])
                    .dot(snapped[end].vector_from(snapped[inner]))
                    > 0.0
            };
            let nearer_back = moved.distance(snapped[count - 1]) <= moved.distance(snapped[0]);
            if nearer_back && beyond(count - 1, count - 2) {
                let tail =
                    working_parameters[count - 1] + moved.distance(snapped[count - 1]) * gauge;
                working_parameters.push(tail);
                snapped.push(moved);
                restore[1] = vertex;
            } else if !nearer_back && beyond(0, 1) {
                working_parameters.insert(
                    0,
                    working_parameters[0] - moved.distance(snapped[0]) * gauge,
                );
                snapped.insert(0, moved);
                restore[0] = vertex;
            }
        }
    }
    let mut refined_points = vec![snapped[0]];
    let mut refined_parameters = vec![working_parameters[0]];
    let mut departure = 0.0_f64;
    for index in 1..snapped.len() {
        let mut pending = vec![(
            working_parameters[index - 1],
            snapped[index - 1],
            working_parameters[index],
            snapped[index],
            0_u32,
        )];
        while let Some((start, from, end, to, depth)) = pending.pop() {
            let chord = Point3::new(
                0.5 * (from.x + to.x),
                0.5 * (from.y + to.y),
                0.5 * (from.z + to.z),
            );
            let middle = 0.5 * (start + end);
            let Some(moved) = snap(ctx, supports, chord, reach, &mut seeds)? else {
                return Ok(None);
            };
            let span = to.vector_from(from);
            let along = moved.vector_from(from).dot(span);
            let inside = along > 0.0 && along < span.dot(span) && middle > start && middle < end;
            let distance = moved.distance(chord);
            if distance <= target {
                departure = departure.max(distance);
            } else if !inside {
                return Ok(None);
            } else if depth >= MAX_TRACE_DEPTH
                || refined_points.len() + pending.len() + (snapped.len() - 1 - index) + 2
                    >= MAX_TRACE_POINTS
            {
                departure = departure.max(distance);
            } else {
                pending.push((middle, moved, end, to, depth + 1));
                pending.push((start, from, middle, moved, depth + 1));
                continue;
            }
            refined_points.push(to);
            refined_parameters.push(end);
        }
    }
    // The end points stay at positions the document states; the measured
    // departure covers their distance to the supports.
    let range = [
        refined_parameters[0],
        refined_parameters[refined_parameters.len() - 1],
    ];
    refined_points[0] = restore[0];
    let last = refined_points.len() - 1;
    refined_points[last] = restore[1];
    let Some(curve) = rebuild(chart, refined_points, &refined_parameters) else {
        return Ok(None);
    };
    let departure = self::departure(ctx, supports, &curve)?
        .unwrap_or(departure)
        .max(vertex_gap);
    Ok(Some(Traced {
        curve,
        departure,
        range,
    }))
}

/// Moves `chart` onto its support surfaces to within half of
/// `document_linear` and returns the departure, millimetres, of the carrier it
/// now holds from those surfaces. A chart that cannot move keeps its points
/// and returns the measured departure, or its stated fit bound when a support
/// cannot be measured. Absent when neither is available. The flag states that
/// the chart moved; it then reaches the edge vertices in `ends` exactly.
pub(crate) fn conform(
    ctx: &DecodeContext<'_>,
    nodes: &Nodes<'_>,
    chart: &mut Chart,
    ends: Option<[Point3; 2]>,
    document_linear: f64,
) -> Result<Option<(f64, bool)>, CodecError> {
    let stated = chart.fit.as_ref().map(|fit| fit.fit_tolerance.get());
    let surfaces = chart.supports.map(|index| {
        nodes
            .get(&index)
            .copied()
            .and_then(|node| geometry::surface(nodes, node))
    });
    let [Some(SurfaceGeometry::Solved(first)), Some(SurfaceGeometry::Solved(second))] = surfaces
    else {
        return Ok(stated.map(|stated| (stated, false)));
    };
    let supports = [&first, &second];
    let reach = stated.map_or(REACH_UNSTATED_MM, |fit| REACH_PER_FIT * fit);
    if let Some(traced) = refine(
        ctx,
        supports,
        &chart.curve,
        ends,
        0.5 * document_linear,
        reach,
    )? {
        if let Ok(fit) = LegacyCache::try_new(traced.departure) {
            chart.fit = Some(fit);
            chart.curve = traced.curve;
            chart.range = traced.range;
            return Ok(Some((traced.departure, true)));
        }
    }
    Ok(departure(ctx, supports, &chart.curve)?
        .or(stated)
        .map(|measured| (measured, false)))
}

/// Largest distance, millimetres, from the vertices and segment midpoints of
/// the degree-one `curve` to either support. Absent when a support does not
/// project or the curve is not a polyline.
pub(crate) fn departure(
    ctx: &DecodeContext<'_>,
    supports: [&SolvedSurfaceGeometry; 2],
    curve: &NurbsCurve,
) -> Result<Option<f64>, CodecError> {
    let Some((points, _)) = points_and_parameters(curve) else {
        return Ok(None);
    };
    let mut seeds: Seeds = [None, None];
    let mut worst = 0.0_f64;
    let mut probes = Vec::with_capacity(2 * points.len());
    for (index, point) in points.iter().enumerate() {
        probes.push(*point);
        if let Some(next) = points.get(index + 1) {
            probes.push(Point3::new(
                0.5 * (point.x + next.x),
                0.5 * (point.y + next.y),
                0.5 * (point.z + next.z),
            ));
        }
    }
    for probe in probes {
        for (support, seed) in supports.into_iter().zip(&mut seeds) {
            let Some(foot) = project(ctx, support, probe, seed)? else {
                return Ok(None);
            };
            worst = worst.max(foot.distance(probe));
        }
    }
    Ok(Some(worst))
}

#[cfg(test)]
mod tests;
