// SPDX-License-Identifier: Apache-2.0
//! Typed-reference parameter scope tails and reference tables.

use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::records::decal::DesignRecordHeader;
use crate::test_support::lp_utf16;

const TYPE: &str = "11111111-2222-3333-4444-555555555555";

fn typed_reference(out: &mut Vec<u8>, target: u32) {
    out.push(1);
    out.extend_from_slice(&u64::from(target).to_le_bytes());
    out.extend_from_slice(&36u32.to_le_bytes());
    out.extend_from_slice(TYPE.as_bytes());
    out.extend_from_slice(&[0, 0]);
}

/// A Sketch scope frame with a two-entry typed table and the 196-byte tail.
fn typed_scope_frame(previous_history: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"336");
    bytes.extend_from_slice(&226u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    typed_reference(&mut bytes, 222);
    typed_reference(&mut bytes, 234);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Sketch");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 15]);
    typed_reference(&mut bytes, 229);
    bytes.extend_from_slice(&previous_history.to_le_bytes());
    bytes.extend_from_slice(&64u32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&64u32.to_le_bytes());
    typed_reference(&mut bytes, 228);
    bytes.extend_from_slice(&[0; 2]);
    typed_reference(&mut bytes, 227);
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"262");
    bytes.extend_from_slice(&226u32.to_le_bytes());
    bytes
}

fn parse(bytes: &[u8]) -> Option<crate::records::feature::scope::DesignParameterScope> {
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 226,
        class_tag: crate::records::references::DesignClassTag::try_from("336".to_owned()).unwrap(),
        byte_offset: 0,
    };
    parse_parameter_scope(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
}

#[test]
fn typed_scope_reads_the_wide_reference_table_and_196_byte_tail() {
    let scope = parse(&typed_scope_frame(5)).expect("typed scope");
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::Sketch
    );
    assert_eq!(
        scope.reference_members().values().collect::<Vec<_>>(),
        [&222, &234]
    );
    assert_eq!(scope.history_state_id(), Some(7));
    assert_eq!(scope.previous_history_state_id(), Some(5));
}

#[test]
fn typed_scope_tail_requires_its_typed_references() {
    let mut bytes = typed_scope_frame(5);
    let tail_reference = bytes.len() - 11 - 2 - 51;
    bytes[tail_reference + 9] = 35;
    assert!(parse(&bytes).is_none());
}
