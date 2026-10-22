// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::SourceFidelity;

use super::{install, PendingLayouts};

const OWNER: &str = "f3d:native:scope#one";
const FIELD: &str = "assembly_alignment.operand_qualifiers.0.path.byte_offset";

#[test]
fn genuine_owner_capture_and_out_of_extent_reads_install_without_changing_source_pointers() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789ABCDEF", &arena, &DecodePolicy::default())
            .expect("root");
    let bulk = ctx
        .register_named_slice(root, ByteRange { start: 4, end: 16 }, "Design/BulkStream")
        .expect("actual bulk member");
    let (_, batch) = ctx
        .record_batch(|| {
            ctx.record(
                OWNER,
                bulk.child(1, 5).expect("actual owner extent"),
                || {
                    assert_eq!(bulk.read_at(1, 1), Some(b"5".as_slice()));
                    assert_eq!(bulk.read_at(8, 2), Some(b"CD".as_slice()));
                    Ok(())
                },
            )
        })
        .expect("actual record");
    let pending = PendingLayouts::new(
        batch,
        BTreeMap::from([(OWNER.to_owned(), BTreeMap::from([(FIELD.to_owned(), 8)]))]),
    );
    let mut fidelity = SourceFidelity::default();
    install(&ctx, &mut fidelity, vec![pending]).expect("final capture handoff");
    let layout = &fidelity.record_layout()[OWNER];
    assert_eq!(layout.stream(), "Design/BulkStream");
    assert_eq!((layout.offset(), layout.byte_len()), (1, 4));
    assert_eq!(
        layout.reads(),
        &BTreeMap::from([
            ("Design/BulkStream".to_owned(), vec![1..2, 8..10]),
            ("file".to_owned(), vec![5..6, 12..14]),
        ])
    );
    assert_eq!(layout.pointers(), &BTreeMap::from([(FIELD.to_owned(), 8)]));
    assert!(layout.payloads().is_empty());
    assert!(ctx.take_record_captures().is_err());
}

#[test]
fn unmatched_pointer_drafts_or_captures_refuse_without_partially_updating_fidelity() {
    for draft_owner in [OWNER, "f3d:native:scope#missing"] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::default()).expect("root");
        let (_, batch) = ctx
            .record_batch(|| ctx.record(OWNER, root, || Ok(())))
            .expect("record");
        let pending = PendingLayouts::new(
            batch,
            if draft_owner == OWNER {
                BTreeMap::new()
            } else {
                BTreeMap::from([(draft_owner.to_owned(), BTreeMap::new())])
            },
        );
        let mut fidelity = SourceFidelity::default();
        fidelity
            .metadata
            .insert("existing".into(), "unchanged".into());
        let before = fidelity.clone();
        assert!(install(&ctx, &mut fidelity, vec![pending]).is_err());
        assert_eq!(fidelity, before);
    }
}

#[test]
fn a_locally_successful_owner_cannot_install_after_its_ancestor_rolls_back() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::default()).expect("root");
    let parent = ctx.read_transaction().expect("parent");
    let (_, batch) = ctx
        .record_batch(|| {
            ctx.record(OWNER, root, || {
                assert_eq!(root.read_at(0, 1), Some(b"A".as_slice()));
                Ok(())
            })
        })
        .expect("local record");
    let pending = PendingLayouts::new(batch, BTreeMap::from([(OWNER.to_owned(), BTreeMap::new())]));
    drop(parent);
    let mut fidelity = SourceFidelity::default();
    let error =
        install(&ctx, &mut fidelity, vec![pending]).expect_err("no committed owner survives");
    assert!(error.to_string().contains("no committed owner capture"));
    assert!(fidelity.record_layout().is_empty());
}
