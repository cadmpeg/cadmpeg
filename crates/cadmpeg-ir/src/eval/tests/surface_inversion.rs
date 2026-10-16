// SPDX-License-Identifier: Apache-2.0
//! Surface inversion regressions.

use super::bilinear_surface;
use super::nurbs_surface_parameter_near_point;
use super::nurbs_surface_parameter_within_tolerance;
use super::nurbs_surface_parameter_within_tolerance_with_budget;
use super::NurbsSurface;
use super::NurbsSurfaceAxis;
use super::NurbsSurfaceLanes;
use super::Point2;
use super::Point3;
use super::WorkBudget;

const EPS_INVERSE_CONTRACT_MARGIN: f64 = 1.0e-12;
const EPS_SURFACE_INVERSE_FIT: f64 = 1.0e-10;

#[test]
fn nurbs_surface_inverse_distinguishes_closest_and_tolerance_contracts() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.2);
    let closest = crate::eval::nurbs_surface_closest_parameter_with_budget(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        None,
        &cadmpeg_core::decode::WorkBudget::new(crate::eval::DEFAULT_NURBS_SURFACE_INVERSION_WORK),
    )
    .expect("find the closest NURBS surface parameters")
    .expect("closest surface parameter");
    assert!((closest.u - 0.3).abs() < 1.0e-12);
    assert!((closest.v - 0.7).abs() < 1.0e-12);
    assert!(nurbs_surface_parameter_within_tolerance(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        None,
        0.19
    )
    .expect("find the NURBS surface parameters within tolerance")
    .is_none());
    assert!(nurbs_surface_parameter_within_tolerance(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        None,
        0.2 + EPS_INVERSE_CONTRACT_MARGIN
    )
    .expect("find the NURBS surface parameters within tolerance")
    .is_some());
}

#[test]
fn nurbs_surface_inverse_admits_its_tolerance_before_the_search() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.2);
    let budget = || WorkBudget::new(crate::eval::DEFAULT_NURBS_SURFACE_INVERSION_WORK);
    for tolerance in [-1.0e-12, f64::NAN, f64::INFINITY] {
        assert!(nurbs_surface_parameter_within_tolerance_with_budget(
            &cadmpeg_test_support::service_decode_context(),
            &surface,
            point,
            None,
            tolerance,
            &budget(),
        )
        .expect("find the NURBS surface parameters within tolerance")
        .is_none());
    }
    let tolerance = crate::scalar::NonNegativeReal::new(0.2 + 1.0e-12).unwrap();
    let parameters = crate::eval::nurbs_surface_parameter_within_nonnegative_tolerance_with_budget(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        None,
        tolerance,
        &budget(),
    )
    .expect("compute the NURBS surface parameter within nonnegative tolerance");
    assert!(parameters.is_some());
    assert_eq!(
        parameters,
        nurbs_surface_parameter_within_tolerance_with_budget(
            &cadmpeg_test_support::service_decode_context(),
            &surface,
            point,
            None,
            tolerance.get(),
            &budget(),
        )
        .expect("find the NURBS surface parameters within tolerance")
    );
}

#[test]
fn budgeted_nurbs_surface_inverse_stops_before_unbounded_patch_work() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.0);
    let budget = WorkBudget::new(0);

    assert!(nurbs_surface_parameter_within_tolerance_with_budget(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        None,
        EPS_SURFACE_INVERSE_FIT,
        &budget,
    )
    .expect("find the NURBS surface parameters within tolerance")
    .is_none());
    assert!(budget.exhausted());

    let budget = WorkBudget::new(10_000);
    let parameters = nurbs_surface_parameter_within_tolerance_with_budget(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        None,
        EPS_SURFACE_INVERSE_FIT,
        &budget,
    )
    .expect("find the NURBS surface parameters within tolerance")
    .expect("a valid surface fits within a larger caller-owned budget");
    assert!((parameters.u - 0.3).abs() < 1.0e-12);
    assert!((parameters.v - 0.7).abs() < 1.0e-12);
    assert!(budget.consumed() > 0);
}

