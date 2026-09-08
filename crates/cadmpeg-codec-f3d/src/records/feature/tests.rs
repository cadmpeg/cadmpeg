// SPDX-License-Identifier: Apache-2.0

mod assembly;

use super::{DesignFeatureKind, DesignParameterScope};

fn empty_scope(kind: DesignFeatureKind) -> serde_json::Value {
    serde_json::to_value(DesignParameterScope::empty("scope", kind, 1)).expect("serialize scope")
}

#[test]
fn flattened_scope_payloads_propagate_invalid_field_errors() {
    for (kind, field) in [
        (DesignFeatureKind::Extrude, "extrude_prologue"),
        (DesignFeatureKind::CoilPrimitive, "coil_extent"),
        (DesignFeatureKind::BaseFlange, "base_flange_operation"),
        (DesignFeatureKind::Loft, "path_feature_construction"),
    ] {
        let mut wire = empty_scope(kind);
        wire[field] = serde_json::json!(17);
        assert!(serde_json::from_value::<DesignParameterScope>(wire).is_err());
    }
}

#[test]
fn flattened_scope_frames_reject_partial_value_offset_pairs() {
    let transform = serde_json::json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ]);
    for (kind, prefix) in [
        (DesignFeatureKind::WorkPlane, "work_plane"),
        (DesignFeatureKind::JointOrigin, "joint_origin"),
    ] {
        for (suffix, value) in [
            ("transform", transform.clone()),
            ("transform_offset", serde_json::json!(10)),
            ("reference", serde_json::json!(2)),
            ("reference_offset", serde_json::json!(20)),
        ] {
            let field = format!("{prefix}_{suffix}");
            let mut wire = empty_scope(kind.clone());
            wire[&field] = value;
            let error = serde_json::from_value::<DesignParameterScope>(wire)
                .expect_err("partial frame must fail");
            assert!(error.to_string().contains(prefix));
        }
        let mut wire = empty_scope(kind);
        wire[format!("{prefix}_transform")] = transform.clone();
        wire[format!("{prefix}_transform_offset")] = serde_json::json!(10);
        wire[format!("{prefix}_reference")] = serde_json::json!(2);
        wire[format!("{prefix}_reference_offset")] = serde_json::json!(20);
        let decoded: DesignParameterScope =
            serde_json::from_value(wire.clone()).expect("complete frame");
        assert_eq!(
            serde_json::to_value(decoded).expect("serialize frame"),
            wire
        );
    }
}

#[test]
fn flattened_sketch_entity_requires_all_identity_fields() {
    for (field, value) in [
        ("entity_id", serde_json::json!("entity:2")),
        ("entity_suffix", serde_json::json!(2)),
        ("entity_reference_offset", serde_json::json!(20)),
    ] {
        let mut wire = empty_scope(DesignFeatureKind::Sketch);
        wire[field] = value;
        let error = serde_json::from_value::<DesignParameterScope>(wire)
            .expect_err("partial identity must fail");
        assert!(error.to_string().contains("entity_id"));
    }
}

#[test]
fn absent_flattened_scope_payloads_preserve_the_wire() {
    for kind in [
        DesignFeatureKind::Extrude,
        DesignFeatureKind::CoilPrimitive,
        DesignFeatureKind::BaseFlange,
        DesignFeatureKind::Loft,
        DesignFeatureKind::Sweep,
        DesignFeatureKind::WorkPlane,
        DesignFeatureKind::JointOrigin,
        DesignFeatureKind::Sketch,
    ] {
        let scope = DesignParameterScope::empty("scope", kind, 1);
        let wire = serde_json::to_string(&scope).expect("serialize scope");
        let decoded: DesignParameterScope = serde_json::from_str(&wire).expect("empty payload");
        assert_eq!(
            serde_json::to_string(&decoded).expect("serialize scope"),
            wire
        );
    }
}

#[test]
fn revolve_opposite_angle_preserves_wire_and_rejects_partial_source_location() {
    let base = r#"{"operation":"join","operation_offset":12,"angle":1.5,"angle_record_index":3,"angle_offset":40"#;
    for tail in [
        "}",
        ",\"opposite_angle_record_index\":4,\"opposite_angle_offset\":80}",
    ] {
        let wire = format!("{base}{tail}");
        let value: crate::records::feature::DesignRevolveConstruction =
            serde_json::from_str(&wire).expect("revolve construction");
        assert_eq!(serde_json::to_string(&value).expect("revolve wire"), wire);
    }
    for tail in [
        ",\"opposite_angle_record_index\":4}",
        ",\"opposite_angle_offset\":80}",
    ] {
        let error = serde_json::from_str::<crate::records::feature::DesignRevolveConstruction>(
            &format!("{base}{tail}"),
        )
        .expect_err("partial opposite angle location");
        assert!(error.to_string().contains("opposite_angle_record_index"));
        assert!(error.to_string().contains("opposite_angle_offset"));
    }
}

#[test]
fn extrude_prefixes_preserve_wire_and_reject_partial_locations() {
    let reference = r#"{"record_index":2,"record_index_offset":26,"trailing_zero_count":7"#;
    for fields in [
        "",
        ",\"operation_prefix_marker\":1,\"operation_prefix_marker_offset\":37",
    ] {
        let wire = format!("{reference}{fields}}}");
        let value: crate::records::feature::DesignExtrudePrologueReference =
            serde_json::from_str(&wire).expect("prologue reference");
        assert_eq!(
            serde_json::to_string(&value).expect("prologue reference wire"),
            wire
        );
    }
    for marker in [0, 2, u8::MAX] {
        let wire = format!("{reference},\"operation_prefix_marker\":{marker},\"operation_prefix_marker_offset\":37}}");
        let error =
            serde_json::from_str::<crate::records::feature::DesignExtrudePrologueReference>(&wire)
                .expect_err("invalid prefix marker");
        assert!(error.to_string().contains("operation_prefix_marker"));
    }
    for field in ["operation_prefix_marker", "operation_prefix_marker_offset"] {
        let error =
            serde_json::from_str::<crate::records::feature::DesignExtrudePrologueReference>(
                &format!("{reference},\"{field}\":1}}"),
            )
            .expect_err("partial prologue reference marker");
        assert!(error.to_string().contains(field));
    }
    for (layout, field, suffix) in [
        (
            "legacy_distance",
            "prefix_value",
            r#","operation":"join","operation_offset":25,"extent_kind":2,"extent_kind_offset":29,"direction_reversed":false,"direction_reversed_offset":33,"geometry_kind":1,"geometry_kind_offset":34}"#,
        ),
        (
            "legacy_shifted",
            "operation_prefix_marker",
            r#","operation":"join","operation_offset":28,"direction_face_extend_values":[1,0],"side_extent_discriminators":[1,0],"side_extent_discriminator_offsets":[105,109],"direction_face_extend_offsets":[32,36],"direction_reversed":false,"direction_reversed_offset":40,"solid_operation":true,"solid_operation_offset":41,"start":"profile_plane","start_offset":42}"#,
        ),
    ] {
        for value in [None, Some(i32::from(layout != "legacy_distance"))] {
            let fields = match value {
                Some(value) => format!(",\"{field}\":{value},\"{field}_offset\":21"),
                None if layout == "legacy_distance" => {
                    format!(",\"{field}\":null,\"{field}_offset\":null")
                }
                None => String::new(),
            };
            let wire = format!("{{\"layout\":\"{layout}\"{fields}{suffix}");
            let value: crate::records::feature::DesignExtrudePrologue =
                serde_json::from_str(&wire).expect("extrude prologue");
            assert_eq!(
                serde_json::to_string(&value).expect("extrude prologue wire"),
                wire
            );
        }
        for invalid in [2, u8::MAX] {
            let wire = format!(
                "{{\"layout\":\"{layout}\",\"{field}\":{invalid},\"{field}_offset\":21{suffix}"
            );
            let error =
                serde_json::from_str::<crate::records::feature::DesignExtrudePrologue>(&wire)
                    .expect_err("invalid prologue prefix");
            assert!(error.to_string().contains(field));
        }
        for partial_field in [field.to_owned(), format!("{field}_offset")] {
            let wire = format!("{{\"layout\":\"{layout}\",\"{partial_field}\":1{suffix}");
            let error =
                serde_json::from_str::<crate::records::feature::DesignExtrudePrologue>(&wire)
                    .expect_err("partial prologue prefix");
            assert!(error.to_string().contains(field));
        }
    }
}

