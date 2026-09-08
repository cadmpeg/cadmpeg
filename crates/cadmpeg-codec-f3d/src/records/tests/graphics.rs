// SPDX-License-Identifier: Apache-2.0

#[test]
fn mesh_feature_body_rows_preserve_wire_and_reject_duplicate_arrays() {
    let identity = serde_json::json!({
        "class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 200
    });
    let body = serde_json::json!({
        "body_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 575},
        "entry_name_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 62}, "guid_record": identity,
        "wrapper_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 40},
        "scene_state_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 95}, "scene_node_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 133},
        "scene_auxiliary_record": identity, "owner_record": identity,
        "entry_name": "mesh.paramesh", "entry_name_offset": 136,
        "fusion_uuid": "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE", "fusion_uuid_offset": 136,
        "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "transform_offsets": [142, 271], "scope_reference_offset": 608,
        "wrapper_reference_offset": 619, "owner_reference_offset": 630,
        "guid_reference_offset": 641, "scene_node_reference_offset": 653,
        "collection_reference_offset": 664, "wrapper_body_reference_offset": 121,
        "entry_guid_reference_offset": 121, "guid_entry_reference_offset": 172,
        "scene_state_reference_offset": 133, "scene_auxiliary_reference_offset": 148
    });
    let base = serde_json::json!({
        "id": "mesh-feature", "scope_record": identity, "scope_base_record": {"class_tag": "256", "record_index": 104, "byte_offset": 270, "frame_length": 30},
        "collection_record": {"class_tag": "256", "record_index": 104, "byte_offset": 0, "frame_length": 95}, "collection_base_record": {"class_tag": "256", "record_index": 104, "byte_offset": 38, "frame_length": 57},
        "texture_table_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 29}, "body_count_offsets": [121, 21, 58],
        "body_record_indices": [104, 104], "scope_body_reference_offsets": [125, 136],
        "collection_body_reference_offsets": [62, 73], "texture_table_reference_offset": 27,
        "collection_owner_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 273}, "collection_owner_reference_offset": 84,
        "collection_owner_backlink_offset": 362, "scope_owner_record_index": 109,
        "scope_owner_reference_offset": 289, "texture_flags_count_offset": 121,
        "texture_filename_count_offset": 125, "bodies": [body, body], "textures": []
    });
    for count in 0..=2 {
        let mut value = base.clone();
        for field in [
            "bodies",
            "body_record_indices",
            "scope_body_reference_offsets",
            "collection_body_reference_offsets",
        ] {
            value[field]
                .as_array_mut()
                .expect("wire array")
                .truncate(count);
        }
        value["collection_record"]["frame_length"] = serde_json::json!(73 + 11 * count);
        value["collection_base_record"]["frame_length"] = serde_json::json!(35 + 11 * count);
        value["collection_owner_reference_offset"] = serde_json::json!(62 + 11 * count);
        let wire: crate::records::DesignMeshFeatureWire =
            serde_json::from_value(value).expect("mesh wire");
        let expected = serde_json::to_string(&wire).expect("original mesh wire");
        let feature: crate::records::DesignMeshFeature =
            serde_json::from_str(&expected).expect("mesh body rows");
        assert_eq!(
            serde_json::to_string(&feature).expect("mesh wire"),
            expected
        );
    }
    for field in [
        "body_record_indices",
        "scope_body_reference_offsets",
        "collection_body_reference_offsets",
        "bodies",
    ] {
        let mut value = base.clone();
        value[field].as_array_mut().expect("wire array").pop();
        assert!(serde_json::from_value::<crate::records::DesignMeshFeature>(value).is_err());
    }
    for field in [
        "body_count_offsets",
        "scope_body_reference_offsets",
        "collection_body_reference_offsets",
    ] {
        let mut value = base.clone();
        value[field][0] = serde_json::json!(0);
        let error = serde_json::from_value::<crate::records::DesignMeshFeature>(value)
            .expect_err("misplaced body reference")
            .to_string();
        assert!(error.contains(field));
    }
    let mut wrong_count = base.clone();
    wrong_count["collection_record"]["frame_length"] = serde_json::json!(84);
    wrong_count["collection_base_record"]["frame_length"] = serde_json::json!(46);
    wrong_count["collection_owner_reference_offset"] = serde_json::json!(73);
    let error = serde_json::from_value::<crate::records::DesignMeshFeature>(wrong_count)
        .expect_err("body count mismatch")
        .to_string();
    assert!(error.contains("bodies count"));
    let mut short_scope = base.clone();
    short_scope["scope_record"]["frame_length"] = serde_json::json!(76);
    short_scope["scope_base_record"]["byte_offset"] = serde_json::json!(146);
    short_scope["scope_owner_reference_offset"] = serde_json::json!(165);
    let error = serde_json::from_value::<crate::records::DesignMeshFeature>(short_scope)
        .expect_err("body references overlap base")
        .to_string();
    assert!(error.contains("bodies reference run"));
    let mut changed_identity = base;
    changed_identity["body_record_indices"][0] = serde_json::json!(105);
    let error = serde_json::from_value::<crate::records::DesignMeshFeature>(changed_identity)
        .expect_err("conflicting body identity")
        .to_string();
    assert!(error.contains("body_record_indices"));
}

