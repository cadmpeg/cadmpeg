// SPDX-License-Identifier: Apache-2.0
//! CATIA native ownership, alias, and wire projections.

use cadmpeg_core::decode::u64_from_index;

use crate::object_graph::{self, HeadToken, ListItem, PayloadField};

use super::edge_definition::CatiaConsolidatedEdgeDefinition;

use super::{
    catalog, container, design_object_id, entity_table, resolved_payload_references,
    resolved_storage_link, value_block, AliasLead, CatiaAliasRow, CatiaAllocationReferenceEncoding,
    CatiaCatalog, CatiaCatalogEntry, CatiaConsolidatedAnalyticCircleBinding,
    CatiaConsolidatedCircle, CatiaConsolidatedClass25Descriptor, CatiaConsolidatedCone,
    CatiaConsolidatedEdgeNode, CatiaConsolidatedEdgeRun, CatiaConsolidatedEdgeUses,
    CatiaConsolidatedLineProfile, CatiaConsolidatedOwnerPacket, CatiaConsolidatedParameterPoint,
    CatiaConsolidatedParameterPointPayload, CatiaConsolidatedPcurve, CatiaConsolidatedPlaneCarrier,
    CatiaConsolidatedPlaneCarrierPayload, CatiaConsolidatedReferenceList,
    CatiaConsolidatedRevolution, CatiaConsolidatedSphere, CatiaConsolidatedSupportBinding,
    CatiaConsolidatedTorus, CatiaDesignClass, CatiaDesignObject, CatiaDesignObjectRelation,
    CatiaDesignObjectRelationSource, CatiaDesignParallelReferenceTable, CatiaDesignReferenceCell,
    CatiaDesignReferenceColumn, CatiaDesignReferenceRow, CatiaEntityRecord, CatiaEntityRecordBody,
    CatiaEntityReference, CatiaExternalReference, CatiaFaceNodeRelation,
    CatiaFaceNodeTargetEncoding, CatiaFinjplSegment, CatiaObjectClass, CatiaObjectEntity,
    CatiaObjectGraph, CatiaObjectOwner, CatiaObjectRecord, CatiaObjectStorage,
    CatiaOuterContainerBinding, CatiaOwnerBoundaryCycle, CatiaOwnerBoundaryEdge,
    CatiaOwnerChartAddress, CatiaOwnerChartAliasBinding, CatiaOwnerChartBridge,
    CatiaOwnerChartBridgeReference, CatiaOwnerChartCarrier, CatiaOwnerChartRelation,
    CatiaOwnerIdentityEncoding, CatiaOwnerIdentityTarget, CatiaOwnerPacketPayload,
    CatiaOwnerReferenceEncoding, CatiaPreviewImage, CatiaReferenceSignature, CatiaValueBlock,
    CatiaValueSchemaSelection, CatiaValueSchemaSelectionKind, CatiaValueSchemaSelectionValue,
    CatiaZeroEntityRecord, CodecError, ConsolidatedRecord, DecodeContext, GraphRecordIndex,
    HashMap, HashSet,
};

/// Groups one graph's owned field records into design objects. `entities`
/// holds the graph's entity records, where entity `i` belongs to record `i`.
pub(super) fn graph_design_objects(
    ctx: &DecodeContext<'_>,
    graph: &CatiaObjectGraph,
    entities: &[CatiaEntityRecord],
    record_index: &GraphRecordIndex,
    objects: &mut Vec<CatiaDesignObject>,
) -> Result<(), CodecError> {
    // Owner groups, their positions and the per-object class sets are dropped
    // once the objects are built.
    let mut scratch = ctx.reserve_scoped(0, "catia_design_owner_scratch")?;
    let mut fields = Vec::<(u32, Vec<&CatiaObjectRecord>)>::new();
    let mut owner_indices = HashMap::<u32, usize>::new();
    for record in ctx.admit_iter(&graph.records, "catia_native_design_graph_record_visits")? {
        let Some(owner) = record.owner_entity_id() else {
            continue;
        };
        let index = match ctx.get_hash_map(&owner_indices, &owner, "catia_design_owner_indices")? {
            Some(index) => *index,
            None => {
                let index = fields.len();
                scratch.with_storage(|| -> Result<(), CodecError> {
                    ctx.push_vec(
                        &mut fields,
                        (owner, Vec::new()),
                        "catia_design_owner_groups",
                    )?;
                    ctx.insert_hash_map(
                        &mut owner_indices,
                        owner,
                        index,
                        "catia_design_owner_indices",
                    )?;
                    Ok(())
                })?;
                index
            }
        };
        scratch.with_storage(|| {
            ctx.push_vec(&mut fields[index].1, record, "catia_design_owner_fields")
        })?;
    }
    for (ordinal, (owner_entity_id, records)) in ctx
        .admit_iter(&fields, "catia_design_owner_group_visits")?
        .enumerate()
    {
        let Some(first_record) = records.first() else {
            continue;
        };
        let owner_record = record_index.record(ctx, &graph.records, *owner_entity_id)?;
        let id = design_object_id(ctx, graph.byte_offset, *owner_entity_id)?;
        let owner_design_object = match owner_record
            .and_then(CatiaObjectRecord::owner_entity_id)
            .filter(|owner| *owner != *owner_entity_id)
        {
            Some(owner)
                if ctx.contains_key_hash_map(
                    &owner_indices,
                    &owner,
                    "catia_design_owner_indices",
                )? =>
            {
                Some(design_object_id(ctx, graph.byte_offset, owner)?)
            }
            Some(_) | None => None,
        };
        let owner_class = match owner_record.filter(|record| record_has_separator_roles(record)) {
            Some(record) => design_class(ctx, record)?,
            None => None,
        };
        let mut field_ids = Vec::new();
        let mut field_classes = Vec::new();
        let mut class_storage = ctx.reserve_scoped(0, "catia_design_field_class_scratch")?;
        let mut seen_classes = std::collections::BTreeSet::new();
        let mut definition_values = Vec::new();
        let mut definition_chain_values = Vec::new();
        let mut relations = Vec::new();
        for record in ctx.admit_iter(records, "catia_design_owner_field_visits")? {
            ctx.push_vec(
                &mut field_ids,
                ctx.copy_retained_text(&record.id, "catia_design_field_id")?,
                "catia_design_fields",
            )?;
            if let (Some(entry), Some(name)) = (record.class_entry(), record.class_name()) {
                if class_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut seen_classes,
                        (entry, name),
                        "catia_design_field_class_set",
                    )
                })? {
                    if let Some(class) = design_class(ctx, record)? {
                        ctx.push_vec(&mut field_classes, class, "catia_design_field_classes")?;
                    }
                }
            }
            if let Some(entity_id) = record.entity_record() {
                let entity = usize::try_from(record.ordinal)
                    .ok()
                    .and_then(|ordinal| entities.get(ordinal))
                    .filter(|entity| entity.ordinal == record.ordinal);
                if entity.is_some_and(|entity| entity.definition_value().is_some()) {
                    ctx.push_vec(
                        &mut definition_values,
                        ctx.copy_retained_text(entity_id, "catia_design_definition_value_id")?,
                        "catia_design_definition_value_rows",
                    )?;
                }
                if entity.is_some_and(|entity| entity.definition_chain_value().is_some()) {
                    ctx.push_vec(
                        &mut definition_chain_values,
                        ctx.copy_retained_text(entity_id, "catia_design_definition_chain_id")?,
                        "catia_design_definition_chain_rows",
                    )?;
                }
            }
            if let (Some(target_field), Some(storage_ref)) =
                (record.storage_record(), record.storage_ref())
            {
                if let Some(target_record) =
                    record_index.record(ctx, &graph.records, storage_ref)?
                {
                    let relation = CatiaDesignObjectRelation {
                        source_field: ctx
                            .copy_retained_text(&record.id, "catia_design_relation_source")?,
                        source_class: design_class(ctx, record)?,
                        source: CatiaDesignObjectRelationSource::Storage,
                        target_entity_id: storage_ref,
                        target_field: ctx
                            .copy_retained_text(target_field, "catia_design_relation_target")?,
                        target_class: design_class(ctx, target_record)?,
                        target_design_object: record
                            .storage_design_object()
                            .map(|id| ctx.copy_retained_text(id, "catia_design_relation_object"))
                            .transpose()?,
                    };
                    ctx.push_vec(&mut relations, relation, "catia_design_relations")?;
                }
            }
            for reference in ctx.admit_iter(
                &record.references,
                "catia_native_design_record_reference_visits",
            )? {
                let Some(target_field) = reference.target() else {
                    continue;
                };
                let Some(target_record) =
                    record_index.record(ctx, &graph.records, reference.entity_id())?
                else {
                    continue;
                };
                let relation = CatiaDesignObjectRelation {
                    source_field: ctx
                        .copy_retained_text(&record.id, "catia_design_relation_source")?,
                    source_class: design_class(ctx, record)?,
                    source: CatiaDesignObjectRelationSource::Payload {
                        payload_offset: reference.payload_offset(),
                        container: reference.source().clone(),
                    },
                    target_entity_id: reference.entity_id(),
                    target_field: ctx
                        .copy_retained_text(target_field, "catia_design_relation_target")?,
                    target_class: design_class(ctx, target_record)?,
                    target_design_object: reference
                        .design_object()
                        .map(|id| ctx.copy_retained_text(id, "catia_design_relation_object"))
                        .transpose()?,
                };
                ctx.push_vec(&mut relations, relation, "catia_design_relations")?;
            }
        }
        let parallel_reference_table =
            design_parallel_reference_table(ctx, records, graph, record_index)?;
        let object = CatiaDesignObject {
            id,
            parent: ctx.copy_retained_text(&graph.id, "catia_design_parent")?,
            ordinal: u64_from_index(ordinal),
            first_field_byte_offset: first_record.byte_offset,
            owner_entity_id: *owner_entity_id,
            owner_record: owner_record
                .map(|record| ctx.copy_retained_text(&record.id, "catia_design_owner_record"))
                .transpose()?,
            owner_design_object,
            owner_class,
            owner_storage_ref: owner_record
                .filter(|record| record_has_separator_roles(record))
                .and_then(CatiaObjectRecord::storage_ref),
            fields: field_ids,
            field_classes,
            definition_values,
            definition_chain_values,
            relations,
            parallel_reference_table,
        };
        ctx.push_vec(objects, object, "catia_design_objects")?;
    }
    Ok(())
}

