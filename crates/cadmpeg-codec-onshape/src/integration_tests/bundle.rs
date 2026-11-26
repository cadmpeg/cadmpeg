// SPDX-License-Identifier: Apache-2.0
//! Synthetic REST bundles only; fixture provenance and generator live in fixtures/.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use cadmpeg_core::serde_erase::json;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use crate::reader::Model;
use crate::test_support::*;
use cadmpeg_core::json;
use cadmpeg_ir::codec::Codec;
use serde_json::Value;

pub(super) fn read(bytes: &[u8]) -> Result<(Model, Report), cadmpeg_core::CodecError> {
    let mut report = Report::default();
    let model = read_bytes(Path::new("test.onshape-bundle"), bytes, &mut report)?;
    Ok((model, report))
}

pub(super) fn changed(bytes: &[u8], mut edit: impl FnMut(&str, &mut Value)) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let mut data = Vec::new();
        member.read_to_end(&mut data).unwrap();
        if member.name().unwrap().ends_with(".json") {
            let mut value = json::from_slice(&data).unwrap();
            edit(&member.name().unwrap(), &mut value);
            data = json::to_vec(&value).unwrap();
        }
        zip.start_file(
            member.name().unwrap().as_ref(),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

pub(super) fn timeline(model: &Model) -> Value {
    let mut wire = cadmpeg_test_support::wire::in_memory_json(&model.ir);
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|f| f["ordinal"].as_u64());
    wire
}

pub(super) fn kinds(model: &Model) -> Vec<String> {
    timeline(model)["model"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            f["definition"]["value"]["kind"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[test]
fn synthetic_bundles_retain_the_expected_cadir_rows_and_final_step() {
    for (name, expected) in [
        ("plate-fillet", vec!["sketch", "extrude", "fillet"]),
        ("revolve", vec!["sketch", "revolve"]),
        ("hole", vec!["sketch", "extrude", "sketch", "hole"]),
        ("unsupported", vec!["sketch", "extrude", "native"]),
    ] {
        let (model, report) = read(&fixture(name)).unwrap();
        assert_eq!(kinds(&model), expected, "{name}: {:?}", report.losses());
        assert_eq!(model.ir.geometry.bodies.len(), 1);
        let wire = timeline(&model);
        let written = json::to_value(&model.ir).unwrap();
        let roundtrip: cadmpeg_ir::CadIr = json::from_value(written.clone()).unwrap();
        assert_eq!(json::to_value(&roundtrip).unwrap(), written);
        assert!(wire["model"].get("bodies").is_none());
        assert!(!wire["geometry"]["edges"].as_array().unwrap().is_empty());
        for edge in wire["geometry"]["edges"].as_array().unwrap() {
            assert!(edge.get("start").is_none());
            assert!(edge["ends"].is_object());
        }
        for arena in ["curves", "surfaces"] {
            for carrier in wire["geometry"][arena].as_array().unwrap() {
                assert_eq!(carrier["geometry"]["kind"], "solved");
                assert!(carrier["geometry"]["value"]["kind"].is_string());
            }
        }
        assert_eq!(
            wire["model"]["features"][1]["outputs"],
            json!([model.ir.geometry.bodies[0].id])
        );
        assert!(model
            .source_step
            .as_ref()
            .unwrap()
            .starts_with("ISO-10303-21;"));
        let losses: Vec<_> = report
            .losses()
            .iter()
            .filter(|loss| {
                loss.code
                    .as_deref()
                    .is_some_and(|s| s.starts_with("onshape/"))
            })
            .collect();
        if name == "unsupported" {
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code.as_deref(),
                Some("onshape/history.not-supported")
            );
            assert!(losses[0].detail.contains("custom FeatureScript"));
        } else {
            assert!(losses.is_empty(), "{name}: {losses:?}");
        }
    }
}

#[test]
fn built_in_plane_queries_preserve_quoted_identity() {
    for (plane, expected) in [
        ("Top", vec!["sketch", "revolve"]),
        ("T op", vec!["native", "native"]),
    ] {
        let bytes = changed(&fixture("revolve"), |path, value| {
            if path.ends_with("features.json") {
                let query = &mut value["features"][0]["parameters"][0]["queries"][0];
                *query = json!({
                    "btType": "BTMIndividualQuery-138",
                    "queryString": format!("qCreatedBy ( makeId ( \"{plane}\" ), EntityType.FACE ) ;")
                });
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model), expected, "{plane}: {:?}", report.losses());
        if plane == "T op" {
            assert!(report
                .losses()
                .iter()
                .any(|loss| loss.detail.contains("sketch plane query")));
        }
    }
}

#[test]
fn exact_input_edges_are_converted_from_metres_at_the_feature_rollback() {
    let (model, _) = read(&fixture("plate-fillet")).unwrap();
    let wire = timeline(&model);
    let state = &wire["model"]["feature_input_topologies"][0];
    assert_eq!(state["input_of"], "onshape:model:feature#Corner%20round");
    assert_eq!(
        state["edge_geometry"][0]["interior"],
        json!({"x": 0.0, "y": 0.0, "z": 1.0})
    );
    assert_eq!(state["edge_geometry"][0]["end"]["z"], 2.0);
    assert_eq!(
        wire["model"]["sketch_entities"][0]["geometry"]["end"]["u"],
        20.0
    );
    assert_eq!(
        wire["model"]["sketch_constraints"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(wire["model"]["parameters"][0]["value"]["value"], 20.0);
}

#[test]
fn rollback_edge_sampling_retains_ids_and_reports_each_failure() {
    for failure in ["ELLIPSE", "midPoint", "direction"] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("bodydetails/2.json") {
                let line = value["bodies"][0]["edges"][0].clone();
                let mut circle = line.clone();
                circle["id"] = json!("circle-edge");
                circle["curve"] = json!({"type":"CIRCLE",
                    "origin":{"x":0.001,"y":0.0,"z":0.001},
                    "normal":{"x":0.0,"y":0.0,"z":1.0}});
                let mut unsampled = line.clone();
                unsampled["id"] = json!("unsampled-edge");
                match failure {
                    "ELLIPSE" => unsampled["curve"]["type"] = json!("ELLIPSE"),
                    "midPoint" => {
                        unsampled["geometry"]
                            .as_object_mut()
                            .unwrap()
                            .remove("midPoint");
                    }
                    _ => {
                        unsampled["curve"]
                            .as_object_mut()
                            .unwrap()
                            .remove("direction");
                    }
                }
                value["bodies"][0]["edges"] = json!([line, circle, unsampled]);
            }
        });
        let (model, report) = read(&bytes).unwrap();
        let wire = timeline(&model);
        let state = &wire["model"]["feature_input_topologies"][0];
        assert_eq!(
            state["edges"],
            json!(["circle-edge", "unsampled-edge", "upright"])
        );
        let samples = state["edge_geometry"].as_array().unwrap();
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0]["edge"], "circle-edge");
        assert_eq!(samples[1]["edge"], "upright");
        let losses: Vec<_> = report
            .losses()
            .iter()
            .filter(|loss| {
                loss.code
                    .as_deref()
                    .is_some_and(|code| code.starts_with("onshape/"))
            })
            .collect();
        assert_eq!(losses.len(), 1, "{failure}: {losses:?}");
        assert_eq!(
            losses[0].code.as_deref(),
            Some("onshape/topology.edge-geometry-unsampled")
        );
        assert!(losses[0].detail.contains("unsampled-edge"));
        let reason = match failure {
            "ELLIPSE" => "ELLIPSE input edges are not supported yet",
            "midPoint" => "edge midpoint is missing",
            _ => "line direction is missing",
        };
        assert!(losses[0].detail.contains(reason));
        assert_eq!(kinds(&model), ["sketch", "extrude", "fillet"]);
    }
}

