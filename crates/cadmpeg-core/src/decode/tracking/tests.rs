// SPDX-License-Identifier: Apache-2.0
//! Exact synthetic record/read boundaries and transaction ownership.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use crate::CodecError;

fn ranges(reads: Vec<Range<u64>>) -> BTreeMap<String, Vec<Range<u64>>> {
    BTreeMap::from([("file".to_owned(), reads)])
}

#[test]
fn nested_records_capture_actual_values_and_only_their_own_reads() {
    let arena = DecodeArena::new();
    let data = [3, 1, 99, 98, 5, 2, 97, 96, 95, 94, 93, 92, 7, 91, 90, 89];
    let (ctx, root) =
        DecodeContext::from_root_bytes(&data, &arena, &DecodePolicy::default()).expect("root");
    let value = ctx
        .record("outer", root, || {
            let mut input = root;
            let first = input.u16_le().expect("outer scalar");
            input.skip(2).expect("navigation");
            let child = input.req_take_child(8).expect("bounded child");
            let inner = ctx.record("inner", child, || {
                let mut input = child;
                Ok(input.u16_le().expect("inner scalar"))
            })?;
            Ok((first, inner, input.u8().expect("last outer scalar")))
        })
        .expect("record");
    assert_eq!(value, (259, 517, 7));
    let captures = ctx.take_record_captures().expect("closed scopes");
    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0].id(), "inner");
    assert_eq!((captures[0].offset(), captures[0].byte_len()), (4, 8));
    assert_eq!(captures[0].reads(), &ranges(vec![4..6]));
    assert_eq!(captures[1].reads(), &ranges(vec![0..2, 12..13]));
    ctx.finish_session().expect("session");
}

#[test]
fn successful_ranges_merge_and_failed_or_empty_reads_add_nothing() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"abcdefgh", &arena, &DecodePolicy::default())
        .expect("root");
    ctx.record("record", root, || {
        assert_eq!(root.read_at(4, 2), Some(&b"ef"[..]));
        assert_eq!(root.read_slice(0..2), Some(&b"ab"[..]));
        assert_eq!(root.read_at(1, 3), Some(&b"bcd"[..]));
        let mut input = root;
        input.seek(8).expect("end");
        assert_eq!(input.take(1), None);
        assert_eq!(input.position(), 8);
        assert_eq!(input.take(0), Some(&b""[..]));
        assert_eq!(root.read_at(usize::MAX, 1), None);
        Ok(())
    })
    .expect("record");
    let captures = ctx.take_record_captures().expect("captures");
    assert_eq!(captures[0].reads(), &ranges(vec![0..6]));
}

#[test]
fn structural_navigation_and_payload_copy_do_not_consume_source() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"headVALUE", &arena, &DecodePolicy::default())
            .expect("root");
    let payload = ctx
        .record("record", root, || {
            let mut input = root;
            let payload = input
                .req_take_child(4)
                .expect("payload extent")
                .window()
                .to_vec();
            assert_eq!(input.unread(), b"VALUE");
            input.skip(2).expect("skip");
            assert_eq!(input.u8(), Some(b'L'));
            input.seek_to_end();
            assert_eq!(input.remaining(), 0);
            Ok(payload)
        })
        .expect("record");
    assert_eq!(payload, b"head");
    assert_eq!(
        ctx.take_record_captures().expect("captures")[0].reads(),
        &ranges(vec![6..7])
    );
}

#[test]
fn failed_child_transaction_preserves_successful_overlapping_parent_reads() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"abcdefgh", &arena, &DecodePolicy::default())
        .expect("root");
    ctx.record("outer", root, || {
        assert_eq!(root.read_at(0, 2), Some(&b"ab"[..]));
        let child = ctx.read_transaction()?;
        assert_eq!(root.read_at(1, 4), Some(&b"bcde"[..]));
        ctx.record("discarded", root.child(5, 8).expect("child"), || {
            assert_eq!(root.read_at(5, 2), Some(&b"fg"[..]));
            Ok(())
        })?;
        drop(child);
        assert_eq!(root.read_at(1, 1), Some(&b"b"[..]));
        Ok(())
    })
    .expect("outer");
    let captures = ctx.take_record_captures().expect("captures");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "outer");
    assert_eq!(captures[0].reads(), &ranges(vec![0..2]));
}