pub(super) fn design_parallel_reference_table(
    ctx: &DecodeContext<'_>,
    records: &[&CatiaObjectRecord],
    graph: &CatiaObjectGraph,
    record_index: &GraphRecordIndex,
) -> Result<Option<CatiaDesignParallelReferenceTable>, CodecError> {
    const MATCH: &str = "catia_native_design_table_cell_checks";
    if records.len() < 2 {
        return Ok(None);
    }
    let mut column_storage = ctx.reserve_scoped(0, "catia_design_column_scratch")?;
    let mut columns = Vec::new();
    let mut record_rows = records.iter();
    while let Some(record) = ctx.next_charged(
        &mut record_rows,
        "catia_native_parallel_reference_record_visits",
    )? {
        let [PayloadField::List {
            declared_count,
            items,
            offset: list_offset,
        }, middle @ .., PayloadField::Terminator] = record.payload.fields.as_slice()
        else {
            return Ok(None);
        };
        if *declared_count < 2
            || usize::try_from(*declared_count).ok() != Some(items.len())
            || !ctx.all_by(
                middle,
                |field| Ok(matches!(field, PayloadField::Atom { .. })),
                "catia_native_parallel_reference_middle_visits",
            )?
            || !ctx.all_by(
                items,
                |item| Ok(matches!(item, ListItem::Reference { .. })),
                "catia_native_parallel_reference_item_visits",
            )?
        {
            return Ok(None);
        }
        column_storage.with_storage(|| {
            ctx.push_vec(
                &mut columns,
                (*record, *list_offset, items.as_slice()),
                "catia_design_columns",
            )
        })?;
    }
    let Some((_, _, first_items)) = columns.first() else {
        return Ok(None);
    };
    let row_count = first_items.len();
    if ctx.any_by(
        &columns,
        |(_, _, references)| Ok(references.len() != row_count),
        "catia_native_parallel_reference_column_checks",
    )? {
        return Ok(None);
    }
    let mut rows = Vec::new();
    for (row, _) in ctx
        .admit_iter(*first_items, "catia_native_parallel_reference_rows")?
        .enumerate()
    {
        let mut cells = Vec::new();
        for (_, _, references) in
            ctx.admit_iter(&columns, "catia_native_parallel_reference_row_cells")?
        {
            let ListItem::Reference {
                value: target_entity_id,
                offset: payload_offset,
            } = &references[row]
            else {
                return Ok(None);
            };
            let target = record_index.record(ctx, &graph.records, *target_entity_id)?;
            let cell = CatiaDesignReferenceCell::from_parts(
                u64_from_index(*payload_offset),
                *target_entity_id,
                Some(*target_entity_id) == record_index.terminal_null_entity_id,
                target
                    .map(|record| ctx.copy_retained_text(&record.id, "catia_design_cell_target"))
                    .transpose()?,
                target
                    .map(|record| design_class(ctx, record))
                    .transpose()?
                    .flatten(),
                target
                    .and_then(|record| record.design_object.as_deref())
                    .map(|id| ctx.copy_retained_text(id, "catia_design_cell_object"))
                    .transpose()?,
            );
            ctx.push_vec(&mut cells, cell, "catia_design_row_cells")?;
        }
        let mut matching_design_object = None;
        if let Some(member) = cells.first().and_then(|cell| cell.design_object()) {
            // Every cell names a field of the member with its column's class,
            // and no field repeats within the row.
            let mut fields_storage = ctx.reserve_scoped(0, "catia_design_row_fields")?;
            let mut fields = std::collections::BTreeSet::new();
            let matches = ctx.all_by(
                columns.iter().zip(&cells),
                |((column, _, _), cell)| {
                    let (
                        Some(column_entry),
                        Some(column_name),
                        Some(field),
                        Some(cell_class),
                        Some(object),
                    ) = (
                        column.class_entry(),
                        column.class_name(),
                        cell.field(),
                        cell.field_class(),
                        cell.design_object(),
                    )
                    else {
                        return Ok(false);
                    };
                    Ok(ctx.equal_bytes(
                        cell_class.entry.as_bytes(),
                        column_entry.as_bytes(),
                        MATCH,
                    )? && ctx.equal_bytes(
                        cell_class.name.as_bytes(),
                        column_name.as_bytes(),
                        MATCH,
                    )? && ctx.equal_bytes(object.as_bytes(), member.as_bytes(), MATCH)?
                        && fields_storage.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut fields,
                                field,
                                "catia_native_design_table_prior_cell_checks",
                            )
                        })?)
                },
                "catia_native_design_table_column_checks",
            )?;
            if matches {
                matching_design_object =
                    Some(ctx.copy_retained_text(member, "catia_design_matching_object")?);
            }
        }
        let reference_row = CatiaDesignReferenceRow {
            cells,
            matching_design_object,
        };
        ctx.push_vec(&mut rows, reference_row, "catia_design_reference_rows")?;
    }
    let columns = ctx.try_collect_vec(
        columns
            .into_iter()
            .map(|(record, offset, _)| -> Result<_, CodecError> {
                Ok(CatiaDesignReferenceColumn {
                    field: ctx.copy_retained_text(&record.id, "catia_design_column_field")?,
                    field_class: design_class(ctx, record)?,
                    list_payload_offset: u64_from_index(offset),
                })
            }),
        "catia_design_table_columns",
    )?;
    CatiaDesignParallelReferenceTable::new(ctx, columns, rows)
}

fn design_class(
    ctx: &DecodeContext<'_>,
    record: &CatiaObjectRecord,
) -> Result<Option<CatiaDesignClass>, CodecError> {
    let (Some(entry), Some(name)) = (record.class_entry(), record.class_name()) else {
        return Ok(None);
    };
    Ok(Some(CatiaDesignClass {
        entry: ctx.copy_retained_text(entry, "catia_design_class_entry")?,
        name: ctx.copy_retained_text(name, "catia_design_class_name")?,
    }))
}

fn record_has_separator_roles(record: &CatiaObjectRecord) -> bool {
    matches!(record.head.get(1), Some(HeadToken::Separator))
}

pub(super) fn consolidated_revolutions(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    circles: &[CatiaConsolidatedCircle],
) -> Result<Vec<CatiaConsolidatedRevolution>, CodecError> {
    let mut temporary = ctx.reserve_scoped(0, "catia native revolution workspace")?;
    let mut resolved_profiles = HashMap::new();
    let (resolved, _resolved_storage) = ctx
        .with_scoped_storage("catia native revolution profiles", || {
            crate::families::b2::records::b2_resolved_revolutions_from_records(ctx, bytes, records)
        })?;
    for resolved in ctx.admit_iter(&resolved, "catia_native_resolved_revolution_visits")? {
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut resolved_profiles,
                u64_from_index(resolved.revolution.pos),
                u64_from_index(resolved.profile.pos),
                "catia_native_revolution_profile_index",
            )
        })?;
    }
    let mut circle_ids = HashMap::new();
    for circle in ctx.admit_iter(circles, "catia_native_revolution_circle_visits")? {
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut circle_ids,
                circle.byte_offset,
                circle.id.as_str(),
                "catia_native_revolution_circle_index",
            )
        })?;
    }
    let mut revolutions = Vec::new();
    for (index, revolution) in
        crate::families::b2::records::b2_revolutions_from_records(ctx, bytes, records)?.enumerate()
    {
        const LOOKUP: &str = "catia_native_revolution_lookups";
        let profile_offset =
            ctx.get_hash_map(&resolved_profiles, &u64_from_index(revolution.pos), LOOKUP)?;
        let profile_circle = match profile_offset
            .map(|offset| ctx.get_hash_map(&circle_ids, offset, LOOKUP))
            .transpose()?
            .flatten()
        {
            Some(id) => Some(ctx.copy_retained_text(id, "catia_native_revolution_profile_circle")?),
            None => None,
        };
        let value = CatiaConsolidatedRevolution {
            id: ctx.format_retained(
                format_args!("catia:consolidated:revolution#{index:00}"),
                "catia_native_revolution_id",
            )?,
            byte_offset: u64_from_index(revolution.pos),
            reference_token: revolution.reference_token,
            profile_allocation_id: revolution.profile_allocation_id,
            origin: revolution.origin.coordinates().into(),
            direction_x: revolution.profile_frame.axis(),
            direction_y: revolution.profile_frame.reference(),
            axis: revolution.axis,
            angular_range: revolution.angular_range,
            profile_range: revolution.profile_range,
            profile_circle,
            angular_scale: revolution.angular_scale,
        };
        ctx.push_vec(&mut revolutions, value, "catia_native_revolutions")?;
    }
    Ok(revolutions)
}

