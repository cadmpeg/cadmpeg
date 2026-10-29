// SPDX-License-Identifier: Apache-2.0
//! Real bounded member failures and independent successful siblings.

use crate::test_support::test_archive::broken_optional_member_archive as archive;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, LimitScope, ResourceDimension,
};
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
    let range = cadmpeg_container::ArchiveSnapshot::new(&ctx, root)
        .unwrap()
        .entry("broken.bin")
        .unwrap()
        .stored_range()
        .unwrap();
    let (model, _native, fidelity, losses) = output.into_parts();
    assert_eq!(model.entity_count(), 0);
    let parts = crate::test_support::source_fidelity::given_up(&fidelity);
    let [(_, retained)] = parts.as_slice() else {
        panic!("one given-up part: {parts:?}")
    };
    assert_eq!(retained.data(), Some(b"\x07".as_slice()));
    assert_eq!(retained.offset(), range.start);
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(loss.code.local_code(), "record.unreadable");
    assert_eq!(loss.provenance.as_ref().unwrap().offset, range.start);
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
    let (_, _, fidelity, losses) = output.into_parts();
    let parts = crate::test_support::source_fidelity::given_up(&fidelity);
    let [(_, retained)] = parts.as_slice() else {
        panic!("one given-up part: {parts:?}")
    };
    assert_eq!(retained.offset(), header_start);
    assert_eq!(
        retained.byte_len(),
        30 + u64::try_from("broken.bin".len()).unwrap()
    );
    assert_eq!(
        retained.data(),
        Some(&bytes[start..start + 30 + "broken.bin".len()])
    );
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(loss.code.local_code(), "record.unreadable");
    assert_eq!(loss.provenance.as_ref().unwrap().offset, header_start);
    assert_eq!(
        loss.provenance.as_ref().unwrap().byte_len(),
        Some(retained.byte_len())
    );
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
    let parts = crate::test_support::source_fidelity::given_up(fidelity);
    let [(_, retained)] = parts.as_slice() else {
        panic!("one actual failed member")
    };
    assert_eq!(retained.offset(), range.start);
    assert_eq!(retained.byte_len(), range.end - range.start);
    assert_eq!(retained.data(), Some(source.window()));
    assert_eq!(
        retained.sha256(),
        cadmpeg_ir::hash::sha256_hex(source.window())
    );
    let classes = cadmpeg_ir::ArenaClasses::from([
        cadmpeg_ir::ArenaClass::Model,
        cadmpeg_ir::ArenaClass::Design,
    ]);
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
    assert_eq!(provenance.offset, range.start);
}
