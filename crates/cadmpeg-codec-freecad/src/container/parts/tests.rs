// SPDX-License-Identifier: Apache-2.0
//! Real bounded member failures and independent successful siblings.

use crate::test_support::test_archive::broken_optional_member_archive as archive;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, LimitScope, ResourceDimension,
};
use cadmpeg_ir::source_fidelity::parts::PartState;
use std::collections::BTreeSet;

#[test]
fn a_damaged_known_endpoint_member_has_one_shared_home_and_keeps_its_peer() {
    let bytes = archive();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let (scan, output) = crate::container::scan_for_decode(&ctx, root).unwrap();
    assert_eq!(scan.data["peer.bin"].window(), b"peer source");
    assert!(scan.data.contains_key("Document.xml"));
    assert!(!scan.data.contains_key("broken.bin"));
    assert_eq!(scan.failed_members, BTreeSet::from(["broken.bin".into()]));
    assert!(!crate::native::ARENAS.contains(&"unreadable_entries"));
    assert!(scan.losses.is_empty());
    let range = scan
        .archive
        .entry("broken.bin")
        .unwrap()
        .stored_range()
        .unwrap();
    let (model, native, fidelity, losses) = output
        .into_parts(&ctx)
        .expect("finalize parser dependencies")
        .into_parts();
    assert_eq!(model.entity_count(), 0);
    let unknowns = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one unknown: {unknowns:?}")
    };
    assert!(unknown.links.is_empty());
    let parts = fidelity.parts();
    assert_eq!(parts.len(), 5);
    let (archive_id, archive_part) = parts
        .iter()
        .find(|(_, part)| part.level().as_str() == "zip.archive")
        .unwrap();
    assert_eq!(archive_part.offset(), 0);
    assert_eq!(archive_part.byte_len(), u64::try_from(bytes.len()).unwrap());
    assert_eq!(archive_part.state(), &PartState::Decoded {});
    assert!(archive_part.parent().is_none());
    let headers = parts
        .values()
        .filter(|part| part.level().as_str() == "zip.local_header")
        .collect::<Vec<_>>();
    assert_eq!(headers.len(), 2);
    for name in ["broken.bin", "peer.bin"] {
        let entry = scan.archive.entry(name).unwrap();
        let header = headers
            .iter()
            .find(|part| part.offset() == entry.header_start)
            .unwrap();
        assert_eq!(header.state(), &PartState::Decoded {});
        assert_eq!(header.parent(), Some(archive_id));
        assert_eq!(
            header.byte_len(),
            entry.data_start.unwrap() - entry.header_start
        );
    }
    let failed = &parts[&unknown.id];
    assert_eq!(failed.level().as_str(), "zip.entry");
    assert_eq!(failed.state(), &PartState::GivenUp {});
    assert_eq!(failed.parent(), Some(archive_id));
    assert_eq!(failed.offset(), range.start);
    assert_eq!(
        failed.byte_len(),
        range.end.checked_sub(range.start).unwrap()
    );
    let peer_range = scan
        .archive
        .entry("peer.bin")
        .unwrap()
        .stored_range()
        .unwrap();
    let peer = parts
        .values()
        .find(|part| part.level().as_str() == "zip.entry" && part.offset() == peer_range.start)
        .unwrap();
    assert_eq!(peer.state(), &PartState::Decoded {});
    assert_eq!(peer.parent(), Some(archive_id));
    assert_eq!(
        peer.byte_len(),
        u64::try_from(b"peer source".len()).unwrap()
    );
    let layout = &fidelity.record_layout()[unknown.id.as_str()];
    assert_eq!(layout.stream(), failed.stream());
    assert_eq!(layout.offset(), range.start);
    assert_eq!(layout.byte_len(), 1);
    assert!(layout.reads().is_empty());
    let retained = fidelity.retained_record(unknown.id.as_str()).unwrap();
    assert_eq!(retained.data(), Some(b"\x07".as_slice()));
    assert_eq!(retained.offset(), range.start);
    assert_eq!(retained.stream(), failed.stream());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(loss.code.local_code(), "record.unreadable");
    assert_eq!(loss.provenance.as_ref().unwrap().offset, range.start);
    crate::test_support::test_archive::assert_only_document_file_version_capture(&ctx, &scan);
}

#[test]
fn a_session_entity_refusal_escapes_the_member_give_up_boundary() {
    let bytes = archive();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = crate::container::scan_for_decode(&ctx, root)
        .err()
        .expect("unknown stub entity admission must refuse");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.scope == LimitScope::Session && limit.dimension == ResourceDimension::Entities)
    );
}

