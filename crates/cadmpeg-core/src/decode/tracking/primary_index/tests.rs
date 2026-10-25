// SPDX-License-Identifier: Apache-2.0
//! The authority index follows genuine transaction, retirement, and batch lifetimes.
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
use crate::CodecError;

fn count(ctx: &DecodeContext<'_>) -> usize {
    ctx.budget
        .session(|session| Ok(session.primaries.by_token.len()))
        .expect("inspect actual primary authority index")
}

#[test]
fn rollback_removes_new_descendants_but_preserves_the_existing_continued_primary() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"ABC", &arena, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    let (_, owner) = ctx
        .record_owner(
            "base",
            root.child(0, 1)
                .expect("select bounded original source extent"),
            || {
                let mut source = root;
                Ok(source.req_u8()?)
            },
        )
        .expect("declare actual primary source owner");
    assert_eq!(count(&ctx), 1);
    let transaction = ctx
        .read_transaction()
        .expect("open isolated source-read transaction");
    let stale = ctx
        .record_fragment(
            &owner,
            root.child(1, 2)
                .expect("select bounded original source extent"),
            || {
                ctx.record(
                    "child",
                    root.child(1, 2)
                        .expect("select bounded original source extent"),
                    || {
                        let mut source = root
                            .child(1, 2)
                            .expect("select bounded original source extent");
                        let value = source.req_u8()?;
                        ctx.record(
                            "grandchild",
                            root.child(2, 3)
                                .expect("select bounded original source extent"),
                            || {
                                let mut source = root
                                    .child(2, 3)
                                    .expect("select bounded original source extent");
                                Ok(source.req_u8()?)
                            },
                        )?;
                        Ok(value)
                    },
                )?;
                Ok(ctx
                    .record_capture_authority("child")?
                    .expect("find expected live capture authority"))
            },
        )
        .expect("commit independent source continuation");
    assert_eq!(count(&ctx), 3);
    drop(transaction);
    assert_eq!(count(&ctx), 1);
    assert!(ctx
        .record_capture_authority("child")
        .expect("query live primary capture authority")
        .is_none());
    assert!(ctx
        .record_capture_authority("grandchild")
        .expect("query live primary capture authority")
        .is_none());
    ctx.record_fragment(
        &owner,
        root.child(1, 2)
            .expect("select bounded original source extent"),
        || {
            let mut source = root
                .child(1, 2)
                .expect("select bounded original source extent");
            Ok(source.req_u8()?)
        },
    )
    .expect("commit independent source continuation");
    assert_eq!(count(&ctx), 1);
    ctx.record(
        "child",
        root.child(2, 3)
            .expect("select bounded original source extent"),
        || {
            let mut source = root
                .child(2, 3)
                .expect("select bounded original source extent");
            Ok(source.req_u8()?)
        },
    )
    .expect("commit successful fixture producer");
    assert!(ctx.retire_record_capture(&stale).is_err());
    let captures = ctx
        .take_record_captures()
        .expect("extract committed source record captures");
    assert_eq!(captures.len(), 2);
    assert!(captures.iter().any(|capture| capture.id() == "base"
        && capture.reads()["file"]
            == std::iter::once(0..2).collect::<Vec<std::ops::Range<u64>>>()));
    assert!(captures.iter().any(|capture| capture.id() == "child"
        && capture.reads()["file"]
            == std::iter::once(2..3).collect::<Vec<std::ops::Range<u64>>>()));
}

