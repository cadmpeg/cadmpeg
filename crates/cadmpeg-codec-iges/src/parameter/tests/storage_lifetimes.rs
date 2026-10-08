// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::mem::{align_of, size_of};

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::card::Section;
use crate::global::GlobalTable;
use crate::parameter::{ParameterRecord, QuarantinedParameterRecord, Token, TokenValue, TrailingPointerAnalysis};
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::test_support::{directory_target, with_service_context};

// Core's ordered storage bound has eleven key/value lanes, sixteen metadata
// words and padding for both lane arrays. Each tested analysis map has one node.
fn one_node<K, V>() -> usize {
    11 * (size_of::<K>() + size_of::<V>()) + 16 * size_of::<usize>()
        + 2 * align_of::<K>().max(align_of::<V>()).max(align_of::<usize>())
}

fn point_parameters(parameters: &str) -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00000000",
        parameters: parameters.into(),
    }])
}

fn assert_live_storage(ctx: &DecodeContext<'_>, expected: usize) -> cadmpeg_core::decode::ResourceLimit {
    let CodecError::ResourceLimit(first) = ctx.reserve_scoped(
        u64::MAX, "test live parameter source storage",
    ).unwrap_err() else {
        panic!("expected materialized storage refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "test live parameter source storage");
    assert_eq!(first.used, u64_from_index(expected));
    assert_eq!(first.additional, u64::MAX);
    assert!(matches!(ctx.reserve_scoped(0, "test original parameter refusal"),
        Err(CodecError::ResourceLimit(actual)) if actual == first
    ));
    first
}

#[test]
fn assembled_sources_hold_only_returned_storage_and_release_it() {
    for (text, quarantine) in [
        ("116,4Habcd,1D0,3,0;", false),
        ("110,1,2,3,0;", true),
        ("116,1,2x3,3,0;", true),
    ] {
        let bytes = point_parameters(text);
        with_service_context(&bytes, |setup| {
            let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
            let (global, _, _global_storage) = crate::global::parse(&scan, setup).unwrap();
            let (directory, quarantined_directory) = crate::directory::parse(
                &scan, global.global_table(), setup,
            ).unwrap();
            for release in [false, true] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                // Native persistence has not run; source records retain no session bytes.
                policy.limits.max_retained_bytes = 0;
                // Tighten the policy to the materialized input allowance's base.
                // Its complete effective allowance must be reusable after Drop.
                policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
                let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
                let assembly = crate::parameter::assemble_with_context(
                    &scan, &directory, &quarantined_directory, &global, &ctx,
                ).unwrap();
                let mut live = assembly.records.capacity() * size_of::<ParameterRecord>()
                    + assembly.quarantined.capacity() * size_of::<QuarantinedParameterRecord>();
                if quarantine {
                    assert!(assembly.records.is_empty());
                    assert_eq!(assembly.quarantined.len(), 1);
                    assert!(assembly.trailing_pointer_analysis.is_empty());
                    assert_eq!(assembly.quarantined[0].bytes().len(), crate::card::CARD_WIDTH);
                    live += crate::card::CARD_WIDTH;
                } else {
                    assert!(assembly.quarantined.is_empty());
                    assert_eq!(assembly.records.len(), 1);
                    let record = &assembly.records[0];
                    assert_eq!(record.directory_sequence, 1);
                    assert_eq!(record.line_range, 1..2);
                    assert_eq!(record.tokens().len(), 5);
                    assert_eq!(record.tokens()[1].value, TokenValue::String(b"abcd".to_vec()));
                    assert_eq!(record.tokens()[2].value, TokenValue::real(1.0));
                    assert_eq!(record.double_precision_reals, [1 << 2]);
                    assert_eq!(assembly.trailing_pointer_analysis.len(), 1);
                    assert!(matches!(assembly.trailing_pointer_analysis[&1],
                        TrailingPointerAnalysis::Ambiguous { candidates: 0, valid: 0 }
                    ));
                    live += record.bytes.capacity() + record.comment.capacity()
                        + record.tokens.capacity() * size_of::<Token>()
                        + record.double_precision_reals.capacity() * size_of::<u64>()
                        + b"abcd".len()
                        + one_node::<u32, TrailingPointerAnalysis>();
                }
                if release {
                    drop(assembly);
                    // The complete allowance is reusable. No ownership index, card
                    // boundary, token scratch or returned source backing remains live.
                    let capacity = ctx.reserve_scoped(
                        policy.limits.max_materialized_bytes,
                        "test released parameter source storage",
                    ).unwrap();
                    drop(capacity);
                    ctx.finish_session().unwrap();
                } else {
                    let first = assert_live_storage(&ctx, live);
                    drop(assembly);
                    assert!(matches!(ctx.finish_session(),
                        Err(CodecError::ResourceLimit(actual)) if actual == first
                    ));
                }
            }
        });
    }
}

#[test]
fn resolved_ownership_releases_phase_indexes_before_return() {
    let bytes = point_parameters("116,1,2,3,0;");
    with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (directory, _) = crate::directory::parse(&scan, GlobalTable::V5Later, setup).unwrap();
        let lines = crate::parameter::ParameterCards::new(scan.section(Section::Parameter), setup).unwrap();
        for release in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let ownership = crate::parameter::resolve_ownership(
                &directory, &lines, &mut crate::card::FramingRecoveries::default(), &ctx,
            ).unwrap();
            assert_eq!(ownership.records.len(), 1);
            assert_eq!(ownership.records[0].entry.sequence, 1);
            assert_eq!(ownership.records[0].cards, [1]);
            assert_eq!(ownership.records[0].quarantine, None);
            let live = ownership.records.capacity() * size_of::<crate::parameter::Ownership<'_>>()
                + ownership.records[0].cards.capacity() * size_of::<u32>();
            if release {
                drop(ownership);
                let capacity = ctx.reserve_scoped(
                    policy.limits.max_materialized_bytes, "test released ownership storage",
                ).unwrap();
                drop(capacity);
                ctx.finish_session().unwrap();
            } else {
                let first = assert_live_storage(&ctx, live);
                drop(ownership);
                assert!(matches!(ctx.finish_session(),
                    Err(CodecError::ResourceLimit(actual)) if actual == first
                ));
            }
        }
    });
}

