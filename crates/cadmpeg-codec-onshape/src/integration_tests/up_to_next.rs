// SPDX-License-Identifier: Apache-2.0
//! `UP_TO_NEXT` extent proof over the covering face.
#![allow(clippy::indexing_slicing)]

use super::bundle::{changed, kinds, read, timeline};
use crate::test_support::*;
use cadmpeg_core::json;
use serde_json::Value;

/// A synthetic box body in metres: twelve straight edges and six planar faces whose
/// `orientation` says whether the outward side follows the stored surface normal.
fn box_body(low: [f64; 3], high: [f64; 3], tag: &str) -> (Vec<Value>, Vec<Value>) {
    let corner = |i: usize| -> [f64; 3] {
        std::array::from_fn(|axis| {
            if i >> axis & 1 == 1 {
                high[axis]
            } else {
                low[axis]
            }
        })
    };
    let point = |p: [f64; 3]| json!({"x": p[0], "y": p[1], "z": p[2]});
    let mut edges = Vec::new();
    let mut ends = Vec::new();
    for a in 0..8usize {
        for axis in 0..3 {
            if a >> axis & 1 == 0 {
                let b = a | 1 << axis;
                let (p, q) = (corner(a), corner(b));
                let mut direction = [0.0; 3];
                direction[axis] = 1.0;
                edges.push(json!({"id": format!("{tag}e{a}{b}"), "curve": {"type": "LINE", "origin": point(p), "direction": point(direction)},
                    "geometry": {"startPoint": point(p), "endPoint": point(q), "midPoint": point(std::array::from_fn(|j| f64::midpoint(p[j], q[j])))}}));
                ends.push((a, b));
            }
        }
    }
    let mut faces = Vec::new();
    for axis in 0..3 {
        for (side, name) in [(0, "low"), (1, "high")] {
            let mut normal = [0.0; 3];
            normal[axis] = 1.0;
            let (min, max) = (
                corner(side << axis),
                corner(7 & !(usize::from(side == 0) << axis)),
            );
            let coedges: Vec<_> = ends
                .iter()
                .filter(|(a, b)| a >> axis & 1 == side && b >> axis & 1 == side)
                .map(|(a, b)| json!({"edgeId": format!("{tag}e{a}{b}"), "orientation": true}))
                .collect();
            faces.push(
                json!({"id": format!("{tag}{name}{axis}"), "orientation": side == 1,
                "surface": {"type": "PLANE", "origin": point(min), "normal": point(normal)},
                "box": {"minCorner": point(min), "maxCorner": point(max)},
                "loops": [{"isOuter": true, "coedges": coedges}]}),
            );
        }
    }
    (edges, faces)
}

pub(super) fn up_to_next(operation: &str, extra: Option<([f64; 3], [f64; 3])>) -> Vec<u8> {
    changed(&fixture("plate-fillet"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][2] = json!({"btType": "BTMFeature-134", "featureType": "extrude", "featureId": "toNext", "name": "To next", "parameters": [
                {"parameterId": "bodyType", "value": "SOLID"}, {"parameterId": "operationType", "value": operation},
                {"parameterId": "entities", "queries": [{"btType": "BTMIndividualSketchRegionQuery-140", "featureId": "Base sketch"}]},
                {"parameterId": "endBound", "value": "UP_TO_NEXT"}
            ]});
        }
        if path.ends_with("bodydetails/2.json") {
            let (mut edges, mut faces) = box_body([0.0; 3], [0.02, 0.01, 0.002], "");
            if let Some((low, high)) = extra {
                let (more_edges, more_faces) = box_body(low, high, "boss");
                edges.extend(more_edges);
                faces.extend(more_faces);
            }
            value["bodies"][0]["edges"] = json!(edges);
            value["bodies"][0]["faces"] = json!(faces);
        }
    })
}

