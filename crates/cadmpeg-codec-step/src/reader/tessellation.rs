// SPDX-License-Identifier: Apache-2.0
//! AP242 indexed tessellation decoding.

use crate::ids::kind;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::{ShadedVertex, Tessellation};
use cadmpeg_ir::transform::Transform;

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::StageOutcome;
use super::{RecordExt, ValueExt};

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    let mut admitted_meshes = u64_from_index(ir.model.tessellations.len());
    let mut coordinates = BTreeMap::new();
    let mut coordinate_map_bytes = ctx.reserve_scoped(0, "step_tessellation_coordinate_lists")?;
    for (&id, record) in exchange.records() {
        if !has_entity(record, "COORDINATES_LIST") {
            continue;
        }
        let scale = geometry.units.length([id]);
        if let Some(vertices) = coordinate_rows(record, scale, ctx)? {
            ctx.charge_collection_items(1, "step_tessellation_coordinate_lists")?;
            coordinate_map_bytes.grow(bytes_for::<(u64, Vec<Point3>)>(
                1,
                ctx,
                "step_tessellation_coordinate_lists",
            )?)?;
            coordinates.insert(id, vertices);
        }
    }
    let mut typed = HashSet::new();
    let mut losses = Vec::new();
    let mut item_bodies = BTreeMap::<u64, BTreeSet<BodyId>>::new();
    let mut item_placements = BTreeMap::<u64, Vec<Transform>>::new();
    let mut unresolved_placements = BTreeSet::new();
    let mut declared_items = BTreeSet::new();
    let mut unresolved_containers = BTreeSet::new();
    let mut body_context_items = BTreeSet::new();
    let mut reservations = AssociationReservations::new(ctx)?;
    for (&id, record) in exchange.records() {
        let Some(kind) = entity_kind(record, &["TESSELLATED_SOLID", "TESSELLATED_SHELL"]) else {
            continue;
        };
        let Some(items) = entity_parameter(record, kind, 0, 1).and_then(ValueExt::list) else {
            push_loss(
                &mut losses,
                StepLossCode::DecodeWarning,
                format_args!("{kind} #{id} has no structured items"),
                ctx,
            )?;
            continue;
        };
        let (item_ids, _container_item_bytes) = container_item_ids(items, kind, id, ctx)?;
        for &item in &item_ids {
            insert_temporary_set(
                &mut declared_items,
                item,
                ctx,
                &mut reservations.declared,
                "step_tessellation_declared_items",
            )?;
        }
        let (candidates, _linked_body_bytes) = linked_bodies(record, kind, topology, ctx)?;
        if candidates.is_empty() {
            insert_temporary_set(
                &mut unresolved_containers,
                id,
                ctx,
                &mut reservations.unresolved_containers,
                "step_tessellation_unresolved_containers",
            )?;
        }
        let mut body_candidate_bytes = temporary_collection::<BodyId>(
            ctx,
            candidates.len(),
            "step_tessellation_body_candidates",
        )?;
        for body in &candidates {
            body_candidate_bytes.grow(u64_from_index(body.as_str().len()))?;
        }
        let body_candidates = candidates.iter().cloned().collect::<Vec<_>>();
        let mut associator = TessellationItemAssociator {
            bodies: &body_candidates,
            exchange,
            item_bodies: &mut item_bodies,
            declared_items: &mut declared_items,
            unresolved_containers: &mut unresolved_containers,
            typed: &mut typed,
            geometry,
            placements: &mut item_placements,
            unresolved_placements: &mut unresolved_placements,
            body_context_items: &mut body_context_items,
            mode: AssociationMode::BodyItems,
            active: BTreeSet::new(),
            reservations: &mut reservations,
            ctx,
        };
        for item in item_ids {
            associator.visit(item, None)?;
        }
        insert_claim(&mut typed, id, ctx)?;
    }
    let mut representation_cache = BTreeMap::new();
    let (product_representations, product_representation_bytes) =
        product_linked_representations(exchange, ctx)?;
    let (product_representation_items, product_item_bytes) =
        product_representation_items(exchange, &product_representations, ctx)?;
    for (&id, record) in exchange.records() {
        if !is_tessellated_shape_representation(record) {
            continue;
        }
        let Some((items, _representation_item_bytes)) = admitted_representation_items(record, ctx)?
        else {
            continue;
        };
        let bodies = super::topology::representation_bodies(
            id,
            exchange,
            topology,
            &mut representation_cache,
            &mut BTreeSet::new(),
            0,
            Some(ctx),
        )?;
        let product_linked = product_representations.contains(&id)
            || items
                .iter()
                .any(|item| product_representation_items.contains(item));
        if bodies.is_empty() && !product_linked {
            continue;
        }
        let mut associator = TessellationItemAssociator {
            bodies: &bodies,
            exchange,
            item_bodies: &mut item_bodies,
            declared_items: &mut declared_items,
            unresolved_containers: &mut unresolved_containers,
            typed: &mut typed,
            geometry,
            placements: &mut item_placements,
            unresolved_placements: &mut unresolved_placements,
            body_context_items: &mut body_context_items,
            mode: AssociationMode::Placements,
            active: BTreeSet::new(),
            reservations: &mut reservations,
            ctx,
        };
        for item in items {
            associator.visit(item, None)?;
        }
    }
    drop(product_representation_items);
    drop(product_item_bytes);
    drop(product_representations);
    drop(product_representation_bytes);
    drop(representation_cache);
    for (&id, record) in exchange.records() {
        if !has_entity(record, "TESSELLATED_ANNOTATION_OCCURRENCE") {
            continue;
        }
        let Some(item) = tessellated_annotation_item(record) else {
            push_loss(
                &mut losses,
                StepLossCode::DecodeWarning,
                format_args!("TESSELLATED_ANNOTATION_OCCURRENCE #{id} has no tessellated item"),
                ctx,
            )?;
            continue;
        };
        let mut associator = TessellationItemAssociator {
            bodies: &[],
            exchange,
            item_bodies: &mut item_bodies,
            declared_items: &mut declared_items,
            unresolved_containers: &mut unresolved_containers,
            typed: &mut typed,
            geometry,
            placements: &mut item_placements,
            unresolved_placements: &mut unresolved_placements,
            body_context_items: &mut body_context_items,
            mode: AssociationMode::DetachedAnnotation,
            active: BTreeSet::new(),
            reservations: &mut reservations,
            ctx,
        };
        associator.visit(item, None)?;
    }
    for id in unresolved_placements {
        push_loss(
            &mut losses,
            StepLossCode::TessellationPlacementUnresolved,
            format_args!("repositioned tessellated item #{id} has no valid AXIS2_PLACEMENT_3D; unresolved placement is not applied"),
            ctx,
        )?;
    }
    reservations.unresolved_placements =
        ctx.reserve_scoped(0, "step_tessellation_unresolved_placements")?;
    for id in unresolved_containers {
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let Some(kind) = entity_kind(record, &["TESSELLATED_SOLID", "TESSELLATED_SHELL"]) else {
            continue;
        };
        push_loss(
            &mut losses,
            StepLossCode::DecodeWarning,
            format_args!("{kind} #{id} has no decoded exact body link"),
            ctx,
        )?;
    }
    reservations.unresolved_containers =
        ctx.reserve_scoped(0, "step_tessellation_unresolved_containers")?;
    for (&item, placements) in &item_placements {
        if !item_bodies.get(&item).is_some_and(BTreeSet::is_empty) {
            continue;
        }
        if distinct_placement(placements).is_none() {
            let distinct_count = placements
                .iter()
                .enumerate()
                .filter(|(index, placement)| !placements[..*index].contains(placement))
                .count();
            push_loss(
                &mut losses,
                StepLossCode::TessellationPlacementAmbiguous,
                format_args!("tessellation item #{item} has {distinct_count} distinct repositioning placements; mesh retained in source coordinates"),
                ctx,
            )?;
        }
    }
    for (&item, bodies) in &item_bodies {
        if body_context_items.contains(&item) && bodies.len() != 1 {
            let detail = if bodies.is_empty() {
                "no decoded body"
            } else if bodies.len() > 1 {
                "multiple candidate bodies"
            } else {
                "an unresolved container association"
            };
            push_loss(
                &mut losses,
                StepLossCode::TessellationItemBodyUnresolved,
                format_args!("tessellation item #{item} has {detail}; mesh retained as detached"),
                ctx,
            )?;
        }
    }
    for (&id, record) in exchange.records() {
        let Some(entity) = TriangulatedEntity::of(record) else {
            continue;
        };
        let kind = entity.name();
        let base_kind = entity.base_name();
        let Some(coordinate_id) =
            inherited_parameter(record, base_kind, 0).and_then(ValueExt::reference)
        else {
            push_loss(
                &mut losses,
                StepLossCode::DecodeWarning,
                format_args!("{kind} #{id} has no COORDINATES_LIST reference"),
                ctx,
            )?;
            continue;
        };
        let Some((vertices, _coordinate_bytes)) = coordinates.get(&coordinate_id) else {
            push_loss(
                &mut losses,
                StepLossCode::DecodeWarning,
                format_args!("{kind} #{id} has no resolved COORDINATES_LIST"),
                ctx,
            )?;
            continue;
        };
        let offset = entity.own_parameter_offset();
        let triangles = match entity {
            TriangulatedEntity::Face | TriangulatedEntity::SurfaceSet => {
                entity_parameter(record, kind, 1, offset)
                    .map(|value| triangle_rows(value, ctx))
                    .transpose()?
                    .flatten()
            }
            TriangulatedEntity::ComplexFace | TriangulatedEntity::ComplexSurfaceSet => {
                complex_triangles(
                    entity_parameter(record, kind, 1, offset),
                    entity_parameter(record, kind, 2, offset),
                    kind,
                    id,
                    ctx,
                )?
            }
        };
        let Some(AdmittedTriangles {
            triangles,
            bytes: triangle_bytes,
        }) = triangles.filter(|triangles| !triangles.triangles.is_empty())
        else {
            push_loss(
                &mut losses,
                StepLossCode::DecodeWarning,
                format_args!("{kind} #{id} has no triangle indices"),
                ctx,
            )?;
            continue;
        };
        let (pnindex, pnindex_bytes) = match entity_parameter(record, kind, 0, offset) {
            None | Some(Value::Omitted) => (Vec::new(), None),
            Some(value) => {
                let Some((indices, bytes)) = index_list(Some(value), ctx)? else {
                    push_loss(
                        &mut losses,
                        StepLossCode::DecodeWarning,
                        format_args!("{kind} #{id} has an invalid pnindex"),
                        ctx,
                    )?;
                    continue;
                };
                (indices, Some(bytes))
            }
        };
        let (
            mut local_vertices,
            local_triangles,
            addressing,
            _local_vertex_bytes,
            local_triangle_bytes,
            _coordinate_index_bytes,
        ) = if pnindex.is_empty() {
            if triangles
                .iter()
                .flatten()
                .any(|index| *index == 0 || *index as usize > vertices.len())
            {
                push_loss(
                    &mut losses,
                    StepLossCode::DecodeWarning,
                    format_args!("{kind} #{id} has an out-of-range one-based coordinate index"),
                    ctx,
                )?;
                continue;
            }
            let mut coordinate_indices = BTreeSet::new();
            let mut coordinate_index_bytes =
                ctx.reserve_scoped(0, "step_tessellation_coordinate_indices")?;
            for &index in triangles.iter().flatten() {
                if !coordinate_indices.contains(&index) {
                    ctx.charge_collection_items(1, "step_tessellation_coordinate_indices")?;
                    coordinate_index_bytes.grow(bytes_for::<u32>(
                        1,
                        ctx,
                        "step_tessellation_coordinate_indices",
                    )?)?;
                    coordinate_indices.insert(index);
                }
            }
            let _local_index_bytes = temporary_collection::<(u32, u32)>(
                ctx,
                coordinate_indices.len(),
                "step_tessellation_local_index",
            )?;
            let local_index = coordinate_indices
                .iter()
                .enumerate()
                .map(|(local, global)| {
                    u32::try_from(local)
                        .map(|local| (*global, local))
                        .map_err(|_| {
                            ctx.refuse_codec_limit(
                                "step_tessellation_local_index_width",
                                u64::from(u32::MAX),
                                u64_from_index(local),
                            )
                        })
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            let local_vertex_bytes = temporary_collection::<Point3>(
                ctx,
                coordinate_indices.len(),
                "step_tessellation_local_vertices",
            )?;
            let local_vertices = coordinate_indices
                .iter()
                .map(|index| vertices[*index as usize - 1])
                .collect::<Vec<_>>();
            let local_triangle_bytes = temporary_collection::<[u32; 3]>(
                ctx,
                triangles.len(),
                "step_tessellation_local_triangles",
            )?;
            let local_triangles = triangles
                .iter()
                .map(|triangle| triangle.map(|index| local_index[&index]))
                .collect::<Vec<_>>();
            (
                local_vertices,
                local_triangles,
                CoordinateAddressing::TriangleIndices(coordinate_indices),
                local_vertex_bytes,
                local_triangle_bytes,
                Some(coordinate_index_bytes),
            )
        } else {
            if pnindex
                .iter()
                .any(|index| *index == 0 || *index as usize > vertices.len())
                || triangles
                    .iter()
                    .flatten()
                    .any(|index| *index == 0 || *index as usize > pnindex.len())
            {
                push_loss(
                    &mut losses,
                    StepLossCode::DecodeWarning,
                    format_args!("{kind} #{id} has an out-of-range one-based tessellation index"),
                    ctx,
                )?;
                continue;
            }
            let local_vertex_bytes = temporary_collection::<Point3>(
                ctx,
                pnindex.len(),
                "step_tessellation_pn_vertices",
            )?;
            let local_triangle_bytes = temporary_collection::<[u32; 3]>(
                ctx,
                triangles.len(),
                "step_tessellation_pn_triangles",
            )?;
            (
                pnindex
                    .iter()
                    .map(|index| vertices[*index as usize - 1])
                    .collect(),
                triangles
                    .iter()
                    .map(|triangle| triangle.map(|index| index - 1))
                    .collect(),
                CoordinateAddressing::PnIndex,
                local_vertex_bytes,
                local_triangle_bytes,
                None,
            )
        };
        drop(triangles);
        drop(triangle_bytes);
        drop(pnindex);
        drop(pnindex_bytes);
        let (source_normals, _source_normal_bytes) = match inherited_parameter(record, base_kind, 2)
        {
            None | Some(Value::Omitted) => (Vec::new(), None),
            Some(value) => match normal_rows(Some(value), ctx)? {
                Some((normals, bytes)) => (normals, Some(bytes)),
                None => {
                    push_loss(
                        &mut losses,
                        StepLossCode::DecodeWarning,
                        format_args!("{kind} #{id} has invalid normal rows; normals omitted"),
                        ctx,
                    )?;
                    (Vec::new(), None)
                }
            },
        };
        // AP242 permits an empty normals aggregate; the IR represents both
        // that spelling and an omitted lane as an absent normal lane.
        let mut _replicated_normal_bytes = None;
        let mut _projected_normal_bytes = None;
        let mut normals = match source_normals.len() {
            0 => None,
            1 => {
                _replicated_normal_bytes = Some(ctx.reserve_scoped(
                    bytes_for::<Vector3>(
                        local_vertices.len(),
                        ctx,
                        "step_tessellation_normal_replication",
                    )?,
                    "step_tessellation_normal_replication",
                )?);
                Some(ctx.alloc_filled(
                    local_vertices.len(),
                    source_normals[0],
                    "step_tessellation_normal_replication",
                )?)
            }
            count if count == local_vertices.len() => Some(source_normals),
            count => match &addressing {
                CoordinateAddressing::TriangleIndices(coordinate_indices)
                    if count == vertices.len() =>
                {
                    _projected_normal_bytes = Some(temporary_collection::<Vector3>(
                        ctx,
                        coordinate_indices.len(),
                        "step_tessellation_projected_normals",
                    )?);
                    Some(
                        coordinate_indices
                            .iter()
                            .map(|index| source_normals[*index as usize - 1])
                            .collect(),
                    )
                }
                CoordinateAddressing::PnIndex | CoordinateAddressing::TriangleIndices(_) => {
                    push_loss(
                        &mut losses,
                        StepLossCode::DecodeWarning,
                        format_args!(
                            "{kind} #{id} carries {count} normals for {} coordinates",
                            local_vertices.len()
                        ),
                        ctx,
                    )?;
                    None
                }
            },
        };
        let mut _placed_vertex_bytes = None;
        let mut _placed_normal_bytes = None;
        if item_bodies.get(&id).is_some_and(BTreeSet::is_empty) {
            if let Some(placement) =
                distinct_placement(item_placements.get(&id).map_or(&[], Vec::as_slice))
            {
                _placed_vertex_bytes = Some(temporary_collection::<Point3>(
                    ctx,
                    local_vertices.len(),
                    "step_tessellation_placed_vertices",
                )?);
                local_vertices = local_vertices
                    .into_iter()
                    .map(|vertex| {
                        placement
                            .apply_point(vertex)
                            .map(cadmpeg_ir::features::FinitePoint3::get)
                    })
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        CodecError::malformed(format!(
                            "{kind} #{id} placed tessellation vertex contains a non-finite coordinate"
                        ))
                    })?;
                if let Some(source_normals) = normals.take() {
                    _placed_normal_bytes = Some(temporary_collection::<Vector3>(
                        ctx,
                        source_normals.len(),
                        "step_tessellation_placed_normals",
                    )?);
                    match source_normals
                        .into_iter()
                        .map(|normal| {
                            placement
                                .apply_normal(normal)
                                .map(cadmpeg_ir::math::Vector3::from)
                        })
                        .collect::<Option<Vec<_>>>()
                    {
                        Some(transformed) => normals = Some(transformed),
                        None => {
                            push_loss(
                                &mut losses,
                                StepLossCode::DecodeWarning,
                                format_args!("{kind} #{id} normal placement could not produce finite unit normals; normals omitted"),
                                ctx,
                            )?;
                        }
                    }
                }
            }
        }
        if let Some(surface_step) = complex_triangulated_face_surface(record) {
            let (surface_id, _surface_id_bytes) = admitted_surface_id(surface_step, ctx)?;
            if let Some(surface) = ir
                .model
                .surfaces
                .iter_mut()
                .find(|surface| surface.id.as_str() == surface_id.as_str())
            {
                if surface.source_object.is_none() {
                    surface.source_object = Some(admitted_source_association(id, ctx)?);
                }
            }
        }
        let vertex_count = local_vertices.len();
        let triangle_count = local_triangles.len();
        let shaded = normals.is_some();
        let _shaded_row_bytes = if shaded {
            ctx.charge_collection_items(
                u64_from_index(vertex_count),
                "step_tessellation_shaded_rows",
            )?;
            Some(ctx.reserve_scoped(
                bytes_for::<ShadedVertex<Point3, Vector3>>(
                    vertex_count,
                    ctx,
                    "step_tessellation_shaded_rows",
                )?,
                "step_tessellation_shaded_rows",
            )?)
        } else {
            None
        };
        let rows = match cadmpeg_ir::tessellation::TessellationMesh::from_list_lanes(
            local_vertices,
            local_triangles,
            normals,
        ) {
            Ok(rows) => rows,
            Err(error) => {
                push_loss(
                    &mut losses,
                    StepLossCode::TessellationInvalidPayload,
                    format_args!("{kind} #{id}: {error}"),
                    ctx,
                )?;
                continue;
            }
        };
        ctx.charge_collection_items(
            u64_from_index(vertex_count),
            "step_tessellation_admitted_vertices",
        )?;
        let _admitted_vertex_bytes = ctx.reserve_scoped(
            if shaded {
                bytes_for::<ShadedVertex<FinitePoint3, Vector3>>(
                    vertex_count,
                    ctx,
                    "step_tessellation_admitted_vertices",
                )?
            } else {
                bytes_for::<FinitePoint3>(vertex_count, ctx, "step_tessellation_admitted_vertices")?
            },
            "step_tessellation_admitted_vertices",
        )?;
        let _admitted_normal_bytes = if shaded {
            ctx.charge_collection_items(
                u64_from_index(vertex_count),
                "step_tessellation_admitted_normals",
            )?;
            Some(ctx.reserve_scoped(
                bytes_for::<ShadedVertex<FinitePoint3, FiniteVector3>>(
                    vertex_count,
                    ctx,
                    "step_tessellation_admitted_normals",
                )?,
                "step_tessellation_admitted_normals",
            )?)
        } else {
            None
        };
        let _validation_triangle_bytes = temporary_collection::<[u32; 3]>(
            ctx,
            triangle_count,
            "step_tessellation_validation_triangles",
        )?;
        let retained_vertex_bytes = if shaded {
            bytes_for::<ShadedVertex<FinitePoint3, FiniteVector3>>(
                vertex_count,
                ctx,
                "step_tessellation_ir_mesh",
            )?
        } else {
            bytes_for::<FinitePoint3>(vertex_count, ctx, "step_tessellation_ir_mesh")?
        };
        ctx.charge_retained(retained_vertex_bytes, "step_tessellation_ir_mesh")?;
        let next_mesh_count = admitted_meshes.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("step_tessellation_mesh_entity", u64::MAX - 1, u64::MAX)
        })?;
        let mut pending_meshes = admitted_meshes;
        ctx.admit_entities(
            next_mesh_count,
            &mut pending_meshes,
            "step_tessellation_mesh_entity",
        )?;
        let mesh = match Tessellation::new(admitted_mesh_id(id, ctx)?, rows, Vec::new()) {
            Ok(mesh) => mesh,
            Err(error) => {
                push_loss(
                    &mut losses,
                    StepLossCode::TessellationInvalidPayload,
                    format_args!("{kind} #{id}: {error}"),
                    ctx,
                )?;
                continue;
            }
        };
        admitted_meshes = pending_meshes;
        local_triangle_bytes.commit()?;
        if !declared_items.contains(&id) {
            push_loss(
                &mut losses,
                StepLossCode::TessellationItemUndeclared,
                format_args!("tessellation item #{id} is not declared by an exact body container; mesh retained as detached"),
                ctx,
            )?;
        }
        ctx.charge_collection_items(1, "step_tessellation_mesh_list")?;
        ctx.charge_retained(
            bytes_for::<Tessellation>(1, ctx, "step_tessellation_mesh_list")?,
            "step_tessellation_mesh_list",
        )?;
        let body = (!item_bodies.get(&id).is_some_and(BTreeSet::is_empty))
            .then(|| item_bodies.get(&id))
            .flatten()
            .filter(|bodies| bodies.len() == 1)
            .and_then(|bodies| bodies.iter().next());
        let body = admitted_mesh_body(body, ctx)?;
        let source_object = if !declared_items.contains(&id)
            || item_bodies.get(&id).is_some_and(BTreeSet::is_empty)
            || item_bodies.get(&id).is_none_or(|bodies| bodies.len() != 1)
        {
            Some(admitted_source_association(id, ctx)?)
        } else {
            None
        };
        ir.model
            .tessellations
            .push(mesh.with_body(body).with_source_object(source_object));
        insert_claim(&mut typed, id, ctx)?;
        insert_claim(&mut typed, coordinate_id, ctx)?;
    }
    if !ir.model.tessellations.is_empty() {
        for (&id, record) in exchange.records() {
            if has_entity(record, "TESSELLATED_SHAPE_REPRESENTATION")
                || has_entity(record, "TESSELLATED_SOLID")
                || has_entity(record, "TESSELLATED_SHELL")
            {
                insert_claim(&mut typed, id, ctx)?;
            }
        }
    }
    Ok(StageOutcome {
        value: (),
        claims: typed,
        losses,
        notes: Vec::new(),
    })
}

