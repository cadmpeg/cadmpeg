// SPDX-License-Identifier: Apache-2.0
//! Unit and fixture tests for OM wire parsers owned by `om`.

#![allow(clippy::unwrap_used)]

#[test]
fn unique_candidate_stops_after_second_hit() {
    let mut yielded = 0;
    let result = super::unique_candidate((0..).inspect(|_| {
        yielded += 1;
    }));

    assert_eq!(result, None);
    assert_eq!(yielded, 2);
}

pub(crate) fn message_bytes(text: &[u8], value: &[u8], count_or_severity: [u8; 2]) -> Vec<u8> {
    let declared_length = u8::try_from(text.len() + 2).expect("short synthesized message");
    let mut bytes = vec![0x03, declared_length];
    bytes.extend_from_slice(text);
    bytes.extend([0, 0, 0, 0, 0]);
    bytes.extend_from_slice(value);
    bytes.extend(count_or_severity);
    bytes
}

mod control_lanes;
mod index_and_lanes;
mod instances_and_stores;
mod operation_records;
mod operation_state;
mod pattern_lanes;
mod registry;
mod sketch_payload;
