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
    }

    let numeric = r#"{"id":"use","stream_ordinal":0,"entity_51_record":"entity","reference_ordinal":5,"referenced_xmt":10,"kind":"doubles","value_record":"value","inflated_offset":8}"#;
    check::<ParasolidEntity51NumericUse>(numeric);
    check::<ParasolidEntity51StructuredUse>(&numeric.replace("doubles", "points"));
    let string = numeric.replace("\"kind\":\"doubles\",", "").replace("value_record", "string_record");
    check::<ParasolidEntity51StringUse>(&string);
}