#[test]
fn pointer_analysis_keeps_only_selected_output_storage() {
    let owner = directory_target(1, 102);
    let record = super::integer_parameter_record(1, &[102, 1, 3, 1, 5, 0]);
    for target_type in [212, 116] {
        let target = directory_target(5, target_type);
        let directory = std::collections::BTreeMap::from([(1, &owner), (5, &target)]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut storage = ctx.reserve_scoped(0, "test selected pointer output").unwrap();
        let analysis = storage.with_storage(||
            crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context(
                &record, &directory, GlobalTable::V5Later, &ctx,
            )
        ).unwrap();
        let live = match &analysis {
            TrailingPointerAnalysis::Unambiguous(groups) => {
                assert_eq!(target_type, 212);
                assert_eq!(groups.associations(), [5]);
                assert!(groups.properties().is_empty());
                groups.associations.capacity() * size_of::<u32>()
            }
            TrailingPointerAnalysis::SingleInvalid(groups) => {
                assert_eq!(target_type, 116);
                assert_eq!(groups.association_pointers.len(), 1);
                assert_eq!(groups.association_pointers[0].raw_pointer, 5);
                assert_eq!(groups.association_pointers[0].resolved, None);
                groups.association_pointers.capacity() * size_of::<crate::parameter::TrailingPointer>()
            }
            other => panic!("unexpected analysis: {other:?}"),
        };
        let first = assert_live_storage(&ctx, live);
        drop(analysis);
        drop(storage);
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(actual)) if actual == first
        ));
    }
}

#[test]
fn source_byte_and_boundary_allocations_refuse_at_their_actual_bounds() {
    let bytes = point_parameters("116,1,2,3,0;");
    with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let lines = crate::parameter::ParameterCards::new(scan.section(Section::Parameter), setup).unwrap();
        // One card supplies 64 source bytes and one boundary offset.
        let complete = 64 + size_of::<usize>();
        for cap in [63, 64, complete] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = u64_from_index(cap);
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let mut storage = ctx.reserve_scoped(0, "test parameter source bytes").unwrap();
            let result = storage.with_storage(|| crate::parameter::owned_bytes(&[1], &lines, &ctx));
            if cap == complete {
                let owned = result.unwrap();
                assert_eq!(owned.bytes.len(), 64);
                assert_eq!(owned.card_boundaries, [64]);
                let first = assert_live_storage(&ctx, complete);
                drop(owned);
                drop(storage);
                assert!(matches!(ctx.finish_session(),
                    Err(CodecError::ResourceLimit(actual)) if actual == first
                ));
            } else {
                let first = match result.map(|_| ()) {
                    Err(CodecError::ResourceLimit(first)) => first,
                    _ => panic!("expected source storage refusal"),
                };
                assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(first.limit, u64_from_index(cap));
                let (operation, used, additional) = if cap == 63 {
                    ("iges owned parameter bytes", 0, 64)
                } else {
                    ("iges parameter card boundaries", 64, size_of::<usize>())
                };
                assert_eq!(first.operation, operation);
                assert_eq!(first.used, u64_from_index(used));
                assert_eq!(first.additional, u64_from_index(additional));
                drop(storage);
                assert!(matches!(ctx.finish_session(),
                    Err(CodecError::ResourceLimit(actual)) if actual == first
                ));
            }
        }
    });
}

