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
    assert!(scan.unreadable_entries.is_empty());
    assert!(scan.losses.is_empty());
    let range = scan
        .archive
        .entry("broken.bin")
        .unwrap()
        .stored_range()
        .unwrap();
    let (model, native, fidelity, losses) = output.into_parts().into_parts();
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
    assert_eq!(parts.len(), 3);
    let (archive_id, archive_part) = parts
        .iter()
        .find(|(_, part)| part.level().as_str() == "zip.archive")
        .unwrap();
    assert_eq!(archive_part.offset(), 0);
    assert_eq!(archive_part.byte_len(), u64::try_from(bytes.len()).unwrap());
    assert_eq!(archive_part.state(), &PartState::Decoded {});
    assert!(archive_part.parent().is_none());
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
