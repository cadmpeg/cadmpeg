// SPDX-License-Identifier: Apache-2.0
//! Actual root metadata parsing and rollback within its XML boundary.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use std::io::Cursor;

#[test]
fn a_bad_root_metadata_field_retains_only_the_root_and_preserves_a_zip_peer() {
    let prefix = "<?xml version='1.0'?><!-- 前 -->";
    let root = "<Document SchemaVersion='invalid' FileVersion='7' ProgramVersion='λ'><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let document = format!("{prefix}{root}<!-- after -->");
    let bytes = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document.as_bytes()),
        RawEntry::stored("peer.bin", b"independent peer"),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one failed root")
    };
    let fidelity = decoded.source_fidelity();
    let part = &fidelity.parts()[&unknown.id];
    assert_eq!(part.level().as_str(), "freecad.document");
    assert_eq!(part.state(), &PartState::GivenUp {});
    assert_eq!(part.offset(), u64::try_from(prefix.len()).unwrap());
    assert_eq!(part.byte_len(), u64::try_from(root.len()).unwrap());
    let xml = &fidelity.parts()[part.parent().unwrap()];
    assert_eq!(xml.level().as_str(), "freecad.document_xml");
    assert_eq!(xml.state(), &PartState::Decoded {});
    let retained = fidelity.retained_record(unknown.id.as_str()).unwrap();
    assert_eq!(retained.data(), Some(root.as_bytes()));
    assert_eq!(
        retained.sha256(),
        cadmpeg_ir::hash::sha256_hex(root.as_bytes())
    );
    assert!(fidelity.record_layout()[unknown.id.as_str()]
        .reads()
        .is_empty());
    assert!(!fidelity
        .record_layout()
        .contains_key(&crate::native::native_id("document", "0")));
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    assert!(native
        .arena_as::<crate::native::DocumentFacts>("document")
        .unwrap()
        .is_empty());
    let all_entries = native
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let entries = all_entries
        .iter()
        .filter(|entry| entry.name() != "Document.xml")
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name(), "peer.bin");
    assert_eq!(all_entries.len(), 2);
    let document_entry = all_entries
        .iter()
        .find(|entry| entry.name() == "Document.xml")
        .unwrap();
    assert_eq!(
        document_entry.id(),
        crate::native::native_id("entry", "Document.xml")
    );
    assert_eq!(
        document_entry.role,
        cadmpeg_core::container::ContainerRole::Document
    );
    assert!(document_entry.referenced_by().is_empty());
    let failures = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1);
    let location = failures[0].provenance.as_ref().unwrap();
    assert_eq!(location.stream_name(), Some(part.stream()));
    assert_eq!(location.offset, part.offset());
    assert_eq!(location.byte_len(), Some(part.byte_len()));
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let snapshot = cadmpeg_container::ArchiveSnapshot::new(&ctx, view).unwrap();
    let actual_document = snapshot.open(&ctx, "Document.xml").unwrap();
    let capture = ctx.capture_part_extent(actual_document).unwrap();
    let entry_layout = &fidelity.record_layout()[document_entry.id()];
    crate::test_support::source_fidelity::assert_entry_header_layout(
        &decoded,
        &bytes,
        "Document.xml",
    );
    assert_eq!(actual_document.window(), document.as_bytes());
    assert!(entry_layout.payloads().is_empty());
    assert_eq!(
        entry_layout.pointers(),
        &crate::source_archive::entry_pointers(
            snapshot.entry("Document.xml").unwrap(),
            capture.byte_len()
        )
        .unwrap()
    );
    let archive = crate::source_archive::OriginalArchive::new(&ctx, view, fidelity).unwrap();
    assert_eq!(
        archive.open(&ctx, "Document.xml").unwrap().window(),
        document.as_bytes()
    );
    assert_eq!(
        archive
            .part(&ctx, unknown.id.as_str(), "Document.xml")
            .unwrap(),
        root.as_bytes()
    );
    assert_eq!(
        archive.open(&ctx, "peer.bin").unwrap().window(),
        b"independent peer"
    );
    let mut shifted = serde_json::to_value(fidelity).unwrap();
    for map in ["parts", "record_layout", "retained_records"] {
        shifted[map][unknown.id.as_str()]["offset"] = serde_json::json!(part.offset() + 1);
    }
    for range in shifted["parts"][unknown.id.as_str()]["framing_bytes"]
        .as_array_mut()
        .unwrap()
    {
        let start = range["start"].as_u64().unwrap();
        let end = range["end"].as_u64().unwrap();
        range["start"] = serde_json::json!(start + 1);
        range["end"] = serde_json::json!(end + 1);
    }
    let shifted: cadmpeg_ir::SourceFidelity = serde_json::from_value(shifted).unwrap();
    assert!(crate::source_archive::OriginalArchive::new(&ctx, view, &shifted).is_err());
}

