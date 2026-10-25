// SPDX-License-Identifier: Apache-2.0
//! Whole-archive rollback only when a reached child has no provable extent.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use std::io::Cursor;

const DOCUMENT: &[u8] = b"<Document SchemaVersion='4' FileVersion='1' ProgramVersion='1.0'><Objects Count='1'><Object name='Box' type='Part::Box'/></Objects><ObjectData Count='1'><Object name='Box'><Properties Count='3'><Property name='Length' type='App::PropertyLength'><Float value='5'/></Property><Property name='Width' type='App::PropertyLength'><Float value='6'/></Property><Property name='Height' type='App::PropertyLength'><Float value='7'/></Property></Properties></Object></ObjectData></Document>";

fn original() -> Vec<u8> {
    zip_fixture::archive(&[
        RawEntry::stored("Document.xml", DOCUMENT),
        RawEntry::stored("peer.bin", b"readable peer"),
        RawEntry::stored("broken.bin", b"opaque source"),
    ])
}

fn damaged(mut bytes: Vec<u8>, lengths: bool) -> Vec<u8> {
    let header = {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        usize::try_from(
            cadmpeg_container::ArchiveSnapshot::new(&ctx, root)
                .unwrap()
                .entry("broken.bin")
                .unwrap()
                .header_start,
        )
        .unwrap()
    };
    if lengths {
        cadmpeg_test_support::bytes::put_u16(&mut bytes, header + 26, u16::MAX);
    } else {
        bytes[header] ^= 1;
    }
    bytes
}

#[test]
fn an_unframeable_optional_header_gives_up_its_archive_and_no_document_records_escape() {
    let baseline = crate::FcstdCodec
        .decode(&mut Cursor::new(original()), &DecodeOptions::default())
        .unwrap();
    assert!(!baseline.ir().model.features.is_empty());
    assert!(!baseline.ir().model.parameters.is_empty());
    let bytes = damaged(original(), true);
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
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    assert_eq!(
        native
            .arenas()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["unknowns"]
    );
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one root unknown: {unknowns:?}")
    };
    assert!(unknown.links.is_empty());
    let fidelity = decoded.source_fidelity();
    let part = &fidelity.parts()[&unknown.id];
    assert_eq!(part.level().as_str(), "zip.archive");
    assert_eq!(part.state(), &PartState::GivenUp {});
    assert!(part.parent().is_none());
    assert_eq!(part.offset(), 0);
    assert_eq!(part.byte_len(), u64::try_from(bytes.len()).unwrap());
    // The genuine peer header and payload ran before the unframeable header.
    // No Document callback may run before all prerequisite member headers.
    let level_counts = fidelity.parts().values().fold(
        std::collections::BTreeMap::<&str, usize>::new(),
        |mut counts, part| {
            *counts.entry(part.level().as_str()).or_default() += 1;
            counts
        },
    );
    assert_eq!(
        level_counts,
        std::collections::BTreeMap::from([
            ("zip.archive", 1),
            ("zip.local_header", 1),
            ("zip.entry", 1),
        ])
    );
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| matches!(
                part.level().as_str(),
                "zip.archive"
                    | "zip.local_header"
                    | "zip.entry"
                    | "freecad.document_xml"
                    | "freecad.document"
            ))
            .count(),
        3
    );
    for (id, child) in fidelity.parts().iter().filter(|(id, _)| *id != &unknown.id) {
        assert_ne!(id, &unknown.id);
        assert_eq!(
            child.state(),
            &PartState::Covered {
                by: unknown.id.clone()
            }
        );
        if child.level().as_str() == "freecad.document" {
            let parent = &fidelity.parts()[child.parent().unwrap()];
            assert_eq!(parent.level().as_str(), "freecad.document_xml");
            let member = &fidelity.parts()[parent.parent().unwrap()];
            assert_eq!(member.level().as_str(), "zip.entry");
            assert_eq!(member.parent(), Some(&unknown.id));
        } else if child.level().as_str() == "freecad.document_xml" {
            let parent = &fidelity.parts()[child.parent().unwrap()];
            assert_eq!(parent.level().as_str(), "zip.entry");
            assert_eq!(parent.parent(), Some(&unknown.id));
        } else if let Some(parent_level) = match child.level().as_str() {
            "freecad.objects" | "freecad.object_data_section" | "freecad.properties" => {
                Some("freecad.document")
            }
            "freecad.object_declaration" => Some("freecad.objects"),
            "freecad.object_data" => Some("freecad.object_data_section"),
            "freecad.property" => Some("freecad.properties"),
            "freecad.property_value" => Some("freecad.property"),
            _ => None,
        } {
            let parent = &fidelity.parts()[child.parent().unwrap()];
            assert_eq!(parent.level().as_str(), parent_level);
        } else {
            assert_eq!(child.parent(), Some(&unknown.id));
        }
    }
    assert_eq!(fidelity.record_layout().len(), 1);
    assert!(fidelity.record_layout()[unknown.id.as_str()]
        .reads()
        .is_empty());
    assert!(fidelity
        .retained_record(crate::fidelity::SOURCE_IMAGE)
        .is_none());
    let retained = fidelity.retained_record(unknown.id.as_str()).unwrap();
    assert_eq!(retained.data(), Some(bytes.as_slice()));
    assert_eq!(retained.stream(), part.stream());
    assert_eq!(retained.offset(), part.offset());
    assert_eq!(retained.byte_len(), part.byte_len());
    assert_eq!(retained.sha256(), cadmpeg_ir::hash::sha256_hex(&bytes));
    let wire = serde_json::to_value(fidelity).unwrap();
    assert_eq!(wire["retained_records"].as_object().unwrap().len(), 1);
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
    assert_eq!(
        fidelity.unrepresented(),
        cadmpeg_ir::ArenaClasses::from([
            cadmpeg_ir::ArenaClass::Model,
            cadmpeg_ir::ArenaClass::Design
        ])
    );
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
}

