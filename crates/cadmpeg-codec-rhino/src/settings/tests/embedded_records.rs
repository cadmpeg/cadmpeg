// SPDX-License-Identifier: Apache-2.0

use super::{metadata_table, parse_test_extensions, parse_test_metadata};
use crate::chunks::{ArchiveVersion, BoundedReader};
use crate::loss::Diagnostics;
use crate::objects::ClassUserdata;
use crate::settings;
use crate::test_support::test_dump::{
    anonymous_chunk, crc_chunk, crc_chunk_excluding, utf16_bytes, uuid_bytes,
};
use crate::wire::Uuid;

fn with_embedded_collection_limit<T>(
    bytes: &[u8],
    limit: u64,
    test: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("embedded fixture root admitted");
    test(&ctx)
}

fn embedded_linetype_with_model_attributes(archive: ArchiveVersion) -> Vec<u8> {
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    let mut body = Vec::new();
    body.extend(2_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    let child_start = body.len();
    body.extend(model_attributes);
    let child_end = body.len();
    body.extend(0_i32.to_le_bytes());
    body.push(0);
    crc_chunk_excluding(
        archive,
        0x4000_8000,
        &body,
        std::slice::from_ref(&(child_start..child_end)),
    )
}

fn embedded_section_style_with_model_attributes(
    archive: ArchiveVersion,
    nested_linetype: bool,
) -> Vec<u8> {
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    let mut body = Vec::new();
    body.extend(1_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    let child_start = body.len();
    body.extend(model_attributes);
    let child_end = body.len();
    let mut children = Vec::new();
    children.push(child_start..child_end);
    if nested_linetype {
        body.push(11);
        let nested_start = body.len();
        body.extend(embedded_linetype_with_model_attributes(archive));
        children.push(nested_start..body.len());
        body.push(0);
    } else {
        body.push(0);
    }
    crc_chunk_excluding(archive, 0x4000_8000, &body, &children)
}

#[test]
fn embedded_linetype_checksum_children_refuse_collection_limit() {
    let archive = ArchiveVersion::V8;
    let bytes = embedded_linetype_with_model_attributes(archive);
    let error = with_embedded_collection_limit(&bytes, 0, |ctx| {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("linetype bounds");
        settings::parse_direct_linetype(ctx, &bytes, &mut reader, archive, &mut Diagnostics::new())
            .expect_err("linetype child exceeds zero collection items")
    });
    assert!(
        matches!(error, crate::chunks::FramingError::Resource(refusal)
        if refusal.operation == "Rhino embedded linetype checksum children")
    );
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("linetype bounds");
    settings::parse_direct_linetype(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &mut reader,
        archive,
        &mut Diagnostics::new(),
    )
    .expect("service profile admits linetype child");
}

#[test]
fn embedded_section_style_first_checksum_child_refuses_collection_limit() {
    let archive = ArchiveVersion::V8;
    let bytes = embedded_section_style_with_model_attributes(archive, false);
    let error = with_embedded_collection_limit(&bytes, 0, |ctx| {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("section-style bounds");
        settings::parse_direct_section_style(
            ctx,
            &bytes,
            &mut reader,
            archive,
            &mut Diagnostics::new(),
        )
        .expect_err("first section-style child exceeds zero collection items")
    });
    assert!(
        matches!(error, crate::chunks::FramingError::Resource(refusal)
        if refusal.operation == "Rhino embedded section-style checksum children")
    );
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("section-style bounds");
    settings::parse_direct_section_style(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &mut reader,
        archive,
        &mut Diagnostics::new(),
    )
    .expect("service profile admits section-style child");
}

#[test]
fn embedded_section_style_nested_checksum_child_refuses_collection_limit() {
    let archive = ArchiveVersion::V8;
    let bytes = embedded_section_style_with_model_attributes(archive, true);
    let error = with_embedded_collection_limit(&bytes, 1, |ctx| {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("section-style bounds");
        settings::parse_direct_section_style(
            ctx,
            &bytes,
            &mut reader,
            archive,
            &mut Diagnostics::new(),
        )
        .expect_err("nested section-style child exceeds one collection item")
    });
    assert!(
        matches!(error, crate::chunks::FramingError::Resource(refusal)
        if refusal.operation == "Rhino embedded section-style checksum children")
    );
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("section-style bounds");
    settings::parse_direct_section_style(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &mut reader,
        archive,
        &mut Diagnostics::new(),
    )
    .expect("service profile admits nested section-style child");
}

#[test]
fn layer_extensions_read_effective_fields_sort_entries_and_apply_root_rule() {
    let archive = ArchiveVersion::V8;
    let first_viewport = Uuid::from_canonical([
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f,
    ]);
    let second_viewport = Uuid::from_canonical([
        0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e,
        0x2f,
    ]);
    let mut full_entry = 63_u32.to_le_bytes().to_vec();
    full_entry.extend(second_viewport.to_wire());
    full_entry.extend([10, 20, 30, 40]);
    full_entry.extend([50, 60, 70, 80]);
    full_entry.extend(1.25_f64.to_le_bytes());
    full_entry.extend([2, 2, 2]);
    full_entry.extend([0xde, 0xad]);
    let mut color_entry = 3_u32.to_le_bytes().to_vec();
    color_entry.extend(first_viewport.to_wire());
    color_entry.extend([90, 100, 110, 120]);
    let entries = [
        anonymous_chunk(archive, 2, &full_entry),
        anonymous_chunk(archive, 2, &color_entry),
    ]
    .concat();
    let mut outer_body = 2_i32.to_le_bytes().to_vec();
    outer_body.extend(entries);
    outer_body.extend([0xbe, 0xef]);
    let payload = anonymous_chunk(archive, 0, &outer_body);
    let descriptor = ClassUserdata {
        range: 0..payload.len(),
        version: (2, 2),
        class_uuid: settings::LAYER_EXTENSIONS,
        item_uuid: settings::LAYER_EXTENSIONS,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        save_context: None,
        payload_range: 0..payload.len(),
    };
    let values = parse_test_extensions(
        &payload,
        &descriptor,
        archive,
        Some(Uuid::from_canonical([1; 16])),
    )
    .expect("layer extensions payload");
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].viewport_id, first_viewport);
    assert_eq!(values[0].settings_mask(), 3);
    assert_eq!(values[0].color, Some([90, 100, 110, 120]));
    assert_eq!(values[1].viewport_id, second_viewport);
    assert_eq!(values[1].settings_mask(), 63);
    assert_eq!(
        values[1].plot_weight_mm.map(settings::LayerPlotWeight::get),
        Some(1.25)
    );
    assert_eq!(
        values[1].visible.map(settings::LayerVisibility::as_u8),
        Some(2)
    );
    assert_eq!(
        values[1]
            .persistent_visibility
            .map(settings::LayerVisibility::as_u8),
        Some(2)
    );

    let root_values = parse_test_extensions(&payload, &descriptor, archive, None)
        .expect("root layer extensions payload");
    assert_eq!(root_values[1].settings_mask(), 31);
    assert_eq!(root_values[1].persistent_visibility, None);
}

