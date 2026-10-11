// SPDX-License-Identifier: Apache-2.0
//! Recovered projections release discarded values and temporary diagnostics.

use crate::chunks::{ArchiveVersion, FramingError};
use crate::loss::{RhinoLossCode, ScratchVec};
use crate::presentation;
use crate::test_support::test_dump::{anonymous_chunk, crc_chunk, utf16_bytes};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const STORAGE_LIMIT: u64 = 16_384;

#[test]
fn malformed_rdk_classification_releases_errors_and_omits_discarded_wrapper() {
    let bytes = super::legacy_rdk_payload("<xml><broken></xml>", false, &[]);
    let userdata = [super::legacy_rdk_descriptor(0..bytes.len())];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = STORAGE_LIMIT;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "Rhino classify_rdk_material_payload text",
        None,
    );
    for _ in 0..512 {
        assert_eq!(
            presentation::legacy_rdk_material_instance_id(&ctx, &bytes, &userdata).unwrap(),
            None
        );
        assert!(
            presentation::rdk_material_userdata_requires_opaque(&ctx, &bytes, &userdata).unwrap()
        );
        let released = ctx
            .reserve_scoped(STORAGE_LIMIT, "RDK classification storage released")
            .unwrap();
        drop(released);
    }
    drop(probe);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn direct_rdk_reader_keeps_the_complete_malformed_xml_diagnostic() {
    let xml = "<xml><broken></xml>";
    let bytes = super::legacy_rdk_payload(xml, false, &[]);
    let error = presentation::parse_legacy_rdk_material_instance_id(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0..bytes.len(),
    )
    .unwrap_err();
    let expected = format!(
        "legacy RDK XML is malformed: {}",
        roxmltree::Document::parse(xml).unwrap_err()
    );
    assert!(
        matches!(error, FramingError::Structural { offset: 0, message } if message == expected)
    );
}

#[test]
fn successful_rdk_classification_releases_tree_storage_after_uuid_selection() {
    let bytes = super::legacy_rdk_payload(
        "<xml><render-content-manager-data><material instance-id=\"11111111-1111-1111-1111-111111111111\"/></render-content-manager-data></xml>",
        false, &[],
    );
    let userdata = [super::legacy_rdk_descriptor(0..bytes.len())];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = STORAGE_LIMIT;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert_eq!(
        presentation::legacy_rdk_material_instance_id(&ctx, &bytes, &userdata).unwrap(),
        Some(crate::wire::Uuid::from_canonical([0x11; 16]))
    );
    assert!(!presentation::rdk_material_userdata_requires_opaque(&ctx, &bytes, &userdata).unwrap());
    let released = ctx
        .reserve_scoped(STORAGE_LIMIT, "RDK tree storage released")
        .unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}

fn rejected_user_strings() -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let mut entry = utf16_bytes("key");
    entry.extend(utf16_bytes(&"v".repeat(4096)));
    let mut body = 2_i32.to_le_bytes().to_vec();
    body.extend(anonymous_chunk(archive, 0, &entry));
    let mut invalid = 2_i32.to_le_bytes().to_vec();
    invalid.extend(0_i32.to_le_bytes());
    body.extend(crc_chunk(archive, 0x4000_8000, &invalid));
    anonymous_chunk(archive, 0, &body)
}

#[test]
fn recovered_user_strings_release_children_while_the_parent_scope_survives() {
    let bytes = rejected_user_strings();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = STORAGE_LIMIT;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut parent = ctx.reserve_scoped(0, "user-string parent record").unwrap();
    let mut losses = ScratchVec::new(&ctx, "user-string durable losses").unwrap();
    for _ in 0..2 {
        let records = parent
            .with_storage(|| {
                presentation::read_user_string_records(
                    &ctx,
                    &bytes,
                    ArchiveVersion::V8,
                    Some(0..bytes.len()),
                    42,
                    presentation::UserStringSource::Object,
                    &mut losses,
                )
            })
            .unwrap();
        assert!(records.is_empty());
    }
    assert_eq!(losses.len(), 2);
    assert!(losses.iter().all(
        |loss| loss.code == RhinoLossCode::ObjectDecodeDiagnostic.kind()
            && loss
                .message
                .contains("object user-string userdata at offset 42 could not be transferred:")
            && loss
                .message
                .ends_with("user-string entry version is unsupported")
    ));
    drop(losses);
    let released = ctx
        .reserve_scoped(STORAGE_LIMIT, "discarded user-string children released")
        .unwrap();
    drop(released);
    drop(parent);
    ctx.finish_session().unwrap();
}

