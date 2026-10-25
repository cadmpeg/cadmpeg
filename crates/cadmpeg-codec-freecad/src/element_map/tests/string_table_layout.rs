// SPDX-License-Identifier: Apache-2.0
//! Naming records retain genuine XML and side-entry reads through source parts.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, SourceOrigin};
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};
use std::collections::BTreeMap;

#[test]
fn inline_entities_unicode_cdata_and_nested_elements_use_a_real_expanded_stream() {
    let xml = "<Document><StringHasher count='4'>\n2.c seed-two\n1.c seed-one\n<![CDATA[a.c.2 Ω]]>&amp;β\n<Unknown>ignored nested payload</Unknown>-3.c.-1 second\u{2003}\n</StringHasher></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "Document.xml").unwrap();
    let source = crate::xml_framing::XmlSource::new(&ctx, root).unwrap();
    let index = source.children(0).next().unwrap();
    let entries = BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, Some(0));
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    ctx.decode_part(
        &mut output,
        source.extent(index).unwrap(),
        source.spec(index, "freecad.string_hasher").unwrap(),
        |stage| {
            crate::element_map::adopted::decode(&ctx, &source, index, None, None, &context, stage)
        },
    )
    .unwrap();
    assert!(output.losses().is_empty(), "{:?}", output.losses());
    let tables = output
        .native()
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .unwrap();
    let table = &tables[0];
    let entries = table.entries().unwrap();
    assert_eq!(entries[2].as_ref().unwrap().string_id, 10);
    assert_eq!(entries[2].as_ref().unwrap().components, [2]);
    assert_eq!(entries[2].as_ref().unwrap().payload, "Ω&β");
    assert_eq!(entries[3].as_ref().unwrap().string_id, 13);
    assert_eq!(entries[3].as_ref().unwrap().components, [1]);
    assert_eq!(entries[3].as_ref().unwrap().payload, "second");
    let streams = ctx.source_streams().unwrap();
    let expanded = streams
        .iter()
        .find(|stream| stream.name().starts_with("fcstd:naming-text:"))
        .unwrap();
    assert_eq!(
        expanded.byte_len(),
        u64::try_from("\n2.c seed-two\n1.c seed-one\na.c.2 Ω&β\n-3.c.-1 second\u{2003}\n".len())
            .unwrap()
    );
    match expanded.origin() {
        SourceOrigin::Expanded { parent, extent } => {
            assert_eq!(parent, "Document.xml");
            assert_eq!(
                extent.start,
                u64::try_from(source.extent(index).unwrap().start()).unwrap()
            );
            assert_eq!(
                extent.end,
                u64::try_from(source.extent(index).unwrap().end()).unwrap()
            );
        }
        origin => panic!("decoded XML text needs an actual expanded origin, got {origin:?}"),
    }
    let captures = ctx.take_record_captures().unwrap();
    let capture = captures
        .iter()
        .find(|capture| capture.id() == table.id())
        .unwrap();
    assert!(!capture.reads().is_empty());
    let native = serde_json::to_value(table).unwrap();
    assert_eq!(native["entries"][2]["payload"], "Ω&β");
    assert!(native["entries"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry.get("raw").is_none()));
}

#[test]
fn side_entry_raw_ranges_use_the_registered_member_view_and_preserve_multiline_payload() {
    let xml = b"<Document><StringHasher new='1'/><StringHasher2 file='table.txt'/></Document>";
    let side = "StringTableStart v1 1\n1.0 1:Ω\nsecond\n".as_bytes();
    let mut input = xml.to_vec();
    input.extend_from_slice(side);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&input, &arena, &DecodePolicy::service()).unwrap();
    let document_view = ctx
        .register_named_slice(
            root,
            cadmpeg_core::decode::ByteRange {
                start: 0,
                end: u64::try_from(xml.len()).unwrap(),
            },
            "Document.xml",
        )
        .unwrap();
    let side_view = ctx
        .register_named_slice(
            root,
            cadmpeg_core::decode::ByteRange {
                start: u64::try_from(xml.len()).unwrap(),
                end: u64::try_from(input.len()).unwrap(),
            },
            "table.txt",
        )
        .unwrap();
    let source = crate::xml_framing::XmlSource::new(&ctx, document_view).unwrap();
    let entries = BTreeMap::from([("table.txt".into(), side_view)]);
    let context = crate::element_map::adopted::Context::new(&entries, Some(1));
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    for index in source.children(0) {
        ctx.decode_part(
            &mut output,
            source.extent(index).unwrap(),
            source.spec(index, "freecad.string_hasher").unwrap(),
            |stage| {
                crate::element_map::adopted::decode(
                    &ctx, &source, index, None, None, &context, stage,
                )
            },
        )
        .unwrap();
    }
    assert!(output.losses().is_empty(), "{:?}", output.losses());
    let tables = output
        .native()
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .unwrap();
    assert_eq!(
        tables[0].entries().unwrap()[0].as_ref().unwrap().payload,
        "Ω\nsecond"
    );
    let captures = ctx.take_record_captures().unwrap();
    let capture = captures
        .iter()
        .find(|capture| capture.id() == tables[0].id())
        .unwrap();
    assert!(capture.reads().contains_key("table.txt"));
    assert_eq!(ctx.source_streams().unwrap().len(), 3);
    let parts = output
        .fidelity()
        .parts()
        .values()
        .map(|part| part.level().as_str())
        .collect::<Vec<_>>();
    assert!(parts.contains(&"freecad.string_table_entry"));
    assert!(parts.contains(&"freecad.string_table_header"));
    assert!(parts.contains(&"freecad.string_table_record"));
}

#[test]
fn raw_ranges_keep_the_original_trimmed_field_semantics_without_changing_payload_text() {
    let text = "\t1.0 0:authored\u{2003}\n\n";
    let (entries, ranges) = super::super::parse_string_table(text.as_bytes(), 1, false).unwrap();
    assert_eq!(entries[0].payload, "authored\u{2003}");
    assert_eq!(&text[ranges[0].clone()], "1.0 0:authored");
    assert_eq!(ranges[0].start, 1);
    assert!(serde_json::to_value(&entries[0])
        .unwrap()
        .get("raw")
        .is_none());
}

#[test]
fn detached_inline_source_cannot_manufacture_a_logical_record_extent() {
    let xml = "<Document><StringHasher count='1'>1.c value</StringHasher></Document>";
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    assert!(crate::xml_framing::XmlSource::new(
        &ctx,
        cadmpeg_core::decode::View::over_retained(xml.as_bytes())
    )
    .is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
