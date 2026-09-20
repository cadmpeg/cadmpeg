// SPDX-License-Identifier: Apache-2.0
//! Surface inversion regressions.

use super::*;

#[test]
fn nurbs_surface_inverse_distinguishes_closest_and_tolerance_contracts() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.2);
    let closest = crate::eval::nurbs_surface_closest_parameter_with_budget(
        &surface,
        point,
        None,
        &cadmpeg_core::decode::WorkBudget::new(crate::eval::DEFAULT_NURBS_SURFACE_INVERSION_WORK),
    )
    .expect("closest surface parameter");
    assert!((closest.u - 0.3).abs() < 1.0e-12);
    assert!((closest.v - 0.7).abs() < 1.0e-12);
    assert!(nurbs_surface_parameter_within_tolerance(&surface, point, None, 0.19).is_none());
    assert!(
        nurbs_surface_parameter_within_tolerance(&surface, point, None, 0.2 + 1.0e-12).is_some()
    );
}

#[test]
fn budgeted_nurbs_surface_inverse_stops_before_unbounded_patch_work() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.0);
    let budget = WorkBudget::new(0);

    assert!(nurbs_surface_parameter_within_tolerance_with_budget(
        &surface, point, None, 1.0e-10, &budget,
    )
    .is_none());
    assert!(budget.exhausted());

    let budget = WorkBudget::new(10_000);
    let parameters = nurbs_surface_parameter_within_tolerance_with_budget(
        &surface, point, None, 1.0e-10, &budget,
    )
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
    let budget = WorkBudget::new(12);
    let parameters = nurbs_surface_parameter_within_tolerance_with_budget(
        &surface,
        point,
        Some(Point2::new(0.3, 0.7)),
        FIT_TOLERANCE,
        &budget,
    )
    .expect("a fit-qualified continuation seed does not need global search");

    assert_eq!(parameters, Point2::new(0.3, 0.7));
    assert_eq!(budget.consumed(), 12);
}

#[test]
fn budgeted_nurbs_surface_inverse_refines_an_approximate_seed_before_global_search() {
    const FIT_TOLERANCE: f64 = 1.0e-10;
    const PARAMETER_TOLERANCE: f64 = 1.0e-12;

    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.0);
    let budget = WorkBudget::new(256);
    let parameters = nurbs_surface_parameter_within_tolerance_with_budget(
        &surface,
        point,
        Some(Point2::new(0.29, 0.69)),
        FIT_TOLERANCE,
        &budget,
    )
    .expect("a nearby seed should be refined before global patch search");

    assert!((parameters.u - 0.3).abs() <= PARAMETER_TOLERANCE);
    assert!((parameters.v - 0.7).abs() <= PARAMETER_TOLERANCE);
    assert!(budget.consumed() > 0);
}

#[test]
fn nurbs_surface_local_inverse_returns_a_forward_checked_candidate() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.2);
    let parameters = nurbs_surface_parameter_near_point(&surface, point, None)
        .expect("bounded local surface candidate");
    let mapped = nurbs_surface_point(&surface, parameters.u, parameters.v).expect("surface point");
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
    let point = nurbs_surface_point(&surface, 0.75, 0.4).expect("surface point");
    let parameters = nurbs_surface_parameter_within_tolerance(&surface, point, None, 1.0e-10)
        .expect("rational multi-span inverse");
    assert!((parameters.u - 0.75).abs() < 1.0e-9);
    assert!((parameters.v - 0.4).abs() < 1.0e-9);
}
