// SPDX-License-Identifier: Apache-2.0
//! Exact model-space lifts of plane-supported parameter curves.

use cadmpeg_ir::geometry::{analytic::PlaneSurface, nurbs::NurbsCurve, pcurve::PcurveGeometry};

use crate::nurbs::LaneRefusals;

pub(super) fn lift_plane_pcurve(
    pcurve: &PcurveGeometry,
    plane: &PlaneSurface,
    refusal: &mut LaneRefusals,
) -> Option<NurbsCurve> {
    let PcurveGeometry::Nurbs { nurbs } = pcurve else {
        return None;
    };
    let origin = plane.origin().get();
    let normal = plane.frame().axis().as_raw();
    let u_axis = plane.frame().reference().as_raw();
    let v_axis = (*normal).cross(*u_axis);
    crate::nurbs::note_refusal(
        nurbs.lift(|point| {
            origin
                .translated(*u_axis, point.u)
                .translated(v_axis, point.v)
        }),
        refusal,
        "weighted boundary curve lifted from a plane pcurve",
    )
}

#[cfg(test)]
mod tests {
    use super::lift_plane_pcurve;
    use crate::nurbs::LaneRefusals;
    use cadmpeg_ir::eval::{decode::nurbs_curve_point_at, nurbs_pcurve_uv};
    use cadmpeg_ir::geometry::{
        analytic::PlaneSurface,
        pcurve::{LinePcurve, PcurveGeometry, PcurveNurbs},
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    #[test]
    fn plane_lift_preserves_polynomial_and_rational_parameterizations() {
        let plane = PlaneSurface::try_new(
            Point3::new(4.0, -3.0, 2.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        )
        .expect("plane frame");
        for (weights, periodic) in [(None, false), (Some(vec![1.0, 2.0, 0.5, 1.0]), true)] {
            let pcurve = PcurveNurbs::from_lanes(
                2,
                vec![-2.0, -2.0, -2.0, -0.5, 3.0, 3.0, 3.0],
                vec![
                    Point2::new(1.0, 0.0),
                    Point2::new(2.0, 3.0),
                    Point2::new(-1.0, 4.0),
                    Point2::new(1.0, 0.0),
                ],
                weights,
                periodic,
            )
            .expect("parameter curve");
            let mut refusal = LaneRefusals::new();
            let lifted = lift_plane_pcurve(
                &PcurveGeometry::Nurbs {
                    nurbs: pcurve.clone(),
                },
                &plane,
                &mut refusal,
            )
            .expect("plane lift");
            assert_eq!(lifted.degree(), pcurve.degree());
            assert_eq!(lifted.periodic(), pcurve.periodic());
            assert_eq!(lifted.weights(), pcurve.weights());
            assert_eq!(lifted.knots(), pcurve.knots());
            assert_eq!(refusal.note_count(), 0);
            crate::test_support::with_service_context(|ctx| {
                let points = pcurve
                    .control_points()
                    .iter()
                    .map(|point| point.get())
                    .collect::<Vec<_>>();
                let weights = pcurve.weights().map(|weights| {
                    weights
                        .into_iter()
                        .map(cadmpeg_ir::scalar::NonZeroReal::get)
                        .collect::<Vec<_>>()
                });
                for parameter in [-2.0, -1.3, -0.5, 0.7, 3.0] {
                    let uv = nurbs_pcurve_uv(
                        ctx,
                        pcurve.degree(),
                        pcurve.knots(),
                        &points,
                        weights.as_deref(),
                        parameter,
                    )
                    .expect("parameter point")
                    .get();
                    let point = nurbs_curve_point_at(ctx, &lifted, parameter)
                        .expect("model point")
                        .get();
                    assert!((point.x - (4.0 + uv.v)).abs() < 1.0e-12);
                    assert!((point.y + 3.0).abs() < 1.0e-12);
                    assert!((point.z - (2.0 + uv.u)).abs() < 1.0e-12);
                }
            });
        }
    }

    #[test]
    fn plane_lift_records_a_nonfinite_model_pole() {
        let large = f64::MAX * 0.75;
        let plane = PlaneSurface::try_new(
            Point3::new(large, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("finite plane");
        let pcurve = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point2::new(large, 0.0), Point2::new(large, 1.0)],
                None,
                false,
            )
            .expect("finite parameter curve"),
        };
        let mut refusal = LaneRefusals::new();
        assert!(lift_plane_pcurve(&pcurve, &plane, &mut refusal).is_none());
        assert_eq!(refusal.note_count(), 1);
    }

    #[test]
    fn plane_lift_leaves_an_unbounded_line_for_its_carrier_path() {
        let plane = PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane frame");
        let pcurve = PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))
                .expect("parameter line"),
        );
        let mut refusal = LaneRefusals::new();
        assert!(lift_plane_pcurve(&pcurve, &plane, &mut refusal).is_none());
        assert_eq!(refusal.note_count(), 0);
    }
}
