// SPDX-License-Identifier: Apache-2.0

#[test]
// Fixture fields are appended from the bounded table of explicit test cases.
#[allow(clippy::format_push_string)]
fn external_version_identity_preserves_wire_and_rejects_partial_forms() {
    {
        let prefix = r#"{"axis_record_index":0,"axis_class_tag":"327","axis_byte_offset":0,"axis_paired_class_tag":"327","axis_paired_byte_offset":0,"selector_record_index":0,"selector_class_tag":"327","selector_byte_offset":0,"selector_paired_class_tag":"327","selector_paired_byte_offset":0,"nested_record_index":0,"nested_record_index_offset":0,"selector_asset_id":"00000004-1111-4111-8111-111111111111","selector_asset_id_offset":0,"selector_context_id":"00000005-1111-4111-8111-111111111111","selector_context_id_offset":0,"occurrence_reference":0,"occurrence_reference_offset":0,"external_object_reference":0,"external_object_reference_offset":0,"external_segment":0,"external_segment_offset":0,"external_asset_id":"00000006-1111-4111-8111-111111111111","external_asset_id_offset":0,"external_link_name":"identity","external_link_name_offset":0"#;
        let suffix = r#","role_record_index":0,"role_class_tag":"327","role_byte_offset":0,"occurrence_role":"00000007-1111-4111-8111-111111111111","occurrence_role_offset":0}"#;
        let fields = [
            (
                "external_property_key",
                "\"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\"",
            ),
            ("external_property_key_offset", "100"),
            ("external_version_urn", "\"urn\""),
            ("external_version_urn_offset", "110"),
        ];
        for mask in 0..16 {
            let mut wire = prefix.to_owned();
            for (index, (field, value)) in fields.iter().enumerate() {
                if mask & (1 << index) != 0 {
                    wire.push_str(&format!(",\"{field}\":{value}"));
                }
            }
            wire.push_str(suffix);
            let result = serde_json::from_str::<
                crate::records::feature::DesignAssemblyAxialSelectorIdentity,
            >(&wire);
            if mask == 0 || mask == 15 {
                let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
                assert_relaxed_guid_fields::<
                    crate::records::feature::DesignAssemblyAxialSelectorIdentity,
                >(
                    &value,
                    &[
                        "selector_asset_id",
                        "selector_context_id",
                        "external_asset_id",
                        "occurrence_role",
                    ],
                );
                if mask == 15 {
                    assert_relaxed_guid_fields::<
                        crate::records::feature::DesignAssemblyAxialSelectorIdentity,
                    >(&value, &["external_property_key"]);
                }
                assert_eq!(
                    serde_json::to_string(&result.expect("complete version form"))
                        .expect("version wire"),
                    wire
                );
            } else {
                let error = result.expect_err("partial version identity").to_string();
                for (field, _) in fields {
                    assert!(error.contains(field));
                }
            }
        }
    }
    {
        let prefix = r#"{"selector_asset_id":"00000004-1111-4111-8111-111111111111","selector_asset_id_offset":0,"selector_context_id":"00000005-1111-4111-8111-111111111111","selector_context_id_offset":0,"occurrence_reference":0,"occurrence_reference_offset":0,"external_body_reference":0,"external_body_reference_offset":0,"external_segment":0,"external_segment_offset":0,"external_asset_id":"00000006-1111-4111-8111-111111111111","external_asset_id_offset":0,"external_link_name":"identity","external_link_name_offset":0"#;
        let suffix = r#","tail_values":[0,0],"tail_value_offsets":[0,0]}"#;
        let fields = [
            (
                "external_property_key",
                "\"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\"",
            ),
            ("external_property_key_offset", "100"),
            ("external_version_urn", "\"urn\""),
            ("external_version_urn_offset", "110"),
        ];
        for mask in 0..16 {
            let mut wire = prefix.to_owned();
            for (index, (field, value)) in fields.iter().enumerate() {
                if mask & (1 << index) != 0 {
                    wire.push_str(&format!(",\"{field}\":{value}"));
                }
            }
            wire.push_str(suffix);
            let result = serde_json::from_str::<
                crate::records::feature::DesignCombineExternalBodyIdentity,
            >(&wire);
            if mask == 0 || mask == 15 {
                let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
                assert_relaxed_guid_fields::<
                    crate::records::feature::DesignCombineExternalBodyIdentity,
                >(
                    &value,
                    &[
                        "selector_asset_id",
                        "selector_context_id",
                        "external_asset_id",
                    ],
                );
                if mask == 15 {
                    assert_relaxed_guid_fields::<
                        crate::records::feature::DesignCombineExternalBodyIdentity,
                    >(&value, &["external_property_key"]);
                }
                assert_eq!(
                    serde_json::to_string(&result.expect("complete version form"))
                        .expect("version wire"),
                    wire
                );
            } else {
                let error = result.expect_err("partial version identity").to_string();
                for (field, _) in fields {
                    assert!(error.contains(field));
                }
            }
        }
    }
}

