// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{
    ProceduralSurfaceDefinition, RollingBallJetDerivative, RollingBallJetSite,
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
    let definition = ProceduralSurfaceDefinition::RollingBallJet(
        crate::geometry::RollingBallJetStations::try_new(
            5,
            vec![station(2.0, 6), station(4.0, 3), station(8.0, 6)],
        )
        .unwrap(),
    );
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
fn rolling_ball_jet_admits_only_clamped_finite_station_payloads() {
    use crate::geometry::RollingBallJetStations;

    let valid = || vec![station(2.0, 6), station(8.0, 6)];
    for degree in [0, u32::MAX] {
        assert!(RollingBallJetStations::try_new(degree, valid()).is_err());
    }
    for stations in [
        Vec::new(),
        vec![station(2.0, 6)],
        vec![station(8.0, 6), station(2.0, 6)],
        vec![station(2.0, 6), station(2.0, 6)],
        vec![station(f64::NAN, 6), station(8.0, 6)],
        vec![station(2.0, 5), station(8.0, 6)],
        vec![station(2.0, 6), station(8.0, 5)],
        vec![station(2.0, 6), station(4.0, 0), station(8.0, 6)],
        vec![station(2.0, 6), station(4.0, 7), station(8.0, 6)],
    ] {
        assert!(RollingBallJetStations::try_new(5, stations).is_err());
    }
    let mut stations = valid();
    stations[0].site.first_derivative.angle = f64::INFINITY;
    assert!(RollingBallJetStations::try_new(5, stations).is_err());
    let mut stations = valid();
    stations[0].site.center.x = f64::NAN;
    assert!(RollingBallJetStations::try_new(5, stations).is_err());
    let mut stations = valid();
    stations[0].site.first_limit = stations[0].site.center;
    assert!(RollingBallJetStations::try_new(5, stations).is_err());
    let mut stations = valid();
    stations[0].site.first_limit.x = 2.0;
    assert!(RollingBallJetStations::try_new(5, stations).is_err());

    assert!(RollingBallJetStations::try_new(1, vec![station(2.0, 2), station(8.0, 2)]).is_ok());
    assert!(RollingBallJetStations::try_new(
        u32::MAX - 1,
        vec![station(2.0, u32::MAX), station(8.0, u32::MAX)],
    )
    .is_ok());
    let mut varying_radius = valid();
    varying_radius[1].site.first_limit.x = 2.0;
    varying_radius[1].site.second_limit.y = 2.0;
    assert!(RollingBallJetStations::try_new(5, varying_radius).is_ok());

    let definition = ProceduralSurfaceDefinition::RollingBallJet(
        RollingBallJetStations::try_new(5, valid()).unwrap(),
    );
    let wire = serde_json::to_value(definition).unwrap();
    for (field, value) in [
        ("degree", json!(0)),
        ("degree", json!(u32::MAX)),
        ("knots", json!([8.0, 2.0])),
        ("multiplicities", json!([5, 6])),
    ] {
        let mut malformed = wire.clone();
        malformed[field] = value;
        assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(malformed).is_err());
    }
    let mut malformed = wire;
    malformed["sites"][0]["first_limit"]["x"] = json!(2.0);
    assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(malformed).is_err());
}
