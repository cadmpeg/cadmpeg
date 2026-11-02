// SPDX-License-Identifier: Apache-2.0
//! Comparisons use the measurement's additive error allowance.

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
        assert!(scalar_disagreement("volume", declared, reading, "mm3").is_none());
    }
    for declared in [99.5, 100.5] {
        assert!(scalar_disagreement("volume", declared, reading, "mm3").is_some());
    }
}

#[test]
fn centroid_comparison_uses_euclidean_distance() {
    let reading = Reading {
        value: Point3::new(0.0, 0.0, 0.0),
        numeric_error: 0.125,
        model_uncertainty: 0.125,
    };
    assert!(point_disagreement("centroid", Point3::new(0.25, 0.0, 0.0), reading).is_none());
    assert!(point_disagreement("centroid", Point3::new(0.25, 0.25, 0.0), reading).is_some());
}