/// How a tessellated item addresses the coordinate list it shares.
///
/// A STEP tessellated item states either a PNINDEX list, which names the
/// coordinates it uses, or no PNINDEX at all, in which case its triangle
/// indices name them directly. The two are one statement of the record, so
/// they are one value: the coordinate set exists exactly when the record
/// states no PNINDEX.
enum CoordinateAddressing {
    /// The record states a PNINDEX list.
    PnIndex,
    /// The record states no PNINDEX; these triangle indices name the
    /// coordinates it uses, in ascending order.
    TriangleIndices(BTreeSet<u32>),
}

#[derive(Debug)]
struct AdmittedTriangles<'a> {
    triangles: Vec<[u32; 3]>,
    bytes: ScopedReservation<'a>,
}

fn complex_triangulated_face_surface(record: &RawRecord) -> Option<u64> {
    has_entity(record, "COMPLEX_TRIANGULATED_FACE")
        .then(|| inherited_parameter(record, "TESSELLATED_FACE", 3))
        .flatten()
        .and_then(ValueExt::reference)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AssociationMode {
    BodyItems,
    Placements,
    DetachedAnnotation,
}

struct AssociationReservations<'a> {
    declared: ScopedReservation<'a>,
    unresolved_containers: ScopedReservation<'a>,
    unresolved_placements: ScopedReservation<'a>,
    body_context: ScopedReservation<'a>,
    item_bodies: ScopedReservation<'a>,
    placements: ScopedReservation<'a>,
}

