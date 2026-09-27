// SPDX-License-Identifier: Apache-2.0

use super::{
    legacy_text_style_bytes, modern_font_chunk, object_rendering_with_negative_minor,
    texture_payload,
};
use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::presentation::rendering_attributes;
use crate::presentation::TextStyleParseInput;
use crate::settings;
use crate::wire::Uuid;

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
