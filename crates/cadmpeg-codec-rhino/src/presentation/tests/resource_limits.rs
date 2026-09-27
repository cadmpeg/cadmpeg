// SPDX-License-Identifier: Apache-2.0

use super::{
    anonymous, bitmap_header, embedded_bitmap_payload, legacy_text_style_bytes, light_payload,
    model_attributes_status_chunk, modern_font_chunk, object_rendering_with_negative_minor,
    stored_bitmap_buffer, texture_payload, utf16_bytes, v5_dimension_style_chunk,
    v5_dimension_style_extra_chunk, windows_bitmap_payload,
};
use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::loss::Diagnostics;
use crate::presentation::rendering_attributes;
use crate::presentation::TextStyleParseInput;
use crate::settings;
use crate::wire::Uuid;
use std::collections::HashMap;
use std::sync::OnceLock;

#[test]
fn legacy_component_name_refuses_retained_limit() {
    let mut body = 8_u32.to_le_bytes().to_vec();
    body.extend(utf16_bytes("name"));
    let bytes = anonymous(0, &body);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("component root admitted");
    let error = crate::presentation::component(
        &ctx,
        &bytes,
        &mut BoundedReader::new(&bytes, 0, bytes.len()).expect("component bounds"),
        ArchiveVersion::V8,
    )
    .expect_err("component name exceeds retained limit");
    assert!(
        matches!(error, FramingError::Resource(refusal) if refusal.operation == "Rhino component name")
    );
}

#[test]
fn modern_component_name_refuses_retained_limit() {
    let bytes = model_attributes_status_chunk([3, 2, 3, 2, 1], "name", &[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("component root admitted");
    let error = crate::presentation::component(
        &ctx,
        &bytes,
        &mut BoundedReader::new(&bytes, 0, bytes.len()).expect("component bounds"),
        ArchiveVersion::V8,
    )
    .expect_err("component name exceeds retained limit");
    assert!(
        matches!(error, FramingError::Resource(refusal) if refusal.operation == "Rhino component name")
    );
}

fn group_refusal(limit: u64) -> FramingError {
    let mut bytes = vec![0x1f];
    bytes.extend(7_i32.to_le_bytes());
    bytes.extend(utf16_bytes("fixtures"));
    bytes.extend([0x44; 16]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("group root admitted");
    crate::presentation::parse_group(&ctx, &bytes, 0..bytes.len(), 120)
        .expect_err("group retained values exceed limit")
}

#[test]
fn group_name_refuses_retained_limit() {
    assert!(
        matches!(group_refusal(0), FramingError::Resource(refusal) if refusal.operation == "Rhino group name")
    );
}

#[test]
fn group_id_refuses_retained_limit() {
    assert!(
        matches!(group_refusal(8), FramingError::Resource(refusal) if refusal.operation == "Rhino group ID")
    );
}

#[test]
fn group_uuid_refuses_retained_limit() {
    let id_len = "rhino:presentation:group#44444444-4444-4444-4444-444444444444".len();
    assert!(
        matches!(group_refusal(u64::try_from(8 + id_len).expect("budget fits")), FramingError::Resource(refusal) if refusal.operation == "Rhino group source UUID")
    );
}

fn duplicate_groups() -> Vec<crate::presentation::GroupRecord> {
    let mut bytes = vec![0x1f];
    bytes.extend(7_i32.to_le_bytes());
    bytes.extend(utf16_bytes("fixtures"));
    bytes.extend([0x44; 16]);
    let ctx = cadmpeg_test_support::service_decode_context();
    vec![
        crate::presentation::parse_group(&ctx, &bytes, 0..bytes.len(), 120)
            .expect("first group admitted"),
        crate::presentation::parse_group(&ctx, &bytes, 0..bytes.len(), 240)
            .expect("second group admitted"),
    ]
}

#[test]
fn group_identity_workspace_refuses_materialized_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = crate::presentation::disambiguate_group_ids(&ctx, &mut duplicate_groups())
        .expect_err("identity workspace exceeds materialized limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino group identity workspace")
    );
}

#[test]
fn group_identity_count_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = crate::presentation::disambiguate_group_ids(&ctx, &mut duplicate_groups())
        .expect_err("identity map exceeds collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino group identity counts")
    );
}

#[test]
fn duplicate_group_indices_refuse_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = crate::presentation::disambiguate_group_ids(&ctx, &mut duplicate_groups())
        .expect_err("duplicate indices exceed collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino duplicate group indices")
    );
}

#[test]
fn disambiguated_group_id_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = crate::presentation::disambiguate_group_ids(&ctx, &mut duplicate_groups())
        .expect_err("disambiguated ID exceeds retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino disambiguated group ID")
    );
}

fn light_refusal(
    retained_limit: u64,
    collection_limit: u64,
    link_order: Option<usize>,
) -> FramingError {
    let bytes = light_payload(0x1f, 0.8);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = retained_limit;
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("light root admitted");
    crate::presentation::parse_light(
        &ctx,
        &bytes,
        0..bytes.len(),
        crate::settings::MillimeterScale::IDENTITY,
        0,
        link_order,
    )
    .expect_err("light value exceeds limit")
}

#[test]
fn light_name_refuses_retained_limit() {
    assert!(
        matches!(light_refusal(0, u64::MAX, None), FramingError::Resource(refusal) if refusal.operation == "Rhino light name")
    );
}

#[test]
fn light_id_refuses_retained_limit() {
    assert!(
        matches!(light_refusal(3, u64::MAX, None), FramingError::Resource(refusal) if refusal.operation == "Rhino light ID")
    );
}

#[test]
fn light_source_uuid_refuses_retained_limit() {
    let id_len = "rhino:presentation:light#55555555-5555-5555-5555-555555555555".len();
    assert!(
        matches!(light_refusal(u64::try_from(3 + id_len).expect("budget fits"), u64::MAX, None), FramingError::Resource(refusal) if refusal.operation == "Rhino light source UUID")
    );
}

#[test]
fn light_object_link_refuses_retained_limit() {
    assert!(
        matches!(light_refusal(3, u64::MAX, Some(7)), FramingError::Resource(refusal) if refusal.operation == "Rhino light object link")
    );
}

#[test]
fn light_links_refuse_collection_limit() {
    assert!(
        matches!(light_refusal(u64::MAX, 0, Some(7)), FramingError::Resource(refusal) if refusal.operation == "Rhino light links")
    );
}

