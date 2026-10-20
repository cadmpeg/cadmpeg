// SPDX-License-Identifier: Apache-2.0
//! Face-layer transfer: face ownership components, loop orientation solving,
//! and the body/shell/face/loop/coedge emit pass.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::topology::{
    AnchoredVertexUse, Body, BodyKind, Coedge, Face, FaceLoops, Loop, LoopBoundaryRole, Region,
    Sense, Shell,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::super::graph::B5Graph;
use super::pcurves::PcurveUses;
use super::{annotate, OrientedLoop, OrientedLoopMember, OwnershipPlan, TransferPlan};
use crate::solve::union_find::UnionFind;

pub(super) fn ownership_plan(
    ctx: &DecodeContext<'_>,
    graph: &B5Graph,
) -> Result<Option<OwnershipPlan>, CodecError> {
    if !graph.complete || graph.faces.is_empty() {
        return Ok(None);
    }
    let mut face_ids = HashSet::new();

    let mut loop_owners = HashMap::<u32, usize>::new();
    for (face_index, face) in graph.faces.iter().enumerate() {
        if !face_ids.insert(face.object_id) || face.loops.is_empty() {
            return Ok(None);
        }
        for loop_id in &face.loops {
            if loop_owners.insert(*loop_id, face_index).is_some() {
                return Ok(None);
            }
        }
    }
    if loop_owners.len() != graph.loops.len()
        || graph.loops.iter().any(|(loop_id, loop_)| {
            loop_id != &loop_.object_id || !loop_owners.contains_key(loop_id)
        })
    {
        return Ok(None);
    }

    let mut parents = UnionFind::new(graph.faces.len());
    let mut first_face_by_edge = HashMap::<u32, usize>::new();
    let mut edge_uses = HashMap::<u32, usize>::new();
    for (loop_id, loop_) in &graph.loops {
        let face = loop_owners[loop_id];
        for member in &loop_.members {
            let edge = member.edge;
            if !graph.vertices.edges().contains_key(&edge) {
                return Ok(None);
            }
            if let Some(count) = edge_uses.get_mut(&edge) {
                *count += 1;
            } else {
                edge_uses.insert(edge, 1);
            }
            if let Some(other_face) = first_face_by_edge.insert(edge, face) {
                parents.union(face, other_face)?;
            }
        }
    }

    let mut labels = HashMap::<usize, usize>::new();

    let mut face_components = Vec::new();

    for face in 0..graph.faces.len() {
        let root = parents.find(face)?;
        let next = labels.len();
        face_components.push(*labels.entry(root).or_insert(next));
    }
    let component_count = labels.len();
    let mut closed_components =
        ctx.alloc_filled(component_count, true, "catia b5 closed components")?;
    let mut component_has_edges =
        ctx.alloc_filled(component_count, false, "catia b5 component edge marks")?;
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
        || (closed_component_count != 0 && closed_component_count != component_count)
    {
        BodyKind::General
    } else if closed_component_count == component_count && component_count != 0 {
        BodyKind::Solid
    } else {
        BodyKind::Sheet
    };
    Ok(Some(OwnershipPlan {
        body_kind,
        face_components,
        loop_owners,
    }))
}

