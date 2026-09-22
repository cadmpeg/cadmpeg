// SPDX-License-Identifier: Apache-2.0

use super::super::*;
const EPS_DISTANCE: f64 = 1e-7;
#[test]
fn numerical_audit_tessellation_keeps_small_crossings_and_triangles() {
    for d in [1., 1e-4] {
        assert!(segments_intersect(
            Point2::new(-d, 0.),
            Point2::new(d, 0.),
            Point2::new(0., -d),
            Point2::new(0., d),
            EPS_DISTANCE
        ));
        let p = [Point2::new(0., 0.), Point2::new(d, 0.), Point2::new(0., d)];
        assert!(is_simple_polygon(&p, EPS_DISTANCE));
        assert_eq!(triangulate_polygon(&p, EPS_DISTANCE), Some(vec![p]));
        let square = [
            Point2::new(0., 0.),
            Point2::new(d, 0.),
            Point2::new(d, d),
            Point2::new(0., d),
        ];
        assert_eq!(triangulate_polygon(&square, EPS_DISTANCE).unwrap().len(), 2);
    }
}
#[test]
fn numerical_audit_tessellation_keeps_translated_area() {
    for offset in [0., 1e8] {
        let p = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]]
            .map(|p| Point2::new(offset + p[0], offset + p[1]));
        assert_eq!(polygon_area_twice(&p), 2.);
        assert!(is_simple_polygon(&p, EPS_DISTANCE));
    }
}
