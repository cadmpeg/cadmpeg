// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::directory::{DirectoryEntry, SourceStatus};
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

const SCRATCH_CAP_BYTES: u64 = 8 * 1024;

// The shared B-tree admission formula bounds 32 Type 322 type keys to seven
// 232-byte nodes. Its descriptor vector holds at most 32 `(i64, usize)` pairs
// (512 bytes at exact capacity). Three Type 316 unit types use one 320-byte
// B-tree node. Each one-record peak fits below this cap; repeating the record
// would exceed it if either temporary index stayed live across iterations.

fn record(values: Vec<TokenValue>, parameter_end: usize) -> ParameterRecord {
    let tokens = values
        .into_iter()
        .map(|value| Token { value, span: 0..0 })
        .collect();
    ParameterRecord::from_test_tokens(1, 0..0, Vec::new(), parameter_end, tokens, Vec::new())
}

fn directory_entry(entity_type: i64, form: i64) -> DirectoryEntry {
    DirectoryEntry {
        source_offset: 0,
        sequence: 1,
        entity_type,
        parameter_start: 0,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: SourceStatus::from_codes([0, 0, 0, 0]),
        line_weight: 0,
        color: 0,
        parameter_line_count: 0,
        form,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    }
}

fn bounded_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_CAP_BYTES;
    policy
}

#[test]
fn type316_unit_type_index_storage_is_released_per_record() {
    let record = record(
        vec![
            TokenValue::Integer(316),
            TokenValue::Integer(3),
            TokenValue::String(b"LENGTH".to_vec()),
            TokenValue::String(b"KN".to_vec()),
            TokenValue::real(1.0),
            TokenValue::String(b"MASS".to_vec()),
            TokenValue::String(b"KG".to_vec()),
            TokenValue::real(1.0),
            TokenValue::String(b"TIME".to_vec()),
            TokenValue::String(b"S".to_vec()),
            TokenValue::real(1.0),
        ],
        11,
    );
    let policy = bounded_policy();

    let result = crate::test_support::with_policy_context(&[], &policy, |ctx| {
        for _ in 0..64 {
            assert!(super::super::unit_values_valid(&record, ctx)?);
        }
        Ok::<_, CodecError>(())
    });

    result.unwrap();
}

#[test]
fn type322_descriptor_and_type_indexes_are_released_for_rejected_record() {
    let mut values = vec![
        TokenValue::Integer(322),
        TokenValue::Integer(7),
        TokenValue::Integer(0),
        TokenValue::Integer(32),
    ];
    for attribute_type in 100..132 {
        values.extend([
            TokenValue::Integer(attribute_type),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
        ]);
    }
    let record = record(values, 4 + 32 * 3);
    let entry = directory_entry(322, 0);
    let entries = BTreeMap::new();
    let policy = bounded_policy();

    let result = crate::test_support::with_policy_context(&[], &policy, |ctx| {
        for _ in 0..16 {
            let (definition_valid, shape) = super::super::attribute_definition_valid_and_shape(
                &entry,
                &record,
                &entries,
                crate::global::GlobalTable::V5Later,
                ctx,
            )?;
            assert!(!definition_valid);
            assert_eq!(shape.descriptors.len(), 32);
            drop(shape);
        }
        Ok::<_, CodecError>(())
    });

    result.unwrap();
}

const CANDIDATE_ITEMS: usize = 4096;
const REJECTED_CANDIDATES: usize = 16;

#[derive(Clone, Copy)]
enum RejectedCandidate {
    Flow,
    Assembly,
    SubfigureMember,
    SubfigureDepth,
    NetworkMember,
    NetworkDepth,
    NetworkPoint,
    InstanceDefinition,
    InstancePoint,
}