pub(super) fn orient_loop_members(
    ctx: &DecodeContext<'_>,
    graph: &B5Graph,
    mut reversed: BTreeMap<u32, Vec<bool>>,
) -> Result<Option<BTreeMap<u32, OrientedLoop>>, CodecError> {
    let mut loop_ids = Vec::new();

    loop_ids.extend(graph.loops.keys().copied());
    let mut node_by_loop = HashMap::new();

    for (node, loop_id) in loop_ids.iter().enumerate() {
        node_by_loop.insert(*loop_id, node);
    }
    if reversed.len() != loop_ids.len()
        || loop_ids.iter().any(|loop_id| {
            reversed
                .get(loop_id)
                .is_none_or(|senses| senses.len() != graph.loops[loop_id].members.len())
        })
    {
        return Ok(None);
    }

    let mut uses = HashMap::<u32, Vec<(usize, bool)>>::new();
    for loop_id in &loop_ids {
        let node = node_by_loop[loop_id];
        for (member, &sense) in graph.loops[loop_id].members.iter().zip(&reversed[loop_id]) {
            uses.entry(member.edge).or_default().push((node, sense));
        }
    }
    let mut constraints = ctx.alloc_filled(
        loop_ids.len(),
        Vec::<(usize, bool)>::new(),
        "catia b5 loop orientation constraints",
    )?;
    for [(left, left_reversed), (right, right_reversed)] in uses
        .values()
        .filter_map(|occurrences| <&[_; 2]>::try_from(occurrences.as_slice()).ok())
    {
        let parity = left_reversed == right_reversed;
        if left == right {
            if parity {
                return Ok(None);
            }
        } else {
            constraints[*left].push((*right, parity));
            constraints[*right].push((*left, parity));
        }
    }

    let mut flips = ctx.alloc_filled(
        loop_ids.len(),
        None,
        "catia b5 loop orientation assignments",
    )?;
    for root in 0..loop_ids.len() {
        if flips[root].is_some() {
            continue;
        }
        flips[root] = Some(false);
        let mut pending = Vec::new();
        pending.push((root, false));
        while let Some((node, flip)) = pending.pop() {
            for &(neighbor, parity) in &constraints[node] {
                let required = flip ^ parity;
                match flips[neighbor] {
                    Some(existing) if existing != required => return Ok(None),
                    Some(_) => {}
                    None => {
                        flips[neighbor] = Some(required);
                        pending.push((neighbor, required));
                    }
                }
            }
        }
    }

    let mut oriented = BTreeMap::new();
    for (node, loop_id) in loop_ids.into_iter().enumerate() {
        let Some(flipped) = flips[node] else {
            return Ok(None);
        };
        let Some(senses) = reversed.remove(&loop_id) else {
            return Ok(None);
        };
        let pcurve_senses = graph.loops[&loop_id].pcurve_senses();
        let members = senses
            .into_iter()
            .zip(pcurve_senses)
            .map(|(reversed, pcurve_reversed)| OrientedLoopMember {
                reversed: reversed ^ flipped,
                pcurve_reversed: pcurve_reversed ^ flipped,
            })
            .collect::<Vec<_>>();
        oriented.insert(loop_id, OrientedLoop { flipped, members });
    }
    Ok(Some(oriented))
}

/// Use each loop's native location control. Face-to-carrier orientation is
/// independent of the outer/inner classification.
fn b5_face_loops(
    _ctx: &DecodeContext<'_>,
    graph: &B5Graph,
    face: &super::super::graph::B5Face,
) -> Result<FaceLoops, CodecError> {
    let mut ids = Vec::new();
    let mut outer = None;
    let mut inner = Vec::new();
    let mut unique_outer = true;
    for &loop_id in &face.loops {
        let id = crate::resource::compose_index_id(
            &cadmpeg_ir::identity_namespace!("catia", "b5", "loop"),
            usize::try_from(loop_id).map_err(|_| {
                CodecError::malformed(format_args!(
                    "{} exceeds its local bound",
                    "catia_b5_face_loop_id"
                ))
            })?,
            LoopId::mint,
        )?;
        let role_id = id.clone();
        ids.push(id);
        let Some(loop_) = graph.loops.get(&loop_id) else {
            unique_outer = false;
            continue;
        };
        match loop_.metadata.framing_controls[1] {
            super::super::graph::controls::B5FramingControl::Control05 => {
                if outer.replace(role_id).is_some() {
                    unique_outer = false;
                }
            }
            super::super::graph::controls::B5FramingControl::Control03 => {
                inner.push(role_id);
            }
        }
    }
    Ok(match outer {
        Some(outer) if unique_outer => FaceLoops::classified(outer, inner),
        _ => FaceLoops::unspecified(ids),
    })
}

