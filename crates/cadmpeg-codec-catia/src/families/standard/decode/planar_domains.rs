// SPDX-License-Identifier: Apache-2.0
//! Planar containment and coupled winding after native boundary curves resolve.

use std::collections::HashMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{FaceLoops, Sense};
use cadmpeg_ir::CadIr;

const CURVED_EDGE_SUBDIVISIONS: u32 = 16;
const PLANAR_SOURCE_ROUNDOFF_FACTOR: f64 = 3.0;

pub(super) fn repair(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<usize, CodecError> {
    let curves: HashMap<_, _> = ir
        .geometry
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect();
    let pcurves: HashMap<_, _> = ir
        .geometry
        .pcurves
        .iter()
        .map(|curve| (&curve.id, curve))
        .collect();
    let edges: HashMap<_, _> = ir
        .geometry
        .edges
        .iter()
        .enumerate()
        .map(|(i, edge)| (&edge.id, i))
        .collect();
    let coedges: HashMap<_, _> = ir
        .geometry
        .coedges
        .iter()
        .enumerate()
        .map(|(i, coedge)| (&coedge.id, i))
        .collect();
    let loops: HashMap<_, _> = ir
        .geometry
        .loops
        .iter()
        .enumerate()
        .map(|(i, boundary)| (&boundary.id, i))
        .collect();
    let surfaces: HashMap<_, _> = ir
        .geometry
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect();
    let point_positions: HashMap<_, _> = ir
        .geometry
        .points
        .iter()
        .map(|point| (&point.id, point.position().get()))
        .collect();
    let vertex_positions: HashMap<_, _> = ir
        .geometry
        .vertices
        .iter()
        .filter_map(|vertex| {
            point_positions
                .get(&vertex.point)
                .map(|point| (&vertex.id, *point))
        })
        .collect();
    let mut uses = HashMap::<usize, Vec<(usize, bool)>>::new();
    for coedge in &ir.geometry.coedges {
        let (Some(&edge), Some(&boundary)) =
            (edges.get(&coedge.edge), loops.get(&coedge.owner_loop))
        else {
            continue;
        };
        uses.entry(edge)
            .or_default()
            .push((boundary, coedge.sense == Sense::Reversed));
    }
    let mut roles = Vec::new();
    let mut constraint = ir.geometry.edges.len();
    for (face_index, face) in ir.geometry.faces.iter().enumerate() {
        let Some(face_loops) = face.loops.boundary().filter(|loops| loops.len() >= 2) else {
            continue;
        };
        let Some(surface) = surfaces.get(&face.surface) else {
            continue;
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = surface else {
            continue;
        };
        let mut rows = Vec::new();
        for id in face_loops {
            let Some(&boundary) = loops.get(id) else {
                continue;
            };
            let mut points = Vec::new();
            for id in ir.geometry.loops[boundary].coedges() {
                let Some(&index) = coedges.get(id) else {
                    points.clear();
                    break;
                };
                let coedge = &ir.geometry.coedges[index];
                let Some(&edge_index) = edges.get(&coedge.edge) else {
                    points.clear();
                    break;
                };
                let edge = &ir.geometry.edges[edge_index];
                let (Some(curve), Some(range), Some(ends)) = (
                    edge.curve().and_then(|id| curves.get(id)),
                    edge.param_range(),
                    &edge.ends,
                ) else {
                    points.clear();
                    break;
                };
                if coedge.use_curve.is_some() {
                    points.clear();
                    break;
                }
                let [mut start, mut end] = range.get();
                let vertex = if coedge.sense == Sense::Reversed {
                    std::mem::swap(&mut start, &mut end);
                    &ends.end
                } else {
                    &ends.start
                };
                let samples =
                    if matches!(curve, CurveGeometry::Solved(SolvedCurveGeometry::Line(_))) {
                        1
                    } else {
                        CURVED_EDGE_SUBDIVISIONS
                    };
                for sample in 0..samples {
                    let point = if sample == 0 {
                        let Some(point) = vertex_positions.get(vertex) else {
                            points.clear();
                            break;
                        };
                        *point
                    } else {
                        let fraction = f64::from(sample) / f64::from(samples);
                        let Some(point) = cadmpeg_ir::eval::finite_or_refusal(
                            cadmpeg_ir::eval::decode::outer_refusal(
                                cadmpeg_ir::eval::decode::curve_point(
                                    ctx,
                                    curve,
                                    start * (1.0 - fraction) + end * fraction,
                                ),
                            )?,
                        )?
                        else {
                            points.clear();
                            break;
                        };
                        point.get()
                    };
                    let Some(point) = project_source_point(point, plane) else {
                        points.clear();
                        break;
                    };
                    points.push(point);
                }
                if points.is_empty() {
                    break;
                }
            }
            rows.push((id.clone(), points));
        }
        let classified =
            crate::boundary_roles::classify_planar_boundaries(surface, &rows, refusal)?;
        let FaceLoops::Classified { outer, inner } = &classified else {
            continue;
        };
        let Some(outer_index) = rows.iter().position(|(id, _)| id == outer) else {
            continue;
        };
        let outer_area = signed_area(&rows[outer_index].1, plane);
        for inner in inner {
            let Some(inner_index) = rows.iter().position(|(id, _)| id == inner) else {
                continue;
            };
            let inner_area = signed_area(&rows[inner_index].1, plane);
            let opposite = outer_area.is_sign_positive() != inner_area.is_sign_positive();
            uses.insert(
                constraint,
                vec![(loops[outer], false), (loops[inner], opposite)],
            );
            constraint += 1;
        }
        roles.push((face_index, classified));
    }
    if roles.is_empty() {
        return Ok(0);
    }
    let Some(flips) = crate::families::standard::topology::solve_boundary_orientation_constraints(
        ctx,
        ir.geometry.loops.len(),
        &uses,
        false,
    )?
    else {
        return Ok(0);
    };
    let mut ring_plans = Vec::new();
    let mut coedge_plans = Vec::new();
    for (index, boundary) in ir.geometry.loops.iter().enumerate() {
        if !flips[index] {
            continue;
        }
        let ids = boundary.coedges();
        if ids.is_empty() {
            return Ok(0);
        }
        let mut order = ids.to_vec();
        order.reverse();
        let next: HashMap<_, _> = ids
            .iter()
            .zip(ids.iter().cycle().skip(1))
            .take(ids.len())
            .collect();
        let mut vertices = boundary.anchored_vertex_uses().to_vec();
        for vertex in &mut vertices {
            let Some(after) = next.get(&vertex.after) else {
                return Ok(0);
            };
            vertex.after = (*after).clone();
        }
        let positions: HashMap<_, _> = order.iter().enumerate().map(|(i, id)| (id, i)).collect();
        vertices.sort_by_key(|vertex| positions[&vertex.after]);
        for id in &order {
            let index = coedges[id];
            let coedge = &ir.geometry.coedges[index];
            if coedge.use_curve.is_some() {
                return Ok(0);
            }
            let mut images = coedge.pcurves.clone();
            for image in &mut images {
                let range = match image.parameter_range {
                    Some(range) => range.endpoints(),
                    None => match pcurves
                        .get(&image.pcurve)
                        .and_then(|curve| curve.parameter_range())
                    {
                        Some(range) => range.get(),
                        None => return Ok(0),
                    },
                };
                image.parameter_range = Some(
                    DirectedParameterRange::new([range[1], range[0]])
                        .map_err(CodecError::malformed)?,
                );
            }
            coedge_plans.push((index, images));
        }
        ring_plans.push((index, order, vertices));
    }
    let changed = roles
        .iter()
        .filter(|(index, role)| ir.geometry.faces[*index].loops != *role)
        .count();
    for (index, images) in coedge_plans {
        let coedge = &mut ir.geometry.coedges[index];
        coedge.sense = if coedge.sense == Sense::Forward {
            Sense::Reversed
        } else {
            Sense::Forward
        };
        coedge.pcurves = images;
    }
    for (index, order, vertices) in ring_plans {
        ir.geometry.loops[index]
            .replace_ring(order, vertices)
            .map_err(CodecError::malformed)?;
    }
    for (index, role) in roles {
        ir.geometry.faces[index].loops = role;
    }
    Ok(changed)
}

fn project_source_point(point: Point3, plane: &PlaneSurface) -> Option<Point3> {
    let origin = plane.origin().get();
    let normal = *plane.frame().axis().as_raw();
    let distance = point.vector_from(origin).dot(normal);
    let scale = [point.x, point.y, point.z, origin.x, origin.y, origin.z]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
    (distance.abs() <= PLANAR_SOURCE_ROUNDOFF_FACTOR * f64::from(f32::EPSILON) * scale)
        .then(|| point.translated(normal, -distance))
}

fn signed_area(points: &[Point3], plane: &PlaneSurface) -> f64 {
    let origin = plane.origin().get();
    let normal = *plane.frame().axis().as_raw();
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| {
            a.vector_from(origin)
                .cross(b.vector_from(origin))
                .dot(normal)
        })
        .sum::<f64>()
        * 0.5
}

#[cfg(test)]
mod tests {
    use super::project_source_point;
    use cadmpeg_ir::geometry::analytic::PlaneSurface;
    use cadmpeg_ir::math::{Point3, Vector3};
    #[test]
    fn source_float_roundoff_projects_onto_the_carrier_plane() {
        let plane = PlaneSurface::try_new(
            Point3::new(0.0, 12.7, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane");
        let projected = project_source_point(Point3::new(1.0, f64::from(12.7_f32), 2.0), &plane)
            .expect("source float roundoff");
        assert!(projected.distance(Point3::new(1.0, 12.7, 2.0)) < 1e-12);
        assert!(project_source_point(Point3::new(1.0, 12.71, 2.0), &plane).is_none());
    }
}
