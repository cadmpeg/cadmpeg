// SPDX-License-Identifier: Apache-2.0

use super::revolution_boundary_pcurve;
use cadmpeg_ir::features::RevolutionAxis;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

#[test]
fn spindle_torus_boundary_pcurve_retains_the_signed_ring_branch() {
    let surface = SurfaceGeometry::Torus(
        cadmpeg_ir::geometry::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            5.0,
        )
        .unwrap(),
    );
    let axis = RevolutionAxis {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(0.0, 0.0, 1.0),
        reference: None,
    };
    let pcurve =
        revolution_boundary_pcurve(&surface, [-3.0, 0.0, 0.0], &axis).expect("spindle boundary");
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let uv = cadmpeg_ir::eval::pcurve_uv(&pcurve, parameter).expect("pcurve point");
        let point = cadmpeg_ir::eval::surface_point(&surface, uv.u, uv.v).expect("surface point");
        assert!((point.x.hypot(point.y) - 3.0).abs() < 1.0e-12);
        assert!(point.z.abs() < 1.0e-12);
    }
}