#[test]
fn up_to_next_ends_on_the_first_face_every_profile_ray_meets() {
    for (bytes, typed) in [
        (up_to_next("REMOVE", None), true),
        (up_to_next("NEW", None), true),
        (up_to_next("INTERSECT", None), true),
        (
            up_to_next(
                "REMOVE",
                Some(([0.005, 0.002, 0.0005], [0.008, 0.004, 0.001])),
            ),
            false,
        ),
    ] {
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(
            kinds(&model),
            [
                "sketch",
                "extrude",
                if typed { "extrude" } else { "native" }
            ]
        );
        let unproved = report.losses().iter().any(|loss| {
            loss.code.as_deref() == Some("onshape/history.not-supported")
                && loss
                    .detail
                    .contains("exact coverage and first intersection over the complete profile")
        });
        if typed {
            assert!(!unproved, "{:?}", report.losses());
            let wire = timeline(&model);
            let termination = &wire["model"]["features"][2]["definition"]["value"]["value"]
                ["extent"]["side"]["termination"];
            assert_eq!(termination["kind"], "to_face");
            assert_eq!(termination["face"]["value"]["faces"], json!(["high2"]));
            // The target's producer is the feature that made the body.
            assert!(wire["model"]["features"][2]["dependencies"]
                .as_array()
                .unwrap()
                .contains(&wire["model"]["features"][1]["id"]));
        } else {
            assert!(unproved, "{:?}", report.losses());
        }
    }
}

