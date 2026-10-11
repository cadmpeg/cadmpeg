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
    let mut face_ids = HashSet::new();
    ctx.reserve_set(
        &mut face_ids,
        graph.faces.len(),
        "catia b5 face ownership ids",
    )?;
    let mut loop_owners = HashMap::<u32, usize>::new();
    for (face_index, face) in graph.faces.iter().enumerate() {
        if !face_ids.insert(face.object_id) || face.loops.is_empty() {
            return Ok(None);
        }
        for loop_id in &face.loops {
            if ctx
                .insert_hash_map(
                    &mut loop_owners,
                    *loop_id,
                    face_index,
                    "catia b5 loop owners",
                )?
                .is_some()
            {
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

    let mut parents =
        UnionFind::charged(ctx, graph.faces.len(), "catia b5 ownership union parents")?;
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
                ctx.insert_hash_map(&mut edge_uses, edge, 1, "catia b5 ownership edge uses")?;
            }
            if let Some(other_face) = ctx.insert_hash_map(
                &mut first_face_by_edge,
                edge,
                face,
                "catia b5 ownership first faces",
            )? {
                parents.union(ctx, face, other_face)?;
            }
        }
    }

    let mut labels = HashMap::<usize, usize>::new();
    ctx.reserve_map(
        &mut labels,
        graph.faces.len(),
        "catia b5 ownership component labels",
    )?;
    let mut face_components = Vec::new();
    ctx.reserve_vec(
        &mut face_components,
        graph.faces.len(),
        "catia b5 face components",
    )?;
    for face in 0..graph.faces.len() {
        let root = parents.find(ctx, face)?;
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
    ctx.reserve_vec(
        &mut loop_ids,
        graph.loops.len(),
        "catia b5 orientation loop ids",
    )?;
    loop_ids.extend(graph.loops.keys().copied());
    let mut node_by_loop = HashMap::new();
    ctx.reserve_map(
        &mut node_by_loop,
        loop_ids.len(),
        "catia b5 orientation loop index",
    )?;
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
            if !uses.contains_key(&member.edge) {
                ctx.insert_hash_map(
                    &mut uses,
                    member.edge,
                    Vec::new(),
                    "catia b5 orientation edge keys",
                )?;
            }
            let Some(occurrences) = uses.get_mut(&member.edge) else {
                return Ok(None);
            };
            ctx.push_vec(occurrences, (node, sense), "catia b5 orientation edge uses")?;
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
            ctx.push_vec(
                &mut constraints[*left],
                (*right, parity),
                "catia b5 orientation adjacent loops",
            )?;
            ctx.push_vec(
                &mut constraints[*right],
                (*left, parity),
                "catia b5 orientation adjacent loops",
            )?;
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
        ctx.push_vec(
            &mut pending,
            (root, false),
            "catia b5 orientation pending loops",
        )?;
        while let Some((node, flip)) = pending.pop() {
            for &(neighbor, parity) in &constraints[node] {
                let required = flip ^ parity;
                match flips[neighbor] {
                    Some(existing) if existing != required => return Ok(None),
                    Some(_) => {}
                    None => {
                        flips[neighbor] = Some(required);
                        ctx.push_vec(
                            &mut pending,
                            (neighbor, required),
                            "catia b5 orientation pending loops",
                        )?;
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
        let pcurve_senses = graph.loops[&loop_id].pcurve_senses(ctx)?;
        let members = ctx.collect_vec(
            senses
                .into_iter()
                .zip(pcurve_senses)
                .map(|(reversed, pcurve_reversed)| OrientedLoopMember {
                    reversed: reversed ^ flipped,
                    pcurve_reversed: pcurve_reversed ^ flipped,
                }),
            "catia b5 oriented loop members",
        )?;
        ctx.insert_btree_map(
            &mut oriented,
            loop_id,
            OrientedLoop { flipped, members },
            "catia b5 oriented loops",
        )?;
    }
    Ok(Some(oriented))
}

/// Use each loop's native location control. Face-to-carrier orientation is
/// independent of the outer/inner classification.
fn b5_face_loops(
    ctx: &DecodeContext<'_>,
    graph: &B5Graph,
    face: &super::super::graph::B5Face,
) -> Result<FaceLoops, CodecError> {
    let mut ids = Vec::new();
    let mut outer = None;
    let mut inner = Vec::new();
    let mut unique_outer = true;
    for &loop_id in &face.loops {
        let id = crate::resource::compose_index_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "b5", "loop"),
            usize::try_from(loop_id)
                .map_err(|_| ctx.refuse_codec_limit("catia_b5_face_loop_id", u64::MAX, u64::MAX))?,
            LoopId::mint,
            "catia_b5_face_loop_id",
        )?;
        let role_id = id.try_clone_for_decode(ctx, "catia_b5_boundary_role_id")?;
        ctx.push_vec(&mut ids, id, "catia_b5_face_loop_ids")?;
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
                ctx.push_vec(&mut inner, role_id, "catia_b5_inner_loop_ids")?;
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
    admission: &mut crate::families::FamilyEntityAdmission<'_, '_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let surface_ids = emitted.surface_ids;
    let pcurve_uses = emitted.pcurve_uses;
    let edge_id_map = emitted.edge_ids;
    let ownership = &plan.ownership;
    let components = ownership.components(admission.ctx)?;
    let loop_orientation = &plan.loop_orientation;

    let body_id = crate::resource::compose_index_id(
        admission.context(),
        &cadmpeg_ir::identity_namespace!("catia", "b5", "body"),
        0,
        BodyId::mint,
        "catia_b5_body_id",
    )?;
    let mut region_ids = BTreeMap::new();
    for &component in components.keys() {
        let id = crate::resource::compose_index_id(
            admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "region"),
            component,
            RegionId::mint,
            "catia_b5_region_id",
        )?;
        admission.context().insert_btree_map(
            &mut region_ids,
            component,
            id,
            "catia_b5_region_ids",
        )?;
    }
    annotate(
        admission.context(),
        annotations,
        &body_id,
        "object_stream_b5_03",
        "single_body",
        Exactness::Inferred,
    )?;
    for field in ["kind", "regions"] {
        crate::resource::derived_annotation(
            admission.context(),
            annotations,
            body_id.as_str(),
            field,
            "catia_b5_body_annotation",
        )?;
    }
    let mut body_regions = Vec::new();
    for id in region_ids.values() {
        let id = id.try_clone_for_decode(admission.context(), "catia_b5_body_region_id")?;
        admission
            .context()
            .push_vec(&mut body_regions, id, "catia_b5_body_regions")?;
    }
    let body_record_id =
        body_id.try_clone_for_decode(admission.context(), "catia_b5_body_record_id")?;
    admission.reserve_entity(&mut ir.model.bodies, "catia_b5_emit_bodies")?;
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
        let region_id = region_ids[component_index]
            .try_clone_for_decode(admission.context(), "catia_b5_region_ref_id")?;
        let shell_id = crate::resource::compose_index_id(
            admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "shell"),
            *component_index,
            ShellId::mint,
            "catia_b5_shell_id",
        )?;
        annotate(
            admission.context(),
            annotations,
            &region_id,
            "object_stream_b5_03",
            "derived_region",
            Exactness::Inferred,
        )?;
        for field in ["body", "shells"] {
            crate::resource::derived_annotation(
                admission.context(),
                annotations,
                region_id.as_str(),
                field,
                "catia_b5_region_annotation",
            )?;
        }
        let region_record_id =
            region_id.try_clone_for_decode(admission.context(), "catia_b5_region_record_id")?;
        let region_body_id =
            body_id.try_clone_for_decode(admission.context(), "catia_b5_region_body_id")?;
        let region_shell_id =
            shell_id.try_clone_for_decode(admission.context(), "catia_b5_region_shell_id")?;
        let mut region_shells = Vec::new();
        admission.context().push_vec(
            &mut region_shells,
            region_shell_id,
            "catia_b5_region_shells",
        )?;
        admission.reserve_entity(&mut ir.model.regions, "catia_b5_emit_regions")?;
        ir.model.regions.push(Region {
            id: region_record_id,
            body: region_body_id,
            shells: region_shells,
        });
        annotate(
            admission.context(),
            annotations,
            &shell_id,
            "object_stream_b5_03",
            "derived_shell",
            Exactness::Inferred,
        )?;
        for field in ["region", "faces"] {
            crate::resource::derived_annotation(
                admission.context(),
                annotations,
                shell_id.as_str(),
                field,
                "catia_b5_shell_annotation",
            )?;
        }
        let mut shell_faces = Vec::new();
        for face in component_faces {
            let face_id = crate::resource::compose_index_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "b5", "face"),
                usize::try_from(graph.faces[*face].object_id).map_err(|_| {
                    admission.context().refuse_codec_limit(
                        "catia_b5_shell_face_id",
                        u64::MAX,
                        u64::MAX,
                    )
                })?,
                FaceId::mint,
                "catia_b5_shell_face_id",
            )?;
            admission
                .context()
                .push_vec(&mut shell_faces, face_id, "catia_b5_shell_faces")?;
        }
        admission.reserve_entity(&mut ir.model.shells, "catia_b5_emit_shells")?;
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
        let face_id = crate::resource::compose_index_id(
            admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "face"),
            usize::try_from(face.object_id).map_err(|_| {
                admission
                    .context()
                    .refuse_codec_limit("catia_b5_face_id", u64::MAX, u64::MAX)
            })?,
            FaceId::mint,
            "catia_b5_face_id",
        )?;
        let shell_id = crate::resource::compose_index_id(
            admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "shell"),
            ownership.face_components[face_index],
            ShellId::mint,
            "catia_b5_face_shell_id",
        )?;
        let face_loops = b5_face_loops(admission.context(), graph, face)?;
        annotate(
            admission.context(),
            annotations,
            &face_id,
            "object_stream_b5_03",
            "5f_face",
            Exactness::Inferred,
        )?;
        for field in ["shell", "surface", "loops"] {
            crate::resource::derived_annotation(
                admission.context(),
                annotations,
                face_id.as_str(),
                field,
                "catia_b5_face_annotation",
            )?;
        }
        let face_record_id =
            face_id.try_clone_for_decode(admission.context(), "catia_b5_face_record_id")?;
        let face_shell_id =
            shell_id.try_clone_for_decode(admission.context(), "catia_b5_face_shell_ref")?;
        let face_surface_id = surface_ids[&face.surface]
            .try_clone_for_decode(admission.context(), "catia_b5_face_surface_ref")?;
        let face_record_index = ir.model.faces.len();
        admission.reserve_entity(&mut ir.model.faces, "catia_b5_emit_faces")?;
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
            let loop_id = crate::resource::compose_index_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "b5", "loop"),
                usize::try_from(*loop_id_value).map_err(|_| {
                    admission.context().refuse_codec_limit(
                        "catia_b5_emitted_loop_id",
                        u64::MAX,
                        u64::MAX,
                    )
                })?,
                LoopId::mint,
                "catia_b5_emitted_loop_id",
            )?;
            let mut coedge_ids_by_member = Vec::new();
            for index in 0..loop_.members.len() {
                let text = admission.context().format_retained(
                    format_args!("catia:b5:coedge#{loop_id_value}-{index}"),
                    "catia_b5_coedge_id",
                )?;
                let id = CoedgeId::mint(text).map_err(CodecError::malformed)?;
                admission.context().push_vec(
                    &mut coedge_ids_by_member,
                    id,
                    "catia_b5_coedge_ids_by_member",
                )?;
            }
            let mut coedge_ids = Vec::new();
            for member in orientation.member_order() {
                let id = coedge_ids_by_member[member]
                    .try_clone_for_decode(admission.context(), "catia_b5_oriented_coedge_id")?;
                admission.context().push_vec(
                    &mut coedge_ids,
                    id,
                    "catia_b5_oriented_coedge_ids",
                )?;
            }
            let mut vertex_uses = Vec::new();
            for member in orientation.member_order() {
                let edge = loop_.members[member].edge;
                let endpoints = graph.vertices.edges()[&edge];
                let endpoint = endpoints[1 - usize::from(orientation.members[member].reversed)]
                    .combined_index(graph.vertices.raw_points().len());
                let vertex = VertexId::mint(admission.context().format_retained(
                    format_args!("catia:b5:vertex#{endpoint}"),
                    "catia_b5_loop_vertex_use_id",
                )?)
                .map_err(CodecError::malformed)?;
                let after = coedge_ids_by_member[member]
                    .try_clone_for_decode(admission.context(), "catia_b5_loop_vertex_after_id")?;
                admission.context().push_vec(
                    &mut vertex_uses,
                    AnchoredVertexUse {
                        vertex,
                        after,
                        pcurves: Vec::new(),
                    },
                    "catia_b5_loop_vertex_uses",
                )?;
            }
            annotate(
                admission.context(),
                annotations,
                &loop_id,
                "object_stream_b5_03",
                "62_loop",
                Exactness::ByteExact,
            )?;
            for field in ["face", "coedges", "vertex_uses"] {
                crate::resource::derived_annotation(
                    admission.context(),
                    annotations,
                    loop_id.as_str(),
                    field,
                    "catia_b5_loop_annotation",
                )?;
            }
            if ir.model.faces[face_record_index].loops.role(&loop_id)
                != LoopBoundaryRole::Unspecified
            {
                crate::resource::derived_annotation(
                    admission.context(),
                    annotations,
                    loop_id.as_str(),
                    "boundary_role",
                    "catia_b5_loop_annotation",
                )?;
            }
            let Ok(ring) =
                cadmpeg_ir::topology::LoopRing::new(admission.context(), coedge_ids, vertex_uses)
                    .map_err(cadmpeg_core::CodecError::from)?
            else {
                return Ok(false);
            };
            let loop_record_id =
                loop_id.try_clone_for_decode(admission.context(), "catia_b5_loop_record_id")?;
            let loop_face_id =
                face_id.try_clone_for_decode(admission.context(), "catia_b5_loop_face_id")?;
            admission.reserve_entity(&mut ir.model.loops, "catia_b5_emit_loops")?;
            ir.model.loops.push(Loop {
                id: loop_record_id,
                face: loop_face_id,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
            });
            for member in orientation.member_order() {
                let edge = loop_.members[member].edge;
                let reversed = orientation.members[member].reversed;
                let id = coedge_ids_by_member[member]
                    .try_clone_for_decode(admission.context(), "catia_b5_coedge_emit_id")?;
                annotate(
                    admission.context(),
                    annotations,
                    &id,
                    "object_stream_b5_03",
                    "serialized_loop_member",
                    Exactness::ByteExact,
                )?;
                for field in ["owner_loop", "edge", "radial_next", "sense", "pcurves"] {
                    crate::resource::derived_annotation(
                        admission.context(),
                        annotations,
                        id.as_str(),
                        field,
                        "catia_b5_coedge_annotation",
                    )?;
                }
                let arena_index = ir.model.coedges.len();
                admission.context().admit_hash_map_entry(
                    &mut coedges_by_edge,
                    &edge,
                    "catia_b5_coedges_by_edge",
                )?;
                admission.context().push_vec(
                    coedges_by_edge.entry(edge).or_default(),
                    arena_index,
                    "catia_b5_coedge_radial_occurrences",
                )?;
                let coedge_record_id =
                    id.try_clone_for_decode(admission.context(), "catia_b5_coedge_record_id")?;
                let owner_loop_id = loop_id
                    .try_clone_for_decode(admission.context(), "catia_b5_coedge_owner_loop_id")?;
                let edge_ref_id = edge_id_map[&edge]
                    .try_clone_for_decode(admission.context(), "catia_b5_coedge_edge_id")?;
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
                    let pcurve_id = pcurve
                        .try_clone_for_decode(admission.context(), "catia_b5_coedge_pcurve_id")?;
                    admission.context().push_vec(
                        &mut coedge_pcurves,
                        cadmpeg_ir::topology::PcurveUse {
                            pcurve: pcurve_id,
                            isoparametric: None,
                            parameter_range: directed,
                        },
                        "catia_b5_coedge_pcurves",
                    )?;
                }
                admission.reserve_entity(&mut ir.model.coedges, "catia_b5_emit_coedges")?;
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
            let next = ir.model.coedges[radial]
                .id
                .try_clone_for_decode(admission.context(), "catia_b5_radial_next_id")?;
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
