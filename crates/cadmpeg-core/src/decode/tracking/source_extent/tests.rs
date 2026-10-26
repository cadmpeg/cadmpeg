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

#[test]
fn framing_extent_activates_the_actual_source_once_with_only_working_names() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcdef", &arena, &DecodePolicy::service()).unwrap();
    assert!(ctx.working_part_extent(root).is_err());
    let cold_available = ctx.retained_bytes_available();
    let child = root.child(1, 4).unwrap();
    let first = ctx.framing_part_extent(child).unwrap();
    assert_eq!(first.stream(), "file");
    assert_eq!(first.offset(), 1);
    assert_eq!(first.byte_len(), 3);
    assert_eq!(first.record_id(), None);
    let activated_available = ctx.retained_bytes_available();
    assert!(
        activated_available < cold_available,
        "actual source tracking infrastructure is admitted once"
    );
    let second = ctx.framing_part_extent(child).unwrap();
    assert_eq!(second.stream(), "file");
    assert_eq!(second.offset(), 1);
    assert_eq!(second.byte_len(), 3);
    assert_eq!(ctx.retained_bytes_available(), activated_available);
    drop(first);
    drop(second);
    assert_eq!(ctx.retained_bytes_available(), activated_available);
    assert!(ctx.take_record_captures().unwrap().is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn framing_extent_preserves_the_actual_owner_and_first_caller_refusal() {
    use crate::decode::{LimitScope, ResourceDimension, ResourceFailure};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4096;
    let (ctx, root) = DecodeContext::from_root_bytes(b"abcdef", &arena, &policy).unwrap();
    let mut first = None;
    let result: Result<(), CodecError> = ctx.record("actual-source-owner", root, || {
        ctx.retain_bytes(ctx.retained_bytes_available(), "actual held output")?;
        let capture = ctx.framing_part_extent(root.child(1, 4).unwrap())?;
        assert_eq!(capture.stream(), "file");
        assert_eq!(capture.offset(), 1);
        assert_eq!(capture.byte_len(), 3);
        assert_eq!(capture.record_id(), Some("actual-source-owner"));
        assert_eq!(ctx.retained_bytes_available(), 0);
        drop(capture);
        assert_eq!(ctx.retained_bytes_available(), 0);
        let error = ctx.retain_bytes(1, "real retained output").unwrap_err();
        let CodecError::ResourceLimit(refusal) = error else { panic!("actual retained ceiling") };
        assert_eq!(refusal.scope, LimitScope::Session);
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.reason, ResourceFailure::BudgetExceeded);
        assert_eq!(refusal.limit, 4096);
        assert_eq!(refusal.used, 4096);
        assert_eq!(refusal.additional, 1);
        assert_eq!(refusal.operation, "real retained output");
        assert!(matches!(ctx.framing_part_extent(root), Err(CodecError::ResourceLimit(actual)) if actual == refusal));
        first = Some(refusal);
        Err(CodecError::ResourceLimit(refusal))
    });
    let refusal = first.unwrap();
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == refusal));
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
    );
}

#[test]
fn framing_extent_never_activates_foreign_sources_or_reopens_extracted_tracking() {
    let arena = DecodeArena::new();
    let foreign_arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcdef", &arena, &DecodePolicy::service()).unwrap();
    let (_, foreign) =
        DecodeContext::from_root_bytes(b"abcdef", &foreign_arena, &DecodePolicy::service())
            .unwrap();
    let available = ctx.retained_bytes_available();
    assert!(ctx.framing_part_extent(foreign).is_err());
    assert!(ctx
        .framing_part_extent(View::over_retained(b"abcdef"))
        .is_err());
    assert_eq!(ctx.retained_bytes_available(), available);
    assert!(ctx.working_part_extent(root).is_err());
    assert!(ctx.resource_refusal().is_none());
    ctx.framing_part_extent(root).unwrap();
    assert!(ctx.take_record_captures().unwrap().is_empty());
    assert!(ctx.framing_part_extent(root).is_err());
    assert!(ctx.working_part_extent(root).is_err());
}
