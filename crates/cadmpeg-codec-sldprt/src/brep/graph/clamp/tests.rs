// SPDX-License-Identifier: Apache-2.0
//! Each replacement generation owns its current knots and controls.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::brep::graph::clamp::clamp_nurbs_curve_to_domain_lanes;
use crate::brep::graph::clamp::insert_nurbs_homogeneous_knot;

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;
const KNOTS: [f64; 4] = [0.0, 0.0, 1.0, 1.0];
const CONTROLS: [[f64; 4]; 2] = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy
}

#[test]
fn homogeneous_replacement_releases_old_generations_and_preserves_numeric_lanes() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut current = insert_nurbs_homogeneous_knot(&ctx, 1, &KNOTS, &CONTROLS, 0.5)
        .unwrap()
        .unwrap();
    for value in [0.75, 0.25] {
        let next = insert_nurbs_homogeneous_knot(&ctx, 1, &current.knots, &current.controls, value)
            .unwrap()
            .unwrap();
        current = next;
    }
    assert_eq!(current.knots, [0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0]);
    assert_eq!(
        current.controls,
        [
            [0.0, 0.0, 0.0, 1.0],
            [0.25, 0.0, 0.0, 1.0],
            [0.5, 0.0, 0.0, 1.0],
            [0.75, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
        ]
    );
    // Core's minimum capacity4 and doubling leave capacity8 in each Vec.
    // The final controls have five elements; the final knots have seven.
    let expected =
        u64::try_from(8 * std::mem::size_of::<f64>() + 8 * std::mem::size_of::<[f64; 4]>())
            .unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(
        SCRATCH_BYTES - expected + 1,
        "test final homogeneous generation",
    ) else {
        panic!("live generation probe");
    };
    assert_eq!(
        (limit.used, limit.additional),
        (expected, SCRATCH_BYTES - expected + 1)
    );
    assert_eq!(limit.limit, SCRATCH_BYTES);
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(current);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit)
    );
}

#[test]
fn unpublished_homogeneous_generation_drop_releases_all_backing() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let generation = insert_nurbs_homogeneous_knot(&ctx, 1, &KNOTS, &CONTROLS, 0.5)
        .unwrap()
        .unwrap();
    drop(generation);
    drop(
        ctx.reserve_scoped(SCRATCH_BYTES, "test released homogeneous generation")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn clamp_raw_segment_releases_initial_and_replacement_generations() {
    let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 2.0, 2.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap()
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = policy();
    // Raw output: four exact knot slots and minimum capacity4 Point3 slots.
    // This pins only the raw-return boundary; converted IR storage is separate.
    let expected = u64::try_from(
        4 * std::mem::size_of::<f64>() + 4 * std::mem::size_of::<cadmpeg_ir::math::Point3>(),
    )
    .unwrap();
    policy.limits.max_retained_bytes = expected;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (knots, points, weights) = clamp_nurbs_curve_to_domain_lanes(&ctx, &curve, [0.5, 1.5])
        .unwrap()
        .unwrap();
    assert_eq!(knots, [0.5, 0.5, 1.5, 1.5]);
    assert_eq!(
        points,
        [
            cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.5, 0.0, 0.0)
        ]
    );
    assert!(weights.is_none());
    drop(
        ctx.reserve_scoped(SCRATCH_BYTES, "test released clamp generations")
            .unwrap(),
    );
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(1, "test raw clamp output")
    else {
        panic!("raw segment storage boundary");
    };
    assert_eq!((limit.used, limit.additional), (expected, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit)
    );
}

#[test]
fn clamp_invalid_domain_preserves_the_original_refusal() {
    let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap()
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test original clamp refusal")
    else {
        panic!("original refusal");
    };
    let result = clamp_nurbs_curve_to_domain_lanes(&ctx, &curve, [1.0, 0.0]);
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit)
    );
}

#[test]
fn homogeneous_insert_refuses_prefix_backing_before_allocation() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        insert_nurbs_homogeneous_knot(&ctx, 1, &KNOTS, &CONTROLS, 0.5)
    });
    let CodecError::ResourceLimit(limit) = result.err().expect("prefix storage refusal") else {
        panic!("resource refusal");
    };
    assert_eq!(allocations, 0);
    assert_eq!(limit.operation, "insert homogeneous NURBS knot lane");
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(
        (limit.used, limit.additional),
        (0, u64::try_from(4 * std::mem::size_of::<f64>()).unwrap())
    );
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit)
    );
}

