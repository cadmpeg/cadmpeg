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
use cadmpeg_ir::tessellation::{ShadedVertex, Tessellation, TessellationMesh};
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
    let mut surface_index_storage = ctx.reserve_scoped(0, "step tessellation surface index")?;
    let mut surface_index = BTreeMap::new();
    let mut source_items = ir.model.surfaces.iter().enumerate();
    for _ in 0..source_items.len() {
        let Some((position, surface)) =
            ctx.next_charged(&mut source_items, "STEP tessellation surface indexing")?
        else {
            break;
        };
        if !ctx.contains_key_btree_map(
            &surface_index,
            surface.id.as_str(),
            "step tessellation surface index",
        )? {
            surface_index_storage.with_storage(|| {
                let key =
                    ctx.copy_retained_text(surface.id.as_str(), "step tessellation surface index")?;
                ctx.insert_btree_map(
                    &mut surface_index,
                    key,
                    position,
                    "step tessellation surface index",
                )
            })?;
        }
    }
    let mut admitted_meshes = u64_from_index(ir.model.tessellations.len());
    let mut coordinate_map_bytes = ctx.reserve_scoped(0, "step_tessellation_coordinate_lists")?;
    let mut coordinates = BTreeMap::new();
    let mut source_items = exchange.records().iter();
    for _ in 0..source_items.len() {
        let Some((&id, record)) = ctx.next_charged(&mut source_items, "STEP decode traversal")?
        else {
            break;
        };
        if !has_entity(ctx, record, "COORDINATES_LIST")? {
            continue;
        }
        let scale = geometry.units.length([id], ctx)?.get();
        if let Some(vertices) = coordinate_rows(record, scale, ctx)? {
            coordinate_map_bytes.with_storage(|| {
                ctx.insert_btree_map(
                    &mut coordinates,
                    id,
                    vertices,
                    "step_tessellation_coordinate_lists",
                )
            })?;
        }
    }
    let mut typed = BTreeSet::new();
    let mut losses = Vec::new();
    let mut reservations = AssociationReservations::new(ctx)?;
    let mut item_bodies = BTreeMap::<u64, BTreeSet<BodyId>>::new();
    let mut item_placements = BTreeMap::<u64, Vec<Transform>>::new();
    let mut unresolved_placements = BTreeSet::new();
    let mut declared_items = BTreeSet::new();
    let mut unresolved_containers = BTreeSet::new();
    let mut body_context_items = BTreeSet::new();
    let mut source_items = exchange.records().iter();
    for _ in 0..source_items.len() {
        let Some((&id, record)) = ctx.next_charged(&mut source_items, "STEP decode traversal")?
        else {
            break;
        };
        let Some(kind) = entity_kind(ctx, record, &["TESSELLATED_SOLID", "TESSELLATED_SHELL"])?
        else {
            continue;
        };
        let Some(items) = entity_parameter(ctx, record, kind, 0, 1)?.and_then(ValueExt::list)
        else {
            push_loss(
                &mut losses,
                StepLossCode::DecodeWarning,
                format_args!("{kind} #{id} has no structured items"),
                ctx,
            )?;
            continue;
        };
        let (item_ids_buffer, _container_item_bytes) = container_item_ids(items, kind, id, ctx)?;
        let item_ids = item_ids_buffer;
        ctx.fold(
            &item_ids,
            (),
            |(), &item| {
                ctx.insert_scoped_btree_value(
                    &mut reservations.declared,
                    &mut declared_items,
                    item,
                    "step_tessellation_declared_items",
                )?;
                Ok(())
            },
            "STEP decode traversal",
        )?;
        let (candidates_buffer, _linked_body_bytes) = linked_bodies(record, kind, topology, ctx)?;
        let candidates = candidates_buffer;
        if candidates.is_empty() {
            ctx.insert_scoped_btree_value(
                &mut reservations.unresolved_containers,
                &mut unresolved_containers,
                id,
                "step_tessellation_unresolved_containers",
            )?;
        }
        let mut body_candidate_bytes =
            { ctx.reserve_scoped(0, "step_tessellation_body_candidates") }?;
        let body_candidates = body_candidate_bytes.with_storage(|| {
            ctx.collect_vec(
                candidates.iter().copied(),
                "step_tessellation_body_candidates",
            )
        })?;
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
            active: HashSet::new(),
            active_storage: ctx.reserve_scoped(0, "step_tessellation_active_items")?,
            reservations: &mut reservations,
            ctx,
        };
        ctx.fold(
            &item_ids,
            (),
            |(), &item| {
                associator.visit(item, None)?;
                Ok(())
            },
            "STEP tessellation item traversal",
        )?;
        drop(item_ids);
        ctx.insert_btree_set(&mut typed, id, "step_tessellation_claims")?;
    }
    let mut representation_storage =
        ctx.reserve_scoped(0, "step tessellation representation storage")?;
    let mut representation_cache = BTreeMap::new();
    let (product_representations_buffer, product_representation_bytes) =
        product_linked_representations(exchange, ctx)?;
    let product_representations = product_representations_buffer;
    let (product_representation_items_buffer, product_item_bytes) =
        product_representation_items(exchange, &product_representations, ctx)?;
    let product_representation_items = product_representation_items_buffer;
    let mut source_items = exchange.records().iter();
    for _ in 0..source_items.len() {
        let Some((&id, record)) = ctx.next_charged(&mut source_items, "STEP decode traversal")?
        else {
            break;
        };
        if !is_tessellated_shape_representation(ctx, record)? {
            continue;
        }
        let Some((item_buffer, _representation_item_bytes)) =
            admitted_representation_items(record, ctx)?
        else {
            continue;
        };
        let items = item_buffer;
        let bodies = representation_storage.with_storage(|| {
            super::topology::representation_bodies(
                id,
                exchange,
                topology,
                &mut representation_cache,
                &mut BTreeSet::new(),
                ctx,
            )
        })?;
        let product_linked =
            ctx.contains_btree_set(&product_representations, &id, "step_tessellation_lookup")?
                || ctx.any_by(
                    &items,
                    |item| {
                        ctx.contains_btree_set(
                            &product_representation_items,
                            item,
                            "step_tessellation_lookup",
                        )
                    },
                    "STEP product item traversal",
                )?;
        if bodies.is_empty() && !product_linked {
            continue;
        }
        let (body_refs_buffer, _body_ref_storage) = ctx
            .with_scoped_storage("step tessellation body references", || {
                ctx.collect_vec(bodies.iter(), "step tessellation body references")
            })?;
        let body_refs = body_refs_buffer;
        let mut associator = TessellationItemAssociator {
            bodies: &body_refs,
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
            active: HashSet::new(),
            active_storage: ctx.reserve_scoped(0, "step_tessellation_active_items")?,
            reservations: &mut reservations,
            ctx,
        };
        ctx.fold(
            &items,
            (),
            |(), &item| {
                associator.visit(item, None)?;
                Ok(())
            },
            "STEP tessellation item traversal",
        )?;
        drop(items);
    }
    drop(product_representation_items);
    drop(product_item_bytes);
    drop(product_representations);
    drop(product_representation_bytes);
    drop(representation_cache);
    drop(representation_storage);
    let mut source_items = exchange.records().iter();
    for _ in 0..source_items.len() {
        let Some((&id, record)) = ctx.next_charged(&mut source_items, "STEP decode traversal")?
        else {
            break;
        };
        if !has_entity(ctx, record, "TESSELLATED_ANNOTATION_OCCURRENCE")? {
            continue;
        }
        let Some(item) = tessellated_annotation_item(ctx, record)? else {
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
            active: HashSet::new(),
            active_storage: ctx.reserve_scoped(0, "step_tessellation_active_items")?,
            reservations: &mut reservations,
            ctx,
        };
        associator.visit(item, None)?;
    }
    let mut source_items = unresolved_placements.into_iter();
    for _ in 0..source_items.len() {
        let Some(id) =
            ctx.next_charged(&mut source_items, "STEP unresolved tessellation traversal")?
        else {
            break;
        };
        push_loss(
            &mut losses,
            StepLossCode::TessellationPlacementUnresolved,
            format_args!("repositioned tessellated item #{id} has no valid AXIS2_PLACEMENT_3D; unresolved placement is not applied"),
            ctx,
        )?;
    }
    drop(source_items);
    reservations.unresolved_placements =
        ctx.reserve_scoped(0, "step_tessellation_unresolved_placements")?;
    let mut source_items = unresolved_containers.into_iter();
    for _ in 0..source_items.len() {
        let Some(id) =
            ctx.next_charged(&mut source_items, "STEP unresolved tessellation traversal")?
        else {
            break;
        };
        let Some(record) =
            ctx.get_btree_map(exchange.records(), &id, "step_tessellation_lookup")?
        else {
            continue;
        };
        let Some(kind) = entity_kind(ctx, record, &["TESSELLATED_SOLID", "TESSELLATED_SHELL"])?
        else {
            continue;
        };
        push_loss(
            &mut losses,
            StepLossCode::DecodeWarning,
            format_args!("{kind} #{id} has no decoded exact body link"),
            ctx,
        )?;
    }
    drop(source_items);
    reservations.unresolved_containers =
        ctx.reserve_scoped(0, "step_tessellation_unresolved_containers")?;
    let mut source_items = item_placements.iter();
    for _ in 0..source_items.len() {
        let Some((&item, placements)) =
            ctx.next_charged(&mut source_items, "STEP tessellation placement traversal")?
        else {
            break;
        };
        if !ctx
            .get_btree_map(&item_bodies, &item, "step_tessellation_lookup")?
            .is_some_and(BTreeSet::is_empty)
        {
            continue;
        }
        if distinct_placement(ctx, placements)?.is_none() {
            let distinct_count = distinct_placement_count(placements, ctx)?;
            push_loss(
                &mut losses,
                StepLossCode::TessellationPlacementAmbiguous,
                format_args!("tessellation item #{item} has {distinct_count} distinct repositioning placements; mesh retained in source coordinates"),
                ctx,
            )?;
        }
    }
    let mut source_items = item_bodies.iter();
    for _ in 0..source_items.len() {
        let Some((&item, bodies)) =
            ctx.next_charged(&mut source_items, "STEP tessellation body traversal")?
        else {
            break;
        };
        if ctx.contains_btree_set(&body_context_items, &item, "step_tessellation_lookup")?
            && bodies.len() != 1
        {
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
    let mut source_items = exchange.records().iter();
    for _ in 0..source_items.len() {
        let Some((&id, record)) = ctx.next_charged(&mut source_items, "STEP decode traversal")?
        else {
            break;
        };
        let Some(entity) = TriangulatedEntity::of(ctx, record)? else {
            continue;
        };
        let kind = entity.name();
        let base_kind = entity.base_name();
        let Some(coordinate_id) =
            inherited_parameter(ctx, record, base_kind, 0)?.and_then(ValueExt::reference)
        else {
            push_loss(
                &mut losses,
                StepLossCode::DecodeWarning,
                format_args!("{kind} #{id} has no COORDINATES_LIST reference"),
                ctx,
            )?;
            continue;
        };
        let Some((vertices, _coordinate_bytes)) =
            ctx.get_btree_map(&coordinates, &coordinate_id, "step_tessellation_lookup")?
        else {
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
                entity_parameter(ctx, record, kind, 1, offset)?
                    .map(|value| triangle_rows(value, ctx))
                    .transpose()?
                    .flatten()
            }
            TriangulatedEntity::ComplexFace | TriangulatedEntity::ComplexSurfaceSet => {
                complex_triangles(
                    entity_parameter(ctx, record, kind, 1, offset)?,
                    entity_parameter(ctx, record, kind, 2, offset)?,
                    kind,
                    id,
                    ctx,
                )?
            }
        };
        let Some(AdmittedTriangles {
            triangles: triangle_buffer,
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
        let triangles = triangle_buffer;
        let (pnindex_buffer, pnindex_bytes) = match entity_parameter(ctx, record, kind, 0, offset)?
        {
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
        let pnindex = pnindex_buffer;
        let (
            vertex_buffer,
            triangle_buffer,
            addressing_buffer,
            mut local_vertex_bytes,
            local_triangle_bytes,
            coordinate_index_bytes,
        ) = if pnindex.is_empty() {
            if ctx.any_by(
                triangles.as_slice(),
                |triangle| {
                    Ok(triangle.iter().any(|index| {
                        *index == 0 || cadmpeg_core::decode::index_from_u32(*index) > vertices.len()
                    }))
                },
                "STEP triangle index traversal",
            )? {
                push_loss(
                    &mut losses,
                    StepLossCode::DecodeWarning,
                    format_args!("{kind} #{id} has an out-of-range one-based coordinate index"),
                    ctx,
                )?;
                continue;
            }
            let mut coordinate_index_bytes =
                ctx.reserve_scoped(0, "step_tessellation_coordinate_indices")?;
            let mut coordinate_indices = BTreeSet::new();
            let mut source_items = triangles.iter();
            for _ in 0..source_items.len() {
                let Some(triangle) =
                    ctx.next_charged(&mut source_items, "STEP triangle index traversal")?
                else {
                    break;
                };
                for &index in triangle {
                    if !ctx.contains_btree_set(
                        &coordinate_indices,
                        &index,
                        "step_tessellation_lookup",
                    )? {
                        coordinate_index_bytes.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut coordinate_indices,
                                index,
                                "step_tessellation_coordinate_indices",
                            )
                        })?;
                    }
                }
            }
            let mut local_index_bytes = ctx.reserve_scoped(0, "step_tessellation_local_index")?;
            let mut local_index = BTreeMap::new();
            let mut source_items = coordinate_indices.iter().enumerate();
            for _ in 0..source_items.len() {
                let Some((local, global)) =
                    ctx.next_charged(&mut source_items, "STEP tessellation local index traversal")?
                else {
                    break;
                };
                // Distinct one-based u32 indices contain at most u32::MAX entries.
                let local = u32::try_from(local).map_err(|_| {
                    CodecError::malformed("invalid STEP tessellation local index width")
                })?;
                local_index_bytes.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut local_index,
                        *global,
                        local,
                        "step_tessellation_local_index",
                    )
                })?;
            }
            let mut local_vertex_bytes =
                { ctx.reserve_scoped(0, "step_tessellation_local_vertices") }?;
            let local_vertices = local_vertex_bytes.with_storage(|| {
                ctx.collect_vec(
                    coordinate_indices
                        .iter()
                        .map(|index| vertices[cadmpeg_core::decode::index_from_u32(*index) - 1]),
                    "step_tessellation_local_vertices",
                )
            })?;
            let mut local_triangle_bytes =
                { ctx.reserve_scoped(0, "step_tessellation_local_triangles") }?;
            let local_triangles = local_triangle_bytes.with_storage(|| {
                ctx.try_collect_vec(
                    triangles.iter().map(|triangle| {
                        let mut local = [0; 3];
                        for (target, index) in local.iter_mut().zip(triangle) {
                            *target = *ctx
                                .get_btree_map(
                                    &local_index,
                                    index,
                                    "step_tessellation_local_index",
                                )?
                                .ok_or_else(|| {
                                    CodecError::malformed("unindexed tessellation corner")
                                })?;
                        }
                        Ok::<_, CodecError>(local)
                    }),
                    "step_tessellation_local_triangles",
                )
            })?;
            (
                local_vertices,
                local_triangles,
                CoordinateAddressing::TriangleIndices(coordinate_indices),
                local_vertex_bytes,
                local_triangle_bytes,
                Some(coordinate_index_bytes),
            )
        } else {
            if ctx.any_by(
                &pnindex[..],
                |index| {
                    Ok(
                        *index == 0
                            || cadmpeg_core::decode::index_from_u32(*index) > vertices.len(),
                    )
                },
                "STEP decode traversal",
            )? || ctx.any_by(
                triangles.as_slice(),
                |triangle| {
                    Ok(triangle.iter().any(|index| {
                        *index == 0 || cadmpeg_core::decode::index_from_u32(*index) > pnindex.len()
                    }))
                },
                "STEP triangle index traversal",
            )? {
                push_loss(
                    &mut losses,
                    StepLossCode::DecodeWarning,
                    format_args!("{kind} #{id} has an out-of-range one-based tessellation index"),
                    ctx,
                )?;
                continue;
            }
            let mut local_vertex_bytes =
                { ctx.reserve_scoped(0, "step_tessellation_pn_vertices") }?;
            let mut local_triangle_bytes =
                { ctx.reserve_scoped(0, "step_tessellation_pn_triangles") }?;
            (
                local_vertex_bytes.with_storage(|| {
                    ctx.collect_vec(
                        pnindex.iter().map(|index| {
                            vertices[cadmpeg_core::decode::index_from_u32(*index) - 1]
                        }),
                        "step_tessellation_pn_vertices",
                    )
                })?,
                local_triangle_bytes.with_storage(|| {
                    ctx.collect_vec(
                        triangles
                            .iter()
                            .map(|triangle| triangle.map(|index| index - 1)),
                        "step_tessellation_pn_triangles",
                    )
                })?,
                CoordinateAddressing::PnIndex,
                local_vertex_bytes,
                local_triangle_bytes,
                None,
            )
        };
        let (mut local_vertices, local_triangles, addressing) =
            (vertex_buffer, triangle_buffer, addressing_buffer);
        drop(triangles);
        drop(triangle_bytes);
        drop(pnindex);
        drop(pnindex_bytes);
        let (source_normals_buffer, source_normal_bytes) =
            match inherited_parameter(ctx, record, base_kind, 2)? {
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
        let source_normals = source_normals_buffer;
        // AP242 permits an empty normals aggregate; the IR represents both
        // that spelling and an omitted lane as an absent normal lane.
        let (normal_buffer, mut normal_bytes) = match source_normals.len() {
            0 => {
                drop(source_normals);
                drop(source_normal_bytes);
                (None, None)
            }
            1 => {
                let (replicated, storage) =
                    replicated_normals(local_vertices.len(), source_normals[0], ctx)?;
                drop(source_normals);
                drop(source_normal_bytes);
                (Some(replicated), Some(storage))
            }
            count if count == local_vertices.len() => (Some(source_normals), source_normal_bytes),
            count => match &addressing {
                CoordinateAddressing::TriangleIndices(coordinate_indices)
                    if count == vertices.len() =>
                {
                    let (projected, storage) =
                        ctx.with_scoped_storage("step_tessellation_projected_normals", || {
                            ctx.collect_vec(
                                coordinate_indices.iter().map(|index| {
                                    source_normals[cadmpeg_core::decode::index_from_u32(*index) - 1]
                                }),
                                "step_tessellation_projected_normals",
                            )
                        })?;
                    drop(source_normals);
                    drop(source_normal_bytes);
                    (Some(projected), Some(storage))
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
                    drop(source_normals);
                    drop(source_normal_bytes);
                    (None, None)
                }
            },
        };
        let mut normals = normal_buffer;
        drop(addressing);
        drop(coordinate_index_bytes);
        let bodies = ctx.get_btree_map(&item_bodies, &id, "step_tessellation_lookup")?;
        if bodies.is_some_and(BTreeSet::is_empty) {
            if let Some(placement) = distinct_placement(
                ctx,
                ctx.get_btree_map(&item_placements, &id, "step_tessellation_lookup")?
                    .map_or(&[], Vec::as_slice),
            )? {
                let (placed, storage) =
                    ctx.with_scoped_storage("step_tessellation_placed_vertices", || {
                        ctx.collect_options(
                            local_vertices
                                .into_iter()
                                .map(|vertex| placement.apply_point(vertex.get())),
                            "step_tessellation_placed_vertices",
                        )
                    })?;
                local_vertices = match placed {
                    Some(placed) => placed,
                    None => return Err(CodecError::malformed(ctx.format_retained(
                        format_args!("{kind} #{id} placed tessellation vertex contains a non-finite coordinate"), "STEP decode text",
                    )?)),
                };
                local_vertex_bytes = storage;
                if let Some(source_normals) = normals.take() {
                    let (placed, storage) =
                        ctx.with_scoped_storage("step_tessellation_placed_normals", || {
                            ctx.collect_options(
                                source_normals.into_iter().map(|normal| {
                                    placement
                                        .apply_normal(normal.get())
                                        .map(FiniteVector3::from)
                                }),
                                "step_tessellation_placed_normals",
                            )
                        })?;
                    match placed {
                        Some(transformed) => {
                            normals = Some(transformed);
                            normal_bytes = Some(storage);
                        }
                        None => {
                            normal_bytes = None;
                            drop(storage);
                            push_loss(
                                &mut losses, StepLossCode::DecodeWarning,
                                format_args!("{kind} #{id} normal placement could not produce finite unit normals; normals omitted"), ctx,
                            )?;
                        }
                    }
                }
            }
        }
        if let Some(surface_step) = complex_triangulated_face_surface(ctx, record)? {
            let (surface_id_buffer, _surface_id_bytes) = admitted_surface_id(surface_step, ctx)?;
            let surface_id = surface_id_buffer;
            if let Some(&position) = ctx.get_btree_map(
                &surface_index,
                surface_id.as_str(),
                "step tessellation surface lookup",
            )? {
                let surface = &mut ir.model.surfaces[position];
                if surface.source_object.is_none() {
                    surface.source_object = Some(super::step_source_association(ctx, id, None)?);
                }
            }
        }
        let triangle_count = local_triangles.len();
        let triangles = ctx.copy_slice(&local_triangles, "step_tessellation_ir_triangles")?;
        let rows = if let Some(normals) = normals {
            TessellationMesh::ShadedList {
                vertices: ctx.collect_vec(
                    local_vertices
                        .iter()
                        .zip(&normals)
                        .map(|(&position, &normal)| ShadedVertex { position, normal }),
                    "step_tessellation_shaded_rows",
                )?,
                triangles,
            }
        } else {
            TessellationMesh::List {
                vertices: ctx.copy_slice(&local_vertices, "step_tessellation_ir_mesh")?,
                triangles,
            }
        };
        drop(local_vertices);
        drop(local_triangles);
        drop(local_triangle_bytes);
        drop(local_vertex_bytes);
        drop(normal_bytes);
        let validation_triangle_bytes = ctx.reserve_scoped_collection::<[u32; 3]>(
            triangle_count,
            "step_tessellation_validation_triangles",
        )?;
        let next_mesh_count = admitted_meshes + 1;
        let mut pending_meshes = admitted_meshes;
        ctx.admit_entities(
            next_mesh_count,
            &mut pending_meshes,
            "step_tessellation_mesh_entity",
        )?;
        let mesh = Tessellation::from_parts(admitted_mesh_id(id, ctx)?, rows, Vec::new());
        drop(validation_triangle_bytes);
        let mesh = match mesh {
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
        let declared = ctx.contains_btree_set(&declared_items, &id, "step_tessellation_lookup")?;
        if !declared {
            push_loss(
                &mut losses,
                StepLossCode::TessellationItemUndeclared,
                format_args!("tessellation item #{id} is not declared by an exact body container; mesh retained as detached"),
                ctx,
            )?;
        }

        ctx.reserve_vec(
            &mut ir.model.tessellations,
            1,
            "step_tessellation_mesh_list",
        )?;
        let body = bodies
            .filter(|bodies| bodies.len() == 1)
            .and_then(|bodies| bodies.first());
        let body = body
            .map(|body| body.try_clone_for_decode(ctx, "step_tessellation_mesh_body"))
            .transpose()?;
        let source_object = if !declared || bodies.is_none_or(|bodies| bodies.len() != 1) {
            Some(super::step_source_association(ctx, id, None)?)
        } else {
            None
        };

        ir.model
            .tessellations
            .push(mesh.with_body(body).with_source_object(source_object));
        ctx.insert_btree_set(&mut typed, id, "step_tessellation_claims")?;
        ctx.insert_btree_set(&mut typed, coordinate_id, "step_tessellation_claims")?;
    }
    if !ir.model.tessellations.is_empty() {
        let mut source_items = exchange.records().iter();
        for _ in 0..source_items.len() {
            let Some((&id, record)) =
                ctx.next_charged(&mut source_items, "STEP decode traversal")?
            else {
                break;
            };
            if has_entity(ctx, record, "TESSELLATED_SHAPE_REPRESENTATION")?
                || has_entity(ctx, record, "TESSELLATED_SOLID")?
                || has_entity(ctx, record, "TESSELLATED_SHELL")?
            {
                ctx.insert_btree_set(&mut typed, id, "step_tessellation_claims")?;
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

fn complex_triangulated_face_surface(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<u64>, CodecError> {
    if !has_entity(ctx, record, "COMPLEX_TRIANGULATED_FACE")? {
        return Ok(None);
    }
    Ok(inherited_parameter(ctx, record, "TESSELLATED_FACE", 3)?.and_then(ValueExt::reference))
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
    bodies: &'a [&'a BodyId],
    exchange: &'a Exchange,
    item_bodies: &'a mut BTreeMap<u64, BTreeSet<BodyId>>,
    declared_items: &'a mut BTreeSet<u64>,
    unresolved_containers: &'a mut BTreeSet<u64>,
    typed: &'a mut BTreeSet<u64>,
    geometry: &'a GeometryData<'a>,
    placements: &'a mut BTreeMap<u64, Vec<Transform>>,
    unresolved_placements: &'a mut BTreeSet<u64>,
    body_context_items: &'a mut BTreeSet<u64>,
    mode: AssociationMode,
    active: HashSet<u64>,
    reservations: &'a mut AssociationReservations<'ctx>,
    ctx: &'ctx DecodeContext<'arena>,
    active_storage: ScopedReservation<'ctx>,
}

impl TessellationItemAssociator<'_, '_, '_> {
    fn visit(&mut self, id: u64, inherited_placement: Option<Transform>) -> Result<(), CodecError> {
        let _depth_guard = self.ctx.enter_nested("step_tessellation_association")?;
        if self
            .ctx
            .contains_hash_set(&self.active, &id, "step_tessellation_lookup")?
        {
            return Ok(());
        }
        self.active_storage.with_storage(|| {
            self.ctx
                .insert_hash_set(&mut self.active, id, "step_tessellation_active_items")
        })?;
        let Some(record) =
            self.ctx
                .get_btree_map(self.exchange.records(), &id, "step_tessellation_lookup")?
        else {
            self.ctx
                .remove_hash_set(&mut self.active, &id, "step_tessellation_lookup")?;
            return Ok(());
        };
        let local_placement = if has_entity(self.ctx, record, "REPOSITIONED_TESSELLATED_ITEM")? {
            let placement = repositioned_placement(self.ctx, record, self.geometry)?;
            if placement.is_none() {
                self.ctx.insert_scoped_btree_value(
                    &mut self.reservations.unresolved_placements,
                    self.unresolved_placements,
                    id,
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
            self.ctx,
            record,
            &[
                "TRIANGULATED_FACE",
                "COMPLEX_TRIANGULATED_FACE",
                "TRIANGULATED_SURFACE_SET",
                "COMPLEX_TRIANGULATED_SURFACE_SET",
            ],
        )?
        .is_some()
        {
            if self.mode != AssociationMode::DetachedAnnotation {
                self.ctx.insert_scoped_btree_value(
                    &mut self.reservations.body_context,
                    self.body_context_items,
                    id,
                    "step_tessellation_body_context_items",
                )?;
            }
            self.ctx.insert_scoped_btree_value(
                &mut self.reservations.declared,
                self.declared_items,
                id,
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
                self.reservations.placements.with_storage(|| {
                    self.ctx.push_btree_group(
                        self.placements,
                        id,
                        placement,
                        "step_tessellation_placement_entries",
                        "step_tessellation_placements",
                    )
                })?;
            }
        } else if let Some(kind) = entity_kind(
            self.ctx,
            record,
            &[
                "TESSELLATED_SOLID",
                "TESSELLATED_SHELL",
                "TESSELLATED_GEOMETRIC_SET",
            ],
        )? {
            if self.mode == AssociationMode::BodyItems {
                self.ctx.insert_scoped_btree_value(
                    &mut self.reservations.declared,
                    self.declared_items,
                    id,
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
                self.ctx
                    .insert_btree_set(self.typed, id, "step_tessellation_claims")?;
            }
            if !self.bodies.is_empty() && matches!(kind, "TESSELLATED_SOLID" | "TESSELLATED_SHELL")
            {
                self.ctx.remove_btree_set(
                    self.unresolved_containers,
                    &id,
                    "step_tessellation_lookup",
                )?;
            }
            let (item_ids_buffer, _container_item_bytes) =
                match entity_parameter(self.ctx, record, kind, 0, 1)?.and_then(ValueExt::list) {
                    Some(items) => container_item_ids(items, kind, id, self.ctx)?,
                    None => (
                        Vec::new(),
                        self.ctx
                            .reserve_scoped(0, "step_tessellation_container_items")?,
                    ),
                };
            let item_ids = item_ids_buffer;
            let mut source_items = item_ids.into_iter();
            for _ in 0..source_items.len() {
                let Some(item) = self
                    .ctx
                    .next_charged(&mut source_items, "STEP tessellation item traversal")?
                else {
                    break;
                };
                self.visit(item, placement)?;
            }
            drop(source_items);
        } else if self.mode == AssociationMode::BodyItems {
            self.ctx.insert_scoped_btree_value(
                &mut self.reservations.declared,
                self.declared_items,
                id,
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
        self.ctx
            .remove_hash_set(&mut self.active, &id, "step_tessellation_lookup")?;
        Ok(())
    }
}

fn distinct_placement_count(
    placements: &[Transform],
    ctx: &DecodeContext<'_>,
) -> Result<usize, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "step_tessellation_distinct_placements")?;
    let mut distinct = BTreeSet::new();
    ctx.fold(
        placements,
        (),
        |(), placement| {
            let key = placement
                .rows()
                .map(|row| row.map(|value| if value == 0.0 { 0 } else { value.to_bits() }));
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut distinct, key, "step_tessellation_distinct_placements")
            })?;
            Ok(())
        },
        "STEP distinct placement count traversal",
    )?;
    Ok(distinct.len())
}

fn distinct_placement(
    ctx: &DecodeContext<'_>,
    placements: &[Transform],
) -> Result<Option<Transform>, CodecError> {
    let Some((&first, rest)) = placements.split_first() else {
        return Ok(None);
    };
    if rest.is_empty() {
        return Ok(Some(first));
    }
    Ok(ctx
        .all_by(
            rest,
            |placement| Ok(*placement == first),
            "STEP distinct placement traversal",
        )?
        .then_some(first))
}

fn repositioned_placement(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    geometry: &GeometryData,
) -> Result<Option<Transform>, CodecError> {
    let Some(placement_id) = entity_parameter(ctx, record, "REPOSITIONED_TESSELLATED_ITEM", 0, 1)?
        .and_then(ValueExt::reference)
    else {
        return Ok(None);
    };
    Ok(ctx
        .get_btree_map(
            &geometry.placements,
            &placement_id,
            "step_tessellation_lookup",
        )?
        .copied()
        .and_then(super::geometry::placement_transform))
}

fn tessellated_annotation_item(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<u64>, CodecError> {
    for name in ["TESSELLATED_ANNOTATION_OCCURRENCE", "STYLED_ITEM"] {
        let Some(partial) = record.partial(ctx, name)? else {
            continue;
        };
        if let Some(item) = ctx.find_map(
            partial.parameters.as_slice().iter().rev(),
            |value| Ok(ValueExt::reference(value)),
            "STEP tessellated annotation reference traversal",
        )? {
            return Ok(Some(item));
        }
    }
    Ok(None)
}

fn associate_bodies(
    associations: &mut BTreeMap<u64, BTreeSet<BodyId>>,
    item: u64,
    bodies: &[&BodyId],
    ctx: &DecodeContext<'_>,
    bytes: &mut ScopedReservation<'_>,
) -> Result<(), CodecError> {
    let associated = bytes.with_storage(|| {
        Ok::<_, CodecError>(
            ctx.entry_btree_map(associations, item, "step_tessellation_item_body_entries")?
                .or_default(),
        )
    })?;
    ctx.fold(
        bodies,
        (),
        |(), &body| {
            if !ctx.contains_btree_set(associated, body, "STEP associated membership")? {
                let copy = bytes.with_storage(|| {
                    body.try_clone_for_decode(ctx, "step_tessellation_item_body_links")
                })?;
                bytes.with_storage(|| {
                    ctx.insert_btree_set(associated, copy, "step_tessellation_item_body_links")
                })?;
            }
            Ok(())
        },
        "STEP associate bodies traversal",
    )?;
    Ok(())
}

fn product_linked_representations<'a>(
    exchange: &Exchange,
    ctx: &'a DecodeContext<'_>,
) -> Result<(BTreeSet<u64>, ScopedReservation<'a>), CodecError> {
    let mut definition_bytes = ctx.reserve_scoped(0, "step_tessellation_product_definitions")?;
    let mut product_shape_definitions = BTreeSet::new();
    for indexed_entity in exchange.entities(ctx, "PRODUCT_DEFINITION_SHAPE")? {
        let (id, record) = indexed_entity?;
        if record.partial(ctx, "PRODUCT_DEFINITION_SHAPE")?.is_some() {
            ctx.insert_scoped_btree_value(
                &mut definition_bytes,
                &mut product_shape_definitions,
                id,
                "step_tessellation_product_definitions",
            )?;
        }
    }
    let mut linked_bytes = ctx.reserve_scoped(0, "step_tessellation_product_representations")?;
    let mut linked = BTreeSet::new();
    for indexed_entity in exchange.entities(ctx, "SHAPE_DEFINITION_REPRESENTATION")? {
        let (_, record) = indexed_entity?;
        let Some(partial) = record.partial(ctx, "SHAPE_DEFINITION_REPRESENTATION")? else {
            continue;
        };
        let Some(definition) = partial.parameters.first().and_then(ValueExt::reference) else {
            continue;
        };
        let representation = if ctx.contains_btree_set(
            &product_shape_definitions,
            &definition,
            "step_tessellation_lookup",
        )? {
            partial.parameters.get(1).and_then(ValueExt::reference)
        } else {
            None
        };
        if let Some(representation) = representation {
            ctx.insert_scoped_btree_value(
                &mut linked_bytes,
                &mut linked,
                representation,
                "step_tessellation_product_representations",
            )?;
        }
    }
    if linked.is_empty() {
        return Ok((linked, linked_bytes));
    }
    let mut relationship_bytes = ctx.reserve_scoped(0, "step_tessellation_relationship_edges")?;
    let mut relationships = BTreeMap::<u64, BTreeSet<u64>>::new();
    let mut source_items = exchange.records().values();
    for _ in 0..source_items.len() {
        let Some(record) = ctx.next_charged(
            &mut source_items,
            "STEP product linked representations map traversal",
        )?
        else {
            break;
        };
        let Some(shape_relationship) = record.partial(ctx, "SHAPE_REPRESENTATION_RELATIONSHIP")?
        else {
            continue;
        };
        let mut left = None;
        let mut pair = ctx.find_map(
            &shape_relationship.parameters,
            |value| {
                let Some(reference) = value.reference() else {
                    return Ok(None);
                };
                Ok(left.replace(reference).map(|first| (first, reference)))
            },
            "STEP shape relationship reference traversal",
        )?;
        if pair.is_none() {
            let Some(base) = record.partial(ctx, "REPRESENTATION_RELATIONSHIP")? else {
                continue;
            };
            let mut left = None;
            pair = ctx.find_map(
                &base.parameters,
                |value| {
                    let Some(reference) = value.reference() else {
                        return Ok(None);
                    };
                    Ok(left.replace(reference).map(|first| (first, reference)))
                },
                "STEP representation relationship reference traversal",
            )?;
        }
        let Some((left, right)) = pair else { continue };
        relationship_bytes.with_storage(|| {
            ctx.insert_btree_group_set(
                &mut relationships,
                left,
                right,
                "step_tessellation_relationship_nodes",
                "step_tessellation_relationship_edges",
            )
        })?;
        relationship_bytes.with_storage(|| {
            ctx.insert_btree_group_set(
                &mut relationships,
                right,
                left,
                "step_tessellation_relationship_nodes",
                "step_tessellation_relationship_edges",
            )
        })?;
    }
    let mut pending_bytes = ctx.reserve_scoped(0, "step_tessellation_product_pending")?;
    let mut pending = Vec::new();
    let mut source_items = linked.iter();
    for _ in 0..source_items.len() {
        let Some(&id) = ctx.next_charged(&mut source_items, "STEP product pending traversal")?
        else {
            break;
        };
        ctx.push_scoped_vec(
            &mut pending_bytes,
            &mut pending,
            id,
            "step_tessellation_product_pending",
        )?;
    }
    while !pending.is_empty() {
        ctx.charge_work(1, "STEP product relationship walk")?;
        let Some(representation) = pending.pop() else {
            break;
        };
        if let Some(items) =
            ctx.get_btree_map(&relationships, &representation, "step_tessellation_lookup")?
        {
            let mut source_items = items.iter();
            for _ in 0..source_items.len() {
                let Some(&related) =
                    ctx.next_charged(&mut source_items, "STEP optional collection traversal")?
                else {
                    break;
                };
                if ctx.insert_scoped_btree_value(
                    &mut linked_bytes,
                    &mut linked,
                    related,
                    "step_tessellation_product_representations",
                )? {
                    ctx.reserve_scoped_vec(
                        &mut pending_bytes,
                        &mut pending,
                        1,
                        "step_tessellation_product_pending",
                    )?;
                    pending.push(related);
                }
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
    let mut bytes = ctx.reserve_scoped(0, "step_tessellation_product_items")?;
    let mut items = BTreeSet::new();
    let mut source_items = representations.iter();
    for _ in 0..source_items.len() {
        let Some(id) = ctx.next_charged(
            &mut source_items,
            "STEP product representation items traversal",
        )?
        else {
            break;
        };
        let Some(record) = ctx.get_btree_map(exchange.records(), id, "step_tessellation_lookup")?
        else {
            continue;
        };
        if let Some((item_buffer, _representation_item_bytes)) =
            admitted_representation_items(record, ctx)?
        {
            let representation_items = item_buffer;
            ctx.fold(
                &representation_items,
                (),
                |(), &item| {
                    ctx.insert_scoped_btree_value(
                        &mut bytes,
                        &mut items,
                        item,
                        "step_tessellation_product_items",
                    )?;
                    Ok(())
                },
                "STEP product representation member traversal",
            )?;
            drop(representation_items);
        }
    }
    Ok((items, bytes))
}

fn admitted_representation_items<'a>(
    record: &RawRecord,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<(Vec<u64>, ScopedReservation<'a>)>, CodecError> {
    let Some(values) = super::representation::item_values(ctx, record)? else {
        return Ok(None);
    };
    let mut bytes = ctx.reserve_scoped(0, "step_tessellation_representation_items")?;
    let mut items = Vec::new();
    let mut source_items = values.iter();
    for _ in 0..source_items.len() {
        let Some(value) = ctx.next_charged(
            &mut source_items,
            "STEP admitted representation items traversal",
        )?
        else {
            break;
        };
        if let Some(id) = value.reference() {
            bytes.with_storage(|| {
                ctx.push_vec(&mut items, id, "step_tessellation_representation_items")
            })?;
        }
    }
    Ok(Some((items, bytes)))
}

fn linked_bodies<'a, 'ir>(
    record: &RawRecord,
    kind: &str,
    topology: &'ir TopologyData,
    ctx: &'a DecodeContext<'_>,
) -> Result<(BTreeSet<&'ir BodyId>, ScopedReservation<'a>), CodecError> {
    let Some(link) = entity_parameter(ctx, record, kind, 1, 1)?.and_then(ValueExt::reference)
    else {
        return Ok((
            BTreeSet::new(),
            ctx.reserve_scoped(0, "step_tessellation_linked_bodies")?,
        ));
    };
    match kind {
        "TESSELLATED_SOLID" => {
            let bodies =
                ctx.get_btree_map(&topology.body_by_root, &link, "step_tessellation_lookup")?;
            let mut bytes = ctx.reserve_scoped(0, "step_tessellation_linked_bodies")?;
            let mut linked = BTreeSet::new();
            ctx.fold(
                bodies.map_or(&[][..], Vec::as_slice),
                (),
                |(), body| {
                    bytes.with_storage(|| {
                        ctx.insert_btree_set(&mut linked, body, "step_tessellation_linked_bodies")
                    })?;
                    Ok(())
                },
                "STEP linked body traversal",
            )?;
            Ok((linked, bytes))
        }
        "TESSELLATED_SHELL" => {
            let bodies =
                ctx.get_btree_map(&topology.body_by_shell, &link, "step_tessellation_lookup")?;
            let mut bytes = ctx.reserve_scoped(0, "step_tessellation_linked_bodies")?;
            let mut linked = BTreeSet::new();
            if let Some(bodies) = bodies {
                let mut source_items = bodies.iter();
                for _ in 0..source_items.len() {
                    let Some(body) =
                        ctx.next_charged(&mut source_items, "STEP linked body traversal")?
                    else {
                        break;
                    };
                    bytes.with_storage(|| {
                        ctx.insert_btree_set(&mut linked, body, "step_tessellation_linked_bodies")
                    })?;
                }
            }
            Ok((linked, bytes))
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
    let mut bytes = { ctx.reserve_scoped(0, "step_tessellation_pnindex") }?;
    Ok(bytes
        .with_storage(|| {
            ctx.collect_options(
                values
                    .iter()
                    .map(|value| u32::try_from(value.integer()?).ok()),
                "step_tessellation_pnindex",
            )
        })?
        .map(|indices| (indices, bytes)))
}

fn container_item_ids<'a>(
    items: &[Value],
    kind: &str,
    id: u64,
    ctx: &'a DecodeContext<'_>,
) -> Result<(Vec<u64>, ScopedReservation<'a>), CodecError> {
    let mut bytes = { ctx.reserve_scoped(0, "step_tessellation_container_items") }?;
    let ids = bytes.with_storage(|| {
        ctx.try_collect_vec(
            items.iter().enumerate().map(|(index, item)| {
                item.reference().ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "{kind} #{id} item {} is not a reference",
                        index + 1
                    ))
                })
            }),
            "step_tessellation_container_items",
        )
    })?;
    Ok((ids, bytes))
}

fn has_entity(ctx: &DecodeContext<'_>, record: &RawRecord, name: &str) -> Result<bool, CodecError> {
    Ok(entity_kind(ctx, record, &[name])?.is_some())
}

fn is_tessellated_shape_representation(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<bool, CodecError> {
    Ok(entity_kind(
        ctx,
        record,
        &[
            "TESSELLATED_SHAPE_REPRESENTATION",
            "TESSELLATED_SHAPE_REPRESENTATION_WITH_ACCURACY_PARAMETERS",
        ],
    )?
    .is_some())
}

fn entity_kind<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    names: &[&str],
) -> Result<Option<&'a str>, CodecError> {
    ctx.find_map(
        &record.partials,
        |partial| {
            Ok(names
                .iter()
                .any(|name| partial.name == *name)
                .then_some(partial.name.as_str()))
        },
        "STEP tessellation entity partial traversal",
    )
}

fn entity_parameter<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    entity: &str,
    index: usize,
    simple_offset: usize,
) -> Result<Option<&'a Value>, CodecError> {
    let Some(partial) = ctx.find_map(
        &record.partials[..],
        |partial| -> Result<Option<_>, CodecError> {
            Ok((partial.name == entity).then_some(partial))
        },
        "STEP tessellation entity parameter traversal",
    )?
    else {
        return Ok(None);
    };
    let offset = if record.partials.len() == 1 {
        simple_offset
    } else {
        0
    };
    Ok(partial.parameters.get(index + offset))
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
    fn of(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<Option<Self>, CodecError> {
        ctx.find_map(
            &record.partials[..],
            |partial| {
                Ok(Some(match partial.name.as_str() {
                    "TRIANGULATED_FACE" => Self::Face,
                    "COMPLEX_TRIANGULATED_FACE" => Self::ComplexFace,
                    "TRIANGULATED_SURFACE_SET" => Self::SurfaceSet,
                    "COMPLEX_TRIANGULATED_SURFACE_SET" => Self::ComplexSurfaceSet,
                    _ => return Ok(None),
                }))
            },
            "STEP triangulated entity traversal",
        )
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

fn inherited_parameter<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    entity: &str,
    index: usize,
) -> Result<Option<&'a Value>, CodecError> {
    if record.partials.len() == 1 {
        Ok(record.parameter(index + 1))
    } else {
        Ok(entity_parameter(ctx, record, entity, index, 0)?)
    }
}

fn decimal_digits(id: u64) -> u64 {
    if id == 0 {
        1
    } else {
        u64::from(id.ilog10()) + 1
    }
}

fn admitted_mesh_id(
    id: u64,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::tessellation::TessellationId, CodecError> {
    let digits = decimal_digits(id);
    let _key_bytes = ctx.reserve_scoped(digits, "step_tessellation_mesh_key")?;
    ctx.charge_retained(
        u64_from_index("step:tessellation:mesh#".len()) + digits,
        "step_tessellation_mesh_id",
    )?;
    Ok(ids::tessellation(kind!("mesh"), id).into())
}

fn admitted_surface_id<'a>(
    id: u64,
    ctx: &'a DecodeContext<'_>,
) -> Result<(cadmpeg_ir::ids::Identity, ScopedReservation<'a>), CodecError> {
    let digits = decimal_digits(id);
    let bytes = u64_from_index("step:data:surface#".len()) + digits * 2;
    let reservation = ctx.reserve_scoped(bytes, "step_tessellation_surface_id")?;
    Ok((ids::data(kind!("surface"), id), reservation))
}

fn push_loss(
    losses: &mut Vec<LossNote>,
    code: StepLossCode,
    message: std::fmt::Arguments<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    ctx.reserve_vec(losses, 1, "step_tessellation_loss_notes")?;
    let vocabulary_bytes = u64_from_index(code.code().len()) + u64_from_index("step".len());
    ctx.charge_retained(vocabulary_bytes, "step_tessellation_loss_notes")?;
    let text = ctx.format_retained(message, "step_tessellation_loss_notes")?;
    losses.push(code.note(text));
    Ok(())
}

fn coordinate_rows<'a>(
    record: &RawRecord,
    scale: f64,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<(Vec<FinitePoint3>, ScopedReservation<'a>)>, CodecError> {
    ctx.find_map(
        &record.partials,
        |partial| {
            ctx.find_map(
                &partial.parameters,
                |value| {
                    let Some(rows) = value.list() else {
                        return Ok(None);
                    };
                    let mut bytes = ctx.reserve_scoped(0, "step_tessellation_coordinate_rows")?;
                    let vertices = bytes.with_storage(|| {
                        ctx.collect_options(
                            rows.iter().map(|row| {
                                let values = row.list()?;
                                if values.len() != 3 {
                                    return None;
                                }
                                FinitePoint3::new(Point3::new(
                                    values[0].number()? * scale,
                                    values[1].number()? * scale,
                                    values[2].number()? * scale,
                                ))
                            }),
                            "step_tessellation_coordinate_rows",
                        )
                    })?;
                    Ok(vertices
                        .filter(|vertices| !vertices.is_empty())
                        .map(|vertices| (vertices, bytes)))
                },
                "STEP record parameter traversal",
            )
        },
        "STEP coordinate rows traversal",
    )
}

fn triangle_rows<'a>(
    value: &Value,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<AdmittedTriangles<'a>>, CodecError> {
    let Some(rows) = value.list() else {
        return Ok(None);
    };
    let mut bytes = { ctx.reserve_scoped(0, "step_tessellation_triangle_rows") }?;
    Ok(bytes
        .with_storage(|| {
            ctx.collect_options(
                rows.iter().map(|row| {
                    let values = row.list()?;
                    if values.len() != 3 {
                        return None;
                    }
                    Some([
                        u32::try_from(values[0].integer()?).ok()?,
                        u32::try_from(values[1].integer()?).ok()?,
                        u32::try_from(values[2].integer()?).ok()?,
                    ])
                }),
                "step_tessellation_triangle_rows",
            )
        })?
        .map(|triangles| AdmittedTriangles { triangles, bytes }))
}

