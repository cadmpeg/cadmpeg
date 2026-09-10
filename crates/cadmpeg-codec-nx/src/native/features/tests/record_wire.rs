// SPDX-License-Identifier: Apache-2.0

use super::{FeatureBodyReference, FeatureOperationBodyWrite, FeatureOperationObjectReference};
use crate::native::features::{
    FeatureInputBlock, FeatureInputBlockIdentityGroup, FeatureOperationLabel,
};

#[test]
fn object_reference_retains_tagged_and_untagged_wire() {
    for json in [
        r#"{"id":"reference","operation_label":"operation","operation_record":"record","ordinal":0,"tag":23,"object_index":3,"raw_object_index":[3],"object_index_source_offset":10,"byte_len":9,"source_offset":7}"#,
        r#"{"id":"reference","operation_label":"operation","operation_record":"record","ordinal":0,"object_index":3,"raw_object_index":[3],"object_index_source_offset":10,"byte_len":10,"source_offset":7}"#,
    ] {
        let reference: FeatureOperationObjectReference = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&reference).unwrap(), json);
    }
}

#[test]
fn body_reference_retains_primary_and_ordered_wire() {
    for json in [
        r#"{"id":"reference","operation_label":"operation","body_object_index":3,"raw_body_object_index":[3],"source_offset":10}"#,
        r#"{"id":"reference","operation_label":"operation","ordinal":0,"body_object_index":3,"raw_body_object_index":[3],"source_offset":10}"#,
    ] {
        let reference: FeatureBodyReference = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&reference).unwrap(), json);
    }
}

#[test]
fn body_write_retains_labeled_and_unlabeled_wire() {
    for json in [
        r#"{"id":"write","operation_label":"operation","operation_record":"record","ordinal":0,"body_identity":1,"group_node":2,"raw_group_node":[2],"group_node_source_offset":11,"endpoint_tag":16,"body_image_object_index":3,"raw_body_image_object_index":[3],"body_image_object_index_source_offset":17,"byte_len":11,"source_offset":8}"#,
        r#"{"id":"write","operation_record":"record","ordinal":0,"body_identity":1,"group_node":2,"raw_group_node":[2],"group_node_source_offset":11,"endpoint_tag":16,"body_image_object_index":3,"raw_body_image_object_index":[3],"body_image_object_index_source_offset":17,"byte_len":11,"source_offset":8}"#,
    ] {
        let write: FeatureOperationBodyWrite = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&write).unwrap(), json);
    }
}

#[test]
fn payload_scalar_preserves_each_payload_owner_key() {
    for json in [
        r#"{"id":"scalar","operation_label":"operation","datum_csys_payload":"payload","ordinal":0,"field_code":100,"value":2.0,"raw_value":[48,0,0,0,0,0,0,0],"payload_offset":10,"source_offset":20}"#,
        r#"{"id":"scalar","operation_label":"operation","construction_payload":"payload","ordinal":0,"field_code":100,"value":2.0,"raw_value":[48,0,0,0,0,0,0,0],"payload_offset":10,"source_offset":20}"#,
    ] {
        let scalar: super::FeaturePayloadScalar = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&scalar).unwrap(), json);
    }
}

#[test]
fn scalar_pair_preserves_payload_key_and_discriminator_presence() {
    for json in [
        r#"{"id":"pair","operation_label":"operation","datum_csys_payload":"payload","ordinal":0,"values":[1.0,2.0],"raw_values":[[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0]],"payload_offset":10,"value_payload_offsets":[25,34],"source_offset":30,"value_source_offsets":[31,39],"discriminator":[8,2,3,1,3,1,192,69,4,0,128,134,2,0,3]}"#,
        r#"{"id":"pair","operation_label":"operation","datum_plane_payload":"payload","ordinal":0,"values":[1.0,2.0],"raw_values":[[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0]],"payload_offset":10,"value_payload_offsets":[28,37],"source_offset":30,"value_source_offsets":[31,39]}"#,
        r#"{"id":"pair","operation_label":"operation","construction_payload":"payload","ordinal":0,"values":[1.0,2.0],"raw_values":[[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0]],"payload_offset":10,"value_payload_offsets":[27,35],"source_offset":30,"value_source_offsets":[31,39],"discriminator":[47,47,65,0,3,1,3,1,192,69,4,0,128,134,2,0,3]}"#,
        r#"{"id":"pair","operation_label":"operation","surface_construction_payload":"payload","ordinal":0,"values":[1.0,2.0],"raw_values":[[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0]],"payload_offset":10,"value_payload_offsets":[25,34],"source_offset":30,"value_source_offsets":[31,39],"discriminator":[8,2,3,1,3,1,192,69,4,0,128,134,2,0,3]}"#,
    ] {
        let pair: super::FeaturePayloadScalarPair = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&pair).unwrap(), json);
    }
}

#[test]
fn construction_payload_preserves_each_ownership_form() {
    for owner in [
        r#""construction_inputs":"inputs""#,
        r#""operation_kind":"CPROJ","construction_references":["reference"]"#,
        r#""operation_kind":"CPROJ_CMB","construction_references":["reference"]"#,
        r#""reference_graph":"graph","group":"first""#,
        r#""reference_graph":"graph","group":"second""#,
        r#""operation_kind":"Pattern Feature","reference_layout":"canonical_graph","construction_references":["reference"]"#,
        r#""operation_kind":"Pattern Geometry","reference_layout":"compact_graph","construction_references":["reference"]"#,
        r#""index_lane":"lane""#,
        r#""construction":"construction""#,
    ] {
        let json = format!(
            r#"{{"id":"payload","operation_label":"operation",{owner},"data_blocks":["block"],"byte_len":8,"sha256":"d04b98f48e8f8bcc15c6ae5ac050801cd6dcfd428fb5f9e65c4e16e7807340fa","block_payload_offsets":[0],"block_byte_lengths":[8],"block_source_offsets":[10]}}"#
        );
        let payload: super::FeatureConstructionPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&payload).unwrap(), json);
    }
}