#[test]
fn mirror_references_preserve_wire_and_reject_partial_locations() {
    let prefix = r#"{"count":2,"count_record_index":11,"count_offset":0,"stitch_tolerance":0.001,"stitch_tolerance_record_index":12,"stitch_tolerance_offset":0,"seed_group_record_index":20,"plane_group_record_index":30"#;
    for fields in ["", ",\"seed_feature_scope_record_index\":40,\"seed_feature_reference_offset\":100", ",\"plane_scope_record_index\":50,\"plane_reference_offset\":200", ",\"seed_feature_scope_record_index\":40,\"seed_feature_reference_offset\":100,\"plane_scope_record_index\":50,\"plane_reference_offset\":200"] {
        let wire = format!("{prefix}{fields}}}");
        let value: crate::records::feature::DesignMirrorConstruction = serde_json::from_str(&wire).expect("mirror construction");
        assert_eq!(serde_json::to_string(&value).expect("mirror wire"), wire);
    }
    for field in [
        "seed_feature_scope_record_index",
        "seed_feature_reference_offset",
        "plane_scope_record_index",
        "plane_reference_offset",
    ] {
        let error = serde_json::from_str::<crate::records::feature::DesignMirrorConstruction>(
            &format!("{prefix},\"{field}\":1}}"),
        )
        .expect_err("partial mirror reference");
        assert!(error.to_string().contains(field));
    }
}

