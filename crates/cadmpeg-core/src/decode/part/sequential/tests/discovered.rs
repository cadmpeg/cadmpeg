// SPDX-License-Identifier: Apache-2.0
//! One actual parse chooses its exact prefix; speculative tail reads never commit.

use crate::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy};
use crate::CodecError;
use std::cell::Cell;

#[test]
fn discovered_success_owns_actual_consumed_prefix_and_real_alias_ancestor_reads() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"xx1234", &arena, &DecodePolicy::service()).unwrap();
    let member = ctx
        .register_named_slice(root, ByteRange { start: 2, end: 6 }, "member")
        .unwrap();
    let prefix = member.child(0, 1).unwrap();
    let calls = Cell::new(0);
    let outcome = ctx
        .part_transaction_discovered(prefix, member, || {
            calls.set(calls.get() + 1);
            ctx.record("row", prefix, || {
                let mut cursor = member;
                let first = cursor.req_u8()?;
                let second = cursor.req_u8()?;
                Ok(((first, second), member.child(0, cursor.position()).unwrap()))
            })
        })
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(outcome.result().unwrap(), &(b'1', b'2'));
    assert_eq!(
        (
            outcome.capture().stream(),
            outcome.capture().offset(),
            outcome.capture().byte_len()
        ),
        ("member", 0, 2)
    );
    let records = ctx.take_record_captures().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].reads()["member"], [0..2]);
    assert_eq!(records[0].reads()["file"], [2..4]);
}

#[test]
fn discovered_tail_reads_in_completed_children_or_ancestor_deltas_cannot_commit() {
    for child in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::service()).unwrap();
        ctx.record("outer", root, || {
            let first = root.child(0, 1).unwrap();
            let outcome = ctx.part_transaction_discovered(first, root, || {
                let read_tail = || {
                    assert_eq!(root.read_at(3, 1), Some(&b"4"[..]));
                    Ok(())
                };
                if child {
                    ctx.record("discarded", first, read_tail)?;
                } else {
                    read_tail()?;
                }
                Ok((17, first))
            })?;
            assert!(matches!(outcome.result(), Err(CodecError::Malformed(_))));
            assert_eq!(outcome.capture().byte_len(), 4);
            if child {
                assert_eq!(outcome.capture().attempted_record_ids(), ["discarded"]);
            }
            Ok(())
        })
        .unwrap();
        let records = ctx.take_record_captures().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id(), "outer");
        assert!(records[0].reads().is_empty());
    }
}

#[test]
fn discovered_foreign_mid_start_empty_and_overwide_results_are_actual_failed_attempts() {
    for bad in 0..4 {
        let arena = DecodeArena::new();
        let other_arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::service()).unwrap();
        let (_, foreign) =
            DecodeContext::from_root_bytes(b"1234", &other_arena, &DecodePolicy::service())
                .unwrap();
        let remaining = root.child(0, 3).unwrap();
        let calls = Cell::new(0);
        let outcome = ctx
            .part_transaction_discovered(root.child(0, 1).unwrap(), remaining, || {
                calls.set(calls.get() + 1);
                ctx.record("failed", root.child(0, 1).unwrap(), || {
                    assert_eq!(root.read_at(0, 1), Some(&b"1"[..]));
                    let claimed = match bad {
                        0 => foreign.child(0, 1).unwrap(),
                        1 => root.child(1, 2).unwrap(),
                        2 => root.child(0, 0).unwrap(),
                        _ => root,
                    };
                    Ok((17, claimed))
                })
            })
            .unwrap();
        assert_eq!(calls.get(), 1);
        assert!(matches!(outcome.result(), Err(CodecError::Malformed(_))));
        assert_eq!(outcome.capture().byte_len(), 3);
        assert_eq!(outcome.capture().attempted_record_ids(), ["failed"]);
        assert!(ctx.take_record_captures().unwrap().is_empty());
    }
}

#[cfg(feature = "test-support")]
#[test]
fn discovered_success_and_failure_capture_selected_metadata_exactly_once() {
    use crate::decode::ResourceDimension;
    for fails in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::service()).unwrap();
        ctx.record("owner", root, || {
            let (outcome, charged) = crate::decode::test_support::charged_at(
                ResourceDimension::RetainedBytes,
                "record tracking text",
                || {
                    ctx.part_transaction_discovered(root.child(0, 1).unwrap(), root, || {
                        let mut cursor = root;
                        let value = cursor.req_u8()?;
                        if fails {
                            return Err(CodecError::malformed("actual row"));
                        }
                        Ok((value, root.child(0, cursor.position()).unwrap()))
                    })
                },
            );
            assert_eq!(
                charged,
                u64::try_from("file".len() + "owner".len()).unwrap()
            );
            let outcome = outcome?;
            assert_eq!(outcome.capture().byte_len(), if fails { 4 } else { 1 });
            assert_eq!(outcome.result().is_err(), fails);
            Ok(())
        })
        .unwrap();
    }
}

#[test]
fn recordless_tail_reads_fail_even_when_a_nested_semantic_attempt_rolls_back() {
    for nested in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::service()).unwrap();
        let calls = Cell::new(0);
        let prefix = root.child(0, 1).unwrap();
        let outcome = ctx
            .part_transaction_discovered(prefix, root, || {
                calls.set(calls.get() + 1);
                let read_tail = || {
                    assert_eq!(root.read_at(3, 1), Some(&b"4"[..]));
                    Err::<(), _>(CodecError::malformed("actual nested attempt"))
                };
                if nested {
                    assert!(ctx
                        .part_transaction(root.child(3, 4).unwrap(), read_tail)?
                        .result()
                        .is_err());
                } else {
                    assert!(read_tail().is_err());
                }
                Ok((17, prefix))
            })
            .unwrap();
        assert_eq!(calls.get(), 1);
        assert!(
            matches!(outcome.result(), Err(CodecError::Malformed(message)) if message == "actual parser read escapes its consumed source prefix")
        );
        assert_eq!(outcome.capture().byte_len(), 4);
        assert!(outcome.capture().attempted_record_ids().is_empty());
        assert!(ctx.take_record_captures().unwrap().is_empty());
    }
}

#[test]
fn recordless_actual_prefix_success_creates_no_semantic_owner_or_read_credit() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::service()).unwrap();
    let outcome = ctx
        .part_transaction_discovered(root.child(0, 1).unwrap(), root, || {
            let mut cursor = root;
            let value = cursor.req_u8()?;
            Ok((value, root.child(0, cursor.position()).unwrap()))
        })
        .unwrap();
    assert_eq!(outcome.result().unwrap(), &b'1');
    assert_eq!(outcome.capture().byte_len(), 1);
    assert_eq!(outcome.capture().record_id(), None);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn ancestor_reads_before_discovery_are_preserved_without_polluting_the_new_boundary() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::service()).unwrap();
    ctx.record("outer", root, || {
        assert_eq!(root.read_at(3, 1), Some(&b"4"[..]));
        let prefix = root.child(0, 1).unwrap();
        let outcome = ctx.part_transaction_discovered(prefix, root, || {
            let mut cursor = root;
            let value = cursor.req_u8()?;
            Ok((value, root.child(0, cursor.position()).unwrap()))
        })?;
        assert_eq!(outcome.result().unwrap(), &b'1');
        assert_eq!(outcome.capture().byte_len(), 1);
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "outer");
    assert_eq!(captures[0].reads()["file"], [0..1, 3..4]);
}
