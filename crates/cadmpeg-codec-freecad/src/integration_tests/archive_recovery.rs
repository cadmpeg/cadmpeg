// SPDX-License-Identifier: Apache-2.0
//! A member with no provable extent gives up only its own entry part.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
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
fn an_unframeable_optional_header_gives_up_only_its_entry_while_document_and_peer_survive() {
    let baseline = crate::FcstdCodec
        .decode(&mut Cursor::new(original()), &DecodeOptions::default())
        .unwrap();
    assert!(!baseline.ir().model.features.is_empty());
    assert!(!baseline.ir().model.parameters.is_empty());
    let bytes = damaged(original(), true);
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
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    // #215 §4.5: the unframeable trailing member gives up only its own
    // zip.entry part, from its header to the end of its stream. The
    // document and peer members still decode.
    assert_eq!(decoded.ir().model, baseline.ir().model);
    // #209 §1.1: the opaque entry bytes may hold either class, so both
    // stay in scope with the admitted Box design carriers.
    assert_eq!(decoded.ir().scope, cadmpeg_ir::ContentScope::full());
    assert_eq!(decoded.report().scope(), cadmpeg_ir::ContentScope::full());
    assert_eq!(
        decoded.ir().source.as_ref().unwrap().dialects(),
        baseline.ir().source.as_ref().unwrap().dialects()
    );
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
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
    let fidelity = decoded.source_fidelity();
    assert_eq!(
        fidelity
            .retained_record(crate::fidelity::SOURCE_IMAGE)
            .unwrap()
            .data(),
        Some(bytes.as_slice())
    );
    let parts = crate::test_support::source_fidelity::given_up(fidelity);
    let [(retained_id, retained)] = parts.as_slice() else {
        panic!("one entry part: {parts:?}")
    };
    // #215 rule 1: the given-up entry keeps one unknowns home.
    let namespace = decoded
        .ir()
        .native
        .namespace("fcstd")
        .expect("one unknowns home");
    let unknowns = &namespace.arenas()["unknowns"];
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].id(), *retained_id);
    assert_eq!(retained.data(), Some(&bytes[header..]));
    assert_eq!(retained.offset(), u64::try_from(header).unwrap());
    assert_eq!(
        retained.byte_len(),
        u64::try_from(bytes.len() - header).unwrap()
    );
    assert_eq!(
        retained.sha256(),
        cadmpeg_ir::hash::sha256_hex(&bytes[header..])
    );
    // The opaque unreferenced entry bytes may hold either class.
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
    assert_eq!(summary.dialects(), baseline.dialects());
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
    // The entry-only give-up leaves the admitted document census intact.
    assert!(summary
        .notes
        .iter()
        .any(|note| note.starts_with("object count=")));
}

#[test]
fn the_document_source_capture_and_successful_peer_parts_survive_the_entry_give_up() {
    let bytes = damaged(original(), true);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let crate::container::ScanOutcome::Readable { scan, parts } =
        crate::container::attempt(&ctx, root).unwrap()
    else {
        panic!("unframeable entry must not give up the archive")
    };
    assert_eq!(scan.entries.len(), 3);
    assert!(scan.failed_members.contains("broken.bin"));
    assert!(scan.data.contains_key("Document.xml"));
    assert!(scan.data.contains_key("peer.bin"));
    let (_, _, fidelity, losses) = parts.into_parts();
    assert_eq!(
        crate::test_support::source_fidelity::given_up(&fidelity).len(),
        1
    );
    assert_eq!(losses.len(), 1);
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
    let decoded = crate::FcstdCodec.decode_with_context(&ctx, root).unwrap();
    assert_eq!(decoded.ir().model, baseline.ir().model);
    let fidelity = decoded.source_fidelity();
    let retained = fidelity
        .retained_record(crate::fidelity::SOURCE_IMAGE)
        .unwrap();
    assert_eq!(retained.offset(), u64::try_from(prefix.len()).unwrap());
    assert_eq!(retained.byte_len(), u64::try_from(bytes.len()).unwrap());
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
    let failed = crate::test_support::source_fidelity::given_up(fidelity);
    let [(_, failed)] = failed.as_slice() else {
        panic!("one given-up header: {failed:?}")
    };
    let baseline_failed =
        crate::test_support::source_fidelity::given_up(baseline.source_fidelity());
    let [(_, baseline_failed)] = baseline_failed.as_slice() else {
        panic!("one given-up header: {baseline_failed:?}")
    };
    assert_eq!(
        failed.offset(),
        u64::try_from(prefix.len()).unwrap() + baseline_failed.offset()
    );
    assert_eq!(failed.byte_len(), baseline_failed.byte_len());
    assert_eq!(failed.data(), baseline_failed.data());
}
