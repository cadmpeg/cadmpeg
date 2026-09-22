// SPDX-License-Identifier: Apache-2.0
use super::super::{analytic_rolling_ball_surface, rational_four_arc_circle};
use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface};
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

fn circle(radius: f64, z: f64, distortion: f64) -> NurbsCurve {
    let mut points = [
        [1., 0.],
        [1., 1.],
        [0., 1.],
        [-1., 1.],
        [-1., 0.],
        [-1., -1.],
        [0., -1.],
        [1., -1.],
        [1., 0.],
    ]
    .into_iter()
    .map(|[x, y]| Point3::new(radius * x, radius * y, z))
    .collect::<Vec<_>>();
    points[1].x += distortion;
    NurbsCurve::from_lanes(
        2,
        vec![0., 0., 0., 1., 1., 2., 2., 3., 3., 4., 4., 4.],
        points,
        Some(
            (0..9)
                .map(|i| {
                    if i % 2 == 0 {
                        1.
                    } else {
                        std::f64::consts::FRAC_1_SQRT_2
                    }
                })
                .collect(),
        ),
        false,
    )
    .unwrap()
}
#[test]
fn numerical_followup_circle_recognition_preserves_relative_shape() {
    for radius in [1e-200, 1e-8, 1.0, 1e150] {
        assert!(rational_four_arc_circle(&circle(radius, 0., 0.)).is_some());
        assert!(rational_four_arc_circle(&circle(radius, 0., 0.004 * radius)).is_none());
    }
}
#[test]
fn numerical_followup_rolling_ball_requires_tangent_supports() {
    for radius in [1e-8, 1.0] {
        let axis = Vector3::new(0., 0., 1.);
        let reference = Vector3::new(1., 0., 0.);
        for mismatch in [0.0, 0.005 * radius] {
            let supports = [
                Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    PlaneSurface::try_new(Point3::new(0., 0., 0.), axis, reference).unwrap(),
                ))),
                Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    CylinderSurface::try_new(
                        Point3::new(0., 0., 0.),
                        axis,
                        reference,
                        2. * radius + mismatch,
                    )
                    .unwrap(),
                ))),
            ];
            let result = analytic_rolling_ball_surface(
                &supports,
                None,
                &circle(3. * radius, radius, 0.),
                radius,
            );
            assert_eq!(result.is_some(), mismatch == 0.);
        }
    }
}

#[test]
fn numerical_audit_pcurve_ranges_keep_active_domain_and_nonzero_intervals() {
    use crate::sab::{Record, Token};
    use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
    use cadmpeg_ir::math::Point2;
    let points = vec![Point2::new(0., 0.), Point2::new(1., 0.)];
    for d in [1., 1e-16] {
        let c =
            PcurveNurbs::from_lanes(1, vec![0., 0., d, d], points.clone(), None, false).unwrap();
        let edge = Record {
            index: 1,
            name: "edge".into(),
            tokens: vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(2),
                Token::Double(2. * d),
                Token::Ref(3),
                Token::Double(3. * d),
                Token::Ref(-1),
                Token::Ref(4),
                Token::False,
            ]
            .into(),
            offset: 0,
            len: 0,
        };
        assert_eq!(
            super::super::pcurve_ranges_on_domain(&c, Some(&edge)),
            Some(vec![[0., d]])
        );
    }
    let c = PcurveNurbs::from_lanes(1, vec![-1., 0., 1., 2.], points, None, false).unwrap();
    assert_eq!(
        super::super::pcurve_ranges_on_domain(&c, None),
        Some(vec![[0., 1.]])
    );
}