fn push_light_refusal(
    collection_limit: u64,
    materialized_limit: u64,
    retained_limit: u64,
    duplicate: bool,
) -> cadmpeg_core::CodecError {
    let bytes = light_payload(0x1f, 0.8);
    let light = crate::presentation::parse_light(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0..bytes.len(),
        crate::settings::MillimeterScale::IDENTITY,
        0,
        None,
    )
    .expect("light fixture admitted");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_materialized_bytes = materialized_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let mut workspace = ctx
        .reserve_scoped(0, "Rhino light identity workspace")
        .expect("empty workspace admitted");
    let mut indexes = HashMap::new();
    if duplicate {
        indexes.insert(Uuid::from_canonical([0x55; 16]), 0);
    }
    crate::presentation::push_light(&ctx, &mut workspace, &mut Vec::new(), &mut indexes, light)
        .expect_err("light collection or identity exceeds limit")
}

#[test]
fn light_identity_workspace_refuses_materialized_limit() {
    assert!(
        matches!(push_light_refusal(u64::MAX, 0, u64::MAX, false), cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino light identity workspace")
    );
}

#[test]
fn light_identity_index_refuses_collection_limit() {
    assert!(
        matches!(push_light_refusal(0, u64::MAX, u64::MAX, false), cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino light identity index")
    );
}

#[test]
fn light_collection_refuses_collection_limit() {
    assert!(
        matches!(push_light_refusal(1, u64::MAX, u64::MAX, false), cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino lights")
    );
}

#[test]
fn duplicate_light_id_refuses_retained_limit() {
    assert!(
        matches!(push_light_refusal(u64::MAX, u64::MAX, 0, true), cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino duplicate light ID")
    );
}

fn embedded_image_refusal(limit: u64) -> FramingError {
    let bytes = embedded_bitmap_payload(1, Uuid::from_canonical([0x44; 16]), 0);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("image root admitted");
    crate::presentation::parse_embedded_image(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V8, 42)
        .expect_err("image retained value exceeds limit")
}

#[test]
fn embedded_image_path_refuses_retained_limit() {
    assert!(
        matches!(embedded_image_refusal(0), FramingError::Resource(refusal) if refusal.operation == "Rhino image path")
    );
}

#[test]
fn embedded_image_name_refuses_retained_limit() {
    assert!(
        matches!(embedded_image_refusal(9), FramingError::Resource(refusal) if refusal.operation == "Rhino image name")
    );
}

#[test]
fn embedded_image_id_refuses_retained_limit() {
    assert!(
        matches!(embedded_image_refusal(16), FramingError::Resource(refusal) if refusal.operation == "Rhino image ID")
    );
}

#[test]
fn embedded_image_source_uuid_refuses_retained_limit() {
    let id_len = "rhino:presentation:image#44444444-4444-4444-4444-444444444444".len();
    assert!(
        matches!(embedded_image_refusal(u64::try_from(16 + id_len).expect("budget fits")), FramingError::Resource(refusal) if refusal.operation == "Rhino image source UUID")
    );
}

#[test]
fn embedded_image_sha256_refuses_retained_limit() {
    let id_len = "rhino:presentation:image#44444444-4444-4444-4444-444444444444".len();
    assert!(
        matches!(embedded_image_refusal(u64::try_from(16 + id_len + 36).expect("budget fits")), FramingError::Resource(refusal) if refusal.operation == "Rhino image SHA-256")
    );
}

fn windows_bitmap_refusal(limit: u64, ex: bool) -> FramingError {
    let class_uuid = if ex {
        crate::presentation::WINDOWS_BITMAP_EX
    } else {
        crate::presentation::WINDOWS_BITMAP
    };
    let pixels = [0x11; 24];
    let bytes = windows_bitmap_payload(
        class_uuid,
        0,
        if ex { "relative/example.bmp" } else { "" },
        bitmap_header(3, 2, 24, 24, 0),
        &[stored_bitmap_buffer(&pixels)],
        &[],
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("bitmap root admitted");
    crate::presentation::parse_windows_bitmap(
        &ctx,
        &bytes,
        0..bytes.len(),
        class_uuid,
        ArchiveVersion::V8,
        72,
    )
    .expect_err("bitmap retained value exceeds limit")
}

#[test]
fn windows_bitmap_path_refuses_retained_limit() {
    assert!(
        matches!(windows_bitmap_refusal(0, true), FramingError::Resource(refusal) if refusal.operation == "Rhino Windows bitmap path")
    );
}

#[test]
fn windows_bitmap_id_refuses_retained_limit() {
    assert!(
        matches!(windows_bitmap_refusal(0, false), FramingError::Resource(refusal) if refusal.operation == "Rhino Windows bitmap ID")
    );
}

#[test]
fn windows_bitmap_class_uuid_refuses_retained_limit() {
    let id_len = "rhino:presentation:windows_bitmap#offset-72".len();
    assert!(
        matches!(windows_bitmap_refusal(u64::try_from(id_len).expect("budget fits"), false), FramingError::Resource(refusal) if refusal.operation == "Rhino Windows bitmap class UUID")
    );
}

#[test]
fn windows_bitmap_sha256_refuses_retained_limit() {
    let id_len = "rhino:presentation:windows_bitmap#offset-72".len();
    assert!(
        matches!(windows_bitmap_refusal(u64::try_from(id_len + 36).expect("budget fits"), false), FramingError::Resource(refusal) if refusal.operation == "Rhino Windows bitmap SHA-256")
    );
}

fn bitmap_install_refusal(embedded: bool) -> cadmpeg_core::CodecError {
    let archive = ArchiveVersion::V8;
    let class_uuid = if embedded {
        crate::presentation::EMBEDDED_BITMAP
    } else {
        crate::presentation::WINDOWS_BITMAP
    };
    let payload = if embedded {
        embedded_bitmap_payload(0, Uuid::from_canonical([0x44; 16]), 0)
    } else {
        let pixels = [0x11; 24];
        windows_bitmap_payload(
            class_uuid,
            0,
            "",
            bitmap_header(3, 2, 24, 24, 0),
            &[stored_bitmap_buffer(&pixels)],
            &[],
        )
    };
    let class =
        crate::test_support::test_dump::class_wrapper(archive, class_uuid.to_wire(), &payload);
    let record = crate::test_support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_8090,
        &class,
        std::slice::from_ref(&(0..class.len())),
    );
    let bytes = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[]),
            crate::test_support::test_dump::table(
                archive,
                crate::presentation::BITMAP_TABLE,
                &[record],
            ),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );
    let scan = crate::container::scan_owned(bytes.clone()).expect("bitmap document scanned");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("bitmap root admitted");
    crate::presentation::install(&ctx, &scan, &mut cadmpeg_ir::document::CadIr::empty())
        .expect_err("bitmap collection exceeds limit")
}

#[test]
fn installed_embedded_image_refuses_collection_limit() {
    assert!(
        matches!(bitmap_install_refusal(true), cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino images")
    );
}

#[test]
fn installed_windows_bitmap_refuses_collection_limit() {
    assert!(
        matches!(bitmap_install_refusal(false), cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino Windows bitmaps")
    );
}

fn texture_mapping_payload() -> Vec<u8> {
    let mut body = crate::test_support::test_dump::MESH_CLASS.to_vec();
    body.extend(6_u32.to_le_bytes());
    body.extend(1_u32.to_le_bytes());
    for _ in 0..2 {
        for index in 0..16 {
            body.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
        }
    }
    body.extend(utf16_bytes("custom mesh mapping"));
    body.extend(crate::test_support::test_dump::class_wrapper(
        ArchiveVersion::V8,
        crate::test_support::test_dump::MESH_CLASS,
        &[],
    ));
    body.extend(0_u32.to_le_bytes());
    body.push(0);
    anonymous(1, &body)
}

fn texture_mapping_refusal(limit: u64) -> FramingError {
    let bytes = texture_mapping_payload();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("mapping root admitted");
    crate::presentation::parse_texture_mapping(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V8, 42)
        .err()
        .expect("mapping retained value exceeds limit")
}

#[test]
fn texture_mapping_name_refuses_retained_limit() {
    assert!(
        matches!(texture_mapping_refusal(0), FramingError::Resource(refusal) if refusal.operation == "Rhino texture mapping name")
    );
}

#[test]
fn texture_mapping_primitive_uuid_refuses_retained_limit() {
    assert!(
        matches!(texture_mapping_refusal(19), FramingError::Resource(refusal) if refusal.operation == "Rhino texture mapping primitive UUID")
    );
}

#[test]
fn texture_mapping_id_refuses_retained_limit() {
    assert!(
        matches!(texture_mapping_refusal(55), FramingError::Resource(refusal) if refusal.operation == "Rhino texture mapping ID")
    );
}

#[test]
fn texture_mapping_source_uuid_refuses_retained_limit() {
    let id_len = "rhino:presentation:texture_mapping#00000000-0000-0000-0000-000000000000".len();
    assert!(
        matches!(texture_mapping_refusal(u64::try_from(55 + id_len).expect("budget fits")), FramingError::Resource(refusal) if refusal.operation == "Rhino texture mapping source UUID")
    );
}

#[test]
fn installed_texture_mapping_refuses_collection_limit() {
    let archive = ArchiveVersion::V8;
    let payload = texture_mapping_payload();
    let class = crate::test_support::test_dump::class_wrapper(
        archive,
        crate::presentation::TEXTURE_MAPPING.to_wire(),
        &payload,
    );
    let record = crate::test_support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_807a,
        &class,
        std::slice::from_ref(&(0..class.len())),
    );
    let bytes = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0025, &[record]),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );
    let scan = crate::container::scan_owned(bytes.clone()).expect("mapping document scanned");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("mapping root admitted");
    let error =
        crate::presentation::install(&ctx, &scan, &mut cadmpeg_ir::document::CadIr::empty())
            .expect_err("mapping collection exceeds limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino texture mappings")
    );
}

