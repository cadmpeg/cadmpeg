// SPDX-License-Identifier: Apache-2.0
use super::bilinear_surface;
use crate::eval::{
    nurbs_surface_parameter_segment_chord_bound, nurbs_surface_point,
    rational_patch_parameter_segment, RationalBezierSurfacePatch,
};
use crate::math::{Point2, Point3};
use crate::topology::IncreasingParameterInterval;
use crate::units::FinitePoint2;

#[test]
fn rational_patch_rejects_out_of_domain_endpoint_instead_of_moving_it() {
    let patch = RationalBezierSurfacePatch {
        u_domain: IncreasingParameterInterval::new([0.0, 1.0]).unwrap(),
        v_domain: IncreasingParameterInterval::new([0.0, 1.0]).unwrap(),
        u_degree: 1,
        v_degree: 1,
        controls: [[0.0, 0.0, 0.0, 1.0]; 4].to_vec(),
        _scratch: cadmpeg_core::decode::WorkBudget::new(0)
            .reserve_scratch(0, "test patch controls")
            .unwrap(),
    };
    let start = FinitePoint2::new(Point2::new(-0.5, 0.0)).unwrap();
    let end = FinitePoint2::new(Point2::new(0.5, 1.0)).unwrap();
    assert!(rational_patch_parameter_segment(&patch, start, end)
        .expect("resource allocation did not fail")
        .is_none());
}

#[test]
fn nurbs_surface_parameter_segment_bound_contains_curved_diagonal() {
    let mut surface = bilinear_surface();
    surface
        .try_map_control_points(|index, point| {
            let mut mapped = point.get();
            if index == 3 {
                mapped.z = 1.0;
            }
            crate::features::FinitePoint3::new(mapped).ok_or(())
        })
        .unwrap();
    let parameters = [Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)];
    let chord = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0)];
    let bound = nurbs_surface_parameter_segment_chord_bound(&cadmpeg_test_support::service_decode_context(), &surface, parameters, chord)
        .expect("resource allocation did not fail")
        .expect("rational Bézier residual bound");

    assert!(bound >= 1.0 / 3.0);
    assert!(bound < 1.0 / 3.0 + 1.0e-12);
    let reverse_bound = nurbs_surface_parameter_segment_chord_bound(&cadmpeg_test_support::service_decode_context(),
        &surface,
        [parameters[1], parameters[0]],
        [chord[1], chord[0]],
    )
    .expect("resource allocation did not fail")
    .expect("reversed rational Bézier residual bound");
    assert!((reverse_bound - bound).abs() < 1.0e-12);
    for index in 0..=100 {
        let parameter = f64::from(index) / 100.0;
        let point = nurbs_surface_point(&surface, parameter, parameter).expect("surface point");
        let target = Point3::new(parameter, parameter, parameter);
        let distance = (point.x - target.x)
            .hypot(point.y - target.y)
            .hypot(point.z - target.z);
        assert!(distance <= bound);
    }
}

#[test]
fn surface_segment_bound_preserves_session_and_local_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let surface = bilinear_surface();
    let parameters = [Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)];
    let chord = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)];
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits, ResourceDimension::RecursionDepth] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = nurbs_surface_parameter_segment_chord_bound(&ctx, &surface, parameters, chord).unwrap_err();
        assert!(matches!(&error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
        assert_eq!(ctx.finish_session().unwrap_err().to_string(), error.to_string());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(nurbs_surface_parameter_segment_chord_bound(&ctx, &surface, parameters, chord).unwrap().is_some());
    ctx.finish_session().unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let budget = ctx.work_budget(0);
    let error = crate::eval::nurbs_surface_parameter_segment_chord_bound_with_budget(&ctx, &surface, parameters, chord, &budget).unwrap_err();
    assert!(matches!(&error, CodecError::ResourceLimit(limit) if limit.operation == "IR surface segment work"));
    assert_eq!(ctx.finish_session().unwrap_err().to_string(), error.to_string());
}
