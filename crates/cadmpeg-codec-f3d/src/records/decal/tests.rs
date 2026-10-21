// SPDX-License-Identifier: Apache-2.0

#[test]
fn decal_mapping_modes_preserve_all_bytes_with_canonical_known_mode() {
    use crate::records::decal::{DesignDecalMappingMode, UnrecognizedDecalMappingMode};
    assert_eq!(
        DesignDecalMappingMode::from_code(0x60),
        DesignDecalMappingMode::FitToFaces
    );
    assert!(UnrecognizedDecalMappingMode::try_from(0x60).is_err());
    for code in 0..=u8::MAX {
        let mode = DesignDecalMappingMode::from_code(code);
        let wire = code.to_string();
        assert_eq!(serde_json::to_string(&mode).expect("Decal mode wire"), wire);
        assert_eq!(
            serde_json::from_str::<DesignDecalMappingMode>(&wire).expect("Decal mode"),
            mode
        );
        if code != 0x60 {
            let unknown = UnrecognizedDecalMappingMode::try_from(code).expect("unrecognized mode");
            assert_eq!(DesignDecalMappingMode::Unknown(unknown), mode);
        }
    }
}

fn decal_wire_fixture() -> serde_json::Value {
    let base = serde_json::json!({
        "id": "f3d:native:decal#0", "scope_record_index": 23,
        "mapping_mode": 96, "target_group_record_index": 24,
        "asset_class_tag": "258", "asset_record_index": 17, "asset_entity_suffix": 50,
        "name_class_tag": "279", "name_record_index": 18, "asset_name": "mark.png"
    });
    base
}

fn decal_image_fixture() -> super::DesignDecalImage {
    let mut wire = decal_wire_fixture();
    wire["id"] = serde_json::json!("f3d:native:decal#0");
    serde_json::from_value(wire).unwrap()
}

