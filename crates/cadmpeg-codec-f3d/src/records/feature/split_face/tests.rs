use super::{DesignSplitFaceDirectionReference, DesignSplitFaceMethod, DesignSplitFaceOperation};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload};
use crate::records::identity::{Located, ReferenceRun};

fn operation() -> DesignSplitFaceOperation {
    cadmpeg_core::serde_erase::json::from_slice(br#"{"method_offset":32,"control":{"family":0,"projection":1,"extension":0},"direction_reference":{"kind":"local","target":{"value":55,"offset":22},"inline_type_guid":null},"carrier_frame":{"primary_offset":120,"secondary_offset":228,"end_offset":239,"version":1},"direction":{"value":[2.4,0.0,0.0],"offset":169},"input_count":{"value":1,"offset":213},"inputs":[{"value":{"kind":"local","target":{"value":88,"offset":218},"inline_type_guid":null},"offset":217}],"inputs_complete":true}"#).unwrap()
}

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::SplitFace,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 0;
            draft.frame_length = 408;
            draft.reference_count_offset = 120;
            draft.reference_members = ReferenceRun::located(vec![Located {
                value: 9,
                offset: 125,
            }]);
            draft.kind_offset = 143;
            draft.feature_ordinal_offset = 330;
            draft.paired_byte_offset = 408;
            let DesignScopePayload::SplitFace(value) = &mut draft.payload else {
                panic!("SplitFace payload")
            };
            *value = Some(operation());
        })
        .unwrap();
    scope
}

#[test]
fn split_face_method_is_derived_from_controls_and_reference() {
    let mut operation = operation();
    assert_eq!(operation.method(), DesignSplitFaceMethod::AlongVector);
    operation.direction_reference = DesignSplitFaceDirectionReference::Unavailable;
    assert_eq!(operation.method(), DesignSplitFaceMethod::Unresolved);
    operation.direction_reference = DesignSplitFaceDirectionReference::Null;
    assert_eq!(operation.method(), DesignSplitFaceMethod::Unresolved);
    operation.control.as_mut().unwrap().family = 1;
    assert_eq!(operation.method(), DesignSplitFaceMethod::ClosestPoint);
    operation.control.as_mut().unwrap().projection = 0;
    operation.control.as_mut().unwrap().extension = 1;
    assert_eq!(
        operation.method(),
        DesignSplitFaceMethod::SurfaceIntersection { extend: true }
    );
}

#[test]
fn split_face_wire_rejects_a_separately_supplied_method() {
    let mut wire = cadmpeg_core::serde_erase::json::to_value(operation()).unwrap();
    wire["method"] = serde_json::json!({"kind":"closest_point"});
    assert!(cadmpeg_core::serde_erase::json::from_value::<DesignSplitFaceOperation>(wire).is_err());
}

#[test]
fn split_face_reference_forms_validate_their_qualifiers_and_exact_extents() {
    let mut value = operation();
    let DesignSplitFaceDirectionReference::Local {
        inline_type_guid, ..
    } = &mut value.direction_reference
    else {
        panic!("local reference")
    };
    *inline_type_guid = Some("00000000-0000-0000-0000-000000000099".into());
    assert!(!value.scope_offsets_valid(0, 120));
    value.method_offset = 72;
    assert!(value.scope_offsets_valid(0, 120));
    let DesignSplitFaceDirectionReference::Local {
        inline_type_guid, ..
    } = &mut value.direction_reference
    else {
        panic!("local reference")
    };
    *inline_type_guid = Some("x".into());
    assert!(!value.scope_offsets_valid(0, 120));
    let mut value = operation();
    value.carrier_frame.as_mut().unwrap().secondary_offset = 226;
    assert!(!value.scope_offsets_valid(0, 120));
    value.carrier_frame.as_mut().unwrap().secondary_offset = 239;
    assert!(!value.scope_offsets_valid(0, 120));
    value.carrier_frame.as_mut().unwrap().secondary_offset = 228;
    value.inputs[0].offset = 218;
    assert!(!value.scope_offsets_valid(0, 120));
}

#[test]
fn split_face_scope_wire_validates_its_owned_locations() {
    let wire = cadmpeg_core::serde_erase::json::to_value(scope()).unwrap();
    assert!(
        cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(wire.clone()).is_ok()
    );
    for at in [18, 20, 115, 120, u64::MAX] {
        let mut invalid = wire.clone();
        invalid["split_face_operation"]["method_offset"] = serde_json::json!(at);
        assert!(
            cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(invalid).is_err(),
            "accepted control location {at}"
        );
    }
    for at in [0, 19, 23, 120, u64::MAX] {
        let mut invalid = wire.clone();
        invalid["split_face_operation"]["direction_reference"]["target"]["offset"] =
            serde_json::json!(at);
        assert!(
            cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(invalid).is_err(),
            "accepted carrier reference location {at}"
        );
    }
    assert!(!operation().scope_offsets_valid(u64::MAX, u64::MAX));
}