#[test]
fn committed_child_is_discarded_with_failed_ancestor_without_refunding_budget() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::default()).expect("root");
    let before = ctx.retained_bytes_available();
    let outer = ctx.read_transaction().expect("transaction");
    ctx.record("child", root, || {
        assert_eq!(root.read_at(1, 1), Some(&b"b"[..]));
        Ok(())
    })
    .expect("child commits locally");
    let after = ctx.retained_bytes_available();
    assert!(after < before);
    assert!(ctx.take_record_captures().is_err());
    drop(outer);
    assert_eq!(ctx.retained_bytes_available(), after);
    assert!(ctx.take_record_captures().expect("captures").is_empty());
}

#[test]
fn a_rolled_back_record_scope_retains_no_text_and_a_kept_record_pays_for_its_own() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::default()).expect("root");
    ctx.record("warm", root, || Ok(()))
        .expect("session activation");
    let before = ctx.retained_bytes_available();
    let refused: Result<(), CodecError> = ctx.record("attempt", root, || {
        assert_eq!(root.read_at(1, 1), Some(&b"b"[..]));
        Err(CodecError::malformed("frame matches no grammar"))
    });
    assert!(refused.is_err());
    assert_eq!(ctx.retained_bytes_available(), before);
    ctx.record("a", root, || Ok(())).expect("short id");
    let short = before - ctx.retained_bytes_available();
    let middle = ctx.retained_bytes_available();
    ctx.record("abcd", root, || Ok(())).expect("long id");
    let long = middle - ctx.retained_bytes_available();
    // Each kept capture pays once for its id and stream text. The two ids
    // differ by three bytes; their stream and storage charges are equal.
    assert_eq!(long - short, 3);
    let captures = ctx.take_record_captures().expect("captures");
    let mut ids: Vec<_> = captures.iter().map(super::RecordCapture::id).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["a", "abcd", "warm"]);
}

#[test]
fn successful_transaction_preserves_reads_made_for_an_open_outer_record() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"abcdefgh", &arena, &DecodePolicy::default())
        .expect("root");
    ctx.record("outer", root, || {
        let transaction = ctx.read_transaction()?;
        assert_eq!(root.read_at(2, 3), Some(&b"cde"[..]));
        transaction.commit()?;
        assert_eq!(root.read_at(0, 2), Some(&b"ab"[..]));
        Ok(())
    })
    .expect("outer");
    assert_eq!(
        ctx.take_record_captures().expect("captures")[0].reads(),
        &ranges(vec![0..5])
    );
}

#[test]
fn failed_record_and_unwind_restore_the_enclosing_record_scope() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"abcdefgh", &arena, &DecodePolicy::default())
        .expect("root");
    ctx.record("outer", root, || {
        assert_eq!(root.read_at(0, 1), Some(&b"a"[..]));
        let failed: Result<(), CodecError> = ctx.record("inner", root, || {
            assert_eq!(root.read_at(1, 1), Some(&b"b"[..]));
            Err(CodecError::malformed("synthetic invalid child"))
        });
        assert!(failed.is_err());
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: Result<(), CodecError> = ctx.record("unwound", root, || {
                assert_eq!(root.read_at(2, 1), Some(&b"c"[..]));
                panic!("synthetic unwinding child")
            });
        }));
        assert!(unwound.is_err());
        // A rolled-back identity can be decoded again, without retaining its
        // failed reads, and its successful reads still belong only to it.
        ctx.record("inner", root.child(4, 5).expect("retry extent"), || {
            assert_eq!(root.read_at(4, 1), Some(&b"e"[..]));
            Ok(())
        })?;
        assert_eq!(root.read_at(7, 1), Some(&b"h"[..]));
        Ok(())
    })
    .expect("outer");
    let captures = ctx.take_record_captures().expect("captures");
    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0].id(), "inner");
    assert_eq!(captures[0].reads(), &ranges(vec![4..5]));
    assert_eq!(captures[1].reads(), &ranges(vec![0..1, 7..8]));
    ctx.finish_session().expect("no stale scopes");
}

