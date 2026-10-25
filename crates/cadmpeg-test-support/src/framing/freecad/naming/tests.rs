// SPDX-License-Identifier: Apache-2.0
//! The independent grammar keeps source boundaries without semantic reads.

use super::*;
use crate::framing::{freecad::frame_xml_member, zip, PartTable};
use crate::zip_fixture::{archive, RawEntry};
use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy, View};

fn inline(
    xml: &[u8],
    tag: &[u8],
    run: impl for<'a> FnOnce(&DecodeContext<'a>, View<'a>, usize, &mut Vec<FramedPart<'a>>),
) {
    let bytes = archive(&[RawEntry::stored("Document.xml", xml)]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut frames = Vec::new();
    let members = zip::frame(&ctx, root, &mut frames).unwrap();
    let member = &members[0];
    let source = ctx
        .register_named_slice(
            root,
            ByteRange {
                start: u64::try_from(member.payload().start()).unwrap(),
                end: u64::try_from(member.payload().end()).unwrap(),
            },
            "Document.xml",
        )
        .unwrap();
    let reached = frame_xml_member(
        &ctx,
        source,
        member.name(),
        member.frame_index(),
        &mut frames,
    )
    .unwrap();
    let index = reached
        .scan()
        .elements()
        .iter()
        .position(|element| element.name().window() == tag)
        .unwrap();
    let element = reached.scan().elements()[index];
    let markers = element.framing().collect::<Vec<_>>();
    let body = source.child(markers[1].end(), markers[2].start()).unwrap();
    ctx.record("fcstd:test:record#framing", source, || {
        run(
            &ctx,
            body,
            reached.frame_for_element(index).unwrap(),
            &mut frames,
        );
        Ok(())
    })
    .unwrap();
    PartTable::load().unwrap().validate(&ctx, &frames).unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert!(captures[0].reads().is_empty());
}

#[test]
fn multiline_payload_header_lookalikes_are_content_and_only_last_newline_is_framing() {
    let xml = b"<Document><StringHasher>z.0 1:payload\n1.c apparent-header\n2.c stable\n</StringHasher></Document>";
    inline(xml, b"StringHasher", |ctx, body, parent, out| {
        let start = out.len();
        frame_string_records(ctx, body, StringRecords::Document { count: 2 }, parent, out).unwrap();
        let records = &out[start..];
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0].view().window(),
            b"z.0 1:payload\n1.c apparent-header\n"
        );
        let internal = records[0].view().start() + b"z.0 1:payload".len();
        assert!(records[0]
            .framing()
            .iter()
            .all(|range| !range.contains(&u64::try_from(internal).unwrap())));
        assert!(records[0]
            .framing()
            .last()
            .unwrap()
            .contains(&u64::try_from(records[0].view().end() - 1).unwrap()));
    });
}

#[test]
fn unsupported_semantic_child_fields_do_not_change_node_and_chain_boundaries() {
    let xml = b"<Document><Properties><Property type='Part::PropertyPartShape'><ElementMap2>7 PostfixCount 1 tag\nMapCount 2\nElementMap bad 7 1\nFace ChildCount 1\nx 0 1 0 0 suffix 0\nNameCount 2\n0\n;stable.0 0\nEndMap\nElementMap 2 7 0\nEndMap\n</ElementMap2></Property></Properties></Document>";
    inline(xml, b"ElementMap2", |ctx, body, parent, out| {
        let start = out.len();
        frame_inline_map(ctx, body, parent, out).unwrap();
        let levels = out[start..]
            .iter()
            .map(|frame| frame.level().as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            levels
                .iter()
                .filter(|level| **level == "freecad.inline_map_node")
                .count(),
            2
        );
        assert!(levels.contains(&"freecad.inline_map_child"));
        assert_eq!(
            levels
                .iter()
                .filter(|level| **level == "freecad.inline_map_name_chain")
                .count(),
            2
        );
    });
}

#[test]
fn unprovable_located_string_tail_preserves_prefix_without_header_resynchronization() {
    let xml = b"<Document><StringHasher>1.c first\n2.0 2:lost\n3.c looks-like-a-header\n</StringHasher></Document>";
    inline(xml, b"StringHasher", |ctx, body, parent, out| {
        let start = out.len();
        assert!(
            frame_string_records(ctx, body, StringRecords::Document { count: 3 }, parent, out)
                .is_err()
        );
        // #215 cut-short/end-extent interpretation: the intact second header
        // locates one unfinished EOF tail; the apparent third header is payload.
        assert_eq!(out[start..].len(), 2);
        assert_eq!(out[start].view().window(), b"1.c first\n");
        assert_eq!(
            out[start + 1].view().window(),
            b"2.0 2:lost\n3.c looks-like-a-header\n"
        );
    });
}

#[test]
fn cut_short_terminated_node_still_has_a_source_extent_after_its_complete_peer() {
    let xml = b"<Document><Properties><Property type='Part::PropertyPartShape'><ElementMap2>7 PostfixCount 0\nMapCount 2\nElementMap 1 7 0\nEndMap\nElementMap 2 7 0\n</ElementMap2></Property></Properties></Document>";
    inline(xml, b"ElementMap2", |ctx, body, parent, out| {
        let start = out.len();
        assert!(frame_inline_map(ctx, body, parent, out).is_err());
        let nodes = out[start..]
            .iter()
            .filter(|frame| frame.level().as_str() == "freecad.inline_map_node")
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 2);
        assert!(nodes[0].view().window().ends_with(b"EndMap\n"));
        assert_eq!(nodes[1].view().window(), b"ElementMap 2 7 0\n");
        assert_eq!(nodes[1].framing().len(), 1);
    });
}