#[test]
fn budgeted_nurbs_surface_inverse_accepts_a_fit_qualified_seed_first() {
    const FIT_TOLERANCE: f64 = 1.0e-12;

    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.0);
    let exhausted_ctx = cadmpeg_test_support::service_decode_context();
    let exhausted_budget = WorkBudget::new(12);
    let exhausted = nurbs_surface_parameter_within_tolerance_with_budget(
        &exhausted_ctx,
        &surface,
        point,
        Some(Point2::new(0.3, 0.7)),
        FIT_TOLERANCE,
        &exhausted_budget,
    );
    // The original cap cannot admit all pole traversals. The local slice
    // runs out, the solve has no value, and the session continues.
    assert_eq!(exhausted, Ok(None));
    assert!(exhausted_budget.exhausted());
    assert_eq!(
        crate::eval::admission::EvaluationAdmission::Decode(&exhausted_ctx)
            .within_work_slice(&exhausted_budget, |admission| {
                crate::eval::decode::nurbs_surface_point(admission, &surface, 0.3, 0.7)
            }),
        Err(crate::eval::EvaluationFailure::WorkExhausted)
    );
    assert_eq!(exhausted_ctx.resource_refusal(), None);
    assert!(exhausted_ctx.finish_session().is_ok());
    let budget = WorkBudget::new(20);
    let parameters = nurbs_surface_parameter_within_tolerance_with_budget(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        Some(Point2::new(0.3, 0.7)),
        FIT_TOLERANCE,
        &budget,
    )
    .expect("find the NURBS surface parameters within tolerance")
    .expect("a fit-qualified continuation seed does not need global search");

    assert_eq!(parameters, Point2::new(0.3, 0.7));
    // Four poles enter each of four coordinate sums and the constant scan.
    assert_eq!(budget.consumed(), 4 * 5);
}

#[test]
fn budgeted_nurbs_surface_inverse_refines_an_approximate_seed_before_global_search() {
    const FIT_TOLERANCE: f64 = 1.0e-10;
    const PARAMETER_TOLERANCE: f64 = 1.0e-12;

    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.0);
    let exhausted_ctx = cadmpeg_test_support::service_decode_context();
    let exhausted_budget = WorkBudget::new(256);
    let exhausted = nurbs_surface_parameter_within_tolerance_with_budget(
        &exhausted_ctx,
        &surface,
        point,
        Some(Point2::new(0.29, 0.69)),
        FIT_TOLERANCE,
        &exhausted_budget,
    );
    // The original cap cannot admit all pole traversals. The local slice
    // runs out, the solve has no value, and the session continues.
    assert_eq!(exhausted, Ok(None));
    assert!(exhausted_budget.exhausted());
    assert_eq!(
        crate::eval::admission::EvaluationAdmission::Decode(&exhausted_ctx)
            .within_work_slice(&exhausted_budget, |admission| {
                crate::eval::decode::nurbs_surface_point(admission, &surface, 0.3, 0.7)
            }),
        Err(crate::eval::EvaluationFailure::WorkExhausted)
    );
    assert_eq!(exhausted_ctx.resource_refusal(), None);
    assert!(exhausted_ctx.finish_session().is_ok());
    let budget = WorkBudget::new(32768);
    let parameters = nurbs_surface_parameter_within_tolerance_with_budget(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        Some(Point2::new(0.29, 0.69)),
        FIT_TOLERANCE,
        &budget,
    )
    .expect("find the NURBS surface parameters within tolerance")
    .expect("a nearby seed should be refined before global patch search");

    assert!((parameters.u - 0.3).abs() <= PARAMETER_TOLERANCE);
    assert!((parameters.v - 0.7).abs() <= PARAMETER_TOLERANCE);
    assert!(budget.consumed() > 0);
}

#[test]
fn nurbs_surface_local_inverse_returns_a_forward_checked_candidate() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.2);
    let parameters = nurbs_surface_parameter_near_point(
        crate::eval::admission::EvaluationAdmission::Standard,
        &surface,
        point,
        None,
    )
    .expect("compute the NURBS surface parameter near point")
    .expect("bounded local surface candidate");
    let mapped = crate::eval::decode::nurbs_surface_point(
        crate::eval::admission::EvaluationAdmission::Standard,
        &surface,
        parameters.u,
        parameters.v,
    )
    .expect("surface point");
    assert!(mapped.distance(point) <= 0.2 + f64::EPSILON * 1024.0);
    assert!((parameters.u - 0.3).abs() < f64::EPSILON * 1024.0);
    assert!((parameters.v - 0.7).abs() < f64::EPSILON * 1024.0);
}

#[test]
fn nurbs_surface_inverse_handles_rational_internal_spans() {
    let surface = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(0.5, 0.0, 0.2), Point3::new(0.5, 1.0, 0.2)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            Some(vec![1.0, 1.0, 0.7, 0.7, 1.0, 1.0])
                .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
        ),
        false,
    )
    .unwrap();
    let point = crate::eval::decode::nurbs_surface_point(
        crate::eval::admission::EvaluationAdmission::Standard,
        &surface,
        0.75,
        0.4,
    )
    .expect("surface point");
    let parameters = nurbs_surface_parameter_within_tolerance(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point.get(),
        None,
        EPS_SURFACE_INVERSE_FIT,
    )
    .expect("find the NURBS surface parameters within tolerance")
    .expect("rational multi-span inverse");
    assert!((parameters.u - 0.75).abs() < 1.0e-9);
    assert!((parameters.v - 0.4).abs() < 1.0e-9);
}
