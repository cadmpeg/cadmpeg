// SPDX-License-Identifier: Apache-2.0

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ExpandSpec};

#[test]
fn readonly_source_membership_survives_capture_seal_without_record_authority() {
    let arena = DecodeArena::new();
    let foreign_arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::service())
        .expect("create tracked fixture source context");
    let (foreign, _) =
        DecodeContext::from_root_bytes(b"source", &foreign_arena, &DecodePolicy::service())
            .expect("create tracked fixture source context");
    let proof = ctx
        .source_extent_proof(
            root.child(1, 5)
                .expect("select bounded original source extent"),
        )
        .expect("bind proof to original source extent");
    assert!(foreign.require_source_membership(&proof).is_err());
    ctx.take_record_captures()
        .expect("extract committed source record captures");
    ctx.require_source_membership(&proof)
        .expect("validate retained source membership");
    assert!(ctx.require_source_authority(&proof).is_err());
    assert!(ctx.source_extent_proof(root).is_err());
}

#[test]
fn source_proofs_follow_every_compressed_origin_and_reject_other_batches() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789", &arena, &DecodePolicy::default())
            .expect("create tracked fixture source context");
    let source = root
        .child(2, 8)
        .expect("select bounded original source extent");
    let proof = ctx
        .source_extent_proof(source)
        .expect("bind proof to original source extent");
    let mut expansion = ctx
        .begin_expand(ExpandSpec::Exact(3))
        .expect("admit bounded fixture expansion");
    expansion
        .write(b"abc")
        .expect("write complete expansion fixture");
    let decoded = expansion
        .finalize_from(source, "decoded")
        .expect("register expansion with original source ancestry");
    assert!(ctx
        .require_source_ancestry(
            &proof,
            decoded
                .child(1, 2)
                .expect("select bounded original source extent")
        )
        .is_ok());
    assert!(ctx
        .require_source_ancestry(
            &proof,
            root.child(1, 3)
                .expect("select bounded original source extent")
        )
        .is_err());
    let composite = ctx
        .concat_named_views(
            &[
                decoded,
                root.child(8, 9)
                    .expect("select bounded original source extent"),
            ],
            "crossing",
        )
        .expect("register original composite source segments");
    assert!(ctx
        .require_source_ancestry(
            &proof,
            composite
                .child(0, 3)
                .expect("select bounded original source extent")
        )
        .is_ok());
    assert!(ctx.require_source_ancestry(&proof, composite).is_err());
    let ((batch_proof, ()), _) = ctx
        .record_batch(|| {
            let proof = ctx.source_extent_proof(source)?;
            ctx.require_source_ancestry(&proof, decoded)?;
            Ok((proof, ()))
        })
        .expect("complete isolated source-record batch");
    assert!(ctx.require_source_ancestry(&batch_proof, decoded).is_err());
    assert!(ctx.require_source_ancestry(&proof, decoded).is_ok());
    assert!(ctx
        .take_record_captures()
        .expect("extract committed source record captures")
        .is_empty());
    assert!(ctx.require_source_ancestry(&proof, decoded).is_err());
}

#[test]
fn source_proofs_reject_new_contexts_after_the_original_context_ends() {
    let proof = {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::default())
                .expect("create tracked fixture source context");
        ctx.source_extent_proof(root)
            .expect("bind proof to original source extent")
    };
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    assert!(ctx.require_source_ancestry(&proof, root).is_err());
}

#[test]
fn empty_registered_alias_and_expansion_keep_source_ancestry_without_read_credit() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::default())
        .expect("create tracked fixture source context");
    let empty = root
        .child(3, 3)
        .expect("select bounded original source extent");
    let proof = ctx
        .source_extent_proof(empty)
        .expect("bind proof to original source extent");
    let alias = ctx
        .register_named_slice(root, crate::decode::ByteRange { start: 3, end: 3 }, "empty")
        .expect("register empty original source alias");
    assert!(ctx.require_source_ancestry(&proof, alias).is_ok());
    let source = root
        .child(1, 5)
        .expect("select bounded original source extent");
    let proof = ctx
        .source_extent_proof(source)
        .expect("bind proof to original source extent");
    let expansion = ctx
        .begin_expand(ExpandSpec::Exact(0))
        .expect("admit bounded fixture expansion");
    let expanded = expansion
        .finalize_from(source, "empty-expanded")
        .expect("register expansion with original source ancestry");
    assert!(ctx.require_source_ancestry(&proof, expanded).is_ok());
    assert!(ctx
        .take_record_captures()
        .expect("extract committed source record captures")
        .is_empty());
}
