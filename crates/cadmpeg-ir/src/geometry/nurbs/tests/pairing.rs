// SPDX-License-Identifier: Apache-2.0

use crate::features::FinitePoint3;
use crate::geometry::nurbs::{
    NurbsCurve, NurbsError, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis,
    NurbsSurfaceLanes,
};
use crate::math::Point3;
use crate::scalar::{FiniteReal, NonZeroReal};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn curve(
    ctx: &DecodeContext<'_>,
    form: usize,
) -> Result<Result<NurbsPoles3<u8>, NurbsError>, CodecError> {
    match form {
        0 => NurbsPoles3::from_lanes(ctx, vec![10, 20], Some(vec![1.0, 2.0])),
        1 => NurbsPoles3::from_finite_lanes(
            ctx,
            vec![10, 20],
            Some(vec![FiniteReal::ONE, FiniteReal::new(2.0).expect("finite")]),
        ),
        2 => NurbsPoles3::from_checked_lanes(
            ctx,
            vec![10, 20],
            Some(vec![
                NonZeroReal::ONE,
                NonZeroReal::new(2.0).expect("nonzero"),
            ]),
        ),
        _ => panic!("test form"),
    }
}

fn grid(
    ctx: &DecodeContext<'_>,
    form: usize,
) -> Result<Result<NurbsPoleGrid<u8>, NurbsError>, CodecError> {
    let rows = vec![vec![10, 20], vec![30, 40]];
    match form {
        0 => NurbsPoleGrid::from_lanes(ctx, rows, Some(vec![vec![1.0, 2.0], vec![3.0, 4.0]])),
        1 => NurbsPoleGrid::from_finite_lanes(
            ctx,
            rows,
            Some(vec![
                vec![FiniteReal::ONE, FiniteReal::new(2.0).expect("finite")],
                vec![
                    FiniteReal::new(3.0).expect("finite"),
                    FiniteReal::new(4.0).expect("finite"),
                ],
            ]),
        ),
        2 => NurbsPoleGrid::from_checked_lanes(
            ctx,
            rows,
            Some(vec![
                vec![NonZeroReal::ONE, NonZeroReal::new(2.0).expect("nonzero")],
                vec![
                    NonZeroReal::new(3.0).expect("nonzero"),
                    NonZeroReal::new(4.0).expect("nonzero"),
                ],
            ]),
        ),
        _ => panic!("test form"),
    }
}

fn refusal<T: std::fmt::Debug>(
    result: Result<T, CodecError>,
    ctx: DecodeContext<'_>,
    dimension: ResourceDimension,
    operation: &'static str,
) {
    let CodecError::ResourceLimit(limit) = result.expect_err("caller admission must refuse") else {
        panic!("original resource refusal required");
    };
    assert_eq!(limit.dimension, dimension);
    assert_eq!(limit.operation, operation);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}

