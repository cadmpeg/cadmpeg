// SPDX-License-Identifier: Apache-2.0

use super::*;

#[derive(Clone, Copy)]
enum Limit {
    Collection,
    Retained,
    Work,
}

fn assert_rectangular_refusal(dimension: Limit, operation: &'static str) {
    let seed = point_entity("generated:test:point#rectangular-alloc-seed", 2.0);
    let second = point_entity("generated:test:point#rectangular-alloc-second", 17.0);
    let third = point_entity("generated:test:point#rectangular-alloc-third", 32.0);
    let relation =
        rectangular_point_relation(3, 1.5, RectangularPatternDistanceForm::AdjacentSpacing);
    let parameters = rectangular_parameters(3, 1.5);
    let dimension = match dimension {
        Limit::Collection => cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Limit::Retained => cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Limit::Work => cadmpeg_core::decode::ResourceDimension::WorkUnits,
    };
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
        exact_rectangular_pattern(
            &relation,
            "native",
            &mut crate::design::constraints::PatternParameters::new(ctx, &parameters),
            &[&seed, &second, &third],
            ctx,
        )
    });
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.operation == operation && failure.dimension == dimension));
}

#[test]
fn rectangular_distance_parameter_id_refuses_retained_limit() {
    assert_rectangular_refusal(
        Limit::Retained,
        "f3d rectangular pattern distance parameter id",
    );
}

#[test]
fn rectangular_count_parameter_id_refuses_retained_limit() {
    assert_rectangular_refusal(
        Limit::Retained,
        "f3d rectangular pattern count parameter id",
    );
}

#[test]
fn rectangular_candidate_match_refuses_work_limit() {
    assert_rectangular_refusal(Limit::Work, "f3d rectangular pattern candidate match");
}

#[test]
fn rectangular_instance_entity_id_refuses_retained_limit() {
    assert_rectangular_refusal(
        Limit::Retained,
        "f3d rectangular pattern instance entity id",
    );
}

#[test]
fn rectangular_instance_entity_refuses_collection_limit() {
    assert_rectangular_refusal(Limit::Collection, "f3d rectangular pattern instance entity");
}

#[test]
fn rectangular_row_refuses_collection_limit() {
    assert_rectangular_refusal(Limit::Collection, "f3d rectangular pattern row");
}

#[test]
fn rectangular_instance_refuses_collection_limit() {
    assert_rectangular_refusal(Limit::Collection, "f3d rectangular pattern instance");
}
