// SPDX-License-Identifier: Apache-2.0
//! Counted-owner surface candidates from source-adjacent A5 chart sides.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::{annotations::AnnotationStorage, AnnotationBuilder};

use super::{invariant_face_carrier_bindings, point_on_nurbs_surface, StandardConsolidatedSource};
use crate::families::b2::counted_owner_tail::CountedOwnerBounds;
use crate::families::standard::records::StandardFaceBounds;

const CHART_PARAMETER_TOLERANCE: f64 = 1.0e-9;
const OWNER_FACE_BOUNDS_TOLERANCE: f64 = 2.0e-3;

pub(super) fn bind_counted_owner_surfaces(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl AnnotationStorage>,
    source: StandardConsolidatedSource<'_>,
    face_bounds: &[Option<StandardFaceBounds>],
    budget: &WorkBudget,
    carriers: &[crate::families::a5a8::records::FreeformSurface],
) -> Result<usize, CodecError> {
    let charts =
        crate::families::consolidated::a5_chart::a5_chart_boundaries(source.data, source.records)
            .into_iter()
            .filter(|chart| {
                chart
                    .sides
                    .iter()
                    .map(|side| side.pos)
                    .collect::<HashSet<_>>()
                    .len()
                    == 4
            })
            .collect::<Vec<_>>();
    let owners = crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(
        source.data,
        source.records,
    )
    .into_iter()
    .filter_map(|relation| {
        let bounds = relation
            .owner
            .tail
            .numeric_bounds(relation.owner.references.len())?;
        Some((relation.owner.source_index, bounds))
    })
    .collect::<Vec<_>>();
    if carriers.is_empty() || charts.is_empty() || owners.is_empty() {
        return Ok(0);
    }
    let carrier_indices = carriers
        .iter()
        .enumerate()
        .map(|(index, carrier)| (carrier.pos, index))
        .collect::<HashMap<_, _>>();
    let mut owner_carriers = Vec::new();
    for (source_index, owner) in &owners {
        let mut candidates = Vec::new();
        for chart in &charts {
            if !budget.charge() {
                return Ok(0);
            }
            if chart.source_index != *source_index || !chart_matches_owner(chart.bounds, *owner) {
                continue;
            }
            if let Some(&index) = carrier_indices.get(&chart.carrier_pos) {
                candidates.push(index);
            }
        }
        owner_carriers.push(candidates);
    }
    let vertex_positions = face_vertices(ir);
    let surface_indices = ir
        .model
        .surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| (surface.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let unknown_faces = ir
        .model
        .faces
        .iter()
        .enumerate()
        .filter_map(|(index, face)| {
            let ordinal = face
                .id
                .as_str()
                .strip_prefix("catia:standard:face#")?
                .parse::<usize>()
                .ok()?;
            let surface = *surface_indices.get(face.surface.as_str())?;
            matches!(
                ir.model.surfaces[surface].geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
            )
            .then_some((index, ordinal, surface))
        })
        .collect::<Vec<_>>();
    if unknown_faces.is_empty() {
        return Ok(0);
    }
    let mut face_domains = Vec::new();
    for &(face, ordinal, _) in &unknown_faces {
        let Some(Some(bounds)) = face_bounds.get(ordinal) else {
            return Ok(0);
        };
        let Some(vertices) = vertex_positions
            .get(face)
            .and_then(Option::as_ref)
            .filter(|points| points.len() >= 3)
        else {
            return Ok(0);
        };
        let mut membership = HashMap::new();
        let mut domain = Vec::new();
        for (owner_index, (_, owner)) in owners.iter().enumerate() {
            if !budget.charge() {
                return Ok(0);
            }
            if owner_carriers[owner_index].is_empty() || !contains_face(*owner, *bounds) {
                continue;
            }
            let mut candidates = Vec::new();
            for &carrier in &owner_carriers[owner_index] {
                if !budget.charge() {
                    return Ok(0);
                }
                let matches = if let Some(&result) = membership.get(&carrier) {
                    result
                } else {
                    let mut result = true;
                    for point in vertices {
                        if point_on_nurbs_surface(ctx, *point, &carriers[carrier].geometry)?
                            != Some(true)
                        {
                            result = false;
                            break;
                        }
                    }
                    membership.insert(carrier, result);
                    result
                };
                if matches {
                    candidates.push(carrier);
                }
            }
            if !candidates.is_empty() {
                domain.push((owner_index, candidates));
            }
        }
        face_domains.push(domain);
    }
    let Some(bindings) =
        invariant_face_carrier_bindings(ctx, &face_domains, owners.len(), Some(budget))?
    else {
        return Ok(0);
    };
    let mut count = 0;
    for ((_, _, surface), carrier) in unknown_faces.into_iter().zip(bindings) {
        let Some(carrier) = carrier else { continue };
        ir.model.surfaces[surface].geometry = SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Nurbs(carriers[carrier].geometry.clone()),
        );
        annotations.derived(&ir.model.surfaces[surface].id, "geometry")?;
        count += 1;
    }
    Ok(count)
}

