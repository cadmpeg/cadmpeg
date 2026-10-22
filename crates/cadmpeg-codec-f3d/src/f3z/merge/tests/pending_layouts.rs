// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::SourceFidelity;

use super::super::{occurrence_stream_prefix, rescope};
use crate::decode::source_layout::{install, PendingLayouts};

const OWNER: &str = "f3d:native:scope#one";
const FIELD: &str = "assembly_alignment.operand_qualifiers.0.path.byte_offset";

#[test]
fn nested_member_batches_follow_actual_native_and_stream_rescoping_at_final_handoff() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789ABCDEF", &arena, &DecodePolicy::default())
            .expect("root");
    let first = ctx
        .register_named_slice(root, ByteRange { start: 0, end: 8 }, "first/BulkStream")
        .expect("first real member");
    let second = ctx
        .register_named_slice(root, ByteRange { start: 8, end: 16 }, "second/BulkStream")
        .expect("second real member");
    let ancestor = ctx.read_transaction().expect("outer archive transaction");
    let (_, first_batch) = ctx
        .record_batch(|| {
            ctx.record(OWNER, first, || {
                assert_eq!(first.read_at(1, 2), Some(b"12".as_slice()));
                Ok(())
            })
        })
        .expect("first member");
    let (_, second_batch) = ctx
        .record_batch(|| {
            ctx.record(OWNER, second, || {
                assert_eq!(second.read_at(2, 2), Some(b"AB".as_slice()));
                Ok(())
            })
        })
        .expect("second member reuses its genuine local ID");
    let draft = || BTreeMap::from([(OWNER.to_owned(), BTreeMap::from([(FIELD.to_owned(), 4)]))]);
    let mut first_pending = PendingLayouts::new(first_batch, draft());
    let mut second_pending = PendingLayouts::new(second_batch, draft());
    let part_ids = Default::default();
    let inner = "inner/occurrence-0";
    let outer = "outer/occurrence-0";
    for pending in [&mut first_pending, &mut second_pending] {
        pending
            .rescope(
                &ctx,
                occurrence_stream_prefix(inner)
                    .expect("actual inner prefix")
                    .as_str(),
                |id| rescope(id, inner, &part_ids),
            )
            .expect("stage actual inner mapping");
    }
    first_pending
        .rescope(
            &ctx,
            occurrence_stream_prefix(outer)
                .expect("actual outer prefix")
                .as_str(),
            |id| rescope(id, outer, &part_ids),
        )
        .expect("nested outer mapping");
    assert!(ctx.take_record_captures().is_err());
    ancestor.commit().expect("all ancestors committed");
    let mut fidelity = SourceFidelity::default();
    install(&ctx, &mut fidelity, vec![first_pending, second_pending])
        .expect("final occurrence installation");
    let first_id = "f3d:xref/outer/occurrence-0/xref/inner/occurrence-0/native:scope#one";
    let second_id = "f3d:xref/inner/occurrence-0/native:scope#one";
    assert_eq!(fidelity.record_layout().len(), 2);
    let first = &fidelity.record_layout()[first_id];
    assert_eq!(
        first.stream(),
        "f3d:xref/outer%2Foccurrence-0/f3d:xref/inner%2Foccurrence-0/first/BulkStream"
    );
    assert_eq!(
        first.reads(),
        &BTreeMap::from([
            (
                "f3d:xref/outer%2Foccurrence-0/f3d:xref/inner%2Foccurrence-0/first/BulkStream"
                    .to_owned(),
                vec![1..3]
            ),
            (
                "f3d:xref/outer%2Foccurrence-0/f3d:xref/inner%2Foccurrence-0/file".to_owned(),
                vec![1..3]
            ),
        ])
    );
    let second = &fidelity.record_layout()[second_id];
    assert_eq!(
        second.stream(),
        "f3d:xref/inner%2Foccurrence-0/second/BulkStream"
    );
    assert_eq!(
        second.reads(),
        &BTreeMap::from([
            (
                "f3d:xref/inner%2Foccurrence-0/second/BulkStream".to_owned(),
                vec![2..4]
            ),
            (
                "f3d:xref/inner%2Foccurrence-0/file".to_owned(),
                vec![10..12]
            ),
        ])
    );
    for layout in [first, second] {
        assert_eq!(layout.pointers(), &BTreeMap::from([(FIELD.to_owned(), 4)]));
        assert_eq!((layout.offset(), layout.byte_len()), (0, 8));
    }
}
