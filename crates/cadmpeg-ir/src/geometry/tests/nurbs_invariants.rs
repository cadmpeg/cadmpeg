// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{
    DirectedParameterRange, IntcurveSupportSide, NurbsCurve, NurbsSurface, PcurveGeometry,
    PcurveNurbs, PolarPcurveNurbs, SupportPcurve,
};
use crate::math::{Point2, Point3};

fn curve() -> NurbsCurve {
    NurbsCurve::new(
        1,
        vec![2.0, 2.0, 5.0, 5.0],
        vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        Some(vec![-1.0, 2.0]),
        true,
    )
    .unwrap()
}

fn surface() -> NurbsSurface {
    NurbsSurface::new(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![2.0, 2.0, 5.0, 5.0],
        vec![
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
        ],
        Some(vec![-1.0, 1.0, 2.0, -2.0])
            .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
        true,
        true,
        false,
    )
    .unwrap()
}

fn pcurve() -> PcurveNurbs {
    PcurveNurbs::new(
        1,
        vec![2.0, 2.0, 5.0, 5.0],
        vec![Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)],
        Some(vec![1.0, 2.0]),
        true,
    )
    .unwrap()
}

fn polar() -> PolarPcurveNurbs {
    PolarPcurveNurbs::new(
        1,
        vec![2.0, 2.0, 5.0, 5.0],
        vec![
            crate::geometry::PolarNurbsPole {
                radial: Point2::new(1.0, 2.0),
                axial: 5.0,
            },
            crate::geometry::PolarNurbsPole {
                radial: Point2::new(3.0, 4.0),
                axial: 6.0,
            },
        ],
        Some(vec![1.0, 2.0]),
        true,
    )
    .unwrap()
}

#[test]
fn construction_rejects_invalid_knots_and_non_finite_poles() {
    fn rejects_descending_knots<T: serde::Serialize + serde::de::DeserializeOwned>(carrier: T) {
        let mut wire = serde_json::to_value(carrier).unwrap();
        wire["knots"] = serde_json::json!([2.0, 5.0, 2.0, 5.0]);
        assert!(serde_json::from_value::<T>(wire).is_err());
    }

    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut knots = curve().knots().to_vec();
        knots[1] = invalid;
        assert!(NurbsCurve::new(
            1,
            knots.clone(),
            curve().control_points().to_vec(),
            None,
            false
        )
        .is_err());
        assert!(PcurveNurbs::new(
            1,
            knots.clone(),
            pcurve().control_points().to_vec(),
            None,
            false
        )
        .is_err());
        assert!(PolarPcurveNurbs::new(
            1,
            knots,
            vec![
                crate::geometry::PolarNurbsPole {
                    radial: Point2::new(0.0, 0.0),
                    axial: 0.0,
                };
                2
            ],
            None,
            false
        )
        .is_err());

        let mut points = curve().control_points().to_vec();
        points[1].z = invalid;
        assert!(NurbsCurve::new(1, curve().knots().to_vec(), points, None, false).is_err());
        assert!(PcurveNurbs::new(
            1,
            pcurve().knots().to_vec(),
            vec![Point2::new(0.0, invalid); 2],
            None,
            false
        )
        .is_err());
        assert!(PolarPcurveNurbs::new(
            1,
            polar().knots().to_vec(),
            vec![
                crate::geometry::PolarNurbsPole {
                    radial: Point2::new(0.0, 0.0),
                    axial: invalid,
                };
                2
            ],
            None,
            false
        )
        .is_err());

        let source = surface();
        let mut points = source.control_grid().to_vec();
        points[0][1].x = invalid;
        assert!(NurbsSurface::new(
            1,
            1,
            source.u_knots().to_vec(),
            source.v_knots().to_vec(),
            points,
            None,
            false,
            false,
            false
        )
        .is_err());
    }

    rejects_descending_knots(curve());
    rejects_descending_knots(pcurve());
    rejects_descending_knots(polar());
    for axis in ["u_knots", "v_knots"] {
        let mut wire = serde_json::to_value(surface()).unwrap();
        wire[axis] = serde_json::json!([0.0, 1.0, 0.0, 1.0]);
        assert!(serde_json::from_value::<NurbsSurface>(wire).is_err());
    }
}

