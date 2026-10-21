// SPDX-License-Identifier: Apache-2.0

use super::{classify_carrier, Carrier};

#[test]
fn native_iso_values_distinguish_interior_and_all_four_domain_sides() {
    let domain = [[0.0, 1.0]; 2];
    for (from, to, expected) in [
        ([0.5, 0.0], [0.5, 1.0], 1),
        ([0.0, 0.5], [1.0, 0.5], 2),
        ([0.0, 0.0], [0.0, 1.0], 3),
        ([0.0, 0.0], [1.0, 0.0], 4),
        ([1.0, 0.0], [1.0, 1.0], 5),
        ([0.0, 1.0], [1.0, 1.0], 6),
        ([0.0, 0.0], [1.0, 1.0], 0),
    ] {
        assert_eq!(classify_carrier(&Carrier::line(from, to), domain), expected);
        assert_eq!(classify_carrier(&Carrier::line(to, from), domain), expected);
    }
}

#[test]
fn native_iso_classification_uses_the_written_domain() {
    let curve = Carrier::line([0.5, 0.0], [0.5, 1.0]);
    assert_eq!(classify_carrier(&curve, [[0.0, 1.0]; 2]), 1);
    assert_eq!(classify_carrier(&curve, [[0.5, 1.5], [0.0, 1.0]]), 3);
    assert_eq!(classify_carrier(&curve, [[-0.5, 0.5], [0.0, 1.0]]), 5);
}

#[test]
fn iso_bounding_box_does_not_admit_a_retracing_or_unclamped_curve() {
    let mut curve = Carrier {
        points: vec![[0.5, 0.0], [0.5, 0.75], [0.5, 0.25], [0.5, 1.0]],
        clamped: true,
    };
    assert_eq!(classify_carrier(&curve, [[0.0, 1.0]; 2]), 0);
    curve.points.swap(1, 2);
    assert_eq!(classify_carrier(&curve, [[0.0, 1.0]; 2]), 1);
    curve.clamped = false;
    assert_eq!(classify_carrier(&curve, [[0.0, 1.0]; 2]), 0);
}
