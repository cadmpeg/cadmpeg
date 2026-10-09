// SPDX-License-Identifier: Apache-2.0

use crate::annotations::{AnnotationBuilder, Annotations};
use crate::Exactness;

const ID: &str = "test:model:point#existing";

#[test]
fn sparse_replacement_preserves_fields_and_supports_deletion() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut builder = AnnotationBuilder::new();
    builder.exactness(&ctx, ID, Exactness::Derived).unwrap();
    builder
        .field_exactness(&ctx, ID, "position", Exactness::Inferred)
        .unwrap();
    let mut annotations = builder.build();
    let mut transaction = annotations
        .sparse_transaction(&ctx, "test sparse replacement")
        .unwrap();
    transaction.exactness(ID, Exactness::ByteExact).unwrap();
    transaction.prepare().unwrap().apply(&mut annotations);
    assert_eq!(annotations.exactness()[ID].entity(), Exactness::ByteExact);
    assert_eq!(
        annotations.exactness()[ID].fields()["position"],
        Exactness::Inferred
    );
    let mut transaction = annotations
        .sparse_transaction(&ctx, "test sparse deletion")
        .unwrap();
    transaction.remove_entity(ID).unwrap();
    transaction.prepare().unwrap().apply(&mut annotations);
    assert!(annotations.exactness().is_empty());
}

#[test]
fn discarded_sparse_edits_leave_all_annotations_unchanged() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut builder = AnnotationBuilder::new();
    builder.exactness(&ctx, ID, Exactness::Inferred).unwrap();
    let annotations = builder.build();
    let before = annotations.clone();
    let mut transaction = annotations
        .sparse_transaction(&ctx, "test sparse abort")
        .unwrap();
    transaction.exactness(ID, Exactness::Derived).unwrap();
    transaction
        .exactness("test:model:point#new", Exactness::Derived)
        .unwrap();
    drop(transaction);
    assert_eq!(annotations, before);
}

#[test]
fn sparse_preparation_refusal_is_atomic_and_sticky() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let annotations = Annotations::default();
    let mut transaction = annotations
        .sparse_transaction(&ctx, "test sparse retained transfer")
        .unwrap();
    transaction.exactness(ID, Exactness::Derived).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = transaction.prepare() else {
        panic!("retained transfer must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert!(annotations.exactness().is_empty());
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}

#[test]
fn sparse_provenance_edits_leave_unselected_entries_unchanged() {
    use crate::annotations::StreamHandle;
    use crate::provenance::StreamName;
    let ctx = cadmpeg_test_support::service_decode_context();
    let stream = StreamHandle::new(
        &ctx,
        StreamName::try_from("synthetic".to_owned()).unwrap(),
        "test stream",
    )
    .unwrap();
    let mut annotations = Annotations::default();
    let mut transaction = annotations
        .sparse_transaction(&ctx, "test provenance creation")
        .unwrap();
    transaction.note(ID, &stream, 7, Some("original")).unwrap();
    transaction
        .note("test:model:point#untouched", &stream, 9, Some("untouched"))
        .unwrap();
    transaction.prepare().unwrap().apply(&mut annotations);
    let before = annotations.provenance["test:model:point#untouched"].clone();
    let mut transaction = annotations
        .sparse_transaction(&ctx, "test provenance edit")
        .unwrap();
    transaction
        .note(ID, &stream, 11, Some("replacement"))
        .unwrap();
    transaction.prepare().unwrap().apply(&mut annotations);
    assert_eq!(annotations.provenance["test:model:point#untouched"], before);
    assert_eq!(annotations.provenance[ID].offset, 11);
    assert_eq!(
        annotations.provenance[ID].tag.as_deref(),
        Some("replacement")
    );
    assert_eq!(annotations.provenance[ID].stream(), "synthetic");
}
