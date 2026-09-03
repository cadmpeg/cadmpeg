// SPDX-License-Identifier: Apache-2.0
//! Preserve passthrough PSB sections and emit legacy persistence arenas.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use serde::Serialize;

use crate::container::{self, role, ContainerScan};

use super::super::native::annotate;
use super::super::native::emit_arena;
use cadmpeg_ir::unknown::UnknownRecord;

pub(in super::super) fn preserve_passthrough_sections(
    scan: &ContainerScan,
    annotations: &mut AnnotationBuilder,
) -> Vec<UnknownRecord> {
    let mut unknowns = Vec::new();
    for section in scan
        .framing
        .sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY || section.role == role::THUMBNAIL)
    {
        let end = (section.offset + section.length).min(scan.framing.data.len());
        let section_bytes = &scan.framing.data[section.offset..end];
        let payload_start = section.raw_name.len().saturating_add(2);
        let raw_is_compressed = section_bytes
            .get(payload_start..)
            .is_some_and(|payload| payload.starts_with(container::UNIX_COMPRESS_MAGIC));
        let (bytes, offset, tag, exactness) = if section.role == role::THUMBNAIL {
            if raw_is_compressed {
                let Some(expanded) = container::expanded_section_for(scan, section) else {
                    continue;
                };
                let Some(marker_offset) = expanded
                    .data
                    .windows(3)
                    .position(|window| window == container::JPEG_MAGIC)
                else {
                    continue;
                };
                (
                    &expanded.data[marker_offset..],
                    expanded.source_offset,
                    "jpeg_thumbnail",
                    Exactness::Derived,
                )
            } else {
                let Some(marker_offset) = section_bytes
                    .windows(3)
                    .position(|window| window == container::JPEG_MAGIC)
                else {
                    continue;
                };
                (
                    &section_bytes[marker_offset..],
                    section.offset.saturating_add(marker_offset),
                    "jpeg_thumbnail",
                    Exactness::ByteExact,
                )
            }
        } else {
            (
                section_bytes,
                section.offset,
                "psb_geometry_section",
                Exactness::Unknown,
            )
        };
        let id = UnknownId(format!("creo:{}:section#{}", section.name, offset));
        annotate(
            annotations,
            &id,
            &section.name,
            offset as u64,
            tag,
            exactness,
        );
        unknowns.push(UnknownRecord::retained(
            id,
            offset as u64,
            bytes.to_vec(),
            Vec::new(),
        ));
    }
    unknowns
}

pub(in super::super) fn legacy_source_stream<'a>(
    scan: &'a ContainerScan<'_>,
    offset: usize,
) -> &'a str {
    scan.framing
        .sections
        .iter()
        .find(|section| {
            offset >= section.offset && offset < section.offset.saturating_add(section.length)
        })
        .map_or("legacy_ascii", |section| section.name.as_str())
}

pub(in super::super) fn emit_legacy_value_arena<T: Serialize>(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    key: &str,
    records: &[crate::legacy::ValueRecord<T>],
    tag: &str,
) -> Result<(), CodecError> {
    emit_arena(ir, annotations, key, records, |annotations, record| {
        annotate(
            annotations,
            &record.id,
            legacy_source_stream(scan, record.offset),
            record.offset as u64,
            tag,
            Exactness::ByteExact,
        );
    })
}

pub(in super::super) fn emit_legacy_arenas(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let Some(legacy) = &scan.framing.legacy_ascii else {
        return Ok(());
    };
    emit_arena(
        ir,
        annotations,
        "legacy_objects",
        &legacy.persistence.objects,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                legacy_source_stream(scan, record.offset),
                record.offset as u64,
                "legacy_type_0_object",
                Exactness::ByteExact,
            );
        },
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_integer_values",
        &legacy.persistence.integer_values,
        "legacy_type_1_integer",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_real_values",
        &legacy.persistence.real_values,
        "legacy_type_2_real",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_3_values",
        &legacy.persistence.type_3_values,
        "legacy_type_3_value",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_4_values",
        &legacy.persistence.type_4_values,
        "legacy_type_4_value",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_string_values",
        &legacy.persistence.string_values,
        "legacy_type_10_string",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_5_values",
        &legacy.persistence.type_5_values,
        "legacy_type_5_value",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_6_values",
        &legacy.persistence.type_6_values,
        "legacy_type_6_value",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_7_values",
        &legacy.persistence.type_7_values,
        "legacy_type_7_value",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_9_values",
        &legacy.persistence.type_9_values,
        "legacy_type_9_value",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_11_values",
        &legacy.persistence.type_11_values,
        "legacy_type_11_value",
    )?;
    if let Some(table) = &scan.framing.legacy_family_table {
        emit_arena(
            ir,
            annotations,
            "configuration_driver_tables",
            std::slice::from_ref(table),
            |annotations, record| {
                annotate(
                    annotations,
                    &record.id,
                    legacy_source_stream(scan, record.offset),
                    record.offset as u64,
                    "legacy_configuration_driver_table",
                    Exactness::ByteExact,
                );
            },
        )?;
    }
    Ok(())
}
