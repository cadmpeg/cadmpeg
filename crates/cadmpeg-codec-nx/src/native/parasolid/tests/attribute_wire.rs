// SPDX-License-Identifier: Apache-2.0

#[test]
fn value_relation_positions_preserve_wire_and_reject_leading_slots() {
    use crate::native::parasolid::{
        ParasolidEntity51NumericUse, ParasolidEntity51StringUse, ParasolidEntity51StructuredUse,
    };

    fn check<T: serde::Serialize + serde::de::DeserializeOwned>(wire: &str) {
        let relation: T = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&relation).unwrap(), wire);
        let invalid = wire.replace("\"reference_ordinal\":5", "\"reference_ordinal\":4");
        assert!(serde_json::from_str::<T>(&invalid).is_err());
        for target in [0, 1] {
            let invalid = wire.replace(
                "\"referenced_xmt\":10",
                &format!("\"referenced_xmt\":{target}"),
            );
            assert!(serde_json::from_str::<T>(&invalid).is_err());
        }
    }

    let numeric = r#"{"id":"use","stream_ordinal":0,"entity_51_record":"entity","reference_ordinal":5,"referenced_xmt":10,"kind":"doubles","value_record":"value","inflated_offset":8}"#;
    check::<ParasolidEntity51NumericUse>(numeric);
    check::<ParasolidEntity51StructuredUse>(&numeric.replace("doubles", "points"));
    let string = numeric
        .replace("\"kind\":\"doubles\",", "")
        .replace("value_record", "string_record");
    check::<ParasolidEntity51StringUse>(&string);
}

#[test]
fn attribute_definition_wire_preserves_codes_and_rejects_invalid_domains() {
    use crate::native::parasolid::{
        ParasolidAttributeDefinition, ParasolidAttributeDefinitionWire,
    };

    let wire = r#"{"id":"definition","stream_ordinal":0,"xmt":2,"next_definition_xmt":1,"identifier_xmt":3,"identifier_inflated_offset":4,"name":"CLASS","type_id":8000,"action_codes":[0,1,2,3,4,5,6,0],"field_names_xmt":1,"legal_owner_flags":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"field_count":2,"field_codes":[0,10],"inflated_offset":8}"#;
    let definition: ParasolidAttributeDefinition = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::to_vec(&definition).unwrap(),
        serde_json::to_vec(&ParasolidAttributeDefinitionWire::from(definition.clone())).unwrap()
    );
    for target in [0, 1, 2, u32::MAX] {
        let wire = wire
            .replace(
                "\"next_definition_xmt\":1",
                &format!("\"next_definition_xmt\":{target}"),
            )
            .replace(
                "\"field_names_xmt\":1",
                &format!("\"field_names_xmt\":{target}"),
            );
        let definition: ParasolidAttributeDefinition = serde_json::from_str(&wire).unwrap();
        assert_eq!(definition.next_definition_xmt.is_none(), target == 1);
        assert_eq!(definition.field_names_xmt.is_none(), target == 1);
        assert_eq!(serde_json::to_string(&definition).unwrap(), wire);
    }
    for invalid in [
        wire.replace("\"xmt\":2", "\"xmt\":1"),
        wire.replace("\"identifier_xmt\":3", "\"identifier_xmt\":0"),
        wire.replace("\"name\":\"CLASS\"", "\"name\":\"\""),
        wire.replace("\"type_id\":8000", "\"type_id\":0"),
        wire.replace("[0,1,2,3,4,5,6,0]", "[0,1,2,3,4,5,7,0]"),
        wire.replace("\"legal_owner_flags\":[0,", "\"legal_owner_flags\":[2,"),
        wire.replace("\"field_codes\":[0,10]", "\"field_codes\":[0,11]"),
    ] {
        assert!(serde_json::from_str::<ParasolidAttributeDefinition>(&invalid).is_err());
    }
}

#[test]
fn attribute_definition_native_limit_refuses_before_field_code_copy() {
    use crate::native::parasolid::ParasolidAttributeDefinition;
    let wire = r#"{"id":"nx:parasolid:attribute-definition#0","stream_ordinal":0,"xmt":2,"next_definition_xmt":1,"identifier_xmt":3,"identifier_inflated_offset":4,"name":"CLASS","type_id":8000,"action_codes":[0,1,2,3,4,5,6,0],"field_names_xmt":1,"legal_owner_flags":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"field_count":2,"field_codes":[0,10],"inflated_offset":8}"#;
    let definition: ParasolidAttributeDefinition = serde_json::from_str(wire).unwrap();
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &definition,
        serde_json::from_str::<serde_json::Value>(wire).unwrap(),
    );
}

