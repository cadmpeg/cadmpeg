// SPDX-License-Identifier: Apache-2.0
use super::{
    point_entity, rectangular_parameter, rectangular_parameters, rectangular_point_relation,
    RectangularPatternDistanceForm,
};
use crate::design::constraints::{exact_rectangular_pattern, PatternParameters};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn pattern_parameter_index_keeps_the_first_duplicate_and_is_reused() {
    let mut parameters = rectangular_parameters(3, 1.5).to_vec();
    parameters.push(rectangular_parameter(20, 99.0));
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut index = PatternParameters::new(ctx, &parameters);
        assert!(std::ptr::eq(
            index.get("native", 20).unwrap().unwrap(),
            &raw const parameters[0]
        ));
        assert!(std::ptr::eq(
            index.get("native", 20).unwrap().unwrap(),
            &raw const parameters[0]
        ));
        assert!(std::ptr::eq(
            index.get("native", 23).unwrap().unwrap(),
            &raw const parameters[3]
        ));
    });
}

#[test]
fn pattern_parameter_index_refuses_before_source_traversal() {
    let parameters = rectangular_parameters(3, 1.5);
    let operation = "f3d pattern parameter scan";
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            PatternParameters::new(ctx, &parameters)
                .get("native", 20)
                .map(|value| value.is_some())
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn rejected_rectangular_geometry_keeps_no_identity_copies() {
    let seed = point_entity("synthetic:test:point#seed", 2.0);
    let second = point_entity("synthetic:test:point#second", 17.0);
    let third = point_entity("synthetic:test:point#third", 100.0);
    let relation =
        rectangular_point_relation(3, 1.5, RectangularPatternDistanceForm::AdjacentSpacing);
    let parameters = rectangular_parameters(3, 1.5);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    assert_eq!(
        crate::test_support::with_decode_policy(&policy, |ctx| {
            exact_rectangular_pattern(
                &relation,
                "native",
                &mut PatternParameters::new(ctx, &parameters),
                &[&seed, &second, &third],
                ctx,
            )
        })
        .unwrap(),
        None
    );
}