#[test]
fn successful_root_metadata_is_staged_once_from_its_original_root_capture() {
    let xml = b"<!-- before --><Document SchemaVersion='4' FileVersion='7' ProgramVersion='1&#46;2'><Objects Count='0'/><ObjectData Count='0'/></Document><!-- after -->";
    let bytes = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml)]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let records = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::DocumentFacts>("document")
        .unwrap();
    let [facts] = records.as_slice() else {
        panic!("exactly one staged document")
    };
    assert_eq!(facts.file_version.as_str(), "7");
    assert_eq!(facts.program_version.as_deref(), Some("1.2"));
    assert_eq!(facts.root_name, "Document");
    assert_eq!(facts.object_count, 0);
    let fidelity = decoded.source_fidelity();
    let root = fidelity
        .parts()
        .values()
        .find(|part| part.level().as_str() == "freecad.document")
        .unwrap();
    assert_eq!(root.state(), &PartState::Decoded {});
    let layout = &fidelity.record_layout()[facts.id.as_str()];
    assert_eq!(layout.offset(), root.offset());
    assert_eq!(layout.byte_len(), root.byte_len());
    let reads = layout.reads().get(layout.stream()).unwrap();
    for spelling in [b"7".as_slice(), b"1&#46;2".as_slice()] {
        let at = xml
            .windows(spelling.len())
            .position(|window| window == spelling)
            .unwrap();
        assert!(reads
            .iter()
            .any(|range| range.start == u64::try_from(at).unwrap()
                && range.end == u64::try_from(at + spelling.len()).unwrap()));
    }
    assert!(decoded.ir().native_unknowns("fcstd").unwrap().is_empty());
}

#[test]
fn a_readable_deflated_document_entry_outlives_failed_root_metadata() {
    use std::io::Write;
    let document = b"<!--before--><Document SchemaVersion='invalid' FileVersion='7'><Objects/><ObjectData/></Document><!--after-->";
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "Document.xml",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    writer.write_all(document).unwrap();
    writer
        .start_file(
            "peer.bin",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
    writer.write_all(b"independent peer").unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    assert!(native
        .arena_as::<crate::native::DocumentFacts>("document")
        .unwrap()
        .is_empty());
    let entries = native
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries.iter().map(|entry| entry.name()).collect::<Vec<_>>(),
        ["Document.xml", "peer.bin"]
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = cadmpeg_container::ArchiveSnapshot::new(&ctx, view).unwrap();
    let expanded = source.open(&ctx, "Document.xml").unwrap();
    let capture = ctx.capture_part_extent(expanded).unwrap();
    assert_eq!(expanded.window(), document);
    let layout = &decoded.source_fidelity().record_layout()[entries[0].id()];
    crate::test_support::source_fidelity::assert_entry_header_layout(
        &decoded,
        &bytes,
        "Document.xml",
    );
    assert!(layout.payloads().is_empty());
    assert_eq!(
        layout.pointers(),
        &crate::source_archive::entry_pointers(
            source.entry("Document.xml").unwrap(),
            capture.byte_len()
        )
        .unwrap()
    );
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one failed root")
    };
    let failed = &decoded.source_fidelity().parts()[&unknown.id];
    assert_eq!(failed.level().as_str(), "freecad.document");
    assert_eq!(failed.state(), &PartState::GivenUp {});
    assert_eq!(failed.stream(), capture.stream());
    assert_eq!(
        failed.offset(),
        u64::try_from(b"<!--before-->".len()).unwrap()
    );
    assert_eq!(
        decoded.source_fidelity().parts()[failed.parent().unwrap()].state(),
        &PartState::Decoded {}
    );
    let original =
        crate::source_archive::OriginalArchive::new(&ctx, view, decoded.source_fidelity()).unwrap();
    assert_eq!(
        original.open(&ctx, "Document.xml").unwrap().window(),
        document
    );
    assert_eq!(
        original.open(&ctx, "peer.bin").unwrap().window(),
        b"independent peer"
    );
}
