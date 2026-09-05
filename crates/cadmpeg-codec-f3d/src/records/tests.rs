// SPDX-License-Identifier: Apache-2.0

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
        let decoded: DesignParameterScope = serde_json::from_value(wire.clone()).expect("complete frame");
        assert_eq!(serde_json::to_value(decoded).expect("serialize frame"), wire);
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
        assert_eq!(serde_json::to_string(&decoded).expect("serialize scope"), wire);
    }
}

#[test]
fn revolve_opposite_angle_preserves_wire_and_rejects_partial_source_location() {
    let base = r#"{"operation":"join","operation_offset":12,"angle":1.5,"angle_record_index":3,"angle_offset":40"#;
    for tail in ["}", ",\"opposite_angle_record_index\":4,\"opposite_angle_offset\":80}"] {
        let wire = format!("{base}{tail}");
        let value: super::DesignRevolveConstruction = serde_json::from_str(&wire).expect("revolve construction");
        assert_eq!(serde_json::to_string(&value).expect("revolve wire"), wire);
    }
    for tail in [",\"opposite_angle_record_index\":4}", ",\"opposite_angle_offset\":80}"] {
        let error = serde_json::from_str::<super::DesignRevolveConstruction>(&format!("{base}{tail}"))
            .expect_err("partial opposite angle location");
        assert!(error.to_string().contains("opposite_angle_record_index"));
        assert!(error.to_string().contains("opposite_angle_offset"));
    }
}

#[test]
fn parameter_discriminator_preserves_wire_and_rejects_partial_location() {
    let prefix = r#"{"id":"parameter","byte_offset":0,"class_tag":"123","record_index":1"#;
    let suffix = r#","source_ordinal":0,"owner_record_index":2,"expression":"1","expression_offset":40,"source_kind":"Distance","source_kind_offset":60,"kind":"feature","name":"d1","name_offset":80,"evaluated_value":1.0,"evaluated_value_offset":90}"#;
    for fields in ["", ",\"family_discriminator\":0,\"family_discriminator_offset\":22"] {
        let wire = format!("{prefix}{fields}{suffix}");
        let value: super::DesignParameter = serde_json::from_str(&wire).expect("parameter frame");
        assert_eq!(serde_json::to_string(&value).expect("parameter wire"), wire);
    }
    for fields in [",\"family_discriminator\":0", ",\"family_discriminator_offset\":22"] {
        let error = serde_json::from_str::<super::DesignParameter>(&format!("{prefix}{fields}{suffix}"))
            .expect_err("partial discriminator location");
        assert!(error.to_string().contains("family_discriminator"));
    }
}

#[test]
fn tracking_identities_preserve_wire_and_reject_partial_locations() {
    let prefix = r#"{"wrapper_record_index":300,"wrapper_byte_offset":0,"wrapper_class_tag":"361","carrier_record_index":301,"carrier_byte_offset":33,"carrier_class_tag":"362","primary_identity":268,"primary_identity_offset":70,"selector":-1,"selector_offset":90,"kind":3,"kind_offset":94"#;
    let suffix = r#","following_record_index":302,"following_byte_offset":130,"following_class_tag":"363"}"#;
    for fields in ["", ",\"first_related_identity\":113,\"first_related_identity_offset\":110,\"second_related_identity\":119,\"second_related_identity_offset\":122"] {
        let wire = format!("{prefix}{fields}{suffix}");
        let value: super::DesignConstructionTrackingPath = serde_json::from_str(&wire).expect("tracking path");
        assert_eq!(serde_json::to_string(&value).expect("tracking wire"), wire);
    }
    for field in ["first_related_identity", "first_related_identity_offset", "second_related_identity", "second_related_identity_offset"] {
        let error = serde_json::from_str::<super::DesignConstructionTrackingPath>(&format!("{prefix},\"{field}\":1{suffix}"))
            .expect_err("partial identity location");
        assert!(error.to_string().contains(field));
    }
}

#[test]
fn body_recipe_selector_tail_preserves_wire_and_rejects_partial_locations() {
    let prefix = r#"{"id":"operand","scope_record_index":1,"scope_reference_ordinal":0,"record_index":2,"byte_offset":0,"class_tag":"365","asset_id":"asset","asset_id_offset":100,"context_id":"context","context_id_offset":150"#;
    let suffix = r#","references":[],"nested_record_index":5,"nested_record_index_offset":80,"recipe_id":"recipe","next_record_index":6,"next_byte_offset":240}"#;
    for fields in ["", ",\"selector_tail\":[7,0,0,0],\"selector_tail_offset\":220"] {
        let wire = format!("{prefix}{fields}{suffix}");
        let value: super::DesignBodyRecipeOperand = serde_json::from_str(&wire).expect("body recipe operand");
        assert_eq!(serde_json::to_string(&value).expect("body recipe operand wire"), wire);
    }
    for fields in [",\"selector_tail\":[7,0,0,0]", ",\"selector_tail_offset\":220"] {
        let error = serde_json::from_str::<super::DesignBodyRecipeOperand>(&format!("{prefix}{fields}{suffix}"))
            .expect_err("partial selector tail location");
        assert!(error.to_string().contains("selector_tail"));
    }
}

#[test]
fn selection_secondary_identities_preserve_wire_and_reject_partial_locations() {
    let fields = r#""record_index":2,"byte_offset":0,"class_tag":"365","asset_id":"asset","asset_id_offset":100,"context_id":"context","context_id_offset":150,"identity_record_index":5,"identity_record_offset":180,"primary_identity":183,"primary_identity_offset":209"#;
    let suffix = r#","next_record_index":6,"next_byte_offset":225}"#;
    for prefix in ["{", "{\"id\":\"operand\",\"scope_record_index\":1,\"group_record_index\":2,\"group_member_ordinal\":0,"] {
        for identities in ["", ",\"secondary_identity\":249,\"secondary_identity_offset\":217", ",\"secondary_identity\":249,\"secondary_identity_offset\":217,\"curve_secondary_identity\":77,\"curve_secondary_identity_offset\":201"] {
            let wire = format!("{prefix}{fields}{identities}{suffix}");
            let encoded = if prefix == "{" {
                let value: super::DesignHoleFaceSelection = serde_json::from_str(&wire).expect("hole selection");
                serde_json::to_string(&value).expect("hole selection wire")
            } else {
                let value: super::DesignEntitySelectionOperand = serde_json::from_str(&wire).expect("entity selection");
                serde_json::to_string(&value).expect("entity selection wire")
            };
            assert_eq!(encoded, wire);
        }
        for field in ["secondary_identity", "secondary_identity_offset", "curve_secondary_identity", "curve_secondary_identity_offset"] {
            let wire = format!("{prefix}{fields},\"{field}\":1{suffix}");
            let error = if prefix == "{" {
                serde_json::from_str::<super::DesignHoleFaceSelection>(&wire).expect_err("partial hole selection identity").to_string()
            } else {
                serde_json::from_str::<super::DesignEntitySelectionOperand>(&wire).expect_err("partial entity selection identity").to_string()
            };
            assert!(error.contains(field));
        }
        let wire = format!("{prefix}{fields},\"curve_secondary_identity\":77,\"curve_secondary_identity_offset\":201{suffix}");
        let error = if prefix == "{" {
            serde_json::from_str::<super::DesignHoleFaceSelection>(&wire).unwrap_err().to_string()
        } else {
            serde_json::from_str::<super::DesignEntitySelectionOperand>(&wire).unwrap_err().to_string()
        };
        assert!(error.contains("secondary_identity"));
        assert!(error.contains("curve_secondary_identity"));
    }
}

