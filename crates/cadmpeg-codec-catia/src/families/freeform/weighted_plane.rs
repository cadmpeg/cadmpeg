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

const PLANE_CURVE_TOLERANCE_MM: f64 = 1.0e-3;

pub(super) fn project_curve(
    curve: &NurbsCurve,
    plane: &PlaneSurface,
    refusal: &mut LaneRefusals,
) -> Option<(
    PcurveGeometry,
    cadmpeg_ir::geometry::legacy_cache::FitTolerance,
)> {
    use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
    use cadmpeg_ir::math::Point2;

    let origin = plane.origin().get();
    let normal = *plane.frame().axis().as_raw();
    let u = *plane.frame().reference().as_raw();
    let v = normal.cross(u);
    if curve
        .weights()
        .is_some_and(|weights| weights.into_iter().any(|weight| weight.get() <= 0.0))
    {
        return None;
    }
    let mut gap_mm = 0.0_f64;
    let mut controls = Vec::new();
    for pole in curve.control_points() {
        let offset = pole.get().vector_from(origin);
        let gap = offset.dot(normal).abs();
        if !gap.is_finite() || gap > PLANE_CURVE_TOLERANCE_MM {
            return None;
        }
        gap_mm = gap_mm.max(gap);
        controls.push(Point2::new(offset.dot(u), offset.dot(v)));
    }
    let nurbs = crate::nurbs::note_refusal(
        PcurveNurbs::from_lanes(
            curve.degree(),
            curve.knots().to_vec(),
            controls,
            curve.weights().map(|weights| {
                weights
                    .into_iter()
                    .map(cadmpeg_ir::scalar::NonZeroReal::get)
                    .collect()
            }),
            curve.periodic(),
        ),
        refusal,
        "weighted boundary curve projected onto its plane",
    )?;
    Some((
        PcurveGeometry::Nurbs { nurbs },
        cadmpeg_ir::geometry::legacy_cache::FitTolerance::try_new(gap_mm).ok()?,
    ))
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
    #[test]
    fn plane_projection_preserves_the_curve_gauge_and_bounds_the_lift() {
        let plane = PlaneSurface::try_new(
            Point3::new(4.0, -3.0, 2.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        )
        .expect("plane frame");
        for (weights, periodic) in [(None, false), (Some(vec![1.0, 2.0, 0.5, 1.0]), true)] {
            let model = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                2,
                vec![-2.0, -2.0, -2.0, -0.5, 3.0, 3.0, 3.0],
                vec![
                    Point3::new(4.0, -3.0, 3.0),
                    Point3::new(7.0, -2.9995, 4.0),
                    Point3::new(8.0, -3.0002, 1.0),
                    Point3::new(4.0, -3.0, 3.0),
                ],
                weights,
                periodic,
            )
            .expect("model curve");
            let mut refusal = LaneRefusals::new();
            let (projected, bound) =
                super::project_curve(&model, &plane, &mut refusal).expect("bounded projection");
            let PcurveGeometry::Nurbs { nurbs } = &projected else {
                panic!("NURBS projection")
            };
            assert_eq!(nurbs.degree(), model.degree());
            assert_eq!(nurbs.knots(), model.knots());
            assert_eq!(nurbs.weights(), model.weights());
            assert_eq!(nurbs.periodic(), model.periodic());
            assert!((bound.get() - 0.0005).abs() < 1.0e-12);
            let lifted = lift_plane_pcurve(&projected, &plane, &mut refusal).expect("plane lift");
            crate::test_support::with_service_context(|ctx| {
                for parameter in [-2.0, -1.3, -0.5, 0.7, 3.0] {
                    let actual = nurbs_curve_point_at(ctx, &lifted, parameter)
                        .expect("lift")
                        .get();
                    let expected = nurbs_curve_point_at(ctx, &model, parameter)
                        .expect("model")
                        .get();
                    assert!((actual.x - expected.x).abs() < 1.0e-12);
                    assert!((actual.z - expected.z).abs() < 1.0e-12);
                    assert!((actual.y + 3.0).abs() < 1.0e-12);
                    assert!(actual.distance(expected) <= bound.get() + 1.0e-12);
                }
            });
            assert!(refusal.take_notes().is_empty());
        }
    }

    #[test]
    fn plane_projection_rejects_poles_beyond_the_bound_and_negative_weights() {
        let plane = PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane frame");
        for (height, weights, accepted) in [
            (0.0, None, true),
            (super::PLANE_CURVE_TOLERANCE_MM, Some(vec![1.0, 2.0]), true),
            (super::PLANE_CURVE_TOLERANCE_MM.next_up(), None, false),
            (0.0, Some(vec![1.0, -1.0]), false),
        ] {
            let model = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, height)],
                weights,
                false,
            )
            .expect("model curve");
            let mut refusal = LaneRefusals::new();
            let projected = super::project_curve(&model, &plane, &mut refusal);
            assert_eq!(projected.is_some(), accepted);
            if let Some((_, bound)) = projected {
                assert_eq!(bound.get(), height);
            }
        }
    }

    #[test]
    fn plane_projection_reports_its_model_source_and_bound() {
        let mut refusal = LaneRefusals::new();
        refusal.push_plane_pcurve_projected(123, 0.0005);
        let notes = refusal.take_notes();
        assert_eq!(notes.len(), 1);
        assert_eq!(
            notes[0].code.local_code(),
            "geometry.plane-pcurve-projected"
        );
        assert!(notes[0]
            .message
            .contains("class-18 model curve at offset 123"));
        assert!(notes[0].message.contains("0.000500000 mm"));
    }
}