impl<'a> AssociationReservations<'a> {
    fn new(ctx: &'a DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            declared: ctx.reserve_scoped(0, "step_tessellation_declared_items")?,
            unresolved_containers: ctx
                .reserve_scoped(0, "step_tessellation_unresolved_containers")?,
            unresolved_placements: ctx
                .reserve_scoped(0, "step_tessellation_unresolved_placements")?,
            body_context: ctx.reserve_scoped(0, "step_tessellation_body_context_items")?,
            item_bodies: ctx.reserve_scoped(0, "step_tessellation_item_bodies")?,
            placements: ctx.reserve_scoped(0, "step_tessellation_item_placements")?,
        })
    }
}

struct TessellationItemAssociator<'a, 'ctx, 'arena> {
    bodies: &'a [BodyId],
    exchange: &'a Exchange,
    item_bodies: &'a mut BTreeMap<u64, BTreeSet<BodyId>>,
    declared_items: &'a mut BTreeSet<u64>,
    unresolved_containers: &'a mut BTreeSet<u64>,
    typed: &'a mut HashSet<u64>,
    geometry: &'a GeometryData,
    placements: &'a mut BTreeMap<u64, Vec<Transform>>,
    unresolved_placements: &'a mut BTreeSet<u64>,
    body_context_items: &'a mut BTreeSet<u64>,
    mode: AssociationMode,
    active: BTreeSet<u64>,
    reservations: &'a mut AssociationReservations<'ctx>,
    ctx: &'ctx DecodeContext<'arena>,
}