#[test]
// Fixture fields are appended from the bounded table of explicit test cases.
#[allow(clippy::format_push_string)]
fn hole_construction_preserves_tangent_and_input_reference_wire() {
    let prefix = r#"{"point_record_index":55,"point_record_byte_offset":10,"position":[1.25,-2.5,3.75],"position_offset":35,"direction":[0.0,0.0,1.0],"direction_offset":59,"point_parameters":[0.125,-0.25],"point_parameter_offsets":[83,91],"reference_type":19,"reference_type_offset":99"#;
    let fields = [
        ("tangent_point_data", "[-1.0,-1.0,-1.0]"),
        ("tangent_point_data_prefix", "127"),
        ("tangent_point_data_offset", "104"),
    ];
    for mask in 0..8 {
        let mut wire = prefix.to_owned();
        for (index, (field, value)) in fields.iter().enumerate() {
            if mask & (1 << index) != 0 {
                wire.push_str(&format!(",\"{field}\":{value}"));
            }
        }
        wire.push_str(r#","input_record_indices":[378,379],"input_record_offsets":[129,134]}"#);
        let result = serde_json::from_str::<crate::records::feature::DesignHoleConstruction>(&wire);
        if mask == 0 || mask == 7 {
            assert_eq!(
                serde_json::to_string(&result.expect("complete tangent form")).expect("hole wire"),
                wire
            );
        } else {
            let error = result.expect_err("partial tangent form").to_string();
            for (field, _) in fields {
                assert!(error.contains(field));
            }
        }
    }
    for (indices, offsets) in [("[]", "[129]"), ("[378]", "[]"), ("[378,379]", "[129]")] {
        let wire = format!(
            "{prefix},\"input_record_indices\":{indices},\"input_record_offsets\":{offsets}}}"
        );
        let error = serde_json::from_str::<crate::records::feature::DesignHoleConstruction>(&wire)
            .expect_err("unequal input arrays")
            .to_string();
        assert!(error.contains("input_record_indices"));
        assert!(error.contains("input_record_offsets"));
    }
}

#[test]
fn coil_values_preserve_optional_locations_and_reject_orphan_offsets() {
    for (field, value) in [
        ("coil_operation", "\"cut\""),
        ("coil_extent", "\"spiral\""),
        ("coil_section", "\"circular\""),
        ("coil_section_placement", "\"inside\""),
        ("coil_clockwise", "false"),
    ] {
        for wire in [
            "{}".to_owned(),
            format!("{{\"{field}\":{value}}}"),
            format!("{{\"{field}\":{value},\"{field}_offset\":0}}"),
            format!("{{\"{field}\":{value},\"{field}_offset\":30}}"),
        ] {
            let parsed: crate::records::feature::DesignCoilScope =
                serde_json::from_str(&wire).expect("valid Coil field");
            assert_eq!(serde_json::to_string(&parsed).expect("Coil wire"), wire);
        }
        let wire = format!("{{\"{field}_offset\":30}}");
        let error = serde_json::from_str::<crate::records::feature::DesignCoilScope>(&wire)
            .expect_err("offset without value")
            .to_string();
        assert!(error.contains(field));
        assert!(error.contains(&format!("{field}_offset")));
    }
}

#[test]
// Fixture fields are appended from the bounded table of explicit test cases.
#[allow(clippy::format_push_string)]
fn legacy_base_feature_form_owns_its_compact_mode() {
    for form in ["compact_one_body", "expanded_two_body"] {
        let (
            suffixes,
            suffix_offsets,
            fields,
            refs,
            ref_offsets,
            parameters,
            parameter_offsets,
            auxiliary,
            auxiliary_offsets,
        ) = if form == "compact_one_body" {
            (
                "[201]",
                "[22]",
                "[[0,0,0,0,0,0]]",
                "[201]",
                "[22]",
                "[301]",
                "[40]",
                "[303]",
                "[50]",
            )
        } else {
            (
                "[401,402]",
                "[22,36]",
                "[[0,0,0,0,0,0],[0,0,0,0,0,0]]",
                "[401,402]",
                "[22,36]",
                "[301,302]",
                "[50,60]",
                "[303,304]",
                "[70,80]",
            )
        };
        for mask in 0..4 {
            let mut wire = format!("{{\"form\":\"{form}\"");
            if mask & 1 != 0 {
                wire.push_str(",\"mode\":0");
            }
            if mask & 2 != 0 {
                wire.push_str(",\"mode_offset\":17");
            }
            wire.push_str(&format!(r#","body_entity_suffixes":{suffixes},"body_entity_suffix_offsets":{suffix_offsets},"body_entity_fields":{fields},"body_reference_records":{refs},"body_reference_record_offsets":{ref_offsets},"parameter_body_records":{parameters},"parameter_body_record_offsets":{parameter_offsets},"auxiliary_records":{auxiliary},"auxiliary_record_offsets":{auxiliary_offsets},"scope_reference":90,"scope_reference_offset":100,"envelope_guid":"11111111-2222-3333-4444-555555555555","envelope_guid_offset":110,"tag_body_based_on_faces":true,"tag_body_based_on_faces_offset":190}}"#));
            let parsed = serde_json::from_str::<
                crate::records::feature::DesignBaseFeatureConstruction,
            >(&wire);
            if (form == "compact_one_body" && mask == 3)
                || (form == "expanded_two_body" && mask == 0)
            {
                assert_eq!(
                    serde_json::to_string(&parsed.expect("complete legacy form"))
                        .expect("legacy wire"),
                    wire
                );
                for guid in ["_".repeat(38), "invalid".into()] {
                    let changed = wire.replace("11111111-2222-3333-4444-555555555555", &guid);
                    let decoded = serde_json::from_str::<
                        crate::records::feature::DesignBaseFeatureConstruction,
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
                            crate::records::feature::DesignBaseFeatureConstruction,
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
                    "body_entity_suffix_offsets",
                    "body_entity_fields",
                    "body_reference_records",
                    "body_reference_record_offsets",
                    "parameter_body_records",
                    "parameter_body_record_offsets",
                    "auxiliary_records",
                    "auxiliary_record_offsets",
                ] {
                    let mut invalid = value.clone();
                    invalid[field].as_array_mut().expect("body array").pop();
                    assert!(
                        serde_json::from_value::<
                            crate::records::feature::DesignBaseFeatureConstruction,
                        >(invalid)
                        .is_err(),
                        "{field}"
                    );
                }
                for field in ["body_reference_records", "body_reference_record_offsets"] {
                    let mut invalid = value.clone();
                    invalid[field][0] = serde_json::json!(999);
                    let error = serde_json::from_value::<
                        crate::records::feature::DesignBaseFeatureConstruction,
                    >(invalid)
                    .expect_err("conflicting body view")
                    .to_string();
                    assert!(error.contains(field));
                }
                let mut invalid = value;
                invalid["tag_body_based_on_faces"] = serde_json::json!(false);
                let error = serde_json::from_value::<
                    crate::records::feature::DesignBaseFeatureConstruction,
                >(invalid)
                .expect_err("false body-source tag")
                .to_string();
                assert!(error.contains("tag_body_based_on_faces"));
            } else {
                let error = parsed.expect_err("mixed legacy mode form").to_string();
                assert!(error.contains("form"));
                assert!(error.contains("mode"));
                assert!(error.contains("mode_offset"));
            }
        }
    }
}

#[test]
fn snapshot_body_rows_preserve_wire_and_reject_unequal_arrays() {
    for values in 0..=2 {
        for offsets in 0..=2 {
            for fields in 0..=2 {
                let values_wire = ["[]", "[101]", "[101,202]"][values];
                let offsets_wire = ["[]", "[22]", "[22,37]"][offsets];
                let fields_wire =
                    ["[]", "[[1,2,3,4,5,6]]", "[[1,2,3,4,5,6],[6,5,4,3,2,1]]"][fields];
                let wire = format!(
                    r#"{{"body_entity_suffixes":{values_wire},"body_entity_suffix_offsets":{offsets_wire},"body_entity_fields":{fields_wire},"related_guids":["aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb","cccccccc-cccc-4ccc-8ccc-cccccccccccc"],"related_guid_offsets":[66,142,275],"linkage_record":301,"linkage_record_offset":234,"auxiliary_record":401,"auxiliary_record_offset":253}}"#
                );
                let parsed = serde_json::from_str::<
                    crate::records::feature::DesignBaseFeatureConstruction,
                >(&wire);
                if values == offsets && values == fields {
                    assert_eq!(
                        serde_json::to_string(&parsed.expect("complete snapshot rows"))
                            .expect("snapshot wire"),
                        wire
                    );
                } else {
                    let error = parsed.expect_err("unequal snapshot arrays").to_string();
                    assert!(error.contains("body_entity_suffixes"));
                    assert!(error.contains("body_entity_suffix_offsets"));
                    assert!(error.contains("body_entity_fields"));
                }
            }
        }
    }
}

#[test]
fn direct_base_feature_emits_its_single_body_reference_views() {
    let wire = r#"{"body_entity_suffixes":[201],"body_entity_suffix_offsets":[22],"body_reference_records":[201],"body_reference_record_offsets":[22],"parameter_body_record":198,"parameter_body_record_offset":100,"auxiliary_record":202,"auxiliary_record_offset":120,"envelope_guid":"fcec56e3-832f-4468-88a4-d710e62e629f","envelope_guid_offset":140,"tag_body_based_on_faces":true,"tag_body_based_on_faces_offset":90}"#;
    let parsed: crate::records::feature::DesignBaseFeatureConstruction =
        serde_json::from_str(wire).expect("direct body form");
    assert_eq!(
        serde_json::to_string(&parsed).expect("direct body wire"),
        wire
    );
    for guid in ["_".repeat(38), "invalid".into()] {
        let changed = wire.replace("fcec56e3-832f-4468-88a4-d710e62e629f", &guid);
        let decoded = serde_json::from_str::<crate::records::feature::DesignBaseFeatureConstruction>(
            &changed,
        );
        if guid == "invalid" {
            assert!(decoded.is_err());
        } else {
            assert_eq!(serde_json::to_string(&decoded.unwrap()).unwrap(), changed);
        }
    }
    assert_eq!(parsed.body_entity_suffixes().collect::<Vec<_>>(), [201]);
    assert_eq!(parsed.body_reference_records().collect::<Vec<_>>(), [201]);
    for (field, old, new) in [
        ("body_entity_suffixes", "[201]", "[]"),
        ("body_entity_suffixes", "[201]", "[201,202]"),
        ("body_entity_suffixes", "[201]", "[4294967296]"),
        ("body_entity_suffix_offsets", "[22]", "[]"),
        ("body_reference_records", "[201]", "[202]"),
        ("body_reference_records", "[201]", "[]"),
        ("body_reference_record_offsets", "[22]", "[23]"),
        ("body_reference_record_offsets", "[22]", "[]"),
        ("tag_body_based_on_faces", "true", "false"),
    ] {
        let invalid = wire.replace(&format!("\"{field}\":{old}"), &format!("\"{field}\":{new}"));
        let error = serde_json::from_str::<crate::records::feature::DesignBaseFeatureConstruction>(
            &invalid,
        )
        .expect_err("invalid direct body view")
        .to_string();
        assert!(error.contains(field));
    }
}

#[test]
fn base_feature_result_rows_preserve_complete_and_unrepeated_runs() {
    for count in 0..=2 {
        let suffixes = ["[]", "[101]", "[101,102]"][count];
        let suffix_offsets = ["[]", "[22]", "[22,37]"][count];
        let references = ["[]", "[201]", "[201,202]"][count];
        let reference_offsets = ["[]", "[52]", "[52,67]"][count];
        let results = ["[]", "[301]", "[301,302]"][count];
        let result_offsets = ["[]", "[82]", "[82,93]"][count];
        let fields = ["[]", "[[0,0,1,0,0,0]]", "[[0,0,1,0,0,0],[0,0,1,0,0,0]]"][count];
        for repeated in ["[]", fields] {
            let wire = format!(
                r#"{{"body_entity_suffixes":{suffixes},"body_entity_suffix_offsets":{suffix_offsets},"body_entity_fields":{fields},"body_reference_records":{references},"body_reference_record_offsets":{reference_offsets},"body_reference_fields":{fields},"repeated_reference_fields":{repeated},"metadata_record":401,"metadata_record_offset":110,"metadata_field":[0,0],"result_records":{results},"result_record_offsets":{result_offsets},"result_fields":{fields}}}"#
            );
            let construction: crate::records::feature::DesignBaseFeatureConstruction =
                serde_json::from_str(&wire).expect("aligned result rows");
            assert_eq!(
                serde_json::to_string(&construction).expect("result wire"),
                wire
            );
            let value: serde_json::Value = serde_json::from_str(&wire).expect("result JSON");
            for field in [
                "body_entity_suffixes",
                "body_entity_suffix_offsets",
                "body_entity_fields",
                "body_reference_records",
                "body_reference_record_offsets",
                "body_reference_fields",
                "result_records",
                "result_record_offsets",
                "result_fields",
            ] {
                let mut invalid = value.clone();
                let array = invalid[field].as_array_mut().expect("result array");
                if count == 0 {
                    array.push(if field.ends_with("fields") {
                        serde_json::json!([0, 0, 0, 0, 0, 0])
                    } else {
                        serde_json::json!(1)
                    });
                } else {
                    array.pop();
                }
                assert!(
                    serde_json::from_value::<crate::records::feature::DesignBaseFeatureConstruction>(invalid)
                        .is_err(),
                    "{field}"
                );
            }
            let mut invalid = value;
            invalid["repeated_reference_fields"] = serde_json::json!(vec![[0; 6]; count + 1]);
            let error = serde_json::from_value::<
                crate::records::feature::DesignBaseFeatureConstruction,
            >(invalid)
            .expect_err("partial repeated run")
            .to_string();
            assert!(error.contains("repeated_reference_fields"));
        }
    }
}

#[test]
// The resize uses only the explicit small lengths in this wire fixture.
#[allow(clippy::disallowed_methods)]
fn copied_body_rows_preserve_wire_and_reject_unequal_runs() {
    let wire = r#"{"body_group_record_index":501,"body_group_class_tag":"264","body_group_byte_offset":100,"body_operand_record_indices":[502,504],"body_operand_record_offsets":[126,137],"relation_record_index":503,"relation_class_tag":"264","relation_byte_offset":200,"source_body_entity_suffixes":[11,13],"source_body_entity_suffix_offsets":[225,255],"copied_body_entity_suffixes":[12,14],"copied_body_entity_suffix_offsets":[240,270]}"#;
    let operation: crate::records::feature::DesignCopyPasteBodiesOperation =
        serde_json::from_str(wire).expect("copy body rows");
    assert_eq!(
        serde_json::to_string(&operation).expect("copy body wire"),
        wire
    );
    assert_eq!(operation.bodies[1].source.value, 13);
    assert_eq!(operation.bodies[1].copied.value, 14);
    let value: serde_json::Value = serde_json::from_str(wire).expect("copy JSON");
    for field in [
        "body_operand_record_indices",
        "body_operand_record_offsets",
        "source_body_entity_suffixes",
        "source_body_entity_suffix_offsets",
        "copied_body_entity_suffixes",
        "copied_body_entity_suffix_offsets",
    ] {
        for length in [0, 1, 3] {
            let mut invalid = value.clone();
            invalid[field]
                .as_array_mut()
                .expect("copy array")
                .resize(length, serde_json::json!(1));
            let error = serde_json::from_value::<
                crate::records::feature::DesignCopyPasteBodiesOperation,
            >(invalid)
            .expect_err("unequal body runs")
            .to_string();
            assert!(error.contains(field), "{field}: {error}");
        }
    }
}

#[test]
fn scale_center_preserves_wire_and_rejects_partial_location() {
    for center in [
        None,
        Some(crate::records::Located {
            value: [1.25, -2.5, 3.75],
            offset: 40,
        }),
    ] {
        let record = crate::records::feature::DesignScaleOperation {
            body_group_record_index: 102,
            center_record_index: 105,
            center_position: center,
            uniform_factor: 2.5,
            uniform_factor_offset: 21,
        };
        let expected = match center {
            None => {
                r#"{"body_group_record_index":102,"center_record_index":105,"uniform_factor":2.5,"uniform_factor_offset":21}"#
            }
            Some(_) => {
                r#"{"body_group_record_index":102,"center_record_index":105,"center_position":[1.25,-2.5,3.75],"center_position_offset":40,"uniform_factor":2.5,"uniform_factor_offset":21}"#
            }
        };
        assert_eq!(serde_json::to_string(&record).unwrap(), expected);
        assert_eq!(
            serde_json::from_str::<crate::records::feature::DesignScaleOperation>(expected)
                .unwrap(),
            record
        );
    }
    for partial in [
        r#""center_position":[1.25,-2.5,3.75]"#,
        r#""center_position_offset":40"#,
    ] {
        let wire = format!(
            r#"{{"body_group_record_index":102,"center_record_index":105,{partial},"uniform_factor":2.5,"uniform_factor_offset":21}}"#
        );
        assert!(
            serde_json::from_str::<crate::records::feature::DesignScaleOperation>(&wire)
                .unwrap_err()
                .to_string()
                .contains("center_position")
        );
    }
}

#[test]
fn rectangular_pattern_rows_preserve_wire_and_reject_parallel_mismatch() {
    let transform = serde_json::json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ]);
    for count in [0, 1, 3] {
        let value = serde_json::json!({
            "record_indices": (0..count).collect::<Vec<u32>>(),
            "transforms": (0..count).map(|_| transform.clone()).collect::<Vec<_>>(),
            "transform_offsets": (0..count).map(|index| u64::from(index) * 100).collect::<Vec<_>>()
        });
        let wire: crate::records::feature::DesignRectangularPatternInstancesWire =
            serde_json::from_value(value.clone()).unwrap();
        let expected = serde_json::to_string(&wire).unwrap();
        let native: crate::records::feature::DesignRectangularPatternInstances =
            serde_json::from_str(&expected).unwrap();
        assert_eq!(native.instance_count(), count as usize);
        assert_eq!(serde_json::to_string(&native).unwrap(), expected);
        if count != 0 {
            let mut component = value.clone();
            component["component_occurrences"] = serde_json::json!({
                "component_guid": "00000001-1111-4111-8111-111111111111", "seed_occurrence_guid": "00000003-1111-4111-8111-111111111111",
                "generated_occurrence_guids": (1..count).map(|index| format!("{index:08}-2222-4222-8222-222222222222")).collect::<Vec<_>>()
            });
            let wire: crate::records::feature::DesignRectangularPatternInstancesWire =
                serde_json::from_value(component.clone()).unwrap();
            let expected = serde_json::to_string(&wire).unwrap();
            let native: crate::records::feature::DesignRectangularPatternInstances =
                serde_json::from_str(&expected).unwrap();
            assert_eq!(serde_json::to_string(&native).unwrap(), expected);
            component["component_occurrences"]["generated_occurrence_guids"] = serde_json::json!([
                "99999999-9999-4999-8999-999999999999",
                "99999999-9999-4999-8999-999999999999",
                "99999999-9999-4999-8999-999999999999"
            ]);
            assert!(serde_json::from_value::<
                crate::records::feature::DesignRectangularPatternInstances,
            >(component)
            .unwrap_err()
            .to_string()
            .contains("generated_occurrence_guids"));
        }
        for field in ["record_indices", "transforms", "transform_offsets"] {
            let mut invalid = value.clone();
            invalid[field]
                .as_array_mut()
                .unwrap()
                .push(if field == "transforms" {
                    transform.clone()
                } else {
                    serde_json::json!(0)
                });
            assert!(serde_json::from_value::<
                crate::records::feature::DesignRectangularPatternInstances,
            >(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
        }
    }
    let empty_component = serde_json::json!({
        "record_indices": [], "transforms": [], "transform_offsets": [],
        "component_occurrences": { "component_guid": "00000001-1111-4111-8111-111111111111", "seed_occurrence_guid": "00000003-1111-4111-8111-111111111111", "generated_occurrence_guids": [] }
    });
    assert!(
        serde_json::from_value::<crate::records::feature::DesignRectangularPatternInstances>(
            empty_component
        )
        .unwrap_err()
        .to_string()
        .contains("seed")
    );
}

#[test]
fn edge_flange_rows_preserve_wire_and_reject_parallel_mismatch() {
    for count in [0, 1, 3] {
        let wire = crate::records::feature::DesignEdgeFlangeOperationSerde {
            edge_wrapper_record_indices: (0..count).map(|index| 100 + index).collect(),
            edge_group_record_indices: (0..count).map(|index| 200 + index).collect(),
            edge_operand_record_indices: (0..count).map(|index| 203 + index).collect(),
            aggregate_group_record_index: 300,
            aggregate_operand_record_indices: (0..count).map(|index| 303 + index).collect(),
            height_owner_record_index: 400,
            height_extent: crate::records::feature::DesignEdgeFlangeHeightExtent::Distance,
            angle_owner_record_index: 401,
            width_mode: Some(crate::records::feature::DesignEdgeWidthMode::FullEdge),
            width_distance_owner_record_indices: Vec::new(),
            width_distance_owner_record_indices_by_edge: Vec::new(),
            auxiliary_reference_record_indices: Vec::new(),
            width_parameter_source:
                crate::records::feature::DesignEdgeFlangeWidthParameterSource::EdgeWidth,
            settings_record_index: 402,
            bend_radius: 0.25,
            bend_radius_offset: 500,
            reference_side_code: 4,
            height_datum: crate::records::feature::DesignSheetMetalHeightDatum::InnerFaces,
            bend_position: crate::records::feature::DesignBendPosition::Adjacent,
        };
        let expected = serde_json::to_string(&wire).unwrap();
        let native: crate::records::feature::DesignEdgeFlangeOperation =
            serde_json::from_str(&expected).unwrap();
        assert_eq!(native.shape.edges().count(), count as usize);
        assert_eq!(serde_json::to_string(&native).unwrap(), expected);
        for radius in [0.0, -1.0, f64::INFINITY, f64::NAN] {
            let mut invalid = wire.clone();
            invalid.bend_radius = radius;
            let error = crate::records::feature::DesignEdgeFlangeOperation::try_from(invalid)
                .expect_err("invalid bend radius");
            assert!(error.contains("bend_radius"));
        }
        for mode in [
            crate::records::feature::DesignEdgeWidthMode::Symmetric,
            crate::records::feature::DesignEdgeWidthMode::TwoSides,
            crate::records::feature::DesignEdgeWidthMode::SymmetricPerEdge,
            crate::records::feature::DesignEdgeWidthMode::TwoSidesPerEdge,
        ] {
            if count == 0
                && matches!(
                    mode,
                    crate::records::feature::DesignEdgeWidthMode::SymmetricPerEdge
                        | crate::records::feature::DesignEdgeWidthMode::TwoSidesPerEdge
                )
            {
                continue;
            }
            let mut width_wire = wire.clone();
            width_wire.width_mode = Some(mode);
            width_wire.width_distance_owner_record_indices = match mode {
                crate::records::feature::DesignEdgeWidthMode::Symmetric => vec![600],
                crate::records::feature::DesignEdgeWidthMode::TwoSides => vec![600, 601],
                crate::records::feature::DesignEdgeWidthMode::SymmetricPerEdge => {
                    (0..count).map(|index| 600 + index).collect()
                }
                crate::records::feature::DesignEdgeWidthMode::TwoSidesPerEdge => {
                    (0..2 * count).map(|index| 600 + index).collect()
                }
                crate::records::feature::DesignEdgeWidthMode::FullEdge => Vec::new(),
            };
            if mode == crate::records::feature::DesignEdgeWidthMode::TwoSidesPerEdge {
                width_wire.width_distance_owner_record_indices_by_edge = (0..count)
                    .map(|index| [600 + 2 * index, 601 + 2 * index])
                    .collect();
            }
            for source in [
                crate::records::feature::DesignEdgeFlangeWidthParameterSource::EdgeWidth,
                crate::records::feature::DesignEdgeFlangeWidthParameterSource::EdgeOffset,
            ] {
                width_wire.width_parameter_source = source;
                let expected = serde_json::to_string(&width_wire).unwrap();
                let decoded = serde_json::from_str::<
                    crate::records::feature::DesignEdgeFlangeOperation,
                >(&expected);
                if source
                    == crate::records::feature::DesignEdgeFlangeWidthParameterSource::EdgeOffset
                    && mode != crate::records::feature::DesignEdgeWidthMode::TwoSidesPerEdge
                {
                    assert!(decoded
                        .unwrap_err()
                        .to_string()
                        .contains("width_parameter_source"));
                } else {
                    assert_eq!(serde_json::to_string(&decoded.unwrap()).unwrap(), expected);
                }
            }
            width_wire.width_parameter_source =
                crate::records::feature::DesignEdgeFlangeWidthParameterSource::EdgeWidth;
            if matches!(
                mode,
                crate::records::feature::DesignEdgeWidthMode::SymmetricPerEdge
                    | crate::records::feature::DesignEdgeWidthMode::TwoSidesPerEdge
            ) {
                let mut invalid = width_wire.clone();
                invalid.width_distance_owner_record_indices.push(999);
                if mode == crate::records::feature::DesignEdgeWidthMode::TwoSidesPerEdge {
                    invalid.width_distance_owner_record_indices.push(1000);
                    invalid
                        .width_distance_owner_record_indices_by_edge
                        .push([999, 1000]);
                }
                assert!(
                    crate::records::feature::DesignEdgeFlangeOperation::try_from(invalid)
                        .unwrap_err()
                        .contains("selected edges")
                );
            }
            width_wire.height_extent =
                crate::records::feature::DesignEdgeFlangeHeightExtent::ToObject {
                    target_group_record_index: 700,
                    target_operand_record_index: 703,
                    offset_owner_record_index: 710,
                    reference_record_indices: [720, 721],
                };
            assert!(
                crate::records::feature::DesignEdgeFlangeOperation::try_from(width_wire)
                    .unwrap_err()
                    .contains("height_extent")
            );
        }
        let mut to_object = wire.clone();
        to_object.height_extent = crate::records::feature::DesignEdgeFlangeHeightExtent::ToObject {
            target_group_record_index: 700,
            target_operand_record_index: 703,
            offset_owner_record_index: 710,
            reference_record_indices: [720, 721],
        };
        let expected = serde_json::to_string(&to_object).unwrap();
        let native: crate::records::feature::DesignEdgeFlangeOperation =
            serde_json::from_str(&expected).unwrap();
        assert_eq!(serde_json::to_string(&native).unwrap(), expected);
        for field in [
            "edge_wrapper_record_indices",
            "edge_group_record_indices",
            "edge_operand_record_indices",
            "aggregate_operand_record_indices",
        ] {
            let mut invalid = serde_json::to_value(&wire).unwrap();
            invalid[field]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!(999));
            assert!(
                serde_json::from_value::<crate::records::feature::DesignEdgeFlangeOperation>(
                    invalid
                )
                .unwrap_err()
                .to_string()
                .contains(field)
            );
        }
    }
}

#[test]
fn scope_reference_runs_preserve_wire_and_reject_partial_locations() {
    let empty = serde_json::to_string(&DesignParameterScope::empty(
        "scope",
        DesignFeatureKind::Sketch,
        1,
    ))
    .unwrap();
    for (values, offsets) in [
        ("[]", "[]"),
        ("[10]", "[]"),
        ("[10]", "[0]"),
        ("[10,20,30]", "[]"),
        ("[10,20,30]", "[0,11,22]"),
    ] {
        let wire = empty
            .replace(
                "\"reference_members\":[]",
                &format!("\"reference_members\":{values}"),
            )
            .replace(
                "\"reference_member_offsets\":[]",
                &format!("\"reference_member_offsets\":{offsets}"),
            );
        let scope: DesignParameterScope = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&scope).unwrap(), wire);
        assert_eq!(
            scope.reference_members.values().len(),
            scope.reference_members.len()
        );
        assert_eq!(
            scope
                .reference_members
                .values_in(0..scope.reference_members.len())
                .unwrap()
                .copied()
                .collect::<Vec<_>>(),
            serde_json::from_str::<Vec<u32>>(values).unwrap()
        );
        assert!(scope
            .reference_members
            .values_in(0..scope.reference_members.len() + 1)
            .is_none());
    }
    for (values, offsets) in [("[]", "[0]"), ("[10]", "[0,11]"), ("[10,20,30]", "[0,11]")] {
        let wire = empty
            .replace(
                "\"reference_members\":[]",
                &format!("\"reference_members\":{values}"),
            )
            .replace(
                "\"reference_member_offsets\":[]",
                &format!("\"reference_member_offsets\":{offsets}"),
            );
        let error = serde_json::from_str::<DesignParameterScope>(&wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("reference_members"));
        assert!(error.contains("reference_member_offsets"));
    }
}

