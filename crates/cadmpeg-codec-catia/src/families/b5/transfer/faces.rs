// SPDX-License-Identifier: Apache-2.0
//! Face-layer transfer: face ownership components, loop orientation solving,
//! and the body/shell/face/loop/coedge emit pass.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{PcurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, EdgeId, FaceId, LoopId, PcurveId, RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Face, Loop, LoopBoundaryRole, Region, Sense, Shell, VertexUse,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::super::graph::B5Graph;
use super::{annotate, OrientedLoop, OwnershipPlan, TransferPlan};
use crate::solve::UnionFind;

const EPS_PLANE_AXES_ORTHO: f64 = 1.0e-8;

pub(super) fn ownership_plan(graph: &B5Graph) -> Option<OwnershipPlan> {
    let mut face_ids = HashSet::new();
    let mut loop_owners = HashMap::<u32, usize>::new();
    for (face_index, face) in graph.faces.iter().enumerate() {
        if !face_ids.insert(face.object_id) || face.loops.is_empty() {
            return None;
        }
        for loop_id in &face.loops {
            if loop_owners.insert(*loop_id, face_index).is_some() {
                return None;
            }
        }
    }
    if loop_owners.len() != graph.loops.len()
        || graph.loops.iter().any(|(loop_id, loop_)| {
            loop_id != &loop_.object_id || !loop_owners.contains_key(loop_id)
        })
    {
        return None;
    }

    let vertex_count = graph
        .vertex_points
        .len()
        .checked_add(graph.logical_vertex_points.len())?;
    let mut parents = UnionFind::new(graph.faces.len());
    let mut first_face_by_edge = HashMap::<u32, usize>::new();
    let mut edge_uses = HashMap::<u32, usize>::new();
    for (loop_id, loop_) in &graph.loops {
        let face = loop_owners[loop_id];
        for edge in &loop_.edges {
            let endpoints = graph.edge_vertices.get(edge)?;
            if endpoints.iter().any(|endpoint| *endpoint >= vertex_count) {
                return None;
            }
            *edge_uses.entry(*edge).or_default() += 1;
            if let Some(other_face) = first_face_by_edge.insert(*edge, face) {
                parents.union(face, other_face);
            }
        }
    }

    let mut labels = HashMap::<usize, usize>::new();
    let mut face_components = Vec::with_capacity(graph.faces.len());
    for face in 0..graph.faces.len() {
        let root = parents.find(face);
        let next = labels.len();
        face_components.push(*labels.entry(root).or_insert(next));
    }
    let mut component_faces = vec![Vec::new(); labels.len()];
    for (face, component) in face_components.iter().copied().enumerate() {
        component_faces[component].push(face);
    }
    let mut closed_components = cadmpeg_core::decode::alloc_filled(
        component_faces.len(),
        true,
        "catia b5 closed components",
    )
    .ok()?;
    let mut component_has_edges = cadmpeg_core::decode::alloc_filled(
        component_faces.len(),
        false,
        "catia b5 component edge marks",
    )
    .ok()?;
    for (&edge, &uses) in &edge_uses {
        let component = face_components[first_face_by_edge[&edge]];
        component_has_edges[component] = true;
        closed_components[component] &= uses == 2;
    }
    let closed_component_count = closed_components
        .iter()
        .zip(component_has_edges)
        .filter(|(closed, has_edges)| **closed && *has_edges)
        .count();
    let body_kind = if edge_uses.values().any(|uses| *uses > 2)
        || (closed_component_count != 0 && closed_component_count != component_faces.len())
    {
        BodyKind::General
    } else if closed_component_count == component_faces.len() && !component_faces.is_empty() {
        BodyKind::Solid
    } else {
        BodyKind::Sheet
    };
    Some(OwnershipPlan {
        body_kind,
        components: component_faces,
        face_components,
        loop_owners,
    })
}

