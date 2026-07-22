// SPDX-License-Identifier: Apache-2.0
//! Face-layer transfer: face ownership components, loop orientation solving,
//! and the body/shell/face/loop/coedge emit pass.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, EdgeId, FaceId, LoopId, PcurveId, RegionId, ShellId, SurfaceId,
};
use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Face, Loop, Region, Sense, Shell};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::super::graph::B5Graph;
use super::{annotate, OrientedLoop, OwnershipPlan, TransferPlan};
use crate::solve::UnionFind;

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
    let mut closed_components = vec![true; component_faces.len()];
    let mut component_has_edges = vec![false; component_faces.len()];
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
        if flip {
            member_order.reverse();
            for sense in reversed.get_mut(&loop_id)? {
                *sense = !*sense;
            }
        }
        oriented.insert(
            loop_id,
            OrientedLoop {
                member_order,
                reversed: reversed.remove(&loop_id)?,
            },
        );
    }
    Some(oriented)
}

/// Emit the single body, its ownership-derived regions and shells, and every
/// face with its loops and coedges, closing radial-next rings by shared edge.
pub(super) fn emit_faces(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    plan: &TransferPlan,
    surface_ids: &HashMap<u32, SurfaceId>,
    pcurve_ids: &HashMap<(u32, usize), PcurveId>,
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
            .derived(&face_id, "sense")
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
        for loop_id_value in &face.loops {
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
            annotate(
                annotations,
                &loop_id,
                "object_stream_b5_03",
                "62_loop",
                Exactness::ByteExact,
            );
            annotations
                .derived(&loop_id, "face")
                .derived(&loop_id, "coedges");
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                coedges: coedge_ids.clone(),
                vertex_uses: Vec::new(),
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
                    pcurves: pcurve_ids
                        .get(&(loop_.object_id, member))
                        .map(|pcurve| cadmpeg_ir::topology::PcurveUse {
                            pcurve: pcurve.clone(),
                            isoparametric: None,
                            parameter_range: None,
                        })
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