impl TessellationItemAssociator<'_, '_, '_> {
    fn visit(&mut self, id: u64, inherited_placement: Option<Transform>) -> Result<(), CodecError> {
        let _depth_guard = self.ctx.enter_nested("step_tessellation_association")?;
        if self.active.contains(&id) {
            return Ok(());
        }
        self.ctx
            .charge_collection_items(1, "step_tessellation_active_items")?;
        let _active_bytes = self.ctx.reserve_scoped(
            bytes_for::<u64>(1, self.ctx, "step_tessellation_active_items")?,
            "step_tessellation_active_items",
        )?;
        self.active.insert(id);
        let Some(record) = self.exchange.records().get(&id) else {
            self.active.remove(&id);
            return Ok(());
        };
        let local_placement = if has_entity(record, "REPOSITIONED_TESSELLATED_ITEM") {
            let placement = repositioned_placement(record, self.geometry);
            if placement.is_none() {
                insert_temporary_set(
                    self.unresolved_placements,
                    id,
                    self.ctx,
                    &mut self.reservations.unresolved_placements,
                    "step_tessellation_unresolved_placements",
                )?;
            }
            placement
        } else {
            None
        };
        let placement = match (inherited_placement, local_placement) {
            (Some(parent), Some(local)) => Some(parent.compose(local).map_err(|error| {
                CodecError::malformed(format_args!(
                    "invalid STEP tessellation placement #{id}: {error}"
                ))
            })?),
            (parent, None) => parent,
            (None, local) => local,
        };
        if entity_kind(
            record,
            &[
                "TRIANGULATED_FACE",
                "COMPLEX_TRIANGULATED_FACE",
                "TRIANGULATED_SURFACE_SET",
                "COMPLEX_TRIANGULATED_SURFACE_SET",
            ],
        )
        .is_some()
        {
            if self.mode != AssociationMode::DetachedAnnotation {
                insert_temporary_set(
                    self.body_context_items,
                    id,
                    self.ctx,
                    &mut self.reservations.body_context,
                    "step_tessellation_body_context_items",
                )?;
            }
            insert_temporary_set(
                self.declared_items,
                id,
                self.ctx,
                &mut self.reservations.declared,
                "step_tessellation_declared_items",
            )?;
            associate_bodies(
                self.item_bodies,
                id,
                self.bodies,
                self.ctx,
                &mut self.reservations.item_bodies,
            )?;
            if let Some(placement) = placement {
                push_placement(
                    self.placements,
                    id,
                    placement,
                    self.ctx,
                    &mut self.reservations.placements,
                )?;
            }
        } else if let Some(kind) = entity_kind(
            record,
            &[
                "TESSELLATED_SOLID",
                "TESSELLATED_SHELL",
                "TESSELLATED_GEOMETRIC_SET",
            ],
        ) {
            if self.mode == AssociationMode::BodyItems {
                insert_temporary_set(
                    self.declared_items,
                    id,
                    self.ctx,
                    &mut self.reservations.declared,
                    "step_tessellation_declared_items",
                )?;
                associate_bodies(
                    self.item_bodies,
                    id,
                    self.bodies,
                    self.ctx,
                    &mut self.reservations.item_bodies,
                )?;
            }
            if self.mode != AssociationMode::DetachedAnnotation {
                insert_claim(self.typed, id, self.ctx)?;
            }
            if !self.bodies.is_empty() && matches!(kind, "TESSELLATED_SOLID" | "TESSELLATED_SHELL")
            {
                self.unresolved_containers.remove(&id);
            }
            let (item_ids, _container_item_bytes) =
                match entity_parameter(record, kind, 0, 1).and_then(ValueExt::list) {
                    Some(items) => container_item_ids(items, kind, id, self.ctx)?,
                    None => (
                        Vec::new(),
                        self.ctx
                            .reserve_scoped(0, "step_tessellation_container_items")?,
                    ),
                };
            for item in item_ids {
                self.visit(item, placement)?;
            }
        } else if self.mode == AssociationMode::BodyItems {
            insert_temporary_set(
                self.declared_items,
                id,
                self.ctx,
                &mut self.reservations.declared,
                "step_tessellation_declared_items",
            )?;
            associate_bodies(
                self.item_bodies,
                id,
                self.bodies,
                self.ctx,
                &mut self.reservations.item_bodies,
            )?;
        }
        self.active.remove(&id);
        Ok(())
    }
}

