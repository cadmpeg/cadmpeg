// SPDX-License-Identifier: Apache-2.0
//! Expanded-section arenas, feature surface replay associations, and FC05 native records.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::ContainerScan;

use super::coverage::{source_section, surface_family};
use super::native::{emit_uniform, store_arena};
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
    let records = expanded_section_records(ctx, scan)?;
    if records.is_empty() {
        return Ok(());
    }
    emit_uniform(
        ctx,
        ir,
        annotations,
        "expanded_sections",
        &records,
        |record| &record.id,
        |record| &record.name,
        |record| record.source_offset as u64,
        "unix_compress_expanded_section",
        Exactness::Derived,
    )?;
    let tables = double_xar_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        "double_xar_tables",
        &tables,
        |table| &table.id,
        |table| &table.table.section_name,
        |table| table.table.section_source_offset as u64,
        "model_scalar_dictionary",
        Exactness::ByteExact,
    )?;
    let primitive_arrays = primitive_scalar_array_records(ctx, scan)?;
    store_arena(ctx, ir, "primitive_scalar_arrays", &primitive_arrays)?;
    Ok(())
}

fn double_xar_records<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<Vec<CreoDoubleXarTableRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for table in &scan.primitives.double_xar_tables {
        let id = ctx.format_retained(
            format_args!(
                "creo:{}:double_xar#{}:{}",
                table.section_name, table.section_source_offset, table.expanded_offset
            ),
            "creo native double-xar IDs",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native double-xar records")?;
        records.push(CreoDoubleXarTableRecord { id, table });
    }
    Ok(records)
}

fn primitive_scalar_array_records<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<Vec<CreoPrimitiveScalarArrayRecord<'a>>, CodecError> {
    let mut records = Vec::new();
    for array in &scan.primitives.scalar_arrays {
        let id = ctx.format_retained(
            format_args!("creo:solid_primdata:scalar_array#{}:{}", array.field.as_str(), array.offset),
            "creo native scalar-array IDs",
        )?;
        ctx.try_reserve_items(&mut records, 1, "creo native scalar-array records")?;
        records.push(CreoPrimitiveScalarArrayRecord {
            id,
            field: array.field.as_str(),
            expanded_offset: array.offset,
            count: array.values.len(),
            values: &array.values,
        });
    }
    Ok(records)
}