#[test]
fn pairing_all_weight_forms_admit_each_pole_and_row_visit() {
    for form in 0..3 {
        for cap in 0..2 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            refusal(
                curve(&ctx, form),
                ctx,
                ResourceDimension::WorkUnits,
                "IR NURBS paired poles",
            );
        }
        for (cap, operation) in [
            (0, "IR NURBS paired grid rows"),
            (1, "IR NURBS paired poles"),
            (2, "IR NURBS paired poles"),
            (3, "IR NURBS paired grid rows"),
            (4, "IR NURBS paired poles"),
            (5, "IR NURBS paired poles"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            refusal(
                grid(&ctx, form),
                ctx,
                ResourceDimension::WorkUnits,
                operation,
            );
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 8;
        policy.limits.max_collection_items = 8;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let paired = curve(&ctx, form)
            .expect("two slots and visits")
            .expect("valid pairing");
        assert_eq!(paired.points(), vec![10, 20]);
        assert_eq!(paired.weights(), Some(vec![1.0, 2.0]));
        let paired = grid(&ctx, form)
            .expect("six more slots and visits")
            .expect("valid pairing");
        assert_eq!(paired.points(), vec![vec![10, 20], vec![30, 40]]);
        assert_eq!(paired.weights(), Some(vec![vec![1.0, 2.0], vec![3.0, 4.0]]));
        ctx.finish_session().expect("exact combined limits");
    }
}

#[test]
fn pairing_storage_refusals_stay_in_the_original_caller_account() {
    for form in 0..3 {
        for dimension in [
            ResourceDimension::RetainedBytes,
            ResourceDimension::CollectionItems,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                _ => panic!("test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            refusal(curve(&ctx, form), ctx, dimension, "IR NURBS paired poles");
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            refusal(
                grid(&ctx, form),
                ctx,
                dimension,
                "IR NURBS paired grid rows",
            );
        }
        for (cap, operation) in [
            (1, "IR NURBS paired poles"),
            (3, "IR NURBS paired grid rows"),
            (5, "IR NURBS paired poles"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            refusal(
                grid(&ctx, form),
                ctx,
                ResourceDimension::CollectionItems,
                operation,
            );
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let points = vec![10_u8, 20];
    let address = points.as_ptr();
    let NurbsPoles3::Polynomial { points } = NurbsPoles3::from_lanes(&ctx, points, None)
        .expect("move needs no new admission")
        .expect("polynomial")
    else {
        panic!("polynomial");
    };
    assert_eq!(points.as_ptr(), address);
    let rows = vec![vec![30_u8, 40], vec![50, 60]];
    let outer = rows.as_ptr();
    let inner = rows[0].as_ptr();
    let NurbsPoleGrid::Polynomial { rows } = NurbsPoleGrid::from_checked_lanes(&ctx, rows, None)
        .expect("move needs no new admission")
        .expect("polynomial")
    else {
        panic!("polynomial");
    };
    assert_eq!(rows.as_ptr(), outer);
    assert_eq!(rows[0].as_ptr(), inner);
    ctx.finish_session().expect("owned storage preserved");
}

#[test]
fn pairing_checks_source_lane_lengths_before_weights_and_admits_diagnostics() {
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        NurbsPoles3::from_lanes(&ctx, vec![10_u8, 20], Some(vec![f64::NAN])).expect("diagnostic"),
        Err(NurbsError::WeightLaneLength {
            field: "poles".into(),
            poles: 2,
            weights: 1
        })
    );
    assert_eq!(
        NurbsPoleGrid::from_lanes(
            &ctx,
            vec![vec![10_u8, 20], vec![30]],
            Some(vec![vec![0.0], vec![f64::NAN]])
        )
        .expect("diagnostic"),
        Err(NurbsError::WeightLaneLength {
            field: "pole grid row".into(),
            poles: 2,
            weights: 1
        })
    );
    assert_eq!(
        NurbsPoleGrid::from_finite_lanes(
            &ctx,
            vec![vec![10_u8], vec![20]],
            Some(vec![vec![FiniteReal::ONE], vec![FiniteReal::ZERO]])
        )
        .expect("diagnostic"),
        Err(NurbsError::UnusableWeight {
            field: "pole grid row".into(),
            index: 0,
            weight: 0.0
        })
    );
    for grid in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        if grid {
            refusal(
                NurbsPoleGrid::from_checked_lanes(&ctx, vec![vec![10_u8]], Some(vec![])),
                ctx,
                ResourceDimension::RetainedBytes,
                "IR NURBS refusal field",
            );
        } else {
            refusal(
                NurbsPoles3::from_lanes(&ctx, vec![10_u8], Some(vec![])),
                ctx,
                ResourceDimension::RetainedBytes,
                "IR NURBS refusal field",
            );
        }
    }
}

#[test]
fn checked_geometry_pairing_scopes_raw_storage_and_retains_admitted_storage() {
    for surface in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let point = Point3::new(1.0, 2.0, 3.0);
        let knots = || vec![0.0, 0.0, 1.0, 1.0];
        if surface {
            refusal(
                NurbsSurface::from_checked_lanes(
                    &ctx,
                    NurbsSurfaceAxis::new(1, knots(), false),
                    NurbsSurfaceAxis::new(1, knots(), false),
                    NurbsSurfaceLanes::new(
                        vec![vec![point; 2]; 2],
                        Some(vec![vec![NonZeroReal::ONE; 2]; 2]),
                    ),
                    false,
                ),
                ctx,
                ResourceDimension::MaterializedBytes,
                "IR NURBS paired grid rows",
            );
        } else {
            refusal(
                NurbsCurve::from_checked_lanes(
                    &ctx,
                    1,
                    knots(),
                    vec![point; 2],
                    Some(vec![NonZeroReal::ONE; 2]),
                    false,
                ),
                ctx,
                ResourceDimension::MaterializedBytes,
                "IR NURBS paired poles",
            );
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let point = FinitePoint3::new(point).expect("finite");
        if surface {
            refusal(
                NurbsSurface::from_finite_lanes(
                    &ctx,
                    NurbsSurfaceAxis::new(
                        1,
                        vec![
                            FiniteReal::ZERO,
                            FiniteReal::ZERO,
                            FiniteReal::ONE,
                            FiniteReal::ONE,
                        ],
                        false,
                    ),
                    NurbsSurfaceAxis::new(
                        1,
                        vec![
                            FiniteReal::ZERO,
                            FiniteReal::ZERO,
                            FiniteReal::ONE,
                            FiniteReal::ONE,
                        ],
                        false,
                    ),
                    NurbsSurfaceLanes::new(
                        vec![vec![point; 2]; 2],
                        Some(vec![vec![FiniteReal::ONE; 2]; 2]),
                    ),
                    false,
                ),
                ctx,
                ResourceDimension::RetainedBytes,
                "IR NURBS paired grid rows",
            );
        } else {
            refusal(
                NurbsCurve::from_finite_lanes(
                    &ctx,
                    1,
                    vec![
                        FiniteReal::ZERO,
                        FiniteReal::ZERO,
                        FiniteReal::ONE,
                        FiniteReal::ONE,
                    ],
                    vec![point; 2],
                    Some(vec![FiniteReal::ONE; 2]),
                    false,
                ),
                ctx,
                ResourceDimension::RetainedBytes,
                "IR NURBS paired poles",
            );
        }
    }
}
