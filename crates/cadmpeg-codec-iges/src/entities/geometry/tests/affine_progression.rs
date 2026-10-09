// SPDX-License-Identifier: Apache-2.0

use super::super::declared_affine_progression;

#[test]
fn declared_intervals_prove_or_reject_an_affine_control_polygon() {
    crate::test_support::with_service_context(&[], |ctx| {
        assert!(declared_affine_progression(&[0.0, 1.0, 2.0, 3.0], &[0.0; 4], ctx).unwrap());
        assert!(declared_affine_progression(
            &[0.0, 1.000_002, 2.000_004, 3.0],
            &[0.0, 5.0e-6, 5.0e-6, 0.0],
            ctx
        )
        .unwrap());
        assert!(!declared_affine_progression(&[0.0, 1.0, 2.2, 3.0], &[0.0; 4], ctx).unwrap());
    });
}

