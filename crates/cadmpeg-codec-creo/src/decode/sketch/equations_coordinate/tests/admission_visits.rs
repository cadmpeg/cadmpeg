// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

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
    for cap in 0..=2 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut matrix = [super::super::SectionLinearRow { coefficients: BTreeMap::new(), rhs: 1.0 }];
        let result = super::super::uniquely_solved_linear_variables(&ctx, &mut matrix, 0);
        if cap < 2 {
            let Err(CodecError::ResourceLimit(original)) = result else { panic!("present row must refuse"); };
            assert_eq!((original.dimension, original.used, original.additional, original.limit),
                (ResourceDimension::WorkUnits, cap, 1, cap));
            assert_eq!(original.operation, if cap == 0 {
                "creo linear solver matrix scale rows"
            } else { "creo section residual matrix rows" });
            for _ in 0..2 {
                assert!(matches!(super::super::uniquely_solved_linear_variables(&ctx, &mut matrix, 0),
                    Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            assert_eq!(result.expect("two rows admitted"), None);
            ctx.finish_session().expect("active session");
        }
    }
}