fn rendering_mapping_refusal(limit: u64) -> FramingError {
    let mut channel_body = 7_i32.to_le_bytes().to_vec();
    channel_body.extend([0x11; 16]);
    channel_body.extend((0..16).flat_map(|value| f64::from(value).to_le_bytes()));
    let channel = anonymous(1, &channel_body);
    let mut mapping_body = vec![0x22; 16];
    mapping_body.extend(1_i32.to_le_bytes());
    mapping_body.extend(channel);
    let mapping = anonymous(0, &mapping_body);
    let mut body = 0_i32.to_le_bytes().to_vec();
    body.extend(1_i32.to_le_bytes());
    body.extend(mapping);
    body.extend([0, 0, 1]);
    let bytes = anonymous(3, &body);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("rendering root admitted");
    crate::presentation::rendering_attributes(
        &ctx,
        &bytes,
        Some(0..bytes.len()),
        ArchiveVersion::V8,
        settings::RenderingAttributesKind::Object,
    )
    .expect_err("rendering UUID exceeds retained limit")
}

#[test]
fn rendering_mapping_plugin_uuid_refuses_retained_limit() {
    assert!(
        matches!(rendering_mapping_refusal(0), FramingError::Resource(refusal) if refusal.operation == "Rhino rendering mapping plugin UUID")
    );
}

#[test]
fn rendering_channel_uuid_refuses_retained_limit() {
    assert!(
        matches!(rendering_mapping_refusal(36), FramingError::Resource(refusal) if refusal.operation == "Rhino rendering channel UUID")
    );
}

fn rendering_material_refusal(limit: u64) -> FramingError {
    let mut obsolete_channel_body = 7_i32.to_le_bytes().to_vec();
    obsolete_channel_body.extend([0x33; 16]);
    obsolete_channel_body.extend((0..16).flat_map(|value| f64::from(value).to_le_bytes()));
    let obsolete_channel = anonymous(1, &obsolete_channel_body);
    let mut material_body = vec![0x11; 16];
    material_body.extend([0x22; 16]);
    material_body.extend(1_i32.to_le_bytes());
    material_body.extend(obsolete_channel);
    material_body.extend([0x44; 16]);
    material_body.extend([3, 0, 0, 0]);
    let material = anonymous(1, &material_body);
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(material);
    body.extend(0_i32.to_le_bytes());
    body.extend([1, 1, 0]);
    let bytes = anonymous(3, &body);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("rendering root admitted");
    crate::presentation::rendering_attributes(
        &ctx,
        &bytes,
        Some(0..bytes.len()),
        ArchiveVersion::V8,
        settings::RenderingAttributesKind::Object,
    )
    .expect_err("rendering material UUID exceeds retained limit")
}

#[test]
fn rendering_material_plugin_uuid_refuses_retained_limit() {
    assert!(
        matches!(rendering_material_refusal(0), FramingError::Resource(refusal) if refusal.operation == "Rhino rendering material plugin UUID")
    );
}

#[test]
fn rendering_front_material_uuid_refuses_retained_limit() {
    assert!(
        matches!(rendering_material_refusal(36), FramingError::Resource(refusal) if refusal.operation == "Rhino rendering front material UUID")
    );
}