#[test]
fn up_to_next_takes_a_slanted_planar_end_over_the_complete_profile() {
    // Raise the top of a synthetic box linearly along X, leaving its bottom on XY.
    let bytes = changed(&up_to_next("REMOVE", None), |path, value| {
        if !path.ends_with("bodydetails/2.json") {
            return;
        }
        let point = |p: &mut Value| {
            let x = p["x"].as_f64().unwrap();
            let z = p["z"].as_f64().unwrap();
            p["z"] = json!(z * (1.0 + 10.0 * x));
        };
        for edge in value["bodies"][0]["edges"].as_array_mut().unwrap() {
            for key in ["startPoint", "midPoint", "endPoint"] {
                point(&mut edge["geometry"][key]);
            }
            point(&mut edge["curve"]["origin"]);
            let start = &edge["geometry"]["startPoint"];
            let end = &edge["geometry"]["endPoint"];
            edge["curve"]["direction"] = json!({
                "x": end["x"].as_f64().unwrap() - start["x"].as_f64().unwrap(),
                "y": end["y"].as_f64().unwrap() - start["y"].as_f64().unwrap(),
                "z": end["z"].as_f64().unwrap() - start["z"].as_f64().unwrap()
            });
        }
        for face in value["bodies"][0]["faces"].as_array_mut().unwrap() {
            point(&mut face["surface"]["origin"]);
            point(&mut face["box"]["minCorner"]);
            point(&mut face["box"]["maxCorner"]);
            if face["id"] == "high2" {
                face["surface"]["normal"] = json!({"x": -0.02, "y": 0.0, "z": 1.0});
            }
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["sketch", "extrude", "extrude"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    let extrude = &wire["model"]["features"][2];
    let termination = &extrude["definition"]["value"]["value"]["extent"]["side"]["termination"];
    assert_eq!(termination["kind"], "to_face");
    assert_eq!(termination["face"]["value"]["faces"], json!(["high2"]));
    assert!(extrude["dependencies"]
        .as_array()
        .unwrap()
        .contains(&wire["model"]["features"][1]["id"]));
}

/// The `UP_TO_NEXT` bundle with a second body in the rollback state.
fn up_to_next_with_body(kind: &str, consumed: bool, low: [f64; 3], high: [f64; 3]) -> Vec<u8> {
    changed(&up_to_next("REMOVE", None), |path, value| {
        if path.ends_with("bodydetails/2.json") {
            let (edges, faces) = box_body(low, high, "other");
            let mut body = json!({"id": "other", "type": kind, "edges": edges, "faces": faces, "vertices": []});
            if consumed {
                body["consumedByComposite"] = json!(true);
            }
            value["bodies"].as_array_mut().unwrap().push(body);
        }
    })
}

fn stopped_with(bytes: &[u8], reason: &str) {
    let (model, report) = read(bytes).unwrap();
    assert_eq!(kinds(&model), ["sketch", "extrude", "native"]);
    assert!(
        report.losses().iter().any(|loss| {
            loss.code.as_deref() == Some("onshape/history.not-supported")
                && loss.detail.contains(reason)
        }),
        "{reason}: {:?}",
        report.losses()
    );
}

#[test]
fn up_to_next_ends_only_on_live_bodies() {
    let (low, high) = ([0.005, 0.002, 0.0005], [0.008, 0.004, 0.001]);
    // A live body of any type in front of the covering face blocks the travel.
    for kind in ["SHEET", "SOLID"] {
        stopped_with(
            &up_to_next_with_body(kind, false, low, high),
            "exact coverage and first intersection",
        );
    }
    // A body consumed by a composite is not a live part.
    for kind in ["SHEET", "SOLID"] {
        let (model, report) = read(&up_to_next_with_body(kind, true, low, high)).unwrap();
        assert_eq!(
            kinds(&model),
            ["sketch", "extrude", "extrude"],
            "{kind}: {:?}",
            report.losses()
        );
    }
}

#[test]
fn up_to_next_scoped_forms_stay_native() {
    for operation in ["NEW", "ADD", "REMOVE", "INTERSECT"] {
        let bytes = changed(&up_to_next(operation, None), |path, value| {
            if path.ends_with("features.json") {
                value["features"][2]["parameters"].as_array_mut().unwrap().push(
                    json!({"parameterId": "booleanScope", "queries": [{"btType": "BTMIndividualQuery-138", "deterministicIds": ["body"]}]}),
                );
            }
        });
        stopped_with(
            &bytes,
            "UP_TO_NEXT with a booleanScope selection is not proved",
        );
    }
}

#[test]
fn a_profile_over_the_work_limit_stays_native() {
    // A convex polygon whose quadratic ordering and convexity work exceeds the budget.
    let sides = 1_100u32;
    let point = |i: u32| {
        let angle = std::f64::consts::TAU * f64::from(i % sides) / f64::from(sides);
        [0.01 * angle.cos(), 0.01 * angle.sin()]
    };
    let bytes = changed(&up_to_next("REMOVE", None), |path, value| {
        if path.ends_with("features.json") {
            let entities: Vec<Value> = (0..sides)
                .map(|i| {
                    let (a, b) = (point(i), point(i + 1));
                    let length = (b[0] - a[0]).hypot(b[1] - a[1]);
                    json!({"btType": "BTMSketchCurveSegment-155", "entityId": format!("line{i}"),
                        "startPointId": format!("line{i}.start"), "endPointId": format!("line{i}.end"),
                        "startParam": 0, "endParam": length,
                        "geometry": {"btType": "BTCurveGeometryLine-117", "pntX": a[0], "pntY": a[1],
                            "dirX": (b[0] - a[0]) / length, "dirY": (b[1] - a[1]) / length}})
                })
                .collect();
            value["features"][0]["entities"] = json!(entities);
        }
    });
    stopped_with(&bytes, "the exact extent work budget is exhausted");
}

#[test]
fn up_to_next_refuses_a_narrow_recess_and_an_uncovered_profile_interior() {
    for interior in [false, true] {
        let bytes = changed(&up_to_next("REMOVE", None), |path, response| {
            if !path.ends_with("bodydetails/2.json") {
                return;
            }
            let low = if interior {
                [0.007, 0.004, 0.002]
            } else {
                [0.001, 0.0, 0.002]
            };
            let high = if interior {
                [0.008, 0.006, 0.004]
            } else {
                [0.002, 0.001, 0.004]
            };
            let (edges, faces) = box_body(low, high, "recess");
            let body = &mut response["bodies"][0];
            let boundary = faces.iter().find(|f| f["id"] == "recesslow2").unwrap()["loops"][0]
                ["coedges"]
                .clone();
            let first_face = body["faces"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|f| f["id"] == "high2")
                .unwrap();
            first_face["loops"]
                .as_array_mut()
                .unwrap()
                .push(json!({"isOuter":false,"coedges":boundary}));
            body["edges"].as_array_mut().unwrap().extend(edges);
            body["faces"]
                .as_array_mut()
                .unwrap()
                .extend(faces.into_iter().filter(|f| f["id"] != "recesslow2"));
        });
        let result = crate::test_support::review::decode(&bytes);
        let wire = crate::test_support::review::wire(&result);
        assert_eq!(
            wire["model"]["features"][2]["definition"]["value"]["kind"],
            "native"
        );
        assert!(result
            .report()
            .losses
            .iter()
            .any(|l| l.code.to_string() == "onshape/history.not-supported"
                && l.message.contains("exact coverage and first intersection")));
        crate::test_support::review::validate(result);
    }
}