pub(super) fn consolidated_line_profiles(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedLineProfile>, CodecError> {
    let mut profiles = Vec::new();
    for (index, line) in
        crate::families::b2::records::b2_line_profiles_from_records(ctx, bytes, records)?
            .enumerate()
    {
        let value = CatiaConsolidatedLineProfile {
            id: ctx.format_retained(
                format_args!("catia:consolidated:line-profile#{index:00}"),
                "catia_native_line_profile_id",
            )?,
            byte_offset: u64_from_index(line.pos),
            origin: line.origin.coordinates().into(),
            direction: line.direction,
            range: line.range,
        };
        ctx.push_vec(&mut profiles, value, "catia_native_line_profiles")?;
    }
    Ok(profiles)
}

pub(super) fn consolidated_spheres(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedSphere>, CodecError> {
    let mut spheres = Vec::new();
    for (index, sphere) in
        crate::families::b2::records::b2_spheres_from_records(ctx, bytes, records)?.enumerate()
    {
        let value = CatiaConsolidatedSphere {
            id: ctx.format_retained(
                format_args!("catia:consolidated:sphere#{index:00}"),
                "catia_native_sphere_id",
            )?,
            byte_offset: u64_from_index(sphere.pos),
            center: sphere.center.coordinates().into(),
            direction_x: sphere.frame.reference(),
            direction_y: sphere.direction_y,
            axis: sphere.frame.axis(),
            radius: sphere.radius,
            azimuth_range: sphere.azimuth_range,
            latitude_range: sphere.latitude_range,
        };
        ctx.push_vec(&mut spheres, value, "catia_native_spheres")?;
    }
    Ok(spheres)
}

pub(super) fn consolidated_tori(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedTorus>, CodecError> {
    let mut tori = Vec::new();
    for (index, torus) in
        crate::families::b2::records::b2_tori_from_records(ctx, bytes, records)?.enumerate()
    {
        let value = CatiaConsolidatedTorus {
            id: ctx.format_retained(
                format_args!("catia:consolidated:torus#{index:00}"),
                "catia_native_torus_id",
            )?,
            byte_offset: u64_from_index(torus.pos),
            center: torus.center.coordinates().into(),
            direction_x: torus.frame.reference(),
            direction_y: torus.direction_y,
            axis: torus.frame.axis(),
            major_radius: torus.major_radius,
            minor_radius: torus.minor_radius,
            major_angular_range: torus.major_angular_range,
            major_angular_domain: torus.major_angular_domain,
            minor_angular_range: torus.minor_angular_range,
            minor_angular_domain: torus.minor_angular_domain,
            major_scale: torus.major_scale,
            minor_scale: torus.minor_scale,
        };
        ctx.push_vec(&mut tori, value, "catia_native_tori")?;
    }
    Ok(tori)
}

pub(super) fn consolidated_circles(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedCircle>, CodecError> {
    let mut circles = Vec::new();
    for (index, circle) in ctx
        .admit_iter(records, "catia_b2_family_record_scan")?
        .filter_map(|record| crate::families::b2::records::b2_circle_from_record(bytes, record))
        .enumerate()
    {
        let value = CatiaConsolidatedCircle {
            id: ctx.format_retained(
                format_args!("catia:consolidated:circle#{index:00}"),
                "catia_native_circle_id",
            )?,
            byte_offset: u64_from_index(circle.pos),
            layout: circle.layout,
            record_id: circle.record_id,
            frame_token: circle.frame_token,
            center_pair: circle.center_pair,
            radius: circle.radius,
            range: circle.range,
            chart_shift: circle.chart_shift,
        };
        ctx.push_vec(&mut circles, value, "catia_native_circles")?;
    }
    Ok(circles)
}

pub(super) fn consolidated_cones(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedCone>, CodecError> {
    let mut cones = Vec::new();
    for (index, cone) in
        crate::families::b2::records::b2_cones_from_records(ctx, bytes, records)?.enumerate()
    {
        let value = CatiaConsolidatedCone {
            id: ctx.format_retained(
                format_args!("catia:consolidated:cone#{index:00}"),
                "catia_native_cone_id",
            )?,
            byte_offset: u64_from_index(cone.pos),
            apex: cone.apex.coordinates().into(),
            direction_x: cone.frame.reference(),
            direction_y: cone.t2,
            axis: cone.frame.axis(),
            half_angle: cone.half_angle,
            reference_radius: cone.reference_radius,
            angular_range: cone.angular_range,
            slant_range: cone.slant_range,
            angular_scale: cone.angular_scale,
            angular_domain: cone.angular_domain,
        };
        ctx.push_vec(&mut cones, value, "catia_native_cones")?;
    }
    Ok(cones)
}

pub(super) fn consolidated_parameter_points(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedParameterPoint>, CodecError> {
    use crate::families::b2::records::B2ParameterPointPayload;

    let mut points = Vec::new();
    for (index, point) in
        crate::families::b2::records::b2_parameter_points_from_records(ctx, bytes, records)?
            .enumerate()
    {
        let payload = match point.payload {
            B2ParameterPointPayload::Scalar { value } => {
                CatiaConsolidatedParameterPointPayload::Scalar { value }
            }
            B2ParameterPointPayload::Uv { uv } => CatiaConsolidatedParameterPointPayload::Uv { uv },
            B2ParameterPointPayload::StationUv { station, uv } => {
                CatiaConsolidatedParameterPointPayload::StationUv { station, uv }
            }
            B2ParameterPointPayload::FiveScalars { values } => {
                CatiaConsolidatedParameterPointPayload::FiveScalars { values }
            }
        };
        let value = CatiaConsolidatedParameterPoint {
            id: ctx.format_retained(
                format_args!("catia:consolidated:parameter-point#{index:00}"),
                "catia_native_parameter_point_id",
            )?,
            byte_offset: u64_from_index(point.pos),
            byte_len: u64_from_index(point.end - point.pos),
            prefix: point.prefix,
            control: point.control,
            payload,
        };
        ctx.push_vec(&mut points, value, "catia_native_parameter_points")?;
    }
    Ok(points)
}

pub(super) fn consolidated_plane_carriers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedPlaneCarrier>, CodecError> {
    use crate::families::b2::records::B2PlaneCarrierPayload;

    let (carriers, _storage) = ctx
        .with_scoped_storage("catia_native_plane_carrier_scratch", || {
            crate::families::b2::records::b2_plane_carriers_from_records(ctx, bytes, records)
        })?;
    ctx.try_collect_vec(
        carriers
            .into_iter()
            .enumerate()
            .map(|(index, carrier)| -> Result<_, CodecError> {
                let payload = match carrier.payload {
                    B2PlaneCarrierPayload::PointDirection2 {
                        origin,
                        frame,
                        tail,
                    } => {
                        let [x, y, _] = origin.coordinates();
                        let direction = frame.reference().as_raw();
                        CatiaConsolidatedPlaneCarrierPayload::PointDirection2 {
                            point: [x, y].into(),
                            direction: [direction.x, direction.y],
                            tail,
                        }
                    }
                    B2PlaneCarrierPayload::PointDirection3 {
                        origin,
                        direction,
                        tail,
                        ..
                    } => {
                        let [x, y, _] = origin.coordinates();
                        CatiaConsolidatedPlaneCarrierPayload::PointDirection3 {
                            point: [x, y].into(),
                            direction,
                            tail,
                        }
                    }
                    B2PlaneCarrierPayload::PointTail { point, tail } => {
                        CatiaConsolidatedPlaneCarrierPayload::PointTail { point, tail }
                    }
                    B2PlaneCarrierPayload::ScalarLane { selector, values } => {
                        CatiaConsolidatedPlaneCarrierPayload::ScalarLane {
                            selector,
                            values: ctx.copy_slice(&values, "catia_native_plane_scalar_values")?,
                        }
                    }
                };
                Ok(CatiaConsolidatedPlaneCarrier {
                    id: ctx.format_retained(
                        format_args!("catia:consolidated:plane-carrier#{index:00}"),
                        "catia_native_plane_carrier_id",
                    )?,
                    byte_offset: u64_from_index(carrier.pos),
                    byte_len: u64_from_index(carrier.end - carrier.pos),
                    width: carrier.width,
                    flag: carrier.flag,
                    header_token: carrier.header_token,
                    payload,
                })
            }),
        "catia_native_plane_carriers",
    )
}

pub(super) fn consolidated_reference_lists(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedReferenceList>, CodecError> {
    let (lists, _storage) = ctx
        .with_scoped_storage("catia_native_reference_list_scratch", || {
            crate::families::b2::records::b2_reference_lists_from_records(ctx, bytes, records)
        })?;
    ctx.try_collect_vec(
        lists
            .into_iter()
            .enumerate()
            .map(|(index, list)| -> Result<_, CodecError> {
                Ok(CatiaConsolidatedReferenceList {
                    id: ctx.format_retained(
                        format_args!("catia:consolidated:reference-list#{index:00}"),
                        "catia_native_reference_list_id",
                    )?,
                    byte_offset: u64_from_index(list.pos),
                    references: ctx
                        .copy_slice(&list.references, "catia_native_reference_list_values")?,
                })
            }),
        "catia_native_reference_lists",
    )
}

pub(crate) fn consolidated_owner_packets(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedOwnerPacket>, CodecError> {
    // Packet rows and the chart, target, cycle, face-node and position indexes
    // are dropped once the packets are built.
    let mut scratch = ctx.reserve_scoped(0, "catia_native_owner_packet_scratch")?;
    let fixed = scratch.with_storage(|| {
        ctx.collect_vec(
            crate::families::b2::records::b2_owner_packets_from_records(ctx, bytes, records)?,
            "catia_native_fixed_owner_packets",
        )
    })?;
    let mut owner_charts = HashMap::new();
    let owner_chart_rows = scratch.with_storage(|| {
        crate::families::b2::records::b2_owner_charts_from_records(ctx, bytes, records)
    })?;
    for chart in ctx.admit_iter(&owner_chart_rows, "catia_native_owner_chart_visits")? {
        let native_reference =
            |reference: crate::families::b2::records::B2OwnerChartBridgeReference| {
                CatiaOwnerChartBridgeReference::new(
                    reference.value,
                    native_allocation_reference_encoding(reference.encoding),
                )
            };
        let key = (chart.source_index, chart.owner_pos);
        let value = CatiaOwnerChartRelation {
            carrier_byte_offset: u64_from_index(chart.carrier_pos),
            carrier: match chart.carrier {
                crate::families::b2::records::B2OwnerChartCarrier::B28 => {
                    CatiaOwnerChartCarrier::B28
                }
                crate::families::b2::records::B2OwnerChartCarrier::B2b => {
                    CatiaOwnerChartCarrier::B2b
                }
                crate::families::b2::records::B2OwnerChartCarrier::A32 => {
                    CatiaOwnerChartCarrier::A32
                }
            },
            bridge: match &chart.bridge {
                crate::families::b2::records::B2OwnerChartBridge::SupportedSurface {
                    pos,
                    carrier_surface,
                    support_surfaces,
                    support_pcurves,
                    middle_controls,
                    terminal_control,
                    construction_radius,
                } => CatiaOwnerChartBridge::SupportedSurface {
                    byte_offset: u64_from_index(*pos),
                    carrier_surface: native_reference(*carrier_surface),
                    support_surfaces: (*support_surfaces).map(native_reference),
                    support_pcurves: (*support_pcurves).map(native_reference),
                    middle_controls: *middle_controls,
                    terminal_control: *terminal_control,
                    construction_radius: *construction_radius,
                },
                crate::families::b2::records::B2OwnerChartBridge::Extended { pos, references } => {
                    CatiaOwnerChartBridge::Extended {
                        byte_offset: u64_from_index(*pos),
                        references: (*references).map(native_reference),
                    }
                }
            },
            parameter_point_byte_offsets: chart.parameter_point_offsets().map(u64_from_index),
        };
        scratch.with_storage(|| {
            ctx.insert_hash_map(&mut owner_charts, key, value, "catia_native_owner_charts")
        })?;
    }
    let mut identity_targets = HashMap::<(usize, usize), Vec<_>>::new();
    let identity_target_rows = scratch.with_storage(|| {
        crate::families::b2::records::b2_owner_identity_targets_from_records(ctx, bytes, records)
    })?;
    for target in ctx.admit_iter(
        &identity_target_rows,
        "catia_native_owner_identity_target_visits",
    )? {
        let key = (target.source_index, target.owner_pos);
        scratch.with_storage(|| {
            ctx.push_hash_group(
                &mut identity_targets,
                key,
                target,
                "catia_native_owner_identity_target_groups",
                "catia_native_owner_identity_target_entries",
            )
        })?;
    }
    let mut boundary_cycles = HashMap::new();
    let boundary_cycle_rows = scratch.with_storage(|| {
        crate::families::consolidated::records::consolidated_owner_boundary_cycles_from_records(
            ctx, bytes, records,
        )
    })?;
    for cycle in ctx.admit_iter(
        &boundary_cycle_rows,
        "catia_native_owner_boundary_cycle_visits",
    )? {
        let key = (cycle.source_index, cycle.owner_pos);
        let value = CatiaOwnerBoundaryCycle {
                    face_node: cycle.face_node.and_then(|face_node| {
                        let byte_len = cycle.owner_pos.checked_sub(face_node.pos)?;
                        Some(CatiaFaceNodeRelation {
                            byte_offset: u64_from_index(face_node.pos),
                            byte_len: u64_from_index(byte_len),
                            header_token: face_node.header_token,
                            target_encoding: match face_node.target_encoding {
                                crate::families::b2::records::B2FaceNode5fTargetEncoding::Compact => {
                                    CatiaFaceNodeTargetEncoding::Compact
                                }
                                crate::families::b2::records::B2FaceNode5fTargetEncoding::TaggedU16Strong => {
                                    CatiaFaceNodeTargetEncoding::TaggedU16Strong
                                }
                            },
                            target: face_node.target,
                            terminal: face_node.terminal,
                        })
                    }),
                    edges: cycle.edges.map(|edge| CatiaOwnerBoundaryEdge {
                        slot: edge.slot,
                        byte_offset: u64_from_index(edge.target_pos),
                        endpoint_records: edge.endpoint_records.map(u64_from_index),
                    }),
                };
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut boundary_cycles,
                key,
                value,
                "catia_native_owner_boundary_cycles",
            )
        })?;
    }
    let adjacent_counted = scratch.with_storage(|| {
        crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(
            ctx, bytes, records,
        )
    })?;
    let mut face_nodes = HashMap::new();
    let adjacent_face_owners = scratch.with_storage(|| {
        crate::families::b2::records::b2_adjacent_face_owners_from_records(ctx, bytes, records)
    })?;
    for linked in ctx.admit_iter(
        &adjacent_face_owners,
        "catia_native_adjacent_face_owner_visits",
    )? {
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut face_nodes,
                (linked.owner.source_index, linked.owner.pos),
                linked.face_node,
                "catia_native_owner_face_nodes",
            )
        })?;
    }
    for linked in ctx.admit_iter(
        &adjacent_counted,
        "catia_native_adjacent_counted_owner_visits",
    )? {
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut face_nodes,
                (linked.owner.source_index, linked.owner.pos),
                linked.face_node,
                "catia_native_owner_face_nodes",
            )
        })?;
    }
    let mut fixed_positions = HashSet::new();
    for packet in ctx.admit_iter(&fixed, "catia_native_fixed_owner_position_visits")? {
        scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut fixed_positions,
                (packet.source_index, packet.pos),
                "catia_native_fixed_owner_positions",
            )
        })?;
    }
    let counted_owners = scratch.with_storage(|| {
        crate::families::b2::records::b2_counted_owners_from_records(ctx, bytes, records)
    })?;
    let row_count = fixed
        .len()
        .checked_add(counted_owners.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit("catia_native_owner_packet_rows", u64::MAX, u64::MAX)
        })?;
    let mut packets =
        scratch.with_storage(|| ctx.collection_vec(row_count, "catia_native_owner_packet_rows"))?;
    for packet in ctx.admit_iter(fixed, "catia_native_fixed_owner_packet_rows")? {
        packets.push((
            packet.pos,
            packet.source_index,
            packet.header_token,
            CatiaOwnerPacketPayload::FixedNine {
                reference_encoding: match packet.reference_encoding {
                    crate::families::b2::records::B2OwnerReferenceEncoding::TaggedU16Strong => {
                        CatiaOwnerReferenceEncoding::TaggedU16Strong
                    }
                    crate::families::b2::records::B2OwnerReferenceEncoding::WidthCodedStrong => {
                        CatiaOwnerReferenceEncoding::WidthCodedStrong
                    }
                    crate::families::b2::records::B2OwnerReferenceEncoding::AllCompact => {
                        CatiaOwnerReferenceEncoding::AllCompact
                    }
                },
                references: packet.references,
                identity_encodings: packet.identity_encodings.map(|encoding| match encoding {
                    crate::families::b2::records::B2OwnerIdentityEncoding::Allocation(encoding) => {
                        CatiaOwnerIdentityEncoding::Allocation(
                            native_allocation_reference_encoding(encoding),
                        )
                    }
                    crate::families::b2::records::B2OwnerIdentityEncoding::RawU8 => {
                        CatiaOwnerIdentityEncoding::RawU8
                    }
                }),
                numeric_tail: packet.numeric_tail,
                identity_targets: Vec::new(),
                owner_chart: None,
                boundary_cycle: None,
            },
        ));
    }
    for packet in ctx.admit_iter(counted_owners, "catia_native_counted_owner_packet_rows")? {
        if ctx.contains_hash_set(
            &fixed_positions,
            &(packet.source_index, packet.pos),
            "catia_native_fixed_owner_positions",
        )? {
            continue;
        }
        packets.push((
            packet.pos,
            packet.source_index,
            packet.header_token,
            CatiaOwnerPacketPayload::Counted {
                references: ctx
                    .copy_slice(&packet.references, "catia_native_owner_counted_references")?,
                tail: crate::families::b2::counted_owner_tail::CountedOwnerTail::new(
                    ctx.copy_slice(packet.tail.as_slice(), "catia_native_owner_counted_tail")?,
                )
                .ok_or_else(|| CodecError::malformed("CATIA counted owner tail is empty"))?,
            },
        ));
    }
    ctx.stable_sort_by_key(
        &mut packets,
        |value| (value.0, value.1),
        Ord::cmp,
        "catia_native_owner_packet_sort",
    )?;
    ctx.try_collect_vec(
        packets
            .into_iter()
            .map(|(pos, source_index, header_token, mut payload)| -> Result<_, CodecError> {
                if let CatiaOwnerPacketPayload::FixedNine {
                    identity_targets: stored_targets,
                    owner_chart,
                    boundary_cycle,
                    ..
                } = &mut payload
                {
                    const LOOKUP: &str = "catia_native_owner_packet_lookups";
                    let key = (source_index, pos);
                    if let Some(targets) = ctx.remove_hash_map(&mut identity_targets, &key, LOOKUP)? {
                        *stored_targets = ctx.collect_vec(
                            targets.into_iter().map(|target| CatiaOwnerIdentityTarget {
                                slot: target.slot, distance: target.distance,
                                target_byte_offset: u64_from_index(target.target_pos), target_class: target.target_class,
                            }), "catia_native_owner_emitted_targets",
                        )?;
                    }
                    *owner_chart = ctx
                        .remove_hash_map(&mut owner_charts, &key, LOOKUP)?
                        .map(|value| -> Result<_, CodecError> {
                            ctx.charge_retained(u64_from_index(std::mem::size_of_val(&value)), "catia_native_owner_packet_box")?;
                            Ok(Box::new(value))
                        }).transpose()?;
                    *boundary_cycle = ctx
                        .get_hash_map(&boundary_cycles, &key, LOOKUP)?
                        .copied()
                        .map(|value| -> Result<_, CodecError> {
                            ctx.charge_retained(u64_from_index(std::mem::size_of_val(&value)), "catia_native_owner_packet_box")?;
                            Ok(Box::new(value))
                        }).transpose()?;
                }
                Ok(CatiaConsolidatedOwnerPacket {
                    id: ctx.format_retained(
                        format_args!("catia:consolidated:owner-packet#{pos:010}"),
                        "catia_native_owner_packet_id",
                    )?,
                    byte_offset: u64_from_index(pos),
                    source_index,
                    header_token,
                    payload,
                    face_node: ctx
                        .get_hash_map(
                            &face_nodes,
                            &(source_index, pos),
                            "catia_native_owner_packet_lookups",
                        )?
                        .and_then(|face_node| {
                            let byte_len = pos.checked_sub(face_node.pos)?;
                            Some(CatiaFaceNodeRelation {
                                byte_offset: u64_from_index(face_node.pos),
                                byte_len: u64_from_index(byte_len),
                                header_token: face_node.header_token,
                                target_encoding: match face_node.target_encoding {
                                    crate::families::b2::records::B2FaceNode5fTargetEncoding::Compact => {
                                        CatiaFaceNodeTargetEncoding::Compact
                                    }
                                    crate::families::b2::records::B2FaceNode5fTargetEncoding::TaggedU16Strong => {
                                        CatiaFaceNodeTargetEncoding::TaggedU16Strong
                                    }
                                },
                                target: face_node.target,
                                terminal: face_node.terminal,
                            })
                        }),
                })
            }),
        "catia_native_owner_packet_output",
    )
}