#[test]
fn mesh_scene_bounds_preserve_wire_and_check_corners_and_offsets() {
    let wire = r#"{"maximum":[1.0,2.0,3.0],"minimum":[-4.0,-5.0,-6.0],"offsets":[100,124]}"#;
    let bounds = crate::records::DesignMeshSceneBounds::from_wire(
        serde_json::from_str(wire).unwrap(),
        [100, 124],
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&bounds.into_wire([100, 124])).unwrap(),
        wire
    );
    assert_eq!(bounds.into_wire([100, 124]).offsets, [100, 124]);
    for (field, bad) in [
        ("minimum", serde_json::json!([2.0, -5.0, -6.0])),
        ("maximum", serde_json::json!([-5.0, 2.0, 3.0])),
        ("offsets", serde_json::json!([100, 125])),
        ("offsets", serde_json::json!([u64::MAX, 0])),
    ] {
        let mut invalid = serde_json::to_value(bounds.into_wire([100, 124])).unwrap();
        invalid[field] = bad;
        assert!(crate::records::DesignMeshSceneBounds::from_wire(
            serde_json::from_value(invalid).unwrap(),
            [100, 124]
        )
        .unwrap_err()
        .clone()
        .contains(field));
    }
    assert!(crate::records::DesignMeshSceneBounds::new([0.0; 3], [0.0; 3]).is_ok());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(crate::records::DesignMeshSceneBounds::new([value, 1.0, 1.0], [0.0; 3]).is_err());
        assert!(crate::records::DesignMeshSceneBounds::new([1.0; 3], [value, 0.0, 0.0]).is_err());
    }
}

#[test]
fn mesh_record_identity_preserves_wire_and_rejects_invalid_headers() {
    let wire = r#"{"class_tag":"256","record_index":1,"byte_offset":100,"frame_length":11}"#;
    let record: crate::records::DesignMeshRecordIdentity = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&record).unwrap(), wire);
    for (field, bad) in [
        ("class_tag", serde_json::json!("25x")),
        ("class_tag", serde_json::json!("2560")),
        ("record_index", serde_json::json!(0)),
        ("frame_length", serde_json::json!(10)),
        ("frame_length", serde_json::json!(u64::MAX)),
    ] {
        let mut invalid = serde_json::to_value(&record).unwrap();
        invalid[field] = bad;
        assert!(
            serde_json::from_value::<crate::records::DesignMeshRecordIdentity>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field)
        );
    }
}

#[test]
fn mesh_affine_transform_preserves_rows_and_rejects_invalid_maps() {
    let rows = [
        [-2.0, 0.0, 0.0, 3.0],
        [0.0, 4.0, 0.0, 5.0],
        [0.0, 0.0, 6.0, 7.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let value = crate::records::MeshAffineTransform::try_from(rows).unwrap();
    let json = serde_json::to_value(rows).unwrap();
    assert_eq!(serde_json::to_value(value).unwrap(), json);
    assert_eq!(
        serde_json::from_value::<crate::records::MeshAffineTransform>(json).unwrap(),
        value
    );
    for (row, column, invalid) in [(3, 0, 1.0), (3, 3, 0.0), (1, 1, 0.0)] {
        let mut changed = rows;
        changed[row][column] = invalid;
        assert!(
            serde_json::from_value::<crate::records::MeshAffineTransform>(
                serde_json::to_value(changed).unwrap()
            )
            .is_err()
        );
    }
    let mut cells = value.cells();
    cells[0] = f64::INFINITY;
    assert!(crate::records::MeshAffineTransform::new(cells).is_err());
    cells[0] = f64::MAX;
    assert!(crate::records::MeshAffineTransform::new(cells).is_err());
}

#[test]
fn mesh_texture_file_derives_basename_and_offset_without_wire_changes() {
    fn parse(wire: serde_json::Value) -> Result<crate::records::DesignMeshTextureTable, String> {
        let record = crate::records::DesignMeshRecordIdentity::new(
            crate::records::DesignClassTag::try_from("256".to_owned())?,
            4,
            0,
            124,
        )?;
        crate::records::DesignMeshTextureTable::from_wire(
            record,
            21,
            69,
            vec![serde_json::from_value(wire).map_err(|error| error.to_string())?],
        )
    }
    let wire = serde_json::json!({
        "ordinal": 0, "resource_guid": "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
        "flags_guid_offset": 29, "flags": 7, "flags_offset": 65,
        "filename_ordinal": 0, "filename_guid_offset": 77,
        "filename_record": {"class_tag": "256", "record_index": 8, "byte_offset": 200, "frame_length": 39},
        "filename_record_reference_offset": 113,
        "filename": "é😀.png", "filename_offset": 225,
        "archive_entry_name": "Textures/é😀.png", "asset": "asset:texture"
    });
    // The basename has seven UTF-16 code units, including the surrogate pair.
    let table = parse(wire.clone()).unwrap();
    let resource = &table.resources()[0];
    assert_eq!(resource.file.filename(), "é😀.png");
    assert_eq!(resource.file.filename_offset(), 225);
    assert_eq!(serde_json::to_value(&table.into_wire().3[0]).unwrap(), wire);
    for field in ["filename", "archive_entry_name", "filename_offset"] {
        let mut bad = wire.clone();
        bad[field] = if field == "filename_offset" {
            226.into()
        } else {
            "other.png".into()
        };
        assert!(parse(bad).is_err());
    }
    let mut bad = wire.clone();
    bad["filename_record"]["frame_length"] = 38.into();
    assert!(parse(bad).is_err());
    let mut bad = wire;
    bad["filename_record"]["byte_offset"] = (u64::MAX - 20).into();
    assert!(parse(bad).is_err());
}

#[test]
fn design_guid_text_preserves_case_and_rejects_non_guids() {
    let wire = "\"aAaAaAaA-bBbB-4cCc-8dDd-eEeEeEeEeEeE\"";
    let value: crate::records::DesignGuidText = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&value).unwrap(), wire);
    for invalid in [
        "",
        "AAAAAAAA_BBBB-4CCC-8DDD-EEEEEEEEEEEE",
        "GAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
    ] {
        assert!(crate::records::DesignGuidText::try_from(invalid.to_owned()).is_err());
    }
}