#[test]
fn staged_retirement_commit_and_rollback_update_the_same_index_without_replacement_authority() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    let (_, owner) = ctx
        .record_owner("same", root, || {
            let mut source = root;
            Ok(source.req_u8()?)
        })
        .expect("declare actual primary source owner");
    let proof = ctx
        .record_capture_authority("same")
        .expect("query live primary capture authority")
        .expect("find expected primary source authority");
    let outer = ctx
        .read_transaction()
        .expect("open isolated source-read transaction");
    let inner = ctx
        .read_transaction()
        .expect("open isolated source-read transaction");
    ctx.retire_record_capture(&proof)
        .expect("retire authenticated source capture");
    ctx.retire_record_capture(&proof)
        .expect("retire authenticated source capture");
    inner.commit().expect("commit source-read transaction");
    assert!(ctx.record_capture_authority("same").is_err());
    let mut ran = false;
    assert!(ctx
        .record_fragment(&owner, root, || {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(!ran);
    drop(outer);
    assert!(ctx
        .record_capture_authority("same")
        .expect("query live primary capture authority")
        .is_some());
    ctx.record_fragment(
        &owner,
        root.child(1, 2)
            .expect("select bounded original source extent"),
        || {
            let mut source = root
                .child(1, 2)
                .expect("select bounded original source extent");
            Ok(source.req_u8()?)
        },
    )
    .expect("commit independent source continuation");
    let retire = ctx
        .read_transaction()
        .expect("open isolated source-read transaction");
    ctx.retire_record_capture(&proof)
        .expect("retire authenticated source capture");
    retire.commit().expect("commit source-read transaction");
    assert_eq!(count(&ctx), 0);
    ctx.record("same", root, || {
        let mut source = root;
        Ok(source.req_u8()?)
    })
    .expect("commit successful fixture producer");
    assert!(ctx.retire_record_capture(&proof).is_err());
    assert_eq!(
        ctx.take_record_captures()
            .expect("extract committed source record captures")
            .len(),
        1
    );
}

#[test]
fn active_primary_foreign_session_other_batches_and_seal_preserve_authority_guards() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"A", &arena, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    let (_, owner) = ctx
        .record_owner("same", root, || {
            let proof = ctx
                .record_capture_authority("same")?
                .expect("find expected live capture authority");
            assert!(ctx.retire_record_capture(&proof).is_err());
            let mut source = root;
            Ok(source.req_u8()?)
        })
        .expect("declare actual primary source owner");
    let proof = ctx
        .record_capture_authority("same")
        .expect("query live primary capture authority")
        .expect("find expected primary source authority");
    let arena2 = DecodeArena::new();
    let (other, _) = DecodeContext::from_root_bytes(b"B", &arena2, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    assert!(other.retire_record_capture(&proof).is_err());
    let (batched, _) = ctx
        .record_batch(|| {
            assert!(ctx.record_capture_authority("same")?.is_none());
            assert!(ctx.record_fragment(&owner, root, || Ok(())).is_err());
            ctx.record("same", root, || {
                let mut source = root;
                Ok(source.req_u8()?)
            })?;
            assert!(ctx.retire_record_capture(&proof).is_err());
            Ok(ctx
                .record_capture_authority("same")?
                .expect("find expected live capture authority"))
        })
        .expect("complete isolated source-record batch");
    assert_eq!(count(&ctx), 2);
    assert!(ctx.retire_record_capture(&batched).is_err());
    assert!(ctx.record("same", root, || Ok(())).is_err());
    assert_eq!(
        ctx.take_record_captures()
            .expect("extract committed source record captures")
            .len(),
        2
    );
    assert_eq!(count(&ctx), 0);
    assert!(ctx.record_capture_authority("same").is_err());
}

#[test]
fn many_primary_insertions_and_continuations_keep_one_index_entry_per_actual_primary() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"A", &arena, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    for ordinal in 0..4096 {
        let (_, owner) = ctx
            .record_owner(&format!("row-{ordinal}"), root, || {
                let mut source = root;
                Ok(source.req_u8()?)
            })
            .expect("declare actual primary source owner");
        ctx.record_fragment(&owner, root, || {
            let mut source = root;
            Ok(source.req_u8()?)
        })
        .expect("commit independent source continuation");
        assert!(ctx
            .record_capture_authority(&format!("row-{ordinal}"))
            .expect("query live primary capture authority")
            .is_some());
    }
    assert_eq!(count(&ctx), 4096);
    let captures = ctx
        .take_record_captures()
        .expect("extract committed source record captures");
    assert_eq!(captures.len(), 4096);
    assert!(captures.iter().all(|capture| capture.reads()["file"]
        == std::iter::once(0..1).collect::<Vec<std::ops::Range<u64>>>()));
}

#[test]
fn a_failed_primary_scope_is_not_a_live_index_declaration() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"A", &arena, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    let result: Result<(), CodecError> = ctx.record("failed", root, || {
        let mut source = root;
        source.req_u8()?;
        Err(CodecError::malformed("failed source producer"))
    });
    assert!(result.is_err());
    assert_eq!(count(&ctx), 0);
    assert!(ctx
        .record_capture_authority("failed")
        .expect("query live primary capture authority")
        .is_none());
}