#[test]
fn extrude_prefixes_preserve_wire_and_reject_partial_locations() {
    let reference = r#"{"record_index":2,"record_index_offset":26,"trailing_zero_count":7"#;
    for fields in ["", ",\"operation_prefix_marker\":1,\"operation_prefix_marker_offset\":37"] {
        let wire = format!("{reference}{fields}}}");
        let value: super::DesignExtrudePrologueReference = serde_json::from_str(&wire).expect("prologue reference");
        assert_eq!(serde_json::to_string(&value).expect("prologue reference wire"), wire);
    }
    for marker in [0, 2, u8::MAX] {
        let wire = format!("{reference},\"operation_prefix_marker\":{marker},\"operation_prefix_marker_offset\":37}}");
        let error = serde_json::from_str::<super::DesignExtrudePrologueReference>(&wire).expect_err("invalid prefix marker");
        assert!(error.to_string().contains("operation_prefix_marker"));
    }
    for field in ["operation_prefix_marker", "operation_prefix_marker_offset"] {
        let error = serde_json::from_str::<super::DesignExtrudePrologueReference>(&format!("{reference},\"{field}\":1}}"))
            .expect_err("partial prologue reference marker");
        assert!(error.to_string().contains(field));
    }
    for (layout, field, suffix) in [
        ("legacy_distance", "prefix_value", r#","operation":"join","operation_offset":25,"extent_kind":2,"extent_kind_offset":29,"direction_reversed":false,"direction_reversed_offset":33,"geometry_kind":1,"geometry_kind_offset":34}"#),
        ("legacy_shifted", "operation_prefix_marker", r#","operation":"join","operation_offset":28,"direction_face_extend_values":[1,0],"side_extent_discriminators":[1,0],"side_extent_discriminator_offsets":[105,109],"direction_face_extend_offsets":[32,36],"direction_reversed":false,"direction_reversed_offset":40,"solid_operation":true,"solid_operation_offset":41,"start":"profile_plane","start_offset":42}"#),
    ] {
        for value in [None, Some(if layout == "legacy_distance" { 0 } else { 1 })] {
            let fields = match value {
                Some(value) => format!(",\"{field}\":{value},\"{field}_offset\":21"),
                None if layout == "legacy_distance" => format!(",\"{field}\":null,\"{field}_offset\":null"),
                None => String::new(),
            };
            let wire = format!("{{\"layout\":\"{layout}\"{fields}{suffix}");
            let value: super::DesignExtrudePrologue = serde_json::from_str(&wire).expect("extrude prologue");
            assert_eq!(serde_json::to_string(&value).expect("extrude prologue wire"), wire);
        }
        for invalid in [2, u8::MAX] {
            let wire = format!("{{\"layout\":\"{layout}\",\"{field}\":{invalid},\"{field}_offset\":21{suffix}");
            let error = serde_json::from_str::<super::DesignExtrudePrologue>(&wire).expect_err("invalid prologue prefix");
            assert!(error.to_string().contains(field));
        }
        for partial_field in [field.to_owned(), format!("{field}_offset")] {
            let wire = format!("{{\"layout\":\"{layout}\",\"{partial_field}\":1{suffix}");
            let error = serde_json::from_str::<super::DesignExtrudePrologue>(&wire).expect_err("partial prologue prefix");
            assert!(error.to_string().contains(field));
        }
    }
}

#[test]
fn mirror_references_preserve_wire_and_reject_partial_locations() {
    let prefix = r#"{"count":2,"count_record_index":11,"count_offset":0,"stitch_tolerance":0.001,"stitch_tolerance_record_index":12,"stitch_tolerance_offset":0,"seed_group_record_index":20,"plane_group_record_index":30"#;
    for fields in ["", ",\"seed_feature_scope_record_index\":40,\"seed_feature_reference_offset\":100", ",\"plane_scope_record_index\":50,\"plane_reference_offset\":200", ",\"seed_feature_scope_record_index\":40,\"seed_feature_reference_offset\":100,\"plane_scope_record_index\":50,\"plane_reference_offset\":200"] {
        let wire = format!("{prefix}{fields}}}");
        let value: super::DesignMirrorConstruction = serde_json::from_str(&wire).expect("mirror construction");
        assert_eq!(serde_json::to_string(&value).expect("mirror wire"), wire);
    }
    for field in ["seed_feature_scope_record_index", "seed_feature_reference_offset", "plane_scope_record_index", "plane_reference_offset"] {
        let error = serde_json::from_str::<super::DesignMirrorConstruction>(&format!("{prefix},\"{field}\":1}}"))
            .expect_err("partial mirror reference");
        assert!(error.to_string().contains(field));
    }
}

#[test]
fn loft_trailing_scope_reference_preserves_wire_and_rejects_partial_locations() {
    let prefix = r#"{"id":"carrier","scope_record_index":12,"scope_reference_ordinal":0,"record_index":20,"byte_offset":0,"class_tag":"322","owner_scope_record_index":12,"owner_scope_record_index_offset":20,"members":[22],"member_offsets":[30],"member_count":1,"member_count_offset":26,"opaque_index":1,"opaque_index_offset":34,"opaque_scalar":1.0,"opaque_scalar_offset":38,"repeated_opaque_index":1,"repeated_opaque_index_offset":46,"next_next_record_index":22,"next_next_reference_offset":50,"flags":[0,0],"flags_offset":59,"next_record_index":21,"next_reference_offset":61"#;
    let suffix = r#","paired_class_tag":"262","paired_byte_offset":98}"#;
    for fields in ["", ",\"trailing_scope_record_index\":12,\"trailing_scope_reference_offset\":88"] {
        let wire = format!("{prefix}{fields}{suffix}");
        let value: super::DesignLoftLegacyBodyCarrier = serde_json::from_str(&wire).expect("loft carrier");
        assert_eq!(serde_json::to_string(&value).expect("loft carrier wire"), wire);
    }
    for field in ["trailing_scope_record_index", "trailing_scope_reference_offset"] {
        let error = serde_json::from_str::<super::DesignLoftLegacyBodyCarrier>(&format!("{prefix},\"{field}\":12{suffix}"))
            .expect_err("partial loft scope reference");
        assert!(error.to_string().contains(field));
    }
}

#[test]
fn external_version_identity_preserves_wire_and_rejects_partial_forms() {
    {
        let prefix = r#"{"axis_record_index":0,"axis_class_tag":"identity","axis_byte_offset":0,"axis_paired_class_tag":"identity","axis_paired_byte_offset":0,"selector_record_index":0,"selector_class_tag":"identity","selector_byte_offset":0,"selector_paired_class_tag":"identity","selector_paired_byte_offset":0,"nested_record_index":0,"nested_record_index_offset":0,"selector_asset_id":"identity","selector_asset_id_offset":0,"selector_context_id":"identity","selector_context_id_offset":0,"occurrence_reference":0,"occurrence_reference_offset":0,"external_object_reference":0,"external_object_reference_offset":0,"external_segment":0,"external_segment_offset":0,"external_asset_id":"identity","external_asset_id_offset":0,"external_link_name":"identity","external_link_name_offset":0"#;
        let suffix = r#","role_record_index":0,"role_class_tag":"identity","role_byte_offset":0,"occurrence_role":"identity","occurrence_role_offset":0}"#;
        let fields = [
            ("external_property_key", "\"key\""),
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
            let result = serde_json::from_str::<super::DesignAssemblyAxialSelectorIdentity>(&wire);
            if mask == 0 || mask == 15 {
                assert_eq!(serde_json::to_string(&result.expect("complete version form")).expect("version wire"), wire);
            } else {
                let error = result.expect_err("partial version identity").to_string();
                for (field, _) in fields {
                    assert!(error.contains(field));
                }
            }
        }
    }
    {
        let prefix = r#"{"selector_asset_id":"identity","selector_asset_id_offset":0,"selector_context_id":"identity","selector_context_id_offset":0,"occurrence_reference":0,"occurrence_reference_offset":0,"external_body_reference":0,"external_body_reference_offset":0,"external_segment":0,"external_segment_offset":0,"external_asset_id":"identity","external_asset_id_offset":0,"external_link_name":"identity","external_link_name_offset":0"#;
        let suffix = r#","tail_values":[0,0],"tail_value_offsets":[0,0]}"#;
        let fields = [
            ("external_property_key", "\"key\""),
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
            let result = serde_json::from_str::<super::DesignCombineExternalBodyIdentity>(&wire);
            if mask == 0 || mask == 15 {
                assert_eq!(serde_json::to_string(&result.expect("complete version form")).expect("version wire"), wire);
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
        let result = serde_json::from_str::<super::DesignHoleConstruction>(&wire);
        if mask == 0 || mask == 7 {
            assert_eq!(serde_json::to_string(&result.expect("complete tangent form")).expect("hole wire"), wire);
        } else {
            let error = result.expect_err("partial tangent form").to_string();
            for (field, _) in fields {
                assert!(error.contains(field));
            }
        }
    }
    for (indices, offsets) in [("[]", "[129]"), ("[378]", "[]"), ("[378,379]", "[129]")] {
        let wire = format!("{prefix},\"input_record_indices\":{indices},\"input_record_offsets\":{offsets}}}");
        let error = serde_json::from_str::<super::DesignHoleConstruction>(&wire).expect_err("unequal input arrays").to_string();
        assert!(error.contains("input_record_indices"));
        assert!(error.contains("input_record_offsets"));
    }
}

#[test]
fn construction_path_preserves_layout_wire_and_rejects_mixed_forms() {
    let prefix = r#"{"record_index":100,"byte_offset":0,"class_tag":"304","entity_ref":174,"entity_ref_offset":22"#;
    let suffix = r#","scope_record_index":90,"scope_record_index_offset":163,"nested_record_index":102,"nested_record_index_offset":174,"following_record_index":101,"following_byte_offset":190,"following_class_tag":"390"}"#;
    let fields = [
        ("transform", "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]"),
        ("transform_offset", "33"),
        ("compact_variant", "false"),
    ];
    for mask in 0..8 {
        let mut wire = prefix.to_owned();
        for (index, (field, value)) in fields.iter().enumerate() {
            if mask & (1 << index) != 0 {
                wire.push_str(&format!(",\"{field}\":{value}"));
            }
        }
        wire.push_str(suffix);
        let result = serde_json::from_str::<super::DesignConstructionOperandPath>(&wire);
        if mask == 3 || mask == 4 {
            assert_eq!(serde_json::to_string(&result.expect("complete placement form")).expect("path wire"), wire);
        } else {
            let error = result.expect_err("invalid placement form").to_string();
            for (field, _) in fields {
                assert!(error.contains(field));
            }
        }
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
            let parsed: super::DesignCoilScope = serde_json::from_str(&wire).expect("valid Coil field");
            assert_eq!(serde_json::to_string(&parsed).expect("Coil wire"), wire);
        }
        let wire = format!("{{\"{field}_offset\":30}}");
        let error = serde_json::from_str::<super::DesignCoilScope>(&wire)
            .expect_err("offset without value")
            .to_string();
        assert!(error.contains(field));
        assert!(error.contains(&format!("{field}_offset")));
    }
}

#[test]
fn material_assignment_preserves_located_and_authored_token_wire() {
    let prefix = r#"{"id":"material#0","asm_body_key":42,"asm_body_key_offset":10,"entity_suffix":985,"entity_suffix_offset":20,"entity_id":"0_985","entity_id_offset":30,"visual_guid":"Prism-001","visual_guid_offset":40"#;
    for field in ["physical_token", "visual_preset"] {
        for value in ["\"\"", "\"Prism-002\""] {
            for offset in [None, Some(0), Some(50)] {
                let mut wire = format!("{prefix},\"{field}\":{value}");
                if let Some(offset) = offset {
                    wire.push_str(&format!(",\"{field}_offset\":{offset}"));
                }
                wire.push('}');
                let parsed: super::DesignMaterialAssignment = serde_json::from_str(&wire).expect("material token");
                assert_eq!(serde_json::to_string(&parsed).expect("material wire"), wire);
            }
        }
        let wire = format!("{prefix},\"{field}_offset\":50}}");
        let error = serde_json::from_str::<super::DesignMaterialAssignment>(&wire)
            .expect_err("orphan material offset")
            .to_string();
        assert!(error.contains(field));
        assert!(error.contains(&format!("{field}_offset")));
    }
    let wire = format!("{prefix}}}");
    let parsed: super::DesignMaterialAssignment = serde_json::from_str(&wire).expect("absent tokens");
    assert_eq!(serde_json::to_string(&parsed).expect("material wire"), wire);
}

#[test]
fn recipe_design_id_preserves_source_and_authored_wire() {
    let prefix = r#"{"id":"recipe#0","byte_offset":27,"kind":"body""#;
    let suffix = r#","recipe_index":0,"record_index":12}"#;
    for value in ["\"\"", "\"301\""] {
        for offset in [None, Some(0), Some(4)] {
            let mut wire = format!("{prefix},\"design_id\":{value}");
            if let Some(offset) = offset {
                wire.push_str(&format!(",\"design_id_offset\":{offset}"));
            }
            wire.push_str(suffix);
            let parsed: super::ConstructionRecipe = serde_json::from_str(&wire).expect("recipe id");
            assert_eq!(serde_json::to_string(&parsed).expect("recipe wire"), wire);
        }
    }
    let wire = format!("{prefix}{suffix}");
    let parsed: super::ConstructionRecipe = serde_json::from_str(&wire).expect("body-less recipe");
    assert_eq!(serde_json::to_string(&parsed).expect("recipe wire"), wire);
    let wire = format!("{prefix},\"design_id_offset\":4{suffix}");
    let error = serde_json::from_str::<super::ConstructionRecipe>(&wire).expect_err("orphan design id offset").to_string();
    assert!(error.contains("design_id_offset"));
}

#[test]
fn segment_base_guid_preserves_source_and_authored_wire() {
    let prefix = r#"{"id":"type#0","byte_offset":0,"type_guid":"11111111-2222-3333-4444-555555555555","type_guid_offset":4"#;
    let suffix = r#","version":1,"version_offset":80,"module":"Fusion","entity_ids":[1],"entity_id_offsets":[]}"#;
    for value in ["\"\"", "\"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\""] {
        for offset in [None, Some(0), Some(44)] {
            let mut wire = format!("{prefix},\"base_type_guid\":{value}");
            if let Some(offset) = offset {
                wire.push_str(&format!(",\"base_type_guid_offset\":{offset}"));
            }
            wire.push_str(suffix);
            let parsed: super::SegmentType = serde_json::from_str(&wire).expect("base GUID");
            assert_eq!(serde_json::to_string(&parsed).expect("segment wire"), wire);
        }
    }
    let wire = format!("{prefix}{suffix}");
    let parsed: super::SegmentType = serde_json::from_str(&wire).expect("root type");
    assert_eq!(serde_json::to_string(&parsed).expect("segment wire"), wire);
    let wire = format!("{prefix},\"base_type_guid_offset\":44{suffix}");
    let error = serde_json::from_str::<super::SegmentType>(&wire).expect_err("orphan base GUID offset").to_string();
    assert!(error.contains("base_type_guid_offset"));
}

#[test]
fn parameter_unit_preserves_source_and_authored_wire() {
    let prefix = r#"{"id":"parameter","byte_offset":0,"class_tag":"123","record_index":1,"source_ordinal":0,"owner_record_index":2,"expression":"1","expression_offset":40,"source_kind":"Distance","source_kind_offset":60,"kind":"feature""#;
    let suffix = r#","name":"d1","name_offset":80,"evaluated_value":1.0,"evaluated_value_offset":90}"#;
    for value in ["\"\"", "\"mm\""] {
        for offset in [None, Some(0), Some(70)] {
            let mut wire = format!("{prefix},\"unit\":{value}");
            if let Some(offset) = offset {
                wire.push_str(&format!(",\"unit_offset\":{offset}"));
            }
            wire.push_str(suffix);
            let parsed: super::DesignParameter = serde_json::from_str(&wire).expect("parameter unit");
            assert_eq!(serde_json::to_string(&parsed).expect("parameter wire"), wire);
        }
    }
    let wire = format!("{prefix}{suffix}");
    let parsed: super::DesignParameter = serde_json::from_str(&wire).expect("dimensionless parameter");
    assert_eq!(serde_json::to_string(&parsed).expect("parameter wire"), wire);
    let wire = format!("{prefix},\"unit_offset\":70{suffix}");
    let error = serde_json::from_str::<super::DesignParameter>(&wire).expect_err("orphan unit offset").to_string();
    assert!(error.contains("unit_offset"));
}

#[test]
fn identity_wrapper_rows_preserve_wire_and_reject_unequal_arrays() {
    for count in 0..=2 {
        for offsets in 0..=2 {
            for tags in 0..=2 {
                let indices = ["[]", "[300]", "[300,305]"][count];
                let offsets_wire = ["[]", "[0]", "[0,24]"][offsets];
                let tags_wire = ["[]", "[\"384\"]", "[\"384\",\"289\"]"][tags];
                let wire = format!(r#"{{"id":"identity#0","group_record_index":200,"wrapper_record_indices":{indices},"wrapper_byte_offsets":{offsets_wire},"wrapper_class_tags":{tags_wire},"following_record_index":310,"following_byte_offset":48,"following_class_tag":"304"}}"#);
                let parsed = serde_json::from_str::<super::DesignConstructionOperandIdentity>(&wire);
                if count == offsets && count == tags {
                    assert_eq!(serde_json::to_string(&parsed.expect("complete rows")).expect("identity wire"), wire);
                } else {
                    let error = parsed.expect_err("unequal wrapper arrays").to_string();
                    assert!(error.contains("wrapper_record_indices"));
                    assert!(error.contains("wrapper_byte_offsets"));
                    assert!(error.contains("wrapper_class_tags"));
                }
            }
        }
    }
}

#[test]
fn legacy_base_feature_form_owns_its_compact_mode() {
    for form in ["compact_one_body", "expanded_two_body"] {
        let (suffixes, suffix_offsets, fields, refs, ref_offsets, parameters, parameter_offsets, auxiliary, auxiliary_offsets) =
            if form == "compact_one_body" {
                ("[201]", "[22]", "[[0,0,0,0,0,0]]", "[201]", "[22]", "[301]", "[40]", "[303]", "[50]")
            } else {
                ("[401,402]", "[22,36]", "[[0,0,0,0,0,0],[0,0,0,0,0,0]]", "[401,402]", "[22,36]", "[301,302]", "[50,60]", "[303,304]", "[70,80]")
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
            let parsed = serde_json::from_str::<super::DesignBaseFeatureConstruction>(&wire);
            if (form == "compact_one_body" && mask == 3) || (form == "expanded_two_body" && mask == 0) {
                assert_eq!(serde_json::to_string(&parsed.expect("complete legacy form")).expect("legacy wire"), wire);
                let value: serde_json::Value = serde_json::from_str(&wire).expect("legacy JSON");
                if form == "compact_one_body" {
                    for mode in [1_u8, 2, u8::MAX] {
                        let mut mode_wire = value.clone();
                        mode_wire["mode"] = mode.into();
                        let decoded = serde_json::from_value::<super::DesignBaseFeatureConstruction>(mode_wire.clone());
                        if mode == 1 {
                            assert_eq!(serde_json::to_value(decoded.expect("mode one")).unwrap(), mode_wire);
                        } else {
                            assert!(decoded.expect_err("unknown compact mode").to_string().contains("mode"));
                        }
                    }
                }
                for field in ["body_entity_suffixes", "body_entity_suffix_offsets", "body_entity_fields", "body_reference_records", "body_reference_record_offsets", "parameter_body_records", "parameter_body_record_offsets", "auxiliary_records", "auxiliary_record_offsets"] {
                    let mut invalid = value.clone();
                    invalid[field].as_array_mut().expect("body array").pop();
                    assert!(serde_json::from_value::<super::DesignBaseFeatureConstruction>(invalid).is_err(), "{field}");
                }
                for field in ["body_reference_records", "body_reference_record_offsets"] {
                    let mut invalid = value.clone();
                    invalid[field][0] = serde_json::json!(999);
                    let error = serde_json::from_value::<super::DesignBaseFeatureConstruction>(invalid).expect_err("conflicting body view").to_string();
                    assert!(error.contains(field));
                }
                let mut invalid = value;
                invalid["tag_body_based_on_faces"] = serde_json::json!(false);
                let error = serde_json::from_value::<super::DesignBaseFeatureConstruction>(invalid).expect_err("false body-source tag").to_string();
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
                let fields_wire = ["[]", "[[1,2,3,4,5,6]]", "[[1,2,3,4,5,6],[6,5,4,3,2,1]]"][fields];
                let wire = format!(r#"{{"body_entity_suffixes":{values_wire},"body_entity_suffix_offsets":{offsets_wire},"body_entity_fields":{fields_wire},"related_guids":["a","b","c"],"related_guid_offsets":[66,142,275],"linkage_record":301,"linkage_record_offset":234,"auxiliary_record":401,"auxiliary_record_offset":253}}"#);
                let parsed = serde_json::from_str::<super::DesignBaseFeatureConstruction>(&wire);
                if values == offsets && values == fields {
                    assert_eq!(serde_json::to_string(&parsed.expect("complete snapshot rows")).expect("snapshot wire"), wire);
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
    let parsed: super::DesignBaseFeatureConstruction = serde_json::from_str(wire).expect("direct body form");
    assert_eq!(serde_json::to_string(&parsed).expect("direct body wire"), wire);
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
        let error = serde_json::from_str::<super::DesignBaseFeatureConstruction>(&invalid).expect_err("invalid direct body view").to_string();
        assert!(error.contains(field));
    }
}

#[test]
fn extrude_selection_group_members_preserve_wire_and_reject_unequal_offsets() {
    let wire = r#"{"id":"group","scope_record_index":7,"scope_reference_ordinal":0,"record_index":9,"byte_offset":0,"class_tag":"277","member_count_offset":32,"members":[10,11],"member_offsets":[37,48],"opaque_index":1,"opaque_index_offset":58,"opaque_scalar":0.0,"opaque_scalar_offset":62,"variant":false,"paired_class_tag":"259","paired_byte_offset":111}"#;
    let group: super::DesignExtrudeSelectionGroup = serde_json::from_str(wire).expect("selection group");
    assert_eq!(serde_json::to_string(&group).expect("selection wire"), wire);
    for offsets in ["[]", "[37]", "[37,48,59]"] {
        let invalid = wire.replace("\"member_offsets\":[37,48]", &format!("\"member_offsets\":{offsets}"));
        let error = serde_json::from_str::<super::DesignExtrudeSelectionGroup>(&invalid).expect_err("unequal member arrays").to_string();
        assert!(error.contains("members"));
        assert!(error.contains("member_offsets"));
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
            let wire = format!(r#"{{"body_entity_suffixes":{suffixes},"body_entity_suffix_offsets":{suffix_offsets},"body_entity_fields":{fields},"body_reference_records":{references},"body_reference_record_offsets":{reference_offsets},"body_reference_fields":{fields},"repeated_reference_fields":{repeated},"metadata_record":401,"metadata_record_offset":110,"metadata_field":[0,0],"result_records":{results},"result_record_offsets":{result_offsets},"result_fields":{fields}}}"#);
            let construction: super::DesignBaseFeatureConstruction = serde_json::from_str(&wire).expect("aligned result rows");
            assert_eq!(serde_json::to_string(&construction).expect("result wire"), wire);
            let value: serde_json::Value = serde_json::from_str(&wire).expect("result JSON");
            for field in ["body_entity_suffixes", "body_entity_suffix_offsets", "body_entity_fields", "body_reference_records", "body_reference_record_offsets", "body_reference_fields", "result_records", "result_record_offsets", "result_fields"] {
                let mut invalid = value.clone();
                let array = invalid[field].as_array_mut().expect("result array");
                if count == 0 {
                    array.push(if field.ends_with("fields") { serde_json::json!([0, 0, 0, 0, 0, 0]) } else { serde_json::json!(1) });
                } else {
                    array.pop();
                }
                assert!(serde_json::from_value::<super::DesignBaseFeatureConstruction>(invalid).is_err(), "{field}");
            }
            let mut invalid = value;
            invalid["repeated_reference_fields"] = serde_json::json!(vec![[0; 6]; count + 1]);
            let error = serde_json::from_value::<super::DesignBaseFeatureConstruction>(invalid).expect_err("partial repeated run").to_string();
            assert!(error.contains("repeated_reference_fields"));
        }
    }
}

#[test]
fn copied_body_rows_preserve_wire_and_reject_unequal_runs() {
    let wire = r#"{"body_group_record_index":501,"body_group_class_tag":"264","body_group_byte_offset":100,"body_operand_record_indices":[502,504],"body_operand_record_offsets":[126,137],"relation_record_index":503,"relation_class_tag":"264","relation_byte_offset":200,"source_body_entity_suffixes":[11,13],"source_body_entity_suffix_offsets":[225,255],"copied_body_entity_suffixes":[12,14],"copied_body_entity_suffix_offsets":[240,270]}"#;
    let operation: super::DesignCopyPasteBodiesOperation = serde_json::from_str(wire).expect("copy body rows");
    assert_eq!(serde_json::to_string(&operation).expect("copy body wire"), wire);
    assert_eq!(operation.bodies[1].source.value, 13);
    assert_eq!(operation.bodies[1].copied.value, 14);
    let value: serde_json::Value = serde_json::from_str(wire).expect("copy JSON");
    for field in ["body_operand_record_indices", "body_operand_record_offsets", "source_body_entity_suffixes", "source_body_entity_suffix_offsets", "copied_body_entity_suffixes", "copied_body_entity_suffix_offsets"] {
        for length in [0, 1, 3] {
            let mut invalid = value.clone();
            invalid[field].as_array_mut().expect("copy array").resize(length, serde_json::json!(1));
            let error = serde_json::from_value::<super::DesignCopyPasteBodiesOperation>(invalid).expect_err("unequal body runs").to_string();
            assert!(error.contains(field), "{field}: {error}");
        }
    }
}

#[test]
fn timeline_items_preserve_wire_and_reject_unequal_offsets() {
    for offsets in ["[0,0]", "[245,256]"] {
        let wire = format!(r#"{{"id":"timeline","byte_offset":200,"class_tag":"256","record_index":35,"source_ordinal":0,"frame_length":100,"context_record_index":17,"context_record_index_offset":220,"item_count_offset":240,"item_record_indices":[101,102],"item_record_index_offsets":{offsets}}}"#);
        let timeline: super::DesignFeatureTimeline = serde_json::from_str(&wire).expect("timeline items");
        assert_eq!(serde_json::to_string(&timeline).expect("timeline wire"), wire);
        for invalid_offsets in ["[]", "[245]", "[245,256,267]"] {
            let invalid = wire.replace(&format!("\"item_record_index_offsets\":{offsets}"), &format!("\"item_record_index_offsets\":{invalid_offsets}"));
            let error = serde_json::from_str::<super::DesignFeatureTimeline>(&invalid).expect_err("unequal timeline arrays").to_string();
            assert!(error.contains("item_record_indices"));
            assert!(error.contains("item_record_index_offsets"));
        }
    }
}

#[test]
fn annotation_return_members_preserve_wire_and_reject_unequal_offsets() {
    let wire = r#"{"id":"annotation","governing_companion_record_index":2,"byte_offset":100,"class_tag":"256","record_index":3,"frame_length":120,"operands":[],"entity_genesis":0,"annotation_bytes":[],"annotation_byte_offset":150,"governing_owner_record_index":4,"governing_owner_reference_offset":170,"return_members":[10,11],"return_member_offsets":[185,196],"paired_class_tag":"259","paired_byte_offset":210,"owner_reference":5,"owner_reference_offset":230}"#;
    let frame: super::DesignDimensionAnnotationFrame = serde_json::from_str(wire).expect("annotation return members");
    assert_eq!(serde_json::to_string(&frame).expect("annotation wire"), wire);
    for offsets in ["[]", "[185]", "[185,196,207]"] {
        let invalid = wire.replace("\"return_member_offsets\":[185,196]", &format!("\"return_member_offsets\":{offsets}"));
        let error = serde_json::from_str::<super::DesignDimensionAnnotationFrame>(&invalid).expect_err("unequal return arrays").to_string();
        assert!(error.contains("return_members"));
        assert!(error.contains("return_member_offsets"));
    }
    let invalid = wire.replace("\"return_members\":[10,11]", "\"return_members\":[10,0]");
    let error = serde_json::from_str::<super::DesignDimensionAnnotationFrame>(&invalid).unwrap_err().to_string();
    assert!(error.contains("return_members"));
}

#[test]
fn dimension_locus_rows_preserve_return_order_and_derive_state_views() {
    for (state, kinds, unknown) in [(0, r#"["coincident"]"#, 0), (32, r#"["perpendicular"]"#, 0), (16384, "[]", 16384)] {
        let wire = format!(r#"{{"id":"locus-group","companion_record_index":2,"byte_offset":100,"class_tag":"256","record_index":3,"frame_length":150,"loci":[{{"geometry_record_index":11,"geometry_reference_offset":125,"role":0,"role_offset":135}},{{"geometry_record_index":10,"geometry_reference_offset":140,"role":0,"role_offset":150}}],"owner_reference":5,"owner_reference_offset":156,"owner_role":0,"owner_role_offset":166,"state":{state},"state_offset":170,"constraint_kinds":{kinds},"unknown_constraint_bits":{unknown},"return_members":[10,11],"return_member_offsets":[179,190],"next_class_tag":"259","next_record_index":4,"next_byte_offset":201}}"#);
        let group: super::DesignDimensionLocusGroup = serde_json::from_str(&wire).expect("locus group rows");
        assert_eq!(serde_json::to_string(&group).expect("locus group wire"), wire);
        assert_eq!(group.loci[0].geometry_record_index, 11);
        assert_eq!(group.loci[0].returned.value, 10);
        let value: serde_json::Value = serde_json::from_str(&wire).expect("locus JSON");
        for field in ["loci", "return_members", "return_member_offsets"] {
            let mut invalid = value.clone();
            invalid[field].as_array_mut().expect("locus array").pop();
            let error = serde_json::from_value::<super::DesignDimensionLocusGroup>(invalid).expect_err("unequal locus arrays").to_string();
            assert!(error.contains(field), "{field}: {error}");
        }
        for (field, replacement) in [("constraint_kinds", serde_json::json!(["parallel"])), ("unknown_constraint_bits", serde_json::json!(1))] {
            let mut invalid = value.clone();
            invalid[field] = replacement;
            let error = serde_json::from_value::<super::DesignDimensionLocusGroup>(invalid).expect_err("inconsistent state projection").to_string();
            assert!(error.contains(field));
        }
    }
}

#[test]
fn construction_auxiliary_rows_preserve_wire_and_reject_unequal_offsets() {
    for fields in ["", r#","auxiliary_record_indices":[103,106],"auxiliary_record_offsets":[37,48]"#] {
        let wire = format!(r#"{{"member_count_offset":20{fields},"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}}"#);
        let frame: super::DesignConstructionOperandGroupFrame = serde_json::from_str(&wire).expect("construction frame");
        assert_eq!(serde_json::to_string(&frame).expect("construction wire"), wire);
    }
    for fields in [r#", "auxiliary_record_indices":[103]"#, r#", "auxiliary_record_offsets":[37]"#, r#", "auxiliary_record_indices":[103,106],"auxiliary_record_offsets":[37]"#] {
        let wire = format!(r#"{{"member_count_offset":20{fields},"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}}"#);
        let error = serde_json::from_str::<super::DesignConstructionOperandGroupFrame>(&wire).expect_err("unequal auxiliary arrays").to_string();
        assert!(error.contains("auxiliary_record_indices"));
        assert!(error.contains("auxiliary_record_offsets"));
    }
}

#[test]
fn construction_trailing_rows_preserve_wire_and_reject_unequal_offsets() {
    for fields in ["", r#","trailing_record_indices":[300],"trailing_record_offsets":[1044]"#, r#","trailing_record_indices":[300,301],"trailing_record_offsets":[1044,1055]"#] {
        let wire = format!(r#"{{"member_count_offset":20{fields},"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}}"#);
        let frame: super::DesignConstructionOperandGroupFrame = serde_json::from_str(&wire).expect("construction frame");
        assert_eq!(serde_json::to_string(&frame).expect("construction wire"), wire);
    }
    for fields in [r#","trailing_record_indices":[300]"#, r#","trailing_record_offsets":[1044]"#, r#","trailing_record_indices":[300,301],"trailing_record_offsets":[1044]"#] {
        let wire = format!(r#"{{"member_count_offset":20{fields},"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}}"#);
        let error = serde_json::from_str::<super::DesignConstructionOperandGroupFrame>(&wire).expect_err("unequal trailing arrays").to_string();
        assert!(error.contains("trailing_record_indices"));
        assert!(error.contains("trailing_record_offsets"));
    }
}

#[test]
fn construction_member_rows_preserve_wire_and_reject_unequal_offsets() {
    for (members, offsets) in [("[]", "[]"), ("[10]", "[0]"), ("[10,11]", "[26,37]")] {
        let wire = format!(r#"{{"id":"group","scope_record_index":7,"scope_reference_ordinal":0,"record_index":9,"byte_offset":0,"class_tag":"277","members":{members},"member_offsets":{offsets},"frame":{{"member_count_offset":21,"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}},"role":0,"role_offset":60,"paired_class_tag":"278","paired_byte_offset":100}}"#);
        let group: super::DesignConstructionOperandGroup = serde_json::from_str(&wire).expect("construction group");
        assert_eq!(serde_json::to_string(&group).expect("construction wire"), wire);
        let invalid = wire.replace(&format!("\"member_offsets\":{offsets}"), "\"member_offsets\":[1,2,3]");
        let error = serde_json::from_str::<super::DesignConstructionOperandGroup>(&invalid).expect_err("unequal member arrays").to_string();
        assert!(error.contains("members"));
        assert!(error.contains("member_offsets"));
    }
}

#[test]
fn segment_entity_runs_preserve_authored_and_located_wire() {
    for (ids, offsets) in [("[]", "[]"), ("[10,11]", "[]"), ("[10,11]", "[0,0]"), ("[10,11]", "[80,88]")] {
        let wire = format!(r#"{{"id":"type","byte_offset":0,"type_guid":"11111111-2222-3333-4444-555555555555","type_guid_offset":4,"version":1,"version_offset":44,"module":"Fusion","entity_ids":{ids},"entity_id_offsets":{offsets}}}"#);
        let entry: super::SegmentType = serde_json::from_str(&wire).expect("type entity run");
        assert_eq!(serde_json::to_string(&entry).expect("type wire"), wire);
    }
    for (ids, offsets) in [("[]", "[80]"), ("[10,11]", "[80]"), ("[10]", "[80,88]")] {
        let wire = format!(r#"{{"id":"type","byte_offset":0,"type_guid":"11111111-2222-3333-4444-555555555555","type_guid_offset":4,"version":1,"version_offset":44,"module":"Fusion","entity_ids":{ids},"entity_id_offsets":{offsets}}}"#);
        let error = serde_json::from_str::<super::SegmentType>(&wire).expect_err("partial entity locations").to_string();
        assert!(error.contains("entity_ids/entity_id_offsets"));
    }
}

#[test]
fn mesh_feature_body_rows_preserve_wire_and_reject_duplicate_arrays() {
    let identity = serde_json::json!({
        "class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 200
    });
    let body = serde_json::json!({
        "body_record": identity, "entry_name_record": identity, "guid_record": identity,
        "wrapper_record": identity, "scene_state_record": identity, "scene_node_record": identity,
        "scene_auxiliary_record": identity, "owner_record": identity,
        "entry_name": "mesh.paramesh", "entry_name_offset": 120,
        "fusion_uuid": "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE", "fusion_uuid_offset": 130,
        "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "transform_offsets": [140, 160], "scope_reference_offset": 170,
        "wrapper_reference_offset": 180, "owner_reference_offset": 190,
        "guid_reference_offset": 200, "scene_node_reference_offset": 210,
        "collection_reference_offset": 220, "wrapper_body_reference_offset": 230,
        "entry_guid_reference_offset": 240, "guid_entry_reference_offset": 250,
        "scene_state_reference_offset": 260, "scene_auxiliary_reference_offset": 270
    });
    let base = serde_json::json!({
        "id": "mesh-feature", "scope_record": identity, "scope_base_record": identity,
        "collection_record": identity, "collection_base_record": identity,
        "texture_table_record": identity, "body_count_offsets": [21, 31, 41],
        "body_record_indices": [104, 104], "scope_body_reference_offsets": [25, 36],
        "collection_body_reference_offsets": [62, 73], "texture_table_reference_offset": 52,
        "collection_owner_record": identity, "collection_owner_reference_offset": 84,
        "collection_owner_backlink_offset": 94, "scope_owner_record_index": 109,
        "scope_owner_reference_offset": 105, "texture_flags_count_offset": 115,
        "texture_filename_count_offset": 125, "bodies": [body, body], "textures": []
    });
    for count in 0..=2 {
        let mut value = base.clone();
        for field in ["bodies", "body_record_indices", "scope_body_reference_offsets", "collection_body_reference_offsets"] {
            value[field].as_array_mut().expect("wire array").truncate(count);
        }
        let wire: super::DesignMeshFeatureWire = serde_json::from_value(value).expect("mesh wire");
        let expected = serde_json::to_string(&wire).expect("original mesh wire");
        let feature: super::DesignMeshFeature = serde_json::from_str(&expected).expect("mesh body rows");
        assert_eq!(serde_json::to_string(&feature).expect("mesh wire"), expected);
    }
    for field in ["body_record_indices", "scope_body_reference_offsets", "collection_body_reference_offsets", "bodies"] {
        let mut value = base.clone();
        value[field].as_array_mut().expect("wire array").pop();
        assert!(serde_json::from_value::<super::DesignMeshFeature>(value).is_err());
    }
    let mut changed_identity = base;
    changed_identity["body_record_indices"][0] = serde_json::json!(105);
    let error = serde_json::from_value::<super::DesignMeshFeature>(changed_identity).expect_err("conflicting body identity").to_string();
    assert!(error.contains("body_record_indices"));
}

#[test]
fn entity_header_runs_derive_counts_and_preserve_absent_reference_slots() {
    let prefix = r#"{"id":"header","byte_offset":0,"entity_suffix":1,"entity_id":"0_1","class_tag":"256","optional_slot_present":false"#;
    for (fields, references, offsets, members) in [
        ("", "[]", "[]", ""),
        ("", "[34]", "[]", ""),
        (r#","record_reference":33,"declared_reference_count":1"#, "[34]", "[]", ""),
        (r#","record_reference_offset":40,"declared_reference_count":0"#, "[]", "[]", ""),
        (r#","record_reference":33,"record_reference_offset":40,"declared_reference_count":2"#, "[34,35]", "[50,61]", r#","member_indices":[11],"member_offsets":[0]"#),
        ("", "[]", "[]", r#","member_indices":[11]"#),
    ] {
        let wire = format!("{prefix}{fields},\"reference_indices\":{references},\"reference_offsets\":{offsets}{members}}}");
        let header: super::DesignEntityHeader = serde_json::from_str(&wire).expect("header runs");
        assert_eq!(serde_json::to_string(&header).expect("header wire"), wire);
    }
    for suffix in [
        r#","declared_reference_count":1,"reference_indices":[],"reference_offsets":[]}"#,
        r#","reference_indices":[34,35],"reference_offsets":[50]}"#,
        r#","reference_indices":[],"reference_offsets":[50]}"#,
        r#","reference_indices":[],"reference_offsets":[],"member_indices":[11,12],"member_offsets":[0]}"#,
        r#","reference_indices":[],"reference_offsets":[],"member_offsets":[0]}"#,
    ] {
        assert!(serde_json::from_str::<super::DesignEntityHeader>(&format!("{prefix}{suffix}")).is_err());
    }
}

#[test]
fn empty_reference_runs_compare_equal_across_wire_round_trip() {
    let wire = r#"{"id":"header","byte_offset":0,"entity_suffix":1,"entity_id":"0_1","class_tag":"256","optional_slot_present":false,"declared_reference_count":0,"reference_indices":[],"reference_offsets":[]}"#;
    let mut header: super::DesignEntityHeader = serde_json::from_str(wire).expect("empty header");
    header.references = super::ReferenceRun::Located(Vec::new());
    header.members = super::ReferenceRun::Located(Vec::new());
    let serialized = serde_json::to_string(&header).expect("empty located runs");
    assert_eq!(serialized, wire);
    let decoded: super::DesignEntityHeader = serde_json::from_str(&serialized).expect("empty run wire");
    assert_eq!(decoded, header);
}

#[test]
fn face_source_rows_preserve_wire_and_reject_unequal_offsets() {
    let prefix = r#"{"id":"face-source","scope_record_index":1,"carrier_reference_ordinal":0,"carrier_record_index":2,"carrier_byte_offset":0,"carrier_class_tag":"302","carrier_frame_length":80,"paired_record_index":3,"paired_byte_offset":80,"paired_class_tag":"303"#;
    let member = r#"{"record_index":100,"byte_offset":1000,"class_tag":"304","persistent_identity":{"local_id":1,"local_id_offset":1021,"asset_id":"AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE","asset_id_offset":1033,"context_id":"11111111-2222-4333-8444-555555555555","context_id_offset":1109,"tail_slot_present":false,"tail_slot_offset":1185,"next_record_index":101,"next_byte_offset":1190}}"#;
    for (members, offsets) in [
        ("[]".to_owned(), "[]"),
        (format!("[{member}]"), "[25]"),
        (format!("[{member},{member}]"), "[25,36]"),
    ] {
        let wire = format!("{prefix},\"source_reference_offsets\":{offsets},\"source_members\":{members}}}");
        let group: super::DesignFaceSourceGroup = serde_json::from_str(&wire).expect("Face source rows");
        assert_eq!(serde_json::to_string(&group).expect("Face source wire"), wire);
        let invalid = wire.replace(&format!("\"source_reference_offsets\":{offsets}"), "\"source_reference_offsets\":[25,36,47]");
        let error = serde_json::from_str::<super::DesignFaceSourceGroup>(&invalid).expect_err("unequal source arrays").to_string();
        assert!(error.contains("source_members"));
        assert!(error.contains("source_reference_offsets"));
    }
}

#[test]
fn face_source_span_rejects_empty_reversed_and_conflicting_lengths() {
    let wire = r#"{"id":"face-source","scope_record_index":1,"carrier_reference_ordinal":0,"carrier_record_index":2,"carrier_byte_offset":10,"carrier_class_tag":"302","carrier_frame_length":80,"paired_record_index":3,"paired_byte_offset":90,"paired_class_tag":"303","source_reference_offsets":[],"source_members":[]}"#;
    for (field, value) in [("paired_byte_offset", 10), ("paired_byte_offset", 9), ("carrier_frame_length", 79)] {
        let mut invalid: serde_json::Value = serde_json::from_str(wire).expect("Face source wire");
        invalid[field] = value.into();
        let error = serde_json::from_value::<super::DesignFaceSourceGroup>(invalid).expect_err("invalid carrier span").to_string();
        assert!(error.contains(field));
    }
    let group: super::DesignFaceSourceGroup = serde_json::from_str(wire).expect("positive carrier span");
    assert_eq!(serde_json::to_string(&group).expect("Face source wire"), wire);
}

#[test]
fn scale_center_preserves_wire_and_rejects_partial_location() {
    for center in [None, Some(super::Located { value: [1.25, -2.5, 3.75], offset: 40 })] {
        let record = super::DesignScaleOperation {
            body_group_record_index: 102,
            center_record_index: 105,
            center_position: center,
            uniform_factor: 2.5,
            uniform_factor_offset: 21,
        };
        let expected = match center {
            None => r#"{"body_group_record_index":102,"center_record_index":105,"uniform_factor":2.5,"uniform_factor_offset":21}"#,
            Some(_) => r#"{"body_group_record_index":102,"center_record_index":105,"center_position":[1.25,-2.5,3.75],"center_position_offset":40,"uniform_factor":2.5,"uniform_factor_offset":21}"#,
        };
        assert_eq!(serde_json::to_string(&record).unwrap(), expected);
        assert_eq!(serde_json::from_str::<super::DesignScaleOperation>(expected).unwrap(), record);
    }
    for partial in [r#""center_position":[1.25,-2.5,3.75]"#, r#""center_position_offset":40"#] {
        let wire = format!(r#"{{"body_group_record_index":102,"center_record_index":105,{partial},"uniform_factor":2.5,"uniform_factor_offset":21}}"#);
        assert!(serde_json::from_str::<super::DesignScaleOperation>(&wire).unwrap_err().to_string().contains("center_position"));
    }
}

#[test]
fn component_placement_preserves_wire_and_rejects_partial_location() {
    let base = serde_json::json!({
        "id": "occurrence", "class_tag": "327", "record_index": 7, "byte_offset": 0,
        "component_record_index": 8, "component_guid": "component", "component_guid_offset": 48,
        "occurrence_guid": "placed", "occurrence_guid_offset": 124, "occurrence_ordinal": 1
    });
    let transform = serde_json::json!([[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]);
    for placed in [false, true] {
        let mut wire = base.clone();
        if placed {
            wire["transform"] = transform.clone();
            wire["transform_offset"] = serde_json::json!(209);
        }
        let record: super::DesignComponentOccurrence = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }
    for (field, value) in [("transform", transform), ("transform_offset", serde_json::json!(209))] {
        let mut wire = base.clone();
        wire[field] = value;
        assert!(serde_json::from_value::<super::DesignComponentOccurrence>(wire).unwrap_err().to_string().contains("transform"));
    }
}

#[test]
fn sketch_auxiliary_rows_preserve_absent_and_complete_offset_runs() {
    let base = r#"{"id":"relation","record_index":1,"class_tag":"000","byte_offset":0,"state_offset":0,"owner_reference":1,"owner_entity_id":"owner","auxiliary_references":[],"auxiliary_reference_offsets":[],"members":[],"resolved_members":[],"member_offsets":[],"owner_reference_offset":0,"state":0,"constraint_kinds":[],"unknown_constraint_bits":0,"member_relation_ordinals":[],"entity_genesis":null,"pattern":null,"return_members":[],"resolved_return_members":[],"return_member_offsets":[],"raw_bytes":""}"#;
    for (values, offsets) in [(vec![], vec![]), (vec![2], vec![]), (vec![2], vec![0]), (vec![2, 3], vec![0, 10])] {
        let expected = base.replace("\"auxiliary_references\":[]", &format!("\"auxiliary_references\":{}", serde_json::to_string(&values).unwrap()))
            .replace("\"auxiliary_reference_offsets\":[]", &format!("\"auxiliary_reference_offsets\":{}", serde_json::to_string(&offsets).unwrap()));
        let relation: super::SketchRelation = serde_json::from_str(&expected).unwrap();
        assert_eq!(relation.auxiliary_references.values().copied().collect::<Vec<_>>(), values);
        assert_eq!(relation.auxiliary_references.offsets().copied().collect::<Vec<_>>(), offsets);
        assert_eq!(serde_json::to_string(&relation).unwrap(), expected);
    }
    for (values, offsets) in [(vec![], vec![10]), (vec![2], vec![10, 20]), (vec![2, 3], vec![10])] {
        let mut wire: serde_json::Value = serde_json::from_str(base).unwrap();
        wire["auxiliary_references"] = serde_json::json!(values);
        wire["auxiliary_reference_offsets"] = serde_json::json!(offsets);
        assert!(serde_json::from_value::<super::SketchRelation>(wire).unwrap_err().to_string().contains("auxiliary_reference"));
    }
}

#[test]
fn sketch_nurbs_poles_preserve_wire_and_reject_partial_weights() {
    let base = r#"{"kind":"nurbs","subtype_class_tag":"302","subtype_record_index":7,"degree":1,"fit_tolerance":0.125,"scalar_width":4,"knots":[0.0,0.0,1.0,1.0],"weights":[],"control_points":[{"x":2.0,"y":3.0,"z":4.0},{"x":5.0,"y":6.0,"z":7.0}]}"#;
    for weights in ["[]", "[1.0,0.5]"] {
        let expected = base.replace("\"weights\":[]", &format!("\"weights\":{weights}"));
        let curve: super::SketchCurveGeometry = serde_json::from_str(&expected).unwrap();
        assert_eq!(serde_json::to_string(&curve).unwrap(), expected);
    }
    for weights in ["[1.0]", "[1.0,0.5,1.0]"] {
        let wire = base.replace("\"weights\":[]", &format!("\"weights\":{weights}"));
        assert!(serde_json::from_str::<super::SketchCurveGeometry>(&wire).unwrap_err().to_string().contains("weights"));
    }
    let empty = super::SketchCurveGeometry::Nurbs {
        carrier_reference: None,
        subtype_class_tag: "302".into(),
        subtype_record_index: 7,
        degree: 1,
        fit_tolerance: 0.125,
        scalar_width: 4,
        knots: vec![0.0, 1.0],
        poles: super::SketchNurbsPoles::Rational(Vec::new()),
    };
    let wire = serde_json::to_string(&empty).unwrap();
    assert_eq!(serde_json::from_str::<super::SketchCurveGeometry>(&wire).unwrap(), empty);
}

#[test]
fn rectangular_pattern_rows_preserve_wire_and_reject_parallel_mismatch() {
    let transform = serde_json::json!([[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]);
    for count in [0, 1, 3] {
        let value = serde_json::json!({
            "record_indices": (0..count).collect::<Vec<u32>>(),
            "transforms": (0..count).map(|_| transform.clone()).collect::<Vec<_>>(),
            "transform_offsets": (0..count).map(|index| u64::from(index) * 100).collect::<Vec<_>>()
        });
        let wire: super::DesignRectangularPatternInstancesWire = serde_json::from_value(value.clone()).unwrap();
        let expected = serde_json::to_string(&wire).unwrap();
        let native: super::DesignRectangularPatternInstances = serde_json::from_str(&expected).unwrap();
        assert_eq!(native.instance_count(), count as usize);
        assert_eq!(serde_json::to_string(&native).unwrap(), expected);
        if count != 0 {
            let mut component = value.clone();
            component["component_occurrences"] = serde_json::json!({
                "component_guid": "component", "seed_occurrence_guid": "seed",
                "generated_occurrence_guids": (1..count).map(|index| format!("generated-{index}")).collect::<Vec<_>>()
            });
            let wire: super::DesignRectangularPatternInstancesWire = serde_json::from_value(component.clone()).unwrap();
            let expected = serde_json::to_string(&wire).unwrap();
            let native: super::DesignRectangularPatternInstances = serde_json::from_str(&expected).unwrap();
            assert_eq!(serde_json::to_string(&native).unwrap(), expected);
            component["component_occurrences"]["generated_occurrence_guids"] = serde_json::json!(["extra", "extra", "extra"]);
            assert!(serde_json::from_value::<super::DesignRectangularPatternInstances>(component).unwrap_err().to_string().contains("generated_occurrence_guids"));
        }
        for field in ["record_indices", "transforms", "transform_offsets"] {
            let mut invalid = value.clone();
            invalid[field].as_array_mut().unwrap().push(if field == "transforms" { transform.clone() } else { serde_json::json!(0) });
            assert!(serde_json::from_value::<super::DesignRectangularPatternInstances>(invalid).unwrap_err().to_string().contains(field));
        }
    }
    let empty_component = serde_json::json!({
        "record_indices": [], "transforms": [], "transform_offsets": [],
        "component_occurrences": { "component_guid": "component", "seed_occurrence_guid": "seed", "generated_occurrence_guids": [] }
    });
    assert!(serde_json::from_value::<super::DesignRectangularPatternInstances>(empty_component).unwrap_err().to_string().contains("seed"));
}

#[test]
fn edge_flange_rows_preserve_wire_and_reject_parallel_mismatch() {
    for count in [0, 1, 3] {
        let wire = super::DesignEdgeFlangeOperationSerde {
            edge_wrapper_record_indices: (0..count).map(|index| 100 + index).collect(),
            edge_group_record_indices: (0..count).map(|index| 200 + index).collect(),
            edge_operand_record_indices: (0..count).map(|index| 203 + index).collect(),
            aggregate_group_record_index: 300,
            aggregate_operand_record_indices: (0..count).map(|index| 303 + index).collect(),
            height_owner_record_index: 400,
            height_extent: super::DesignEdgeFlangeHeightExtent::Distance,
            angle_owner_record_index: 401,
            width_mode: Some(super::DesignEdgeWidthMode::FullEdge),
            width_distance_owner_record_indices: Vec::new(),
            width_distance_owner_record_indices_by_edge: Vec::new(),
            auxiliary_reference_record_indices: Vec::new(),
            width_parameter_source: super::DesignEdgeFlangeWidthParameterSource::EdgeWidth,
            settings_record_index: 402,
            bend_radius: 0.25,
            bend_radius_offset: 500,
            reference_side_code: 4,
            height_datum: super::DesignSheetMetalHeightDatum::InnerFaces,
            bend_position: super::DesignBendPosition::Adjacent,
        };
        let expected = serde_json::to_string(&wire).unwrap();
        let native: super::DesignEdgeFlangeOperation = serde_json::from_str(&expected).unwrap();
        assert_eq!(native.shape.edges().count(), count as usize);
        assert_eq!(serde_json::to_string(&native).unwrap(), expected);
        for radius in [0.0, -1.0, f64::INFINITY, f64::NAN] {
            let mut invalid = wire.clone();
            invalid.bend_radius = radius;
            let error = super::DesignEdgeFlangeOperation::try_from(invalid).expect_err("invalid bend radius");
            assert!(error.contains("bend_radius"));
        }
        for mode in [super::DesignEdgeWidthMode::Symmetric, super::DesignEdgeWidthMode::TwoSides,
            super::DesignEdgeWidthMode::SymmetricPerEdge, super::DesignEdgeWidthMode::TwoSidesPerEdge] {
            if count == 0 && matches!(mode, super::DesignEdgeWidthMode::SymmetricPerEdge | super::DesignEdgeWidthMode::TwoSidesPerEdge) {
                continue;
            }
            let mut width_wire = wire.clone();
            width_wire.width_mode = Some(mode);
            width_wire.width_distance_owner_record_indices = match mode {
                super::DesignEdgeWidthMode::Symmetric => vec![600],
                super::DesignEdgeWidthMode::TwoSides => vec![600, 601],
                super::DesignEdgeWidthMode::SymmetricPerEdge => (0..count).map(|index| 600 + index).collect(),
                super::DesignEdgeWidthMode::TwoSidesPerEdge => (0..2 * count).map(|index| 600 + index).collect(),
                super::DesignEdgeWidthMode::FullEdge => Vec::new(),
            };
            if mode == super::DesignEdgeWidthMode::TwoSidesPerEdge {
                width_wire.width_distance_owner_record_indices_by_edge = (0..count).map(|index| [600 + 2 * index, 601 + 2 * index]).collect();
            }
            for source in [super::DesignEdgeFlangeWidthParameterSource::EdgeWidth, super::DesignEdgeFlangeWidthParameterSource::EdgeOffset] {
                width_wire.width_parameter_source = source;
                let expected = serde_json::to_string(&width_wire).unwrap();
                let decoded = serde_json::from_str::<super::DesignEdgeFlangeOperation>(&expected);
                if source == super::DesignEdgeFlangeWidthParameterSource::EdgeOffset && mode != super::DesignEdgeWidthMode::TwoSidesPerEdge {
                    assert!(decoded.unwrap_err().to_string().contains("width_parameter_source"));
                } else {
                    assert_eq!(serde_json::to_string(&decoded.unwrap()).unwrap(), expected);
                }
            }
            width_wire.width_parameter_source = super::DesignEdgeFlangeWidthParameterSource::EdgeWidth;
            if matches!(mode, super::DesignEdgeWidthMode::SymmetricPerEdge | super::DesignEdgeWidthMode::TwoSidesPerEdge) {
                let mut invalid = width_wire.clone();
                invalid.width_distance_owner_record_indices.push(999);
                if mode == super::DesignEdgeWidthMode::TwoSidesPerEdge {
                    invalid.width_distance_owner_record_indices.push(1000);
                    invalid.width_distance_owner_record_indices_by_edge.push([999, 1000]);
                }
                assert!(super::DesignEdgeFlangeOperation::try_from(invalid).unwrap_err().contains("selected edges"));
            }
            width_wire.height_extent = super::DesignEdgeFlangeHeightExtent::ToObject {
                target_group_record_index: 700, target_operand_record_index: 703,
                offset_owner_record_index: 710, reference_record_indices: [720, 721],
            };
            assert!(super::DesignEdgeFlangeOperation::try_from(width_wire).unwrap_err().contains("height_extent"));
        }
        let mut to_object = wire.clone();
        to_object.height_extent = super::DesignEdgeFlangeHeightExtent::ToObject {
            target_group_record_index: 700, target_operand_record_index: 703,
            offset_owner_record_index: 710, reference_record_indices: [720, 721],
        };
        let expected = serde_json::to_string(&to_object).unwrap();
        let native: super::DesignEdgeFlangeOperation = serde_json::from_str(&expected).unwrap();
        assert_eq!(serde_json::to_string(&native).unwrap(), expected);
        for field in [
            "edge_wrapper_record_indices",
            "edge_group_record_indices",
            "edge_operand_record_indices",
            "aggregate_operand_record_indices",
        ] {
            let mut invalid = serde_json::to_value(&wire).unwrap();
            invalid[field].as_array_mut().unwrap().push(serde_json::json!(999));
            assert!(serde_json::from_value::<super::DesignEdgeFlangeOperation>(invalid)
                .unwrap_err().to_string().contains(field));
        }
    }
}

#[test]
fn scope_reference_runs_preserve_wire_and_reject_partial_locations() {
    let empty = serde_json::to_string(&DesignParameterScope::empty("scope", DesignFeatureKind::Sketch, 1)).unwrap();
    for (values, offsets) in [
        ("[]", "[]"),
        ("[10]", "[]"),
        ("[10]", "[0]"),
        ("[10,20,30]", "[]"),
        ("[10,20,30]", "[0,11,22]"),
    ] {
        let wire = empty.replace("\"reference_members\":[]", &format!("\"reference_members\":{values}"))
            .replace("\"reference_member_offsets\":[]", &format!("\"reference_member_offsets\":{offsets}"));
        let scope: DesignParameterScope = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&scope).unwrap(), wire);
        assert_eq!(scope.reference_members.values().len(), scope.reference_members.len());
        assert_eq!(scope.reference_members.values_in(0..scope.reference_members.len()).unwrap().copied().collect::<Vec<_>>(),
            serde_json::from_str::<Vec<u32>>(values).unwrap());
        assert!(scope.reference_members.values_in(0..scope.reference_members.len() + 1).is_none());
    }
    for (values, offsets) in [("[]", "[0]"), ("[10]", "[0,11]"), ("[10,20,30]", "[0,11]")] {
        let wire = empty.replace("\"reference_members\":[]", &format!("\"reference_members\":{values}"))
            .replace("\"reference_member_offsets\":[]", &format!("\"reference_member_offsets\":{offsets}"));
        let error = serde_json::from_str::<DesignParameterScope>(&wire).unwrap_err().to_string();
        assert!(error.contains("reference_members"));
        assert!(error.contains("reference_member_offsets"));
    }
}

#[test]
fn assembly_forms_preserve_partial_and_mixed_qualifier_wire() {
    let frame = super::DesignAssemblyOperandFrame {
        reference_record_index: 10,
        reference_offset: 11,
        transform: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
        transform_offset: 22,
    };
    let path = super::DesignAssemblyOperandPath {
        link: super::DesignAssemblyOperandPathLink {
            locator_reference_offset: 11, locator_record_index: 10,
            locator_class_tag: "363".into(), locator_byte_offset: 100,
            locator_scope_reference_offset: 111, wrapper_record_index: 20,
            wrapper_reference_offset: 122, wrapper_class_tag: "388".into(),
            wrapper_byte_offset: 200, path_reference_offset: 211,
        },
        record_index: 30, class_tag: "386".into(), byte_offset: 300,
        occurrence_guids: vec![super::Located { value: "11111111-1111-4111-8111-111111111111".into(), offset: 311 }],
        identity_guids: vec![super::Located { value: "22222222-2222-4222-8222-222222222222".into(), offset: 322 }],
    };
    let limits = super::DesignAssemblyLimits {
        kind: super::DesignAssemblyLimitKind::Angular,
        minimum: -1.0, maximum: 1.0,
        owner_record_indices: [40, 50], value_offsets: [411, 511],
    };
    let joint_origin = super::DesignAssemblyOperandQualifier::JointOrigin {
        scope_record_index: 60, class_tag: "307".into(), byte_offset: 600,
        paired_class_tag: "264".into(), paired_byte_offset: 700,
    };
    let axial = super::DesignAssemblyOperandQualifier::AxialTarget {
        target: super::DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin { scope_record_index: 60 },
    };
    let occurrence = super::DesignAssemblyOperandQualifier::OccurrencePath { path: path.clone() };
    for form in [
        None,
        Some(super::DesignAssemblyAlignmentForm::DatumEnvelope { joint_origin_scope_record_index: 60 }),
        Some(super::DesignAssemblyAlignmentForm::SolvedOnly {
            solved_frame: super::DesignAssemblySolvedFrame { reference_record_index: 30, reference_offset: 33, record_byte_offset: 300, class_tag: "258".into(), transform: frame.transform, transform_offset: 325 },
            limits: Some(limits.clone()),
        }),
        Some(super::DesignAssemblyAlignmentForm::LimitsOnly { limits }),
        Some(super::DesignAssemblyAlignmentForm::Frames { frames: [frame.clone(), frame.clone()] }),
        Some(super::DesignAssemblyAlignmentForm::UnframedPaths([path.clone(), path])),
        Some(super::DesignAssemblyAlignmentForm::qualified([frame.clone(), frame.clone()], [occurrence.clone(), occurrence.clone()])),
        Some(super::DesignAssemblyAlignmentForm::qualified([frame.clone(), frame.clone()], [occurrence, joint_origin])),
        Some(super::DesignAssemblyAlignmentForm::qualified([frame.clone(), frame.clone()], [axial.clone(), axial])),
    ] {
        let alignment = super::DesignAssemblyAlignment {
            angle: 0.0, offset: [0.0; 3],
            owners: vec![super::Located { value: 10, offset: 11 }, super::Located { value: 20, offset: 22 }],
            form,
        };
        let wire = serde_json::to_string(&alignment).unwrap();
        let decoded: super::DesignAssemblyAlignment = serde_json::from_str(&wire).unwrap();
        assert_eq!(decoded, alignment);
        assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
        let mut invalid = serde_json::from_str::<serde_json::Value>(&wire).unwrap();
        invalid["value_offsets"] = serde_json::json!([11]);
        let error = serde_json::from_value::<super::DesignAssemblyAlignment>(invalid).unwrap_err().to_string();
        assert!(error.contains("owner_record_indices"));
        assert!(error.contains("value_offsets"));
    }
}

#[test]
fn legacy_assembly_wire_derives_carrier_frames_and_checks_repeated_fields() {
    let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let selection = |record_index| super::DesignAssemblyLegacySelection {
        record_index, byte_offset: 400, class_tag: "307".into(),
        asset_id: "11111111-1111-4111-8111-111111111111".into(), asset_id_offset: 411,
        context_id: "22222222-2222-4222-8222-222222222222".into(), context_id_offset: 422,
        recipe_record_index: 50, recipe_record_byte_offset: 500, recipe_id: "recipe".into(),
        recipe_kind: super::ConstructionRecipeKind::Face, recipe_references: Vec::new(), next_byte_offset: 600,
    };
    let carriers = super::DesignAssemblyLegacyOperands {
        point: super::DesignAssemblyLegacyOperand {
            construction_class_tag: "256".into(), reference_offset: 11,
            construction: Box::new(super::DesignWorkPointConstruction {
                point_record_index: 10, point_record_byte_offset: 100,
                position: [1.0, 2.0, 3.0], position_offset: 125,
                rule: crate::records::DesignWorkPointRule::try_from(crate::records::DesignWorkPointRuleForm::Native { reference_type: 0, inputs: Vec::new() }).expect("compatible WorkPoint rule"),
                reference_type_offset: 150,
            }),
            selection: selection(40),
        },
        hole: super::DesignAssemblyLegacyOperand {
            construction_class_tag: "257".into(), reference_offset: 22,
            construction: Box::new(super::DesignHoleConstruction {
                point_record_index: 20, point_record_byte_offset: 200,
                position: [4.0, 5.0, 6.0], position_offset: 225,
                direction: [0.0, 0.0, 1.0], direction_offset: 250,
                point_parameters: [0.0, 0.0], point_parameter_offsets: [275, 283],
                reference_type: 0, reference_type_offset: 291, tangent_point_data: None,
                input_records: Vec::new(), face_selection: None,
            }),
            selection: selection(41),
        },
    };
    let solved_frame = super::DesignAssemblySolvedFrame {
        reference_record_index: 30, reference_offset: 33, record_byte_offset: 300,
        class_tag: "258".into(), transform: identity, transform_offset: 325,
    };
    for frames_field_present in [false, true] {
        let alignment = super::DesignAssemblyAlignment {
            angle: 0.0, offset: [0.0; 3], owners: Vec::new(),
            form: Some(super::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                carriers: carriers.clone(), solved_frame: solved_frame.clone(), limits: None, frames_field_present,
            }),
        };
        let frames = alignment.operand_frames().unwrap();
        assert_eq!(frames[0].reference_record_index, 10);
        assert_eq!(frames[1].reference_record_index, 20);
        assert_eq!(frames[0].transform_offset, 325);
        assert_eq!(frames[0].transform, [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 2.0], [0.0, 0.0, 1.0, 3.0], [0.0, 0.0, 0.0, 1.0]]);
        assert_eq!(frames[1].transform[2][3], 6.0);
        let wire = serde_json::to_string(&alignment).unwrap();
        let decoded: super::DesignAssemblyAlignment = serde_json::from_str(&wire).unwrap();
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
            let error = serde_json::from_value::<super::DesignAssemblyAlignment>(invalid).unwrap_err().to_string();
            assert!(error.contains(field));
        }
        let mut invalid = value.clone();
        invalid["legacy_operand_carriers"].as_array_mut().unwrap().swap(0, 1);
        assert!(serde_json::from_value::<super::DesignAssemblyAlignment>(invalid).unwrap_err().to_string().contains("construction"));
        if frames_field_present {
            let mut invalid = value;
            invalid["operand_frames"][0]["transform"][0][3] = serde_json::json!(99.0);
            assert!(serde_json::from_value::<super::DesignAssemblyAlignment>(invalid).unwrap_err().to_string().contains("operand_frames"));
        }
    }
}

#[test]
fn assembly_path_wire_pairs_guid_locations() {
    for count in [0, 1, 3] {
        let values: Vec<_> = (0..count).map(|index| format!("guid-{index}")).collect();
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
            let path: super::DesignAssemblyOperandPath = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(path.occurrence_guids.len(), count);
            assert_eq!(serde_json::to_value(&path).unwrap(), wire);
            for (value_field, offset_field) in [
                ("occurrence_guids", "occurrence_guid_offsets"),
                ("identity_guids", "identity_guid_offsets"),
            ] {
                let mut invalid = wire.clone();
                let mut bad_offsets = invalid.get(offset_field).and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
                bad_offsets.push(serde_json::json!(999));
                invalid[offset_field] = serde_json::Value::Array(bad_offsets);
                let error = serde_json::from_value::<super::DesignAssemblyOperandPath>(invalid).unwrap_err().to_string();
                assert!(error.contains(value_field));
                assert!(error.contains(offset_field));
            }
        }
    }
}

#[test]
fn historical_loop_wire_preserves_each_complete_binding_stage() {
    for count in [0_u32, 1, 3] {
        for stage in 0..4 {
            let mut wire = serde_json::json!({
                "loop_slot": 10,
                "coedge_slots": (0..count).map(|index| 20 + index).collect::<Vec<_>>(),
                "edge_slots": (0..count).map(|index| 30 + index).collect::<Vec<_>>()
            });
            if count != 0 {
                if stage >= 1 { wire["vertex_slots"] = serde_json::json!((0..count).map(|index| 40 + index).collect::<Vec<_>>()); }
                if stage >= 2 { wire["point_slots"] = serde_json::json!((0..count).map(|index| 50 + index).collect::<Vec<_>>()); }
                if stage >= 3 { wire["positions"] = serde_json::json!((0..count).map(|index| cadmpeg_ir::math::Point3::new(f64::from(index), 0.0, 0.0)).collect::<Vec<_>>()); }
            }
            let context: super::DesignHistoricalFaceLoopContext = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(context.boundary.coedges().count(), count as usize);
            assert_eq!(serde_json::to_value(&context).unwrap(), wire);
            for field in ["edge_slots", "vertex_slots", "point_slots", "positions"] {
                let mut invalid = wire.clone();
                let mut values = invalid.get(field).and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
                for _ in 0..count + 1 {
                    values.push(if field == "positions" { serde_json::to_value(cadmpeg_ir::math::Point3::new(9.0, 0.0, 0.0)).unwrap() } else { serde_json::json!(99) });
                }
                invalid[field] = serde_json::Value::Array(values);
                assert!(serde_json::from_value::<super::DesignHistoricalFaceLoopContext>(invalid).unwrap_err().to_string().contains(field));
            }
        }
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
            wire["intermediate_parameters"] = serde_json::json!((0..intermediate_count).map(|index| f64::from(index + 1) / 4.0).collect::<Vec<_>>());
            wire["intermediate_parameter_record_indexes"] = serde_json::json!((0..intermediate_count).map(|index| 20 + index).collect::<Vec<_>>());
            wire["intermediate_parameter_offsets"] = serde_json::json!((0..intermediate_count).map(|index| 200 + index * 8).collect::<Vec<_>>());
        }
        for with_tangency in [false, true] {
            let mut wire = wire.clone();
            if with_tangency {
                wire["tangency_weight"] = serde_json::json!({ "value": 1.0, "record_index": 5, "value_offset": 50 });
            }
            let group: super::DesignFixedFilletGroup = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(group.law.radii().count(), radius_count as usize);
            assert_eq!(group.law.intermediate().len(), intermediate_count as usize);
            assert_eq!(serde_json::to_value(&group).unwrap(), wire);
            for field in ["radii", "radius_record_indexes", "radius_offsets", "intermediate_parameters", "intermediate_parameter_record_indexes", "intermediate_parameter_offsets"] {
                let mut invalid = wire.clone();
                let mut column = invalid.get(field).and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
                column.push(serde_json::json!(1));
                invalid[field] = serde_json::Value::Array(column);
                assert!(serde_json::from_value::<super::DesignFixedFilletGroup>(invalid).unwrap_err().to_string().contains(field));
            }
            let mut invalid = wire;
            for field in ["radii", "radius_record_indexes", "radius_offsets"] {
                invalid[field] = serde_json::json!([]);
            }
            assert!(serde_json::from_value::<super::DesignFixedFilletGroup>(invalid).unwrap_err().to_string().contains("radii"));
        }
    }
    for radii in [vec![1.0], vec![1.0, 2.0, 3.0]] {
        let wire = serde_json::json!({
            "radius_record_indexes": vec![10; radii.len()], "radius_offsets": vec![100; radii.len()], "radii": radii,
            "intermediate_parameters": [0.25, 0.5], "intermediate_parameter_record_indexes": [20, 21], "intermediate_parameter_offsets": [200, 208]
        });
        assert!(serde_json::from_value::<super::DesignFixedFilletGroup>(wire).unwrap_err().to_string().contains("intermediate_parameters"));
    }
}

#[test]
fn face_operand_wire_derives_node_offsets() {
    for count in [0_u32, 1, 3] {
        let offsets: Vec<_> = (0..count).map(|index| 100 + index * 16).collect();
        let nodes: Vec<_> = offsets.iter().map(|offset| serde_json::json!({
            "byte_offset": offset, "end_byte_offset": offset + 16,
            "program": [-1, -1, 2, 7], "recipe_structure": null
        })).collect();
        let base = serde_json::json!({
            "id": "face", "scope_record_index": 1, "scope_reference_ordinal": 0,
            "record_index": 2, "byte_offset": 10, "class_tag": "346",
            "paired_byte_offset": 20, "paired_class_tag": "262",
            "recipe_record_index": 3, "recipe_record_byte_offset": 30,
            "recipe_id": "recipe", "recipe_prefix_offset": 40, "recipe_prefix_bytes": "",
            "recipe_references": [], "recipe_kind": "bounded_face",
            "recipe_program_offset": 50, "recipe_program": [0, -1, 1],
            "recipe_node_offsets": offsets, "recipe_nodes": nodes,
            "next_record_index": 4, "next_byte_offset": 200
        });
        for grouped in [false, true] {
            let mut wire = base.clone();
            if grouped {
                wire["group_record_index"] = serde_json::json!(5);
                wire["group_member_ordinal"] = serde_json::json!(0);
            }
            for field in ["group_record_index", "group_member_ordinal"] {
                let mut invalid = wire.clone();
                invalid.as_object_mut().unwrap().remove("group_record_index");
                invalid.as_object_mut().unwrap().remove("group_member_ordinal");
                invalid[field] = serde_json::json!(5);
                let error = serde_json::from_value::<super::DesignFaceOperand>(invalid).unwrap_err().to_string();
                assert!(error.contains("group_record_index"));
                assert!(error.contains("group_member_ordinal"));
            }
            let operand: super::DesignFaceOperand = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(&operand).unwrap(), wire);
            let mut invalid = wire.clone();
            invalid["recipe_node_offsets"].as_array_mut().unwrap().push(serde_json::json!(999));
            assert!(serde_json::from_value::<super::DesignFaceOperand>(invalid).unwrap_err().to_string().contains("recipe_node_offsets"));
            if count != 0 {
                let mut invalid = wire;
                invalid["recipe_node_offsets"][0] = serde_json::json!(999);
                assert!(serde_json::from_value::<super::DesignFaceOperand>(invalid).unwrap_err().to_string().contains("recipe_node_offsets"));
            }
        }
    }
}

#[test]
fn selector_context_wire_rejects_partial_clauses_and_derives_singleton() {
    let entry = super::DesignTopologyRecipeEntry {
        selector: 3,
        boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
        topology_triplets: std::array::from_fn(|_| super::DesignTopologyRecipeTriplet {
            outer: std::num::NonZeroU32::new(3).unwrap(),
            middle: 2,
            vertex_ordinal: 2,
            incident_edge_ordinal: Some(1),
            incident_side: Some(super::DesignTopologyIncidentSide::Preceding),
        }),
        common_incident_edge_ordinal: Some(1),
    };
    for edges in [vec![], vec![7], vec![7, 8]] {
        for count in [0, 1, 3] {
            let entries: Vec<_> = (0..count).map(|index| (index % 2 == 0).then(|| entry.clone())).collect();
            let slots: Vec<_> = (0..count).map(|index| (index % 2 == 0).then(|| [vec![7, 8], vec![7]])).collect();
            let mut wire = serde_json::json!({
                "selector": 3, "clause_entries": entries,
                "clause_triplet_edge_slots": slots,
                "incidence_matching_edge_slots": edges,
                "boundary_count_matching_edge_slots": [7, 8]
            });
            if edges.len() == 1 {
                wire["unique_incidence_edge_slot"] = serde_json::json!(7);
            }
            let context: super::DesignEdgeRecipeSelectorContext = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(context.clauses.len(), count);
            assert_eq!(serde_json::to_value(&context).unwrap(), wire);
            let mut invalid = wire.clone();
            invalid["unique_incidence_edge_slot"] = serde_json::json!(9);
            assert!(serde_json::from_value::<super::DesignEdgeRecipeSelectorContext>(invalid).unwrap_err().to_string().contains("unique_incidence_edge_slot"));
            for field in ["clause_entries", "clause_triplet_edge_slots"] {
                let mut invalid = wire.clone();
                invalid[field].as_array_mut().unwrap().push(serde_json::Value::Null);
                assert!(serde_json::from_value::<super::DesignEdgeRecipeSelectorContext>(invalid).unwrap_err().to_string().contains("clause_triplet_edge_slots"));
                if count != 0 {
                    let mut invalid = wire.clone();
                    invalid[field][0] = serde_json::Value::Null;
                    let error = serde_json::from_value::<super::DesignEdgeRecipeSelectorContext>(invalid).unwrap_err().to_string();
                    assert!(error.contains("clause_entries"));
                    assert!(error.contains("clause_triplet_edge_slots"));
                }
            }
        }
    }
}

#[test]
fn circular_pattern_axis_wire_preserves_shared_identity_and_rejects_partial_rows() {
    let inline = serde_json::json!({
        "kind": "inline", "origin": [1.0, 2.0, 3.0], "origin_offset": 12,
        "direction": [0.0, 0.0, 1.0], "direction_offset": 36
    });
    let axis: super::DesignCircularPatternAxis = serde_json::from_value(inline.clone()).unwrap();
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
                wire["resolved_origin"] = serde_json::to_value(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap();
                wire["resolved_direction"] = serde_json::to_value(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap();
            }
            let axis: super::DesignCircularPatternAxis = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(axis).unwrap(), wire);
            for field in ["wrapper_record_indices", "identity_offsets", "persistent_identities"] {
                let mut invalid = wire.clone();
                invalid[field].as_array_mut().unwrap().push(serde_json::json!(99));
                assert!(serde_json::from_value::<super::DesignCircularPatternAxis>(invalid).unwrap_err().to_string().contains(field));
            }
            let mut invalid = wire.clone();
            invalid["persistent_identities"] = serde_json::json!([]);
            assert!(serde_json::from_value::<super::DesignCircularPatternAxis>(invalid).unwrap_err().to_string().contains("persistent_identities"));
            for field in ["resolved_origin", "resolved_direction"] {
                let mut invalid = wire.clone();
                invalid.as_object_mut().unwrap().remove("resolved_origin");
                invalid.as_object_mut().unwrap().remove("resolved_direction");
                invalid[field] = serde_json::to_value(cadmpeg_ir::math::Point3::new(0.0, 0.0, 1.0)).unwrap();
                let error = serde_json::from_value::<super::DesignCircularPatternAxis>(invalid).unwrap_err().to_string();
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
    for fields in [String::new(), format!(",\"plane_origin\":{origin},\"plane_normal\":{normal}")] {
        let wire = format!("{prefix}{fields}}}");
        let construction: super::DesignMirrorConstruction = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&construction).unwrap(), wire);
    }
    for (field, value) in [("plane_origin", origin), ("plane_normal", normal)] {
        let invalid = format!("{prefix},\"{field}\":{value}}}");
        let error = serde_json::from_str::<super::DesignMirrorConstruction>(&invalid).unwrap_err().to_string();
        assert!(error.contains("plane_origin"));
        assert!(error.contains("plane_normal"));
    }
}

#[test]
fn edge_operand_wire_rejects_partial_resolved_axis() {
    let prefix = r#"{"id":"edge","scope_record_index":1,"scope_reference_ordinal":0,"record_index":2,"byte_offset":10,"class_tag":"346","paired_byte_offset":20,"paired_class_tag":"262","recipe_record_index":3,"recipe_record_byte_offset":30,"recipe_id":"recipe","recipe_prefix_offset":40,"recipe_prefix_bytes":"","recipe_references":[],"recipe_program_offset":50,"recipe_program":[]"#;
    let suffix = r#","next_record_index":4,"next_byte_offset":100}"#;
    let origin = serde_json::to_string(&cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap();
    let direction = serde_json::to_string(&cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap();
    for fields in [String::new(), format!(",\"resolved_axis_origin\":{origin},\"resolved_axis_direction\":{direction}")] {
        let wire = format!("{prefix}{fields}{suffix}");
        let operand: super::DesignEdgeOperand = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&operand).unwrap(), wire);
    }
    for (field, value) in [("resolved_axis_origin", origin), ("resolved_axis_direction", direction)] {
        let invalid = format!("{prefix},\"{field}\":{value}{suffix}");
        let error = serde_json::from_str::<super::DesignEdgeOperand>(&invalid).unwrap_err().to_string();
        assert!(error.contains("resolved_axis_origin"));
        assert!(error.contains("resolved_axis_direction"));
    }
}

#[test]
fn historical_binding_wire_rejects_partial_identity_and_orphan_states() {
    fn check<T>(base: &serde_json::Value)
    where
        T: serde::de::DeserializeOwned + serde::Serialize + std::fmt::Debug,
    {
        for binding in [
            serde_json::json!({}),
            serde_json::json!({"historical_entity_kind": "loop", "historical_entity_ref": 42}),
            serde_json::json!({"historical_entity_kind": "loop", "historical_entity_ref": 42, "historical_state_ids": [2, 3]}),
        ] {
            let mut wire = base.clone();
            wire.as_object_mut().unwrap().extend(binding.as_object().unwrap().clone());
            let value: T = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(value).unwrap(), wire);
        }
        for binding in [
            serde_json::json!({"historical_entity_kind": "loop"}),
            serde_json::json!({"historical_entity_ref": 42}),
            serde_json::json!({"historical_state_ids": [2]}),
            serde_json::json!({"historical_entity_kind": "loop", "historical_state_ids": [2]}),
            serde_json::json!({"historical_entity_ref": 42, "historical_state_ids": [2]}),
        ] {
            let mut invalid = base.clone();
            invalid.as_object_mut().unwrap().extend(binding.as_object().unwrap().clone());
            let error = serde_json::from_value::<T>(invalid).unwrap_err().to_string();
            assert!(error.contains("historical_entity_kind"));
            assert!(error.contains("historical_entity_ref"));
            assert!(error.contains("historical_state_ids"));
        }
    }
    let mut member = serde_json::json!({
        "id": "member", "group_record_index": 1, "group_member_ordinal": 0,
        "record_index": 2, "byte_offset": 10, "class_tag": "346",
        "local_id": 17, "local_id_offset": 20,
        "asset_id": "asset", "asset_id_offset": 30,
        "context_id": "context", "context_id_offset": 40,
        "tail_slot_present": false, "tail_slot_offset": 0,
        "next_record_index": 3, "next_byte_offset": 50
    });
    check::<super::DesignExtrudeSelectionMember>(&member);
    for field in ["tail_slot_present", "tail_slot_offset", "next_record_index", "next_byte_offset"] {
        member.as_object_mut().unwrap().remove(field);
    }
    member["scope_record_index"] = serde_json::json!(4);
    member["compact_layout"] = serde_json::json!(false);
    check::<super::DesignEdgeIdentityOperand>(&member);
}

#[test]
fn variable_fillet_midpoints_preserve_wire_and_reject_unpaired_records() {
    let wire = r#"{"kind":"variable","start_radius_parameter_record_index":51,"end_radius_parameter_record_index":61,"middle_radius_parameter_record_indices":[71],"middle_parameter_record_indices":[81]}"#;
    let law: super::DesignFilletRadiusLaw = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&law).unwrap(), wire);
    for invalid in [
        wire.replace("[71]", "[]"),
        wire.replace("[81]", "[]"),
    ] {
        let error = serde_json::from_str::<super::DesignFilletRadiusLaw>(&invalid).unwrap_err().to_string();
        assert!(error.contains("middle_radius_parameter_record_indices"));
        assert!(error.contains("middle_parameter_record_indices"));
    }
}

#[test]
fn parameter_source_preserves_wire_and_rejects_inconsistent_ownership() {
    let prefix = r#"{"id":"parameter","byte_offset":0,"class_tag":"123","record_index":1"#;
    let tail = r#","name":"d1","name_offset":80,"evaluated_value":1.0,"evaluated_value_offset":90}"#;
    for (source_kind, kind, owner) in [
        ("User Parameter", "user", ""),
        ("Linear Dimension-2", "dimension", ",\"owner_record_index\":2"),
        ("Distance", "feature", ",\"owner_record_index\":2"),
    ] {
        for discriminator in [0, 3, 4, 5, 6] {
            let wire = format!("{prefix},\"family_discriminator\":{discriminator},\"family_discriminator_offset\":22,\"source_ordinal\":0{owner},\"expression\":\"1\",\"expression_offset\":40,\"source_kind\":\"{source_kind}\",\"source_kind_offset\":60,\"kind\":\"{kind}\"{tail}");
            let parameter: super::DesignParameter = serde_json::from_str(&wire).unwrap();
            assert_eq!(parameter.source_kind(), source_kind);
            assert_eq!(serde_json::to_string(&parameter).unwrap(), wire);
            let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
            let mut wrong_kind = value.clone();
            wrong_kind["kind"] = serde_json::json!(if kind == "user" { "feature" } else { "user" });
            assert!(serde_json::from_value::<super::DesignParameter>(wrong_kind).unwrap_err().to_string().contains("kind"));
            let mut wrong_owner = value.clone();
            if kind == "user" {
                wrong_owner["owner_record_index"] = serde_json::json!(2);
            } else {
                wrong_owner.as_object_mut().unwrap().remove("owner_record_index");
            }
            assert!(serde_json::from_value::<super::DesignParameter>(wrong_owner).unwrap_err().to_string().contains("owner_record_index"));
            let mut invalid_discriminator = value.clone();
            invalid_discriminator["family_discriminator"] = serde_json::json!(7);
            assert!(serde_json::from_value::<super::DesignParameter>(invalid_discriminator).unwrap_err().to_string().contains("family_discriminator"));
            let mut no_discriminator = value;
            no_discriminator.as_object_mut().unwrap().remove("family_discriminator");
            no_discriminator.as_object_mut().unwrap().remove("family_discriminator_offset");
            if kind == "user" {
                assert!(serde_json::from_value::<super::DesignParameter>(no_discriminator).unwrap_err().to_string().contains("family_discriminator"));
            } else {
                let parameter: super::DesignParameter = serde_json::from_value(no_discriminator.clone()).unwrap();
                assert_eq!(serde_json::to_value(parameter).unwrap(), no_discriminator);
            }
        }
    }
    assert!(super::DesignParameterSource::new(String::new(), Some(2), None).unwrap_err().contains("source_kind"));
}

#[test]
fn construction_recipe_design_preserves_wire_and_rejects_orphan_selector() {
    let prefix = r#"{"id":"recipe","byte_offset":80,"kind":"body"#;
    let suffix = r#","recipe_index":0,"record_index":7}"#;
    for fields in [
        "",
        ",\"design_id\":\"301\"",
        ",\"design_id\":\"301\",\"design_id_offset\":12",
        ",\"design_id\":\"301\",\"design_selector\":{\"value\":2,\"byte_offset\":0}",
        ",\"design_id\":\"301\",\"design_id_offset\":12,\"design_selector\":{\"value\":2,\"byte_offset\":15}",
    ] {
        let wire = format!("{prefix}\"{fields}{suffix}");
        let recipe: super::ConstructionRecipe = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&recipe).unwrap(), wire);
    }
    let wire = format!("{prefix}\",\"design_selector\":{{\"value\":2,\"byte_offset\":15}}{suffix}");
    let error = serde_json::from_str::<super::ConstructionRecipe>(&wire).unwrap_err().to_string();
    assert!(error.contains("design_id"));
    assert!(error.contains("design_selector"));
}

#[test]
fn coil_selection_preserves_wire_and_rejects_dependent_fields_without_identity() {
    let persistent = r#"{"kind":"persistent","asset_id":"asset","context_id":"context","identity_record_index":3,"primary_identity":7"#;
    for fields in [
        "",
        ",\"secondary_identity\":11",
        ",\"secondary_identity\":11,\"curve_secondary_identity\":0",
        ",\"secondary_identity\":11,\"curve_secondary_identity\":13",
    ] {
        let wire = format!("{persistent}{fields}}}");
        let selection: super::DesignCoilSelection = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&selection).unwrap(), wire);
    }
    let wire = format!("{persistent},\"curve_secondary_identity\":13}}");
    let error = serde_json::from_str::<super::DesignCoilSelection>(&wire).unwrap_err().to_string();
    assert!(error.contains("secondary_identity"));
    assert!(error.contains("curve_secondary_identity"));

    let face = r#"{"kind":"face_recipe","asset_id":"asset","context_id":"context","recipe_record_index":3,"recipe_record_byte_offset":40,"recipe_id":"recipe","recipe_kind":"#;
    for kind in ["face", "bounded_face"] {
        for fields in [
            "",
            ",\"design_id\":\"body\"",
            ",\"design_id\":\"body\",\"design_selector\":{\"value\":2,\"byte_offset\":60}",
        ] {
            let wire = format!("{face}\"{kind}\"{fields}}}");
            let selection: super::DesignCoilSelection = serde_json::from_str(&wire).unwrap();
            assert_eq!(serde_json::to_string(&selection).unwrap(), wire);
        }
    }
    let wire = format!("{face}\"face\",\"design_selector\":{{\"value\":2,\"byte_offset\":60}}}}");
    let error = serde_json::from_str::<super::DesignCoilSelection>(&wire).unwrap_err().to_string();
    assert!(error.contains("design_id"));
    assert!(error.contains("design_selector"));
    for kind in ["body", "edge", "vertex"] {
        let wire = format!("{face}\"{kind}\"}}");
        let error = serde_json::from_str::<super::DesignCoilSelection>(&wire).unwrap_err().to_string();
        assert!(error.contains("recipe_kind"));
    }
}

#[test]
fn companion_timestamp_preserves_wire_and_rejects_zero() {
    let wire = r#"{"id":"companion","byte_offset":0,"class_tag":"123","record_index":3,"owner_record_index":2,"timestamp_micros":1,"timestamp_micros_offset":42,"payload_byte_offset":58,"payload_byte_length":0}"#;
    let companion: super::DesignParameterCompanion = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&companion).unwrap(), wire);
    let legacy = wire.replace("timestamp_micros_offset", "opaque_value_offset").replace("timestamp_micros", "opaque_value");
    let companion: super::DesignParameterCompanion = serde_json::from_str(&legacy).unwrap();
    assert_eq!(serde_json::to_string(&companion).unwrap(), wire);
    for invalid in [wire.replace("\"timestamp_micros\":1", "\"timestamp_micros\":0"), legacy.replace("\"opaque_value\":1", "\"opaque_value\":0")] {
        let error = serde_json::from_str::<super::DesignParameterCompanion>(&invalid).unwrap_err().to_string();
        assert!(error.contains("timestamp_micros"));
    }
}

#[test]
fn dimension_operands_preserve_null_and_required_index_wires() {
    for index in [0, 7, u32::MAX] {
        let wire = format!(r#"{{"geometry_record_index":{index},"geometry_reference_offset":25,"role":3,"role_offset":35}}"#);
        let operand: super::DesignDimensionAnnotationOperand = serde_json::from_str(&wire).unwrap();
        assert_eq!(operand.geometry_record_index, std::num::NonZeroU32::new(index));
        assert_eq!(serde_json::to_string(&operand).unwrap(), wire);
        if index == 0 {
            let error = serde_json::from_str::<super::DesignDimensionPresentationOperand>(&wire).unwrap_err().to_string();
            assert!(error.contains("geometry_record_index"));
        } else {
            let operand: super::DesignDimensionPresentationOperand = serde_json::from_str(&wire).unwrap();
            assert_eq!(operand.geometry_record_index.get(), index);
            assert_eq!(serde_json::to_string(&operand).unwrap(), wire);
        }
    }
}

#[test]
fn legacy_extrude_constants_and_geometry_preserve_wire() {
    for geometry_kind in [0, 1] {
        let wire = format!(r#"{{"layout":"legacy_distance","prefix_value":0,"prefix_value_offset":21,"operation":"join","operation_offset":25,"extent_kind":2,"extent_kind_offset":29,"direction_reversed":false,"direction_reversed_offset":33,"geometry_kind":{geometry_kind},"geometry_kind_offset":34}}"#);
        let prologue: super::DesignExtrudePrologue = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&prologue).unwrap(), wire);
        assert_eq!(prologue.extent(), Some(super::DesignExtrudeExtent::OneSidedDistance));
        assert_eq!(prologue.solid_operation(), geometry_kind == 1);
        for (field, before, after) in [
            ("prefix_value", "\"prefix_value\":0".to_owned(), "\"prefix_value\":1".to_owned()),
            ("extent_kind", "\"extent_kind\":2".to_owned(), "\"extent_kind\":1".to_owned()),
            ("geometry_kind", format!("\"geometry_kind\":{geometry_kind}"), "\"geometry_kind\":2".to_owned()),
        ] {
            let invalid = wire.replace(&before, &after);
            let error = serde_json::from_str::<super::DesignExtrudePrologue>(&invalid).unwrap_err().to_string();
            assert!(error.contains(field));
        }
    }
}

#[test]
fn coil_placement_derives_only_the_encoded_identity_matrix() {
    let prefix = r#"{"selection_record_index":1,"selection_record_byte_offset":0,"selection_class_tag":"353","selection":{"kind":"persistent","asset_id":"a","context_id":"c","identity_record_index":2,"primary_identity":3},"transform_record_index":4,"transform_record_byte_offset":5,"transform_class_tag":"450","transform":"#;
    let identity = "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]";
    let translated = "[[1.0,0.0,0.0,2.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]";
    for (matrix, offset) in [(identity, ""), (identity, ",\"transform_offset\":55"), (translated, ",\"transform_offset\":55")] {
        let wire = format!("{prefix}{matrix}{offset}}}");
        let placement: super::DesignCoilPlacement = serde_json::from_str(&wire).expect("coil placement");
        assert_eq!(serde_json::to_string(&placement).expect("coil placement wire"), wire);
        assert_eq!(placement.explicit_transform.is_some(), !offset.is_empty());
    }
    let error = serde_json::from_str::<super::DesignCoilPlacement>(&format!("{prefix}{translated}}}"))
        .expect_err("matrix requires its location");
    assert!(error.to_string().contains("transform"));
    assert!(error.to_string().contains("transform_offset"));
}

#[test]
fn move_form_preserves_its_closed_integer_wire_domain() {
    for code in [1, 5] {
        let wire = code.to_string();
        let form: super::DesignMoveForm = serde_json::from_str(&wire).expect("move form");
        assert_eq!(serde_json::to_string(&form).expect("move form wire"), wire);
    }
    for code in [0, 2, 3, 4, 6, u32::MAX] {
        let error = serde_json::from_str::<super::DesignMoveForm>(&code.to_string()).expect_err("invalid move form");
        assert!(error.to_string().contains("form"));
    }
}

#[test]
fn profile_region_member_preserves_fixed_words_and_closed_incidence_values() {
    let wire = |kind: u32, identity: u64, words: [u32; 8]| {
        format!("{{\"kind\":{kind},\"kind_offset\":40,\"curve_primary_id\":{identity},\"curve_primary_id_offset\":44,\"incidence_words\":{},\"incidence_words_offset\":48}}", serde_json::to_string(&words).expect("incidence words"))
    };
    for identity in [1, u64::from(u32::MAX)] {
        for flag in [0, 1] {
            for first in [1, 2] {
                for second in [1, 2] {
                    let json = wire(3, identity, [0, 0, 0, flag, first, second, 0, 0]);
                    let member: super::DesignSketchProfileRegionMember = serde_json::from_str(&json).expect("region member");
                    assert_eq!(serde_json::to_string(&member).expect("region member wire"), json);
                }
            }
        }
    }
    for kind in [0, 1, 2, 4, u32::MAX] {
        let error = serde_json::from_str::<super::DesignSketchProfileRegionMember>(&wire(kind, 1, [0, 0, 0, 0, 1, 1, 0, 0])).expect_err("fixed kind");
        assert!(error.to_string().contains("kind"));
    }
    for identity in [0, u64::from(u32::MAX) + 1, u64::MAX] {
        let error = serde_json::from_str::<super::DesignSketchProfileRegionMember>(&wire(3, identity, [0, 0, 0, 0, 1, 1, 0, 0])).expect_err("nonzero u32 identity");
        assert!(error.to_string().contains("curve_primary_id"));
    }
    for index in 0..8 {
        let mut words = [0, 0, 0, 0, 1, 1, 0, 0];
        words[index] = 3;
        let error = serde_json::from_str::<super::DesignSketchProfileRegionMember>(&wire(3, 1, words)).expect_err("invalid incidence word");
        assert!(error.to_string().contains("incidence_words"));
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
        (translated, ",\"transform_offset\":50,\"carrier_transform_offset\":40"),
    ] {
        let wire = format!("{prefix}{matrix}{offsets}}}");
        let construction: super::DesignComponentInsertConstruction = serde_json::from_str(&wire).expect("component placement");
        assert_eq!(serde_json::to_string(&construction).expect("component placement wire"), wire);
        assert_eq!(construction.placement.is_some(), !offsets.is_empty());
    }
    for (matrix, offsets) in [(translated, ""), (identity, ",\"carrier_transform_offset\":40")] {
        let wire = format!("{prefix}{matrix}{offsets}}}");
        let error = serde_json::from_str::<super::DesignComponentInsertConstruction>(&wire).expect_err("missing scope matrix location");
        assert!(error.to_string().contains("transform_offset"));
    }
}

#[test]
fn component_occurrence_derives_base_ordinal_and_requires_nonzero_placed_ordinal() {
    let prefix = r#"{"id":"occurrence","class_tag":"327","record_index":7,"byte_offset":0,"component_record_index":8,"component_guid":"component","component_guid_offset":48,"occurrence_guid":"placed","occurrence_guid_offset":124,"occurrence_ordinal":"#;
    let matrix = r#","transform":[[1.0,0.0,0.0,2.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],"transform_offset":209"#;
    for (ordinal, placed) in [(1, false), (1, true), (2, true), (u32::MAX, true)] {
        let suffix = if placed { matrix } else { "" };
        let wire = format!("{prefix}{ordinal}{suffix}}}");
        let occurrence: super::DesignComponentOccurrence = serde_json::from_str(&wire).expect("component occurrence");
        assert_eq!(serde_json::to_string(&occurrence).expect("component occurrence wire"), wire);
        assert_eq!(occurrence.occurrence_ordinal(), ordinal);
    }
    for (ordinal, placed) in [(0, false), (0, true), (2, false), (u32::MAX, false)] {
        let suffix = if placed { matrix } else { "" };
        let wire = format!("{prefix}{ordinal}{suffix}}}");
        let error = serde_json::from_str::<super::DesignComponentOccurrence>(&wire).expect_err("invalid occurrence ordinal");
        assert!(error.to_string().contains("occurrence_ordinal"));
    }
}

#[test]
fn mirror_scope_tolerance_pairs_repeated_markers_with_their_locations() {
    for (marker, repeated) in [(61, None), (89, Some(59)), (94, Some(58)), (100, Some(59))] {
        let offset = repeated.map_or_else(String::new, |offset| format!(",\"repeated_marker_offset\":{offset}"));
        let wire = format!("{{\"marker\":{marker},\"marker_offset\":47{offset},\"first_reference\":12,\"first_reference_offset\":63,\"second_reference\":11,\"second_reference_offset\":76}}");
        let lane: super::DesignMirrorScopeTolerance = serde_json::from_str(&wire).expect("mirror scalar lane");
        assert_eq!(serde_json::to_string(&lane).expect("mirror scalar lane wire"), wire);
        assert_eq!(lane.marker.code(), marker);
    }
    for (marker, repeated) in [(0, None), (89, None), (94, None), (100, None), (61, Some(59)), (90, Some(59))] {
        let offset = repeated.map_or_else(String::new, |offset| format!(",\"repeated_marker_offset\":{offset}"));
        let wire = format!("{{\"marker\":{marker},\"marker_offset\":47{offset},\"first_reference\":12,\"first_reference_offset\":63,\"second_reference\":11,\"second_reference_offset\":76}}");
        let error = serde_json::from_str::<super::DesignMirrorScopeTolerance>(&wire).expect_err("invalid mirror scalar lane");
        assert!(error.to_string().contains("marker"));
        assert!(error.to_string().contains("repeated_marker_offset"));
    }
}

#[test]
fn mirror_derives_count_and_requires_one_tolerance_carrier() {
    let wire = r#"{"count":2,"count_record_index":11,"count_offset":0,"stitch_tolerance":0.001,"stitch_tolerance_offset":51,"stitch_tolerance_scope":{"marker":89,"marker_offset":47,"repeated_marker_offset":59,"first_reference":12,"first_reference_offset":63,"second_reference":11,"second_reference_offset":76},"seed_group_record_index":20,"plane_group_record_index":30}"#;
    let construction: super::DesignMirrorConstruction = serde_json::from_str(wire).expect("inline mirror tolerance");
    assert_eq!(serde_json::to_string(&construction).expect("inline mirror tolerance wire"), wire);
    let source: serde_json::Value = serde_json::from_str(wire).expect("mirror wire");
    for count in [0, 1, 3, u32::MAX] {
        let mut invalid = source.clone();
        invalid["count"] = count.into();
        let error = serde_json::from_value::<super::DesignMirrorConstruction>(invalid).expect_err("fixed mirror count");
        assert!(error.to_string().contains("count"));
    }
    for present in [false, true] {
        let mut invalid = source.clone();
        if present {
            invalid["stitch_tolerance_record_index"] = 12.into();
        } else {
            invalid.as_object_mut().expect("mirror object").remove("stitch_tolerance_scope");
        }
        let error = serde_json::from_value::<super::DesignMirrorConstruction>(invalid).expect_err("one mirror tolerance carrier");
        assert!(error.to_string().contains("stitch_tolerance_record_index"));
        assert!(error.to_string().contains("stitch_tolerance_scope"));
    }
}

#[test]
fn combine_requires_boolean_operation_local_target_and_nonempty_tools() {
    for operation in ["join", "cut", "intersect"] {
        for tools in [r#"[{"record_index":2}]"#, r#"[{"record_index":2},{"record_index":3}]"#] {
            let wire = format!("{{\"form\":\"standard\",\"operation\":\"{operation}\",\"operation_offset\":20,\"keep_tools\":false,\"keep_tools_offset\":25,\"target\":{{\"record_index\":1}},\"tools\":{tools}}}");
            let combine: super::DesignCombineOperation = serde_json::from_str(&wire).expect("combine operation");
            assert_eq!(serde_json::to_string(&combine).expect("combine operation wire"), wire);
        }
    }
    let base = serde_json::json!({
        "form": "standard", "operation": "join", "operation_offset": 20,
        "keep_tools": false, "keep_tools_offset": 25,
        "target": {"record_index": 1}, "tools": [{"record_index": 2}]
    });
    for (field, value) in [("operation", serde_json::json!("new_body")), ("tools", serde_json::json!([]))] {
        let mut invalid = base.clone();
        invalid[field] = value;
        let error = serde_json::from_value::<super::DesignCombineOperation>(invalid).expect_err("invalid combine form");
        assert!(error.to_string().contains(field));
    }
    let mut external_target = base;
    external_target["target"]["external_identity"] = serde_json::json!({
        "selector_asset_id": "asset", "selector_asset_id_offset": 0,
        "selector_context_id": "context", "selector_context_id_offset": 0,
        "occurrence_reference": 1, "occurrence_reference_offset": 0,
        "external_body_reference": 2, "external_body_reference_offset": 0,
        "external_segment": 1, "external_segment_offset": 0,
        "external_asset_id": "asset", "external_asset_id_offset": 0,
        "external_link_name": "link", "external_link_name_offset": 0
    });
    let error = serde_json::from_value::<super::DesignCombineOperation>(external_target).expect_err("local combine target");
    assert!(error.to_string().contains("target.external_identity"));
}

#[test]
fn thread_nominal_size_preserves_spelling_and_derives_numeric_wire_value() {
    let wire = |text: &str, number: &str| format!("{{\"form\":\"standard\",\"designation_offset\":38,\"designation\":\"M1\",\"nominal_size_text\":\"{text}\",\"nominal_size\":{number},\"profile\":\"ISO Metric profile\",\"major_diameter\":1.0,\"minor_diameter\":0.5,\"pitch\":0.1,\"pitch_diameter\":0.75,\"face_group_record_indices\":[10]}}");
    for (text, number) in [("1.0", "1.0"), ("+1.00", "1.0"), ("1.25e1", "12.5"), ("0.125", "0.125")] {
        let json = wire(text, number);
        let thread: super::DesignThreadConstruction = serde_json::from_str(&json).expect("thread nominal size");
        assert_eq!(thread.nominal_size.text(), text);
        assert_eq!(serde_json::to_string(&thread).expect("thread nominal-size wire"), json);
    }
    for text in ["", "-", "0", "-0.0", "-1", "NaN", "inf", "1e9999"] {
        let error = serde_json::from_str::<super::DesignThreadConstruction>(&wire(text, "1.0")).expect_err("invalid nominal-size spelling");
        assert!(error.to_string().contains("nominal_size_text"));
    }
    let error = serde_json::from_str::<super::DesignThreadConstruction>(&wire("1.0", "2.0")).expect_err("derived nominal size");
    assert!(error.to_string().contains("nominal_size"));
    assert!(error.to_string().contains("nominal_size_text"));
    let compact = wire("1.0", "1.0").replace("\"standard\"", "\"compact\"");
    for index in [1, u32::MAX] {
        let json = compact.replace("\"face_group_record_indices\"", &format!("\"trailing_reference_record_index\":{index},\"trailing_reference_offset\":100,\"face_group_record_indices\""));
        let thread: super::DesignThreadConstruction = serde_json::from_str(&json).expect("compact trailer reference");
        assert_eq!(serde_json::to_string(&thread).expect("compact trailer wire"), json);
    }
    let invalid = compact.replace("\"face_group_record_indices\"", "\"trailing_reference_record_index\":0,\"trailing_reference_offset\":100,\"face_group_record_indices\"");
    let error = serde_json::from_str::<super::DesignThreadConstruction>(&invalid).expect_err("nonzero compact trailer reference");
    assert!(error.to_string().contains("trailing_reference_record_index"));
}

#[test]
fn vertex_recipe_resolution_preserves_wire_and_rejects_partial_pairs() {
    use super::{DesignVertexRecipe, DesignVertexResolution, DesignWorkPlaneConstruction};

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
        let decoded: DesignVertexRecipe = serde_json::from_value(wire.clone()).expect("valid recipe");
        assert_eq!(serde_json::to_value(&decoded).expect("serialize recipe"), wire);
        assert_eq!(decoded.resolution.map(|value| (value.state_id, value.vertex_slot())), resolution);
        let plane_wire = serde_json::json!({
            "kind": "three_point", "placement_record_index": 9,
            "inputs": [wire.clone(), wire.clone(), wire]
        });
        let plane: DesignWorkPlaneConstruction = serde_json::from_value(plane_wire.clone()).expect("three-point plane");
        assert_eq!(serde_json::to_value(plane).expect("serialize plane"), plane_wire);
    }
    for (state, slot) in [(Some(4), None), (None, Some(0)), (Some(4), Some(-1))] {
        let mut wire = base.clone();
        if let Some(state) = state {
            wire["recipe_state_id"] = state.into();
        }
        if let Some(slot) = slot {
            wire["resolved_vertex_slot"] = slot.into();
        }
        let error = serde_json::from_value::<DesignVertexRecipe>(wire).expect_err("invalid resolution");
        assert!(error.to_string().contains("resolved_vertex_slot"));
    }
    assert!(DesignVertexResolution::new(4, -1).is_none());
}

#[test]
fn work_point_rules_preserve_supported_and_native_forms_without_aliases() {
    use super::DesignWorkPointRule;

    let input = serde_json::json!({"record_index": 2, "reference_offset": 10});
    for (kind, code, arity) in [
        ("circle_center", 5, 1), ("two_edge_intersection", 7, 2),
        ("three_plane_intersection", 8, 3), ("vertex", 10, 1),
        ("edge_plane_intersection", 14, 2), ("distance_on_edge", 20, 1),
    ] {
        let inputs = (0..arity).map(|_| input.clone()).collect::<Vec<_>>();
        let mut wire = serde_json::json!({"kind": kind});
        if arity == 1 {
            wire["input"] = input.clone();
        } else {
            wire["inputs"] = serde_json::json!(inputs);
        }
        let rule: DesignWorkPointRule = serde_json::from_value(wire.clone()).expect("supported rule");
        assert_eq!(rule.reference_type(), code);
        assert_eq!(serde_json::to_value(rule).expect("serialize rule"), wire);
        let alias = serde_json::json!({"kind": "native", "reference_type": code, "inputs": inputs});
        let error = serde_json::from_value::<DesignWorkPointRule>(alias).expect_err("native alias rejected");
        assert!(error.to_string().contains("reference_type"));
    }
    for (code, inputs) in [(0, vec![]), (u32::MAX, vec![input.clone()]), (5, vec![input.clone(), input])] {
        let wire = serde_json::json!({"kind": "native", "reference_type": code, "inputs": inputs});
        let rule: DesignWorkPointRule = serde_json::from_value(wire.clone()).expect("unassigned native form");
        assert_eq!(serde_json::to_value(rule).expect("serialize native form"), wire);
    }
}

#[test]
fn feature_kind_classifies_nonempty_native_names_at_construction() {
    for name in ["Fillet", "Esboço", "Extrusão", "Thread", "Unsupported", "fillet", " "] {
        let kind = DesignFeatureKind::try_from(name.to_owned()).expect("nonempty name");
        assert_eq!(kind.as_str(), name);
        assert_eq!(serde_json::to_value(&kind).expect("serialize kind"), serde_json::json!(name));
        let decoded: DesignFeatureKind = serde_json::from_value(serde_json::json!(name)).expect("decode kind");
        assert_eq!(decoded, kind);
        assert_eq!(matches!(kind, DesignFeatureKind::Native(_)), matches!(name, "Unsupported" | "fillet" | " "));
    }
    assert!(DesignFeatureKind::try_from(String::new()).is_err());
    let error = serde_json::from_value::<DesignFeatureKind>(serde_json::json!("")).expect_err("empty name rejected");
    assert!(error.to_string().contains("kind"));
}

#[test]
fn scope_feature_ordinal_preserves_positive_wire_values_and_rejects_zero() {
    let base = empty_scope(DesignFeatureKind::Fillet);
    for ordinal in [1, u32::MAX] {
        let mut wire = base.clone();
        wire["feature_ordinal"] = ordinal.into();
        let scope: DesignParameterScope = serde_json::from_value(wire.clone()).expect("positive ordinal");
        assert_eq!(scope.feature_ordinal.get(), ordinal);
        assert_eq!(serde_json::to_value(scope).expect("serialize scope"), wire);
    }
    let mut wire = base;
    wire["feature_ordinal"] = 0.into();
    let error = serde_json::from_value::<DesignParameterScope>(wire).expect_err("zero ordinal rejected");
    assert!(error.to_string().contains("feature_ordinal"));
}

#[test]
fn scope_history_state_offset_is_derived_and_wire_mismatches_are_rejected() {
    for kind_offset in [0_u64, 8, 100, u64::MAX] {
        let mut scope = DesignParameterScope::empty("scope", DesignFeatureKind::Sketch, 1);
        scope.kind_offset = kind_offset;
        let wire = serde_json::to_value(&scope).expect("serialize scope");
        assert_eq!(wire["history_state_id_offset"], kind_offset.saturating_sub(8));
        let decoded: DesignParameterScope = serde_json::from_value(wire.clone()).expect("valid derived offset");
        assert_eq!(decoded, scope);
        assert_eq!(serde_json::to_value(decoded).expect("serialize scope"), wire);
        let mut bad = wire;
        bad["history_state_id_offset"] = (kind_offset.saturating_sub(8) + 1).into();
        let error = serde_json::from_value::<DesignParameterScope>(bad).expect_err("mismatched offset rejected");
        assert!(error.to_string().contains("history_state_id_offset"));
    }
}

#[test]
fn bend_radius_requires_a_positive_finite_value() {
    for value in [0.0, -0.0, -1.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        assert!(super::DesignBendRadius::new(value).is_none());
    }
    for radius in [f64::MIN_POSITIVE, 0.25, f64::MAX] {
        let value = super::DesignBendRadius::new(radius).expect("positive finite radius");
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
        let operation: super::DesignHemOperation = serde_json::from_value(wire.clone()).expect("valid radius");
        assert_eq!(operation.bend_radius.get(), radius);
        assert_eq!(serde_json::to_value(operation).expect("serialize hem"), wire);
        for invalid in [0.0, -1.0] {
            let mut bad = wire.clone();
            bad["bend_radius"] = invalid.into();
            let error = serde_json::from_value::<super::DesignHemOperation>(bad).expect_err("invalid bend radius");
            assert!(error.to_string().contains("bend_radius"));
        }
    }
}
