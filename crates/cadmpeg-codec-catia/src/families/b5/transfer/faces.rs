// SPDX-License-Identifier: Apache-2.0
//! Face-layer transfer: face ownership components, loop orientation solving,
//! and the body/shell/face/loop/coedge emit pass.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{pcurve::PcurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3};
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
                parents.union(face, other_face);
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
        let root = parents.find(face);
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    graph: &B5Graph,
    loop_id: u32,
    loop_orientation: &OrientedLoop,
    surface_id: &SurfaceId,
    pcurve_uses: &PcurveUses,
) -> Result<Option<Vec<Point3>>, cadmpeg_core::CodecError> {
    let Some(surface) = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == *surface_id)
    else {
        return Ok(None);
    };
    let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved() else {
        return Ok(None);
    };
    let origin = plane_surface.origin().get();
    let u_axis = *plane_surface.frame().reference().as_raw();
    let v_axis = *plane_surface.frame().binormal().as_raw();
    let Some(loop_) = graph.loops.get(&loop_id) else {
        return Ok(None);
    };
    let mut points = Vec::new();
    ctx.reserve_vec(
        &mut points,
        loop_.members.len(),
        "catia_b5_planar_loop_points",
    )?;
    for member in loop_orientation.member_order() {
        let edge = loop_.members[member].edge;
        let Some(mut endpoints) = graph.vertices.edge_points(edge) else {
            return Ok(None);
        };
        if loop_orientation.members[member].reversed {
            endpoints.swap(0, 1);
        }
        let [start, end] = endpoints.map(|point| Point3::new(point[0], point[1], point[2]));
        let Some((pcurve_id, parameter_range)) = pcurve_uses.get(&(loop_id, member)) else {
            return Ok(None);
        };
        if parameter_range[0] == parameter_range[1] {
            return Ok(None);
        }
        let Some(pcurve) = ir
            .model
            .pcurves
            .iter()
            .find(|pcurve| pcurve.id == *pcurve_id)
        else {
            return Ok(None);
        };
        let PcurveGeometry::Line(line_pcurve) = &pcurve.geometry else {
            return Ok(None);
        };
        let uv_origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
        let uv_endpoints = parameter_range.map(|parameter| {
            Point2::new(
                uv_origin.u + parameter.get() * direction.u,
                uv_origin.v + parameter.get() * direction.v,
            )
        });
        let lifted = uv_endpoints.map(|uv| b5_plane_point(origin, u_axis, v_axis, uv));
        let forward_error = lifted[0].distance(start).max(lifted[1].distance(end));
        let reverse_error = lifted[1].distance(start).max(lifted[0].distance(end));
        let error = forward_error.min(reverse_error);
        if !error.is_finite() || error > 2e-3 {
            return Ok(None);
        }
        points.push(start);
    }
    Ok(Some(points))
}

/// The face's loop ids and their classification, built once from the loop
/// rows the graph states for this face.
fn b5_face_loops(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    graph: &B5Graph,
    face: &super::super::graph::B5Face,
    loop_orientation: &BTreeMap<u32, OrientedLoop>,
    surface_ids: &HashMap<u32, SurfaceId>,
    pcurve_uses: &PcurveUses,
) -> Result<cadmpeg_ir::topology::FaceLoops, cadmpeg_core::CodecError> {
    let mut ids = Vec::new();
    ctx.reserve_vec(&mut ids, face.loops.len(), "catia_b5_face_loop_ids")?;
    for loop_id in &face.loops {
        let id = crate::resource::compose_index_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "b5", "loop"),
            usize::try_from(*loop_id)
                .map_err(|_| ctx.refuse_codec_limit("catia_b5_face_loop_id", u64::MAX, u64::MAX))?,
            LoopId::mint,
            "catia_b5_face_loop_id",
        )?;
        ids.push(id);
    }
    let unspecified = || -> Result<_, cadmpeg_core::CodecError> {
        let mut copy = Vec::new();
        for id in &ids {
            let id = id.try_clone_for_decode(ctx, "catia_b5_unspecified_loop_id_copy")?;
            ctx.push_vec(&mut copy, id, "catia_b5_unspecified_loop_ids")?;
        }
        Ok(cadmpeg_ir::topology::FaceLoops::unspecified(copy))
    };
    if let [single] = ids.as_slice() {
        return Ok(cadmpeg_ir::topology::FaceLoops::classified(
            single.try_clone_for_decode(ctx, "catia_b5_single_loop_id_copy")?,
            Vec::new(),
        ));
    }
    let Some(surface_id) = surface_ids.get(&face.surface) else {
        return unspecified();
    };
    let mut rows = Vec::new();
    for (loop_id, id) in face.loops.iter().zip(&ids) {
        let Some(orientation) = loop_orientation.get(loop_id) else {
            return unspecified();
        };
        let Some(points) = b5_planar_loop_points(
            ctx,
            ir,
            graph,
            *loop_id,
            orientation,
            surface_id,
            pcurve_uses,
        )?
        else {
            return unspecified();
        };
        let id = id.try_clone_for_decode(ctx, "catia_b5_planar_loop_id_copy")?;
        ctx.push_vec(&mut rows, (id, points), "catia_b5_planar_loop_rows")?;
    }
    let Some(surface) = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == *surface_id)
    else {
        return unspecified();
    };
    crate::boundary_roles::classify_planar_boundaries(ctx, &surface.geometry, &rows)
}

