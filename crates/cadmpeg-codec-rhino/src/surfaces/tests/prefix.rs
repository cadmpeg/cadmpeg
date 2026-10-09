// SPDX-License-Identifier: Apache-2.0
use super::super::{extrusion_rows, sum_nurbs};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::{Point3, Vector3};

fn rational_profile(weight: f64) -> NurbsCurve {
    let mut knots = vec![0.0, 0.0];
    knots.extend((1..32).map(f64::from));
    knots.push(31.0);
    NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        knots,
        vec![Point3::new(0.0, 0.0, 0.0); 32],
        Some(vec![weight; 32]),
        false,
    )
    .expect("fixture admission")
    .expect("finite rational profile")
}

#[test]
fn sum_surface_invalid_first_weight_leaves_product_suffix_unvisited() {
    let first = rational_profile(1.0e200);
    let second = rational_profile(1.0e200);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    policy.limits.max_materialized_bytes = 64 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let error = sum_nurbs(&ctx, &first, &second, Vector3::new(0.0, 0.0, 0.0), 0)
        .expect_err("first weight product overflows");
    assert!(matches!(error, crate::curves::GeometryError::Malformed(_)));
    assert!(error.to_string().contains("sum surface weight is invalid"));
    assert!(ctx.resource_refusal().is_none());
    // The product buffers have been destroyed. No scratch lane reservation survives the error.
    let storage = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "sum source release control",
        )
        .expect("scratch released");
    drop(storage);
    assert_eq!(first.pole_count(), 32);
    assert_eq!(second.pole_count(), 32);
}

#[test]
fn sum_surface_first_inner_visit_preserves_work_refusal() {
    let first = rational_profile(1.0);
    let second = rational_profile(1.0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let crate::curves::GeometryError::Codec(CodecError::ResourceLimit(limit)) =
        sum_nurbs(&ctx, &first, &second, Vector3::new(0.0, 0.0, 0.0), 0)
            .expect_err("first inner visit refuses")
    else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "Rhino sum nurbs traversal");
    assert_eq!((limit.used, limit.additional), (1, 1));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

#[test]
fn sum_surface_complete_rational_grid_preserves_dimensions_and_weights() {
    let first = rational_profile(2.0);
    let second = rational_profile(3.0);
    super::with_test_context(|ctx| {
        let surface = sum_nurbs(ctx, &first, &second, Vector3::new(1.0, 2.0, 3.0), 0)
            .expect("finite tensor control");
        assert_eq!((surface.u_count(), surface.v_count()), (32, 32));
        assert!(surface
            .weights()
            .expect("rational weights")
            .iter()
            .flatten()
            .all(|weight| weight.get() == 6.0));
        assert!(surface
            .poles()
            .iter()
            .all(|point| point.get() == Point3::new(1.0, 2.0, 3.0)));
    });
}

#[test]
fn extrusion_rows_storage_refusal_leaves_later_rows_unvisited() {
    let point = FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite point");
    let first = [point; 8];
    let second = [point; 8];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The eight outer Vec slots fit; the first inner row requires additional backing.
    policy.limits.max_retained_bytes =
        u64::try_from(8 * std::mem::size_of::<Vec<FinitePoint3>>()).expect("fixture size");
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let crate::curves::GeometryError::Codec(CodecError::ResourceLimit(limit)) =
        extrusion_rows(&ctx, &first, &second).expect_err("first inner row refuses")
    else {
        panic!("storage refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "Rhino extrusion surface rows");
    assert_eq!(limit.used, policy.limits.max_retained_bytes);
    assert!(limit.additional > 0);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

#[test]
fn extrusion_rows_complete_at_exact_source_visit_limit() {
    let first = [FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite point"); 8];
    let second = [FinitePoint3::new(Point3::new(4.0, 5.0, 6.0)).expect("finite point"); 8];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 8;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let rows = extrusion_rows(&ctx, &first, &second).expect("all eight pairs fit");
    assert_eq!(rows.len(), 8);
    assert!(rows.iter().all(|row| row == &[first[0], second[0]]));
    ctx.finish_session().expect("no exhaustion visit");
}
