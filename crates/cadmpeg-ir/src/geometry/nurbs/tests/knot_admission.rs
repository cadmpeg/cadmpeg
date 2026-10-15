// SPDX-License-Identifier: Apache-2.0
use crate::geometry::nurbs::{admit_knots, require_nondecreasing_knots, KnotVector, NurbsError};

#[test]
fn knot_checks_refuse_non_finite_before_order_with_axis_prefix() {
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
                require_nondecreasing_knots(&knots, prefix),
                Err(NurbsError::Structure(message))
            );
        }
    }
    assert_eq!(
        KnotVector::new(vec![0.0, 2.0, 1.0, f64::NAN]),
        Err(NurbsError::Structure(
            "knots contains a non-finite value".into()
        ))
    );
    assert_eq!(
        KnotVector::new(vec![0.0, 2.0, 1.0]),
        Err(NurbsError::Structure("knots must be non-decreasing".into()))
    );
}

#[test]
fn knot_construction_moves_storage_and_keeps_admitted_knots() {
    let values = vec![0.0, 0.0, 1.0, 1.0];
    let storage = values.as_ptr();
    let knots = KnotVector::new(values).expect("ordered knots");
    assert_eq!(knots.as_ptr(), storage);
    let Ok(retained) = admit_knots(knots, "") else {
        panic!("admitted knots are kept");
    };
    assert_eq!(retained.as_ptr(), storage);
}
