// SPDX-License-Identifier: Apache-2.0
//! Reference checking stops at the first invalid visit.

use super::super::{LocationFactor, ShapeSet, TextLocation};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::transform::Transform;

#[test]
fn shape_check_invalid_location_skips_unvisited_locations_and_factors() {
    for count in [1, 4096] {
        let mut facts = super::allocation_tests::shape_payload()
            .payload
            .shape_set()
            .unwrap()
            .clone();
        facts.locations = vec![
            TextLocation {
                factors: vec![
                    LocationFactor {
                        location: 0,
                        power: 1
                    };
                    count
                ],
                transform: Transform::identity(),
            };
            count
        ];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 32;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(facts.validate_charged(&ctx), Err(CodecError::Malformed(message))
            if message == "location factor reference index 0 is out of range 1..=0")
        );
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn shape_check_refuses_before_first_reference_visit() {
    let mut facts: ShapeSet = super::allocation_tests::shape_payload()
        .payload
        .shape_set()
        .unwrap()
        .clone();
    facts.locations.push(TextLocation {
        factors: Vec::new(),
        transform: Transform::identity(),
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(facts.validate_charged(&ctx), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits)
    );
}

#[test]
fn shape_check_polygon_zero_skips_unvisited_node_suffix() {
    for count in [1, 4096] {
        let mut facts = super::allocation_tests::shape_payload()
            .payload
            .shape_set()
            .unwrap()
            .clone();
        facts
            .polygons_on_triangulations
            .push(super::super::TextPolygonOnTriangulation {
                nodes: vec![0; count],
                deflection: cadmpeg_ir::scalar::NonNegativeReal::from_finite(
                    cadmpeg_ir::scalar::FiniteReal::ZERO,
                )
                .unwrap(),
                parameters: None,
            });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 32;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = facts
            .check(&[0], |visits| {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(visits),
                    "test shape visits",
                )
            })
            .unwrap();
        assert_eq!(
            result.unwrap_err(),
            "PolygonOnTriangulations[0] node indices must be one-based"
        );
        assert_eq!(ctx.resource_refusal(), None);
    }
}
