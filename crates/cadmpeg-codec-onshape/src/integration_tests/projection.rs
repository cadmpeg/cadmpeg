// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use std::io::{Cursor, Read, Write};

use serde_json::{json, Value};

use crate::reader::{self, id, query, topology};
use crate::test_support::projection::{base_fixture, changed, changed_fixture, selected};
use crate::test_support::review::parameter;
use crate::test_support::{fixture, Report};

pub(super) fn read(bytes: &[u8]) -> (Value, Report) {
    let mut report = Report::default();
    let model = crate::test_support::read_bytes(
        std::path::Path::new("synthetic.onshape-bundle"),
        bytes,
        &mut report,
    )
    .unwrap();
    let mut wire = cadmpeg_test_support::wire::in_memory_json(&model.ir)["model"].take();
    wire["features"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|f| f["ordinal"].as_u64());
    (wire, report)
}

// A deliberately simple literal encoder, independent of the fixture generator's
// dictionary/reference encoder. Only synthetic records pass through this helper.
pub(super) fn compact(value: &Value) -> String {
    match value {
        Value::String(s) => format!("S{:x}${s}", s.len()),
        Value::Number(n) => {
            let n = n.as_i64().unwrap();
            if n < 0 {
                format!("D-{:x}", n.unsigned_abs())
            } else {
                format!("D{n:x}")
            }
        }
        Value::Bool(b) => if *b { "T" } else { "F" }.to_owned(),
        Value::Array(a) => format!(
            "A{:x}{}",
            a.len(),
            a.iter().map(compact).collect::<String>()
        ),
        Value::Object(o) => format!(
            "M{:x}{}",
            o.len(),
            o.iter()
                .map(|(k, v)| compact(&json!(k)) + &compact(v))
                .collect::<String>()
        ),
        Value::Null => panic!("unsupported synthetic compact value"),
    }
}

pub(super) fn replace_query(query: &mut Value, value: &Value) {
    query["queryString"] = json!(format!(
        "query=qCompressed(1.0,{},id);",
        cadmpeg_core::serde_erase::json::to_string(&format!("%{}", compact(value))).unwrap()
    ));
}

