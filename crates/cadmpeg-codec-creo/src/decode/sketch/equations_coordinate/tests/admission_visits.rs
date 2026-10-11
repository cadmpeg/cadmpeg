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
fn zero_variable_pivot_columns_charge_only_present_rows() {
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
            match result {
Err(CodecError::ResourceLimit(original)) => {
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
                Err(original.into())

}
Err(error) => panic!("unexpected route refusal: {error:?}"),
Ok(value) => {
                assert_eq!(value, None);
                Ok(())

}
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
fn coordinate_solver_without_equations_is_free() {
    with_zero_resources(|ctx| {
        let values = super::super::solve_section_coordinate_equations(
            &ctx, &[], &BTreeMap::new(),
        ).expect("no equation components");
        assert!(values.is_empty());
    });
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