fn copy_face_loops(ctx: &DecodeContext<'_>, loops: &FaceLoops) -> Result<FaceLoops, CodecError> {
    match loops {
        FaceLoops::Unspecified { loops } => {
            let mut copy = Vec::new();
            for id in loops {
                let id = id.try_clone_for_decode(ctx, "catia_b5_face_loop_copy_id")?;
                ctx.push_vec(&mut copy, id, "catia_b5_face_loop_copy")?;
            }
            Ok(FaceLoops::unspecified(copy))
        }
        FaceLoops::Classified { outer, inner } => {
            let outer = outer.try_clone_for_decode(ctx, "catia_b5_outer_loop_copy_id")?;
            let mut copy = Vec::new();
            for id in inner {
                let id = id.try_clone_for_decode(ctx, "catia_b5_inner_loop_copy_id")?;
                ctx.push_vec(&mut copy, id, "catia_b5_inner_loop_copy")?;
            }
            Ok(FaceLoops::classified(outer, copy))
        }
    }
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
        let face_loops = b5_face_loops(
            admission.context(),
            ir,
            graph,
            face,
            loop_orientation,
            surface_ids,
            pcurve_uses,
        )?;
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
        let face_loop_copy = copy_face_loops(admission.context(), &face_loops)?;
        admission.reserve_entity(&mut ir.model.faces, "catia_b5_emit_faces")?;
        ir.model.faces.push(Face {
            id: face_record_id,
            shell: face_shell_id,
            surface: face_surface_id,
            sense: Sense::Forward,
            loops: face_loop_copy,
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
            if face_loops.role(&loop_id) != LoopBoundaryRole::Unspecified {
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
    use std::collections::{BTreeMap, HashMap};

    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{
        pcurve::{Pcurve, PcurveGeometry},
        SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{LoopId, PcurveId, SurfaceId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    use super::super::super::graph::{B5Face, B5Graph, B5Loop, B5LoopMember, B5LoopMetadata};
    use super::{b5_face_loops, OrientedLoop, OrientedLoopMember};

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
                let edge = edge_base + u32::try_from(member).expect("fixture value fits u32");
                let pcurve = pcurve_base + u32::try_from(member).expect("fixture value fits u32");
                edge_vertices.insert(
                    edge,
                    [start, end].map(crate::families::b5::graph::vertex_refs::B5VertexRef::Raw),
                );
                loop_edges.push(edge);
                loop_pcurves.push(pcurve);
                let start_point = points[start];
                let end_point = points[end];
                pcurves.push(Pcurve {
                    id: PcurveId::mint(format!("catia:test:pcurve#pc%23{pcurve}"))
                        .expect("identity grammar"),
                    geometry: PcurveGeometry::Line(
                        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                            Point2::new(start_point[0], start_point[1]),
                            Point2::new(
                                end_point[0] - start_point[0],
                                end_point[1] - start_point[1],
                            ),
                        )
                        .expect("valid LinePcurve fixture"),
                    ),
                    metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                        None,
                        Some(
                            cadmpeg_ir::units::FiniteVector::new([0.0, 1.0])
                                .expect("finite fixture range"),
                        ),
                        None,
                    ),
                });
                pcurve_uses.insert(
                    (loop_id, member),
                    (
                        PcurveId::mint(format!("catia:test:pcurve#pc%23{pcurve}"))
                            .expect("identity grammar"),
                        crate::test_support::test_b5::finite_pair([0.0, 1.0]),
                    ),
                );
            }
            loops.insert(
                loop_id,
                B5Loop {
                    object_id: loop_id,
                    members: loop_pcurves
                        .into_iter()
                        .zip(loop_edges)
                        .map(|(pcurve, edge)| B5LoopMember {
                            pcurve,
                            edge,
                            controls: [0, 0, 0],
                        })
                        .collect(),
                    metadata: B5LoopMetadata {
                        framing_controls:
                            [crate::families::b5::graph::controls::B5FramingControl::Control03; 2],
                        extension: None,
                    },
                    surface: 10,
                },
            );
            orientations.insert(
                loop_id,
                OrientedLoop {
                    flipped: false,
                    members: vec![
                        OrientedLoopMember {
                            reversed: false,
                            pcurve_reversed: false
                        };
                        4
                    ],
                },
            );
        }
        let graph = B5Graph {
            complete: true,
            faces: vec![B5Face {
                object_id: 1,
                surface: 10,
                loops: vec![3, 2],
                terminal_control: Some(
                    crate::families::b5::graph::controls::B5FramingControl::Control03,
                ),
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
            vertices: crate::families::b5::graph::vertex_refs::B5Vertices::try_new(
                points
                    .into_iter()
                    .map(crate::test_support::test_b5::point)
                    .collect(),
                Vec::new(),
                edge_vertices,
            )
            .expect("valid vertex bindings"),
            edge_parameter_incidences: BTreeMap::new(),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("catia:test:surface#surface%2310".to_string())
                .expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
        ir.model.pcurves = pcurves;

        assert_eq!(
            crate::test_support::with_service_context(|ctx| b5_face_loops(
                ctx,
                &ir,
                &graph,
                &graph.faces[0],
                &orientations,
                &HashMap::from([(
                    10,
                    SurfaceId::mint("catia:test:surface#surface%2310".to_string())
                        .expect("identity grammar")
                )]),
                &pcurve_uses,
            ))
            .expect("service context admits B5 face loops"),
            cadmpeg_ir::topology::FaceLoops::classified(
                LoopId::mint("catia:b5:loop#2".to_string()).expect("identity grammar"),
                vec![LoopId::mint("catia:b5:loop#3".to_string()).expect("identity grammar")]
            )
        );
    }
}