fn chart_matches_owner(chart: [[f64; 2]; 2], owner: CountedOwnerBounds) -> bool {
    (0..2).all(|axis| {
        [
            (chart[axis][0], owner.lower[axis]),
            (chart[axis][1], owner.upper[axis]),
        ]
        .into_iter()
        .all(|(a, b)| (a - b).abs() <= CHART_PARAMETER_TOLERANCE * a.abs().max(b.abs()).max(1.0))
    })
}

fn contains_face(owner: CountedOwnerBounds, face: StandardFaceBounds) -> bool {
    (0..3).all(|axis| {
        let lower = face.aabb_center[axis].get() - face.aabb_half_extents[axis].get();
        let upper = face.aabb_center[axis].get() + face.aabb_half_extents[axis].get();
        lower >= f64::from(owner.bounds[axis][0]) - OWNER_FACE_BOUNDS_TOLERANCE
            && upper <= f64::from(owner.bounds[axis][1]) + OWNER_FACE_BOUNDS_TOLERANCE
    })
}

fn face_vertices(ir: &CadIr) -> Vec<Option<Vec<Point3>>> {
    let points = ir
        .model
        .points
        .iter()
        .map(|point| (point.id.as_str(), point.position().get()))
        .collect::<HashMap<_, _>>();
    let vertices = ir
        .model
        .vertices
        .iter()
        .filter_map(|vertex| Some((vertex.id.as_str(), *points.get(vertex.point.as_str())?)))
        .collect::<HashMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let coedges = ir
        .model
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<HashMap<_, _>>();
    let loops = ir
        .model
        .loops
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect::<HashMap<_, _>>();
    ir.model
        .faces
        .iter()
        .map(|face| {
            let mut seen = HashSet::new();
            let mut result = Vec::new();
            let mut add_vertex = |id: &cadmpeg_ir::ids::VertexId| {
                if seen.insert(id.as_str().to_owned()) {
                    result.push(*vertices.get(id.as_str())?);
                }
                Some(())
            };
            for loop_id in &face.loops {
                let boundary = loops.get(loop_id.as_str())?;
                if let cadmpeg_ir::topology::LoopBoundary::Vertex { vertex, .. } =
                    &boundary.boundary
                {
                    add_vertex(vertex)?;
                }
                for coedge_id in boundary.coedges() {
                    let coedge = coedges.get(coedge_id.as_str())?;
                    let edge = edges.get(coedge.edge.as_str())?;
                    for id in [&edge.start, &edge.end] {
                        add_vertex(id)?;
                    }
                }
                for vertex_use in boundary.anchored_vertex_uses() {
                    add_vertex(&vertex_use.vertex)?;
                }
            }
            Some(result)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::face_vertices;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::ids::{FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Face, FaceLoops, Loop, LoopBoundary, Point, Sense, Vertex};

    #[test]
    fn face_vertex_witness_includes_vertex_only_loops() {
        let face_id = FaceId::mint("catia:test:face#pole").expect("face id");
        let loop_id = LoopId::mint("catia:test:loop#pole").expect("loop id");
        let vertex_id = VertexId::mint("catia:test:vertex#pole").expect("vertex id");
        let point_id = PointId::mint("catia:test:point#pole").expect("point id");
        let mut ir = CadIr::empty();
        ir.model.points.push(Point::new(
            point_id.clone(),
            FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite point"),
            None,
        ));
        ir.model.vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: None,
        });
        ir.model.loops.push(Loop {
            id: loop_id.clone(),
            face: face_id.clone(),
            boundary: LoopBoundary::Vertex {
                vertex: vertex_id,
                pcurves: Vec::new(),
            },
        });
        ir.model.faces.push(Face {
            id: face_id,
            shell: ShellId::mint("catia:test:shell#pole").expect("shell id"),
            surface: SurfaceId::mint("catia:test:surface#pole").expect("surface id"),
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(vec![loop_id]),
            name: None,
            color: None,
            tolerance: None,
        });
        let witnesses = face_vertices(&ir);
        let [Some(points)] = witnesses.as_slice() else {
            panic!("one complete face witness")
        };
        assert_eq!(points.len(), 1);
        assert!((points[0].z - 3.0).abs() < 1.0e-12);
        ir.model.points.clear();
        assert!(face_vertices(&ir)[0].is_none());
    }
}
