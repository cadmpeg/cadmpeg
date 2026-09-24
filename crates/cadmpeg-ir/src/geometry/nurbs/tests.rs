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
    assert_eq!(curve.control_points(), curve.pole_rows().points());
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
    assert_eq!(surface.control_grid(), surface.pole_grid().points());
    assert_eq!(surface.poles(), surface.pole_grid().points().concat());
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
    assert_eq!(pcurve.control_points(), pcurve.pole_rows().points());
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
