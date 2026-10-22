// SPDX-License-Identifier: Apache-2.0
use super::super::rescope_fidelity;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::source_fidelity::{RetainedSourceRecord, SourceFidelity};

const ID: &str = "f3d:native:record#one";

fn source(stream: &str) -> SourceFidelity {
    let mut annotations = AnnotationBuilder::new();
    let handle = StreamHandle::new(cadmpeg_ir::StreamName::try_from(stream.to_owned()).unwrap());
    annotations.note(ID, &handle, 7, Some("retained"));
    annotations.derived(ID, "geometry").unwrap();
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
    let output = rescope_fidelity(input.clone(), "part/occurrence-0", Default::default()).unwrap();
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
    let first = rescope_fidelity(source("b/c"), "a", Default::default()).unwrap();
    let second = rescope_fidelity(source("c"), "a/b", Default::default()).unwrap();
    let escaped = rescope_fidelity(source("c"), "a%2Fb", Default::default()).unwrap();
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
    let stream = StreamHandle::new(cadmpeg_ir::StreamName::try_from("member".to_owned()).unwrap());
    builder.note(ID, &stream, 7, None);
    let output = rescope_fidelity(
        SourceFidelity::with_annotations(builder.build()),
        "part",
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        output.annotations.provenance["f3d:xref/part/native:record#one"].tag,
        None
    );
}

#[test]
fn source_rescoping_and_append_union_unrepresented_classes() {
    use cadmpeg_ir::document::ArenaClass::{Design, Model};
    let mut parent = source("parent");
    parent.mark_untyped(ID, [Model]).unwrap();
    let mut child = source("member");
    child.mark_untyped(ID, [Design]).unwrap();
    let rescoped = rescope_fidelity(child, "part/occurrence-0", Default::default()).unwrap();
    assert!(rescoped.unrepresented().contains(&Design));
    assert!(!rescoped.unrepresented().contains(&Model));
    parent.append(rescoped).unwrap();
    assert_eq!(parent.unrepresented(), [Model, Design].into());
}

#[test]
fn record_layout_rescoping_preserves_alias_reads_pointers_and_payloads() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::native::bytes::NativeBytes;

    let bytes = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "member").unwrap();
    let alias = ctx
        .register_named_slice(
            root,
            cadmpeg_core::decode::ByteRange { start: 4, end: 12 },
            "member/inner",
        )
        .unwrap();
    ctx.record(ID, alias, || {
        let mut cursor = alias;
        assert_eq!(cursor.u8(), Some(5));
        cursor.skip(1).unwrap();
        assert_eq!(cursor.u8(), Some(7));
        Ok(())
    })
    .unwrap();
    let mut input = SourceFidelity::default();
    input
        .install_record_captures(&ctx, |_, layout| {
            layout.insert_pointer("next".into(), 9)?;
            layout.insert_payload("opaque".into(), NativeBytes::new(vec![0xaa, 0xbb]))
        })
        .unwrap();
    let layout = input.record_layout()[ID].clone();

    let output = rescope_fidelity(input, "part/occurrence-0", Default::default()).unwrap();
    let scoped = &output.record_layout()["f3d:xref/part/occurrence-0/native:record#one"];
    let prefix = "f3d:xref/part%2Foccurrence-0/";
    assert_eq!(scoped.stream(), format!("{prefix}member/inner"));
    assert_eq!((scoped.offset(), scoped.byte_len()), (0, 8));
    assert_eq!(scoped.reads().len(), 2);
    assert_eq!(scoped.reads()[&format!("{prefix}member")], [4..5, 6..7]);
    assert_eq!(
        scoped.reads()[&format!("{prefix}member/inner")],
        [0..1, 2..3]
    );
    assert_eq!(scoped.pointers(), layout.pointers());
    assert_eq!(scoped.payloads(), layout.payloads());
    assert!(!output.record_layout().contains_key(ID));
}
