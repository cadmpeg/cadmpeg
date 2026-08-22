// SPDX-License-Identifier: Apache-2.0
//! Unit and fixture tests for OM wire parsers owned by `om`.

#![allow(clippy::unwrap_used)]

pub(crate) use super::*;

#[test]
fn unique_candidate_stops_after_second_hit() {
    let mut yielded = 0;
    let result = super::unique_candidate((0..).inspect(|_| {
        yielded += 1;
    }));

    assert_eq!(result, None);
    assert_eq!(yielded, 2);
}

mod index_and_lanes;
mod instances_and_stores;
mod operation_data_block_references;
mod pattern_lanes;
mod sketch_payload;
mod state_block;
mod tagged_references;