fn empty_attributes() -> crate::objects::ObjectAttributes {
    let bytes = crate::test_support::test_dump::tagged_attributes(&[], 0);
    crate::objects::parse_attributes(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut crate::loss::Diagnostics::new(),
    )
    .unwrap()
}

#[test]
fn recovered_object_rendering_keeps_only_the_surviving_projection_storage() {
    let bytes = super::object_rendering_with_negative_minor(3, 0, Some(-1), None);
    let mut attributes = empty_attributes();
    attributes.rendering_range = Some(0..bytes.len());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = STORAGE_LIMIT;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut parent = ctx.reserve_scoped(0, "object presentation parent").unwrap();
    let mut losses = ScratchVec::new(&ctx, "object durable losses").unwrap();
    let projected = parent
        .with_storage(|| {
            presentation::object_attributes_presentation(
                &ctx,
                &bytes,
                &attributes,
                &[],
                &[],
                presentation::ObjectPresentationSource {
                    archive: ArchiveVersion::V8,
                    offset: 42,
                    uuid: attributes.object_id,
                },
                &mut losses,
            )
        })
        .unwrap();
    assert!(projected.rendering_materials.is_empty());
    assert!(projected.rendering_mappings.is_empty());
    assert!(projected.name.is_empty() && projected.url.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        RhinoLossCode::PresentationRecordDropped.kind()
    );
    assert!(losses[0]
        .message
        .contains("object rendering attributes at offset 42 could not be transferred:"));
    assert!(losses[0]
        .message
        .ends_with("rendering mapping minor version is negative"));
    drop(losses);
    let surviving = u64::try_from(projected.source_uuid.len()).unwrap();
    let released = ctx
        .reserve_scoped(
            STORAGE_LIMIT - surviving,
            "discarded rendering children released",
        )
        .unwrap();
    drop(released);
    drop(projected);
    drop(parent);
    ctx.finish_session().unwrap();
}

#[test]
fn recoverable_user_string_projection_propagates_actual_storage_refusal() {
    let bytes = rejected_user_strings();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "Rhino user-string key",
        None,
    );
    let mut losses = Vec::new();
    let result = presentation::read_user_string_records(
        &ctx,
        &bytes,
        ArchiveVersion::V8,
        Some(0..bytes.len()),
        42,
        presentation::UserStringSource::Object,
        &mut losses,
    );
    drop(probe);
    let Err(CodecError::ResourceLimit(limit)) = result else {
        panic!("actual key admission refusal");
    };
    assert_eq!(limit.operation, "Rhino user-string key");
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}

