// SPDX-License-Identifier: Apache-2.0

use super::{analytic, Analytic, Refusal};
use crate::records::node_type::{CONE, PLANE, TORUS};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

fn surface(analytic: Result<Analytic, Refusal>) -> (SolvedSurfaceGeometry, bool) {
    match analytic {
        Ok(Analytic::Surface {
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

fn cone_values(sine: f64, cosine: f64) -> [f64; 12] {
    [
        0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.002, sine, cosine, 1.0, 0.0, 0.0,
    ]
}

#[test]
fn a_cone_half_angle_comes_from_its_sine_and_cosine() {
    let angle = 0.3_f64;
    let (geometry, _) = surface(analytic(CONE, &cone_values(angle.sin(), angle.cos())));
    let SolvedSurfaceGeometry::Cone(cone) = geometry else {
        panic!("cone");
    };
    assert!((cone.half_angle().get() - angle).abs() < 1.0e-15);
    for (sine, cosine) in [(1.0, 0.0), (0.0, 1.0), (0.5, 0.5)] {
        assert!(
            analytic(CONE, &cone_values(sine, cosine)).is_err(),
            "sin {sine} cos {cosine}"
        );
    }
}

#[test]
fn a_cone_with_a_negative_half_angle_field_is_refused_for_its_sign() {
    let root_half = std::f64::consts::FRAC_1_SQRT_2;
    for (sine, cosine) in [
        (-root_half, root_half),
        (root_half, -root_half),
        (-root_half, -root_half),
    ] {
        assert_eq!(
            analytic(CONE, &cone_values(sine, cosine)),
            Err(Refusal::ConeHalfAngleSign),
            "sin {sine} cos {cosine}"
        );
    }
}

#[test]
fn a_cone_pair_off_the_unit_identity_by_1e7_is_refused() {
    let sine = 0.6_f64;
    let cosine = (1.0 + 1.0e-7 - sine * sine).sqrt();
    assert!((sine * sine + cosine * cosine - 1.0 - 1.0e-7).abs() < 1.0e-15);
    assert_eq!(
        analytic(CONE, &cone_values(sine, cosine)),
        Err(Refusal::ConeHalfAngleIdentity)
    );
}

#[test]
fn a_cone_with_an_invalid_frame_is_refused_for_its_fields() {
    let mut v = cone_values(-0.6, 0.8);
    v[9] = 0.0;
    v[11] = 1.0;
    assert_eq!(analytic(CONE, &v), Err(Refusal::Fields));
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
    assert_eq!(analytic(PLANE, &[0.0; 8]), Err(Refusal::Fields));
    let mut v = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0];
    v[0] = f64::NAN;
    assert_eq!(analytic(PLANE, &v), Err(Refusal::Fields));
}
