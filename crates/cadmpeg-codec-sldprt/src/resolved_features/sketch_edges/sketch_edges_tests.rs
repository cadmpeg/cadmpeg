//! Tests for the `sketch_edges` module.

use super::super::compact_reference_planes::principal_sketch_frame;
use cadmpeg_ir::features::PrincipalPlane;

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
