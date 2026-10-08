// SPDX-License-Identifier: Apache-2.0
//! Polynomial transformations hold the source and destination workspaces only.

use super::super::{polynomial_interval_value_bound, sylvester_polynomial, BoundedCoefficient, SylvesterEntry};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

const EPS_TAYLOR_BOUND: f64 = 1.0e-10;

#[test]
fn interval_derivatives_release_replaced_workspace() {
    let coefficients = [
        BoundedCoefficient { value: 1.0, bound: 0.0 },
        BoundedCoefficient { value: 0.0, bound: 0.0 },
        BoundedCoefficient { value: 0.0, bound: 0.0 },
        BoundedCoefficient { value: 0.0, bound: 0.0 },
        BoundedCoefficient { value: 1.0, bound: 0.0 },
    ];
    // At most two coefficient vectors coexist. Geometric vector growth fits
    // each within the next power of two of the original coefficient count.
    let bytes = 2 * coefficients.len().next_power_of_two()
        * std::mem::size_of::<BoundedCoefficient>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::try_from(bytes).expect("small workspace");
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let bound = polynomial_interval_value_bound(&ctx, &coefficients, 0.0, 1.0)
        .expect("successive derivatives reuse workspace");
    // At zero, the Taylor displacement bound of 1 + x^4 over radius one is
    // one, plus the finite rounding bound carried by the computation.
    assert!((bound - 1.0).abs() <= EPS_TAYLOR_BOUND);
    ctx.reserve_scoped(policy.limits.max_materialized_bytes, "released derivative workspace")
        .expect("all coefficient workspaces are released");
}

#[test]
fn sylvester_products_release_replaced_workspace() {
    let quadratic = SylvesterEntry { coefficients: [1.0, 2.0, 1.0], len: 3 };
    let one = SylvesterEntry { coefficients: [1.0, 0.0, 0.0], len: 1 };
    let matrix = [
        [Some(quadratic), None, None, None],
        [None, Some(quadratic), None, None],
        [None, None, Some(one), None],
        [None, None, None, Some(one)],
    ];
    // The product is a quartic: source and destination each need at most
    // five scalar slots while one multiplication executes.
    let bytes = 2 * 5 * std::mem::size_of::<f64>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::try_from(bytes).expect("small workspace");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let determinant = sylvester_polynomial(&ctx, &matrix, |sign| sign)
        .expect("successive products reuse workspace");
    assert_eq!(determinant, [1.0, 4.0, 6.0, 4.0, 1.0]);
    ctx.reserve_scoped(policy.limits.max_materialized_bytes, "released product workspace")
        .expect("all product workspaces are released");
}
