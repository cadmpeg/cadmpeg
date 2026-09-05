use super::{
    annotation_settings, grid_defaults, render_settings, render_userdata, ANONYMOUS, CLASS_END,
    CLASS_USERDATA,
};
use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{
    anonymous_chunk, crc_chunk, long_chunk, metadata_record, short_chunk, utf16_bytes,
};
use crate::wire::Uuid;

fn push_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend(value.to_le_bytes());
}

fn push_f64(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend(value.to_le_bytes());
}

fn push_color(bytes: &mut Vec<u8>, value: [u8; 4]) {
    bytes.extend(value);
}

fn legacy_body(version: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_i32(&mut bytes, version);
    push_i32(&mut bytes, 1);
    push_i32(&mut bytes, 1234);
    push_i32(&mut bytes, 567);
    push_color(&mut bytes, [1, 2, 3, 4]);
    push_i32(&mut bytes, 2);
    push_color(&mut bytes, [5, 6, 7, 8]);
    bytes.extend(utf16_bytes("background.png"));
    for value in [1, 0, 1, 0, 1, 0, 1, 0, 1] {
        push_i32(&mut bytes, value);
    }
    for value in [3, 2, 2048, 1024] {
        push_i32(&mut bytes, value);
    }
    push_f64(&mut bytes, 1.25);
    if version >= 101 {
        push_f64(&mut bytes, 144.5);
        push_i32(&mut bytes, 2);
    }
    if version >= 102 {
        push_color(&mut bytes, [9, 10, 11, 12]);
    }
    if version >= 103 {
        bytes.push(1);
    }
    bytes.extend([0xaa, 0xbb]);
    bytes
}

fn modern_body(minor: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_i32(&mut bytes, 1);
    push_i32(&mut bytes, minor);
    bytes.push(1);
    push_i32(&mut bytes, 1234);
    push_i32(&mut bytes, 567);
    push_f64(&mut bytes, 144.5);
    push_i32(&mut bytes, 2);
    push_color(&mut bytes, [1, 2, 3, 4]);
    push_i32(&mut bytes, 2);
    push_color(&mut bytes, [5, 6, 7, 8]);
    push_color(&mut bytes, [9, 10, 11, 12]);
    bytes.extend(utf16_bytes("background.png"));
    bytes.extend([1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0]);
    for value in [3, 2, 2048, 1024] {
        push_i32(&mut bytes, value);
    }
    push_f64(&mut bytes, 1.25);
    if minor >= 1 {
        push_i32(&mut bytes, 2);
        push_f64(&mut bytes, 100.0);
        push_f64(&mut bytes, 64.0);
        push_f64(&mut bytes, 0.1);
        push_i32(&mut bytes, 10);
    }
    if minor >= 2 {
        push_i32(&mut bytes, 2);
        bytes.extend(utf16_bytes("specific-viewport"));
        bytes.extend(utf16_bytes("named-view"));
        bytes.extend(utf16_bytes("snapshot"));
    }
    if minor >= 3 {
        bytes.push(1);
    }
    bytes.extend([0xaa, 0xbb]);
    bytes
}

fn annotation_body(minor: u8) -> Vec<u8> {
    let mut bytes = vec![0x10 | minor];
    for value in [1.0, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5] {
        push_f64(&mut bytes, value);
    }
    push_i32(&mut bytes, 2);
    for value in [4, 1, 2, 3, 2, 6] {
        push_i32(&mut bytes, value);
    }
    bytes.extend(utf16_bytes("WitnessFace"));
    if minor >= 1 {
        push_f64(&mut bytes, 1.25);
        bytes.push(0);
    }
    if minor >= 2 {
        push_f64(&mut bytes, 2.5);
        bytes.push(0);
    }
    if minor >= 3 {
        bytes.extend([1, 0]);
    }
    if minor >= 4 {
        bytes.push(1);
        bytes.extend([0; 16]);
    }
    bytes.extend([0xde, 0xad]);
    bytes
}

fn grid_body() -> Vec<u8> {
    let mut bytes = vec![0x1f];
    push_f64(&mut bytes, 2.5);
    push_f64(&mut bytes, 0.75);
    for value in [42, 3, 0, 1, 0] {
        push_i32(&mut bytes, value);
    }
    bytes.extend([0xde, 0xad]);
    bytes
}

