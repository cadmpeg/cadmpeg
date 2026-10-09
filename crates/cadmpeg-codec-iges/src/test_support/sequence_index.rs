// SPDX-License-Identifier: Apache-2.0

use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use super::test_owned::{owned_test_file, OwnedTestEntity};
use cadmpeg_core::decode::u64_from_index;

pub(crate) fn parameter_inputs(count: usize) -> (Vec<ParameterRecord>, ProjectedGlobal) {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 116, form: 0, label: "POINT".into(), status: "00000000",
        parameters: "116,1,2,3,0;".into(),
    }]);
    super::with_service_context(&bytes, |ctx| {
        let scan = crate::card::scan_with_context(&bytes, ctx).unwrap();
        let (global, _, _storage) = crate::global::parse(&scan, ctx).unwrap();
        let (directory, quarantined) = crate::directory::parse(&scan, global.global_table(), ctx).unwrap();
        assert!(quarantined.is_empty());
        let assembled = crate::parameter::assemble_with_context(
            &scan, &directory, &quarantined, &global, ctx,
        ).unwrap();
        assert_eq!(assembled.records.len(), 1);
        let parameters = (0..count).map(|index| {
            let mut record = assembled.records[0].clone();
            record.directory_sequence = u32::try_from(2 * index + 1).unwrap();
            record
        }).collect();
        (parameters, global.length_context().unwrap())
    })
}

/// Source-derived admission for unique u32 keys and borrowed record pointers.
pub(crate) fn work(count: usize) -> u64 {
    let alignment = std::mem::align_of::<u32>()
        .max(std::mem::align_of::<&ParameterRecord>()).max(std::mem::align_of::<usize>());
    let node = u64_from_index(11 * (std::mem::size_of::<u32>()
        + std::mem::size_of::<&ParameterRecord>()) + 16 * std::mem::size_of::<usize>()
        + 2 * alignment);
    (0..count).map(|index| {
        let comparisons = if index == 0 { 0 } else {
            let height = index.div_ceil(2).ilog(6) + 1;
            u64_from_index(index).min(11 * u64::from(height))
        };
        // One next, two actual key queries, insertion shift/split node bound.
        1 + 2 * u64_from_index(std::mem::size_of::<u32>()) * comparisons
            + node * (1 + 2 * u64::from(index.is_multiple_of(5)))
    }).sum()
}
