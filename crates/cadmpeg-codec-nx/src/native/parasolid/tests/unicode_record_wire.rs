// SPDX-License-Identifier: Apache-2.0
#[test]
fn unicode_record_wire_derives_exact_utf16_and_rejects_disagreement() {
    let wire = r#"{"id":"unicode","stream_ordinal":0,"xmt":2,"code_units":[78,88,55357,56960],"value":"NX🚀","byte_len":8,"inflated_offset":0}"#;
    let record: crate::native::parasolid::ParasolidEntity62UnicodeRecord =
        serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&record).unwrap(), wire);
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(
            &crate::native::parasolid::ParasolidEntity62UnicodeRecordWire::from(record.clone())
        )
        .unwrap()
    );
    let inconsistent = wire.replace("[78,88,55357,56960]", "[78,88,55357]");
    assert!(
        serde_json::from_str::<crate::native::parasolid::ParasolidEntity62UnicodeRecord>(
            &inconsistent
        )
        .is_err()
    );
}

#[test]
fn unicode_record_native_limit_refuses_before_code_unit_copy() {
    let wire = r#"{"id":"nx:parasolid:unicode-record#0","stream_ordinal":0,"xmt":2,"code_units":[78,88,55357,56960],"value":"NX🚀","byte_len":8,"inflated_offset":0}"#;
    let record: crate::native::parasolid::ParasolidEntity62UnicodeRecord =
        serde_json::from_str(wire).unwrap();
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::from_str::<serde_json::Value>(wire).unwrap(),
    );
}
