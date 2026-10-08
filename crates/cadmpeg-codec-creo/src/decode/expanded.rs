// SPDX-License-Identifier: Apache-2.0
//! Expanded-section arenas, feature surface replay associations, and FC05 native records.

use crate::decode::native::CreoArena;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::ContainerScan;

use super::coverage::{source_section_ref, surface_family};
use super::native::{emit_uniform, store_arena, UniformArena};
use super::native_records::{
    CreoFc05CircleRecord, CreoFc05CylinderCapPairRecord, CreoFeatureSurfaceReplayAssociation,
    CreoHalfEdgeRef,
};
use super::records::double_xar::CreoDoubleXarTableRecord;
use super::records::{expanded_section_records, CreoPrimitiveScalarArrayRecord};

pub(super) fn attach_expanded_sections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    // The whole expansion namespace is gated on there being expanded sections at
    // all: with none, the double-xar and primitive-scalar arenas are skipped even
    // when their scan tables are non-empty. Preserve that early return.
    let (records, _records_storage) = expanded_section_records(ctx, scan)?;
    if records.is_empty() {
        return Ok(());
    }
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::ExpandedSections,
            records: &records,
            id: |record| &record.id,
            stream: |record| &record.name,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.source_offset),
            tag: "unix_compress_expanded_section",
            exactness: Exactness::Derived,
        },
    )?;
    let (tables, _tables_storage) = double_xar_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::DoubleXarTables,
            records: &tables,
            id: |table| &table.id,
            stream: |table| &table.table.section_name,
            offset: |table| cadmpeg_core::decode::u64_from_index(table.table.section_source_offset),
            tag: "model_scalar_dictionary",
            exactness: Exactness::ByteExact,
        },
    )?;
    let (primitive_arrays, _primitive_arrays_storage) = primitive_scalar_array_records(ctx, scan)?;
    store_arena(ctx, ir, CreoArena::PrimitiveScalarArrays, &primitive_arrays)?;
    Ok(())
}

fn double_xar_records<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<(Vec<CreoDoubleXarTableRecord<'a>>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
    ctx.with_scoped_storage("creo expanded projection storage", || {
        let mut records = Vec::new();
        for table in ctx.admit_iter(&scan.primitives.double_xar_tables, "creo expanded record traversal")? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:{}:double_xar#{}:{}",
                    table.section_name, table.section_source_offset, table.expanded_offset
                ),
                "creo native double-xar IDs",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native double-xar records")?;
            records.push(CreoDoubleXarTableRecord { id, table });
        }
        Ok(records)
    })
}

