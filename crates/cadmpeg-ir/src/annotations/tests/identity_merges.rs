// SPDX-License-Identifier: Apache-2.0
use crate::annotations::{AnnotationBuilder, AnnotationIdentityCollision, StreamHandle};
use crate::provenance::Exactness;

#[test]
fn remapping_refuses_collisions_across_tables_without_mutation() {
    let mut builder = AnnotationBuilder::new();
    let stream = StreamHandle::new(crate::stream_name!("source"));
    builder
        .note("test:model:point#provenance", &stream, 7)
        .tag("point");
    builder.exactness("test:model:point#exactness", Exactness::Inferred);
    let mut annotations = builder.build();
    let before = annotations.clone();
    let ctx = cadmpeg_test_support::service_decode_context();
    let error = annotations
        .map_ids(&ctx, |_| ctx.copy_retained_text("test:model:point#merged", "test remapped identity"), "remap annotation identities")
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(error, AnnotationIdentityCollision { id } if id == "test:model:point#merged")
    );
    assert_eq!(annotations, before);
}

#[test]
fn remapping_calls_once_per_identity_and_preserves_each_annotation() {
    let mut builder = AnnotationBuilder::new();
    let stream = StreamHandle::new(crate::stream_name!("source"));
    builder.note("test:model:point#a", &stream, 7).tag("point");
    builder.exactness("test:model:point#a", Exactness::Inferred);
    builder.derived(&cadmpeg_test_support::service_decode_context(), "test:model:point#b", "position").unwrap();
    let mut annotations = builder.build();
    let before = annotations.clone();
    let mut calls = Vec::new();
    let ctx = cadmpeg_test_support::service_decode_context();
    annotations
        .map_ids(&ctx, |id| {
            calls.push(id.to_owned());
            ctx.format_retained(format_args!("{id}-mapped"), "test remapped identity")
        }, "remap annotation identities")
        .unwrap()
        .unwrap();
    assert_eq!(calls, ["test:model:point#a", "test:model:point#b"]);
    for (id, note) in &before.provenance {
        assert_eq!(
            annotations.provenance.get(&format!("{id}-mapped")),
            Some(note)
        );
    }
    for (id, note) in before.exactness() {
        assert_eq!(
            annotations.exactness().get(&format!("{id}-mapped")),
            Some(note)
        );
    }
    assert_eq!(annotations.provenance.len(), before.provenance.len());
    assert_eq!(annotations.exactness().len(), before.exactness().len());
}

#[test]
fn appending_refuses_shared_identities_in_either_table_without_mutation() {
    for reverse in [false, true] {
        let stream = StreamHandle::new(crate::stream_name!("source"));
        let mut left = AnnotationBuilder::new();
        left.note("test:model:point#shared", &stream, 1);
        let mut right = AnnotationBuilder::new();
        right.note("test:model:point#new", &stream, 2);
        right.exactness("test:model:point#shared", Exactness::Derived);
        let (mut target, incoming) = if reverse {
            (right.build(), left.build())
        } else {
            (left.build(), right.build())
        };
        let before = target.clone();
        let error = target.append(&cadmpeg_test_support::service_decode_context(), incoming, "append annotation identities").unwrap().unwrap_err();
        assert!(
            matches!(error, AnnotationIdentityCollision { id } if id == "test:model:point#shared")
        );
        assert_eq!(target, before);
    }
}

#[test]
fn appending_disjoint_annotations_preserves_both_tables() {
    let stream = StreamHandle::new(crate::stream_name!("source"));
    let mut left = AnnotationBuilder::new();
    left.note("test:model:point#a", &stream, 1);
    let mut right = AnnotationBuilder::new();
    right.note("test:model:point#b", &stream, 2).tag("point");
    right.exactness("test:model:point#b", Exactness::Derived);
    let mut target = left.build();
    let incoming = right.build();
    let original = target.clone();
    target.append(&cadmpeg_test_support::service_decode_context(), incoming.clone(), "append annotation identities").unwrap().unwrap();
    assert_eq!(target.provenance.len(), 2);
    assert_eq!(target.exactness(), incoming.exactness());
    for (id, note) in original.provenance.iter().chain(&incoming.provenance) {
        assert_eq!(target.provenance.get(id), Some(note));
    }
}

#[test]
fn charged_remapping_preserves_collision_text_and_tables() {
    let mut builder = AnnotationBuilder::new();
    let stream = StreamHandle::new(crate::stream_name!("source"));
    builder.note("source-a", &stream, 7).tag("point");
    builder.exactness("source-b", Exactness::Inferred);
    let mut annotations = builder.build();
    let before = annotations.clone();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = cadmpeg_core::CodecError::from(
        annotations
            .map_ids(&ctx, |_| Ok("merged".into()), "test_annotation_remap")
            .unwrap()
            .unwrap_err(),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::Malformed(ref message)
        if message == "annotation identity collision at merged")
    );
    assert_eq!(annotations, before);
}

#[test]
fn empty_annotation_append_returns_an_existing_session_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(first) = ctx.charge_work(1, "existing annotation refusal").unwrap_err() else { panic!("work must refuse"); };
    let mut annotations = crate::annotations::Annotations::default();
    assert!(matches!(annotations.append(&ctx, Default::default(), "empty annotation append"), Err(CodecError::ResourceLimit(original)) if original == first));
    assert_eq!(annotations, Default::default());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == first));
}