#[test]
fn construction_payload_rejects_untyped_operation_kinds() {
    let json = r#"{"id":"payload","operation_label":"operation","operation_kind":"other","construction_references":[],"data_blocks":[],"byte_len":0,"sha256":"d04b98f48e8f8bcc15c6ae5ac050801cd6dcfd428fb5f9e65c4e16e7807340fa","block_payload_offsets":[],"block_byte_lengths":[],"block_source_offsets":[]}"#;
    let error = serde_json::from_str::<super::FeatureConstructionPayload>(json).unwrap_err();
    assert!(error.to_string().contains("operation_kind"));
}

#[test]
fn datum_plane_payload_derives_terminal_index_count() {
    let json = r#"{"id":"payload","operation_label":"operation","datum_plane_header":"header","data_blocks":["block"],"byte_len":8,"sha256":"d04b98f48e8f8bcc15c6ae5ac050801cd6dcfd428fb5f9e65c4e16e7807340fa","block_payload_offsets":[0],"block_byte_lengths":[8],"block_source_offsets":[10],"index_lane_offset":2,"index_lane_declared_count":2,"index_lane_values":[1],"index_lane_raw_indices":[[1]],"index_lane_value_offsets":[4],"index_lane_trailer":0}"#;
    let payload: super::FeatureDatumPlanePayload = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&payload).unwrap(), json);
    for count in [0, 1, 3, 256] {
        let invalid = json.replace(
            "\"index_lane_declared_count\":2",
            &format!("\"index_lane_declared_count\":{count}"),
        );
        let error = serde_json::from_str::<super::FeatureDatumPlanePayload>(&invalid).unwrap_err();
        assert!(error.to_string().contains("index_lane_declared_count"));
    }
}

#[test]
fn datum_plane_payload_retains_checked_compact_tokens() {
    let json = r#"{"id":"payload","operation_label":"operation","datum_plane_header":"header","data_blocks":["block"],"byte_len":8,"sha256":"d04b98f48e8f8bcc15c6ae5ac050801cd6dcfd428fb5f9e65c4e16e7807340fa","block_payload_offsets":[0],"block_byte_lengths":[8],"block_source_offsets":[10],"index_lane_offset":2,"index_lane_declared_count":3,"index_lane_values":[4096,1],"index_lane_raw_indices":[[144,0],[128,1]],"index_lane_value_offsets":[4,6],"index_lane_trailer":0}"#;
    let payload: super::FeatureDatumPlanePayload = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&payload).unwrap(), json);
    for raw in [vec![255], vec![144], vec![144, 0, 0], vec![1]] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["index_lane_raw_indices"][0] = serde_json::json!(raw);
        let error = serde_json::from_value::<super::FeatureDatumPlanePayload>(wire).unwrap_err();
        assert!(error.to_string().contains("index_lane_values[0]"));
    }
    for count in [0, 254, 255] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["index_lane_declared_count"] = serde_json::json!(count + 1);
        wire["index_lane_values"] = serde_json::json!(vec![2; count]);
        wire["index_lane_raw_indices"] = serde_json::json!(vec![vec![2]; count]);
        wire["index_lane_value_offsets"] = serde_json::json!((4..4 + count).collect::<Vec<_>>());
        assert_eq!(
            serde_json::from_value::<super::FeatureDatumPlanePayload>(wire).is_ok(),
            count == 254
        );
    }
}

#[test]
fn sketch_construction_members_preserve_wire_and_reject_unpaired_blocks() {
    let json = r#"{"id":"inputs","operation_label":"operation","sketch_record":"sketch","member_references":["reference"],"member_data_blocks":["block"],"terminal_reference":"terminal","terminal_data_block":"last"}"#;
    let inputs: super::FeatureSketchConstructionInputs = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&inputs).unwrap(), json);
    for field in ["member_references", "member_data_blocks"] {
        let mut invalid: serde_json::Value = serde_json::from_str(json).unwrap();
        invalid[field] = serde_json::json!([]);
        let error =
            serde_json::from_value::<super::FeatureSketchConstructionInputs>(invalid).unwrap_err();
        assert!(error.to_string().contains(field));
    }
}

#[test]
fn block_construction_members_have_eighteen_paired_entries() {
    let references =
        serde_json::to_string(&(0..18).map(|n| format!("reference{n}")).collect::<Vec<_>>())
            .unwrap();
    let blocks =
        serde_json::to_string(&(0..18).map(|n| format!("block{n}")).collect::<Vec<_>>()).unwrap();
    let json = format!(
        r#"{{"id":"construction","operation_label":"operation","control":38,"member_references":{references},"member_data_blocks":{blocks},"terminal_reference":"terminal","terminal_data_block":"last"}}"#
    );
    let construction: super::FeatureBlockConstruction = serde_json::from_str(&json).unwrap();
    assert_eq!(serde_json::to_string(&construction).unwrap(), json);
    for fields in [
        vec!["member_references"],
        vec!["member_data_blocks"],
        vec!["member_references", "member_data_blocks"],
    ] {
        let mut invalid: serde_json::Value = serde_json::from_str(&json).unwrap();
        for field in fields {
            invalid[field].as_array_mut().unwrap().pop();
        }
        let error = serde_json::from_value::<super::FeatureBlockConstruction>(invalid).unwrap_err();
        assert!(error.to_string().contains("member_references"));
    }
}

