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

use super::super::graph::{controls::B5FramingControl, B5Face, B5Graph, B5Surface};
use super::super::vecmath::{components, cross};
use super::pcurves::PcurveUses;
use super::{annotate, dot, OrientedLoop, OrientedLoopMember, OwnershipPlan, TransferPlan};
use crate::solve::union_find::UnionFind;

pub(super) fn native_carrier_face_sense(graph: &B5Graph, face: &B5Face) -> Option<Sense> {
    let mut reverse_chart = false;
    match graph.surfaces.get(&face.surface)? {
        B5Surface::Plane {
            frame, direction_v, ..
        } => {
            (dot(
                cross(components(frame.reference()), direction_v.get()),
                components(frame.axis()),
            ) > 0.0)
                .then_some(())?;
        }
        B5Surface::Cylinder { angular_scale, .. } => {
            (angular_scale.get() > 0.0).then_some(())?;
        }
        B5Surface::Cone {
            frame,
            direction_y,
            surface: Some(_),
            ..
        } => {
            let orientation = dot(
                cross(components(frame.reference()), components(direction_y)),
                components(frame.axis()),
            );
            if orientation == 0.0 {
                return None;
            }
            reverse_chart = orientation < 0.0;
        }
        B5Surface::Torus { .. } => {}
        B5Surface::Nurbs(surface) => {
            (!surface.normal_reversed()).then_some(())?;
        }
        B5Surface::Revolution {
            profile_curve,
            profile_range,
            ..
        } => {
            graph
                .profiles
                .get(profile_curve)?
                .parameter_range()
                .endpoints()
                .into_iter()
                .zip(profile_range.endpoints())
                .all(|(profile, surface)| profile.to_bits() == surface.to_bits())
                .then_some(())?;
        }
        _ => return None,
    }
    let mut outer = None;
    for loop_id in &face.loops {
        let loop_ = graph.loops.get(loop_id)?;
        if loop_.surface != face.surface {
            return None;
        }
        if loop_.metadata.framing_controls[1] == B5FramingControl::Control05
            && outer.replace(loop_).is_some()
        {
            return None;
        }
    }
    Some(match (outer?.metadata.framing_controls[0], reverse_chart) {
        (B5FramingControl::Control03, false) | (B5FramingControl::Control05, true) => {
            Sense::Reversed
        }
        (B5FramingControl::Control05, false) | (B5FramingControl::Control03, true) => {
            Sense::Forward
        }
    })
}

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
            sense: plan.face_senses[face_index],
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
    fn native_plane_face_with_inner_loop() -> B5Graph {
        let mut graph = native_face_with_inner_loop();
        let bytes =
            crate::test_support::test_b5::b5_closed_triangle_stream_with_native_vertex_chain();
        let mut carrier = crate::test_support::with_service_context(|ctx| {
            super::super::super::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
        })
        .expect("service decode")
        .expect("native plane graph");
        let id = graph.faces[0].surface;
        graph.surfaces.insert(
            id,
            carrier.surfaces.remove(&id).expect("native plane carrier"),
        );
        graph
    }

    #[test]
    fn native_plane_sense_uses_unique_outer_control_in_any_record_order() {
        use cadmpeg_ir::topology::Sense;
        let mut graph = native_plane_face_with_inner_loop();
        assert_eq!(
            super::native_carrier_face_sense(&graph, &graph.faces[0]),
            Some(Sense::Reversed)
        );
        graph.faces[0].loops.reverse();
        assert_eq!(
            super::native_carrier_face_sense(&graph, &graph.faces[0]),
            Some(Sense::Reversed)
        );
        let outer = graph.faces[0].loops[0];
        graph
            .loops
            .get_mut(&outer)
            .expect("native outer")
            .metadata
            .framing_controls[0] = B5FramingControl::Control05;
        assert_eq!(
            super::native_carrier_face_sense(&graph, &graph.faces[0]),
            Some(Sense::Forward)
        );
    }

    #[test]
    fn native_plane_sense_withholds_missing_ambiguous_or_mismatched_outer() {
        for location in [B5FramingControl::Control03, B5FramingControl::Control05] {
            let mut graph = native_plane_face_with_inner_loop();
            for loop_ in graph.loops.values_mut() {
                loop_.metadata.framing_controls[1] = location;
            }
            assert!(super::native_carrier_face_sense(&graph, &graph.faces[0]).is_none());
        }
        let mut graph = native_plane_face_with_inner_loop();
        graph.loops.remove(&graph.faces[0].loops[0]);
        assert!(super::native_carrier_face_sense(&graph, &graph.faces[0]).is_none());
        let mut graph = native_plane_face_with_inner_loop();
        let id = graph.faces[0].loops[0];
        graph.loops.get_mut(&id).expect("owned loop").surface = 101;
        assert!(super::native_carrier_face_sense(&graph, &graph.faces[0]).is_none());
    }

    #[test]
    fn native_plane_sense_withholds_inconsistent_carrier_chart() {
        let mut graph = native_plane_face_with_inner_loop();
        let surface = graph.faces[0].surface;
        let super::B5Surface::Plane { direction_v, .. } =
            graph.surfaces.get_mut(&surface).expect("plane")
        else {
            panic!("native plane carrier");
        };
        *direction_v =
            crate::checked::ExactUnitVector3::new([0.0, -1.0, 0.0]).expect("unit opposite V");
        assert!(super::native_carrier_face_sense(&graph, &graph.faces[0]).is_none());
    }

    #[test]
    fn native_plane_domain_with_inconsistent_loop_repairs_is_withheld() {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_plane_negative.catpart"
        ));
        let mut graph = crate::test_support::with_service_context(|ctx| {
            super::super::super::graph::parse(ctx, bytes, &mut crate::nurbs::LaneRefusals::new())
        })
        .expect("service decode")
        .expect("negative native plane graph");
        let mut second = graph.faces[0].clone();
        second.object_id += 1;
        let mut loop_ = graph.loops[&second.loops[0]].clone();
        loop_.object_id += 1;
        second.loops = vec![loop_.object_id];
        let mut record = graph.face_records[&graph.faces[0].object_id].clone();
        record.object_id = second.object_id;
        record.references[1] = loop_.object_id;
        graph.face_records.insert(record.object_id, record);
        graph.loops.insert(loop_.object_id, loop_);
        graph.faces.push(second);
        let payload = cadmpeg_ir::ids::UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "payload", "unknown"),
            cadmpeg_ir::identity_key!("plane-orientation-test"),
        );
        let plan = crate::test_support::with_service_context(|ctx| {
            super::super::build_plan(
                ctx,
                &graph,
                &payload,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("bounded native transfer planning");
        assert!(plan.is_none());
    }
    #[test]
    fn native_carrier_sense_rejects_nonpositive_cylinder_chart_scale() {
        use crate::test_support::test_b5::{finite, positive_length};
        use cadmpeg_ir::topology::Sense;
        let mut graph = native_plane_face_with_inner_loop();
        let id = graph.faces[0].surface;
        let super::B5Surface::Plane {
            origin,
            frame,
            u_range,
            v_range,
            ..
        } = graph.surfaces.remove(&id).expect("plane source carrier")
        else {
            panic!("plane source carrier");
        };
        for (scale, expected) in [(1.0, Some(Sense::Reversed)), (0.0, None), (-1.0, None)] {
            graph.surfaces.insert(
                id,
                super::B5Surface::Cylinder {
                    origin,
                    frame,
                    radius: positive_length(1.0),
                    u_range,
                    v_range,
                    angular_scale: finite(scale),
                    chart_origin: finite(0.0),
                },
            );
            assert_eq!(
                super::native_carrier_face_sense(&graph, &graph.faces[0]),
                expected
            );
        }
    }

    #[test]
    fn native_carrier_sense_keeps_unproved_carrier_abstentions() {
        use crate::test_support::test_b5::positive;
        use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
        use cadmpeg_ir::math::Point3;
        let mut graph = native_plane_face_with_inner_loop();
        let id = graph.faces[0].surface;
        let super::B5Surface::Plane {
            origin,
            frame,
            u_range,
            v_range,
            ..
        } = graph.surfaces.remove(&id).expect("plane source carrier")
        else {
            panic!("plane source carrier");
        };
        graph.surfaces.insert(
            id,
            super::B5Surface::Unknown {
                family: 181,
                class: 52,
                payload: Vec::new(),
            },
        );
        assert!(super::native_carrier_face_sense(&graph, &graph.faces[0]).is_none());
        graph.surfaces.insert(
            id,
            super::B5Surface::Revolution {
                profile_curve: 101,
                axis_origin: origin,
                axis_direction: *frame.axis(),
                profile_range: u_range,
                angular_range: v_range,
                angular_scale: positive(1.0),
            },
        );
        assert!(super::native_carrier_face_sense(&graph, &graph.faces[0]).is_none());
        let surface = NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                ],
                None,
            ),
            true,
        )
        .expect("oriented reversed carrier");
        graph.surfaces.insert(id, super::B5Surface::Nurbs(surface));
        assert!(super::native_carrier_face_sense(&graph, &graph.faces[0]).is_none());
    }

    #[test]
    fn native_revolution_face_sense_requires_matching_profile_interval() {
        use crate::families::b5::graph::B5Profile;
        use crate::test_support::test_b5::{exact_unit, increasing, point, positive, unit};
        use cadmpeg_ir::topology::Sense;
        let mut graph = native_plane_face_with_inner_loop();
        let id = graph.faces[0].surface;
        graph.surfaces.insert(
            id,
            super::B5Surface::Revolution {
                profile_curve: 101,
                axis_origin: point([0.0; 3]),
                axis_direction: unit([0.0, 0.0, 1.0]),
                profile_range: increasing([0.0, 1.0]),
                angular_range: increasing([0.0, 1.0]),
                angular_scale: positive(1.0),
            },
        );
        assert!(super::native_carrier_face_sense(&graph, &graph.faces[0]).is_none());
        for (interval, expected) in [([0.0, 2.0], None), ([0.0, 1.0], Some(Sense::Reversed))] {
            graph.profiles.insert(
                101,
                B5Profile::Line {
                    point: point([1.0, 0.0, 0.0]),
                    direction: exact_unit([0.0, 0.0, 1.0]),
                    parameter_range: increasing(interval),
                },
            );
            assert_eq!(
                super::native_carrier_face_sense(&graph, &graph.faces[0]),
                expected
            );
        }
    }

    #[test]
    fn native_revolution_face_sense_keeps_profile_and_angle_axis_order() {
        use crate::families::b5::graph::B5Profile;
        use crate::test_support::test_b5::{
            exact_unit, increasing, point, positive, positive_length, unit,
        };
        use cadmpeg_ir::topology::Sense;
        let mut graph = native_plane_face_with_inner_loop();
        let id = graph.faces[0].surface;
        graph.surfaces.insert(
            id,
            super::B5Surface::Revolution {
                profile_curve: 101,
                axis_origin: point([0.0; 3]),
                axis_direction: unit([0.0, 0.0, 1.0]),
                profile_range: increasing([0.0, 1.0]),
                angular_range: increasing([0.0, 1.0]),
                angular_scale: positive(1.0),
            },
        );
        graph.profiles.insert(
            101,
            B5Profile::Arc {
                center: point([3.0, 0.0, 0.0]),
                direction_x: exact_unit([1.0, 0.0, 0.0]),
                direction_y: exact_unit([0.0, 0.0, 1.0]),
                radius: positive_length(1.0),
                parameter_range: increasing([0.0, 1.0]),
            },
        );
        assert_eq!(
            super::native_carrier_face_sense(&graph, &graph.faces[0]),
            Some(Sense::Reversed)
        );
        graph.faces[0].loops.reverse();
        assert_eq!(
            super::native_carrier_face_sense(&graph, &graph.faces[0]),
            Some(Sense::Reversed)
        );
    }

    fn native_torus_face_with_inner_loop() -> B5Graph {
        use crate::test_support::test_b5::{exact_unit, positive, positive_length};
        let mut graph = native_plane_face_with_inner_loop();
        let id = graph.faces[0].surface;
        let super::B5Surface::Plane {
            origin,
            frame,
            u_range,
            v_range,
            ..
        } = graph.surfaces.remove(&id).expect("plane carrier")
        else {
            panic!("plane carrier");
        };
        graph.surfaces.insert(
            id,
            super::B5Surface::Torus {
                center: origin,
                frame,
                direction_y: exact_unit([0.0, 1.0, 0.0]),
                major_radius: positive_length(4.0),
                minor_radius: positive_length(1.0),
                major_angular_range: u_range,
                major_angular_domain: u_range,
                minor_angular_range: v_range,
                minor_angular_domain: v_range,
                major_scale: positive(2.0),
                minor_scale: positive(3.0),
            },
        );
        graph
    }

    #[test]
    fn native_torus_sense_uses_unique_outer_without_record_order_inference() {
        use cadmpeg_ir::topology::Sense;
        let mut graph = native_torus_face_with_inner_loop();
        assert_eq!(
            super::native_carrier_face_sense(&graph, &graph.faces[0]),
            Some(Sense::Reversed)
        );
        graph.faces[0].loops.reverse();
        assert_eq!(
            super::native_carrier_face_sense(&graph, &graph.faces[0]),
            Some(Sense::Reversed)
        );
        let outer = graph.faces[0].loops[0];
        graph
            .loops
            .get_mut(&outer)
            .expect("outer")
            .metadata
            .framing_controls[0] = B5FramingControl::Control05;
        assert_eq!(
            super::native_carrier_face_sense(&graph, &graph.faces[0]),
            Some(Sense::Forward)
        );
    }

    #[test]
    fn native_torus_sense_withholds_ambiguous_outer_and_mismatched_carrier() {
        for location in [B5FramingControl::Control03, B5FramingControl::Control05] {
            let mut graph = native_torus_face_with_inner_loop();
            for loop_ in graph.loops.values_mut() {
                loop_.metadata.framing_controls[1] = location;
            }
            assert!(super::native_carrier_face_sense(&graph, &graph.faces[0]).is_none());
        }
        let mut graph = native_torus_face_with_inner_loop();
        let outer = graph.faces[0].loops[0];
        graph.loops.get_mut(&outer).expect("outer").surface = graph.faces[0].surface + 1;
        assert!(super::native_carrier_face_sense(&graph, &graph.faces[0]).is_none());
    }

    fn native_cone_face_with_inner_loop(handedness: f64) -> B5Graph {
        use crate::test_support::test_b5::{
            finite, nonnegative_length, positive, positive_angle, unit,
        };
        use cadmpeg_ir::geometry::analytic::ConeSurface;
        use cadmpeg_ir::scalar::Angle;
        let mut graph = native_plane_face_with_inner_loop();
        let id = graph.faces[0].surface;
        let super::B5Surface::Plane {
            origin,
            frame,
            u_range,
            v_range,
            ..
        } = graph.surfaces.remove(&id).expect("plane carrier")
        else {
            panic!("plane carrier");
        };
        let surface = ConeSurface::new(
            origin,
            frame,
            nonnegative_length(4.0),
            positive(1.0),
            Angle::new(std::f64::consts::FRAC_PI_3).expect("cone half angle"),
        );
        graph.surfaces.insert(
            id,
            super::B5Surface::Cone {
                origin,
                frame,
                direction_y: unit([0.0, handedness, 0.0]),
                radial_angle: positive_angle(std::f64::consts::FRAC_PI_6),
                reference_radius: finite(4.0),
                angular_range: u_range,
                slant_range: v_range,
                slant_shift: finite(0.0),
                angular_scale: positive(2.0),
                angular_domain: u_range,
                surface: Some(surface),
            },
        );
        graph
    }

    #[test]
    fn native_cone_sense_handles_signed_chart_outer_order() {
        use cadmpeg_ir::topology::Sense;
        for (h, expected) in [(1.0, Sense::Reversed), (-1.0, Sense::Forward)] {
            let mut graph = native_cone_face_with_inner_loop(h);
            assert_eq!(
                super::native_carrier_face_sense(&graph, &graph.faces[0]),
                Some(expected)
            );
            graph.faces[0].loops.reverse();
            assert_eq!(
                super::native_carrier_face_sense(&graph, &graph.faces[0]),
                Some(expected)
            );
        }
    }

    #[test]
    fn native_cone_sense_withholds_unknown_frame_or_missing_neutral_carrier() {
        let mut graph = native_cone_face_with_inner_loop(1.0);
        let id = graph.faces[0].surface;
        let super::B5Surface::Cone { surface, .. } = graph.surfaces.get_mut(&id).expect("cone")
        else {
            panic!("cone");
        };
        *surface = None;
        assert!(super::native_carrier_face_sense(&graph, &graph.faces[0]).is_none());
        let mut graph = native_cone_face_with_inner_loop(1.0);
        let super::B5Surface::Cone { direction_y, .. } = graph.surfaces.get_mut(&id).expect("cone")
        else {
            panic!("cone");
        };
        *direction_y = crate::test_support::test_b5::unit([1.0, 0.0, 0.0]);
        assert!(super::native_carrier_face_sense(&graph, &graph.faces[0]).is_none());
    }
}