#[test]
fn homogeneous_insert_invalid_shape_is_free_before_a_refusal() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        insert_nurbs_homogeneous_knot(&ctx, 0, &KNOTS, &CONTROLS, 0.5)
            .unwrap()
            .is_none()
    );
    ctx.finish_session().unwrap();
}

#[test]
fn homogeneous_insert_invalid_shape_preserves_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) =
        ctx.charge_work(1, "test original homogeneous refusal")
    else {
        panic!("original refusal");
    };
    let result = insert_nurbs_homogeneous_knot(&ctx, 0, &KNOTS, &CONTROLS, 0.5);
    assert!(matches!(&result, Err(CodecError::ResourceLimit(actual)) if *actual == limit));
    drop(result);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit)
    );
}

fn conversion_source(rational: bool) -> cadmpeg_ir::geometry::nurbs::NurbsCurve {
    cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 2.0, 2.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
        ],
        rational.then(|| vec![2.0, 2.0]),
        false,
    )
    .unwrap()
    .unwrap()
}

fn converted_bytes(rational: bool) -> u64 {
    let pole_size = if rational {
        std::mem::size_of::<
            cadmpeg_ir::geometry::nurbs::WeightedPole3<cadmpeg_ir::features::FinitePoint3>,
        >()
    } else {
        std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>()
    };
    // copy_slice retains four exact knots; IR collects two exact converted poles.
    u64::try_from(4 * std::mem::size_of::<f64>() + 2 * pole_size).unwrap()
}

#[test]
fn clamp_conversion_retains_only_knots_and_converted_poles() {
    for rational in [false, true] {
        let source = conversion_source(rational);
        let arena = DecodeArena::new();
        let mut policy = policy();
        let expected = converted_bytes(rational);
        policy.limits.max_retained_bytes = expected;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let curve = super::clamp_nurbs_curve_to_domain(&ctx, &source, [0.5, 1.5])
            .unwrap()
            .unwrap();
        assert_eq!(curve.knots().as_slice(), [0.5, 0.5, 1.5, 1.5]);
        assert_eq!(curve.pole_count(), 2);
        assert_eq!(
            curve.pole_rows().point_at(0).unwrap().get(),
            cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0)
        );
        assert_eq!(
            curve.pole_rows().point_at(1).unwrap().get(),
            cadmpeg_ir::math::Point3::new(1.5, 0.0, 0.0)
        );
        assert_eq!(curve.pole_rows().weight_at(0), rational.then_some(2.0));
        assert_eq!(curve.pole_rows().weight_at(1), rational.then_some(2.0));
        drop(
            ctx.reserve_scoped(SCRATCH_BYTES, "test consumed clamp raw poles")
                .unwrap(),
        );
        let Err(CodecError::ResourceLimit(limit)) =
            ctx.charge_retained(1, "test converted clamp backing")
        else {
            panic!("converted output boundary");
        };
        assert_eq!((limit.used, limit.additional), (expected, 1));
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit)
        );
    }
}

#[test]
fn clamp_conversion_transfers_only_surviving_backing_to_scoped_parent() {
    for rational in [false, true] {
        let source = conversion_source(rational);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
        let output_storage;
        let output;
        (output, output_storage) = ctx
            .with_scoped_storage("test clamp output parent", || {
                super::clamp_nurbs_curve_to_domain(&ctx, &source, [0.5, 1.5])
            })
            .unwrap();
        let curve = output.unwrap();
        assert_eq!(curve.pole_count(), 2);
        drop(
            ctx.reserve_scoped(
                SCRATCH_BYTES - converted_bytes(rational),
                "test live converted clamp backing",
            )
            .unwrap(),
        );
        drop(curve);
        drop(output_storage);
        drop(
            ctx.reserve_scoped(SCRATCH_BYTES, "test released converted clamp backing")
                .unwrap(),
        );
        ctx.finish_session().unwrap();
    }
}

