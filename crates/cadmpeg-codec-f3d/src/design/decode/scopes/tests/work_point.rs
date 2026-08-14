// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

#[test]
fn work_point_direct_record_carries_model_space_position() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"427");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "WorkPoint");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);

    let point_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 27]);
    let position_at = bytes.len();
    for value in [1.25, -2.5, 3.75] {
        bytes.extend_from_slice(&f64::to_le_bytes(value));
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    for _ in 0..3 {
        bytes.extend_from_slice(&f64::to_le_bytes(-1.0));
    }
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for target in [56u32, 57] {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(target).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.resize(point_at + 208, 0);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&55u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "427".into(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("WorkPoint scope");
    let frame = exact_work_point_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(frame.position, [1.25, -2.5, 3.75]);
    assert_eq!(frame.position_offset, position_at as u64);
    assert_eq!(frame.rule.reference_type(), 7);
    assert_eq!(work_point_input_indices(&frame.rule), [56, 57]);
    bytes[point_at + 66..point_at + 70].copy_from_slice(&1u32.to_le_bytes());
    bytes[point_at + 94..point_at + 98].copy_from_slice(&1u32.to_le_bytes());
    bytes.drain(point_at + 197..point_at + 208);
    let frame = exact_work_point_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(frame.position, [1.25, -2.5, 3.75]);
    assert_eq!(frame.position_offset, position_at as u64);
    assert_eq!(frame.rule.reference_type(), 1);
    assert_eq!(work_point_input_indices(&frame.rule), [56]);
}

#[test]
fn work_point_input_count_frames_the_rule_inputs() {
    // The counted input run is framed by its serialized count. The rule
    // selector is retained independently and does not impose a fixed arity.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"427");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "WorkPoint");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);

    let point_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 27]);
    let position_at = bytes.len();
    for value in [4.0, 5.0, 6.0] {
        bytes.extend_from_slice(&f64::to_le_bytes(value));
    }
    bytes.extend_from_slice(&18u32.to_le_bytes());
    for _ in 0..3 {
        bytes.extend_from_slice(&f64::to_le_bytes(-1.0));
    }
    let count_at = bytes.len();
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for target in [56u32, 57] {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(target).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.resize(point_at + 208, 0);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&55u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "427".into(),
        byte_offset: 0,
    };
    let records = IndexedRecordOffsets::build(&bytes);
    let scope = parse_parameter_scope(&bytes, &records, &header).expect("WorkPoint scope");
    let frame = exact_work_point_construction(&bytes, &records, &scope, &HashMap::new())
        .expect("work point frame");
    assert_eq!(frame.rule.reference_type(), 18);
    assert_eq!(work_point_input_indices(&frame.rule), [56, 57]);

    bytes[count_at..count_at + 4].copy_from_slice(&1u32.to_le_bytes());
    let records = IndexedRecordOffsets::build(&bytes);
    let frame = exact_work_point_construction(&bytes, &records, &scope, &HashMap::new())
        .expect("work point frame");
    assert_eq!(frame.rule.reference_type(), 18);
    assert_eq!(work_point_input_indices(&frame.rule), [56]);

    // A rule above the values the shipped range check admitted still names a
    // coordinate when its input arity agrees.
    bytes[position_at + 24..position_at + 28].copy_from_slice(&64u32.to_le_bytes());
    let records = IndexedRecordOffsets::build(&bytes);
    let frame = exact_work_point_construction(&bytes, &records, &scope, &HashMap::new())
        .expect("work point frame");
    assert_eq!(frame.position, [4.0, 5.0, 6.0]);
    assert_eq!(frame.rule.reference_type(), 64);
    assert_eq!(work_point_input_indices(&frame.rule), [56]);
}

fn work_point_input_indices(rule: &crate::records::DesignWorkPointRule) -> Vec<u32> {
    rule.inputs()
        .iter()
        .map(|input| input.record_index)
        .collect()
}