#[test]
fn a_provable_bad_signature_preserves_document_and_peer_while_inspection_keeps_the_shared_error() {
    let bytes = damaged(original(), false);
    let baseline = crate::FcstdCodec
        .decode(&mut Cursor::new(original()), &DecodeOptions::default())
        .unwrap();
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model, baseline.ir().model);
    let entries = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    assert!(entries.iter().any(|entry| entry.name() == "peer.bin"));
    assert!(entries.iter().any(|entry| entry.name() == "Document.xml"));
    assert!(!entries.iter().any(|entry| entry.name() == "broken.bin"));
    assert_eq!(
        decoded
            .source_fidelity()
            .retained_record(crate::fidelity::SOURCE_IMAGE)
            .unwrap()
            .data(),
        Some(bytes.as_slice())
    );
    let summary = crate::FcstdCodec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .unwrap();
    assert_eq!(
        summary
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["Document.xml", "peer.bin", "broken.bin"]
    );
    assert_eq!(
        summary
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
}

#[test]
fn whole_archive_inspection_keeps_actual_identity_entries_and_its_source_owned_error() {
    let bytes = damaged(original(), true);
    let summary = crate::FcstdCodec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .unwrap();
    let baseline = crate::FcstdCodec
        .inspect(&mut Cursor::new(original()), &InspectOptions::default())
        .unwrap();
    assert!(summary.dialects().is_none());
    assert!(baseline.dialects().is_some());
    assert_eq!(summary.entries.len(), baseline.entries.len());
    assert_eq!(summary.entries[..2], baseline.entries[..2]);
    let failed = &summary.entries[2];
    let original = &baseline.entries[2];
    assert_eq!(failed.name, original.name);
    assert_eq!(failed.role, original.role);
    assert_eq!(failed.storage, original.storage);
    for key in ["crc32", "header_offset", "central_header_offset"] {
        assert_eq!(failed.attributes[key], original.attributes[key]);
    }
    assert!(!failed.attributes.contains_key("data_offset"));
    assert_eq!(
        failed.attributes["local_header_error"],
        "ZIP local header lengths escape member area"
    );
    assert_eq!(
        summary
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    assert!(summary
        .notes
        .iter()
        .all(|note| !note.starts_with("object count=")));
}

#[test]
fn a_failed_root_owner_rejects_competing_images_and_mismatched_retained_extents() {
    let bytes = damaged(original(), true);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let unknown = &decoded.ir().native_unknowns("fcstd").unwrap()[0];
    let mut wire = serde_json::to_value(decoded.source_fidelity()).unwrap();
    let root = wire["retained_records"][unknown.id.as_str()].clone();
    wire["retained_records"][crate::fidelity::SOURCE_IMAGE] = root;
    let duplicate: cadmpeg_ir::SourceFidelity = serde_json::from_value(wire).unwrap();
    assert!(crate::source_archive::image(&duplicate).is_err());
    let mut wire = serde_json::to_value(decoded.source_fidelity()).unwrap();
    wire["retained_records"][unknown.id.as_str()]["offset"] = serde_json::json!(1);
    let mismatched: cadmpeg_ir::SourceFidelity = serde_json::from_value(wire).unwrap();
    assert!(crate::source_archive::image(&mismatched).is_err());
}

#[test]
fn the_document_source_capture_and_successful_peer_parts_rollback_with_the_archive() {
    let bytes = damaged(original(), true);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let crate::container::ScanOutcome::GivenUp { summary, parts, .. } =
        crate::container::attempt(&ctx, root).unwrap()
    else {
        panic!("unframeable header must give up archive")
    };
    assert_eq!(summary.entries.len(), 3);
    let (_, native, fidelity, losses) = parts.into_parts(&ctx).unwrap().into_parts();
    assert_eq!(
        native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
            .count(),
        1
    );
    assert_eq!(losses.len(), 1);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn the_public_nonzero_archive_decode_keeps_source_coordinates_for_bounded_failure_reopening() {
    let bytes = damaged(original(), false);
    let baseline = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let prefix = b"unrelated enclosing content";
    let mut embedded = prefix.to_vec();
    embedded.extend_from_slice(&bytes);
    embedded.extend_from_slice(b"unrelated enclosing suffix");
    let arena = DecodeArena::new();
    let (ctx, outer) =
        DecodeContext::from_root_bytes(&embedded, &arena, &DecodePolicy::service()).unwrap();
    let root = outer
        .child(prefix.len(), prefix.len() + bytes.len())
        .unwrap();
    let source = ctx.capture_part_extent(root).unwrap();
    let decoded = crate::FcstdCodec.decode_with_context(&ctx, root).unwrap();
    assert_eq!(decoded.ir().model, baseline.ir().model);
    let fidelity = decoded.source_fidelity();
    let retained = fidelity
        .retained_record(crate::fidelity::SOURCE_IMAGE)
        .unwrap();
    assert_eq!(retained.stream(), source.stream());
    assert_eq!(retained.offset(), u64::try_from(prefix.len()).unwrap());
    assert_eq!(retained.byte_len(), u64::try_from(bytes.len()).unwrap());
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
    let shift = u64::try_from(prefix.len()).unwrap();
    let expected_root = baseline.source_fidelity().b1_spans()[source.stream()]
        .spans()
        .iter()
        .map(|span| {
            let range = span.range();
            (range.start + shift..range.end + shift, span.classes())
        })
        .collect::<Vec<_>>();
    let actual_root = fidelity.b1_spans()[source.stream()]
        .spans()
        .iter()
        .map(|span| (span.range(), span.classes()))
        .collect::<Vec<_>>();
    assert_eq!(actual_root, expected_root);
    assert!(actual_root
        .iter()
        .all(|(range, _)| range.start >= source.offset()
            && range.end <= source.offset() + source.byte_len()));
    crate::fidelity::b1::assert_classified_spans(&decoded);
    let baseline_arena = DecodeArena::new();
    let (baseline_ctx, baseline_root) =
        DecodeContext::from_root_bytes(&bytes, &baseline_arena, &DecodePolicy::service()).unwrap();
    let before_archive =
        cadmpeg_container::ArchiveSnapshot::new(&baseline_ctx, baseline_root).unwrap();
    let after_arena = DecodeArena::new();
    let (after_ctx, after_outer) =
        DecodeContext::from_root_bytes(&embedded, &after_arena, &DecodePolicy::service()).unwrap();
    let after_root = after_outer
        .child(prefix.len(), prefix.len() + bytes.len())
        .unwrap();
    let after_archive = cadmpeg_container::ArchiveSnapshot::new(&after_ctx, after_root).unwrap();
    assert_eq!(
        fidelity.b1_spans().len(),
        baseline.source_fidelity().b1_spans().len()
    );
    for entry in before_archive.entries() {
        match (
            before_archive.open(&baseline_ctx, &entry.name),
            after_archive.open(&after_ctx, &entry.name),
        ) {
            (Ok(before_view), Ok(after_view)) => {
                assert_eq!(before_view.window(), after_view.window());
                let before_stream = baseline_ctx.capture_part_extent(before_view).unwrap();
                let after_stream = after_ctx.capture_part_extent(after_view).unwrap();
                assert_eq!(
                    baseline.source_fidelity().b1_spans()[before_stream.stream()].spans(),
                    fidelity.b1_spans()[after_stream.stream()].spans()
                );
            }
            (Err(_), Err(_)) => {}
            _ => panic!("actual bounded member availability changed: {}", entry.name),
        }
    }
    let (id, failed) = fidelity
        .parts()
        .iter()
        .find(|(_, part)| {
            part.level().as_str() == "zip.local_header" && part.state() == &PartState::GivenUp {}
        })
        .unwrap();
    let baseline_failed = baseline
        .source_fidelity()
        .parts()
        .values()
        .find(|part| {
            part.level().as_str() == "zip.local_header" && part.state() == &PartState::GivenUp {}
        })
        .unwrap();
    assert_eq!(failed.offset(), source.offset() + baseline_failed.offset());
    assert_eq!(failed.byte_len(), baseline_failed.byte_len());
    assert_eq!(
        fidelity.retained_record(id.as_str()).unwrap().data(),
        baseline
            .source_fidelity()
            .retained_records()
            .values()
            .find(|record| {
                record.offset() == baseline_failed.offset()
                    && record.byte_len() == baseline_failed.byte_len()
            })
            .unwrap()
            .data()
    );
    let source_arena = DecodeArena::new();
    let (source_ctx, outer) =
        DecodeContext::from_root_bytes(&embedded, &source_arena, &DecodePolicy::service()).unwrap();
    let source_root = outer
        .child(prefix.len(), prefix.len() + bytes.len())
        .unwrap();
    let original =
        crate::source_archive::OriginalArchive::new(&source_ctx, source_root, fidelity).unwrap();
    assert_eq!(
        original
            .part(&source_ctx, id.as_str(), "broken.bin")
            .unwrap(),
        fidelity
            .retained_record(id.as_str())
            .unwrap()
            .data()
            .unwrap()
    );
    assert_eq!(
        original.open(&source_ctx, "peer.bin").unwrap().window(),
        b"readable peer"
    );
    assert!(source_ctx.take_record_captures().unwrap().is_empty());
    let detached_arena = DecodeArena::new();
    let (detached_ctx, detached_root) =
        DecodeContext::from_root_bytes(&bytes, &detached_arena, &DecodePolicy::service()).unwrap();
    let error = crate::source_archive::OriginalArchive::new(&detached_ctx, detached_root, fidelity)
        .err()
        .unwrap();
    assert!(
        matches!(error, cadmpeg_core::CodecError::UnsupportedContent(ref message) if message == "original FCStd archive needs its actual source origin for layout reuse")
    );
}
