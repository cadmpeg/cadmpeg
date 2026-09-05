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
