// SPDX-License-Identifier: Apache-2.0
//! Actual compressed `AmDc` bodies, recoverable damage, and exact peer survival.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartOutput;

use crate::assembly::{inventory, tests::occurrence_fixture, OCCURRENCE_TYPE};
use crate::container::InventorContainer;
use crate::native::{AssemblyOccurrenceRecord, RseRecordRecord};
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
use crate::test_support::given_up_records;
use crate::test_support::test_fixtures::assembly_envelope_fixture;

#[test]
fn damaged_occurrence_body_has_one_home_and_its_readable_peer_is_exact() {
    let peer = occurrence_fixture(42, &[0x8000_0008]);
    let bytes = assembly_envelope_fixture(&[
        (OCCURRENCE_TYPE, Vec::new()),
        (OCCURRENCE_TYPE, peer.clone()),
    ]);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let mut container =
        InventorContainer::open(&ctx, root).expect("the fixture opens as an inventor container");
    let segment = container
        .rse
        .segments
        .iter()
        .find(|s| s.kind == SegmentKind::AmDc)
        .expect("the container holds an am dc segment");
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
    let failed_body = crate::records::source_admission::body(bulk.expanded, &table.records[0])
        .expect("the first record frame holds a body");
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
    let parts = std::mem::replace(
        &mut container.rse.source_parts,
        PartOutput::new(cadmpeg_ir::CodecFormat::Inventor),
    );
    let inventory = inventory(&ctx, &container.rse, parts).expect("the assembly inventory decodes");
    assert!(!inventory
        .parts
        .native()
        .namespace("inventor")
        .expect("the inventor namespace exists")
        .arenas()
        .contains_key("assembly_record_issues"));
    assert_eq!(inventory.occurrences.len(), 1);
    assert_eq!(inventory.occurrences[0].occurrence_id, 42);
    assert_eq!(inventory.occurrences[0].related_references, [0x8000_0008]);
    let (model, native, fidelity, losses) = inventory.parts.into_parts();
    assert_eq!(model.entity_count(), 0);
    let namespace = native
        .namespace("inventor")
        .expect("the inventor namespace exists");
    let occurrences = namespace
        .arena_as_for_decode::<AssemblyOccurrenceRecord>(&ctx, "assembly_occurrences")
        .expect("the occurrences arena holds occurrence records");
    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].id, good_occurrence);
    assert!(!occurrences
        .iter()
        .any(|record| record.id == failed_occurrence));
    assert_eq!(occurrences[0].occurrence_id, 42);
    assert_eq!(occurrences[0].related_references, [0x8000_0008]);
    let wrappers = namespace
        .arena_as_for_decode::<RseRecordRecord>(&ctx, "rse_records")
        .expect("the rse records arena holds rse records");
    assert_eq!(
        wrappers,
        [
            RseRecordRecord::from_frame(token, &table.records[0])
                .expect("the first record frame forms a wrapper"),
            RseRecordRecord::from_frame(token, &table.records[1])
                .expect("the second record frame forms a wrapper"),
        ]
    );
    assert_eq!(wrappers[0].id, failed_wrapper);
    assert_eq!(wrappers[1].id, good_wrapper);
    // #215 rule 1: the given-up part keeps one unknowns home.
    let unknowns = namespace
        .arenas()
        .get("unknowns")
        .expect("one unknowns home");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        unknowns[0].id(),
        fidelity
            .retained_records()
            .keys()
            .next()
            .expect("one retained part")
            .as_str()
    );
    let given_up = given_up_records(&fidelity);
    assert_eq!(given_up.len(), 1);
    assert_eq!(given_up[0].data(), Some(failed_bytes.as_slice()));
    assert!(!given_up[0].untyped().is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert!(!losses[0].message.contains(&failed_wrapper));
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
}

#[test]
fn detached_or_foreign_payload_cannot_be_admitted_as_a_source_body() {
    let payload = occurrence_fixture(7, &[]);
    let bytes = assembly_envelope_fixture(&[(OCCURRENCE_TYPE, payload.clone())]);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let mut container =
        InventorContainer::open(&ctx, root).expect("the fixture opens as an inventor container");
    let segment = container
        .rse
        .segments
        .iter_mut()
        .find(|s| s.kind == SegmentKind::AmDc)
        .expect("the container holds an am dc segment");
    let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
        panic!("real bulk")
    };
    let RecordFrameState::Framed(table) = &mut bulk.records else {
        panic!("real records")
    };
    table.records[0].payload = cadmpeg_core::decode::View::over_retained(&payload);
    assert!(crate::records::source_admission::body(bulk.expanded, &table.records[0]).is_err());
    let foreign_arena = DecodeArena::new();
    let (_, foreign) =
        DecodeContext::from_root_bytes(&payload, &foreign_arena, &DecodePolicy::service())
            .expect("the payload fits the service policy");
    table.records[0].payload = foreign;
    assert!(crate::records::source_admission::body(bulk.expanded, &table.records[0]).is_err());
}

