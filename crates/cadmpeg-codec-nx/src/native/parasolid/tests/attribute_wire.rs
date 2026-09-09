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
    use crate::native::parasolid::ParasolidAttributeDefinition;

    let wire = r#"{"id":"definition","stream_ordinal":0,"xmt":2,"next_definition_xmt":1,"identifier_xmt":3,"identifier_inflated_offset":4,"name":"CLASS","type_id":8000,"action_codes":[0,1,2,3,4,5,6,0],"field_names_xmt":1,"legal_owner_flags":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"field_count":2,"field_codes":[0,10],"inflated_offset":8}"#;
    let definition: ParasolidAttributeDefinition = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&definition).unwrap(), wire);
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