#[test]
fn legacy_render_settings_gate_each_v5_suffix() {
    let value_100 = render_settings(
        &legacy_body(100),
        0..legacy_body(100).len(),
        7,
        ArchiveVersion::V5,
        1.0,
    )
    .expect("legacy version 100 settings");
    assert_eq!(value_100.image_dpi, None);
    assert_eq!(value_100.image_unit_system, None);
    assert_eq!(value_100.background_bottom_color, None);
    assert!(!value_100.scale_background_to_fit);

    let value_101 = legacy_body(101);
    let value_101 = render_settings(&value_101, 0..value_101.len(), 7, ArchiveVersion::V5, 1.0)
        .expect("legacy version 101 settings");
    assert_eq!(value_101.image_dpi, Some(144.5));
    assert_eq!(value_101.image_unit_system, Some(2));
    assert_eq!(value_101.background_bottom_color, None);

    let value_102 = legacy_body(102);
    let value_102 = render_settings(&value_102, 0..value_102.len(), 7, ArchiveVersion::V5, 1.0)
        .expect("legacy version 102 settings");
    assert_eq!(value_102.background_bottom_color, Some([9, 10, 11, 12]));
    assert!(!value_102.scale_background_to_fit);

    let value_103 = legacy_body(103);
    let value_103 = render_settings(&value_103, 0..value_103.len(), 7, ArchiveVersion::V5, 1.0)
        .expect("legacy version 103 settings");
    assert!(value_103.scale_background_to_fit);
    assert_eq!(value_103.shadowmap_size_pixels, [2048, 1024]);
}

#[test]
fn annotation_settings_gate_packed_minor_fields_and_skip_suffix() {
    for minor in 0..=5 {
        let bytes = annotation_body(minor);
        let value = annotation_settings(&bytes, 0..bytes.len(), 19, 2.0)
            .expect("annotation settings packed version");

        assert_eq!(value.source_offset, 19);
        assert_eq!(value.dimension_scale, 1.0);
        assert_eq!(value.text_height_mm, 5.0);
        assert_eq!(value.extension_line_extension_mm, 7.0);
        assert_eq!(value.extension_line_offset_mm, 9.0);
        assert_eq!(value.arrow_length_mm, 11.0);
        assert_eq!(value.arrow_width_mm, 13.0);
        assert_eq!(value.center_mark_mm, 15.0);
        assert_eq!(value.dimension_units, 2);
        assert_eq!(value.font_face, "WitnessFace");
        assert_eq!(value.world_view_text_scale, (minor >= 1).then_some(1.25));
        assert_eq!(value.annotation_scaling, (minor >= 1).then_some(false));
        assert_eq!(value.world_view_hatch_scale, (minor >= 2).then_some(2.5));
        assert_eq!(value.hatch_scaling, (minor >= 2).then_some(false));
        assert_eq!(
            value.model_space_annotation_scaling,
            (minor >= 3).then_some(true)
        );
        assert_eq!(
            value.layout_space_annotation_scaling,
            (minor >= 3).then_some(false)
        );
        assert_eq!(value.use_dimension_layer, (minor >= 4).then_some(true));
        assert_eq!(value.dimension_layer_uuid, None);
    }

    let mut bytes = annotation_body(4);
    let uuid_offset = bytes.len() - 2 - 16;
    bytes[uuid_offset..uuid_offset + 16].copy_from_slice(&[
        0x40, 0x30, 0x20, 0x10, 0x60, 0x50, 0x80, 0x70, 0x90, 0xa0, 0xb0, 0xc0, 0xd0, 0xe0, 0xf0,
        0x01,
    ]);
    let value = annotation_settings(&bytes, 0..bytes.len(), 19, 1.0)
        .expect("annotation settings dimension-layer UUID");
    assert_eq!(
        value.dimension_layer_uuid.as_deref(),
        Some("10203040-5060-7080-90a0-b0c0d0e0f001")
    );
}

#[test]
fn grid_defaults_accept_future_minor_and_scale_lengths() {
    let bytes = grid_body();
    let value = grid_defaults(&bytes, 0..bytes.len(), 23, 2.0).expect("grid defaults");

    assert_eq!(value.source_offset, 23);
    assert_eq!(value.grid_spacing_mm, 5.0);
    assert_eq!(value.snap_spacing_mm, 1.5);
    assert_eq!(value.grid_line_count, 42);
    assert_eq!(value.thick_line_frequency, 3);
    assert!(!value.show_grid && value.show_grid_axes && !value.show_world_axes);
}

#[test]
fn modern_render_settings_consumes_known_prefix_and_future_suffix() {
    let body = modern_body(4);
    let bytes = crc_chunk(ArchiveVersion::V8, ANONYMOUS, &body);
    let value = render_settings(&bytes, 0..bytes.len(), 11, ArchiveVersion::V8, 1.0)
        .expect("modern future-minor settings");

    assert_eq!(value.source_offset, 11);
    assert_eq!(value.image_width_pixels, 1234);
    assert_eq!(value.image_height_pixels, 567);
    assert_eq!(value.image_dpi, Some(144.5));
    assert_eq!(value.image_unit_system, Some(2));
    assert_eq!(value.background_bitmap_path, "background.png");
    assert_eq!(
        value.obsolete_focal_blur,
        Some([2.0, 100.0, 64.0, 0.1, 10.0])
    );
    assert_eq!(value.rendering_source, Some(2));
    assert_eq!(value.specific_viewport, "specific-viewport");
    assert_eq!(value.named_view, "named-view");
    assert_eq!(value.snapshot, "snapshot");
    assert_eq!(value.force_viewport_aspect_ratio, Some(true));
    assert!(value.use_hidden_lights && value.flat_shade);
    assert!(!value.depth_cue && !value.transparent_background);
}