#[test]
fn mesh_guid_record_requires_prefix_and_derives_join_offsets() {
    let guid =
        crate::records::DesignGuidText::try_from("AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".to_owned())
            .unwrap();
    for frame_length in [83, 100] {
        let identity = crate::records::DesignMeshRecordIdentity::new(
            crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            200,
            frame_length,
        )
        .unwrap();
        let record = crate::records::DesignMeshGuid::new(identity, guid.clone()).unwrap();
        assert_eq!(record.value_offset(), 236);
        assert_eq!(record.entry_reference_offset(), 272);
        assert_eq!(record.record().frame_length(), frame_length);
    }
    let short = crate::records::DesignMeshRecordIdentity::new(
        crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
        4,
        200,
        82,
    )
    .unwrap();
    assert!(crate::records::DesignMeshGuid::new(short, guid).is_err());
}

#[test]
fn mesh_uuid_preserves_wire_and_requires_lowercase_version_four() {
    let text = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let value = crate::records::DesignMeshUuid::try_from(text.to_owned()).unwrap();
    assert_eq!(value.as_str(), text);
    assert_eq!(
        serde_json::to_value(&value).unwrap(),
        serde_json::json!(text)
    );
    assert_eq!(
        serde_json::from_value::<crate::records::DesignMeshUuid>(serde_json::json!(text)).unwrap(),
        value
    );
    for invalid in [
        "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
        "aaaaaaaa-bbbb-3ccc-8ddd-eeeeeeeeeeee",
        "aaaaaaaa-bbbb-4ccc-7ddd-eeeeeeeeeeee",
        "",
    ] {
        assert!(
            serde_json::from_value::<crate::records::DesignMeshUuid>(serde_json::json!(invalid))
                .is_err()
        );
    }
}

#[test]
fn mesh_entry_name_layout_uses_utf16_units_and_exact_record_end() {
    let identity = |length| {
        crate::records::DesignMeshRecordIdentity::new(
            crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            100,
            length,
        )
        .unwrap()
    };
    let entry = crate::records::DesignMeshEntryName::new(identity(42), "a😀".to_owned()).unwrap();
    assert_eq!(entry.name_offset(), 136);
    assert_eq!(entry.guid_reference_offset(), 121);
    assert_eq!(entry.name(), "a😀");
    assert!(crate::records::DesignMeshEntryName::new(identity(40), "a😀".to_owned()).is_err());
    assert!(crate::records::DesignMeshEntryName::new(identity(44), "a😀".to_owned()).is_err());
    assert!(crate::records::DesignMeshEntryName::new(identity(36), String::new()).is_err());
}

