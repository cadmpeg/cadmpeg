// SPDX-License-Identifier: Apache-2.0

use super::super::ViewAttributes;

struct AttributesField<'a>(Option<&'a ViewAttributes>);

impl serde::Serialize for AttributesField<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        super::super::serialize_view_attributes(self.0, serializer)
    }
}

fn sample_attributes() -> ViewAttributes {
    ViewAttributes {
        view_type: 1,
        width: crate::test_support::finite(210.0),
        height: crate::test_support::finite(297.0),
        display: Some("display-uuid".to_string()),
        version: [1, 2].into(),
        page_settings: None,
        projection_locked: false,
        clipping_planes: Vec::new(),
        named_view_uuid: None,
        show_construction_z_axis: false,
        focal_blur_distance_mm: None,
        focal_blur_aperture: None,
        focal_blur_jitter: None,
        focal_blur_sample_count: None,
        focal_blur_mode: None,
        rendering_size_pixels: None,
        section_behavior: None,
    }
}

#[test]
fn view_attributes_projection_emits_the_documented_keys_in_order() {
    let attributes = sample_attributes();
    let json = serde_json::to_string(&AttributesField(Some(&attributes)))
        .expect("view attributes serialize");
    let keys = [
        "\"attributes\":{",
        "\"view_type\":1",
        "\"width\":210.0",
        "\"height\":297.0",
        "\"display\":\"display-uuid\"",
    ];
    // #212 restores the Appendix B keep field ViewAttributes.version.
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["attributes"]["version"], serde_json::json!("0102"));
    assert!(!json.contains("page_width_mm"));
    let mut cursor = 0;
    for key in keys {
        let found = json[cursor..]
            .find(key)
            .unwrap_or_else(|| panic!("{key} missing after offset {cursor} in {json}"));
        cursor += found + key.len();
    }
}

#[test]
fn view_attributes_projection_emits_null_keys_when_absent() {
    let json = serde_json::to_string(&AttributesField(None)).expect("view attributes serialize");
    assert_eq!(json, "{\"attributes\":null}");
}

#[test]
fn typed_view_handoff_keeps_source_flags_modes_camera_vectors_and_absence() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let archive = crate::chunks::ArchiveVersion::V5;
    let end_marker =
        crate::test_support::test_dump::short_chunk(archive, crate::chunks::TCODE_ENDOFTABLE, 0);
    let view_record = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        super::super::VIEW_RECORD,
        &end_marker,
    );
    let mut bytes = 1_i32.to_le_bytes().to_vec();
    bytes.extend(view_record);
    let record =
        crate::container::Record::long(super::super::NAMED_VIEWS, 0..bytes.len(), 0..bytes.len());
    let (mut views, losses) = super::super::parse_list(
        &ctx,
        &bytes,
        &record,
        archive,
        crate::settings::MillimeterScale::IDENTITY,
        super::super::ViewListKind::Named,
    )
    .unwrap();
    assert!(losses.is_empty());
    let mut view = views.pop().unwrap();
    let absent = super::super::ViewProjection::from(view);
    assert!(absent.camera.is_none());
    assert!(absent.modern_display_mode.is_none());
    assert!(!absent.has_attributes);

    let (mut views, losses) = super::super::parse_list(
        &ctx,
        &bytes,
        &record,
        archive,
        crate::settings::MillimeterScale::IDENTITY,
        super::super::ViewListKind::Named,
    )
    .unwrap();
    assert!(losses.is_empty());
    view = views.pop().unwrap();
    view.name = "Source camera".to_owned();
    view.show_construction_grid = false;
    view.show_construction_axes = true;
    view.show_world_axes = false;
    view.legacy_display_mode = Some(7);
    view.attributes = Some(sample_attributes());
    let viewport = super::viewport();
    view.viewport = Some(
        super::super::parse_viewport(
            &viewport,
            0..viewport.len(),
            crate::test_support::millimeter_scale(10.0),
        )
        .unwrap(),
    );
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native
        .namespace_mut("rhino")
        .set_arena(&ctx, "views", &[&view])
        .unwrap();
    let native = ir.native.clone();
    let handoff = super::super::ViewProjection::from(view);
    assert_eq!(ir.native, native);
    assert_eq!(
        handoff.native_id,
        native.namespace("rhino").unwrap().arenas()["views"][0].id()
    );
    assert!(handoff.has_attributes);
    assert_eq!(handoff.name, "Source camera");
    assert!(!handoff.show_construction_grid);
    assert!(handoff.show_construction_axes);
    assert!(!handoff.show_world_axes);
    assert_eq!(handoff.legacy_display_mode, Some(7));
    assert_eq!(handoff.modern_display_mode.as_deref(), Some("display-uuid"));
    let camera = handoff.camera.unwrap();
    assert_eq!(camera.location.map(|value| value.get()), [10.0, 20.0, 30.0]);
    assert_eq!(camera.direction.map(|value| value.get()), [0.0, 0.0, -1.0]);
    assert_eq!(camera.up.map(|value| value.get()), [0.0, 1.0, 0.0]);
    assert_eq!(camera.x_axis.map(|value| value.get()), [1.0, 0.0, 0.0]);
    assert_eq!(camera.y_axis.map(|value| value.get()), [0.0, 1.0, 0.0]);
    assert_eq!(camera.z_axis.map(|value| value.get()), [0.0, 0.0, 1.0]);
}

#[test]
fn native_view_records_keep_version_bytes_and_child_byte_length() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let archive = crate::chunks::ArchiveVersion::V5;
    let end_marker =
        crate::test_support::test_dump::short_chunk(archive, crate::chunks::TCODE_ENDOFTABLE, 0);
    let record_bytes = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        super::super::VIEW_RECORD,
        &end_marker,
    );
    let mut bytes = 1_i32.to_le_bytes().to_vec();
    bytes.extend(record_bytes);
    let record =
        crate::container::Record::long(super::super::NAMED_VIEWS, 0..bytes.len(), 0..bytes.len());
    let (mut views, losses) = super::super::parse_list(
        &ctx,
        &bytes,
        &record,
        archive,
        crate::settings::MillimeterScale::IDENTITY,
        super::super::ViewListKind::Named,
    )
    .unwrap();
    assert!(losses.is_empty());
    let mut view = views.pop().unwrap();
    assert!(views.is_empty());
    let viewport = super::viewport();
    view.viewport = Some(
        super::super::parse_viewport(
            &viewport,
            0..viewport.len(),
            crate::settings::MillimeterScale::IDENTITY,
        )
        .unwrap(),
    );
    let mut position = vec![0x12];
    position.extend(0_i32.to_le_bytes());
    for value in [0.1_f64, 0.9, 0.2, 0.8] {
        position.extend(value.to_le_bytes());
    }
    position.push(3);
    view.window_position =
        Some(super::super::parse_window_position(&position, 0..position.len()).unwrap());
    view.attributes = Some(sample_attributes());
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native
        .namespace_mut("rhino")
        .set_arena(&ctx, "views", &[&view])
        .unwrap();
    let native = &ir.native.namespace("rhino").unwrap().arenas()["views"][0];
    assert_eq!(native.field("viewport").unwrap()["version"], "0105");
    assert_eq!(native.field("window_position").unwrap()["version"], "0102");
    assert_eq!(native.field("attributes").unwrap()["version"], "0102");
    let children = native.field("children").unwrap();
    let children = children.as_array().unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(
        children[0]["byte_len"],
        serde_json::json!(u64::try_from(end_marker.len()).unwrap())
    );
    assert!(children[0].get("source_offset").is_none());
    assert!(children[0].get("sha256").is_none());
}