fn distinct_placement(placements: &[Transform]) -> Option<Transform> {
    let first = *placements.first()?;
    placements
        .iter()
        .all(|placement| *placement == first)
        .then_some(first)
}

fn repositioned_placement(record: &RawRecord, geometry: &GeometryData) -> Option<Transform> {
    let placement_id = entity_parameter(record, "REPOSITIONED_TESSELLATED_ITEM", 0, 1)
        .and_then(ValueExt::reference)?;
    geometry
        .placements
        .get(&placement_id)
        .copied()
        .and_then(super::geometry::placement_transform)
}

fn tessellated_annotation_item(record: &RawRecord) -> Option<u64> {
    for name in ["TESSELLATED_ANNOTATION_OCCURRENCE", "STYLED_ITEM"] {
        let Some(partial) = record.partials.iter().find(|partial| partial.name == name) else {
            continue;
        };
        if let Some(item) = partial
            .parameters
            .iter()
            .rev()
            .find_map(ValueExt::reference)
        {
            return Some(item);
        }
    }
    None
}

fn insert_temporary_set<T: Ord + Copy>(
    values: &mut BTreeSet<T>,
    value: T,
    ctx: &DecodeContext<'_>,
    bytes: &mut ScopedReservation<'_>,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if values.contains(&value) {
        return Ok(false);
    }
    ctx.charge_collection_items(1, operation)?;
    bytes.grow(bytes_for::<T>(1, ctx, operation)?)?;
    Ok(values.insert(value))
}

fn insert_claim(
    claims: &mut HashSet<u64>,
    id: u64,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if !claims.contains(&id) {
        ctx.charge_collection_items(1, "step_tessellation_claims")?;
        ctx.charge_retained(
            bytes_for::<u64>(1, ctx, "step_tessellation_claims")?,
            "step_tessellation_claims",
        )?;
        claims.insert(id);
    }
    Ok(())
}

fn associate_bodies(
    associations: &mut BTreeMap<u64, BTreeSet<BodyId>>,
    item: u64,
    bodies: &[BodyId],
    ctx: &DecodeContext<'_>,
    bytes: &mut ScopedReservation<'_>,
) -> Result<(), CodecError> {
    if !associations.contains_key(&item) {
        ctx.charge_collection_items(1, "step_tessellation_item_body_entries")?;
        bytes.grow(bytes_for::<(u64, BTreeSet<BodyId>)>(
            1,
            ctx,
            "step_tessellation_item_body_entries",
        )?)?;
    }
    let associated = associations.entry(item).or_default();
    for body in bodies {
        if !associated.contains(body) {
            ctx.charge_collection_items(1, "step_tessellation_item_body_links")?;
            let body_bytes = bytes_for::<BodyId>(1, ctx, "step_tessellation_item_body_links")?
                .checked_add(u64_from_index(body.as_str().len()))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "step_tessellation_item_body_links",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?;
            bytes.grow(body_bytes)?;
            associated.insert(body.clone());
        }
    }
    Ok(())
}

fn push_placement(
    placements: &mut BTreeMap<u64, Vec<Transform>>,
    item: u64,
    placement: Transform,
    ctx: &DecodeContext<'_>,
    bytes: &mut ScopedReservation<'_>,
) -> Result<(), CodecError> {
    if !placements.contains_key(&item) {
        ctx.charge_collection_items(1, "step_tessellation_placement_entries")?;
        bytes.grow(bytes_for::<(u64, Vec<Transform>)>(
            1,
            ctx,
            "step_tessellation_placement_entries",
        )?)?;
    }
    ctx.charge_collection_items(1, "step_tessellation_placements")?;
    bytes.grow(bytes_for::<Transform>(
        1,
        ctx,
        "step_tessellation_placements",
    )?)?;
    placements.entry(item).or_default().push(placement);
    Ok(())
}

