// SPDX-License-Identifier: Apache-2.0
use super::FeatureExtractFaceConstructionEnvelope;
use cadmpeg_core::serde_erase::json;

fn wire() -> serde_json::Value {
    cadmpeg_core::json!({
        "id":"nx:test:extract#0", "operation_label":"nx:test:operation#0", "parameter_class_index":3,
        "control_words":[0,0,0,0], "lanes":[[],[]],
        "terminal":{"object_index":0,"raw_object_index":"8000"}, "navigator_text":"é",
        "byte_len":48,"source_offset":100
    })
}

#[test]
fn extract_face_wire_requires_complete_bounded_lanes_tokens_and_utf8_span() {
    let wire = wire();
    let envelope: FeatureExtractFaceConstructionEnvelope = json::from_value(wire.clone()).unwrap();
    assert_eq!(json::to_value(&envelope).unwrap(), wire);
    cadmpeg_test_support::native_serialization::assert_native_limit(&envelope, wire.clone());
    for (field, value) in [
        ("parameter_class_index", cadmpeg_core::json!(0)),
        ("lanes", cadmpeg_core::json!([[]])),
        ("byte_len", cadmpeg_core::json!(47)),
        ("source_offset", cadmpeg_core::json!(u64::MAX)),
        ("navigator_text", cadmpeg_core::json!("é".repeat(127))),
        (
            "terminal",
            cadmpeg_core::json!({"object_index":0,"raw_object_index":"f000"}),
        ),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(json::from_value::<FeatureExtractFaceConstructionEnvelope>(invalid).is_err());
    }
    let member = cadmpeg_core::json!({"key":0,"left":{"object_index":0,"raw_object_index":"00"},"right":{"object_index":0,"raw_object_index":"00"}});
    let mut largest = wire.clone();
    largest["lanes"][0] = serde_json::Value::Array(vec![member.clone(); 254]);
    largest["byte_len"] = cadmpeg_core::json!(48 + 254 * 3);
    assert!(json::from_value::<FeatureExtractFaceConstructionEnvelope>(largest.clone()).is_ok());
    largest["lanes"][0].as_array_mut().unwrap().push(member);
    largest["byte_len"] = cadmpeg_core::json!(48 + 255 * 3);
    assert!(json::from_value::<FeatureExtractFaceConstructionEnvelope>(largest).is_err());
}