pub(super) fn feature_surface_replay_associations(
    scan: &ContainerScan,
) -> Vec<CreoFeatureSurfaceReplayAssociation> {
    let mut associations = Vec::new();
    for table in &scan.features.entity_tables {
        let owner_feature_id = table.feature_id;
        let visible_ids = table
            .entries
            .iter()
            .take_while(|entry| entry.class_id() == 254)
            .map(|entry| entry.entity_id)
            .collect::<Vec<_>>();
        if visible_ids.is_empty() {
            continue;
        }
        let visible_rows = visible_ids
            .iter()
            .map(|id| crate::surface::unique_surface_row(&scan.surfaces.rows, *id))
            .collect::<Option<Vec<_>>>();
        let Some(visible_rows) = visible_rows else {
            continue;
        };
        let replay_entries = &table.entries[visible_ids.len()..];
        let mut replay_ordinal = 0;
        let mut cursor = 0;
        while cursor + visible_rows.len() <= replay_entries.len() {
            let candidate_entries = &replay_entries[cursor..cursor + visible_rows.len()];
            if candidate_entries
                .iter()
                .any(|entry| entry.class_id() != 214)
            {
                cursor += 1;
                continue;
            }
            let candidate_rows = candidate_entries
                .iter()
                .map(|entry| {
                    crate::surface::unique_surface_row(
                        &scan.surfaces.nonvisible_rows,
                        entry.entity_id,
                    )
                })
                .collect::<Option<Vec<_>>>();
            let Some(candidate_rows) = candidate_rows else {
                cursor += 1;
                continue;
            };
            if visible_rows
                .iter()
                .zip(&candidate_rows)
                .all(|(visible, replay)| {
                    visible.feature_id == owner_feature_id
                        && replay.feature_id == owner_feature_id
                        && visible.kind == replay.kind
                })
            {
                associations.extend(visible_rows.iter().zip(candidate_rows).map(
                    |(visible, replay)| CreoFeatureSurfaceReplayAssociation {
                        id: format!(
                            "creo:allfeatur:surface_replay#{}:{}:{}:{}",
                            owner_feature_id, table.offset, replay_ordinal, visible.id
                        ),
                        owner_feature_id,
                        visible_surface_id: visible.id,
                        replay_surface_id: replay.id,
                        replay_ordinal,
                        surface_family: surface_family(visible.kind).to_string(),
                        table_offset: table.offset,
                    },
                ));
                replay_ordinal += 1;
                cursor += visible_rows.len();
            } else {
                cursor += 1;
            }
        }
    }
    associations
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

pub(super) fn fc05_circle_records(scan: &ContainerScan) -> Vec<CreoFc05CircleRecord> {
    scan.curves
        .fc05_circles
        .iter()
        .map(|record| CreoFc05CircleRecord {
            id: format!("creo:curve:fc05_circle#{}", record.curve_id),
            curve_id: record.curve_id,
            center_row_frame: record.center_row_frame,
            radius_mm: record.radius_mm,
            sample_direction_row_frame: record.sample_direction_row_frame.get(),
            angle_parameter: record.angle_parameter,
            cap_ordinate_row_frame: record.cap_ordinate_row_frame,
            point_count: record.point_count,
            max_residual: record.max_residual,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn fc05_cylinder_cap_pair_records(
    scan: &ContainerScan,
) -> Vec<CreoFc05CylinderCapPairRecord> {
    scan.curves
        .fc05_cylinder_cap_pairs
        .iter()
        .map(|record| CreoFc05CylinderCapPairRecord {
            id: format!("creo:surface:fc05_cylinder_cap_pair#{}", record.surface_id),
            surface_id: record.surface_id,
            cap_edges: record.cap_edges.clone(),
            center_row_frame: record.center_row_frame,
            radius_mm: record.radius_mm,
            reference_direction_row_frame: record.reference_direction_row_frame,
            parameter_sign: record.parameter_sense.as_i8(),
            cap_ordinates_row_frame: record.cap_ordinates_row_frame.clone(),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{double_xar_records, primitive_scalar_array_records};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn primitive_scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        scan.primitives.double_xar_tables.push(crate::container::ModelDoubleXarTable {
            section_name: "Body".to_string(),
            section_source_offset: 0,
            expanded_offset: 0,
            entries: Vec::new(),
        });
        scan.primitives.scalar_arrays.push(crate::primdata::PrimitiveScalarArray {
            field: crate::primdata::PrimitiveArrayField::Points,
            offset: 0,
            values: vec![cadmpeg_ir::scalar::FiniteReal::new(2.5).expect("finite scalar")],
        });
        scan
    }

    fn with_limits(
        retained: u64,
        items: u64,
        project: impl FnOnce(&DecodeContext<'_>, &crate::container::ContainerScan<'_>)
            -> Result<serde_json::Value, cadmpeg_core::CodecError>,
    ) -> Result<serde_json::Value, cadmpeg_core::CodecError> {
        let scan = primitive_scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = retained;
        policy.limits.max_collection_items = items;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        project(&ctx, &scan)
    }

    #[test]
    fn native_double_xar_id_refuses_retained_limit() {
        let limit = "creo:Body:double_xar#0:0".len() as u64 - 1;
        let error = with_limits(limit, 1, |ctx, scan| {
            let records = double_xar_records(ctx, scan)?;
            Ok(serde_json::json!(records.len()))
        }).expect_err("table ID needs its full retained length");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native double-xar IDs"));
    }

    #[test]
    fn native_double_xar_row_refuses_collection_limit() {
        let error = with_limits(u64::MAX, 0, |ctx, scan| {
            let records = double_xar_records(ctx, scan)?;
            Ok(serde_json::json!(records.len()))
        }).expect_err("one table needs an output row");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native double-xar records"));
        let value = with_limits(u64::MAX, 1, |ctx, scan| {
            let records = double_xar_records(ctx, scan)?;
            Ok(serde_json::to_value(&records[0]).expect("record JSON"))
        }).expect("one table record");
        assert_eq!(value["id"], "creo:Body:double_xar#0:0");
        assert_eq!(value["count"], 0);
    }

    #[test]
    fn native_scalar_array_id_refuses_retained_limit() {
        let limit = "creo:solid_primdata:scalar_array#pts:0".len() as u64 - 1;
        let error = with_limits(limit, 1, |ctx, scan| {
            let records = primitive_scalar_array_records(ctx, scan)?;
            Ok(serde_json::json!(records.len()))
        }).expect_err("scalar-array ID needs its full retained length");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native scalar-array IDs"));
    }

    #[test]
    fn native_scalar_array_row_refuses_collection_limit() {
        let error = with_limits(u64::MAX, 0, |ctx, scan| {
            let records = primitive_scalar_array_records(ctx, scan)?;
            Ok(serde_json::json!(records.len()))
        }).expect_err("one scalar array needs an output row");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native scalar-array records"));
        let value = with_limits(u64::MAX, 1, |ctx, scan| {
            let records = primitive_scalar_array_records(ctx, scan)?;
            Ok(serde_json::to_value(&records[0]).expect("record JSON"))
        }).expect("one scalar-array record");
        assert_eq!(value["id"], "creo:solid_primdata:scalar_array#pts:0");
        assert_eq!(value["field"], "pts");
        assert_eq!(value["values"], serde_json::json!([2.5]));
    }
}
