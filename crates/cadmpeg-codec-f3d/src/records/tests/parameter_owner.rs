use crate::records::{
    parameters::{DesignParameterOwner, DesignParameterOwnerWire},
    references::DesignClassTag,
};

fn wire() -> DesignParameterOwnerWire {
    DesignParameterOwnerWire {
        id: "f3d:Design/BulkStream.dat:design-parameter-owner#100".into(),
        byte_offset: 100,
        frame_length: 104,
        class_tag: DesignClassTag::try_from("292".to_owned()).unwrap(),
        record_index: 10,
        scope_record_index: 3,
        local_ordinal: 0,
        evaluated_value: 2.0,
        evaluated_value_offset: 140,
        parameter_record_index: 11,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 12,
    }
}

#[test]
fn parameter_owner_orders_preserve_wire_fields() {
    for (owner, parameter, companion) in [(10, 11, 12), (11, 10, 12), (10, 12, 11)] {
        let mut input = wire();
        input.record_index = owner;
        input.parameter_record_index = parameter;
        input.companion_record_index = companion;
        let value = DesignParameterOwner::try_from(input).unwrap();
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(value.record_index(), owner);
        assert_eq!(value.parameter_record_index(), parameter);
        assert_eq!(value.companion_record_index(), companion);
        assert_eq!(serde_json::to_value(&value).unwrap(), json);
        let restored: DesignParameterOwner = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), json);
    }
}

#[test]
fn parameter_owner_frame_forms_are_closed() {
    for (length, delta, variant) in [
        (99, 40, None),
        (103, 40, None),
        (100, 41, None),
        (107, 44, None),
        (100, 41, Some(0)),
        (101, 41, Some(0)),
        (101, 41, Some(1)),
        (103, 40, Some(1)),
        (104, 40, Some(0)),
        (104, 40, Some(1)),
        (107, 44, Some(0)),
        (108, 44, Some(0)),
        (108, 44, Some(1)),
    ] {
        let mut input = wire();
        input.frame_length = length;
        input.evaluated_value_offset = input.byte_offset + delta;
        input.variant = variant;
        let source = DesignParameterOwner::try_from(input).unwrap();
        let semantic = serde_json::to_value(source).unwrap();
        assert!(serde_json::from_value::<DesignParameterOwner>(semantic).is_ok());
    }
    for (field, value) in [
        ("frame_length", serde_json::json!(0)),
        ("frame_length", serde_json::json!(99)),
        ("evaluated_value_offset", serde_json::json!(139)),
        ("variant", serde_json::json!(2)),
        ("variant", serde_json::Value::Null),
        ("record_index", serde_json::json!(9)),
        ("parameter_record_index", serde_json::json!(12)),
        ("companion_record_index", serde_json::json!(11)),
    ] {
        let mut json = serde_json::to_value(wire()).unwrap();
        json[field] = value;
        assert!(
            serde_json::from_value::<DesignParameterOwnerWire>(json)
                .map_err(|error| error.to_string())
                .and_then(DesignParameterOwner::try_from)
                .is_err(),
            "{field}"
        );
    }
}

#[test]
fn parameter_owner_rejects_nonfinite_values_and_index_overflow() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut input = wire();
        input.evaluated_value = value;
        assert!(DesignParameterOwner::try_from(input)
            .unwrap_err()
            .contains("evaluated_value"));
    }
    let mut input = wire();
    input.record_index = u32::MAX;
    input.parameter_record_index = u32::MAX;
    input.companion_record_index = u32::MAX;
    let json = serde_json::to_value(&input).unwrap();
    assert!(DesignParameterOwner::try_from(input).is_err());
    assert!(serde_json::from_value::<DesignParameterOwner>(json).is_err());
}

#[test]
fn parameter_owner_legacy_forms_keep_external_scalar_offsets() {
    for (length, class_tag, scope) in [(68, "268", 0), (88, "284", 3)] {
        let mut input = wire();
        input.frame_length = length;
        input.class_tag = DesignClassTag::try_from(class_tag.to_owned()).unwrap();
        input.scope_record_index = scope;
        input.evaluated_value_offset = 1000;
        input.variant = None;
        let owner = DesignParameterOwner::try_from(input).unwrap();
        assert_eq!(owner.evaluated_value_offset(), Some(1000));
        assert_eq!(owner.scope_record_index(), (length == 88).then_some(scope));
        assert_eq!(owner.local_ordinal(), None);
        let semantic = serde_json::to_value(owner).unwrap();
        let restored: DesignParameterOwner = serde_json::from_value(semantic.clone()).unwrap();
        assert_eq!(restored.evaluated_value_offset(), None);
        assert_eq!(restored.evaluated_value().get(), 2.0);
        assert_eq!(serde_json::to_value(restored).unwrap(), semantic);
    }
}
