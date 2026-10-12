// SPDX-License-Identifier: Apache-2.0
//! Admission before procedural edge and station visits.

use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, WorkBudget,
};
use cadmpeg_core::CodecError;

use crate::eval::admission::EvaluationAdmission;
use crate::eval::{
    construction_curve_parameter, rolling_ball_jet_point_admitted, EvaluationFailure,
};
use crate::geometry::{
    Curve, CurveGeometry, ProceduralSurfaceDefinition, RollingBallJetDerivative,
    RollingBallJetSite, RollingBallJetStation, RollingBallJetStations, SolvedCurveGeometry,
};
use crate::index::{ModelIndex, StandardIndex};
use crate::math::{Point3, Vector3};
use crate::scalar::FiniteReal;
use crate::topology::{Edge, EdgeCarrier};
use crate::CadIr;

fn two_station_jet() -> ProceduralSurfaceDefinition {
    let zero = RollingBallJetDerivative {
        first_limit: Vector3::new(0.0, 0.0, 0.0),
        second_limit: Vector3::new(0.0, 0.0, 0.0),
        center: Vector3::new(0.0, 0.0, 0.0),
        angle: 0.0,
    };
    let stations = [2.0, 5.0]
        .into_iter()
        .map(|knot| RollingBallJetStation {
            knot,
            multiplicity: 6,
            site: RollingBallJetSite {
                first_limit: Point3::new(2.0, 0.0, 0.0),
                second_limit: Point3::new(0.0, 2.0, 0.0),
                center: Point3::new(0.0, 0.0, 0.0),
                angle: std::f64::consts::FRAC_PI_2,
                first_derivative: zero,
                second_derivative: zero,
            },
        })
        .collect();
    ProceduralSurfaceDefinition::RollingBallJet(
        RollingBallJetStations::try_new(
            5,
            stations,
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap()
        .unwrap(),
    )
}

#[test]
fn rolling_ball_visits_charge_the_selected_session_once() {
    let definition = two_station_jet();
    let expected = crate::eval::rolling_ball_jet_point(&definition, 3.5, 0.5).unwrap();
    // Two radius rows and the first matching knot span; no interior knot.
    for cap in 0..=3 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = rolling_ball_jet_point_admitted((&ctx).into(), &definition, 3.5, 0.5);
        if cap == 3 {
            assert_eq!(result.unwrap(), expected);
            ctx.finish_session().unwrap();
        } else {
            let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
                panic!("visited station must preserve its resource refusal");
            };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
            assert_eq!(
                first.operation,
                if cap < 2 {
                    "rolling-ball jet radius scan"
                } else {
                    "rolling-ball jet span scan"
                }
            );
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
            );
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let parent = ctx.work_budget(3);
    let result = EvaluationAdmission::from(&ctx).within_work_slice(&parent, |admission| {
        rolling_ball_jet_point_admitted(admission, &definition, 3.5, 0.5)
    });
    assert_eq!(result.unwrap(), expected);
    assert_eq!(parent.consumed(), 3);
    ctx.finish_session().unwrap();
    for cap in 0..=3 {
        let parent = WorkBudget::new(cap);
        let result = EvaluationAdmission::Standard.within_work_slice(&parent, |admission| {
            rolling_ball_jet_point_admitted(admission, &definition, 3.5, 0.5)
        });
        if cap == 3 {
            assert_eq!(result.unwrap(), expected);
            assert_eq!(parent.consumed(), 3);
        } else {
            assert_eq!(result, Err(EvaluationFailure::NoValue));
            assert!(parent.exhausted());
        }
    }
}

