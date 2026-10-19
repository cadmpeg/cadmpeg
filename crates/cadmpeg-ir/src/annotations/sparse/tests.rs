// SPDX-License-Identifier: Apache-2.0

use crate::annotations::{AnnotationBuilder, Annotations};
use crate::Exactness;

const ID: &str = "test:model:point#existing";

#[test]
fn sparse_replacement_preserves_fields_and_supports_deletion() {
    let mut builder = AnnotationBuilder::new();
    builder.exactness(ID, Exactness::Derived);
    builder
        .field_exactness(ID, "position", Exactness::Inferred)
        .unwrap();
    let mut annotations = builder.build();
    let mut transaction = annotations.sparse_transaction();
    transaction.exactness(ID, Exactness::ByteExact);
    transaction.prepare().apply(&mut annotations);
    assert_eq!(annotations.exactness()[ID].entity(), Exactness::ByteExact);
    assert_eq!(
        annotations.exactness()[ID].fields()["position"],
        Exactness::Inferred
    );
    let mut transaction = annotations.sparse_transaction();
    transaction.remove_entity(ID);
    transaction.prepare().apply(&mut annotations);
    assert!(annotations.exactness().is_empty());
}

#[test]
fn discarded_sparse_edits_leave_all_annotations_unchanged() {
    let mut builder = AnnotationBuilder::new();
    builder.exactness(ID, Exactness::Inferred);
    let annotations = builder.build();
    let before = annotations.clone();
    let mut transaction = annotations.sparse_transaction();
    transaction.exactness(ID, Exactness::Derived);
    transaction.exactness("test:model:point#new", Exactness::Derived);
    drop(transaction);
    assert_eq!(annotations, before);
}

#[test]
fn sparse_provenance_edits_leave_unselected_entries_unchanged() {
    use crate::annotations::StreamHandle;
    use crate::provenance::StreamName;
    let stream = StreamHandle::new(StreamName::try_from("synthetic".to_owned()).unwrap());
    let mut annotations = Annotations::default();
    let mut transaction = annotations.sparse_transaction();
    transaction.note(ID, &stream, 7, Some("original"));
    transaction.note("test:model:point#untouched", &stream, 9, Some("untouched"));
    transaction.prepare().apply(&mut annotations);
    let before = annotations.provenance["test:model:point#untouched"].clone();
    let mut transaction = annotations.sparse_transaction();
    transaction.note(ID, &stream, 11, Some("replacement"));
    transaction.prepare().apply(&mut annotations);
    assert_eq!(annotations.provenance["test:model:point#untouched"], before);
    assert_eq!(annotations.provenance[ID].offset, 11);
    assert_eq!(
        annotations.provenance[ID].tag.as_deref(),
        Some("replacement")
    );
    assert_eq!(annotations.provenance[ID].stream(), "synthetic");
}

#[test]
fn sparse_long_identity_edits_admit_thousands_without_binary_tree_overbilling() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    const COUNT: usize = 8192;
    const WORK: u64 = 512_000_000;
    let identity = |index| {
        format!("test:model:point#annotation-transaction-key-with-a-long-prefix-{index:08}")
    };
    let mut builder = AnnotationBuilder::new();
    for index in 0..COUNT {
        builder.exactness(identity(index), Exactness::Derived);
    }
    let mut annotations = builder.build();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut transaction = annotations.sparse_transaction();
    for index in 0..COUNT {
        transaction.exactness(&identity(index), Exactness::Inferred);
    }
    transaction.prepare().apply(&mut annotations);
    assert_eq!(annotations.exactness().len(), COUNT);
    assert!(annotations
        .exactness()
        .values()
        .all(|note| note.entity() == Exactness::Inferred));
    // The former bound bills this base lookup during both copying and application.
    // Those two probes alone exceed the ceiling; this excludes all other work.
    let old_base_probe = (15 * 32 + 4) * u64::try_from(identity(0).len()).unwrap() + 1;
    assert!(2 * u64::try_from(COUNT).unwrap() * old_base_probe > WORK);
    ctx.finish_session().unwrap();
}