#[test]
fn fixed_fillet_law_wire_preserves_scalar_order_and_rejects_partial_lanes() {
    for radius_count in [1_u32, 2, 3, 5] {
        let intermediate_count = radius_count.saturating_sub(2);
        let mut wire = serde_json::json!({
            "radii": (0..radius_count).map(|index| f64::from(index + 1)).collect::<Vec<_>>(),
            "radius_record_indexes": (0..radius_count).map(|index| 10 + index).collect::<Vec<_>>(),
            "radius_offsets": (0..radius_count).map(|index| 100 + index * 8).collect::<Vec<_>>()
        });
        if intermediate_count != 0 {
            wire["intermediate_parameters"] = serde_json::json!((0..intermediate_count)
                .map(|index| f64::from(index + 1) / 4.0)
                .collect::<Vec<_>>());
            wire["intermediate_parameter_record_indexes"] = serde_json::json!((0
                ..intermediate_count)
                .map(|index| 20 + index)
                .collect::<Vec<_>>());
            wire["intermediate_parameter_offsets"] = serde_json::json!((0..intermediate_count)
                .map(|index| 200 + index * 8)
                .collect::<Vec<_>>());
        }
        for with_tangency in [false, true] {
            let mut wire = wire.clone();
            if with_tangency {
                wire["tangency_weight"] =
                    serde_json::json!({ "value": 1.0, "record_index": 5, "value_offset": 50 });
            }
            let group: crate::records::feature::DesignFixedFilletGroup =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(group.law.radii().count(), radius_count as usize);
            assert_eq!(group.law.intermediate().len(), intermediate_count as usize);
            assert_eq!(serde_json::to_value(&group).unwrap(), wire);
            for field in [
                "radii",
                "radius_record_indexes",
                "radius_offsets",
                "intermediate_parameters",
                "intermediate_parameter_record_indexes",
                "intermediate_parameter_offsets",
            ] {
                let mut invalid = wire.clone();
                let mut column = invalid
                    .get(field)
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                column.push(serde_json::json!(1));
                invalid[field] = serde_json::Value::Array(column);
                assert!(
                    serde_json::from_value::<crate::records::feature::DesignFixedFilletGroup>(
                        invalid
                    )
                    .unwrap_err()
                    .to_string()
                    .contains(field)
                );
            }
            let mut invalid = wire;
            for field in ["radii", "radius_record_indexes", "radius_offsets"] {
                invalid[field] = serde_json::json!([]);
            }
            assert!(
                serde_json::from_value::<crate::records::feature::DesignFixedFilletGroup>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("radii")
            );
        }
    }
    for radii in [vec![1.0], vec![1.0, 2.0, 3.0]] {
        let wire = serde_json::json!({
            "radius_record_indexes": vec![10; radii.len()], "radius_offsets": vec![100; radii.len()], "radii": radii,
            "intermediate_parameters": [0.25, 0.5], "intermediate_parameter_record_indexes": [20, 21], "intermediate_parameter_offsets": [200, 208]
        });
        assert!(
            serde_json::from_value::<crate::records::feature::DesignFixedFilletGroup>(wire)
                .unwrap_err()
                .to_string()
                .contains("intermediate_parameters")
        );
    }
}

