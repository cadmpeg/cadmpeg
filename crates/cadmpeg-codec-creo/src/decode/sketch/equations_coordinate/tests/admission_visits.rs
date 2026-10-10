// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn unsigned_coordinate_validation_admits_present_terms_and_preserves_solution() {
    // Each point-value equation has one term; validation stops at its metadata boundary.
    let values = crate::test_support::assert_work_boundaries(
        &["creo coordinate validation terms"],
        super::unsigned_value_fixture,
    );
    assert_eq!(values, BTreeMap::from([((2, super::SectionAxis::U), 1.0)]));
}

#[test]
fn zero_variable_pivot_columns_are_free_and_preserve_original_refusal() {
    // One matrix scale row plus the first inconsistent residual row: exactly two visits.
    // No coefficient, pivot column, free column, output or scratch backing exists.
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &[
            "creo linear solver matrix scale rows",
            "creo section residual matrix rows",
        ],
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut matrix = [super::super::SectionLinearRow {
                coefficients: BTreeMap::new(),
                rhs: 1.0,
            }];
            let result = super::super::uniquely_solved_linear_variables(&ctx, &mut matrix, 0);
            let refused = result.is_err();
            if refused {
                let Err(CodecError::ResourceLimit(original)) = result else {
                    panic!("present row must refuse");
                };
                assert_eq!(
                    (
                        original.dimension,
                        original.used,
                        original.additional,
                        original.limit
                    ),
                    (ResourceDimension::WorkUnits, cap, 1, cap)
                );
                assert_eq!(
                    original.operation,
                    if cap == 0 {
                        "creo linear solver matrix scale rows"
                    } else {
                        "creo section residual matrix rows"
                    }
                );
                for _ in 0..2 {
                    assert!(
                        matches!(super::super::uniquely_solved_linear_variables(&ctx, &mut matrix, 0),
                    Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                }
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                Err(original.into())
            } else {
                assert_eq!(result.expect("two rows admitted"), None);
                ctx.finish_session().expect("active session");
                Ok(())
            }
        },
    );
}

fn with_zero_resources<T>(run: impl FnOnce(DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    run(ctx)
}

#[test]
fn empty_coordinate_component_seed_is_free_and_keeps_original_refusal() {
    for refused in [false, true] {
        with_zero_resources(|ctx| {
            let original = refused.then(|| match ctx.charge_work(1, "seed component refusal") {
                Err(CodecError::ResourceLimit(limit)) => limit,
                other => panic!("expected seed refusal: {other:?}"),
            });
            let mut remaining = BTreeSet::new();
            for _ in 0..2 {
                let result = super::super::next_section_component(&ctx, &mut remaining, &[]);
                if let Some(original) = original {
                    assert!(
                        matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                } else {
                    assert_eq!(result.expect("no component seed"), None);
                }
                assert!(remaining.is_empty());
            }
            if let Some(original) = original {
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
            } else {
                ctx.finish_session().expect("active session");
            }
        });
    }
}

#[test]
fn coordinate_solver_without_equations_is_free_and_keeps_original_refusal() {
    for refused in [false, true] {
        with_zero_resources(|ctx| {
            let original =
                refused.then(|| match ctx.charge_work(1, "seed empty equation refusal") {
                    Err(CodecError::ResourceLimit(limit)) => limit,
                    other => panic!("expected seed refusal: {other:?}"),
                });
            for _ in 0..2 {
                let result =
                    super::super::solve_section_coordinate_equations(&ctx, &[], &BTreeMap::new());
                if let Some(original) = original {
                    assert!(
                        matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                } else {
                    assert!(result.expect("no equation components").is_empty());
                }
            }
            if let Some(original) = original {
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
            } else {
                ctx.finish_session().expect("active session");
            }
        });
    }
}

#[test]
fn coordinate_component_seed_refuses_before_the_first_present_row() {
    let component =
        crate::test_support::assert_work_boundaries(&["creo section component seed scan"], |ctx| {
            let mut remaining = BTreeSet::from([0]);
            let component =
                super::super::next_section_component(ctx, &mut remaining, &[BTreeSet::new()])?;
            assert!(remaining.is_empty());
            Ok(component)
        });
    assert_eq!(component, Some(BTreeSet::from([0])));
}
