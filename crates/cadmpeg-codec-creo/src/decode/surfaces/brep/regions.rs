// SPDX-License-Identifier: Apache-2.0
//! Disjoint planar face regions.

use std::collections::BTreeMap;

use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;

use crate::decode::analytic::pcurves::{unique_oriented_native_pcurve, NativePcurveCandidates};
use crate::decode::uniqueness::exactly_one;
use crate::topology::HalfEdgeId;

use super::{
    bounded_planar_circle_loop_parameters, parameter_points_agree, vectors_are_parallel,
    NativeCurveEvidence,
};

const EPS_REGION: f64 = 1.0e-9;

type Circle = ([f64; 2], f64);

struct CircularArc {
    circle: Circle,
    endpoints: [[f64; 2]; 2],
    midpoint: [f64; 2],
    span: f64,
}

struct OuterRegion {
    polygon: Vec<[f64; 2]>,
    bounds: [[f64; 2]; 2],
}

struct ProjectedRing {
    polygon: Vec<[f64; 2]>,
    world: Vec<[Point3; 2]>,
}

fn projected_ring(
    lp: &crate::topology::Loop,
    surface: &SurfaceGeometry,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<ProjectedRing> {
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = surface else {
        return None;
    };
    let normal = plane.frame().axis().as_raw();
    let origin = plane.origin().get();
    let mut polygon = Vec::new();
    let mut world = Vec::new();
    let mut projected = Vec::new();
    for edge in lp.half_edges() {
        let binding = incidence.get(edge)?;
        let mut segment = [Point3::new(0.0, 0.0, 0.0); 2];
        let mut uv = [[0.0; 2]; 2];
        for ((point, param), vertex) in segment
            .iter_mut()
            .zip(&mut uv)
            .zip([binding.start_vertex_id, binding.end_vertex_id?])
        {
            *point = Point3::from(*vertices.get(&vertex.get())?);
            let delta = point.vector_from(origin);
            (delta.dot(*normal).abs() <= EPS_REGION * delta.norm().max(1.0)).then_some(())?;
            let value = cadmpeg_ir::eval::analytic_surface_parameters(surface, *point)?.get();
            *param = [value.u, value.v];
        }
        polygon.push(uv[0]);
        projected.push(uv);
        world.push(segment);
    }
    if projected.iter().enumerate().any(|(index, segment)| {
        !parameter_points_agree(segment[1], projected[(index + 1) % projected.len()][0])
    }) {
        return None;
    }
    Some(ProjectedRing { polygon, world })
}

fn cross(first: [f64; 2], second: [f64; 2], point: [f64; 2]) -> f64 {
    (second[0] - first[0]).mul_add(
        point[1] - first[1],
        -(second[1] - first[1]) * (point[0] - first[0]),
    )
}

fn arc_contains(arc: &CircularArc, point: [f64; 2]) -> bool {
    if arc
        .endpoints
        .iter()
        .any(|end| parameter_points_agree(point, *end))
    {
        return true;
    }
    let center = arc.circle.0;
    let reference = [
        arc.endpoints[0][0] - center[0],
        arc.endpoints[0][1] - center[1],
    ];
    let middle = [arc.midpoint[0] - center[0], arc.midpoint[1] - center[1]];
    let radial = [point[0] - center[0], point[1] - center[1]];
    let orientation = reference[0]
        .mul_add(middle[1], -reference[1] * middle[0])
        .signum();
    let sine = orientation * reference[0].mul_add(radial[1], -reference[1] * radial[0]);
    let cosine = reference[0].mul_add(radial[0], reference[1] * radial[1]);
    if orientation == 0.0 || !sine.is_finite() || !cosine.is_finite() {
        return true;
    }
    sine.atan2(cosine).rem_euclid(std::f64::consts::TAU) <= arc.span + EPS_REGION
}

fn segment_arc_crosses(start: [f64; 2], end: [f64; 2], arc: &CircularArc) -> bool {
    let circle = arc.circle;
    let delta = [end[0] - start[0], end[1] - start[1]];
    let length_squared = delta[0].mul_add(delta[0], delta[1] * delta[1]);
    if !length_squared.is_finite() || length_squared <= 0.0 {
        return true;
    }
    let to_center = [circle.0[0] - start[0], circle.0[1] - start[1]];
    let projection = (to_center[0] * delta[0] + to_center[1] * delta[1]) / length_squared;
    let closest = [
        start[0] + projection * delta[0] - circle.0[0],
        start[1] + projection * delta[1] - circle.0[1],
    ];
    let residual = circle.1 * circle.1 - closest[0].mul_add(closest[0], closest[1] * closest[1]);
    let scale = circle.1.max(length_squared.sqrt()).max(1.0);
    if !projection.is_finite() || !residual.is_finite() {
        return true;
    }
    if residual < -EPS_REGION * scale * scale {
        return false;
    }
    let half = (residual.max(0.0) / length_squared).sqrt();
    [projection - half, projection + half].into_iter().any(|t| {
        if !(-EPS_REGION..=1.0 + EPS_REGION).contains(&t) {
            return false;
        }
        let point = [start[0] + t * delta[0], start[1] + t * delta[1]];
        if arc
            .endpoints
            .iter()
            .any(|allowed| parameter_points_agree(point, *allowed))
        {
            return false;
        }
        arc_contains(arc, point)
    })
}

fn arcs_cross(first: &CircularArc, second: &CircularArc) -> bool {
    let (a, ar) = first.circle;
    let (b, br) = second.circle;
    let delta = [b[0] - a[0], b[1] - a[1]];
    let distance = delta[0].hypot(delta[1]);
    let tolerance = EPS_REGION * ar.max(br).max(distance).max(1.0);
    if !distance.is_finite() {
        return true;
    }
    if distance <= tolerance {
        return (ar - br).abs() <= tolerance;
    }
    if distance > ar + br + tolerance || distance + ar.min(br) + tolerance < ar.max(br) {
        return false;
    }
    let along = (ar * ar - br * br + distance * distance) / (2.0 * distance);
    let height_squared = ar * ar - along * along;
    if !along.is_finite() || !height_squared.is_finite() {
        return true;
    }
    if height_squared < -tolerance * ar.max(1.0) {
        return false;
    }
    let unit = [delta[0] / distance, delta[1] / distance];
    let base = [a[0] + along * unit[0], a[1] + along * unit[1]];
    let height = height_squared.max(0.0).sqrt();
    [-1.0, 1.0].into_iter().any(|sign| {
        let point = [
            base[0] - sign * height * unit[1],
            base[1] + sign * height * unit[0],
        ];
        let shared_endpoint = first.endpoints.iter().any(|a| {
            parameter_points_agree(point, *a)
                && second
                    .endpoints
                    .iter()
                    .any(|b| parameter_points_agree(point, *b))
        });
        !shared_endpoint && arc_contains(first, point) && arc_contains(second, point)
    })
}

fn outer_region(
    lp: &crate::topology::Loop,
    surface: &SurfaceGeometry,
    polygon: Vec<[f64; 2]>,
    world: &[[Point3; 2]],
    evidence: NativeCurveEvidence<'_>,
) -> Option<OuterRegion> {
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = surface else {
        return None;
    };
    crate::decode::sweep::revolution_cap::convex_polygon_contains_circles(&polygon, &[])
        .then_some(())?;
    let winding = cross(polygon[0], polygon[1], polygon[2]).signum();
    let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for point in &polygon {
        for axis in 0..2 {
            bounds[0][axis] = bounds[0][axis].min(point[axis]);
            bounds[1][axis] = bounds[1][axis].max(point[axis]);
        }
    }
    let mut arcs = Vec::new();
    for (index, half_edge) in lp.half_edges().iter().enumerate() {
        let curve = exactly_one(evidence.model_curves.iter().filter(|curve| {
            crate::identity::matches_numbered_identity(
                curve.id.as_str(),
                "creo:visibgeom:curve#",
                half_edge.curve_id,
            )
        }))?;
        let geometry = evidence.source_carriers.curve_geometry(curve);
        match geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) => {
                let origin = line.origin().get();
                let direction = *line.direction().as_raw();
                for point in world[index] {
                    let delta = point.vector_from(origin);
                    (delta.cross(direction).norm() <= EPS_REGION * delta.norm().max(1.0))
                        .then_some(())?;
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) => {
                let normal = plane.frame().axis().as_raw();
                let center = circle.center().get();
                let delta = center.vector_from(plane.origin().get());
                (vectors_are_parallel(*circle.frame().axis().as_raw(), *normal)
                    && delta.dot(*normal).abs() <= EPS_REGION * delta.norm().max(1.0))
                .then_some(())?;
                let [start, end] = curve.parameter_range?.endpoints();
                let span = (end - start).abs();
                (span > EPS_REGION && span <= std::f64::consts::PI + EPS_REGION).then_some(())?;
                let reference = circle.frame().reference().as_raw();
                let transverse = circle.frame().axis().as_raw().cross(*reference);
                let project = |angle: f64| {
                    let radial = reference.scale(angle.cos()) + transverse.scale(angle.sin());
                    let point = center.translated(radial, circle.radius().get());
                    let uv = cadmpeg_ir::eval::analytic_surface_parameters(surface, point)?.get();
                    Some([uv.u, uv.v])
                };
                let first = project(start)?;
                let last = project(end)?;
                let next = (index + 1) % polygon.len();
                ((parameter_points_agree(first, polygon[index])
                    && parameter_points_agree(last, polygon[next]))
                    || (parameter_points_agree(last, polygon[index])
                        && parameter_points_agree(first, polygon[next])))
                .then_some(())?;
                let middle = project(start.midpoint(end))?;
                let chord_length = (polygon[next][0] - polygon[index][0])
                    .hypot(polygon[next][1] - polygon[index][1]);
                (winding * cross(polygon[index], polygon[next], middle)
                    < -EPS_REGION * chord_length * chord_length.max(1.0))
                .then_some(())?;
                let center = cadmpeg_ir::eval::analytic_surface_parameters(surface, center)?.get();
                let carrier = ([center.u, center.v], circle.radius().get());
                let bounded_arc = CircularArc {
                    circle: carrier,
                    endpoints: [first, last],
                    midpoint: middle,
                    span,
                };
                arcs.iter()
                    .all(|previous| !arcs_cross(previous, &bounded_arc))
                    .then_some(())?;
                for (other, point) in polygon.iter().enumerate() {
                    if other == index {
                        continue;
                    }
                    let after = (other + 1) % polygon.len();
                    if segment_arc_crosses(*point, polygon[after], &bounded_arc) {
                        return None;
                    }
                }
                let [lower, upper] = &mut bounds;
                for ((lower, upper), coordinate) in
                    lower.iter_mut().zip(upper.iter_mut()).zip(carrier.0)
                {
                    *lower = lower.min(coordinate - carrier.1);
                    *upper = upper.max(coordinate + carrier.1);
                }
                arcs.push(bounded_arc);
            }
            _ => return None,
        }
    }
    Some(OuterRegion { polygon, bounds })
}