fn insert_relationship(
    relationships: &mut BTreeMap<u64, BTreeSet<u64>>,
    source: u64,
    target: u64,
    ctx: &DecodeContext<'_>,
    bytes: &mut ScopedReservation<'_>,
) -> Result<(), CodecError> {
    if !relationships.contains_key(&source) {
        ctx.charge_collection_items(1, "step_tessellation_relationship_nodes")?;
        bytes.grow(bytes_for::<(u64, BTreeSet<u64>)>(
            1,
            ctx,
            "step_tessellation_relationship_nodes",
        )?)?;
    }
    let targets = relationships.entry(source).or_default();
    insert_temporary_set(
        targets,
        target,
        ctx,
        bytes,
        "step_tessellation_relationship_edges",
    )?;
    Ok(())
}

fn product_linked_representations<'a>(
    exchange: &Exchange,
    ctx: &'a DecodeContext<'_>,
) -> Result<(BTreeSet<u64>, ScopedReservation<'a>), CodecError> {
    let mut product_shape_definitions = BTreeSet::new();
    let mut definition_bytes = ctx.reserve_scoped(0, "step_tessellation_product_definitions")?;
    for (id, record) in exchange.entities("PRODUCT_DEFINITION_SHAPE") {
        if record
            .partials
            .iter()
            .any(|partial| partial.name == "PRODUCT_DEFINITION_SHAPE")
        {
            insert_temporary_set(
                &mut product_shape_definitions,
                id,
                ctx,
                &mut definition_bytes,
                "step_tessellation_product_definitions",
            )?;
        }
    }
    let mut linked = BTreeSet::new();
    let mut linked_bytes = ctx.reserve_scoped(0, "step_tessellation_product_representations")?;
    for (_, record) in exchange.entities("SHAPE_DEFINITION_REPRESENTATION") {
        let representation = (|| {
            let partial = record
                .partials
                .iter()
                .find(|partial| partial.name == "SHAPE_DEFINITION_REPRESENTATION")?;
            let definition = partial.parameters.first().and_then(ValueExt::reference)?;
            product_shape_definitions
                .contains(&definition)
                .then(|| partial.parameters.get(1).and_then(ValueExt::reference))?
        })();
        if let Some(representation) = representation {
            insert_temporary_set(
                &mut linked,
                representation,
                ctx,
                &mut linked_bytes,
                "step_tessellation_product_representations",
            )?;
        }
    }
    if linked.is_empty() {
        return Ok((linked, linked_bytes));
    }
    let mut relationships = BTreeMap::<u64, BTreeSet<u64>>::new();
    let mut relationship_bytes = ctx.reserve_scoped(0, "step_tessellation_relationship_edges")?;
    for record in exchange.records().values() {
        let Some(shape_relationship) = record
            .partials
            .iter()
            .find(|partial| partial.name == "SHAPE_REPRESENTATION_RELATIONSHIP")
        else {
            continue;
        };
        let (left, right) = {
            let mut references = shape_relationship
                .parameters
                .iter()
                .filter_map(ValueExt::reference);
            match (references.next(), references.next()) {
                (Some(left), Some(right)) => (left, right),
                _ => {
                    let Some(base) = record
                        .partials
                        .iter()
                        .find(|partial| partial.name == "REPRESENTATION_RELATIONSHIP")
                    else {
                        continue;
                    };
                    let mut references = base.parameters.iter().filter_map(ValueExt::reference);
                    let (Some(left), Some(right)) = (references.next(), references.next()) else {
                        continue;
                    };
                    (left, right)
                }
            }
        };
        insert_relationship(
            &mut relationships,
            left,
            right,
            ctx,
            &mut relationship_bytes,
        )?;
        insert_relationship(
            &mut relationships,
            right,
            left,
            ctx,
            &mut relationship_bytes,
        )?;
    }
    let mut pending_bytes =
        temporary_collection::<u64>(ctx, linked.len(), "step_tessellation_product_pending")?;
    let mut pending = linked.iter().copied().collect::<Vec<_>>();
    while let Some(representation) = pending.pop() {
        for &related in relationships.get(&representation).into_iter().flatten() {
            if insert_temporary_set(
                &mut linked,
                related,
                ctx,
                &mut linked_bytes,
                "step_tessellation_product_representations",
            )? {
                ctx.charge_collection_items(1, "step_tessellation_product_pending")?;
                pending_bytes.grow(bytes_for::<u64>(
                    1,
                    ctx,
                    "step_tessellation_product_pending",
                )?)?;
                pending.push(related);
            }
        }
    }
    Ok((linked, linked_bytes))
}

fn product_representation_items<'a>(
    exchange: &Exchange,
    representations: &BTreeSet<u64>,
    ctx: &'a DecodeContext<'_>,
) -> Result<(BTreeSet<u64>, ScopedReservation<'a>), CodecError> {
    let mut items = BTreeSet::new();
    let mut bytes = ctx.reserve_scoped(0, "step_tessellation_product_items")?;
    for record in representations
        .iter()
        .filter_map(|id| exchange.records().get(id))
    {
        if let Some((representation_items, _representation_item_bytes)) =
            admitted_representation_items(record, ctx)?
        {
            for item in representation_items {
                insert_temporary_set(
                    &mut items,
                    item,
                    ctx,
                    &mut bytes,
                    "step_tessellation_product_items",
                )?;
            }
        }
    }
    Ok((items, bytes))
}

fn admitted_representation_items<'a>(
    record: &RawRecord,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<(Vec<u64>, ScopedReservation<'a>)>, CodecError> {
    let Some(values) = super::representation::item_values(record) else {
        return Ok(None);
    };
    let count = values
        .iter()
        .filter(|value| value.reference().is_some())
        .count();
    let bytes = temporary_collection::<u64>(ctx, count, "step_tessellation_representation_items")?;
    let items = values.iter().filter_map(ValueExt::reference).collect();
    Ok(Some((items, bytes)))
}

fn linked_bodies<'a>(
    record: &RawRecord,
    kind: &str,
    topology: &TopologyData,
    ctx: &'a DecodeContext<'_>,
) -> Result<(BTreeSet<BodyId>, ScopedReservation<'a>), CodecError> {
    let Some(link) = entity_parameter(record, kind, 1, 1).and_then(ValueExt::reference) else {
        return Ok((
            BTreeSet::new(),
            ctx.reserve_scoped(0, "step_tessellation_linked_bodies")?,
        ));
    };
    match kind {
        "TESSELLATED_SOLID" => {
            let bodies = topology.body_by_root.get(&link);
            let count = bodies.map_or(0, Vec::len);
            let mut bytes =
                temporary_collection::<BodyId>(ctx, count, "step_tessellation_linked_bodies")?;
            for body in bodies.into_iter().flatten() {
                bytes.grow(u64_from_index(body.as_str().len()))?;
            }
            let linked = bodies.into_iter().flatten().cloned().collect();
            Ok((linked, bytes))
        }
        "TESSELLATED_SHELL" => {
            let bodies = topology.body_by_shell.get(&link);
            let count = bodies.map_or(0, BTreeSet::len);
            let mut bytes =
                temporary_collection::<BodyId>(ctx, count, "step_tessellation_linked_bodies")?;
            for body in bodies.into_iter().flatten() {
                bytes.grow(u64_from_index(body.as_str().len()))?;
            }
            Ok((bodies.cloned().unwrap_or_default(), bytes))
        }
        _ => Ok((
            BTreeSet::new(),
            ctx.reserve_scoped(0, "step_tessellation_linked_bodies")?,
        )),
    }
}

