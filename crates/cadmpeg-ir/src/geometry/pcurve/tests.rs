// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::{
    examples::unit_cube,
    geometry::pcurve::{LinePcurve, OffsetPcurve, PcurveGeometry, PcurveNurbs},
    math::Point2,
    test_support::nurbs::{pcurve, polar},
};

#[test]
fn a_refused_polar_pole_edit_keeps_the_prior_poles() {
    let mut polar = polar();
    let original = polar.clone();
    let refusal = polar.edit_poles(|radial, axial| {
        radial.u = 9.0;
        *axial = 9.0;
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
    assert_eq!(polar, original);
}

#[test]
fn a_refused_pcurve_pole_edit_keeps_the_prior_poles() {
    let mut pcurve = pcurve();
    let original = pcurve.clone();
    let refusal = pcurve.edit_control_points(|point| {
        point.u = 9.0;
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
    assert_eq!(pcurve, original);
}

#[test]
fn a_refused_nurbs_pcurve_scale_keeps_the_prior_poles() {
    let mut geometry = PcurveGeometry::Nurbs { nurbs: pcurve() };
    let original = geometry.clone();
    assert!(geometry.try_scale_coordinates([1e308, 1e308]).is_err());
    assert_eq!(geometry, original);
}

#[test]
fn nurbs_pcurve_scaling_scales_the_poles_and_keeps_the_knot_lane() {
    let mut geometry = PcurveGeometry::Nurbs { nurbs: pcurve() };
    assert!(geometry.try_scale_coordinates([2.0, 3.0]).is_ok());
    let expected = PcurveNurbs::from_lanes(
        pcurve().degree(),
        pcurve().knots().to_vec(),
        vec![Point2::new(2.0, 6.0), Point2::new(6.0, 12.0)],
        pcurve().weights(),
        pcurve().periodic(),
    )
    .unwrap();
    assert_eq!(geometry, PcurveGeometry::Nurbs { nurbs: expected });
}
mod metadata;

#[test]
fn pcurve_lift_rejects_non_finite_model_poles() {
    let curve = crate::geometry::pcurve::PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            crate::math::Point2::new(0.0, 0.0),
            crate::math::Point2::new(1.0, 1.0),
        ],
        None,
        false,
    )
    .unwrap();
    assert!(curve
        .lift(|point| crate::math::Point3::new(point.u, point.v, f64::NAN))
        .is_err());
}

#[test]
fn asm_inline_pcurve_metadata_lives_under_its_own_nested_key() {
    let pcurve = crate::geometry::pcurve::Pcurve {
        id: crate::ids::PcurveId::mint("test:model:pcurve#inline").expect("valid identity"),
        geometry: crate::geometry::pcurve::PcurveGeometry::Line(
            crate::geometry::pcurve::LinePcurve::try_new(
                crate::math::Point2::new(1.0, 2.0),
                crate::math::Point2::new(3.0, 4.0),
            )
            .unwrap(),
        ),
        metadata: crate::geometry::pcurve::PcurveMetadata::AsmInline {
            form: crate::geometry::pcurve::PcurveInlineForm::try_new(
                false,
                [true, false, true, false],
                [-1.0, 2.0],
                0.001,
            )
            .unwrap(),
        },
    };
    let value = serde_json::to_value(&pcurve).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "id": "test:model:pcurve#inline",
            "geometry": {
                "kind": "line",
                "origin": {"u": 1.0, "v": 2.0},
                "direction": {"u": 3.0, "v": 4.0}
            },
            "metadata": {
                "source": "asm_inline",
                "form": {
                    "wrapper_reversed": false,
                    "native_tail_flags": [true, false, true, false],
                    "parameter_range": [-1.0, 2.0],
                    "fit_tolerance": 0.001
                }
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<crate::geometry::pcurve::Pcurve>(value).unwrap(),
        pcurve
    );
}

#[test]
fn incomplete_asm_inline_pcurve_metadata_is_rejected() {
    let result = serde_json::from_value::<crate::geometry::pcurve::Pcurve>(serde_json::json!({
        "id": "test:model:pcurve#incomplete",
        "geometry": {
            "kind": "line",
            "origin": {"u": 1.0, "v": 2.0},
            "direction": {"u": 3.0, "v": 4.0}
        },
        "wrapper_reversed": false,
        "native_tail_flags": [true, false, true, false],
        "parameter_range": [-1.0, 2.0]
    }));
    assert!(result.is_err());
}

#[test]
fn pcurve_coordinate_scaling_keeps_the_original_when_a_nested_result_overflows() {
    let mut geometry = PcurveGeometry::Offset(
        OffsetPcurve::try_new(1e300, Box::new(PcurveGeometry::Line(LinePcurve::U_AXIS))).unwrap(),
    );
    let original = geometry.clone();
    assert!(geometry.try_scale_coordinates([1e300, 1e300]).is_err());
    assert_eq!(geometry, original);
}

#[test]
fn line_pcurve_direction_uses_the_shared_nonzero_vector_contract() {
    let origin = Point2::new(0.0, 0.0);
    let below = f64::EPSILON.sqrt() / 2.0;
    let above = f64::EPSILON.sqrt() * 2.0;
    for u in [0.0, 1e-300, below] {
        assert!(LinePcurve::try_new(origin, Point2::new(u, 0.0)).is_err());
        let wire =
            serde_json::json!({"origin": {"u": 0.0, "v": 0.0}, "direction": {"u": u, "v": 0.0}});
        assert!(serde_json::from_value::<LinePcurve>(wire).is_err());
    }
    let wire =
        serde_json::json!({"origin": {"u": 0.0, "v": 0.0}, "direction": {"u": above, "v": 0.0}});
    let line = LinePcurve::try_new(origin, Point2::new(above, 0.0)).unwrap();
    assert_eq!(serde_json::to_value(line).unwrap(), wire);
    assert_eq!(serde_json::from_value::<LinePcurve>(wire).unwrap(), line);
}

/// One-pcurve document built on the unit cube, used to drive the pcurve
/// carriers over the same route a checked-in document takes.
fn document_with_pcurve(geometry: &serde_json::Value) -> serde_json::Value {
    let mut document = serde_json::to_value(unit_cube().expect("valid unit cube fixture")).unwrap();
    document["model"]["pcurves"] = serde_json::json!([{
        "id": "synthetic:cube:pcurve#0",
        "geometry": geometry.clone(),
    }]);
    document
}

#[test]
fn every_pcurve_carrier_refuses_an_unknown_key_on_the_document_route() {
    let line = serde_json::json!({
        "kind": "line",
        "origin": {"u": 0.0, "v": 0.0},
        "direction": {"u": 1.0, "v": 0.0},
    });
    let cases: [(&str, serde_json::Value); 11] = [
        ("line", line.clone()),
        (
            "polar_harmonic",
            serde_json::json!({
                "kind": "polar_harmonic",
                "radial_center": {"u": 0.0, "v": 0.0},
                "radial_cos": {"u": 1.0, "v": 0.0},
                "radial_sin": {"u": 0.0, "v": 1.0},
                "axial_origin": 0.0,
                "axial_cos": 1.0,
                "axial_sin": 0.0,
            }),
        ),
        (
            "spherical_great_circle",
            serde_json::json!({
                "kind": "spherical_great_circle",
                "azimuth_origin": 0.0,
                "azimuth_rate": 1.0,
                "plane_phase": 0.0,
                "plane_slope": 1.0,
            }),
        ),
        (
            "circle",
            serde_json::json!({
                "kind": "circle",
                "center": {"u": 0.0, "v": 0.0},
                "x_axis": {"u": 1.0, "v": 0.0},
                "y_axis": {"u": 0.0, "v": 1.0},
                "radius": 1.0,
            }),
        ),
        (
            "ellipse",
            serde_json::json!({
                "kind": "ellipse",
                "center": {"u": 0.0, "v": 0.0},
                "x_axis": {"u": 1.0, "v": 0.0},
                "y_axis": {"u": 0.0, "v": 1.0},
                "major_radius": 2.0,
                "minor_radius": 1.0,
            }),
        ),
        (
            "harmonic",
            serde_json::json!({
                "kind": "harmonic",
                "center": {"u": 0.0, "v": 0.0},
                "cosine": {"u": 1.0, "v": 0.0},
                "sine": {"u": 0.0, "v": 1.0},
            }),
        ),
        (
            "parabola",
            serde_json::json!({
                "kind": "parabola",
                "vertex": {"u": 0.0, "v": 0.0},
                "x_axis": {"u": 1.0, "v": 0.0},
                "y_axis": {"u": 0.0, "v": 1.0},
                "focal_distance": 1.0,
            }),
        ),
        (
            "hyperbola",
            serde_json::json!({
                "kind": "hyperbola",
                "center": {"u": 0.0, "v": 0.0},
                "x_axis": {"u": 1.0, "v": 0.0},
                "y_axis": {"u": 0.0, "v": 1.0},
                "major_radius": 2.0,
                "minor_radius": 1.0,
            }),
        ),
        (
            "hyperbolic",
            serde_json::json!({
                "kind": "hyperbolic",
                "center": {"u": 0.0, "v": 0.0},
                "cosine": {"u": 1.0, "v": 0.0},
                "sine": {"u": 0.0, "v": 1.0},
            }),
        ),
        (
            "trimmed",
            serde_json::json!({
                "kind": "trimmed",
                "parameter_range": [0.0, 1.0],
                "same_sense": true,
                "basis": line.clone(),
            }),
        ),
        (
            "offset",
            serde_json::json!({
                "kind": "offset",
                "distance": 1.0,
                "basis": line,
            }),
        ),
    ];
    for (kind, geometry) in cases {
        let document = document_with_pcurve(&geometry);
        serde_json::from_value::<crate::CadIr>(document)
            .unwrap_or_else(|error| panic!("{kind} is a legal carrier: {error}"));

        let mut stray = geometry.as_object().unwrap().clone();
        stray.insert("zz_bogus".into(), serde_json::json!(1));
        let document = document_with_pcurve(&serde_json::Value::Object(stray));
        let error = serde_json::from_value::<crate::CadIr>(document)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{kind}: {error}");
    }
}

#[test]
fn parabola_coordinate_scaling_preserves_parameterization() {
    use crate::geometry::pcurve::ParabolaPcurve;
    let original = PcurveGeometry::Parabola(
        ParabolaPcurve::try_new(
            Point2::new(1.0, -2.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            2.0,
        )
        .unwrap(),
    );
    for scales in [[3.0, 3.0], [2.0, 5.0]] {
        let mut scaled = original.clone();
        scaled.try_scale_coordinates(scales).unwrap();
        for t in [-2.0, 0.0, 1.0, 3.0] {
            let before = crate::eval::pcurve_uv(&original, t).unwrap();
            let after = crate::eval::pcurve_uv(&scaled, t).unwrap();
            assert_eq!(
                after,
                Point2::new(before.u * scales[0], before.v * scales[1])
            );
        }
    }
}