#[test]
fn modern_render_settings_rejects_negative_minor() {
    let body = modern_body(-1);
    let bytes = crc_chunk(ArchiveVersion::V8, ANONYMOUS, &body);
    let error = render_settings(&bytes, 0..bytes.len(), 0, ArchiveVersion::V8, 1.0)
        .expect_err("negative modern minor");
    assert!(error
        .to_string()
        .contains("render-settings version is unsupported"));
}

#[test]
fn render_userdata_uses_shared_header_grammar_and_outer_suffix_boundaries() {
    let archive = ArchiveVersion::V8;
    let class_uuid = Uuid::from_wire((1_u8..=16).collect::<Vec<_>>().try_into().expect("UUID"));
    let item_uuid = Uuid::from_wire((17_u8..=32).collect::<Vec<_>>().try_into().expect("UUID"));
    let application_uuid =
        Uuid::from_wire((33_u8..=48).collect::<Vec<_>>().try_into().expect("UUID"));

    let mut header_body = class_uuid.to_wire().to_vec();
    header_body.extend(item_uuid.to_wire());
    header_body.extend(1_i32.to_le_bytes());
    for value in [1.0_f64; 16] {
        header_body.extend(value.to_le_bytes());
    }
    header_body.extend(application_uuid.to_wire());
    header_body.push(0);
    header_body.extend(60_i32.to_le_bytes());
    header_body.extend(202_400_i32.to_le_bytes());
    header_body.extend([0xde, 0xad]);
    let header = crc_chunk(archive, 0x0002_fff9, &header_body);
    let payload = anonymous_chunk(archive, 4, &[0x51, 0x52, 0xbe, 0xef]);
    let mut major_two_body = vec![0x2f];
    major_two_body.extend(header);
    major_two_body.extend(payload);
    major_two_body.extend([0xca, 0xfe]);
    let major_two = long_chunk(archive, CLASS_USERDATA, &major_two_body);

    let mut major_one_body = vec![0x10];
    major_one_body.extend(class_uuid.to_wire());
    major_one_body.extend(item_uuid.to_wire());
    major_one_body.extend(2_i32.to_le_bytes());
    major_one_body.extend([0_u8; 16 * 8]);
    major_one_body.extend(anonymous_chunk(archive, 0, &[0x61, 0x62]));
    let major_one = long_chunk(archive, CLASS_USERDATA, &major_one_body);

    let mut body = major_two;
    body.extend(long_chunk(archive, 0x4000_1234, &[0xaa, 0xbb]));
    body.extend(major_one);
    body.extend(short_chunk(archive, CLASS_END, 0));
    body.extend([0xfa, 0xce]);
    let (data, record) = metadata_record(0x2000_8136, body);
    let descriptor = render_userdata(&data, &record, archive).expect("render userdata");

    assert_eq!(descriptor.source, record.range);
    assert_eq!(descriptor.items.len(), 2);
    assert_eq!(descriptor.unknown_chunks.len(), 1);
    assert_eq!(descriptor.suffix, data.len() - 2..data.len());
    let modern = &descriptor.items[0];
    let crate::objects::UserdataDescriptor::Known {
        version,
        class_uuid: modern_class,
        item_uuid: modern_item,
        copy_count,
        application_uuid: modern_application,
        last_saved_as_goo,
        archive_version,
        writer_version,
        payload_range,
        ..
    } = modern
    else {
        panic!("expected known userdata");
    };
    assert_eq!(*version, (2, 15));
    assert_eq!(*modern_class, class_uuid);
    assert_eq!(*modern_item, item_uuid);
    assert_eq!(*copy_count, 1);
    assert_eq!(*modern_application, Some(application_uuid));
    assert_eq!(*last_saved_as_goo, Some(false));
    assert_eq!(*archive_version, Some(60));
    assert_eq!(*writer_version, Some(202_400));
    assert!(!payload_range.is_empty());
    let legacy = &descriptor.items[1];
    let crate::objects::UserdataDescriptor::Known {
        version,
        copy_count,
        application_uuid,
        last_saved_as_goo,
        archive_version,
        writer_version,
        ..
    } = legacy
    else {
        panic!("expected known userdata");
    };
    assert_eq!(*version, (1, 0));
    assert_eq!(*copy_count, 2);
    assert_eq!(*application_uuid, None);
    assert_eq!(*last_saved_as_goo, None);
    assert_eq!(*archive_version, None);
    assert_eq!(*writer_version, None);
}