#[test]
fn circular_pattern_axis_wire_preserves_shared_identity_and_rejects_partial_rows() {
    let inline = serde_json::json!({
        "kind": "inline", "origin": [1.0, 2.0, 3.0], "origin_offset": 12,
        "direction": [0.0, 0.0, 1.0], "direction_offset": 36
    });
    let axis: crate::records::feature::DesignCircularPatternAxis =
        serde_json::from_value(inline.clone()).unwrap();
    assert_eq!(serde_json::to_value(axis).unwrap(), inline);
    for count in [1_u32, 2] {
        for resolved in [false, true] {
            let mut wire = serde_json::json!({
                "kind": "historical_edge",
                "wrapper_record_indices": (0..count).map(|index| 10 + index).collect::<Vec<_>>(),
                "persistent_identities": [17],
                "identity_offsets": (0..count).map(|index| 100 + index * 8).collect::<Vec<_>>()
            });
            if resolved {
                wire["resolved_origin"] =
                    serde_json::to_value(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap();
                wire["resolved_direction"] =
                    serde_json::to_value(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap();
            }
            let axis: crate::records::feature::DesignCircularPatternAxis =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(axis).unwrap(), wire);
            for field in [
                "wrapper_record_indices",
                "identity_offsets",
                "persistent_identities",
            ] {
                let mut invalid = wire.clone();
                invalid[field]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!(99));
                assert!(
                    serde_json::from_value::<crate::records::feature::DesignCircularPatternAxis>(
                        invalid
                    )
                    .unwrap_err()
                    .to_string()
                    .contains(field)
                );
            }
            let mut invalid = wire.clone();
            invalid["persistent_identities"] = serde_json::json!([]);
            assert!(
                serde_json::from_value::<crate::records::feature::DesignCircularPatternAxis>(
                    invalid
                )
                .unwrap_err()
                .to_string()
                .contains("persistent_identities")
            );
            for field in ["resolved_origin", "resolved_direction"] {
                let mut invalid = wire.clone();
                invalid.as_object_mut().unwrap().remove("resolved_origin");
                invalid
                    .as_object_mut()
                    .unwrap()
                    .remove("resolved_direction");
                invalid[field] =
                    serde_json::to_value(cadmpeg_ir::math::Point3::new(0.0, 0.0, 1.0)).unwrap();
                let error = serde_json::from_value::<
                    crate::records::feature::DesignCircularPatternAxis,
                >(invalid)
                .unwrap_err()
                .to_string();
                assert!(error.contains("resolved_origin"));
                assert!(error.contains("resolved_direction"));
            }
        }
    }
}

