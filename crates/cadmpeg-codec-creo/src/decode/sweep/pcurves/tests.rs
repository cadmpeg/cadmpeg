// SPDX-License-Identifier: Apache-2.0

use super::revolution_boundary_pcurve;
use cadmpeg_ir::features::RevolutionAxis;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

#[test]
fn spindle_torus_boundary_pcurve_retains_the_signed_ring_branch() {
    let surface = SurfaceGeometry::Torus {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 2.0,
        minor_radius: 5.0,
    };
    let axis = RevolutionAxis {
        origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
            .expect("finite point fixture"),
        direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
            .expect("valid direction fixture"),
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