fn index_list<'a>(
    value: Option<&Value>,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<(Vec<u32>, ScopedReservation<'a>)>, CodecError> {
    let Some(values) = value.and_then(ValueExt::list) else {
        return Ok(None);
    };
    let bytes = temporary_collection::<u32>(ctx, values.len(), "step_tessellation_pnindex")?;
    Ok(values
        .iter()
        .map(|value| u32::try_from(value.integer()?).ok())
        .collect::<Option<Vec<_>>>()
        .map(|indices| (indices, bytes)))
}

fn container_item_ids<'a>(
    items: &[Value],
    kind: &str,
    id: u64,
    ctx: &'a DecodeContext<'_>,
) -> Result<(Vec<u64>, ScopedReservation<'a>), CodecError> {
    let bytes = temporary_collection::<u64>(ctx, items.len(), "step_tessellation_container_items")?;
    let ids = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            item.reference().ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{kind} #{id} item {} is not a reference",
                    index + 1
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((ids, bytes))
}

fn has_entity(record: &RawRecord, name: &str) -> bool {
    entity_kind(record, &[name]).is_some()
}

fn is_tessellated_shape_representation(record: &RawRecord) -> bool {
    entity_kind(
        record,
        &[
            "TESSELLATED_SHAPE_REPRESENTATION",
            "TESSELLATED_SHAPE_REPRESENTATION_WITH_ACCURACY_PARAMETERS",
        ],
    )
    .is_some()
}

fn entity_kind<'a>(record: &'a RawRecord, names: &[&str]) -> Option<&'a str> {
    record
        .partials
        .iter()
        .find(|partial| names.iter().any(|name| *name == partial.name))
        .map(|partial| partial.name.as_str())
}

fn entity_parameter<'a>(
    record: &'a RawRecord,
    entity: &str,
    index: usize,
    simple_offset: usize,
) -> Option<&'a Value> {
    let partial = record
        .partials
        .iter()
        .find(|partial| partial.name == entity)?;
    let offset = if record.partials.len() == 1 {
        simple_offset
    } else {
        0
    };
    partial.parameters.get(index + offset)
}

/// One triangulated tessellation entity, parsed from the record's partial names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TriangulatedEntity {
    Face,
    ComplexFace,
    SurfaceSet,
    ComplexSurfaceSet,
}

impl TriangulatedEntity {
    fn of(record: &RawRecord) -> Option<Self> {
        record.partials.iter().find_map(|partial| {
            Some(match partial.name.as_str() {
                "TRIANGULATED_FACE" => Self::Face,
                "COMPLEX_TRIANGULATED_FACE" => Self::ComplexFace,
                "TRIANGULATED_SURFACE_SET" => Self::SurfaceSet,
                "COMPLEX_TRIANGULATED_SURFACE_SET" => Self::ComplexSurfaceSet,
                _ => return None,
            })
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Face => "TRIANGULATED_FACE",
            Self::ComplexFace => "COMPLEX_TRIANGULATED_FACE",
            Self::SurfaceSet => "TRIANGULATED_SURFACE_SET",
            Self::ComplexSurfaceSet => "COMPLEX_TRIANGULATED_SURFACE_SET",
        }
    }

    fn base_name(self) -> &'static str {
        match self {
            Self::Face | Self::ComplexFace => "TESSELLATED_FACE",
            Self::SurfaceSet | Self::ComplexSurfaceSet => "TESSELLATED_SURFACE_SET",
        }
    }

    fn own_parameter_offset(self) -> usize {
        match self {
            Self::Face | Self::ComplexFace => 5,
            Self::SurfaceSet | Self::ComplexSurfaceSet => 4,
        }
    }
}

fn inherited_parameter<'a>(record: &'a RawRecord, entity: &str, index: usize) -> Option<&'a Value> {
    if record.partials.len() == 1 {
        record.parameter(index + 1)
    } else {
        entity_parameter(record, entity, index, 0)
    }
}

fn bytes_for<T>(
    count: usize,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<u64, CodecError> {
    u64_from_index(count)
        .checked_mul(u64_from_index(std::mem::size_of::<T>()))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))
}

fn decimal_digits(id: u64) -> u64 {
    if id == 0 {
        1
    } else {
        u64::from(id.ilog10()) + 1
    }
}

fn admitted_mesh_id(id: u64, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
    let digits = decimal_digits(id);
    let _key_bytes = ctx.reserve_scoped(digits, "step_tessellation_mesh_key")?;
    ctx.charge_retained(
        u64_from_index("step:tessellation:mesh#".len())
            .checked_add(digits)
            .ok_or_else(|| {
                ctx.refuse_codec_limit("step_tessellation_mesh_id", u64::MAX - 1, u64::MAX)
            })?,
        "step_tessellation_mesh_id",
    )?;
    Ok(ids::tessellation(kind!("mesh"), id).into_string())
}

fn admitted_surface_id<'a>(
    id: u64,
    ctx: &'a DecodeContext<'_>,
) -> Result<(cadmpeg_ir::ids::Identity, ScopedReservation<'a>), CodecError> {
    let digits = decimal_digits(id);
    let bytes = u64_from_index("step:data:surface#".len())
        .checked_add(digits)
        .and_then(|length| length.checked_add(digits))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("step_tessellation_surface_id", u64::MAX - 1, u64::MAX)
        })?;
    let reservation = ctx.reserve_scoped(bytes, "step_tessellation_surface_id")?;
    Ok((ids::data(kind!("surface"), id), reservation))
}