#[test]
fn mirror_plane_wire_rejects_partial_placement() {
    let prefix = r#"{"count":2,"count_record_index":11,"count_offset":0,"stitch_tolerance":0.001,"stitch_tolerance_record_index":12,"stitch_tolerance_offset":0,"seed_group_record_index":20,"plane_group_record_index":30"#;
    let origin = serde_json::to_string(&cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap();
    let normal = serde_json::to_string(&cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap();
    for fields in [
        String::new(),
        format!(",\"plane_origin\":{origin},\"plane_normal\":{normal}"),
    ] {
        let wire = format!("{prefix}{fields}}}");
        let construction: crate::records::feature::DesignMirrorConstruction =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&construction).unwrap(), wire);
    }
    for (field, value) in [("plane_origin", origin), ("plane_normal", normal)] {
        let invalid = format!("{prefix},\"{field}\":{value}}}");
        let error =
            serde_json::from_str::<crate::records::feature::DesignMirrorConstruction>(&invalid)
                .unwrap_err()
                .to_string();
        assert!(error.contains("plane_origin"));
        assert!(error.contains("plane_normal"));
    }
}

#[test]
fn coil_selection_preserves_wire_and_rejects_dependent_fields_without_identity() {
    let persistent = r#"{"kind":"persistent","asset_id":"00000001-1111-4111-8111-111111111111","context_id":"00000002-1111-4111-8111-111111111111","identity_record_index":3,"primary_identity":7"#;
    for fields in [
        "",
        ",\"secondary_identity\":11",
        ",\"secondary_identity\":11,\"curve_secondary_identity\":0",
        ",\"secondary_identity\":11,\"curve_secondary_identity\":13",
    ] {
        let wire = format!("{persistent}{fields}}}");
        let selection: crate::records::feature::DesignCoilSelection =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&selection).unwrap(), wire);
    }
    let wire = format!("{persistent},\"curve_secondary_identity\":13}}");
    let error = serde_json::from_str::<crate::records::feature::DesignCoilSelection>(&wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("secondary_identity"));
    assert!(error.contains("curve_secondary_identity"));

    let face = r#"{"kind":"face_recipe","asset_id":"00000001-1111-4111-8111-111111111111","context_id":"00000002-1111-4111-8111-111111111111","recipe_record_index":3,"recipe_record_byte_offset":40,"recipe_id":"recipe","recipe_kind":"#;
    for kind in ["face", "bounded_face"] {
        for fields in [
            "",
            ",\"design_id\":\"body\"",
            ",\"design_id\":\"body\",\"design_selector\":{\"value\":2,\"byte_offset\":60}",
        ] {
            let wire = format!("{face}\"{kind}\"{fields}}}");
            let selection: crate::records::feature::DesignCoilSelection =
                serde_json::from_str(&wire).unwrap();
            assert_eq!(serde_json::to_string(&selection).unwrap(), wire);
        }
    }
    let wire = format!("{face}\"face\",\"design_selector\":{{\"value\":2,\"byte_offset\":60}}}}");
    let error = serde_json::from_str::<crate::records::feature::DesignCoilSelection>(&wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("design_id"));
    assert!(error.contains("design_selector"));
    for kind in ["body", "edge", "vertex"] {
        let wire = format!("{face}\"{kind}\"}}");
        let error = serde_json::from_str::<crate::records::feature::DesignCoilSelection>(&wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("recipe_kind"));
    }
}

#[test]
fn legacy_extrude_constants_and_geometry_preserve_wire() {
    for geometry_kind in [0, 1] {
        let wire = format!(
            r#"{{"layout":"legacy_distance","prefix_value":0,"prefix_value_offset":21,"operation":"join","operation_offset":25,"extent_kind":2,"extent_kind_offset":29,"direction_reversed":false,"direction_reversed_offset":33,"geometry_kind":{geometry_kind},"geometry_kind_offset":34}}"#
        );
        let prologue: crate::records::feature::DesignExtrudePrologue =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&prologue).unwrap(), wire);
        assert_eq!(
            prologue.extent(),
            Some(crate::records::feature::DesignExtrudeExtent::OneSidedDistance)
        );
        assert_eq!(prologue.solid_operation(), geometry_kind == 1);
        for (field, before, after) in [
            (
                "prefix_value",
                "\"prefix_value\":0".to_owned(),
                "\"prefix_value\":1".to_owned(),
            ),
            (
                "extent_kind",
                "\"extent_kind\":2".to_owned(),
                "\"extent_kind\":1".to_owned(),
            ),
            (
                "geometry_kind",
                format!("\"geometry_kind\":{geometry_kind}"),
                "\"geometry_kind\":2".to_owned(),
            ),
        ] {
            let invalid = wire.replace(&before, &after);
            let error =
                serde_json::from_str::<crate::records::feature::DesignExtrudePrologue>(&invalid)
                    .unwrap_err()
                    .to_string();
            assert!(error.contains(field));
        }
    }
}

#[test]
fn coil_placement_derives_only_the_encoded_identity_matrix() {
    let prefix = r#"{"selection_record_index":1,"selection_record_byte_offset":0,"selection_class_tag":"353","selection":{"kind":"persistent","asset_id":"00000001-1111-4111-8111-111111111111","context_id":"00000002-1111-4111-8111-111111111111","identity_record_index":2,"primary_identity":3},"transform_record_index":4,"transform_record_byte_offset":5,"transform_class_tag":"450","transform":"#;
    let identity = "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]";
    let translated = "[[1.0,0.0,0.0,2.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]";
    for (matrix, offset) in [
        (identity, ""),
        (identity, ",\"transform_offset\":55"),
        (translated, ",\"transform_offset\":55"),
    ] {
        let wire = format!("{prefix}{matrix}{offset}}}");
        let placement: crate::records::feature::DesignCoilPlacement =
            serde_json::from_str(&wire).expect("coil placement");
        assert_eq!(
            serde_json::to_string(&placement).expect("coil placement wire"),
            wire
        );
        assert_eq!(placement.explicit_transform.is_some(), !offset.is_empty());
    }
    let error = serde_json::from_str::<crate::records::feature::DesignCoilPlacement>(&format!(
        "{prefix}{translated}}}"
    ))
    .expect_err("matrix requires its location");
    assert!(error.to_string().contains("transform"));
    assert!(error.to_string().contains("transform_offset"));
}

#[test]
fn move_form_preserves_its_closed_integer_wire_domain() {
    for code in [1, 5] {
        let wire = code.to_string();
        let form: crate::records::feature::DesignMoveForm =
            serde_json::from_str(&wire).expect("move form");
        assert_eq!(serde_json::to_string(&form).expect("move form wire"), wire);
    }
    for code in [0, 2, 3, 4, 6, u32::MAX] {
        let error =
            serde_json::from_str::<crate::records::feature::DesignMoveForm>(&code.to_string())
                .expect_err("invalid move form");
        assert!(error.to_string().contains("form"));
    }
}

#[test]
fn mirror_scope_tolerance_pairs_repeated_markers_with_their_locations() {
    for (marker, repeated) in [(61, None), (89, Some(59)), (94, Some(58)), (100, Some(59))] {
        let offset = repeated.map_or_else(String::new, |offset| {
            format!(",\"repeated_marker_offset\":{offset}")
        });
        let wire = format!("{{\"marker\":{marker},\"marker_offset\":47{offset},\"first_reference\":12,\"first_reference_offset\":63,\"second_reference\":11,\"second_reference_offset\":76}}");
        let lane: crate::records::feature::DesignMirrorScopeTolerance =
            serde_json::from_str(&wire).expect("mirror scalar lane");
        assert_eq!(
            serde_json::to_string(&lane).expect("mirror scalar lane wire"),
            wire
        );
        assert_eq!(lane.marker.code(), marker);
    }
    for (marker, repeated) in [
        (0, None),
        (89, None),
        (94, None),
        (100, None),
        (61, Some(59)),
        (90, Some(59)),
    ] {
        let offset = repeated.map_or_else(String::new, |offset| {
            format!(",\"repeated_marker_offset\":{offset}")
        });
        let wire = format!("{{\"marker\":{marker},\"marker_offset\":47{offset},\"first_reference\":12,\"first_reference_offset\":63,\"second_reference\":11,\"second_reference_offset\":76}}");
        let error =
            serde_json::from_str::<crate::records::feature::DesignMirrorScopeTolerance>(&wire)
                .expect_err("invalid mirror scalar lane");
        assert!(error.to_string().contains("marker"));
        assert!(error.to_string().contains("repeated_marker_offset"));
    }
}