#[test]
fn unresolved_edges_never_fall_back_to_final_topology_or_another_rollback() {
    for failure in ["id", "index", "microversion", "curve", "duplicate"] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("bodydetails/2.json") {
                match failure {
                    "id" => value["bodies"][0]["edges"][0]["id"] = json!("other"),
                    "microversion" => value["microversionId"]["theId"] = json!("other"),
                    "curve" => value["bodies"][0]["edges"][0]["curve"]["type"] = json!("BCURVE"),
                    "duplicate" => {
                        let row = value["bodies"][0]["edges"][0].clone();
                        value["bodies"][0]["edges"]
                            .as_array_mut()
                            .unwrap()
                            .push(row);
                    }
                    _ => {}
                }
            }
            if path.ends_with("bodydetails/index.json") && failure == "index" {
                *value = json!([1]);
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model), ["sketch", "extrude", "native"]);
        assert!(model.ir.model.feature_input_topologies.is_empty());
        assert!(
            report
                .losses()
                .iter()
                .any(|loss| loss.code.as_deref() == Some("onshape/history.not-supported")),
            "{failure}: {:?}",
            report.losses()
        );
    }
}

#[test]
fn wrong_magic_schema_and_manifest_identity_are_refused() {
    assert!(read(b"not a zip").is_err());
    for failure in ["schema", "path"] {
        let bytes = changed(&fixture("revolve"), |path, value| {
            if path == "manifest.json" {
                match failure {
                    "schema" => value["schema"] = json!("dutchman.onshape-bundle/2"),
                    "path" => value["elements"][0]["id"] = json!("../studio"),
                    _ => {}
                }
            }
        });
        assert!(read(&bytes).is_err(), "{failure}");
    }
}

#[test]
fn consistency_flag_reports_mixed_snapshots_and_preserves_legacy_v1() {
    // The checked-in v1 fixtures predate the optional consistency/provenance fields.
    let legacy = fixture("revolve");
    let (original, _) = read(&legacy).unwrap();
    let consistent = changed(&legacy, |path, value| {
        if path == "manifest.json" {
            value["consistent"] = json!(true);
        }
    });
    let (current, _) = read(&consistent).unwrap();
    assert_eq!(current.ir, original.ir);
    assert_eq!(current.source_step, original.source_step);

    let inconsistent = changed(&legacy, |path, value| {
        if path == "manifest.json" {
            value["consistent"] = json!(false);
        }
        if path.ends_with("features.json") {
            value["features"] = Value::Null;
        }
    });
    let (model, report) = read(&inconsistent).unwrap();
    assert!(model.ir.model.features.is_empty());
    assert_eq!(
        model.ir.geometry.bodies.len(),
        original.ir.geometry.bodies.len()
    );
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("onshape/history.not-supported")));
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("onshape/record.malformed")));
    for flag in [Value::Null, json!("false"), json!(0)] {
        let malformed = changed(&legacy, |path, value| {
            if path == "manifest.json" {
                value["consistent"] = flag.clone();
            }
        });
        assert!(read(&malformed)
            .unwrap_err()
            .to_string()
            .contains("consistency flag must be true or false"));
    }
}

#[test]
fn unsupported_sketch_constraints_keep_exact_curves_with_a_named_loss() {
    let bytes = changed(&fixture("revolve"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][0]["constraints"][0]["constraintType"] = json!("FREEZE");
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(kinds(&model), ["sketch", "revolve"]);
    assert_eq!(model.ir.model.sketch_entities.len(), 5);
    assert_eq!(model.ir.model.sketch_constraints.len(), 1);
    assert!(report.losses().iter().any(|loss| loss.code.as_deref()
        == Some("onshape/sketch.constraint-not-supported")
        && loss.detail.contains("FREEZE")));
}

#[test]
fn extrude_operations_through_all_and_equal_chamfers_map() {
    for operation in ["NEW", "ADD", "REMOVE", "INTERSECT"] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("features.json") {
                value["features"][1]["parameters"][1]["value"] = json!(operation);
                value["features"][1]["parameters"][3]["value"] = json!("THROUGH_ALL");
                value["features"][2]["featureType"] = json!("chamfer");
                value["features"][2]["parameters"][1]["parameterId"] = json!("width");
                value["features"][2]["parameters"].as_array_mut().unwrap().push(json!({"btType": "BTMParameterEnum-145", "parameterId": "chamferType", "value": "EQUAL_OFFSETS"}));
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(
            kinds(&model),
            ["sketch", "extrude", "chamfer"],
            "{:?}",
            report.losses()
        );
        let wire = timeline(&model);
        assert_eq!(
            wire["model"]["features"][1]["definition"]["value"]["value"]["extent"]["side"]
                ["termination"]["kind"],
            "through_all"
        );
        assert_eq!(
            wire["model"]["features"][2]["source_properties"],
            json!({"tangent_propagation": "off"})
        );
        assert_eq!(wire["model"]["features"][1]["source_properties"], json!({}));
    }
}

#[test]
fn up_to_face_uses_analytic_boundaries_of_that_feature_input() {
    let bytes = changed(&fixture("plate-fillet"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][2] = json!({"btType": "BTMFeature-134", "featureType": "extrude", "featureId": "toFace", "name": "To face", "parameters": [
                {"parameterId": "bodyType", "value": "SOLID"}, {"parameterId": "operationType", "value": "NEW"},
                {"parameterId": "entities", "queries": [{"btType": "BTMIndividualSketchRegionQuery-140", "featureId": "Base sketch"}]},
                {"parameterId": "endBound", "value": "UP_TO_SURFACE"},
                {"parameterId": "endBoundEntityFace", "queries": [{"btType": "BTMIndividualQuery-138", "deterministicIds": ["top"]}]}
            ]});
        }
        if path.ends_with("bodydetails/2.json") {
            let points = [
                [0.0, 0.0, 0.002],
                [0.02, 0.0, 0.002],
                [0.02, 0.01, 0.002],
                [0.0, 0.01, 0.002],
            ];
            let point = |p: [f64; 3]| json!({"x": p[0], "y": p[1], "z": p[2]});
            let edges: Vec<_> = (0..4).map(|i| {
                let a = points[i];
                let b = points[(i+1)%4];
                let delta = std::array::from_fn::<_,3,_>(|j| b[j]-a[j]);
                let length = delta.iter().map(|v| v*v).sum::<f64>().sqrt();
                json!({"id": format!("edge{i}"), "curve": {"type": "LINE", "origin": point(a), "direction": point(delta.map(|v| v/length))},
                    "geometry": {"startPoint": point(a), "endPoint": point(b), "midPoint": point(std::array::from_fn(|j| f64::midpoint(a[j], b[j])))}})
            }).collect();
            value["bodies"][0]["edges"] = json!(edges);
            value["bodies"][0]["faces"] = json!([{"id": "top", "surface": {"type": "PLANE", "origin": point(points[0]), "normal": point([0.0,0.0,1.0])},
                "loops": [{"isOuter": true, "coedges": (0..4).map(|i| json!({"edgeId": format!("edge{i}"), "orientation": true})).collect::<Vec<_>>()}]}]);
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
    let termination = &wire["model"]["features"][2]["definition"]["value"]["value"]["extent"]
        ["side"]["termination"];
    assert_eq!(termination["kind"], "to_face");
    assert_eq!(
        termination["face"]["value"]["state"],
        "onshape:model:input#toFace"
    );
    assert_eq!(
        wire["model"]["feature_input_topologies"][0]["edge_geometry"][0]["faces"],
        json!(["top"])
    );

    let curved = changed(&bytes, |path, value| {
        if path.ends_with("bodydetails/2.json") {
            value["bodies"][0]["faces"][0]["surface"]["type"] = json!("CYLINDER");
        }
    });
    let (model, report) = read(&curved).unwrap();
    assert_eq!(kinds(&model), ["sketch", "extrude", "native"]);
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("non-planar terminating")));
}

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