#[test]
fn rendering_back_material_uuid_refuses_retained_limit_after_obsolete_channel() {
    assert!(
        matches!(rendering_material_refusal(72), FramingError::Resource(refusal) if refusal.operation == "Rhino rendering back material UUID")
    );
}

fn v5_dimension_extra_refusal(collection_limit: u64, retained_limit: u64) -> FramingError {
    let bytes = v5_dimension_style_extra_chunk();
    let descriptor = crate::objects::ClassUserdata {
        range: 0..bytes.len(),
        version: (2, 2),
        class_uuid: crate::presentation::DIMSTYLE_EXTRA,
        item_uuid: crate::presentation::DIMSTYLE_EXTRA,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        save_context: None,
        payload_range: 0..bytes.len(),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("dimension extra root admitted");
    crate::presentation::parse_v5_dimension_style_extra(
        &ctx,
        &bytes,
        &descriptor,
        ArchiveVersion::V5,
        crate::settings::MillimeterScale::IDENTITY,
    )
    .expect_err("dimension extra exceeds limit")
}

#[test]
fn v5_dimension_valid_fields_refuse_collection_limit() {
    assert!(
        matches!(v5_dimension_extra_refusal(2, u64::MAX), FramingError::Resource(refusal) if refusal.operation == "Rhino V5 dimension valid fields")
    );
}

#[test]
fn v5_dimension_parent_uuid_refuses_retained_limit() {
    assert!(
        matches!(v5_dimension_extra_refusal(u64::MAX, 0), FramingError::Resource(refusal) if refusal.operation == "Rhino V5 dimension parent UUID")
    );
}

#[test]
fn v5_dimension_source_uuid_refuses_retained_limit() {
    assert!(
        matches!(v5_dimension_extra_refusal(u64::MAX, 36), FramingError::Resource(refusal) if refusal.operation == "Rhino V5 dimension source UUID")
    );
}

fn v5_dimension_style_refusal(limit: u64) -> FramingError {
    let bytes = v5_dimension_style_chunk();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("dimension root admitted");
    crate::presentation::parse_v5_dimension_style(
        &ctx,
        &bytes,
        0..bytes.len(),
        crate::settings::MillimeterScale::IDENTITY,
        321,
        None,
    )
    .expect_err("V5 dimension text exceeds limit")
}

macro_rules! v5_dimension_text_limit {
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(v5_dimension_style_refusal($limit), FramingError::Resource(refusal) if refusal.operation == $operation));
        }
    };
}

fn v5_dimension_prefix_budget() -> u64 {
    let keys = ["v5_version", "v5_arrow_type", "v5_angular_units"];
    u64::try_from(22 + keys.iter().map(|key| key.len()).sum::<usize>()).expect("prefix budget fits")
}

fn v5_dimension_id_budget() -> u64 {
    let later_keys = [
        "v5_length_factor",
        "v5_alternate_angle_format",
        "v5_alternate_angle_resolution",
        "v5_unused",
        "v5_leader_arrow_type",
    ];
    v5_dimension_prefix_budget()
        + 4
        + u64::try_from(later_keys.iter().map(|key| key.len()).sum::<usize>())
            .expect("ID budget fits")
}

v5_dimension_text_limit!(
    v5_dimension_name_refuses_retained_limit,
    0,
    "Rhino V5 dimension name"
);
v5_dimension_text_limit!(
    v5_dimension_control_key_refuses_retained_limit,
    22,
    "Rhino dimension control key"
);

#[test]
fn v5_dimension_controls_refuse_collection_limit() {
    let bytes = v5_dimension_style_chunk();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("dimension root admitted");
    let error = crate::presentation::parse_v5_dimension_style(
        &ctx,
        &bytes,
        0..bytes.len(),
        crate::settings::MillimeterScale::IDENTITY,
        321,
        None,
    )
    .expect_err("dimension controls exceed collection limit");
    assert!(
        matches!(error, FramingError::Resource(refusal) if refusal.operation == "Rhino dimension controls")
    );
}

#[test]
fn dimension_controls_preserve_sorted_serialization_and_replacement() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut entries = crate::presentation::DimensionControlEntries::default();
    entries
        .insert_with(&ctx, "z", || Ok(serde_json::json!(1)))
        .expect("first key admitted");
    entries
        .insert_with(&ctx, "a", || Ok(serde_json::json!(2)))
        .expect("second key admitted");
    entries
        .insert_with(&ctx, "z", || Ok(serde_json::json!(3)))
        .expect("duplicate key replaced");
    let controls = crate::presentation::DimensionStyleControls {
        controls: &entries,
        extra: None,
    };
    assert_eq!(
        serde_json::to_string(&controls).expect("control serialization"),
        "{\"a\":2,\"z\":3}"
    );
}
v5_dimension_text_limit!(
    v5_dimension_prefix_refuses_retained_limit,
    v5_dimension_prefix_budget(),
    "Rhino V5 dimension prefix"
);
v5_dimension_text_limit!(
    v5_dimension_suffix_refuses_retained_limit,
    v5_dimension_prefix_budget() + 1,
    "Rhino V5 dimension suffix"
);
v5_dimension_text_limit!(
    v5_dimension_alternate_prefix_refuses_retained_limit,
    v5_dimension_prefix_budget() + 2,
    "Rhino V5 dimension alternate prefix"
);
v5_dimension_text_limit!(
    v5_dimension_alternate_suffix_refuses_retained_limit,
    v5_dimension_prefix_budget() + 3,
    "Rhino V5 dimension alternate suffix"
);
v5_dimension_text_limit!(
    v5_dimension_id_refuses_retained_limit,
    v5_dimension_id_budget(),
    "Rhino dimension style ID"
);

#[test]
fn v5_dimension_source_record_uuid_refuses_retained_limit() {
    let id_len = "rhino:presentation:dimension_style#33333333-3333-3333-3333-333333333333".len();
    assert!(
        matches!(v5_dimension_style_refusal(v5_dimension_id_budget() + u64::try_from(id_len).expect("budget fits")), FramingError::Resource(refusal) if refusal.operation == "Rhino dimension style source UUID")
    );
}

fn modern_dimension_style_refusal(limit: u64) -> FramingError {
    let bytes = super::future_dimension_style_chunk();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("modern dimension root admitted");
    crate::presentation::parse_dimension_style(
        &ctx,
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V8,
        crate::settings::MillimeterScale::IDENTITY,
        321,
    )
    .expect_err("modern dimension text exceeds limit")
}

#[test]
fn modern_dimension_prefix_refuses_retained_limit() {
    assert!(
        matches!(modern_dimension_style_refusal(15), FramingError::Resource(refusal) if refusal.operation == "Rhino dimension prefix")
    );
}