pub(crate) fn consolidated_edge_runs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    pcurves: &[CatiaConsolidatedPcurve],
    nodes: &[CatiaConsolidatedEdgeNode],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<CatiaConsolidatedEdgeRun>, CodecError> {
    const LOOKUP: &str = "catia_native_edge_run_lookups";
    let mut lookup_storage = ctx.reserve_scoped(0, "CATIA native edge run lookup")?;
    let mut pcurve_ids = HashMap::new();
    for pcurve in ctx.admit_iter(pcurves, "catia_native_edge_run_pcurve_visits")? {
        lookup_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut pcurve_ids,
                pcurve.byte_offset,
                pcurve.id.as_str(),
                "catia_native_edge_run_pcurve_index",
            )
        })?;
    }
    let resolved_blocks = lookup_storage.with_storage(|| {
        crate::families::consolidated::records::resolve_consolidated_edge_blocks_from_records(
            ctx, bytes, records, refusal,
        )
    })?;
    let mut resolved = HashMap::new();
    for block in ctx.admit_iter(&resolved_blocks, "catia_native_resolved_edge_block_visits")? {
        lookup_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut resolved,
                block.block.pcurves[0].pos,
                block,
                "catia_native_edge_run_resolved_index",
            )
        })?;
    }
    let mut nodes_by_offset = HashMap::new();
    for node in ctx.admit_iter(nodes, "catia_native_edge_run_node_visits")? {
        lookup_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut nodes_by_offset,
                node.byte_offset,
                node,
                "catia_native_edge_run_node_index",
            )
        })?;
    }
    let mut output = Vec::new();
    let topology_edge_runs = lookup_storage.with_storage(|| {
        crate::families::consolidated::records::consolidated_topology_edge_runs_from_records(
            ctx, bytes, records,
        )
    })?;
    for (index, run) in ctx
        .admit_iter(&topology_edge_runs, "catia_native_topology_edge_run_visits")?
        .enumerate()
    {
        let pcurve_offsets = run
            .edge
            .pcurves
            .each_ref()
            .map(|pcurve| u64_from_index(pcurve.pos));
        let resolved = ctx.get_hash_map(&resolved, &run.edge.pcurves[0].pos, LOOKUP)?;
        let Some(node) =
            ctx.get_hash_map(&nodes_by_offset, &u64_from_index(run.node.pos), LOOKUP)?
        else {
            continue;
        };
        if node.uses.is_none() {
            continue;
        }
        let (Some(first), Some(second)) = (
            ctx.get_hash_map(&pcurve_ids, &pcurve_offsets[0], LOOKUP)?,
            ctx.get_hash_map(&pcurve_ids, &pcurve_offsets[1], LOOKUP)?,
        ) else {
            continue;
        };
        let shared_loci = resolved
            .and_then(|resolved| resolved.shared_loci.as_ref())
            .map(|points| {
                ctx.collect_vec(
                    points.iter().map(point_coordinates),
                    "catia_native_edge_run_shared_loci",
                )
            })
            .transpose()?;
        let value = CatiaConsolidatedEdgeRun {
            id: ctx.format_retained(
                format_args!("catia:consolidated:edge-run#{index:01}"),
                "catia_native_edge_run_id",
            )?,
            byte_offset: pcurve_offsets[0],
            pcurves: [
                ctx.copy_retained_text(first, "catia_native_edge_run_first_pcurve_id")?,
                ctx.copy_retained_text(second, "catia_native_edge_run_second_pcurve_id")?,
            ],
            parameter_range: run.edge.parameters.range,
            tolerance: run.edge.parameters.tolerance,
            node: ctx.copy_retained_text(&node.id, "catia_native_edge_run_node_id")?,
            support_bindings: resolved.map_or([None, None], |resolved| {
                resolved
                    .supports
                    .each_ref()
                    .map(|binding| binding.as_ref().map(native_consolidated_support_binding))
            }),
            shared_loci,
            endpoint_loci: resolved
                .and_then(|resolved| resolved.endpoint_loci.as_ref())
                .map(|points| points.map(|point| point_coordinates(&point))),
        };
        ctx.push_vec(&mut output, value, "catia_native_edge_runs")?;
    }
    Ok(output)
}

