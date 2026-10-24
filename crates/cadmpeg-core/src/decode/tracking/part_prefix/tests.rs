// SPDX-License-Identifier: Apache-2.0
//! Sorted boundary intervals preserve sparse holes without scanning earlier reads.

use super::*;
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn increasing_sparse_reads_keep_every_gap_using_the_monotone_append_path() {
    let budget = DecodeBudget::new(DecodePolicy::service(), 0);
    let mut reads = Vec::new();
    for index in 0..4096u64 {
        observe_read(&budget, &mut reads, SpaceId::ROOT, 2 * index..2 * index + 1).unwrap();
    }
    assert_eq!(reads.len(), 4096);
    for (index, (space, range)) in reads.iter().enumerate() {
        let start = 2 * u64::try_from(index).unwrap();
        assert_eq!(*space, SpaceId::ROOT);
        assert_eq!(*range, start..start + 1);
    }
    assert_eq!(budget.fused(), None);
}

#[test]
fn out_of_order_overlap_and_adjacency_merge_only_within_the_same_source_space() {
    let budget = DecodeBudget::new(DecodePolicy::service(), 0);
    let root = SpaceId::ROOT;
    let other = SpaceId::from_index(1);
    let mut reads = Vec::new();
    for (space, range) in [
        (root, 4..6),
        (other, 9..10),
        (root, 0..2),
        (root, 1..5),
        (root, 8..9),
    ] {
        observe_read(&budget, &mut reads, space, range).unwrap();
    }
    assert_eq!(reads, [(root, 0..6), (root, 8..9), (other, 9..10)]);
    observe_read(&budget, &mut reads, root, 6..8).unwrap();
    assert_eq!(reads, [(root, 0..9), (other, 9..10)]);
    observe_read(&budget, &mut reads, other, 0..1).unwrap();
    assert_eq!(reads, [(root, 0..9), (other, 0..1), (other, 9..10)]);
    observe_read(&budget, &mut reads, other, 0..10).unwrap();
    assert_eq!(reads, [(root, 0..9), (other, 0..10)]);
    observe_read(&budget, &mut reads, root, 3..4).unwrap();
    assert_eq!(reads, [(root, 0..9), (other, 0..10)]);
}

#[test]
fn actual_composite_projection_keeps_physical_gaps_and_rejects_reading_into_them() {
    for read_gap in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"a12d", &arena, &DecodePolicy::service()).unwrap();
        let joined = ctx
            .concat_named_views(
                &[root.child(0, 1).unwrap(), root.child(3, 4).unwrap()],
                "joined",
            )
            .unwrap();
        let prefix = joined.child(0, 1).unwrap();
        let outcome = ctx
            .part_transaction_discovered(prefix, joined, || {
                ctx.record("row", prefix, || {
                    assert_eq!(joined.read_at(0, 2), Some(&b"ad"[..]));
                    if read_gap {
                        assert_eq!(root.read_at(1, 1), Some(&b"1"[..]));
                    }
                    Ok((17, joined))
                })
            })
            .unwrap();
        assert_eq!(outcome.result().is_err(), read_gap);
        assert_eq!(outcome.capture().byte_len(), 2);
        let captures = ctx.take_record_captures().unwrap();
        if read_gap {
            assert!(captures.is_empty());
        } else {
            assert_eq!(captures.len(), 1);
            assert_eq!(captures[0].reads()["joined"], [0..2]);
            assert_eq!(captures[0].reads()["file"], [0..1, 3..4]);
        }
    }
}

#[test]
fn binary_prefix_coverage_keeps_sparse_holes_at_large_projection_counts() {
    let budget = DecodeBudget::new(DecodePolicy::service(), 0);
    let mut reads = Vec::new();
    for index in 0..4096u64 {
        observe_read(&budget, &mut reads, SpaceId::ROOT, 2 * index..2 * index + 1).unwrap();
    }
    for index in 0..4096u64 {
        assert!(contains_read(
            &reads,
            SpaceId::ROOT,
            &(2 * index..2 * index + 1)
        ));
        assert!(!contains_read(
            &reads,
            SpaceId::ROOT,
            &(2 * index + 1..2 * index + 2)
        ));
        assert!(!contains_read(
            &reads,
            SpaceId::ROOT,
            &(2 * index..2 * index + 2)
        ));
        assert!(!contains_read(
            &reads,
            SpaceId::from_index(1),
            &(2 * index..2 * index + 1)
        ));
    }
}

