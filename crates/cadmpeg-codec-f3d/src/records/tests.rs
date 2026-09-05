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
