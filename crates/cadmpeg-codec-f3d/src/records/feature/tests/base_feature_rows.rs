// SPDX-License-Identifier: Apache-2.0
//! Base Feature body rows preserve their wire and their capture pointers.

#[test]
fn coil_values_preserve_optional_locations_and_reject_orphan_offsets() {
    type Coil = crate::records::feature::coil::DesignCoilScope;
    for (field, value, stored_flag) in [
        ("coil_operation", "\"cut\"", false),
        ("coil_extent", "\"spiral\"", true),
        ("coil_section", "\"circular\"", true),
        ("coil_section_placement", "\"inside\"", true),
        ("coil_clockwise", "false", true),
    ] {
        let mut wires = vec!["{}".to_owned(), format!("{{\"{field}\":{value}}}")];
        if stored_flag {
            wires.push(format!("{{\"{field}\":{value},\"{field}_stored\":true}}"));
        }
        for wire in &wires {
            let parsed: Coil = serde_json::from_str(wire).expect("valid Coil field");
            assert_eq!(&serde_json::to_string(&parsed).expect("Coil wire"), wire);
        }
        let stored = wires.last().expect("stored wire");
        let pointer = format!("{field}_offset");
        for offset in [0, 30] {
            crate::records::feature::test_support::assert_located_at(
                serde_json::from_str::<Coil>(stored).expect("stored Coil field"),
                &[(pointer.as_str(), offset)],
            );
        }
        if stored_flag {
            let fixed: Coil = serde_json::from_str(&wires[1]).expect("dialect-fixed Coil field");
            crate::records::feature::test_support::assert_located_pointers(&fixed, &[]);
            let error = serde_json::from_str::<Coil>(&format!("{{\"{field}_stored\":true}}"))
                .expect_err("stored flag without value")
                .to_string();
            assert!(error.contains(field), "{error}");
        }
    }
}
#[test]
// Fixture fields are appended from the bounded table of explicit test cases.
#[allow(clippy::format_push_string)]
fn legacy_base_feature_form_owns_its_compact_mode() {
    for form in ["compact_one_body", "expanded_two_body"] {
        let (suffixes, fields, refs, parameters, auxiliary, body_offsets) =
            if form == "compact_one_body" {
                (
                    "[201]",
                    "[\"000000000000\"]",
                    "[201]",
                    "[301]",
                    "[303]",
                    &[(22, 40, 50)][..],
                )
            } else {
                (
                    "[401,402]",
                    "[\"000000000000\",\"000000000000\"]",
                    "[401,402]",
                    "[301,302]",
                    "[303,304]",
                    &[(22, 50, 70), (36, 60, 80)][..],
                )
            };
        for mask in 0..2 {
            let mut wire = format!("{{\"form\":\"{form}\"");
            if mask & 1 != 0 {
                wire.push_str(",\"mode\":0");
            }
            wire.push_str(&format!(r#","body_entity_suffixes":{suffixes},"body_entity_fields":{fields},"body_reference_records":{refs},"parameter_body_records":{parameters},"auxiliary_records":{auxiliary},"scope_reference":90,"envelope_guid":"11111111-2222-3333-4444-555555555555","tag_body_based_on_faces":true}}"#));
            let parsed = serde_json::from_str::<
                crate::records::feature::base_feature::DesignBaseFeatureConstruction,
            >(&wire);
            if (form == "compact_one_body" && mask == 1)
                || (form == "expanded_two_body" && mask == 0)
            {
                let parsed = parsed.expect("complete legacy form");
                assert_eq!(serde_json::to_string(&parsed).expect("legacy wire"), wire);
                let mut pointers = vec![
                    ("scope_reference_offset".to_owned(), 100),
                    ("envelope_guid_offset".to_owned(), 110),
                    ("tag_body_based_on_faces_offset".to_owned(), 190),
                ];
                if mask == 1 {
                    pointers.push(("mode".to_owned(), 17));
                }
                for (index, (entity, parameter, auxiliary)) in body_offsets.iter().enumerate() {
                    pointers.push((format!("bodies.{index}.entity.offset"), *entity));
                    pointers.push((format!("bodies.{index}.parameter_body"), *parameter));
                    pointers.push((format!("bodies.{index}.auxiliary"), *auxiliary));
                }
                let pointers = pointers
                    .iter()
                    .map(|(name, offset)| (name.as_str(), *offset))
                    .collect::<Vec<_>>();
                crate::records::feature::test_support::assert_located_at(parsed, &pointers);
                for guid in ["_".repeat(38), "invalid".into()] {
                    let changed = wire.replace("11111111-2222-3333-4444-555555555555", &guid);
                    let decoded = serde_json::from_str::<
                        crate::records::feature::base_feature::DesignBaseFeatureConstruction,
                    >(&changed);
                    if guid == "invalid" {
                        assert!(decoded.is_err());
                    } else {
                        assert_eq!(serde_json::to_string(&decoded.unwrap()).unwrap(), changed);
                    }
                }
                let value: serde_json::Value = serde_json::from_str(&wire).expect("legacy JSON");
                if form == "compact_one_body" {
                    for mode in [1_u8, 2, u8::MAX] {
                        let mut mode_wire = value.clone();
                        mode_wire["mode"] = mode.into();
                        let decoded = serde_json::from_value::<
                            crate::records::feature::base_feature::DesignBaseFeatureConstruction,
                        >(mode_wire.clone());
                        if mode == 1 {
                            assert_eq!(
                                serde_json::to_value(decoded.expect("mode one")).unwrap(),
                                mode_wire
                            );
                        } else {
                            assert!(decoded
                                .expect_err("unknown compact mode")
                                .to_string()
                                .contains("mode"));
                        }
                    }
                }
                for field in [
                    "body_entity_suffixes",
                    "body_entity_fields",
                    "body_reference_records",
                    "parameter_body_records",
                    "auxiliary_records",
                ] {
                    let mut invalid = value.clone();
                    invalid[field].as_array_mut().expect("body array").pop();
                    assert!(
                        serde_json::from_value::<
                            crate::records::feature::base_feature::DesignBaseFeatureConstruction,
                        >(invalid)
                        .is_err(),
                        "{field}"
                    );
                }
                for field in ["body_reference_records"] {
                    let mut invalid = value.clone();
                    invalid[field][0] = serde_json::json!(999);
                    let error = serde_json::from_value::<
                        crate::records::feature::base_feature::DesignBaseFeatureConstruction,
                    >(invalid)
                    .expect_err("conflicting body view")
                    .to_string();
                    assert!(error.contains(field));
                }
                let mut invalid = value;
                invalid["tag_body_based_on_faces"] = serde_json::json!(false);
                let error = serde_json::from_value::<
                    crate::records::feature::base_feature::DesignBaseFeatureConstruction,
                >(invalid)
                .expect_err("false body-source tag")
                .to_string();
                assert!(error.contains("tag_body_based_on_faces"));
            } else {
                let error = parsed.expect_err("mixed legacy mode form").to_string();
                assert!(error.contains("form"));
                assert!(error.contains("mode"));
            }
        }
    }
}

#[test]
fn snapshot_body_rows_preserve_wire_and_reject_unequal_arrays() {
    for values in 0..=2 {
        for fields in 0..=2 {
            let values_wire = ["[]", "[101]", "[101,202]"][values];
            let fields_wire = [
                "[]",
                "[\"000003040506\"]",
                "[\"000003040506\",\"000004030201\"]",
            ][fields];
            let wire = format!(
                r#"{{"body_entity_suffixes":{values_wire},"body_entity_fields":{fields_wire},"related_guids":["aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb","cccccccc-cccc-4ccc-8ccc-cccccccccccc"],"linkage_record":301,"auxiliary_record":401}}"#
            );
            let parsed = serde_json::from_str::<
                crate::records::feature::base_feature::DesignBaseFeatureConstruction,
            >(&wire);
            if values == fields {
                let parsed = parsed.expect("complete snapshot rows");
                assert_eq!(serde_json::to_string(&parsed).expect("snapshot wire"), wire);
                let mut pointers = vec![
                    ("related_guid_offsets.0", 66),
                    ("related_guid_offsets.1", 142),
                    ("related_guid_offsets.2", 275),
                    ("linkage_record_offset", 234),
                    ("auxiliary_record_offset", 253),
                ];
                pointers.extend(
                    [("bodies.0.offset", 22), ("bodies.1.offset", 37)]
                        .into_iter()
                        .take(values),
                );
                crate::records::feature::test_support::assert_located_at(parsed, &pointers);
            } else {
                let error = parsed.expect_err("unequal snapshot arrays").to_string();
                assert!(error.contains("body_entity_suffixes"));
                assert!(error.contains("body_entity_fields"));
            }
        }
    }
}

#[test]
fn direct_base_feature_emits_its_single_body_reference_views() {
    let wire = r#"{"body_entity_suffixes":[201],"body_reference_records":[201],"parameter_body_record":198,"auxiliary_record":202,"envelope_guid":"fcec56e3-832f-4468-88a4-d710e62e629f","tag_body_based_on_faces":true}"#;
    let parsed: crate::records::feature::base_feature::DesignBaseFeatureConstruction =
        serde_json::from_str(wire).expect("direct body form");
    assert_eq!(
        serde_json::to_string(&parsed).expect("direct body wire"),
        wire
    );
    for guid in ["_".repeat(38), "invalid".into()] {
        let changed = wire.replace("fcec56e3-832f-4468-88a4-d710e62e629f", &guid);
        let decoded = serde_json::from_str::<
            crate::records::feature::base_feature::DesignBaseFeatureConstruction,
        >(&changed);
        if guid == "invalid" {
            assert!(decoded.is_err());
        } else {
            assert_eq!(serde_json::to_string(&decoded.unwrap()).unwrap(), changed);
        }
    }
    assert_eq!(parsed.body_entity_suffixes().collect::<Vec<_>>(), [201]);
    assert_eq!(parsed.body_reference_records().collect::<Vec<_>>(), [201]);
    crate::records::feature::test_support::assert_located_at(
        parsed,
        &[
            ("body", 22),
            ("parameter_body_record_offset", 100),
            ("auxiliary_record_offset", 120),
            ("envelope_guid_offset", 140),
            ("tag_body_based_on_faces_offset", 90),
        ],
    );
    for (field, old, new) in [
        ("body_entity_suffixes", "[201]", "[]"),
        ("body_entity_suffixes", "[201]", "[201,202]"),
        ("body_entity_suffixes", "[201]", "[4294967296]"),
        ("body_reference_records", "[201]", "[202]"),
        ("body_reference_records", "[201]", "[]"),
        ("tag_body_based_on_faces", "true", "false"),
    ] {
        let invalid = wire.replace(&format!("\"{field}\":{old}"), &format!("\"{field}\":{new}"));
        let error = serde_json::from_str::<
            crate::records::feature::base_feature::DesignBaseFeatureConstruction,
        >(&invalid)
        .expect_err("invalid direct body view")
        .to_string();
        assert!(error.contains(field));
    }
}

#[test]
fn base_feature_result_rows_preserve_complete_and_unrepeated_runs() {
    for count in 0..=2 {
        let suffixes = ["[]", "[101]", "[101,102]"][count];
        let references = ["[]", "[201]", "[201,202]"][count];
        let results = ["[]", "[301]", "[301,302]"][count];
        let fields = [
            "[]",
            "[\"000001000000\"]",
            "[\"000001000000\",\"000001000000\"]",
        ][count];
        for repeated in ["[]", fields] {
            let wire = format!(
                r#"{{"body_entity_suffixes":{suffixes},"body_entity_fields":{fields},"body_reference_records":{references},"body_reference_fields":{fields},"repeated_reference_fields":{repeated},"metadata_record":401,"metadata_field":"0000","result_records":{results},"result_fields":{fields}}}"#
            );
            let construction: crate::records::feature::base_feature::DesignBaseFeatureConstruction =
                serde_json::from_str(&wire).expect("aligned result rows");
            assert_eq!(
                serde_json::to_string(&construction).expect("result wire"),
                wire
            );
            let mut pointers = vec![("metadata_record_offset".to_owned(), 110)];
            for (index, (entity, reference, result)) in [(22, 52, 82), (37, 67, 93)]
                .into_iter()
                .take(count)
                .enumerate()
            {
                pointers.push((format!("bodies.{index}.entity.offset"), entity));
                pointers.push((format!("bodies.{index}.reference.offset"), reference));
                pointers.push((format!("bodies.{index}.result.offset"), result));
            }
            let pointers = pointers
                .iter()
                .map(|(name, offset)| (name.as_str(), *offset))
                .collect::<Vec<_>>();
            crate::records::feature::test_support::assert_located_at(construction, &pointers);
            let value: serde_json::Value = serde_json::from_str(&wire).expect("result JSON");
            for field in [
                "body_entity_suffixes",
                "body_entity_fields",
                "body_reference_records",
                "body_reference_fields",
                "result_records",
                "result_fields",
            ] {
                let mut invalid = value.clone();
                let array = invalid[field].as_array_mut().expect("result array");
                if count == 0 {
                    array.push(if field.ends_with("fields") {
                        serde_json::json!("000000000000")
                    } else {
                        serde_json::json!(1)
                    });
                } else {
                    array.pop();
                }
                assert!(
                    serde_json::from_value::<
                        crate::records::feature::base_feature::DesignBaseFeatureConstruction,
                    >(invalid)
                    .is_err(),
                    "{field}"
                );
            }
            let mut invalid = value;
            invalid["repeated_reference_fields"] = serde_json::json!(vec![
                    cadmpeg_ir::native::bytes::NativeBytes::from([0; 6]);
                    count + 1
                ]);
            let error = serde_json::from_value::<
                crate::records::feature::base_feature::DesignBaseFeatureConstruction,
            >(invalid)
            .expect_err("partial repeated run")
            .to_string();
            assert!(error.contains("repeated_reference_fields"));
        }
    }
}