#[test]
fn surface_branch_counts_are_derived_on_the_wire() {
    let member = r#"{"ordinal":0,"object_index":1,"raw_object_index":[240,1],"source_offset":8}"#;
    let terminal =
        r#"{"ordinal":1,"object_index":2,"raw_object_index":[240,2],"source_offset":20}"#;
    let branch = format!(
        r#"{{"ordinal":0,"mode":21,"declared_count":2,"state_lane":[0,0,0,0,0],"members":[{member}],"terminal":{terminal},"suffix":[129,88],"source_offset":5}}"#
    );
    let group = format!(
        r#"{{"id":"group","operation_label":"operation","declared_count":2,"branches":[{branch}],"terminator":[0,0,0,0,0,0,255,0,255,1],"source_offset":4}}"#
    );
    let decoded: crate::native::features::thru_curve_branches::FeatureThruCurveConstructionBranchGroup = serde_json::from_str(&group).unwrap();
    assert_eq!(serde_json::to_string(&decoded).unwrap(), group);
    let invalid = group.replace("[0,0,0,0,0]", "[0,0,0,0]");
    assert!(serde_json::from_str::<
        crate::native::features::thru_curve_branches::FeatureThruCurveConstructionBranchGroup,
    >(&invalid)
    .unwrap_err()
    .to_string()
    .contains("state_lane"));
    let invalid = group.replace("\"declared_count\":2", "\"declared_count\":3");
    assert!(serde_json::from_str::<
        crate::native::features::thru_curve_branches::FeatureThruCurveConstructionBranchGroup,
    >(&invalid)
    .unwrap_err()
    .to_string()
    .contains("declared_count"));
    let invalid = group.replacen("\"declared_count\":2", "\"declared_count\":3", 1);
    assert!(serde_json::from_str::<
        crate::native::features::thru_curve_branches::FeatureThruCurveConstructionBranchGroup,
    >(&invalid)
    .unwrap_err()
    .to_string()
    .contains("declared_count"));
    let member = r#"{"ordinal":0,"object_index":1,"raw_object_index":[240,1],"source_offset":8}"#;
    let terminal =
        r#"{"ordinal":1,"object_index":2,"raw_object_index":[240,2],"source_offset":18}"#;
    let surface = format!(
        r#"{{"id":"branch","operation_label":"operation","ordinal":0,"family":20,"header_code":19,"mode":64,"declared_count":2,"witnessed":false,"members":[{member}],"terminal":{terminal},"suffix":[129,88],"source_offset":5}}"#
    );
    let decoded: crate::native::features::surface_branches::FeatureSurfaceConstructionBranch =
        serde_json::from_str(&surface).unwrap();
    assert_eq!(serde_json::to_string(&decoded).unwrap(), surface);
    let invalid = surface.replace("\"declared_count\":2", "\"declared_count\":3");
    assert!(serde_json::from_str::<
        crate::native::features::surface_branches::FeatureSurfaceConstructionBranch,
    >(&invalid)
    .unwrap_err()
    .to_string()
    .contains("declared_count"));
}

#[test]
fn swp104_state_wire_preserves_independent_witness_and_absence() {
    let member = r#"{"ordinal":0,"object_index":1,"raw_object_index":[240,1],"source_offset":240}"#;
    let raw = "[47,164,122,225,71,174,20,123]";
    for (state, terminal_offset, byte_len) in [
        (
            r#""witnessed_count":4,"state_lane":[0,1,1,0,0,0,0]"#,
            254,
            57,
        ),
        (r#""witnessed_count":2,"state_lane":[0,0,0,0,0]"#, 252, 55),
        (r#""state_lane":[0,0,0,0,0]"#, 250, 53),
    ] {
        let terminal = format!(
            r#"{{"ordinal":1,"object_index":2,"raw_object_index":[240,2],"source_offset":{terminal_offset}}}"#
        );
        let json = format!(
            r#"{{"id":"branch","operation_label":"operation","discriminator":33,"scalars":[0.04,0.04,0.04,0.04],"raw_scalars":[{raw},{raw},{raw},{raw}],"leading_zero":false,"mode":35,"declared_count":2,{state},"members":[{member}],"terminal":{terminal},"byte_len":{byte_len},"source_offset":200}}"#
        );
        let branch: crate::native::features::swp104_branch::FeatureSwp104LeadingBranch =
            serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&branch).unwrap(), json);
        let invalid = json.replace("\"declared_count\":2", "\"declared_count\":3");
        assert!(serde_json::from_str::<
            crate::native::features::swp104_branch::FeatureSwp104LeadingBranch,
        >(&invalid)
        .unwrap_err()
        .to_string()
        .contains("declared_count"));
        let invalid = json.replace("0.04", "0.05");
        assert!(serde_json::from_str::<
            crate::native::features::swp104_branch::FeatureSwp104LeadingBranch,
        >(&invalid)
        .unwrap_err()
        .to_string()
        .contains("scalars"));
        for (path, bad) in [
            (vec!["byte_len"], serde_json::json!(byte_len + 1)),
            (vec!["source_offset"], serde_json::json!(u64::MAX)),
            (vec!["terminal", "ordinal"], serde_json::json!(0)),
            (
                vec!["terminal", "source_offset"],
                serde_json::json!(terminal_offset + 1),
            ),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(&json).unwrap();
            let mut field = &mut invalid;
            for key in path {
                field = &mut field[key];
            }
            *field = bad;
            assert!(serde_json::from_value::<
                crate::native::features::swp104_branch::FeatureSwp104LeadingBranch,
            >(invalid)
            .is_err());
        }
        for field in ["ordinal", "source_offset"] {
            let mut invalid: serde_json::Value = serde_json::from_str(&json).unwrap();
            invalid["members"][0][field] = serde_json::json!(99);
            assert!(serde_json::from_value::<
                crate::native::features::swp104_branch::FeatureSwp104LeadingBranch,
            >(invalid)
            .is_err());
        }
        for field in ["discriminator", "mode"] {
            let mut invalid: serde_json::Value = serde_json::from_str(&json).unwrap();
            invalid[field] = serde_json::json!(0);
            assert!(serde_json::from_value::<
                crate::native::features::swp104_branch::FeatureSwp104LeadingBranch,
            >(invalid)
            .is_err());
        }
    }
}

#[test]
fn construction_scalar_wire_derives_the_number_from_its_atom() {
    for payload in ["datum_csys_payload", "construction_payload"] {
        let json = format!(
            r#"{{"id":"scalar","operation_label":"operation","{payload}":"payload","ordinal":0,"field_code":100,"value":1.0,"raw_value":[47,240,0,0,0,0,0,0],"payload_offset":8,"source_offset":108}}"#
        );
        let scalar: super::FeaturePayloadScalar = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&scalar).unwrap(), json);
        for invalid in [
            json.replace("1.0", "2.0"),
            json.replace("[47,240", "[0,240"),
        ] {
            let error = serde_json::from_str::<super::FeaturePayloadScalar>(&invalid).unwrap_err();
            assert!(error.to_string().contains("value/raw_value"));
        }
    }
}