pub(crate) fn consolidated_edge_nodes(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    circles: &[CatiaConsolidatedCircle],
) -> Result<Vec<CatiaConsolidatedEdgeNode>, CodecError> {
    let mut temporary = ctx.reserve_scoped(0, "catia native edge node workspace")?;
    let mut circle_ids = HashMap::new();
    for circle in ctx.admit_iter(circles, "catia_native_edge_circle_visits")? {
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut circle_ids,
                circle.byte_offset,
                circle.id.as_str(),
                "catia_native_edge_circle_ids",
            )
        })?;
    }
    let mut frames = HashMap::new();
    for record in ctx
        .admit_iter(records, "catia_native_edge_frame_record_visits")?
        .filter(|record| {
            record.family() == crate::wire::records::ConsolidatedFamily::B && record.class() == 0x5e
        })
    {
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut frames,
                record.byte_offset(),
                (record.width(), record.flag(), record.source_index()),
                "catia_native_edge_frames",
            )
        })?;
    }
    let mut owned_nodes = HashMap::new();
    let owned_node_rows = temporary.with_storage(|| {
        crate::families::consolidated::records::consolidated_owned_edge_nodes_from_records(
            ctx, bytes, records,
        )
    })?;
    for owned in ctx.admit_iter(&owned_node_rows, "catia_native_owned_edge_node_visits")? {
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut owned_nodes,
                owned.node.pos,
                (owned.owner_pos, owned.allocation_ordinal),
                "catia_native_owned_edge_nodes",
            )
        })?;
    }
    let mut compact_endpoints = HashMap::new();
    let compact_endpoint_bindings = temporary.with_storage(|| {
        crate::families::consolidated::records::consolidated_compact_edge_endpoints_from_records(
            ctx, bytes, records,
        )
    })?;
    for binding in ctx.admit_iter(
        &compact_endpoint_bindings,
        "catia_native_compact_endpoint_visits",
    )? {
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut compact_endpoints,
                binding.node.pos,
                binding.endpoint_records.map(u64_from_index),
                "catia_native_edge_compact_endpoints",
            )
        })?;
    }
    let mut use_runs = HashMap::new();
    let use_rows = temporary.with_storage(|| {
        crate::families::consolidated::records::consolidated_edge_use_runs_from_records(
            ctx, bytes, records,
        )
    })?;
    for run in ctx.admit_iter(&use_rows, "catia_native_edge_use_run_visits")? {
        let Some(uses) = native_consolidated_edge_uses(&run.uses) else {
            continue;
        };
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut use_runs,
                run.node.pos,
                (uses, run.definition.as_ref()),
                "catia_native_edge_use_runs",
            )
        })?;
    }
    let mut analytic_circles = HashMap::new();
    let analytic_rows = temporary.with_storage(|| {
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs_from_records(
            ctx, bytes, records,
        )
    })?;
    for run in ctx.admit_iter(&analytic_rows, "catia_native_analytic_edge_run_visits")? {
        let Some(circle) = ctx.get_hash_map(
            &circle_ids,
            &u64_from_index(run.circle.pos),
            "catia_native_edge_circle_ids",
        )?
        else {
            continue;
        };
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut analytic_circles,
                run.node.pos,
                (&run.descriptor, *circle),
                "catia_native_analytic_edge_bindings",
            )
        })?;
    }
    let mut class25_descriptors = temporary.with_storage(|| {
        ctx.collect_hash_map(
            crate::families::consolidated::records::consolidated_class25_edge_runs_from_records(
                ctx, bytes, records,
            )?
            .into_iter()
            .map(|run| {
                (
                    run.node.pos,
                    CatiaConsolidatedClass25Descriptor {
                        byte_offset: u64_from_index(run.descriptor.pos),
                        record_id: run.descriptor.record_id,
                        control: run.descriptor.control,
                        values: run.descriptor.values,
                    },
                )
            }),
            "catia_native_class25_edge_descriptors",
        )
    })?;
    let mut output = Vec::new();
    for (index, node) in
        crate::families::b2::records::b2_edge_nodes_from_records(ctx, bytes, records)?.enumerate()
    {
        const LOOKUP: &str = "catia_native_edge_node_lookups";
        let Some(&(width, flag, source_index)) = ctx.get_hash_map(&frames, &node.pos, LOOKUP)?
        else {
            continue;
        };
        ctx.reserve_vec(&mut output, 1, "catia_native_consolidated_edge_nodes")?;
        let allocation = ctx
            .get_hash_map(&owned_nodes, &node.pos, LOOKUP)?
            .map(|(pos, ordinal)| {
                Ok::<_, CodecError>((
                    ctx.format_retained(
                        format_args!("catia:consolidated:owner-packet#{:010}", *pos),
                        "catia_native_edge_owner_id",
                    )?,
                    *ordinal,
                ))
            })
            .transpose()?;
        let (uses, definition) = match ctx.remove_hash_map(&mut use_runs, &node.pos, LOOKUP)? {
            Some((uses, definition)) => (Some(uses), definition),
            None => (None, None),
        };
        let definition = definition
            .map(|source| -> Result<_, CodecError> {
                let frame = crate::wire::records::ConsolidatedRawFrame::new(
                    source.frame.pos,
                    source.frame.width(),
                    source.frame.flag,
                    source.frame.header_token(),
                    ctx.copy_slice(
                        &source.frame.payload,
                        "catia_native_edge_definition_payload",
                    )?,
                )
                .map_err(CodecError::malformed)?;
                CatiaConsolidatedEdgeDefinition::from_source(
                    ctx,
                    crate::families::consolidated::records::ConsolidatedEdgeDefinition {
                        frame,
                        class: source.class,
                    },
                )
            })
            .transpose()?;
        let analytic_circle = ctx
            .remove_hash_map(&mut analytic_circles, &node.pos, LOOKUP)?
            .map(|(descriptor, circle)| -> Result<_, CodecError> {
                Ok(CatiaConsolidatedAnalyticCircleBinding {
                    descriptor: crate::wire::records::ConsolidatedRawFrame::new(
                        u64_from_index(descriptor.pos),
                        descriptor.width(),
                        descriptor.flag,
                        descriptor.header_token(),
                        ctx.copy_slice(
                            &descriptor.payload,
                            "catia_native_analytic_edge_descriptor",
                        )?,
                    )
                    .map_err(CodecError::malformed)?,
                    circle: ctx
                        .copy_retained_text(circle, "catia_native_analytic_edge_circle_id")?,
                })
            })
            .transpose()?;
        output.push(CatiaConsolidatedEdgeNode {
            id: ctx.format_retained(
                format_args!("catia:consolidated:edge-node#{index:01}"),
                "catia_native_edge_node_id",
            )?,
            byte_offset: u64_from_index(node.pos),
            source_index,
            token: crate::wire::records::WidthCodedToken::new(width, node.header_token)
                .map_err(CodecError::malformed)?,
            flag,
            allocation,
            curve_ref: node.curve_ref,
            vertex_refs: [node.start_vertex_ref, node.end_vertex_ref],
            endpoint_records: ctx
                .get_hash_map(&compact_endpoints, &node.pos, LOOKUP)?
                .copied(),
            parameter_selectors: [node.start_parameter_ref, node.end_parameter_ref],
            reference_encodings: node
                .reference_encodings
                .map(native_allocation_reference_encoding),
            terminal_value: node.terminal_value,
            terminal_encoding: native_allocation_reference_encoding(node.terminal_encoding),
            tail: node.tail,
            definition,
            uses,
            analytic_circle,
            class25_descriptor: ctx.remove_hash_map(&mut class25_descriptors, &node.pos, LOOKUP)?,
        });
    }
    Ok(output)
}

pub(super) fn native_allocation_reference_encoding(
    encoding: crate::wire::bytes::AllocationReferenceEncoding,
) -> CatiaAllocationReferenceEncoding {
    match encoding {
        crate::wire::bytes::AllocationReferenceEncoding::BackwardDistance => {
            CatiaAllocationReferenceEncoding::BackwardDistance
        }
        crate::wire::bytes::AllocationReferenceEncoding::OwnedChild => {
            CatiaAllocationReferenceEncoding::OwnedChild
        }
        crate::wire::bytes::AllocationReferenceEncoding::WidthCoded => {
            CatiaAllocationReferenceEncoding::WidthCoded
        }
        crate::wire::bytes::AllocationReferenceEncoding::Selector2 => {
            CatiaAllocationReferenceEncoding::Selector2
        }
        crate::wire::bytes::AllocationReferenceEncoding::TaggedU8 => {
            CatiaAllocationReferenceEncoding::TaggedU8
        }
        crate::wire::bytes::AllocationReferenceEncoding::TaggedU16 => {
            CatiaAllocationReferenceEncoding::TaggedU16
        }
    }
}

pub(super) fn native_consolidated_edge_uses(
    uses: &[crate::families::b2::records::B2UseMetadata; 2],
) -> Option<CatiaConsolidatedEdgeUses> {
    let [Some(first), Some(second)] = uses
        .each_ref()
        .map(|use_| use_.references()?.try_into().ok())
    else {
        return None;
    };
    let references = [first, second];
    let [Some(first_sense), Some(second_sense)] =
        uses.each_ref().map(|use_| match use_.sense()? {
            crate::families::b2::records::B2UseSense::Sense84 => Some(0x84),
            crate::families::b2::records::B2UseSense::Sense88 => Some(0x88),
        })
    else {
        return None;
    };
    ([first_sense, second_sense] == [0x88, 0x84])
        .then_some(CatiaConsolidatedEdgeUses { references })
}

pub(crate) fn point_coordinates(point: &cadmpeg_ir::math::Point3) -> [f64; 3] {
    [point.x, point.y, point.z]
}

