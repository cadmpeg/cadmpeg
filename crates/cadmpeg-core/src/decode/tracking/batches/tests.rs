// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use crate::decode::{
    ByteRange, DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, ResourceDimension,
};
use crate::CodecError;

#[test]
fn member_local_duplicate_ids_keep_distinct_actual_extents_and_unbatched_identity() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABCDE", &arena, &DecodePolicy::default()).expect("root");
    let (first, first_batch) = ctx
        .record_batch(|| {
            ctx.record(
                "native:record#1",
                root.child(0, 2).expect("first extent"),
                || Ok(root.read_at(0, 2).expect("first actual value").to_vec()),
            )
        })
        .expect("first member");
    let (second, second_batch) = ctx
        .record_batch(|| {
            ctx.record(
                "native:record#1",
                root.child(2, 4).expect("second extent"),
                || Ok(root.read_at(2, 2).expect("second actual value").to_vec()),
            )
        })
        .expect("second member");
    ctx.record(
        "native:record#1",
        root.child(4, 5).expect("unbatched extent"),
        || {
            assert_eq!(root.read_at(4, 1), Some(b"E".as_slice()));
            Ok(())
        },
    )
    .expect("unbatched namespace");
    assert_eq!(first, b"AB");
    assert_eq!(second, b"CD");
    assert_ne!(first_batch, second_batch);
    let captures = ctx.take_record_captures().expect("outer final extraction");
    assert_eq!(captures.len(), 3);
    let unbatched = captures
        .iter()
        .find(|capture| capture.batch_id().is_none())
        .expect("unbatched record");
    assert_eq!(
        unbatched.reads(),
        &BTreeMap::from([("file".to_owned(), vec![4..5])])
    );
    for (batch, offset, ranges) in [(first_batch, 0, vec![0..2]), (second_batch, 2, vec![2..4])] {
        let capture = captures
            .iter()
            .find(|capture| capture.batch_id() == Some(batch))
            .expect("member capture");
        assert_eq!(capture.id(), "native:record#1");
        assert_eq!((capture.offset(), capture.byte_len()), (offset, 2));
        assert_eq!(
            capture.reads(),
            &BTreeMap::from([("file".to_owned(), ranges)])
        );
    }
}

#[test]
fn duplicates_are_still_refused_inside_a_batch_and_in_unbatched_scopes() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::default()).expect("root");
    let (_, batch) = ctx
        .record_batch(|| {
            ctx.record("same", root, || {
                assert_eq!(root.read_at(0, 1), Some(b"A".as_slice()));
                Ok(())
            })?;
            let mut ran = false;
            assert!(ctx
                .record("same", root, || {
                    ran = true;
                    Ok(())
                })
                .is_err());
            assert!(!ran);
            ctx.record("other", root, || Ok(()))?;
            Ok(())
        })
        .expect("batch remains usable after caught duplicate");
    ctx.record("same", root, || Ok(()))
        .expect("unbatched first");
    assert!(ctx.record("same", root, || Ok(())).is_err());
    let captures = ctx.take_record_captures().expect("final extraction");
    assert_eq!(
        captures
            .iter()
            .filter(|capture| capture.batch_id() == Some(batch))
            .count(),
        2
    );
    assert_eq!(
        captures
            .iter()
            .filter(|capture| capture.batch_id().is_none())
            .count(),
        1
    );
}

#[test]
fn nested_batches_restore_the_outer_namespace_and_cannot_export_before_ancestors_close() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABC", &arena, &DecodePolicy::default()).expect("root");
    let ancestor = ctx.read_transaction().expect("ancestor");
    let (inner_batch, outer_batch) = ctx
        .record_batch(|| {
            assert!(ctx.take_record_captures().is_err());
            ctx.record("same", root, || {
                assert_eq!(root.read_at(0, 1), Some(b"A".as_slice()));
                Ok(())
            })?;
            let (_, inner_batch) = ctx.record_batch(|| {
                ctx.record("same", root, || {
                    assert_eq!(root.read_at(1, 1), Some(b"B".as_slice()));
                    Ok(())
                })
            })?;
            assert!(ctx.record("same", root, || Ok(())).is_err());
            ctx.record("last", root, || {
                assert_eq!(root.read_at(2, 1), Some(b"C".as_slice()));
                Ok(())
            })?;
            Ok(inner_batch)
        })
        .expect("nested batch");
    assert_ne!(inner_batch, outer_batch);
    assert!(ctx.take_record_captures().is_err());
    ancestor.commit().expect("actual ancestor commits");
    let captures = ctx.take_record_captures().expect("outer-only export");
    assert_eq!(captures.len(), 3);
    assert_eq!(
        captures
            .iter()
            .filter(|capture| capture.batch_id() == Some(outer_batch))
            .count(),
        2
    );
    let inner = captures
        .iter()
        .find(|capture| capture.batch_id() == Some(inner_batch))
        .expect("inner");
    assert_eq!(
        inner.reads(),
        &BTreeMap::from([("file".to_owned(), vec![1..2])])
    );
    assert!(ctx.record_batch(|| Ok(())).is_err());
    assert!(ctx.record("after-seal", root, || Ok(())).is_err());
    assert!(ctx.take_record_captures().is_err());
}