#[test]
fn a_locatable_damaged_local_header_is_given_up_without_relabeling_its_payload() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let mut bytes = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='0'/><ObjectData Count='0'/></Document>"),
        RawEntry::stored("broken.bin", b"opaque source"),
        RawEntry::stored("peer.bin", b"peer source"),
    ]);
    let header_start = {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        cadmpeg_container::ArchiveSnapshot::new(&ctx, root)
            .unwrap()
            .entry("broken.bin")
            .unwrap()
            .header_start
    };
    let start = usize::try_from(header_start).unwrap();
    bytes[start] ^= 1;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (scan, output) = crate::container::scan_for_decode(&ctx, root).unwrap();
    assert!(scan.data.contains_key("Document.xml"));
    assert_eq!(scan.data["peer.bin"].window(), b"peer source");
    assert!(!scan.data.contains_key("broken.bin"));
    assert_eq!(scan.failed_members, BTreeSet::from(["broken.bin".into()]));
    assert!(!crate::native::ARENAS.contains(&"unreadable_entries"));
    assert!(scan.losses.is_empty());
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let unknowns = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one unknown: {unknowns:?}")
    };
    assert!(unknown.links.is_empty());
    let failed = &fidelity.parts()[&unknown.id];
    assert_eq!(failed.level().as_str(), "zip.local_header");
    assert_eq!(failed.state(), &PartState::GivenUp {});
    assert_eq!(failed.offset(), header_start);
    assert_eq!(
        failed.byte_len(),
        30 + u64::try_from("broken.bin".len()).unwrap()
    );
    assert_eq!(
        failed.framing_bytes(),
        &[
            header_start..header_start + 4,
            header_start + 6..header_start + 8,
            header_start + 18..header_start + 22,
            header_start + 26..header_start + 30,
        ]
    );
    let retained = fidelity.retained_record(unknown.id.as_str()).unwrap();
    assert_eq!(retained.stream(), failed.stream());
    assert_eq!(retained.offset(), header_start);
    assert_eq!(
        retained.data(),
        Some(&bytes[start..start + 30 + "broken.bin".len()])
    );
    assert!(fidelity.record_layout()[unknown.id.as_str()]
        .reads()
        .is_empty());
    assert_eq!(fidelity.parts().len(), 4);
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| part.level().as_str() == "zip.entry")
            .count(),
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
    assert_eq!(
        fidelity.parts()[failed.parent().unwrap()].level().as_str(),
        "zip.archive"
    );
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(loss.code.local_code(), "record.unreadable");
    assert_eq!(loss.provenance.as_ref().unwrap().offset, header_start);
    assert_eq!(
        loss.provenance.as_ref().unwrap().byte_len(),
        Some(failed.byte_len())
    );
    crate::test_support::test_archive::assert_only_document_file_version_capture(&ctx, &scan);
}

#[test]
fn a_given_up_payload_has_default_b1_classes_at_its_actual_physical_extent() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    let bytes = archive();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let physical = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let range = physical
        .entry("broken.bin")
        .unwrap()
        .stored_range()
        .unwrap();
    let source = root
        .child(
            usize::try_from(range.start).unwrap(),
            usize::try_from(range.end).unwrap(),
        )
        .unwrap();
    assert_eq!(source.window(), b"\x07");
    let decoded = crate::FcstdCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let fidelity = decoded.source_fidelity();
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one actual failed member")
    };
    assert!(unknown.links.is_empty());
    let part = &fidelity.parts()[&unknown.id];
    assert_eq!(part.level().as_str(), "zip.entry");
    assert_eq!(part.state(), &PartState::GivenUp {});
    assert_eq!(part.offset(), range.start);
    assert_eq!(part.byte_len(), range.end - range.start);
    let retained = fidelity.retained_record(unknown.id.as_str()).unwrap();
    assert_eq!(retained.stream(), part.stream());
    assert_eq!(retained.offset(), range.start);
    assert_eq!(retained.byte_len(), range.end - range.start);
    assert_eq!(retained.data(), Some(source.window()));
    assert_eq!(
        retained.sha256(),
        cadmpeg_ir::hash::sha256_hex(source.window())
    );
    let layout = &fidelity.record_layout()[unknown.id.as_str()];
    assert_eq!(layout.stream(), part.stream());
    assert_eq!(layout.offset(), range.start);
    assert_eq!(layout.byte_len(), range.end - range.start);
    assert!(layout.reads().is_empty());
    let classes = cadmpeg_ir::ArenaClasses::from([
        cadmpeg_ir::ArenaClass::Model,
        cadmpeg_ir::ArenaClass::Design,
    ]);
    let span = fidelity.b1_spans()[part.stream()]
        .spans()
        .iter()
        .find(|span| span.range().start <= range.start && span.range().end >= range.end)
        .expect("the genuine given-up extent is B1");
    assert_eq!(span.classes(), classes);
    assert_eq!(retained.untyped(), &classes);
    assert_eq!(fidelity.unrepresented(), classes);
    let losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    let provenance = losses[0].provenance.as_ref().unwrap();
    assert_eq!(provenance.stream_name(), Some(part.stream()));
    assert_eq!(provenance.offset, range.start);
}
