// SPDX-License-Identifier: Apache-2.0
//! A bounded mandatory document failure preserves independently opened archive members.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions, LimitScope};
use cadmpeg_ir::source_fidelity::parts::PartState;
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
    for (case, document) in [
        b"<Document SchemaVersion='4'><Objects></Document>".as_slice(),
        b"<Document SchemaVersion='4'>\xff</Document>".as_slice(),
        b"<Document SchemaVersion='invalid' FileVersion='1'><Objects/><ObjectData/></Document>"
            .as_slice(),
        b"<Document SchemaVersion='4' FileVersion='1'><Objects/><Objects/><ObjectData/></Document>"
            .as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        let bytes = source(document);
        let decoded = crate::FcstdCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir().scope, cadmpeg_ir::ContentScope::full());
        assert_eq!(decoded.report().scope(), cadmpeg_ir::ContentScope::full());
        assert_eq!(decoded.ir().model.entity_count(), 0);
        let identity = decoded.ir().source.as_ref().unwrap();
        assert_eq!(identity.format(), "fcstd");
        let root_survives = matches!(case, 0 | 3);
        assert_eq!(identity.dialects().is_some(), root_survives);
        if let Some(dialects) = identity.dialects() {
            assert_eq!(dialects.primary().dialect().as_str(), "fcstd:schema-4");
        }
        let namespace = decoded.ir().native.namespace("fcstd").unwrap();
        let entries = namespace
            .arena_as::<crate::native::EntryRecord>("entries")
            .unwrap();
        let peers = entries
            .iter()
            .filter(|entry| entry.name() != "Document.xml")
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(peers, [expected_peer.clone()]);
        let document_entry = entries
            .iter()
            .find(|entry| entry.name() == "Document.xml")
            .unwrap();
        assert_eq!(document_entry, expected_document);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            namespace
                .arena_as::<crate::native::DocumentFacts>("document")
                .unwrap()
                .len(),
            usize::from(root_survives)
        );
        let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
        let [unknown] = unknowns.as_slice() else {
            panic!("one failed document")
        };
        let fidelity = decoded.source_fidelity();
        assert_eq!(
            fidelity.unrepresented(),
            cadmpeg_ir::ArenaClasses::from([
                cadmpeg_ir::ArenaClass::Model,
                cadmpeg_ir::ArenaClass::Design
            ])
        );
        let part = &fidelity.parts()[&unknown.id];
        let (expected_level, raw, offset) = match case {
            0 => (
                "freecad.objects",
                b"<Objects>".as_slice(),
                document
                    .windows(b"<Objects>".len())
                    .position(|part| part == b"<Objects>")
                    .unwrap(),
            ),
            1 => ("freecad.document", document, 0),
            2 => ("freecad.document", document, 0),
            3 => (
                "freecad.objects",
                b"<Objects/>".as_slice(),
                document
                    .windows(b"<Objects/>".len())
                    .rposition(|part| part == b"<Objects/>")
                    .unwrap(),
            ),
            _ => unreachable!(),
        };
        assert_eq!(part.level().as_str(), expected_level);
        assert_eq!(part.state(), &PartState::GivenUp {});
        assert_eq!(part.offset(), u64::try_from(offset).unwrap());
        assert_eq!(part.byte_len(), u64::try_from(raw.len()).unwrap());
        let parent = &fidelity.parts()[part.parent().unwrap()];
        let parent = if root_survives {
            assert_eq!(parent.level().as_str(), "freecad.document");
            assert_eq!(parent.state(), &PartState::Decoded {});
            &fidelity.parts()[parent.parent().unwrap()]
        } else {
            parent
        };
        let parent = if expected_level != "freecad.document_xml" {
            assert_eq!(parent.level().as_str(), "freecad.document_xml");
            assert_eq!(parent.state(), &PartState::Decoded {});
            &fidelity.parts()[parent.parent().unwrap()]
        } else {
            parent
        };
        assert_eq!(parent.level().as_str(), "zip.entry");
        assert_eq!(parent.state(), &PartState::Decoded {});
        let archive = &fidelity.parts()[parent.parent().unwrap()];
        assert_eq!(archive.level().as_str(), "zip.archive");
        assert_eq!(archive.state(), &PartState::Decoded {});
        assert_eq!(fidelity.parts().len(), [9, 8, 8, 11][case]);
        let retained = fidelity.retained_record(unknown.id.as_str()).unwrap();
        assert_eq!(retained.data(), Some(raw));
        assert_eq!(retained.stream(), part.stream());
        assert_eq!(retained.sha256(), cadmpeg_ir::hash::sha256_hex(raw));
        assert!(fidelity.record_layout()[unknown.id.as_str()]
            .reads()
            .is_empty());
        assert_eq!(
            fidelity
                .record_layout()
                .contains_key(&crate::native::native_id("document", "0")),
            root_survives
        );
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
            Some(part.stream())
        );
        assert_eq!(
            failed[0].provenance.as_ref().unwrap().byte_len(),
            Some(part.byte_len())
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
        assert_eq!(inspection.dialects(), identity.dialects());
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
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let archive = crate::source_archive::OriginalArchive::new(&ctx, root, fidelity).unwrap();
        assert_eq!(
            archive.open(&ctx, "peer.bin").unwrap().window(),
            b"readable peer"
        );
        if root_survives {
            let reopened = archive.open(&ctx, "Document.xml").unwrap();
            assert_eq!(
                reopened.child(offset, offset + raw.len()).unwrap().window(),
                raw
            );
        } else {
            assert_eq!(
                archive
                    .part(&ctx, unknown.id.as_str(), "Document.xml")
                    .unwrap(),
                raw
            );
        }
        let mut shifted = serde_json::to_value(fidelity).unwrap();
        for map in ["parts", "retained_records", "record_layout"] {
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
        assert!(crate::source_archive::OriginalArchive::new(&ctx, root, &shifted).is_err());
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
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one failed entry")
    };
    let part = &decoded.source_fidelity().parts()[&unknown.id];
    assert_eq!(part.level().as_str(), "zip.entry");
    assert_eq!(part.state(), &PartState::GivenUp {});
    assert_eq!(part.stream(), "file");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let extent = archive
        .entry("Document.xml")
        .unwrap()
        .stored_range()
        .unwrap();
    assert_eq!(part.offset(), extent.start);
    assert_eq!(part.byte_len(), extent.end - extent.start);
    assert_eq!(
        decoded
            .source_fidelity()
            .retained_record(unknown.id.as_str())
            .unwrap()
            .data(),
        Some(b"stored mandatory bytes".as_slice())
    );
    let entries = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| entry.name() == "peer.bin"));
    assert!(entries.iter().any(|entry| entry.name() == "Document.xml"));
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
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one real archive failure")
    };
    let fidelity = decoded.source_fidelity();
    assert_eq!(
        fidelity.unrepresented(),
        cadmpeg_ir::ArenaClasses::from([
            cadmpeg_ir::ArenaClass::Model,
            cadmpeg_ir::ArenaClass::Design
        ])
    );
    assert_eq!(fidelity.parts().len(), 3);
    for child in fidelity
        .parts()
        .values()
        .filter(|part| part.parent().is_some())
    {
        assert!(matches!(
            child.level().as_str(),
            "zip.local_header" | "zip.entry"
        ));
        assert_eq!(
            child.state(),
            &PartState::Covered {
                by: unknown.id.clone()
            }
        );
        assert_eq!(child.parent(), Some(&unknown.id));
    }
    let part = &fidelity.parts()[&unknown.id];
    assert_eq!(part.level().as_str(), "zip.archive");
    assert_eq!(part.offset(), 0);
    assert_eq!(part.byte_len(), u64::try_from(bytes.len()).unwrap());
    assert_eq!(part.state(), &PartState::GivenUp {});
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(
        fidelity
            .retained_record(unknown.id.as_str())
            .unwrap()
            .data(),
        Some(bytes.as_slice())
    );
    assert!(fidelity
        .retained_record(crate::fidelity::SOURCE_IMAGE)
        .is_none());
    assert!(decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap()
        .is_empty());
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
