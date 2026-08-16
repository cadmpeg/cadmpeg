use super::{annotation_settings, grid_defaults, render_settings, ANONYMOUS};
use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{crc_chunk, utf16_bytes};

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
    for minor in 0..=4 {
        let bytes = annotation_body(minor);
        let value = annotation_settings(&bytes, 0..bytes.len(), 19, 2.0)
            .expect("annotation settings packed version");

        assert_eq!(value.source_offset, 19);
        assert_eq!(value.dimension_scale, 1.0);
        assert_eq!(value.text_height_mm, 5.0);
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