#[test]
fn one_real_composite_read_preserves_all_sparse_original_parent_ranges() {
    const SEGMENTS: usize = 2048;
    let bytes = vec![b'x'; 2 * SEGMENTS];
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let pieces = (0..SEGMENTS)
        .map(|index| root.child(2 * index, 2 * index + 1).unwrap())
        .collect::<Vec<_>>();
    let joined = ctx.concat_named_views(&pieces, "sparse-parent").unwrap();
    let outcome = ctx
        .part_transaction_discovered(joined.child(0, 1).unwrap(), joined, || {
            ctx.record("row", joined.child(0, 1).unwrap(), || {
                assert_eq!(joined.read_at(0, SEGMENTS).unwrap().len(), SEGMENTS);
                Ok((SEGMENTS, joined))
            })
        })
        .unwrap();
    assert_eq!(outcome.result().unwrap(), &SEGMENTS);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].reads()["sparse-parent"],
        [0..u64::try_from(SEGMENTS).unwrap()]
    );
    let original = &captures[0].reads()["file"];
    assert_eq!(original.len(), SEGMENTS);
    for (index, range) in original.iter().enumerate() {
        let start = 2 * u64::try_from(index).unwrap();
        assert_eq!(*range, start..start + 1);
    }
}

#[test]
fn discovered_fragment_navigation_keeps_the_actual_primary_and_consumed_extents_distinct() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::service()).unwrap();
    let prefix = root.child(0, 1).unwrap();
    let outcome = ctx
        .part_transaction_discovered(prefix, root, || {
            let (kind, owner) = ctx.record_owner("row", prefix, || {
                let mut actual = prefix;
                Ok(actual.req_u8()?)
            })?;
            let value = ctx.record_fragment(&owner, root, || {
                let mut body = root;
                body.skip(1).unwrap();
                Ok(body.req_u8()?)
            })?;
            Ok(((kind, value), root.child(0, 2).unwrap()))
        })
        .unwrap();
    assert_eq!(outcome.result().unwrap(), &(b'1', b'2'));
    assert_eq!(outcome.capture().byte_len(), 2);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "row");
    assert_eq!((captures[0].offset(), captures[0].byte_len()), (0, 1));
    assert_eq!(captures[0].reads()["file"], [0..2]);
}

#[test]
fn genuine_continuation_authority_does_not_permit_tail_reads_beyond_consumed_prefix() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::service()).unwrap();
    let prefix = root.child(0, 1).unwrap();
    let outcome = ctx
        .part_transaction_discovered(prefix, root, || {
            let (_, owner) = ctx.record_owner("row", prefix, || {
                let mut actual = prefix;
                Ok(actual.req_u8()?)
            })?;
            let value = ctx.record_fragment(&owner, root, || {
                let mut tail = root;
                tail.skip(3).unwrap();
                Ok(tail.req_u8()?)
            })?;
            Ok((value, root.child(0, 2).unwrap()))
        })
        .unwrap();
    assert!(
        matches!(outcome.result(), Err(CodecError::Malformed(message)) if message == "actual parser read escapes its consumed source prefix")
    );
    assert_eq!(outcome.capture().byte_len(), 4);
    assert_eq!(outcome.capture().attempted_record_ids(), ["row"]);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn a_new_primary_owner_cannot_claim_an_extent_beyond_the_discovered_prefix() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::service()).unwrap();
    let prefix = root.child(0, 1).unwrap();
    let outcome = ctx
        .part_transaction_discovered(prefix, root, || {
            let (value, _owner) = ctx.record_owner("row", root, || {
                let mut actual = prefix;
                Ok(actual.req_u8()?)
            })?;
            Ok((value, prefix))
        })
        .unwrap();
    assert!(
        matches!(outcome.result(), Err(CodecError::Malformed(message)) if message == "discovered record escapes its consumed source prefix")
    );
    assert_eq!(outcome.capture().attempted_record_ids(), ["row"]);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