fn admitted_mesh_body(
    body: Option<&BodyId>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<BodyId>, CodecError> {
    body.map(|body| {
        ctx.charge_retained(
            u64_from_index(body.as_str().len()),
            "step_tessellation_mesh_body",
        )?;
        Ok(body.clone())
    })
    .transpose()
}

fn admitted_source_association(
    id: u64,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::SourceObjectAssociation, CodecError> {
    ctx.charge_retained(decimal_digits(id) + 1, "step_tessellation_source_object_id")?;
    Ok(super::step_source_association(id, None))
}

fn push_loss(
    losses: &mut Vec<LossNote>,
    code: StepLossCode,
    message: std::fmt::Arguments<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    struct CountBytes(Option<u64>);

    impl std::fmt::Write for CountBytes {
        fn write_str(&mut self, value: &str) -> std::fmt::Result {
            self.0 = self
                .0
                .and_then(|count| count.checked_add(u64_from_index(value.len())));
            if self.0.is_some() {
                Ok(())
            } else {
                Err(std::fmt::Error)
            }
        }
    }

    let mut count = CountBytes(Some(0));
    std::fmt::write(&mut count, message).map_err(|_| {
        ctx.refuse_codec_limit("step_tessellation_loss_message", u64::MAX - 1, u64::MAX)
    })?;
    let message_bytes = count.0.ok_or_else(|| {
        ctx.refuse_codec_limit("step_tessellation_loss_message", u64::MAX - 1, u64::MAX)
    })?;
    ctx.charge_collection_items(1, "step_tessellation_loss_notes")?;
    let retained_bytes = u64_from_index(std::mem::size_of::<LossNote>())
        .checked_add(message_bytes)
        .and_then(|bytes| bytes.checked_add(u64_from_index(code.code().len())))
        .and_then(|bytes| bytes.checked_add(u64_from_index("step".len())))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("step_tessellation_loss_notes", u64::MAX - 1, u64::MAX)
        })?;
    ctx.charge_retained(retained_bytes, "step_tessellation_loss_notes")?;
    losses.push(code.note(message.to_string()));
    Ok(())
}

fn temporary_collection<'a, T>(
    ctx: &'a DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<ScopedReservation<'a>, CodecError> {
    ctx.charge_collection_items(u64_from_index(count), operation)?;
    ctx.reserve_scoped(bytes_for::<T>(count, ctx, operation)?, operation)
}

fn coordinate_rows<'a>(
    record: &RawRecord,
    scale: f64,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<(Vec<Point3>, ScopedReservation<'a>)>, CodecError> {
    for rows in record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .filter_map(ValueExt::list)
    {
        let bytes =
            temporary_collection::<Point3>(ctx, rows.len(), "step_tessellation_coordinate_rows")?;
        let vertices = rows
            .iter()
            .map(|row| {
                let values = row.list()?;
                if values.len() != 3 {
                    return None;
                }
                let point = Point3::new(
                    values[0].number()? * scale,
                    values[1].number()? * scale,
                    values[2].number()? * scale,
                );
                point.is_finite().then_some(point)
            })
            .collect::<Option<Vec<_>>>()
            .filter(|vertices| !vertices.is_empty());
        if let Some(vertices) = vertices {
            return Ok(Some((vertices, bytes)));
        }
    }
    Ok(None)
}

fn triangle_rows<'a>(
    value: &Value,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<AdmittedTriangles<'a>>, CodecError> {
    let Some(rows) = value.list() else {
        return Ok(None);
    };
    let bytes =
        temporary_collection::<[u32; 3]>(ctx, rows.len(), "step_tessellation_triangle_rows")?;
    Ok(rows
        .iter()
        .map(|row| {
            let values = row.list()?;
            if values.len() != 3 {
                return None;
            }
            Some([
                u32::try_from(values[0].integer()?).ok()?,
                u32::try_from(values[1].integer()?).ok()?,
                u32::try_from(values[2].integer()?).ok()?,
            ])
        })
        .collect::<Option<Vec<_>>>()
        .map(|triangles| AdmittedTriangles { triangles, bytes }))
}

fn complex_triangles<'a>(
    strips: Option<&Value>,
    fans: Option<&Value>,
    kind: &str,
    id: u64,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<AdmittedTriangles<'a>>, CodecError> {
    let (strips, _strip_rows_bytes, _strip_indices_bytes) =
        index_rows(strips, kind, id, "strip", ctx)?;
    let (fans, _fan_rows_bytes, _fan_indices_bytes) = index_rows(fans, kind, id, "fan", ctx)?;
    let triangle_count = strips
        .iter()
        .chain(fans.iter())
        .try_fold(0usize, |count, row| count.checked_add(row.len() - 2))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("step_complex_triangle_count", u64::MAX - 1, u64::MAX)
        })?;
    let bytes = temporary_collection::<[u32; 3]>(
        ctx,
        triangle_count,
        "step_complex_tessellation_triangles",
    )?;
    let mut triangles = Vec::new();
    for strip in strips {
        for index in 0..strip.len() - 2 {
            triangles.push(if index % 2 == 0 {
                [strip[index], strip[index + 1], strip[index + 2]]
            } else {
                [strip[index + 1], strip[index], strip[index + 2]]
            });
        }
    }
    for fan in fans {
        for index in 1..fan.len() - 1 {
            triangles.push([fan[0], fan[index], fan[index + 1]]);
        }
    }
    Ok((!triangles.is_empty()).then_some(AdmittedTriangles { triangles, bytes }))
}

fn index_rows<'a>(
    value: Option<&Value>,
    kind: &str,
    id: u64,
    lane: &str,
    ctx: &'a DecodeContext<'_>,
) -> Result<(Vec<Vec<u32>>, ScopedReservation<'a>, ScopedReservation<'a>), CodecError> {
    let Some(rows) = value.and_then(ValueExt::list) else {
        return Ok((
            Vec::new(),
            ctx.reserve_scoped(0, "step_complex_tessellation_rows")?,
            ctx.reserve_scoped(0, "step_complex_tessellation_indices")?,
        ));
    };
    let row_bytes =
        temporary_collection::<Vec<u32>>(ctx, rows.len(), "step_complex_tessellation_rows")?;
    let mut index_bytes = ctx.reserve_scoped(0, "step_complex_tessellation_indices")?;
    let indices = rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            let invalid = || {
                CodecError::malformed(format_args!(
                    "{kind} #{id} {lane} row {} is invalid",
                    row_index + 1
                ))
            };
            let Some(values) = row.list().filter(|values| values.len() >= 3) else {
                return Err(invalid());
            };
            ctx.charge_collection_items(
                u64_from_index(values.len()),
                "step_complex_tessellation_indices",
            )?;
            index_bytes.grow(bytes_for::<u32>(
                values.len(),
                ctx,
                "step_complex_tessellation_indices",
            )?)?;
            values
                .iter()
                .map(|value| {
                    u32::try_from(value.integer().ok_or_else(invalid)?).map_err(|_| invalid())
                })
                .collect()
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((indices, row_bytes, index_bytes))
}

fn normal_rows<'a>(
    value: Option<&Value>,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<(Vec<Vector3>, ScopedReservation<'a>)>, CodecError> {
    let Some(rows) = value.and_then(ValueExt::list) else {
        return Ok(None);
    };
    let bytes = temporary_collection::<Vector3>(ctx, rows.len(), "step_tessellation_normal_rows")?;
    Ok(rows
        .iter()
        .map(|row| {
            let values = row.list()?;
            if values.len() != 3 {
                return None;
            }
            let normal = Vector3::new(
                values[0].number()?,
                values[1].number()?,
                values[2].number()?,
            );
            super::geometry::normalize(normal)
        })
        .collect::<Option<Vec<_>>>()
        .map(|normals| (normals, bytes)))
}
#[cfg(test)]
pub(crate) mod tests;