fn primitive_scalar_array_records<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<(Vec<CreoPrimitiveScalarArrayRecord<'a>>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
    ctx.with_scoped_storage("creo expanded projection storage", || {
        let mut records = Vec::new();
        for array in ctx.admit_iter(&scan.primitives.scalar_arrays, "creo expanded record traversal")? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:solid_primdata:scalar_array#{}:{}",
                    array.field.as_str(),
                    array.offset
                ),
                "creo native scalar-array IDs",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native scalar-array records")?;
            records.push(CreoPrimitiveScalarArrayRecord {
                id,
                field: array.field.as_str(),
                expanded_offset: array.offset,
                count: array.values.len(),
                values: &array.values,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_surface_replay_associations<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<(Vec<CreoFeatureSurfaceReplayAssociation>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
    ctx.with_scoped_storage("creo expanded projection storage", || {
        let mut associations = Vec::new();
        visit_feature_surface_replays(
            ctx,
            scan,
            |owner_feature_id, table_offset, replay_ordinal, visible, replay| {
                let id = ctx.format_retained(
                    format_args!(
                        "creo:allfeatur:surface_replay#{}:{}:{}:{}",
                        owner_feature_id, table_offset, replay_ordinal, visible.id
                    ),
                    "creo native surface replay IDs",
                )?;
                ctx.reserve_vec(&mut associations, 1, "creo native surface replay records")?;
                associations.push(CreoFeatureSurfaceReplayAssociation {
                    id,
                    owner_feature_id,
                    visible_surface_id: visible.id,
                    replay_surface_id: replay.id,
                    replay_ordinal,
                    surface_family: surface_family(visible.kind),
                    table_offset,
                });
                Ok(())
            },
        )?;
        Ok(associations)
    })
}

pub(super) fn feature_surface_replay_association_count(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<usize, CodecError> {
    let mut count = 0usize;
    visit_feature_surface_replays(ctx, scan, |_, _, _, _, _| {
        count = count.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("creo native surface replay count", u64::MAX, u64::MAX)
        })?;
        Ok(())
    })?;
    Ok(count)
}

fn visit_feature_surface_replays(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    mut emit: impl FnMut(
        u32,
        usize,
        usize,
        &crate::surface::SurfaceRow,
        &crate::surface::SurfaceRow,
    ) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    for table in ctx.admit_iter(&scan.features.entity_tables, "creo expanded record traversal")? {
        let owner_feature_id = table.feature_id;
        let visible_count = ctx.position_by(&table.entries, |entry| Ok(entry.class_id() != 254), "creo surface replay entry scan")?.unwrap_or(table.entries.len());
        if visible_count == 0 { continue; }
        let visible_entries = &table.entries[..visible_count];
        if ctx.any_by(visible_entries, |entry| Ok(scan.surfaces.rows.unique(entry.entity_id).is_none()), "creo surface replay validation")? { continue; }
        let replay_entries = &table.entries[visible_count..];
        let mut replay_ordinal = 0;
        let mut cursor = 0usize;
        while let Some(end) = cursor
            .checked_add(visible_count)
            .filter(|end| *end <= replay_entries.len())
        {
            let candidate_entries = &replay_entries[cursor..end];
            if ctx.any_by(candidate_entries, |entry| Ok(entry.class_id() != 214), "creo surface replay candidate work")?
            {
                cursor += 1;
                continue;
            }
            if ctx.all_by(visible_entries.iter().zip(candidate_entries), |(visible_entry, replay_entry)| {
                    let visible = scan.surfaces.rows.unique(visible_entry.entity_id);
                    let replay = scan.surfaces.nonvisible_rows.unique(replay_entry.entity_id);
                    Ok(visible.zip(replay).is_some_and(|(visible, replay)| {
                        visible.feature_id == owner_feature_id
                            && replay.feature_id == owner_feature_id
                            && visible.kind == replay.kind
                    }))
                }, "creo surface replay correspondence work")?
            {
                for (visible_entry, replay_entry) in ctx.admit_iter(visible_entries, "creo surface replay emission lookups")?.zip(candidate_entries) {
                    let visible = scan.surfaces.rows.unique(visible_entry.entity_id)
                    .ok_or_else(|| {
                        CodecError::malformed("matched visible replay row disappeared")
                    })?;
                    let replay = scan.surfaces.nonvisible_rows.unique(replay_entry.entity_id)
                    .ok_or_else(|| {
                        CodecError::malformed("matched nonvisible replay row disappeared")
                    })?;
                    emit(
                        owner_feature_id,
                        table.offset,
                        replay_ordinal,
                        visible,
                        replay,
                    )?;
                }
                replay_ordinal += 1;
                cursor += visible_count;
            } else {
                cursor += 1;
            }
        }
    }
    Ok(())
}

pub(super) fn affected_kind(kind: crate::feature::rows::AffectedIdKind) -> &'static str {
    match kind {
        crate::feature::rows::AffectedIdKind::Geometry => "geometry",
        crate::feature::rows::AffectedIdKind::Edges => "edges",
        crate::feature::rows::AffectedIdKind::StrongParents => "strong_parents",
        crate::feature::rows::AffectedIdKind::Parents => "parents",
        crate::feature::rows::AffectedIdKind::Contours => "contours",
        crate::feature::rows::AffectedIdKind::Quilts => "quilts",
    }
}

pub(super) fn extent_source(source: crate::feature::rows::ReplayExtentSource) -> &'static str {
    match source {
        crate::feature::rows::ReplayExtentSource::Explicit => "explicit",
        crate::feature::rows::ReplayExtentSource::Inherited => "inherited",
    }
}

pub(super) fn half_edge_ref(id: crate::topology::HalfEdgeId) -> CreoHalfEdgeRef {
    CreoHalfEdgeRef {
        curve_id: id.curve_id,
        side: id.side,
    }
}

pub(super) fn fc05_circle_records<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<(Vec<CreoFc05CircleRecord<'a>>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
    ctx.with_scoped_storage("creo expanded projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(&scan.curves.fc05_circles, "creo expanded record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:curve:fc05_circle#{}", record.curve_id),
                "creo native FC05 circle IDs",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native FC05 circle records")?;
            records.push(CreoFc05CircleRecord {
                id,
                curve_id: record.curve_id,
                center_row_frame: record.center_row_frame,
                radius_mm: record.radius_mm,
                sample_direction_row_frame: record.sample_direction_row_frame.get(),
                angle_parameter: record.angle_parameter,
                cap_ordinate_row_frame: record.cap_ordinate_row_frame,
                point_count: record.point_count,
                max_residual: record.max_residual,
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn fc05_cylinder_cap_pair_records<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<(Vec<CreoFc05CylinderCapPairRecord<'a>>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
    ctx.with_scoped_storage("creo expanded projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(&scan.curves.fc05_cylinder_cap_pairs, "creo expanded record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:surface:fc05_cylinder_cap_pair#{}", record.surface_id),
                "creo native FC05 cap pair IDs",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native FC05 cap pair records")?;
            records.push(CreoFc05CylinderCapPairRecord {
                id,
                surface_id: record.surface_id,
                cap_edges: &record.cap_edges,
                center_row_frame: record.center_row_frame,
                radius_mm: record.radius_mm,
                reference_direction_row_frame: record.reference_direction_row_frame,
                parameter_sign: record.parameter_sense.as_i8(),
                cap_ordinates_row_frame: &record.cap_ordinates_row_frame,
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        double_xar_records, fc05_circle_records, fc05_cylinder_cap_pair_records,
        feature_surface_replay_association_count, feature_surface_replay_associations,
        primitive_scalar_array_records,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn primitive_scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.primitives
            .double_xar_tables
            .push(crate::container::ModelDoubleXarTable {
                section_name: "Body".to_string(),
                section_source_offset: 0,
                expanded_offset: 0,
                entries: Vec::new(),
            });
        scan.primitives
            .scalar_arrays
            .push(crate::primdata::PrimitiveScalarArray {
                field: crate::primdata::PrimitiveArrayField::Points,
                offset: 0,
                values: vec![cadmpeg_ir::scalar::FiniteReal::new(2.5).expect("finite scalar")],
            });
        scan.curves.fc05_circles.push(crate::curve::Fc05Circle {
            curve_id: 20,
            center_row_frame: [3.0, 4.0],
            radius_mm: 2.0,
            sample_direction_row_frame: cadmpeg_ir::units::HypotDirection2::normalized_with_length(
                [1.0, 0.0],
            )
            .expect("unit sample direction")
            .0,
            angle_parameter: crate::curve::Fc05AngleParameterRelation::Consistent {
                sense: crate::curve::ParameterSense::Increasing,
                reference_direction_row_frame: [1.0, 0.0],
            },
            cap_ordinate_row_frame: Some(-5.0),
            point_count: 8,
            max_residual: 0.0,
            offset: 0,
        });
        scan.curves
            .fc05_cylinder_cap_pairs
            .push(crate::curve::Fc05CylinderCapPair {
                surface_id: 10,
                cap_edges: vec![
                    crate::curve::Fc05CapEdge {
                        curve_id: 20,
                        cap_plane_id: 11,
                        cap_ordinate_row_frame: -5.0,
                    },
                    crate::curve::Fc05CapEdge {
                        curve_id: 21,
                        cap_plane_id: 12,
                        cap_ordinate_row_frame: 7.0,
                    },
                ],
                center_row_frame: [3.0, 4.0],
                radius_mm: 2.0,
                reference_direction_row_frame: [1.0, 0.0],
                parameter_sense: crate::curve::ParameterSense::Increasing,
                cap_ordinates_row_frame: vec![-5.0, 7.0],
                offset: 0,
            });
        scan
    }

    fn with_limits(
        materialized: u64,
        items: u64,
        project: impl FnOnce(
            &DecodeContext<'_>,
            &crate::container::ContainerScan<'_>,
        ) -> Result<serde_json::Value, cadmpeg_core::CodecError>,
    ) -> Result<serde_json::Value, cadmpeg_core::CodecError> {
        let scan = primitive_scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = materialized;
        policy.limits.max_collection_items = items;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        project(&ctx, &scan)
    }

    fn replay_scan() -> crate::container::ContainerScan<'static> {
        use crate::feature::entity::{
            EntryPayload, FeatureEntityTable, FeatureEntityTableEntry, PlainClass,
        };
        let mut scan = crate::test_support::empty_container_scan();
        let entry = |entity_id, class_id| FeatureEntityTableEntry {
            entity_id,
            payload: EntryPayload::Plain {
                class: PlainClass::new(class_id).expect("plain entry class"),
            },
            prefixed: false,
            offset: 0,
            end_offset: 0,
        };
        scan.features.entity_tables.push(FeatureEntityTable::new(
            4,
            913,
            vec![entry(7, 254), entry(9, 214)],
            &std::collections::BTreeSet::from([7, 9]),
            0,
        ));
        let row = |id| crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 0,
        };
        scan.surfaces.rows.push(row(7));
        scan.surfaces.nonvisible_rows.push(row(9));
        scan
    }

    fn with_replay_limits(
        materialized: u64,
        items: u64,
        work: u64,
        project: impl FnOnce(
            &DecodeContext<'_>,
            &crate::container::ContainerScan<'_>,
        ) -> Result<serde_json::Value, cadmpeg_core::CodecError>,
    ) -> Result<serde_json::Value, cadmpeg_core::CodecError> {
        let scan = replay_scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = materialized;
        policy.limits.max_collection_items = items;
        policy.limits.max_work_units = work;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        project(&ctx, &scan)
    }

    #[test]
    fn native_surface_replay_candidate_refuses_work_limit() {
        let scan = replay_scan();
        crate::test_support::assert_work_boundaries(
            &["creo surface replay candidate work"],
            |ctx| feature_surface_replay_association_count(ctx, &scan),
        );
    }

    #[test]
    fn native_surface_replay_id_refuses_materialized_limit() {
        let scan = replay_scan();
        let error = crate::test_support::last_refusal_at(
            &[], ResourceDimension::MaterializedBytes, "creo native surface replay IDs",
            |ctx| feature_surface_replay_associations(ctx, &scan).map(|_| ()),
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native surface replay IDs"));
    }

    #[test]
    fn native_surface_replay_row_refuses_collection_limit() {
        let error = with_replay_limits(u64::MAX, 0, u64::MAX, |ctx, scan| {
            let (records, _records_storage) = feature_surface_replay_associations(ctx, scan)?;
            Ok(serde_json::json!(records.len()))
        })
        .expect_err("one association needs an output row");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native surface replay records")
        );
        let record = with_replay_limits(u64::MAX, 1, u64::MAX, |ctx, scan| {
            let (records, _records_storage) = feature_surface_replay_associations(ctx, scan)?;
            Ok(serde_json::to_value(&records[0]).expect("record JSON"))
        })
        .expect("one association record");
        assert_eq!(record["id"], "creo:allfeatur:surface_replay#4:0:0:7");
        assert_eq!(record["visible_surface_id"], 7);
        assert_eq!(record["replay_surface_id"], 9);
        assert_eq!(record["surface_family"], "plane");
        assert_eq!(
            with_replay_limits(u64::MAX, 0, u64::MAX, |ctx, scan| {
                let count = feature_surface_replay_association_count(ctx, scan)?;
                Ok(serde_json::json!(count))
            })
            .expect("the metadata count makes no record copy"),
            1
        );
    }

    #[test]
    fn native_double_xar_id_refuses_materialized_limit() {
        let scan = primitive_scan();
        let error = crate::test_support::last_refusal_at(
            &[], ResourceDimension::MaterializedBytes, "creo native double-xar IDs",
            |ctx| double_xar_records(ctx, &scan).map(|_| ()),
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native double-xar IDs"));
    }

    #[test]
    fn native_double_xar_row_refuses_collection_limit() {
        let error = with_limits(u64::MAX, 0, |ctx, scan| {
            let (records, _records_storage) = double_xar_records(ctx, scan)?;
            Ok(serde_json::json!(records.len()))
        })
        .expect_err("one table needs an output row");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native double-xar records")
        );
        let value = with_limits(u64::MAX, 1, |ctx, scan| {
            let (records, _records_storage) = double_xar_records(ctx, scan)?;
            Ok(serde_json::to_value(&records[0]).expect("record JSON"))
        })
        .expect("one table record");
        assert_eq!(value["id"], "creo:Body:double_xar#0:0");
        assert_eq!(value["count"], 0);
    }

    #[test]
    fn native_scalar_array_id_refuses_materialized_limit() {
        let scan = primitive_scan();
        let error = crate::test_support::last_refusal_at(
            &[], ResourceDimension::MaterializedBytes, "creo native scalar-array IDs",
            |ctx| primitive_scalar_array_records(ctx, &scan).map(|_| ()),
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native scalar-array IDs"));
    }

    #[test]
    fn native_scalar_array_row_refuses_collection_limit() {
        let error = with_limits(u64::MAX, 0, |ctx, scan| {
            let (records, _records_storage) = primitive_scalar_array_records(ctx, scan)?;
            Ok(serde_json::json!(records.len()))
        })
        .expect_err("one scalar array needs an output row");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native scalar-array records")
        );
        let value = with_limits(u64::MAX, 1, |ctx, scan| {
            let (records, _records_storage) = primitive_scalar_array_records(ctx, scan)?;
            Ok(serde_json::to_value(&records[0]).expect("record JSON"))
        })
        .expect("one scalar-array record");
        assert_eq!(value["id"], "creo:solid_primdata:scalar_array#pts:0");
        assert_eq!(value["field"], "pts");
        assert_eq!(value["values"], serde_json::json!([2.5]));
    }

    #[test]
    fn native_fc05_circle_id_refuses_materialized_limit() {
        let scan = primitive_scan();
        let error = crate::test_support::last_refusal_at(
            &[], ResourceDimension::MaterializedBytes, "creo native FC05 circle IDs",
            |ctx| fc05_circle_records(ctx, &scan).map(|_| ()),
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native FC05 circle IDs"));
    }

    #[test]
    fn native_fc05_circle_row_refuses_collection_limit() {
        let error = with_limits(u64::MAX, 0, |ctx, scan| {
            let (records, _records_storage) = fc05_circle_records(ctx, scan)?;
            Ok(serde_json::json!(records.len()))
        })
        .expect_err("one FC05 circle needs one output row");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native FC05 circle records")
        );
        let value = with_limits(u64::MAX, 1, |ctx, scan| {
            let (records, _records_storage) = fc05_circle_records(ctx, scan)?;
            Ok(serde_json::to_value(&records[0]).expect("record JSON"))
        })
        .expect("one FC05 circle record");
        assert_eq!(value["id"], "creo:curve:fc05_circle#20");
        assert_eq!(value["radius_mm"], 2.0);
        assert_eq!(value["point_count"], 8);
    }

    #[test]
    fn native_fc05_cap_pair_id_refuses_materialized_limit() {
        let scan = primitive_scan();
        let error = crate::test_support::last_refusal_at(
            &[], ResourceDimension::MaterializedBytes, "creo native FC05 cap pair IDs",
            |ctx| fc05_cylinder_cap_pair_records(ctx, &scan).map(|_| ()),
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native FC05 cap pair IDs"));
    }

    #[test]
    fn native_fc05_cap_pair_row_refuses_collection_limit() {
        let error = with_limits(u64::MAX, 0, |ctx, scan| {
            let (records, _records_storage) = fc05_cylinder_cap_pair_records(ctx, scan)?;
            Ok(serde_json::json!(records.len()))
        })
        .expect_err("one FC05 cap pair needs one output row");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native FC05 cap pair records")
        );
        let value = with_limits(u64::MAX, 1, |ctx, scan| {
            let (records, _records_storage) = fc05_cylinder_cap_pair_records(ctx, scan)?;
            Ok(serde_json::to_value(&records[0]).expect("record JSON"))
        })
        .expect("one FC05 cap-pair record");
        assert_eq!(value["id"], "creo:surface:fc05_cylinder_cap_pair#10");
        assert_eq!(value["curve_ids"], serde_json::json!([20, 21]));
        assert_eq!(value["cap_plane_ids"], serde_json::json!([11, 12]));
        assert_eq!(
            value["curve_cap_ordinates_row_frame"],
            serde_json::json!([-5.0, 7.0])
        );
        assert_eq!(
            value["cap_ordinates_row_frame"],
            serde_json::json!([-5.0, 7.0])
        );
    }
    #[test]
    fn expanded_projection_storage_releases_all_buffers_at_drop() {
        let scan = primitive_scan();
        let replay = replay_scan();
        let run = |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let double_xar = double_xar_records(&ctx, &scan)?;
            let arrays = primitive_scalar_array_records(&ctx, &scan)?;
            let circles = fc05_circle_records(&ctx, &scan)?;
            let cap_pairs = fc05_cylinder_cap_pair_records(&ctx, &scan)?;
            let associations = feature_surface_replay_associations(&ctx, &replay)?;
            assert_eq!(double_xar.0.len(), 1);
            assert_eq!(arrays.0.len(), 1);
            assert_eq!(circles.0.len(), 1);
            assert_eq!(cap_pairs.0.len(), 1);
            assert_eq!(associations.0.len(), 1);
            drop((double_xar, arrays, circles, cap_pairs, associations));
            let _all_storage = ctx.reserve_scoped(cap, "test expanded storage release")?;
            Ok::<_, cadmpeg_core::CodecError>(())
        };
        let cap = crate::test_support::allocation_limit_at(ResourceDimension::MaterializedBytes, None, run);
        run(cap).expect("all projection storage releases at drop");
    }

}