#[test]
fn named_point_wire_preserves_scalar_atoms_and_frame_offsets() {
    let json = r#"{"id":"point","name":"Point1","data_blocks":["first","second"],"values":[1.0,2.0],"raw_values":[[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0]],"value_source_offsets":[10,20],"source_offset":5}"#;
    let point: super::OffsetStoreNamedPoint = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&point).unwrap(), json);
    let invalid = json.replace("1.0", "3.0");
    assert!(
        serde_json::from_str::<super::OffsetStoreNamedPoint>(&invalid)
            .unwrap_err()
            .to_string()
            .contains("values/raw_values")
    );
}

#[test]
fn binary64_pair_wire_preserves_all_payload_owner_forms() {
    for payload in [
        "datum_csys_payload",
        "datum_plane_payload",
        "construction_payload",
        "surface_construction_payload",
    ] {
        let discriminator = if payload == "datum_plane_payload" {
            ""
        } else {
            r#","discriminator":[8,2,3,1,3,1,192,69,4,0,128,134,2,0,3]"#
        };
        let positions = if payload == "datum_plane_payload" {
            "23,32"
        } else {
            "20,29"
        };
        let json = format!(
            r#"{{"id":"pair","operation_label":"operation","{payload}":"payload","ordinal":0,"values":[1.0,2.0],"raw_values":[[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0]],"payload_offset":5,"value_payload_offsets":[{positions}],"source_offset":105,"value_source_offsets":[120,129]{discriminator}}}"#
        );
        let pair: super::FeaturePayloadScalarPair = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&pair).unwrap(), json);
        let invalid = json.replace("1.0", "3.0");
        assert!(
            serde_json::from_str::<super::FeaturePayloadScalarPair>(&invalid)
                .unwrap_err()
                .to_string()
                .contains("values/raw_values")
        );
    }
    let json = r#"{"id":"header","operation_label":"operation","scalars":[1.0,2.0],"raw_scalars":[[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0]],"source_offset":100}"#;
    let header: super::FeatureExtrudePayloadHeader = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&header).unwrap(), json);
    let invalid = json.replace("1.0", "3.0");
    assert!(
        serde_json::from_str::<super::FeatureExtrudePayloadHeader>(&invalid)
            .unwrap_err()
            .to_string()
            .contains("scalars")
    );
}

#[test]
fn body_scalar_triple_wire_checks_the_atom_value_and_width() {
    let json = r#"{"id":"triple","operation_label":"operation","body_reference_ordinal":0,"body_object_index":10,"branch":28,"values":[0.0,3.0,1.0],"encodings":["zero","binary32","binary64"],"raw_values":[[0],[80,64,0,0],[47,240,0,0,0,0,0,0]],"source_offsets":[100,101,105]}"#;
    let triple: crate::native::features::body_scalar_triple::FeatureOperationBodyScalarTriple =
        serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&triple).unwrap(), json);
    for invalid in [
        json.replace("3.0", "4.0"),
        json.replacen("\"binary32\"", "\"binary64\"", 1),
        json.replace("[[0],", "[[0,0],"),
    ] {
        assert!(serde_json::from_str::<
            crate::native::features::body_scalar_triple::FeatureOperationBodyScalarTriple,
        >(&invalid)
        .is_err());
    }
}

#[test]
fn payload_text_records_preserve_unicode_and_reject_control_text() {
    let json =
        r#"{"id":"text","operation_record":"record","ordinal":0,"value":" × ","source_offset":10}"#;
    let record: super::FeaturePayloadString = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&record).unwrap(), json);
    for value in ["", "\0", "\n", "\u{85}"] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["value"] = value.into();
        assert!(serde_json::from_value::<super::FeaturePayloadString>(wire)
            .unwrap_err()
            .to_string()
            .contains("value"));
    }
}

#[test]
fn construction_reference_records_preserve_wire_and_check_tokens() {
    fn check<T: serde::Serialize + serde::de::DeserializeOwned>(json: &str) {
        let record: T = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
        let wire: serde_json::Value = serde_json::from_str(json).unwrap();
        for (field, invalid) in [
            ("object_index", serde_json::json!(256)),
            ("raw_object_index", serde_json::json!([240])),
            ("raw_object_index", serde_json::json!([240, 1, 0])),
            ("raw_object_index", serde_json::json!([255])),
            ("raw_object_index", serde_json::json!([241, 0, 1])),
        ] {
            let mut malformed = wire.clone();
            malformed[field] = invalid;
            match serde_json::from_value::<T>(malformed) {
                Err(error) => assert!(error.to_string().contains(field)),
                Ok(_) => panic!("invalid reference token accepted"),
            }
        }
    }
    check::<super::FeatureSketchReference>(
        r#"{"id":"r","operation_label":"o","ordinal":0,"declared_count":1,"terminal":true,"object_index":1,"raw_object_index":[240,1],"source_offset":10}"#,
    );
    check::<super::FeatureProjectedCurveReference>(
        r#"{"id":"r","operation_label":"o","ordinal":0,"object_index":1,"raw_object_index":[240,1],"source_offset":10}"#,
    );
    check::<crate::native::features::pattern::FeaturePatternReference>(
        r#"{"id":"r","operation_label":"o","layout":"canonical_graph","ordinal":0,"object_index":1,"raw_object_index":[240,1],"source_offset":10}"#,
    );
    check::<super::FeaturePointConstructionHeader>(
        r#"{"id":"r","operation_label":"o","object_index":1,"raw_object_index":[240,1],"mode":2,"source_offset":10}"#,
    );
    check::<crate::native::features::draft::FeatureDraftConstructionReference>(
        r#"{"id":"r","operation_label":"o","ordinal":0,"object_index":1,"raw_object_index":[240,1],"source_offset":10}"#,
    );
    check::<super::FeatureSurfaceConstructionReference>(
        r#"{"id":"r","operation_label":"o","ordinal":0,"object_index":1,"raw_object_index":[240,1],"source_offset":10}"#,
    );
    check::<super::FeatureExtrudeProfileReference>(
        r#"{"id":"r","operation_label":"o","ordinal":0,"field_tag":1,"object_index":1,"raw_object_index":[240,1],"source_offset":10}"#,
    );
    check::<crate::native::features::block_reference::FeatureBlockConstructionReference>(
        r#"{"id":"r","operation_label":"o","control":1,"ordinal":0,"terminal":false,"object_index":1,"raw_object_index":[240,1],"source_offset":10}"#,
    );
}