#[test]
fn mirror_derives_count_and_requires_one_tolerance_carrier() {
    let wire = r#"{"count":2,"count_record_index":11,"count_offset":0,"stitch_tolerance":0.001,"stitch_tolerance_offset":51,"stitch_tolerance_scope":{"marker":89,"marker_offset":47,"repeated_marker_offset":59,"first_reference":12,"first_reference_offset":63,"second_reference":11,"second_reference_offset":76},"seed_group_record_index":20,"plane_group_record_index":30}"#;
    let construction: crate::records::feature::DesignMirrorConstruction =
        serde_json::from_str(wire).expect("inline mirror tolerance");
    assert_eq!(
        serde_json::to_string(&construction).expect("inline mirror tolerance wire"),
        wire
    );
    let source: serde_json::Value = serde_json::from_str(wire).expect("mirror wire");
    for count in [0, 1, 3, u32::MAX] {
        let mut invalid = source.clone();
        invalid["count"] = count.into();
        let error =
            serde_json::from_value::<crate::records::feature::DesignMirrorConstruction>(invalid)
                .expect_err("fixed mirror count");
        assert!(error.to_string().contains("count"));
    }
    for present in [false, true] {
        let mut invalid = source.clone();
        if present {
            invalid["stitch_tolerance_record_index"] = 12.into();
        } else {
            invalid
                .as_object_mut()
                .expect("mirror object")
                .remove("stitch_tolerance_scope");
        }
        let error =
            serde_json::from_value::<crate::records::feature::DesignMirrorConstruction>(invalid)
                .expect_err("one mirror tolerance carrier");
        assert!(error.to_string().contains("stitch_tolerance_record_index"));
        assert!(error.to_string().contains("stitch_tolerance_scope"));
    }
}

#[test]
fn combine_requires_boolean_operation_local_target_and_nonempty_tools() {
    for operation in ["join", "cut", "intersect"] {
        for tools in [
            r#"[{"record_index":2}]"#,
            r#"[{"record_index":2},{"record_index":3}]"#,
        ] {
            let wire = format!("{{\"form\":\"standard\",\"operation\":\"{operation}\",\"operation_offset\":20,\"keep_tools\":false,\"keep_tools_offset\":25,\"target\":{{\"record_index\":1}},\"tools\":{tools}}}");
            let combine: crate::records::feature::DesignCombineOperation =
                serde_json::from_str(&wire).expect("combine operation");
            assert_eq!(
                serde_json::to_string(&combine).expect("combine operation wire"),
                wire
            );
        }
    }
    let base = serde_json::json!({
        "form": "standard", "operation": "join", "operation_offset": 20,
        "keep_tools": false, "keep_tools_offset": 25,
        "target": {"record_index": 1}, "tools": [{"record_index": 2}]
    });
    for (field, value) in [
        ("operation", serde_json::json!("new_body")),
        ("tools", serde_json::json!([])),
    ] {
        let mut invalid = base.clone();
        invalid[field] = value;
        let error =
            serde_json::from_value::<crate::records::feature::DesignCombineOperation>(invalid)
                .expect_err("invalid combine form");
        assert!(error.to_string().contains(field));
    }
    let mut external_target = base;
    external_target["target"]["external_identity"] = serde_json::json!({
        "selector_asset_id": "00000004-1111-4111-8111-111111111111", "selector_asset_id_offset": 0,
        "selector_context_id": "00000005-1111-4111-8111-111111111111", "selector_context_id_offset": 0,
        "occurrence_reference": 1, "occurrence_reference_offset": 0,
        "external_body_reference": 2, "external_body_reference_offset": 0,
        "external_segment": 1, "external_segment_offset": 0,
        "external_asset_id": "00000006-1111-4111-8111-111111111111", "external_asset_id_offset": 0,
        "external_link_name": "link", "external_link_name_offset": 0
    });
    let error =
        serde_json::from_value::<crate::records::feature::DesignCombineOperation>(external_target)
            .expect_err("local combine target");
    assert!(error.to_string().contains("target.external_identity"));
}

#[test]
fn thread_nominal_size_preserves_spelling_and_derives_numeric_wire_value() {
    let wire = |text: &str, number: &str| {
        format!("{{\"form\":\"standard\",\"designation_offset\":38,\"designation\":\"M1\",\"nominal_size_text\":\"{text}\",\"nominal_size\":{number},\"profile\":\"ISO Metric profile\",\"major_diameter\":1.0,\"minor_diameter\":0.5,\"pitch\":0.1,\"pitch_diameter\":0.75,\"face_group_record_indices\":[10]}}")
    };
    for (text, number) in [
        ("1.0", "1.0"),
        ("+1.00", "1.0"),
        ("1.25e1", "12.5"),
        ("0.125", "0.125"),
    ] {
        let json = wire(text, number);
        let thread: crate::records::feature::DesignThreadConstruction =
            serde_json::from_str(&json).expect("thread nominal size");
        assert_eq!(thread.nominal_size.text(), text);
        assert_eq!(
            serde_json::to_string(&thread).expect("thread nominal-size wire"),
            json
        );
    }
    for text in ["", "-", "0", "-0.0", "-1", "NaN", "inf", "1e9999"] {
        let error = serde_json::from_str::<crate::records::feature::DesignThreadConstruction>(
            &wire(text, "1.0"),
        )
        .expect_err("invalid nominal-size spelling");
        assert!(error.to_string().contains("nominal_size_text"));
    }
    let error = serde_json::from_str::<crate::records::feature::DesignThreadConstruction>(&wire(
        "1.0", "2.0",
    ))
    .expect_err("derived nominal size");
    assert!(error.to_string().contains("nominal_size"));
    assert!(error.to_string().contains("nominal_size_text"));
    let compact = wire("1.0", "1.0").replace("\"standard\"", "\"compact\"");
    for index in [1, u32::MAX] {
        let json = compact.replace("\"face_group_record_indices\"", &format!("\"trailing_reference_record_index\":{index},\"trailing_reference_offset\":100,\"face_group_record_indices\""));
        let thread: crate::records::feature::DesignThreadConstruction =
            serde_json::from_str(&json).expect("compact trailer reference");
        assert_eq!(
            serde_json::to_string(&thread).expect("compact trailer wire"),
            json
        );
    }
    let invalid = compact.replace("\"face_group_record_indices\"", "\"trailing_reference_record_index\":0,\"trailing_reference_offset\":100,\"face_group_record_indices\"");
    let error = serde_json::from_str::<crate::records::feature::DesignThreadConstruction>(&invalid)
        .expect_err("nonzero compact trailer reference");
    assert!(error
        .to_string()
        .contains("trailing_reference_record_index"));
}

#[test]
fn vertex_recipe_resolution_preserves_wire_and_rejects_partial_pairs() {
    use crate::records::feature::{
        DesignVertexRecipe, DesignVertexResolution, DesignWorkPlaneConstruction,
    };

    let base = serde_json::json!({
        "record_index": 2, "byte_offset": 10, "class_tag": "369",
        "paired_byte_offset": 20, "paired_class_tag": "261",
        "recipe_record_index": 5, "recipe_record_byte_offset": 30,
        "recipe_id": "vertex", "recipe_prefix_offset": 41,
        "recipe_prefix_bytes": "AP8=", "recipe_references": [],
        "recipe_program_offset": 43, "recipe_program": [0],
        "next_record_index": 7, "next_byte_offset": 50
    });
    for resolution in [None, Some((i64::MIN, 0)), Some((i64::MAX, i64::MAX))] {
        let mut wire = base.clone();
        if let Some((state, slot)) = resolution {
            wire["recipe_state_id"] = state.into();
            wire["resolved_vertex_slot"] = slot.into();
        }
        let decoded: DesignVertexRecipe =
            serde_json::from_value(wire.clone()).expect("valid recipe");
        assert_eq!(
            serde_json::to_value(&decoded).expect("serialize recipe"),
            wire
        );
        assert_eq!(
            decoded
                .resolution
                .map(|value| (value.state_id, value.vertex_slot())),
            resolution
        );
        let plane_wire = serde_json::json!({
            "kind": "three_point", "placement_record_index": 9,
            "inputs": [wire.clone(), wire.clone(), wire]
        });
        let plane: DesignWorkPlaneConstruction =
            serde_json::from_value(plane_wire.clone()).expect("three-point plane");
        assert_eq!(
            serde_json::to_value(&plane).expect("serialize plane"),
            plane_wire
        );
        let mut wrong_kind = plane_wire.clone();
        wrong_kind["kind"] = "two_point".into();
        assert!(
            serde_json::from_value::<DesignWorkPlaneConstruction>(wrong_kind).is_err(),
            "unknown work-plane construction kind"
        );
    }
    for (state, slot) in [(Some(4), None), (None, Some(0)), (Some(4), Some(-1))] {
        let mut wire = base.clone();
        if let Some(state) = state {
            wire["recipe_state_id"] = state.into();
        }
        if let Some(slot) = slot {
            wire["resolved_vertex_slot"] = slot.into();
        }
        let error =
            serde_json::from_value::<DesignVertexRecipe>(wire).expect_err("invalid resolution");
        assert!(error.to_string().contains("resolved_vertex_slot"));
    }
    assert!(DesignVertexResolution::new(4, -1).is_none());
}