impl RejectedCandidate {
    fn input(self, sequence: u32) -> (DirectoryEntry, ParameterRecord, &'static str, u64, u64) {
        let count = i64::try_from(CANDIDATE_ITEMS).unwrap();
        let member = i64::try_from(std::mem::size_of::<u32>()).unwrap();
        let item = i64::try_from(std::mem::size_of::<(u32, u32)>()).unwrap();
        let point = i64::try_from(std::mem::size_of::<Option<u32>>()).unwrap();
        let (entity_type, form, mut values, reason, first, peak) = match self {
            Self::Flow => (402, 20, vec![402, 1, count, 0, 0, 0, 0, 0, 0],
                "flow class counts, flags, typed links, required back pointers, continuation tree, or directory status is invalid", member * count, member * count),
            Self::Assembly => (184, 0, vec![184, count],
                "solid-assembly item tuple is invalid", item * count, item * count),
            Self::SubfigureMember | Self::SubfigureDepth => (308, 0,
                vec![308, if matches!(self, Self::SubfigureDepth) { -1 } else { 0 }, 0, count],
                "subfigure depth, member count, or member pointer is invalid", member * count, member * count),
            Self::NetworkMember | Self::NetworkDepth => (320, 0,
                vec![320, if matches!(self, Self::NetworkDepth) { -1 } else { 0 }, 0, count],
                "network definition header or member list is invalid", member * count, member * count),
            Self::NetworkPoint => (320, 0, vec![320, 0, 0, 0, 0, 0, 0, count + 1],
                "network definition connect-point count is invalid", 4 * point, 3 * count * point / 2),
            Self::InstanceDefinition | Self::InstancePoint => (420, 0,
                vec![420, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, count + i64::from(matches!(self, Self::InstancePoint))],
                "network instance definition or count is invalid", 4 * point, 3 * count * point / 2),
        };
        let pointer = if matches!(self, Self::SubfigureDepth | Self::NetworkDepth) {
            i64::from(sequence)
        } else { 0 };
        values.extend(std::iter::repeat_n(pointer, CANDIDATE_ITEMS));
        if matches!(self, Self::Assembly) {
            values.extend(std::iter::repeat_n(0, CANDIDATE_ITEMS));
        }
        if matches!(self, Self::NetworkPoint | Self::InstancePoint) { values.push(2); }
        let mut entry = directory_entry(entity_type, form);
        entry.sequence = sequence;
        let parameter_end = values.len();
        let mut input = record(values.into_iter().map(TokenValue::Integer).collect(), parameter_end);
        input.directory_sequence = sequence;
        (entry, input, reason, u64::try_from(first).unwrap(), u64::try_from(peak).unwrap())
    }
}

