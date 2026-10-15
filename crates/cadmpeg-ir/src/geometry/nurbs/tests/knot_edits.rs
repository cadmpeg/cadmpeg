// SPDX-License-Identifier: Apache-2.0
use crate::geometry::nurbs::NurbsError;

#[test]
fn knot_edit_applies_the_edit_and_keeps_poles() {
    let original = super::curve();
    let mut curve = original.clone();
    curve
        .edit_knots(|knots| {
            for knot in knots {
                *knot += 2.0;
            }
        })
        .expect("valid edit");
    assert_eq!(
        curve.knots().as_slice(),
        original
            .knots()
            .iter()
            .map(|knot| knot + 2.0)
            .collect::<Vec<_>>()
    );
    assert_eq!(curve.pole_rows(), original.pole_rows());
}

#[test]
fn knot_edit_refuses_non_finite_before_order_and_keeps_the_curve() {
    let mut curve = super::curve();
    let before = curve.clone();
    assert_eq!(
        curve.edit_knots(|knots| {
            knots.reverse();
            knots[0] = f64::INFINITY;
        },),
        Err(NurbsError::Structure(
            "knots contains a non-finite value".into()
        ))
    );
    assert_eq!(curve, before);
}

#[test]
fn knot_replacement_moves_the_new_knots_and_keeps_poles() {
    let original = super::curve();
    let knots = original
        .knots()
        .iter()
        .map(|knot| knot + 2.0)
        .collect::<Vec<_>>();
    let address = knots.as_ptr();
    let curve = original
        .clone()
        .with_knots(knots)
        .expect("valid replacement");
    assert_eq!(curve.knots().as_ptr(), address);
    assert_eq!(curve.pole_rows(), original.pole_rows());
}