pub(super) fn partition<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    loops: &[&'a crate::topology::Loop],
    face: (u32, &SurfaceGeometry),
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    vertices: &BTreeMap<u32, [f64; 3]>,
    pcurves: &NativePcurveCandidates,
    evidence: NativeCurveEvidence<'_>,
) -> Result<Option<Vec<Vec<&'a crate::topology::Loop>>>, cadmpeg_core::CodecError> {
    let mut outer = Vec::new();
    let mut holes = Vec::new();
    for lp in loops {
        let Some(ProjectedRing { polygon, world }) =
            projected_ring(lp, face.1, incidence, vertices)
        else {
            return Ok(None);
        };
        for (index, edge) in lp.half_edges().iter().enumerate() {
            let Some(candidates) = pcurves.get(&(edge.curve_id, face.0)) else {
                continue;
            };
            let points = world[index].map(Into::into);
            let Some(selection) = unique_oriented_native_pcurve(ctx, face.1, candidates, points)?
            else {
                return Ok(None);
            };
            if !parameter_points_agree(selection.endpoints[0], polygon[index])
                || !parameter_points_agree(
                    selection.endpoints[1],
                    polygon[(index + 1) % polygon.len()],
                )
            {
                return Ok(None);
            }
        }
        if polygon.len() == 2 {
            let Some(circle) =
                bounded_planar_circle_loop_parameters(lp, &polygon, face.1, evidence)
            else {
                return Ok(None);
            };
            holes.push((*lp, circle));
        } else {
            let Some(region) = outer_region(lp, face.1, polygon, &world, evidence) else {
                return Ok(None);
            };
            outer.push((*lp, region));
        }
    }
    if outer.len() < 2 {
        return Ok(None);
    }
    for (index, (_, first)) in outer.iter().enumerate() {
        for (_, second) in outer.iter().skip(index + 1) {
            let scale = first
                .bounds
                .iter()
                .flatten()
                .chain(second.bounds.iter().flatten())
                .map(|x| x.abs())
                .fold(1.0, f64::max);
            if !(0..2).any(|axis| {
                first.bounds[1][axis] + EPS_REGION * scale < second.bounds[0][axis]
                    || second.bounds[1][axis] + EPS_REGION * scale < first.bounds[0][axis]
            }) {
                return Ok(None);
            }
        }
    }
    let mut groups: Vec<_> = outer.iter().map(|(lp, _)| vec![*lp]).collect();
    let mut assigned: Vec<Vec<Circle>> = outer.iter().map(|_| Vec::new()).collect();
    for (lp, circle) in holes {
        let Some(index) =
            exactly_one(outer.iter().enumerate().filter_map(|(index, (_, region))| {
                crate::decode::sweep::revolution_cap::convex_polygon_contains_circles(
                    &region.polygon,
                    &[circle],
                )
                .then_some(index)
            }))
        else {
            return Ok(None);
        };
        if assigned[index].iter().any(|previous| {
            let distance = (previous.0[0] - circle.0[0]).hypot(previous.0[1] - circle.0[1]);
            distance <= previous.1 + circle.1 + EPS_REGION * previous.1.max(circle.1).max(1.0)
        }) {
            return Ok(None);
        }
        assigned[index].push(circle);
        groups[index].push(lp);
    }
    groups.sort_by_key(|group| group[0].half_edges()[0]);
    Ok(Some(groups))
}

#[cfg(test)]
mod tests;
