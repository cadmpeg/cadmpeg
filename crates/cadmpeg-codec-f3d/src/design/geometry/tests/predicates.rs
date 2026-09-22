// SPDX-License-Identifier: Apache-2.0

use super::super::*;
#[test]
fn numerical_audit_area_keeps_translated_square() {
    for offset in [0., 1e8] {
        let p = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]]
            .map(|p| Point2::new(offset + p[0], offset + p[1]));
        assert_eq!(signed_polygon_area(&p), 1.);
    }
}
#[test]
fn numerical_audit_line_arc_does_not_keep_outside_root() {
    let arc = ProfileBoundarySegment::Arc {
        center: Point2::new(0., 0.),
        radius: 0.001,
        start_angle: 0.,
        end_angle: std::f64::consts::TAU,
    };
    for start in [-1., -1e20] {
        assert_eq!(
            line_arc_intersection_points((Point2::new(start, 0.), Point2::new(0., 0.)), &arc),
            Some(vec![Point2::new(-0.001, 0.)])
        );
    }
}
