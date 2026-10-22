// SPDX-License-Identifier: Apache-2.0
//! Occurrence composition of an actual retained part home and its source tree.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::CodecFormat;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec, PartState};
use cadmpeg_ir::source_fidelity::RecordLayout;
use cadmpeg_ir::{Native, NativeUnknownRecord, SourceFidelity};

use super::super::{extend_native, occurrence_stream_prefix, rescope_fidelity};

const SOURCE: &[u8] = b"\x03\0\0\0bad";
const OWNER: &str = "f3d:native:record#owner";
const SCOPED_PART: &str =
    "f3d:source:part#c3de81cb0dc2403fb27d9b7538c8e5d8f6f7e7f1ac57c7ebfa8c8cd2477e1db1";

fn source<'a>(
    ctx: &DecodeContext<'a>,
    root: cadmpeg_core::decode::View<'a>,
) -> (Native, SourceFidelity) {
    let root = ctx.name_stream(root, "member").expect("actual stream");
    let mut output = PartOutput::new(CodecFormat::F3d);
    let spec =
        |name, frames| PartSpec::new(PartLevel::parse(name).expect("Appendix C level"), frames);
    ctx.record(OWNER, root, || {
        ctx.decode_part(&mut output, root, spec("f3d.manifest", vec![]), |staged| {
            ctx.decode_part(staged, root, spec("f3d.manifest_field", vec![0..4]), |_| {
                Err::<(), _>(CodecError::malformed("invalid manifest version bad"))
            })?;
            Err::<(), _>(CodecError::malformed("manifest is unreadable"))
        })
        .map(drop)
    })
    .expect("owner and failed bounded child");
    let (_, mut native, mut fidelity, losses) = output.into_parts();
    assert_eq!(losses.len(), 1);
    let namespace = native.namespace_mut("f3d");
    let mut unknowns = namespace
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("stub");
    assert_eq!(unknowns.len(), 1);
    let part = unknowns[0].id.clone();
    unknowns.push(NativeUnknownRecord {
        id: OWNER.try_into().expect("owner ID"),
        links: vec![part.clone().into()],
    });
    namespace
        .set_arena(ctx, "unknowns", &unknowns)
        .expect("owner link to part");
    let capture = ctx
        .take_record_captures()
        .expect("owner capture")
        .pop()
        .expect("record");
    let (id, layout) = RecordLayout::from_capture(capture);
    fidelity
        .insert_record_layout(id, layout)
        .expect("owner layout");
    let mut annotations = AnnotationBuilder::new();
    let stream = StreamHandle::new("member".to_owned().try_into().expect("stream"));
    annotations.note(part.as_str(), &stream, 0, Some("failed manifest"));
    let mut annotated = SourceFidelity::with_annotations(annotations.build());
    annotated.append(fidelity).expect("source tables");
    (native, annotated)
}

