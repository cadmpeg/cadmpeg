// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use crate::chunks::ArchiveVersion;
use crate::curves::GeometryError;
use crate::settings::MillimeterScale;
use crate::test_support::test_archive::polyline_payload;

#[test]
fn unbacked_polyline_counts_fail_before_allocating_the_claimed_points() {
    for count in [-1_i32, i32::MAX] {
        let mut bytes = vec![0x10];
        bytes.extend(count.to_le_bytes());
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
        let failure = crate::curves::decode(
            &ctx,
            &bytes,
            crate::curves::POLYLINE,
            0..bytes.len(),
            MillimeterScale::IDENTITY,
            ArchiveVersion::V5,
        )
        .expect_err("unbacked point count cannot allocate");
        assert!(
            matches!(failure, GeometryError::Malformed(crate::chunks::FramingError::InvalidLength { offset: 1, value }) if value == i128::from(count))
        );
        ctx.charge_entities(1, "valid sibling after malformed count")
            .expect("malformed input is not a fabricated resource refusal");
    }
}

#[test]
fn polyline_source_entry_preserves_the_actual_caller_depth_allowance() {
    let bytes = polyline_payload(&[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0]], &[10.0, 12.0]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root source");
    let failure = crate::curves::decode(
        &ctx,
        &bytes,
        crate::curves::POLYLINE,
        0..bytes.len(),
        MillimeterScale::IDENTITY,
        ArchiveVersion::V5,
    )
    .expect_err("caller depth allowance is not reset");
    let GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) = failure else {
        panic!("real caller resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.limit, 0);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