/// References to the records emitted by the preceding B5 passes.
pub(super) struct EmittedFaceInputs<'a> {
    pub(super) surface_ids: &'a HashMap<u32, SurfaceId>,
    pub(super) pcurve_uses: &'a PcurveUses,
    pub(super) edge_ids: &'a HashMap<u32, EdgeId>,
}

/// Emit the single body, its ownership-derived regions and shells, and every
/// face with its loops and coedges, closing radial-next rings by shared edge.
pub(super) fn emit_faces(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    graph: &B5Graph,
    plan: &TransferPlan,
    emitted: &EmittedFaceInputs<'_>,
    _refusal: &mut crate::nurbs::LaneRefusals,
    admission: &mut crate::families::FamilyEntityAdmission<'_, '_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let surface_ids = emitted.surface_ids;
    let pcurve_uses = emitted.pcurve_uses;
    let edge_id_map = emitted.edge_ids;
    let ownership = &plan.ownership;
    let components = ownership.components();
    let loop_orientation = &plan.loop_orientation;

    let body_id = crate::resource::compose_index_id(
        &cadmpeg_ir::identity_namespace!("catia", "b5", "body"),
        0,
        BodyId::mint,
    )?;
    let mut region_ids = BTreeMap::new();
    for &component in components.keys() {
        let id = crate::resource::compose_index_id(
            &cadmpeg_ir::identity_namespace!("catia", "b5", "region"),
            component,
            RegionId::mint,
        )?;
        region_ids.insert(component, id);
    }
    annotate(
        annotations,
        &body_id,
        "object_stream_b5_03",
        "single_body",
        Exactness::Inferred,
    )?;
    for field in ["kind", "regions"] {
        annotations.derived(body_id.as_str(), field)?;
    }
    let mut body_regions = Vec::new();
    for id in region_ids.values() {
        let id = id.clone();
        body_regions.push(id);
    }
    let body_record_id = body_id.clone();
    admission.charge()?;
    ir.model.bodies.push(Body {
        id: body_record_id,
        kind: ownership.body_kind,
        regions: body_regions,
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    for (component_index, component_faces) in &components {
        let region_id = region_ids[component_index].clone();
        let shell_id = crate::resource::compose_index_id(
            &cadmpeg_ir::identity_namespace!("catia", "b5", "shell"),
            *component_index,
            ShellId::mint,
        )?;
        annotate(
            annotations,
            &region_id,
            "object_stream_b5_03",
            "derived_region",
            Exactness::Inferred,
        )?;
        for field in ["body", "shells"] {
            annotations.derived(region_id.as_str(), field)?;
        }
        let region_record_id = region_id.clone();
        let region_body_id = body_id.clone();
        let region_shell_id = shell_id.clone();
        let region_shells = vec![region_shell_id];
        admission.charge()?;
        ir.model.regions.push(Region {
            id: region_record_id,
            body: region_body_id,
            shells: region_shells,
        });
        annotate(
            annotations,
            &shell_id,
            "object_stream_b5_03",
            "derived_shell",
            Exactness::Inferred,
        )?;
        for field in ["region", "faces"] {
            annotations.derived(shell_id.as_str(), field)?;
        }
        let mut shell_faces = Vec::new();
        for face in component_faces {
            let face_id = crate::resource::compose_u32_id(
                &cadmpeg_ir::identity_namespace!("catia", "b5", "face"),
                graph.faces[*face].object_id,
                FaceId::mint,
            )?;
            shell_faces.push(face_id);
        }
        admission.charge()?;
        ir.model.shells.push(
            match Shell::new(shell_id, region_id, shell_faces, Vec::new(), Vec::new()) {
                Ok(shell) => shell,
                Err(_) => {
                    return Ok(false);
                }
            },
        );
    }

    let mut coedges_by_edge = HashMap::<u32, Vec<usize>>::new();
    for (face_index, face) in graph.faces.iter().enumerate() {
        let face_id = crate::resource::compose_u32_id(
            &cadmpeg_ir::identity_namespace!("catia", "b5", "face"),
            face.object_id,
            FaceId::mint,
        )?;
        let shell_id = crate::resource::compose_index_id(
            &cadmpeg_ir::identity_namespace!("catia", "b5", "shell"),
            ownership.face_components[face_index],
            ShellId::mint,
        )?;
        let face_loops = b5_face_loops(admission.context(), graph, face)?;
        annotate(
            annotations,
            &face_id,
            "object_stream_b5_03",
            "5f_face",
            Exactness::Inferred,
        )?;
        for field in ["shell", "surface", "loops"] {
            annotations.derived(face_id.as_str(), field)?;
        }
        let face_record_id = face_id.clone();
        let face_shell_id = shell_id.clone();
        let face_surface_id = surface_ids[&face.surface].clone();
        let face_record_index = ir.model.faces.len();
        admission.charge()?;
        ir.model.faces.push(Face {
            id: face_record_id,
            shell: face_shell_id,
            surface: face_surface_id,
            sense: Sense::Forward,
            loops: face_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        for loop_id_value in &face.loops {
            let loop_ = &graph.loops[loop_id_value];
            let orientation = &loop_orientation[loop_id_value];
            let loop_id = crate::resource::compose_u32_id(
                &cadmpeg_ir::identity_namespace!("catia", "b5", "loop"),
                *loop_id_value,
                LoopId::mint,
            )?;
            let mut coedge_ids_by_member = Vec::new();
            for index in 0..loop_.members.len() {
                let text = format!("catia:b5:coedge#{loop_id_value}-{index}");
                let id = CoedgeId::mint(text).map_err(CodecError::malformed)?;
                coedge_ids_by_member.push(id);
            }
            let mut coedge_ids = Vec::new();
            for member in orientation.member_order() {
                let id = coedge_ids_by_member[member].clone();
                coedge_ids.push(id);
            }
            let mut vertex_uses = Vec::new();
            for member in orientation.member_order() {
                let edge = loop_.members[member].edge;
                let endpoints = graph.vertices.edges()[&edge];
                let endpoint = endpoints[1 - usize::from(orientation.members[member].reversed)]
                    .combined_index(graph.vertices.raw_points().len());
                let vertex = VertexId::mint(format!("catia:b5:vertex#{endpoint}"))
                    .map_err(CodecError::malformed)?;
                let after = coedge_ids_by_member[member].clone();
                vertex_uses.push(AnchoredVertexUse {
                    vertex,
                    after,
                    pcurves: Vec::new(),
                });
            }
            annotate(
                annotations,
                &loop_id,
                "object_stream_b5_03",
                "62_loop",
                Exactness::ByteExact,
            )?;
            for field in ["face", "coedges", "vertex_uses"] {
                annotations.derived(loop_id.as_str(), field)?;
            }
            if ir.model.faces[face_record_index].loops.role(&loop_id)
                != LoopBoundaryRole::Unspecified
            {
                annotations
                    .derived(loop_id.as_str(), "boundary_role")
                    .map_err(cadmpeg_core::CodecError::from)?;
            }
            let Ok(ring) = cadmpeg_ir::topology::LoopRing::new(coedge_ids, vertex_uses) else {
                return Ok(false);
            };
            let loop_record_id = loop_id.clone();
            let loop_face_id = face_id.clone();
            admission.charge()?;
            ir.model.loops.push(Loop {
                id: loop_record_id,
                face: loop_face_id,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
            });
            for member in orientation.member_order() {
                let edge = loop_.members[member].edge;
                let reversed = orientation.members[member].reversed;
                let id = coedge_ids_by_member[member].clone();
                annotate(
                    annotations,
                    &id,
                    "object_stream_b5_03",
                    "serialized_loop_member",
                    Exactness::ByteExact,
                )?;
                for field in ["owner_loop", "edge", "radial_next", "sense", "pcurves"] {
                    annotations.derived(id.as_str(), field)?;
                }
                let arena_index = ir.model.coedges.len();

                coedges_by_edge.entry(edge).or_default().push(arena_index);
                let coedge_record_id = id.clone();
                let owner_loop_id = loop_id.clone();
                let edge_ref_id = edge_id_map[&edge].clone();
                let mut coedge_pcurves = Vec::new();
                if let Some((pcurve, parameter_range)) = pcurve_uses.get(&(loop_.object_id, member))
                {
                    let directed = if orientation.members[member].pcurve_reversed {
                        match cadmpeg_ir::geometry::DirectedParameterRange::new([
                            parameter_range[1].get(),
                            parameter_range[0].get(),
                        ]) {
                            Ok(range) => Some(range),
                            Err(_) => return Ok(false),
                        }
                    } else {
                        None
                    };
                    let pcurve_id = pcurve.clone();
                    coedge_pcurves.push(cadmpeg_ir::topology::PcurveUse {
                        pcurve: pcurve_id,
                        isoparametric: None,
                        parameter_range: directed,
                    });
                }
                admission.charge()?;
                ir.model.coedges.push(Coedge {
                    id: coedge_record_id,
                    owner_loop: owner_loop_id,
                    edge: edge_ref_id,
                    radial_next: id,
                    sense: if reversed {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    pcurves: coedge_pcurves,
                    use_curve: None,
                });
            }
        }
    }
    for occurrences in coedges_by_edge.values() {
        for (position, &arena_index) in occurrences.iter().enumerate() {
            let radial = occurrences[(position + 1) % occurrences.len()];
            let next = ir.model.coedges[radial].id.clone();
            ir.model.coedges[arena_index].radial_next = next;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::ids::LoopId;
    use cadmpeg_ir::topology::FaceLoops;

    use super::super::super::graph::controls::B5FramingControl;
    use super::super::super::graph::B5Graph;
    use super::b5_face_loops;

    fn native_face_with_inner_loop() -> B5Graph {
        let bytes =
            crate::test_support::test_b5::b5_closed_triangle_stream_with_native_vertex_chain();
        let mut graph = crate::test_support::with_service_context(|ctx| {
            super::super::super::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
        })
        .expect("service decode")
        .expect("native triangle graph");
        let outer_id = graph.faces[0].loops[0];
        let outer = graph.loops.get_mut(&outer_id).expect("owned outer loop");
        outer.metadata.framing_controls =
            [B5FramingControl::Control03, B5FramingControl::Control05];
        let mut inner = outer.clone();
        inner.object_id += 1;
        inner.metadata.framing_controls =
            [B5FramingControl::Control05, B5FramingControl::Control03];
        graph.faces[0].loops = vec![inner.object_id, outer_id];
        graph.loops.insert(inner.object_id, inner);
        graph.surfaces.clear();
        graph
    }

    #[test]
    fn native_location_controls_classify_loops_without_carrier_geometry() {
        let graph = native_face_with_inner_loop();
        let face = &graph.faces[0];
        let result =
            crate::test_support::with_service_context(|ctx| b5_face_loops(ctx, &graph, face))
                .expect("service classification");
        assert_eq!(
            result,
            FaceLoops::classified(
                LoopId::mint(format!("catia:b5:loop#{}", face.loops[1])).expect("fixture identity"),
                vec![LoopId::mint(format!("catia:b5:loop#{}", face.loops[0]))
                    .expect("fixture identity")],
            )
        );
    }

    #[test]
    fn ambiguous_native_outer_location_withholds_classification() {
        for location in [B5FramingControl::Control03, B5FramingControl::Control05] {
            let mut graph = native_face_with_inner_loop();
            for loop_ in graph.loops.values_mut() {
                loop_.metadata.framing_controls[1] = location;
            }
            let face = &graph.faces[0];
            let result =
                crate::test_support::with_service_context(|ctx| b5_face_loops(ctx, &graph, face))
                    .expect("service classification");
            assert!(matches!(result, FaceLoops::Unspecified { .. }));
        }
    }
}
