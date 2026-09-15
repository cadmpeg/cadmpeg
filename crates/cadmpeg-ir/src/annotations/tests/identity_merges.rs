// SPDX-License-Identifier: Apache-2.0
use crate::annotations::{AnnotationBuilder, StreamHandle};
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
    let error = annotations
        .map_ids(|_| "test:model:point#merged".into())
        .unwrap_err();
    assert_eq!(error.id, "test:model:point#merged");
    assert_eq!(annotations, before);
}

#[test]
fn remapping_calls_once_per_identity_and_preserves_each_annotation() {
    let mut builder = AnnotationBuilder::new();
    let stream = StreamHandle::new(crate::stream_name!("source"));
    builder.note("test:model:point#a", &stream, 7).tag("point");
    builder.exactness("test:model:point#a", Exactness::Inferred);
    builder.derived("test:model:point#b", "position").unwrap();
    let mut annotations = builder.build();
    let before = annotations.clone();
    let mut calls = Vec::new();
    annotations
        .map_ids(|id| {
            calls.push(id.to_owned());
            format!("{id}-mapped")
        })
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
        let error = target.append(incoming).unwrap_err();
        assert_eq!(error.id, "test:model:point#shared");
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
    target.append(incoming.clone()).unwrap();
    assert_eq!(target.provenance.len(), 2);
    assert_eq!(target.exactness(), incoming.exactness());
    for (id, note) in original.provenance.iter().chain(&incoming.provenance) {
        assert_eq!(target.provenance.get(id), Some(note));
    }
}
