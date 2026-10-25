// SPDX-License-Identifier: Apache-2.0
use crate::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy, View};
use crate::CodecError;

fn warm(ctx: &DecodeContext<'_>, root: View<'_>) {
    ctx.record("actual:source-owner", root, || {
        assert_eq!(
            root.read_slice(root.start()..root.end()),
            Some(root.window())
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn warm_actual_extent_uses_no_retained_output_or_extra_record_capture() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"prefixfield", &arena, &DecodePolicy::service()).unwrap();
    warm(&ctx, root);
    let available = ctx.retained_bytes_available();
    let capture = ctx.working_part_extent(root.child(6, 11).unwrap()).unwrap();
    assert_eq!(capture.stream(), "file");
    assert_eq!(capture.offset(), 6);
    assert_eq!(capture.byte_len(), 5);
    assert_eq!(capture.record_id(), None);
    assert_eq!(ctx.retained_bytes_available(), available);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "actual:source-owner");
    assert_eq!(captures[0].reads()["file"], [0..11]);
}

#[test]
fn named_alias_uses_actual_registered_child_coordinates() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"prefixfield", &arena, &DecodePolicy::service()).unwrap();
    let alias = ctx
        .register_named_slice(root, ByteRange { start: 6, end: 11 }, "actual-field")
        .unwrap();
    warm(&ctx, root);
    let capture = ctx.working_part_extent(alias).unwrap();
    assert_eq!(capture.stream(), "actual-field");
    assert_eq!(capture.offset(), 0);
    assert_eq!(capture.byte_len(), 5);
}

#[test]
fn foreign_and_detached_views_never_gain_extent_authority() {
    let arena = DecodeArena::new();
    let other_arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"field", &arena, &DecodePolicy::service()).unwrap();
    let (_, foreign) =
        DecodeContext::from_root_bytes(b"field", &other_arena, &DecodePolicy::service()).unwrap();
    warm(&ctx, root);
    for view in [foreign, View::over_retained(b"field")] {
        assert!(ctx.working_part_extent(view).is_err());
    }
}

#[test]
fn cold_and_extracted_sessions_are_not_activated_or_reopened() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"field", &arena, &DecodePolicy::service()).unwrap();
    let available = ctx.retained_bytes_available();
    assert!(ctx.working_part_extent(root).is_err());
    assert_eq!(ctx.retained_bytes_available(), available);
    warm(&ctx, root);
    assert_eq!(ctx.take_record_captures().unwrap().len(), 1);
    assert!(ctx.working_part_extent(root).is_err());
}

#[test]
fn working_extent_preserves_the_first_fused_refusal() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"field", &arena, &DecodePolicy::service()).unwrap();
    warm(&ctx, root);
    assert!(ctx
        .reserve_working(&mut Vec::<u64>::new(), usize::MAX, "extent prior refusal")
        .is_err());
    let refusal = ctx.resource_refusal().unwrap();
    assert!(
        matches!(ctx.working_part_extent(root), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
    );
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn actual_enclosing_record_identity_also_uses_working_storage() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"prefixfield", &arena, &DecodePolicy::service()).unwrap();
    ctx.record("actual:source-owner", root, || {
        let available = ctx.retained_bytes_available();
        let capture = ctx.working_part_extent(root.child(6, 11).unwrap())?;
        assert_eq!(capture.record_id(), Some("actual:source-owner"));
        assert_eq!(capture.stream(), "file");
        assert_eq!(ctx.retained_bytes_available(), available);
        Ok(())
    })
    .unwrap();
    assert_eq!(ctx.take_record_captures().unwrap().len(), 1);
}

#[test]
fn named_alias_inside_an_actual_ancestor_record_uses_only_working_provenance_scratch() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"prefixfield", &arena, &DecodePolicy::service()).unwrap();
    let alias = ctx
        .register_named_slice(root, ByteRange { start: 6, end: 11 }, "actual-field")
        .unwrap();
    ctx.record("actual:root-owner", root, || {
        assert_eq!(root.read_slice(0..11), Some(b"prefixfield".as_slice()));
        let available = ctx.retained_bytes_available();
        let capture = ctx.working_part_extent(alias)?;
        assert_eq!(capture.stream(), "actual-field");
        assert_eq!(capture.offset(), 0);
        assert_eq!(capture.byte_len(), 5);
        assert_eq!(capture.record_id(), Some("actual:root-owner"));
        assert!(capture.attempted_record_ids().is_empty());
        assert_eq!(ctx.retained_bytes_available(), available);
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "actual:root-owner");
    assert_eq!(captures[0].stream(), "file");
    assert_eq!(captures[0].offset(), 0);
    assert_eq!(captures[0].byte_len(), 11);
    assert_eq!(captures[0].reads().len(), 1);
    assert_eq!(captures[0].reads()["file"], [0..11]);
    assert!(ctx.take_record_captures().is_err());
}