#[test]
fn copied_views_and_moved_context_keep_the_same_read_session() {
    fn context<'a>(arena: &'a DecodeArena) -> (DecodeContext<'a>, View<'a>) {
        DecodeContext::from_root_bytes(b"abc", arena, &DecodePolicy::default()).expect("root")
    }
    let arena = DecodeArena::new();
    let (ctx, root) = context(&arena);
    let ctx = std::hint::black_box(ctx);
    let copy = root;
    ctx.record("record", copy, || {
        assert_eq!(copy.read_at(0, 2), Some(&b"ab"[..]));
        assert_eq!(root.read_at(1, 2), Some(&b"bc"[..]));
        Ok(())
    })
    .expect("record");
    assert_eq!(
        ctx.take_record_captures().expect("captures")[0].reads(),
        &ranges(vec![0..3])
    );
}

#[test]
fn two_contexts_on_one_arena_never_share_records_or_source_coordinates() {
    let arena = DecodeArena::new();
    let (first, a) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::default()).expect("first");
    let (second, b) =
        DecodeContext::from_root_bytes(b"xyz", &arena, &DecodePolicy::default()).expect("second");
    assert!(first.record("wrong", b, || Ok(())).is_err());
    assert!(first.name_stream(b, "other").is_err());
    first
        .record("same-id", a, || {
            assert_eq!(a.read_at(0, 1), Some(&b"a"[..]));
            assert_eq!(b.read_at(2, 1), Some(&b"z"[..])); // no active second record
            Ok(())
        })
        .expect("first record");
    second
        .record("same-id", b, || {
            assert_eq!(b.read_at(1, 1), Some(&b"y"[..]));
            Ok(())
        })
        .expect("second record");
    assert_eq!(
        first.take_record_captures().expect("first captures")[0].reads(),
        &ranges(vec![0..1])
    );
    assert_eq!(
        second.take_record_captures().expect("second captures")[0].reads(),
        &ranges(vec![1..2])
    );
}

#[test]
fn detached_reads_and_out_of_scope_reads_cannot_claim_root_coverage() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::default()).expect("root");
    assert_eq!(root.read_at(0, 1), Some(&b"a"[..]));
    let detached = View::over_retained(b"abc");
    assert!(ctx.record("detached", detached, || Ok(())).is_err());
    ctx.record("empty", root, || {
        let mut input = detached;
        assert_eq!(input.u8(), Some(b'a'));
        Ok(())
    })
    .expect("empty capture");
    let captures = ctx.take_record_captures().expect("captures");
    assert!(captures[0].reads().is_empty());
    assert_eq!((captures[0].offset(), captures[0].byte_len()), (0, 3));
}

#[test]
fn stable_stream_name_and_capture_identity_are_owned_without_mutable_read_maps() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::default()).expect("root");
    assert!(ctx.name_stream(root, "").is_err());
    let root = ctx
        .name_stream(root, "input.bin")
        .expect("name actual root");
    ctx.record("record", root.child(1, 3).expect("extent"), || {
        assert_eq!(root.read_at(1, 2), Some(&b"bc"[..]));
        Ok(())
    })
    .expect("record");
    assert!(ctx.name_stream(root, "renamed").is_err());
    assert!(ctx.record("record", root, || Ok(())).is_err());
    let captures = ctx.take_record_captures().expect("captures");
    let (id, stream, offset, byte_len, reads) = captures
        .into_iter()
        .next()
        .expect("one capture")
        .into_parts();
    assert_eq!(
        (id.as_str(), stream.as_str(), offset, byte_len),
        ("record", "input.bin", 1, 2)
    );
    assert_eq!(
        reads,
        BTreeMap::from([("input.bin".to_owned(), vec![1..3])])
    );
    assert!(ctx.record("later", root, || Ok(())).is_err());
}

