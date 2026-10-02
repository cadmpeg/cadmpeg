// SPDX-License-Identifier: Apache-2.0
use super::{ProfileEntity, ValidatedProfile};
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::SketchGeometry;

fn polygon_curve(points: &[[f64; 2]]) -> ProfileEntity {
    let mut knots = vec![0.0, 0.0];
    for index in 1..points.len() - 1 {
        knots.push(cadmpeg_core::convert::f64_from_index(index).expect("index"));
    }
    knots.push(cadmpeg_core::convert::f64_from_index(points.len() - 1).expect("index"));
    let last = *knots.last().expect("knots");
    knots.push(last);
    let curve = PcurveNurbs::from_lanes(
        1,
        knots,
        points.iter().map(|p| Point2::new(p[0], p[1])).collect(),
        None,
        false,
    )
    .expect("curve");
    crate::decode::with_test_decode_ctx(|ctx| {
        ProfileEntity::new(ctx, SketchGeometry::nurbs(curve), false)
    })
    .expect("admission")
    .expect("finite endpoints")
}

#[test]
fn single_nurbs_crossing_spans_cannot_mint_validated_profile() {
    let entity = polygon_curve(&[[0.0, 0.0], [3.0, 3.0], [0.0, 3.0], [2.0, 0.0], [0.0, 0.0]]);
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| ValidatedProfile::new(ctx, vec![entity]))
            .expect("profile resources")
            .is_none()
    );
}

#[test]
fn single_nurbs_simple_polygon_keeps_area() {
    let entity = polygon_curve(&[[0.0, 0.0], [3.0, 0.0], [3.0, 3.0], [0.0, 3.0], [0.0, 0.0]]);
    let profile =
        crate::decode::with_test_decode_ctx(|ctx| ValidatedProfile::new(ctx, vec![entity]))
            .expect("profile resources")
            .expect("simple profile");
    assert!((profile.area() - 9.0).abs() < f64::EPSILON * 64.0);
}

#[test]
fn adjacent_nurbs_entities_cannot_cross_away_from_shared_endpoints() {
    let first = polygon_curve(&[[0.0, 0.0], [3.0, 3.0], [0.0, 3.0]]);
    let second = polygon_curve(&[[0.0, 3.0], [2.0, 0.0], [0.0, 0.0]]);
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| super::ordered_extrusion_profiles(
            ctx,
            vec![vec![first, second]]
        ))
        .expect("profile resources")
        .is_none()
    );
}

#[test]
fn empty_extrusion_profile_is_withheld_before_containment() {
    let entity = polygon_curve(&[[0.0, 0.0], [3.0, 0.0], [3.0, 3.0], [0.0, 3.0], [0.0, 0.0]]);
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| super::ordered_extrusion_profiles(
            ctx,
            vec![vec![], vec![entity]]
        ))
        .expect("profile resources")
        .is_none()
    );
}

#[test]
fn singular_nurbs_is_excluded_before_profile_intersection() {
    let curve = PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        Some(vec![1.0, -1.0]),
        false,
    )
    .expect("structural curve");
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| ProfileEntity::new(
            ctx,
            SketchGeometry::nurbs(curve),
            false
        ))
        .expect("resources")
        .is_none()
    );
}

#[test]
fn circular_profile_contains_point_outside_quadrant_diamond() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let circle = |center: Point2, radius, reversed| {
            ProfileEntity::new(
                ctx,
                SketchGeometry::try_from(cadmpeg_ir::sketches::SketchGeometryDefinition::Circle {
                    center,
                    radius: cadmpeg_ir::scalar::Length::new(radius).expect("radius"),
                })
                .expect("circle"),
                reversed,
            )
            .expect("resources")
            .expect("finite ends")
        };
        let outer = vec![circle(Point2::new(0.0, 0.0), 1.0, false)];
        assert!(super::profile_strictly_contains(ctx, &outer, [0.7, 0.7]).expect("containment"));
        let hole = vec![circle(Point2::new(0.7, 0.7), 0.001, true)];
        assert!(super::ordered_extrusion_profiles(ctx, vec![outer, hole])
            .expect("resources")
            .is_some());
    });
}

#[test]
fn profile_entity_rejects_overflowed_circle_and_arc_endpoints() {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;
    let radius = cadmpeg_ir::scalar::Length::new(1.0e308).expect("finite radius");
    let center = Point2::new(1.0e308, 0.0);
    for definition in [
        SketchGeometryDefinition::Circle { center, radius },
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle: cadmpeg_ir::scalar::Angle::new(0.0).expect("angle"),
            end_angle: cadmpeg_ir::scalar::Angle::new(1.0).expect("angle"),
        },
    ] {
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| ProfileEntity::new(
                ctx,
                SketchGeometry::try_from(definition).expect("finite inputs"),
                false
            ))
            .expect("resources")
            .is_none()
        );
    }
}
