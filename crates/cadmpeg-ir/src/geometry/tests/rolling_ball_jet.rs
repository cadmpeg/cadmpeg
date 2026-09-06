// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{
    ProceduralSurface, ProceduralSurfaceDefinition, RollingBallJetDerivative, RollingBallJetSite,
    RollingBallJetStation,
};
use crate::math::{Point3, Vector3};
use serde_json::json;

fn station(knot: f64, multiplicity: u32) -> RollingBallJetStation {
    let derivative = RollingBallJetDerivative {
        first_limit: Vector3::new(0.0, 0.0, 0.0),
        second_limit: Vector3::new(0.0, 0.0, 0.0),
        center: Vector3::new(0.0, 0.0, 0.0),
        angle: 0.0,
    };
    RollingBallJetStation {
        knot,
        multiplicity,
        site: RollingBallJetSite {
            first_limit: Point3::new(1.0, 0.0, knot),
            second_limit: Point3::new(0.0, 1.0, knot),
            center: Point3::new(0.0, 0.0, knot),
            angle: std::f64::consts::FRAC_PI_2,
            first_derivative: derivative.clone(),
            second_derivative: derivative,
        },
    }
}

#[test]
fn rolling_ball_jet_station_rows_preserve_the_flat_wire() {
    let definition = ProceduralSurfaceDefinition::RollingBallJet {
        degree: 5,
        stations: vec![station(2.0, 6), station(4.0, 3), station(8.0, 6)],
    };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["knots"], json!([2.0, 4.0, 8.0]));
    assert_eq!(wire["multiplicities"], json!([6, 3, 6]));
    assert_eq!(
        wire["sites"][1]["center"],
        json!({"x": 0.0, "y": 0.0, "z": 4.0})
    );
    assert!(wire.get("stations").is_none());
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire.clone()).unwrap(),
        definition
    );
    for column in ["knots", "multiplicities", "sites"] {
        let mut malformed = wire.clone();
        malformed[column].as_array_mut().unwrap().pop();
        let error = serde_json::from_value::<ProceduralSurfaceDefinition>(malformed).unwrap_err();
        assert!(error.to_string().contains("must have equal lengths"));
    }
}

#[test]
fn rolling_ball_jet_extreme_degree_reports_invalid_payload_without_overflow() {
    let mut ir = crate::CadIr::empty();
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        "test:model:procedural_surface#jet".into(),
        ProceduralSurfaceDefinition::RollingBallJet {
            degree: u32::MAX,
            stations: vec![station(2.0, u32::MAX), station(8.0, u32::MAX)],
        },
        None,
    ));
    let report = crate::validate_neutral(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message == "rolling-ball jet payload is invalid"));
}