#[test]
fn occurrence_composition_keeps_canonical_part_ids_and_every_home_reference() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::service()).expect("root");
    let (native, mut fidelity) = source(&ctx, root);
    let before = fidelity.clone();
    assert!(rescope_fidelity(fidelity.clone(), "a", &Default::default()).is_err());
    let ids = fidelity
        .rescope_part_tree(
            &ctx,
            CodecFormat::F3d,
            occurrence_stream_prefix("a").expect("owner").as_str(),
        )
        .expect("checked address mapping");
    let mut output_native = Native::default();
    extend_native(&ctx, &mut output_native, native, "a", &ids).expect("native composition");
    let output = rescope_fidelity(fidelity, "a", &ids).expect("fidelity composition");
    let unknowns = output_native
        .namespace("f3d")
        .expect("native")
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("typed stubs");
    assert_eq!(unknowns.len(), 2);
    let stub = unknowns
        .iter()
        .find(|record| record.id.as_str() == SCOPED_PART)
        .expect("canonical part stub");
    assert_eq!(
        stub.links.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
        ["f3d:xref/a/native:record#owner"]
    );
    let owner = unknowns
        .iter()
        .find(|record| record.id.as_str() == "f3d:xref/a/native:record#owner")
        .expect("scoped owner");
    assert_eq!(
        owner.links.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
        [SCOPED_PART]
    );
    assert_eq!(output.parts().len(), 2);
    assert_eq!(output.parts()[SCOPED_PART].stream(), "f3d:xref/a/member");
    assert_eq!(output.parts()[SCOPED_PART].state(), &PartState::GivenUp);
    let child = output
        .parts()
        .values()
        .find(|part| part.parent().is_some())
        .expect("reached covered child");
    assert_eq!(child.parent().map(|id| id.as_str()), Some(SCOPED_PART));
    assert_eq!(
        child.state(),
        &PartState::Covered {
            by: SCOPED_PART.try_into().expect("canonical ID")
        }
    );
    let retained = output
        .retained_record(SCOPED_PART)
        .expect("single source home");
    assert_eq!(retained.data(), Some(SOURCE));
    assert_eq!(
        (retained.stream(), retained.offset()),
        ("f3d:xref/a/member", 0)
    );
    let layout = &output.record_layout()[SCOPED_PART];
    assert_eq!(
        (layout.stream(), layout.offset(), layout.byte_len()),
        ("f3d:xref/a/member", 0, 7)
    );
    assert!(layout.reads().is_empty());
    let provenance = &output.annotations.provenance[SCOPED_PART];
    assert_eq!(
        (
            provenance.stream(),
            provenance.offset,
            provenance.tag.as_deref()
        ),
        ("f3d:xref/a/member", 0, Some("failed manifest"))
    );
    for old in before.parts().keys() {
        assert!(!output.parts().contains_key(old));
        assert!(!output.record_layout().contains_key(old.as_str()));
        assert!(unknowns.iter().all(|record| &record.id != old));
    }
    let wire = serde_json::to_value(&output).expect("wire");
    assert_eq!(
        serde_json::from_value::<SourceFidelity>(wire).expect("checked tree roundtrip"),
        output
    );
}

#[test]
fn distinct_and_nested_occurrences_recompute_ids_from_actual_escaped_streams() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::service()).expect("root");
    let (_, input) = source(&ctx, root);
    let mut first = input.clone();
    let ids = first
        .rescope_part_tree(
            &ctx,
            CodecFormat::F3d,
            occurrence_stream_prefix("a").expect("owner").as_str(),
        )
        .expect("first address map");
    let mut first = rescope_fidelity(first, "a", &ids).expect("first");
    let ids = first
        .rescope_part_tree(
            &ctx,
            CodecFormat::F3d,
            occurrence_stream_prefix("b/c").expect("owner").as_str(),
        )
        .expect("nested address map");
    let nested = rescope_fidelity(first, "b/c", &ids).expect("nested");
    assert!(nested
        .parts()
        .values()
        .all(|part| part.stream() == "f3d:xref/b%2Fc/f3d:xref/a/member"));
    assert!(nested
        .retained_records()
        .values()
        .all(|record| record.stream() == "f3d:xref/b%2Fc/f3d:xref/a/member"));
    let mut other = input;
    let ids = other
        .rescope_part_tree(
            &ctx,
            CodecFormat::F3d,
            occurrence_stream_prefix("b%2Fc")
                .expect("different owner")
                .as_str(),
        )
        .expect("other address map");
    let other = rescope_fidelity(other, "b%2Fc", &ids).expect("other");
    assert!(other
        .parts()
        .keys()
        .all(|id| !nested.parts().contains_key(id)));
    let mut joined = nested;
    joined.append(other).expect("distinct occurrence trees");
    assert_eq!(joined.parts().len(), 4);
    assert_eq!(joined.retained_records().len(), 2);
    let before = joined.clone();
    assert!(joined.append(before.clone()).is_err());
    assert_eq!(joined, before);
}
