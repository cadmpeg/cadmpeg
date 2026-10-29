// SPDX-License-Identifier: Apache-2.0
//! A bounded mandatory document failure preserves independently opened archive members.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions, LimitScope};
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use std::io::Cursor;

const VALID: &[u8] = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='0'/><ObjectData Count='0'/></Document>";

fn source(document: &[u8]) -> Vec<u8> {
    zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document),
        RawEntry::stored("peer.bin", b"readable peer"),
    ])
}

#[test]
fn bounded_xml_utf8_and_schema_failures_preserve_the_same_successful_peer() {
    let baseline = crate::FcstdCodec
        .decode(&mut Cursor::new(source(VALID)), &DecodeOptions::default())
        .unwrap();
    let baseline_entries = baseline
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let expected_peer = baseline_entries
        .iter()
        .find(|entry| entry.name() == "peer.bin")
        .unwrap();
    let expected_document = baseline_entries
        .iter()
        .find(|entry| entry.name() == "Document.xml")
        .unwrap();
    for document in [
        b"<Document SchemaVersion='4'><Objects></Document>".as_slice(),
        b"<Document SchemaVersion='4'>\xff</Document>".as_slice(),
        b"<Document SchemaVersion='invalid' FileVersion='1'><Objects/><ObjectData/></Document>"
            .as_slice(),
        b"<Document SchemaVersion='4' FileVersion='1'><Objects/><Objects/><ObjectData/></Document>"
            .as_slice(),
    ] {
        let bytes = source(document);
        let decoded = crate::FcstdCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir().scope, cadmpeg_ir::ContentScope::full());
        assert_eq!(decoded.report().scope(), cadmpeg_ir::ContentScope::full());
        assert_eq!(decoded.ir().model.entity_count(), 0);
        let identity = decoded.ir().source.as_ref().unwrap();
        assert_eq!(identity.format(), "fcstd");
        assert!(identity.dialects().is_none());
        let namespace = decoded.ir().native.namespace("fcstd").unwrap();
        let entries = namespace
            .arena_as::<crate::native::EntryRecord>("entries")
            .unwrap();
        let peers = entries
            .iter()
            .filter(|entry| entry.name() != "Document.xml")
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(peers, std::slice::from_ref(expected_peer));
        let document_entry = entries
            .iter()
            .find(|entry| entry.name() == "Document.xml")
            .unwrap();
        assert_eq!(document_entry, expected_document);
        assert_eq!(entries.len(), 2);
        assert!(namespace
            .arena_as::<crate::native::DocumentFacts>("document")
            .unwrap()
            .is_empty());
        let fidelity = decoded.source_fidelity();
        let parts = crate::test_support::source_fidelity::given_up(fidelity);
        let [(_, retained)] = parts.as_slice() else {
            panic!("one failed document")
        };
        assert_eq!(
            fidelity.unrepresented(),
            cadmpeg_ir::ArenaClasses::from([
                cadmpeg_ir::ArenaClass::Model,
                cadmpeg_ir::ArenaClass::Design
            ])
        );
        assert_eq!(retained.data(), Some(document));
        assert_eq!(retained.offset(), 0);
        assert_eq!(retained.sha256(), cadmpeg_ir::hash::sha256_hex(document));
        assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
        let failed = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].severity, cadmpeg_ir::report::Severity::Error);
        assert_eq!(
            failed[0].provenance.as_ref().unwrap().stream_name(),
            Some("Document.xml")
        );
        assert_eq!(
            failed[0].provenance.as_ref().unwrap().byte_len(),
            Some(retained.byte_len())
        );
        assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
        let geometry = crate::FcstdCodec
            .decode(
                &mut Cursor::new(&bytes),
                &DecodeOptions {
                    depth: cadmpeg_core::decode::DecodeDepth::Geometry,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        assert_eq!(geometry.ir().scope, cadmpeg_ir::ContentScope::geometry());
        assert_eq!(
            geometry.report().scope(),
            cadmpeg_ir::ContentScope::geometry()
        );
        assert_eq!(geometry.ir().model.entity_count(), 0);
        assert_eq!(
            geometry.source_fidelity().unrepresented(),
            fidelity.unrepresented()
        );
        assert_eq!(
            crate::source_archive::image(geometry.source_fidelity()).unwrap(),
            bytes
        );
        let inspection = crate::FcstdCodec
            .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
            .unwrap();
        assert_eq!(inspection.format(), "fcstd");
        assert!(inspection.dialects().is_none());
        assert_eq!(
            inspection
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["Document.xml", "peer.bin"]
        );
        assert_eq!(
            inspection
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == "record.unreadable")
                .count(),
            1
        );
    }
}

