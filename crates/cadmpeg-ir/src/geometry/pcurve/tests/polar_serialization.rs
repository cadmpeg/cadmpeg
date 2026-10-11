// SPDX-License-Identifier: Apache-2.0
use crate::geometry::nurbs::StandardNurbsAdmission;
use crate::geometry::pcurve::{
    PcurveGeometry, PolarNurbsPole, PolarNurbsPoles, PolarPcurveNurbs, WeightedPolarNurbsPole,
};
use crate::math::Point2;
use crate::scalar::NonZeroReal;

const POLYNOMIAL_WIRE: &str = concat!(
    "{\"degree\":1,\"knots\":[-0.0,0.0,1.0,1.0],",
    "\"poles\":{\"form\":\"polynomial\",\"poles\":[",
    "{\"radial\":{\"u\":1.7976931348623157e+308,\"v\":5e-324},\"axial\":-1.7976931348623157e+308},",
    "{\"radial\":{\"u\":-0.0,\"v\":-2.2250738585072014e-308},\"axial\":2.2250738585072014e-308}]},",
    "\"periodic\":false}"
);
const RATIONAL_WIRE: &str = concat!(
    "{\"degree\":1,\"knots\":[-0.0,0.0,1.0,1.0],",
    "\"poles\":{\"form\":\"rational\",\"poles\":[",
    "{\"radial\":{\"u\":1.7976931348623157e+308,\"v\":5e-324},\"axial\":-1.7976931348623157e+308,\"weight\":-1.0},",
    "{\"radial\":{\"u\":-0.0,\"v\":-2.2250738585072014e-308},\"axial\":2.2250738585072014e-308,\"weight\":-2.0}]},",
    "\"periodic\":false}"
);

fn fixture(rational: bool, periodic: bool) -> PolarPcurveNurbs {
    let poles = [
        PolarNurbsPole {
            radial: Point2::new(f64::MAX, f64::from_bits(1)),
            axial: -f64::MAX,
        },
        PolarNurbsPole {
            radial: Point2::new(-0.0, -f64::MIN_POSITIVE),
            axial: f64::MIN_POSITIVE,
        },
    ];
    let poles = if rational {
        PolarNurbsPoles::Rational {
            poles: poles
                .into_iter()
                .zip([-1.0, -2.0])
                .map(|(pole, weight)| WeightedPolarNurbsPole {
                    radial: pole.radial,
                    axial: pole.axial,
                    weight: NonZeroReal::new(weight).expect("nonzero weight"),
                })
                .collect(),
        }
    } else {
        PolarNurbsPoles::Polynomial {
            poles: poles.to_vec(),
        }
    };
    crate::geometry::pcurve::construction::build_polar(
        &StandardNurbsAdmission,
        1,
        vec![-0.0, 0.0, 1.0, 1.0],
        poles,
        periodic,
    )
    .expect("actual Standard construction")
}

#[test]
fn polar_nurbs_borrowed_serialization_preserves_exact_wire_for_both_pole_forms() {
    for rational in [false, true] {
        for periodic in [false, true] {
            let curve = fixture(rational, periodic);
            let expected = if rational {
                RATIONAL_WIRE
            } else {
                POLYNOMIAL_WIRE
            }
            .replace("\"periodic\":false", &format!("\"periodic\":{periodic}"));
            assert_eq!(
                serde_json::to_string(&curve).expect("direct payload"),
                expected
            );
            let flattened = format!("{{\"kind\":\"polar_nurbs\",{}", &expected[1..]);
            assert_eq!(
                serde_json::to_string(&PcurveGeometry::PolarNurbs { nurbs: curve })
                    .expect("flattened geometry"),
                flattened
            );
        }
    }
}

#[test]
fn polar_nurbs_borrowed_serialization_roundtrips_all_stored_bits() {
    for rational in [false, true] {
        for periodic in [false, true] {
            let original = fixture(rational, periodic);
            let wire = serde_json::to_string(&original).expect("direct wire");
            let restored: PolarPcurveNurbs =
                serde_json::from_str(&wire).expect("context-free reader");
            assert_eq!(restored.degree(), 1);
            assert_eq!(restored.periodic(), periodic);
            assert_eq!(
                restored
                    .knots()
                    .iter()
                    .copied()
                    .map(f64::to_bits)
                    .collect::<Vec<_>>(),
                [-0.0_f64, 0.0, 1.0, 1.0].map(f64::to_bits)
            );
            let points = restored.poles();
            assert_eq!(points.len(), 2);
            assert_eq!(
                points[0]
                    .radial
                    .coordinates()
                    .map(|value| value.get().to_bits()),
                [f64::MAX, f64::from_bits(1)].map(f64::to_bits)
            );
            assert_eq!(points[0].axial.get().to_bits(), (-f64::MAX).to_bits());
            assert_eq!(
                points[1]
                    .radial
                    .coordinates()
                    .map(|value| value.get().to_bits()),
                [-0.0, -f64::MIN_POSITIVE].map(f64::to_bits)
            );
            assert_eq!(points[1].axial.get().to_bits(), f64::MIN_POSITIVE.to_bits());
            assert_eq!(
                restored.weights().map(|weights| weights
                    .into_iter()
                    .map(|weight| weight.get().to_bits())
                    .collect::<Vec<_>>()),
                rational.then(|| [-1.0_f64, -2.0].map(f64::to_bits).to_vec())
            );
            assert_eq!(
                serde_json::to_string(&restored).expect("restored wire"),
                wire
            );
        }
    }
}