#[test]
fn fixed_reference_groups_preserve_wire_and_check_each_token() {
    fn check<T: serde::Serialize + serde::de::DeserializeOwned>(json: &str, raw_field: &str) {
        let record: T = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
        let wire: serde_json::Value = serde_json::from_str(json).unwrap();
        for slot in 0..wire[raw_field].as_array().unwrap().len() {
            let mut malformed = wire.clone();
            malformed[raw_field][slot] = serde_json::json!([255]);
            match serde_json::from_value::<T>(malformed) {
                Err(error) => assert!(error.to_string().contains(raw_field)),
                Ok(_) => panic!("invalid grouped reference token accepted"),
            }
        }
    }
    check::<super::FeatureDatumCsysConstruction>(
        r#"{"id":"c","operation_label":"o","control":19,"object_indices":[0,1,2,3,4,5,6,7],"raw_object_indices":[[240,0],[240,1],[240,2],[240,3],[240,4],[240,5],[240,6],[240,7]],"data_blocks":["a","b","c","d","e","f","g","h"],"source_offsets":[14,16,18,20,22,24,26,28]}"#,
        "raw_object_indices",
    );
    let hole = r#"{"id":"c","operation_label":"o","selector":70,"branch":17,"object_indices":[1,2,3,4],"raw_object_indices":[[240,1],[240,2],[240,3],[240,4]],"data_blocks":["a","b","c","d"],"payload_offset":20,"source_offset":120,"reference_source_offsets":[132,134,141,143]}"#;
    check::<crate::native::features::holes::FeatureHolePackageConstructionGroupLane>(
        hole,
        "raw_object_indices",
    );
    for field in ["selector", "branch"] {
        let mut invalid: serde_json::Value = serde_json::from_str(hole).unwrap();
        invalid[field] = serde_json::json!(0);
        assert!(serde_json::from_value::<
            crate::native::features::holes::FeatureHolePackageConstructionGroupLane,
        >(invalid)
        .is_err());
    }
    let fset = r#"{"id":"g","operation_label":"o","selector":"s","first_object_indices":[1,2],"raw_first_object_indices":[[144,0,1],[144,0,2]],"first_data_blocks":["a",null],"second_object_indices":[3,4,5],"raw_second_object_indices":[[144,0,3],[144,0,4],[144,0,5]],"second_data_blocks":[null,"d","e"],"source_offset":10,"first_source_offsets":[14,17],"second_source_offsets":[21,24,27]}"#;
    check::<crate::native::features::fset::FeatureFsetReferenceGraph>(
        fset,
        "raw_first_object_indices",
    );
    check::<crate::native::features::fset::FeatureFsetReferenceGraph>(
        fset,
        "raw_second_object_indices",
    );
}

#[test]
fn delete_reference_fields_preserve_wire_and_reject_invalid_null_slots() {
    let wire = serde_json::json!({
        "id": "delete", "operation_label": "operation", "control": 12,
        "object_indices": [32, null, 520, 521, null],
        "raw_object_indices": [[240, 32], [255], [241, 2, 8], [241, 2, 9], [255]],
        "data_blocks": ["block-32", null, "block-520", null, null],
        "source_offset": 100,
        "object_index_source_offsets": [107, 109, 110, 113, 116]
    });
    let field: crate::native::features::delete::FeatureDeleteReferenceField =
        serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(field).unwrap(), wire);
    for slot in [1, 4] {
        let mut invalid = wire.clone();
        invalid["raw_object_indices"][slot] = serde_json::json!([]);
        assert!(serde_json::from_value::<
            crate::native::features::delete::FeatureDeleteReferenceField,
        >(invalid)
        .is_err());
        let mut invalid = wire.clone();
        invalid["data_blocks"][slot] = serde_json::json!("block");
        assert!(serde_json::from_value::<
            crate::native::features::delete::FeatureDeleteReferenceField,
        >(invalid)
        .is_err());
    }
    for slot in [0, 2, 3] {
        let mut invalid = wire.clone();
        invalid["object_indices"][slot] = serde_json::json!(999);
        assert!(serde_json::from_value::<
            crate::native::features::delete::FeatureDeleteReferenceField,
        >(invalid)
        .is_err());
    }
}

#[test]
fn body_11_continuation_preserves_wire_and_checks_both_token_grammars() {
    let wire = r#"{"id":"continuation","operation_label":"operation","body_reference_ordinal":0,"body_object_index":114,"continuation_index":67,"raw_continuation_index":[128,67],"continuation_source_offset":126,"terminal_object_index":113,"raw_terminal_object_index":[113],"terminal_source_offset":131}"#;
    let record: super::super::FeatureOperationBody11Continuation =
        serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&record).unwrap(), wire);
    for field in ["continuation_index", "terminal_object_index"] {
        let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
        invalid[field] = serde_json::json!(999);
        let error =
            serde_json::from_value::<super::super::FeatureOperationBody11Continuation>(invalid)
                .unwrap_err();
        assert!(error.to_string().contains(field));
    }
    for field in ["raw_continuation_index", "raw_terminal_object_index"] {
        let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
        invalid[field] = serde_json::json!([255]);
        assert!(
            serde_json::from_value::<super::super::FeatureOperationBody11Continuation>(invalid)
                .is_err()
        );
    }
}

