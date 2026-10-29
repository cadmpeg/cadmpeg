// SPDX-License-Identifier: Apache-2.0
//! Actual root metadata parsing and rollback within its XML boundary.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
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
    let fidelity = decoded.source_fidelity();
    let parts = crate::test_support::source_fidelity::given_up(fidelity);
    let [(_, retained)] = parts.as_slice() else {
        panic!("one failed root")
    };
    assert_eq!(retained.offset(), u64::try_from(prefix.len()).unwrap());
    assert_eq!(retained.byte_len(), u64::try_from(root.len()).unwrap());
    assert_eq!(retained.data(), Some(root.as_bytes()));
    assert_eq!(
        retained.sha256(),
        cadmpeg_ir::hash::sha256_hex(root.as_bytes())
    );
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
    assert_eq!(location.stream_name(), Some("Document.xml"));
    assert_eq!(location.offset, retained.offset());
    assert_eq!(location.byte_len(), Some(retained.byte_len()));
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
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
    assert!(crate::test_support::source_fidelity::given_up(decoded.source_fidelity()).is_empty());
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
        entries
            .iter()
            .map(crate::native::EntryRecord::name)
            .collect::<Vec<_>>(),
        ["Document.xml", "peer.bin"]
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = cadmpeg_container::ArchiveSnapshot::new(&ctx, view).unwrap();
    let expanded = source.open(&ctx, "Document.xml").unwrap();
    assert_eq!(expanded.window(), document);
    let parts = crate::test_support::source_fidelity::given_up(decoded.source_fidelity());
    let [(_, failed)] = parts.as_slice() else {
        panic!("one failed root")
    };
    assert_eq!(
        failed.offset(),
        u64::try_from(b"<!--before-->".len()).unwrap()
    );
    assert_eq!(
        source.open(&ctx, "peer.bin").unwrap().window(),
        b"independent peer"
    );
}
