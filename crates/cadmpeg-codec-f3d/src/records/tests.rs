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
