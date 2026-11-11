// SPDX-License-Identifier: Apache-2.0
//! Reuse established shell edges on a missing face’s native chart.

use std::collections::BTreeMap;

use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point2, Point3};

use super::super::{edge_alignment, Patch, SheetBoundaryRecovery, Use, GAP_MM};
use super::{
    contains_hole, cycle, delta, det, Carrier, DecodedFace, Ring, Trim, TRIM_STEPS, UV_AREA,
};

pub(super) fn recover(face: &DecodedFace<'_>, edges: &BTreeMap<usize, &Use>) -> Option<Patch> {
    let carrier = &face.carrier;
    let mut rings = std::iter::once(&face.outer).chain(&face.inner);
    if rings.any(|ring| {
        ring.trims
            .iter()
            .any(|trim| !edges.contains_key(&trim.edge))
    }) {
        return None;
    }
    let bounds = carrier.bounds()?;
    let solved = if matches!(carrier, Carrier::Plane(_)) {
        carrier.solved([[0.0; 2]; 2], false)?
    } else {
        if !bounds.into_iter().flatten().all(f64::is_finite) {
            return None;
        }
        let [lo, hi] = [
            carrier.point([bounds[0][0], bounds[1][0]]),
            carrier.point([bounds[0][1], bounds[1][1]]),
        ];
        let limits = std::array::from_fn(|axis| [lo[axis].min(hi[axis]), lo[axis].max(hi[axis])]);
        carrier.solved(limits, true)?
    };
    let project = |trim: &Trim, edge: &Use| match &solved {
        SolvedSurfaceGeometry::Plane(plane) => planar_pcurve(edge, *plane),
        SolvedSurfaceGeometry::Nurbs(spline) => {
            let chart = |t| {
                let uv = trim.uv(t)?;
                let bounded =
                    std::array::from_fn(|axis| uv[axis].clamp(bounds[axis][0], bounds[axis][1]));
                Some(carrier.point(bounded))
            };
            let [a, b] = trim
                .range
                .map(|t| chart(t).and_then(|uv| carrier.evaluate(uv)));
            let [a, b] = [a?, b?];
            let [start, end] = edge.vertices.map(|(_, point)| point);
            let direct = a.distance(start) + b.distance(end);
            let reversed = a.distance(end) + b.distance(start);
            if (direct - reversed).abs() <= GAP_MM {
                return None;
            }
            let [lo, hi] = trim.range;
            let range = if direct < reversed {
                [lo, hi]
            } else {
                [hi, lo]
            };
            edge_alignment::project_spline(edge, spline, |fraction| {
                let [u, v] = chart(super::interval_parameter(range, fraction))?;
                Some(Point2::new(u, v))
            })
        }
        _ => None,
    };
    let forward = face.member.forward != carrier.reversed();
    let mut sides = ring(&face.outer, edges, &project, forward)?;
    let mut inner_starts = Vec::new();
    let outer_end = sides.len();
    for hole in &face.inner {
        let hole = ring(hole, edges, &project, !forward)?;
        if !contains_hole(&sides[..outer_end], &hole)?
            || hole.iter().any(|side| {
                sides[outer_end..].iter().any(|other| {
                    side.vertices
                        .iter()
                        .any(|(vertex, _)| other.vertices.iter().any(|(id, _)| id == vertex))
                })
            })
        {
            return None;
        }
        inner_starts.push(sides.len());
        sides.extend(hole);
    }
    Some(Patch {
        face: face.face_pos,
        recovery: SheetBoundaryRecovery::default(),
        body: face.member.body,
        forward,
        sense_inferred: carrier.roster_sense_inferred(),
        surface_fit: carrier.surface_fit(),
        surface: solved,
        priority: super::super::sheet_group::VERTEX_RETRY_PRIORITY,
        sides,
        inner_starts,
    })
}

