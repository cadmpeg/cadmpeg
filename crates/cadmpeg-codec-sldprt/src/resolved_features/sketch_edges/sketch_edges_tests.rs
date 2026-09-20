//! Tests for the `sketch_edges` module.

use super::super::compact_reference_planes::principal_sketch_frame;
use super::{circle_contains_point, ellipse_contains_point};
use cadmpeg_ir::features::PrincipalPlane;
use cadmpeg_ir::math::Point2;

#[test]
fn every_principal_plane_has_a_sketch_frame() {
    for plane in [
        PrincipalPlane::Front,
        PrincipalPlane::Top,
        PrincipalPlane::Right,
    ] {
        let (_, normal, u_axis) = principal_sketch_frame(plane);
        assert!((super::dot(normal, normal) - 1.0).abs() <= 1.0e-12);
        assert!((super::dot(u_axis, u_axis) - 1.0).abs() <= 1.0e-12);
        assert!(super::dot(normal, u_axis).abs() <= 1.0e-12);
    }
}

#[test]
fn rejects_analytic_carriers_that_do_not_contain_the_edge_vertex() {
    assert!(!circle_contains_point(
        Point2::new(-35.0, -5.85),
        1.25,
        Point2::new(-75.0, -8.85),
        1.0e-9,
    ));
    assert!(!ellipse_contains_point(
        Point2::new(-60.0, -150.0),
        0.0,
        7.5,
        f64::MIN_POSITIVE,
        Point2::new(140.0, -70.5),
        1.0e-9,
    ));
}

#[test]
fn accepts_vertices_on_nondegenerate_analytic_carriers() {
    assert!(circle_contains_point(
        Point2::new(2.0, 3.0),
        4.0,
        Point2::new(6.0, 3.0),
        1.0e-9,
    ));
    assert!(ellipse_contains_point(
        Point2::new(2.0, 3.0),
        0.0,
        4.0,
        2.0,
        Point2::new(2.0, 5.0),
        1.0e-9,
    ));
}
