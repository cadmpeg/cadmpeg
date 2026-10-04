// SPDX-License-Identifier: Apache-2.0

use super::{cylinder_from_single_cap_outline, CapOutline, ExtrusionSpan};

#[test]
fn extrusion_span_admits_a_finite_interval_that_contains_the_section() {
    let span = ExtrusionSpan::new(-1.5, 2.0).expect("interval containing the section");
    assert_eq!((span.lower(), span.upper()), (-1.5, 2.0));
    assert!(ExtrusionSpan::new(0.0, 3.0).is_some());
    assert!(ExtrusionSpan::new(-3.0, 0.0).is_some());

    assert!(ExtrusionSpan::new(0.0, 0.0).is_none());
    assert!(ExtrusionSpan::new(1.0, 2.0).is_none());
    assert!(ExtrusionSpan::new(-2.0, -1.0).is_none());
    assert!(ExtrusionSpan::new(2.0, -1.0).is_none());
    assert!(ExtrusionSpan::new(f64::NAN, 1.0).is_none());
    assert!(ExtrusionSpan::new(-1.0, f64::NAN).is_none());
    assert!(ExtrusionSpan::new(f64::NEG_INFINITY, 0.0).is_none());
    assert!(ExtrusionSpan::new(0.0, f64::INFINITY).is_none());
    assert!(ExtrusionSpan::new(-f64::MAX, f64::MAX).is_none());
}

#[test]
fn cap_outline_cylinder_refuses_an_overflowing_radius() {
    assert!(cylinder_from_single_cap_outline(CapOutline {
        surface_id: 46,
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        corners: [[-f64::MAX, -f64::MAX, 0.0], [f64::MAX, f64::MAX, 0.0]],
    })
    .is_none());
}