pub(super) fn orient_loop_members(
    graph: &B5Graph,
    mut reversed: BTreeMap<u32, Vec<bool>>,
) -> Option<BTreeMap<u32, OrientedLoop>> {
    let loop_ids: Vec<u32> = graph.loops.keys().copied().collect();
    let node_by_loop: HashMap<u32, usize> = loop_ids
        .iter()
        .enumerate()
        .map(|(node, loop_id)| (*loop_id, node))
        .collect();
    if reversed.len() != loop_ids.len()
        || loop_ids.iter().any(|loop_id| {
            reversed
                .get(loop_id)
                .is_none_or(|senses| senses.len() != graph.loops[loop_id].edges.len())
        })
    {
        return None;
    }

    let mut uses = HashMap::<u32, Vec<(usize, bool)>>::new();
    for loop_id in &loop_ids {
        let node = node_by_loop[loop_id];
        for (&edge, &sense) in graph.loops[loop_id].edges.iter().zip(&reversed[loop_id]) {
            uses.entry(edge).or_default().push((node, sense));
        }
    }
    let mut constraints = vec![Vec::<(usize, bool)>::new(); loop_ids.len()];
    for occurrences in uses.values().filter(|occurrences| occurrences.len() == 2) {
        let [(left, left_reversed), (right, right_reversed)] = occurrences.as_slice() else {
            unreachable!("filtered to two occurrences");
        };
        let parity = left_reversed == right_reversed;
        if left == right {
            if parity {
                return None;
            }
        } else {
            constraints[*left].push((*right, parity));
            constraints[*right].push((*left, parity));
        }
    }

    let mut flips = vec![None; loop_ids.len()];
    for root in 0..loop_ids.len() {
        if flips[root].is_some() {
            continue;
        }
        flips[root] = Some(false);
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            let flip = flips[node]?;
            for &(neighbor, parity) in &constraints[node] {
                let required = flip ^ parity;
                match flips[neighbor] {
                    Some(existing) if existing != required => return None,
                    Some(_) => {}
                    None => {
                        flips[neighbor] = Some(required);
                        pending.push(neighbor);
                    }
                }
            }
        }
    }

    let mut oriented = BTreeMap::new();
    for (node, loop_id) in loop_ids.into_iter().enumerate() {
        let member_count = graph.loops[&loop_id].edges.len();
        let flip = flips[node]?;
        let mut member_order: Vec<usize> = (0..member_count).collect();
        let mut pcurve_reversed = graph.loops[&loop_id].pcurve_senses();
        if pcurve_reversed.len() != member_count {
            return None;
        }
        if flip {
            member_order.reverse();
            for sense in reversed.get_mut(&loop_id)? {
                *sense = !*sense;
            }
            for sense in &mut pcurve_reversed {
                *sense = !*sense;
            }
        }
        oriented.insert(
            loop_id,
            OrientedLoop {
                member_order,
                reversed: reversed.remove(&loop_id)?,
                pcurve_reversed,
            },
        );
    }
    Some(oriented)
}

fn b5_plane_point(
    origin: Point3,
    u_axis: cadmpeg_ir::math::Vector3,
    v_axis: cadmpeg_ir::math::Vector3,
    uv: Point2,
) -> Point3 {
    Point3::new(
        origin.x + uv.u * u_axis.x + uv.v * v_axis.x,
        origin.y + uv.u * u_axis.y + uv.v * v_axis.y,
        origin.z + uv.u * u_axis.z + uv.v * v_axis.z,
    )
}

fn b5_planar_loop_points(
    ir: &CadIr,
    graph: &B5Graph,
    loop_id: u32,
    loop_orientation: &OrientedLoop,
    surface_id: &SurfaceId,
    pcurve_uses: &HashMap<(u32, usize), (PcurveId, [f64; 2])>,
) -> Option<Vec<Point3>> {
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == *surface_id)?;
    let SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = &surface.geometry
    else {
        return None;
    };
    let normal = normal.unit()?;
    let u_axis = u_axis.unit()?;
    if normal.dot(u_axis).abs() > EPS_PLANE_AXES_ORTHO {
        return None;
    }
    let v_axis = normal.cross(u_axis).unit()?;
    let loop_ = graph.loops.get(&loop_id)?;
    let mut points = Vec::with_capacity(loop_.edges.len());
    for &member in &loop_orientation.member_order {
        let edge = loop_.edges[member];
        let endpoints = graph.edge_vertices.get(&edge)?;
        let endpoint_indices = if loop_orientation.reversed[member] {
            [endpoints[1], endpoints[0]]
        } else {
            *endpoints
        };
        let [Some(start), Some(end)] = endpoint_indices.map(|index| {
            super::b5_vertex_point(graph, index)
                .map(|point| Point3::new(point[0], point[1], point[2]))
        }) else {
            return None;
        };
        let (pcurve_id, parameter_range) = pcurve_uses.get(&(loop_id, member))?;
        if !parameter_range
            .iter()
            .all(|parameter| parameter.is_finite())
            || parameter_range[0] == parameter_range[1]
        {
            return None;
        }
        let pcurve = ir
            .model
            .pcurves
            .iter()
            .find(|pcurve| pcurve.id == *pcurve_id)?;
        let PcurveGeometry::Line {
            origin: uv_origin,
            direction,
        } = &pcurve.geometry
        else {
            return None;
        };
        let uv_endpoints = parameter_range.map(|parameter| {
            Point2::new(
                uv_origin.u + parameter * direction.u,
                uv_origin.v + parameter * direction.v,
            )
        });
        let lifted = uv_endpoints.map(|uv| b5_plane_point(*origin, u_axis, v_axis, uv));
        let forward_error = lifted[0].distance(start).max(lifted[1].distance(end));
        let reverse_error = lifted[1].distance(start).max(lifted[0].distance(end));
        let error = forward_error.min(reverse_error);
        if !error.is_finite() || error > 2e-3 {
            return None;
        }
        points.push(start);
    }
    Some(points)
}