#[test]
fn modern_dimension_suffix_refuses_retained_limit() {
    assert!(
        matches!(modern_dimension_style_refusal(16), FramingError::Resource(refusal) if refusal.operation == "Rhino dimension suffix")
    );
}

#[test]
fn modern_dimension_alternate_prefix_refuses_retained_limit() {
    assert!(
        matches!(modern_dimension_style_refusal(17), FramingError::Resource(refusal) if refusal.operation == "Rhino dimension alternate prefix")
    );
}

#[test]
fn modern_dimension_alternate_suffix_refuses_retained_limit() {
    assert!(
        matches!(modern_dimension_style_refusal(18), FramingError::Resource(refusal) if refusal.operation == "Rhino dimension alternate suffix")
    );
}

#[test]
fn dimension_override_bits_refuse_collection_limit() {
    let bytes = super::dimension_style_with_override_bits_chunk();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("dimension root admitted");
    let error = crate::presentation::parse_dimension_style(
        &ctx,
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V8,
        crate::settings::MillimeterScale::IDENTITY,
        321,
    )
    .expect_err("override bits exceed collection limit");
    assert!(
        matches!(error, FramingError::Resource(refusal) if refusal.operation == "Rhino dimension override bits")
    );
    let admitted = crate::presentation::parse_dimension_style(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V8,
        crate::settings::MillimeterScale::IDENTITY,
        321,
    )
    .expect("override bits fit service profile");
    assert_eq!(
        admitted.details.controls()["field_override_bits"],
        serde_json::json!([1])
    );
}

#[test]
fn dimension_child_digest_refuses_retained_limit() {
    let bytes = anonymous(0, &[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 63;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("child root admitted");
    let error = crate::presentation::named_child(
        &ctx,
        &bytes,
        &mut BoundedReader::new(&bytes, 0, bytes.len()).expect("child bounds"),
        ArchiveVersion::V8,
    )
    .expect_err("digest exceeds retained limit");
    assert!(
        matches!(error, FramingError::Resource(refusal) if refusal.operation == "Rhino dimension child SHA-256")
    );
    let admitted = crate::presentation::named_child(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &mut BoundedReader::new(&bytes, 0, bytes.len()).expect("child bounds"),
        ArchiveVersion::V8,
    )
    .expect("digest fits service profile");
    assert_eq!(admitted["sha256"], cadmpeg_ir::hash::sha256_hex(&bytes));
}

fn presentation_install_scan() -> &'static crate::container::Scan<'static> {
    static SCAN: OnceLock<crate::container::Scan<'static>> = OnceLock::new();
    SCAN.get_or_init(|| {
        use crate::test_support::test_dump::{
            class_wrapper, crc_chunk, minimal_document, object_record_with_attribute_userdata,
            table, tagged_attributes,
        };
        let archive = ArchiveVersion::V5;
        let mut groups = vec![0x1f];
        groups.extend(7_i32.to_le_bytes());
        groups.extend(utf16_bytes("fixtures"));
        groups.extend([0x44; 16]);
        let group = crc_chunk(
            archive,
            0x2000_8073,
            &class_wrapper(archive, crate::presentation::GROUP.to_wire(), &groups),
        );
        let mut group_item = 1_i32.to_le_bytes().to_vec();
        group_item.extend(7_i32.to_le_bytes());
        let attributes = tagged_attributes(&[(18, group_item)], 0);
        let object = object_record_with_attribute_userdata(
            archive,
            1,
            crate::test_support::test_dump::POINT_CLASS,
            &attributes,
            &[],
        );
        let bytes = minimal_document(
            "50",
            &[
                table(archive, 0x1000_0014, &[]),
                table(archive, 0x1000_0015, &[]),
                table(archive, 0x1000_0018, &[group]),
                table(archive, 0x1000_0013, &[object]),
            ],
        );
        let mut scan = crate::container::scan_owned(bytes).expect("presentation fixture scan");
        let group_table = scan
            .tables
            .iter()
            .find(|table| table.typecode == 0x1000_0018)
            .expect("group table retained");
        let group_record = group_table.records.first().expect("group record retained");
        let group_range = crate::presentation::class_data(
            scan.data,
            group_record,
            archive,
            crate::presentation::GROUP,
        )
        .expect("group class admitted");
        crate::presentation::parse_group(
            &cadmpeg_test_support::service_decode_context(),
            scan.data,
            group_range,
            group_record.range.start,
        )
        .expect("group payload admitted");
        crate::test_support::test_dump::set_test_units(&mut scan, 1.0);
        scan.metadata.layers.push(settings::LayerRecord {
            source: settings::SourceRange { range: 0..1 },
            index: 3,
            iges_level: None,
            render_material_index: -1,
            color: [1, 2, 3, 255],
            name: "Layer".to_owned(),
            description: Some("Description".to_owned()),
            visible: true,
            locked: false,
            id: Some(Uuid::from_wire([1; 16])),
            hierarchy: None,
            linetype_index: None,
            plot: None,
            display_material_id: Some(Uuid::from_wire([2; 16])),
            no_clipping_planes: None,
            visible_in_new_details: None,
            rendering_range: None,
            extension_items: Vec::new(),
            embedded_linetype: None,
            embedded_section_style: None,
            per_viewport_settings: vec![settings::LayerPerViewportSettings {
                viewport_id: Uuid::from_wire([3; 16]),
                color: None,
                plot_color: None,
                plot_weight_mm: None,
                visible: None,
                persistent_visibility: None,
            }],
        });
        scan
    })
}

fn presentation_install_limit_operations(
    dimension: cadmpeg_core::decode::ResourceDimension,
) -> Vec<&'static str> {
    let scan = presentation_install_scan();
    let mut limit = 0_u64;
    let mut operations = Vec::new();
    for _ in 0..256 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        match dimension {
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = limit;
            }
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = limit;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = limit;
            }
            other => panic!("unsupported install test limit: {other:?}"),
        }
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
                .expect("presentation root admitted");
        match crate::presentation::install(&ctx, scan, &mut cadmpeg_ir::document::CadIr::empty()) {
            Ok(_) => return operations,
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == dimension =>
            {
                operations.push(refusal.operation);
                let next = refusal.used + refusal.additional;
                assert!(next > limit, "limit ladder must advance at {limit}");
                limit = next;
            }
            Err(error) => panic!("unexpected presentation install failure: {error}"),
        }
    }
    panic!("presentation install limit ladder did not terminate");
}

fn presentation_install_collection_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        presentation_install_limit_operations(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
        )
    })
}

