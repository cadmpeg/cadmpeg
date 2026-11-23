// SPDX-License-Identifier: Apache-2.0

use super::*;

const LINEAR_TOLERANCE: f64 = 1e-6;
const PLANE_SEPARATION: f64 = 1e-8;
const ANGULAR_TOLERANCE: f64 = 1e-9;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{SpatialSketchGeometry, SpatialSketchGeometryDefinition};

#[test]
fn numerical_0922_identical_circle_planes_match_tight_tolerance() {
    let normal = cadmpeg_ir::features::geometry::FiniteVector3::new(Vector3::new(2.0, 13.0, 1.0))
        .and_then(cadmpeg_ir::features::geometry::FiniteVector3::unit_nonzero)
        .unwrap();
    let reference = cadmpeg_ir::features::geometry::FiniteVector3::new(
        normal.cross(Vector3::new(1.0, 0.0, 0.0)),
    )
    .and_then(cadmpeg_ir::features::geometry::FiniteVector3::unit_nonzero)
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

#[test]
fn concentric_ring_regions_emit_disjoint_annuli_with_hole_boundaries() {
    use cadmpeg_ir::features::profiles::{SketchProfileBoundaryUse, SketchProfileRegion};
    use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{SketchEntityId, SketchEntityUse};

    let make_use = |name: &str| SketchEntityUse {
        entity: SketchEntityId::mint(format!("test:sketch:entity#{name}")).unwrap(),
        reversed: false,
    };
    let u_inner = make_use("c825");
    let u_mid = make_use("c900");
    let u_outer = make_use("c9597");
    let circles = [
        ConcentricCircle {
            index: 1,
            center: Point2::new(0.0, 0.0),
            radius: 8.25,
            use_: &u_inner,
        },
        ConcentricCircle {
            index: 0,
            center: Point2::new(0.0, 0.0),
            radius: 9.0,
            use_: &u_mid,
        },
        ConcentricCircle {
            index: 2,
            center: Point2::new(0.0, 0.0),
            radius: 9.597,
            use_: &u_outer,
        },
    ];

    let full = |use_: &SketchEntityUse, reversed: bool| SketchProfileBoundaryUse {
        entity: use_.entity.clone(),
        parameter_range: DirectedParameterRange::new([0.0, std::f64::consts::TAU]).unwrap(),
        reversed,
    };
    let ring = |outer: &SketchEntityUse, hole: Option<&SketchEntityUse>| {
        SketchProfileRegion::trimmed(
            vec![full(outer, false)],
            hole.map(|hole| vec![full(hole, true)])
                .into_iter()
                .collect(),
        )
        .unwrap()
    };

    assert_eq!(
        concentric_ring_regions(&circles, &[0, 2], Some(&circles[0])).unwrap(),
        [ring(&u_mid, Some(&u_inner)), ring(&u_outer, Some(&u_mid))]
    );
    assert_eq!(
        concentric_ring_regions(&circles, &[0, 2], None).unwrap(),
        [ring(&u_mid, None), ring(&u_outer, Some(&u_mid))]
    );
}