pub(super) fn kinds(wire: &Value) -> Vec<&str> {
    wire["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["definition"]["value"]["kind"].as_str().unwrap())
        .collect()
}

#[test]
fn real_shaped_synthetic_bundle_resolves_plane_and_selected_region() {
    let (wire, report) = read(&base_fixture());
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    assert!(
        report.losses().iter().all(|loss| !loss
            .code
            .as_deref()
            .unwrap_or("")
            .starts_with("onshape/")),
        "{:?}",
        report.losses()
    );
    assert_eq!(
        wire["sketches"][0]["placement"]["frame"]["normal"],
        json!({"x":0.0,"y":0.0,"z":1.0})
    );
    let profile = &wire["features"][1]["definition"]["value"]["value"]["profile"]["value"];
    assert_eq!(profile["kind"], "sketch_regions");
    assert_eq!(
        profile["value"]["regions"][0]["outer_boundary"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        wire["features"][1]["definition"]["value"]["value"]["extent"]["side"]["termination"]
            ["length"],
        5.0
    );
}

#[test]
fn compressed_default_planes_resolve_their_own_full_frames() {
    for (plane, normal, u) in [
        ("Top", [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ("Front", [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
        ("Right", [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ] {
        let bytes = changed(|response| {
            let query = &mut response["features"][0]["parameters"][0]["queries"][0];
            let mut value = query::resolved(query).unwrap();
            value["operationId"] = json!([format!("{plane}.planeOp")]);
            replace_query(query, &value);
        });
        let (wire, report) = read(&bytes);
        assert_eq!(
            kinds(&wire),
            ["sketch", "extrude"],
            "{plane}: {:?}",
            report.losses()
        );
        let frame = &wire["sketches"][0]["placement"]["frame"];
        assert_eq!(frame["origin"], topology::point([0.0; 3]));
        assert_eq!(frame["normal"], topology::point(normal));
        assert_eq!(frame["u_axis"], topology::point(u));
    }
}

#[test]
fn several_regions_keep_their_boundaries_and_never_expand_to_the_whole_sketch() {
    let bytes = changed(|response| {
        let mut second = response["features"][0]["entities"].as_array().unwrap()[..4].to_vec();
        for entity in &mut second {
            for field in ["entityId", "startPointId", "endPointId"] {
                entity[field] = json!(format!("second.{}", entity[field].as_str().unwrap()));
            }
            entity["geometry"]["pntX"] = json!(entity["geometry"]["pntX"].as_f64().unwrap() + 0.03);
        }
        response["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .extend(second);
        let first = selected(response);
        let mut value = query::resolved(first).unwrap();
        for boundary in value["disambiguationData"][0]["entities"]
            .as_array_mut()
            .unwrap()
        {
            let entity = &mut boundary[0]["derivedFrom"]["sketchEntityId"];
            *entity = json!(format!("second.{}", entity.as_str().unwrap()));
        }
        let mut query = first.clone();
        query["deterministicIds"] = json!(["second-region"]);
        replace_query(&mut query, &value);
        response["features"][1]["parameters"][2]["queries"]
            .as_array_mut()
            .unwrap()
            .push(query);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let profile = &wire["features"][1]["definition"]["value"]["value"]["profile"]["value"];
    assert_eq!(profile["kind"], "sketch_regions");
    assert_eq!(profile["value"]["regions"].as_array().unwrap().len(), 2);
}

#[test]
fn ambiguous_region_queries_get_specific_losses_and_keep_the_sketch() {
    for (case, reason) in [
        ("source", "another sketch"),
        ("entity", "no solved source curve"),
        ("repeat", "repeats a source curve"),
        ("ids", "one boundary description"),
        ("disambiguation", "unique boundary"),
        ("side", "resolved side"),
        ("empty", "no resolved boundary edges"),
    ] {
        let bytes = changed(|response| {
            let query = selected(response);
            let mut value = query::resolved(query).unwrap();
            let boundaries = &mut value["disambiguationData"][0]["entities"];
            match case {
                "source" => {
                    boundaries[0][0]["derivedFrom"]["operationId"] = json!(["different.wireOp"]);
                }
                "entity" => boundaries[0][0]["derivedFrom"]["sketchEntityId"] = json!("absent"),
                "repeat" => {
                    let repeated = boundaries[0].clone();
                    boundaries.as_array_mut().unwrap().push(repeated);
                }
                "ids" => query["deterministicIds"] = json!(["one", "two"]),
                "disambiguation" => value["disambiguationData"] = json!([]),
                "side" => boundaries[0][1] = json!(0),
                "empty" => *boundaries = json!([]),
                _ => panic!("unknown test case"),
            }
            replace_query(query, &value);
        });
        let (wire, report) = read(&bytes);
        assert_eq!(kinds(&wire), ["sketch", "native"]);
        assert!(
            report
                .losses()
                .iter()
                .any(|loss| loss.detail.contains(reason)),
            "{case}: {:?}",
            report.losses()
        );
    }
}

#[test]
fn a_selected_region_excludes_other_closed_geometry_of_its_sketch() {
    let bytes = changed(|response| {
        let mut other = response["features"][0]["entities"].as_array().unwrap()[..4].to_vec();
        for entity in &mut other {
            for field in ["entityId", "startPointId", "endPointId"] {
                entity[field] = json!(format!("unused.{}", entity[field].as_str().unwrap()));
            }
            entity["geometry"]["pntX"] = json!(entity["geometry"]["pntX"].as_f64().unwrap() + 0.03);
        }
        response["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .extend(other);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let profile = &wire["features"][1]["definition"]["value"]["value"]["profile"]["value"];
    let boundary = profile["value"]["regions"][0]["outer_boundary"]
        .as_array()
        .unwrap();
    assert_eq!(boundary.len(), 4);
    assert!(boundary
        .iter()
        .all(|b| !b["entity"].as_str().unwrap().contains("unused")));
}

#[test]
fn a_region_can_keep_an_outer_circle_and_an_inner_hole() {
    let bytes = changed(|response| {
        response["features"][0]["entities"] = json!([
            {"btType":"BTMSketchCurve-4", "entityId":"outer", "geometry":{"btType":"BTCurveGeometryCircle-115", "xCenter":0.01, "yCenter":0.01, "radius":0.01}},
            {"btType":"BTMSketchCurve-4", "entityId":"inner", "geometry":{"btType":"BTCurveGeometryCircle-115", "xCenter":0.01, "yCenter":0.01, "radius":0.005}}
        ]);
        let query = selected(response);
        let mut value = query::resolved(query).unwrap();
        let mut boundary = value["disambiguationData"][0]["entities"][0].clone();
        boundary[0]["derivedFrom"]["sketchEntityId"] = json!("outer");
        let mut hole = boundary.clone();
        hole[0]["derivedFrom"]["sketchEntityId"] = json!("inner");
        hole[1] = json!(-1);
        value["disambiguationData"][0]["entities"] = json!([hole, boundary]);
        replace_query(query, &value);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let region = &wire["features"][1]["definition"]["value"]["value"]["profile"]["value"]["value"]
        ["regions"][0];
    assert!(region["outer_boundary"][0]["entity"]
        .as_str()
        .unwrap()
        .ends_with("outer"));
    assert_eq!(region["hole_boundaries"].as_array().unwrap().len(), 1);
}

#[test]
fn adjacent_regions_keep_the_shared_edge_in_each_directed_boundary() {
    let bytes = changed(|response| {
        let line = |name: &str, x, y, dx, dy| {
            json!({
                "btType":"BTMSketchCurveSegment-155", "entityId":name,
                "startParam":0.0, "endParam":0.01,
                "geometry":{"btType":"BTCurveGeometryLine-117", "pntX":x, "pntY":y, "dirX":dx, "dirY":dy}
            })
        };
        response["features"][0]["entities"] = json!([
            line("bottom-left", 0.0, 0.0, 1.0, 0.0),
            line("bottom-right", 0.01, 0.0, 1.0, 0.0),
            line("right", 0.02, 0.0, 0.0, 1.0),
            line("top-right", 0.02, 0.01, -1.0, 0.0),
            line("top-left", 0.01, 0.01, -1.0, 0.0),
            line("left", 0.0, 0.01, 0.0, -1.0),
            line("divider", 0.01, 0.0, 0.0, 1.0)
        ]);
        let original = selected(response).clone();
        let resolved = query::resolved(&original).unwrap();
        let edge = resolved["disambiguationData"][0]["entities"][0].clone();
        let mut queries = Vec::new();
        for (i, names) in [
            ["bottom-left", "divider", "top-left", "left"],
            ["bottom-right", "right", "top-right", "divider"],
        ]
        .iter()
        .enumerate()
        {
            let mut value = resolved.clone();
            value["disambiguationData"][0]["entities"] = Value::Array(
                names
                    .iter()
                    .map(|name| {
                        let mut edge = edge.clone();
                        edge[0]["derivedFrom"]["sketchEntityId"] = json!(name);
                        if i == 1 && *name == "divider" {
                            edge[1] = json!(-1);
                        }
                        edge
                    })
                    .collect(),
            );
            let mut query = original.clone();
            query["deterministicIds"] = json!([format!("region-{i}")]);
            replace_query(&mut query, &value);
            queries.push(query);
        }
        response["features"][1]["parameters"][2]["queries"] = json!(queries);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let regions = wire["features"][1]["definition"]["value"]["value"]["profile"]["value"]["value"]
        ["regions"]
        .as_array()
        .unwrap();
    assert_eq!(regions.len(), 2);
    for region in regions {
        let boundary = region["outer_boundary"].as_array().unwrap();
        assert_eq!(boundary.len(), 4);
        assert_eq!(
            boundary
                .iter()
                .filter(|b| b["entity"].as_str().unwrap().ends_with("divider"))
                .count(),
            1
        );
    }
}

#[test]
fn two_sketch_dimensions_get_distinct_ordinals_within_their_feature() {
    let bytes = changed(|response| {
        let dimension = |id: &str, line: &str, expression: &str| {
            json!({"btType": "BTMSketchConstraint-2", "entityId": id, "constraintType": "LENGTH",
                "parameters": [
                    {"btType": "BTMParameterString-149", "parameterId": "localFirst", "value": line},
                    {"btType": "BTMParameterQuantity-147", "parameterId": "length", "expression": expression}]})
        };
        response["features"][0]["constraints"] = json!([
            dimension("length", "outline.side0", "20 mm"),
            dimension("width", "outline.side1", "10 mm")
        ]);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let parameters = wire["parameters"].as_array().unwrap();
    assert_eq!(parameters.len(), 2, "{parameters:?}");
    let mut ordinals: Vec<u64> = parameters
        .iter()
        .map(|p| p["ordinal"].as_u64().unwrap())
        .collect();
    ordinals.sort_unstable();
    assert_eq!(ordinals, [0, 1]);
    assert_eq!(parameters[0]["owner"], parameters[1]["owner"]);
}

#[test]
fn sketch_dimension_ordinals_restart_for_each_owner() {
    let bytes = changed(|response| {
        let first = &mut response["features"][0];
        first["constraints"] = json!([{
            "entityId": "length", "constraintType": "LENGTH", "parameters": [
                {"parameterId": "localFirst", "value": "outline.side0"},
                {"parameterId": "length", "expression": "20 mm"}
            ]
        }]);
        let mut other = first.clone();
        other["featureId"] = json!("otherSketch");
        other.as_object_mut().unwrap().remove("name");
        response["features"]
            .as_array_mut()
            .unwrap()
            .insert(1, other);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(
        kinds(&wire),
        ["sketch", "sketch", "extrude"],
        "{:?}",
        report.losses()
    );
    let parameters = wire["parameters"].as_array().unwrap();
    assert_eq!(parameters.len(), 2, "{parameters:?}");
    assert_ne!(parameters[0]["owner"], parameters[1]["owner"]);
    assert!(parameters.iter().all(|parameter| parameter["ordinal"] == 0));
    let unnamed = wire["sketches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sketch| sketch["id"] == id("sketch", "otherSketch"))
        .unwrap();
    assert!(unnamed.get("name").is_none());
}

#[test]
fn zlib_cap_queries_refuse_unresolved_attachment_frames() {
    use base64::Engine;
    let bytes = changed(|response| {
        let mut face_sketch = response["features"][0].clone();
        face_sketch["featureId"] = json!("cap-sketch");
        let creator = response["features"][1]["featureId"].as_str().unwrap();
        let cap = json!({"entityType":"FACE", "historyType":"CREATION", "queryType":"CAP_FACE",
            "operationId":[format!("{creator}.opExtrude")], "isStart":false});
        let literal = format!("%{}", compact(&cap));
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(literal.as_bytes()).unwrap();
        let encoded = format!(
            "&{:x}${}",
            literal.len(),
            base64::engine::general_purpose::STANDARD.encode(encoder.finish().unwrap())
        );
        face_sketch["parameters"][0]["queries"][0] = json!({"queryString":format!("query=qCompressed(1.0,{},id);", cadmpeg_core::serde_erase::json::to_string(&encoded).unwrap())});
        response["features"]
            .as_array_mut()
            .unwrap()
            .push(face_sketch);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude", "native"]);
    assert_eq!(wire["sketches"].as_array().unwrap().len(), 1);
    assert!(report.losses().iter().any(|l| l.code.as_deref()
        == Some("onshape/history.not-supported")
        && l.detail
            .contains("attachment translation and rotation unresolved")));
}

// A sketch on a side face: the zlib query names the face by its deterministic id
// in the rollback state before the sketch, which supplies the face's plane.
pub(super) fn face_sketch_bundle(normal: [f64; 3], orientation: bool) -> Vec<u8> {
    use base64::Engine;
    let bytes = changed(|response| {
        let mut face_sketch = response["features"][0].clone();
        face_sketch["featureId"] = json!("side-sketch");
        let creator = response["features"][1]["featureId"].as_str().unwrap();
        let side = json!({"entityType":"FACE", "historyType":"CREATION", "queryType":"SWEPT_FACE",
            "operationId":[format!("{creator}.opExtrude")]});
        let literal = format!("%{}", compact(&side));
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(literal.as_bytes()).unwrap();
        let encoded = format!(
            "&{:x}${}",
            literal.len(),
            base64::engine::general_purpose::STANDARD.encode(encoder.finish().unwrap())
        );
        face_sketch["parameters"][0]["queries"][0] = json!({"deterministicIds":["side"],
            "queryString":format!("query=qCompressed(1.0,{},id);", cadmpeg_core::serde_erase::json::to_string(&encoded).unwrap())});
        response["features"]
            .as_array_mut()
            .unwrap()
            .push(face_sketch);
    });
    let rollback = json!({"btType":"BTExportModelBodiesResponse-734",
        "microversionId":{"theId":"synthetic-microversion"},
        "bodies":[{"id":"body","faces":[{"id":"side","orientation":orientation,"loops":[],
            "surface":{"type":"PLANE","origin":topology::point([0.01, -0.02, 0.005]),
            "normal":topology::point(normal)}}]}]});
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let name = member.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        if name.ends_with("bodydetails/index.json") {
            bytes = b"[0, 1, 2]".to_vec();
        }
        output
            .start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output
        .start_file(
            "partstudios/studio/bodydetails/2.json",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    output
        .write_all(&cadmpeg_core::serde_erase::json::to_vec(&rollback).unwrap())
        .unwrap();
    output.finish().unwrap().into_inner()
}

#[test]
fn zlib_face_queries_refuse_plane_only_attachment_frames() {
    for normal in [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        for orientation in [false, true] {
            let (wire, report) = read(&face_sketch_bundle(normal, orientation));
            assert_eq!(kinds(&wire), ["sketch", "extrude", "native"]);
            assert_eq!(wire["sketches"].as_array().unwrap().len(), 1);
            assert!(report.losses().iter().any(|l| l
                .detail
                .contains("attachment translation and rotation unresolved")));
        }
    }
}

#[test]
fn face_sketches_on_unmeasured_orientations_are_refused_by_name() {
    let (wire, report) = read(&face_sketch_bundle([0.6, 0.0, 0.8], true));
    assert_eq!(kinds(&wire)[2], "native");
    assert!(
        report.losses().iter().any(|loss| loss
            .detail
            .contains("attachment translation and rotation unresolved")),
        "{:?}",
        report.losses()
    );
}

#[test]
fn revolve_uses_a_construction_line_in_the_sketch_frame() {
    let mut native = String::new();
    let bytes = changed_fixture(&fixture("revolve"), |response| {
        native = response["features"][0]["featureId"]
            .as_str()
            .unwrap()
            .to_owned();
        response["features"][0]["entities"].as_array_mut().unwrap().push(json!({"btType":"BTMSketchCurveSegment-155","entityId":"axis-line","isConstruction":true,
            "geometry":{"btType":"BTCurveGeometryLine-117","dirY":1.0},"startParam":0.0,"endParam":0.015}));
        let query = json!({"entityType":"EDGE","historyType":"CREATION","queryType":"SKETCH_ENTITY","operationId":[format!("{native}.wireOp")],"sketchEntityId":"axis-line"});
        let axis = response["features"][1]["parameters"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| p["parameterId"] == "axis")
            .unwrap();
        replace_query(&mut axis["queries"][0], &query);
        response["features"][1]["parameters"]
            .as_array_mut()
            .unwrap()
            .retain(|p| p["parameterId"] != "revolveType");
        response["features"][1]["parameters"]
            .as_array_mut()
            .unwrap()
            .push(json!({"parameterId":"fullRevolve","value":true}));
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "revolve"], "{:?}", report.losses());
    assert_eq!(
        wire["features"][1]["definition"]["value"]["value"]["construction"]["value"]["axis"]
            ["direction"],
        topology::point([0.0, 1.0, 0.0])
    );
    assert_eq!(
        wire["features"][1]["definition"]["value"]["value"]["construction"]["value"]["axis"]
            ["reference"],
        json!({"kind":"sketch_curves","value":{"sketch":reader::id("sketch", &native),
            "curves":[reader::id("entity", &format!("{native}/axis-line"))]}})
    );
}

#[test]
fn origin_coincidence_and_midpoint_keep_explicit_point_relations() {
    let bytes = changed(|response| {
        response["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .extend([
                json!({"btType":"BTMSketchPoint-158","entityId":"zero"}),
                json!({"btType":"BTMSketchPoint-158","entityId":"left-point","x":-0.004}),
                json!({"btType":"BTMSketchPoint-158","entityId":"right-point","x":0.004}),
            ]);
        response["features"][0]["constraints"] = json!([
            {"constraintType":"COINCIDENT","entityId":"origin-relation","parameters":[
                {"parameterId":"localFirst","value":"zero"},{"parameterId":"externalSecond","queries":[{"queryString":"qOrigin(EntityType.VERTEX);"}]}]},
            {"constraintType":"MIDPOINT","entityId":"middle-relation","parameters":[
                {"parameterId":"localMidpoint","value":"zero"},{"parameterId":"localEntity1","value":"left-point"},{"parameterId":"localEntity2","value":"right-point"}]}
        ]);
    });
    let (wire, report) = read(&bytes);
    let constraints = wire["sketch_constraints"].as_array().unwrap();
    assert!(
        constraints
            .iter()
            .any(|c| c["definition"]["kind"] == "point_coordinate_values"),
        "{:?}",
        report.losses()
    );
    assert!(
        constraints
            .iter()
            .any(|c| c["definition"]["kind"] == "midpoint"),
        "{:?}",
        report.losses()
    );
}

#[test]
fn variables_keep_dependencies_and_drive_a_symmetric_extrusion() {
    let bytes = changed(|response| {
        let rows = response["features"].as_array_mut().unwrap();
        rows[1]["parameters"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| p["parameterId"] == "depth")
            .unwrap()["expression"] = json!("2 * (#span + 1 mm)");
        rows[1]["parameters"]
            .as_array_mut()
            .unwrap()
            .push(json!({"parameterId":"symmetric","value":true}));
        rows.insert(0,json!({"featureId":"spacing-variable","featureType":"assignVariable","parameters":[
            {"parameterId":"mode","value":"ASSIGNED"},{"parameterId":"variableType","value":"LENGTH"},
            {"parameterId":"name","value":"span"},{"parameterId":"lengthValue","expression":"3 mm"}]}));
    });
    let (wire, report) = read(&bytes);
    assert_eq!(
        kinds(&wire),
        ["tree_node", "sketch", "extrude"],
        "{:?}",
        report.losses()
    );
    let extrude = &wire["features"][2]["definition"]["value"]["value"];
    assert_eq!(extrude["extent"]["kind"], "symmetric");
    assert_eq!(extrude["extent"]["side"]["termination"]["length"], 8.0);
    let owned = wire["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| {
            p.pointer("/properties/source_kind").and_then(Value::as_str) == Some("TotalDistance")
        })
        .unwrap();
    assert_eq!(owned["value"]["value"], 8.0);
    assert!(!report
        .losses()
        .iter()
        .any(|l| l.code.as_deref() == Some("onshape/history.variable-evaluated")));
    assert_eq!(
        owned["dependencies"],
        json!([id("parameter", "spacing-variable")])
    );
    assert!(owned["expression"].as_str().unwrap().contains("span"));
}

#[test]
fn compressed_hole_locations_keep_only_the_selected_sketch_point() {
    let bytes = changed(|response| {
        let sketch = response["features"][0]["featureId"]
            .as_str()
            .unwrap()
            .to_owned();
        response["features"][0]["entities"].as_array_mut().unwrap().extend([
            json!({"btType":"BTMSketchPoint-158","entityId":"chosen-point","x":0.002,"y":0.003}),
            json!({"btType":"BTMSketchPoint-158","entityId":"other-point","x":0.004,"y":0.003})]);
        let mut query = json!({});
        replace_query(
            &mut query,
            &json!({"entityType":"VERTEX","historyType":"CREATION","queryType":"SKETCH_ENTITY", "operationId":[format!("{sketch}.wireOp")],"sketchEntityId":"chosen-point"}),
        );
        response["features"].as_array_mut().unwrap().push(json!({"featureId":"hole","featureType":"hole","parameters":[
            {"parameterId":"locations","queries":[query]}, {"parameterId":"style","value":"SIMPLE"},
            {"parameterId":"endStyle","value":"THROUGH"},{"parameterId":"holeDiameter","expression":"1 mm"}]}));
        response["rollbackIndex"] = json!(response["features"].as_array().unwrap().len());
    });
    let (wire, report) = read(&bytes);
    assert_eq!(
        kinds(&wire),
        ["sketch", "extrude", "hole"],
        "{:?}",
        report.losses()
    );
    let profile = &wire["features"][2]["definition"]["value"]["value"]["profile"];
    assert_eq!(profile["kind"], "sketch_entities");
    assert_eq!(profile["value"]["entities"].as_array().unwrap().len(), 1);
    assert!(profile["value"]["entities"][0]
        .as_str()
        .unwrap()
        .ends_with("chosen-point"));
}

#[test]
fn the_origin_named_by_its_deterministic_id_holds_a_point_at_the_sketch_origin() {
    let constrain = |x: f64| {
        changed(|response| {
            response["features"][0]["entities"]
                .as_array_mut()
                .unwrap()
                .push(json!({"btType":"BTMSketchPoint-158","entityId":"zero","x":x}));
            response["features"][0]["constraints"] = json!([{"constraintType":"COINCIDENT","entityId":"origin-relation","parameters":[
                {"parameterId":"localFirst","value":"zero"},{"parameterId":"externalSecond","queries":[{"queryString":"","deterministicIds":["IB"]}]}]}]);
        })
    };
    let (wire, report) = read(&constrain(0.0));
    let held = wire["sketch_constraints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["definition"]["kind"] == "point_coordinate_values")
        .unwrap_or_else(|| panic!("{:?}", report.losses()));
    assert_eq!(held["definition"]["values"], json!([0.0, 0.0]));
    assert!(!report
        .losses()
        .iter()
        .any(|l| l.code.as_deref() == Some("onshape/sketch.constraint-not-supported")));
    // A point the source solve left elsewhere is not tied to the origin.
    let (wire, report) = read(&constrain(0.002));
    assert!(!wire["sketch_constraints"]
        .as_array()
        .is_some_and(|rows| rows
            .iter()
            .any(|c| c["definition"]["kind"] == "point_coordinate_values")));
    assert!(report.losses().iter().any(|l| l.code.as_deref()
        == Some("onshape/sketch.constraint-not-supported")
        && l.detail.contains("not at the origin")));
}

#[test]
fn a_midpoint_between_arc_ends_keeps_its_carrier() {
    let bytes = changed(|response| {
        response["features"][0]["entities"].as_array_mut().unwrap().extend([
            json!({"btType":"BTMSketchCurveSegment-155","entityId":"bow","startPointId":"bow.start","endPointId":"bow.end",
                "startParam":0.0,"endParam":std::f64::consts::PI,
                "geometry":{"btType":"BTCurveGeometryCircle-115","radius":0.003,"xDir":1.0}}),
            json!({"btType":"BTMSketchPoint-158","entityId":"middle"}),
        ]);
        response["features"][0]["constraints"] = json!([
            {"constraintType":"MIDPOINT","entityId":"middle-relation","parameters":[
                {"parameterId":"localMidpoint","value":"middle"},{"parameterId":"localEntity1","value":"bow.start"},{"parameterId":"localEntity2","value":"bow.end"}]}
        ]);
    });
    let (wire, report) = read(&bytes);
    let rows = wire["sketch_constraints"].as_array().unwrap();
    assert!(
        rows.iter().any(|c| c["definition"]["kind"] == "midpoint"),
        "{:?}",
        report.losses()
    );
    let carrier = wire["sketch_entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"].as_str().unwrap().contains("midpoint"))
        .unwrap();
    let start = &carrier["geometry"]["start"];
    let end = &carrier["geometry"]["end"];
    assert!(
        (start["u"].as_f64().unwrap() - 3.0).abs() < 1e-9,
        "{carrier}"
    );
    assert!((end["u"].as_f64().unwrap() + 3.0).abs() < 1e-9, "{carrier}");
    assert!(start["v"].as_f64().unwrap().abs() < 1e-9 && end["v"].as_f64().unwrap().abs() < 1e-9);
}

#[test]
fn an_opaque_external_id_is_not_assumed_to_be_the_origin() {
    let bytes = changed(|response| {
        response["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .push(json!({"btType":"BTMSketchPoint-158","entityId":"zero"}));
        response["features"][0]["constraints"] = json!([{"constraintType":"COINCIDENT","entityId":"external-relation","parameters":[
            {"parameterId":"localFirst","value":"zero"},{"parameterId":"externalSecond","queries":[{"queryString":"","deterministicIds":["unidentified-reference"]}]}]}]);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"]);
    assert!(report.losses().iter().any(|l| l.code.as_deref()
        == Some("onshape/sketch.constraint-not-supported")
        && l.detail.contains("does not identify the sketch origin")));
}

#[test]
fn ellipse_regions_keep_exact_angular_boundaries_and_holes() {
    let bytes = changed(|response| {
        response["features"][0]["entities"] = json!([
            {"btType":"BTMSketchCurve-4","entityId":"outer","geometry":{
                "btType":"BTCurveGeometryEllipse-1189","xCenter":0.01,"yCenter":0.01,
                "xDir":0.0,"yDir":1.0,"radius":0.009,"minorRadius":0.006}},
            {"btType":"BTMSketchCurve-4","entityId":"inner","geometry":{
                "btType":"BTCurveGeometryEllipse-1189","xCenter":0.01,"yCenter":0.01,
                "xDir":0.0,"yDir":1.0,"radius":0.003,"minorRadius":0.002}}
        ]);
        let query = selected(response);
        let mut value = query::resolved(query).unwrap();
        let mut outer = value["disambiguationData"][0]["entities"][0].clone();
        outer[0]["derivedFrom"]["sketchEntityId"] = json!("outer");
        let mut inner = outer.clone();
        inner[0]["derivedFrom"]["sketchEntityId"] = json!("inner");
        inner[1] = json!(-1);
        value["disambiguationData"][0]["entities"] = json!([inner, outer]);
        replace_query(query, &value);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let region = &wire["features"][1]["definition"]["value"]["value"]["profile"]["value"]["value"]
        ["regions"][0];
    assert_eq!(
        region["outer_boundary"][0]["parameter_range"],
        json!([0.0, std::f64::consts::TAU])
    );
    assert!(region["outer_boundary"][0]["entity"]
        .as_str()
        .unwrap()
        .ends_with("outer"));
    assert_eq!(region["hole_boundaries"].as_array().unwrap().len(), 1);
}

#[test]
fn elliptical_arc_regions_keep_the_source_interval_and_closing_chord() {
    let span = 4.0_f64;
    let bytes = changed(|response| {
        let start = [0.006, 0.0];
        let end = [0.006 * span.cos(), 0.002 * span.sin()];
        let (dx, dy) = (start[0] - end[0], start[1] - end[1]);
        let length = dx.hypot(dy);
        response["features"][0]["entities"] = json!([
            {"btType":"BTMSketchCurveSegment-155","entityId":"outer","startParam":0.0,"endParam":span,
                "geometry":{"btType":"BTCurveGeometryEllipse-1189","xDir":1.0,"radius":0.006,"minorRadius":0.002}},
            {"btType":"BTMSketchCurveSegment-155","entityId":"chord","startParam":0.0,"endParam":length,
                "geometry":{"btType":"BTCurveGeometryLine-117","pntX":end[0],"pntY":end[1],"dirX":dx/length,"dirY":dy/length}}
        ]);
        let query = selected(response);
        let mut value = query::resolved(query).unwrap();
        let mut outer = value["disambiguationData"][0]["entities"][0].clone();
        outer[0]["derivedFrom"]["sketchEntityId"] = json!("outer");
        let mut chord = outer.clone();
        chord[0]["derivedFrom"]["sketchEntityId"] = json!("chord");
        value["disambiguationData"][0]["entities"] = json!([outer, chord]);
        replace_query(query, &value);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let boundary = wire["features"][1]["definition"]["value"]["value"]["profile"]["value"]["value"]
        ["regions"][0]["outer_boundary"]
        .as_array()
        .unwrap();
    assert_eq!(boundary.len(), 2);
    assert!(boundary
        .iter()
        .any(|b| b["parameter_range"] == json!([0.0, span])));
}

// Synthetic ellipse cut by two solved chords. Both sides of the middle region
// use the same ellipse carrier, with distinct imprint intervals across its seam.
fn repeated_ellipse_region(response: &mut Value) {
    response["features"][0]["entities"] = json!([
        {"btType":"BTMSketchCurve-4","entityId":"oval","geometry":{
            "btType":"BTCurveGeometryEllipse-1189","radius":0.010,"minorRadius":0.005,"xDir":1.0}},
        {"btType":"BTMSketchCurveSegment-155","entityId":"upper","startParam":0.0,"endParam":0.016,
            "geometry":{"btType":"BTCurveGeometryLine-117","pntX":-0.008,"pntY":0.003,"dirX":1.0}},
        {"btType":"BTMSketchCurveSegment-155","entityId":"lower","startParam":0.0,"endParam":0.016,
            "geometry":{"btType":"BTCurveGeometryLine-117","pntX":-0.008,"pntY":-0.003,"dirX":1.0}}
    ]);
    response["features"][0]["constraints"] = json!([]);
    let query = selected(response);
    let mut value = query::resolved(query).unwrap();
    let operation = value["operationId"][0]
        .as_str()
        .unwrap()
        .strip_suffix(".imprint")
        .unwrap()
        .to_owned();
    let source = |name| {
        json!({"entityType":"EDGE","historyType":"CREATION","queryType":"SKETCH_ENTITY",
        "operationId":[format!("{operation}.wireOp")],"sketchEntityId":name})
    };
    let vertex = |line, order, side| {
        json!([
        {"entityType":"VERTEX","historyType":"CREATION","queryType":"INTERSECT",
            "operationId":[format!("{operation}.imprint")],"derivedFrom":[source("oval"),source(line)],
            "disambiguationData":[{"disambiguationType":"ORDER","order":order}]},side])
    };
    let ellipse = |vertices| {
        json!([
        {"entityType":"EDGE","historyType":"CREATION","queryType":"IMPRINT",
            "operationId":[format!("{operation}.imprint")],"derivedFrom":source("oval"),
            "disambiguationData":[{"disambiguationType":"TOPOLOGY","entities":vertices}]},1])
    };
    let line = |name, side| {
        json!([
        {"entityType":"EDGE","historyType":"CREATION","queryType":"IMPRINT",
            "operationId":[format!("{operation}.imprint")],"derivedFrom":source(name)},side])
    };
    value["disambiguationData"][0]["entities"] = json!([
        ellipse(json!([vertex("upper", 1, 1), vertex("lower", 0, -1)])),
        ellipse(json!([vertex("lower", 1, 1), vertex("upper", 0, -1)])),
        line("upper", -1),
        line("lower", 1)
    ]);
    replace_query(query, &value);
}

#[test]
fn repeated_ellipse_carrier_uses_the_solved_chords_and_intersection_order() {
    let bytes = changed(repeated_ellipse_region);
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let boundary = wire["features"][1]["definition"]["value"]["value"]["profile"]["value"]["value"]
        ["regions"][0]["outer_boundary"]
        .as_array()
        .unwrap();
    assert_eq!(boundary.len(), 4);
    let ranges: Vec<_> = boundary
        .iter()
        .filter(|e| e["entity"].as_str().unwrap().ends_with("/oval"))
        .map(|e| {
            (
                e["parameter_range"][0].as_f64().unwrap(),
                e["parameter_range"][1].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(ranges.len(), 2, "{boundary:?}");
    let sweep: f64 = ranges.iter().map(|(a, b)| b - a).sum();
    assert!((sweep - 4.0 * 0.6_f64.asin()).abs() < 1e-12);
    assert!(ranges.iter().any(|(_, b)| *b > std::f64::consts::TAU));
}

#[test]
fn repeated_ellipse_intersections_refuse_ties_and_unsolved_endpoints() {
    for tie in [false, true] {
        let bytes = changed(|response| {
            repeated_ellipse_region(response);
            let chord = &mut response["features"][0]["entities"][1];
            if tie {
                chord["endParam"] = json!(0.0);
            } else {
                chord["geometry"]["pntY"] = json!(0.004);
            }
        });
        let (wire, report) = read(&bytes);
        assert_eq!(kinds(&wire), ["sketch", "native"]);
        assert!(
            report.losses().iter().any(|l| l.code.as_deref()
                == Some("onshape/history.not-supported")
                && l.detail.contains(if tie {
                    "order is ambiguous"
                } else {
                    "not a solved ellipse endpoint"
                })),
            "{:?}",
            report.losses()
        );
    }
}

#[test]
fn fractional_quantity_literals_use_the_unit_suffix_instead_of_the_default_zero() {
    for (expression, expected) in [
        ("3/8 in", 9.525),
        ("(3/8) in", 9.525),
        ("(2 + 1) / 4 mm", 0.75),
    ] {
        let bytes = changed(|response| {
            response["features"][1]["parameters"][4]["expression"] = json!(expression);
            response["features"][1]["parameters"][4]["value"] = json!(0.0);
        });
        let (wire, report) = read(&bytes);
        assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
        assert!(
            (wire["features"][1]["definition"]["value"]["value"]["extent"]["side"]["termination"]
                ["length"]
                .as_f64()
                .unwrap()
                - expected)
                .abs()
                < 1e-12
        );
    }
}

#[test]
fn outside_region_witnesses_recover_the_outline_and_unlisted_holes() {
    let bytes = changed(|response| {
        repeated_ellipse_region(response);
        let entities = response["features"][0]["entities"].as_array_mut().unwrap();
        for (i, (x, y, dx, dy, length)) in [
            (-0.02, -0.015, 1.0, 0.0, 0.04),
            (0.02, -0.015, 0.0, 1.0, 0.03),
            (0.02, 0.015, -1.0, 0.0, 0.04),
            (-0.02, 0.015, 0.0, -1.0, 0.03),
        ]
        .into_iter()
        .enumerate()
        {
            entities.push(json!({"btType":"BTMSketchCurveSegment-155","entityId":format!("frame{i}"),"startParam":0.0,"endParam":length,
                "geometry":{"btType":"BTCurveGeometryLine-117","pntX":x,"pntY":y,"dirX":dx,"dirY":dy}}));
        }
        for (name, x, y) in [("hole", 0.013, 0.006), ("separate", 0.06, 0.06)] {
            entities.push(json!({"btType":"BTMSketchCurve-4","entityId":name,
                "geometry":{"btType":"BTCurveGeometryCircle-115","radius":0.002,"xCenter":x,"yCenter":y,"xDir":1.0}}));
        }
        let query = selected(response);
        let mut value = query::resolved(query).unwrap();
        let mut witness = value["disambiguationData"][0]["entities"][0].clone();
        witness[1] = json!(-1);
        value["disambiguationData"][0]["entities"] = json!([witness]);
        replace_query(query, &value);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let region = &wire["features"][1]["definition"]["value"]["value"]["profile"]["value"]["value"]
        ["regions"][0];
    assert_eq!(region["outer_boundary"].as_array().unwrap().len(), 4);
    let holes = region["hole_boundaries"].as_array().unwrap();
    assert_eq!(holes.len(), 2, "{region}");
    assert!(holes
        .iter()
        .any(|h| h[0]["entity"].as_str().unwrap().ends_with("/hole")));
    assert!(holes
        .iter()
        .any(|h| h[0]["entity"].as_str().unwrap().ends_with("/oval")
            && h[0]["parameter_range"] == json!([0.0, std::f64::consts::TAU])));
}

#[test]
fn a_spline_segment_maps_only_over_its_whole_knot_domain() {
    for (end, whole) in [(2.0, true), (1.5, false)] {
        let bytes = changed(|response| {
            response["features"][0]["entities"].as_array_mut().unwrap().push(json!({
                "btType":"BTMSketchCurveSegment-155","entityId":"wave","startPointId":"wave.start","endPointId":"wave.end",
                "startParam":0.0,"endParam":end,
                "geometry":{"btType":"BTCurveGeometrySpline-118","isPeriodic":false,"isRational":false,"dimension":2,"degree":3,
                    "controlPointCount":4,"controlPoints":[0.0,0.03,0.005,0.035,0.01,0.03,0.015,0.035],
                    "knots":[0.0,0.0,0.0,0.0,2.0,2.0,2.0,2.0]}}));
        });
        let (wire, report) = read(&bytes);
        if whole {
            assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
            assert!(wire["sketch_entities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["geometry"]["kind"] == "nurbs"));
        } else {
            assert_eq!(kinds(&wire), ["sketch", "extrude"]);
            assert!(wire["sketch_entities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["geometry"]["kind"] == "line"));
            assert!(wire["sketch_entities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["geometry"]["kind"] == "native"));
            assert!(report
                .losses()
                .iter()
                .any(|l| l.code.as_deref() == Some("onshape/sketch.geometry-not-supported")));
            assert!(report.losses().iter().any(|l| l
                .detail
                .contains("trimmed sketch splines are not supported yet")));
        }
    }
}

#[test]
fn a_sketcher_pick_of_the_origin_point_is_a_coincidence_with_the_origin() {
    let bytes = changed(|response| {
        response["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .push(json!({"btType":"BTMSketchPoint-158","entityId":"zero","x":0.0,"y":0.0}));
        response["features"][0]["constraints"] = json!([{"constraintType":"COINCIDENT","entityId":"origin-pick","parameters":[
            {"parameterId":"localFirst","value":"zero"},
            {"parameterId":"externalSecond","queries":[{"btType":"BTMIndividualQuery-138","queryString":"","deterministicIds":["IB"]}]}]}]);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"]);
    assert!(
        report
            .losses()
            .iter()
            .all(|l| l.code.as_deref() != Some("onshape/sketch.constraint-not-supported")),
        "{:?}",
        report.losses()
    );
    assert!(wire["sketch_constraints"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["definition"]["kind"] == "point_coordinate_values"));
}

#[test]
fn basic_sketch_constraint_kinds_keep_their_neutral_relations() {
    let bytes = changed(|response| {
        response["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .extend([
                json!({"btType":"BTMSketchCurve-4","entityId":"round","centerId":"round.center",
                "geometry":{"btType":"BTCurveGeometryCircle-115","radius":0.007}}),
                json!({"btType":"BTMSketchPoint-158","entityId":"middle","x":0.010}),
            ]);
        response["features"][0]["constraints"] = json!([
            {"constraintType":"HORIZONTAL","entityId":"horizontal","parameters":[
                {"parameterId":"localFirst","value":"outline.side0"}]},
            {"constraintType":"VERTICAL","entityId":"vertical","parameters":[
                {"parameterId":"localFirst","value":"outline.side1"}]},
            {"constraintType":"COINCIDENT","entityId":"corner","parameters":[
                {"parameterId":"localFirst","value":"line0.end"},{"parameterId":"localSecond","value":"line1.start"}]},
            {"constraintType":"LENGTH","entityId":"length","parameters":[
                {"parameterId":"localFirst","value":"outline.side0"},{"parameterId":"length","expression":"20 mm"}]},
            {"constraintType":"DIAMETER","entityId":"diameter","parameters":[
                {"parameterId":"localFirst","value":"round"},{"parameterId":"length","expression":"14 mm"}]},
            {"constraintType":"MIDPOINT","entityId":"middle-relation","parameters":[
                {"parameterId":"localMidpoint","value":"middle"},{"parameterId":"localEntity1","value":"line0.start"},{"parameterId":"localEntity2","value":"line0.end"}]}
        ]);
    });
    let (wire, report) = read(&bytes);
    assert!(
        !report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("onshape/sketch.constraint-not-supported")),
        "{:?}",
        report.losses()
    );
    let mut definitions: Vec<_> = wire["sketch_constraints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["definition"]["kind"].as_str().unwrap())
        .collect();
    definitions.sort_unstable();
    assert_eq!(
        definitions,
        [
            "coincident_loci",
            "coincident_loci",
            "coincident_loci",
            "diameter",
            "distance",
            "horizontal",
            "midpoint",
            "vertical"
        ]
    );
    let parameters = wire["parameters"].as_array().unwrap();
    for value in [20.0, 14.0] {
        assert!(parameters.iter().any(|p| p["value"]["value"] == value));
    }
}

#[test]
fn a_dropped_constraint_names_its_kind_even_when_an_operand_is_missing() {
    let bytes = changed(|response| {
        response["features"][0]["constraints"] = json!([
            {"constraintType":"HORIZONTAL","entityId":"missing-line","parameters":[
                {"parameterId":"localFirst","value":"absent"}]}
        ]);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"]);
    assert!(report.losses().iter().any(|loss| loss.code.as_deref()
        == Some("onshape/sketch.constraint-not-supported")
        && loss
            .detail
            .starts_with("HORIZONTAL sketch constraint is not supported yet:")
        && loss.detail.contains("kept the solved sketch geometry")));
}

/// `fixture` with `variables` placed before its features. Every later feature moves down by
/// as many rollback positions, so the body details captured at those rollbacks move with it.
fn with_variables(
    fixture: &[u8],
    variables: &[Value],
    mut edit: impl FnMut(&mut Value),
) -> Vec<u8> {
    let shift = cadmpeg_core::decode::u64_from_index(variables.len());
    let mut archive = zip::ZipArchive::new(Cursor::new(fixture)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let mut name = member.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        if name.ends_with("features.json") {
            let mut response: Value = cadmpeg_core::serde_erase::json::from_slice(&bytes).unwrap();
            edit(&mut response);
            let rows = response["features"].as_array_mut().unwrap();
            for (at, row) in variables.iter().enumerate() {
                rows.insert(at, row.clone());
            }
            bytes = cadmpeg_core::serde_erase::json::to_vec(&response).unwrap();
        } else if name.ends_with("bodydetails/index.json") {
            let indices: Vec<u64> = cadmpeg_core::serde_erase::json::from_slice(&bytes).unwrap();
            let moved: Vec<u64> = indices.iter().map(|i| i + shift).collect();
            bytes = cadmpeg_core::serde_erase::json::to_vec(&moved).unwrap();
        } else if let Some((folder, file)) = name.rsplit_once("bodydetails/") {
            let index: u64 = file.strip_suffix(".json").unwrap().parse().unwrap();
            name = format!("{folder}bodydetails/{}.json", index + shift);
        }
        output
            .start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap().into_inner()
}

fn variable(feature: &str, name: &str, kind: &str, key: &str, expression: &str) -> Value {
    json!({"featureId":feature,"featureType":"assignVariable","parameters":[
        {"parameterId":"mode","value":"ASSIGNED"},{"parameterId":"variableType","value":kind},
        {"parameterId":"name","value":name},{"parameterId":key,"expression":expression}]})
}

#[test]
fn a_variable_revolve_angle_becomes_a_parameter_the_revolve_owns() {
    let bytes = with_variables(
        &fixture("revolve"),
        &[variable(
            "turn-variable",
            "turn",
            "ANGLE",
            "angleValue",
            "180 deg",
        )],
        |response| {
            let parameters = response["features"][1]["parameters"]
                .as_array_mut()
                .unwrap();
            for p in parameters.iter_mut() {
                if p["parameterId"] == "revolveType" {
                    p["value"] = json!("ONE_DIRECTION");
                }
            }
            parameters.push(json!({"parameterId":"angle","expression":"#turn / 2"}));
        },
    );
    let (wire, report) = read(&bytes);
    assert_eq!(
        kinds(&wire),
        ["tree_node", "sketch", "revolve"],
        "{:?}",
        report.losses()
    );
    let owned = wire["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| {
            p.pointer("/properties/source_kind").and_then(Value::as_str) == Some("AlongAngle")
        })
        .unwrap();
    assert!(
        (owned["value"]["value"].as_f64().unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    );
    assert_eq!(owned["expression"], "(turn / 2)");
    assert_eq!(
        owned["dependencies"],
        json!([id("parameter", "turn-variable")])
    );
}

#[test]
fn suppressed_features_and_variables_do_not_report_variable_loss() {
    for suppression in ["suppressed", "parentSuppressed"] {
        for suppress_variable in [false, true] {
            let mut round = variable("round-variable", "round", "LENGTH", "lengthValue", "1 mm");
            round[suppression] = json!(suppress_variable);
            let bytes = with_variables(&fixture("plate-fillet"), &[round], |response| {
                let fillet = &mut response["features"][2];
                fillet[suppression] = json!(true);
                for p in fillet["parameters"].as_array_mut().unwrap() {
                    if p["parameterId"] == "radius" {
                        p["expression"] = json!("#round");
                    }
                }
            });
            let (wire, report) = read(&bytes);
            assert_eq!(wire["features"][0]["suppressed"], suppress_variable);
            assert_eq!(wire["features"][3]["suppressed"], true);
            assert_eq!(
                wire["parameters"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| { p["id"] == id("parameter", "round-variable") }),
                !suppress_variable
            );
            assert!(
                !report.losses().iter().any(|loss| loss
                    .code
                    .as_deref()
                    .is_some_and(|code| code.starts_with("onshape/history."))),
                "{suppression}, suppress_variable={suppress_variable}: {:?}",
                report.losses()
            );
        }
    }
}

#[test]
fn a_variable_value_no_parameter_drives_is_a_named_loss() {
    let bytes = with_variables(
        &fixture("plate-fillet"),
        &[variable(
            "round-variable",
            "round",
            "LENGTH",
            "lengthValue",
            "1 mm",
        )],
        |response| {
            for p in response["features"][2]["parameters"]
                .as_array_mut()
                .unwrap()
            {
                if p["parameterId"] == "radius" {
                    p["expression"] = json!("#round");
                }
            }
        },
    );
    let (wire, report) = read(&bytes);
    assert_eq!(
        kinds(&wire),
        ["tree_node", "sketch", "extrude", "fillet"],
        "{:?}",
        report.losses()
    );
    let losses: Vec<_> = report
        .losses()
        .iter()
        .filter(|l| l.code.as_deref() == Some("onshape/history.variable-evaluated"))
        .collect();
    assert_eq!(losses.len(), 1, "{:?}", report.losses());
    assert!(losses[0]
        .detail
        .starts_with("radius uses Part Studio variables"));
}

#[test]
fn directed_regions_reject_open_separated_crossing_and_reversed_boundaries() {
    for case in ["open", "gap", "disjoint", "nested", "reversed", "crossing"] {
        let bytes = changed(|response| {
            response["features"][0]["constraints"] = json!([]);
            let query_template = query::resolved(selected(response)).unwrap();
            let edge = query_template["disambiguationData"][0]["entities"][0][0].clone();
            let mut resolved = query_template;
            let mut boundaries = Vec::new();
            if matches!(case, "disjoint" | "nested") {
                response["features"][0]["entities"] = json!([
                    {"btType":"BTMSketchCurve-4","entityId":"outer","geometry":{"btType":"BTCurveGeometryCircle-115","radius":0.01}},
                    {"btType":"BTMSketchCurve-4","entityId":"inner","geometry":{"btType":"BTCurveGeometryCircle-115","radius":0.003,"xCenter":if case == "disjoint" {0.03} else {0.0}}}]);
                for (name, side) in [("outer", 1), ("inner", -1)] {
                    let mut edge = edge.clone();
                    edge["derivedFrom"]["sketchEntityId"] = json!(name);
                    boundaries.push(json!([edge, side]));
                }
            } else if case == "open" {
                response["features"][0]["entities"] = json!([{"btType":"BTMSketchCurveSegment-155","entityId":"open","startParam":0.0,"endParam":1.0,
                    "geometry":{"btType":"BTCurveGeometryLine-117","pntX":1.0,"dirY":1.0}}]);
                let mut edge = edge.clone();
                edge["derivedFrom"]["sketchEntityId"] = json!("open");
                boundaries.push(json!([edge, 1]));
            } else {
                boundaries = resolved["disambiguationData"][0]["entities"]
                    .as_array()
                    .unwrap()
                    .clone();
                if case == "gap" {
                    response["features"][0]["entities"][0]["geometry"]["pntX"] = json!(0.001);
                }
                if case == "reversed" {
                    boundaries[0][1] = json!(-1);
                }
                if case == "crossing" {
                    let points = [[0.0, 0.0], [0.02, 0.02], [0.0, 0.02], [0.02, 0.0]];
                    for (i, entity) in response["features"][0]["entities"]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .take(4)
                        .enumerate()
                    {
                        let a = points[i];
                        let b = points[(i + 1) % 4];
                        entity["startParam"] = json!(0.0);
                        entity["endParam"] = json!(1.0);
                        entity["geometry"] = json!({"btType":"BTCurveGeometryLine-117","pntX":a[0],"pntY":a[1],"dirX":b[0]-a[0],"dirY":b[1]-a[1]});
                    }
                }
            }
            resolved["disambiguationData"][0]["entities"] = json!(boundaries);
            replace_query(selected(response), &resolved);
        });
        let result = crate::test_support::review::decode(&bytes);
        let wire = crate::test_support::review::wire(&result);
        let feature = &wire["model"]["features"][1]["definition"]["value"];
        assert_eq!(
            feature["kind"],
            if case == "nested" {
                "extrude"
            } else {
                "native"
            },
            "{case}: {:?}",
            result.report().losses
        );
        if case == "nested" {
            assert_eq!(
                feature["value"]["profile"]["value"]["value"]["regions"][0]["hole_boundaries"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
        } else {
            assert!(result
                .report()
                .losses
                .iter()
                .any(|l| l.code.to_string() == "onshape/history.not-supported"));
        }
        crate::test_support::review::validate(result);
    }
}

#[test]
fn face_plane_tangential_origins_and_rotated_local_geometry_keep_attachment_native() {
    for sense in [false, true] {
        for rotated in [false, true] {
            let bytes = crate::test_support::review::edit(
                &face_sketch_bundle([0.0, 0.0, 1.0], sense),
                |path, response| {
                    if path.ends_with("bodydetails/2.json") {
                        response["bodies"][0]["faces"][0]["surface"]["origin"] =
                            topology::point([0.04, 0.07, 0.005]);
                    }
                    if path.ends_with("features.json") && rotated {
                        for entity in response["features"][2]["entities"].as_array_mut().unwrap() {
                            if entity["geometry"]["btType"] != "BTCurveGeometryLine-117" {
                                continue;
                            }
                            let geometry = &mut entity["geometry"];
                            let x = geometry["pntX"].as_f64().unwrap_or(0.0);
                            let y = geometry["pntY"].as_f64().unwrap_or(0.0);
                            let dx = geometry["dirX"].as_f64().unwrap_or(0.0);
                            let dy = geometry["dirY"].as_f64().unwrap_or(0.0);
                            geometry["pntX"] = json!(-y);
                            geometry["pntY"] = json!(x);
                            geometry["dirX"] = json!(-dy);
                            geometry["dirY"] = json!(dx);
                        }
                    }
                },
            );
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
                .any(|l| l.code.to_string() == "onshape/history.not-supported"));
            crate::test_support::review::validate(result);
        }
    }
}

fn stored_region_boundaries(close_hole: bool) -> Vec<u8> {
    changed(|response| {
        response["features"][0]["constraints"] = json!([]);
        let mut resolved = query::resolved(selected(response)).unwrap();
        let template = resolved["disambiguationData"][0]["entities"][0][0].clone();
        response["features"][0]["entities"] = json!([
            {"btType":"BTMSketchCurve-4","entityId":"outer","geometry":{
                "btType":"BTCurveGeometryCircle-115","radius":0.01}},
            {"btType":if close_hole {"BTMSketchCurve-4"} else {"BTMSketchCurveSegment-155"},
                "entityId":"hole","startParam":0.0,"endParam":std::f64::consts::PI,
                "geometry":{"btType":"BTCurveGeometryCircle-115","radius":0.003}}
        ]);
        resolved["disambiguationData"][0]["entities"] = json!([("outer", 1), ("hole", -1)]
            .into_iter()
            .map(|(name, side)| {
                let mut edge = template.clone();
                edge["derivedFrom"]["sketchEntityId"] = json!(name);
                json!([edge, side])
            })
            .collect::<Vec<_>>());
        replace_query(selected(response), &resolved);
    })
}

#[test]
fn stored_closed_boundaries_prove_region_hole_containment() {
    for clockwise in [false, true] {
        let bytes = changed_fixture(&stored_region_boundaries(true), |response| {
            for entity in response["features"][0]["entities"].as_array_mut().unwrap() {
                entity["geometry"]["clockwise"] = json!(clockwise);
            }
            if clockwise {
                let mut query = query::resolved(selected(response)).unwrap();
                for boundary in query["disambiguationData"][0]["entities"]
                    .as_array_mut()
                    .unwrap()
                {
                    boundary[1] = json!(-boundary[1].as_i64().unwrap());
                }
                replace_query(selected(response), &query);
            }
        });
        let result = crate::test_support::review::decode(&bytes);
        let wire = crate::test_support::review::wire(&result);
        let feature = &wire["model"]["features"][1]["definition"]["value"];
        assert_eq!(feature["kind"], "extrude", "{:?}", result.report().losses);
        let region = &feature["value"]["profile"]["value"]["value"]["regions"][0];
        assert_eq!(region["outer_boundary"].as_array().unwrap().len(), 1);
        assert_eq!(region["hole_boundaries"].as_array().unwrap().len(), 1);
        crate::test_support::review::validate(result);
    }
    let bytes = changed_fixture(&stored_region_boundaries(true), |response| {
        response["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        response["features"][0]["entities"][0]["geometry"]["clockwise"] = json!(true);
        let mut query = query::resolved(selected(response)).unwrap();
        query["disambiguationData"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        query["disambiguationData"][0]["entities"][0][1] = json!(-1);
        replace_query(selected(response), &query);
    });
    let result = crate::test_support::review::decode(&bytes);
    let wire = crate::test_support::review::wire(&result);
    assert_eq!(
        wire["model"]["features"][1]["definition"]["value"]["kind"],
        "extrude",
        "{:?}",
        result.report().losses
    );
    crate::test_support::review::validate(result);
}

#[test]
fn a_missing_closed_hole_boundary_refuses_region_construction() {
    let result = crate::test_support::review::decode(&stored_region_boundaries(false));
    let wire = crate::test_support::review::wire(&result);
    assert_eq!(
        wire["model"]["features"][1]["definition"]["value"]["kind"],
        "native"
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.to_string() == "onshape/history.not-supported"
            && loss.message.contains("boundary is open")
    }));
    crate::test_support::review::validate(result);
}

pub(super) fn stored_attachment() -> Vec<u8> {
    crate::test_support::review::edit(
        &face_sketch_bundle([0.0, 0.0, 1.0], true),
        |path, response| {
            if path.ends_with("features.json") {
                response["features"][2]["plane"] = json!({
                    "origin":{"x":0.04,"y":0.07,"z":0.005},
                    "normal":{"x":0.0,"y":0.0,"z":1.0},
                    "x":{"x":0.0,"y":1.0,"z":0.0}
                });
            }
        },
    )
}

#[test]
fn stored_attachment_plane_and_x_axis_keep_the_full_frame_typed() {
    for sense in [1.0, -1.0] {
        let bytes = crate::test_support::review::edit(&stored_attachment(), |path, response| {
            if path.ends_with("features.json") {
                response["features"][2]["plane"]["normal"]["z"] = json!(sense);
            }
        });
        let result = crate::test_support::review::decode(&bytes);
        let wire = crate::test_support::review::wire(&result);
        assert_eq!(
            wire["model"]["features"][2]["definition"]["value"]["kind"],
            "sketch",
            "{:?}",
            result.report().losses
        );
        let sketches = wire["model"]["sketches"].as_array().unwrap();
        let frame = &sketches
            .iter()
            .find(|sketch| sketch["id"] == id("sketch", "side-sketch"))
            .unwrap()["placement"]["frame"];
        assert_eq!(frame["origin"], topology::point([40.0, 70.0, 5.0]));
        assert_eq!(frame["normal"], topology::point([0.0, 0.0, sense]));
        assert_eq!(frame["u_axis"], topology::point([0.0, 1.0, 0.0]));
        assert_eq!(
            wire["model"]["features"][2]["dependencies"],
            json!([id("feature", "raised-plate")])
        );
        crate::test_support::review::validate(result);
    }
}

#[test]
fn absent_stored_attachment_datums_refuse_the_frame_with_its_named_loss() {
    for missing in ["plane", "origin", "normal", "x"] {
        let bytes = crate::test_support::review::edit(&stored_attachment(), |path, response| {
            if path.ends_with("features.json") {
                let sketch = &mut response["features"][2];
                if missing == "plane" {
                    sketch.as_object_mut().unwrap().remove("plane");
                } else {
                    sketch["plane"].as_object_mut().unwrap().remove(missing);
                }
            }
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
            .any(|loss| loss.code.to_string() == "onshape/history.not-supported"));
        crate::test_support::review::validate(result);
    }
}

pub(super) fn stored_thin(open: bool, flip: bool, reversed: bool) -> Vec<u8> {
    crate::test_support::review::edit(&fixture("plate-fillet"), |path, response| {
        if !path.ends_with("features.json") {
            return;
        }
        response["features"].as_array_mut().unwrap().truncate(2);
        response["rollbackIndex"] = json!(2);
        let sketch = &mut response["features"][0];
        sketch["constraints"] = json!([]);
        if open {
            sketch["entities"].as_array_mut().unwrap().truncate(1);
            if reversed {
                sketch["entities"][0]["geometry"]["pntX"] = json!(0.02);
                sketch["entities"][0]["geometry"]["dirX"] = json!(-1.0);
            }
        } else {
            sketch["entities"] = json!([{"btType":"BTMSketchCurve-4","entityId":"circle","geometry":{
                "btType":"BTCurveGeometryCircle-115","radius":0.005,"clockwise":reversed}}]);
        }
        let feature = &mut response["features"][1];
        parameter(feature, "bodyType")["value"] = json!("THIN");
        let profile = parameter(feature, "entities");
        profile["parameterId"] = json!("wallShape");
        let query = json!({"entityType":"EDGE","historyType":"CREATION","queryType":"SKETCH_ENTITY",
            "operationId":["Base sketch.wireOp"],"sketchEntityId":if open {"line0"} else {"circle"}});
        profile["queries"] = json!([{}]);
        replace_query(&mut profile["queries"][0], &query);
        feature["parameters"].as_array_mut().unwrap().extend([
            json!({"parameterId":"domain","value":"MODEL"}),
            json!({"parameterId":"midplane","value":false}),
            json!({"parameterId":"flipWall","value":flip}),
            json!({"parameterId":"oppositeDirection","value":false}),
            json!({"parameterId":"thickness","expression":"0.3 mm"}),
            json!({"parameterId":"thickness1","expression":"0.3 mm"}),
            json!({"parameterId":"thickness2","expression":"0 mm"}),
        ]);
    })
}

#[test]
fn stored_thin_keeps_an_unselected_native_sketch_entity() {
    let bytes =
        crate::test_support::review::edit(&stored_thin(true, false, false), |path, response| {
            if path.ends_with("features.json") {
                response["features"][0]["entities"]
                    .as_array_mut()
                    .unwrap()
                    .push(
                        json!({"btType":"BTMSketchCurve-4","entityId":"native-curve",
                        "geometry":{"btType":"synthetic-curve"}}),
                    );
            }
        });
    let result = crate::test_support::review::decode(&bytes);
    let wire = crate::test_support::review::wire(&result);
    assert_eq!(result.ir().design.features.len(), 2);
    let entity = wire["design"]["sketch_entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entity| entity["id"] == id("entity", "Base sketch/native-curve"))
        .unwrap();
    assert_eq!(entity["definition"]["kind"], "defined");
    assert_eq!(entity["definition"]["value"]["form"]["kind"], "native");
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == "onshape/sketch.geometry-not-supported"));
    crate::test_support::review::validate(result);
}

#[test]
fn stored_thin_domain_has_a_product_scope_and_one_container_membership() {
    for open in [false, true] {
        for symmetric in [false, true] {
            let bytes = crate::test_support::review::edit(
                &stored_thin(open, false, false),
                |path, response| {
                    if path.ends_with("features.json") {
                        crate::test_support::review::parameter(
                            &mut response["features"][1],
                            "midplane",
                        )["value"] = json!(symmetric);
                    }
                },
            );
            let result = crate::test_support::review::decode(&bytes);
            let wire = crate::test_support::review::wire(&result);
            assert_eq!(
                wire["model"]["features"][1]["definition"]["value"]["kind"],
                "native",
                "{:?}",
                result.report().losses
            );
            assert_eq!(result.ir().design.features.len(), 2);
            assert_eq!(result.ir().design.containers.len(), 1);
            assert_eq!(result.ir().design.scopes.len(), 1);
            let [sketch] = result.ir().design.sketches.as_slice() else {
                panic!("thin profile must have one design sketch");
            };
            assert_eq!(sketch.id.as_str(), id("sketch", "Base sketch"));
            assert_eq!(sketch.owner.as_str(), id("feature", "Base sketch"));
            assert_eq!(
                result.ir().design.sketch_entities.len(),
                result.ir().model.sketch_entities.len()
            );
            for entity in &result.ir().design.sketch_entities {
                assert_eq!(entity.sketch, sketch.id);
                assert!(result
                    .ir()
                    .model
                    .sketch_entities
                    .iter()
                    .any(|source| source.id() == &entity.id));
            }
            assert_eq!(
                wire["design"]["features"][0]["definition"]["operation"],
                json!({"kind":"sketch","value":id("sketch", "Base sketch")})
            );
            assert_eq!(
                wire["design"]["containers"][0]["value"]["members"],
                json!([{"kind":"feature","value":id("feature", "Base sketch")},{"kind":"feature","value":id("feature", "Plate")}])
            );
            assert_eq!(
                wire["design"]["scopes"][0]["definition"],
                wire["model"]["product_definitions"][0]["id"]
            );
            assert_eq!(
                wire["design"]["scopes"][0]["current"][0]["bodies"],
                wire["model"]["product_definitions"][0]["bodies"]
            );
            assert_eq!(
                wire["design"]["scopes"][0]["current"][0]["at"],
                json!({"kind":"after","value":{"kind":"feature","value":id("feature", "Plate")}})
            );
            crate::test_support::review::validate(result);
        }
    }
    let bytes = crate::test_support::review::edit(&fixture("plate-fillet"), |path, response| {
        if path.ends_with("features.json") {
            response["features"].as_array_mut().unwrap().truncate(2);
            response["rollbackIndex"] = json!(2);
            response["features"][0]["entities"].as_array_mut().unwrap().push(json!({"btType":"BTMSketchPoint-158","entityId":"unselected-point","x":0.03,"y":0.04}));
            let feature = &mut response["features"][1];
            parameter(feature, "bodyType")["value"] = json!("THIN");
            parameter(feature, "entities")["parameterId"] = json!("wallShape");
            feature["parameters"].as_array_mut().unwrap().extend([
                json!({"parameterId":"domain","value":"MODEL"}),
                json!({"parameterId":"midplane","value":true}),
                json!({"parameterId":"flipWall","value":false}),
                json!({"parameterId":"oppositeDirection","value":false}),
                json!({"parameterId":"thickness","expression":"0.3 mm"}),
            ]);
        }
    });
    let result = crate::test_support::review::decode(&bytes);
    assert_eq!(
        result.ir().design.features.len(),
        2,
        "{:?}",
        result.report().losses
    );
    crate::test_support::review::validate(result);
}

#[test]
fn absent_stored_thin_domain_refuses_the_scoped_construction() {
    let bytes =
        crate::test_support::review::edit(&stored_thin(true, false, false), |path, response| {
            if path.ends_with("features.json") {
                response["features"][1]["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|parameter| parameter["parameterId"] != "domain");
            }
        });
    let result = crate::test_support::review::decode(&bytes);
    assert!(result.ir().design.features.is_empty());
    assert!(result.ir().design.containers.is_empty());
    assert!(result.ir().design.scopes.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == "onshape/extrude.thin-not-supported"));
    crate::test_support::review::validate(result);
}

#[test]
fn stored_open_thin_side_follows_the_stored_line_direction_and_flip() {
    for open in [false, true] {
        let bytes = crate::test_support::review::edit(
            &stored_thin(open, false, false),
            |path, response| {
                if path.ends_with("features.json") {
                    crate::test_support::review::parameter(
                        &mut response["features"][1],
                        "thickness2",
                    )["expression"] = json!("0.3 mm");
                }
            },
        );
        let result = crate::test_support::review::decode(&bytes);
        let wire = crate::test_support::review::wire(&result);
        assert_eq!(
            wire["design"]["features"][1]["definition"]["operation"]["value"]["output"]["body"]
                ["thickness"],
            json!({"kind":"symmetric","value":{"kind":"literal","value":0.6}})
        );
        crate::test_support::review::validate(result);
    }
    for reversed in [false, true] {
        for flip in [false, true] {
            for second_side in [false, true] {
                let bytes = crate::test_support::review::edit(
                    &stored_thin(true, flip, reversed),
                    |path, response| {
                        if path.ends_with("features.json") && second_side {
                            parameter(&mut response["features"][1], "thickness1")["expression"] =
                                json!("0 mm");
                            parameter(&mut response["features"][1], "thickness2")["expression"] =
                                json!("0.3 mm");
                        }
                    },
                );
                let result = crate::test_support::review::decode(&bytes);
                let wire = crate::test_support::review::wire(&result);
                assert_eq!(
                    result.ir().design.features.len(),
                    2,
                    "{:?}",
                    result.report().losses
                );
                assert_eq!(
                    wire["design"]["features"][1]["definition"]["operation"]["value"]["output"]
                        ["body"]["thickness"]["value"]["side"]["kind"],
                    if flip ^ second_side { "left" } else { "right" }
                );
                crate::test_support::review::validate(result);
            }
        }
    }
    for clockwise in [false, true] {
        for flip in [false, true] {
            let result = crate::test_support::review::decode(&stored_thin(false, flip, clockwise));
            let wire = crate::test_support::review::wire(&result);
            assert_eq!(
                result.ir().design.features.len(),
                2,
                "{:?}",
                result.report().losses
            );
            assert_eq!(
                wire["design"]["features"][1]["definition"]["operation"]["value"]["output"]["body"]
                    ["thickness"]["value"]["side"]["kind"],
                if flip ^ clockwise {
                    "inside"
                } else {
                    "outside"
                }
            );
            crate::test_support::review::validate(result);
        }
    }
    let bytes =
        crate::test_support::review::edit(&stored_thin(true, false, false), |path, response| {
            if path.ends_with("features.json") {
                response["features"][0]["entities"][0]["geometry"]
                    .as_object_mut()
                    .unwrap()
                    .remove("dirY");
            }
        });
    let result = crate::test_support::review::decode(&bytes);
    assert_eq!(
        result.ir().design.features.len(),
        2,
        "{:?}",
        result.report().losses
    );
    crate::test_support::review::validate(result);
}

#[test]
fn reversed_open_thin_interval_keeps_its_side_and_reports_the_open_convention() {
    for reversed_interval in [false, true] {
        let bytes = crate::test_support::review::edit(
            &stored_thin(true, false, false),
            |path, response| {
                if path.ends_with("features.json") && reversed_interval {
                    let line = &mut response["features"][0]["entities"][0];
                    let start = line["startParam"].clone();
                    line["startParam"] = line["endParam"].clone();
                    line["endParam"] = start;
                }
            },
        );
        let result = crate::test_support::review::decode(&bytes);
        let wire = crate::test_support::review::wire(&result);
        assert_eq!(
            wire["design"]["features"][1]["definition"]["operation"]["value"]["output"]["body"]
                ["thickness"]["value"]["side"]["kind"],
            "right",
            "{:?}",
            result.report().losses
        );
        let named = result
            .report()
            .losses
            .iter()
            .any(|loss| format!("{loss:?}").contains("ONSHAPE-007"));
        assert_eq!(named, reversed_interval, "{:?}", result.report().losses);
        crate::test_support::review::validate(result);
    }
}

#[test]
fn absent_stored_thin_side_or_direction_refuses_the_wall() {
    for missing in [
        "midplane",
        "flipWall",
        "oppositeDirection",
        "thickness2",
        "direction",
        "clockwise",
    ] {
        let bytes = crate::test_support::review::edit(
            &stored_thin(missing != "clockwise", false, false),
            |path, response| {
                if !path.ends_with("features.json") {
                    return;
                }
                if missing == "direction" {
                    response["features"][0]["entities"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("geometry");
                } else if missing == "clockwise" {
                    response["features"][0]["entities"][0]["geometry"]
                        .as_object_mut()
                        .unwrap()
                        .remove("clockwise");
                } else {
                    response["features"][1]["parameters"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|parameter| parameter["parameterId"] != missing);
                }
            },
        );
        let result = crate::test_support::review::decode(&bytes);
        assert!(result.ir().design.features.is_empty());
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == "onshape/extrude.thin-not-supported"));
        crate::test_support::review::validate(result);
    }
}
