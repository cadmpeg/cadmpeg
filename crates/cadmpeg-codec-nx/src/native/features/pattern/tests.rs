// SPDX-License-Identifier: Apache-2.0

use super::*;

use crate::native::features::test_support::check_lane_wire;

#[test]
fn pattern_fixed_lane_preserves_parallel_wire_and_requires_complete_tokens() {
    check_lane_wire::<FeaturePatternConstructionFixedLane>(
        r#"{"id":"lane","operation_label":"operation","construction_payload":"payload","ordinal":0,"values":[0.25,0.5],"markers":[48,176],"raw_values":[[32,0,0,0,0,0,0],[64,0,0,0,0,0,0]],"payload_offset":0,"value_payload_offsets":[18,26],"source_offset":100,"value_source_offsets":[118,126]}"#,
        &[
            "values",
            "markers",
            "raw_values",
            "value_payload_offsets",
            "value_source_offsets",
        ],
    );
}

#[test]
fn multi_instance_lane_preserves_wire_and_requires_complete_rows_and_references() {
    check_lane_wire::<FeatureMultiInstanceOutputLane>(
        r#"{"id":"lane","operation_label":"operation","declared_count":3,"selectors":[7,8],"raw_selectors":[[7],[8]],"ordinals":[2,2],"row_indices":[2,3],"instance_count":2,"trailing_object_indices":[9],"raw_trailing_object_indices":[[9]],"source_offset":100,"selector_source_offsets":[110,120],"trailing_object_index_source_offsets":[130]}"#,
        &[
            "selectors",
            "raw_selectors",
            "ordinals",
            "row_indices",
            "selector_source_offsets",
            "trailing_object_indices",
            "raw_trailing_object_indices",
            "trailing_object_index_source_offsets",
        ],
    );
}

#[test]
fn identical_instance_lane_preserves_wire_and_requires_complete_selectors() {
    let json = r#"{"id":"lane","operation_label":"operation","leading_schema_index":4,"count_schema_index":5,"row_schema_indices":[6,7,8],"declared_count":3,"selectors":[7,8],"raw_selectors":[[7],[8]],"source_offset":100,"selector_source_offsets":[110,120]}"#;
    check_lane_wire::<FeatureIdenticalInstanceOutputLane>(
        json,
        &["selectors", "raw_selectors", "selector_source_offsets"],
    );
    for (field, value) in [
        ("count_schema_index", serde_json::json!(253)),
        ("row_schema_indices", serde_json::json!([6, 7, 9])),
    ] {
        let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
        malformed[field] = value;
        let error =
            serde_json::from_value::<FeatureIdenticalInstanceOutputLane>(malformed).unwrap_err();
        assert!(error.to_string().contains(field));
    }
}

#[test]
fn pattern_transform_lane_preserves_wire_and_requires_complete_rows() {
    let columns = &[
        "encodings",
        "values",
        "raw_values",
        "selectors",
        "raw_selectors",
        "value_source_offsets",
        "selector_source_offsets",
    ];
    check_lane_wire::<FeaturePatternTransformLane>(
        r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"scalar_rows","declared_count":3,"encodings":["binary32","binary64"],"values":[2.5,4.0],"raw_values":[[80,32,0,0],[48,16,0,0,0,0,0,0]],"selectors":[7,8],"raw_selectors":[[7],[8]],"source_offset":100,"value_source_offsets":[110,120],"selector_source_offsets":[114,128]}"#,
        columns,
    );
    check_lane_wire::<FeaturePatternTransformLane>(
        r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"wide_rows","declared_count":2,"encodings":["binary64","binary64","binary64","binary64","exact_one"],"values":[2.5,4.0,5.0,6.0,1.0],"raw_values":[[48,4,0,0,0,0,0,0],[48,16,0,0,0,0,0,0],[48,20,0,0,0,0,0,0],[48,24,0,0,0,0,0,0],[1]],"selectors":[7],"raw_selectors":[[7]],"source_offset":100,"value_source_offsets":[110,118,126,134,142],"selector_source_offsets":[143]}"#,
        columns,
    );
}

