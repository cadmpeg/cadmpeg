// SPDX-License-Identifier: Apache-2.0
use crate::{
    geometry::nurbs::NurbsSurface,
    math::Point3,
    test_support::nurbs::{curve, surface},
};

#[test]
fn nurbs_stores_hold_admitted_poles_and_take_admitted_lanes() {
    use crate::features::FinitePoint3;
    use crate::geometry::nurbs::{
        bezier::positive_controls, NurbsCurve, NurbsError, NurbsPoles3, NurbsSurfaceAxis,
        NurbsSurfaceLanes,
    };
    use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles, PolarPcurveNurbs};
    use crate::math::Point2;
    use crate::scalar::{FiniteReal, NonZeroReal};
    use crate::test_support::nurbs::{pcurve, polar};
    use crate::units::FinitePoint2;

    let curve = curve();
    let held: &NurbsPoles3<FinitePoint3> = curve.pole_rows();
    assert_eq!(
        NurbsCurve::new(
            &cadmpeg_test_support::service_decode_context(),
            1,
            curve.knots().to_vec(),
            held.clone(),
            true
        )
        .expect("fixture final NURBS admission"),
        Ok(curve.clone())
    );
    assert_eq!(held.to_raw().points(), curve.pole_rows().raw_points());
    assert_eq!(
        NurbsCurve::from_checked_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            curve.knots().clone(),
            curve.control_points(),
            curve.weights(),
            true,
        )
        .expect("fixture pole pairing admission"),
        Ok(curve.clone())
    );
    let weight = NonZeroReal::new(1.0).unwrap();
    assert_eq!(
        NurbsCurve::from_checked_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            curve.knots().clone(),
            curve.control_points(),
            Some(vec![weight]),
            true,
        )
        .expect("fixture pole pairing admission"),
        Err(NurbsError::WeightLaneLength {
            field: "poles".to_owned(),
            poles: 2,
            weights: 1,
        })
    );
    let non_finite = vec![Point3::new(f64::NAN, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    let raw_refusal = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        non_finite.clone(),
        None,
        false,
    )
    .expect("fixture constructor admission")
    .unwrap_err();
    assert_eq!(
        NurbsCurve::from_checked_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            crate::geometry::nurbs::KnotVector::new(
                &cadmpeg_test_support::service_decode_context(),
                vec![0.0, 0.0, 1.0, 1.0]
            )
            .expect("fixture knot admission")
            .unwrap(),
            non_finite,
            None,
            false,
        )
        .expect("fixture pole pairing admission"),
        Err(raw_refusal)
    );
    assert_eq!(
        NurbsCurve::from_checked_lanes(
            &cadmpeg_test_support::service_decode_context(),
            4,
            crate::geometry::nurbs::KnotVector::new(
                &cadmpeg_test_support::service_decode_context(),
                vec![0.0, 0.0, 1.0, 1.0]
            )
            .expect("fixture knot admission")
            .unwrap(),
            vec![Point3::new(f64::NAN, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture pole pairing admission"),
        Err(NurbsError::Structure(
            "control_points must contain more than degree 4 poles, found 2".into()
        ))
    );
    // At t = 3 the weights -1 and 2 blend to -1 * 2/3 + 2 * 1/3 = 0, so the
    // homogeneous point has no projection.
    assert_eq!(
        crate::eval::decode::nurbs_curve_point_at(
            crate::eval::admission::EvaluationAdmission::Standard,
            &curve,
            3.0
        ),
        Err(crate::eval::EvaluationFailure::NoValue)
    );

    let mut mapped = curve.clone();
    let refusal = mapped
        .try_map_control_points(
            |_, _| Err(NurbsError::EditRefused("kept".into())),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("pole edit admission");
    assert_eq!(refusal, Err(NurbsError::EditRefused("kept".into())));
    assert_eq!(mapped, curve);
    mapped
        .try_map_control_points(
            |_, point| Ok::<_, NurbsError>(point.negated()),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("pole edit admission")
        .unwrap();
    assert_eq!(
        mapped.control_points(),
        vec![Point3::new(-1.0, -2.0, -3.0), Point3::new(-4.0, -5.0, -6.0)]
    );
    assert_eq!(mapped.weights(), curve.weights());

    let surface = surface();
    let u = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], true);
    let v = || NurbsSurfaceAxis::new(1, vec![2.0, 2.0, 5.0, 5.0], false);
    assert_eq!(
        crate::geometry::nurbs::NurbsSurface::new(
            &cadmpeg_test_support::service_decode_context(),
            u(),
            v(),
            surface.pole_grid().clone(),
            true
        )
        .expect("fixture final NURBS admission"),
        Ok(surface.clone())
    );
    assert_eq!(
        crate::geometry::nurbs::NurbsSurface::from_checked_lanes(
            &cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(1, surface.u_knots().clone(), true),
            NurbsSurfaceAxis::new(1, surface.v_knots().clone(), false),
            NurbsSurfaceLanes::new(surface.control_grid(), surface.weights()),
            true,
        )
        .expect("fixture pole pairing admission"),
        Ok(surface.clone())
    );
    assert_eq!(
        positive_controls(
            &cadmpeg_test_support::service_decode_context(),
            &surface.poles(),
            Some(&[1.0, 1.0, 2.0, 2.0]),
            "Bezier positive controls"
        )
        .expect("resource allocation did not fail")
        .map(|output| output.to_vec()),
        positive_controls(
            &cadmpeg_test_support::service_decode_context(),
            &surface.pole_grid().raw_points().concat(),
            Some(&[1.0, 1.0, 2.0, 2.0]),
            "Bezier positive controls"
        )
        .expect("resource allocation did not fail")
        .map(|output| output.to_vec())
    );
    assert_eq!(
        positive_controls(
            &cadmpeg_test_support::service_decode_context(),
            &surface.poles(),
            None,
            "Bezier positive controls"
        )
        .expect("resource allocation did not fail")
        .map(|output| output.to_vec()),
        positive_controls(
            &cadmpeg_test_support::service_decode_context(),
            &surface.poles(),
            Some(&[1.0; 4]),
            "Bezier positive controls"
        )
        .expect("resource allocation did not fail")
        .map(|output| output.to_vec())
    );
    assert_eq!(
        positive_controls(
            &cadmpeg_test_support::service_decode_context(),
            &[Point3::new(f64::INFINITY, 0.0, 0.0)],
            Some(&[1.0]),
            "Bezier positive controls"
        )
        .expect("resource allocation did not fail")
        .map(|output| output.to_vec()),
        None
    );
    let mut mapped = surface.clone();
    mapped
        .try_map_control_points(
            |_, point| Ok::<_, NurbsError>(point.negated()),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("pole edit admission")
        .unwrap();
    assert_eq!(
        mapped.pole(1, 1).map(FinitePoint3::get),
        Some(Point3::new(-1.0, -1.0, 0.0))
    );
    assert_eq!(mapped.weights(), surface.weights());

    let pcurve = pcurve();
    let held: &PcurveNurbsPoles<FinitePoint2> = pcurve.pole_rows();
    assert_eq!(
        PcurveNurbs::new(
            &cadmpeg_test_support::service_decode_context(),
            1,
            pcurve.knots().to_vec(),
            held.clone(),
            true
        )
        .expect("fixture pcurve construction admission"),
        Ok(pcurve.clone())
    );
    assert_eq!(
        PcurveNurbs::from_checked_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            pcurve.knots().clone(),
            pcurve.control_points(),
            pcurve.weights(),
            true,
        )
        .expect("fixture pcurve construction admission"),
        Ok(pcurve.clone())
    );
    let mut mapped = pcurve.clone();
    mapped
        .try_map_control_points(
            |_, point| Ok::<_, NurbsError>(point.negated()),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("pole edit admission")
        .unwrap();
    assert_eq!(
        mapped.control_points(),
        vec![Point2::new(-1.0, -2.0), Point2::new(-3.0, -4.0)]
    );
    let lifted = pcurve
        .lift(|point| Point3::new(point.u, point.v, 0.0))
        .unwrap();
    assert_eq!(lifted.weights(), pcurve.weights());
    assert_eq!(
        pcurve.lift(|_| Point3::new(f64::NAN, 0.0, 0.0)),
        Err(NurbsError::Structure(
            "control_points contains a non-finite point".into()
        ))
    );

    let polar = polar();
    assert_eq!(
        PolarPcurveNurbs::new(
            &cadmpeg_test_support::service_decode_context(),
            1,
            polar.knots().to_vec(),
            polar.pole_rows().clone(),
            true
        )
        .expect("fixture pcurve construction admission"),
        Ok(polar.clone())
    );
    assert_eq!(
        PolarPcurveNurbs::from_checked_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            polar.knots().clone(),
            polar.poles(),
            polar.weights(),
            true,
        )
        .expect("fixture pcurve construction admission"),
        Ok(polar.clone())
    );
    assert_eq!(
        polar.axial_control_values(),
        vec![FiniteReal::new(5.0).unwrap(), FiniteReal::new(6.0).unwrap()]
    );
}

#[test]
fn context_free_pole_reconstruction_does_not_enter_a_decode_constructor() {
    use crate::geometry::nurbs::{NurbsCurve, NurbsError, NurbsPoleGrid, NurbsPoles3, NurbsSurfaceAxis, PoleValue};
    use crate::features::FinitePoint3;

    #[derive(Clone, Copy)]
    struct Pole(Point3);
    impl PoleValue<FinitePoint3> for Pole {
        fn admit(self) -> Option<FinitePoint3> {
            FinitePoint3::new(self.0)
        }
        fn admit_curve_poles<E>(
            poles: NurbsPoles3<Self>,
            convert: impl FnOnce(NurbsPoles3<Self>) -> Result<NurbsPoles3<FinitePoint3>, E>,
        ) -> Result<NurbsPoles3<FinitePoint3>, E> {
            assert!(
                std::any::type_name::<E>() == std::any::type_name::<NurbsError>(),
                "context-free curve reconstruction must not start a decode session"
            );
            convert(poles)
        }
        fn admit_surface_poles<E>(
            grid: NurbsPoleGrid<Self>,
            convert: impl FnOnce(NurbsPoleGrid<Self>) -> Result<NurbsPoleGrid<FinitePoint3>, E>,
        ) -> Result<NurbsPoleGrid<FinitePoint3>, E> {
            assert!(
                std::any::type_name::<E>() == std::any::type_name::<NurbsError>(),
                "context-free surface reconstruction must not start a decode session"
            );
            convert(grid)
        }
    }
    let points = vec![
        Pole(Point3::new(0.0, 0.0, 0.0)),
        Pole(Point3::new(1.0, 0.0, 0.0)),
    ];
    let curve = crate::geometry::nurbs::build_curve(
        &crate::geometry::nurbs::StandardNurbsAdmission,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        NurbsPoles3::Polynomial {
            points: points.clone(),
        },
        false,
    )
    .unwrap();
    assert_eq!(
        curve.control_points(),
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
    );
    let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    let surface = crate::geometry::nurbs::build_surface(
        &crate::geometry::nurbs::StandardNurbsAdmission,
        axis(),
        axis(),
        NurbsPoleGrid::Polynomial {
            rows: vec![points.clone(), points],
        },
        false,
    )
    .unwrap();
    assert_eq!(surface.u_count(), 2);
    assert_eq!(surface.v_count(), 2);
    assert_eq!(
        serde_json::from_value::<NurbsCurve>(serde_json::to_value(&curve).unwrap()).unwrap(),
        curve
    );
    assert_eq!(
        serde_json::from_value::<NurbsSurface>(serde_json::to_value(&surface).unwrap()).unwrap(),
        surface
    );
}

