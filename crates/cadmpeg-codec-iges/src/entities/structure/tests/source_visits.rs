// SPDX-License-Identifier: Apache-2.0

use super::super::attribute_definition_valid_and_shape;
use crate::directory::{DirectoryEntry, SourceStatus};
use crate::global::GlobalTable;
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn entry(form: i64) -> DirectoryEntry {
    DirectoryEntry {
        source_offset: 0, sequence: 1, entity_type: 322, parameter_start: 0,
        structure: 0, line_font: 0, level: 0, view: 0, transform: 0, label_display: 0,
        status: SourceStatus::from_codes([0, 0, 0, 0]), line_weight: 0, color: 0,
        parameter_line_count: 0, form, reserved: [[b' '; 8]; 2], label: [b' '; 8], subscript: 0,
    }
}

fn record(count: i64, fields: &[i64]) -> ParameterRecord {
    let mut values = vec![TokenValue::Integer(322), TokenValue::Omitted,
        TokenValue::Integer(1), TokenValue::Integer(count)];
    values.extend(fields.iter().copied().map(TokenValue::Integer));
    let parameter_end = values.len();
    let tokens = values.into_iter().map(|value| Token { value, span: 0..0 }).collect();
    ParameterRecord::from_test_tokens(1, 0..0, Vec::new(), parameter_end, tokens, Vec::new())
}