#[test]
fn operation_body_member_preserves_exact_compact_token_and_rejects_invalid_wire() {
    for (value, raw) in [(1, "[1]"), (1, "[128,1]"), (4097, "[144,1]")] {
        let wire = format!(
            r#"{{"id":"member","operation_label":"operation","body_reference_ordinal":0,"body_object_index":66,"ordinal":0,"member_index":{value},"raw_member_index":{raw},"source_offset":122}}"#
        );
        let record: super::super::FeatureOperationBodyMember = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), wire);
        for invalid_raw in [
            serde_json::json!([]),
            serde_json::json!([255]),
            serde_json::json!([128]),
            serde_json::json!([144, 0, 1]),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(&wire).unwrap();
            invalid["raw_member_index"] = invalid_raw;
            let error = serde_json::from_value::<super::super::FeatureOperationBodyMember>(invalid)
                .unwrap_err();
            assert!(error.to_string().contains("member_index"));
        }
        let mut invalid: serde_json::Value = serde_json::from_str(&wire).unwrap();
        invalid["member_index"] = serde_json::json!(value + 1);
        assert!(
            serde_json::from_value::<super::super::FeatureOperationBodyMember>(invalid).is_err()
        );
    }
}

#[test]
fn operation_body_operand_preserves_exact_token_and_optional_relations() {
    for relations in [
        "",
        r#", "operand_data_block":"block", "segment_body_bindings":["binding"]"#,
    ] {
        let wire = format!(r#"{{"id":"operand","operation_label":"operation","body_object_index":66,"body_reference_ordinal":0,"ordinal":0,"operand_object_index":4097,"raw_operand_object_index":[144,1]{relations},"source_offset":122}}"#).replace(", ", ",");
        let record: super::super::FeatureOperationBodyOperand =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), wire);
        for raw in [
            serde_json::json!([]),
            serde_json::json!([255]),
            serde_json::json!([144]),
            serde_json::json!([144, 0, 1]),
            serde_json::json!([1]),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(&wire).unwrap();
            invalid["raw_operand_object_index"] = raw;
            let error =
                serde_json::from_value::<super::super::FeatureOperationBodyOperand>(invalid)
                    .unwrap_err();
            assert!(error.to_string().contains("operand_object_index"));
        }
    }
}

#[test]
fn operation_object_reference_requires_canonical_feature_token() {
    for (value, raw, byte_len) in [
        (0, "[0]", 10),
        (128, "[128,128]", 11),
        (4096, "[144,16,0]", 12),
    ] {
        let wire = format!(
            r#"{{"id":"reference","operation_label":"operation","operation_record":"record","ordinal":0,"object_index":{value},"raw_object_index":{raw},"object_index_source_offset":10,"byte_len":{byte_len},"source_offset":7}}"#
        );
        let record: FeatureOperationObjectReference = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), wire);
        for invalid_raw in [
            serde_json::json!([]),
            serde_json::json!([255]),
            serde_json::json!([128, 0]),
            serde_json::json!([144, 0, 0]),
            serde_json::json!([240, 0]),
            serde_json::json!([144, 16]),
            serde_json::json!([0, 0]),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(&wire).unwrap();
            invalid["raw_object_index"] = invalid_raw;
            assert!(
                serde_json::from_value::<FeatureOperationObjectReference>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("object_index/raw_object_index")
            );
        }
        let mut invalid: serde_json::Value = serde_json::from_str(&wire).unwrap();
        invalid["object_index"] = serde_json::json!(value + 1);
        assert!(serde_json::from_value::<FeatureOperationObjectReference>(invalid).is_err());
    }
}

#[test]
fn body_reference_preserves_alternate_widths_and_rejects_invalid_tokens() {
    for (value, raw) in [
        (0, "[0]"),
        (0, "[128,0]"),
        (0, "[144,0,0]"),
        (6466, "[144,25,66]"),
    ] {
        let wire = format!(
            r#"{{"id":"reference","operation_label":"operation","body_object_index":{value},"raw_body_object_index":{raw},"source_offset":10}}"#
        );
        let record: FeatureBodyReference = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), wire);
        for raw in [
            serde_json::json!([]),
            serde_json::json!([255]),
            serde_json::json!([144, 0]),
            serde_json::json!([240, 0]),
            serde_json::json!([0, 0]),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(&wire).unwrap();
            invalid["raw_body_object_index"] = raw;
            assert!(serde_json::from_value::<FeatureBodyReference>(invalid)
                .unwrap_err()
                .to_string()
                .contains("body_object_index/raw_body_object_index"));
        }
        let mut invalid: serde_json::Value = serde_json::from_str(&wire).unwrap();
        invalid["body_object_index"] = serde_json::json!(value + 1);
        assert!(serde_json::from_value::<FeatureBodyReference>(invalid).is_err());
    }
}

#[test]
fn direct_reference_wire_rejects_tag_and_position_drift() {
    let wire = serde_json::json!({"id":"reference", "operation_label":"operation", "operation_record":"record",
        "ordinal":0, "tag":23, "object_index":1, "raw_object_index":[1],
        "object_index_source_offset":10, "byte_len":9, "source_offset":7});
    for (field, value) in [
        ("tag", serde_json::json!(2)),
        ("byte_len", serde_json::json!(4)),
        ("object_index_source_offset", serde_json::json!(11)),
        ("source_offset", serde_json::json!(u64::MAX)),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<FeatureOperationObjectReference>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field)
        );
    }
}

#[test]
fn input_block_reference_preserves_header_encodings_and_rejects_invalid_pairs() {
    for (value, raw) in [
        (0, "[0]"),
        (0, "[128,0]"),
        (0, "[144,0,0]"),
        (65535, "[144,255,255]"),
    ] {
        let wire = format!(
            r#"{{"id":"input","operation_label":"operation","input_slot":0,"object_index":{value},"raw_object_index":{raw},"data_block":"block","source_offset":10}}"#
        );
        let record: FeatureInputBlock = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), wire);
        assert_eq!(record.object.value(), value);
        for raw in [
            serde_json::json!([]),
            serde_json::json!([255]),
            serde_json::json!([144, 0]),
            serde_json::json!([240, 0]),
            serde_json::json!([241, 1, 0]),
            serde_json::json!([0, 0]),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(&wire).unwrap();
            invalid["raw_object_index"] = raw;
            assert!(serde_json::from_value::<FeatureInputBlock>(invalid)
                .unwrap_err()
                .to_string()
                .contains("raw_object_index"));
        }
        let mut invalid: serde_json::Value = serde_json::from_str(&wire).unwrap();
        invalid["object_index"] = serde_json::json!(value + 1);
        assert!(serde_json::from_value::<FeatureInputBlock>(invalid)
            .unwrap_err()
            .to_string()
            .contains("object_index"));
    }
}

