// SPDX-License-Identifier: Apache-2.0
use crate::geometry::nurbs::admitted::ConstructionError;
use crate::geometry::nurbs::{
    admit_knots, require_nondecreasing_knots, KnotVector, NurbsError, StandardNurbsAdmission,
};

#[test]
fn knot_checks_refuse_non_finite_before_order_with_axis_prefix() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for prefix in ["", "u_", "v_"] {
        for (knots, message) in [
            (
                vec![f64::NAN, 1.0, 0.0, 1.0],
                format!("{prefix}knots contains a non-finite value"),
            ),
            (
                vec![0.0, 1.0, 0.0, 1.0],
                format!("{prefix}knots must be non-decreasing"),
            ),
        ] {
            assert_eq!(
                require_nondecreasing_knots(&StandardNurbsAdmission, &knots, prefix),
                Err(NurbsError::Structure(message.clone()))
            );
            assert!(matches!(
                require_nondecreasing_knots(&ctx, &knots, prefix),
                Err(ConstructionError::Geometry(NurbsError::Structure(text))) if text == message
            ));
        }
    }
    assert_eq!(
        KnotVector::new(&ctx, vec![0.0, 2.0, 1.0, f64::NAN]).expect("knot admission"),
        Err(NurbsError::Structure(
            "knots contains a non-finite value".into()
        ))
    );
    assert_eq!(
        KnotVector::new(&ctx, vec![0.0, 2.0, 1.0]).expect("knot admission"),
        Err(NurbsError::Structure("knots must be non-decreasing".into()))
    );
}

#[test]
fn knot_construction_moves_storage_and_keeps_admitted_knots() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let values = vec![0.0, 0.0, 1.0, 1.0];
    let storage = values.as_ptr();
    let knots = KnotVector::new(&ctx, values)
        .expect("knot admission")
        .expect("ordered knots");
    assert_eq!(knots.as_ptr(), storage);
    let Ok(retained) = admit_knots(&ctx, knots, "") else {
        panic!("admitted knots are kept");
    };
    assert_eq!(retained.as_ptr(), storage);
}
