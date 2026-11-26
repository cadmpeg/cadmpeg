// SPDX-License-Identifier: Apache-2.0
//! A stiffening rib's point against its bend face on each carrier that the
//! separation bound reads. The bend face is the cube's bottom face and the
//! point is (x, 5, 3).

use super::separation::{cone, cylinder, sheet as nurbs};
use super::{bottom, rib, ruled, set_surface, with};
use crate::geometry::analytic::{SphereSurface, TorusSurface};
use crate::geometry::{PlacedSurface, SolvedSurfaceGeometry};
use crate::math::{Point3, Vector3};
use crate::transform::Transform;
use crate::validate::design::fixture;

const RULE: &str = "design.stiffening-rib-point-off-bend";

fn up() -> Vector3 {
    Vector3::new(0.0, 0.0, 1.0)
}

fn across() -> Vector3 {
    Vector3::new(1.0, 0.0, 0.0)
}

/// The number of findings with the bend face on `geometry` and the rib at
/// (`x`, 5, 3).
fn off(geometry: SolvedSurfaceGeometry, x: f64) -> usize {
    let mut ir = fixture::document();
    let face = bottom(&ir);
    set_surface(&mut ir, face, geometry);
    with(&mut ir, |ir| rib(ir, x));
    ruled(&ir, RULE).len()
}

/// `geometry` placed by the affine map of `rows`.
fn placed(geometry: SolvedSurfaceGeometry, rows: [[f64; 4]; 3]) -> SolvedSurfaceGeometry {
    let placed = PlacedSurface::try_new(Box::new(geometry), Transform::affine(rows).unwrap());
    SolvedSurfaceGeometry::Transformed(placed.unwrap())
}

#[test]
fn a_stiffening_rib_point_on_a_cone_lies_on_its_bend_face() {
    // The section at height 3 has radius 4 + 3 / 3 = 5.
    let bend = || cone(0.0, 4.0, 1.0, (1.0_f64 / 3.0).atan());
    assert_eq!(off(bend(), 10.0), 0);
    assert_eq!(off(bend(), 5.0), 1);
}

#[test]
fn a_stiffening_rib_point_on_a_sphere_lies_on_its_bend_face() {
    // (10, 5, 3) lies 29^0.5 from (5, 5, 5).
    let bend = || {
        let center = Point3::new(5.0, 5.0, 5.0);
        let sphere = SphereSurface::try_new(center, up(), across(), 29.0_f64.sqrt()).unwrap();
        SolvedSurfaceGeometry::Sphere(sphere)
    };
    assert_eq!(off(bend(), 10.0), 0);
    assert_eq!(off(bend(), 5.0), 1);
}

#[test]
fn a_stiffening_rib_point_on_a_torus_lies_on_its_bend_face() {
    // The tube of radius 1 about the circle of radius 4 at height 3 holds
    // the points 5 from the axis at that height.
    let bend = || {
        let center = Point3::new(5.0, 5.0, 3.0);
        let torus = TorusSurface::try_new(center, up(), across(), 4.0, 1.0).unwrap();
        SolvedSurfaceGeometry::Torus(torus)
    };
    assert_eq!(off(bend(), 10.0), 0);
    assert_eq!(off(bend(), 5.0), 1);
}

#[test]
fn a_stiffening_rib_point_on_a_nurbs_surface_lies_on_its_bend_face() {
    assert_eq!(off(nurbs(3.0, None), 5.0), 0);
    assert_eq!(off(nurbs(3.0, None), 20.0), 1);
}

#[test]
fn a_stiffening_rib_point_on_a_rigidly_placed_surface_lies_on_its_bend_face() {
    // The shift of 5 along x carries the cylinder's axis to (10, 5).
    let shift = [
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    assert_eq!(off(placed(cylinder(5.0), shift), 15.0), 0);
    assert_eq!(off(placed(cylinder(5.0), shift), 10.0), 1);
    let stretch = [
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    assert_eq!(off(placed(cylinder(5.0), stretch), 5.0), 0);
}
