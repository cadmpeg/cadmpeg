// SPDX-License-Identifier: Apache-2.0
//! Polynomial transformations use fixed degree-four stack storage.

use super::super::{polynomial_interval_value_bound, sylvester_polynomial, BoundedCoefficient, QuarticPolynomial, SylvesterEntry};
use super::super::{common_plane_conic_parameters, intersect_two_planes_with_torus, PlaneConicEquation, PlaneEquation, TorusEquation};
use crate::decode::quadratic::Coefficient;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

const EPS_TAYLOR_BOUND: f64 = 1.0e-10;

#[test]
fn interval_derivatives_use_fixed_workspace() {
    let coefficients = [
        BoundedCoefficient { value: 1.0, bound: 0.0 },
        BoundedCoefficient { value: 0.0, bound: 0.0 },
        BoundedCoefficient { value: 0.0, bound: 0.0 },
        BoundedCoefficient { value: 0.0, bound: 0.0 },
        BoundedCoefficient { value: 1.0, bound: 0.0 },
    ];
    let coefficients = QuarticPolynomial::from_slice(&coefficients).expect("quartic");
    let bound = polynomial_interval_value_bound(&coefficients, 0.0, 1.0);
    // At zero, the Taylor displacement bound of 1 + x^4 over radius one is
    // one, plus the finite rounding bound carried by the computation.
    assert!((bound - 1.0).abs() <= EPS_TAYLOR_BOUND);

}

#[test]
fn sylvester_products_use_fixed_workspace() {
    let quadratic = SylvesterEntry { coefficients: [1.0, 2.0, 1.0], len: 3 };
    let one = SylvesterEntry { coefficients: [1.0, 0.0, 0.0], len: 1 };
    let matrix = [
        [Some(quadratic), None, None, None],
        [None, Some(quadratic), None, None],
        [None, None, Some(one), None],
        [None, None, None, Some(one)],
    ];
    let determinant = sylvester_polynomial(&matrix, |sign| sign).expect("quartic product");
    assert_eq!(determinant.as_slice(), [1.0, 4.0, 6.0, 4.0, 1.0]);

}

#[test]
fn torus_quartic_work_is_free_before_output_admission() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_recursion_depth = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 4;
    policy.limits.max_retained_bytes = 4 * std::mem::size_of::<[f64; 3]>() as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let first = PlaneEquation { origin: [0.0; 3], normal: [1.0, 0.0, 0.0] };
    let second = PlaneEquation { origin: [0.0; 3], normal: [0.0, 0.0, 1.0] };
    let torus = TorusEquation { center: [0.0; 3], axis: [0.0, 0.0, 1.0], ref_direction: [1.0, 0.0, 0.0], major_radius: 3.0, minor_radius: 1.0 };
    let points = intersect_two_planes_with_torus(&ctx, first, second, torus)
        .expect("only the four returned points require admission");
    // cross(+x, +z) is -y; ascending line parameters give descending y.
    assert_eq!(points, [[0.0, 4.0, 0.0], [0.0, 2.0, 0.0], [0.0, -2.0, 0.0], [0.0, -4.0, 0.0]]);
}

#[test]
fn conic_quartic_work_is_free_before_output_admission() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_recursion_depth = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 2;
    policy.limits.max_retained_bytes = 4 * std::mem::size_of::<[f64; 2]>() as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let circle = |u, constant| PlaneConicEquation {
        uu: Coefficient::single(1.0), uv: Coefficient::single(0.0), vv: Coefficient::single(1.0),
        u: Coefficient::single(u), v: Coefficient::single(0.0), constant: Coefficient::single(constant),
    };
    let points = common_plane_conic_parameters(&ctx, circle(0.0, -1.0), circle(-2.0, 0.0))
        .expect("only the two returned parameters require admission");
    assert_eq!(points.len(), 2);
    for [u, v] in points {
        assert!((u - 0.5).abs() <= EPS_TAYLOR_BOUND);
        assert!((v.abs() - 0.75_f64.sqrt()).abs() <= EPS_TAYLOR_BOUND);
    }
}

#[test]
fn fixed_quartic_paths_preserve_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let original = ctx.charge_work_limit(1, "original quartic refusal").expect_err("refuse session");
    let plane = PlaneEquation { origin: [0.0; 3], normal: [0.0, 0.0, 1.0] };
    let torus = TorusEquation { center: [0.0; 3], axis: [0.0, 0.0, 1.0], ref_direction: [1.0, 0.0, 0.0], major_radius: 3.0, minor_radius: 1.0 };
    let error = intersect_two_planes_with_torus(&ctx, plane, plane, torus)
        .expect_err("parallel planes preserve refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref refusal) if refusal == &original));
    let zero = PlaneConicEquation { uu: Coefficient::single(0.0), uv: Coefficient::single(0.0), vv: Coefficient::single(0.0), u: Coefficient::single(0.0), v: Coefficient::single(0.0), constant: Coefficient::single(0.0) };
    let error = common_plane_conic_parameters(&ctx, zero, zero).expect_err("empty roots preserve refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref refusal) if refusal == &original));
}
