// SPDX-License-Identifier: Apache-2.0
//! Sequential primary owners retain exact extents and transactional authority.
use super::*;

#[test]
fn sequential_owner_parses_once_and_has_one_exact_primary_charge() {
    let mut spent = Vec::new();
    for dynamic in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(b"HABT", &arena, &DecodePolicy::default())
            .expect("create tracked fixture source context");
        let before = ctx.retained_bytes_available();
        let calls = std::cell::Cell::new(0);
        let decode = || {
            calls.set(calls.get() + 1);
            let mut source = root
                .child(1, 4)
                .expect("select bounded original source extent");
            Ok((source.req_u16_le()?, 2))
        };
        let (value, used, owner) = if dynamic {
            ctx.sequential_record_owner(
                "joined",
                root.child(1, 4)
                    .expect("select bounded original source extent"),
                decode,
            )
            .expect("parse exactly one primary source record")
        } else {
            let ((value, used), owner) = ctx
                .record_owner(
                    "joined",
                    root.child(1, 3)
                        .expect("select bounded original source extent"),
                    decode,
                )
                .expect("declare actual primary source owner");
            (value, used, owner)
        };
        let header = ctx
            .record_fragment(
                &owner,
                root.child(0, 1)
                    .expect("select bounded original source extent"),
                || {
                    let mut source = root
                        .child(0, 1)
                        .expect("select bounded original source extent");
                    Ok(source.req_u8()?)
                },
            )
            .expect("commit independent source continuation");
        assert_eq!(
            (header, value, used, calls.get()),
            (b'H', u16::from_le_bytes(*b"AB"), 2, 1)
        );
        let captures = ctx
            .take_record_captures()
            .expect("extract committed source record captures");
        assert_eq!(captures.len(), 1);
        assert_eq!((captures[0].offset(), captures[0].byte_len()), (1, 2));
        assert_eq!(
            captures[0].reads(),
            &BTreeMap::from([(
                "file".into(),
                std::iter::once(0..3).collect::<Vec<std::ops::Range<u64>>>()
            )])
        );
        spent.push(before - ctx.retained_bytes_available());
        let mut ran = false;
        assert!(ctx
            .record_fragment(&owner, root, || {
                ran = true;
                Ok(())
            })
            .is_err());
        assert!(!ran);
    }
    assert_eq!(spent[0], spent[1]);
}

#[test]
fn sequential_owner_refuses_reads_outside_the_consumed_prefix_and_rolls_back_children() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    let before = ctx.retained_bytes_available();
    let result = ctx.sequential_record_owner("failed", root, || {
        let mut source = root;
        let value = source.req_u16_le()?;
        ctx.record(
            "child",
            root.child(0, 1)
                .expect("select bounded original source extent"),
            || {
                let mut source = root;
                Ok(source.req_u8()?)
            },
        )?;
        Ok((value, 1))
    });
    assert!(
        matches!(result, Err(CodecError::Malformed(message)) if message == "sequential record read exceeds consumed extent")
    );
    assert!(ctx.retained_bytes_available() < before);
    assert!(ctx
        .take_record_captures()
        .expect("extract committed source record captures")
        .is_empty());
}

#[test]
fn sequential_owner_rollback_retires_its_token_and_foreign_context_cannot_use_it() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"ABC", &arena, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    let transaction = ctx
        .read_transaction()
        .expect("open isolated source-read transaction");
    let (_, _, owner) = ctx
        .sequential_record_owner("rolled-back", root, || {
            let mut source = root;
            Ok((source.req_u8()?, 1))
        })
        .expect("parse exactly one primary source record");
    drop(transaction);
    let mut ran = false;
    assert!(ctx
        .record_fragment(&owner, root, || {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(!ran);
    let arena2 = DecodeArena::new();
    let (other, foreign) = DecodeContext::from_root_bytes(b"Z", &arena2, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    assert!(other
        .record_fragment(&owner, foreign, || {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(!ran);
    assert!(ctx
        .take_record_captures()
        .expect("extract committed source record captures")
        .is_empty());
    assert!(other
        .take_record_captures()
        .expect("extract committed source record captures")
        .is_empty());
}
