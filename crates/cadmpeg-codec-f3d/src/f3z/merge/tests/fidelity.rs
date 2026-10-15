// SPDX-License-Identifier: Apache-2.0
use super::super::rescope_fidelity;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::source_fidelity::{RetainedSourceRecord, SourceFidelity};

const ID: &str = "f3d:native:record#one";

fn source(stream: &str) -> SourceFidelity {
    let mut annotations = AnnotationBuilder::new();
    let handle = StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::StreamName::try_from(stream.to_owned()).unwrap(),
        "fixture stream handle",
    )
    .unwrap();
    annotations
        .note(
            &cadmpeg_test_support::service_decode_context(),
            ID,
            &handle,
            7,
            Some("retained"),
        )
        .unwrap();
    annotations
        .derived(
            &cadmpeg_test_support::service_decode_context(),
            ID,
            "geometry",
        )
        .unwrap();
    let mut fidelity = SourceFidelity::with_annotations(annotations.build());
    fidelity
        .insert_retained_record(
            cadmpeg_ir::ids::UnknownId::mint(ID).unwrap(),
            RetainedSourceRecord::from_bytes(
                stream,
                7,
                cadmpeg_ir::source_fidelity::RetainedBytes::Inline {
                    data: vec![1, 2, 3],
                },
            )
            .unwrap(),
        )
        .unwrap();
    fidelity
}

#[test]
fn source_rescoping_preserves_bytes_extent_tag_and_exactness() {
    let input = source("member");
    let original = input.retained_record(ID).unwrap().clone();
    let output = rescope_fidelity(
        &cadmpeg_test_support::service_decode_context(),
        input.clone(),
        "part/occurrence-0",
    )
    .unwrap();
    let id = "f3d:xref/part/occurrence-0/native:record#one";
    let retained = output.retained_record(id).unwrap();
    assert_eq!(retained.data(), original.data());
    assert_eq!(retained.offset(), 7);
    assert_eq!(retained.end_offset(), 10);
    assert_eq!(retained.sha256(), original.sha256());
    assert_eq!(retained.stream(), "f3d:xref/part%2Foccurrence-0/member");
    let provenance = &output.annotations.provenance[id];
    assert_eq!(provenance.stream(), retained.stream());
    assert_eq!(provenance.offset, 7);
    assert_eq!(provenance.tag.as_deref(), Some("retained"));
    assert_eq!(
        output.annotations.exactness()[id],
        input.annotations.exactness()[ID]
    );
}

#[test]
fn occurrence_and_stream_boundaries_cannot_alias() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let first = rescope_fidelity(&ctx, source("b/c"), "a").unwrap();
    let second = rescope_fidelity(&ctx, source("c"), "a/b").unwrap();
    let escaped = rescope_fidelity(&ctx, source("c"), "a%2Fb").unwrap();
    let owner = |fidelity: &SourceFidelity| {
        fidelity
            .retained_records()
            .values()
            .next()
            .unwrap()
            .stream()
            .to_owned()
    };
    assert_eq!(owner(&first), "f3d:xref/a/b/c");
    assert_eq!(owner(&second), "f3d:xref/a%2Fb/c");
    assert_eq!(owner(&escaped), "f3d:xref/a%252Fb/c");
}

#[test]
fn source_rescoping_preserves_absent_provenance_tag() {
    let mut builder = AnnotationBuilder::new();
    let stream = StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::StreamName::try_from("member".to_owned()).unwrap(),
        "fixture stream handle",
    )
    .unwrap();
    builder
        .note(
            &cadmpeg_test_support::service_decode_context(),
            ID,
            &stream,
            7,
            None,
        )
        .unwrap();
    let output = rescope_fidelity(
        &cadmpeg_test_support::service_decode_context(),
        SourceFidelity::with_annotations(builder.build()),
        "part",
    )
    .unwrap();
    assert_eq!(
        output.annotations.provenance["f3d:xref/part/native:record#one"].tag,
        None
    );
}