#[test]
fn a_mandatory_member_decompression_failure_keeps_its_stored_extent_and_peer() {
    let bytes = zip_fixture::archive(&[
        RawEntry::compressed("Document.xml", 12, b"stored mandatory bytes", VALID),
        RawEntry::stored("peer.bin", b"readable peer"),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let parts = crate::test_support::source_fidelity::given_up(decoded.source_fidelity());
    let [(_, retained)] = parts.as_slice() else {
        panic!("one failed entry")
    };
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let extent = archive
        .entry("Document.xml")
        .unwrap()
        .stored_range()
        .unwrap();
    assert_eq!(retained.offset(), extent.start);
    assert_eq!(retained.byte_len(), extent.end - extent.start);
    assert_eq!(retained.data(), Some(b"stored mandatory bytes".as_slice()));
    let entries = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name(), "peer.bin");
    assert!(decoded.ir().source.as_ref().unwrap().dialects().is_none());
    assert_eq!(
        crate::source_archive::image(decoded.source_fidelity()).unwrap(),
        bytes
    );
}

#[test]
fn non_fcstd_zip_and_session_refusal_remain_hard_refusals() {
    let not_fcstd = zip_fixture::archive(&[RawEntry::stored("other.xml", VALID)]);
    let failure = crate::FcstdCodec
        .decode(&mut Cursor::new(not_fcstd), &DecodeOptions::default())
        .unwrap_err();
    assert!(matches!(
        failure,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::WrongFormat(_))
    ));
    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 0;
    let failure = crate::FcstdCodec
        .decode(
            &mut Cursor::new(source(b"<Document SchemaVersion='bad'/>")),
            &options,
        )
        .unwrap_err();
    assert!(
        matches!(failure, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Session)
    );
}

#[test]
fn an_unframeable_mandatory_header_gives_up_only_the_real_archive_parent() {
    let mut bytes = source(VALID);
    let offset = {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
        usize::try_from(archive.entry("Document.xml").unwrap().header_start).unwrap()
    };
    cadmpeg_test_support::bytes::put_u16(&mut bytes, offset + 26, u16::MAX);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.entity_count(), 0);
    assert_eq!(decoded.ir().scope, cadmpeg_ir::ContentScope::container());
    assert_eq!(
        decoded.report().scope(),
        cadmpeg_ir::ContentScope::container()
    );
    assert!(decoded.ir().source.as_ref().unwrap().dialects().is_none());
    let fidelity = decoded.source_fidelity();
    let parts = crate::test_support::source_fidelity::given_up(fidelity);
    let [(_, retained)] = parts.as_slice() else {
        panic!("one real archive failure")
    };
    assert_eq!(
        fidelity.unrepresented(),
        cadmpeg_ir::ArenaClasses::from([
            cadmpeg_ir::ArenaClass::Model,
            cadmpeg_ir::ArenaClass::Design
        ])
    );
    assert_eq!(retained.offset(), 0);
    assert_eq!(retained.byte_len(), u64::try_from(bytes.len()).unwrap());
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(retained.data(), Some(bytes.as_slice()));
    assert!(fidelity
        .retained_record(crate::fidelity::SOURCE_IMAGE)
        .is_none());
    assert!(decoded
        .ir()
        .native
        .namespace("fcstd")
        .is_none_or(|native| native
            .arena_as::<crate::native::EntryRecord>("entries")
            .unwrap()
            .is_empty()));
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
}
