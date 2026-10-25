// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::decode::{DecodeArena, DecodePolicy};

#[test]
fn retirement_removes_every_fragment_and_keeps_an_independent_producer() {
    for transactional in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::default())
            .expect("create tracked fixture source context");
        let ((), owner) = ctx
            .record_owner(
                "lost",
                root.child(0, 1)
                    .expect("select bounded original source extent"),
                || {
                    assert_eq!(root.read_slice(0..1), Some(&b"a"[..]));
                    Ok(())
                },
            )
            .expect("declare actual primary source owner");
        ctx.record_fragment(
            &owner,
            root.child(1, 2)
                .expect("select bounded original source extent"),
            || {
                assert_eq!(root.read_slice(1..2), Some(&b"b"[..]));
                Ok(())
            },
        )
        .expect("commit independent source continuation");
        ctx.record(
            "peer",
            root.child(2, 3)
                .expect("select bounded original source extent"),
            || {
                assert_eq!(root.read_slice(2..3), Some(&b"c"[..]));
                Ok(())
            },
        )
        .expect("commit successful fixture producer");
        if transactional {
            let transaction = ctx
                .read_transaction()
                .expect("open isolated source-read transaction");
            ctx.retire_record_owner(&owner)
                .expect("retire authenticated primary owner");
            transaction
                .commit()
                .expect("commit source-read transaction");
        } else {
            ctx.retire_record_owner(&owner)
                .expect("retire authenticated primary owner");
        }
        assert!(ctx.record_fragment(&owner, root, || Ok(())).is_err());
        let captures = ctx
            .take_record_captures()
            .expect("extract committed source record captures");
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].id(), "peer");
        assert_eq!(
            captures[0].reads()["file"],
            std::iter::once(2..3).collect::<Vec<std::ops::Range<u64>>>()
        );
    }
}

#[test]
fn retirement_rolls_back_and_refuses_active_foreign_or_wrong_batch_authority() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"ab", &arena, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    let ((), owner) = ctx
        .record_owner("row", root, || {
            root.read_slice(0..1)
                .expect("read bounded fixture source bytes");
            Ok(())
        })
        .expect("declare actual primary source owner");
    let proof = ctx
        .record_capture_authority("row")
        .expect("query live primary capture authority")
        .expect("find expected primary source authority");
    let transaction = ctx
        .read_transaction()
        .expect("open isolated source-read transaction");
    ctx.retire_record_capture(&proof)
        .expect("retire authenticated source capture");
    assert!(ctx.record_capture_authority("row").is_err());
    assert!(ctx.record_fragment(&owner, root, || Ok(())).is_err());
    drop(transaction);
    ctx.record_fragment(&owner, root, || {
        root.read_slice(1..2)
            .expect("read bounded fixture source bytes");
        Ok(())
    })
    .expect("commit independent source continuation");
    ctx.record("active", root, || {
        let proof = ctx
            .record_capture_authority("active")?
            .expect("find expected live capture authority");
        assert!(ctx.retire_record_capture(&proof).is_err());
        Ok(())
    })
    .expect("commit successful fixture producer");
    let arena2 = DecodeArena::new();
    let (other, _) = DecodeContext::from_root_bytes(b"ab", &arena2, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    assert!(other.retire_record_capture(&proof).is_err());
    ctx.record_batch(|| {
        assert!(ctx.retire_record_capture(&proof).is_err());
        Ok(())
    })
    .expect("complete isolated source-record batch");
    let captures = ctx
        .take_record_captures()
        .expect("extract committed source record captures");
    assert_eq!(
        captures
            .iter()
            .find(|capture| capture.id() == "row")
            .expect("locate expected retained producer capture")
            .reads()["file"],
        std::iter::once(0..2).collect::<Vec<std::ops::Range<u64>>>()
    );
    assert!(ctx.retire_record_capture(&proof).is_err());
}

#[test]
fn stale_generation_and_atomic_foreign_batch_cannot_retire_a_replacement() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"ab", &arena, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    let transaction = ctx
        .read_transaction()
        .expect("open isolated source-read transaction");
    ctx.record("row", root, || {
        root.read_slice(0..1)
            .expect("read bounded fixture source bytes");
        Ok(())
    })
    .expect("commit successful fixture producer");
    let stale = ctx
        .record_capture_authority("row")
        .expect("query live primary capture authority")
        .expect("find expected primary source authority");
    drop(transaction);
    ctx.record("row", root, || {
        root.read_slice(1..2)
            .expect("read bounded fixture source bytes");
        Ok(())
    })
    .expect("commit successful fixture producer");
    let replacement = ctx
        .record_capture_authority("row")
        .expect("query live primary capture authority")
        .expect("find expected primary source authority");
    assert!(ctx.retire_record_capture(&stale).is_err());
    let (batched, _) = ctx
        .record_batch(|| {
            ctx.record("batch", root, || {
                root.read_slice(0..1)
                    .expect("read bounded fixture source bytes");
                Ok(())
            })?;
            Ok(ctx
                .record_capture_authority("batch")?
                .expect("find expected live capture authority"))
        })
        .expect("complete isolated source-record batch");
    assert!(ctx
        .retire_record_captures([&replacement, &batched])
        .is_err());
    let captures = ctx
        .take_record_captures()
        .expect("extract committed source record captures");
    assert_eq!(captures.len(), 2);
    assert_eq!(
        captures
            .iter()
            .find(|capture| capture.id() == "row")
            .expect("locate expected retained producer capture")
            .reads()["file"],
        std::iter::once(1..2).collect::<Vec<std::ops::Range<u64>>>()
    );
}
