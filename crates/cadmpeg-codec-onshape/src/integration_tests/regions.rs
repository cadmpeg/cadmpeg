// SPDX-License-Identifier: Apache-2.0
//! Sketch-region boundaries resolved by exact conic intersection and stored tangency.
#![allow(clippy::indexing_slicing, clippy::panic)]
use cadmpeg_core::json;

use super::projection::replace_query;
use crate::reader::query;
use crate::test_support::projection::{changed, selected};
use crate::test_support::review;

#[test]
fn exact_conic_intersections_map_closed_lenses_and_refuse_missing_intervals() {
    for elliptical in [false, true] {
        for missing in [false, true] {
            let bytes = changed(|response| {
                response["features"][0]["constraints"] = json!([]);
                let angle = if elliptical {
                    (1.0_f64 / 3.0).acos()
                } else {
                    std::f64::consts::PI / 3.0
                };
                let geometry = |x| {
                    if elliptical {
                        json!({"btType":"BTCurveGeometryEllipse-1189","xCenter":x,"yCenter":0.0,"radius":0.003,"minorRadius":0.002,"xDir":1.0,"yDir":0.0,"clockwise":false})
                    } else {
                        json!({"btType":"BTCurveGeometryCircle-115","xCenter":x,"yCenter":0.0,"radius":0.002,"xDir":1.0,"yDir":0.0,"clockwise":false})
                    }
                };
                response["features"][0]["entities"] = json!([
                    {"btType":"BTMSketchCurveSegment-155","entityId":"left","startParam":-angle,"endParam":angle,"geometry":geometry(-0.001)},
                    {"btType":"BTMSketchCurveSegment-155","entityId":"right","startParam":std::f64::consts::PI-angle,"endParam":std::f64::consts::PI+angle,"geometry":geometry(0.001)}]);
                if missing {
                    response["features"][0]["entities"][1]
                        .as_object_mut()
                        .unwrap()
                        .remove("endParam");
                }
                let mut query = query::resolved(selected(response)).unwrap();
                let edge = query["disambiguationData"][0]["entities"][0][0].clone();
                let edges: Vec<_> = ["left", "right"]
                    .into_iter()
                    .map(|name| {
                        let mut edge = edge.clone();
                        edge["derivedFrom"]["sketchEntityId"] = json!(name);
                        json!([edge, 1])
                    })
                    .collect();
                query["disambiguationData"][0]["entities"] = json!(edges);
                replace_query(selected(response), &query);
            });
            let result = review::decode(&bytes);
            let wire = review::wire(&result);
            assert_eq!(
                wire["model"]["features"][1]["definition"]["value"]["kind"],
                if missing { "native" } else { "extrude" },
                "ellipse={elliptical}, missing={missing}: {:?}",
                result.report().losses
            );
            if missing {
                let reason = if elliptical {
                    "the sketch-region source curve has no resolved boundary parameter range"
                } else {
                    "the directed sketch-region boundary is open or ambiguous"
                };
                assert!(
                    result
                        .report()
                        .losses
                        .iter()
                        .any(|l| l.code.to_string() == "onshape/history.not-supported"
                            && l.message.contains(reason)),
                    "{:?}",
                    result.report().losses
                );
            }
            review::validate(result);
        }
    }
}

#[test]
fn stored_tangency_resolves_adjacent_roundoff_contacts_and_missing_or_conflicting_data_refuses() {
    for datum in ["present", "absent", "contradictory"] {
        let bytes = changed(|response| {
            let top = if datum == "contradictory" {
                0.0011
            } else {
                0.001 + 8e-18
            };
            response["features"][0]["entities"] = json!([
                {"btType":"BTMSketchCurveSegment-155","entityId":"right","startParam":-std::f64::consts::FRAC_PI_2,"endParam":std::f64::consts::FRAC_PI_2,"geometry":{"btType":"BTCurveGeometryCircle-115","xCenter":0.001,"yCenter":0.0,"radius":0.001,"xDir":1.0,"yDir":0.0}},
                {"btType":"BTMSketchCurveSegment-155","entityId":"top","startParam":0.0,"endParam":0.002,"geometry":{"btType":"BTCurveGeometryLine-117","pntX":0.001,"pntY":top,"dirX":-1.0,"dirY":0.0}},
                {"btType":"BTMSketchCurveSegment-155","entityId":"left","startParam":std::f64::consts::FRAC_PI_2,"endParam":3.0*std::f64::consts::FRAC_PI_2,"geometry":{"btType":"BTCurveGeometryCircle-115","xCenter":-0.001,"yCenter":0.0,"radius":0.001,"xDir":1.0,"yDir":0.0}},
                {"btType":"BTMSketchCurveSegment-155","entityId":"bottom","startParam":0.0,"endParam":0.002,"geometry":{"btType":"BTCurveGeometryLine-117","pntX":-0.001,"pntY":-0.001,"dirX":1.0,"dirY":0.0}}
            ]);
            response["features"][0]["constraints"] = if datum == "absent" {
                json!([])
            } else {
                json!([("right","top"),("right","bottom"),("left","top"),("left","bottom")].into_iter().enumerate().map(|(i,(a,b))| json!({"entityId":format!("tangent-{i}"),"constraintType":"TANGENT","parameters":[{"parameterId":"localFirst","value":a},{"parameterId":"localSecond","value":b}]})).collect::<Vec<_>>())
            };
            let mut query = query::resolved(selected(response)).unwrap();
            let template = query["disambiguationData"][0]["entities"][0][0].clone();
            query["disambiguationData"][0]["entities"] = json!(["right", "top", "left", "bottom"]
                .into_iter()
                .map(|name| {
                    let mut edge = template.clone();
                    edge["derivedFrom"]["sketchEntityId"] = json!(name);
                    json!([edge, 1])
                })
                .collect::<Vec<_>>());
            replace_query(selected(response), &query);
        });
        let result = review::decode(&bytes);
        assert_eq!(
            review::wire(&result)["model"]["features"][1]["definition"]["value"]["kind"],
            if datum == "present" {
                "extrude"
            } else {
                "native"
            },
            "{datum}: {:?}",
            result.report().losses
        );
        if datum != "present" {
            assert!(result
                .report()
                .losses
                .iter()
                .any(|l| l.code.to_string() == "onshape/history.not-supported"));
        }
        review::validate(result);
    }
}
