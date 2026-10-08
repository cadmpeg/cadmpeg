// SPDX-License-Identifier: Apache-2.0
//! Preserve passthrough PSB sections and emit legacy persistence arenas.

use crate::container::SectionRole;

use crate::decode::native::CreoArena;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use serde::Serialize;

use crate::container::{self, ContainerScan};

use super::super::native::annotate;
use super::super::native::emit_arena;
use cadmpeg_ir::unknown::UnknownRecord;

/// Retain every PSB geometry and thumbnail section as an unknown record.
///
/// A section whose declared extent runs past the scanned buffer is a refusal
/// naming the section, its declared end, and the buffer length. The declared
/// extent is the record, so a shortened region would retain bytes the source
/// never stated.
pub(super) fn preserve_passthrough_sections(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut AnnotationBuilder,
) -> Result<Vec<UnknownRecord>, CodecError> {
    let mut unknowns = Vec::new();
    let mut sections = scan.framing.sections.iter();
    while let Some(section) = ctx.next_charged(&mut sections, "creo passthrough sections")? {
        if section.role() != SectionRole::PsbGeometry && section.role() != SectionRole::Thumbnail {
            continue;
        }
        let Some(section_bytes) = container::section_region(&scan.framing.data, section) else {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!(
                    "creo section `{}` declares the region {}..{}, past the scanned file length {}",
                    section.name(),
                    section.offset(),
                    section.end(),
                    scan.framing.data.len(),
                ),
                "creo passthrough section bounds error",
            )?));
        };
        let payload_start = section
            .raw_name()
            .len()
            .checked_add(2)
            .ok_or_else(|| CodecError::malformed("section payload start exceeds usize"))?;
        let raw_is_compressed = section_bytes
            .get(payload_start..)
            .is_some_and(|payload| payload.starts_with(container::UNIX_COMPRESS_MAGIC));
        let (bytes, offset, tag, exactness) = if section.role() == SectionRole::Thumbnail {
            if raw_is_compressed {
                let Some(expanded) = container::expanded_section_for(ctx, scan, section)? else {
                    continue;
                };
                let Some(marker_offset) = ctx.find_bytes(
                    &expanded.data,
                    container::JPEG_MAGIC,
                    "creo compressed thumbnail marker search",
                )?
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
                let Some(marker_offset) = ctx.find_bytes(
                    section_bytes,
                    container::JPEG_MAGIC,
                    "creo thumbnail marker search",
                )?
                else {
                    continue;
                };
                (
                    &section_bytes[marker_offset..],
                    // `marker_offset` is a position inside the section's own
                    // payload, so the sum is a byte offset of the file.
                    section.offset() + marker_offset,
                    "jpeg_thumbnail",
                    Exactness::ByteExact,
                )
            }
        } else {
            (
                section_bytes,
                section.offset(),
                "psb_geometry_section",
                Exactness::Unknown,
            )
        };
        let namespace = crate::identity::section_namespace(section.name())
            .ok_or_else(|| CodecError::malformed("invalid Creo passthrough section namespace"))?;
        let id = crate::identity::compose_checked::<UnknownId>(
            ctx,
            &namespace,
            offset,
            "creo passthrough section identity",
        )?;
        annotate(
            ctx,
            annotations,
            &id,
            section.name(),
            cadmpeg_core::decode::u64_from_index(offset),
            tag,
            exactness,
        )?;
        ctx.reserve_vec(&mut unknowns, 1, "creo passthrough unknown records")?;
        unknowns.push(UnknownRecord::retained(
            id,
            cadmpeg_core::decode::u64_from_index(offset),
            ctx.copy_retained(bytes, "retain Creo passthrough section")?,
            Vec::new(),
        ));
    }
    Ok(unknowns)
}

fn legacy_source_stream<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    offset: usize,
) -> Result<&'a str, CodecError> {
    Ok(ctx
        .find_by(
            &scan.framing.sections,
            |section| Ok(section.contains(offset)),
            "creo legacy source stream sections",
        )?
        .map_or("legacy_ascii", |section| section.name()))
}

