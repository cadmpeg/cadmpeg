// SPDX-License-Identifier: Apache-2.0

use super::{dimension_core_is_bounded, entry_range, ENTRY_END};

fn initial_entry() -> Vec<u8> {
    b"\xe0\x00feat_defs_911\0\xf4\x06\xf8\x50\xf7\x82\x15\xfb\xe3\xf7\x03\x85\x84\xf6\x02".to_vec()
}

#[test]
fn first_current_key_does_not_treat_the_stored_count_as_a_key_count() {
    let mut source = initial_entry();
    let start = source.len();
    source.extend_from_slice(b"\xe3S2D0015\0");
    source.extend_from_slice(ENTRY_END);
    assert_eq!(entry_range(&source, 1412, 16), Some(start..source.len()));
}

#[test]
fn copied_current_key_is_distinct_from_its_replay_source() {
    let mut source = initial_entry();
    source.extend_from_slice(b"\xe3S2D0015\0");
    source.extend_from_slice(ENTRY_END);
    let first_end = source.len();
    source.extend_from_slice(b"\xf4\x05\xe3\x8c\x3a\xf6\x02");
    let copied_start = source.len();
    source.extend_from_slice(
        b"\xe3\xc0\xab\xcd\xe3\xf7\x81\x50\x85\x84\xe1\x00\x00\xe1\xe3S2D0015\0",
    );
    source.extend_from_slice(ENTRY_END);
    assert_eq!(
        entry_range(&source, 3130, 16),
        Some(copied_start..source.len())
    );
    assert_eq!(
        entry_range(&source, 1412, 16).map(|range| range.end),
        Some(first_end)
    );
}

#[test]
fn unsupported_next_entry_delimits_but_does_not_gain_ownership() {
    let mut source = initial_entry();
    source.extend_from_slice(b"\xe3S2D0015\0");
    source.extend_from_slice(ENTRY_END);
    let first_end = source.len();
    source.extend_from_slice(b"\x00\xe3\x8c\x3a\xf6\x02\xe3S2D0016\0");
    source.extend_from_slice(ENTRY_END);
    assert_eq!(
        entry_range(&source, 1412, 16).map(|range| range.end),
        Some(first_end)
    );
    assert!(entry_range(&source, 3130, 16).is_none());
}

#[test]
fn duplicated_current_keys_do_not_choose_one_entry() {
    let mut source = initial_entry();
    source.extend_from_slice(ENTRY_END);
    source.extend_from_slice(b"\xe3\x85\x84\xf6\x02");
    source.extend_from_slice(ENTRY_END);
    assert!(entry_range(&source, 1412, 16).is_none());
}

#[test]
fn an_unauthenticated_duplicate_key_still_prevents_selection() {
    let mut source = initial_entry();
    source.extend_from_slice(ENTRY_END);
    source.extend_from_slice(b"\x00\xe3\x85\x84\xf6\x02");
    source.extend_from_slice(ENTRY_END);
    assert!(entry_range(&source, 1412, 16).is_none());
}

#[test]
fn multiple_closures_do_not_certify_one_table_scope() {
    let mut source = initial_entry();
    source.extend_from_slice(ENTRY_END);
    source.extend_from_slice(ENTRY_END);
    assert!(entry_range(&source, 1412, 16).is_none());
}

#[test]
fn a_truncated_or_unsupported_first_header_cannot_supply_a_key() {
    let mut source = initial_entry();
    source.pop();
    assert!(entry_range(&source, 1412, 16).is_none());
    let mut source = initial_entry();
    let flag = source.iter().position(|byte| *byte == 0xf4).expect("flag");
    source[flag + 1] = 4;
    source.extend_from_slice(ENTRY_END);
    assert!(entry_range(&source, 1412, 16).is_none());
}

#[test]
fn unknown_nested_heads_do_not_leak_a_later_table_backwards() {
    let mut source = initial_entry();
    source.extend_from_slice(b"\xe3\x8c\x3a\xf6\x02\xe3S2D0016\0");
    source.extend_from_slice(ENTRY_END);
    assert!(entry_range(&source, 1412, 16).is_none());
    assert!(entry_range(&source, 3130, 16).is_none());
}

#[test]
fn dimensional_core_requires_its_full_field_images_within_one_entry() {
    use crate::feature::definitions::{DimensionValue, FeatureDimension};
    let source = [2, 0x2a, 0xc0, 0, 0, 0x18, 0];
    let mut row = FeatureDimension {
        dimension_type: 2,
        value: DimensionValue::Resolved(0.125),
        value_body: vec![0x2a, 0xc0, 0],
        direction_byte: 0,
        auxiliary_value: Some(0.0),
        auxiliary_body: vec![0x18],
        external_id: 0,
        references: None,
        offset: 0,
    };
    assert!(dimension_core_is_bounded(&source, &(0..7), &row));
    assert!(!dimension_core_is_bounded(&source, &(0..6), &row));
    row.external_id = 4;
    assert!(!dimension_core_is_bounded(&source, &(0..7), &row));
    row.offset = usize::MAX;
    assert!(!dimension_core_is_bounded(&source, &(0..usize::MAX), &row));
}
