// SPDX-License-Identifier: Apache-2.0

use super::super::take_spline_mixed_derivatives;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn assert_fixed_read(body: &[u8], start: usize, expected: Option<[[f64; 3]; 4]>, end: usize) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let cache = crate::scalar::ScalarCache::default();
    let mut cursor = start;
    assert_eq!(take_spline_mixed_derivatives(&ctx, body, &mut cursor, &cache)
        .expect("fixed literal slots have no input-sized admission"), expected);
    assert_eq!(cursor, end);
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.charge_work_limit(1, "after fixed positional mixed slots")
        .expect_err("zero cap");
    assert_eq!((original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, 0, 1));
    for _ in 0..2 {
        let mut cursor = start;
        assert!(matches!(take_spline_mixed_derivatives(&ctx, body, &mut cursor, &cache),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(cursor, start);
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn positional_mixed_slots_use_fixed_storage_and_preserve_scalar_widths() {
    let body = [0xe3, 0xe3, 0xe3, 0x0d, 0x0f, 0xe4, 0xe4, 0x0d, 0x0f,
        0x0f, 0xe4, 0x0d, 0x0d, 0xe4, 0x0f, 0xe3];
    assert_fixed_read(&body, 3, Some([[-1.0, 0.0, 1.0], [1.0, -1.0, 0.0],
        [0.0, 1.0, -1.0], [-1.0, 1.0, 0.0]]), 15);
    let mut terminal_zero = [0x0f; 12];
    terminal_zero[11] = 0x18;
    assert_fixed_read(&terminal_zero, 0, Some([[0.0; 3]; 4]), 12);
    // The named mixed lane's 28 form supplies the leading 3f IEEE byte.
    let mut wide = vec![0x28];
    wide.extend_from_slice(&1.0_f64.to_be_bytes()[1..]);
    wide.extend_from_slice(&[0x0f; 11]);
    assert_fixed_read(&wide, 0, Some([[1.0, 0.0, 0.0], [0.0; 3], [0.0; 3], [0.0; 3]]), 19);
}

#[test]
fn positional_mixed_slots_keep_extent_and_partial_cursor_recovery() {
    for length in 0..12 {
        assert_fixed_read(&[0x0f; 12][..length], 0, None, 0);
    }
    assert_fixed_read(&[0x0f; 12], 13, None, 13);
    assert_fixed_read(&[0x0f; 12], usize::MAX, None, usize::MAX);
    for absent in [0, 5, 11] {
        let mut body = [0x0f; 18];
        body[absent] = 0;
        // An unrecognized scalar returns no value and does not advance
        // the cursor; earlier complete slots keep their original advancement.
        assert_fixed_read(&body, 0, None, absent);
    }
    let mut truncated = [0x0f; 12];
    truncated[11] = 0x28;
    assert_fixed_read(&truncated, 0, None, 11);
}
