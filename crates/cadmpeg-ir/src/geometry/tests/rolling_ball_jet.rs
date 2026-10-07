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
            first_derivative: derivative,
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
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("fixture rolling-ball admission")
        .unwrap(),
    );
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["stations"][0]["knot"], json!(2.0));
    assert_eq!(wire["stations"][1]["multiplicity"], json!(3));
    assert_eq!(
        wire["stations"][1]["site"]["center"],
        json!({"x": 0.0, "y": 0.0, "z": 4.0})
    );
    // The three columns travel as one row each, so a knot without its
    // multiplicity or its site has no spelling.
    assert!(wire.get("knots").is_none());
    assert!(wire.get("multiplicities").is_none());
    assert!(wire.get("sites").is_none());
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire.clone()).unwrap(),
        definition
    );
    for column in ["knot", "multiplicity", "site"] {
        let mut malformed = wire.clone();
        malformed["stations"][1]
            .as_object_mut()
            .unwrap()
            .remove(column);
        let error = serde_json::from_value::<ProceduralSurfaceDefinition>(malformed).unwrap_err();
        assert!(error.to_string().contains(column), "{error}");
    }
}

#[test]
fn rolling_ball_jet_admits_only_clamped_finite_station_payloads() {
    use crate::geometry::RollingBallJetStations;

    let valid = || vec![station(2.0, 6), station(8.0, 6)];
    for degree in [0, u32::MAX] {
        assert!(RollingBallJetStations::try_new(
            degree,
            valid(),
            &cadmpeg_test_support::service_decode_context()
        )
        .expect("fixture rolling-ball admission")
        .is_err());
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
        assert!(RollingBallJetStations::try_new(
            5,
            stations,
            &cadmpeg_test_support::service_decode_context()
        )
        .expect("fixture rolling-ball admission")
        .is_err());
    }
    let mut stations = valid();
    stations[0].site.first_derivative.angle = f64::INFINITY;
    assert!(RollingBallJetStations::try_new(
        5,
        stations,
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("fixture rolling-ball admission")
    .is_err());
    let mut stations = valid();
    stations[0].site.center.x = f64::NAN;
    assert!(RollingBallJetStations::try_new(
        5,
        stations,
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("fixture rolling-ball admission")
    .is_err());
    let mut stations = valid();
    stations[0].site.first_limit = stations[0].site.center;
    assert!(RollingBallJetStations::try_new(
        5,
        stations,
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("fixture rolling-ball admission")
    .is_err());
    let mut stations = valid();
    stations[0].site.first_limit.x = 2.0;
    assert!(RollingBallJetStations::try_new(
        5,
        stations,
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("fixture rolling-ball admission")
    .is_err());

    assert!(RollingBallJetStations::try_new(
        1,
        vec![station(2.0, 2), station(8.0, 2)],
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("fixture rolling-ball admission")
    .is_ok());
    assert!(RollingBallJetStations::try_new(
        u32::MAX - 1,
        vec![station(2.0, u32::MAX), station(8.0, u32::MAX)],
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("fixture rolling-ball admission")
    .is_ok());
    let mut varying_radius = valid();
    varying_radius[1].site.first_limit.x = 2.0;
    varying_radius[1].site.second_limit.y = 2.0;
    assert!(RollingBallJetStations::try_new(
        5,
        varying_radius,
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("fixture rolling-ball admission")
    .is_ok());

    let definition = ProceduralSurfaceDefinition::RollingBallJet(
        RollingBallJetStations::try_new(
            5,
            valid(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("fixture rolling-ball admission")
        .unwrap(),
    );
    let wire = serde_json::to_value(definition).unwrap();
    for (field, value) in [("degree", json!(0)), ("degree", json!(u32::MAX))] {
        let mut malformed = wire.clone();
        malformed[field] = value;
        assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(malformed).is_err());
    }
    for (path, value) in [
        (["stations", "0", "knot"], json!(8.0)),
        (["stations", "1", "knot"], json!(2.0)),
        (["stations", "0", "multiplicity"], json!(5)),
    ] {
        let mut malformed = wire.clone();
        malformed[path[0]][path[1].parse::<usize>().unwrap()][path[2]] = value;
        assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(malformed).is_err());
    }
    let mut malformed = wire;
    malformed["stations"][0]["site"]["first_limit"]["x"] = json!(2.0);
    assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(malformed).is_err());
}

#[test]
fn admitted_rolling_ball_stations_keep_the_refusals_of_raw_stations() {
    use crate::geometry::RollingBallJetStations;
    use crate::scalar::FiniteReal;

    let admit = |stations: Vec<RollingBallJetStation>| {
        stations
            .into_iter()
            .map(|station| RollingBallJetStation {
                knot: FiniteReal::new(station.knot).unwrap(),
                multiplicity: station.multiplicity,
                site: station.site.admit().unwrap(),
            })
            .collect::<Vec<_>>()
    };
    let mut collapsed_radius = vec![station(2.0, 6), station(8.0, 6)];
    collapsed_radius[0].site.first_limit = collapsed_radius[0].site.center;
    let mut unequal_radii = vec![station(2.0, 6), station(8.0, 6)];
    unequal_radii[0].site.first_limit.x = 2.0;
    for (degree, stations) in [
        (5, vec![station(2.0, 6), station(4.0, 3), station(8.0, 6)]),
        (0, vec![station(2.0, 6), station(8.0, 6)]),
        (5, vec![station(2.0, 6)]),
        (5, vec![station(8.0, 6), station(2.0, 6)]),
        (5, vec![station(2.0, 6), station(2.0, 6)]),
        (5, vec![station(2.0, 5), station(8.0, 6)]),
        (5, vec![station(2.0, 6), station(4.0, 7), station(8.0, 6)]),
        (5, collapsed_radius),
        (5, unequal_radii),
    ] {
        assert_eq!(
            RollingBallJetStations::from_parts(
                degree,
                admit(stations.clone()),
                &cadmpeg_test_support::service_decode_context()
            )
            .expect("fixture rolling-ball admission"),
            RollingBallJetStations::try_new(
                degree,
                stations,
                &cadmpeg_test_support::service_decode_context()
            )
            .expect("fixture rolling-ball admission")
        );
    }
}

#[test]
fn rolling_ball_jet_decode_refuses_work_and_retained_rows() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::RetainedBytes,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if dimension == ResourceDimension::WorkUnits {
            policy.limits.max_work_units = 0;
        } else {
            policy.limits.max_retained_bytes = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = crate::geometry::RollingBallJetStations::try_new(
            5,
            vec![station(2.0, 6), station(8.0, 6)],
            &ctx,
        );
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == dimension)
        );
    }
}

#[test]
fn rolling_ball_constructors_preserve_named_caller_work_refusals() {
    use cadmpeg_core::decode::ResourceDimension;
    for typed in [false, true] {
        for operation in [
            "rolling-ball jet multiplicities",
            "rolling-ball jet knots",
            "rolling-ball jet station controls",
        ] {
            cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                operation,
                |cap| {
                    super::budget::with_limit(ResourceDimension::WorkUnits, cap, |ctx| {
                        let raw = vec![station(2.0, 6), station(8.0, 6)];
                        if typed {
                            let rows = raw
                                .into_iter()
                                .map(|row| RollingBallJetStation {
                                    knot: crate::scalar::FiniteReal::new(row.knot).unwrap(),
                                    multiplicity: row.multiplicity,
                                    site: row.site.admit().unwrap(),
                                })
                                .collect();
                            crate::geometry::RollingBallJetStations::from_parts(5, rows, ctx)
                        } else {
                            crate::geometry::RollingBallJetStations::try_new(5, raw, ctx)
                        }
                    })
                },
            );
        }
    }
}

#[test]
fn rolling_ball_typed_constructor_moves_owned_rows_without_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let rows = [station(2.0, 6), station(8.0, 6)]
        .into_iter()
        .map(|row| RollingBallJetStation {
            knot: crate::scalar::FiniteReal::new(row.knot).unwrap(),
            multiplicity: row.multiplicity,
            site: row.site.admit().unwrap(),
        })
        .collect::<Vec<_>>();
    let pointer = rows.as_ptr();
    let expected = rows.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let jet = crate::geometry::RollingBallJetStations::from_parts(5, rows, &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(jet.stations().as_ptr(), pointer);
    assert_eq!(jet.stations(), expected);
    ctx.finish_session().unwrap();
    let wire = serde_json::to_value(&jet).unwrap();
    assert_eq!(
        serde_json::from_value::<crate::geometry::RollingBallJetStations>(wire).unwrap(),
        jet
    );
}
