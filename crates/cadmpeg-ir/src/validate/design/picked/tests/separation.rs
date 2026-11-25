// SPDX-License-Identifier: Apache-2.0
//! The separation of a patch support face from its boundary curve on
//! cylinders, tori, and cones. The curve is a top edge of the cube: each of
//! its points lies between 5 and 50^0.5 from the vertical axis through the
//! cube's middle, at height 10.

use super::{ruled, set_surface, top_on_floor};
use crate::geometry::analytic::{ConeSurface, CylinderSurface, TorusSurface};
use crate::geometry::SolvedSurfaceGeometry;
use crate::math::{Point3, Vector3};

const RULE: &str = "design.patch-support-face-off-curve";

fn up() -> Vector3 {
    Vector3::new(0.0, 0.0, 1.0)
}

fn across() -> Vector3 {
    Vector3::new(1.0, 0.0, 0.0)
}

/// The number of findings with the support face on `geometry`.
fn off(geometry: SolvedSurfaceGeometry) -> usize {
    let (mut ir, floor) = top_on_floor();
    set_surface(&mut ir, floor, geometry);
    ruled(&ir, RULE).len()
}

fn cylinder(radius: f64) -> SolvedSurfaceGeometry {
    let axis = Point3::new(5.0, 5.0, 0.0);
    SolvedSurfaceGeometry::Cylinder(CylinderSurface::try_new(axis, up(), across(), radius).unwrap())
}

fn torus(major: f64, minor: f64) -> SolvedSurfaceGeometry {
    let center = Point3::new(5.0, 5.0, 10.0);
    let torus = TorusSurface::try_new(center, up(), across(), major, minor).unwrap();
    SolvedSurfaceGeometry::Torus(torus)
}

/// The cone about the vertical axis through the cube's middle whose section
/// at height `base` has major radius `radius`.
fn cone(base: f64, radius: f64, ratio: f64, half_angle: f64) -> SolvedSurfaceGeometry {
    let origin = Point3::new(5.0, 5.0, base);
    let cone = ConeSurface::try_new(origin, up(), across(), radius, ratio, half_angle).unwrap();
    SolvedSurfaceGeometry::Cone(cone)
}

#[test]
fn a_patch_support_on_a_cylinder_meets_its_boundary_curve() {
    assert_eq!(off(cylinder(6.0)), 0);
    assert_eq!(off(cylinder(2.0)), 1);
    assert_eq!(off(cylinder(9.0)), 1);
}

#[test]
fn a_patch_support_on_a_torus_meets_its_boundary_curve() {
    assert_eq!(off(torus(6.0, 0.5)), 0);
    assert_eq!(off(torus(20.0, 1.0)), 1);
}

#[test]
fn a_patch_support_on_a_cone_meets_its_boundary_curve() {
    // The section at height 10 has radius 4 + 10 * 0.2 = 6.
    assert_eq!(off(cone(0.0, 4.0, 1.0, 0.2_f64.atan())), 0);
    // An elliptic section of radii 6 and 3 at height 10.
    assert_eq!(off(cone(10.0, 6.0, 0.5, 0.0)), 0);
    assert_eq!(off(cone(10.0, 20.0, 1.0, 0.3)), 1);
    // The minor radius 15 of the section at height 10 lies 10 beyond the
    // curve's nearest point.
    assert_eq!(off(cone(10.0, 30.0, 0.5, 0.2)), 1);
}