fn up_to_next(operation: &str, extra: Option<([f64; 3], [f64; 3])>) -> Vec<u8> {
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
    for bytes in [
        up_to_next("REMOVE", None),
        up_to_next("NEW", None),
        up_to_next(
            "REMOVE",
            Some(([0.005, 0.002, 0.0005], [0.008, 0.004, 0.001])),
        ),
    ] {
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model), ["sketch", "extrude", "native"]);
        assert!(report.losses().iter().any(|loss| loss.code.as_deref()
            == Some("onshape/history.not-supported")
            && loss
                .detail
                .contains("exact coverage and first intersection")));
    }
}

#[test]
fn up_to_next_retains_a_slanted_planar_end_for_the_writer_to_gate() {
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
        ["sketch", "extrude", "native"],
        "{:?}",
        report.losses()
    );
    assert!(report.losses().iter().any(|loss| loss
        .detail
        .contains("exact coverage and first intersection")));
}

#[test]
fn symmetric_non_blind_extrudes_keep_a_specific_loss() {
    for bound in ["UP_TO_NEXT", "UP_TO_SURFACE", "THROUGH_ALL"] {
        let bytes = changed(&up_to_next("REMOVE", None), |path, value| {
            if path.ends_with("features.json") {
                let parameters = value["features"][2]["parameters"].as_array_mut().unwrap();
                parameters
                    .iter_mut()
                    .find(|p| p["parameterId"] == "endBound")
                    .unwrap()["value"] = json!(bound);
                parameters.push(json!({"parameterId": "symmetric", "value": true}));
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model), ["sketch", "extrude", "native"]);
        assert!(
            report.losses().iter().any(|loss| loss
                .detail
                .contains("symmetric extrudes with this bound are not supported yet")),
            "{:?}",
            report.losses()
        );
    }
}

#[test]
fn circles_arcs_and_simple_splines_remain_exact_sketch_entities() {
    let bytes = changed(&fixture("revolve"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][0]["constraints"] = json!([]);
            value["features"][0]["entities"] = json!([
                {"btType": "BTMSketchCurve-4", "entityId": "circle", "geometry": {"btType": "BTCurveGeometryCircle-115", "xCenter": 0.0, "yCenter": 0.0, "radius": 0.005}},
                {"btType": "BTMSketchCurveSegment-155", "entityId": "arc", "startParam": 0.0, "endParam": std::f64::consts::FRAC_PI_2,
                 "geometry": {"btType": "BTCurveGeometryCircle-115", "xCenter": 0.01, "yCenter": 0.0, "radius": 0.002, "xDir": 1.0, "yDir": 0.0}},
                {"btType": "BTMSketchCurve-4", "entityId": "spline", "isConstruction": true,
                 "geometry": {"btType": "BTCurveGeometrySpline-118", "degree": 3, "dimension": 2, "controlPointCount": 4,
                 "controlPoints": [0.0,0.0, 0.001,0.002, 0.003,0.002, 0.004,0.0], "knots": [0.0,0.0,0.0,0.0,1.0,1.0,1.0,1.0]}}
            ]);
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(kinds(&model), ["sketch", "native"], "{:?}", report.losses());
    assert!(report.losses().iter().any(|loss| loss.code.as_deref()
        == Some("onshape/history.not-supported")
        && loss
            .detail
            .contains("selected sketch entity has no solved geometry")));
    let wire = timeline(&model);
    let entities = wire["model"]["sketch_entities"].as_array().unwrap();
    assert_eq!(
        entities
            .iter()
            .map(|e| e["geometry"]["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["arc", "circle", "nurbs"]
    );
    assert_eq!(entities[1]["geometry"]["radius"], 5.0);
    assert_eq!(
        entities[2]["geometry"]["curve"]["poles"]["points"][1],
        json!({"u": 1.0, "v": 2.0})
    );
}

#[test]
fn archive_allocation_and_expansion_are_bounded_before_decode() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "manifest.json",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )
    .unwrap();
    zip.write_all(&vec![b' '; (32 << 20) + 1]).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    assert!(read(&bytes)
        .unwrap_err()
        .to_string()
        .contains("expanded data"));

    let mut bytes = fixture("revolve");
    let end = bytes.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap();
    bytes[end + 8..end + 12].copy_from_slice(&[0xff; 4]);
    assert!(read(&bytes)
        .unwrap_err()
        .to_string()
        .contains("ZIP directory"));

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("../manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"{}").unwrap();
    assert!(read(&zip.finish().unwrap().into_inner()).is_err());
}

#[test]
fn rest_zero_defaults_and_empty_constraints_can_be_omitted() {
    let bytes = changed(&fixture("plate-fillet"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][0]
                .as_object_mut()
                .unwrap()
                .remove("constraints");
            for entity in value["features"][0]["entities"].as_array_mut().unwrap() {
                if let Some(geometry) = entity.get_mut("geometry").and_then(Value::as_object_mut) {
                    geometry.retain(|_, value| value.as_f64() != Some(0.0));
                }
                entity.as_object_mut().unwrap().remove("startParam");
            }
        }
        if path.ends_with("bodydetails/2.json") {
            let edge = &mut value["bodies"][0]["edges"][0];
            for key in ["origin", "direction"] {
                edge["curve"][key]
                    .as_object_mut()
                    .unwrap()
                    .retain(|_, v| v.as_f64() != Some(0.0));
            }
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["sketch", "extrude", "fillet"],
        "{:?}",
        report.losses()
    );
    assert!(model.ir.model.sketch_constraints.is_empty());
}

#[test]
fn unsupported_variants_and_unresolved_quantities_get_named_losses() {
    for (field, expected) in [
        ("radius", "radius"),
        (
            "tangentPropagation",
            "tangent propagation needs complete input vertices and endpoint tangents",
        ),
        ("isVariable", "isVariable"),
        ("deterministicIds", "deterministic"),
        ("crossSection", "circular constant-radius"),
    ] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("features.json") {
                let parameters = value["features"][2]["parameters"].as_array_mut().unwrap();
                match field {
                    "radius" => {
                        parameters[1] = json!({"parameterId": "radius", "expression": "#unknown"});
                    }
                    "deterministicIds" => {
                        parameters[0]["queries"][0]["deterministicIds"] = json!([]);
                    }
                    "crossSection" => parameters[2]["value"] = json!("CONIC"),
                    _ => parameters.push(json!({"parameterId": field, "value": true})),
                }
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model), ["sketch", "extrude", "native"]);
        assert!(
            report.losses().iter().any(|loss| loss.code.as_deref()
                == Some("onshape/history.not-supported")
                && loss.detail.contains(expected)),
            "{field}: {:?}",
            report.losses()
        );
    }
}

#[test]
fn boolean_scopes_must_resolve_to_the_sole_rollback_body() {
    for operation in ["ADD", "REMOVE", "INTERSECT"] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("features.json") {
                value["features"][2] = json!({"btType": "BTMFeature-134", "featureType": "extrude", "featureId": "scoped", "name": "Scoped extrude", "parameters": [
                    {"parameterId": "bodyType", "value": "SOLID"}, {"parameterId": "operationType", "value": operation},
                    {"parameterId": "entities", "queries": [{"btType": "BTMIndividualSketchRegionQuery-140", "featureId": "Base sketch"}]},
                    {"parameterId": "endBound", "value": "THROUGH_ALL"},
                    {"parameterId": "booleanScope", "queries": [{"btType": "BTMIndividualQuery-138", "deterministicIds": ["body"]}]}
                ]});
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
        assert_eq!(
            wire["model"]["features"][1]["outputs"],
            json!([model.ir.geometry.bodies[0].id])
        );
        let bad_scope = changed(&bytes, |path, value| {
            if path.ends_with("features.json") {
                value["features"][2]["parameters"][4]["queries"][0]["deterministicIds"] =
                    json!(["other"]);
            }
        });
        let (model, report) = read(&bad_scope).unwrap();
        assert_eq!(kinds(&model), ["sketch", "extrude", "native"]);
        assert!(report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains("Boolean scope does not select")));
    }
}