#[test]
fn mesh_placement_layout_requires_prefix_and_terminal_reference() {
    let transform = crate::records::MeshAffineTransform::try_from([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    let identity = |length| {
        crate::records::DesignMeshRecordIdentity::new(
            crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            100,
            length,
        )
        .unwrap()
    };
    for length in [575, 800] {
        let placement =
            crate::records::DesignMeshPlacement::new(identity(length), transform).unwrap();
        assert_eq!(placement.transform_offsets(), [142, 271]);
        assert_eq!(placement.scope_reference_offset(), 608);
        assert_eq!(placement.wrapper_reference_offset(), 619);
        assert_eq!(placement.owner_reference_offset(), 630);
        assert_eq!(placement.guid_reference_offset(), 641);
        assert_eq!(placement.scene_node_reference_offset(), 653);
        assert_eq!(placement.collection_reference_offset(), 100 + length - 11);
    }
    assert!(crate::records::DesignMeshPlacement::new(identity(574), transform).is_err());
}

#[test]
fn mesh_fixed_record_derives_length_and_rejects_another_layout() {
    let identity = |length| {
        crate::records::DesignMeshRecordIdentity::new(
            crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            100,
            length,
        )
        .unwrap()
    };
    let wrapper = crate::records::DesignMeshFixedRecord::<40>::try_from(identity(40)).unwrap();
    assert_eq!(wrapper.byte_offset(), 100);
    assert_eq!(wrapper.record_index(), 4);
    let roundtrip: crate::records::DesignMeshRecordIdentity = wrapper.into();
    assert_eq!(roundtrip, identity(40));
    assert!(crate::records::DesignMeshFixedRecord::<40>::try_from(identity(95)).is_err());
    assert!(crate::records::DesignMeshFixedRecord::<95>::try_from(identity(40)).is_err());
}

#[test]
fn mesh_scene_forms_derive_bounds_and_transform_locations() {
    let identity = |length| {
        crate::records::DesignMeshRecordIdentity::new(
            crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            200,
            length,
        )
        .unwrap()
    };
    let bounds =
        crate::records::DesignMeshSceneBounds::new([1.0, 2.0, 3.0], [-1.0, -2.0, -3.0]).unwrap();
    let transform = crate::records::MeshAffineTransform::try_from([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    for (length, placement, offsets) in
        [(133, None, [284, 308]), (261, Some(transform), [412, 436])]
    {
        let node =
            crate::records::DesignMeshSceneNode::new(identity(length), Some(bounds), placement)
                .unwrap();
        assert_eq!(node.bounds_offsets(), offsets);
        assert_eq!(node.state_reference_offset(), 233);
        assert_eq!(node.auxiliary_reference_offset(), 248);
        assert_eq!(
            node.transform().map(|located| located.offset),
            placement.map(|_| 284)
        );
        let (record, bound_wire, transform_wire) = node.clone().into_wire();
        assert_eq!(record, identity(length));
        assert_eq!(bound_wire.as_ref().unwrap().offsets, offsets);
        assert_eq!(
            crate::records::DesignMeshSceneNode::from_wire(
                record.clone(),
                bound_wire.clone(),
                transform_wire
            )
            .unwrap(),
            node
        );
        let mut bad_bounds = bound_wire.unwrap();
        bad_bounds.offsets[0] += 1;
        assert!(crate::records::DesignMeshSceneNode::from_wire(
            record,
            Some(bad_bounds),
            transform_wire
        )
        .is_err());
    }
    assert!(
        crate::records::DesignMeshSceneNode::new(identity(133), None, Some(transform)).is_err()
    );
    assert!(crate::records::DesignMeshSceneNode::new(identity(261), None, None).is_err());
    assert!(crate::records::DesignMeshSceneNode::from_wire(
        identity(261),
        None,
        Some(crate::records::Located {
            value: transform,
            offset: 285
        })
    )
    .is_err());
    let state =
        crate::records::DesignMeshSceneState::new(identity(95).try_into().unwrap(), Some(bounds));
    let (record, bound_wire) = state.clone().into_wire();
    assert_eq!(bound_wire.as_ref().unwrap().offsets, [246, 270]);
    assert_eq!(
        crate::records::DesignMeshSceneState::from_wire(record.clone(), bound_wire.clone())
            .unwrap(),
        state
    );
    let mut bad_bounds = bound_wire.unwrap();
    bad_bounds.offsets = [247, 271];
    assert!(crate::records::DesignMeshSceneState::from_wire(record, Some(bad_bounds)).is_err());
}

#[test]
fn mesh_collection_owner_derives_fixed_and_terminal_backlinks() {
    let identity = |length| {
        crate::records::DesignMeshRecordIdentity::new(
            crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            100,
            length,
        )
        .unwrap()
    };
    for (length, relative) in [
        (252, 241),
        (400, 241),
        (273, 262),
        (400, 262),
        (200, 189),
        (400, 389),
    ] {
        let owner =
            crate::records::DesignMeshCollectionOwner::new(identity(length), 100 + relative)
                .unwrap();
        assert_eq!(owner.backlink_offset(), 100 + relative);
        assert_eq!(owner.record(), &identity(length));
        assert_eq!(
            crate::records::DesignMeshCollectionOwner::new(
                owner.record().clone(),
                owner.backlink_offset()
            )
            .unwrap(),
            owner
        );
    }
    for (length, offset) in [(250, 341), (272, 362), (400, 99), (400, 488)] {
        assert!(crate::records::DesignMeshCollectionOwner::new(identity(length), offset).is_err());
    }
}

#[test]
fn mesh_texture_table_checks_permutations_and_preserves_wire_row_order() {
    const GUID_A: &str = "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE";
    const GUID_B: &str = "BBBBBBBB-BBBB-4CCC-8DDD-EEEEEEEEEEEE";
    let record = |length| {
        crate::records::DesignMeshRecordIdentity::new(
            crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            0,
            length,
        )
        .unwrap()
    };
    let row = |ordinal, filename_ordinal, guid, flags_guid, filename_guid| {
        serde_json::json!({
            "ordinal": ordinal, "resource_guid": guid, "flags_guid_offset": flags_guid,
            "flags": 7, "flags_offset": flags_guid + 36,
            "filename_ordinal": filename_ordinal, "filename_guid_offset": filename_guid,
            "filename_record": {"class_tag": "256", "record_index": 8, "byte_offset": 300, "frame_length": 35},
            "filename_record_reference_offset": filename_guid + 36,
            "filename": "a.png", "filename_offset": 325,
            "archive_entry_name": "Textures/a.png", "asset": "asset:texture"
        })
    };
    let rows = serde_json::json!([row(1, 0, GUID_B, 73, 121), row(0, 1, GUID_A, 29, 172)]);
    let parse = |rows: serde_json::Value| {
        crate::records::DesignMeshTextureTable::from_wire(
            record(219),
            21,
            113,
            serde_json::from_value(rows).unwrap(),
        )
    };
    let table = parse(rows.clone()).unwrap();
    assert_eq!(
        table
            .resources_in_flags_order()
            .iter()
            .map(|resource| resource.resource_guid.as_str())
            .collect::<Vec<_>>(),
        [GUID_A, GUID_B]
    );
    let (identity, flags_count, filename_count, encoded) = table.into_wire();
    assert_eq!(identity, record(219));
    assert_eq!((flags_count, filename_count), (21, 113));
    assert_eq!(serde_json::to_value(encoded).unwrap(), rows);
    let mut duplicate = rows.clone();
    duplicate[0]["ordinal"] = 0.into();
    duplicate[0]["flags_guid_offset"] = 29.into();
    duplicate[0]["flags_offset"] = 65.into();
    assert!(parse(duplicate).unwrap_err().contains("ordinal"));
    let mut duplicate = rows.clone();
    duplicate[0]["filename_ordinal"] = 1.into();
    duplicate[0]["filename_guid_offset"] = 172.into();
    duplicate[0]["filename_record_reference_offset"] = 208.into();
    assert!(parse(duplicate).unwrap_err().contains("filename_ordinal"));
    let mut duplicate = rows.clone();
    duplicate[0]["resource_guid"] = GUID_A.to_ascii_lowercase().into();
    assert!(parse(duplicate).unwrap_err().contains("resource_guid"));
    let mut out_of_range = rows.clone();
    out_of_range[0]["ordinal"] = 2.into();
    out_of_range[0]["flags_guid_offset"] = 117.into();
    out_of_range[0]["flags_offset"] = 153.into();
    assert!(parse(out_of_range).unwrap_err().contains("ordinal"));
    for field in [
        "flags_guid_offset",
        "flags_offset",
        "filename_guid_offset",
        "filename_record_reference_offset",
    ] {
        let mut bad = rows.clone();
        bad[0][field] = (u64::MAX - 35).into();
        assert!(parse(bad).is_err());
    }
    assert!(crate::records::DesignMeshTextureTable::from_wire(
        record(218),
        21,
        113,
        serde_json::from_value(rows.clone()).unwrap()
    )
    .is_err());
    assert!(crate::records::DesignMeshTextureTable::from_wire(
        record(219),
        21,
        112,
        serde_json::from_value(rows).unwrap()
    )
    .is_err());
    assert!(crate::records::DesignMeshTextureTable::new(record(29), Vec::new()).is_ok());
}

#[test]
fn mesh_scope_constructs_only_same_index_closing_bases() {
    let identity = |index, offset, length| {
        crate::records::DesignMeshRecordIdentity::new(
            crate::records::DesignClassTag::try_from("256".to_owned()).expect("class tag"),
            index,
            offset,
            length,
        )
        .expect("record identity")
    };
    let scope =
        crate::records::DesignMeshScope::new(identity(104, 100, 200), identity(104, 270, 30), 109)
            .expect("scope");
    assert_eq!(scope.base_record(), identity(104, 270, 30));
    assert_eq!(scope.owner_reference_offset(), 289);
    assert_eq!(scope.owner_record_index(), 109);
    for base in [
        identity(105, 270, 30),
        identity(104, 269, 30),
        identity(104, 270, 31),
    ] {
        assert!(crate::records::DesignMeshScope::new(identity(104, 100, 200), base, 109).is_err());
    }
    assert!(crate::records::DesignMeshScope::new(
        identity(104, 100, 200),
        identity(104, 270, 30),
        0
    )
    .is_err());
    assert!(crate::records::DesignMeshScope::new(
        identity(104, 100, 54),
        identity(104, 124, 30),
        109
    )
    .is_err());
}

#[test]
fn mesh_collection_constructs_only_complete_nested_body_runs() {
    let identity = |index, offset, length| {
        crate::records::DesignMeshRecordIdentity::new(
            crate::records::DesignClassTag::try_from("256".to_owned()).expect("class tag"),
            index,
            offset,
            length,
        )
        .expect("record identity")
    };
    for count in [0, 1, 2, u64::from(u32::MAX)] {
        let length = 73 + 11 * count;
        let collection = crate::records::DesignMeshCollection::new(
            identity(104, 100, length),
            identity(104, 138, length - 38),
        )
        .expect("collection");
        assert_eq!(collection.base_record(), identity(104, 138, length - 38));
        assert_eq!(collection.body_count(), count);
        assert_eq!(collection.texture_table_reference_offset(), 127);
        assert_eq!(collection.owner_reference_offset(), 100 + length - 11);
    }
    for base in [
        identity(105, 138, 57),
        identity(104, 137, 57),
        identity(104, 138, 58),
    ] {
        assert!(crate::records::DesignMeshCollection::new(identity(104, 100, 95), base).is_err());
    }
    for length in [72, 74, 73 + 11 * (u64::from(u32::MAX) + 1)] {
        assert!(crate::records::DesignMeshCollection::new(
            identity(104, 100, length),
            identity(104, 138, length - 38)
        )
        .is_err());
    }
}

#[test]
fn canvas_prologue_reconstructs_both_flags_and_fixed_zero_bytes() {
    for first_flag in [0, 1] {
        for visible in [0, 1] {
            let mut bytes = [0; 15];
            bytes[10] = first_flag;
            bytes[14] = visible;
            let prologue =
                crate::records::DesignCanvasPrologue::try_from(bytes).expect("Canvas prologue");
            assert_eq!(prologue.bytes(), bytes);
            assert_eq!(prologue.visible(), visible != 0);
        }
    }
    for offset in 0..15 {
        let mut bytes = [0; 15];
        bytes[offset] = if matches!(offset, 10 | 14) { 2 } else { 1 };
        assert!(crate::records::DesignCanvasPrologue::try_from(bytes).is_err());
    }
}

#[test]
fn canvas_geometry_prologue_decodes_visibility_in_both_forms() {
    use crate::records::DesignCanvasPrologue;
    let mut expanded = [0; 15];
    expanded[14] = 1;
    assert!(DesignCanvasPrologue::try_from(expanded).is_ok());
    assert_eq!(
        DesignCanvasPrologue::try_from(expanded)
            .ok()
            .map(DesignCanvasPrologue::visible),
        Some(true)
    );

    expanded[14] = 0;
    assert_eq!(
        DesignCanvasPrologue::try_from(expanded)
            .ok()
            .map(DesignCanvasPrologue::visible),
        Some(false)
    );

    let mut compact = [0; 15];
    compact[10] = 1;
    assert!(DesignCanvasPrologue::try_from(compact).is_ok());
    assert_eq!(
        DesignCanvasPrologue::try_from(compact)
            .ok()
            .map(DesignCanvasPrologue::visible),
        Some(false)
    );

    compact[14] = 1;
    assert_eq!(
        DesignCanvasPrologue::try_from(compact)
            .ok()
            .map(DesignCanvasPrologue::visible),
        Some(true)
    );

    compact[11] = 1;
    assert!(DesignCanvasPrologue::try_from(compact).is_err());
}

#[test]
fn canvas_geometry_payload_preserves_source_float_bits() {
    let mut bytes = [0; 77];
    bytes[..4].copy_from_slice(&(-0.0_f32).to_le_bytes());
    for (offset, value) in [
        (5, -0.0_f64),
        (13, 2.5),
        (21, -3.5),
        (29, 1.0),
        (37, -0.0),
        (45, 0.0),
        (53, -0.0),
        (61, 0.0),
        (69, 1.0),
    ] {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let payload = crate::records::DesignCanvasGeometryPayload::try_from(bytes.as_slice())
        .expect("Canvas payload");
    assert_eq!(payload.bytes(), bytes);
    let (opacity, origin, u_axis, v_axis) = payload.decoded();
    assert_eq!(opacity.to_bits(), (-0.0_f32).to_bits());
    assert_eq!(origin.x.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(origin.y, 25.0);
    assert_eq!(origin.z, -35.0);
    assert_eq!(u_axis.y.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(v_axis.x.to_bits(), (-0.0_f64).to_bits());
    assert!(crate::records::DesignCanvasGeometryPayload::try_from(&bytes[..76]).is_err());
}

#[test]
fn canvas_image_wire_derives_visibility_and_geometry_values() {
    let mut payload = [0; 77];
    payload[..4].copy_from_slice(&0.75_f32.to_le_bytes());
    for (offset, value) in [
        (5, 1.0_f64),
        (13, 2.0),
        (21, 3.0),
        (29, 1.0),
        (37, 0.0),
        (45, 0.0),
        (53, 0.0),
        (61, 0.0),
        (69, 1.0),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut prologue = [0; 15];
    prologue[14] = 1;
    let base = serde_json::json!({
        "id": "canvas", "scope_record_index": 103, "scope_reference_offset": 247,
        "geometry_class_tag": "256", "geometry_record_index": 101,
        "geometry_reference_offset": 424, "geometry_byte_offset": 100,
        "geometry_prologue": prologue, "visible": true, "visibility_offset": 125,
        "geometry_frame_length": 229, "paired_geometry_class_tag": "257",
        "paired_geometry_byte_offset": 329, "paired_component_reference_offset": 349,
        "boundary_segments": [[{"u":-2.0,"v":-1.0},{"u":3.0,"v":-1.0}],[{"u":-2.0,"v":4.0},{"u":3.0,"v":4.0}]],
        "boundary_coordinate_offsets": [126,134,142,150,281,289,297,305],
        "second_boundary_present_offset": 280, "plane_entity_suffix": 200,
        "plane_reference_offset": 159, "component_entity_suffix": 201,
        "component_reference_offset": 258, "asset_class_tag": "258", "asset_record_index": 102,
        "asset_reference_offset": 270, "asset_byte_offset": 359, "asset_name": "image.png",
        "asset_name_offset": 384, "label": "Canvas", "label_offset": 317,
        "opacity": 0.75, "origin": {"x":10.0,"y":20.0,"z":30.0},
        "u_axis": {"x":1.0,"y":0.0,"z":0.0}, "v_axis": {"x":0.0,"y":0.0,"z":1.0},
        "geometry_payload": payload.as_slice()
    });
    for first_flag in [0, 1] {
        for visible in [false, true] {
            let mut value = base.clone();
            value["geometry_prologue"][10] = serde_json::json!(first_flag);
            value["geometry_prologue"][14] = serde_json::json!(u8::from(visible));
            value["visible"] = serde_json::json!(visible);
            let wire: crate::records::DesignCanvasImageWire =
                serde_json::from_value(value).expect("Canvas wire");
            let expected = serde_json::to_string(&wire).expect("Canvas wire bytes");
            let image: crate::records::DesignCanvasImage =
                serde_json::from_str(&expected).expect("Canvas image");
            assert_eq!(
                serde_json::to_string(&image).expect("Canvas image bytes"),
                expected
            );
        }
    }
    for geometry_reference_offset in [424, 428] {
        let mut value = base.clone();
        value["geometry_reference_offset"] = serde_json::json!(geometry_reference_offset);
        let wire: crate::records::DesignCanvasImageWire =
            serde_json::from_value(value).expect("Canvas scope form");
        let expected = serde_json::to_string(&wire).expect("Canvas scope wire");
        let image: crate::records::DesignCanvasImage =
            serde_json::from_str(&expected).expect("Canvas scope binding");
        assert_eq!(image.scope_byte_offset(), 402);
        assert_eq!(
            serde_json::to_string(&image).expect("Canvas scope bytes"),
            expected
        );
    }
    let mut unicode = base.clone();
    unicode["label"] = serde_json::json!("A😀");
    unicode["asset_name"] = serde_json::json!("图😀.png");
    for (field, value) in [
        ("geometry_frame_length", 223),
        ("paired_geometry_byte_offset", 323),
        ("paired_component_reference_offset", 343),
        ("asset_byte_offset", 353),
        ("asset_name_offset", 378),
        ("geometry_reference_offset", 414),
    ] {
        unicode[field] = serde_json::json!(value);
    }
    let wire: crate::records::DesignCanvasImageWire =
        serde_json::from_value(unicode).expect("Canvas Unicode wire");
    let expected = serde_json::to_string(&wire).expect("Canvas Unicode bytes");
    let image: crate::records::DesignCanvasImage =
        serde_json::from_str(&expected).expect("Canvas Unicode frame");
    assert_eq!(image.scope_byte_offset(), 392);
    assert_eq!(
        serde_json::to_string(&image).expect("Canvas Unicode output"),
        expected
    );
    for field in [
        "scope_reference_offset",
        "visibility_offset",
        "geometry_frame_length",
        "paired_geometry_byte_offset",
        "paired_component_reference_offset",
        "second_boundary_present_offset",
        "plane_reference_offset",
        "component_reference_offset",
        "asset_reference_offset",
        "asset_byte_offset",
        "asset_name_offset",
        "label_offset",
        "geometry_reference_offset",
    ] {
        let mut value = base.clone();
        value[field] = serde_json::json!(0);
        let error = serde_json::from_value::<crate::records::DesignCanvasImage>(value)
            .expect_err("misplaced Canvas field")
            .to_string();
        assert!(error.contains(field));
    }
    for field in [
        "geometry_class_tag",
        "paired_geometry_class_tag",
        "asset_class_tag",
        "label",
        "asset_name",
    ] {
        let mut value = base.clone();
        value[field] = serde_json::json!("");
        let error = serde_json::from_value::<crate::records::DesignCanvasImage>(value)
            .expect_err("empty Canvas field")
            .to_string();
        assert!(error.contains(field));
    }
    let mut repeated_record = base.clone();
    repeated_record["asset_record_index"] = serde_json::json!(101);
    let error = serde_json::from_value::<crate::records::DesignCanvasImage>(repeated_record)
        .expect_err("repeated Canvas record identity")
        .to_string();
    assert!(error.contains("asset_record_index"));
    let mut overflow = base.clone();
    overflow["geometry_byte_offset"] = serde_json::json!(u64::MAX);
    let error = serde_json::from_value::<crate::records::DesignCanvasImage>(overflow)
        .expect_err("Canvas extent overflow")
        .to_string();
    assert!(error.contains("geometry_byte_offset"));
    let mut boundary_offset = base.clone();
    boundary_offset["boundary_coordinate_offsets"][0] = serde_json::json!(0);
    let error = serde_json::from_value::<crate::records::DesignCanvasImage>(boundary_offset)
        .expect_err("Canvas boundary offset")
        .to_string();
    assert!(error.contains("boundary_coordinate_offsets"));
    for (field, replacement) in [
        ("visible", serde_json::json!(false)),
        ("opacity", serde_json::json!(0.5)),
        ("origin", serde_json::json!({"x":11.0,"y":20.0,"z":30.0})),
        ("u_axis", serde_json::json!({"x":1.0,"y":1.0,"z":0.0})),
        ("v_axis", serde_json::json!({"x":0.0,"y":0.0,"z":0.0})),
        (
            "boundary_segments",
            serde_json::json!([[{"u":-2.0,"v":-1.0},{"u":3.0,"v":-1.0}],[{"u":-2.0,"v":4.0},{"u":2.0,"v":4.0}]]),
        ),
    ] {
        let mut value = base.clone();
        value[field] = replacement;
        let error = serde_json::from_value::<crate::records::DesignCanvasImage>(value)
            .expect_err("inconsistent decoded Canvas value")
            .to_string();
        assert!(error.contains(field));
    }
}

#[test]
fn canvas_geometry_payload_decodes_opacity_and_plane_frame() {
    use crate::records::DesignCanvasGeometryPayload;
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut payload = [0; 77];
    payload[..4].copy_from_slice(&0.75f32.to_le_bytes());
    for (offset, value) in [
        (5, 1.0f64),
        (13, 2.0),
        (21, 3.0),
        (29, 1.0),
        (37, 0.0),
        (45, 0.0),
        (53, 0.0),
        (61, 0.0),
        (69, 1.0),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        DesignCanvasGeometryPayload::try_from(payload.as_slice())
            .ok()
            .map(|payload| payload.decoded()),
        Some((
            0.75,
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );

    payload[4] = 1;
    assert!(DesignCanvasGeometryPayload::try_from(payload.as_slice()).is_err());
    payload[4] = 0;
    payload[53..61].copy_from_slice(&1.0f64.to_le_bytes());
    assert!(DesignCanvasGeometryPayload::try_from(payload.as_slice()).is_err());
}

#[test]
fn canvas_bounds_preserve_segment_order_and_derive_extents() {
    use cadmpeg_ir::math::Point2;
    for (segments, mirroring) in [
        (
            [
                [Point2::new(3.0, 4.0), Point2::new(-2.0, 4.0)],
                [Point2::new(3.0, -1.0), Point2::new(-2.0, -1.0)],
            ],
            (true, true),
        ),
        (
            [
                [Point2::new(3.0, -1.0), Point2::new(3.0, 4.0)],
                [Point2::new(-2.0, -1.0), Point2::new(-2.0, 4.0)],
            ],
            (true, false),
        ),
    ] {
        let bounds = crate::records::DesignCanvasBounds::try_from(segments).expect("Canvas bounds");
        assert_eq!(bounds.segments(), segments);
        assert_eq!(bounds.mirroring(), mirroring);
        assert_eq!(
            bounds.extents(),
            [Point2::new(-2.0, -1.0), Point2::new(3.0, 4.0)]
        );
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let segments = [
            [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
            [Point2::new(-2.0, 4.0), Point2::new(invalid, 4.0)],
        ];
        assert!(crate::records::DesignCanvasBounds::try_from(segments).is_err());
    }
}

#[test]
fn canvas_bounds_decode_u_and_v_mirroring_from_endpoint_order() {
    use crate::records::DesignCanvasBounds;
    use cadmpeg_ir::math::Point2;
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
            [Point2::new(-2.0, 4.0), Point2::new(3.0, 4.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((false, false))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(3.0, -1.0), Point2::new(-2.0, -1.0)],
            [Point2::new(3.0, 4.0), Point2::new(-2.0, 4.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((true, false))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(-2.0, 4.0), Point2::new(3.0, 4.0)],
            [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((false, true))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(3.0, 4.0), Point2::new(-2.0, 4.0)],
            [Point2::new(3.0, -1.0), Point2::new(-2.0, -1.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((true, true))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(-2.0, 4.0), Point2::new(-2.0, -1.0)],
            [Point2::new(3.0, 4.0), Point2::new(3.0, -1.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((false, true))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(3.0, -1.0), Point2::new(3.0, 4.0)],
            [Point2::new(-2.0, -1.0), Point2::new(-2.0, 4.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((true, false))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
            [
                Point2::new(f64::from_bits((-2.0f64).to_bits() + 4), 4.0),
                Point2::new(f64::from_bits(3.0f64.to_bits() + 4), 4.0),
            ],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((false, false))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
            [Point2::new(-2.0, 4.0), Point2::new(2.0, 4.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        None
    );
}

#[test]
fn decal_mapping_modes_preserve_all_bytes_with_canonical_known_mode() {
    use crate::records::{DesignDecalMappingMode, UnrecognizedDecalMappingMode};
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

#[test]
fn decal_image_wire_derives_consecutive_records_and_scope_offsets() {
    let base = serde_json::json!({
        "id": "decal", "scope_record_index": 23, "asset_reference_offset": 222,
        "mapping_mode": 96, "mapping_mode_offset": 232, "target_group_record_index": 24,
        "target_group_reference_offset": 234, "asset_class_tag": "258", "asset_record_index": 17,
        "asset_byte_offset": 100, "asset_frame_length": 30, "asset_entity_suffix": 50,
        "asset_entity_reference_offset": 120, "name_class_tag": "279", "name_record_index": 18,
        "name_byte_offset": 130, "name_frame_length": 41, "asset_name": "mark.png", "asset_name_offset": 155
    });
    for mode in [0, 0x60, 0x61, 0xff] {
        let mut value = base.clone();
        value["mapping_mode"] = serde_json::json!(mode);
        let wire: crate::records::DesignDecalImageWire =
            serde_json::from_value(value).expect("Decal wire");
        let expected = serde_json::to_string(&wire).expect("Decal wire bytes");
        let image: crate::records::DesignDecalImage =
            serde_json::from_str(&expected).expect("Decal binding");
        assert_eq!(image.scope_byte_offset(), 200);
        assert_eq!(
            serde_json::to_string(&image).expect("Decal binding bytes"),
            expected
        );
    }
    let mut unicode = base.clone();
    unicode["asset_name"] = serde_json::json!("图😀.png");
    unicode["name_frame_length"] = serde_json::json!(39);
    unicode["asset_record_index"] = serde_json::json!(u32::MAX - 1);
    unicode["name_record_index"] = serde_json::json!(u32::MAX);
    let wire: crate::records::DesignDecalImageWire =
        serde_json::from_value(unicode).expect("Decal Unicode wire");
    let expected = serde_json::to_string(&wire).expect("Decal Unicode bytes");
    let image: crate::records::DesignDecalImage =
        serde_json::from_str(&expected).expect("Decal Unicode name");
    assert_eq!(
        serde_json::to_string(&image).expect("Decal Unicode output"),
        expected
    );
    for field in [
        "asset_reference_offset",
        "mapping_mode_offset",
        "target_group_reference_offset",
        "asset_frame_length",
        "asset_entity_reference_offset",
        "name_record_index",
        "name_byte_offset",
        "name_frame_length",
        "asset_name_offset",
    ] {
        let mut value = base.clone();
        value[field] = serde_json::json!(0);
        let error = serde_json::from_value::<crate::records::DesignDecalImage>(value)
            .expect_err("invalid Decal field")
            .to_string();
        assert!(error.contains(field));
    }
    for field in ["asset_class_tag", "name_class_tag", "asset_name"] {
        let mut value = base.clone();
        value[field] = serde_json::json!("");
        let error = serde_json::from_value::<crate::records::DesignDecalImage>(value)
            .expect_err("empty Decal field")
            .to_string();
        assert!(error.contains(field));
    }
    for field in ["asset_reference_offset", "asset_byte_offset"] {
        let mut value = base.clone();
        value[field] = serde_json::json!(u64::MAX);
        let error = serde_json::from_value::<crate::records::DesignDecalImage>(value)
            .expect_err("Decal byte extent overflow")
            .to_string();
        assert!(error.contains(field));
    }
    let mut no_successor = base;
    no_successor["asset_record_index"] = serde_json::json!(u32::MAX);
    no_successor["name_record_index"] = serde_json::json!(u32::MAX);
    let error = serde_json::from_value::<crate::records::DesignDecalImage>(no_successor)
        .expect_err("Decal record index overflow")
        .to_string();
    assert!(error.contains("name_record_index"));
}

#[test]
fn relaxed_guid_text_accepts_relaxed_only_value() {
    let wire = "\"GAAAAAAA_BBBB-4CCC-8DDD-EEEEEEEEEEEE\"";
    let value: crate::records::DesignRelaxedGuidText = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&value).unwrap(), wire);
}

#[test]
fn strict_guid_text_rejects_relaxed_only_value() {
    assert!(serde_json::from_str::<crate::records::DesignGuidText>(
        "\"GAAAAAAA_BBBB-4CCC-8DDD-EEEEEEEEEEEE\""
    )
    .is_err());
}

#[test]
fn affine_placement_preserves_shear_and_rejects_invalid_wire() {
    use crate::records::DesignAffineTransform;
    let mut rows = cadmpeg_ir::transform::Transform::identity().rows();
    rows[0][1] = 2.0;
    rows[2][2] = 0.0;
    let placement = DesignAffineTransform::try_from(rows).unwrap();
    let wire = serde_json::to_value(rows).unwrap();
    assert_eq!(serde_json::to_value(placement).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<DesignAffineTransform>(wire).unwrap(),
        placement
    );
    rows[3][0] = 1.0;
    assert!(DesignAffineTransform::try_from(rows).is_err());
    assert!(
        serde_json::from_value::<DesignAffineTransform>(serde_json::to_value(rows).unwrap())
            .unwrap_err()
            .to_string()
            .contains("transform")
    );
    rows[3][0] = 0.0;
    rows[0][0] = f64::INFINITY;
    assert!(DesignAffineTransform::try_from(rows).is_err());
}
