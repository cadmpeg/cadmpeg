// SPDX-License-Identifier: Apache-2.0
//! Complete A8 map admission and bounded sample allocation.

use super::parse;

use crate::test_support::test_parameter_map::payload;

#[test]
fn a8_map_retains_block_pairs_zero_fit_and_active_subrange() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for count in [2, 17, 127, 128, 232] {
        for descending in [false, true] {
            let parsed = parse(&ctx, &payload(count, descending, 0.0))
                .expect("bounded parse")
                .expect("complete map");
            assert_eq!((parsed.reference, parsed.co_curve), (1, 2));
            assert_eq!(parsed.reference_parameters.len(), usize::from(count));
            assert_eq!(parsed.co_parameters.len(), usize::from(count));
            assert_eq!(parsed.reference_parameters[0].get(), 0.0);
            assert_eq!(
                parsed.reference_parameters[usize::from(count) - 1].get(),
                1.0
            );
            assert_eq!(
                parsed.co_parameters[0].get(),
                if descending { 3.0 } else { 2.0 }
            );
            assert_eq!(parsed.active_reference_range.endpoints(), [0.25, 0.75]);
            assert_eq!(parsed.fit_tolerance.get(), 0.0);
        }
    }
}

#[test]
fn a8_map_declines_incomplete_counts_markers_and_numeric_invariants() {
    let bytes = payload(3, true, 0.0);
    let ctx = cadmpeg_test_support::service_decode_context();
    for end in 0..bytes.len() {
        assert!(parse(&ctx, &bytes[..end]).expect("bounded parse").is_none());
    }
    for (offset, value) in [
        (0, 0x84),
        (3, 0x80),
        (4, 0x14),
        (45, 0x81),
        (bytes.len() - 1, 0),
    ] {
        let mut wrong = bytes.clone();
        wrong[offset] = value;
        assert!(parse(&ctx, &wrong).expect("bounded parse").is_none());
    }
    for (offset, value) in [
        (5, 1.0),
        (13, f64::INFINITY),
        (21, -1.0),
        (46 + 8, 0.0),
        (46 + 24 + 8, 3.0),
        (46 + 24, 10.0),
        (bytes.len() - 25, -0.25),
        (bytes.len() - 17, 1.25),
        (bytes.len() - 9, -0.0),
    ] {
        let mut wrong = bytes.clone();
        wrong[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        assert!(
            parse(&ctx, &wrong).expect("bounded parse").is_none(),
            "offset {offset}"
        );
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(parse(&ctx, &trailing).expect("bounded parse").is_none());
}

#[test]
fn a8_map_samples_are_bounded_before_collection_allocation() {
    let bytes = payload(232, false, 0.0);
    let error = crate::test_support::with_work_limit(0, |ctx| parse(ctx, &bytes))
        .expect_err("parse charges work");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    let error = crate::test_support::with_collection_limit(1, |ctx| parse(ctx, &bytes))
        .expect_err("parsed arrays charge collection items");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}