fn emit_legacy_value_arena<K: crate::legacy::LegacyCode>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    key: CreoArena,
    records: &[crate::legacy::ValueRecord<K>],
    tag: &str,
) -> Result<(), CodecError>
where
    K::Payload: Serialize,
{
    emit_arena(ctx, ir, annotations, key, records, |annotations, record| {
        annotate(
            ctx,
            annotations,
            record.id(),
            legacy_source_stream(ctx, scan, record.offset)?,
            cadmpeg_core::decode::u64_from_index(record.offset),
            tag,
            Exactness::ByteExact,
        )?;
        Ok(())
    })
}

pub(super) fn emit_legacy_arenas(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let Some(legacy) = scan.framing.layout.legacy_ascii() else {
        return Ok(());
    };
    emit_arena(
        ctx,
        ir,
        annotations,
        CreoArena::LegacyObjects,
        &legacy.persistence.objects,
        |annotations, record| {
            annotate(
                ctx,
                annotations,
                record.id(),
                legacy_source_stream(ctx, scan, record.offset)?,
                cadmpeg_core::decode::u64_from_index(record.offset),
                "legacy_type_0_object",
                Exactness::ByteExact,
            )?;
            Ok(())
        },
    )?;
    emit_legacy_value_arena(
        ctx,
        scan,
        ir,
        annotations,
        CreoArena::LegacyIntegerValues,
        &legacy.persistence.integer_values.rows,
        "legacy_type_1_integer",
    )?;
    emit_legacy_value_arena(
        ctx,
        scan,
        ir,
        annotations,
        CreoArena::LegacyRealValues,
        &legacy.persistence.real_values.rows,
        "legacy_type_2_real",
    )?;
    emit_legacy_value_arena(
        ctx,
        scan,
        ir,
        annotations,
        CreoArena::LegacyType3Values,
        &legacy.persistence.type_3_values.rows,
        "legacy_type_3_value",
    )?;
    emit_legacy_value_arena(
        ctx,
        scan,
        ir,
        annotations,
        CreoArena::LegacyType4Values,
        &legacy.persistence.type_4_values.rows,
        "legacy_type_4_value",
    )?;
    emit_legacy_value_arena(
        ctx,
        scan,
        ir,
        annotations,
        CreoArena::LegacyStringValues,
        &legacy.persistence.string_values,
        "legacy_type_10_string",
    )?;
    emit_legacy_value_arena(
        ctx,
        scan,
        ir,
        annotations,
        CreoArena::LegacyType5Values,
        &legacy.persistence.type_5_values.rows,
        "legacy_type_5_value",
    )?;
    emit_legacy_value_arena(
        ctx,
        scan,
        ir,
        annotations,
        CreoArena::LegacyType6Values,
        &legacy.persistence.type_6_values.rows,
        "legacy_type_6_value",
    )?;
    emit_legacy_value_arena(
        ctx,
        scan,
        ir,
        annotations,
        CreoArena::LegacyType7Values,
        &legacy.persistence.type_7_values.rows,
        "legacy_type_7_value",
    )?;
    emit_legacy_value_arena(
        ctx,
        scan,
        ir,
        annotations,
        CreoArena::LegacyType9Values,
        &legacy.persistence.type_9_values.rows,
        "legacy_type_9_value",
    )?;
    emit_legacy_value_arena(
        ctx,
        scan,
        ir,
        annotations,
        CreoArena::LegacyType11Values,
        &legacy.persistence.type_11_values.rows,
        "legacy_type_11_value",
    )?;
    if let Some(table) = &scan.framing.legacy_family_table {
        emit_arena(
            ctx,
            ir,
            annotations,
            CreoArena::ConfigurationDriverTables,
            std::slice::from_ref(table),
            |annotations, record| {
                annotate(
                    ctx,
                    annotations,
                    record.id(),
                    legacy_source_stream(ctx, scan, record.offset)?,
                    cadmpeg_core::decode::u64_from_index(record.offset),
                    "legacy_configuration_driver_table",
                    Exactness::ByteExact,
                )?;
                Ok(())
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{legacy_source_stream, preserve_passthrough_sections};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    #[test]
    fn passthrough_sections_refuse_work_limit() {
        let mut scan = crate::test_support::empty_container_scan();
        scan.framing.sections.push(
            crate::container::Section::scan_for_test(
                "ND:0:VisibGeom:0".to_owned(),
                0,
                48,
                None,
                &[0u8; 48],
            )
            .expect("section extent")
            .section,
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error =
            preserve_passthrough_sections(&ctx, &scan, &mut cadmpeg_ir::AnnotationBuilder::new())
                .expect_err("section traversal exceeds work limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo passthrough sections")
        );
    }

    #[test]
    fn passthrough_legacy_source_stream_refuses_work_limit() {
        let mut scan = crate::test_support::empty_container_scan();
        scan.framing.sections.push(
            crate::container::Section::scan_for_test(
                "ND:0:VisibGeom:0".to_owned(),
                0,
                48,
                None,
                &[0u8; 48],
            )
            .expect("section extent")
            .section,
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error =
            legacy_source_stream(&ctx, &scan, 1).expect_err("source search exceeds work limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo legacy source stream sections")
        );
        crate::decode::with_test_decode_ctx(|ctx| {
            assert_eq!(legacy_source_stream(ctx, &scan, 1)?, "VisibGeom");
            assert_eq!(legacy_source_stream(ctx, &scan, 49)?, "legacy_ascii");
            Ok::<(), cadmpeg_core::CodecError>(())
        })
        .expect("service source searches admitted");
    }

    #[test]
    fn passthrough_unknown_record_refuses_collection_limit() {
        let mut scan = crate::test_support::empty_container_scan();
        scan.framing.data = vec![0u8; 48].into();
        scan.framing.sections.push(
            crate::container::Section::scan_for_test(
                "ND:0:VisibGeom:0".to_owned(),
                0,
                48,
                None,
                &[0u8; 48],
            )
            .expect("section extent")
            .section,
        );
        let error = crate::test_support::last_refusal_at(
            &[], cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo passthrough unknown records",
            |ctx| preserve_passthrough_sections(ctx, &scan, &mut cadmpeg_ir::AnnotationBuilder::new()),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == "creo passthrough unknown records")
        );
        let records = crate::decode::with_test_decode_ctx(|ctx| {
            preserve_passthrough_sections(ctx, &scan, &mut cadmpeg_ir::AnnotationBuilder::new())
        })
        .expect("service records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id().as_str(), "creo:VisibGeom:section#0");
    }

    #[test]
    fn passthrough_section_bounds_error_refuses_retained_limit() {
        let section = crate::container::Section::scan_for_test(
            "ND:0:VisibGeom:0".to_owned(),
            32,
            48,
            None,
            &[0u8; 48],
        )
        .expect("section extent")
        .section;
        let mut scan = crate::test_support::empty_container_scan();
        scan.framing.data = vec![0u8; 16].into();
        scan.framing.sections.push(section);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error =
            preserve_passthrough_sections(&ctx, &scan, &mut cadmpeg_ir::AnnotationBuilder::new())
                .expect_err("bounds error text exceeds retained limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo passthrough section bounds error")
        );
        crate::decode::with_test_decode_ctx(|ctx| {
            let error = preserve_passthrough_sections(
                ctx,
                &scan,
                &mut cadmpeg_ir::AnnotationBuilder::new(),
            )
            .expect_err("declared section exceeds scanned bytes");
            assert!(error.to_string().contains("VisibGeom"));
            Ok::<(), cadmpeg_core::CodecError>(())
        })
        .expect("service error text admitted");
    }

    #[test]
    fn passthrough_bounds_refusal_admits_only_the_first_visited_section() {
        use cadmpeg_core::CodecError;
        use cadmpeg_core::decode::ResourceDimension;
        let section = crate::container::Section::scan_for_test(
            "ND:0:VisibGeom:0".to_owned(), 32, 48, None, &[0u8; 48],
        ).expect("section extent").section;
        let mut scan = crate::test_support::empty_container_scan();
        scan.framing.data = vec![0u8; 16].into();
        scan.framing.sections = vec![section.clone(), section];
        let error = crate::test_support::last_refusal_at(
            &[], ResourceDimension::WorkUnits, "creo passthrough sections",
            |ctx| match preserve_passthrough_sections(ctx, &scan, &mut cadmpeg_ir::AnnotationBuilder::new()) {
                Err(error @ CodecError::ResourceLimit(_)) => Err(error),
                result => Ok(result),
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(resource) if resource.operation == "creo passthrough sections" && resource.additional == 1));
        crate::decode::with_test_decode_ctx(|ctx| {
            let error = preserve_passthrough_sections(ctx, &scan, &mut cadmpeg_ir::AnnotationBuilder::new()).expect_err("the first section is outside the source extent");
            assert!(matches!(error, CodecError::Malformed(message) if message.contains("VisibGeom")));
        });
    }
}