#[test]
fn diameter_dimensions_use_the_length_parameter() {
    let bytes = changed(&fixture("hole"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][2]["entities"] = json!([{"btType":"BTMSketchCurve-4","entityId":"circle",
                "geometry":{"btType":"BTCurveGeometryCircle-115","radius":0.003}}]);
            value["features"][2]["constraints"] = json!([{"constraintType":"DIAMETER","entityId":"diameter-dimension",
                "parameters":[{"parameterId":"localFirst","value":"circle"},{"parameterId":"length","expression":"6 mm"}]}]);
        }
    });
    let (model, report) = read(&bytes).unwrap();
    let wire = timeline(&model);
    assert!(
        wire["model"]["sketch_constraints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["definition"]["kind"] == "diameter"),
        "{:?}",
        report.losses()
    );
    assert!(wire["model"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["value"]["value"] == 6.0));
}

#[test]
fn holes_use_locations_and_retain_entry_forms() {
    for (style, form, extra) in [
        ("SIMPLE", "simple", json!([])),
        (
            "C_BORE",
            "counterbore",
            json!([{"parameterId":"cBoreDiameter","expression":"4 mm"},{"parameterId":"cBoreDepth","expression":"1 mm"}]),
        ),
        (
            "C_SINK",
            "countersink",
            json!([{"parameterId":"cSinkDiameter","expression":"4 mm"},{"parameterId":"cSinkAngle","expression":"90 deg"}]),
        ),
    ] {
        let bytes = changed(&fixture("hole"), |path, value| {
            if path.ends_with("features.json") {
                let params = value["features"][3]["parameters"].as_array_mut().unwrap();
                params[0]["parameterId"] = json!("locations");
                params
                    .iter_mut()
                    .find(|p| p["parameterId"] == "style")
                    .unwrap()["value"] = json!(style);
                params.extend(extra.as_array().unwrap().clone());
            }
        });
        let (model, report) = read(&bytes).unwrap();
        let wire = timeline(&model);
        assert_eq!(
            kinds(&model).last().unwrap(),
            "hole",
            "{:?}",
            report.losses()
        );
        assert_eq!(
            wire["model"]["features"][3]["definition"]["value"]["value"]["shape"]["construction"]
                ["value"]["kind"]["kind"],
            form
        );
    }
}

