// SPDX-License-Identifier: Apache-2.0
use crate::features::FinitePoint3;
use crate::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
use crate::math::Point3;
use crate::scalar::FiniteReal;

fn raw(parameters: Option<[f64; 3]>) -> PolylineSamples {
    let points = [
        Point3::new(0., 0., 0.),
        Point3::new(1., 0., 0.),
        Point3::new(1., 1., 0.),
    ];
    match parameters {
        None => PolylineSamples::Unparameterized {
            points: points.to_vec().try_into().expect("three points"),
        },
        Some(parameters) => PolylineSamples::Parameterized {
            vertices: points
                .into_iter()
                .zip(parameters)
                .map(|(point, parameter)| PolylineVertex { parameter, point })
                .collect::<Vec<_>>()
                .try_into()
                .expect("three vertices"),
        },
    }
}

fn checked(parameters: Option<[f64; 3]>) -> PolylineSamples<FiniteReal, FinitePoint3> {
    match raw(parameters) {
        PolylineSamples::Unparameterized { points } => PolylineSamples::Unparameterized {
            points: points.map(|point| FinitePoint3::new(point).expect("finite")),
        },
        PolylineSamples::Parameterized { vertices } => PolylineSamples::Parameterized {
            vertices: vertices.map(|vertex| PolylineVertex {
                parameter: FiniteReal::new(vertex.parameter).expect("finite"),
                point: FinitePoint3::new(vertex.point).expect("finite"),
            }),
        },
    }
}

#[test]
fn polyline_construction_preserves_diagnostic_order() {
    let mut samples = raw(Some([f64::INFINITY, 0., 0.]));
    let PolylineSamples::Parameterized { vertices } = &mut samples else {
        panic!("parameterized fixture");
    };
    vertices[0].point.x = f64::INFINITY;
    assert_eq!(
        PolylineCurve::new(samples, -1.)
            .expect_err("points precede deflection")
            .to_string(),
        "points must be finite"
    );
    assert_eq!(
        PolylineCurve::new(raw(Some([f64::INFINITY, 0., 0.])), -1.)
            .expect_err("deflection precedes parameters")
            .to_string(),
        "chordal_deflection must be finite and non-negative"
    );
    assert_eq!(
        PolylineCurve::new(raw(Some([f64::INFINITY, 0., 0.])), 0.)
            .expect_err("nonfinite parameter")
            .to_string(),
        "parameters must be finite and strictly monotonic"
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        PolylineCurve::from_checked_samples(checked(Some([0., 0., 0.])), -1.)
            .expect_err("negative deflection")
            .to_string(),
        "chordal_deflection must be finite and non-negative"
    );
    ctx.finish_session().unwrap();
}

#[test]
fn polyline_construction_accepts_both_lanes_and_either_parameter_direction() {
    for parameters in [None, Some([0., 1., 2.]), Some([2., 1., 0.])] {
        let curve = PolylineCurve::new(raw(parameters), 0.25).expect("valid");
        assert_eq!(
            serde_json::from_str::<PolylineCurve>(&serde_json::to_string(&curve).expect("wire"))
                .expect("context-free serde"),
            curve
        );
    }
}

#[test]
fn checked_polyline_construction_moves_sample_rows() {
    for parameters in [None, Some([0., 1., 2.]), Some([2., 1., 0.])] {
        let curve = match checked(parameters) {
            PolylineSamples::Unparameterized { points } => {
                let address = points.as_ptr();
                let curve = PolylineCurve::from_checked_samples(
                    PolylineSamples::Unparameterized { points },
                    0.25,
                )
                .expect("valid");
                let PolylineSamples::Unparameterized { points } = &curve.samples else {
                    panic!("same sample lane");
                };
                assert_eq!(points.as_ptr(), address);
                curve
            }
            PolylineSamples::Parameterized { vertices } => {
                let address = vertices.as_ptr();
                let curve = PolylineCurve::from_checked_samples(
                    PolylineSamples::Parameterized { vertices },
                    0.25,
                )
                .expect("valid");
                let PolylineSamples::Parameterized { vertices } = &curve.samples else {
                    panic!("same sample lane");
                };
                assert_eq!(vertices.as_ptr(), address);
                curve
            }
        };
        assert_eq!(curve.chordal_deflection().get(), 0.25);
    }
}
