// SPDX-License-Identifier: Apache-2.0

use super::outward;

#[test]
fn an_outward_bound_does_not_lie_inside_its_value() {
    for value in [
        0.1,
        -0.1,
        1.0 / 3.0,
        -1.0 / 3.0,
        0.0,
        1e-40,
        -1e-40,
        12_345.678_9,
    ] {
        let low = outward(value, false);
        let high = outward(value, true);
        assert!(f64::from(low) <= value, "{value}");
        assert!(f64::from(high) >= value, "{value}");
    }
    assert_eq!(outward(0.5, true), 0.5);
}
