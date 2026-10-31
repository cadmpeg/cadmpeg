// SPDX-License-Identifier: Apache-2.0
//! Strict containment of circular holes in a planar cap.

const EPS_CAP_CONTAINMENT: f64 = 1.0e-9;

pub(in crate::decode) fn convex_polygon_contains_circles(
    polygon: &[[f64; 2]],
    circles: &[([f64; 2], f64)],
) -> bool {
    if polygon.len() < 3 || polygon.iter().flatten().any(|value| !value.is_finite()) {
        return false;
    }
    let scale = polygon
        .iter()
        .flat_map(|point| {
            [
                (point[0] - polygon[0][0]).abs(),
                (point[1] - polygon[0][1]).abs(),
            ]
        })
        .chain(circles.iter().map(|(_, radius)| *radius))
        .fold(1.0_f64, f64::max);
    let tolerance = EPS_CAP_CONTAINMENT * scale;
    let cross = |first: [f64; 2], second: [f64; 2], point: [f64; 2]| {
        (second[0] - first[0]).mul_add(
            point[1] - first[1],
            -(second[1] - first[1]) * (point[0] - first[0]),
        )
    };
    let winding = cross(polygon[0], polygon[1], polygon[2]).signum();
    if !winding.is_finite() || winding == 0.0 {
        return false;
    }
    for (index, first) in polygon.iter().copied().enumerate() {
        let next = (index + 1) % polygon.len();
        let second = polygon[next];
        let length = (second[0] - first[0]).hypot(second[1] - first[1]);
        if !length.is_finite() || length <= tolerance {
            return false;
        }
        if polygon.iter().copied().enumerate().any(|(vertex, point)| {
            vertex != index
                && vertex != next
                && winding * cross(first, second, point) / length <= tolerance
        }) || circles.iter().any(|(center, radius)| {
            !radius.is_finite()
                || *radius <= 0.0
                || winding * cross(first, second, *center) / length <= *radius + tolerance
        }) {
            return false;
        }
    }
    !circles
        .iter()
        .enumerate()
        .any(|(index, (first, first_radius))| {
            circles
                .iter()
                .skip(index + 1)
                .any(|(second, second_radius)| {
                    (first[0] - second[0]).hypot(first[1] - second[1])
                        <= first_radius + second_radius + tolerance
                })
        })
}

pub(in crate::decode) fn projected_loop_polygon(
    lp: &crate::topology::Loop,
    surface: &cadmpeg_ir::geometry::SurfaceGeometry,
    incidence: &std::collections::BTreeMap<
        crate::topology::HalfEdgeId,
        &crate::topology::HalfEdgeVertexIncidence,
    >,
    vertices: &std::collections::BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<[f64; 2]>> {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = surface else {
        return None;
    };
    (lp.half_edges().len() == 2).then_some(())?;
    let origin = plane.origin().get();
    let normal = plane.frame().axis().as_raw();
    let mut segments = [[[0.0; 2]; 2]; 2];
    for (segment, half_edge) in segments.iter_mut().zip(lp.half_edges()) {
        let binding = incidence.get(half_edge)?;
        for (uv, vertex) in segment
            .iter_mut()
            .zip([binding.start_vertex_id, binding.end_vertex_id?])
        {
            let point = cadmpeg_ir::math::Point3::from(*vertices.get(&vertex.get())?);
            let delta = point.vector_from(origin);
            let scale = delta.norm().max(1.0);
            (delta.dot(*normal).abs() <= EPS_CAP_PROJECTION * scale).then_some(())?;
            let parameters = cadmpeg_ir::eval::analytic_surface_parameters(surface, point)?.get();
            *uv = [parameters.u, parameters.v];
        }
    }
    let agrees = |first: [f64; 2], second: [f64; 2]| {
        let scale = first
            .into_iter()
            .chain(second)
            .map(f64::abs)
            .fold(1.0, f64::max);
        first
            .into_iter()
            .zip(second)
            .all(|(first, second)| (first - second).abs() <= EPS_CAP_PROJECTION * scale)
    };
    (agrees(segments[0][1], segments[1][0]) && agrees(segments[1][1], segments[0][0]))
        .then_some(())?;
    Some(vec![segments[0][0], segments[1][0]])
}

const EPS_CAP_PROJECTION: f64 = 1.0e-9;
