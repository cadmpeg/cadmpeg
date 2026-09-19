// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::geometry::analytic::EllipseCurve;
use crate::geometry::pcurve::{
    CirclePcurve, EllipsePcurve, HarmonicPcurve, LinePcurve, OffsetPcurve, PcurveGeometry,
    SphericalGreatCirclePcurve, TrimmedPcurve,
};
use crate::geometry::{
    nurbs::{NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface},
    pcurve::{PcurveNurbs, PcurveNurbsPoles, PolarNurbsPoles, PolarPcurveNurbs},
};
use crate::math::{Point2, Point3, Vector3};
use crate::test_support::nurbs::{curve, pcurve, polar, surface};

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
        assert!(
            NurbsCurve::from_lanes(1, knots.clone(), curve().control_points(), None, false)
                .is_err()
        );
        assert!(
            PcurveNurbs::from_lanes(1, knots.clone(), pcurve().control_points(), None, false)
                .is_err()
        );
        assert!(PolarPcurveNurbs::from_lanes(
            1,
            knots,
            vec![
                crate::geometry::pcurve::PolarNurbsPole {
                    radial: Point2::new(0.0, 0.0),
                    axial: 0.0,
                };
                2
            ],
            None,
            false
        )
        .is_err());

        let mut points = curve().control_points();
        points[1].z = invalid;
        assert!(NurbsCurve::from_lanes(1, curve().knots().to_vec(), points, None, false).is_err());
        assert!(PcurveNurbs::from_lanes(
            1,
            pcurve().knots().to_vec(),
            vec![Point2::new(0.0, invalid); 2],
            None,
            false
        )
        .is_err());
        assert!(PolarPcurveNurbs::from_lanes(
            1,
            polar().knots().to_vec(),
            vec![
                crate::geometry::pcurve::PolarNurbsPole {
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
        let mut points = source.control_grid();
        points[0][1].x = invalid;
        assert!(NurbsSurface::from_lanes(
            crate::geometry::nurbs::NurbsSurfaceAxis::new(1, source.u_knots().to_vec(), false),
            crate::geometry::nurbs::NurbsSurfaceAxis::new(1, source.v_knots().to_vec(), false),
            crate::geometry::nurbs::NurbsSurfaceLanes::new(points, None),
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

fn curve_weights(
    curve: &NurbsCurve,
    weights: Vec<f64>,
) -> Result<NurbsPoles3, crate::geometry::nurbs::NurbsError> {
    NurbsPoles3::from_lanes(curve.control_points(), Some(weights))
}

fn surface_weights(
    surface: &NurbsSurface,
    weights: Vec<Vec<f64>>,
) -> Result<NurbsPoleGrid, crate::geometry::nurbs::NurbsError> {
    NurbsPoleGrid::from_lanes(surface.control_grid(), Some(weights))
}

fn pcurve_weights(
    pcurve: &PcurveNurbs,
    weights: Vec<f64>,
) -> Result<PcurveNurbsPoles, crate::geometry::nurbs::NurbsError> {
    PcurveNurbsPoles::from_lanes(pcurve.control_points(), Some(weights))
}

fn polar_weights(
    polar: &PolarPcurveNurbs,
    weights: Vec<f64>,
) -> Result<PolarNurbsPoles, crate::geometry::nurbs::NurbsError> {
    PolarNurbsPoles::from_lanes(polar.poles(), Some(weights))
}

#[test]
fn weight_rules_preserve_signed_3d_and_positive_parameter_space_carriers() {
    let mut curve = curve();
    let mut surface = surface();
    let mut pcurve = pcurve();
    let mut polar = polar();
    curve
        .set_poles(curve_weights(&curve, vec![1e-200, -1e-200]).unwrap())
        .unwrap();
    surface
        .set_poles(surface_weights(&surface, vec![vec![-1e-200; 2]; 2]).unwrap())
        .unwrap();
    pcurve
        .set_poles(pcurve_weights(&pcurve, vec![1e-200; 2]).unwrap())
        .unwrap();
    polar
        .set_poles(polar_weights(&polar, vec![1e-200; 2]).unwrap())
        .unwrap();
    for invalid in [0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(curve_weights(&curve, vec![invalid, 1.0]).is_err());
        assert!(surface_weights(&surface, vec![vec![invalid; 2]; 2]).is_err());
        assert!(pcurve_weights(&pcurve, vec![invalid, 1.0]).is_err());
        assert!(polar_weights(&polar, vec![invalid, 1.0]).is_err());
    }
    assert!(pcurve_weights(&pcurve, vec![-1.0, 1.0]).is_err());
    assert!(polar_weights(&polar, vec![-1.0, 1.0]).is_err());
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

    // A weight travels in its pole row, so a weight list beside the poles is
    // an unknown key and a zero or wrongly signed weight is refused at the
    // pole's own scalar mint.
    let mut wire = serde_json::to_value(&curve).unwrap();
    wire["weights"] = serde_json::json!([0.0, 1.0]);
    assert!(serde_json::from_value::<NurbsCurve>(wire).is_err());
    let mut wire = serde_json::to_value(&curve).unwrap();
    wire["poles"]["points"][0]["weight"] = serde_json::json!(0.0);
    assert!(serde_json::from_value::<NurbsCurve>(wire).is_err());
    let mut wire = serde_json::to_value(&surface).unwrap();
    wire["poles"]["rows"][0][0]["weight"] = serde_json::json!(0.0);
    assert!(serde_json::from_value::<NurbsSurface>(wire).is_err());
    let mut wire = serde_json::to_value(&pcurve).unwrap();
    wire["poles"]["points"][0]["weight"] = serde_json::json!(-1.0);
    assert!(serde_json::from_value::<PcurveNurbs>(wire).is_err());
    let mut wire = serde_json::to_value(&polar).unwrap();
    wire["poles"]["poles"][0]["weight"] = serde_json::json!(-1.0);
    assert!(serde_json::from_value::<PolarPcurveNurbs>(wire).is_err());
}

#[test]
fn failed_numeric_edits_preserve_the_whole_carrier() {
    let mut curve = curve();
    let original = curve.clone();
    assert!(curve.edit_knots(<[f64]>::reverse).is_err());
    assert!(curve
        .edit_control_points(|point| {
            point.x = f64::INFINITY;
            Ok(())
        })
        .is_err());
    assert!(curve_weights(&curve, vec![0.0, 1.0]).is_err());
    assert!(curve_weights(&curve, vec![1.0]).is_err());
    assert_eq!(curve, original);

    let mut surface = surface();
    let original = surface.clone();
    assert!(surface.edit_u_knots(<[f64]>::reverse).is_err());
    assert!(surface.edit_v_knots(|knots| knots[1] = f64::NAN).is_err());
    assert!(surface
        .edit_control_points(|point| {
            point.y = f64::NEG_INFINITY;
            Ok(())
        })
        .is_err());
    assert!(surface_weights(&surface, vec![vec![0.0, 1.0], vec![1.0, 1.0]]).is_err());
    assert!(surface_weights(&surface, vec![vec![1.0]]).is_err());
    assert_eq!(surface, original);

    let mut pcurve = pcurve();
    let original = pcurve.clone();
    assert!(pcurve.edit_knots(<[f64]>::reverse).is_err());
    assert!(pcurve
        .edit_control_points(|point| {
            point.u = f64::NAN;
            Ok(())
        })
        .is_err());
    assert!(pcurve_weights(&pcurve, vec![1.0, -1.0]).is_err());
    assert!(pcurve_weights(&pcurve, vec![1.0]).is_err());
    assert_eq!(pcurve, original);

    let mut polar = polar();
    let original = polar.clone();
    assert!(polar.edit_knots(<[f64]>::reverse).is_err());
    assert!(polar
        .edit_poles(|radial, _| {
            radial.v = f64::INFINITY;
            Ok(())
        })
        .is_err());
    assert!(polar
        .edit_poles(|_, axial| {
            *axial = f64::NAN;
            Ok(())
        })
        .is_err());
    assert!(polar_weights(&polar, vec![1.0, -1.0]).is_err());
    assert!(polar_weights(&polar, vec![1.0]).is_err());
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
        vec![original.control_points()[1], original.control_points()[0]]
    );
    assert_eq!(curve.weights(), Some(vec![2.0, -1.0]));
    curve.reverse_parameterization();
    assert_eq!(curve, original);

    let mut pcurve = pcurve();
    let original = pcurve.clone();
    pcurve.reverse_parameterization();
    assert_eq!(pcurve.knots(), &[-5.0, -5.0, -2.0, -2.0]);
    assert_eq!(
        pcurve.control_points(),
        vec![original.control_points()[1], original.control_points()[0]]
    );
    assert_eq!(pcurve.weights(), Some(vec![2.0, 1.0]));
    pcurve.reverse_parameterization();
    assert_eq!(pcurve, original);

    let mut polar = polar();
    let original = polar.clone();
    polar.reverse_parameterization();
    assert_eq!(polar.knots(), &[-5.0, -5.0, -2.0, -2.0]);
    assert_eq!(
        polar.poles(),
        vec![original.poles()[1], original.poles()[0]]
    );
    assert_eq!(polar.weights(), Some(vec![2.0, 1.0]));
    polar.reverse_parameterization();
    assert_eq!(polar, original);
}

#[test]
fn analytic_pcurve_admission_preserves_nonunit_axes_and_unordered_radii() {
    let origin = Point2::new(0.0, 0.0);
    let x = Point2::new(2.0, 0.0);
    let y = Point2::new(1.0, 3.0);
    assert!(CirclePcurve::try_new(origin, x, y, 1.0).is_ok());
    assert!(EllipsePcurve::try_new(origin, x, y, 1.0, 2.0).is_ok());
    assert!(EllipseCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        1.0,
        2.0,
    )
    .is_err());
    assert!(LinePcurve::try_new(origin, origin).is_err());
    assert!(HarmonicPcurve::try_new(origin, origin, x).is_ok());
    assert!(HarmonicPcurve::try_new(origin, origin, origin).is_err());
    assert!(SphericalGreatCirclePcurve::try_new(0.0, 1e-200, 0.0, 0.0).is_ok());
    assert!(SphericalGreatCirclePcurve::try_new(0.0, 0.0, 0.0, 0.0).is_err());
    let line = PcurveGeometry::Line(LinePcurve::try_new(origin, x).unwrap());
    assert!(TrimmedPcurve::try_new([2.0, 1.0], true, Box::new(line.clone())).is_err());
    assert!(TrimmedPcurve::try_new([1.0, 1.0], false, Box::new(line.clone())).is_ok());
    assert!(OffsetPcurve::try_new(f64::INFINITY, Box::new(line.clone())).is_err());
    let offset = PcurveGeometry::Offset(OffsetPcurve::try_new(-2.0, Box::new(line)).unwrap());
    let mut wire = serde_json::to_value(offset).unwrap();
    wire["basis"]["direction"] = serde_json::json!({"u": 0.0, "v": 0.0});
    assert!(serde_json::from_value::<PcurveGeometry>(wire).is_err());
}