#[test]
fn pattern_rows_reject_scalar_families_outside_their_layout() {
    let narrow = r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"scalar_rows","declared_count":2,"encodings":["exact_one"],"values":[1.0],"raw_values":[[1]],"selectors":[7],"raw_selectors":[[7]],"source_offset":100,"value_source_offsets":[110],"selector_source_offsets":[111]}"#;
    assert!(serde_json::from_str::<FeaturePatternTransformLane>(narrow).is_err());
    let wide = r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"wide_rows","declared_count":2,"encodings":["binary64","binary64","binary64","binary64","exact_one"],"values":[2.5,4.0,5.0,6.0,1.0],"raw_values":[[48,4,0,0,0,0,0,0],[48,16,0,0,0,0,0,0],[48,20,0,0,0,0,0,0],[48,24,0,0,0,0,0,0],[1]],"selectors":[7],"raw_selectors":[[7]],"source_offset":100,"value_source_offsets":[110,118,126,134,142],"selector_source_offsets":[143]}"#;
    let mut wrong_first: serde_json::Value = serde_json::from_str(wide).unwrap();
    wrong_first["encodings"][0] = serde_json::json!("binary32");
    wrong_first["raw_values"][0] = serde_json::json!([80, 32, 0, 0]);
    assert!(serde_json::from_value::<FeaturePatternTransformLane>(wrong_first).is_err());
    let mut wrong_terminal: serde_json::Value = serde_json::from_str(wide).unwrap();
    wrong_terminal["encodings"][4] = serde_json::json!("binary64");
    wrong_terminal["raw_values"][4] = serde_json::json!([47, 240, 0, 0, 0, 0, 0, 0]);
    assert!(serde_json::from_value::<FeaturePatternTransformLane>(wrong_terminal).is_err());
    assert!(serde_json::from_str::<FeaturePatternTransformLane>(
        &wide.replace("\"row_schema_index\":3", "\"row_schema_index\":0")
    )
    .is_err());
}

#[test]
fn pattern_counted_references_preserve_wire_and_reject_token_disagreement() {
    let json = r#"{"id":"lane","operation_label":"operation","declared_count":2,"object_indices":[1],"raw_object_indices":[[240,1]],"data_blocks":[null],"source_offset":18,"object_index_source_offsets":[20]}"#;
    let lane: crate::native::features::pattern::FeaturePatternCountedReferenceLane =
        serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&lane).unwrap(), json);
    for raw in [vec![240, 2], vec![255], vec![240], vec![240, 1, 0]] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["raw_object_indices"] = serde_json::json!([raw]);
        let error = serde_json::from_value::<
            crate::native::features::pattern::FeaturePatternCountedReferenceLane,
        >(wire)
        .unwrap_err();
        assert!(error.to_string().contains("raw_object_indices"), "{error}");
    }
}

#[test]
fn pattern_transform_selectors_require_matching_compact_tokens() {
    let json = r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"scalar_rows","declared_count":2,"encodings":["binary32"],"values":[2.5],"raw_values":[[80,32,0,0]],"selectors":[4096],"raw_selectors":[[144,0]],"source_offset":100,"value_source_offsets":[110],"selector_source_offsets":[114]}"#;
    check_lane_wire::<FeaturePatternTransformLane>(json, &[]);
    for (field, value) in [
        ("selectors", serde_json::json!([4097])),
        ("raw_selectors", serde_json::json!([[144]])),
        ("raw_selectors", serde_json::json!([[144, 0, 0]])),
        ("raw_selectors", serde_json::json!([[255]])),
    ] {
        let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
        malformed[field] = value;
        let error = serde_json::from_value::<FeaturePatternTransformLane>(malformed).unwrap_err();
        assert!(error.to_string().contains("selectors"), "{error}");
    }
}

