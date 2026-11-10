// SPDX-License-Identifier: Apache-2.0

use super::*;

const LINEAR_TOLERANCE: f64 = 1e-6;
const PLANE_SEPARATION: f64 = 1e-8;
const ANGULAR_TOLERANCE: f64 = 1e-9;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{SpatialSketchGeometry, SpatialSketchGeometryDefinition};

#[test]
fn numerical_0922_identical_circle_planes_match_tight_tolerance() {
    let normal = cadmpeg_ir::features::FiniteVector3::new(Vector3::new(2.0, 13.0, 1.0))
        .and_then(cadmpeg_ir::features::FiniteVector3::unit_nonzero)
        .unwrap();
    let reference =
        cadmpeg_ir::features::FiniteVector3::new(normal.cross(Vector3::new(1.0, 0.0, 0.0)))
            .and_then(cadmpeg_ir::features::FiniteVector3::unit_nonzero)
            .unwrap();
    let geometry = SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        normal,
        reference_direction: reference,
        radius: Length::new(1.0).unwrap(),
    })
    .unwrap();
    assert!(coincident_spatial_profile_geometry(
        &geometry,
        &geometry,
        LINEAR_TOLERANCE,
        ANGULAR_TOLERANCE
    ));
}
#[test]
fn numerical_0922_different_circle_planes_fail_tight_tolerance() {
    let a = Vector3::new(0., 0., 1.);
    let angle: f64 = PLANE_SEPARATION;
    let b = Vector3::new(0., angle.sin(), angle.cos());
    let make = |normal| {
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Circle {
            center: Point3::new(0., 0., 0.),
            normal,
            reference_direction: Vector3::new(1., 0., 0.),
            radius: Length::new(1.).unwrap(),
        })
        .unwrap()
    };
    let agrees = coincident_spatial_profile_geometry(
        &make(a),
        &make(b),
        LINEAR_TOLERANCE,
        ANGULAR_TOLERANCE,
    );
    println!("Fusion circle planes at1e-8 with angular tolerance1e-9 agree={agrees}");
    assert!(!agrees);
}

#[test]
fn circles_outside_a_body_fill_every_ring_out_to_the_new_circle() {
    let circles = [(0, 9.1), (1, 11.25), (2, 9.5), (3, 10.9)];
    assert_eq!(
        circle_indices_outside_body(&circles, 9.9, 11.25, LINEAR_TOLERANCE),
        Some(vec![3, 1])
    );
}

#[test]
fn circles_outside_a_body_leave_one_ring_to_the_cylinder_match() {
    let tube = [(0, 9.1), (1, 9.9)];
    assert_eq!(
        circle_indices_outside_body(&tube, 9.1, 9.9, LINEAR_TOLERANCE),
        None
    );
    let outer_ring = [(1, 11.25), (3, 10.9)];
    assert_eq!(
        circle_indices_outside_body(&outer_ring, 10.9, 11.25, LINEAR_TOLERANCE),
        None
    );
}

#[test]
fn circles_inward_of_a_body_fill_every_ring_out_to_the_first_circle_past_it() {
    let circles = [(1, 8.25), (0, 9.0), (2, 9.596_940_582_174_966), (4, 10.5)];
    assert_eq!(
        circle_indices_inward_shoulder(&circles, 8.25, 9.1, 9.9, LINEAR_TOLERANCE),
        Some(vec![0, 2])
    );
}

#[test]
fn circles_inward_of_a_body_stop_without_a_circle_past_the_body() {
    let circles = [(1, 8.25), (0, 9.0)];
    assert_eq!(
        circle_indices_inward_shoulder(&circles, 8.25, 9.1, 9.9, LINEAR_TOLERANCE),
        None
    );
}

#[test]
fn circles_inward_of_a_body_stop_when_the_next_circle_passes_the_wall() {
    let circles = [(1, 8.25), (0, 9.0), (2, 10.5)];
    assert_eq!(
        circle_indices_inward_shoulder(&circles, 8.25, 9.1, 9.9, LINEAR_TOLERANCE),
        None
    );
    assert_eq!(
        circle_indices_inward_shoulder(&circles, 9.1, 8.25, 9.9, LINEAR_TOLERANCE),
        None
    );
}