fn presentation_install_retained_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        presentation_install_limit_operations(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        )
    })
}

fn presentation_install_materialized_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        presentation_install_limit_operations(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        )
    })
}

macro_rules! presentation_install_limit_test {
    ($name:ident, $operations:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let operations = $operations();
            assert!(operations.contains(&$operation), "reached {operations:?}");
        }
    };
}

presentation_install_limit_test!(
    object_identity_counts_refuse_collection_limit,
    presentation_install_collection_operations,
    "Rhino object identity counts"
);
presentation_install_limit_test!(
    group_member_keys_refuse_collection_limit,
    presentation_install_collection_operations,
    "Rhino group member keys"
);
presentation_install_limit_test!(
    group_member_links_refuse_collection_limit,
    presentation_install_collection_operations,
    "Rhino group member links"
);
presentation_install_limit_test!(
    object_presentation_links_refuse_collection_limit,
    presentation_install_collection_operations,
    "Rhino object presentation links"
);
presentation_install_limit_test!(
    object_presentation_records_refuse_collection_limit,
    presentation_install_collection_operations,
    "Rhino object presentation records"
);
presentation_install_limit_test!(
    layer_identity_counts_refuse_collection_limit,
    presentation_install_collection_operations,
    "Rhino layer identity counts"
);
presentation_install_limit_test!(
    layer_viewport_settings_refuse_collection_limit,
    presentation_install_collection_operations,
    "Rhino layer presentation viewport settings"
);
presentation_install_limit_test!(
    layer_presentation_records_refuse_collection_limit,
    presentation_install_collection_operations,
    "Rhino layer presentation records"
);
presentation_install_limit_test!(
    group_index_counts_refuse_collection_limit,
    presentation_install_collection_operations,
    "Rhino group index counts"
);
presentation_install_limit_test!(
    group_member_link_refuses_retained_limit,
    presentation_install_retained_operations,
    "Rhino group member link"
);
presentation_install_limit_test!(
    object_presentation_link_refuses_retained_limit,
    presentation_install_retained_operations,
    "Rhino object presentation link"
);
presentation_install_limit_test!(
    object_presentation_id_refuses_retained_limit,
    presentation_install_retained_operations,
    "Rhino object presentation ID"
);
presentation_install_limit_test!(
    layer_presentation_id_refuses_retained_limit,
    presentation_install_retained_operations,
    "Rhino layer presentation ID"
);
presentation_install_limit_test!(
    layer_presentation_source_uuid_refuses_retained_limit,
    presentation_install_retained_operations,
    "Rhino layer presentation source UUID"
);
presentation_install_limit_test!(
    layer_presentation_name_refuses_retained_limit,
    presentation_install_retained_operations,
    "Rhino layer presentation name"
);
presentation_install_limit_test!(
    layer_presentation_description_refuses_retained_limit,
    presentation_install_retained_operations,
    "Rhino layer presentation description"
);
presentation_install_limit_test!(
    layer_presentation_display_material_uuid_refuses_retained_limit,
    presentation_install_retained_operations,
    "Rhino layer presentation display material UUID"
);
presentation_install_limit_test!(
    object_identity_workspace_refuses_materialized_limit,
    presentation_install_materialized_operations,
    "Rhino object identity workspace"
);
presentation_install_limit_test!(
    group_member_workspace_refuses_materialized_limit,
    presentation_install_materialized_operations,
    "Rhino group member workspace"
);
presentation_install_limit_test!(
    layer_identity_workspace_refuses_materialized_limit,
    presentation_install_materialized_operations,
    "Rhino layer identity workspace"
);
presentation_install_limit_test!(
    group_index_workspace_refuses_materialized_limit,
    presentation_install_materialized_operations,
    "Rhino group index workspace"
);

fn font_refusal(limit: u64) -> FramingError {
    let bytes = modern_font_chunk(7, &[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("font root admitted");
    crate::presentation::parse_font(
        &ctx,
        &bytes,
        &mut BoundedReader::new(&bytes, 0, bytes.len()).expect("font bounds"),
        ArchiveVersion::V8,
        None,
    )
    .expect_err("font text exceeds retained limit")
}

macro_rules! font_text_limit {
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(
                font_refusal($limit),
                FramingError::Resource(refusal) if refusal.operation == $operation
            ));
        }
    };
}

font_text_limit!(
    font_wide_string_refuses_retained_limit,
    0,
    "Rhino wide string"
);
font_text_limit!(
    font_postscript_refuses_retained_limit,
    5,
    "Rhino font PostScript name"
);
font_text_limit!(
    font_obsolete_description_refuses_retained_limit,
    12,
    "Rhino font obsolete description"
);
font_text_limit!(
    font_family_refuses_retained_limit,
    25,
    "Rhino font family name"
);
font_text_limit!(
    font_locale_refuses_retained_limit,
    30,
    "Rhino font locale name"
);
font_text_limit!(
    font_localized_postscript_refuses_retained_limit,
    35,
    "Rhino font localized PostScript name"
);
font_text_limit!(
    font_english_postscript_refuses_retained_limit,
    42,
    "Rhino font English PostScript name"
);
font_text_limit!(
    font_localized_logfont_refuses_retained_limit,
    49,
    "Rhino font localized LOGFONT name"
);
font_text_limit!(
    font_english_logfont_refuses_retained_limit,
    54,
    "Rhino font English LOGFONT name"
);
font_text_limit!(
    font_localized_family_refuses_retained_limit,
    59,
    "Rhino font localized family name"
);
font_text_limit!(
    font_english_family_refuses_retained_limit,
    64,
    "Rhino font English family name"
);
font_text_limit!(
    font_localized_face_refuses_retained_limit,
    69,
    "Rhino font localized face name"
);
font_text_limit!(
    font_english_face_refuses_retained_limit,
    76,
    "Rhino font English face name"
);

fn legacy_text_style_refusal(limit: u64) -> FramingError {
    let bytes = legacy_text_style_bytes();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("legacy style root admitted");
    crate::presentation::parse_text_style(
        &ctx,
        &bytes,
        TextStyleParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V8,
            writer_version: Some(201_802_231),
            apple_runtime: false,
            source_offset: 42,
        },
        &mut Vec::new(),
    )
    .expect_err("legacy style text exceeds retained limit")
}

#[test]
fn legacy_text_style_description_refuses_retained_limit() {
    assert!(matches!(
        legacy_text_style_refusal(0),
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino legacy text style description"
    ));
}

#[test]
fn legacy_font_face_refuses_retained_limit() {
    assert!(matches!(
        legacy_text_style_refusal(14),
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino legacy font face"
    ));
}

