// SPDX-License-Identifier: Apache-2.0

use super::super::super::{parse_dimension_locus_group, parse_scoped};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn group_bytes() -> Vec<u8> {
    let mut bytes = vec![0; 86];
    for (at, value) in [(0, 3u32), (7, 249), (20, 1), (25, 175), (35, 2),
        (41, 172), (51, 1), (59, 1), (64, 175), (75, 3), (82, 250)] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[4..7].copy_from_slice(b"286");
    bytes[79..82].copy_from_slice(b"314");
    for at in [19, 24, 40, 63] {
        bytes[at] = 1;
    }
    bytes
}

fn retained_group(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<crate::records::dimensions::DesignDimensionLocusGroup, CodecError> {
    let parsed = parse_scoped(ctx, || {
        parse_dimension_locus_group(ctx, bytes, 0, 240, &[175], &[172])
    })?.expect("complete counted locus frame");
    let storage = parsed.1;
    let group = parsed.0;
    storage.commit_value(group)
}

#[test]
fn locus_group_promotes_only_surviving_loci_and_tags() {
    let bytes = group_bytes();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let group = retained_group(&ctx, &bytes).unwrap();
    assert_eq!(group.loci.len(), 1);
    assert_eq!(group.loci[0].geometry_record_index, 175);
    assert_eq!(group.loci[0].returned.value, 175);
    let actual_output = u64::try_from(
        group.loci.capacity() * std::mem::size_of::<crate::records::dimensions::DesignDimensionLocus>()
            + group.class_tag.as_str().len() + group.next_class_tag.as_str().len(),
    ).unwrap();
    assert!(matches!(ctx.charge_retained(u64::MAX, "probe locus retained backing"),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes
            && limit.used == actual_output));
}

#[test]
fn locus_group_releases_geometry_and_transferred_scratch() {
    let bytes = group_bytes();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let group = retained_group(&ctx, &bytes).unwrap();
    assert_eq!(group.record_index, 249);
    assert!(matches!(ctx.reserve_scoped(u64::MAX, "probe released locus scratch"),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.used == 0));
    assert_eq!(group.next_record_index, 250);
}

#[test]
fn locus_group_keeps_original_promotion_refusal() {
    let bytes = group_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = retained_group(&ctx, &bytes).unwrap_err();
    let CodecError::ResourceLimit(ref original) = error else {
        panic!("expected retained promotion refusal: {error:?}");
    };
    assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(original.operation, "f3d dimension frame candidate");
    assert!(matches!(ctx.charge_work(0, "after locus promotion refusal"),
        Err(CodecError::ResourceLimit(limit)) if limit == *original));
}
