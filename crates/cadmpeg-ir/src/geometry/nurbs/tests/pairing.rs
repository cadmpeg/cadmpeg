// SPDX-License-Identifier: Apache-2.0

use crate::geometry::nurbs::{NurbsError, NurbsPoleGrid, NurbsPoles3};
use crate::scalar::{FiniteReal, NonZeroReal};

fn curve(form: usize) -> Result<NurbsPoles3<u8>, NurbsError> {
    match form {
        0 => NurbsPoles3::from_lanes(vec![10, 20], Some(vec![1.0, 2.0])),
        1 => NurbsPoles3::from_finite_lanes(
            vec![10, 20],
            Some(vec![FiniteReal::ONE, FiniteReal::new(2.0).expect("finite")]),
        ),
        2 => NurbsPoles3::from_checked_lanes(
            vec![10, 20],
            Some(vec![
                NonZeroReal::ONE,
                NonZeroReal::new(2.0).expect("nonzero"),
            ]),
        ),
        _ => panic!("test form"),
    }
}

fn grid(form: usize) -> Result<NurbsPoleGrid<u8>, NurbsError> {
    let rows = vec![vec![10, 20], vec![30, 40]];
    match form {
        0 => NurbsPoleGrid::from_lanes(rows, Some(vec![vec![1.0, 2.0], vec![3.0, 4.0]])),
        1 => NurbsPoleGrid::from_finite_lanes(
            rows,
            Some(vec![
                vec![FiniteReal::ONE, FiniteReal::new(2.0).expect("finite")],
                vec![
                    FiniteReal::new(3.0).expect("finite"),
                    FiniteReal::new(4.0).expect("finite"),
                ],
            ]),
        ),
        2 => NurbsPoleGrid::from_checked_lanes(
            rows,
            Some(vec![
                vec![NonZeroReal::ONE, NonZeroReal::new(2.0).expect("nonzero")],
                vec![
                    NonZeroReal::new(3.0).expect("nonzero"),
                    NonZeroReal::new(4.0).expect("nonzero"),
                ],
            ]),
        ),
        _ => panic!("test form"),
    }
}

#[test]
fn pairing_checks_source_lane_lengths_before_weights() {
    assert_eq!(
        NurbsPoles3::from_lanes(vec![10_u8, 20], Some(vec![f64::NAN])),
        Err(NurbsError::WeightLaneLength {
            field: "poles".into(),
            poles: 2,
            weights: 1
        })
    );
    assert_eq!(
        NurbsPoleGrid::from_lanes(
            vec![vec![10_u8, 20], vec![30]],
            Some(vec![vec![0.0], vec![f64::NAN]]),
        ),
        Err(NurbsError::WeightLaneLength {
            field: "pole grid row".into(),
            poles: 2,
            weights: 1
        })
    );
    assert_eq!(
        NurbsPoleGrid::from_finite_lanes(
            vec![vec![10_u8], vec![20]],
            Some(vec![vec![FiniteReal::ONE], vec![FiniteReal::ZERO]]),
        ),
        Err(NurbsError::UnusableWeight {
            field: "pole grid row".into(),
            index: 0,
            weight: 0.0
        })
    );
}

#[test]
fn pairing_all_weight_forms_pair_curve_and_grid_lanes() {
    for form in 0..3 {
        let paired = curve(form).expect("valid pairing");
        assert_eq!(paired.points(), vec![10, 20]);
        assert_eq!(paired.weights(), Some(vec![1.0, 2.0]));
        let paired = grid(form).expect("valid pairing");
        assert_eq!(paired.points(), vec![vec![10, 20], vec![30, 40]]);
        assert_eq!(paired.weights(), Some(vec![vec![1.0, 2.0], vec![3.0, 4.0]]));
    }
}

#[test]
fn polynomial_pairing_moves_pole_storage() {
    let points = vec![10_u8, 20];
    let address = points.as_ptr();
    let NurbsPoles3::Polynomial { points } =
        NurbsPoles3::from_lanes(points, None).expect("polynomial")
    else {
        panic!("polynomial");
    };
    assert_eq!(points.as_ptr(), address);
    let rows = vec![vec![30_u8, 40], vec![50, 60]];
    let outer = rows.as_ptr();
    let inner = rows[0].as_ptr();
    let NurbsPoleGrid::Polynomial { rows } =
        NurbsPoleGrid::from_checked_lanes(rows, None).expect("polynomial")
    else {
        panic!("polynomial");
    };
    assert_eq!(rows.as_ptr(), outer);
    assert_eq!(rows[0].as_ptr(), inner);
}