#[test]
fn component_placement_preserves_wire_and_rejects_partial_location() {
    let base = serde_json::json!({
        "id": "occurrence", "class_tag": "327", "record_index": 7, "byte_offset": 0,
        "component_record_index": 8, "component_guid": "00000001-1111-4111-8111-111111111111", "component_guid_offset": 48,
        "occurrence_guid": "00000002-1111-4111-8111-111111111111", "occurrence_guid_offset": 124, "occurrence_ordinal": 1
    });
    assert_relaxed_guid_fields::<crate::records::feature::DesignComponentOccurrence>(
        &base,
        &["component_guid", "occurrence_guid"],
    );
    let transform = serde_json::json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ]);
    for placed in [false, true] {
        let mut wire = base.clone();
        if placed {
            wire["transform"] = transform.clone();
            wire["transform_offset"] = serde_json::json!(209);
        }
        let record: crate::records::feature::DesignComponentOccurrence =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }
    for (field, value) in [
        ("transform", transform),
        ("transform_offset", serde_json::json!(209)),
    ] {
        let mut wire = base.clone();
        wire[field] = value;
        assert!(
            serde_json::from_value::<crate::records::feature::DesignComponentOccurrence>(wire)
                .unwrap_err()
                .to_string()
                .contains("transform")
        );
    }
}

#[test]
fn assembly_forms_preserve_partial_and_mixed_qualifier_wire() {
    let frame = crate::records::feature::DesignAssemblyOperandFrame {
        reference_record_index: 10,
        reference_offset: 11,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap(),
        transform_offset: 22,
    };
    let path = crate::records::feature::DesignAssemblyOperandPath {
        link: crate::records::feature::DesignAssemblyOperandPathLink {
            locator_reference_offset: 11,
            locator_record_index: 10,
            locator_class_tag: crate::records::DesignClassTag::try_from("363".to_owned()).unwrap(),
            locator_byte_offset: 100,
            locator_scope_reference_offset: 111,
            wrapper_record_index: 20,
            wrapper_reference_offset: 122,
            wrapper_class_tag: crate::records::DesignClassTag::try_from("388".to_owned()).unwrap(),
            wrapper_byte_offset: 200,
            path_reference_offset: 211,
        },
        record_index: 30,
        class_tag: crate::records::DesignClassTag::try_from("386".to_owned()).unwrap(),
        byte_offset: 300,
        occurrence_guids: vec![crate::records::Located {
            value: "11111111-1111-4111-8111-111111111111"
                .to_owned()
                .try_into()
                .expect("GUID"),
            offset: 311,
        }],
        identity_guids: vec![crate::records::Located {
            value: "22222222-2222-4222-8222-222222222222"
                .to_owned()
                .try_into()
                .expect("GUID"),
            offset: 322,
        }],
    };
    let limits = crate::records::feature::DesignAssemblyLimits {
        kind: crate::records::feature::DesignAssemblyLimitKind::Angular,
        minimum: -1.0,
        maximum: 1.0,
        owner_record_indices: [40, 50],
        value_offsets: [411, 511],
    };
    let joint_origin = crate::records::feature::DesignAssemblyOperandQualifier::JointOrigin {
        scope_record_index: 60,
        class_tag: crate::records::DesignClassTag::try_from("307".to_owned()).unwrap(),
        byte_offset: 600,
        paired_class_tag: crate::records::DesignClassTag::try_from("264".to_owned()).unwrap(),
        paired_byte_offset: 700,
    };
    let axial = crate::records::feature::DesignAssemblyOperandQualifier::AxialTarget {
        target:
            crate::records::feature::DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                scope_record_index: 60,
            },
    };
    let occurrence = crate::records::feature::DesignAssemblyOperandQualifier::OccurrencePath {
        path: path.clone(),
    };
    for form in [
        None,
        Some(
            crate::records::feature::DesignAssemblyAlignmentForm::DatumEnvelope {
                joint_origin_scope_record_index: 60,
            },
        ),
        Some(
            crate::records::feature::DesignAssemblyAlignmentForm::SolvedOnly {
                solved_frame: crate::records::feature::DesignAssemblySolvedFrame {
                    reference_record_index: 30,
                    reference_offset: 33,
                    record_byte_offset: 300,
                    class_tag: crate::records::DesignClassTag::try_from("258".to_owned()).unwrap(),
                    transform: frame.transform,
                    transform_offset: 325,
                },
                limits: Some(limits.clone()),
            },
        ),
        Some(crate::records::feature::DesignAssemblyAlignmentForm::LimitsOnly { limits }),
        Some(
            crate::records::feature::DesignAssemblyAlignmentForm::Frames {
                frames: [frame.clone(), frame.clone()],
            },
        ),
        Some(
            crate::records::feature::DesignAssemblyAlignmentForm::UnframedPaths([
                path.clone(),
                path,
            ]),
        ),
        Some(
            crate::records::feature::DesignAssemblyAlignmentForm::qualified(
                [frame.clone(), frame.clone()],
                [occurrence.clone(), occurrence.clone()],
            ),
        ),
        Some(
            crate::records::feature::DesignAssemblyAlignmentForm::qualified(
                [frame.clone(), frame.clone()],
                [occurrence, joint_origin],
            ),
        ),
        Some(
            crate::records::feature::DesignAssemblyAlignmentForm::qualified(
                [frame.clone(), frame.clone()],
                [axial.clone(), axial],
            ),
        ),
    ] {
        let alignment = crate::records::feature::DesignAssemblyAlignment {
            angle: 0.0,
            offset: [0.0; 3],
            owners: vec![
                crate::records::Located {
                    value: 10,
                    offset: 11,
                },
                crate::records::Located {
                    value: 20,
                    offset: 22,
                },
            ],
            form,
        };
        let wire = serde_json::to_string(&alignment).unwrap();
        let decoded: crate::records::feature::DesignAssemblyAlignment =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(decoded, alignment);
        assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
        let mut invalid = serde_json::from_str::<serde_json::Value>(&wire).unwrap();
        invalid["value_offsets"] = serde_json::json!([11]);
        let error =
            serde_json::from_value::<crate::records::feature::DesignAssemblyAlignment>(invalid)
                .unwrap_err()
                .to_string();
        assert!(error.contains("owner_record_indices"));
        assert!(error.contains("value_offsets"));
    }
}