#[test]
fn weight_rules_preserve_signed_3d_and_positive_parameter_space_carriers() {
    let mut curve = curve();
    let mut surface = surface();
    let mut pcurve = pcurve();
    let mut polar = polar();
    curve.set_weights(Some(vec![1e-200, -1e-200])).unwrap();
    surface
        .set_weights(Some(vec![vec![-1e-200; 2]; 2]))
        .unwrap();
    pcurve.set_weights(Some(vec![1e-200; 2])).unwrap();
    polar.set_weights(Some(vec![1e-200; 2])).unwrap();
    for invalid in [0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(curve.set_weights(Some(vec![invalid, 1.0])).is_err());
        assert!(surface
            .set_weights(Some(vec![vec![invalid; 2]; 2]))
            .is_err());
        assert!(pcurve.set_weights(Some(vec![invalid, 1.0])).is_err());
        assert!(polar.set_weights(Some(vec![invalid, 1.0])).is_err());
    }
    assert!(pcurve.set_weights(Some(vec![-1.0, 1.0])).is_err());
    assert!(polar.set_weights(Some(vec![-1.0, 1.0])).is_err());
    assert_eq!(
        serde_json::from_value::<NurbsCurve>(serde_json::to_value(&curve).unwrap()).unwrap(),
        curve
    );
    assert_eq!(
        serde_json::from_value::<NurbsSurface>(serde_json::to_value(&surface).unwrap()).unwrap(),
        surface
    );
    assert_eq!(
        serde_json::from_value::<PcurveNurbs>(serde_json::to_value(&pcurve).unwrap()).unwrap(),
        pcurve
    );
    assert_eq!(
        serde_json::from_value::<PolarPcurveNurbs>(serde_json::to_value(&polar).unwrap()).unwrap(),
        polar
    );

    let mut wire = serde_json::to_value(&curve).unwrap();
    wire["weights"] = serde_json::json!([0.0, 1.0]);
    assert!(serde_json::from_value::<NurbsCurve>(wire).is_err());
    let mut wire = serde_json::to_value(&surface).unwrap();
    wire["weights"] = serde_json::json!([1.0, 1.0, 0.0, 1.0]);
    assert!(serde_json::from_value::<NurbsSurface>(wire).is_err());
    let mut wire = serde_json::to_value(&pcurve).unwrap();
    wire["weights"] = serde_json::json!([-1.0, 1.0]);
    assert!(serde_json::from_value::<PcurveNurbs>(wire).is_err());
    let mut wire = serde_json::to_value(&polar).unwrap();
    wire["weights"] = serde_json::json!([-1.0, 1.0]);
    assert!(serde_json::from_value::<PolarPcurveNurbs>(wire).is_err());
}

#[test]
fn failed_numeric_edits_preserve_the_whole_carrier() {
    let mut curve = curve();
    let original = curve.clone();
    assert!(curve.edit_knots(<[f64]>::reverse).is_err());
    assert!(curve
        .edit_control_points(|points| points[0].x = f64::INFINITY)
        .is_err());
    assert!(curve.edit_weights(|weights| weights[0] = 0.0).is_err());
    assert!(curve.set_weights(Some(vec![1.0])).is_err());
    assert_eq!(curve, original);

    let mut surface = surface();
    let original = surface.clone();
    assert!(surface.edit_u_knots(<[f64]>::reverse).is_err());
    assert!(surface.edit_v_knots(|knots| knots[1] = f64::NAN).is_err());
    assert!(surface
        .edit_control_points(|points| points[1][1].y = f64::NEG_INFINITY)
        .is_err());
    assert!(surface.edit_weights(|weights| weights[1][0] = 0.0).is_err());
    assert!(surface.set_weights(Some(vec![vec![1.0]])).is_err());
    assert_eq!(surface, original);

    let mut pcurve = pcurve();
    let original = pcurve.clone();
    assert!(pcurve.edit_knots(<[f64]>::reverse).is_err());
    assert!(pcurve
        .edit_control_points(|points| points[1].u = f64::NAN)
        .is_err());
    assert!(pcurve.edit_weights(|weights| weights[1] = -1.0).is_err());
    assert!(pcurve.set_weights(Some(vec![1.0])).is_err());
    assert_eq!(pcurve, original);

    let mut polar = polar();
    let original = polar.clone();
    assert!(polar.edit_knots(<[f64]>::reverse).is_err());
    assert!(polar
        .edit_poles(|poles| poles[1].radial.v = f64::INFINITY)
        .is_err());
    assert!(polar.edit_poles(|poles| poles[0].axial = f64::NAN).is_err());
    assert!(polar.edit_weights(|weights| weights[1] = -1.0).is_err());
    assert!(polar.set_weights(Some(vec![1.0])).is_err());
    assert_eq!(polar, original);
}

