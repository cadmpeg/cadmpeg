// SPDX-License-Identifier: Apache-2.0
//! String-table raw fields retain their decoded text with genuine provenance.

use super::*;
use crate::element_map::SourceLayouts;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, SourceOrigin};
use std::collections::BTreeMap;

#[test]
fn inline_entities_unicode_cdata_and_nested_elements_use_a_real_expanded_stream() {
    // Dotted components refer to earlier string ids, so ids 2 and 1 precede
    // the two source records whose raw payloads and deltas this control checks.
    let xml = "<Document><StringHasher count='4'>\n2.c seed-two\n1.c seed-one\n<![CDATA[a.c.2 Ω]]>&amp;β\n<Unknown>ignored nested payload</Unknown>-3.c.-1 second\u{2003}\n</StringHasher></Document>";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "Document.xml").unwrap();
    let document = roxmltree::Document::parse(xml).unwrap();
    let carrier = document
        .descendants()
        .find(|node| node.has_tag_name("StringHasher"))
        .unwrap();
    let entries = BTreeMap::new();
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    let mut pending = BTreeMap::new();
    let property_ranges = BTreeMap::new();
    let reader =
        crate::property_reader::PropertyReader::source(&ctx, root, &property_ranges).unwrap();
    let sources = crate::entry_sources::EntrySources::new(&ctx, &entries).unwrap();
    let (tables, maps) = super::super::parse(
        &reader,
        &document,
        Some(0),
        &[],
        &[],
        &sources,
        &mut Vec::new(),
        Some(&mut SourceLayouts {
            document: root,
            entries: &entries,
            fidelity: &mut fidelity,
            payloads: &mut pending,
        }),
    )
    .unwrap();
    crate::fidelity::install_layouts(&ctx, &mut fidelity, pending, BTreeMap::new()).unwrap();
    let table = &tables.as_slice()[0];
    assert!(maps.is_empty());
    assert_eq!(table.entries().unwrap()[2].string_id, 10);
    assert_eq!(table.entries().unwrap()[2].components, [2]);
    assert_eq!(table.entries().unwrap()[2].payload, "Ω&β");
    assert_eq!(table.entries().unwrap()[3].string_id, 13);
    assert_eq!(table.entries().unwrap()[3].components, [1]);
    assert_eq!(table.entries().unwrap()[3].payload, "second");
    let layout = &fidelity.record_layout()[&table.id()];
    assert_eq!(layout.offset(), 0);
    assert_eq!(
        layout.byte_len(),
        u64::try_from("\n2.c seed-two\n1.c seed-one\na.c.2 Ω&β\n-3.c.-1 second\u{2003}\n".len())
            .unwrap()
    );
    assert_eq!(
        layout.payloads()["entries.2.raw"].as_ref(),
        "a.c.2 Ω&β".as_bytes()
    );
    assert_eq!(
        layout.payloads()["entries.3.raw"].as_ref(),
        b"-3.c.-1 second"
    );
    assert!(fidelity.retained_records().is_empty());
    let streams = ctx.source_streams().unwrap();
    let stream = streams
        .iter()
        .find(|stream| stream.name() == layout.stream())
        .unwrap();
    assert_eq!(stream.byte_len(), layout.byte_len());
    match stream.origin() {
        SourceOrigin::Expanded { parent, extent } => {
            assert_eq!(parent, "Document.xml");
            assert_eq!(extent.start, u64::try_from(carrier.range().start).unwrap());
            assert_eq!(extent.end, u64::try_from(carrier.range().end).unwrap());
        }
        origin => panic!("decoded XML text needs an actual expanded origin, got {origin:?}"),
    }
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
    let document = roxmltree::Document::parse(std::str::from_utf8(xml).unwrap()).unwrap();
    let entry = legacy_entry("table.txt", side);
    let entries = BTreeMap::from([("table.txt".into(), side_view)]);
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    let mut pending = BTreeMap::new();
    let property_ranges = BTreeMap::new();
    let reader =
        crate::property_reader::PropertyReader::source(&ctx, document_view, &property_ranges)
            .unwrap();
    let sources = crate::entry_sources::EntrySources::new(&ctx, &entries).unwrap();
    let (tables, _) = super::super::parse(
        &reader,
        &document,
        Some(1),
        &[],
        &[entry.record.clone()],
        &sources,
        &mut Vec::new(),
        Some(&mut SourceLayouts {
            document: document_view,
            entries: &entries,
            fidelity: &mut fidelity,
            payloads: &mut pending,
        }),
    )
    .unwrap();
    crate::fidelity::install_layouts(&ctx, &mut fidelity, pending, BTreeMap::new()).unwrap();
    let table = &tables.as_slice()[0];
    assert_eq!(table.entries().unwrap()[0].payload, "Ω\nsecond");
    let layout = &fidelity.record_layout()[&table.id()];
    assert_eq!(layout.stream(), "table.txt");
    assert_eq!(layout.offset(), 0);
    assert_eq!(layout.byte_len(), u64::try_from(side.len()).unwrap());
    assert_eq!(
        layout.payloads()["entries.0.raw"].as_ref(),
        "1.0 1:Ω\nsecond".as_bytes()
    );
    assert_eq!(ctx.source_streams().unwrap().len(), 3);
    assert!(fidelity.retained_records().is_empty());
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
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    let document = roxmltree::Document::parse(xml).unwrap();
    let entries = BTreeMap::new();
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    let before = fidelity.clone();
    let mut pending = BTreeMap::new();
    let property_ranges = BTreeMap::new();
    let reader =
        crate::property_reader::PropertyReader::source(&ctx, root, &property_ranges).unwrap();
    let sources = crate::entry_sources::EntrySources::new(&ctx, &entries).unwrap();
    assert!(super::super::parse(
        &reader,
        &document,
        Some(0),
        &[],
        &[],
        &sources,
        &mut Vec::new(),
        Some(&mut SourceLayouts {
            document: cadmpeg_core::decode::View::over_retained(xml.as_bytes()),
            entries: &entries,
            fidelity: &mut fidelity,
            payloads: &mut pending,
        })
    )
    .is_err());
    assert!(pending.is_empty());
    assert_eq!(fidelity, before);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
