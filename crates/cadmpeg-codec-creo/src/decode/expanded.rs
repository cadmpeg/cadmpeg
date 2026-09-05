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
use super::records::{
    expanded_section_records, CreoDoubleXarEntryRecord, CreoDoubleXarTableRecord,
    CreoPrimitiveScalarArrayRecord,
};

pub(crate) fn attach_expanded_sections(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    // The whole expansion namespace is gated on there being expanded sections at
    // all: with none, the double-xar and primitive-scalar arenas are skipped even
    // when their scan tables are non-empty. Preserve that early return.
    let records = expanded_section_records(scan);
    if records.is_empty() {
        return Ok(());
    }
    emit_uniform(
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
    let tables = scan
        .primitives
        .double_xar_tables
        .iter()
        .map(|table| CreoDoubleXarTableRecord {
            id: format!(
                "creo:{}:double_xar#{}:{}",
                table.section_name, table.section_source_offset, table.expanded_offset
            ),
            section_name: table.section_name.clone(),
            section_source_offset: table.section_source_offset,
            expanded_offset: table.expanded_offset,
            count: table.count,
            entries: table
                .entries
                .iter()
                .map(|entry| CreoDoubleXarEntryRecord {
                    index: entry.index,
                    raw: entry.raw.clone(),
                    value: entry.value,
                    kind: entry.kind,
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    emit_uniform(
        ir,
        annotations,
        "double_xar_tables",
        &tables,
        |table| &table.id,
        |table| &table.section_name,
        |table| table.section_source_offset as u64,
        "model_scalar_dictionary",
        Exactness::ByteExact,
    )?;
    let primitive_arrays = scan
        .primitives
        .scalar_arrays
        .iter()
        .map(|array| CreoPrimitiveScalarArrayRecord {
            id: format!(
                "creo:solid_primdata:scalar_array#{}:{}",
                array.field, array.offset
            ),
            field: array.field.clone(),
            expanded_offset: array.offset,
            count: array.count,
            values: array.values.clone(),
        })
        .collect::<Vec<_>>();
    store_arena(ir, "primitive_scalar_arrays", &primitive_arrays)?;
    Ok(())
}

pub(crate) fn feature_surface_replay_associations(
    scan: &ContainerScan,
) -> Vec<CreoFeatureSurfaceReplayAssociation> {
    let mut associations = Vec::new();
    for table in &scan.features.entity_tables {
        let owner_feature_id = table.feature_id;
        let visible_ids = table
            .entries
            .iter()
            .take_while(|entry| entry.class_id == 254)
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
            if candidate_entries.iter().any(|entry| entry.class_id != 214) {
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

pub(crate) fn affected_kind(kind: crate::feature::AffectedIdKind) -> &'static str {
    match kind {
        crate::feature::AffectedIdKind::Geometry => "geometry",
        crate::feature::AffectedIdKind::Edges => "edges",
        crate::feature::AffectedIdKind::StrongParents => "strong_parents",
        crate::feature::AffectedIdKind::Parents => "parents",
        crate::feature::AffectedIdKind::Contours => "contours",
        crate::feature::AffectedIdKind::Quilts => "quilts",
    }
}

pub(crate) fn extent_source(source: crate::feature::ReplayExtentSource) -> &'static str {
    match source {
        crate::feature::ReplayExtentSource::Explicit => "explicit",
        crate::feature::ReplayExtentSource::Inherited => "inherited",
    }
}

pub(crate) fn half_edge_ref(id: crate::topology::HalfEdgeId) -> CreoHalfEdgeRef {
    CreoHalfEdgeRef {
        curve_id: id.curve_id,
        side: id.side,
    }
}

pub(crate) fn fc05_circle_records(scan: &ContainerScan) -> Vec<CreoFc05CircleRecord> {
    scan.curves
        .fc05_circles
        .iter()
        .map(|record| CreoFc05CircleRecord {
            id: format!("creo:curve:fc05_circle#{}", record.curve_id),
            curve_id: record.curve_id,
            center_row_frame: record.center_row_frame,
            radius_mm: record.radius_mm,
            sample_direction_row_frame: record.sample_direction_row_frame,
            reference_direction_row_frame: record.reference_direction_row_frame,
            parameter_sign: record.parameter_sign,
            cap_ordinate_row_frame: record.cap_ordinate_row_frame,
            point_count: record.point_count,
            max_residual: record.max_residual,
            angle_parameter_consistent: record.angle_parameter_consistent,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(crate) fn fc05_cylinder_cap_pair_records(
    scan: &ContainerScan,
) -> Vec<CreoFc05CylinderCapPairRecord> {
    scan.curves
        .fc05_cylinder_cap_pairs
        .iter()
        .map(|record| CreoFc05CylinderCapPairRecord {
            id: format!("creo:surface:fc05_cylinder_cap_pair#{}", record.surface_id),
            surface_id: record.surface_id,
            curve_ids: record.curve_ids.clone(),
            cap_plane_ids: record.cap_plane_ids.clone(),
            curve_cap_ordinates_row_frame: record.curve_cap_ordinates_row_frame.clone(),
            center_row_frame: record.center_row_frame,
            radius_mm: record.radius_mm,
            reference_direction_row_frame: record.reference_direction_row_frame,
            parameter_sign: record.parameter_sign,
            cap_ordinates_row_frame: record.cap_ordinates_row_frame.clone(),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}