#[test]
fn legacy_assembly_wire_derives_carrier_frames_and_checks_repeated_fields() {
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let selection = |record_index| crate::records::feature::DesignAssemblyLegacySelection {
        record_index,
        byte_offset: 400,
        class_tag: crate::records::DesignClassTag::try_from("307".to_owned()).unwrap(),
        asset_id: "11111111-1111-4111-8111-111111111111"
            .to_owned()
            .try_into()
            .unwrap(),
        asset_id_offset: 411,
        context_id: "22222222-2222-4222-8222-222222222222"
            .to_owned()
            .try_into()
            .unwrap(),
        context_id_offset: 422,
        recipe_record_index: 50,
        recipe_record_byte_offset: 500,
        recipe_id: "recipe".into(),
        recipe_kind: crate::records::ConstructionRecipeKind::Face,
        recipe_references: Vec::new(),
        next_byte_offset: 600,
    };
    let carriers = crate::records::feature::DesignAssemblyLegacyOperands::try_new(
        crate::records::feature::DesignAssemblyLegacyOperand {
            construction_class_tag: crate::records::DesignClassTag::try_from("256".to_owned())
                .unwrap(),
            reference_offset: 11,
            construction: Box::new(crate::records::feature::DesignWorkPointConstruction {
                point_record_index: 10,
                point_record_byte_offset: 100,
                position: [1.0, 2.0, 3.0],
                position_offset: 125,
                rule: crate::records::feature::DesignWorkPointRule::try_from(
                    crate::records::feature::DesignWorkPointRuleForm::Native {
                        reference_type: 0,
                        inputs: Vec::new(),
                    },
                )
                .expect("compatible WorkPoint rule"),
                reference_type_offset: 150,
            }),
            selection: selection(40),
        },
        crate::records::feature::DesignAssemblyLegacyOperand {
            construction_class_tag: crate::records::DesignClassTag::try_from("257".to_owned())
                .unwrap(),
            reference_offset: 22,
            construction: Box::new(crate::records::feature::DesignHoleConstruction {
                point_record_index: 20,
                point_record_byte_offset: 200,
                position: [4.0, 5.0, 6.0],
                position_offset: 225,
                direction: [0.0, 0.0, 1.0],
                direction_offset: 250,
                point_parameters: [0.0, 0.0],
                point_parameter_offsets: [275, 283],
                reference_type: 0,
                reference_type_offset: 291,
                tangent_point_data: None,
                input_records: Vec::new(),
                face_selection: None,
            }),
            selection: selection(41),
        },
    )
    .unwrap();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut point = carriers.point.clone();
        point.construction.position[0] = value;
        assert!(
            crate::records::feature::DesignAssemblyLegacyOperands::try_new(
                point,
                carriers.hole.clone()
            )
            .is_err()
        );
        let mut hole = carriers.hole.clone();
        hole.construction.position[2] = value;
        assert!(
            crate::records::feature::DesignAssemblyLegacyOperands::try_new(
                carriers.point.clone(),
                hole
            )
            .is_err()
        );
    }
    let solved_frame = crate::records::feature::DesignAssemblySolvedFrame {
        reference_record_index: 30,
        reference_offset: 33,
        record_byte_offset: 300,
        class_tag: crate::records::DesignClassTag::try_from("258".to_owned()).unwrap(),
        transform: identity.try_into().unwrap(),
        transform_offset: 325,
    };
    for frames_field_present in [false, true] {
        let alignment = crate::records::feature::DesignAssemblyAlignment {
            angle: 0.0,
            offset: [0.0; 3],
            owners: Vec::new(),
            form: Some(
                crate::records::feature::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                    carriers: carriers.clone(),
                    solved_frame: solved_frame.clone(),
                    limits: None,
                    frames_field_present,
                },
            ),
        };
        let frames = alignment.operand_frames().unwrap();
        assert_eq!(frames[0].reference_record_index, 10);
        assert_eq!(frames[1].reference_record_index, 20);
        assert_eq!(frames[0].transform_offset, 325);
        assert_eq!(
            frames[0].transform,
            [
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 2.0],
                [0.0, 0.0, 1.0, 3.0],
                [0.0, 0.0, 0.0, 1.0]
            ]
            .try_into()
            .unwrap()
        );
        assert_eq!(frames[1].transform[2][3], 6.0);
        let wire = serde_json::to_string(&alignment).unwrap();
        let decoded: crate::records::feature::DesignAssemblyAlignment =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(decoded, alignment);
        assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
        let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
        assert_eq!(value.get("operand_frames").is_some(), frames_field_present);
        for (field, replacement) in [
            ("construction_record_index", serde_json::json!(99)),
            ("construction_byte_offset", serde_json::json!(99)),
            ("frame", serde_json::to_value(&frames[1]).unwrap()),
        ] {
            let mut invalid = value.clone();
            invalid["legacy_operand_carriers"][0][field] = replacement;
            let error =
                serde_json::from_value::<crate::records::feature::DesignAssemblyAlignment>(invalid)
                    .unwrap_err()
                    .to_string();
            assert!(error.contains(field));
        }
        let mut invalid = value.clone();
        invalid["legacy_operand_carriers"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        assert!(
            serde_json::from_value::<crate::records::feature::DesignAssemblyAlignment>(invalid)
                .unwrap_err()
                .to_string()
                .contains("construction")
        );
        if frames_field_present {
            let mut invalid = value;
            invalid["operand_frames"][0]["transform"][0][3] = serde_json::json!(99.0);
            assert!(
                serde_json::from_value::<crate::records::feature::DesignAssemblyAlignment>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("operand_frames")
            );
        }
    }
}