#[test]
fn legacy_postscript_name_refuses_retained_limit() {
    assert!(matches!(
        legacy_text_style_refusal(28),
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino legacy PostScript name"
    ));
}

#[test]
fn legacy_font_description_refuses_retained_limit() {
    assert!(matches!(
        legacy_text_style_refusal(42),
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino legacy font description"
    ));
}

#[test]
fn legacy_text_style_id_refuses_retained_limit() {
    assert!(matches!(
        legacy_text_style_refusal(56),
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino text style ID"
    ));
}

#[test]
fn legacy_text_style_uuid_refuses_retained_limit() {
    let id_len = "rhino:presentation:text_style#index-7-offset-42".len();
    assert!(matches!(
        legacy_text_style_refusal(u64::try_from(56 + id_len).expect("budget fits")),
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino text style source UUID"
    ));
}

#[test]
fn legacy_text_style_name_refuses_retained_limit() {
    let id_len = "rhino:presentation:text_style#index-7-offset-42".len();
    assert!(matches!(
        legacy_text_style_refusal(u64::try_from(56 + id_len + 36).expect("budget fits")),
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino text style name"
    ));
}

#[test]
fn unstamped_font_loss_refuses_collection_limit() {
    let bytes = legacy_text_style_bytes();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("legacy style root admitted");
    let error = crate::presentation::parse_text_style(
        &ctx,
        &bytes,
        TextStyleParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V8,
            writer_version: None,
            apple_runtime: false,
            source_offset: 42,
        },
        &mut Vec::new(),
    )
    .expect_err("unstamped loss exceeds collection limit");
    assert!(matches!(
        error,
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino text style writer-stamp losses"
    ));
}

#[test]
fn unstamped_font_loss_text_refuses_retained_limit() {
    let bytes = legacy_text_style_bytes();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 28;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("legacy style root admitted");
    let error = crate::presentation::parse_text_style(
        &ctx,
        &bytes,
        TextStyleParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V8,
            writer_version: None,
            apple_runtime: false,
            source_offset: 42,
        },
        &mut Vec::new(),
    )
    .expect_err("unstamped loss text exceeds retained limit");
    assert!(matches!(
        error,
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino text style writer-stamp loss text"
    ));
}

#[test]
fn texture_file_reference_refuses_retained_limit() {
    let bytes = texture_payload(2, &[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("root bytes admitted");
    let error = crate::presentation::parse_texture(
        &ctx,
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V8,
        42,
        &mut Vec::new(),
    )
    .expect_err("file-reference path exceeds retained limit");
    assert!(matches!(
        error,
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino file reference full path"
    ));
}

fn texture_minor_zero_refusal(limit: u64) -> FramingError {
    let bytes = texture_payload(0, &[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("texture root admitted");
    crate::presentation::parse_texture(
        &ctx,
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V8,
        42,
        &mut Vec::new(),
    )
    .expect_err("texture value exceeds retained limit")
}

#[test]
fn texture_legacy_path_refuses_retained_limit() {
    assert!(
        matches!(texture_minor_zero_refusal(0), FramingError::Resource(refusal) if refusal.operation == "Rhino texture legacy path")
    );
}

#[test]
fn texture_source_uuid_refuses_retained_limit() {
    assert!(
        matches!(texture_minor_zero_refusal(11), FramingError::Resource(refusal) if refusal.operation == "Rhino texture source UUID")
    );
}

#[test]
fn texture_transparency_uuid_refuses_retained_limit() {
    assert!(
        matches!(texture_minor_zero_refusal(47), FramingError::Resource(refusal) if refusal.operation == "Rhino texture transparency UUID")
    );
}

#[test]
fn texture_file_reference_loss_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let mut diagnostics = Diagnostics::new();
    diagnostics.push("invalid reference");
    let error = crate::presentation::append_file_reference_diagnostics(
        &ctx,
        &mut Vec::new(),
        diagnostics,
        42,
    )
    .expect_err("loss exceeds collection limit");
    assert!(
        matches!(error, FramingError::Resource(refusal) if refusal.operation == "Rhino texture file-reference losses")
    );
}

#[test]
fn texture_file_reference_loss_text_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let mut diagnostics = Diagnostics::new();
    diagnostics.push("invalid reference");
    let error = crate::presentation::append_file_reference_diagnostics(
        &ctx,
        &mut Vec::new(),
        diagnostics,
        42,
    )
    .expect_err("loss text exceeds retained limit");
    assert!(
        matches!(error, FramingError::Resource(refusal) if refusal.operation == "Rhino texture file-reference loss text")
    );
}

#[test]
fn projected_user_string_entries_refuse_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = crate::presentation::user_string_records(
        &ctx,
        vec![("key".to_string(), "value".to_string())],
    )
    .expect_err("projected entry exceeds collection limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "Rhino projected user-string entries"
    ));
}

fn presentation_attributes() -> crate::objects::ObjectAttributes {
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
    .expect("empty tagged attributes")
}

fn projected_attributes_refusal(
    attributes: &crate::objects::ObjectAttributes,
    collection_limit: u64,
    retained_limit: u64,
) -> cadmpeg_core::CodecError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    crate::presentation::object_attributes_presentation(
        &ctx,
        &[],
        attributes,
        &[],
        &[],
        ArchiveVersion::V8,
        0,
        attributes.object_id,
        &mut Vec::new(),
    )
    .expect_err("projected attributes exceed configured limit")
}

fn assert_projected_attributes_resource(error: &cadmpeg_core::CodecError, operation: &str) {
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == operation),
        "expected resource operation {operation}, got {error:?}"
    );
}

#[test]
fn projected_object_name_and_url_refuse_retained_limit() {
    let mut attributes = presentation_attributes();
    attributes.name = "name".to_string();
    assert_projected_attributes_resource(
        &projected_attributes_refusal(&attributes, 100, 3),
        "Rhino projected object name",
    );
    attributes.name.clear();
    attributes.url = "url".to_string();
    assert_projected_attributes_resource(
        &projected_attributes_refusal(&attributes, 100, 2),
        "Rhino projected object URL",
    );
}

#[test]
fn projected_object_groups_refuse_collection_limit() {
    let mut attributes = presentation_attributes();
    attributes.groups.push(7);
    assert_projected_attributes_resource(
        &projected_attributes_refusal(&attributes, 0, 100),
        "Rhino projected object groups",
    );
}