#[test]
fn repeated_reads_do_not_allocate_an_unbounded_event_log() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::default()).expect("root");
    ctx.record("record", root, || {
        assert_eq!(root.read_at(1, 1), Some(&b"b"[..]));
        let available = ctx.retained_bytes_available();
        for _ in 0..1000 {
            assert_eq!(root.read_at(1, 1), Some(&b"b"[..]));
        }
        assert_eq!(ctx.retained_bytes_available(), available);
        Ok(())
    })
    .expect("record");
    assert_eq!(
        ctx.take_record_captures().expect("captures")[0].reads(),
        &ranges(vec![1..2])
    );
}

#[test]
fn tracking_and_native_storage_share_one_fused_retained_budget() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(b"abc", &arena, &policy)
        .expect("root storage is input, not retained");
    let mut ran = false;
    let error = ctx
        .record("record", root, || {
            ran = true;
            Ok(())
        })
        .expect_err("no read metadata capacity");
    assert!(!ran);
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
    );
    assert!(ctx.retain_bytes(1, "native field").is_err());
    assert!(ctx.finish_session().is_err());
}

#[test]
fn an_exhausted_budget_refuses_the_record_when_it_keeps_its_reads() {
    let arena = DecodeArena::new();
    let (ctx, mut root) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::default()).expect("root");
    let extent = root;
    let error = ctx
        .record("record", extent, || {
            ctx.retain_bytes(ctx.retained_bytes_available(), "synthetic retained output")?;
            // An open scope's read deltas are working storage.
            assert_eq!(root.take(1), Some(&b"a"[..]));
            assert_eq!(root.position(), 1);
            Ok(())
        })
        .expect_err("tracking must not bypass the native retained-byte charge");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
    );
    assert!(ctx.finish_session().is_err());
}

#[test]
fn unfinished_and_out_of_order_transactions_cannot_yield_successful_captures() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::default()).expect("root");
    let first = ctx.read_transaction().expect("outer");
    let second = ctx.read_transaction().expect("inner");
    assert!(first.commit().is_err());
    drop(second);
    assert!(ctx.take_record_captures().is_err());
    assert!(ctx.finish_session().is_err());

    let (ctx, _) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::default()).expect("root");
    let active = ctx.read_transaction().expect("unfinished");
    assert!(ctx.take_record_captures().is_err());
    assert!(ctx.finish_session().is_err());
    drop(active);
}

#[test]
fn a_session_fuse_takes_precedence_over_foreign_or_detached_record_errors() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).expect("root");
    let limit = match ctx.retain_bytes(1, "synthetic retained field") {
        Err(CodecError::ResourceLimit(limit)) => limit,
        other => panic!("expected the original retained-byte refusal, got {other:?}"),
    };
    let detached = View::over_retained(b"abc");
    assert!(matches!(ctx.record("record", detached, || Ok(())),
        Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert!(matches!(ctx.name_stream(detached, "other"),
        Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert!(matches!(ctx.take_record_captures(),
        Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn ended_context_slot_is_reused_without_stale_view_reads_claiming_new_coverage() {
    let arena = DecodeArena::new();
    let (old, stale) =
        DecodeContext::from_root_bytes(b"OLD", &arena, &DecodePolicy::default()).expect("old root");
    let original_handle = stale.session().expect("live root");
    drop(old);
    assert_eq!(stale.read_at(0, 3), Some(&b"OLD"[..]));
    let (new, root) =
        DecodeContext::from_root_bytes(b"new", &arena, &DecodePolicy::default()).expect("new root");
    let current_handle = root.session().expect("live root");
    assert_eq!(original_handle.index, current_handle.index);
    assert_ne!(original_handle.generation, current_handle.generation);
    assert!(new.record("stale", stale, || Ok(())).is_err());
    assert!(new.name_stream(stale, "stale").is_err());
    new.record("current", root, || {
        // The original View remains a valid byte view, but cannot alias the
        // reused slot's current record merely because both are ROOT space.
        assert_eq!(stale.read_at(0, 3), Some(&b"OLD"[..]));
        assert_eq!(root.read_at(1, 1), Some(&b"e"[..]));
        Ok(())
    })
    .expect("new record");
    let captures = new.take_record_captures().expect("captures");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].reads(), &ranges(vec![1..2]));
}

#[test]
fn sequential_unused_contexts_release_session_state_and_preserve_budget_semantics() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 17;
    for _ in 0..1000 {
        let (ctx, root) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).expect("root");
        assert_eq!(ctx.retained_bytes_available(), 17);
        assert_eq!(root.read_at(0, 3), Some(&b"abc"[..]));
        ctx.retain_bytes(17, "actual retained output")
            .expect("unchanged allowance without tracking");
        ctx.finish_session().expect("session");
    }
    let sessions = arena.tracking.sessions.borrow();
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].state.is_none());
}