pub(crate) fn native_consolidated_support_binding(
    binding: &crate::families::consolidated::records::ConsolidatedSupportBinding,
) -> CatiaConsolidatedSupportBinding {
    match binding {
        crate::families::consolidated::records::ConsolidatedSupportBinding::Cylinder { pos } => {
            CatiaConsolidatedSupportBinding::Cylinder {
                byte_offset: u64_from_index(*pos),
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::EmbeddedCylinder {
            pos,
            wrapper_pos,
        } => CatiaConsolidatedSupportBinding::EmbeddedCylinder {
            byte_offset: u64_from_index(*pos),
            wrapper_byte_offset: u64_from_index(*wrapper_pos),
        },
        crate::families::consolidated::records::ConsolidatedSupportBinding::Circle { pos } => {
            CatiaConsolidatedSupportBinding::Circle {
                byte_offset: u64_from_index(*pos),
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Cone { pos } => {
            CatiaConsolidatedSupportBinding::Cone {
                byte_offset: u64_from_index(*pos),
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Sphere { pos } => {
            CatiaConsolidatedSupportBinding::Sphere {
                byte_offset: u64_from_index(*pos),
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Torus { pos } => {
            CatiaConsolidatedSupportBinding::Torus {
                byte_offset: u64_from_index(*pos),
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Plane { pos } => {
            CatiaConsolidatedSupportBinding::Plane {
                byte_offset: u64_from_index(*pos),
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::NurbsCarrier {
            pos,
            offset,
        } => CatiaConsolidatedSupportBinding::NurbsCarrier {
            byte_offset: u64_from_index(*pos),
            offset: *offset,
        },
    }
}

pub(crate) fn zero_entity_record(
    records: &[CatiaZeroEntityRecord],
    ordinal: u32,
) -> Option<&CatiaZeroEntityRecord> {
    let index = usize::try_from(ordinal.checked_sub(1)?).ok()?;
    records.get(index)
}

pub(crate) fn zero_entity_vertex_owner(
    records: &[CatiaZeroEntityRecord],
    incidence_ordinal: u32,
) -> Option<&CatiaZeroEntityRecord> {
    let incidence = zero_entity_record(records, incidence_ordinal)?;
    let owner = zero_entity_record(records, incidence_ordinal.checked_add(1)?)?;
    (incidence.logical_end == owner.byte_offset && owner.tag == [0x5d, 0x06]).then_some(owner)
}

pub(crate) fn finjpl_family(kind: container::FinjplKind) -> &'static str {
    match kind {
        container::FinjplKind::Storage => "storage",
        container::FinjplKind::ProjectFlags => "project-flags",
        container::FinjplKind::Other => "other",
    }
}

/// The one segment containing an extent. Segments are disjoint and in offset
/// order, each running from one marker to the next, so only the last segment
/// starting at or before the extent can contain it, together with its
/// predecessor when the extent is empty and on their shared boundary.
pub(crate) fn containing_finjpl_segment<'s>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    byte_offset: u64,
    byte_len: u64,
    segments: &'s [CatiaFinjplSegment],
) -> Result<Option<&'s str>, CodecError> {
    let Some(byte_end) = byte_offset.checked_add(byte_len) else {
        return Ok(None);
    };
    let contains = |segment: &CatiaFinjplSegment| {
        segment.byte_offset <= byte_offset
            && segment
                .byte_offset
                .checked_add(segment.byte_len)
                .is_some_and(|segment_end| byte_end <= segment_end)
    };
    let after = ctx.partition_point(
        segments,
        |segment| Ok(segment.byte_offset <= byte_offset),
        "catia_native_finjpl_segment_search",
    )?;
    let Some(last) = after.checked_sub(1) else {
        return Ok(None);
    };
    let previous = last
        .checked_sub(1)
        .and_then(|index| segments.get(index))
        .filter(|segment| contains(segment));
    Ok(
        match (
            segments.get(last).filter(|segment| contains(segment)),
            previous,
        ) {
            (Some(segment), None) | (None, Some(segment)) => Some(segment.id.as_str()),
            (Some(_), Some(_)) | (None, None) => None,
        },
    )
}

pub(crate) fn preview_views(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    segments: &[CatiaFinjplSegment],
) -> Result<Vec<CatiaPreviewImage>, cadmpeg_core::CodecError> {
    let mut views = Vec::new();
    for segment in ctx.admit_iter(segments, "catia_native_preview_segment_visits")? {
        let (previews, _storage) = ctx
            .with_scoped_storage("catia_native_preview_scratch", || {
                container::preview_images(ctx, &segment.data)
            })?;
        for preview in ctx.admit_iter(&previews, "catia_native_preview_item_visits")? {
            let Some(byte_offset) = segment
                .byte_offset
                .checked_add(u64_from_index(preview.range.start))
            else {
                continue;
            };
            let id = ctx.format_retained(
                format_args!("catia:outer:preview#{}", views.len()),
                "catia_native_preview_id",
            )?;
            let data = ctx.copy_slice(
                &segment.data[preview.range.clone()],
                "catia_native_preview_bytes",
            )?;
            ctx.push_vec(
                &mut views,
                CatiaPreviewImage {
                    id,
                    byte_offset,
                    byte_len: u64_from_index(preview.range.end - preview.range.start),
                    width: preview.width,
                    height: preview.height,
                    components: preview.components,
                    data,
                },
                "catia_native_preview_views",
            )?;
        }
    }
    Ok(views)
}

pub(crate) fn external_reference_views(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    segments: &[CatiaFinjplSegment],
) -> Result<Vec<CatiaExternalReference>, cadmpeg_core::CodecError> {
    let mut views = Vec::new();
    for segment in ctx.admit_iter(segments, "catia_native_external_reference_segment_visits")? {
        let (references, _storage) = ctx
            .with_scoped_storage("catia_native_external_reference_scratch", || {
                container::external_references(ctx, &segment.data)
            })?;
        for reference in ctx.admit_iter(references, "catia_native_external_reference_visits")? {
            let Some(byte_offset) = segment
                .byte_offset
                .checked_add(u64_from_index(reference.offset))
            else {
                continue;
            };
            let id = ctx.format_retained(
                format_args!("catia:outer:external-reference#{}", views.len()),
                "catia_native_external_reference_id",
            )?;
            let segment_id =
                ctx.copy_retained_text(&segment.id, "catia_native_external_reference_segment")?;
            ctx.push_vec(
                &mut views,
                CatiaExternalReference {
                    id,
                    byte_offset,
                    target: ctx.copy_retained_text(
                        &reference.target,
                        "catia_native_external_reference_target",
                    )?,
                    segment: segment_id,
                },
                "catia_native_external_reference_views",
            )?;
        }
    }
    Ok(views)
}

pub(crate) fn resolve_alias_surface_tags(
    ctx: &DecodeContext<'_>,
    rows: &mut [CatiaAliasRow],
) -> Result<(), CodecError> {
    // The stored tag of each group, or `None` when several rows store one.
    let (stored_by_group, _storage) = ctx.unique_index(
        ctx.admit_iter(&*rows, "catia_native_alias_tag_read_visits")?
            .filter_map(|row| {
                let group = row.group.as_ref()?;
                (row.lead() == AliasLead::SurfaceSupportStorage)
                    .then_some(((group.prototype, group.group_id), row.tag()))
            }),
        "catia_alias_group_index",
    )?;
    for row in ctx.admit_iter(rows, "catia_native_alias_tag_write_visits")? {
        row.canonical_surface_tag = match row.lead() {
            AliasLead::SurfaceSupportStorage => Some(row.tag()),
            AliasLead::NonSurfaceAlias => match row.group.as_ref() {
                Some(group) => ctx
                    .get_hash_map(
                        &stored_by_group,
                        &(group.prototype, group.group_id),
                        "catia_alias_group_index",
                    )?
                    .copied()
                    .flatten(),
                None => None,
            },
            _ => None,
        };
    }
    Ok(())
}

pub(crate) fn resolve_owner_chart_support_aliases(
    ctx: &DecodeContext<'_>,
    packets: &mut [CatiaConsolidatedOwnerPacket],
    aliases: &[CatiaAliasRow],
) -> Result<(), CodecError> {
    let (unique_by_tag, _storage) = ctx.unique_index(
        aliases.iter().map(|row| (row.tag(), row)),
        "catia_owner_alias_index",
    )?;
    let resolve = |reference: &mut CatiaOwnerChartBridgeReference| -> Result<(), CodecError> {
        if let CatiaOwnerChartAddress::WidthCoded { alias } = &mut reference.address {
            *alias = if let Some(row) = ctx
                .get_hash_map(&unique_by_tag, &reference.value, "catia_owner_alias_index")?
                .copied()
                .flatten()
            {
                let id = ctx.copy_retained_text(&row.id, "catia_owner_alias_binding_id")?;
                cadmpeg_core::text::NonBlankString::for_decode(ctx, id, "validate nonblank text")?
                    .map(|id| CatiaOwnerChartAliasBinding::new(id, row.canonical_surface_tag))
            } else {
                None
            };
        }
        Ok(())
    };
    for packet in ctx.admit_iter(packets, "catia_native_owner_alias_packet_visits")? {
        let Some(chart) = packet.owner_chart_mut() else {
            continue;
        };
        let CatiaOwnerChartBridge::SupportedSurface {
            support_surfaces,
            support_pcurves,
            ..
        } = &mut chart.bridge
        else {
            continue;
        };
        for reference in support_surfaces.iter_mut().chain(support_pcurves) {
            resolve(reference)?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn validate_alias_surface_tags(
    rows: &[CatiaAliasRow],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let mut expected = rows.to_vec();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("validation fixture fits service input limit");
    resolve_alias_surface_tags(&ctx, &mut expected)
        .expect("validation fixture fits service resource limits");
    if rows
        .iter()
        .zip(expected)
        .all(|(row, expected)| row.canonical_surface_tag == expected.canonical_surface_tag)
    {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "alias rows have invalid canonical surface tags".to_string(),
        ))
    }
}

#[cfg(test)]
pub(crate) fn validate_owner_chart_support_aliases(
    packets: &[CatiaConsolidatedOwnerPacket],
    aliases: &[CatiaAliasRow],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let mut expected = packets.to_vec();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("validation fixture fits service input limit");
    resolve_owner_chart_support_aliases(&ctx, &mut expected, aliases)
        .expect("validation fixture fits service resource limits");
    if packets == expected {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "owner-chart support references have invalid alias links".to_string(),
        ))
    }
}

pub(crate) fn value_schema_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    block_id: &str,
    block_byte_offset: u64,
    fields: &[value_block::ValueField],
    catalog: &CatiaCatalog,
) -> Result<Vec<CatiaValueSchemaSelection>, cadmpeg_core::CodecError> {
    let mut selector_storage = ctx.reserve_scoped(0, "catia_value_selector_indices")?;
    let mut selector_indices = Vec::new();
    for (index, field) in ctx
        .admit_iter(fields, "catia_native_value_selector_field_visits")?
        .enumerate()
    {
        let value_block::ValueField::SchemaSelector { ordinal, .. } = field else {
            continue;
        };
        if usize::try_from(*ordinal)
            .ok()
            .is_some_and(|ordinal| ordinal <= catalog.entries.len())
        {
            selector_storage.with_storage(|| {
                ctx.push_vec(&mut selector_indices, index, "catia_value_selector_indices")
            })?;
        }
    }
    let mut selections = Vec::new();
    for (selector_rank, &index) in ctx
        .admit_iter(
            &selector_indices,
            "catia_native_value_selector_index_visits",
        )?
        .enumerate()
    {
        let value_block::ValueField::SchemaSelector { ordinal, offset } = &fields[index] else {
            continue;
        };
        let Some(ordinal_index) = usize::try_from(*ordinal).ok() else {
            continue;
        };
        if ordinal_index > catalog.entries.len() {
            continue;
        }
        let offset = u64_from_index(*offset);
        let Some(byte_offset) = block_byte_offset
            .checked_add(6)
            .and_then(|base| base.checked_add(offset))
        else {
            continue;
        };
        let value_end = selector_indices
            .get(selector_rank + 1)
            .copied()
            .unwrap_or(fields.len());
        let kind = if let Some(entry) = catalog.entries.get(ordinal_index) {
            let mut encoded_value = Vec::new();
            for field in ctx.admit_iter(
                &fields[index + 1..value_end],
                "catia_native_value_selection_field_visits",
            )? {
                let copy = value_block::copy_field_charged(ctx, field)?;
                ctx.push_vec(&mut encoded_value, copy, "catia_value_selection_fields")?;
            }
            CatiaValueSchemaSelectionKind::Selected(CatiaValueSchemaSelectionValue {
                class: CatiaDesignClass {
                    entry: ctx.copy_retained_text(&entry.id, "catia_value_selection_class_id")?,
                    name: ctx
                        .copy_retained_text(&entry.value, "catia_value_selection_class_name")?,
                },
                encoded_value,
            })
        } else {
            CatiaValueSchemaSelectionKind::Terminal
        };
        let id = ctx.format_retained(
            format_args!("catia:outer:value-selection#{byte_offset:010}"),
            "catia_value_selection_id",
        )?;
        let parent = ctx.copy_retained_text(block_id, "catia_value_selection_parent")?;
        ctx.push_vec(
            &mut selections,
            CatiaValueSchemaSelection {
                id,
                parent,
                offset,
                ordinal: *ordinal,
                kind,
            },
            "catia_value_selections",
        )?;
    }
    Ok(selections)
}

impl CatiaValueBlock {
    pub(super) fn from_parts(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        block: &value_block::ValueBlock,
        catalog: &CatiaCatalog,
        object_graph: Option<&CatiaObjectGraph>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let id = ctx.format_retained(
            format_args!("catia:outer:value-block#{:010}", block.pos),
            "catia_value_block_id",
        )?;
        // The tokenized fields are dropped once the selections are built.
        let (fields, _fields_storage) = ctx
            .with_scoped_storage("catia_value_block_fields", || {
                value_block::tokenize_charged(ctx, &block.payload)
            })?;
        let byte_offset = u64_from_index(block.pos);
        let schema_selections = value_schema_selections(ctx, &id, byte_offset, &fields, catalog)?;
        Ok(Self {
            id,
            byte_offset,
            object_graph: object_graph
                .map(|graph| ctx.copy_retained_text(&graph.id, "catia_value_block_graph_id"))
                .transpose()?,
            catalog: ctx.copy_retained_text(&catalog.id, "catia_value_block_catalog_id")?,
            payload: ctx.copy_slice(&block.payload, "catia_native_value_block_payload")?,
            schema_selections,
        })
    }
}

impl CatiaAliasRow {
    pub(super) fn from_source(
        ctx: &DecodeContext<'_>,
        row: object_graph::SurfaceAlias,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: ctx.format_retained(
                format_args!("catia:outer:alias-row#{:010}", row.pos),
                "catia_native_alias_row_id",
            )?,
            byte_offset: u64_from_index(row.pos),
            lead_raw: row.lead_raw,
            tag_raw: row.tag_raw,
            flag: row.flag,
            f1: row.f1,
            object_graph: None,
            object_record: None,
            design_object: None,
            f2: row.f2,
            f3: row.f3,
            group: row.group,
            canonical_surface_tag: None,
        })
    }
}

impl CatiaCatalog {
    pub(super) fn from_source(
        ctx: &DecodeContext<'_>,
        catalog: catalog::Catalog,
    ) -> Result<Self, CodecError> {
        let id = ctx.format_retained(
            format_args!("catia:outer:catalog#{:010}", catalog.pos),
            "catia_native_catalog_id",
        )?;
        let entries = ctx.try_collect_vec(
            catalog
                .entries
                .into_iter()
                .map(|entry| -> Result<_, CodecError> {
                    Ok(CatiaCatalogEntry {
                        id: ctx.format_retained(
                            format_args!("catia:outer:catalog-entry#{:010}", entry.pos),
                            "catia_native_catalog_entry_id",
                        )?,
                        parent: ctx.copy_retained_text(&id, "catia_native_catalog_entry_parent")?,
                        ordinal: entry.ordinal,
                        byte_offset: u64_from_index(entry.pos),
                        value: ctx
                            .copy_retained_text(&entry.value, "catia_native_catalog_entry_value")?,
                    })
                }),
            "catia_native_catalog_entries",
        )?;
        Ok(Self {
            id,
            byte_offset: u64_from_index(catalog.pos),
            byte_len: u64_from_index(catalog.total_len),
            entries,
        })
    }
}

/// Projects one parsed graph and its entity records. The returned record
/// index lives in the caller's scoped storage.
pub(crate) fn native_object_graph(
    ctx: &DecodeContext<'_>,
    index_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    graph: &object_graph::ObjectGraph,
    entity_records: &[entity_table::EntityRecord],
    finjpl_segment: Option<String>,
    outer_container: Option<CatiaOuterContainerBinding>,
) -> Result<(CatiaObjectGraph, Vec<CatiaEntityRecord>, GraphRecordIndex), CodecError> {
    let id = ctx.format_retained(
        format_args!("catia:outer:object-graph#{:010}", graph.pos),
        "catia_native_graph_id",
    )?;
    let mut records = Vec::new();
    for (ordinal, record) in ctx
        .admit_iter(&graph.records, "catia_native_graph_record_visits")?
        .enumerate()
    {
        let entity = entity_records.get(ordinal);
        let roles = record.roles();
        let row = CatiaObjectRecord {
            id: ctx.format_retained(
                format_args!("catia:outer:object-record#{:010}", record.pos),
                "catia_native_record_id",
            )?,
            parent: ctx.copy_retained_text(&id, "catia_native_record_parent")?,
            design_object: None,
            entity: entity
                .map(|entity| -> Result<CatiaObjectEntity, CodecError> {
                    Ok(CatiaObjectEntity {
                        record: ctx.format_retained(
                            format_args!("catia:outer:entity-record#{:010}", entity.pos),
                            "catia_native_record_entity_id",
                        )?,
                        id: entity.entity_id,
                    })
                })
                .transpose()?,
            ordinal: u64_from_index(ordinal),
            byte_offset: u64_from_index(record.pos),
            byte_len: u64_from_index(record.total_len),
            lead: record.lead,
            head: ctx.copy_slice(record.head(), "catia_native_record_head")?,
            inline_body: record
                .inline_body()
                .map(|bytes| ctx.copy_slice(bytes, "catia_native_record_inline_body"))
                .transpose()?,
            owner: roles.owner.map(CatiaObjectOwner::from),
            class: roles.class_ref.map(|class_ref| CatiaObjectClass {
                ordinal: class_ref,
                name: None,
                entry: None,
            }),
            storage: roles.storage_ref.map(|storage_ref| CatiaObjectStorage {
                reference: storage_ref,
                record: None,
                design_object: None,
            }),
            payload: record.payload().copy_charged(ctx)?,
            repeated_reference_schema_selection: None,
            references: Vec::new(),
        };
        ctx.push_vec(&mut records, row, "catia_native_graph_records")?;
    }
    for record in ctx.admit_iter(&mut records, "catia_native_graph_design_object_ids")? {
        record.design_object = record
            .owner_entity_id()
            .map(|owner| design_object_id(ctx, u64_from_index(graph.pos), owner))
            .transpose()?;
    }
    let record_index = GraphRecordIndex::new(ctx, index_storage, &records)?;
    for index in ctx.admit_iter(
        &(0..records.len()),
        "catia_native_graph_record_link_updates",
    )? {
        let storage_ref = records[index]
            .storage
            .as_ref()
            .map(|storage| storage.reference);
        let (storage_record, storage_design_object) =
            resolved_storage_link(ctx, storage_ref, &records, &record_index)?;
        let references =
            resolved_payload_references(ctx, &records[index].payload, &records, &record_index)?;
        let record = &mut records[index];
        if let Some(storage) = &mut record.storage {
            storage.record = storage_record;
            storage.design_object = storage_design_object;
        }
        record.references = references;
    }
    let mut entities = Vec::new();
    for (ordinal, entity) in ctx
        .admit_iter(entity_records, "catia_native_graph_entity_visits")?
        .enumerate()
    {
        let Some(object_record) = records.get(ordinal) else {
            continue;
        };
        let reference_signature = entity.reference_signature(ctx)?;
        let body = match &entity.body {
            entity_table::EntityBody::Inline(bytes) => CatiaEntityRecordBody::Inline(
                ctx.copy_slice(bytes, "catia_native_entity_inline_body")?,
            ),
            entity_table::EntityBody::Nested {
                prefix,
                suffix,
                value_payload,
                record_suffix,
                ..
            } => CatiaEntityRecordBody::Nested {
                definition_prefix: ctx
                    .copy_slice(prefix, "catia_native_entity_definition_prefix")?,
                definition_suffix: ctx
                    .copy_slice(suffix, "catia_native_entity_definition_suffix")?,
                value_payload: ctx
                    .copy_slice(value_payload, "catia_native_entity_value_payload")?,
                record_suffix: ctx
                    .copy_slice(record_suffix, "catia_native_entity_record_suffix")?,
            },
        };
        let row = CatiaEntityRecord {
            id: ctx.format_retained(
                format_args!("catia:outer:entity-record#{:010}", entity.pos),
                "catia_native_entity_id",
            )?,
            object_graph: ctx.copy_retained_text(&id, "catia_native_entity_graph")?,
            object_record: ctx
                .copy_retained_text(&object_record.id, "catia_native_entity_object")?,
            ordinal: u64_from_index(ordinal),
            byte_offset: u64_from_index(entity.pos),
            lead: entity.lead,
            body,
            definition_schema_selections: Vec::new(),
            entity_id: entity.entity_id,
            value_schema_selections: Vec::new(),
            object_production: None,
            value_production: None,
            range_interval: None,
            reference_signature: reference_signature.map(|production| CatiaReferenceSignature {
                production,
                first_entity: CatiaEntityReference::Unresolved { entity_id: 0 },
                second_entity: CatiaEntityReference::Unresolved { entity_id: 0 },
            }),
            suffix: None,
            suffix_schema_selection: None,
        };
        index_storage
            .with_storage(|| ctx.push_vec(&mut entities, row, "catia_native_graph_entities"))?;
    }
    Ok((
        CatiaObjectGraph {
            id,
            byte_offset: u64_from_index(graph.pos),
            byte_len: u64_from_index(graph.total_len),
            finjpl_segment,
            outer_container,
            catalog_byte_offset: graph.catalog_pos.map(u64_from_index),
            catalog: None,
            records,
        },
        entities,
        record_index,
    ))
}

impl CatiaOuterContainerBinding {
    pub(super) fn from_source(
        ctx: &DecodeContext<'_>,
        declaration: &container::OuterContainerDeclaration,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            data_offset: u64_from_index(declaration.data_offset),
            ordinal: declaration.ordinal,
            class_name: ctx
                .copy_retained_text(&declaration.class_name, "catia_native_outer_class")?,
            base_class: ctx
                .copy_retained_text(&declaration.base_class, "catia_native_outer_base_class")?,
            stream_name: ctx
                .copy_retained_text(&declaration.stream_name, "catia_native_outer_stream_name")?,
        })
    }
}

#[cfg(test)]
mod consolidated_edge_run_limit_tests {
    use std::collections::HashSet;

    #[test]
    fn native_edge_runs_propagate_a5_surface_limit() {
        let bytes = crate::test_support::test_a5a8::a5_surface_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            super::consolidated_edge_runs(
                ctx,
                &bytes,
                &records,
                &[],
                &[],
                &mut crate::nurbs::LaneRefusals::new(),
            )
        });
        assert!(matches!(limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == "catia_a5_distinct_knots"));
        let runs = crate::test_support::with_service_context(|ctx| {
            super::consolidated_edge_runs(
                ctx,
                &bytes,
                &records,
                &[],
                &[],
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("service collection budget");
        assert!(runs.is_empty());
    }

    #[test]
    fn native_edge_run_indexes_loci_and_ids_refuse_limits() {
        let mut supported = crate::test_support::test_b2::b2_cylinder_stream();
        for point in [
            [1.0f32, 4.0, 3.0],
            [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
        ] {
            supported.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in point {
                supported.extend_from_slice(&value.to_le_bytes());
            }
        }
        supported.extend_from_slice(
            &crate::test_support::test_a5_bound::a5_native_edge_run_stream(6, 139, 142),
        );
        let fixtures = [
            crate::test_support::test_a5_bound::a5_native_edge_run_stream(6, 139, 142),
            supported,
        ];
        let mut collection_refusals = HashSet::new();
        let mut retained_refusals = HashSet::new();
        let mut scoped_refusals = HashSet::new();
        for bytes in fixtures {
            let native = super::super::CatiaNative::decode(&bytes);
            assert_eq!(native.consolidated_edge_runs.len(), 1);
            let records = crate::wire::records::consolidated_records(&bytes);
            for limit in 0..1024 {
                let result = crate::test_support::with_collection_limit(limit, |ctx| {
                    super::consolidated_edge_runs(
                        ctx,
                        &bytes,
                        &records,
                        &native.consolidated_pcurves,
                        &native.consolidated_edge_nodes,
                        &mut crate::nurbs::LaneRefusals::new(),
                    )
                });
                if let Err(cadmpeg_core::CodecError::ResourceLimit(error)) = result {
                    collection_refusals.insert(error.operation);
                }
            }
            let mut limit = 0;
            for _ in 0..4096 {
                let result = crate::test_support::with_retained_limit(limit, |ctx| {
                    super::consolidated_edge_runs(
                        ctx,
                        &bytes,
                        &records,
                        &native.consolidated_pcurves,
                        &native.consolidated_edge_nodes,
                        &mut crate::nurbs::LaneRefusals::new(),
                    )
                });
                if let Err(cadmpeg_core::CodecError::ResourceLimit(error)) = result {
                    retained_refusals.insert(error.operation);
                    assert!(error.used + error.additional > limit);
                    limit = error.used + error.additional;
                } else {
                    assert!(result.is_ok());
                    break;
                }
            }
            let mut limit = 0;
            for _ in 0..4096 {
                let result = crate::test_support::with_materialized_limit(limit, |ctx| {
                    super::consolidated_edge_runs(
                        ctx,
                        &bytes,
                        &records,
                        &native.consolidated_pcurves,
                        &native.consolidated_edge_nodes,
                        &mut crate::nurbs::LaneRefusals::new(),
                    )
                });
                if let Err(cadmpeg_core::CodecError::ResourceLimit(error)) = result {
                    scoped_refusals.insert(error.operation);
                    assert!(error.used + error.additional > limit);
                    limit = error.used + error.additional;
                } else {
                    assert!(result.is_ok());
                    break;
                }
            }
        }
        for operation in [
            "catia_native_edge_run_pcurve_index",
            "catia_native_edge_run_resolved_index",
            "catia_native_edge_run_node_index",
            "catia_native_edge_run_shared_loci",
            "catia_native_edge_runs",
        ] {
            assert!(
                collection_refusals.contains(operation),
                "{operation} did not refuse"
            );
        }
        for operation in [
            "catia_native_edge_run_id",
            "catia_native_edge_run_first_pcurve_id",
            "catia_native_edge_run_second_pcurve_id",
            "catia_native_edge_run_node_id",
        ] {
            assert!(
                retained_refusals.contains(operation),
                "{operation} did not refuse"
            );
        }
        assert!(scoped_refusals.contains("catia_native_edge_run_pcurve_index"));
    }
}

#[cfg(test)]
mod consolidated_analytic_limit_tests {
    use super::{
        consolidated_circles, consolidated_cones, consolidated_line_profiles,
        consolidated_parameter_points, consolidated_plane_carriers, consolidated_reference_lists,
        consolidated_revolutions, consolidated_spheres, consolidated_tori,
    };
    use cadmpeg_core::CodecError;

    #[test]
    fn native_analytic_carriers_refuse_output_and_id_limits() {
        macro_rules! check {
            ($fixture:ident, $decode:ident, $output:literal, $id:literal) => {{
            let bytes = crate::test_support::test_b2::$fixture();
            let records = crate::wire::records::consolidated_records(&bytes);
            let limited = crate::test_support::with_collection_limit(0, |ctx| $decode(ctx, &bytes, &records));
            assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
                if error.operation == $output));
            let limited = crate::test_support::with_retained_limit(0, |ctx| $decode(ctx, &bytes, &records));
            assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
                if error.operation == $id));
            }};
        }
        check!(
            b2_circle_stream,
            consolidated_circles,
            "catia_native_circles",
            "catia_native_circle_id"
        );
        check!(
            b2_cone_stream,
            consolidated_cones,
            "catia_native_cones",
            "catia_native_cone_id"
        );
        check!(
            b2_sphere_stream,
            consolidated_spheres,
            "catia_native_spheres",
            "catia_native_sphere_id"
        );
        check!(
            b2_torus_stream,
            consolidated_tori,
            "catia_native_tori",
            "catia_native_torus_id"
        );
    }

    #[test]
    fn native_consolidated_circle_record_scan_propagates_work_refusal() {
        let bytes = crate::test_support::test_b2::b2_circle_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let service = crate::test_support::with_service_context(|ctx| {
            consolidated_circles(ctx, &bytes, &records)
        })
        .expect("service context admits the consolidated circle");
        assert_eq!(service.len(), 1);

        let operation = "catia_b2_family_record_scan";
        let refused = crate::test_support::with_work_refusal(operation, |ctx| {
            let result = consolidated_circles(ctx, &bytes, &records).map(|circles| circles.len());
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        });
        assert!(matches!(
            refused,
            Err(CodecError::ResourceLimit(limit)) if limit.operation == operation
        ));
    }

    #[test]
    fn native_revolution_refuses_profile_map_output_and_id_limits() {
        let bytes = crate::test_support::test_b2::b2_resolved_revolution_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(2, |ctx| {
            consolidated_revolutions(ctx, &bytes, &records, &[])
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_revolution_profile_index"));
        let limited = crate::test_support::with_collection_limit(3, |ctx| {
            consolidated_revolutions(ctx, &bytes, &records, &[])
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_revolutions"));
        let limited = crate::test_support::with_retained_limit(0, |ctx| {
            consolidated_revolutions(ctx, &bytes, &records, &[])
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_revolution_id"));
    }

    #[test]
    fn native_parameter_points_and_line_profiles_refuse_output_and_id_limits() {
        let bytes = crate::test_support::test_b2::b2_parameter_point_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            consolidated_parameter_points(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_parameter_points"));
        let limited = crate::test_support::with_retained_limit(0, |ctx| {
            consolidated_parameter_points(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_parameter_point_id"));
        let bytes = crate::test_support::test_b2::b2_line_profile_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            consolidated_line_profiles(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_line_profiles"));
        let limited = crate::test_support::with_retained_limit(0, |ctx| {
            consolidated_line_profiles(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_line_profile_id"));
    }

    #[test]
    fn native_reference_lists_refuse_output_and_id_limits() {
        let bytes = crate::test_support::test_b2::b2_reference_list_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "catia_native_reference_lists",
            |cap| {
                crate::test_support::with_collection_limit(cap, |ctx| {
                    consolidated_reference_lists(ctx, &bytes, &records)
                })
            },
        );
        assert!(
            matches!(limited, CodecError::ResourceLimit(error) if error.operation == "catia_native_reference_lists")
        );
        let limited = crate::test_support::with_retained_refusal(
            &[],
            "catia_native_reference_list_id",
            |ctx| consolidated_reference_lists(ctx, &bytes, &records),
        );
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_reference_list_id"));
    }

    #[test]
    fn native_plane_carriers_refuse_output_and_id_limits() {
        let bytes = crate::test_support::test_b2::b2_plane_carrier_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(3, |ctx| {
            consolidated_plane_carriers(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_plane_carriers"));
        let limited = crate::test_support::with_retained_refusal(
            &[],
            "catia_native_plane_carrier_id",
            |ctx| consolidated_plane_carriers(ctx, &bytes, &records),
        );
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_plane_carrier_id"));
    }
}

#[cfg(test)]
mod design_limit_tests {
    #![allow(clippy::doc_markdown, clippy::unwrap_used)]

    use crate::test_support::test_object_graph::{
        catalog_stream, object_graph_record, sequential_entity_backed_object_graph,
    };

    #[test]
    fn parallel_reference_table_refuses_nested_collection_limit() {
        let list_a = [0x3b, 0x82, 0x81, 0x83, 0x81, 0x84, 0x85, 0xfe];
        let list_b = [0x3b, 0x82, 0x81, 0x84, 0x81, 0x83, 0x86, 0xfe];
        let mut bytes = sequential_entity_backed_object_graph(&[
            object_graph_record(&[0x04, 0x01, 0x81, 0x83], &list_a),
            object_graph_record(&[0x04, 0x01, 0x81, 0x84], &list_b),
            object_graph_record(&[0x04, 0x01, 0x83, 0x83], &[0xfe]),
            object_graph_record(&[0x04, 0x01, 0x83, 0x84], &[0xfe]),
        ]);
        bytes.extend(catalog_stream(&[
            "CATCatalogManager",
            "catalogManager",
            "catalogLinks",
            "",
            "Profile",
            "Limit",
            "Profile",
            "Limit",
        ]));
        let native = crate::native::CatiaNative::decode(&bytes);
        let graph = &native.object_graphs[0];
        let owner = native.design_objects[0].owner_entity_id;
        let fields = graph
            .records
            .iter()
            .filter(|record| record.owner_entity_id() == Some(owner))
            .collect::<Vec<_>>();
        let record_index = crate::test_support::with_service_context(|ctx| {
            super::super::GraphRecordIndex::new(
                ctx,
                &mut ctx.reserve_scoped(0, "test record index")?,
                &graph.records,
            )
        })
        .expect("service profile admits record index");
        let refused = crate::test_support::with_collection_limit(2, |ctx| {
            super::design_parallel_reference_table(ctx, &fields, graph, &record_index)
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_design_row_cells")
        );
        let retained =
            crate::test_support::with_retained_refusal(&[], "catia_design_column_field", |ctx| {
                super::design_parallel_reference_table(ctx, &fields, graph, &record_index)
            });
        assert!(
            matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_design_column_field")
        );
        let admitted = crate::test_support::with_service_context(|ctx| {
            super::design_parallel_reference_table(ctx, &fields, graph, &record_index)
        })
        .expect("service profile admits parallel reference table");
        assert_eq!(admitted, native.design_objects[0].parallel_reference_table);
    }
}