#[test]
fn assembly_path_wire_pairs_guid_locations() {
    for count in [0, 1, 3] {
        let values: Vec<_> = (0..count)
            .map(|index| format!("{index:08}-1111-4111-8111-111111111111"))
            .collect();
        let offsets: Vec<_> = (0..count).map(|index| 300 + index * 80).collect();
        let wire = serde_json::json!({
            "link": {
                "locator_reference_offset": 11, "locator_record_index": 10,
                "locator_class_tag": "363", "locator_byte_offset": 100,
                "locator_scope_reference_offset": 111, "wrapper_record_index": 20,
                "wrapper_reference_offset": 122, "wrapper_class_tag": "388",
                "wrapper_byte_offset": 200, "path_reference_offset": 211
            },
            "record_index": 30, "class_tag": "386", "byte_offset": 300,
            "occurrence_guids": values, "occurrence_guid_offsets": offsets
        });
        for identities in [false, true] {
            let mut wire = wire.clone();
            if identities && count != 0 {
                wire["identity_guids"] = wire["occurrence_guids"].clone();
                wire["identity_guid_offsets"] = wire["occurrence_guid_offsets"].clone();
            }
            let path: crate::records::feature::DesignAssemblyOperandPath =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(path.occurrence_guids.len(), count);
            assert_eq!(serde_json::to_value(&path).unwrap(), wire);
            for (value_field, offset_field) in [
                ("occurrence_guids", "occurrence_guid_offsets"),
                ("identity_guids", "identity_guid_offsets"),
            ] {
                let mut invalid = wire.clone();
                let mut bad_offsets = invalid
                    .get(offset_field)
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                bad_offsets.push(serde_json::json!(999));
                invalid[offset_field] = serde_json::Value::Array(bad_offsets);
                let error = serde_json::from_value::<
                    crate::records::feature::DesignAssemblyOperandPath,
                >(invalid)
                .unwrap_err()
                .to_string();
                assert!(error.contains(value_field));
                assert!(error.contains(offset_field));
            }
        }
    }
}