#[test]
fn identical_instance_selectors_check_tokens_and_terminal_count_capacity() {
    let json = r#"{"id":"lane","operation_label":"operation","leading_schema_index":4,"count_schema_index":5,"row_schema_indices":[6,7,8],"declared_count":2,"selectors":[4096],"raw_selectors":[[144,0]],"source_offset":100,"selector_source_offsets":[110]}"#;
    check_lane_wire::<FeatureIdenticalInstanceOutputLane>(json, &[]);
    for (field, value) in [
        ("selectors", serde_json::json!([4097])),
        ("raw_selectors", serde_json::json!([[144]])),
        ("raw_selectors", serde_json::json!([[144, 0, 0]])),
        ("raw_selectors", serde_json::json!([[255]])),
    ] {
        let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
        malformed[field] = value;
        let error =
            serde_json::from_value::<FeatureIdenticalInstanceOutputLane>(malformed).unwrap_err();
        assert!(error.to_string().contains("selectors"), "{error}");
    }
    for count in [253, 254] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["declared_count"] = serde_json::json!(count + 1);
        wire["selectors"] = serde_json::json!(vec![1; count]);
        wire["raw_selectors"] = serde_json::json!(vec![vec![1]; count]);
        wire["selector_source_offsets"] = serde_json::json!(vec![110; count]);
        let result = serde_json::from_value::<FeatureIdenticalInstanceOutputLane>(wire.clone());
        if count == 253 {
            assert_eq!(serde_json::to_value(result.unwrap()).unwrap(), wire);
        } else {
            assert!(result.unwrap_err().to_string().contains("selectors"));
        }
    }
}

#[test]
fn multi_instance_groups_preserve_interleaving_and_reject_invalid_ordinals() {
    let json = r#"{"id":"lane","operation_label":"operation","declared_count":5,"selectors":[7,8,7,8],"raw_selectors":[[7],[8],[128,7],[128,8]],"ordinals":[2,2,3,3],"row_indices":[2,3,4,5],"instance_count":3,"trailing_object_indices":[9,256],"raw_trailing_object_indices":[[9],[144,1,0]],"source_offset":100,"selector_source_offsets":[110,120,130,140],"trailing_object_index_source_offsets":[150,160]}"#;
    check_lane_wire::<FeatureMultiInstanceOutputLane>(json, &[]);
    for (field, value, error_field) in [
        ("ordinals", serde_json::json!([1, 2, 3, 3]), "ordinals"),
        ("ordinals", serde_json::json!([2, 2, 2, 3]), "ordinals"),
        ("ordinals", serde_json::json!([2, 2, 4, 3]), "ordinals"),
        (
            "raw_selectors",
            serde_json::json!([[7], [8], [255], [128, 8]]),
            "selectors",
        ),
        (
            "raw_trailing_object_indices",
            serde_json::json!([[9], [240, 1]]),
            "trailing_object_indices",
        ),
        (
            "trailing_object_indices",
            serde_json::json!([9, 257]),
            "trailing_object_indices",
        ),
    ] {
        let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
        malformed[field] = value;
        let error =
            serde_json::from_value::<FeatureMultiInstanceOutputLane>(malformed).unwrap_err();
        assert!(error.to_string().contains(error_field), "{error}");
    }
    let mut incomplete: serde_json::Value = serde_json::from_str(json).unwrap();
    incomplete["declared_count"] = serde_json::json!(4);
    for field in [
        "selectors",
        "raw_selectors",
        "ordinals",
        "row_indices",
        "selector_source_offsets",
    ] {
        incomplete[field].as_array_mut().unwrap().pop();
    }
    let error = serde_json::from_value::<FeatureMultiInstanceOutputLane>(incomplete).unwrap_err();
    assert!(error.to_string().contains("trailing_object_indices"));
}

#[test]
fn pattern_counted_references_require_nonempty_framed_payload_tokens() {
    let json = r#"{"id":"lane","operation_label":"operation","declared_count":3,"object_indices":[1,258],"raw_object_indices":[[240,1],[241,1,2]],"data_blocks":[null,"block"],"source_offset":18,"object_index_source_offsets":[20,22]}"#;
    let lane: super::FeaturePatternCountedReferenceLane = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&lane).unwrap(), json);
    for (field, value) in [
        ("object_index_source_offsets", serde_json::json!([20, 23])),
        ("source_offset", serde_json::json!(u64::MAX)),
        ("raw_object_indices", serde_json::json!([[1], [241, 1, 2]])),
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = value;
        let error = serde_json::from_value::<super::FeaturePatternCountedReferenceLane>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains(field), "{error}");
    }
    let mut empty: serde_json::Value = serde_json::from_str(json).unwrap();
    empty["declared_count"] = serde_json::json!(1);
    for field in [
        "object_indices",
        "raw_object_indices",
        "data_blocks",
        "object_index_source_offsets",
    ] {
        empty[field] = serde_json::json!([]);
    }
    let error = serde_json::from_value::<super::FeaturePatternCountedReferenceLane>(empty)
        .unwrap_err()
        .to_string();
    assert!(error.contains("declared_count"), "{error}");
}
