// SPDX-License-Identifier: Apache-2.0
use crate::{
    geometry::nurbs::NurbsSurface,
    math::Point3,
    test_support::nurbs::{curve, surface},
};

#[test]
fn a_refused_curve_pole_edit_keeps_the_prior_poles() {
    let mut curve = curve();
    let original = curve.clone();
    let refusal = curve.edit_control_points(|point| {
        point.z = 9.0;
        Err(crate::geometry::nurbs::NurbsError::EditRefused(
            "caller refused this pole".into(),
        ))
    });
    assert_eq!(
        refusal,
        Err(crate::geometry::nurbs::NurbsError::EditRefused(
            "caller refused this pole".into()
        ))
    );
    assert_eq!(curve, original);
}

#[test]
fn a_refused_surface_pole_edit_keeps_the_prior_poles() {
    let mut surface = surface();
    let original = surface.clone();
    let refusal = surface.edit_control_points(|point| {
        point.z = 9.0;
        Err(crate::geometry::nurbs::NurbsError::EditRefused(
            "caller refused this pole".into(),
        ))
    });
    assert_eq!(
        refusal,
        Err(crate::geometry::nurbs::NurbsError::EditRefused(
            "caller refused this pole".into()
        ))
    );
    assert_eq!(surface, original);
}

/// The control grid states both pole counts, so the surface wire carries no
/// `u_count` or `v_count`, and rows of unequal length are refused.
#[test]
fn a_nurbs_surface_states_its_pole_counts_in_its_control_grid() {
    let surface = surface();
    let wire = serde_json::to_value(&surface).expect("serializes");
    assert!(wire.get("u_count").is_none());
    assert!(wire.get("v_count").is_none());
    assert_eq!(wire["poles"]["rows"].as_array().expect("rows").len(), 2);
    assert_eq!(surface.u_count(), 2);
    assert_eq!(surface.v_count(), 2);
    assert_eq!(
        serde_json::from_value::<NurbsSurface>(wire.clone()).expect("round trip"),
        surface
    );

    let mut restated = wire.clone();
    restated["u_count"] = serde_json::json!(2);
    let error = serde_json::from_value::<NurbsSurface>(restated)
        .unwrap_err()
        .to_string();
    assert!(error.contains("u_count"), "{error}");

    let mut ragged = wire;
    ragged["poles"]["rows"][1] =
        serde_json::json!([{"point": {"x": 0.0, "y": 0.0, "z": 0.0}, "weight": 1.0}]);
    let error = serde_json::from_value::<NurbsSurface>(ragged)
        .unwrap_err()
        .to_string();
    assert!(error.contains("control_points row"), "{error}");
}

#[test]
fn surface_transposition_preserves_every_pole_and_weight() {
    let points = vec![
        vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        vec![Point3::new(7.0, 8.0, 9.0), Point3::new(10.0, 11.0, 12.0)],
        vec![Point3::new(13.0, 14.0, 15.0), Point3::new(16.0, 17.0, 18.0)],
    ];
    for weights in [
        None,
        Some(vec![vec![-1.0, 2.0], vec![3.0, -4.0], vec![5.0, 6.0]]),
    ] {
        let mut surface = NurbsSurface::from_lanes(
            crate::geometry::nurbs::NurbsSurfaceAxis::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                true,
            ),
            crate::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![2.0, 2.0, 5.0, 5.0], false),
            crate::geometry::nurbs::NurbsSurfaceLanes::new(points.clone(), weights),
            true,
        )
        .unwrap();
        let original = surface.clone();
        surface.transpose_parameter_axes();
        assert_eq!((surface.u_count(), surface.v_count()), (2, 3));
        assert_eq!((surface.u_degree(), surface.v_degree()), (1, 2));
        assert_eq!(surface.u_knots(), original.v_knots());
        assert_eq!(surface.v_knots(), original.u_knots());
        assert_eq!((surface.u_periodic(), surface.v_periodic()), (false, true));
        assert!(surface.normal_reversed());
        for u in 0..3 {
            for v in 0..2 {
                assert_eq!(surface.pole(v, u), original.pole(u, v));
                assert_eq!(surface.weight(v, u), original.weight(u, v));
            }
        }
        surface.transpose_parameter_axes();
        assert_eq!(surface, original);
    }
}

#[test]
fn bspline_surface_edit_refusal_keeps_control_points() {
    use crate::geometry::nurbs::{BsplineSurface, NurbsError};
    let points = vec![vec![Point3::new(0.0, 0.0, 0.0); 2]; 2];
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    let mut surface = BsplineSurface::new(1, 1, knots.clone(), knots, points).unwrap();
    let original = surface.clone();
    let refusal = surface.edit_control_points(|point| {
        point.z = 3.0;
        Err(NurbsError::EditRefused("caller refused this pole".into()))
    });
    assert_eq!(
        refusal,
        Err(NurbsError::EditRefused("caller refused this pole".into()))
    );
    assert_eq!(surface, original);
}

