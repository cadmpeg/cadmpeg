// SPDX-License-Identifier: Apache-2.0
use super::PointerListBlockFields;

#[test]
fn block_wire_preserves_chain_and_enforces_storage_and_population() {
    let json = r#"{"entry_array_length":3,"n_entries":2,"index_map_offset":0,"next_block":21,"entries":[30,31,1]}"#;
    let fields: PointerListBlockFields = serde_json::from_str(json).unwrap();
    assert_eq!(fields.references().collect::<Vec<_>>(), [21, 30, 31, 1]);
    assert_eq!(serde_json::to_string(&fields).unwrap(), json);
    for (field, value) in [
        ("entry_array_length", 2),
        ("n_entries", 4),
        ("index_map_offset", 1),
        ("next_block", 0),
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = value.into();
        assert!(serde_json::from_value::<PointerListBlockFields>(wire).is_err());
    }
    for entries in [[30, 1, 1], [30, 31, 32], [0, 31, 1]] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["entries"] = serde_json::json!(entries);
        assert!(serde_json::from_value::<PointerListBlockFields>(wire).is_err());
    }
    let empty: PointerListBlockFields = serde_json::from_str(r#"{"entry_array_length":1,"n_entries":0,"index_map_offset":0,"next_block":1,"entries":[1]}"#).unwrap();
    assert_eq!(empty.references().collect::<Vec<_>>(), [1, 1]);
}

#[test]
fn nonnull_block_chain_and_incomplete_storage() {
    let block = |xmt: u16, next: u16, entry: u16| {
        let mut bytes = vec![0, 74];
        bytes.extend(1u32.to_be_bytes());
        bytes.extend(xmt.to_be_bytes());
        bytes.extend(1u32.to_be_bytes());
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(next.to_be_bytes());
        bytes.push(1);
        bytes.extend(entry.to_be_bytes());
        bytes.push(1);
        bytes
    };
    let first = block(20, 21, 30);
    let second = block(21, 1, 31);
    let mut bytes = first.clone();
    bytes.extend(second.clone());
    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &bytes))
            .unwrap();
    assert_eq!(census.records.len(), 2);
    assert_eq!(census.records[0].family.references(), [21, 30]);
    assert_eq!(census.records[1].family.references(), [1, 31]);
    for end in 0..first.len() {
        assert!(
            crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(
                ctx,
                &first[..end]
            ))
            .unwrap()
            .records
            .is_empty()
        );
    }
}

#[test]
fn block_identity_must_be_nonnull_before_materialization() {
    for xmt in [0u16, 1] {
        let mut bytes = vec![0, 74];
        bytes.extend(1u32.to_be_bytes());
        bytes.extend(xmt.to_be_bytes());
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(0u32.to_be_bytes());
        bytes.extend([0, 1, 1, 0, 1, 1]);
        assert!(
            crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(
                ctx, &bytes
            ))
            .unwrap()
            .records
            .is_empty()
        );
    }
}