#[test]
fn placement_parse_and_conversion_failures_share_the_helper_and_keep_the_exact_peer() {
    use crate::assembly::{tests::placement_fixture, PLACEMENT_TYPE_B9, PLACEMENT_TYPE_CA};
    use crate::native::{AssemblyPlacementRecord, AssemblyPlacementRecordWire};
    use crate::test_support::test_fixtures::assembly_graphics_envelope_fixture;

    let mut empty_suffix = placement_fixture(7, false, 0x8421, 0x7bde, &[]);
    empty_suffix.truncate(empty_suffix.len() - 48);
    let mut nonfinite = vec![0; 15];
    nonfinite.extend_from_slice(&0_u16.to_le_bytes());
    nonfinite.extend_from_slice(&0_u16.to_le_bytes());
    nonfinite.extend_from_slice(&f64::INFINITY.to_le_bytes());
    let peer = placement_fixture(8, true, 0x8124, 0x7657, &[1.0, 2.0, 3.0]);
    let bytes = assembly_graphics_envelope_fixture(&[
        (PLACEMENT_TYPE_CA, empty_suffix.clone()),
        (PLACEMENT_TYPE_B9, nonfinite.clone()),
        (PLACEMENT_TYPE_CA, peer.clone()),
    ]);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let mut container =
        InventorContainer::open(&ctx, root).expect("the fixture opens as an inventor container");
    let segment = container
        .rse
        .segments
        .iter()
        .find(|s| s.kind == SegmentKind::AmGraphics)
        .expect("the container holds an am graphics segment");
    let token = segment.pair.token.as_str();
    let SegmentBulkState::Framed(bulk) = &segment.bulk else {
        panic!("actual framed bulk")
    };
    let RecordFrameState::Framed(table) = &bulk.records else {
        panic!("actual framed records")
    };
    assert_eq!(table.records[0].payload.window(), empty_suffix);
    assert_eq!(table.records[1].payload.window(), nonfinite);
    assert_eq!(table.records[2].payload.window(), peer);
    let source_bodies = table
        .records
        .iter()
        .map(|record| {
            crate::records::source_admission::body(bulk.expanded, record)
                .expect("each record frame holds a body")
                .window()
                .to_vec()
        })
        .collect::<Vec<_>>();
    let parts = std::mem::replace(
        &mut container.rse.source_parts,
        PartOutput::new(cadmpeg_ir::CodecFormat::Inventor),
    );
    let inventory = inventory(&ctx, &container.rse, parts).expect("the assembly inventory decodes");
    assert!(inventory.occurrences.is_empty());
    assert_eq!(inventory.placements.len(), 1);
    let expected = AssemblyPlacementRecord::try_from(AssemblyPlacementRecordWire::from_placement(
        &inventory.placements[0],
    ))
    .expect("the placement converts to a record");
    assert_eq!(expected.occurrence_id, 8);
    let (_, native, fidelity, losses) = inventory.parts.into_parts();
    let namespace = native
        .namespace("inventor")
        .expect("the inventor namespace exists");
    assert!(!namespace.arenas().contains_key("assembly_record_issues"));
    let placements = namespace
        .arena_as_for_decode::<AssemblyPlacementRecord>(&ctx, "assembly_placements")
        .expect("the placements arena holds placement records");
    assert_eq!(placements, [expected]);
    assert_eq!(
        placements[0].id,
        format!("inventor:assembly:placement#{token}-2")
    );
    // Row-major mask bits encode -1 at (0,2), +1 at (1,1)
    // and (2,0), with explicit translations at (0,3), (1,3), (2,3).
    assert_eq!(placements[0].transform.masks(), (0x8124, 0x7657));
    assert_eq!(
        placements[0].transform.rows(),
        [
            [0.0, 0.0, -1.0, 1.0],
            [0.0, 1.0, 0.0, 2.0],
            [1.0, 0.0, 0.0, 3.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
    let wrappers = namespace
        .arena_as_for_decode::<RseRecordRecord>(&ctx, "rse_records")
        .expect("the rse records arena holds rse records");
    assert_eq!(
        wrappers,
        table
            .records
            .iter()
            .map(|record| RseRecordRecord::from_frame(token, record)
                .expect("each record frame forms a wrapper"))
            .collect::<Vec<_>>()
    );
    assert_eq!(given_up_records(&fidelity).len(), 2);
    assert_eq!(losses.len(), 2);
    for loss in &losses {
        assert_eq!(loss.code.to_string(), "decode/record.unreadable");
        assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Error);
    }
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("suffix_len")));
    assert!(losses.iter().any(|loss| loss.message.contains("finite")));
    // #215 rule 1: each given-up part keeps one unknowns home.
    let unknowns = namespace
        .arenas()
        .get("unknowns")
        .expect("one unknowns home");
    assert_eq!(unknowns.len(), 2);
    let mut stubbed: Vec<_> = unknowns
        .iter()
        .map(cadmpeg_ir::native::NativeRecord::id)
        .collect();
    stubbed.sort_unstable();
    let mut retained: Vec<_> = fidelity
        .retained_records()
        .keys()
        .map(cadmpeg_ir::ids::UnknownId::as_str)
        .collect();
    retained.sort_unstable();
    assert_eq!(stubbed, retained);
    let retained = given_up_records(&fidelity)
        .iter()
        .map(|record| {
            record
                .data()
                .expect("each given-up record retains its data")
                .to_vec()
        })
        .collect::<Vec<_>>();
    assert!(retained.contains(&source_bodies[0]));
    assert!(retained.contains(&source_bodies[1]));
}