fn single_viewport_extension(bits: u32, field_bytes: &[u8]) -> (Vec<u8>, ClassUserdata) {
    let archive = ArchiveVersion::V8;
    let mut entry = bits.to_le_bytes().to_vec();
    entry.extend(Uuid::from_canonical([1; 16]).to_wire());
    entry.extend(field_bytes);
    let mut outer_body = 1_i32.to_le_bytes().to_vec();
    outer_body.extend(anonymous_chunk(archive, 2, &entry));
    let payload = anonymous_chunk(archive, 0, &outer_body);
    let descriptor = ClassUserdata {
        range: 0..payload.len(),
        version: (2, 2),
        class_uuid: settings::LAYER_EXTENSIONS,
        item_uuid: settings::LAYER_EXTENSIONS,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        save_context: None,
        payload_range: 0..payload.len(),
    };
    (payload, descriptor)
}

#[test]
fn layer_extension_entries_refuse_cumulative_collection_limit() {
    let (payload, descriptor) = single_viewport_extension(
        super::super::LAYER_PER_VIEWPORT_ID | super::super::LAYER_PER_VIEWPORT_COLOR,
        &[1, 2, 3, 4],
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy)
        .expect("test input fits the service profile");
    assert_eq!(
        settings::parse_layer_extensions(&ctx, &payload, &descriptor, ArchiveVersion::V8, None)
            .expect("first layer entry fits the limit")
            .len(),
        1
    );
    let refusal =
        settings::parse_layer_extensions(&ctx, &payload, &descriptor, ArchiveVersion::V8, None)
            .expect_err("second layer entry exceeds the cumulative limit");
    assert!(
        matches!(refusal, crate::chunks::FramingError::Resource(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );

    let service = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &service)
        .expect("test input fits the service profile");
    for _ in 0..2 {
        assert_eq!(
            settings::parse_layer_extensions(&ctx, &payload, &descriptor, ArchiveVersion::V8, None)
                .expect("service profile admits both layer entries")
                .len(),
            1
        );
    }
}

fn assert_malformed_viewport_visibility(bits: u32, field_bytes: &[u8]) {
    let (payload, descriptor) = single_viewport_extension(bits, field_bytes);
    let error = parse_test_extensions(
        &payload,
        &descriptor,
        ArchiveVersion::V8,
        Some(Uuid::from_canonical([2; 16])),
    )
    .expect_err("malformed visibility-only entry");
    assert!(error.to_string().contains("visibility"), "{error}");
}

#[test]
fn malformed_present_viewport_visibility_is_reported() {
    assert_malformed_viewport_visibility(
        super::super::LAYER_PER_VIEWPORT_ID | super::super::LAYER_PER_VIEWPORT_VISIBLE,
        &[3, 1],
    );
}

#[test]
fn malformed_present_viewport_persistent_visibility_is_reported() {
    assert_malformed_viewport_visibility(
        super::super::LAYER_PER_VIEWPORT_ID
            | super::super::LAYER_PER_VIEWPORT_PERSISTENT_VISIBILITY,
        &[3],
    );
}

fn assert_malformed_viewport_plot_weight(weight: f64) {
    let (payload, descriptor) = single_viewport_extension(
        super::super::LAYER_PER_VIEWPORT_ID | super::super::LAYER_PER_VIEWPORT_PLOT_WEIGHT,
        &weight.to_le_bytes(),
    );
    let error = parse_test_extensions(&payload, &descriptor, ArchiveVersion::V8, None)
        .expect_err("malformed weight-only entry");
    assert!(error.to_string().contains("plot weight"), "{error}");
}

#[test]
fn negative_viewport_plot_weight_is_reported() {
    assert_malformed_viewport_plot_weight(-2.0);
}

#[test]
fn nonfinite_viewport_plot_weight_is_reported() {
    assert_malformed_viewport_plot_weight(f64::NAN);
}

#[test]
fn viewport_plot_weight_keeps_the_exact_unset_sentinel() {
    let (payload, descriptor) = single_viewport_extension(
        super::super::LAYER_PER_VIEWPORT_ID | super::super::LAYER_PER_VIEWPORT_PLOT_WEIGHT,
        &(-1.0_f64).to_le_bytes(),
    );
    let values = parse_test_extensions(&payload, &descriptor, ArchiveVersion::V8, None)
        .expect("unset plot weight");
    assert_eq!(
        values[0].plot_weight_mm,
        Some(settings::LayerPlotWeight::Unset)
    );
    assert_eq!(
        serde_json::to_value(values[0].plot_weight_mm).expect("serialized plot weight"),
        serde_json::json!(-1.0)
    );
}

#[test]
fn layer_extensions_reject_negative_count() {
    let archive = ArchiveVersion::V8;
    let payload = anonymous_chunk(archive, 0, &(-1_i32).to_le_bytes());
    let descriptor = ClassUserdata {
        range: 0..payload.len(),
        version: (2, 2),
        class_uuid: settings::LAYER_EXTENSIONS,
        item_uuid: settings::LAYER_EXTENSIONS,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        save_context: None,
        payload_range: 0..payload.len(),
    };
    assert!(parse_test_extensions(&payload, &descriptor, archive, None).is_err());
}

#[test]
fn rendering_attributes_parse_object_mapping_and_future_suffix() {
    let mut channel_body = 7_i32.to_le_bytes().to_vec();
    channel_body.extend(uuid_bytes());
    for value in 0..16 {
        channel_body.extend((value as f64).to_le_bytes());
    }
    let channel = anonymous_chunk(ArchiveVersion::V8, 1, &channel_body);
    let mut mapping_body = uuid_bytes();
    mapping_body.extend(1_i32.to_le_bytes());
    mapping_body.extend(channel);
    let mut mapping_payload = 1_i32.to_le_bytes().to_vec();
    mapping_payload.extend(0_i32.to_le_bytes());
    let channel_start = mapping_payload.len() + 16 + 4;
    mapping_payload.extend(mapping_body);
    #[allow(clippy::single_range_in_vec_init)] // One nested mapping-channel range.
    let mapping = crc_chunk_excluding(
        ArchiveVersion::V8,
        0x4000_8000,
        &mapping_payload,
        &[channel_start..mapping_payload.len()],
    );

    let mut rendering_body = vec![1, 0, 0, 0, 4, 0, 0, 0];
    rendering_body.extend(0_i32.to_le_bytes());
    rendering_body.extend(1_i32.to_le_bytes());
    let mapping_start = rendering_body.len();
    rendering_body.extend(mapping);
    let mapping_end = rendering_body.len();
    rendering_body.extend([1, 0, 1]);
    rendering_body.extend([0xaa, 0xbb]);
    #[allow(clippy::single_range_in_vec_init)] // The range is one direct child.
    let bytes = crc_chunk_excluding(
        ArchiveVersion::V8,
        0x4000_8000,
        &rendering_body,
        &[mapping_start..mapping_end],
    );
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded chunk reader");
    let mut warnings = Diagnostics::new();
    let range = settings::parse_rendering_attributes(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &mut reader,
        ArchiveVersion::V8,
        settings::RenderingAttributesKind::Object,
        &mut warnings,
    )
    .expect("object reader consumes mapping channels and leaves the suffix bounded");
    assert_eq!(range, 0..bytes.len());
    assert!(warnings.is_empty());
}

#[test]
fn rendering_attributes_accept_nonempty_obsolete_material_mapping_channels() {
    let archive = ArchiveVersion::V8;
    let mut channel_body = 7_i32.to_le_bytes().to_vec();
    channel_body.extend([0x33; 16]);
    channel_body.extend(
        (0..16)
            .map(|index| if index % 5 == 0 { 1.0 } else { 0.0 })
            .flat_map(f64::to_le_bytes),
    );
    let channel = anonymous_chunk(archive, 1, &channel_body);

    let mut material_body = vec![1, 0, 0, 0, 1, 0, 0, 0];
    material_body.extend([0x11; 16]);
    material_body.extend([0x22; 16]);
    material_body.extend(1_i32.to_le_bytes());
    let channel_start = material_body.len();
    material_body.extend(channel);
    let channel_end = material_body.len();
    material_body.extend([0x44; 16]);
    material_body.extend([3, 0, 0, 0]);
    let material = crc_chunk_excluding(
        archive,
        0x4000_8000,
        &material_body,
        std::slice::from_ref(&(channel_start..channel_end)),
    );

    let mut rendering_body = vec![1, 0, 0, 0, 4, 0, 0, 0, 1, 0, 0, 0];
    let material_start = rendering_body.len();
    rendering_body.extend(material);
    let material_end = rendering_body.len();
    rendering_body.extend(0_i32.to_le_bytes());
    rendering_body.extend([1, 1, 0]);
    let bytes = crc_chunk_excluding(
        archive,
        0x4000_8000,
        &rendering_body,
        std::slice::from_ref(&(material_start..material_end)),
    );

    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded rendering chunk");
    let mut warnings = Diagnostics::new();
    let range = settings::parse_rendering_attributes(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &mut reader,
        archive,
        settings::RenderingAttributesKind::Object,
        &mut warnings,
    )
    .expect("valid obsolete material mapping channels are consumed");
    assert_eq!(range, 0..bytes.len());
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn future_linetype_extension_stops_at_unknown_code() {
    let archive = ArchiveVersion::V8;
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    let mut body = Vec::new();
    body.extend(2_i32.to_le_bytes());
    body.extend(4_i32.to_le_bytes());
    body.extend(&model_attributes);
    body.extend(0_i32.to_le_bytes());
    body.extend([7, 0xaa, 0xbb, 0, 0xde]);
    #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
    let chunk = crc_chunk_excluding(
        archive,
        0x4000_8000,
        &body,
        std::slice::from_ref(&(8..8 + model_attributes.len())),
    );
    let mut reader = BoundedReader::new(&chunk, 0, chunk.len()).expect("bounded linetype");
    let mut warnings = Diagnostics::new();
    let descriptor = settings::parse_direct_linetype(
        &cadmpeg_test_support::service_decode_context(),
        &chunk,
        &mut reader,
        archive,
        &mut warnings,
    )
    .expect("future linetype code is bounded by the anonymous chunk");
    assert_eq!(descriptor.version, (2, 4));
    assert_eq!(reader.remaining(), 0);
    assert!(warnings.is_empty());
}

#[test]
fn embedded_linetype_accepts_unset_and_future_segment_tags() {
    let archive = ArchiveVersion::V8;
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    let mut body = Vec::new();
    body.extend(2_i32.to_le_bytes());
    body.extend(4_i32.to_le_bytes());
    body.extend(&model_attributes);
    body.extend(2_i32.to_le_bytes());
    body.extend(1.5_f64.to_le_bytes());
    body.extend(0xffff_ffff_u32.to_le_bytes());
    body.extend(2.5_f64.to_le_bytes());
    body.extend(7_u32.to_le_bytes());
    body.push(0);
    let chunk = crc_chunk_excluding(
        archive,
        0x4000_8000,
        &body,
        std::slice::from_ref(&(8..8 + model_attributes.len())),
    );
    let mut reader = BoundedReader::new(&chunk, 0, chunk.len()).expect("bounded linetype");
    let mut warnings = Diagnostics::new();
    let descriptor = settings::parse_direct_linetype(
        &cadmpeg_test_support::service_decode_context(),
        &chunk,
        &mut reader,
        archive,
        &mut warnings,
    )
    .expect("documented unset and future segment tags are admitted");
    assert_eq!(descriptor.version, (2, 4));
    assert_eq!(reader.remaining(), 0);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn legacy_embedded_linetype_accepts_unset_and_future_segment_tags() {
    let archive = ArchiveVersion::V5;
    let mut body = Vec::new();
    body.extend(1_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend(7_i32.to_le_bytes());
    body.extend(utf16_bytes("legacy-linetype"));
    body.extend(2_i32.to_le_bytes());
    body.extend(1.5_f64.to_le_bytes());
    body.extend(0xffff_ffff_u32.to_le_bytes());
    body.extend(2.5_f64.to_le_bytes());
    body.extend(7_u32.to_le_bytes());
    body.extend([0x55; 16]);
    let chunk = crc_chunk(archive, 0x4000_8000, &body);
    let mut reader = BoundedReader::new(&chunk, 0, chunk.len()).expect("bounded legacy linetype");
    let mut warnings = Diagnostics::new();
    let descriptor = settings::parse_direct_linetype(
        &cadmpeg_test_support::service_decode_context(),
        &chunk,
        &mut reader,
        archive,
        &mut warnings,
    )
    .expect("legacy documented segment tags are admitted");
    assert_eq!(descriptor.version, (1, 1));
    assert_eq!(reader.remaining(), 0);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn linetype_out_of_order_id_leaves_value_at_boundary() {
    let archive = ArchiveVersion::V8;
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    let mut body = Vec::new();
    body.extend(2_i32.to_le_bytes());
    body.extend(4_i32.to_le_bytes());
    body.extend(&model_attributes);
    body.extend(0_i32.to_le_bytes());
    body.push(3);
    body.extend(2.0_f64.to_le_bytes());
    // Item 1 follows item 3. The source cascade consumes the ID and closes;
    // the one-byte value has no generic width.
    body.extend([1, 0xaa]);
    #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
    let chunk = crc_chunk_excluding(
        archive,
        0x4000_8000,
        &body,
        std::slice::from_ref(&(8..8 + model_attributes.len())),
    );
    let mut reader = BoundedReader::new(&chunk, 0, chunk.len()).expect("bounded linetype");
    let mut warnings = Diagnostics::new();
    settings::parse_direct_linetype(
        &cadmpeg_test_support::service_decode_context(),
        &chunk,
        &mut reader,
        archive,
        &mut warnings,
    )
    .expect("source ordered cascade leaves out-of-order value bounded");
    assert_eq!(reader.remaining(), 0);
    assert!(warnings.is_empty());
}

#[test]
fn future_section_style_extension_stops_at_unknown_code() {
    let archive = ArchiveVersion::V8;
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    let mut body = Vec::new();
    body.extend(1_i32.to_le_bytes());
    body.extend(4_i32.to_le_bytes());
    body.extend(&model_attributes);
    body.extend([12, 0xaa, 0xbb, 0, 0xde]);
    #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
    let chunk = crc_chunk_excluding(
        archive,
        0x4000_8000,
        &body,
        std::slice::from_ref(&(8..8 + model_attributes.len())),
    );
    let mut reader = BoundedReader::new(&chunk, 0, chunk.len()).expect("bounded section style");
    let mut warnings = Diagnostics::new();
    let descriptor = settings::parse_direct_section_style(
        &cadmpeg_test_support::service_decode_context(),
        &chunk,
        &mut reader,
        archive,
        &mut warnings,
    )
    .expect("future section-style code is bounded by the anonymous chunk");
    assert_eq!(descriptor.version, (1, 4));
    assert_eq!(reader.remaining(), 0);
    assert!(warnings.is_empty());
}

#[test]
fn section_style_out_of_order_id_leaves_value_at_boundary() {
    let archive = ArchiveVersion::V8;
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    let mut body = Vec::new();
    body.extend(1_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend(&model_attributes);
    body.push(5);
    body.extend(2.0_f64.to_le_bytes());
    // The source reader has passed item 1 after consuming item 5. It reads
    // this ID and closes the anonymous chunk; the value has no generic width.
    body.extend([1, 0xaa]);
    #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
    let chunk = crc_chunk_excluding(
        archive,
        0x4000_8000,
        &body,
        std::slice::from_ref(&(8..8 + model_attributes.len())),
    );
    let mut reader = BoundedReader::new(&chunk, 0, chunk.len()).expect("bounded section style");
    let mut warnings = Diagnostics::new();
    settings::parse_direct_section_style(
        &cadmpeg_test_support::service_decode_context(),
        &chunk,
        &mut reader,
        archive,
        &mut warnings,
    )
    .expect("source ordered cascade leaves out-of-order value bounded");
    assert_eq!(reader.remaining(), 0);
    assert!(warnings.is_empty());
}

#[test]
fn parses_selector_widths_and_skips_direct_suffix() {
    let mut settings_value = settings::DocumentSettings::default();
    let mut material_data = 42_i32.to_le_bytes().to_vec();
    material_data.extend(3_i32.to_le_bytes());
    material_data.extend([0xaa, 0xbb]);
    let material_record =
        crate::container::Record::long(0x2000_8039, 0..material_data.len(), 0..material_data.len());
    settings::parse_setting(
        &cadmpeg_test_support::service_decode_context(),
        &material_data,
        &material_record,
        &mut settings_value,
        ArchiveVersion::V8,
    )
    .expect("required invariant");
    assert_eq!(
        settings_value
            .current_material
            .map(|selection| selection.value),
        Some(42)
    );
    assert_eq!(
        settings_value
            .current_material
            .map(|selection| selection.source),
        Some(3)
    );

    let mut color_data = vec![1, 2, 3, 4];
    color_data.extend(2_i32.to_le_bytes());
    color_data.extend([0xcc, 0xdd]);
    let color_record =
        crate::container::Record::long(0x2000_803a, 0..color_data.len(), 0..color_data.len());
    settings::parse_setting(
        &cadmpeg_test_support::service_decode_context(),
        &color_data,
        &color_record,
        &mut settings_value,
        ArchiveVersion::V8,
    )
    .expect("required invariant");
    assert_eq!(
        settings_value
            .current_color
            .map(|selection| selection.value),
        Some([1, 2, 3, 4])
    );
    assert_eq!(
        settings_value
            .current_color
            .map(|selection| selection.source),
        Some(2)
    );

    for (typecode, value) in [
        (0xa000_0038, 3),
        (0xa000_003c, 5),
        (0xa000_0132, 7),
        (0xa000_0133, 9),
    ] {
        let record = crate::container::Record::short(typecode, 0..0, value);
        settings::parse_setting(
            &cadmpeg_test_support::service_decode_context(),
            &[],
            &record,
            &mut settings_value,
            ArchiveVersion::V8,
        )
        .expect("required invariant");
    }
    assert_eq!(settings_value.current_layer, Some(3));
    assert_eq!(settings_value.current_wire_density, Some(5));
    assert_eq!(settings_value.current_font, Some(7));
    assert_eq!(settings_value.current_dimstyle, Some(9));
}

#[test]
fn current_material_accepts_the_source_reader_i32_range() {
    let mut data = (-2_i32).to_le_bytes().to_vec();
    data.extend(3_i32.to_le_bytes());
    let record = crate::container::Record::long(0x2000_8039, 0..data.len(), 0..data.len());
    let mut settings_value = settings::DocumentSettings::default();

    settings::parse_setting(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        &record,
        &mut settings_value,
        ArchiveVersion::V8,
    )
    .expect("source reader accepts every signed i32 material index");

    assert_eq!(
        settings_value
            .current_material
            .map(|selection| selection.value),
        Some(-2)
    );
    assert_eq!(
        settings_value
            .current_material
            .map(|selection| selection.source),
        Some(3)
    );
}

#[test]
fn duplicate_singleton_settings_use_the_later_valid_record_and_report_it() {
    let table = metadata_table(
        0x1000_0015,
        0,
        vec![
            crate::container::Record::short(0xa000_0038, 0..0, 3),
            crate::container::Record::short(0xa000_0038, 0..0, 7),
        ],
    );
    let mut warnings = Diagnostics::new();
    let metadata = parse_test_metadata(&[], ArchiveVersion::V5, &[table], &mut warnings);
    assert_eq!(metadata.settings.current_layer, Some(7));
    assert_eq!(
        warnings.messages().collect::<Vec<_>>(),
        ["duplicate singleton metadata record 0xa0000038; later record wins"]
    );
}
