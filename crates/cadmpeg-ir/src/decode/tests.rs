// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use crate::codec::CodecError;

use super::*;

fn policy_with(mut edit: impl FnMut(&mut ResourceLimits)) -> DecodePolicy {
    let mut policy = DecodePolicy::default();
    edit(&mut policy.limits);
    policy
}

#[test]
fn root_limit_is_enforced() {
    let bytes = [0_u8; 5];
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_input_bytes = 4);
    assert!(matches!(
        DecodeContext::from_root_bytes(&bytes, &arena, &policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::InputBytes
    ));
}

#[test]
fn views_bound_reads_and_children() {
    let bytes = [0, 1, 2, 3, 4];
    let arena = DecodeArena::new();
    let (_, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let mut child = root.child(1, 4).unwrap();
    assert_eq!(child.take(3), Some(&bytes[1..4]));
    assert!(child.take(1).is_none());
    assert!(root.child(3, 6).is_none());
}

#[test]
fn counted_requires_a_physically_possible_count() {
    let bytes = [0_u8; 8];
    let arena = DecodeArena::new();
    let (_, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    assert!(root.counted(3, 4).is_none());
    assert_eq!(root.counted(2, 4).unwrap().get(), 2);
}

#[test]
fn exact_expansion_enforces_size_and_limit() {
    let bytes = [0_u8; 4];
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| {
        limits.max_decompressed_bytes_total = 8;
        limits.max_decompressed_bytes_per_expand = 8;
    });
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut writer = ctx.begin_expand(root, ExpandSpec::Exact(4)).unwrap();
    writer.write(&[1, 2, 3, 4]).unwrap();
    let view = writer.finalize().unwrap();
    assert_eq!(view.window(), &[1, 2, 3, 4]);

    let mut writer = ctx.begin_expand(root, ExpandSpec::Unknown).unwrap();
    assert!(matches!(
        writer.write(&[0; 9]),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn concatenation_and_stored_slices_have_distinct_spaces() {
    let bytes = [0, 1, 2, 3, 4, 5];
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let first = root.child(0, 2).unwrap();
    let second = root.child(4, 6).unwrap();
    let concat = ctx.concat_views(&[first, second]).unwrap();
    assert_eq!(concat.window(), &[0, 1, 4, 5]);
    let slice = ctx
        .register_slice(root, ByteRange { start: 1, end: 3 })
        .unwrap();
    assert_eq!(slice.window(), &[1, 2]);
    assert_ne!(concat.space(), slice.space());
}