#[test]
fn texture_file_reference_diagnostics_release_before_record_handoff() {
    let mut bytes = super::texture_payload(2, &[]);
    let reference = crate::test_support::test_dump::file_reference(
        ArchiveVersion::V8,
        "/full/source.3dm",
        "source.3dm",
    );
    let start = bytes
        .windows(reference.len())
        .position(|window| window == reference)
        .unwrap();
    bytes[start + reference.len() - 1] ^= 1;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = STORAGE_LIMIT;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut parent = ctx.reserve_scoped(0, "texture parent record").unwrap();
    let mut losses = ScratchVec::new(&ctx, "texture durable losses").unwrap();
    let texture = parent
        .with_storage(|| {
            presentation::parse_texture(
                &ctx,
                &bytes,
                0..bytes.len(),
                ArchiveVersion::V8,
                42,
                &mut losses,
            )
        })
        .unwrap();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, RhinoLossCode::IntegrityFailure.kind());
    assert_eq!(
        losses[0].provenance.as_ref().unwrap().offset,
        u64::try_from(start).unwrap()
    );
    assert_eq!(texture.legacy_file_path, "texture.png");
    let reference = texture.file_reference.as_ref().unwrap();
    assert_eq!(reference.full_path, "/full/source.3dm");
    let surviving = texture.legacy_file_path.len()
        + texture.source_uuid.as_ref().map_or(0, String::len)
        + texture
            .transparency_texture_uuid
            .as_ref()
            .map_or(0, String::len)
        + reference.full_path.len()
        + reference.relative_path.len()
        + reference.name_sha1.len()
        + reference.content_sha1.len()
        + reference.embedded_file_uuid.as_ref().map_or(0, String::len);
    drop(losses);
    let released = ctx
        .reserve_scoped(
            STORAGE_LIMIT - u64::try_from(surviving).unwrap(),
            "temporary texture diagnostics released",
        )
        .unwrap();
    drop(released);
    drop(texture);
    drop(parent);
    ctx.finish_session().unwrap();
}

fn assert_text_style_component_storage(minor: i32) {
    let archive = ArchiveVersion::V8;
    let mut component = 13_u32.to_le_bytes().to_vec();
    component.extend([0x11; 16]);
    component.extend(7_i32.to_le_bytes());
    let original = "c".repeat(4096);
    component.extend(utf16_bytes(&original));
    let mut body = anonymous_chunk(archive, 0, &component);
    body.extend([0, 0]);
    if minor >= 1 {
        body.extend([0x22; 16]);
        body.extend(utf16_bytes("final"));
    }
    let bytes = anonymous_chunk(archive, minor, &body);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = STORAGE_LIMIT;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let (style, storage) = ctx
        .with_scoped_storage("text style parent record", || {
            presentation::parse_text_style(
                &ctx,
                &bytes,
                presentation::TextStyleParseInput {
                    range: 0..bytes.len(),
                    archive,
                    writer_version: None,
                    apple_runtime: false,
                    source_offset: 42,
                },
                &mut Vec::new(),
            )
        })
        .unwrap();
    assert_eq!(style.archive_index, Some(7));
    assert_eq!(style.name, if minor >= 1 { "final" } else { &original });
    assert_eq!(
        style.source_uuid,
        Some(
            crate::wire::Uuid::from_wire(if minor >= 1 { [0x22; 16] } else { [0x11; 16] },)
                .to_string()
        )
    );
    let surviving =
        style.id.len() + style.source_uuid.as_ref().map_or(0, String::len) + style.name.len();
    let released = ctx
        .reserve_scoped(
            STORAGE_LIMIT - u64::try_from(surviving).unwrap(),
            "unused text style component released",
        )
        .unwrap();
    drop(released);
    drop(style);
    drop(storage);
    ctx.finish_session().unwrap();
}

#[test]
fn modern_text_style_releases_the_overridden_component_name() {
    assert_text_style_component_storage(1);
}

#[test]
fn original_text_style_keeps_the_selected_component_name_admitted() {
    assert_text_style_component_storage(0);
}

#[test]
fn recoverable_object_rendering_propagates_actual_storage_refusal() {
    let bytes = super::object_rendering_with_negative_minor(3, 0, Some(-1), None);
    let mut attributes = empty_attributes();
    attributes.rendering_range = Some(0..bytes.len());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "Rhino rendering material plugin UUID",
        None,
    );
    let mut losses = Vec::new();
    let result = presentation::object_attributes_presentation(
        &ctx,
        &bytes,
        &attributes,
        &[],
        &[],
        presentation::ObjectPresentationSource {
            archive: ArchiveVersion::V8,
            offset: 42,
            uuid: attributes.object_id,
        },
        &mut losses,
    );
    drop(probe);
    let Err(CodecError::ResourceLimit(limit)) = result else {
        panic!("actual rendering admission refusal");
    };
    assert_eq!(limit.operation, "Rhino rendering material plugin UUID");
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}
