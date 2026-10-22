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
    use cadmpeg_ir::source_fidelity::RecordLayout;

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
    let capture = ctx.take_record_captures().unwrap().pop().unwrap();
    let (id, mut layout) = RecordLayout::from_capture(capture);
    layout.insert_pointer("next".into(), 9).unwrap();
    layout
        .insert_payload("opaque".into(), NativeBytes::new(vec![0xaa, 0xbb]))
        .unwrap();
    let mut input = SourceFidelity::default();
    input.insert_record_layout(id, layout.clone()).unwrap();

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

#[test]
fn whole_source_ledger_streams_follow_retained_streams_exactly_once() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::source_fidelity::freecad::FreecadSourceLedgers;

    let facts = serde_json::json!({
        "physical_ledger":[{"id":"fcstd:native:span#0","start":0,"end":3,"role":"compressed-payload","entry":"模型.bin"}],
        "logical_ledger":[{"id":"fcstd:native:logical-span#0","entry":"模型.bin","start":0,"end":3,"classification":"named_opaque","owner":"fcstd:native:property#Pad:Payload"}],
        "byte_coverage":{"id":"fcstd:native:coverage#0","physical_byte_len":3,"physical_span_count":1,"logical_entry_count":1,"logical_byte_len":3,"logical_span_count":1,"classification_bytes":{"named_opaque":3},"named_opaque_entries":["模型.bin"],"exact":true}
    });
    let mut input = source("member");
    input
        .insert_freecad_ledgers(
            "member".into(),
            serde_json::from_value::<FreecadSourceLedgers>(facts.clone()).unwrap(),
        )
        .unwrap();
    let direct = rescope_fidelity(input.clone(), "part/0", Default::default()).unwrap();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).unwrap();
    let ids = input
        .rescope_part_tree(&ctx, cadmpeg_ir::CodecFormat::F3d, "f3d:xref/part%2F0/")
        .unwrap();
    let staged = rescope_fidelity(input, "part/0", ids).unwrap();
    assert_eq!(staged.freecad_ledgers(), direct.freecad_ledgers());
    assert_eq!(staged.freecad_ledgers().len(), 1);
    let expected = "f3d:xref/part%2F0/member";
    assert_eq!(
        serde_json::to_value(&staged.freecad_ledgers()[expected]).unwrap(),
        facts
    );
    assert_eq!(
        staged.retained_records().values().next().unwrap().stream(),
        expected
    );
    let nested = rescope_fidelity(staged, "nested", Default::default()).unwrap();
    let expected = "f3d:xref/nested/f3d:xref/part%2F0/member";
    assert_eq!(nested.freecad_ledgers().len(), 1);
    assert_eq!(
        serde_json::to_value(&nested.freecad_ledgers()[expected]).unwrap(),
        facts
    );
    assert_eq!(
        nested.retained_records().values().next().unwrap().stream(),
        expected
    );
}
