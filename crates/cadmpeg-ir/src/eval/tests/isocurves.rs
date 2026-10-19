// SPDX-License-Identifier: Apache-2.0

use crate::eval::admission::EvaluationAdmission;
use crate::eval::nurbs_surface_isocurve;
use crate::geometry::nurbs::{
    NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes, SurfaceParameterAxis,
};
use crate::math::Point3;

fn surface(rational: bool) -> NurbsSurface {
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            rational.then(|| vec![vec![1.0; 2]; 2]),
        ),
        false,
    )
    .unwrap()
}

#[test]
fn isocurve_keeps_free_axis_knots_and_the_surface_weight_form() {
    for rational in [false, true] {
        let surface = surface(rational);
        let ctx = cadmpeg_test_support::service_decode_context();
        let curve = nurbs_surface_isocurve(&ctx, &surface, SurfaceParameterAxis::U, 0.5)
            .unwrap()
            .unwrap();
        assert_eq!(
            curve.control_points(),
            [Point3::new(0.5, 0.0, 0.0), Point3::new(0.5, 1.0, 0.0)]
        );
        assert_eq!(curve.knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            curve.weights().map(|weights| weights
                .into_iter()
                .map(crate::scalar::NonZeroReal::get)
                .collect::<Vec<_>>()),
            rational.then(|| vec![1.0; 2])
        );
        assert_eq!(
            curve,
            nurbs_surface_isocurve(
                EvaluationAdmission::Standard,
                &surface,
                SurfaceParameterAxis::U,
                0.5
            )
            .unwrap()
            .unwrap()
        );
        ctx.finish_session().unwrap();
    }
}