#[test]
fn component_insert_pairs_explicit_matrix_with_scope_and_carrier_locations() {
    let prefix = r#"{"relation_record_index":1,"carrier_record_index":2,"neutron_role":"role","neutron_role_offset":30,"transform":"#;
    let identity = "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]";
    let translated = "[[1.0,0.0,0.0,2.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]";
    for (matrix, offsets) in [
        (identity, ""),
        (identity, ",\"transform_offset\":50"),
        (translated, ",\"transform_offset\":50"),
        (
            translated,
            ",\"transform_offset\":50,\"carrier_transform_offset\":40",
        ),
    ] {
        let wire = format!("{prefix}{matrix}{offsets}}}");
        let construction: crate::records::feature::DesignComponentInsertConstruction =
            serde_json::from_str(&wire).expect("component placement");
        assert_eq!(
            serde_json::to_string(&construction).expect("component placement wire"),
            wire
        );
        assert_eq!(construction.placement.is_some(), !offsets.is_empty());
    }
    for (matrix, offsets) in [
        (translated, ""),
        (identity, ",\"carrier_transform_offset\":40"),
    ] {
        let wire = format!("{prefix}{matrix}{offsets}}}");
        let error = serde_json::from_str::<
            crate::records::feature::DesignComponentInsertConstruction,
        >(&wire)
        .expect_err("missing scope matrix location");
        assert!(error.to_string().contains("transform_offset"));
    }
}

#[test]
fn component_occurrence_derives_base_ordinal_and_requires_nonzero_placed_ordinal() {
    let prefix = r#"{"id":"occurrence","class_tag":"327","record_index":7,"byte_offset":0,"component_record_index":8,"component_guid":"00000001-1111-4111-8111-111111111111","component_guid_offset":48,"occurrence_guid":"00000002-1111-4111-8111-111111111111","occurrence_guid_offset":124,"occurrence_ordinal":"#;
    let matrix = r#","transform":[[1.0,0.0,0.0,2.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],"transform_offset":209"#;
    for (ordinal, placed) in [(1, false), (1, true), (2, true), (u32::MAX, true)] {
        let suffix = if placed { matrix } else { "" };
        let wire = format!("{prefix}{ordinal}{suffix}}}");
        let occurrence: crate::records::feature::DesignComponentOccurrence =
            serde_json::from_str(&wire).expect("component occurrence");
        assert_eq!(
            serde_json::to_string(&occurrence).expect("component occurrence wire"),
            wire
        );
        assert_eq!(occurrence.occurrence_ordinal(), ordinal);
    }
    for (ordinal, placed) in [(0, false), (0, true), (2, false), (u32::MAX, false)] {
        let suffix = if placed { matrix } else { "" };
        let wire = format!("{prefix}{ordinal}{suffix}}}");
        let error =
            serde_json::from_str::<crate::records::feature::DesignComponentOccurrence>(&wire)
                .expect_err("invalid occurrence ordinal");
        assert!(error.to_string().contains("occurrence_ordinal"));
    }
}

fn assert_relaxed_guid_fields<T: serde::de::DeserializeOwned + serde::Serialize>(
    wire: &serde_json::Value,
    fields: &[&str],
) {
    for field in fields {
        for guid in ["g".repeat(36), "_".repeat(38), "invalid".into()] {
            let mut changed = wire.clone();
            changed[field] = serde_json::json!(guid);
            let decoded = serde_json::from_value::<T>(changed.clone());
            if guid == "invalid" {
                assert!(decoded.is_err(), "{field}");
            } else {
                let decoded = decoded.unwrap_or_else(|error| panic!("{field}: {error}"));
                assert_eq!(serde_json::to_value(decoded).unwrap(), changed);
            }
        }
    }
}