#[test]
fn failed_batch_rolls_back_descendant_captures_and_parent_reads_without_refunding_budget() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABCDEFG", &arena, &DecodePolicy::default()).expect("root");
    let good_batch = ctx
        .record("outer", root, || {
            assert_eq!(root.read_at(0, 2), Some(b"AB".as_slice()));
            let before = ctx.retained_bytes_available();
            let failed: Result<((), _), CodecError> = ctx.record_batch(|| {
                assert_eq!(root.read_at(1, 4), Some(b"BCDE".as_slice()));
                ctx.record_batch(|| {
                    ctx.record("same", root, || {
                        assert_eq!(root.read_at(4, 1), Some(b"E".as_slice()));
                        Ok(())
                    })
                })?;
                Err(CodecError::malformed("member failure"))
            });
            assert!(failed.is_err());
            assert!(ctx.retained_bytes_available() < before);
            let (_, good_batch) = ctx.record_batch(|| {
                assert_eq!(root.read_at(1, 2), Some(b"BC".as_slice()));
                ctx.record("same", root, || {
                    assert_eq!(root.read_at(5, 1), Some(b"F".as_slice()));
                    Ok(())
                })
            })?;
            Ok(good_batch)
        })
        .expect("surviving outer");
    let captures = ctx.take_record_captures().expect("captures");
    assert_eq!(captures.len(), 2);
    let outer = captures
        .iter()
        .find(|capture| capture.batch_id().is_none())
        .expect("outer");
    assert_eq!(
        outer.reads(),
        &BTreeMap::from([("file".to_owned(), vec![0..3])])
    );
    let good = captures
        .iter()
        .find(|capture| capture.batch_id() == Some(good_batch))
        .expect("good sibling");
    assert_eq!(
        good.reads(),
        &BTreeMap::from([("file".to_owned(), vec![5..6])])
    );
}

#[test]
fn committed_batch_disappears_with_failed_ancestor_and_its_token_is_never_reused() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::default()).expect("root");
    let ancestor = ctx.read_transaction().expect("ancestor");
    let (_, discarded) = ctx
        .record_batch(|| {
            ctx.record("same", root, || {
                assert_eq!(root.read_at(0, 1), Some(b"A".as_slice()));
                Ok(())
            })
        })
        .expect("locally committed member");
    let after = ctx.retained_bytes_available();
    drop(ancestor);
    assert_eq!(ctx.retained_bytes_available(), after);
    let (_, surviving) = ctx
        .record_batch(|| {
            ctx.record("same", root, || {
                assert_eq!(root.read_at(1, 1), Some(b"B".as_slice()));
                Ok(())
            })
        })
        .expect("later member");
    assert_ne!(discarded, surviving);
    let captures = ctx.take_record_captures().expect("captures");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].batch_id(), Some(surviving));
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".to_owned(), vec![1..2])])
    );
}