#[test]
fn reversal_preserves_weight_and_parameter_correspondence() {
    let mut curve = curve();
    let original = curve.clone();
    curve.reverse_parameterization();
    assert_eq!(curve.knots(), &[-5.0, -5.0, -2.0, -2.0]);
    assert_eq!(
        curve.control_points(),
        &[original.control_points()[1], original.control_points()[0]]
    );
    assert_eq!(curve.weights(), Some(&[2.0, -1.0][..]));
    curve.reverse_parameterization();
    assert_eq!(curve, original);

    let mut pcurve = pcurve();
    let original = pcurve.clone();
    pcurve.reverse_parameterization();
    assert_eq!(pcurve.knots(), &[-5.0, -5.0, -2.0, -2.0]);
    assert_eq!(
        pcurve.control_points(),
        &[original.control_points()[1], original.control_points()[0]]
    );
    assert_eq!(pcurve.weights(), Some(&[2.0, 1.0][..]));
    pcurve.reverse_parameterization();
    assert_eq!(pcurve, original);

    let mut polar = polar();
    let original = polar.clone();
    polar.reverse_parameterization();
    assert_eq!(polar.knots(), &[-5.0, -5.0, -2.0, -2.0]);
    assert_eq!(polar.poles(), &[original.poles()[1], original.poles()[0]]);
    assert_eq!(polar.weights(), Some(&[2.0, 1.0][..]));
    polar.reverse_parameterization();
    assert_eq!(polar, original);
}

#[test]
fn support_mapping_preserves_decreasing_parameter_direction() {
    for endpoints in [[0.0, 0.0], [0.0, f64::NAN], [f64::INFINITY, 0.0]] {
        assert!(DirectedParameterRange::new(endpoints).is_err());
    }
    assert!(serde_json::from_str::<DirectedParameterRange>("[2.0,2.0]").is_err());
    let side = IntcurveSupportSide {
        surface: None,
        pcurve: Some(SupportPcurve::new(
            PcurveGeometry::Nurbs { nurbs: pcurve() },
            Some(DirectedParameterRange::new([5.0, 2.0]).unwrap()),
        )),
    };
    assert_eq!(side.pcurve_parameter([0.0, 1.0], 0.0), Some(5.0));
    assert_eq!(side.pcurve_parameter([0.0, 1.0], 1.0), Some(2.0));
    assert_eq!(
        serde_json::from_value::<IntcurveSupportSide>(serde_json::to_value(&side).unwrap())
            .unwrap(),
        side
    );
}

#[test]
fn finite_support_endpoints_do_not_require_a_representable_span() {
    for (solved, mapped, midpoint) in [
        ([-f64::MAX, f64::MAX], [5.0, 2.0], 3.5),
        ([-1.0, 1.0], [-f64::MAX, f64::MAX], 0.0),
        ([-f64::MAX, f64::MAX], [f64::MAX, -f64::MAX], 0.0),
    ] {
        let side = IntcurveSupportSide {
            surface: None,
            pcurve: Some(SupportPcurve::new(
                PcurveGeometry::Nurbs { nurbs: pcurve() },
                Some(DirectedParameterRange::new(mapped).unwrap()),
            )),
        };
        assert_eq!(side.pcurve_parameter(solved, solved[0]), Some(mapped[0]));
        assert_eq!(side.pcurve_parameter(solved, 0.0), Some(midpoint));
        assert_eq!(side.pcurve_parameter(solved, solved[1]), Some(mapped[1]));
        assert_eq!(side.pcurve_parameter(solved, f64::NAN), None);
    }
}

#[test]
fn support_mapping_preserves_endpoints_despite_subtraction_cancellation() {
    let side = IntcurveSupportSide {
        surface: None,
        pcurve: Some(SupportPcurve::new(
            PcurveGeometry::Nurbs { nurbs: pcurve() },
            Some(DirectedParameterRange::new([1e16, 1.0]).unwrap()),
        )),
    };
    assert_eq!(side.pcurve_parameter([0.0, 1.0], 0.0), Some(1e16));
    assert_eq!(side.pcurve_parameter([0.0, 1.0], 1.0), Some(1.0));
}

/// The control grid states both pole counts, so the surface wire carries no
/// `u_count` or `v_count`, and rows of unequal length are refused.
#[test]
fn a_nurbs_surface_states_its_pole_counts_in_its_control_grid() {
    let surface = surface();
    let wire = serde_json::to_value(&surface).expect("serializes");
    assert!(wire.get("u_count").is_none());
    assert!(wire.get("v_count").is_none());
    assert_eq!(wire["control_points"].as_array().expect("rows").len(), 2);
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
    ragged["control_points"][1] = serde_json::json!([{"x": 0.0, "y": 0.0, "z": 0.0}]);
    let error = serde_json::from_value::<NurbsSurface>(ragged)
        .unwrap_err()
        .to_string();
    assert!(error.contains("control_points row"), "{error}");
}
