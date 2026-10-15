// SPDX-License-Identifier: Apache-2.0
use super::ListFields;

#[test]
fn complete_list_contract_and_boundary_controls() {
    let json = r#"{"node_id":0,"list_type":4,"notransmit":true,"owner":3,"next":1,"previous":1,"list_length":65538,"block_length":20,"finger_index":21,"finger_block":53,"list_block":52}"#;
    let fields: ListFields = serde_json::from_str(json).unwrap();
    assert_eq!(fields.references(), [3, 1, 1, 53, 52]);
    assert_eq!(serde_json::to_string(&fields).unwrap(), json);
    let mut empty: serde_json::Value = serde_json::from_str(json).unwrap();
    empty["list_length"] = 0.into();
    empty["finger_index"] = 1.into();
    assert!(serde_json::from_value::<ListFields>(empty).is_ok());
    for (field, value) in [
        ("node_id", 1),
        ("list_type", 3),
        ("block_length", 19),
        ("finger_index", 0),
        ("finger_index", 65539),
        ("owner", 1),
        ("next", 0),
        ("previous", 0),
        ("finger_block", 1),
        ("list_block", 1),
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = value.into();
        assert!(
            serde_json::from_value::<ListFields>(wire).is_err(),
            "{field}: {value}"
        );
    }
}

#[test]
fn list_storage_end_and_every_truncation() {
    // Modern primary order: identity, node, byte, logical, three pointers,
    // three complete u32 fields, then distinct finger and head pointers.
    let bytes = [
        0, 7, 0, 0, 0, 0, 4, 0, 0, 3, 1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 2, 0, 0, 0, 20, 0, 0, 0, 21, 0,
        53, 0, 0, 52, 0,
    ];
    let (xmt, fields, end) = crate::deltas::list_body(&bytes, 0).unwrap();
    assert_eq!(xmt, 7);
    assert_eq!(end, bytes.len());
    let wire = serde_json::to_value(fields).unwrap();
    assert_eq!(wire["list_length"], 65538);
    assert_eq!(wire["notransmit"], false);
    assert_eq!(wire["finger_index"], 21);
    assert_eq!(wire["finger_block"], 53);
    assert_eq!(wire["list_block"], 52);
    for end in 0..bytes.len() {
        assert!(crate::deltas::list_body(&bytes[..end], 0).is_none());
    }
    for at in [10, 13, 16, 31, 34] {
        let mut invalid = bytes;
        invalid[at] = 2;
        assert!(crate::deltas::list_body(&invalid, 0).is_none());
    }
}