#[test]
fn split_face_carrier_requires_a_consistent_bounded_snapshot() {
    let wire = cadmpeg_core::serde_erase::json::to_value(scope()).unwrap();
    let mutations = [
        ("carrier_frame", serde_json::json!(null)),
        (
            "carrier_frame",
            serde_json::json!({"primary_offset":169,"secondary_offset":228,"end_offset":239,"version":1}),
        ),
        (
            "carrier_frame",
            serde_json::json!({"primary_offset":120,"secondary_offset":192,"end_offset":239,"version":1}),
        ),
        (
            "direction",
            serde_json::json!({"value":[2.4,0.0,0.0],"offset":u64::MAX}),
        ),
        ("input_count", serde_json::json!({"value":0,"offset":213})),
        ("input_count", serde_json::json!({"value":2,"offset":213})),
        ("input_count", serde_json::json!({"value":1,"offset":212})),
        ("input_count", serde_json::json!({"value":1,"offset":228})),
        ("input_count", serde_json::json!(null)),
        (
            "direction_reference",
            serde_json::json!({"kind":"cross_segment","target":{"value":55,"offset":22},"inline_type_guid":null,"segment":7}),
        ),
    ];
    for (field, value) in mutations {
        let mut invalid = wire.clone();
        invalid["split_face_operation"][field] = value;
        assert!(
            cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(invalid).is_err(),
            "accepted inconsistent {field}"
        );
    }
    for at in [0, 213, 218, 228, u64::MAX] {
        let mut invalid = wire.clone();
        invalid["split_face_operation"]["inputs"][0]["offset"] = serde_json::json!(at);
        assert!(
            cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(invalid).is_err(),
            "accepted input marker {at}"
        );
    }
}

#[test]
fn split_face_partial_inputs_and_missing_sources_remain_admitted() {
    let mut operation = operation();
    operation.inputs_complete = false;
    operation.input_count.as_mut().unwrap().value = 2;
    assert!(operation.scope_offsets_valid(0, 120));
    operation.inputs.clear();
    operation.input_count = None;
    assert!(operation.scope_offsets_valid(0, 120));
    operation.direction = None;
    operation.carrier_frame = None;
    assert!(operation.scope_offsets_valid(0, 120));
    operation.direction_reference = DesignSplitFaceDirectionReference::Unavailable;
    assert!(!operation.scope_offsets_valid(0, 120));
    operation.control = None;
    assert!(operation.scope_offsets_valid(0, 120));
}

#[test]
fn local_reference_envelopes_and_vector_minimum_are_checked() {
    let wire = cadmpeg_core::serde_erase::json::to_value(scope()).unwrap();
    for (field, value) in [
        (
            "direction",
            serde_json::json!({"value":[2.4,0.0,0.0],"offset":121}),
        ),
        (
            "carrier_frame",
            serde_json::json!({"primary_offset":120,"secondary_offset":226,"end_offset":239,"version":1}),
        ),
        (
            "carrier_frame",
            serde_json::json!({"primary_offset":120,"secondary_offset":239,"end_offset":239,"version":1}),
        ),
    ] {
        let mut invalid = wire.clone();
        invalid["split_face_operation"][field] = value;
        assert!(
            cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(invalid).is_err()
        );
    }
    for guid in ["x", "00000000-0000-0000-0000-000000000099"] {
        let mut invalid = wire.clone();
        invalid["split_face_operation"]["direction_reference"]["inline_type_guid"] =
            serde_json::json!(guid);
        assert!(
            cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(invalid).is_err()
        );
    }
}

#[test]
fn split_face_optional_wire_fields_use_omission_and_nullable_references() {
    let wire = serde_json::json!({"method_offset": 19, "direction_reference": {"kind": "null"}});
    let value: DesignSplitFaceOperation =
        cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(value).unwrap(),
        serde_json::json!({
            "method_offset": 19, "direction_reference": {"kind": "null"}, "inputs_complete": false
        })
    );
    for field in ["control", "carrier_frame", "direction", "input_count"] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::Value::Null;
        assert!(
            cadmpeg_core::serde_erase::json::from_value::<DesignSplitFaceOperation>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field)
        );
    }
    let reference = serde_json::json!({"kind":"local","target":{"value":55,"offset":22},"inline_type_guid":null});
    assert!(
        cadmpeg_core::serde_erase::json::from_value::<DesignSplitFaceDirectionReference>(
            reference.clone()
        )
        .is_ok()
    );
    let mut missing = reference;
    missing.as_object_mut().unwrap().remove("inline_type_guid");
    assert!(
        cadmpeg_core::serde_erase::json::from_value::<DesignSplitFaceDirectionReference>(missing)
            .is_err()
    );
}

#[test]
fn absent_external_type_cannot_claim_a_different_effective_guid() {
    let mut wire = cadmpeg_core::serde_erase::json::to_value(scope()).unwrap();
    let input = serde_json::json!({"kind":"cross_document","target":{"value":88,"offset":218},"inline_type_guid":"00000000-0000-0000-0000-000000000099","segment":7,"asset_guid":"asset","target_type_guid":"00000000-0000-0000-0000-000000000099","external_type_guid_present":false,"link_name":"link","version":null});
    wire["split_face_operation"]["inputs"][0]["value"] = input;
    wire["split_face_operation"]["carrier_frame"]["secondary_offset"] = serde_json::json!(299);
    wire["split_face_operation"]["carrier_frame"]["end_offset"] = serde_json::json!(310);
    assert!(
        cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(wire.clone()).is_ok()
    );
    wire["split_face_operation"]["inputs"][0]["value"]["target_type_guid"] =
        serde_json::json!("00000000-0000-0000-0000-000000000098");
    assert!(
        cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(wire.clone()).is_err()
    );
    wire["split_face_operation"]["inputs"][0]["value"]["external_type_guid_present"] =
        serde_json::json!(true);
    wire["split_face_operation"]["carrier_frame"]["secondary_offset"] = serde_json::json!(339);
    wire["split_face_operation"]["carrier_frame"]["end_offset"] = serde_json::json!(350);
    assert!(cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(wire).is_ok());
}
