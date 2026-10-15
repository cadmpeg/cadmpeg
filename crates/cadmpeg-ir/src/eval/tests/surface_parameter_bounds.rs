// SPDX-License-Identifier: Apache-2.0
use super::bilinear_surface;
use crate::eval::nurbs_surface_parameter_segment_chord_bound;
use crate::eval::rational_patch_parameter_segment;
use crate::eval::RationalBezierSurfacePatch;
use crate::math::{Point2, Point3};
use crate::topology::IncreasingParameterInterval;
use crate::units::FinitePoint2;
use cadmpeg_core::decode::WorkBudget;

#[test]
fn rational_patch_rejects_out_of_domain_endpoint_instead_of_moving_it() {
    let patch = RationalBezierSurfacePatch {
        u_domain: IncreasingParameterInterval::new([0.0, 1.0]).unwrap(),
        v_domain: IncreasingParameterInterval::new([0.0, 1.0]).unwrap(),
        u_degree: 1,
        v_degree: 1,
        controls: [[0.0, 0.0, 0.0, 1.0]; 4].to_vec(),
    };
    let start = FinitePoint2::new(Point2::new(-0.5, 0.0)).unwrap();
    let end = FinitePoint2::new(Point2::new(0.5, 1.0)).unwrap();
    assert!(rational_patch_parameter_segment(&patch, start, end).is_none());
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
    let bound = nurbs_surface_parameter_segment_chord_bound(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        parameters,
        chord,
    )
    .expect("resource allocation did not fail")
    .expect("rational Bézier residual bound");

    assert!(bound >= 1.0 / 3.0);
    assert!(bound < 1.0 / 3.0 + 1.0e-12);
    let reverse_bound = nurbs_surface_parameter_segment_chord_bound(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        [parameters[1], parameters[0]],
        [chord[1], chord[0]],
    )
    .expect("resource allocation did not fail")
    .expect("reversed rational Bézier residual bound");
    assert!((reverse_bound - bound).abs() < 1.0e-12);
    for index in 0..=100 {
        let parameter = f64::from(index) / 100.0;
        let point = crate::eval::decode::nurbs_surface_point(
            crate::eval::admission::EvaluationAdmission::Standard,
            &surface,
            parameter,
            parameter,
        )
        .expect("surface point");
        let target = Point3::new(parameter, parameter, parameter);
        let distance = (point.x - target.x)
            .hypot(point.y - target.y)
            .hypot(point.z - target.z);
        assert!(distance <= bound);
    }
}

#[test]
fn surface_segment_bound_refuses_zero_depth_and_keeps_slice_exhaustion_local() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let surface = bilinear_surface();
    let parameters = [Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)];
    let chord = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        nurbs_surface_parameter_segment_chord_bound(&ctx, &surface, parameters, chord).unwrap_err();
    assert!(
        matches!(&error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RecursionDepth)
    );
    assert_eq!(
        ctx.finish_session().unwrap_err().to_string(),
        error.to_string()
    );

    let ctx = cadmpeg_test_support::service_decode_context();
    let budget = WorkBudget::new(0);
    let bound = crate::eval::nurbs_surface_parameter_segment_chord_bound_with_budget(
        &ctx, &surface, parameters, chord, &budget,
    )
    .expect("an exhausted caller budget is not a session refusal");
    assert!(bound.is_none());
    assert!(budget.exhausted());
    ctx.finish_session().unwrap();
}

#[test]
fn surface_patches_extract_split_and_bound_the_bilinear_net() {
    let surface = bilinear_surface();
    let ctx = cadmpeg_test_support::service_decode_context();
    let budget = WorkBudget::new(1_000_000);
    let patches =
        crate::eval::rational_surface_patches_with_budget(&surface, &budget).expect("active patch");
    assert_eq!(patches.len(), 1);
    let patch = &patches[0];
    assert_eq!(patch.u_domain.endpoints(), [0.0, 1.0]);
    assert_eq!(patch.v_domain.endpoints(), [0.0, 1.0]);
    assert_eq!(
        patch.controls,
        [
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 0.0, 1.0],
        ]
    );
    for split_u in [false, true] {
        let children = crate::eval::split_rational_surface_patch(patch, split_u, &budget)
            .expect("divisible patch");
        for (side, child) in children.iter().enumerate() {
            let range = if side == 0 { [0.0, 0.5] } else { [0.5, 1.0] };
            assert_eq!(
                child.u_domain.endpoints(),
                if split_u { range } else { [0.0, 1.0] }
            );
            assert_eq!(
                child.v_domain.endpoints(),
                if split_u { [0.0, 1.0] } else { range }
            );
            let u = child.u_domain.endpoints();
            let v = child.v_domain.endpoints();
            for (index, control) in child.controls.iter().enumerate() {
                assert_eq!(*control, [u[index / 2], v[index % 2], 0.0, 1.0]);
            }
        }
    }
    let (lower, diameter) = crate::eval::rational_patch_distance_bounds_with_budget(patch, &budget)
        .expect("finite hull");
    assert_eq!(lower, 0.0);
    assert!((diameter - 2.0_f64.sqrt()).abs() <= 4.0 * f64::EPSILON);
    let exhausted = WorkBudget::new(0);
    assert!(crate::eval::rational_surface_patches_with_budget(&surface, &exhausted).is_none());
    ctx.finish_session()
        .expect("local exhaustion is not a session refusal");
}

#[test]
fn surface_start_search_lands_on_the_projected_parameter_for_both_contracts() {
    let surface = bilinear_surface();
    let ctx = cadmpeg_test_support::service_decode_context();
    for tolerance in [None, Some(0.0)] {
        let budget = WorkBudget::new(1_000_000);
        let point = Point3::new(0.3, 0.7, if tolerance.is_some() { 0.0 } else { 0.2 });
        let starts = crate::eval::complete_nurbs_surface_starts(
            &ctx, &surface, point, None, tolerance, &budget,
        )
        .expect("no session refusal")
        .expect("surface starts");
        assert!(!starts.is_empty());
        for start in &starts {
            assert!((start.u - 0.3).abs() <= 64.0 * f64::EPSILON);
            assert!((start.v - 0.7).abs() <= 64.0 * f64::EPSILON);
        }
    }
    ctx.finish_session().unwrap();
}