pub(super) fn select_line(
    trim: &mut Trim,
    plane: PlaneSurface,
    vertices: &BTreeMap<usize, Point3>,
) {
    let Some(alternate) = &trim.alternate_line else {
        return;
    };
    let surface = SolvedSurfaceGeometry::Plane(plane);
    let sites = trim
        .vertices
        .iter()
        .filter_map(|id| {
            eval::analytic_surface_parameters_solved(&surface, *vertices.get(id)?)
                .map(cadmpeg_ir::units::FinitePoint2::get)
        })
        .collect::<Vec<_>>();
    if sites.is_empty() {
        return;
    }
    let alternate = PcurveGeometry::Line(*alternate);
    let contains = |geometry: &PcurveGeometry| {
        let endpoints = trim.range.map(|t| {
            eval::decode::pcurve_uv(EvaluationAdmission::Standard, geometry, t)
                .ok()
                .map(cadmpeg_ir::units::FinitePoint2::get)
        });
        let [Some(a), Some(b)] = endpoints else {
            return false;
        };
        let direction = delta([a.u, a.v], [b.u, b.v]);
        let length = direction[0].hypot(direction[1]);
        if length == 0.0 {
            return false;
        }
        let unit = direction.map(|v| v / length);
        sites.iter().all(|p| {
            let offset = delta([a.u, a.v], [p.u, p.v]);
            let station = offset[0].mul_add(unit[0], offset[1] * unit[1]);
            det(unit, offset).abs() <= GAP_MM && (-GAP_MM..=length + GAP_MM).contains(&station)
        })
    };
    match (contains(&trim.pcurve), contains(&alternate)) {
        (true, false) => trim.alternate_line = None,
        (false, true) => {
            trim.pcurve = alternate;
            trim.alternate_line = None;
        }
        _ => (),
    }
}

fn planar_pcurve(edge: &Use, plane: PlaneSurface) -> Option<PcurveGeometry> {
    let origin = plane.origin().get();
    let u = *plane.frame().reference().as_raw();
    let v = *plane.frame().binormal().as_raw();
    let normal = *plane.frame().axis().as_raw();
    let points = edge
        .curve
        .control_points()
        .iter()
        .map(|point| {
            let vector = point.get().vector_from(origin);
            (vector.dot(normal).abs() <= GAP_MM)
                .then_some(Point2::new(vector.dot(u), vector.dot(v)))
        })
        .collect::<Option<Vec<_>>>()?;
    let nurbs = PcurveNurbs::from_checked_lanes(
        edge.curve.degree(),
        edge.curve.knots().clone(),
        points,
        edge.curve.weights(),
        false,
    )
    .ok()?;
    Some(PcurveGeometry::Nurbs { nurbs })
}

fn ring(
    ring: &Ring,
    edges: &BTreeMap<usize, &Use>,
    project: &impl Fn(&Trim, &Use) -> Option<PcurveGeometry>,
    forward: bool,
) -> Option<Vec<Use>> {
    let mut sides = Vec::new();
    for trim in &ring.trims {
        let edge = *edges.get(&trim.edge)?;
        let ids = edge.vertices.map(|(id, _)| id);
        if ids != trim.vertices && ids != [trim.vertices[1], trim.vertices[0]] {
            return None;
        }
        let pcurve = project(trim, edge)?;
        sides.push(Use {
            pcurve,
            inferred_accuracy: trim.inferred_accuracy,
            lift_error_mm: 0.0,
            ..edge.clone()
        });
    }
    if !ring.contains_vertices(&sides) {
        return None;
    }
    let identities = sides
        .iter()
        .map(|side| side.vertices.map(|(id, _)| id))
        .collect::<Vec<_>>();
    let directions = cycle(&identities, |a, b| a == b)?;
    let mut points = Vec::new();
    for (side, direction) in sides.iter_mut().zip(directions) {
        side.forward = direction;
        let range = side.curve.full_knot_endpoints().endpoints();
        for i in 0..TRIM_STEPS {
            let fraction =
                f64::from(if direction { i } else { TRIM_STEPS - i }) / f64::from(TRIM_STEPS);
            let t = super::interval_parameter(range, fraction);
            let point = eval::decode::pcurve_uv(EvaluationAdmission::Standard, &side.pcurve, t)
                .ok()?
                .get();
            points.push([point.u, point.v]);
        }
    }
    let center = *points.first()?;
    let area = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&a, &b)| det(delta(center, a), delta(center, b)))
        .sum::<f64>();
    if !area.is_finite() || area.abs() <= UV_AREA {
        return None;
    }
    if (area > 0.0) != forward {
        sides.reverse();
        for side in &mut sides {
            side.forward = !side.forward;
        }
    }
    Some(sides)
}
