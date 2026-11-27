// SPDX-License-Identifier: Apache-2.0
use super::super::{analytic_surface_point, analytic_surface_second_partials};
use crate::geometry::analytic::{
    ConeSurface, CylinderSurface, PlaneSurface, SphereSurface, TorusSurface,
};
use crate::geometry::SolvedSurfaceGeometry;
use crate::math::{Point3, Vector3};

#[test]
fn analytic_point_has_the_bits_of_the_second_order_jet_point() {
    let origin = Point3::new(1.25, -3.5, 7.0e3);
    let axis = Vector3::new(0.3, -0.4, 0.866_025_403_784_438_6);
    let reference = Vector3::new(0.8, 0.6, 0.0);
    let surfaces = [
        SolvedSurfaceGeometry::Plane(PlaneSurface::try_new(origin, axis, reference).unwrap()),
        SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(origin, axis, reference, 12.5).unwrap(),
        ),
        SolvedSurfaceGeometry::Cone(
            ConeSurface::try_new(origin, axis, reference, 4.0, 0.75, 0.35).unwrap(),
        ),
        SolvedSurfaceGeometry::Sphere(
            SphereSurface::try_new(origin, axis, reference, 9.0).unwrap(),
        ),
        SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(origin, axis, reference, 20.0, 3.0).unwrap(),
        ),
    ];
    let bits = |point: Point3| [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()];
    for surface in &surfaces {
        for step_u in 0..=40 {
            for step_v in 0..=40 {
                let u = f64::from(step_u).mul_add(0.37, -7.4);
                let v = f64::from(step_v).mul_add(0.11, -2.2);
                let jet = analytic_surface_second_partials(surface, u, v).unwrap();
                let point = analytic_surface_point(surface, u, v).unwrap();
                assert_eq!(bits(point), bits(jet.point), "{surface:?} at ({u}, {v})");
            }
        }
    }
}