fn source_refusal(input: &ParameterRecord, form: i64, work: u64) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let directory = entry(form);
    let entries = BTreeMap::new();
    let Err(CodecError::ResourceLimit(first)) = attribute_definition_valid_and_shape(
        &directory, input, &entries, GlobalTable::V5Later, &ctx,
    ) else {
        panic!("expected attribute definition source refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges structure list traversal");
    assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
    for replay in [input, &record(0, &[])] {
        assert!(matches!(attribute_definition_valid_and_shape(
            &directory, replay, &entries, GlobalTable::V5Later, &ctx,
        ), Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn attribute_descriptor_source_refuses_one_visit_before_any_type_or_shape_allocation() {
    source_refusal(&record(3, &[-1, 7, 0, -1, 7, 0, -1, 7, 0]), 0, 0);
}

#[test]
fn attribute_value_source_refuses_one_visit_after_one_descriptor_without_admitting_the_tail() {
    // The invalid type prevents type-index storage; the three integer values
    // still belong to the supplied form1 descriptor and are checked in order.
    source_refusal(&record(1, &[-1, 1, 3, 7, 11, 19]), 1, 1);
}

#[test]
fn attribute_shape_allocation_refuses_after_one_descriptor_without_admitting_the_tail() {
    let input = record(3, &[-1, 1, 1, -1, 1, 1, -1, 1, 1]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let directory = entry(0);
    let entries = BTreeMap::new();
    let Err(CodecError::ResourceLimit(first)) = attribute_definition_valid_and_shape(
        &directory, &input, &entries, GlobalTable::V5Later, &ctx,
    ) else {
        panic!("expected the first actual attribute descriptor allocation to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "iges attribute shape descriptors");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for replay in [&input, &record(0, &[])] {
        assert!(matches!(attribute_definition_valid_and_shape(
            &directory, replay, &entries, GlobalTable::V5Later, &ctx,
        ), Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

fn empty_shape_acceptance(input: &ParameterRecord, form: i64, work: u64) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (valid, shape) = attribute_definition_valid_and_shape(
        &entry(form), input, &BTreeMap::new(), GlobalTable::V5Later, &ctx,
    ).unwrap();
    assert!(!valid);
    assert!(shape.descriptors.is_empty());
    drop(shape);
    ctx.finish_session().unwrap();
}

#[test]
fn attribute_descriptor_source_visits_all_rejected_descriptors_and_no_empty_end_probe() {
    empty_shape_acceptance(&record(3, &[-1, 7, 0, -1, 7, 0, -1, 7, 0]), 0, 3);
    empty_shape_acceptance(&record(0, &[]), 0, 0);
}

#[test]
fn attribute_value_source_visits_exactly_three_values_and_preserves_rejection() {
    empty_shape_acceptance(&record(1, &[-1, 1, 3, 7, 11, 19]), 1, 1 + 3);
    empty_shape_acceptance(&record(0, &[]), 1, 0);
}

#[derive(Clone, Copy)]
enum OwnershipSource { Records, Properties, Associations }

impl OwnershipSource {
    fn operation(self) -> &'static str {
        match self {
            Self::Records => "iges structure ownership records",
            Self::Properties => "iges structure property references",
            Self::Associations => "iges structure association references",
        }
    }

    fn prefix_work(self, count: usize) -> u64 {
        use cadmpeg_core::decode::u64_from_index;
        if matches!(self, Self::Records) { return u64_from_index(count); }
        let node = |value_size| u64_from_index(11 * (std::mem::size_of::<u32>() + value_size)
            + 16 * std::mem::size_of::<usize>() + 2 * std::mem::align_of::<usize>());
        let map_node = node(match self {
            Self::Properties => std::mem::size_of::<Vec<u32>>(),
            Self::Associations => std::mem::size_of::<std::collections::BTreeSet<u32>>(),
            Self::Records => unreachable!(),
        });
        let query_count = if matches!(self, Self::Properties) { 4 } else { 3 };
        (0..count).map(|index| {
            let comparisons = if index == 0 { 0 } else {
                u64_from_index(index).min(11 * u64::from(index.div_ceil(2).ilog(6) + 1))
            };
            // Actual next, keyed group queries, and the existing node movement
            // bound. New property Vec has no old backing to move. Each new
            // association target holds one owner in a previously empty set.
            1 + query_count * u64_from_index(std::mem::size_of::<u32>()) * comparisons
                + map_node * (1 + 2 * u64::from(index.is_multiple_of(5)))
                + if matches!(self, Self::Associations) { 3 * node(0) } else { 0 }
        }).sum()
    }

    fn prelude(self) -> u64 {
        if matches!(self, Self::Records) { 0 } else {
            1 + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<u32>())
        }
    }
}

fn ownership_inputs(source: OwnershipSource, count: usize) -> (
    Vec<ParameterRecord>, BTreeMap<u32, crate::parameter::TrailingPointerAnalysis>,
    crate::global::ProjectedGlobal,
) {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    if matches!(source, OwnershipSource::Records) {
        let (records, global) = crate::test_support::sequence_index::parameter_inputs(count);
        return (records, BTreeMap::new(), global);
    }
    let pointers = (0..count).map(|index| (2 * index + 3).to_string())
        .collect::<Vec<_>>().join(",");
    let groups = match source {
        OwnershipSource::Properties => format!("0,{count},{pointers}"),
        OwnershipSource::Associations => format!("{count},{pointers},0"),
        OwnershipSource::Records => unreachable!(),
    };
    let mut entities = vec![OwnedTestEntity {
        entity_type: 116, form: 0, label: "OWNER".into(), status: "00000000",
        parameters: format!("116,0,0,0,0,{groups};"),
    }];
    for _ in 0..count {
        entities.push(match source {
            OwnershipSource::Properties => OwnedTestEntity {
                entity_type: 406, form: 15, label: "PROPERTY".into(), status: "00010000",
                parameters: "406,1,4HNAME;".into(),
            },
            OwnershipSource::Associations => OwnedTestEntity {
                entity_type: 402, form: 14, label: "GROUP".into(), status: "00000000",
                parameters: "402,1,1;".into(),
            },
            OwnershipSource::Records => unreachable!(),
        });
    }
    let bytes = owned_test_file(&entities);
    crate::test_support::with_service_context(&bytes, |ctx| {
        let scan = crate::card::scan_with_context(&bytes, ctx).unwrap();
        let (global, _, _storage) = crate::global::parse(&scan, ctx).unwrap();
        let (directory, quarantined) = crate::directory::parse(&scan, global.global_table(), ctx).unwrap();
        assert!(quarantined.is_empty());
        let mut assembly = crate::parameter::assemble_with_context(
            &scan, &directory, &quarantined, &global, ctx,
        ).unwrap();
        assert!(assembly.quarantined.is_empty());
        let owner = assembly.records.iter().find(|record| record.directory_sequence == 1).unwrap().clone();
        let analysis = assembly.trailing_pointer_analysis.remove(&1).unwrap();
        let crate::parameter::TrailingPointerAnalysis::Unambiguous(ref groups) = analysis
            else { panic!("fixture requires actual resolved trailing groups"); };
        let expected: Vec<u32> = (0..count).map(|index| u32::try_from(2 * index + 3).unwrap()).collect();
        match source {
            OwnershipSource::Properties => {
                assert!(groups.associations().is_empty());
                assert_eq!(groups.properties(), expected);
            },
            OwnershipSource::Associations => {
                assert!(groups.properties().is_empty());
                assert_eq!(groups.associations(), expected);
            },
            OwnershipSource::Records => unreachable!(),
        }
        (vec![owner], BTreeMap::from([(1, analysis)]), global.length_context().unwrap())
    })
}

fn ownership_run(
    ir: &mut cadmpeg_ir::CadIr, records: &[ParameterRecord],
    analysis: &BTreeMap<u32, crate::parameter::TrailingPointerAnalysis>,
    global: &crate::global::ProjectedGlobal, ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let records = records.iter().map(|record| (record.directory_sequence, record)).collect();
    let (outcome, rejections) = super::super::project(ir, &[], (&BTreeMap::new(), &records),
        analysis, global, ctx, &mut super::super::super::geometry::SourceSequences::default())?;
    assert!(outcome.decoded.is_empty());
    assert!(outcome.losses.is_empty());
    assert!(rejections.is_empty());
    drop(outcome);
    Ok(())
}

fn ownership_boundaries(source: OwnershipSource) {
    for count in [1, 64] {
        let (records, analysis, global) = ownership_inputs(source, count);
        for (visited, accepts) in [(0, false), (count - 1, false), (count, true)] {
            let cap = source.prelude() + source.prefix_work(visited);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            if matches!(source, OwnershipSource::Records) {
                policy.limits.max_collection_items = 0;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_entities = 0;
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut ir = cadmpeg_ir::CadIr::empty();
            let expected = ir.model.clone();
            let result = ownership_run(&mut ir, &records, &analysis, &global, &ctx);
            if accepts {
                result.unwrap();
                ctx.finish_session().unwrap();
            } else {
                let Err(CodecError::ResourceLimit(first)) = result
                    else { panic!("expected actual ownership source refusal"); };
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(first.operation, source.operation());
                assert_eq!((first.used, first.additional, first.limit), (cap, 1, cap));
                for replay in [&records[..], &[]] {
                    assert!(matches!(ownership_run(&mut ir, replay, &analysis, &global, &ctx),
                        Err(CodecError::ResourceLimit(last)) if last == first));
                    assert_eq!(ir.model, expected);
                }
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
            assert_eq!(ir.model, expected);
        }
    }
}

#[test]
fn structure_ownership_record_index_admits_exact_first_last_and_complete_visits() {
    ownership_boundaries(OwnershipSource::Records);
}
#[test]
fn structure_property_owner_index_admits_exact_first_last_and_complete_visits() {
    ownership_boundaries(OwnershipSource::Properties);
}
#[test]
fn structure_association_owner_index_admits_exact_first_last_and_complete_visits() {
    ownership_boundaries(OwnershipSource::Associations);
}

#[test]
fn empty_structure_ownership_indexes_accept_fresh_zero_budgets() {
    let (records, analysis, global) = ownership_inputs(OwnershipSource::Records, 0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ownership_run(&mut ir, &records, &analysis, &global, &ctx).unwrap();
    assert_eq!(ir.model, cadmpeg_ir::CadIr::empty().model);
    ctx.finish_session().unwrap();
}
