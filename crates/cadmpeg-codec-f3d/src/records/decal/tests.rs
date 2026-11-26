// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;

fn decal_wire_fixture() -> serde_json::Value {
    let base = cadmpeg_core::json!({
        "id": "decal", "scope_record_index": 23, "scope_layout": "compact", "asset_reference_offset": 222,
        "scope_code": 96, "scope_code_offset": 232, "target_group_record_index": 24,
        "target_group_reference_offset": 234, "asset_class_tag": "258", "asset_record_index": 17,
        "asset_byte_offset": 100, "asset_frame_length": 30, "asset_entity_suffix": 50,
        "asset_entity_reference_offset": 120, "name_class_tag": "279", "name_record_index": 18,
        "name_byte_offset": 130, "name_frame_length": 41, "asset_name": "mark.png", "asset_name_offset": 155
    });
    base
}

fn decal_image_fixture() -> super::DesignDecalImage {
    let mut wire = decal_wire_fixture();
    wire["id"] = cadmpeg_core::json!("f3d:native:decal#0");
    json::from_value(wire).unwrap()
}

#[test]
fn decal_image_borrowed_wire_matches_owned_wire_bytes() {
    let image = decal_image_fixture();
    let owned = super::DesignDecalImageWire::from(image.clone());
    assert_eq!(json::to_vec(&image).unwrap(), json::to_vec(&owned).unwrap());
}

#[test]
fn decal_image_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let image = decal_image_fixture();
    let arena_name = "design_decal_images";
    let needed = json::to_vec(&image).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    super::DECAL_IMAGE_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&image))
        .unwrap_err();
    super::DECAL_IMAGE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&image))
        .unwrap();
    assert_eq!(namespace.arenas()[arena_name].len(), 1);
}

#[test]
fn decal_image_wire_derives_consecutive_records_and_scope_offsets() {
    let base = decal_wire_fixture();
    for code in [0, 0x60, 0x61, 0xff] {
        let mut value = base.clone();
        value["scope_code"] = cadmpeg_core::json!(code);
        let wire: crate::records::decal::DesignDecalImageWire =
            json::from_value(value).expect("Decal wire");
        let expected = json::to_string(&wire).expect("Decal wire bytes");
        let image: crate::records::decal::DesignDecalImage =
            serde_json::from_str(&expected).expect("Decal binding");
        assert_eq!(image.scope_byte_offset(), 200);
        assert_eq!(
            json::to_string(&image).expect("Decal binding bytes"),
            expected
        );
    }
    let mut unicode = base.clone();
    unicode["asset_name"] = cadmpeg_core::json!("图😀.png");
    unicode["name_frame_length"] = cadmpeg_core::json!(39);
    unicode["asset_record_index"] = cadmpeg_core::json!(u32::MAX - 1);
    unicode["name_record_index"] = cadmpeg_core::json!(u32::MAX);
    let wire: crate::records::decal::DesignDecalImageWire =
        json::from_value(unicode).expect("Decal Unicode wire");
    let expected = json::to_string(&wire).expect("Decal Unicode bytes");
    let image: crate::records::decal::DesignDecalImage =
        serde_json::from_str(&expected).expect("Decal Unicode name");
    assert_eq!(
        json::to_string(&image).expect("Decal Unicode output"),
        expected
    );
    for field in [
        "asset_reference_offset",
        "scope_code_offset",
        "target_group_reference_offset",
        "asset_frame_length",
        "asset_entity_reference_offset",
        "name_record_index",
        "name_byte_offset",
        "name_frame_length",
        "asset_name_offset",
    ] {
        let mut value = base.clone();
        value[field] = cadmpeg_core::json!(0);
        let error = json::from_value::<crate::records::decal::DesignDecalImage>(value)
            .expect_err("invalid Decal field")
            .to_string();
        assert!(error.contains(field));
    }
    for field in ["asset_class_tag", "name_class_tag", "asset_name"] {
        let mut value = base.clone();
        value[field] = cadmpeg_core::json!("");
        let error = json::from_value::<crate::records::decal::DesignDecalImage>(value)
            .expect_err("empty Decal field")
            .to_string();
        assert!(error.contains(field));
    }
    for field in ["asset_reference_offset", "asset_byte_offset"] {
        let mut value = base.clone();
        value[field] = cadmpeg_core::json!(u64::MAX);
        let error = json::from_value::<crate::records::decal::DesignDecalImage>(value)
            .expect_err("Decal byte extent overflow")
            .to_string();
        assert!(error.contains(field));
    }
    let mut no_successor = base;
    no_successor["asset_record_index"] = cadmpeg_core::json!(u32::MAX);
    no_successor["name_record_index"] = cadmpeg_core::json!(u32::MAX);
    let error = json::from_value::<crate::records::decal::DesignDecalImage>(no_successor)
        .expect_err("Decal record index overflow")
        .to_string();
    assert!(error.contains("name_record_index"));
}

