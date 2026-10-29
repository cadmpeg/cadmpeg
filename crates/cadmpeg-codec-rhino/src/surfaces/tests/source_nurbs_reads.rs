// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use crate::chunks::ArchiveVersion;
use crate::curves::GeometryError;
use crate::settings::MillimeterScale;
use crate::test_support::test_archive::nurbs_surface_source;

#[test]
fn unbacked_surface_counts_fail_before_allocating_the_claimed_lane_or_grid() {
    for count in [-1_i32, i32::MAX] {
        let fixture = nurbs_surface_source(Some([2.0, 4.0, 2.0, 4.0]));
        for offset in [
            fixture.u_knot_count_offset,
            fixture.v_knot_count_offset,
            fixture.cv_count_offset,
        ] {
            let mut bytes = fixture.bytes[..offset].to_vec();
            bytes.extend(count.to_le_bytes());
            let arena = DecodeArena::new();
            let policy = DecodePolicy::default();
            let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
            let failure = crate::curves::decode(
                &ctx,
                &bytes,
                crate::surfaces::NURBS_SURFACE,
                0..bytes.len(),
                MillimeterScale::IDENTITY,
                ArchiveVersion::V5,
            )
            .expect_err("unbacked count cannot allocate");
            assert!(
                matches!(failure, GeometryError::Malformed(crate::chunks::FramingError::InvalidLength { offset: actual, value }) if actual == offset && value == i128::from(count))
            );
            ctx.charge_entities(1, "valid sibling after malformed surface count")
                .expect("malformed count is not a fabricated resource refusal");
        }
    }
}

#[test]
fn nurbs_surface_source_entry_preserves_the_caller_depth_allowance_and_refusal() {
    let fixture = nurbs_surface_source(None);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&fixture.bytes, &arena, &policy).expect("root source");
    let failure = crate::curves::decode(
        &ctx,
        &fixture.bytes,
        crate::surfaces::NURBS_SURFACE,
        0..fixture.bytes.len(),
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
