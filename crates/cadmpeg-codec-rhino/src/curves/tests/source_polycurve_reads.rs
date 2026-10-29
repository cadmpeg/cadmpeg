// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use crate::chunks::ArchiveVersion;
use crate::curves::GeometryError;
use crate::settings::MillimeterScale;
use crate::test_support::test_archive;

fn children() -> Vec<([u8; 16], Vec<u8>)> {
    vec![
        (
            test_archive::LINE_CLASS,
            test_archive::line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]),
        ),
        (
            test_archive::LINE_CLASS,
            test_archive::line_payload([1.0, 0.0, 0.0], [2.0, 1.0, 0.0], [0.0, 1.0]),
        ),
    ]
}

#[test]
fn polycurve_source_entry_keeps_the_actual_caller_depth_refusal_latched() {
    let fixture = test_archive::polycurve_source(&[-2.0, 1.0, 6.0], &children());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&fixture.bytes, &arena, &policy).expect("source");
    let GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) =
        crate::curves::decode(
            &ctx,
            &fixture.bytes,
            crate::curves::POLYCURVE,
            0..fixture.bytes.len(),
            MillimeterScale::IDENTITY,
            ArchiveVersion::V5,
        )
        .expect_err("caller allowance")
    else {
        panic!("actual caller resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.limit, 0);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
}

#[test]
fn unbacked_polycurve_child_count_fails_before_parameter_allocation_without_fusing_the_caller() {
    let mut bytes = vec![0x10];
    bytes.extend(i32::MAX.to_le_bytes());
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("source");
    let failure = crate::curves::decode(
        &ctx,
        &bytes,
        crate::curves::POLYCURVE,
        0..bytes.len(),
        MillimeterScale::IDENTITY,
        ArchiveVersion::V5,
    )
    .expect_err("declared children must fit before any parameter allocation");
    assert!(
        matches!(failure, GeometryError::Malformed(crate::chunks::FramingError::InvalidLength {
        offset: 1, value,
    }) if value == i128::from(i32::MAX))
    );
    ctx.charge_entities(1, "valid peer after malformed polycurve count")
        .expect("malformed count is not a fabricated resource refusal");
}