#[test]
fn counterbored_countersunk_and_tapped_holes_keep_their_forms_threads_and_drill_points() {
    let (model, report) = read(&fixture("hole-forms")).unwrap();
    assert_eq!(
        kinds(&model),
        ["sketch", "extrude", "sketch", "hole", "hole", "hole"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    let hole = |i: usize| wire["model"]["features"][i]["definition"]["value"]["value"].clone();
    let counterbore = hole(3);
    assert_eq!(counterbore["profile"]["kind"], "sketch_entities");
    assert_eq!(
        counterbore["profile"]["value"]["entities"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        counterbore["shape"]["construction"]["value"]["kind"],
        json!({"kind": "counterbore", "diameter": 6.0, "depth": 2.0})
    );
    assert_eq!(counterbore["extent"]["kind"], "through_all");
    let countersink = hole(4)["shape"]["construction"]["value"]["kind"].clone();
    assert_eq!(countersink["kind"], "countersink");
    assert!((countersink["angle"].as_f64().unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    let tapped = hole(5);
    assert_eq!(tapped["shape"]["diameter"], 3.3);
    assert_eq!(tapped["extent"], json!({"kind": "blind", "length": 6.0}));
    assert_eq!(tapped["bottom"]["kind"], "angled");
    assert_eq!(tapped["bottom"]["depth_to_tip"], false);
    assert!(
        (tapped["bottom"]["included_angle"].as_f64().unwrap() - 118f64.to_radians()).abs() < 1e-12
    );
    let thread = &tapped["shape"]["construction"]["value"]["specification"];
    assert_eq!(thread["kind"], "threaded");
    assert_eq!(thread["standard"], "ISO");
    assert_eq!(thread["designation"], "M4");
    assert_eq!(thread["major_diameter"], 4.0);
    assert!((thread["pitch"].as_f64().unwrap() - 0.7).abs() < 1e-12);
    assert_eq!(thread["depth"], json!({"kind": "blind", "depth": 5.0}));
}

#[test]
fn hole_standards_and_drill_points_outside_the_mapping_name_their_loss() {
    for (edit, reason) in [
        (
            json!({"parameterId": "standardTappedOrClearance", "value": {"standard": "ISO", "size": "M4", "type": "Pipe"}}),
            "hole standard type is not supported yet",
        ),
        (
            json!({"parameterId": "standardBlindInLast", "value": {"standard": "ISO"}}),
            "hole standard table is not supported yet",
        ),
        (
            json!({"parameterId": "tipAngle", "expression": "200 deg"}),
            "drill point angle is not supported yet",
        ),
    ] {
        let bytes = changed(&fixture("hole-forms"), |path, value| {
            if path.ends_with("features.json") {
                let params = value["features"][5]["parameters"].as_array_mut().unwrap();
                params.retain(|p| p["parameterId"] != edit["parameterId"]);
                params.push(edit.clone());
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model).last().unwrap(), "native", "{reason}");
        assert!(
            report.losses().iter().any(|l| l.detail.contains(reason)),
            "{reason}: {:?}",
            report.losses()
        );
    }
    // A flat-bottomed blind hole states a 180 degree tip.
    let bytes = changed(&fixture("hole-forms"), |path, value| {
        if path.ends_with("features.json") {
            let parameters = value["features"][5]["parameters"].as_array_mut().unwrap();
            parameters.retain(|p| p["parameterId"] != "tipAngle");
            parameters.push(json!({"parameterId": "tipAngle", "expression": "180 deg"}));
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model).last().unwrap(),
        "hole",
        "{:?}",
        report.losses()
    );
    assert_eq!(
        timeline(&model)["model"]["features"][5]["definition"]["value"]["value"]["bottom"]["kind"],
        "flat"
    );
}

#[test]
fn a_resolvable_default_tangent_fillet_maps_and_missing_tangents_name_the_loss() {
    for complete in [true, false] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("features.json") {
                value["features"][2]["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"parameterId":"tangentPropagation","value":true}));
            }
            if path.ends_with("bodydetails/2.json") && complete {
                value["bodies"][0]["vertices"] =
                    json!([{"id":"bottom-vertex"},{"id":"top-vertex"}]);
                let edge = &mut value["bodies"][0]["edges"][0];
                edge["vertices"] = json!(["bottom-vertex", "top-vertex"]);
                edge["geometry"]["startVector"] = json!({"z":1.0});
                edge["geometry"]["endVector"] = json!({"z":1.0});
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(
            kinds(&model).last().unwrap(),
            if complete { "fillet" } else { "native" },
            "{:?}",
            report.losses()
        );
        if complete {
            // The resolved chain is every edge the fillet blends: nothing propagates past it.
            assert_eq!(
                timeline(&model)["model"]["features"][2]["source_properties"],
                json!({"tangent_propagation": "resolved"})
            );
        } else {
            assert!(report.losses().iter().any(|l| l
                .detail
                .contains("tangent propagation needs complete input vertices")));
        }
    }
}

#[test]
fn missing_step_retains_body_history_with_a_geometry_loss() {
    let source = fixture("plate-fillet");
    let mut input = zip::ZipArchive::new(Cursor::new(source)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        if entry.name().unwrap().ends_with("export.step") {
            continue;
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        output
            .start_file(
                entry.name().unwrap().as_ref(),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        output.write_all(&data).unwrap();
    }
    let bytes = output.finish().unwrap().into_inner();
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(kinds(&model), ["sketch", "extrude", "fillet"]);
    assert!(model.ir.geometry.bodies.is_empty());
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("onshape/step.unreadable")));
}

#[test]
fn ellipses_and_elliptical_arcs_keep_axes_units_bounds_and_endpoint_loci() {
    for clockwise in [false, true] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if !path.ends_with("features.json") {
                return;
            }
            let entities = value["features"][0]["entities"].as_array_mut().unwrap();
            let ellipse = json!({"btType":"BTMSketchCurve-4", "entityId":"oval", "isConstruction":true,
                "geometry":{"btType":"BTCurveGeometryEllipse-1189", "xCenter":0.03,"yCenter":0.04,
                    "xDir":0.0,"yDir":1.0,"radius":0.006,"minorRadius":0.002,"clockwise":clockwise}});
            entities.push(ellipse.clone());
            let mut arc = ellipse;
            arc["entityId"] = json!("oval-arc");
            arc["btType"] = json!("BTMSketchCurveSegment-155");
            arc["startParam"] = json!(0.2);
            arc["endParam"] = json!(2.4);
            arc["startPointId"] = json!("oval-arc.start");
            arc["endPointId"] = json!("oval-arc.end");
            entities.push(arc);
            value["features"][0]["constraints"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                "entityId":"oval-joint","constraintType":"COINCIDENT","parameters":[
                    {"parameterId":"localFirst","value":"oval-arc.start"},
                    {"parameterId":"localSecond","value":"line0.start"}]}));
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(
            kinds(&model),
            ["sketch", "extrude", "fillet"],
            "{:?}",
            report.losses()
        );
        let wire = timeline(&model);
        let entities = wire["model"]["sketch_entities"].as_array().unwrap();
        let ellipses: Vec<_> = entities
            .iter()
            .filter(|e| e["geometry"]["kind"] == "ellipse")
            .collect();
        assert_eq!(ellipses.len(), 2);
        let joint = wire["model"]["sketch_constraints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["definition"]["kind"] == "coincident_loci")
            .unwrap();
        assert_eq!(
            joint["definition"]["loci"][0]["kind"],
            if clockwise { "end" } else { "start" }
        );
        let whole = &ellipses[0]["geometry"];
        assert_eq!(whole["center"], json!({"u":30.0,"v":40.0}));
        assert_eq!(whole["major_radius"], 6.0);
        assert_eq!(whole["minor_radius"], 2.0);
        assert_eq!(whole["major_angle"], std::f64::consts::FRAC_PI_2);
        assert!(whole.get("bounds").is_none());
        assert_eq!(
            ellipses[1]["geometry"]["bounds"],
            if clockwise {
                json!([-2.4, -0.2])
            } else {
                json!([0.2, 2.4])
            }
        );
    }
}

#[test]
fn thin_extrusions_refuse_unscoped_design_construction() {
    for midplane in [false, true] {
        for flip in [false, true] {
            for second in [0.0, 0.2] {
                let bytes = changed(&fixture("plate-fillet"), |path, value| {
                    if !path.ends_with("features.json") {
                        return;
                    }
                    let params = value["features"][1]["parameters"].as_array_mut().unwrap();
                    params[0]["value"] = json!("THIN");
                    params
                        .iter_mut()
                        .find(|p| p["parameterId"] == "entities")
                        .unwrap()["parameterId"] = json!("wallShape");
                    params.push(json!({"parameterId":"entities","queries":[]}));
                    params.extend([
                        json!({"parameterId":"midplane","value":midplane}),
                        json!({"parameterId":"flipWall","value":flip}),
                        json!({"parameterId":"thickness","expression":"0.3 mm"}),
                        json!({"parameterId":"thickness1","expression":"0.1 mm"}),
                        json!({"parameterId":"thickness2","expression":format!("{second} mm")}),
                    ]);
                });
                let result = codec()
                    .decode(
                        &mut Cursor::new(bytes),
                        &cadmpeg_ir::codec::DecodeOptions::default(),
                    )
                    .unwrap();
                let (ir, report, fidelity) = result.into_parts();
                assert!(ir.design.features.is_empty());
                let wire = cadmpeg_test_support::wire::in_memory_json(&ir);
                let feature = wire["model"]["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|f| f["ordinal"] == 1)
                    .unwrap();
                assert_eq!(feature["definition"]["value"]["kind"], "native");
                assert!(report
                    .losses
                    .iter()
                    .any(|l| l.code.to_string() == "onshape/extrude.thin-not-supported"));
                assert!(cadmpeg_ir::validate_neutral_with_source_fidelity(
                    &ir,
                    &fidelity,
                    report.losses
                )
                .unwrap()
                .is_ok());
            }
        }
    }
}

#[test]
fn chamfer_forms_keep_both_dimensions_and_direction() {
    for (form, expected) in [
        (
            "TWO_OFFSETS",
            json!({"kind":"two_distances","first":0.4,"second":0.8}),
        ),
        (
            "DISTANCE_ANGLE",
            json!({"kind":"distance_angle","distance":0.4,"angle":std::f64::consts::FRAC_PI_6}),
        ),
    ] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if !path.ends_with("features.json") {
                return;
            }
            let feature = &mut value["features"][2];
            feature["featureType"] = json!("chamfer");
            feature["parameters"].as_array_mut().unwrap().extend([
                json!({"parameterId":"chamferType","value":form}),
                json!({"parameterId":"width","expression":"0.4 mm"}),
                json!({"parameterId":"width1","expression":"0.4 mm"}),
                json!({"parameterId":"width2","expression":"0.8 mm"}),
                json!({"parameterId":"angle","expression":"30 deg"}),
                json!({"parameterId":"oppositeDirection","value":true}),
            ]);
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(
            kinds(&model),
            ["sketch", "extrude", "chamfer"],
            "{:?}",
            report.losses()
        );
        let wire = timeline(&model);
        let chamfer = &wire["model"]["features"][2]["definition"]["value"]["value"];
        let mut actual = chamfer["groups"][0]["spec"].clone();
        let mut expected = expected;
        if form == "DISTANCE_ANGLE" {
            let angle = actual
                .as_object_mut()
                .unwrap()
                .remove("angle")
                .unwrap()
                .as_f64()
                .unwrap();
            let expected_angle = expected
                .as_object_mut()
                .unwrap()
                .remove("angle")
                .unwrap()
                .as_f64()
                .unwrap();
            assert!((angle - expected_angle).abs() <= f64::EPSILON);
        }
        assert_eq!(actual, expected);
        assert_eq!(chamfer["flip_direction"], true);
    }
}

#[test]
fn an_unused_ellipse_does_not_block_an_explicit_line_region() {
    let bytes = changed(&fixture("default-plane-regions"), |path, value| {
        if !path.ends_with("features.json") {
            return;
        }
        value["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .push(json!({
            "btType":"BTMSketchCurve-4","entityId":"separate-oval","isConstruction":false,
            "geometry":{"btType":"BTCurveGeometryEllipse-1189","xCenter":0.04,"yCenter":0.04,
                "xDir":1.0,"yDir":0.0,"radius":0.003,"minorRadius":0.001}}));
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["sketch", "extrude"],
        "{:?}",
        report.losses()
    );
    assert!(timeline(&model)["model"]["sketch_entities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["geometry"]["kind"] == "ellipse" && e["construction"] == false));
}

#[test]
fn inch_variable_arithmetic_keeps_canonical_values_and_source_units() {
    let bytes = changed(&fixture("plate-fillet"), |path, value| {
        if !path.ends_with("features.json") {
            return;
        }
        let rows = value["features"].as_array_mut().unwrap();
        rows.truncate(2);
        for (at, name, expression) in [(0, "stock", "0.5 inch"), (1, "reach", "#stock + 1 cm")] {
            rows.insert(
                at,
                json!({"featureId":name,"featureType":"assignVariable","parameters":[
                {"parameterId":"name","value":name},{"parameterId":"variableType","value":"LENGTH"},
                {"parameterId":"lengthValue","expression":expression}]}),
            );
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["tree_node", "tree_node", "sketch", "extrude"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    let parameters = wire["model"]["parameters"].as_array().unwrap();
    let stock = parameters.iter().find(|p| p["name"] == "stock").unwrap();
    assert_eq!(stock["expression"], "0.5 in");
    assert_eq!(stock["value"]["value"], 12.7);
    assert_eq!(stock["properties"]["display_unit"], "in");
    let reach = parameters.iter().find(|p| p["name"] == "reach").unwrap();
    assert_eq!(reach["value"]["value"], 22.7);
    assert_eq!(reach["expression"], "(stock + 1 cm)");
    assert_eq!(reach["dependencies"], json!([stock["id"]]));
}

#[test]
fn elliptical_curves_with_unresolved_axes_or_bounds_leave_a_named_loss() {
    for bad in ["axis", "radius", "backward", "multi-turn"] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if !path.ends_with("features.json") {
                return;
            }
            let mut curve = json!({"btType":"BTMSketchCurveSegment-155","entityId":"bad-oval",
                "startParam":0.0,"endParam":1.0,
                "geometry":{"btType":"BTCurveGeometryEllipse-1189","xDir":1.0,"radius":0.004,"minorRadius":0.002}});
            match bad {
                "axis" => curve["geometry"]["xDir"] = json!(0.0),
                "radius" => curve["geometry"]["minorRadius"] = json!(-0.002),
                "backward" => curve["endParam"] = json!(-1.0),
                _ => curve["endParam"] = json!(7.0),
            }
            value["features"][0]["entities"]
                .as_array_mut()
                .unwrap()
                .push(curve);
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model)[0], "sketch");
        assert!(report
            .losses()
            .iter()
            .any(|l| l.code.as_deref() == Some("onshape/sketch.geometry-not-supported")));
        let wire = timeline(&model);
        let entities = wire["model"]["sketch_entities"].as_array().unwrap();
        assert_eq!(
            entities
                .iter()
                .filter(|e| e["geometry"]["kind"] == "line")
                .count(),
            4
        );
        assert!(entities.iter().any(|e| e["geometry"]["kind"] == "native"));
        assert!(model.source_step.is_some());
    }
}

use crate::test_support::regions as region_fixture;

#[test]
fn an_open_divider_cannot_expand_the_selected_frame_face() {
    for divider in [false, true] {
        let bytes = changed(&fixture("default-plane-regions"), |path, value| {
            if path.ends_with("features.json") {
                region_fixture::frame(value, divider);
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(
            kinds(&model),
            if divider {
                vec!["sketch", "native"]
            } else {
                vec!["sketch", "extrude"]
            }
        );
        assert_eq!(
            report.losses().iter().any(|l| l.code.as_deref()
                == Some("onshape/history.not-supported")
                && l.detail.contains("open divider")),
            divider,
            "{:?}",
            report.losses()
        );
    }
}

#[test]
fn repeated_exterior_consumers_exhaust_the_bundle_budget_early() {
    let bytes = changed(&fixture("default-plane-regions"), |path, value| {
        if !path.ends_with("features.json") {
            return;
        }
        region_fixture::frame(value, false);
        let entities = value["features"][0]["entities"].as_array_mut().unwrap();
        for i in 0..128 {
            entities.push(json!({"btType":"BTMSketchCurve-4","entityId":format!("small-hole-{i}"),"geometry":{
                "btType":"BTCurveGeometryCircle-115","radius":0.00005,
                "xCenter":0.011 + f64::from(i % 16)*0.0004,"yCenter":0.001 + f64::from(i / 16)*0.0004,"xDir":1.0}}));
        }
        let template = value["features"][1].clone();
        let features = value["features"].as_array_mut().unwrap();
        for i in 0..128 {
            let mut feature = template.clone();
            feature["featureId"] = json!(format!("repeat-{i}"));
            if i == 127 {
                feature["parameters"][0]["value"] = json!("THIN");
            }
            features.push(feature);
        }
    });
    let started = std::time::Instant::now();
    let (model, report) = read(&bytes).unwrap();
    let elapsed = started.elapsed();
    let kinds = kinds(&model);
    assert_eq!(kinds[1], "extrude");
    let first_loss = kinds.iter().position(|k| k == "native").unwrap();
    assert!(
        first_loss < 100,
        "refuse well before expanding every consumer: {first_loss}"
    );
    assert!(kinds[first_loss..].iter().all(|k| k == "native"));
    let wire = timeline(&model);
    let boundary_bytes: usize = wire["model"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f.pointer("/definition/value/value/profile/value/value/regions"))
        .map(|regions| json::to_vec(regions).unwrap().len())
        .sum();
    eprintln!("Cumulative recovery: refused at consumer {first_loss} of 129 (128 extra holes); emitted {boundary_bytes} region JSON bytes; reader {elapsed:?}");
    assert!(
        report
            .losses()
            .iter()
            .any(|l| l.detail.contains("budget is exhausted")),
        "{:?}",
        report.losses()
    );
    assert!(report
        .losses()
        .iter()
        .filter(|l| l.detail.contains("budget is exhausted"))
        .all(|l| l.code.as_deref() == Some("onshape/history.not-supported")));
}

fn isolated_arcs(response: &mut Value) {
    region_fixture::frame(response, false);
    let entities = response["features"][0]["entities"].as_array_mut().unwrap();
    for (name, radius, from, to) in [
        ("free-main", 0.003, 0.0, 1.5 * std::f64::consts::PI),
        (
            "free-gap",
            0.003,
            5.0 * std::f64::consts::PI / 3.0,
            11.0 * std::f64::consts::PI / 6.0,
        ),
        (
            "free-inner",
            0.001,
            std::f64::consts::PI / 4.0,
            3.0 * std::f64::consts::PI / 4.0,
        ),
    ] {
        entities.push(json!({"btType":"BTMSketchCurveSegment-155","entityId":name,
            "startParam":from,"endParam":to,"geometry":{"btType":"BTCurveGeometryCircle-115",
            "xCenter":0.014,"yCenter":0.010,"radius":radius,"xDir":1.0}}));
    }
}

#[test]
fn isolated_arcs_do_not_partition_a_face_but_an_attached_arc_does() {
    for attached in [false, true] {
        let bytes = changed(&fixture("default-plane-regions"), |path, value| {
            if path.ends_with("features.json") {
                if attached {
                    region_fixture::arc_divider(value);
                } else {
                    isolated_arcs(value);
                }
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(
            kinds(&model),
            if attached {
                vec!["sketch", "native"]
            } else {
                vec!["sketch", "extrude"]
            },
            "{:?}",
            report.losses()
        );
        if attached {
            assert!(report
                .losses()
                .iter()
                .any(|l| l.detail.contains("open divider")));
        } else {
            let wire = timeline(&model);
            let region = &wire["model"]["features"][1]["definition"]["value"]["value"]["profile"]
                ["value"]["value"]["regions"][0];
            assert_eq!(region["outer_boundary"].as_array().unwrap().len(), 4);
            assert_eq!(region["hole_boundaries"].as_array().unwrap().len(), 1);
            let free: Vec<_> = wire["model"]["sketch_entities"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["id"].as_str().unwrap().contains("/free-"))
                .collect();
            assert_eq!(free.len(), 3);
            assert!(free.iter().all(|e| e["construction"] == false));
        }
    }
}

#[test]
fn overlapping_dividers_are_not_lost_as_a_zero_area_ring() {
    let bytes = changed(&fixture("default-plane-regions"), |path, value| {
        if path.ends_with("features.json") {
            region_fixture::coincident_dividers(value);
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(kinds(&model), vec!["sketch", "native"]);
    assert!(report
        .losses()
        .iter()
        .any(|l| l.detail.contains("open divider")));
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

#[test]
fn a_whole_sketch_region_query_maps_despite_its_resolved_region_ids() {
    for (query_string, mapped) in [
        ("query = qSketchRegion(id + \"Base sketch\", false);", true),
        (
            "query = qSketchRegion(id + \"Other sketch\", false);",
            false,
        ),
        ("query = qSketchRegion(id + \"Base sketch\", true);", false),
        (
            "query = qUnion([qSketchRegion(id + \"Base sketch\", false)]);",
            false,
        ),
    ] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("features.json") {
                let query = &mut value["features"][1]["parameters"][2]["queries"][0];
                query["deterministicIds"] = json!(["JGC"]);
                query["queryString"] = json!(query_string);
                query["filterInnerLoops"] = json!(false);
            }
        });
        let (model, report) = read(&bytes).unwrap();
        let deterministic = report.losses().iter().any(|loss| {
            loss.code.as_deref() == Some("onshape/history.not-supported")
                && loss.detail.contains("deterministic sketch-region subsets")
        });
        if mapped {
            assert_eq!(kinds(&model)[1], "extrude", "{query_string}");
            assert!(!deterministic, "{query_string}: {:?}", report.losses());
        } else {
            assert_eq!(kinds(&model)[1], "native", "{query_string}");
            assert!(deterministic, "{query_string}: {:?}", report.losses());
        }
    }
}

#[test]
fn whole_sketch_inner_loop_filter_selects_the_ring_and_preserves_unfiltered_queries() {
    for (nested, filter, clockwise) in [
        (false, false, false),
        (false, true, false),
        (true, false, false),
        (true, true, false),
        (true, true, true),
    ] {
        {
            let bytes = changed(&fixture("default-plane-regions"), |path, value| {
                if !path.ends_with("features.json") {
                    return;
                }
                region_fixture::frame(value, false);
                if clockwise {
                    let lines = [
                        (0.0, 0.0, 0.0, 1.0),
                        (0.0, 0.02, 1.0, 0.0),
                        (0.02, 0.02, 0.0, -1.0),
                        (0.02, 0.0, -1.0, 0.0),
                    ];
                    for (i, (x, y, dx, dy)) in lines.into_iter().enumerate() {
                        let geometry = &mut value["features"][0]["entities"][i]["geometry"];
                        geometry["pntX"] = json!(x);
                        geometry["pntY"] = json!(y);
                        geometry["dirX"] = json!(dx);
                        geometry["dirY"] = json!(dy);
                    }
                }
                if !nested {
                    value["features"][0]["entities"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }
                let query = &mut value["features"][1]["parameters"][2]["queries"][0];
                *query = json!({"btType":"BTMIndividualSketchRegionQuery-140",
                    "featureId":"drawn-profile", "deterministicIds":["outer-region", "inner-region"],
                    "filterInnerLoops":filter,
                    "queryString":format!("query = qSketchRegion(id + \"drawn-profile\", {filter});")});
            });
            let (model, report) = read(&bytes).unwrap();
            assert_eq!(kinds(&model)[1], "extrude", "{:?}", report.losses());
            let wire = timeline(&model);
            let profile =
                &wire["model"]["features"][1]["definition"]["value"]["value"]["profile"]["value"];
            if nested && filter {
                assert_eq!(profile["kind"], "sketch_regions");
                let regions = profile["value"]["regions"].as_array().unwrap();
                assert_eq!(regions.len(), 1);
                assert_eq!(regions[0]["outer_boundary"].as_array().unwrap().len(), 4);
                let holes = regions[0]["hole_boundaries"].as_array().unwrap();
                assert_eq!(holes.len(), 1);
                assert_eq!(holes[0].as_array().unwrap().len(), 1);
                assert!(holes[0][0]["entity"].as_str().unwrap().ends_with("/hole"));
                // The outer ring runs counterclockwise about the sketch normal whichever
                // way the lines are stored: twice the signed area of the 20 mm square is
                // 2 * 20 * 20. The hole circle is one use over [0, TAU] and runs the
                // opposite way.
                let mut doubled_area = 0.0;
                for part in regions[0]["outer_boundary"].as_array().unwrap() {
                    let entity = wire["model"]["sketch_entities"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|e| e["id"] == part["entity"])
                        .unwrap();
                    let point = |key: &str| {
                        let p = &entity["geometry"][key];
                        [p["u"].as_f64().unwrap(), p["v"].as_f64().unwrap()]
                    };
                    let (a, b) = if part["reversed"].as_bool().unwrap() {
                        (point("end"), point("start"))
                    } else {
                        (point("start"), point("end"))
                    };
                    doubled_area += a[0] * b[1] - a[1] * b[0];
                }
                assert!((doubled_area - 800.0).abs() < 1e-6, "{doubled_area}");
                assert_eq!(holes[0][0]["reversed"], true);
            } else {
                assert_eq!(profile["kind"], "sketch");
            }
        }
    }
}

#[test]
fn overlapping_source_loops_keep_all_sketch_regions() {
    for filter in [false, true] {
        let bytes = changed(&fixture("default-plane-regions"), |path, value| {
            if !path.ends_with("features.json") {
                return;
            }
            region_fixture::frame(value, false);
            let entities = value["features"][0]["entities"].as_array_mut().unwrap();
            entities.pop();
            let mut shifted = entities.clone();
            for (i, entity) in shifted.iter_mut().enumerate() {
                entity["entityId"] = json!(format!("shifted{i}"));
                for coordinate in ["pntX", "pntY"] {
                    let origin = entity["geometry"][coordinate].as_f64().unwrap();
                    entity["geometry"][coordinate] = json!(origin + 0.01);
                }
            }
            entities.extend(shifted);
            value["features"][1]["parameters"][2]["queries"][0] = json!({
                "btType":"BTMIndividualSketchRegionQuery-140", "featureId":"drawn-profile",
                "deterministicIds":["left-region", "overlap-region", "right-region"],
                "filterInnerLoops":filter,
                "queryString":format!("query = qSketchRegion(id + \"drawn-profile\", {filter});")});
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model)[1], "extrude", "{:?}", report.losses());
        assert_eq!(
            timeline(&model)["model"]["features"][1]["definition"]["value"]["value"]["profile"]
                ["value"]["kind"],
            "sketch"
        );
    }
}

fn filtered_region_bytes(edit: impl Fn(&mut Value)) -> Vec<u8> {
    changed(&fixture("default-plane-regions"), |path, value| {
        if !path.ends_with("features.json") {
            return;
        }
        region_fixture::frame(value, false);
        value["features"][1]["parameters"][2]["queries"][0] = json!({
            "btType":"BTMIndividualSketchRegionQuery-140", "featureId":"drawn-profile",
            "deterministicIds":["outer-region"], "filterInnerLoops":true,
            "queryString":"query = qSketchRegion(id + \"drawn-profile\", true);"});
        edit(value);
    })
}

fn filtered_refusal(report: &Report, detail: &str) -> bool {
    report.losses().iter().any(|loss| {
        loss.code.as_deref() == Some("onshape/history.not-supported")
            && loss.detail.contains(detail)
    })
}

#[test]
fn an_island_inside_a_filtered_hole_keeps_the_extrude_native() {
    // The island lies inside the hole circle, so it has two containing loops.
    let bytes = filtered_region_bytes(|value| {
        value["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .push(json!({
            "btType":"BTMSketchCurve-4","entityId":"island","geometry":{
            "btType":"BTCurveGeometryCircle-115","radius":0.001,"xCenter":0.005,
            "yCenter":0.01,"xDir":1.0}}));
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(kinds(&model)[1], "native", "{:?}", report.losses());
    assert!(
        filtered_refusal(&report, "nested more than one level"),
        "{:?}",
        report.losses()
    );
}

#[test]
fn a_filtered_query_that_is_not_a_region_query_keeps_the_extrude_native() {
    let bytes = filtered_region_bytes(|value| {
        let query = &mut value["features"][1]["parameters"][2]["queries"][0];
        query["btType"] = json!("BTMIndividualCreatedByQuery-137");
        query["deterministicIds"] = json!([]);
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(kinds(&model)[1], "native", "{:?}", report.losses());
    assert!(
        filtered_refusal(&report, "filtered inner sketch regions"),
        "{:?}",
        report.losses()
    );
}

mod planes;