#[test]
fn decal_scope_layouts_place_references_and_asset_frames() {
    use crate::records::decal::DesignDecalScopeLayout;
    for (layout, wire, asset_reference, scope_code, target_group_reference, len, asset_frame) in [
        (
            DesignDecalScopeLayout::Compact,
            "compact",
            21,
            32,
            33,
            44,
            30,
        ),
        (
            DesignDecalScopeLayout::Extended,
            "extended",
            25,
            36,
            37,
            48,
            30,
        ),
        (
            DesignDecalScopeLayout::TypedCompact,
            "typed_compact",
            21,
            72,
            73,
            124,
            70,
        ),
        (
            DesignDecalScopeLayout::TypedExtended,
            "typed_extended",
            25,
            76,
            77,
            128,
            70,
        ),
    ] {
        assert_eq!(json::to_value(layout).unwrap(), wire);
        assert_eq!(layout.asset_reference(), asset_reference);
        assert_eq!(layout.scope_code(), scope_code);
        assert_eq!(layout.target_group_reference(), target_group_reference);
        assert_eq!(layout.len(), len);
        assert_eq!(layout.asset_frame_length(), asset_frame);
    }
}

#[test]
fn decal_image_wire_refuses_an_asset_frame_of_another_layout() {
    let mut value = decal_wire_fixture();
    value["scope_layout"] = cadmpeg_core::json!("typed_compact");
    value["scope_code_offset"] = cadmpeg_core::json!(272);
    value["target_group_reference_offset"] = cadmpeg_core::json!(274);
    let error = json::from_value::<crate::records::decal::DesignDecalImage>(value)
        .expect_err("typed layout with a 30-byte asset frame")
        .to_string();
    assert!(error.contains("asset_frame_length"));
}

#[test]
fn decal_image_wire_admits_a_placement_frame_before_the_asset_record() {
    let placement = cadmpeg_core::json!({
        "byte_offset": 40, "class_tag": "267", "vertex_count": 3, "ordinal": 1,
        "u_axis": [0.5, 0.0, 0.0], "v_axis": [0.0, -0.5, 0.0], "flag_bytes": "010000", "reference_separator": 1, "normal_gap": 4,
        "normal": [0.0, 0.0, -1.0], "origin": [1.0, 2.0, 0.0], "image_name": "mark",
        "triangle_index_count": 3
    });
    let mut value = decal_wire_fixture();
    value["placement"] = placement.clone();
    let image: crate::records::decal::DesignDecalImage =
        json::from_value(value.clone()).expect("Decal binding with a placement");
    assert_eq!(json::to_value(&image).expect("Decal wire"), value);
    let asset_at = value["asset_byte_offset"].clone();
    for (field, bad) in [
        ("byte_offset", asset_at),
        ("class_tag", cadmpeg_core::json!("")),
        ("image_name", cadmpeg_core::json!("")),
        ("normal", cadmpeg_core::json!([0.0, 0.0, 2.0])),
        ("reference_separator", cadmpeg_core::json!(2)),
        ("normal_gap", cadmpeg_core::json!(3)),
        ("normal_gap", cadmpeg_core::json!(6)),
    ] {
        let mut refused = value.clone();
        refused["placement"][field] = bad;
        assert!(
            json::from_value::<crate::records::decal::DesignDecalImage>(refused).is_err(),
            "{field}"
        );
    }
    let mut null = value;
    null["placement"] = serde_json::Value::Null;
    assert!(json::from_value::<crate::records::decal::DesignDecalImage>(null).is_err());
}

#[test]
fn placement_normal_is_parallel_to_the_axes_within_the_tolerance() {
    let u = [1.0f64, 2.0, 0.5];
    let v = [-0.3f64, 0.4, 2.0];
    let cross = [3.8f64, -2.15, 1.0];
    let length = cross.iter().map(|value| value * value).sum::<f64>().sqrt();
    let unit = cross.map(|value| value / length);
    let u_length = u.iter().map(|value| value * value).sum::<f64>().sqrt();
    let in_plane = u.map(|value| value / u_length);
    let placement = |normal: [f64; 3], u_axis: [f64; 3]| {
        json::from_value::<crate::records::decal::DesignDecalPlacement>(cadmpeg_core::json!({
            "byte_offset": 40, "class_tag": "267", "vertex_count": 3, "ordinal": 1,
            "u_axis": u_axis, "v_axis": v, "flag_bytes": "010000",
            "reference_separator": 1, "normal_gap": 4,
            "normal": normal, "origin": [1.0, 2.0, 0.0], "image_name": "mark",
            "triangle_index_count": 3
        }))
        .expect("placement wire")
    };
    let tilted = |degrees: f64| {
        let angle = degrees.to_radians();
        [0, 1, 2].map(|lane| angle.cos() * unit[lane] + angle.sin() * in_plane[lane])
    };
    assert!(placement(unit, u).normal_is_parallel_to_axes());
    assert!(placement(unit.map(|value| -value), u).normal_is_parallel_to_axes());
    assert!(placement(tilted(0.01), u).normal_is_parallel_to_axes());
    assert!(!placement(tilted(0.2), u).normal_is_parallel_to_axes());
    assert!(!placement(in_plane, u).normal_is_parallel_to_axes());
    assert!(!placement(unit, v).normal_is_parallel_to_axes());
}
