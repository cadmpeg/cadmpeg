// SPDX-License-Identifier: Apache-2.0

use super::super::super::{
    perpendicular_round_edge_cylinder_frame, round_edge_cylinder_frame,
    unique_support_tangent_cylinder_frame, PerpendicularRoundEdgeFailure,
};
use crate::decode::analytic::equations::PlaneEquation;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn edge_envelope() -> crate::surface::Type24RoundEdgeEnvelope {
    crate::surface::Type24RoundEdgeEnvelope {
        parameter_interval: [0.25, 5.25],
        vertices: [[1.0, 0.2, 3.0], [1.2, 0.0, 8.0]],
        generated_entity_reference: None,
    }
}

fn visit_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_retained_bytes = 0;
    policy
}

fn assert_exact_visits(
    operations: &[&'static str],
    query: impl Fn(&DecodeContext<'_>) -> Result<bool, CodecError>,
) {
    let visits = u64::try_from(operations.len()).expect("fixture visits fit u64");
    crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, operations, |cap| {
        let arena = DecodeArena::new();
        let policy = visit_policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = query(&ctx);
        if let Err(CodecError::ResourceLimit(original)) = &result {
            assert_eq!((original.limit, original.used), (cap, cap));
            assert_eq!(ctx.resource_refusal().as_ref(), Some(original));
            assert_eq!(original.additional, 1);
            assert!(
                matches!(query(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == *original)
            );
        }
        result.map(|matched| assert!(matched))
    });
    let arena = DecodeArena::new();
    let policy = visit_policy(visits);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(query(&ctx).expect("all present visits admitted"));
    let original = ctx
        .charge_work_limit(1, "after cylinder visits")
        .expect_err("exact visits");
    assert_eq!(
        (original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, visits, 1)
    );
    assert!(matches!(query(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn round_edge_fixed_preconditions_preserve_original_refusal_without_work() {
    let arena = DecodeArena::new();
    let mut policy = visit_policy(0);
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let check = |refused| {
        let mut nonfinite = edge_envelope();
        nonfinite.vertices[0][0] = f64::NAN;
        let results = [
            round_edge_cylinder_frame(&ctx, edge_envelope(), 0.0, &[]).map(|frame| frame.is_none()),
            round_edge_cylinder_frame(&ctx, edge_envelope(), -1.0, &[])
                .map(|frame| frame.is_none()),
            round_edge_cylinder_frame(&ctx, edge_envelope(), f64::NAN, &[])
                .map(|frame| frame.is_none()),
            round_edge_cylinder_frame(&ctx, edge_envelope(), f64::INFINITY, &[])
                .map(|frame| frame.is_none()),
            round_edge_cylinder_frame(&ctx, nonfinite, 0.2, &[]).map(|frame| frame.is_none()),
            round_edge_cylinder_frame(&ctx, edge_envelope(), 0.2, &[]).map(|frame| frame.is_none()),
            perpendicular_round_edge_cylinder_frame(&ctx, edge_envelope(), &[]).map(|frame| {
                frame == Err(PerpendicularRoundEdgeFailure::NoPerpendicularSupportPair)
            }),
        ];
        for result in results {
            if refused {
                let original = ctx.resource_refusal().expect("seeded refusal");
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
            } else {
                assert!(result.expect("fixed precondition needs no input visit"));
            }
        }
    };
    check(false);
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx
        .charge_work_limit(1, "after cylinder fixed preconditions")
        .expect_err("zero cap");
    assert_eq!(
        (original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, 0, 1)
    );
    check(true);
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn round_edge_pair_search_admits_present_supports_and_skips_invalid_tails() {
    let parallel = PlaneEquation {
        origin: [1.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    };
    let invalid = PlaneEquation {
        origin: [1.0, 0.0, 0.0],
        normal: [0.0; 3],
    };
    for count in [0, 1, 2, 5] {
        for plane in [parallel, invalid] {
            let planes = vec![plane; count];
            let operations = |first, second| {
                let mut visits = Vec::new();
                for index in 0..count {
                    visits.push(first);
                    if plane.normal != [0.0; 3] {
                        visits.extend(std::iter::repeat_n(second, count - index - 1));
                    }
                }
                visits
            };
            // Each valid first support visits the remaining suffix once:
            // n first visits + n*(n-1)/2 second visits. Invalid first normals
            // stop before constructing or visiting that suffix.
            assert_exact_visits(
                &operations(
                    "creo round-edge first support planes",
                    "creo round-edge second support planes",
                ),
                |ctx| {
                    round_edge_cylinder_frame(ctx, edge_envelope(), 0.2, &planes)
                        .map(|frame| frame.is_none())
                },
            );
            assert_exact_visits(
                &operations(
                    "creo perpendicular round-edge first support planes",
                    "creo perpendicular round-edge second support planes",
                ),
                |ctx| {
                    perpendicular_round_edge_cylinder_frame(ctx, edge_envelope(), &planes).map(
                        |frame| {
                            frame == Err(PerpendicularRoundEdgeFailure::NoPerpendicularSupportPair)
                        },
                    )
                },
            );
        }
    }
}

#[test]
fn solved_round_edge_frame_admits_each_executed_support_pass_once() {
    let supports = [
        PlaneEquation {
            origin: [1.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        },
        PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        },
    ];
    let frame_matches = |frame: crate::surface::PositionalCylinderFrame| {
        frame
            .frame()
            .origin()
            .into_iter()
            .zip([1.2, 0.2, 0.0])
            .all(|(actual, expected)| (actual - expected).abs() < super::super::EPS_TEST_GEOMETRY)
            && frame.frame().axis() == [0.0, 0.0, 1.0]
            && (frame.radius().get() - 0.2).abs() < super::super::EPS_TEST_GEOMETRY
            && frame.length().map(cadmpeg_ir::scalar::PositiveLength::get) == Some(5.0)
    };
    // Two first-support visits and one second-support visit. The perpendicular
    // solver executes its own three visits, then the same three in the frame solver.
    let direct = [
        "creo round-edge first support planes",
        "creo round-edge second support planes",
        "creo round-edge first support planes",
    ];
    assert_exact_visits(&direct, |ctx| {
        round_edge_cylinder_frame(ctx, edge_envelope(), 0.2, &supports)
            .map(|frame| frame.is_some_and(frame_matches))
    });
    let delegated = [
        "creo perpendicular round-edge first support planes",
        "creo perpendicular round-edge second support planes",
        "creo perpendicular round-edge first support planes",
        direct[0],
        direct[1],
        direct[2],
    ];
    assert_exact_visits(&delegated, |ctx| {
        perpendicular_round_edge_cylinder_frame(ctx, edge_envelope(), &supports)
            .map(|frame| frame.is_ok_and(frame_matches))
    });
}

#[test]
fn support_tangent_absence_admits_only_present_planes_and_releases_workspace() {
    let stored = super::axial_interval_candidate([10.0, 7.0, 9.0]);
    let unmatched = PlaneEquation {
        origin: [0.0, 20.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    };
    let invalid = PlaneEquation {
        origin: [0.0; 3],
        normal: [0.0; 3],
    };
    for count in [0, 1, 5] {
        let planes = vec![unmatched; count];
        assert_exact_visits(&vec!["creo support tangent support planes"; count], |ctx| {
            unique_support_tangent_cylinder_frame(ctx, stored, &planes).map(|frame| frame.is_none())
        });
        let arena = DecodeArena::new();
        let policy = visit_policy(u64::try_from(count).expect("fixture count fits u64"));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(unique_support_tangent_cylinder_frame(&ctx, stored, &planes)
            .expect("present planes")
            .is_none());
        // Empty-root effective materialized allowance is 16 MiB. Both origin
        // and witness backing are gone after the scalar absence result returns.
        let full_allowance = ctx
            .reserve_scoped(16 * 1024 * 1024, "after tangent workspace")
            .expect("workspace released");
        drop(full_allowance);
    }
    let mut planes = vec![unmatched; 129];
    planes[0] = invalid;
    assert_exact_visits(&["creo support tangent support planes"], |ctx| {
        unique_support_tangent_cylinder_frame(ctx, stored, &planes).map(|frame| frame.is_none())
    });
}