#[test]
fn decal_image_borrowed_wire_matches_owned_wire_bytes() {
    let image = decal_image_fixture();
    let owned = super::DesignDecalImageWire::from(image.clone());
    assert_eq!(
        serde_json::to_vec(&image).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn decal_image_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let image = decal_image_fixture();
    let arena_name = "design_decal_images";
    let needed = serde_json::to_vec(&image).unwrap().len();
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
fn decal_document_retains_content_without_source_layout() {
    let base = decal_wire_fixture();
    for mode in [0, 0x60, 0x61, 0xff] {
        let mut value = base.clone();
        value["mapping_mode"] = serde_json::json!(mode);
        let image: super::DesignDecalImage = serde_json::from_value(value.clone()).unwrap();
        assert!(image.scope_byte_offset().is_none());
        assert!(image.asset.byte_offset.is_none());
        assert_eq!(serde_json::to_value(&image).unwrap(), value);
        let mut fidelity = cadmpeg_ir::SourceFidelity::default();
        image.retain_source(&mut fidelity);
        assert!(fidelity.metadata.is_empty());
    }
    let mut unicode = base.clone();
    unicode["asset_name"] = serde_json::json!("图😀.png");
    unicode["asset_record_index"] = serde_json::json!(u32::MAX - 1);
    unicode["name_record_index"] = serde_json::json!(u32::MAX);
    let image: super::DesignDecalImage = serde_json::from_value(unicode.clone()).unwrap();
    assert_eq!(serde_json::to_value(image).unwrap(), unicode);
    for field in [
        "asset_reference_offset",
        "mapping_mode_offset",
        "target_group_reference_offset",
        "asset_byte_offset",
        "asset_frame_length",
        "asset_entity_reference_offset",
        "name_byte_offset",
        "name_frame_length",
        "asset_name_offset",
    ] {
        let mut value = base.clone();
        value[field] = serde_json::json!(0);
        let error = serde_json::from_value::<super::DesignDecalImage>(value).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn decal_semantic_admission_still_validates_asset_identity_and_names() {
    let base = decal_wire_fixture();
    for field in ["asset_class_tag", "name_class_tag", "asset_name"] {
        let mut value = base.clone();
        value[field] = serde_json::json!("");
        let error = serde_json::from_value::<super::DesignDecalImage>(value).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
    for (asset, name) in [(17, 17), (u32::MAX, u32::MAX)] {
        let mut value = base.clone();
        value["asset_record_index"] = serde_json::json!(asset);
        value["name_record_index"] = serde_json::json!(name);
        let error = serde_json::from_value::<super::DesignDecalImage>(value).unwrap_err();
        assert!(error.to_string().contains("name_record_index"), "{error}");
    }
}

#[test]
fn decal_scanner_validates_real_extents_and_retains_actual_layout_separately() {
    use super::{DesignDecalAsset, DesignDecalImage, DesignDecalMappingMode};
    use crate::records::identity::Located;
    let asset = || {
        DesignDecalAsset::new(
            ["258".into(), "279".into()],
            [17, 18],
            100,
            50,
            "mark.png".into(),
        )
        .unwrap()
    };
    assert!(DesignDecalAsset::new(
        ["258".into(), "279".into()],
        [17, 18],
        u64::MAX,
        50,
        "mark.png".into()
    )
    .is_err());
    assert!(DesignDecalImage::new(
        "f3d:native:decal#0".into(),
        Located {
            value: 23,
            offset: u64::MAX
        },
        DesignDecalMappingMode::FitToFaces,
        24,
        asset()
    )
    .is_err());
    let scanned = DesignDecalImage::new(
        "f3d:native:decal#0".into(),
        Located {
            value: 23,
            offset: 200,
        },
        DesignDecalMappingMode::FitToFaces,
        24,
        asset(),
    )
    .unwrap();
    assert_eq!(scanned.scope_byte_offset(), Some(200));
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    scanned.retain_source(&mut fidelity);
    assert_eq!(
        fidelity.metadata["f3d:native:decal#0.scope_byte_offset"],
        "200"
    );
    assert_eq!(
        fidelity.metadata["f3d:native:decal#0.asset_byte_offset"],
        "100"
    );
    assert!(fidelity.unrepresented().is_empty());
    let wire = serde_json::to_value(&scanned).unwrap();
    assert_eq!(wire, decal_wire_fixture());
    let reloaded: DesignDecalImage = serde_json::from_value(wire).unwrap();
    assert!(reloaded.scope_byte_offset().is_none());
    assert!(reloaded.asset.byte_offset.is_none());
}

#[test]
fn indexed_record_header_keeps_identity_and_class_without_source_position() {
    let source = super::DesignRecordHeader {
        id: "f3d:Design:record#0000000042".into(),
        record_index: 42,
        class_tag: "365".to_owned().try_into().unwrap(),
        byte_offset: Some(987),
    };
    let wire = serde_json::to_value(&source).unwrap();
    assert_eq!(wire.as_object().unwrap().len(), 3);
    assert!(wire.get("byte_offset").is_none());
    let restored: super::DesignRecordHeader = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(restored.id, source.id);
    assert_eq!(restored.record_index, source.record_index);
    assert_eq!(restored.class_tag, source.class_tag);
    assert!(restored.byte_offset.is_none());
    assert_eq!(serde_json::to_value(&restored).unwrap(), wire);
    let mut obsolete_wire = wire;
    obsolete_wire["byte_offset"] = 987.into();
    assert!(serde_json::from_value::<super::DesignRecordHeader>(obsolete_wire).is_err());
    let native = crate::native::F3dNative {
        design_record_headers: vec![source],
        ..Default::default()
    };
    let fidelity = native.retained_fidelity().unwrap();
    assert_eq!(
        fidelity.metadata["f3d:Design:record#0000000042.byte_offset"],
        "987"
    );
    assert!(fidelity.unrepresented().is_empty());
    assert!(fidelity.retained_records().is_empty());
}