#[test]
fn quarantine_source_copy_refuses_before_its_eighty_byte_allocation() {
    let bytes = point_parameters("116,1,2,3,0;");
    with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (directory, _) = crate::directory::parse(&scan, GlobalTable::V5Later, setup).unwrap();
        let lines = crate::parameter::ParameterCards::new(scan.section(Section::Parameter), setup).unwrap();
        for cap in [79, 80] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let mut storage = ctx.reserve_scoped(0, "test quarantined source").unwrap();
            let result = storage.with_storage(|| crate::parameter::quarantine(
                &directory[0], &[1], &lines, crate::parameter::ParameterDefect::NoOwnedCards, None, &ctx,
            ));
            if cap == 80 {
                let quarantine = result.unwrap();
                assert_eq!(quarantine.sequence, 1);
                assert_eq!(quarantine.cards(), 1);
                assert_eq!(quarantine.bytes(), lines.line(1).unwrap().payload);
                let first = assert_live_storage(&ctx, 80);
                drop(quarantine);
                drop(storage);
                assert!(matches!(ctx.finish_session(),
                    Err(CodecError::ResourceLimit(actual)) if actual == first
                ));
            } else {
                let first = match result {
                    Err(CodecError::ResourceLimit(first)) => first,
                    _ => panic!("expected quarantine byte refusal"),
                };
                assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(first.operation, "iges quarantined parameter bytes");
                assert_eq!(first.used, 0);
                assert_eq!(first.additional, 80);
                drop(storage);
                assert!(matches!(ctx.finish_session(),
                    Err(CodecError::ResourceLimit(actual)) if actual == first
                ));
            }
        }
    });
}

#[test]
fn ownership_index_refuses_before_first_named_owner_node() {
    let bytes = point_parameters("116,1,2,3,0;");
    with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (directory, _) = crate::directory::parse(&scan, GlobalTable::V5Later, setup).unwrap();
        let lines = crate::parameter::ParameterCards::new(scan.section(Section::Parameter), setup).unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The typed-owner set and four candidate slots precede the first
        // named-owner node. Its one-card group vector is not allocated yet.
        let used = one_node::<u32, ()>() + 4 * size_of::<&crate::directory::DirectoryEntry>();
        let additional = one_node::<u32, Vec<u32>>();
        policy.limits.max_materialized_bytes = u64_from_index(used + additional - 1);
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let first = match crate::parameter::resolve_ownership(
            &directory, &lines, &mut crate::card::FramingRecoveries::default(), &ctx,
        ) {
            Err(CodecError::ResourceLimit(first)) => first,
            _ => panic!("expected named-owner node refusal"),
        };
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(first.operation, "iges named parameter owners");
        assert_eq!(first.used, u64_from_index(used));
        assert_eq!(first.additional, u64_from_index(additional));
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(actual)) if actual == first
        ));
    });
}

#[test]
fn quarantine_loss_retains_only_final_text_and_preserves_zero_range_rejection() {
    use crate::parameter::{ParameterDefect, QuarantinedCards};

    for (ownership, message) in [
        (QuarantinedCards::Owned { range: 2..3, bytes: Vec::new(), first_offset: 160 },
            "IGES Parameter Data of D3 (P2 through P2) is quarantined because no Parameter Data card is owned; its 0 raw card(s) are retained and no token was interpreted"),
        (QuarantinedCards::None { directory_offset: 160 },
            "IGES Parameter Data of D3 (no owned Parameter Data card) is quarantined because no Parameter Data card is owned; its 0 raw card(s) are retained and no token was interpreted"),
    ] {
        let record = QuarantinedParameterRecord {
            sequence: 3, ownership, failing_offset: None, defect: ParameterDefect::NoOwnedCards,
        };
        let tag = "D3:parameter";
        let code = crate::loss::IgesLossCode::ParameterDataQuarantined;
        // Only final message, tag, qualified kind and source-format bytes survive.
        let retained = u64_from_index(message.len() + tag.len() + 4 + code.code().len() + 4);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = retained;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let loss = record.loss_note(&ctx).unwrap();
        assert_eq!(loss.message, message);
        assert_eq!(loss.code, code.kind());
        assert_eq!(loss.provenance.as_ref().unwrap().tag.as_deref(), Some(tag));
        assert_eq!(loss.provenance.as_ref().unwrap().offset, 160);
        let CodecError::ResourceLimit(first) = ctx.charge_retained(
            1, "test complete quarantine loss retained text",
        ).unwrap_err() else { panic!("expected retained text refusal"); };
        assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(first.used, retained);
        assert_eq!(first.additional, 1);
        drop(loss);
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(actual)) if actual == first
        ));
    }

    let record = QuarantinedParameterRecord {
        sequence: 3,
        ownership: QuarantinedCards::Owned { range: 0..0, bytes: Vec::new(), first_offset: 0 },
        failing_offset: None, defect: ParameterDefect::NoOwnedCards,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(record.loss_note(&ctx), Err(CodecError::Malformed(message))
        if message == "IGES owned Parameter Data range ends at zero"));
    let CodecError::ResourceLimit(first) = ctx.charge_work(
        1, "test prior malformed-range refusal",
    ).unwrap_err() else { panic!("expected work refusal"); };
    assert!(matches!(record.loss_note(&ctx),
        Err(CodecError::ResourceLimit(actual)) if actual == first
    ));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == first
    ));
}