#[test]
fn resolved_class_relations_preserve_non_null_definition_wire() {
    use crate::native::parasolid::{
        ParasolidAttributeClassUseWire, ParasolidTopologyAttributeClassUseWire,
    };
    fn check<T: serde::Serialize + serde::de::DeserializeOwned>(wire: &str) {
        let relation: T = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&relation).unwrap(), wire);
        for target in [0, 1] {
            let invalid = wire.replace(
                "\"definition_xmt\":2",
                &format!("\"definition_xmt\":{target}"),
            );
            assert!(serde_json::from_str::<T>(&invalid).is_err());
        }
    }
    check::<ParasolidAttributeClassUseWire>(
        r#"{"id":"class","stream_ordinal":0,"entity_51_record":"entity","definition_xmt":2,"attribute_definition":"definition"}"#,
    );
    check::<ParasolidTopologyAttributeClassUseWire>(
        r#"{"id":"class","topology_attribute_reference":"topology","entity_51_record":"entity","attribute_class_use":"use","definition_xmt":2,"attribute_definition":"definition"}"#,
    );
}

#[test]
fn attribute_class_use_borrowed_wire_matches_owned_bytes() {
    use crate::native::parasolid::{ParasolidAttributeClassUse, ParasolidAttributeClassUseWire};

    let record = ParasolidAttributeClassUse {
        id: "nx:s3:attribute-class-use#2-8".into(),
        stream_ordinal: 3,
        entity_51_record: "entity".into(),
        definition_xmt: 2_u32.try_into().unwrap(),
        attribute_definition: "definition".into(),
        inflated_offset: 8,
    };
    let old = serde_json::to_vec(&ParasolidAttributeClassUseWire::from(record.clone())).unwrap();
    assert_eq!(serde_json::to_vec(&record).unwrap(), old);
}

#[test]
fn attribute_class_use_retained_limit_refuses_borrowed_serialization() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::NativeNamespace;

    use crate::native::parasolid::{ParasolidAttributeClassUse, ATTRIBUTE_CLASS_USE_CLONE_COUNT};

    let record = ParasolidAttributeClassUse {
        id: "nx:s3:attribute-class-use#2-8".into(),
        stream_ordinal: 3,
        entity_51_record: "entity".into(),
        definition_xmt: 2_u32.try_into().unwrap(),
        attribute_definition: "definition".into(),
        inflated_offset: 8,
    };
    let json = serde_json::to_vec(&record).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(json.len()).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    ATTRIBUTE_CLASS_USE_CLONE_COUNT.with(|count| count.set(0));
    let error = NativeNamespace::default()
        .set_arena(&limited, "a", std::slice::from_ref(&record))
        .unwrap_err();
    ATTRIBUTE_CLASS_USE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    NativeNamespace::default()
        .set_arena(&service, "a", &[record])
        .unwrap();
    ATTRIBUTE_CLASS_USE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn topology_attribute_class_use_borrowed_wire_matches_owned_bytes() {
    use crate::native::parasolid::{
        ParasolidTopologyAttributeClassUse, ParasolidTopologyAttributeClassUseWire,
    };

    let record = ParasolidTopologyAttributeClassUse {
        id: "nx:s3:topology-attribute-class-use#2-8".into(),
        topology_attribute_reference: "topology".into(),
        entity_51_record: "entity".into(),
        attribute_class_use: "class".into(),
        definition_xmt: 2_u32.try_into().unwrap(),
        attribute_definition: "definition".into(),
        stream_ordinal: 3,
        inflated_offset: 8,
    };
    let old = serde_json::to_vec(&ParasolidTopologyAttributeClassUseWire::from(
        record.clone(),
    ))
    .unwrap();
    assert_eq!(serde_json::to_vec(&record).unwrap(), old);
}

#[test]
fn topology_attribute_class_use_retained_limit_refuses_borrowed_serialization() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::NativeNamespace;

    use crate::native::parasolid::{
        ParasolidTopologyAttributeClassUse, TOPOLOGY_ATTRIBUTE_CLASS_USE_CLONE_COUNT,
    };

    let record = ParasolidTopologyAttributeClassUse {
        id: "nx:s3:topology-attribute-class-use#2-8".into(),
        topology_attribute_reference: "topology".into(),
        entity_51_record: "entity".into(),
        attribute_class_use: "class".into(),
        definition_xmt: 2_u32.try_into().unwrap(),
        attribute_definition: "definition".into(),
        stream_ordinal: 3,
        inflated_offset: 8,
    };
    let json = serde_json::to_vec(&record).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(json.len()).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    TOPOLOGY_ATTRIBUTE_CLASS_USE_CLONE_COUNT.with(|count| count.set(0));
    let error = NativeNamespace::default()
        .set_arena(&limited, "a", std::slice::from_ref(&record))
        .unwrap_err();
    TOPOLOGY_ATTRIBUTE_CLASS_USE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    NativeNamespace::default()
        .set_arena(&service, "a", &[record])
        .unwrap();
    TOPOLOGY_ATTRIBUTE_CLASS_USE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
}
