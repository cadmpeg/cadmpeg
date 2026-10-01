// SPDX-License-Identifier: Apache-2.0
use super::{ProfileEntity, ValidatedProfile};
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::SketchGeometry;

fn polygon_curve(points: &[[f64; 2]]) -> ProfileEntity {
    let mut knots = vec![0.0, 0.0];
    for index in 1..points.len() - 1 { knots.push(cadmpeg_core::convert::f64_from_index(index).expect("index")); }
    knots.push(cadmpeg_core::convert::f64_from_index(points.len() - 1).expect("index"));
    let last = *knots.last().expect("knots");
    knots.push(last);
    let curve = PcurveNurbs::from_lanes(1, knots,
        points.iter().map(|p| Point2::new(p[0], p[1])).collect(), None, false).expect("curve");
    crate::decode::with_test_decode_ctx(|ctx| ProfileEntity::new(ctx, SketchGeometry::nurbs(curve), false)).expect("admission").expect("finite endpoints")
}

#[test]
fn single_nurbs_crossing_spans_cannot_mint_validated_profile() {
    let entity = polygon_curve(&[[0.0,0.0],[3.0,3.0],[0.0,3.0],[2.0,0.0],[0.0,0.0]]);
    assert!(crate::decode::with_test_decode_ctx(|ctx| ValidatedProfile::new(ctx, vec![entity])).expect("profile resources").is_none());
}

#[test]
fn single_nurbs_simple_polygon_keeps_area() {
    let entity = polygon_curve(&[[0.0,0.0],[3.0,0.0],[3.0,3.0],[0.0,3.0],[0.0,0.0]]);
    let profile = crate::decode::with_test_decode_ctx(|ctx| ValidatedProfile::new(ctx, vec![entity])).expect("profile resources").expect("simple profile");
    assert!((profile.area() - 9.0).abs() < f64::EPSILON * 64.0);
}