fn complex_triangles<'a>(
    strips: Option<&Value>,
    fans: Option<&Value>,
    kind: &str,
    id: u64,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<AdmittedTriangles<'a>>, CodecError> {
    let (strips_buffer, _strip_rows_bytes, _strip_indices_bytes) =
        index_rows(strips, kind, id, "strip", ctx)?;
    let strips = strips_buffer;
    let (fans_buffer, _fan_rows_bytes, _fan_indices_bytes) =
        index_rows(fans, kind, id, "fan", ctx)?;
    let fans = fans_buffer;
    let mut bytes = ctx.reserve_scoped(0, "step_complex_tessellation_triangles")?;
    let mut triangles = Vec::new();
    let mut source_items = strips.iter();
    for _ in 0..source_items.len() {
        let Some(strip) =
            ctx.next_charged(&mut source_items, "STEP complex triangles traversal")?
        else {
            break;
        };
        let mut source_items = 0..strip.len() - 2;
        for _ in 0..source_items.len() {
            let Some(index) =
                ctx.next_charged(&mut source_items, "STEP strip triangle traversal")?
            else {
                break;
            };
            let triangle = if index % 2 == 0 {
                [strip[index], strip[index + 1], strip[index + 2]]
            } else {
                [strip[index + 1], strip[index], strip[index + 2]]
            };
            bytes.with_storage(|| {
                ctx.push_vec(
                    &mut triangles,
                    triangle,
                    "step_complex_tessellation_triangles",
                )
            })?;
        }
    }
    let mut source_items = fans.iter();
    for _ in 0..source_items.len() {
        let Some(fan) =
            ctx.next_charged(&mut source_items, "STEP complex triangle fan traversal")?
        else {
            break;
        };
        let mut source_items = 1..fan.len() - 1;
        for _ in 0..source_items.len() {
            let Some(index) = ctx.next_charged(&mut source_items, "STEP fan triangle traversal")?
            else {
                break;
            };
            bytes.with_storage(|| {
                ctx.push_vec(
                    &mut triangles,
                    [fan[0], fan[index], fan[index + 1]],
                    "step_complex_tessellation_triangles",
                )
            })?;
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
    let mut row_bytes = { ctx.reserve_scoped(0, "step_complex_tessellation_rows") }?;
    let mut index_bytes = ctx.reserve_scoped(0, "step_complex_tessellation_indices")?;
    let indices = row_bytes.with_storage(|| {
        ctx.try_collect_vec(
            rows.iter().enumerate().map(|(row_index, row)| {
                let invalid = || {
                    CodecError::malformed(format_args!(
                        "{kind} #{id} {lane} row {} is invalid",
                        row_index + 1
                    ))
                };
                let Some(values) = row.list().filter(|values| values.len() >= 3) else {
                    return Err(invalid());
                };
                index_bytes.with_storage(|| {
                    ctx.try_collect_vec(
                        values.iter().map(|value| {
                            u32::try_from(value.integer().ok_or_else(invalid)?)
                                .map_err(|_| invalid())
                        }),
                        "step_complex_tessellation_indices",
                    )
                })
            }),
            "step_complex_tessellation_rows",
        )
    })?;
    Ok((indices, row_bytes, index_bytes))
}

fn replicated_normals<'a>(
    count: usize,
    normal: FiniteVector3,
    ctx: &'a DecodeContext<'_>,
) -> Result<(Vec<FiniteVector3>, ScopedReservation<'a>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "step_tessellation_normal_replication")?;
    let normals = storage
        .with_storage(|| ctx.alloc_filled(count, normal, "step_tessellation_normal_replication"))?;
    Ok((normals, storage))
}

fn normal_rows<'a>(
    value: Option<&Value>,
    ctx: &'a DecodeContext<'_>,
) -> Result<Option<(Vec<FiniteVector3>, ScopedReservation<'a>)>, CodecError> {
    let Some(rows) = value.and_then(ValueExt::list) else {
        return Ok(None);
    };
    let mut bytes = { ctx.reserve_scoped(0, "step_tessellation_normal_rows") }?;
    Ok(bytes
        .with_storage(|| {
            ctx.collect_options(
                rows.iter().map(|row| {
                    let values = row.list()?;
                    if values.len() != 3 {
                        return None;
                    }
                    let normal = Vector3::new(
                        values[0].number()?,
                        values[1].number()?,
                        values[2].number()?,
                    );
                    super::geometry::normalize(normal).map(FiniteVector3::from)
                }),
                "step_tessellation_normal_rows",
            )
        })?
        .map(|normals| (normals, bytes)))
}
#[cfg(test)]
pub(crate) mod tests;