#[test]
fn batch_storage_and_entities_use_the_original_caller_budget_and_preserve_its_fuse() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::default()).expect("root");
    ctx.retain_bytes(
        ctx.retained_bytes_available(),
        "exhaust original retained budget",
    )
    .expect("last retained bytes");
    let mut ran = false;
    let error = ctx
        .record_batch(|| {
            ran = true;
            Ok(())
        })
        .expect_err("batch frame is budgeted");
    assert!(!ran);
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
    );
    assert!(ctx.take_record_captures().is_err());

    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 1;
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"AB", &arena, &policy).expect("second session");
    ctx.record_batch(|| {
        ctx.charge_entities(1, "first member record")?;
        ctx.record("same", root, || Ok(()))
    })
    .expect("first charge");
    let error = ctx
        .record_batch(|| {
            ctx.charge_entities(1, "second member record")?;
            Ok(())
        })
        .expect_err("batch cannot reset entity totals");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::Entities)
    );
    assert!(ctx.record_batch(|| Ok(())).is_err());
    assert!(ctx.finish_session().is_err());
}

#[test]
fn foreign_views_remain_refused_and_batch_tokens_do_not_collide_between_sessions() {
    let arena = DecodeArena::new();
    let (first, root) =
        DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::default()).expect("first");
    let (second, foreign) =
        DecodeContext::from_root_bytes(b"CD", &arena, &DecodePolicy::default()).expect("second");
    let mut ran = false;
    assert!(first
        .record_batch(|| first.record("same", foreign, || {
            ran = true;
            Ok(())
        }))
        .is_err());
    assert!(!ran);
    let (_, first_batch) = first
        .record_batch(|| first.record("same", root, || Ok(())))
        .expect("first real view");
    let (_, second_batch) = second
        .record_batch(|| second.record("same", foreign, || Ok(())))
        .expect("second real view");
    assert_ne!(first_batch, second_batch);
    assert_eq!(
        first.take_record_captures().expect("first captures")[0].batch_id(),
        Some(first_batch)
    );
    assert_eq!(
        second.take_record_captures().expect("second captures")[0].batch_id(),
        Some(second_batch)
    );
}

#[test]
fn batch_records_keep_named_alias_and_compressed_ancestor_read_provenance() {
    let arena = DecodeArena::new();
    let bytes = [90, 1, 3, 0, 252, 255, b'a', b'b', b'c', 91];
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("root");
    let alias = ctx
        .register_named_slice(root, ByteRange { start: 1, end: 9 }, "member/compressed")
        .expect("actual stored alias");
    let output = ctx
        .inflate_probe(alias, 3, false)
        .expect("decompress")
        .expect("raw stored block");
    let mut expansion = ctx
        .begin_expand(ExpandSpec::Exact(3))
        .expect("shared expansion budget");
    expansion.write(&output).expect("actual decoded bytes");
    let decoded = expansion
        .finalize_from(alias, "member/decoded")
        .expect("compressed source provenance");
    let (_, batch) = ctx
        .record_batch(|| {
            ctx.record("same", decoded, || {
                assert_eq!(decoded.read_at(1, 1), Some(b"b".as_slice()));
                Ok(())
            })
        })
        .expect("member record");
    let captures = ctx.take_record_captures().expect("captures");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].batch_id(), Some(batch));
    assert_eq!(captures[0].stream(), "member/decoded");
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([
            ("member/decoded".to_owned(), vec![1..2]),
            ("member/compressed".to_owned(), vec![0..8]),
            ("file".to_owned(), vec![1..9]),
        ])
    );
}

#[test]
fn dropped_context_batch_tokens_and_stale_views_cannot_alias_a_reused_session() {
    let arena = DecodeArena::new();
    let (old, stale) = DecodeContext::from_root_bytes(b"OLD", &arena, &DecodePolicy::default())
        .expect("old context");
    let (_, old_batch) = old
        .record_batch(|| old.record("same", stale, || Ok(())))
        .expect("old batch");
    drop(old);
    assert_eq!(stale.read_at(0, 3), Some(b"OLD".as_slice()));
    let (current, root) = DecodeContext::from_root_bytes(b"NEW", &arena, &DecodePolicy::default())
        .expect("new context");
    assert!(current
        .record_batch(|| current.record("same", stale, || Ok(())))
        .is_err());
    let (_, current_batch) = current
        .record_batch(|| {
            current.record("same", root, || {
                assert_eq!(stale.read_at(0, 2), Some(b"OL".as_slice()));
                assert_eq!(root.read_at(1, 1), Some(b"E".as_slice()));
                Ok(())
            })
        })
        .expect("new genuine record");
    assert_ne!(old_batch, current_batch);
    let captures = current.take_record_captures().expect("current captures");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].batch_id(), Some(current_batch));
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".to_owned(), vec![1..2])])
    );
}