#[test]
fn clamp_conversion_refuses_at_candidate_publication() {
    let source = conversion_source(false);
    let operation = "hold clamped NURBS output candidate";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = super::clamp_nurbs_curve_to_domain(&ctx, &source, [0.5, 1.5]);
            let Err(crate::brep::graph::nurbs_pcurves::NurbsPcurveFailure::Resource(error)) =
                result
            else {
                return Ok(());
            };
            let CodecError::ResourceLimit(limit) = &error else {
                panic!("resource refusal");
            };
            let limit = *limit;
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit)
            );
            Err(error)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal");
    };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((limit.used, limit.additional), (0, converted_bytes(false)));
    assert_eq!(limit.limit, converted_bytes(false) - 1);
}

#[test]
fn clamp_raw_conversion_refuses_each_backing_before_allocation() {
    let point_bytes = u64::try_from(4 * std::mem::size_of::<cadmpeg_ir::math::Point3>()).unwrap();
    let weight_bytes = u64::try_from(4 * std::mem::size_of::<f64>()).unwrap();
    for (materialized, operation, used, additional, expected_allocations) in [
        (0, "collect clamped NURBS controls", 0, point_bytes, 0),
        (
            point_bytes,
            "collect clamped NURBS weights",
            point_bytes,
            weight_bytes,
            1,
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = policy();
        policy.limits.max_materialized_bytes = materialized;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
            ctx.with_scoped_storage("test clamp raw conversion", || {
                super::clamped_raw_poles(&ctx, &CONTROLS, true)
            })
        });
        let CodecError::ResourceLimit(limit) = result.expect_err("raw backing refusal") else {
            panic!("resource refusal");
        };
        assert_eq!(allocations, expected_allocations);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!((limit.used, limit.additional), (used, additional));
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit)
        );
    }
}

#[test]
fn clamp_raw_conversion_refuses_first_slot_before_push() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        ctx.with_scoped_storage("test clamp raw conversion", || {
            super::clamped_raw_poles(&ctx, &CONTROLS, false)
        })
    });
    let CodecError::ResourceLimit(limit) = result.expect_err("first raw slot refusal") else {
        panic!("resource refusal");
    };
    assert_eq!(allocations, 1);
    assert_eq!(limit.operation, "collect clamped NURBS controls");
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!((limit.used, limit.additional), (0, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit)
    );
}

#[test]
fn clamp_conversion_invalid_prefix_preserves_original_fuse_without_allocation() {
    let source = conversion_source(false);
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) =
        ctx.charge_work(1, "test original clamp conversion refusal")
    else {
        panic!("original refusal");
    };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        super::clamped_conversion_lanes(&ctx, &source, [1.0, 0.0])
    });
    let CodecError::ResourceLimit(actual) = result.err().expect("original conversion refusal")
    else {
        panic!("resource refusal");
    };
    assert_eq!(actual, limit);
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit)
    );
}

#[test]
fn clamp_finite_rational_source_reaches_original_nonfinite_carrier_refusal() {
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsError};
    use cadmpeg_ir::math::Point3;

    // Finite admitted poles and positive weights can overflow their homogeneous
    // coordinates. Knot insertion retains that infinity; IR rejects the raw
    // dehomogenized point with its original structural error.
    let source = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            Point3::new(f64::MAX, 0.0, 0.0),
            Point3::new(f64::MAX, 1.0, 0.0),
        ],
        Some(vec![2.0, 2.0]),
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(source.pole_rows().point_at(0).unwrap().get().x, f64::MAX);
    assert_eq!(source.pole_rows().weight_at(0), Some(2.0));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes =
        u64::try_from("control_points contains a non-finite point".len()).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::clamp_nurbs_curve_to_domain(&ctx, &source, [0.25, 0.75]);
    let Err(crate::brep::graph::nurbs_pcurves::NurbsPcurveFailure::Carrier(NurbsError::Structure(
        message,
    ))) = &result
    else {
        panic!("finite source overflow reaches the original carrier refusal");
    };
    assert_eq!(message, "control_points contains a non-finite point");
    assert!(ctx.resource_refusal().is_none());
    // Only the escaping diagnostic remains retained; all candidate backing
    // and raw/homogeneous workspace have expired.
    drop(
        ctx.reserve_scoped(SCRATCH_BYTES, "reuse rejected clamp workspaces")
            .unwrap(),
    );
    drop(result);
    ctx.finish_session().unwrap();
}

