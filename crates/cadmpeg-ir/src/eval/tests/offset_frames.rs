// SPDX-License-Identifier: Apache-2.0
use crate::{eval::fitted_nurbs_offset_candidate, math::Point2};

#[test]
fn offset_frames_reject_extreme_perpendicular_tangents() {
    let source = [
        (Point2::new(0.0, 0.0), Point2::new(1e200, 0.0)),
        (Point2::new(1.0, 0.0), Point2::new(1e200, 0.0)),
    ];
    let result = [
        (Point2::new(0.0, 1.0), Point2::new(0.0, 1e200)),
        (Point2::new(1.0, 1.0), Point2::new(0.0, 1e200)),
    ];
    assert!(fitted_nurbs_offset_candidate(source, result, 0.0).is_none());
    let parallel = result.map(|(point, _)| (point, Point2::new(1e200, 0.0)));
    assert_eq!(
        fitted_nurbs_offset_candidate(source, parallel, 0.0),
        Some(1.0)
    );
}