#[test]
fn projected_display_materials_refuse_collection_limit() {
    let mut attributes = presentation_attributes();
    attributes
        .display_materials
        .push((Uuid::nil(), Uuid::nil()));
    assert_projected_attributes_resource(
        &projected_attributes_refusal(&attributes, 0, 100),
        "Rhino projected display materials",
    );
}

#[test]
fn projected_display_material_uuid_text_refuses_retained_limit() {
    let mut attributes = presentation_attributes();
    attributes
        .display_materials
        .push((Uuid::nil(), Uuid::nil()));
    assert_projected_attributes_resource(
        &projected_attributes_refusal(&attributes, 1, 35),
        "Rhino projected display viewport UUID",
    );
    assert_projected_attributes_resource(
        &projected_attributes_refusal(&attributes, 1, 36),
        "Rhino projected display material UUID",
    );
}

#[test]
fn projected_active_viewport_uuid_refuses_retained_limit() {
    let mut attributes = presentation_attributes();
    attributes.viewport_id = Uuid::from_canonical([1; 16]);
    assert_projected_attributes_resource(
        &projected_attributes_refusal(&attributes, 100, 35),
        "Rhino projected active viewport UUID",
    );
}

#[test]
fn projected_clipping_plane_uuids_refuse_collection_limit() {
    let mut attributes = presentation_attributes();
    attributes.clipping_plane_ids.push(Uuid::nil());
    assert_projected_attributes_resource(
        &projected_attributes_refusal(&attributes, 0, 100),
        "Rhino projected clipping plane UUIDs",
    );
}

#[test]
fn projected_clipping_plane_uuid_text_refuses_retained_limit() {
    let mut attributes = presentation_attributes();
    attributes.clipping_plane_ids.push(Uuid::nil());
    assert_projected_attributes_resource(
        &projected_attributes_refusal(&attributes, 1, 35),
        "Rhino projected clipping plane UUID text",
    );
}

#[test]
fn projected_source_uuid_refuses_retained_limit() {
    let attributes = presentation_attributes();
    assert_projected_attributes_resource(
        &projected_attributes_refusal(&attributes, 100, 35),
        "Rhino projected source UUID",
    );
}

fn displacement_with_sub_item() -> crate::mesh_modifiers::DisplacementModifier {
    use crate::mesh_modifiers::{DisplacementModifier, DisplacementSubItem};
    DisplacementModifier {
        xml_version: 2,
        on: true,
        texture: None,
        channel: 0,
        black_point: crate::test_support::finite(0.0),
        white_point: crate::test_support::finite(1.0),
        sweep_pitch: 1000,
        refine_steps: 1,
        refine_sensitivity: crate::test_support::finite(0.5),
        face_count_limit_enabled: false,
        face_count_limit: 10_000,
        post_weld_angle: crate::test_support::finite(40.0),
        mesh_memory_limit: 512,
        fairing_enabled: false,
        fairing_amount: 4,
        sub_object_count: Some(1),
        sweep_resolution_formula: 0,
        sub_items: vec![DisplacementSubItem {
            face_index: 7,
            on: true,
            texture: None,
            channel: 0,
            black_point: crate::test_support::finite(0.0),
            white_point: crate::test_support::finite(1.0),
        }],
    }
}

fn displacement_projection_refusal(
    value: &crate::mesh_modifiers::DisplacementModifier,
    collection_limit: u64,
    retained_limit: u64,
) -> cadmpeg_core::CodecError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    crate::presentation::displacement_record(&ctx, value)
        .expect_err("displacement projection exceeds configured limit")
}

#[test]
fn projected_displacement_sub_items_refuse_collection_limit() {
    let value = displacement_with_sub_item();
    assert_projected_attributes_resource(
        &displacement_projection_refusal(&value, 0, 100),
        "Rhino projected displacement sub-items",
    );
}

#[test]
fn projected_displacement_sub_item_texture_refuses_retained_limit() {
    let mut value = displacement_with_sub_item();
    value.sub_items[0].texture = Some(Uuid::from_canonical([1; 16]));
    assert_projected_attributes_resource(
        &displacement_projection_refusal(&value, 1, 35),
        "Rhino projected displacement sub-item texture UUID",
    );
}

#[test]
fn projected_displacement_texture_refuses_retained_limit() {
    let mut value = displacement_with_sub_item();
    value.texture = Some(Uuid::from_canonical([1; 16]));
    assert_projected_attributes_resource(
        &displacement_projection_refusal(&value, 1, 35),
        "Rhino projected displacement texture UUID",
    );
}

fn shut_lining_projection_refusal(
    value: &crate::mesh_modifiers::ShutLiningModifier,
    collection_limit: u64,
    retained_limit: u64,
) -> cadmpeg_core::CodecError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    crate::presentation::shut_lining_record(&ctx, value)
        .expect_err("shut-lining projection exceeds configured limit")
}

#[test]
fn projected_shut_lining_curves_refuse_collection_limit() {
    let value = crate::mesh_modifiers::ShutLiningModifier {
        xml_version: 2,
        on: true,
        faceted: false,
        auto_update: false,
        force_update: false,
        curves: vec![crate::mesh_modifiers::ShutLiningCurve {
            uuid: Some(Uuid::from_canonical([1; 16])),
            radius: crate::test_support::finite(1.0),
            profile: 0,
            enabled: true,
            pull: false,
            is_bump: false,
        }],
    };
    assert_projected_attributes_resource(
        &shut_lining_projection_refusal(&value, 0, 100),
        "Rhino projected shut-lining curves",
    );
    assert_projected_attributes_resource(
        &shut_lining_projection_refusal(&value, 1, 35),
        "Rhino projected shut-lining curve UUID",
    );
}

fn projected_rendering_collection_refusal(limit: u64) -> FramingError {
    let bytes = object_rendering_with_negative_minor(3, 0, Some(0), Some(1));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("root bytes admitted");
    rendering_attributes(
        &ctx,
        &bytes,
        Some(0..bytes.len()),
        ArchiveVersion::V8,
        settings::RenderingAttributesKind::Object,
    )
    .expect_err("rendering projection exceeds collection limit")
}

#[test]
fn projected_rendering_materials_refuse_collection_limit() {
    assert!(matches!(
        projected_rendering_collection_refusal(0),
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino projected rendering materials"
    ));
}

#[test]
fn projected_rendering_mappings_refuse_collection_limit() {
    assert!(matches!(
        projected_rendering_collection_refusal(1),
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino projected rendering mappings"
    ));
}

#[test]
fn projected_rendering_channels_refuse_collection_limit() {
    assert!(matches!(
        projected_rendering_collection_refusal(2),
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino projected rendering channels"
    ));
}