#[test]
fn operation_label_wire_preserves_nullable_header_tokens() {
    for identity in ["", r#","stable_identity":"stable""#] {
        let wire = format!(
            r#"{{"id":"label","section_link":"section","ordinal":0,"value":"EXTRUDE","object_indices":[null,0,0,0],"raw_object_indices":[[255],[0],[128,0],[144,0,0]]{identity},"source_offset":19}}"#
        );
        let label: FeatureOperationLabel = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&label).unwrap(), wire);
        for (field, slot, value) in [
            ("object_indices", 0, serde_json::json!(0)),
            ("object_indices", 1, serde_json::json!(null)),
            ("object_indices", 2, serde_json::json!(1)),
            ("raw_object_indices", 0, serde_json::json!([])),
            ("raw_object_indices", 0, serde_json::json!([0])),
            ("raw_object_indices", 3, serde_json::json!([144, 0])),
            ("raw_object_indices", 3, serde_json::json!([240, 0])),
            ("raw_object_indices", 3, serde_json::json!([0, 0])),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(&wire).unwrap();
            invalid[field][slot] = value;
            assert!(serde_json::from_value::<FeatureOperationLabel>(invalid)
                .unwrap_err()
                .to_string()
                .contains("object_indices"));
        }
    }
}

#[test]
fn operation_input_wire_requires_one_of_the_four_header_slots() {
    for slot in 0..4 {
        let wire = format!(
            r#"{{"id":"input","operation_label":"operation","input_slot":{slot},"object_index":0,"raw_object_index":[0],"data_block":"block","source_offset":10}}"#
        );
        let input: FeatureInputBlock = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&input).unwrap(), wire);
    }
    for slot in [4, 255] {
        let wire = serde_json::json!({"id":"input","operation_label":"operation","input_slot":slot,
            "object_index":0,"raw_object_index":[0],"data_block":"block","source_offset":10});
        assert!(serde_json::from_value::<FeatureInputBlock>(wire)
            .unwrap_err()
            .to_string()
            .contains("input_slot"));
        let group = serde_json::json!({"id":"group","data_block":"block","input_blocks":["input"],
            "operation_labels":["operation"],"input_slots":[slot],"source_offsets":[10]});
        assert!(
            serde_json::from_value::<FeatureInputBlockIdentityGroup>(group)
                .unwrap_err()
                .to_string()
                .contains("input_slots")
        );
    }
}

#[test]
fn sketch_reference_wire_derives_terminal_and_retains_zero_count_form() {
    use crate::native::features::FeatureSketchReference;
    for (count, ordinal, terminal) in [(0, 0, true), (1, 0, true), (2, 0, false), (2, 1, true)] {
        for target in ["", r#","data_block":"block""#] {
            let wire = format!(
                r#"{{"id":"reference","operation_label":"sketch","ordinal":{ordinal},"declared_count":{count},"terminal":{terminal},"object_index":66,"raw_object_index":[240,66]{target},"source_offset":100}}"#
            );
            let reference: FeatureSketchReference = serde_json::from_str(&wire).unwrap();
            assert_eq!(serde_json::to_string(&reference).unwrap(), wire);
            let mut invalid = serde_json::to_value(&reference).unwrap();
            invalid["ordinal"] = serde_json::json!(count.max(1));
            assert!(serde_json::from_value::<FeatureSketchReference>(invalid)
                .unwrap_err()
                .to_string()
                .contains("ordinal"));
            let mut invalid = serde_json::to_value(&reference).unwrap();
            invalid["terminal"] = serde_json::json!(!terminal);
            assert!(serde_json::from_value::<FeatureSketchReference>(invalid)
                .unwrap_err()
                .to_string()
                .contains("terminal"));
        }
    }
}

#[test]
fn datum_plane_payload_rejects_detached_member_positions_and_frame_overflow() {
    let json = r#"{"id":"payload","operation_label":"operation","datum_plane_header":"header","data_blocks":["block"],"byte_len":8,"sha256":"d04b98f48e8f8bcc15c6ae5ac050801cd6dcfd428fb5f9e65c4e16e7807340fa","block_payload_offsets":[0],"block_byte_lengths":[8],"block_source_offsets":[10],"index_lane_offset":2,"index_lane_declared_count":3,"index_lane_values":[4096,1],"index_lane_raw_indices":[[144,0],[128,1]],"index_lane_value_offsets":[4,6],"index_lane_trailer":0}"#;
    for offsets in [[3, 6], [4, 5], [4, u64::MAX]] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["index_lane_value_offsets"] = serde_json::json!(offsets);
        let error = serde_json::from_value::<super::FeatureDatumPlanePayload>(wire).unwrap_err();
        assert!(error.to_string().contains("index_lane_value_offsets"));
    }
    let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
    let origin = u64::MAX - 11;
    wire["index_lane_offset"] = serde_json::json!(origin);
    wire["index_lane_value_offsets"] = serde_json::json!([origin + 2, origin + 4]);
    let payload: super::FeatureDatumPlanePayload = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(payload).unwrap(), wire);
    wire["index_lane_offset"] = serde_json::json!(origin + 1);
    wire["index_lane_value_offsets"] = serde_json::json!([origin + 3, origin + 5]);
    let error = serde_json::from_value::<super::FeatureDatumPlanePayload>(wire).unwrap_err();
    assert!(error.to_string().contains("index_lane_offset"));
}