#[test]
fn transaction_lease_outlives_context_without_releasing_a_new_sessions_depth() {
    let arena = DecodeArena::new();
    let (old, _) =
        DecodeContext::from_root_bytes(b"old", &arena, &DecodePolicy::default()).expect("old root");
    let old_index = old.budget.index;
    let lease = old.read_transaction().expect("old lease");
    drop(old);
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 1;
    let (new, root) = DecodeContext::from_root_bytes(b"new", &arena, &policy).expect("new root");
    assert_ne!(old_index, new.budget.index);
    let active = new.read_transaction().expect("one new nesting level");
    drop(lease);
    assert!(matches!(new.read_transaction(),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RecursionDepth));
    drop(active);
    assert_eq!(root.read_at(0, 3), None); // current live session is fused
    assert!(new.finish_session().is_err());
    assert_eq!(root.read_at(0, 3), Some(&b"new"[..])); // byte view remains valid after context ends
}

#[test]
fn tracking_charges_the_shared_account_body_once_without_recharging_encode_clones() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::new(&arena, &policy, crate::decode::DecodeDepth::Full);
    let before = ctx.retained_bytes_available();
    let first = crate::encode::EncodeSession::from_context(&ctx).expect("first shared account");
    let second = crate::encode::EncodeSession::from_context(&ctx).expect("same shared account");
    assert_eq!(ctx.retained_bytes_available(), before);
    ctx.budget
        .session(|session| session.activate())
        .expect("first actual tracking activation");
    let storage = crate::decode::u64_from_index(
        size_of::<super::SessionSlot>() + size_of::<crate::operation_budget::OperationBudget>(),
    );
    assert_eq!(ctx.retained_bytes_available(), before - storage);
    let third = crate::encode::EncodeSession::from_context(&ctx).expect("clone after activation");
    ctx.budget
        .session(|session| session.activate())
        .expect("existing tracking activation");
    assert_eq!(ctx.retained_bytes_available(), before - storage);
    first.finish().expect("first handle");
    second.finish().expect("second handle");
    third.finish().expect("third handle");
    ctx.finish_session()
        .expect("one actual account body charge");
}

#[test]
fn independent_context_tracking_charges_two_slots_and_one_account_body() {
    for child_first in [false, true] {
        let parent_arena = DecodeArena::new();
        let child_arena = DecodeArena::new();
        let parent = DecodeContext::new(
            &parent_arena,
            &DecodePolicy::service(),
            crate::decode::DecodeDepth::Full,
        );
        let (child, _) = DecodeContext::from_root_bytes_shared(&[], &child_arena, &parent).unwrap();
        let account = parent.operation_budget();
        let before = account.retained_used();
        let contexts = if child_first {
            [&child, &parent]
        } else {
            [&parent, &child]
        };
        for context in contexts {
            context
                .budget
                .session(|session| session.activate())
                .unwrap();
        }
        let actual_storage = crate::decode::u64_from_index(
            2 * size_of::<super::SessionSlot>()
                + size_of::<crate::operation_budget::OperationBudget>(),
        );
        assert_eq!(account.retained_used(), before + actual_storage);
        for context in contexts {
            context
                .budget
                .session(|session| session.activate())
                .unwrap();
        }
        assert_eq!(account.retained_used(), before + actual_storage);
    }
}
