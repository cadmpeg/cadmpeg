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
        "\"view_type\":1",
        "\"page_width_mm\":210.0",
        "\"page_height_mm\":297.0",
        "\"display_mode_uuid\":\"display-uuid\"",
        "\"attributes_version\":\"0102\"",
        "\"attributes\":{",
    ];
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
    assert_eq!(
        json,
        concat!(
            "{\"view_type\":null,",
            "\"page_width_mm\":null,",
            "\"page_height_mm\":null,",
            "\"display_mode_uuid\":null,",
            "\"attributes_version\":null,",
            "\"attributes\":null}"
        )
    );
}