fn b5_boundary_roles(
    ir: &CadIr,
    graph: &B5Graph,
    face: &super::super::graph::B5Face,
    loop_orientation: &BTreeMap<u32, OrientedLoop>,
    surface_ids: &HashMap<u32, SurfaceId>,
    pcurve_uses: &HashMap<(u32, usize), (PcurveId, [f64; 2])>,
) -> Vec<LoopBoundaryRole> {
    if face.loops.len() == 1 {
        return vec![LoopBoundaryRole::Outer];
    }
    let unspecified = vec![LoopBoundaryRole::Unspecified; face.loops.len()];
    let Some(surface_id) = surface_ids.get(&face.surface) else {
        return unspecified;
    };
    let Some(boundaries) = face
        .loops
        .iter()
        .map(|loop_id| {
            b5_planar_loop_points(
                ir,
                graph,
                *loop_id,
                loop_orientation.get(loop_id)?,
                surface_id,
                pcurve_uses,
            )
        })
        .collect::<Option<Vec<_>>>()
    else {
        return unspecified;
    };
    let Some(surface) = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == *surface_id)
    else {
        return unspecified;
    };
    crate::boundary_roles::classify_planar_boundary_roles(&surface.geometry, &boundaries)
}

/// Emit the single body, its ownership-derived regions and shells, and every
/// face with its loops and coedges, closing radial-next rings by shared edge.
pub(super) fn emit_faces(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    plan: &TransferPlan,
    surface_ids: &HashMap<u32, SurfaceId>,
    pcurve_uses: &HashMap<(u32, usize), (PcurveId, [f64; 2])>,
    edge_id_map: &HashMap<u32, EdgeId>,
) {
    let ownership = &plan.ownership;
    let loop_orientation = &plan.loop_orientation;

    let body_id = BodyId("catia:b5:body#0".to_string());
    let region_ids: Vec<RegionId> = (0..ownership.components.len())
        .map(|component| RegionId(format!("catia:b5:region#{component}")))
        .collect();
    annotate(
        annotations,
        &body_id,
        "object_stream_b5_03",
        "single_body",
        Exactness::Inferred,
    );
    annotations
        .derived(&body_id, "kind")
        .derived(&body_id, "regions");
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: ownership.body_kind,
        regions: region_ids.clone(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    for (component_index, component_faces) in ownership.components.iter().enumerate() {
        let region_id = region_ids[component_index].clone();
        let shell_id = ShellId(format!("catia:b5:shell#{component_index}"));
        annotate(
            annotations,
            &region_id,
            "object_stream_b5_03",
            "derived_region",
            Exactness::Inferred,
        );
        annotations
            .derived(&region_id, "body")
            .derived(&region_id, "shells");
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id.clone()],
        });
        annotate(
            annotations,
            &shell_id,
            "object_stream_b5_03",
            "derived_shell",
            Exactness::Inferred,
        );
        annotations
            .derived(&shell_id, "region")
            .derived(&shell_id, "faces");
        ir.model.shells.push(Shell {
            id: shell_id,
            region: region_id,
            faces: component_faces
                .iter()
                .map(|face| FaceId(format!("catia:b5:face#{}", graph.faces[*face].object_id)))
                .collect(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
    }

    let mut coedges_by_edge = HashMap::<u32, Vec<usize>>::new();
    for (face_index, face) in graph.faces.iter().enumerate() {
        let face_id = FaceId(format!("catia:b5:face#{}", face.object_id));
        let shell_id = ShellId(format!(
            "catia:b5:shell#{}",
            ownership.face_components[face_index]
        ));
        let boundary_roles =
            b5_boundary_roles(ir, graph, face, loop_orientation, surface_ids, pcurve_uses);
        annotate(
            annotations,
            &face_id,
            "object_stream_b5_03",
            "5f_face",
            Exactness::Inferred,
        );
        annotations
            .derived(&face_id, "shell")
            .derived(&face_id, "surface")
            .derived(&face_id, "loops");
        ir.model.faces.push(Face {
            id: face_id.clone(),
            shell: shell_id.clone(),
            surface: surface_ids[&face.surface].clone(),
            sense: Sense::Forward,
            loops: face
                .loops
                .iter()
                .map(|loop_id| LoopId(format!("catia:b5:loop#{loop_id}")))
                .collect(),
            name: None,
            color: None,
            tolerance: None,
        });
        for (loop_position, loop_id_value) in face.loops.iter().enumerate() {
            let loop_ = &graph.loops[loop_id_value];
            let orientation = &loop_orientation[loop_id_value];
            let senses = &orientation.reversed;
            let member_order = &orientation.member_order;
            let loop_id = LoopId(format!("catia:b5:loop#{loop_id_value}"));
            let coedge_ids_by_member: Vec<CoedgeId> = (0..loop_.edges.len())
                .map(|index| CoedgeId(format!("catia:b5:coedge#{loop_id_value}-{index}")))
                .collect();
            let coedge_ids: Vec<CoedgeId> = member_order
                .iter()
                .map(|member| coedge_ids_by_member[*member].clone())
                .collect();
            let vertex_uses: Vec<VertexUse> = member_order
                .iter()
                .map(|&member| {
                    let edge = loop_.edges[member];
                    let endpoints = graph.edge_vertices[&edge];
                    let endpoint = endpoints[1 - usize::from(senses[member])];
                    VertexUse {
                        vertex: VertexId(format!("catia:b5:vertex#{endpoint}")),
                        after: Some(coedge_ids_by_member[member].clone()),
                        pcurves: Vec::new(),
                    }
                })
                .collect();
            annotate(
                annotations,
                &loop_id,
                "object_stream_b5_03",
                "62_loop",
                Exactness::ByteExact,
            );
            annotations
                .derived(&loop_id, "face")
                .derived(&loop_id, "coedges")
                .derived(&loop_id, "vertex_uses");
            let boundary_role = boundary_roles
                .get(loop_position)
                .copied()
                .unwrap_or_default();
            if boundary_role != LoopBoundaryRole::Unspecified {
                annotations.derived(&loop_id, "boundary_role");
            }
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                boundary_role,
                coedges: coedge_ids.clone(),
                vertex_uses,
            });
            for (position, &member) in member_order.iter().enumerate() {
                let edge = loop_.edges[member];
                let reversed = senses[member];
                let id = coedge_ids_by_member[member].clone();
                annotate(
                    annotations,
                    &id,
                    "object_stream_b5_03",
                    "serialized_loop_member",
                    Exactness::ByteExact,
                );
                for field in [
                    "owner_loop",
                    "edge",
                    "next",
                    "previous",
                    "radial_next",
                    "sense",
                    "pcurves",
                ] {
                    annotations.derived(&id, field);
                }
                let arena_index = ir.model.coedges.len();
                coedges_by_edge.entry(edge).or_default().push(arena_index);
                ir.model.coedges.push(Coedge {
                    id: id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge_id_map[&edge].clone(),
                    next: coedge_ids[(position + 1) % coedge_ids.len()].clone(),
                    previous: coedge_ids[(position + coedge_ids.len() - 1) % coedge_ids.len()]
                        .clone(),
                    radial_next: id,
                    sense: if reversed {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    pcurves: pcurve_uses
                        .get(&(loop_.object_id, member))
                        .map(
                            |(pcurve, parameter_range)| cadmpeg_ir::topology::PcurveUse {
                                pcurve: pcurve.clone(),
                                isoparametric: None,
                                parameter_range: orientation.pcurve_reversed[member]
                                    .then_some([parameter_range[1], parameter_range[0]]),
                            },
                        )
                        .into_iter()
                        .collect(),
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
            }
        }
    }
    for occurrences in coedges_by_edge.values() {
        for (position, &arena_index) in occurrences.iter().enumerate() {
            let radial = occurrences[(position + 1) % occurrences.len()];
            ir.model.coedges[arena_index].radial_next = ir.model.coedges[radial].id.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{Pcurve, PcurveGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::{PcurveId, SurfaceId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::LoopBoundaryRole;
    use cadmpeg_ir::units::Units;

    use super::super::super::graph::{B5Face, B5Graph, B5Loop, B5LoopMetadata};
    use super::{b5_boundary_roles, OrientedLoop};

    #[test]
    fn planar_line_pcurve_faces_derive_roles_from_containment() {
        let outer = [
            [0.0, 0.0, 0.0],
            [5.0, 0.0, 0.0],
            [5.0, 5.0, 0.0],
            [0.0, 5.0, 0.0],
        ];
        let inner = [
            [1.0, 1.0, 0.0],
            [2.0, 1.0, 0.0],
            [2.0, 2.0, 0.0],
            [1.0, 2.0, 0.0],
        ];
        let points = outer.into_iter().chain(inner).collect::<Vec<_>>();
        let mut edge_vertices = BTreeMap::new();
        let mut loops = BTreeMap::new();
        let mut pcurves = Vec::new();
        let mut pcurve_uses = HashMap::new();
        let mut orientations = BTreeMap::new();
        for (loop_id, vertices, edge_base, pcurve_base) in
            [(2, 0..4, 100, 1000), (3, 4..8, 200, 2000)]
        {
            let vertices = vertices.collect::<Vec<_>>();
            let mut loop_pcurves = Vec::new();
            let mut loop_edges = Vec::new();
            for member in 0..4 {
                let start = vertices[member];
                let end = vertices[(member + 1) % vertices.len()];
                let edge = edge_base + member as u32;
                let pcurve = pcurve_base + member as u32;
                edge_vertices.insert(edge, [start, end]);
                loop_edges.push(edge);
                loop_pcurves.push(pcurve);
                let start_point = points[start];
                let end_point = points[end];
                pcurves.push(Pcurve {
                    id: PcurveId(format!("pc#{pcurve}")),
                    geometry: PcurveGeometry::Line {
                        origin: Point2::new(start_point[0], start_point[1]),
                        direction: Point2::new(
                            end_point[0] - start_point[0],
                            end_point[1] - start_point[1],
                        ),
                    },
                    wrapper_reversed: None,
                    native_tail_flags: None,
                    parameter_range: Some([0.0, 1.0]),
                    fit_tolerance: None,
                });
                pcurve_uses.insert(
                    (loop_id, member),
                    (PcurveId(format!("pc#{pcurve}")), [0.0, 1.0]),
                );
            }
            loops.insert(
                loop_id,
                B5Loop {
                    object_id: loop_id,
                    pcurves: loop_pcurves,
                    edges: loop_edges,
                    metadata: B5LoopMetadata {
                        framing_controls: [0, 0],
                        edge_controls: vec![[0, 0, 0]; 4],
                        extension: None,
                    },
                    surface: 10,
                },
            );
            orientations.insert(
                loop_id,
                OrientedLoop {
                    member_order: vec![0, 1, 2, 3],
                    reversed: vec![false; 4],
                    pcurve_reversed: vec![false; 4],
                },
            );
        }
        let graph = B5Graph {
            complete: true,
            faces: vec![B5Face {
                object_id: 1,
                surface: 10,
                loops: vec![3, 2],
                terminal_control: Some(3),
            }],
            face_records: BTreeMap::new(),
            loops,
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            surface_aliases: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            extrusion_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::new(),
            edges: BTreeMap::new(),
            vertex_incidence_links: BTreeMap::new(),
            vertex_points: points,
            logical_vertex_points: Vec::new(),
            logical_vertex_refs: Vec::new(),
            edge_vertices,
            edge_parameter_incidences: BTreeMap::new(),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };
        let mut ir = CadIr::empty(Units::default());
        ir.model.surfaces.push(Surface {
            id: SurfaceId("surface#10".to_string()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        ir.model.pcurves = pcurves;

        assert_eq!(
            b5_boundary_roles(
                &ir,
                &graph,
                &graph.faces[0],
                &orientations,
                &HashMap::from([(10, SurfaceId("surface#10".to_string()))]),
                &pcurve_uses,
            ),
            vec![LoopBoundaryRole::Inner, LoopBoundaryRole::Outer]
        );
    }
}
