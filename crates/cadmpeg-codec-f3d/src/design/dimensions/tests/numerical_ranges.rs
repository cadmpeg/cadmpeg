// SPDX-License-Identifier: Apache-2.0
use crate::design::dimensions::{line_angle_matches, parallel_line_offset};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
fn line(start: Point2, end: Point2) -> SketchGeometry {
    SketchGeometryDefinition::Line { start, end }
        .try_into()
        .unwrap()
}
#[test]
fn numerical_ranges_line_angle_is_independent_of_lengths() {
    for length in [1e-10, 1.0, 1e200] {
        let a = line(Point2::new(0., 0.), Point2::new(length, 0.));
        let b = line(Point2::new(0., 0.), Point2::new(length, length));
        assert!(line_angle_matches(&a, &b, std::f64::consts::FRAC_PI_4));
        assert!(!line_angle_matches(&a, &b, 0.));
    }
}
#[test]
fn numerical_ranges_offset_rejects_huge_perpendicular_lines() {
    let a = line(Point2::new(0., 0.), Point2::new(1e200, 0.));
    let b = line(Point2::new(0., 1.), Point2::new(0., 1e200));
    assert_eq!(parallel_line_offset(&a, &b), None);
    let c = line(Point2::new(0., 1.), Point2::new(1e200, 1.));
    assert_eq!(parallel_line_offset(&a, &c), Some(1.));
}