#[test]
fn bspline_surface_numeric_admission_and_transactional_edit() {
    use crate::geometry::nurbs::BsplineSurface;
    use crate::math::Point3;
    let points = vec![vec![Point3::new(0.0, 0.0, 0.0); 2]; 2];
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    assert!(BsplineSurface::new(
        1,
        1,
        vec![0.0, 1.0, 0.0, 1.0],
        knots.clone(),
        points.clone()
    )
    .is_err());
    let mut surface = BsplineSurface::new(1, 1, knots.clone(), knots, points).unwrap();
    let original = surface.clone();
    assert!(surface
        .edit_control_points(|point| {
            point.x = f64::NAN;
            Ok(())
        })
        .is_err());
    assert_eq!(surface, original);
    let mut wire = serde_json::to_value(&surface).unwrap();
    wire["u_knots"] = serde_json::json!([0.0, 1.0, 0.0, 1.0]);
    assert!(serde_json::from_value::<BsplineSurface>(wire).is_err());
    surface
        .edit_control_points(|point| {
            point.z = 2.0;
            Ok(())
        })
        .unwrap();
    assert!(surface
        .control_points
        .iter()
        .flatten()
        .all(|point| point.z == 2.0));
}

#[test]
fn nurbs_stores_hand_out_their_admitted_poles_knots_and_weights() {
    use crate::test_support::nurbs::{pcurve, polar};

    let curve = curve();
    assert_eq!(curve.control_points(), curve.pole_rows().raw_points());
    assert_eq!(curve.knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        curve.knots().iter().copied().collect::<Vec<_>>(),
        [2.0, 2.0, 5.0, 5.0]
    );
    assert_eq!(
        curve.weights().map(|weights| weights
            .into_iter()
            .map(crate::scalar::NonZeroReal::get)
            .collect()),
        curve.pole_rows().weights()
    );
    assert_eq!(curve.full_knot_endpoints().endpoints(), [2.0, 5.0]);
    assert_eq!(
        crate::eval::nurbs_curve_parameter_domain(&curve)
            .map(crate::topology::IncreasingParameterInterval::endpoints),
        Some([2.0, 5.0])
    );
    assert_eq!(
        crate::eval::nurbs_pcurve_parameter_domain(1, &[0.0, 1.0, 1.0, 2.0], 2),
        None
    );
    let mut reversed = curve.clone();
    reversed.reverse_parameterization();
    assert_eq!(reversed.knots().as_slice(), [-5.0, -5.0, -2.0, -2.0]);

    let surface = surface();
    assert_eq!(surface.control_grid(), surface.pole_grid().raw_points());
    assert_eq!(surface.poles(), surface.pole_grid().raw_points().concat());
    assert_eq!(
        surface.pole(1, 0).map(crate::features::FinitePoint3::get),
        Some(Point3::new(1.0, 0.0, 0.0))
    );
    assert_eq!(surface.pole(2, 0), None);
    assert_eq!(surface.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        surface.weight(1, 1).map(crate::scalar::NonZeroReal::get),
        Some(-2.0)
    );
    assert_eq!(
        surface.pole_weights().map(|weights| weights
            .into_iter()
            .map(crate::scalar::NonZeroReal::get)
            .collect()),
        surface.pole_grid().weights().map(|rows| rows.concat())
    );

    let pcurve = pcurve();
    assert_eq!(pcurve.control_points(), pcurve.pole_rows().raw_points());
    assert_eq!(pcurve.knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        pcurve.weights().map(|weights| weights
            .into_iter()
            .map(crate::scalar::NonZeroReal::get)
            .collect()),
        pcurve.pole_rows().weights()
    );

    let polar = polar();
    assert_eq!(polar.knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        polar.weights().map(|weights| weights
            .into_iter()
            .map(crate::scalar::NonZeroReal::get)
            .collect::<Vec<_>>()),
        Some(vec![1.0, 2.0])
    );
}