fn assert_rejected_candidate_storage(candidate: RejectedCandidate) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, ResourceDimension};
    use cadmpeg_ir::report::loss::LossNote;
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    let fixtures: Vec<_> = (0..REJECTED_CANDIDATES).map(|i| {
        candidate.input(u32::try_from(2 * i + 1).unwrap())
    }).collect();
    let directory: Vec<_> = fixtures.iter().map(|(entry, _, _, _, _)| entry.clone()).collect();
    let entries = directory.iter().map(|entry| (entry.sequence, entry)).collect();
    let records = fixtures.iter().map(|(_, record, _, _, _)| (record.directory_sequence, record)).collect();
    let (_, _, reason, first, peak) = fixtures[0];
    // Exact list capacities, or the largest old/new overlap of doubling a
    // 4096-element optional-pointer vector, plus all surviving loss slots.
    // Sixteen losses use capacities 4, 8, 16; slot growth overlap is at most
    // 24 slots and is below this candidate-plus-16-slot bound.
    let slots = u64::try_from(REJECTED_CANDIDATES * std::mem::size_of::<LossNote>()).unwrap();
    assert!(peak > slots);
    type Rejection = super::super::PlacementRejection;
    let rejection_nodes = if matches!(candidate, RejectedCandidate::InstanceDefinition | RejectedCandidate::InstancePoint) {
        let nodes = (REJECTED_CANDIDATES - 1) / 5 + 1;
        u64::try_from(nodes * (11 * (std::mem::size_of::<u32>() + std::mem::size_of::<Rejection>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<u32>().max(std::mem::align_of::<Rejection>()).max(std::mem::align_of::<usize>()))).unwrap()
    } else { 0 };
    for cap in [first - 1, peak + slots + rejection_nodes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut sequences = super::super::super::geometry::SourceSequences::new(&ctx).unwrap();
        let _retained_map_probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::RetainedBytes, "iges placement rejection nodes", None);
        let result = super::super::project(&mut ir, &directory, (&entries, &records),
            &BTreeMap::new(), &global, &ctx, &mut sequences);
        if cap == first - 1 {
            let first_refusal = match result.err().expect("expected candidate refusal") {
                CodecError::ResourceLimit(first) => first,
                _ => panic!("expected the first candidate allocation to refuse"),
            };
            assert_eq!(first_refusal.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!((first_refusal.limit, first_refusal.used, first_refusal.additional), (cap, 0, first));
            drop(sequences);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first_refusal));
        } else {
            let (outcome, rejections, rejection_storage) = result.unwrap();
            assert!(outcome.decoded.is_empty());
            assert_eq!(outcome.losses.len(), REJECTED_CANDIDATES);
            for loss in &outcome.losses { assert!(loss.message.ends_with(reason)); }
            if matches!(candidate, RejectedCandidate::InstanceDefinition | RejectedCandidate::InstancePoint) {
                assert_eq!(rejections.len(), REJECTED_CANDIDATES);
                assert!(rejections.values().all(|value| *value == super::super::PlacementRejection::InvalidDefinition));
            } else { assert!(rejections.is_empty()); }
            assert_eq!(ir, cadmpeg_ir::CadIr::empty());
            // Loss slots and placement-rejection nodes remain live for their readers.
            let released = ctx.reserve_scoped(cap - slots - rejection_nodes, "test discarded structure candidate backing").unwrap();
            drop(released);
            drop(rejections);
            drop(rejection_storage);
            let released = ctx.reserve_scoped(cap - slots, "test discarded placement rejection backing").unwrap();
            drop(released);
            drop(outcome);
            drop(sequences);
            let released = ctx.reserve_scoped(cap, "test destroyed structure outcome backing").unwrap();
            drop(released);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn rejected_flow_candidates_release_pointer_storage_per_attempt() {
    assert_rejected_candidate_storage(RejectedCandidate::Flow);
}
#[test]
fn rejected_assembly_candidates_release_item_storage_per_attempt() {
    assert_rejected_candidate_storage(RejectedCandidate::Assembly);
}
#[test]
fn rejected_subfigure_members_release_storage_per_attempt() {
    assert_rejected_candidate_storage(RejectedCandidate::SubfigureMember);
}
#[test]
fn rejected_subfigure_depth_releases_valid_member_storage_per_attempt() {
    assert_rejected_candidate_storage(RejectedCandidate::SubfigureDepth);
}
#[test]
fn rejected_network_members_release_storage_per_attempt() {
    assert_rejected_candidate_storage(RejectedCandidate::NetworkMember);
}
#[test]
fn rejected_network_depth_releases_valid_member_storage_per_attempt() {
    assert_rejected_candidate_storage(RejectedCandidate::NetworkDepth);
}
#[test]
fn rejected_network_points_release_storage_per_attempt() {
    assert_rejected_candidate_storage(RejectedCandidate::NetworkPoint);
}
#[test]
fn rejected_instance_definition_releases_valid_point_storage_per_attempt() {
    assert_rejected_candidate_storage(RejectedCandidate::InstanceDefinition);
}
#[test]
fn rejected_instance_points_release_storage_per_attempt() {
    assert_rejected_candidate_storage(RejectedCandidate::InstancePoint);
}

fn sheet_identity_storage_refusal(at_table: bool, collection: bool) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, ResourceDimension};
    use cadmpeg_ir::report::loss::LossNote;
    use std::mem::{align_of, size_of};
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    let input = record(vec![TokenValue::Integer(406), TokenValue::Integer(2),
        TokenValue::Integer(2), TokenValue::String(b"C".to_vec())], 4);
    let directory = [directory_entry(406, 33)];
    let entries = BTreeMap::from([(1, &directory[0])]);
    let records = BTreeMap::from([(1, &input)]);
    // A singleton borrowed identity table has four SwissTable buckets,
    // alignment padding, four bucket controls and sixteen trailing controls.
    let table = u64::try_from(4 * size_of::<((i64, &[u8]), Option<u32>)>()
        + align_of::<((i64, &[u8]), Option<u32>)>().max(16) - 1 + 4 + 16).unwrap();
    let slots = if size_of::<LossNote>() <= 1024 { 4 } else { 1 };
    let loss_slots = u64::try_from(slots * size_of::<LossNote>()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    if collection {
        policy.limits.max_collection_items = 0;
    } else {
        policy.limits.max_materialized_bytes = table - u64::from(at_table);
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut sequences = super::super::super::geometry::SourceSequences::default();
    let result = super::super::project(&mut ir, &directory, (&entries, &records),
        &BTreeMap::new(), &global, &ctx, &mut sequences);
    let first = match result.as_ref() {
        Err(CodecError::ResourceLimit(first)) => *first,
        _ => panic!("sheet table or following loss slots must refuse"),
    };
    drop(result);
    if collection {
        assert_eq!(first.dimension, ResourceDimension::CollectionItems);
        assert_eq!(first.operation, "iges sheet identity index");
        assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    } else {
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        if at_table {
            assert_eq!(first.operation, "iges sheet identity index");
            assert_eq!((first.limit, first.used, first.additional), (table - 1, 0, table));
        } else {
            assert_eq!(first.operation, "iges entity loss slots");
            assert_eq!((first.limit, first.used, first.additional), (table, table, loss_slots));
        }
    }
    for _ in 0..64 {
        for replay in [&directory[..], &[]] {
            assert!(matches!(super::super::project(&mut ir, replay, (&entries, &records),
                &BTreeMap::new(), &global, &ctx, &mut sequences),
                Err(CodecError::ResourceLimit(last)) if last == first));
            assert_eq!(ir, cadmpeg_ir::CadIr::empty());
        }
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn sheet_identity_index_refuses_exact_first_table_storage_and_item() {
    sheet_identity_storage_refusal(true, false);
    sheet_identity_storage_refusal(true, true);
}

#[test]
fn sheet_identity_index_holds_only_table_backing_before_recovery() {
    sheet_identity_storage_refusal(false, false);
}

#[test]
fn sheet_identity_table_is_released_before_attribute_recovery() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, ResourceDimension};
    use cadmpeg_ir::report::loss::LossNote;
    use std::mem::{align_of, size_of};
    let bytes = crate::test_support::test_solids_and_structure::distinct_drawing_sheet_ids_file();
    let (mut directory, mut inputs, analysis, global) =
        crate::test_support::with_service_context(&bytes, |setup| {
            let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
            let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
            let (directory, quarantined) =
                crate::directory::parse(&scan, global.global_table(), setup).unwrap();
            assert!(quarantined.is_empty());
            let assembly = crate::parameter::assemble_with_context(
                &scan, &directory, &quarantined, &global, setup,
            ).unwrap();
            assert!(assembly.quarantined.is_empty());
            (directory, assembly.records, assembly.trailing_pointer_analysis,
                global.length_context().unwrap())
        });
    let mut attribute = directory_entry(322, 0);
    attribute.sequence = 13;
    directory.push(attribute);
    // The invalid type skips its type set. A valid descriptor still allocates
    // its temporary vector before the definition recovers as a loss.
    let mut attribute = record(vec![TokenValue::Integer(322), TokenValue::Omitted,
        TokenValue::Integer(0), TokenValue::Integer(1), TokenValue::Integer(-1),
        TokenValue::Integer(1), TokenValue::Integer(100)], 7);
    attribute.directory_sequence = 13;
    inputs.push(attribute);
    let entries = directory.iter().map(|entry| (entry.sequence, entry)).collect();
    let records = inputs.iter().map(|record| (record.directory_sequence, record)).collect();
    // Each ordered tree has one backing node: eleven key/value lanes,
    // sixteen pointer slots and two alignment paddings. Two group values
    // each contain one exact-capacity u32 vector.
    let owners = 11 * (size_of::<u32>() + size_of::<Vec<u32>>())
        + 16 * size_of::<usize>()
        + 2 * align_of::<u32>().max(align_of::<Vec<u32>>()).max(align_of::<usize>())
        + 2 * size_of::<u32>();
    let decoded = 11 * size_of::<u32>() + 16 * size_of::<usize>()
        + 2 * align_of::<u32>().max(align_of::<usize>());
    let table = 4 * size_of::<((i64, &[u8]), Option<u32>)>()
        + align_of::<((i64, &[u8]), Option<u32>)>().max(16) - 1 + 4 + 16;
    let slots = if size_of::<LossNote>() <= 1024 { 4 } else { 1 };
    let loss_slots = slots * size_of::<LossNote>();
    let live = u64::try_from(owners + decoded).unwrap();
    let peak = live + u64::try_from(table).unwrap();
    assert!(4 * size_of::<(i64, usize)>() <= table);
    assert!(live + u64::try_from(loss_slots).unwrap() > peak);
    for cap in [peak - 1, peak] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut sequences = super::super::super::geometry::SourceSequences::default();
        let result = super::super::project(&mut ir, &directory, (&entries, &records),
            &analysis, &global, &ctx, &mut sequences);
        let first = match result.as_ref() {
            Err(CodecError::ResourceLimit(first)) => *first,
            _ => panic!("decoded index or following attribute loss must refuse"),
        };
        drop(result);
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        if cap == peak {
            assert_eq!(first.operation, "iges entity loss slots");
            assert_eq!((first.limit, first.used, first.additional),
                (cap, live, u64::try_from(loss_slots).unwrap()));
        } else {
            assert_eq!(first.operation, "iges structure decoded sequences");
            assert_eq!((first.limit, first.used, first.additional),
                (cap, u64::try_from(owners + table).unwrap(), u64::try_from(decoded).unwrap()));
        }
        for _ in 0..64 {
            for replay in [&directory[..], &[]] {
                assert!(matches!(super::super::project(&mut ir, replay, (&entries, &records),
                    &analysis, &global, &ctx, &mut sequences),
                    Err(CodecError::ResourceLimit(last)) if last == first));
                assert_eq!(ir, cadmpeg_ir::CadIr::empty());
            }
        }
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
}