#[test]
fn homogeneous_blend_rejection_charges_only_one_control() {
    let knots = [-f64::MAX; 9]
        .into_iter()
        .chain([f64::MAX; 9])
        .collect::<Vec<_>>();
    let controls = [[0.0, 0.0, 0.0, 1.0]; 9];
    let error = crate::test_support::work_refusal_at("blend homogeneous NURBS controls", |ctx| {
        insert_nurbs_homogeneous_knot(ctx, 8, &knots, &controls, 0.0).map(|_| ())
    });
    let CodecError::ResourceLimit(limit) = error else {
        panic!("blend visit refusal");
    };
    assert_eq!(limit.additional, 1);
    let mut policy = policy();
    policy.limits.max_work_units = limit.used + limit.additional;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        insert_nurbs_homogeneous_knot(&ctx, 8, &knots, &controls, 0.0)
            .unwrap()
            .is_none()
    );
    drop(
        ctx.reserve_scoped(SCRATCH_BYTES, "test rejected blend release")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn clamped_knot_insertion_refuses_one_attempt() {
    let knots = [-f64::MAX; 9]
        .into_iter()
        .chain([f64::MAX; 9])
        .collect::<Vec<_>>();
    let source = crate::brep::graph::test_support::test_nurbs_curve(
        8,
        knots,
        vec![cadmpeg_ir::math::Point3::new(0., 0., 0.); 9],
        None,
    );
    let error = crate::test_support::work_refusal_at("insert clamped NURBS knots", |ctx| {
        super::clamp_nurbs_curve_to_domain_lanes(ctx, &source, [0.0, 1.0]).map(|_| ())
    });
    let CodecError::ResourceLimit(limit) = error else {
        panic!("insertion visit refusal");
    };
    assert_eq!(limit.additional, 1);
}

#[test]
fn negative_first_weight_rejects_before_homogeneous_control_tail() {
    let source = crate::brep::graph::test_support::test_nurbs_curve(
        8,
        [0.0; 9].into_iter().chain([1.0; 9]).collect(),
        vec![cadmpeg_ir::math::Point3::new(0., 0., 0.); 9],
        Some([-1.0].into_iter().chain([1.0; 8]).collect()),
    );
    let error = crate::test_support::work_refusal_at("collect homogeneous NURBS controls", |ctx| {
        super::clamp_nurbs_curve_to_domain_lanes(ctx, &source, [0.25, 0.75]).map(|_| ())
    });
    let CodecError::ResourceLimit(limit) = error else {
        panic!("homogeneous visit refusal");
    };
    assert_eq!(limit.additional, 1);
    let mut policy = policy();
    policy.limits.max_work_units = limit.used + limit.additional;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        super::clamp_nurbs_curve_to_domain_lanes(&ctx, &source, [0.25, 0.75])
            .unwrap()
            .is_none()
    );
    drop(
        ctx.reserve_scoped(SCRATCH_BYTES, "test rejected negative weight release")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn clamped_raw_pole_rejection_charges_only_one_control() {
    let controls = [[0.0, 0.0, 0.0, 0.0]; 9];
    let error = crate::test_support::work_refusal_at("collect clamped NURBS controls", |ctx| {
        super::clamped_raw_poles(ctx, &controls, true).map(|_| ())
    });
    let CodecError::ResourceLimit(limit) = error else {
        panic!("raw pole visit refusal");
    };
    assert_eq!(limit.additional, 1);
    let mut policy = policy();
    policy.limits.max_work_units = limit.used + limit.additional;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, storage) = ctx
        .with_scoped_storage("hold test raw pole rejection", || {
            super::clamped_raw_poles(&ctx, &controls, true)
        })
        .unwrap();
    assert!(result.is_none());
    drop(storage);
    drop(
        ctx.reserve_scoped(SCRATCH_BYTES, "test raw pole rejection release")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
}