#[test]
fn binary64_pair_wire_requires_owner_form_and_complete_payload_extent() {
    let forms = [
        (
            "datum_csys_payload",
            Some(vec![8, 2, 3, 1, 3, 1, 192, 69, 4, 0, 128, 134, 2, 0, 3]),
            15_u64,
            1_u64,
        ),
        (
            "surface_construction_payload",
            Some(vec![
                8, 2, 3, 1, 129, 2, 1, 192, 69, 4, 0, 128, 134, 2, 0, 3,
            ]),
            16,
            1,
        ),
        ("datum_plane_payload", None, 18, 1),
        (
            "construction_payload",
            Some(vec![
                47, 47, 65, 0, 3, 1, 3, 1, 192, 69, 4, 0, 128, 134, 2, 0, 3,
            ]),
            17,
            0,
        ),
    ];
    for (owner, discriminator, prefix, separator) in forms {
        let origin = u64::MAX - prefix - 16 - separator;
        let mut wire = serde_json::json!({
            "id": "pair", "operation_label": "operation", "ordinal": 0,
            "values": [1.0,2.0], "raw_values": [[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0]],
            "payload_offset": origin, "value_payload_offsets": [origin + prefix, origin + prefix + 8 + separator],
            "source_offset": 300, "value_source_offsets": [900,20]
        });
        wire[owner] = "payload".into();
        if let Some(discriminator) = discriminator {
            wire["discriminator"] = serde_json::json!(discriminator);
        }
        let record: super::FeaturePayloadScalarPair = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
        for slot in 0..2 {
            let mut invalid = wire.clone();
            invalid["value_payload_offsets"][slot] = serde_json::json!(origin);
            assert!(
                serde_json::from_value::<super::FeaturePayloadScalarPair>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("value_payload_offsets")
            );
        }
        let mut invalid = wire.clone();
        invalid["payload_offset"] = serde_json::json!(origin + 1);
        assert!(
            serde_json::from_value::<super::FeaturePayloadScalarPair>(invalid)
                .unwrap_err()
                .to_string()
                .contains("payload_offset")
        );
        if owner != "datum_plane_payload" {
            for discriminator in [
                vec![],
                vec![8, 2, 3, 1, 3, 1],
                vec![0, 0, 65, 0, 3, 1, 3, 1, 192, 69, 4, 0, 128, 134, 2, 0, 3],
            ] {
                let mut invalid = wire.clone();
                invalid["discriminator"] = serde_json::json!(discriminator);
                assert!(
                    serde_json::from_value::<super::FeaturePayloadScalarPair>(invalid)
                        .unwrap_err()
                        .to_string()
                        .contains("discriminator")
                );
            }
        }
        if owner == "construction_payload" {
            let mut invalid = wire.clone();
            invalid.as_object_mut().unwrap().remove(owner);
            invalid["datum_csys_payload"] = "payload".into();
            assert!(
                serde_json::from_value::<super::FeaturePayloadScalarPair>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("discriminator")
            );
        }
    }
}

#[test]
fn datum_csys_wire_derives_offsets_from_the_complete_payload_frame() {
    let wire = r#"{"id":"c","operation_label":"o","control":255,"object_indices":[0,256,1,512,2,768,3,1024],"raw_object_indices":[[240,0],[241,1,0],[240,1],[241,2,0],[240,2],[241,3,0],[240,3],[241,4,0]],"data_blocks":["","b","c","d","e","f","g","h"],"source_offsets":[114,116,119,121,124,126,129,131]}"#;
    let parsed: super::FeatureDatumCsysConstruction = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&parsed).unwrap(), wire);
    for slot in 0..8 {
        let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
        invalid["source_offsets"][slot] = serde_json::json!(1000);
        let error =
            serde_json::from_value::<super::FeatureDatumCsysConstruction>(invalid).unwrap_err();
        assert!(error.to_string().contains("source_offsets"));
    }
    for origin in [0, u64::MAX - 20] {
        let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
        invalid["source_offsets"][0] = serde_json::json!(origin);
        assert!(serde_json::from_value::<super::FeatureDatumCsysConstruction>(invalid).is_err());
    }
    let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
    invalid["raw_object_indices"][0] = serde_json::json!([0]);
    let error = serde_json::from_value::<super::FeatureDatumCsysConstruction>(invalid).unwrap_err();
    assert!(error.to_string().contains("raw_object_indices[0]"));
}

#[test]
fn surface_reference_requires_the_payload_token_grammar() {
    let wire = r#"{"id":"r","operation_label":"o","ordinal":0,"object_index":0,"raw_object_index":[240,0],"data_block":"","source_offset":10}"#;
    let reference: super::FeatureSurfaceConstructionReference = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&reference).unwrap(), wire);
    let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
    invalid["raw_object_index"] = serde_json::json!([0]);
    assert!(serde_json::from_value::<super::FeatureSurfaceConstructionReference>(invalid).is_err());
}

#[test]
fn draft_reference_requires_the_payload_token_grammar() {
    use crate::native::features::draft::FeatureDraftConstructionReference;
    let wire = r#"{"id":"r","operation_label":"o","ordinal":0,"object_index":0,"raw_object_index":[240,0],"data_block":"","source_offset":10}"#;
    let reference: FeatureDraftConstructionReference = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&reference).unwrap(), wire);
    let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
    invalid["raw_object_index"] = serde_json::json!([0]);
    assert!(serde_json::from_value::<FeatureDraftConstructionReference>(invalid).is_err());
}

#[test]
fn point_header_mode_admits_only_two_wire_bytes() {
    for mode in 0..=u8::MAX {
        let wire = serde_json::json!({
            "id": "header", "operation_label": "operation", "object_index": 1,
            "raw_object_index": [240, 1], "mode": mode, "source_offset": 10
        });
        let header =
            serde_json::from_value::<super::super::FeaturePointConstructionHeader>(wire.clone());
        if matches!(mode, 2 | 3) {
            assert_eq!(serde_json::to_value(header.unwrap()).unwrap(), wire);
        } else {
            assert!(header.unwrap_err().to_string().contains("mode"));
        }
    }
}
