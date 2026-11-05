// SPDX-License-Identifier: Apache-2.0

use super::{analytic, Analytic};
use crate::records::node_type::{CONE, PLANE, TORUS};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

fn surface(analytic: Option<Analytic>) -> (SolvedSurfaceGeometry, bool) {
    match analytic {
        Some(Analytic::Surface {
            geometry: SurfaceGeometry::Solved(geometry),
            negative_major,
        }) => (geometry, negative_major),
        other => panic!("expected a surface: {other:?}"),
    }
}

#[test]
fn a_plane_keeps_its_stored_frame_and_scales_its_origin() {
    let (geometry, negative_major) = surface(analytic(
        PLANE,
        &[0.001, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0],
    ));
    assert!(!negative_major);
    let SolvedSurfaceGeometry::Plane(plane) = geometry else {
        panic!("plane");
    };
    assert_eq!(plane.origin().get().x, 1.0);
}

#[test]
fn a_cone_half_angle_comes_from_its_sine_and_cosine() {
    let angle = 0.3_f64;
    let mut v = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.002, 0.0, 0.0, 1.0, 0.0, 0.0];
    v[7] = angle.sin();
    v[8] = angle.cos();
    let (geometry, _) = surface(analytic(CONE, &v));
    let SolvedSurfaceGeometry::Cone(cone) = geometry else {
        panic!("cone");
    };
    assert!((cone.half_angle().get() - angle).abs() < 1.0e-15);
    v[7] = -angle.sin();
    let (geometry, _) = surface(analytic(CONE, &v));
    let SolvedSurfaceGeometry::Cone(cone) = geometry else {
        panic!("cone");
    };
    assert!((cone.half_angle().get() + angle).abs() < 1.0e-15);
    for (sine, cosine) in [(1.0, 0.0), (0.0, 1.0), (0.5, 0.5)] {
        v[7] = sine;
        v[8] = cosine;
        assert!(analytic(CONE, &v).is_none(), "sin {sine} cos {cosine}");
    }
}

#[test]
fn a_negative_torus_major_radius_reverses_at_its_magnitude() {
    let v = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, -0.002, 0.003, 1.0, 0.0, 0.0];
    let (geometry, negative_major) = surface(analytic(TORUS, &v));
    assert!(negative_major);
    let SolvedSurfaceGeometry::Torus(torus) = geometry else {
        panic!("torus");
    };
    assert_eq!(torus.major_radius().get(), 2.0);
}

#[test]
fn a_wrong_value_count_or_a_non_finite_value_has_no_geometry() {
    assert!(analytic(PLANE, &[0.0; 8]).is_none());
    let mut v = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0];
    v[0] = f64::NAN;
    assert!(analytic(PLANE, &v).is_none());
}
