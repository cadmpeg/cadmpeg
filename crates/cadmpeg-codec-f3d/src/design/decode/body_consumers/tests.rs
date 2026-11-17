// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::design::test_support::{design_type, primary_record};
use crate::test_support::{indexed_header, lp_utf16};

const SELECTION: &str = "9716F783-676D-42E0-93F9-EBF273E7C035";
const SELECTION_BASE: &str = "6144EA59-3ACA-4172-9F14-A6531E94D69B";
const UUID: &str = "11111111-2222-3333-4444-555555555555";

fn local(bytes: &mut Vec<u8>, target: u64) {
    bytes.push(1);
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
}

fn source(consumer: &str, hidden: bool, flag: bool, first_run: bool) -> (Vec<u8>, MetaStream) {
    let (mut bytes, selection) = crate::test_support::body_reference_tail_test::frame(false, 9, 0);
    bytes.truncate(selection.member_end - 4 - LOCAL_REFERENCE_LEN);
    local(&mut bytes, 30);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    if first_run {
        let first_count = selection.member_end - 4 - LOCAL_REFERENCE_LEN - 8;
        bytes[first_count..first_count + 4].copy_from_slice(&1u32.to_le_bytes());
        let mut reference = Vec::new();
        local(&mut reference, 31);
        bytes.splice(first_count + 4..first_count + 4, reference);
    }
    local(&mut bytes, 40);
    let node_start = bytes.len();
    indexed_header(&mut bytes, *b"257", 30);
    bytes.extend_from_slice(&[0; 10]);
    lp_utf16(&mut bytes, UUID);
    bytes.extend_from_slice(&[u8::from(hidden), u8::from(flag)]);
    local(&mut bytes, 50);
    local(&mut bytes, 60);
    let scope_start = bytes.len();
    indexed_header(&mut bytes, *b"258", 40);
    let meta = MetaStream {
        types: vec![
            design_type(SELECTION, Some(SELECTION_BASE), 1, "Fusion", vec![17]),
            design_type(
                crate::design::presentation::BROWSER_NODE_TYPE_GUID,
                None,
                2,
                "Fusion",
                vec![30],
            ),
            design_type(consumer, None, 0, "Fusion", vec![40]),
        ],
        records: vec![
            primary_record(17, 0),
            primary_record(30, node_start),
            primary_record(40, scope_start),
        ],
        secondary_records: Vec::new(),
    };
    (bytes, meta)
}

#[test]
fn hidden_body_consumption_uses_the_second_run_and_either_browser_flag() {
    for consumer in [COMBINE_TYPE_GUID, REMOVE_BODY_TYPE_GUID] {
        for flag in [false, true] {
            for first_run in [false, true] {
                let (bytes, meta) = source(consumer, true, flag, first_run);
                let consumed = consumed_body_entities(View::over_retained(&bytes), &meta).unwrap();
                assert_eq!(consumed, HashSet::from([50]));
            }
        }
    }
}

#[test]
fn visible_nodes_and_other_scope_types_supply_no_consumption() {
    for (consumer, hidden) in [(COMBINE_TYPE_GUID, false), (SELECTION_BASE, true)] {
        let (bytes, meta) = source(consumer, hidden, true, false);
        assert!(consumed_body_entities(View::over_retained(&bytes), &meta)
            .unwrap()
            .is_empty());
    }
}
