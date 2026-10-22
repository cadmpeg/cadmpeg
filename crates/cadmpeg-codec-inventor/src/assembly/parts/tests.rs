// SPDX-License-Identifier: Apache-2.0
//! Actual compressed AmDc bodies, recoverable damage, and exact peer survival.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::NativeUnknownRecord;

use crate::assembly::{inventory, tests::occurrence_fixture, OCCURRENCE_TYPE};
use crate::container::InventorContainer;
use crate::native::{AssemblyOccurrenceRecord, RseRecordRecord};
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
use crate::test_support::test_fixtures::assembly_envelope_fixture;

#[test]
fn damaged_occurrence_body_has_one_home_and_its_readable_peer_is_exact() {
    let peer = occurrence_fixture(42, &[0x8000_0008]);
    let bytes = assembly_envelope_fixture(&[
        (OCCURRENCE_TYPE, Vec::new()),
        (OCCURRENCE_TYPE, peer.clone()),
    ]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let container = InventorContainer::open(&ctx, root).unwrap();
    let segment = container
        .rse
        .segments
        .iter()
        .find(|s| s.kind == SegmentKind::AmDc)
        .unwrap();
    let SegmentBulkState::Framed(bulk) = &segment.bulk else {
        panic!("real framed bulk")
    };
    let RecordFrameState::Framed(table) = &bulk.records else {
        panic!("real framed records")
    };
    assert_eq!(table.records[0].payload.window(), b"".as_slice());
    assert_eq!(table.records[1].payload.window(), peer);
    let token = segment.pair.token.as_str();
    let failed_wrapper = format!("inventor:rse:record#{token}-0");
    let failed_occurrence = format!("inventor:assembly:occurrence#{token}-0");
    let good_wrapper = format!("inventor:rse:record#{token}-1");
    let good_occurrence = format!("inventor:assembly:occurrence#{token}-1");
    let failed_body = super::body(&ctx, bulk.expanded, &table.records[0]).unwrap();
    let failed_bytes = failed_body.window().to_vec();
    // Registry major18 legally has no extended record trailer.
    assert!(table
        .records
        .iter()
        .all(|record| record.trailer.window().is_empty()));
    let original_trailers = table
        .records
        .iter()
        .map(|r| r.trailer.window().to_vec())
        .collect::<Vec<_>>();
    let inventory = inventory(&ctx, &container.rse).unwrap();
    assert!(inventory.issues.is_empty());
    assert_eq!(inventory.occurrences.len(), 1);
    assert_eq!(inventory.occurrences[0].occurrence_id, 42);
    assert_eq!(inventory.occurrences[0].related_references, [0x8000_0008]);
    let (model, native, mut fidelity, losses) =
        inventory.parts.into_parts(&ctx).unwrap().into_parts();
    assert_eq!(model.entity_count(), 0);
    let namespace = native.namespace("inventor").unwrap();
    let occurrences = namespace
        .arena_as_for_decode::<AssemblyOccurrenceRecord>(&ctx, "assembly_occurrences")
        .unwrap();
    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].id, good_occurrence);
    assert_eq!(occurrences[0].occurrence_id, 42);
    assert_eq!(occurrences[0].related_references, [0x8000_0008]);
    let wrappers = namespace
        .arena_as_for_decode::<RseRecordRecord>(&ctx, "rse_records")
        .unwrap();
    assert_eq!(
        wrappers,
        [
            RseRecordRecord::from_frame(token, &table.records[0]).unwrap(),
            RseRecordRecord::from_frame(token, &table.records[1]).unwrap(),
        ]
    );
    assert_eq!(wrappers[0].id, failed_wrapper);
    assert_eq!(wrappers[1].id, good_wrapper);
    let unknowns = namespace
        .arena_as_for_decode::<NativeUnknownRecord>(&ctx, "unknowns")
        .unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(failed_bytes.as_slice())
    );
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp {}))
            .count(),
        1
    );
    assert_eq!(fidelity.parts().len(), 2);
    assert_eq!(losses.len(), 1);
    assert!(!losses[0].message.contains(&failed_wrapper));
    assert!(losses[0]
        .message
        .ends_with(&format!("dropped=[\"{failed_occurrence}\"]")));
    assert!(unknowns[0].links.is_empty());
    assert!(losses[0].message.contains(&failed_occurrence));
    assert!(!losses[0].message.contains(&good_wrapper));
    assert!(!losses[0].message.contains(&good_occurrence));
    assert_eq!(
        table
            .records
            .iter()
            .map(|r| r.trailer.window().to_vec())
            .collect::<Vec<_>>(),
        original_trailers
    );
    fidelity.install_mapped_record_captures(&ctx, Ok).unwrap();
    assert!(fidelity.record_layout().contains_key(&failed_wrapper));
    let wrapper_layout = &fidelity.record_layout()[&failed_wrapper];
    let failed_part = fidelity
        .parts()
        .values()
        .find(|part| matches!(part.state(), PartState::GivenUp {}))
        .unwrap();
    assert_ne!(wrapper_layout.stream(), failed_part.stream());
    assert!(!fidelity.record_layout().contains_key(&failed_occurrence));
    assert!(fidelity.record_layout().contains_key(&good_wrapper));
    assert!(fidelity.record_layout().contains_key(&good_occurrence));
}

#[test]
fn detached_or_foreign_payload_cannot_be_admitted_as_a_source_body() {
    let payload = occurrence_fixture(7, &[]);
    let bytes = assembly_envelope_fixture(&[(OCCURRENCE_TYPE, payload.clone())]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut container = InventorContainer::open(&ctx, root).unwrap();
    let segment = container
        .rse
        .segments
        .iter_mut()
        .find(|s| s.kind == SegmentKind::AmDc)
        .unwrap();
    let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
        panic!("real bulk")
    };
    let RecordFrameState::Framed(table) = &mut bulk.records else {
        panic!("real records")
    };
    table.records[0].payload = cadmpeg_core::decode::View::over_retained(&payload);
    assert!(super::body(&ctx, bulk.expanded, &table.records[0]).is_err());
    let foreign_arena = DecodeArena::new();
    let (_, foreign) =
        DecodeContext::from_root_bytes(&payload, &foreign_arena, &DecodePolicy::service()).unwrap();
    table.records[0].payload = foreign;
    assert!(super::body(&ctx, bulk.expanded, &table.records[0]).is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