#[test]
fn work_point_rules_preserve_supported_and_native_forms_without_aliases() {
    use crate::records::feature::DesignWorkPointRule;

    let input = serde_json::json!({"record_index": 2, "reference_offset": 10});
    for (kind, code, arity) in [
        ("circle_center", 5, 1),
        ("two_edge_intersection", 7, 2),
        ("three_plane_intersection", 8, 3),
        ("vertex", 10, 1),
        ("edge_plane_intersection", 14, 2),
        ("distance_on_edge", 20, 1),
    ] {
        let inputs = (0..arity).map(|_| input.clone()).collect::<Vec<_>>();
        let mut wire = serde_json::json!({"kind": kind});
        if arity == 1 {
            wire["input"] = input.clone();
        } else {
            wire["inputs"] = serde_json::json!(inputs);
        }
        let rule: DesignWorkPointRule =
            serde_json::from_value(wire.clone()).expect("supported rule");
        assert_eq!(rule.reference_type(), code);
        assert_eq!(serde_json::to_value(rule).expect("serialize rule"), wire);
        let alias = serde_json::json!({"kind": "native", "reference_type": code, "inputs": inputs});
        let error = serde_json::from_value::<DesignWorkPointRule>(alias)
            .expect_err("native alias rejected");
        assert!(error.to_string().contains("reference_type"));
    }
    for (code, inputs) in [
        (0, vec![]),
        (u32::MAX, vec![input.clone()]),
        (5, vec![input.clone(), input]),
    ] {
        let wire = serde_json::json!({"kind": "native", "reference_type": code, "inputs": inputs});
        let rule: DesignWorkPointRule =
            serde_json::from_value(wire.clone()).expect("unassigned native form");
        assert_eq!(
            serde_json::to_value(rule).expect("serialize native form"),
            wire
        );
    }
}

#[test]
fn feature_kind_classifies_nonempty_native_names_at_construction() {
    for name in [
        "Fillet",
        "Esboço",
        "Extrusão",
        "Thread",
        "Unsupported",
        "fillet",
        " ",
    ] {
        let kind = DesignFeatureKind::try_from(name.to_owned()).expect("nonempty name");
        assert_eq!(kind.as_str(), name);
        assert_eq!(
            serde_json::to_value(&kind).expect("serialize kind"),
            serde_json::json!(name)
        );
        let decoded: DesignFeatureKind =
            serde_json::from_value(serde_json::json!(name)).expect("decode kind");
        assert_eq!(decoded, kind);
        assert_eq!(
            matches!(kind, DesignFeatureKind::Native(_)),
            matches!(name, "Unsupported" | "fillet" | " ")
        );
    }
    assert!(DesignFeatureKind::try_from(String::new()).is_err());
    let error = serde_json::from_value::<DesignFeatureKind>(serde_json::json!(""))
        .expect_err("empty name rejected");
    assert!(error.to_string().contains("kind"));
}

#[test]
fn scope_feature_ordinal_preserves_positive_wire_values_and_rejects_zero() {
    let base = empty_scope(DesignFeatureKind::Fillet);
    for ordinal in [1, u32::MAX] {
        let mut wire = base.clone();
        wire["feature_ordinal"] = ordinal.into();
        let scope: DesignParameterScope =
            serde_json::from_value(wire.clone()).expect("positive ordinal");
        assert_eq!(scope.feature_ordinal.get(), ordinal);
        assert_eq!(serde_json::to_value(scope).expect("serialize scope"), wire);
    }
    let mut wire = base;
    wire["feature_ordinal"] = 0.into();
    let error =
        serde_json::from_value::<DesignParameterScope>(wire).expect_err("zero ordinal rejected");
    assert!(error.to_string().contains("feature_ordinal"));
}

#[test]
fn scope_history_state_offset_is_derived_and_wire_mismatches_are_rejected() {
    for kind_offset in [0_u64, 8, 100, u64::MAX] {
        let mut scope = DesignParameterScope::empty("scope", DesignFeatureKind::Sketch, 1);
        scope.kind_offset = kind_offset;
        let wire = serde_json::to_value(&scope).expect("serialize scope");
        assert_eq!(
            wire["history_state_id_offset"],
            kind_offset.saturating_sub(8)
        );
        let decoded: DesignParameterScope =
            serde_json::from_value(wire.clone()).expect("valid derived offset");
        assert_eq!(decoded, scope);
        assert_eq!(
            serde_json::to_value(decoded).expect("serialize scope"),
            wire
        );
        let mut bad = wire;
        bad["history_state_id_offset"] = (kind_offset.saturating_sub(8) + 1).into();
        let error = serde_json::from_value::<DesignParameterScope>(bad)
            .expect_err("mismatched offset rejected");
        assert!(error.to_string().contains("history_state_id_offset"));
    }
}

#[test]
fn bend_radius_requires_a_positive_finite_value() {
    for value in [0.0, -0.0, -1.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        assert!(crate::records::feature::DesignBendRadius::new(value).is_none());
    }
    for radius in [f64::MIN_POSITIVE, 0.25, f64::MAX] {
        let value =
            crate::records::feature::DesignBendRadius::new(radius).expect("positive finite radius");
        assert_eq!(value.get(), radius);
        let wire = serde_json::json!({
            "edge_wrapper_record_index": 1, "edge_group_record_index": 2,
            "edge_operand_record_index": 5, "aggregate_group_record_index": 6,
            "aggregate_operand_record_index": 9,
            "parameter_owners": {"kind": "gap_length", "gap_owner_record_index": 10, "length_owner_record_index": 11},
            "settings_record_index": 12, "bend_radius": radius, "bend_radius_offset": 100,
            "form_code": 3, "direction_code": 1, "direction_reversal_byte": 0,
            "reference_side_code": 4
        });
        let operation: crate::records::feature::DesignHemOperation =
            serde_json::from_value(wire.clone()).expect("valid radius");
        assert_eq!(operation.bend_radius.get(), radius);
        assert_eq!(
            serde_json::to_value(operation).expect("serialize hem"),
            wire
        );
        for invalid in [0.0, -1.0] {
            let mut bad = wire.clone();
            bad["bend_radius"] = invalid.into();
            let error = serde_json::from_value::<crate::records::feature::DesignHemOperation>(bad)
                .expect_err("invalid bend radius");
            assert!(error.to_string().contains("bend_radius"));
        }
    }
}

#[test]
fn rectangular_pattern_sidecar_rejects_invalid_axis_counts() {
    for (u_count, v_count, u_extent, v_extent) in [
        (0, 2, 0.0, 1.0),
        (2, 0, 1.0, 0.0),
        (1, 1, 0.0, 0.0),
        (1, 2, 1.0, 1.0),
        (2, 1, 0.0, 0.0),
        (2, 1, 1.0, 1.0),
        (1, 2, 0.0, 0.0),
    ] {
        let wire = serde_json::json!({
            "u_count": u_count, "v_count": v_count,
            "u_extent": u_extent, "v_extent": v_extent,
            "owner_record_indices": [1, 2, 3, 4],
            "value_offsets": [10, 20, 30, 40]
        });
        assert!(
            serde_json::from_value::<super::DesignRectangularPatternConstruction>(wire).is_err()
        );
    }
}

#[test]
fn rectangular_pattern_sidecar_preserves_signed_spans() {
    for (u_count, v_count, u_extent, v_extent) in [(3, 1, -10.0, 0.0), (2, 2, 1.0, -1.0)] {
        let wire = serde_json::json!({
            "u_count": u_count, "v_count": v_count,
            "u_extent": u_extent, "v_extent": v_extent,
            "owner_record_indices": [1, 2, 3, 4],
            "value_offsets": [10, 20, 30, 40]
        });
        let record: super::DesignRectangularPatternConstruction =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }
}

#[test]
fn surface_trim_sidecar_requires_nonempty_matching_cell_count() {
    let entry = serde_json::json!({"record_index": 4, "record_reference_offset": 0,
        "ordinal": 1, "ordinal_offset": 0});
    let mut wire = serde_json::json!({"id": "trim", "scope_record_index": 1,
        "selection_record_index": 2, "selection_byte_offset": 0,
        "selection_next_record_index": 3, "selection_next_byte_offset": 0,
        "chain_records": [
            {"record_index": 3, "byte_offset": 0, "class_tag": "288", "frame_length": 11},
            {"record_index": 6, "byte_offset": 11, "class_tag": "271", "frame_length": 11}
        ], "cell_table_record_index": 4, "cell_table_byte_offset": 0,
        "cell_table_class_tag": "325", "cell_table_frame_length": 0,
        "cell_table_paired_class_tag": "257", "cell_table_paired_byte_offset": 0,
        "cell_count": 1, "cell_count_offset": 0, "cell_entries": [entry],
        "trailing_value": 1, "trailing_value_offset": 0, "trailing_zero_offset": 0});
    let record: super::DesignSurfaceTrimOperation = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), wire);
    for count in [0, 1, 3] {
        let mut invalid_chain = wire.clone();
        invalid_chain["chain_records"] = serde_json::Value::Array(
            std::iter::repeat_n(wire["chain_records"][0].clone(), count).collect(),
        );
        assert!(
            serde_json::from_value::<super::DesignSurfaceTrimOperation>(invalid_chain).is_err()
        );
    }
    wire["cell_count"] = 2.into();
    assert!(serde_json::from_value::<super::DesignSurfaceTrimOperation>(wire.clone()).is_err());
    wire["cell_count"] = 0.into();
    wire["cell_entries"] = serde_json::json!([]);
    assert!(serde_json::from_value::<super::DesignSurfaceTrimOperation>(wire).is_err());
}
