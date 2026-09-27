// SPDX-License-Identifier: Apache-2.0

use super::{
    anonymous, bitmap_header, embedded_bitmap_payload, legacy_text_style_bytes, light_payload,
    model_attributes_status_chunk, modern_font_chunk, object_rendering_with_negative_minor,
    stored_bitmap_buffer, texture_payload, utf16_bytes, windows_bitmap_payload,
};
use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::loss::Diagnostics;
use crate::presentation::rendering_attributes;
use crate::presentation::TextStyleParseInput;
use crate::settings;
use crate::wire::Uuid;
use std::collections::HashMap;

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
