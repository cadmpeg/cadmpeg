// SPDX-License-Identifier: Apache-2.0
//! Selected source authority, rollback and exactly-once metadata accounting.

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, LimitScope, ResourceDimension};
use crate::CodecError;
use std::cell::Cell;

mod discovered;

#[test]
fn selected_failure_capture_recomputes_its_containing_owner_and_moves_actual_attempts() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::default()).unwrap();
    let calls = Cell::new(0);
    ctx.record("outer", root, || {
        ctx.record("narrow", root.child(0, 1).unwrap(), || {
            let outcome = ctx.part_transaction_with_failure_extent(root.child(0, 1).unwrap(), root, || {
                calls.set(calls.get() + 1);
                ctx.record("discarded", root.child(0, 1).unwrap(), || {
                    assert_eq!(root.read_at(0, 1), Some(&b"1"[..]));
                    Ok(())
                })?;
                Err::<(), _>(CodecError::malformed("actual row"))
            })?;
            assert_eq!(outcome.capture().record_id(), Some("outer"));
            assert_eq!(outcome.capture().byte_len(), 4);
            assert_eq!(outcome.capture().attempted_record_ids(), ["discarded"]);
            assert!(matches!(outcome.result(), Err(CodecError::Malformed(message)) if message == "actual row"));
            Ok(())
        })
    }).unwrap();
    assert_eq!(calls.get(), 1);
    let records = ctx.take_record_captures().unwrap();
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| record.reads().is_empty()));
}

#[cfg(feature = "test-support")]
#[test]
fn success_and_failure_each_charge_only_the_selected_capture_text_once() {
    for fails in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::default()).unwrap();
        ctx.record("owner", root, || {
            let (outcome, charged) = crate::decode::test_support::charged_at(
                ResourceDimension::RetainedBytes,
                "record tracking text",
                || {
                    ctx.part_transaction_with_failure_extent(
                        root.child(0, 1).unwrap(),
                        root,
                        || {
                            if fails {
                                Err(CodecError::malformed("row"))
                            } else {
                                Ok(17)
                            }
                        },
                    )
                },
            );
            let outcome = outcome?;
            assert_eq!(
                charged,
                u64::try_from("file".len() + "owner".len()).unwrap()
            );
            assert_eq!(outcome.capture().byte_len(), if fails { 4 } else { 1 });
            assert_eq!(outcome.result().is_err(), fails);
            Ok(())
        })
        .unwrap();
    }
}

#[test]
fn invalid_or_foreign_failure_extents_refuse_before_the_parser_runs() {
    let arena = DecodeArena::new();
    let other_arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"1234", &arena, &DecodePolicy::default()).unwrap();
    let (_, other) =
        DecodeContext::from_root_bytes(b"1234", &other_arena, &DecodePolicy::default()).unwrap();
    let calls = Cell::new(0);
    for (row, suffix) in [
        (root.child(0, 2).unwrap(), root.child(0, 1).unwrap()),
        (root.child(1, 2).unwrap(), root),
        (root.child(0, 1).unwrap(), other),
        (other.child(0, 1).unwrap(), root),
    ] {
        assert!(ctx
            .part_transaction_with_failure_extent(row, suffix, || {
                calls.set(calls.get() + 1);
                Ok(())
            })
            .is_err());
    }
    assert_eq!(calls.get(), 0);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn a_new_part_refusal_recovers_but_a_preexisting_refusal_never_runs_the_parser() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"12", &arena, &DecodePolicy::default()).unwrap();
    let outcome = ctx
        .part_transaction_with_failure_extent(root.child(0, 1).unwrap(), root, || {
            Err::<(), _>(ctx.refuse_part_limit(ResourceDimension::Entities, 0, 0, 1, "actual part"))
        })
        .unwrap();
    assert!(
        matches!(outcome.result(), Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part)
    );
    assert_eq!(outcome.capture().byte_len(), 2);
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.refuse_part_limit(ResourceDimension::Entities, 0, 0, 1, "prior part");
    let error = ctx
        .part_transaction_with_failure_extent::<()>(root.child(0, 1).unwrap(), root, || {
            panic!("prior refusal must stop the parser")
        })
        .unwrap_err();
    assert_eq!(error.to_string(), original.to_string());
    assert!(ctx.resource_refusal().is_some());
}

#[test]
fn fatal_errors_and_unwind_roll_back_the_actual_attempt_without_successful_capture() {
    for unwind in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"12", &arena, &DecodePolicy::default()).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.part_transaction_with_failure_extent(root.child(0, 1).unwrap(), root, || {
                ctx.record("rolled-back", root.child(0, 1).unwrap(), || {
                    assert_eq!(root.read_at(0, 1), Some(&b"1"[..]));
                    if unwind {
                        panic!("actual parser unwind");
                    }
                    Err::<(), _>(CodecError::Io(std::io::Error::other("actual I/O")))
                })
            })
        }));
        if unwind {
            assert!(result.is_err());
        } else {
            assert!(matches!(result.unwrap(), Err(CodecError::Io(_))));
        }
        assert!(ctx.take_record_captures().unwrap().is_empty());
    }
}

#[test]
fn a_session_refusal_escapes_and_remains_fused_instead_of_selecting_a_recovery_capture() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(b"12", &arena, &policy).unwrap();
    let error = ctx
        .part_transaction_with_failure_extent(root.child(0, 1).unwrap(), root, || {
            ctx.charge_entities(1, "actual semantic row")
        })
        .unwrap_err();
    assert!(
        matches!(&error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session && limit.dimension == ResourceDimension::Entities && limit.additional == 1)
    );
    assert_eq!(
        ctx.take_record_captures().unwrap_err().to_string(),
        error.to_string()
    );
    assert_eq!(
        ctx.finish_session().unwrap_err().to_string(),
        error.to_string()
    );
}
