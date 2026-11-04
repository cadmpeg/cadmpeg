// SPDX-License-Identifier: Apache-2.0
//! Comparisons add measured and source representation error bounds.

use super::{point_disagreement, scalar_disagreement, Reading};
use cadmpeg_ir::math::Point3;

#[test]
fn scalar_comparison_uses_numeric_and_model_errors_on_both_sides() {
    let reading = Reading {
        value: 100.0,
        numeric_error: 0.125,
        model_uncertainty: 0.125,
    };
    for declared in [99.75, 100.0, 100.25] {
        assert!(scalar_disagreement("volume", declared, reading, "mm3", 0.0)
            .expect("finite allowance")
            .is_none());
    }
    for declared in [99.5, 100.5] {
        assert!(scalar_disagreement("volume", declared, reading, "mm3", 0.0)
            .expect("finite allowance")
            .is_some());
    }
}

#[test]
fn source_error_extends_scalar_and_centroid_allowances() {
    let scalar = Reading {
        value: 100.0,
        numeric_error: 0.125,
        model_uncertainty: 0.125,
    };
    assert!(scalar_disagreement("volume", 100.75, scalar, "mm3", 0.5)
        .expect("finite allowance")
        .is_none());
    assert!(scalar_disagreement("volume", 100.751, scalar, "mm3", 0.5)
        .expect("finite allowance")
        .is_some());

    let point = Reading {
        value: Point3::new(0.0, 0.0, 0.0),
        numeric_error: 0.125,
        model_uncertainty: 0.125,
    };
    assert!(
        point_disagreement("centroid", Point3::new(0.75, 0.0, 0.0), point, 0.5)
            .expect("finite allowance")
            .is_none()
    );
    assert!(
        point_disagreement("centroid", Point3::new(0.751, 0.0, 0.0), point, 0.5)
            .expect("finite allowance")
            .is_some()
    );
}

#[test]
fn centroid_comparison_uses_euclidean_distance() {
    let reading = Reading {
        value: Point3::new(0.0, 0.0, 0.0),
        numeric_error: 0.125,
        model_uncertainty: 0.125,
    };
    assert!(
        point_disagreement("centroid", Point3::new(0.25, 0.0, 0.0), reading, 0.0)
            .expect("finite allowance")
            .is_none()
    );
    assert!(
        point_disagreement("centroid", Point3::new(0.25, 0.25, 0.0), reading, 0.0)
            .expect("finite allowance")
            .is_some()
    );
}

#[test]
fn overflowed_error_allowance_warns_as_unavailable() {
    let scalar = Reading {
        value: 100.0,
        numeric_error: f64::MAX,
        model_uncertainty: f64::MAX,
    };
    let scalar_gap = scalar_disagreement("volume", 100.0, scalar, "mm3", f64::MAX)
        .expect_err("overflowed scalar allowance");
    assert_eq!(scalar_gap.code, "declared.aggregate-unavailable");

    let point = Reading {
        value: Point3::new(0.0, 0.0, 0.0),
        numeric_error: f64::MAX,
        model_uncertainty: f64::MAX,
    };
    let point_gap = point_disagreement("centroid", Point3::new(0.0, 0.0, 0.0), point, f64::MAX)
        .expect_err("overflowed centroid allowance");
    assert_eq!(point_gap.code, "declared.aggregate-unavailable");
}