#[test]
fn a_bspline_surface_holds_its_admitted_knots_and_poles() {
    use crate::features::FinitePoint3;
    use crate::geometry::nurbs::{BsplineSurface, KnotVector};
    use crate::math::Point3;

    let points = vec![
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
        vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 2.0)],
    ];
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    let surface = BsplineSurface::new(1, 1, knots.clone(), knots.clone(), points.clone()).unwrap();
    assert_eq!(surface.u_knots, KnotVector::new(knots.clone()).unwrap());
    assert_eq!(
        surface.control_points[1][1],
        FinitePoint3::new(Point3::new(1.0, 1.0, 2.0)).unwrap()
    );
    let wire = serde_json::to_value(&surface).unwrap();
    assert_eq!(wire["u_knots"], serde_json::json!(knots));
    assert_eq!(wire["control_points"], serde_json::json!(points));
    assert_eq!(
        serde_json::from_value::<BsplineSurface>(wire).unwrap(),
        surface
    );
}

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
        NurbsCurve::new(1, curve.knots().to_vec(), held.clone(), true),
        Ok(curve.clone())
    );
    assert_eq!(held.to_raw().points(), curve.pole_rows().raw_points());
    assert_eq!(
        NurbsCurve::from_checked_lanes(
            1,
            curve.knots().to_vec(),
            curve.control_points(),
            curve.weights(),
            true,
        ),
        Ok(curve.clone())
    );
    let weight = NonZeroReal::new(1.0).unwrap();
    assert_eq!(
        NurbsCurve::from_checked_lanes(
            1,
            curve.knots().to_vec(),
            curve.control_points(),
            Some(vec![weight]),
            true,
        ),
        Err(NurbsError::WeightLaneLength {
            field: "poles".to_owned(),
            poles: 2,
            weights: 1,
        })
    );
    let non_finite = vec![Point3::new(f64::NAN, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    let raw_refusal =
        NurbsCurve::from_lanes(1, vec![0.0, 0.0, 1.0, 1.0], non_finite.clone(), None, false)
            .unwrap_err();
    assert_eq!(
        NurbsCurve::from_checked_lanes(1, vec![0.0, 0.0, 1.0, 1.0], non_finite, None, false),
        Err(raw_refusal)
    );
    assert_eq!(
        NurbsCurve::from_checked_lanes(
            4,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(f64::NAN, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        ),
        Err(NurbsError::Structure(
            "control_points must contain more than degree 4 poles, found 2".into()
        ))
    );
    assert_eq!(
        crate::eval::nurbs_curve_point_at(&curve, 3.0),
        crate::eval::nurbs_curve_point(
            1,
            curve.knots(),
            &curve.pole_rows().raw_points(),
            curve.pole_rows().weights().as_deref(),
            3.0,
        )
    );

    let mut mapped = curve.clone();
    let refusal = mapped.map_control_points(|_| Err(NurbsError::EditRefused("kept".into())));
    assert_eq!(refusal, Err(NurbsError::EditRefused("kept".into())));
    assert_eq!(mapped, curve);
    mapped
        .map_control_points(|point| Ok(point.negated()))
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
        crate::geometry::nurbs::NurbsSurface::new(u(), v(), surface.pole_grid().clone(), true),
        Ok(surface.clone())
    );
    assert_eq!(
        crate::geometry::nurbs::NurbsSurface::from_checked_lanes(
            u(),
            v(),
            NurbsSurfaceLanes::new(surface.control_grid(), surface.weights()),
            true,
        ),
        Ok(surface.clone())
    );
    assert_eq!(
        positive_controls(&surface.poles(), &[1.0, 1.0, 2.0, 2.0]),
        positive_controls(
            &surface.pole_grid().raw_points().concat(),
            &[1.0, 1.0, 2.0, 2.0]
        )
    );
    assert_eq!(
        positive_controls(&[Point3::new(f64::INFINITY, 0.0, 0.0)], &[1.0]),
        None
    );
    let mut mapped = surface.clone();
    mapped
        .map_control_points(|point| Ok(point.negated()))
        .unwrap();
    assert_eq!(
        mapped.pole(1, 1).map(FinitePoint3::get),
        Some(Point3::new(-1.0, -1.0, 0.0))
    );
    assert_eq!(mapped.weights(), surface.weights());

    let pcurve = pcurve();
    let held: &PcurveNurbsPoles<FinitePoint2> = pcurve.pole_rows();
    assert_eq!(
        PcurveNurbs::new(1, pcurve.knots().to_vec(), held.clone(), true),
        Ok(pcurve.clone())
    );
    assert_eq!(
        PcurveNurbs::from_checked_lanes(
            1,
            pcurve.knots().to_vec(),
            pcurve.control_points(),
            pcurve.weights(),
            true,
        ),
        Ok(pcurve.clone())
    );
    let mut mapped = pcurve.clone();
    mapped
        .map_control_points(|point| Ok(point.negated()))
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
        PolarPcurveNurbs::new(1, polar.knots().to_vec(), polar.pole_rows().clone(), true),
        Ok(polar.clone())
    );
    assert_eq!(
        PolarPcurveNurbs::from_checked_lanes(
            1,
            polar.knots().to_vec(),
            polar.poles(),
            polar.weights(),
            true,
        ),
        Ok(polar.clone())
    );
    assert_eq!(
        polar.axial_control_values(),
        vec![FiniteReal::new(5.0).unwrap(), FiniteReal::new(6.0).unwrap()]
    );
}
