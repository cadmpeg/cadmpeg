// SPDX-License-Identifier: Apache-2.0

use super::super::{CoordinateEquationIndex, SectionAxis, SectionCoordinateEquation};
use crate::decode::sketch::equations_scalar::SectionEquationAuxiliaryConstraints;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};
use std::mem::{align_of, size_of};

const EMPTY_ROOT_MATERIALIZED_BYTES: u64 = 16 * 1024 * 1024;

#[test]
fn coordinate_index_keys_admit_present_terms_and_stop_at_first_nan() {
    let mut nan_terms = BTreeMap::from([((0, SectionAxis::U), f64::NAN)]);
    for point in 1..=512 {
        nan_terms.insert((point, SectionAxis::U), 1.0);
    }
    for (terms, visits, accepted) in [
        (BTreeMap::new(), 0, true),
        (BTreeMap::from([((1, SectionAxis::U), 1.0)]), 1, true),
        (
            BTreeMap::from([((1, SectionAxis::U), 1.0), ((2, SectionAxis::V), 2.0)]),
            2,
            true,
        ),
        (nan_terms, 1, false),
    ] {
        let equation = SectionCoordinateEquation { terms, rhs: 2.0 };
        crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = CoordinateEquationIndex::key(&ctx, &equation);
            let refused = result.is_err();
            if refused {
                let original = ctx.resource_refusal().expect("present term refusal");
                assert_eq!(
                    (
                        original.dimension,
                        original.used,
                        original.additional,
                        original.operation
                    ),
                    (
                        ResourceDimension::WorkUnits,
                        cap,
                        1,
                        "creo coordinate equation index terms"
                    )
                );
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                assert!(matches!(CoordinateEquationIndex::key(&ctx, &equation),
                    Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                let key = result.expect("exact term visits");
                assert_eq!(key.is_some(), accepted);
                if let Some(key) = key {
                    assert_eq!(key.0.len(), usize::try_from(visits).expect("two terms"));
                    for (actual, (variable, coefficient)) in key.0.iter().zip(&equation.terms) {
                        assert_eq!(*actual, (*variable, coefficient.to_bits()));
                    }
                }
                let original = ctx
                    .charge_work_limit(1, "after coordinate key")
                    .expect_err("all term work consumed");
                assert_eq!((original.used, original.additional), (visits, 1));
                assert!(matches!(CoordinateEquationIndex::key(&ctx, &equation),
                    Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
            if refused {
                Err(ctx.resource_refusal().expect("present term refusal").into())
            } else {
                Ok(())
            }
        });
    }
}

#[test]
fn nonfinite_coordinate_index_lookup_is_free_and_preserves_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let index = CoordinateEquationIndex::from_equations(&ctx, &[]).expect("empty index");
    let equation = SectionCoordinateEquation {
        terms: BTreeMap::new(),
        rhs: f64::INFINITY,
    };
    assert!(!index
        .contains(&ctx, &equation)
        .expect("nonfinite rhs is free"));
    let original = ctx
        .charge_work_limit(1, "before nonfinite index lookup")
        .expect_err("zero work");
    assert!(matches!(index.contains(&ctx, &equation),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(index.terms.is_empty());
}

#[test]
fn nonfinite_coordinate_index_insert_is_free_and_preserves_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut index = CoordinateEquationIndex::from_equations(&ctx, &[]).expect("empty index");
    let equation = SectionCoordinateEquation {
        terms: BTreeMap::new(),
        rhs: f64::INFINITY,
    };
    index
        .insert(&ctx, &equation)
        .expect("nonfinite rhs is free");
    let original = ctx
        .charge_work_limit(1, "before nonfinite index insert")
        .expect_err("zero work");
    assert!(matches!(index.insert(&ctx, &equation),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(index.terms.is_empty());
}

#[test]
fn absent_saved_coordinate_segments_are_free_and_preserve_original_refusal() {
    let mut definition = super::incomplete_segment_definition();
    definition.segments = None;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let run = || {
        super::super::saved_section_coordinate_witnesses(&ctx, &definition, &BTreeSet::default())
    };
    assert!(run().expect("absent segment table is free").is_empty());
    let original = ctx
        .charge_work_limit(1, "before absent coordinate segments")
        .expect_err("zero work");
    assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn coordinate_index_keeps_only_surviving_key_rhs_and_node_storage() {
    type Term = ((u32, SectionAxis), u64);
    type Key = Vec<Term>;
    type Values = Vec<f64>;
    let alignment = align_of::<Key>()
        .max(align_of::<Values>())
        .max(align_of::<usize>());
    let node =
        11 * (size_of::<Key>() + size_of::<Values>()) + 16 * size_of::<usize>() + 2 * alignment;
    // One node, one four-slot key buffer and one four-slot rhs buffer survive.
    // A duplicate key is discarded; its rhs fits the same admitted buffer.
    let live_bytes = u64::try_from(node + 4 * size_of::<Term>() + 4 * size_of::<f64>())
        .expect("index storage bound");
    let equation = |rhs| SectionCoordinateEquation {
        terms: BTreeMap::from([((7, SectionAxis::U), 1.0)]),
        rhs,
    };
    for rows in [vec![equation(1.0)], vec![equation(1.0), equation(2.0)]] {
        for nested in [false, true] {
            for overflow in [false, true] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_materialized_bytes = EMPTY_ROOT_MATERIALIZED_BYTES;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let parent_bytes = if nested { 37 } else { 0 };
                let mut parent = ctx
                    .reserve_scoped(parent_bytes, "coordinate index parent")
                    .expect("parent storage");
                let run = || {
                    let index = CoordinateEquationIndex::from_equations(&ctx, &rows)?;
                    assert_eq!(index.terms.len(), 1);
                    assert_eq!(
                        index.terms.values().next().expect("one key").len(),
                        rows.len()
                    );
                    for row in &rows {
                        assert!(index.contains(&ctx, row)?);
                    }
                    let available = EMPTY_ROOT_MATERIALIZED_BYTES - live_bytes - parent_bytes;
                    let probe = ctx.reserve_scoped(
                        available + u64::from(overflow),
                        "coordinate index live storage",
                    )?;
                    drop(probe);
                    drop(index);
                    drop(ctx.reserve_scoped(
                        EMPTY_ROOT_MATERIALIZED_BYTES - parent_bytes,
                        "coordinate index cleanup",
                    )?);
                    Ok::<(), CodecError>(())
                };
                let result = if nested {
                    parent.with_storage(run)
                } else {
                    run()
                };
                drop(parent);
                if overflow {
                    let original = ctx
                        .resource_refusal()
                        .expect("one byte above actual index backing");
                    assert!(
                        matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                    assert_eq!(
                        (
                            original.dimension,
                            original.used,
                            original.additional,
                            original.operation
                        ),
                        (
                            ResourceDimension::MaterializedBytes,
                            live_bytes + parent_bytes,
                            EMPTY_ROOT_MATERIALIZED_BYTES - live_bytes - parent_bytes + 1,
                            "coordinate index live storage"
                        )
                    );
                    assert_eq!(
                        ctx.charge_work_limit(1, "after refused index"),
                        Err(original)
                    );
                } else {
                    result.expect("exact backing without receipt-vector allocation");
                    drop(
                        ctx.reserve_scoped(
                            EMPTY_ROOT_MATERIALIZED_BYTES,
                            "coordinate parent cleanup",
                        )
                        .expect("all temporary storage released"),
                    );
                    let original = ctx
                        .charge_retained_limit(1, "coordinate index retained cleanup")
                        .expect_err("index is entirely scoped");
                    assert_eq!((original.used, original.additional), (0, 1));
                }
            }
        }
    }
}

#[test]
fn saved_coordinate_search_has_only_present_witness_boundaries() {
    let mut definition = super::incomplete_segment_definition();
    definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| rows[0].vertical_horizontal = Some(0));
    let segments = definition
        .segments
        .as_ref()
        .expect("segments")
        .rows
        .ordinary()
        .collect::<Vec<_>>();
    for (witnesses, visits) in [(Vec::new(), 0), (vec![(1, [0.0, 0.0]), (2, [0.0, 1.0])], 4)] {
        let observed = std::cell::RefCell::new(std::collections::BTreeSet::new());
        let actual = crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            &[],
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let run = || {
                    super::super::section_linear_distance_coordinate(
                        &ctx,
                        &definition,
                        &segments,
                        [1, 2],
                        &BTreeMap::new(),
                        &witnesses,
                        &BTreeSet::default(),
                    )
                };
                let result = run();
                if let Err(CodecError::ResourceLimit(original)) = &result {
                    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                    if original.operation == "creo saved coordinate witnesses" {
                        assert_eq!(original.additional, 1);
                        observed.borrow_mut().insert(original.used);
                    }
                    assert!(
                        matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == *original)
                    );
                }
                result
            },
        );
        assert_eq!(actual, Some(SectionAxis::V));
        // Each of the two endpoint checks consumes the two actual witnesses.
        assert_eq!(observed.borrow().len(), visits);
    }
}

#[test]
fn point_on_line_candidate_transfer_keeps_only_the_actual_equation_backing() {
    let coordinates = BTreeMap::from([
        (1, [Some(0.0), Some(0.0)]),
        (2, [Some(1.0), Some(0.0)]),
        (3, [Some(0.0), None]),
    ]);
    let alignment = align_of::<(u32, SectionAxis)>()
        .max(align_of::<f64>())
        .max(align_of::<usize>());
    let terms_node = 11 * (size_of::<(u32, SectionAxis)>() + size_of::<f64>())
        + 16 * size_of::<usize>()
        + 2 * alignment;
    let terms_bytes = u64::try_from(terms_node).expect("one equation node");
    let vector_bytes =
        u64::try_from(4 * size_of::<SectionCoordinateEquation>()).expect("four slots");
    let live_bytes = terms_bytes + vector_bytes;
    for constraints in [vec![(3, 1, 2)], vec![(3, 1, 2), (3, 1, 2)]] {
        crate::test_support::assert_refusal_order(
            ResourceDimension::RetainedBytes,
            &[
                "creo point-on-line candidate scratch",
                "creo section coordinate equations",
            ],
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let mut equations = Vec::new();
                let result = super::super::append_point_on_line_equations(
                    &ctx,
                    &constraints,
                    &coordinates,
                    &mut equations,
                );
                let refused = result.is_err();
                if refused {
                    let original = ctx
                        .resource_refusal()
                        .expect("actual output storage refusal");
                    let (used, additional, operation) = if cap < terms_bytes {
                        (0, terms_bytes, "creo point-on-line candidate scratch")
                    } else {
                        (
                            terms_bytes,
                            vector_bytes,
                            "creo section coordinate equations",
                        )
                    };
                    assert_eq!(
                        (
                            original.dimension,
                            original.used,
                            original.additional,
                            original.operation
                        ),
                        (
                            ResourceDimension::RetainedBytes,
                            used,
                            additional,
                            operation
                        )
                    );
                    assert!(
                        matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                    assert!(equations.is_empty());
                    assert!(
                        matches!(super::super::append_point_on_line_equations(&ctx, &constraints,
                    &coordinates, &mut equations),
                    Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                } else {
                    assert!(result.expect("exact equation and vector backing"));
                    assert_eq!(equations.len(), 1);
                    assert_eq!(
                        equations[0].terms,
                        BTreeMap::from([((3, SectionAxis::U), 0.0), ((3, SectionAxis::V), 1.0)])
                    );
                    assert_eq!(equations[0].rhs, 0.0);
                    let original = ctx
                        .charge_retained_limit(1, "after point-on-line equation")
                        .expect_err("all actual output backing retained");
                    assert_eq!((original.used, original.additional), (live_bytes, 1));
                }
                if refused {
                    Err(ctx
                        .resource_refusal()
                        .expect("actual output storage refusal")
                        .into())
                } else {
                    Ok(())
                }
            },
        );
    }
}

#[test]
fn solved_coordinate_state_transfers_only_the_final_points_map() {
    type Coordinates = [Option<f64>; 2];
    let mut definition = super::incomplete_segment_definition();
    definition.variables = None;
    definition.segments = None;
    definition.relations = None;
    let input = [
        SectionCoordinateEquation {
            terms: BTreeMap::from([((7, SectionAxis::U), 1.0)]),
            rhs: 3.0,
        },
        SectionCoordinateEquation {
            terms: BTreeMap::from([((7, SectionAxis::V), 1.0)]),
            rhs: 4.0,
        },
    ];
    let alignment = align_of::<u32>()
        .max(align_of::<Coordinates>())
        .max(align_of::<usize>());
    let node = 11 * (size_of::<u32>() + size_of::<Coordinates>())
        + 16 * size_of::<usize>()
        + 2 * alignment;
    let live_bytes = u64::try_from(node).expect("one final point node");
    crate::test_support::assert_refusal_order(
        ResourceDimension::RetainedBytes,
        &["creo solved coordinate state scratch"],
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut equations = input.to_vec();
            let mut scalar_values = BTreeMap::new();
            let result = super::super::solve_section_coordinates_with_derived_constraints(
                &ctx,
                &definition,
                &mut equations,
                &BTreeMap::new(),
                (&[], &[]),
                &SectionEquationAuxiliaryConstraints::default(),
                &mut scalar_values,
            );
            let refused = result.is_err();
            if refused {
                let original = ctx.resource_refusal().expect("final map transfer refusal");
                assert_eq!(
                    (
                        original.dimension,
                        original.used,
                        original.additional,
                        original.operation
                    ),
                    (
                        ResourceDimension::RetainedBytes,
                        0,
                        live_bytes,
                        "creo solved coordinate state scratch"
                    )
                );
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                assert!(
                    matches!(super::super::solve_section_coordinates_with_derived_constraints(&ctx,
                &definition, &mut equations, &BTreeMap::new(), (&[], &[]),
                &SectionEquationAuxiliaryConstraints::default(), &mut scalar_values),
                Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
            } else {
                let points = result.expect("only the final point map is retained");
                assert_eq!(points, BTreeMap::from([(7, [Some(3.0), Some(4.0)])]));
                let original = ctx
                    .charge_retained_limit(1, "after solved coordinate state")
                    .expect_err("exact final point backing consumed");
                assert_eq!((original.used, original.additional), (live_bytes, 1));
            }
            if refused {
                Err(ctx
                    .resource_refusal()
                    .expect("final map transfer refusal")
                    .into())
            } else {
                Ok(())
            }
        },
    );
}