#[test]
fn construction_directrix_scan_stops_at_the_first_disagreeing_range() {
    let directrix: crate::ids::CurveId = "test:model:curve#directrix".try_into().unwrap();
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: directrix.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    for (index, range) in [[0.0, 1.0], [0.0, 2.0], [0.0, 3.0]].into_iter().enumerate() {
        ir.model.edges.push(Edge {
            id: format!("test:model:edge#{index}").try_into().unwrap(),
            carrier: EdgeCarrier::new(Some(directrix.clone()), Some(range)).unwrap(),
            start: "test:model:vertex#start".try_into().unwrap(),
            end: "test:model:vertex#end".try_into().unwrap(),
            tolerance: None,
        });
    }
    // Unprimed Standard index: contextual lookup visits one record, then
    // admits the current TextWork equality gate and the identical ID bytes.
    // Each edge similarly admits one visit, equality gate and ID bytes.
    let lookup_and_visit = cadmpeg_core::decode::u64_from_index(directrix.as_str().len()) + 2;
    let exact_prefix = 3 * lookup_and_visit;
    for cap in [exact_prefix - 1, exact_prefix] {
        let index = ModelIndex::build(&ir, StandardIndex);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = construction_curve_parameter(
            (&ctx).into(),
            &index,
            &directrix,
            0.5,
            FiniteReal::array([0.0, 1.0]),
            None,
            false,
        );
        if cap == exact_prefix {
            assert_eq!(result, Err(EvaluationFailure::NoValue));
            ctx.finish_session().unwrap();
        } else {
            let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
                panic!("second matching edge must admit the last compared byte");
            };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
            assert_eq!(
                first.operation,
                "construction directrix identity comparison"
            );
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
            );
        }
    }
}

fn three_station_jet(interior_multiplicity: u32, last_radius: f64) -> ProceduralSurfaceDefinition {
    let ProceduralSurfaceDefinition::RollingBallJet(jet) = two_station_jet() else {
        panic!("jet fixture");
    };
    let mut stations = jet.stations().to_vec();
    let mut middle = stations[0];
    middle.knot = FiniteReal::new(3.5).unwrap();
    middle.multiplicity = interior_multiplicity;
    stations.insert(1, middle);
    stations[2].site.first_limit =
        crate::features::FinitePoint3::new(Point3::new(last_radius, 0.0, 0.0)).unwrap();
    stations[2].site.second_limit =
        crate::features::FinitePoint3::new(Point3::new(0.0, last_radius, 0.0)).unwrap();
    ProceduralSurfaceDefinition::RollingBallJet(
        RollingBallJetStations::from_parts(
            5,
            stations,
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap()
        .unwrap(),
    )
}

#[test]
fn rolling_ball_interior_multiplicity_refuses_before_later_station_work() {
    let definition = three_station_jet(2, 2.0);
    for cap in [0, 1] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = rolling_ball_jet_point_admitted((&ctx).into(), &definition, 3.0, 0.5);
        if cap == 1 {
            assert_eq!(result, Err(EvaluationFailure::NoValue));
            ctx.finish_session().unwrap();
        } else {
            let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
                panic!("interior visit needs admission");
            };
            assert_eq!(first.operation, "rolling-ball jet multiplicity scan");
            assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
            );
        }
    }
}

#[test]
fn rolling_ball_radius_mismatch_stops_before_span_work() {
    let definition = three_station_jet(3, 3.0);
    // One interior multiplicity, then three radius visits. The last
    // radius differs between stations but each station has valid radii.
    for cap in [3, 4] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = rolling_ball_jet_point_admitted((&ctx).into(), &definition, 3.0, 0.5);
        if cap == 4 {
            assert_eq!(result, Err(EvaluationFailure::NoValue));
            ctx.finish_session().unwrap();
        } else {
            let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
                panic!("last radius visit needs admission");
            };
            assert_eq!(first.operation, "rolling-ball jet radius scan");
            assert_eq!((first.limit, first.used, first.additional), (3, 3, 1));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
            );
        }
    }
}

#[test]
fn rolling_ball_span_search_charges_only_the_matching_prefix() {
    let definition = three_station_jet(3, 2.0);
    // One interior multiplicity and three radius visits precede the
    // actual span search. The shared knot belongs to its first span.
    for (parameter, expected_visits) in [(3.0, 1), (3.5, 1), (4.0, 2)] {
        let expected = crate::eval::rolling_ball_jet_point(&definition, parameter, 0.0).unwrap();
        let exact = 4 + expected_visits;
        for cap in [exact - 1, exact] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result =
                rolling_ball_jet_point_admitted((&ctx).into(), &definition, parameter, 0.0);
            if cap == exact {
                assert_eq!(result.unwrap(), expected);
                ctx.finish_session().unwrap();
            } else {
                let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
                    panic!("last visited span needs admission");
                };
                assert_eq!(first.operation, "rolling-ball jet span scan");
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
                );
            }
        }
    }
}
