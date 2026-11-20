// SPDX-License-Identifier: Apache-2.0
//! Codec-wide inspect and decode over code-built designs.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{archive, RawEntry};
use cadmpeg_test_support::EditableDecodeResult;
use serde_json::json;

use crate::test_support::{blob_name, box_document, container, import_container, STEP_TEXT};
use crate::SolidcraftCodec;

fn loss_codes(bytes: &[u8]) -> Vec<String> {
    let result = SolidcraftCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.to_string())
        .collect()
}

fn dialect(bytes: &[u8]) -> String {
    let summary = SolidcraftCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .unwrap();
    serde_json::to_value(&summary).unwrap()["identity"]["dialects"]["primary"]["dialect"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn plain_json_and_container_designs_classify_by_encoding_and_format() {
    let json57 = serde_json::to_vec(&box_document("solidcraft/57")).unwrap();
    let json13 = serde_json::to_vec(&box_document("solvecraft/13")).unwrap();
    assert_eq!(dialect(&json57), "solidcraft:json-v57");
    assert_eq!(dialect(&json13), "solidcraft:json-v1-56");
    assert_eq!(
        dialect(&container(&box_document("solidcraft/57"), &[])),
        "solidcraft:container-1-v57"
    );
    assert_eq!(
        dialect(&container(&box_document("solidcraft/20"), &[])),
        "solidcraft:container-1-v1-56"
    );
    assert_eq!(
        dialect(&container(&box_document("solidcraft/58"), &[])),
        "solidcraft:unknown"
    );
}

#[test]
fn a_verified_blob_reports_no_integrity_loss() {
    let codes = loss_codes(&import_container());
    assert!(
        codes.iter().all(|code| !code.contains("container.blob")),
        "{codes:?}"
    );
    assert!(codes.contains(&"solidcraft/blob.payload-undecoded".to_owned()));
    assert!(codes.contains(&"solidcraft/cache.model-absent".to_owned()));
}

#[test]
fn blob_digest_missing_malformed_and_unreferenced_blobs_are_charged() {
    let wrong = blob_name(b"other bytes", "step");
    let stray = blob_name(b"stray", "png");
    let mut document = crate::test_support::import_document(&wrong);
    document["features"][0]["mesh_blob"] = json!("blobs/short.mesh");
    document["features"][0]["texture_blob"] = json!(blob_name(b"absent", "texture"));
    let bytes = container(&document, &[(&wrong, STEP_TEXT), (&stray, b"png")]);
    let codes = loss_codes(&bytes);
    for code in [
        "solidcraft/container.blob-digest-mismatch",
        "solidcraft/container.blob-reference-malformed",
        "solidcraft/container.blob-missing",
        "solidcraft/container.blob-unreferenced",
    ] {
        assert!(codes.contains(&code.to_owned()), "{code} in {codes:?}");
    }
}

#[test]
fn every_member_survives_as_a_retained_record() {
    let bytes = container(
        &box_document("solidcraft/57"),
        &[
            ("cache/thumbnail.png", b"\x89PNG"),
            ("plugins/io.example/x", b"data"),
        ],
    );
    let result = SolidcraftCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let unknowns = result
        .ir()
        .native
        .namespace("solidcraft")
        .and_then(|namespace| namespace.arenas().get("unknowns"))
        .map_or(0, Vec::len);
    // The four members, and the appearances section on its own.
    assert_eq!(unknowns, 5);
}

#[test]
fn a_drawing_container_is_its_own_row() {
    let bytes = archive(&[
        RawEntry::stored("drawing.json", br#"{"format":"solidcraft-drawing/3"}"#),
        RawEntry::stored(
            "container.json",
            br#"{"format":"solidcraft-container/1","content":"drawing"}"#,
        ),
    ]);
    assert_eq!(dialect(&bytes), "solidcraft:drawing-container-1");
    assert!(loss_codes(&bytes).contains(&"solidcraft/document.drawing-undecoded".to_owned()));
}

#[test]
fn an_unreadable_container_document_is_residual_and_charged() {
    let bytes = archive(&[
        RawEntry::stored("document.json", b"{not json"),
        RawEntry::stored("container.json", br#"{"format":"solidcraft-container/1"}"#),
    ]);
    assert_eq!(dialect(&bytes), "solidcraft:unknown");
    let codes = loss_codes(&bytes);
    assert!(codes.contains(&"solidcraft/document.unreadable".to_owned()));
    assert!(codes.contains(&"solidcraft/source.dialect-unverified".to_owned()));
}

#[test]
fn the_final_cached_state_decodes_into_one_closed_body() {
    let result = SolidcraftCodec
        .decode(
            &mut Cursor::new(crate::test_support::CACHED_DESIGN),
            &DecodeOptions::default(),
        )
        .unwrap();
    let geometry = &result.ir().geometry;
    assert_eq!(geometry.bodies.len(), 1);
    assert_eq!(
        geometry.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
    assert_eq!(geometry.bodies[0].name.as_deref(), Some("Body1"));
    assert_eq!(geometry.faces.len(), 8);
    assert_eq!(geometry.edges.len(), 18);
    assert_eq!(geometry.vertices.len(), 12);
    assert_eq!(geometry.coedges.len(), 36);
    let codes: Vec<String> = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.to_string())
        .collect();
    assert!(
        codes.iter().all(|code| !code.contains("cache.body")),
        "{codes:?}"
    );
    assert!(codes
        .iter()
        .any(|code| code.ends_with("cache.model-undecoded")));
}

#[test]
fn a_cache_of_another_document_is_stale() {
    let members = crate::test_support::members(crate::test_support::CACHED_DESIGN);
    let mut document = members
        .iter()
        .find(|(name, _)| name == "document.json")
        .unwrap()
        .1
        .clone();
    document.push(b'\n');
    let bytes = crate::test_support::cached_design_with(&[("document.json", &document)]);
    let codes = loss_codes(&bytes);
    assert!(
        codes.iter().any(|code| code.ends_with("cache.model-stale")),
        "{codes:?}"
    );
}

#[test]
fn a_payload_that_its_digest_does_not_pin_is_malformed() {
    let members = crate::test_support::members(crate::test_support::CACHED_DESIGN);
    let mut payload = members
        .iter()
        .find(|(name, _)| name == "cache/bodies.bin")
        .unwrap()
        .1
        .clone();
    payload.push(0);
    let bytes = crate::test_support::cached_design_with(&[("cache/bodies.bin", &payload)]);
    let codes = loss_codes(&bytes);
    assert!(
        codes
            .iter()
            .any(|code| code.ends_with("cache.model-malformed")),
        "{codes:?}"
    );
}

fn features(bytes: &[u8]) -> Vec<serde_json::Value> {
    let result = SolidcraftCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    result
        .ir()
        .model
        .features
        .iter()
        .map(|feature| serde_json::to_value(feature).unwrap())
        .collect()
}

#[test]
fn primitives_carry_their_evaluated_dimensions_and_placement() {
    let features = features(crate::test_support::CACHED_DESIGN);
    assert_eq!(features.len(), 3);
    let block = &features[0]["definition"]["value"];
    assert_eq!(block["kind"], "primitive");
    assert_eq!(
        block["value"]["solid"],
        json!({"kind": "box", "length": 20.0, "width": 10.0, "height": 5.0})
    );
    assert_eq!(block["value"]["op"], "new_body");
    assert_eq!(features[0]["outputs"].as_array().unwrap().len(), 1);
    let hole = &features[1]["definition"]["value"]["value"];
    assert_eq!(hole["solid"]["radius"], 2.0);
    assert_eq!(hole["op"], "cut");
    assert_eq!(hole["placement"][2][3], -1.0);
    assert_eq!(features[2]["definition"]["value"]["kind"], "fillet");
    assert_eq!(features[2]["source_tag"], "fillet");
}

#[test]
fn a_sketch_and_its_extrude_resolve_through_the_parameters() {
    let document = json!({
        "format": "solidcraft/57",
        "name": "Plate",
        "units": "mm",
        "params": [{"name": "width", "expr": "40 mm", "unit": "mm", "model": false}],
        "features": [
            {"id": 1, "name": "Sketch1", "suppressed": false, "type": "sketch",
             "plane": {"type": "origin", "name": "XZ"},
             "sketch": {"points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true},
                                   {"id": "p1", "pos": [0.0, 0.0]}, {"id": "p2", "pos": [40.0, 0.0]},
                                   {"id": "c", "pos": [10.0, 10.0]}],
                        "curves": [{"id": "l1", "type": "line", "a": 1, "b": 2},
                                   {"id": "c1", "type": "circle", "c": 3, "r": 2.0}],
                        "constraints": []}},
            {"id": 2, "name": "Extrude1", "suppressed": true, "type": "extrude", "sketch": 1,
             "profiles": {"type": "all"},
             "extent": {"distance": "width / 2", "direction": "symmetric", "convention": "legacy_per_side"},
             "operation": "join"}
        ]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let features = features(&bytes);
    assert_eq!(features[1]["suppressed"], true);
    assert_eq!(
        features[1]["dependencies"],
        json!(["solidcraft:design:feature#1"])
    );
    let extrude = &features[1]["definition"]["value"]["value"];
    assert_eq!(
        extrude["extent"],
        json!({"kind": "symmetric", "side": {"termination": {"kind": "blind", "length": 40.0}}})
    );
    assert_eq!(extrude["op"], "join");
    let result = SolidcraftCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let sketch = serde_json::to_value(&result.ir().model.sketches[0]).unwrap();
    assert_eq!(
        sketch["placement"]["frame"]["normal"],
        json!({"x": 0.0, "y": 1.0, "z": 0.0})
    );
    assert_eq!(result.ir().model.sketch_entities.len(), 2);
}

#[test]
fn sketch_constraints_and_dimensions_drive_the_design_parameters() {
    let document = json!({
        "format": "solidcraft/57",
        "name": "Bracket",
        "units": "mm",
        "params": [
            {"name": "width", "expr": "40 mm", "unit": "mm", "model": false},
            {"name": "d1", "expr": "=width", "unit": "mm", "model": true},
            {"name": "d2", "expr": "2 d1 + 1", "unit": "mm", "model": true}
        ],
        "features": [
            {"id": 1, "name": "Sketch1", "suppressed": false, "type": "sketch",
             "plane": {"type": "origin", "name": "XY"},
             "sketch": {"points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true},
                                   {"id": "p1", "pos": [0.0, 0.0]}, {"id": "p2", "pos": [40.0, 0.0]},
                                   {"id": "p3", "pos": [40.0, 10.0]}, {"id": "c", "pos": [20.0, 5.0]}],
                        "curves": [{"id": "l1", "type": "line", "a": 1, "b": 2},
                                   {"id": "l2", "type": "line", "a": 2, "b": 3},
                                   {"id": "c1", "type": "circle", "c": 4, "r": 2.0}],
                        "constraints": [
                            {"id": "k1", "type": "coincident", "p": 0, "q": 1},
                            {"id": "k2", "type": "horizontal", "l": 0},
                            {"id": "k3", "type": "perpendicular", "a": 0, "b": 1},
                            {"id": "k4", "type": "length", "l": 0, "value": 40.0, "param": "d1"},
                            {"id": "k5", "type": "radius", "c": 2, "value": 2.0, "driven": true},
                            {"id": "k6", "type": "arc_length", "c": 2, "value": 3.0}
                        ]}},
            {"id": 2, "name": "Extrude1", "suppressed": false, "type": "extrude", "sketch": 1,
             "profiles": {"type": "all"},
             "extent": {"distance": "d2", "direction": "positive"},
             "operation": "new_body"}
        ]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let result = SolidcraftCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let model = serde_json::to_value(&result.ir().model).unwrap();
    let parameter = |name: &str| {
        model["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|parameter| parameter["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(
        parameter("d1")["value"],
        json!({"kind": "length", "value": 40.0})
    );
    assert_eq!(parameter("d1")["owner"], "solidcraft:design:feature#1");
    assert_eq!(
        parameter("d1")["dependencies"],
        json!(["solidcraft:design:parameter#width"])
    );
    assert_eq!(
        parameter("d2")["value"],
        json!({"kind": "length", "value": 81.0})
    );
    assert_eq!(parameter("d2")["owner"], "solidcraft:design:feature#2");
    assert!(parameter("width").get("owner").is_none());
    assert_eq!(
        parameter("k5")["value"],
        json!({"kind": "length", "value": 2.0})
    );
    let constraint = |name: &str| {
        model["sketch_constraints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|constraint| {
                constraint["id"]
                    == format!(
                        "solidcraft:design:sketch-constraint#1%3A{}",
                        name.replace(':', "%3A")
                    )
            })
            .unwrap()
            .clone()
    };
    let entity = |name: &str| format!("solidcraft:design:sketch-entity#1%3A{name}");
    // The lines share p2.
    assert_eq!(
        constraint("shared:p2")["definition"]["loci"],
        json!([{"kind": "end", "value": entity("l1")}, {"kind": "start", "value": entity("l2")}])
    );
    assert_eq!(
        constraint("k1")["definition"]["loci"],
        json!([{"kind": "entity", "value": entity("origin")}, {"kind": "start", "value": entity("l1")}])
    );
    assert_eq!(constraint("origin")["definition"]["kind"], "fixed");
    assert_eq!(constraint("k3")["definition"]["kind"], "perpendicular");
    assert_eq!(
        constraint("k4")["definition"],
        json!({"kind": "distance", "entities": [entity("l1")], "parameter": "solidcraft:design:parameter#d1"})
    );
    assert_eq!(constraint("k4")["driving"], true);
    assert_eq!(constraint("k5")["driving"], false);
    assert_eq!(constraint("k6")["definition"]["kind"], "native");
    assert_eq!(
        constraint("k6")["definition"]["native_kind"],
        "solidcraft:arc_length"
    );
    let codes = loss_codes(&bytes);
    assert!(codes.contains(&"solidcraft/history.sketch-constraint-native".to_owned()));
    assert!(!codes.contains(&"solidcraft/history.parameters-undecoded".to_owned()));
}

#[test]
fn the_operation_families_carry_their_semantics() {
    let document = json!({
        "format": "solidcraft/57",
        "name": "Families",
        "units": "mm",
        "params": [{"name": "r", "expr": "2", "unit": "mm", "model": true}],
        "features": [
            {"id": 1, "name": "Base", "suppressed": false, "type": "box",
             "corner": [0, 0, 0], "length": "40", "width": "20", "height": "10", "operation": "new_body"},
            {"id": 2, "name": "Fillet1", "suppressed": false, "type": "fillet", "param_names": ["r"],
             "edges": [[20.0, 0.0, 10.0]], "edge_names": ["F1:top|F1:front"], "radius": "r"},
            {"id": 3, "name": "Chamfer1", "suppressed": false, "type": "chamfer",
             "edges": [[20.0, 20.0, 10.0]], "distance": "1", "angle": "30 deg", "flip": true},
            {"id": 4, "name": "Shell1", "suppressed": false, "type": "shell", "body": "Body1",
             "faces": [[20.0, 10.0, 10.0]], "thickness": "1.5", "direction": "outside"},
            {"id": 5, "name": "Pattern1", "suppressed": false, "type": "pattern", "features": [2],
             "pattern": {"type": "rectangular", "dir1": [1.0, 0.0, 0.0], "count1": "3", "spacing1": "5",
                         "dir2": [0.0, 1.0, 0.0], "count2": "2", "spacing2": "4", "alignment2": "center"}},
            {"id": 6, "name": "Pattern2", "suppressed": false, "type": "pattern", "features": [],
             "bodies": ["Body1"],
             "pattern": {"type": "circular", "origin": [0, 0, 0], "axis": [0.0, 0.0, 1.0],
                         "count": "4", "angle": "360 deg"}},
            {"id": 7, "name": "Plane1", "suppressed": false, "type": "construction_plane",
             "plane": {"type": "offset", "base": {"type": "origin", "name": "YZ"}, "distance": "20"}},
            {"id": 8, "name": "Mirror1", "suppressed": false, "type": "mirror", "features": [1],
             "plane": {"type": "construction", "feature": 7}},
            {"id": 9, "name": "Move1", "suppressed": false, "type": "move", "bodies": ["Body1"],
             "translate": ["1", "2", "3"], "rotate_axis": [0.0, 0.0, 1.0], "origin": [0, 0, 0],
             "angle": "90 deg", "copy": true},
            {"id": 10, "name": "Scale1", "suppressed": false, "type": "scale", "bodies": ["Body1"],
             "origin": [1.0, 2.0, 3.0], "factor": "2"},
            {"id": 11, "name": "Combine1", "suppressed": false, "type": "combine", "target": "Body1",
             "tools": ["Body2"], "operation": "cut", "keep_tools": true},
            {"id": 12, "name": "Remove1", "suppressed": false, "type": "remove", "bodies": ["Body2"]},
            {"id": 13, "name": "Path", "suppressed": false, "type": "sketch",
             "plane": {"type": "origin", "name": "XZ"},
             "sketch": {"points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true},
                                   {"id": "p1", "pos": [0.0, 0.0]}, {"id": "p2", "pos": [0.0, 30.0]}],
                        "curves": [{"id": "l1", "type": "line", "a": 1, "b": 2}], "constraints": []}},
            {"id": 14, "name": "Section", "suppressed": false, "type": "sketch",
             "plane": {"type": "origin", "name": "XY"},
             "sketch": {"points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true},
                                   {"id": "c", "pos": [0.0, 0.0]}],
                        "curves": [{"id": "c1", "type": "circle", "c": 1, "r": 2.0}], "constraints": []}},
            {"id": 15, "name": "Sweep1", "suppressed": false, "type": "sweep", "sketch": 14,
             "profiles": {"type": "all"}, "path_sketch": 13, "path": ["l1"], "operation": "new_body"},
            {"id": 16, "name": "Loft1", "suppressed": false, "type": "loft", "operation": "join",
             "sections": [{"sketch": 14, "profiles": {"type": "all"}}, {"sketch": 13, "profiles": {"type": "all"}}]},
            {"id": 17, "name": "Coil1", "suppressed": false, "type": "coil", "axis": [0.0, 0.0, 1.0],
             "base": [0, 0, 0], "diameter": "10", "pitch": "3", "turns": "5", "section": "square",
             "section_size": "1", "start_angle": "90 deg", "operation": "new_body"},
            {"id": 19, "name": "PathPattern", "suppressed": false, "type": "pattern", "features": [1],
             "pattern": {"type": "path", "path_sketch": 13, "path": ["l1"], "count": "4", "spacing": "30", "extent": true}},
            {"id": 20, "name": "Open", "suppressed": false, "type": "delete_face",
             "faces": [[1.0, 2.0, 3.0]], "face_names": ["F1:top"], "heal": false},
            {"id": 18, "name": "Raise", "suppressed": false, "type": "offset_face",
             "faces": [[7.0, 15.0, 10.0]], "face_names": ["F1:top"], "distance": "2"}
        ]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let mut features = features(&bytes);
    features.sort_by_key(|feature| feature["ordinal"].as_u64());
    let kinds: Vec<&str> = features
        .iter()
        .map(|feature| feature["definition"]["value"]["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "primitive",
            "fillet",
            "chamfer",
            "shell",
            "pattern",
            "pattern",
            "datum_plane",
            "pattern",
            "move_body",
            "scale",
            "combine",
            "delete_body",
            "sketch",
            "sketch",
            "sweep",
            "loft",
            "coil",
            "pattern",
            "delete_face",
            "move_face"
        ]
    );
    let operation = |index: usize| features[index]["definition"]["value"]["value"].clone();
    assert_eq!(
        operation(1)["groups"][0]["radius"],
        json!({"kind": "constant", "radius": 2.0})
    );
    assert_eq!(operation(4)["pattern"]["second"]["symmetric"], true);
    assert_eq!(
        operation(5)["pattern"]["angle"].as_f64().unwrap(),
        3.0 * std::f64::consts::TAU / 4.0
    );
    assert_eq!(
        operation(7)["pattern"]["plane_origin"],
        json!({"x": 20.0, "y": 0.0, "z": 0.0})
    );
    assert_eq!(
        features[7]["dependencies"],
        json!(["solidcraft:design:feature#1", "solidcraft:design:feature#7"])
    );
    assert_eq!(operation(8)["copies"], 1);
    let coil = operation(16)["construction"].clone();
    assert_eq!(coil["clockwise"], true);
    assert_eq!(coil["placement"]["radial"]["y"], 1.0);
    assert_eq!(operation(17)["pattern"]["spacing"], 10.0);
}

#[test]
fn a_counterbore_hole_maps_to_the_neutral_hole() {
    let document = json!({
        "format": "solidcraft/57",
        "name": "Holes",
        "units": "mm",
        "params": [],
        "features": [
            {"id": 1, "name": "Base", "suppressed": false, "type": "box",
             "corner": [0, 0, 0], "length": "40", "width": "20", "height": "10", "operation": "new_body"},
            {"id": 2, "name": "Counterbore1", "suppressed": false, "type": "hole",
             "position": [20.0, 10.0, 10.0], "direction": [0.0, 0.0, -1.0],
             "diameter": "6", "depth": "8",
             "hole": {"type": "counterbore", "cb_diameter": "10", "cb_depth": "3"}}
        ]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let features = features(&bytes);
    let hole = &features[1]["definition"]["value"];
    assert_eq!(hole["kind"], "hole");
    assert_eq!(hole["value"]["shape"]["diameter"], 6.0);
    assert_eq!(
        hole["value"]["shape"]["construction"]["value"]["kind"]["kind"],
        "counterbore"
    );
    assert_eq!(
        hole["value"]["extent"],
        json!({"kind": "blind", "length": 8.0})
    );
}

#[test]
fn a_split_by_a_construction_plane_maps_to_the_neutral_split() {
    let document = json!({
        "format": "solidcraft/57",
        "name": "Splits",
        "units": "mm",
        "params": [],
        "features": [
            {"id": 1, "name": "Base", "suppressed": false, "type": "box",
             "corner": [0, 0, 0], "length": "40", "width": "20", "height": "10", "operation": "new_body"},
            {"id": 2, "name": "Plane1", "suppressed": false, "type": "construction_plane",
             "plane": {"type": "offset", "base": {"type": "origin", "name": "YZ"}, "distance": "20"}},
            {"id": 3, "name": "SplitBody1", "suppressed": false, "type": "split_body",
             "body": "Body1", "plane": {"type": "construction", "feature": 2}},
            {"id": 4, "name": "SplitFace1", "suppressed": false, "type": "split_face",
             "faces": [[20.0, 10.0, 10.0]], "face_names": ["F1:top"],
             "plane": {"type": "construction", "feature": 2}},
            {"id": 5, "name": "SplitBody2", "suppressed": false, "type": "split_body",
             "body": "Body1", "plane": {"type": "origin", "name": "XY"}},
            {"id": 6, "name": "SplitFace2", "suppressed": false, "type": "split_face",
             "faces": [[20.0, 10.0, 10.0]], "face_names": ["F1:top"],
             "plane": {"type": "construction", "feature": 99}}
        ]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let features = features(&bytes);
    let kind = |index: usize| features[index]["definition"]["value"]["kind"].clone();
    assert_eq!(kind(2), "split_body");
    assert_eq!(kind(3), "split_face");
    assert_eq!(kind(4), "native");
    assert_eq!(kind(5), "native");
    let split_body = &features[2]["definition"]["value"]["value"];
    assert_eq!(split_body["targets"]["kind"], "native");
    assert_eq!(split_body["tool"]["kind"], "plane");
    assert_eq!(
        features[2]["dependencies"],
        json!(["solidcraft:design:feature#2"])
    );
    assert_eq!(
        features[3]["definition"]["value"]["value"]["tool"]["kind"],
        "plane"
    );
}

#[test]
fn a_draft_on_an_origin_plane_maps_to_the_neutral_draft() {
    let document = json!({
        "format": "solidcraft/57",
        "name": "Drafts",
        "units": "mm",
        "params": [],
        "features": [
            {"id": 1, "name": "Draft1", "suppressed": false, "type": "draft",
             "faces": [[20.0, 10.0, 5.0]], "face_names": ["F1:front"],
             "neutral": "XY", "pull": [0.0, 0.0, 1.0], "angle": "10 deg"},
            {"id": 2, "name": "Draft2", "suppressed": false, "type": "draft",
             "faces": [[20.0, 10.0, 5.0]], "face_names": ["F1:front"],
             "neutral": {"type": "construction", "feature": 1}, "angle": "10 deg"}
        ]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let features = features(&bytes);
    let feature = |name: &str| {
        features
            .iter()
            .find(|feature| feature["name"] == name)
            .unwrap()
            .clone()
    };
    let draft = feature("Draft1");
    assert_eq!(draft["definition"]["value"]["kind"], "draft");
    let operation = &draft["definition"]["value"]["value"];
    assert_eq!(operation["anchor"]["kind"], "neutral_plane");
    assert_eq!(
        operation["anchor"]["pull"]["direction"],
        json!({"x": 0.0, "y": 0.0, "z": 1.0})
    );
    assert!((operation["angle"].as_f64().unwrap() - 10.0_f64.to_radians()).abs() < 1e-12);
    assert_eq!(feature("Draft2")["definition"]["value"]["kind"], "native");
}

#[test]
fn a_thread_maps_to_a_cosmetic_thread_with_its_designation() {
    let document = json!({
        "format": "solidcraft/57",
        "name": "Threads",
        "units": "mm",
        "params": [],
        "features": [
            {"id": 1, "name": "Thread1", "suppressed": false, "type": "thread",
             "face": [20.0, 10.0, 5.0], "face_names": ["F1:bore"],
             "designation": "M6", "continuous_flanks": true},
            {"id": 2, "name": "Thread2", "suppressed": false, "type": "thread",
             "face": [20.0, 10.0, 5.0], "face_names": ["F1:bore"],
             "designation": "1/4-20 UNC"}
        ]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let features = features(&bytes);
    let thread = |name: &str| {
        features
            .iter()
            .find(|feature| feature["name"] == name)
            .unwrap()["definition"]["value"]
            .clone()
    };
    let metric = thread("Thread1");
    assert_eq!(metric["kind"], "cosmetic_thread");
    assert_eq!(metric["value"]["standard"], "metric");
    assert_eq!(metric["value"]["designation"], "M6");
    assert!(metric["value"].get("diameter").is_none());
    assert!(metric["value"].get("pitch").is_none());
    assert!(metric["value"].get("modeled").is_none());
    let other = thread("Thread2");
    assert_eq!(other["kind"], "cosmetic_thread");
    assert!(other["value"].get("standard").is_none());
    assert_eq!(other["value"]["designation"], "1/4-20 UNC");
}

#[test]
fn components_occurrences_and_joints_form_the_product_structure() {
    let identity = json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ]);
    let moved = json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [10.0, 20.0, 30.0, 1.0]
    ]);
    let document = json!({
        "format": "solidcraft/57",
        "name": "Hinge",
        "units": "mm",
        "params": [],
        "components": [
            {"id": 1, "name": "Leaf", "parent": 0, "identification_color_index": 0},
            {"id": 5, "name": "Pin", "parent": 1, "identification_color_index": 1}
        ],
        "occurrences": [
            {"id": 2, "component": 1, "parent": 0, "name": "Leaf:1", "transform": identity, "grounded": true},
            {"id": 3, "component": 1, "parent": 0, "name": "Leaf:2", "transform": moved, "grounded": false},
            {"id": 6, "component": 5, "parent": 1, "name": "Pin:1", "transform": moved, "grounded": false}
        ],
        "assembly": {
            "joints": [
                {"id": 1, "name": "Hinge", "kind": "revolute", "a": {"occurrence": 2, "snap":
                    {"type": "frame", "origin": [1.0, 2.0, 3.0], "x": [0.0, 1.0, 0.0], "z": [0.0, 0.0, 1.0]}},
                 "b": {"occurrence": 3, "snap": {"type": "point", "point": [0.0, 0.0, 0.0]}},
                 "values": [0.5], "limits": [[-1.0, 1.0]], "flip": false, "offset": 2.0,
                 "offset_xy": [1.0, 0.0], "angle": 0.0},
                {"id": 2, "name": "Pivot", "kind": "revolute", "a": {"occurrence": 2, "snap":
                    {"type": "circle_center", "pick": [1.0, 37.0, 4.0]}},
                 "b": {"occurrence": 3, "snap": {"type": "point", "point": [0.0, 0.0, 0.0]}},
                 "values": [0.0], "limits": [null], "flip": true, "offset": 0.0, "offset_xy": [0.0, 0.0], "angle": 0.0}
            ],
            "links": [{"a": 1, "b": 2, "ratio": 1.0}]
        },
        "features": [
            {"id": 7, "name": "Block", "suppressed": false, "type": "box", "component": 1,
             "corner": [0, 0, 0], "length": "4", "width": "4", "height": "4", "operation": "new_body"}
        ]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let result = SolidcraftCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let model = serde_json::to_value(&result.ir().model).unwrap();
    let ids = |key: &str| -> Vec<String> {
        model[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                row["id"]
                    .as_str()
                    .unwrap()
                    .rsplit('#')
                    .next()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    };
    assert_eq!(ids("product_definitions"), ["0", "1", "5"]);
    // Each placement is one occurrence, owned by the definition it sits in:
    // the design places the two leaves, and the leaf places the pin once.
    assert_eq!(ids("occurrences"), ["2", "3", "6"]);
    let owned = |definition: &str| -> Vec<String> {
        let row = model["product_definitions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"].as_str().unwrap().ends_with(definition))
            .unwrap();
        row["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().rsplit('#').next().unwrap().to_owned())
            .collect()
    };
    assert_eq!(owned("#0"), ["2", "3"]);
    assert_eq!(owned("#1"), ["6"]);
    assert!(owned("#5").is_empty());
    let pin = model["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"].as_str().unwrap().ends_with("#6"))
        .unwrap();
    assert_eq!(pin["prototype"], "solidcraft:design:product#5");
    assert_eq!(pin["transform"][0], json!([1.0, 0.0, 0.0, 10.0]));
    assert_eq!(pin["transform"][2], json!([0.0, 0.0, 1.0, 30.0]));
    assert_eq!(
        model["features"][0]["product_definition"],
        "solidcraft:design:product#1"
    );
    // The first joint has frames; the second snaps to geometry.
    assert_eq!(ids("assembly_joints"), ["1", "grounded%3A2"]);
    let joint = &model["assembly_joints"][0]["operands"]["value"];
    assert_eq!(joint["kind"]["kind"], "revolute");
    assert_eq!(joint["kind"]["angular_limits"]["maximum"], 1.0);
    assert_eq!(joint["frame_sense"], "opposed");
    assert_eq!(
        joint["connectors"][0]["frame"][1],
        json!([1.0, 0.0, 0.0, 2.0])
    );
    assert_eq!(joint["offset_frames"][1][2][3], 2.0);
    let codes = loss_codes(&bytes);
    assert!(codes.contains(&"solidcraft/assembly.joint-unresolved".to_owned()));
    assert!(codes.contains(&"solidcraft/assembly.links-undecoded".to_owned()));
}

#[test]
fn named_views_and_hidden_occurrences_are_presentation() {
    let mut document = box_document("solidcraft/57");
    document["named_views"] = json!([
        {"name": "Front", "target": [1.0, 2.0, 3.0], "yaw": 0.0, "pitch": 0.0, "distance": 10.0, "fov": 0.0}
    ]);
    document["components"] = json!([{"id": 1, "name": "Leaf", "parent": 0}]);
    document["occurrences"] = json!([
        {"id": 2, "component": 1, "parent": 0, "name": "Leaf:1", "grounded": false}
    ]);
    document["visibility"] = json!({"hidden_occurrences": [[2]]});
    let bytes = serde_json::to_vec(&document).unwrap();
    let result = SolidcraftCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let model = serde_json::to_value(&result.ir().model).unwrap();
    let state = &model["presentation_documents"][0]["states"][0];
    assert_eq!(state["attributes"]["name"], "Front");
    assert_eq!(state["kind"]["value"]["position"], json!([1.0, -8.0, 3.0]));
    assert_eq!(
        state["kind"]["value"]["properties"]["projection"],
        "orthographic"
    );
    let hidden = model["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"].as_str().unwrap().ends_with("#2"))
        .unwrap();
    assert_eq!(hidden["visible"], false);
}

#[test]
fn an_earlier_design_decodes_through_the_upgrade() {
    let document = json!({
        "format": "solidcraft/5",
        "name": "Old",
        "units": "mm",
        "params": [],
        "features": [
            {"id": 1, "name": "S", "suppressed": false, "type": "sketch",
             "plane": {"type": "origin", "name": "XZ"},
             "sketch": {"points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true},
                                   {"id": "c", "pos": [0.0, 0.0]}],
                        "curves": [{"id": "c1", "type": "circle", "c": 1, "r": 2.0}], "constraints": []}},
            {"id": 2, "name": "E", "suppressed": false, "type": "extrude", "sketch": 1,
             "profiles": {"type": "all"}, "operation": "new_body",
             "extent": {"distance": "10", "direction": "negative", "distance2": "4"}}
        ]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let result = SolidcraftCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    // The old XZ plane keeps its frame: x along +X, y along +Z.
    let sketch = serde_json::to_value(&result.ir().model.sketches[0]).unwrap();
    assert_eq!(
        sketch["placement"]["frame"]["normal"],
        json!({"x": 0.0, "y": -1.0, "z": 0.0})
    );
    // The negative direction of a two-sided extrude was ignored, and the
    // second side followed the first across the plane.
    let features = features(&bytes);
    let extrude = &features[1]["definition"]["value"]["value"];
    assert_eq!(extrude["direction"], json!({"kind": "profile_normal"}));
    assert_eq!(
        extrude["extent"],
        json!({"kind": "two_sided",
               "first": {"termination": {"kind": "blind", "length": 10.0}},
               "second": {"termination": {"kind": "blind", "length": 4.0}}})
    );
    assert!(!loss_codes(&bytes).contains(&"solidcraft/history.features-undecoded".to_owned()));
    // The frame of the XZ plane came from the upgrade, so it is derived.
    let result = cadmpeg_test_support::EditableDecodeResult::from(result);
    let fidelity = serde_json::to_value(result.source_fidelity()).unwrap();
    assert_eq!(
        fidelity["annotations"]["exactness"]["solidcraft:design:sketch#1"]["fields"]["placement"],
        "derived"
    );
}

#[test]
fn a_current_two_sided_extrude_keeps_the_side_of_each_distance() {
    let extrude = |direction: &str, distance: &str, follows: bool| {
        let mut extent = json!({"distance": distance, "direction": direction, "distance2": "4"});
        if follows {
            extent["second_follows_first"] = json!(true);
        }
        let document = json!({
            "format": "solidcraft/57", "name": "T", "units": "mm", "params": [],
            "features": [
                {"id": 1, "name": "S", "suppressed": false, "type": "sketch",
                 "plane": {"type": "origin", "name": "XY"},
                 "sketch": {"points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true},
                                       {"id": "c", "pos": [0.0, 0.0]}],
                            "curves": [{"id": "c1", "type": "circle", "c": 1, "r": 2.0}], "constraints": []}},
                {"id": 2, "name": "E", "suppressed": false, "type": "extrude", "sketch": 1,
                 "profiles": {"type": "all"}, "operation": "new_body", "extent": extent}
            ]
        });
        let features = features(&serde_json::to_vec(&document).unwrap());
        features[1]["definition"]["value"]["value"].clone()
    };
    let negative = extrude("negative", "10", false);
    assert_eq!(
        negative["direction"],
        json!({"kind": "reversed_profile_normal"})
    );
    assert_eq!(negative["extent"]["second"]["termination"]["length"], 4.0);
    // A negative distance turns its own side over: the extrude has no
    // neutral two-sided form, because both sides lie against the normal.
    assert_eq!(
        extrude("positive", "-10", false)["kind"],
        "solidcraft:extrude"
    );
    // An older design's second side follows the first across the plane.
    let follows = extrude("positive", "-10", true);
    assert_eq!(
        follows["direction"],
        json!({"kind": "reversed_profile_normal"})
    );
}

#[test]
fn values_the_document_does_not_state_are_not_byte_exact() {
    let document = json!({
        "format": "solidcraft/57",
        "name": "Exact",
        "units": "mm",
        "params": [
            {"name": "w", "expr": "20 mm", "unit": "mm", "model": false},
            {"name": "h", "expr": "w * 2", "unit": "mm", "model": false}
        ],
        "features": [
            {"id": 1, "name": "Sketch1", "suppressed": false, "type": "sketch",
             "plane": {"type": "origin", "name": "XY"},
             "sketch": {"points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true},
                                   {"id": "c", "pos": [0.0, 0.0]}],
                        "curves": [{"id": "c1", "type": "circle", "c": 1, "r": 2.0}],
                        "constraints": [{"id": "k1", "type": "radius", "c": 2, "value": 2.0}]}}
        ]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let result = SolidcraftCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let result = cadmpeg_test_support::EditableDecodeResult::from(result);
    let fidelity = serde_json::to_value(result.source_fidelity()).unwrap();
    let exactness = &fidelity["annotations"]["exactness"];
    // The value of h follows from its expression, so it is derived.
    let h = &exactness["solidcraft:design:parameter#h"];
    assert!(
        h.is_object(),
        "a derived parameter is annotated: {exactness}"
    );
    // The expression of a dimension parameter is this decoder's own text.
    let dimension = &exactness["solidcraft:design:parameter#1%3Ak1"];
    assert!(
        dimension.is_object(),
        "a dimension parameter is annotated: {exactness}"
    );
}

#[test]
fn a_body_transfer_reparents_the_body_to_its_component() {
    let identity = json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ]);
    let document = json!({
        "format": "solidcraft/57",
        "name": "Transfer",
        "units": "mm",
        "params": [],
        "components": [{"id": 4, "name": "Sub", "parent": 0}],
        "occurrences": [{"id": 5, "component": 4, "parent": 0, "name": "Sub:1", "transform": identity, "grounded": false}],
        "features": [
            {"id": 1, "name": "Box1", "suppressed": false, "type": "box", "corner": [0, 0, 0],
             "length": "4", "width": "4", "height": "4", "operation": "new_body", "body_names": ["Body1"]},
            {"id": 2, "name": "Move", "suppressed": false, "type": "body_transfer",
             "bodies": ["Body1"], "from": 0, "to": 4, "component": 4, "transform": identity}
        ]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let features = features(&bytes);
    let transfer = &features[1]["definition"]["value"];
    assert_eq!(features[1]["source_tag"], "body_transfer");
    assert_eq!(transfer["kind"], "component_transfer");
    assert_eq!(transfer["value"]["mode"], "reparent");
    assert_eq!(
        transfer["value"]["source_owner"],
        "solidcraft:design:product#0"
    );
    assert_eq!(
        transfer["value"]["target_owner"],
        "solidcraft:design:product#4"
    );
    assert_eq!(
        transfer["value"]["target_transform"][0],
        json!([1.0, 0.0, 0.0, 0.0])
    );
    assert!(!loss_codes(&bytes).contains(&"solidcraft/history.features-undecoded".to_owned()));
}

#[test]
fn a_pending_format_names_only_the_changes_it_does_not_read() {
    let mut document = box_document("solidcraft/58");
    document["format_pending"] = json!(["joint-lateral-offsets", "sketch-3d-relations"]);
    let bytes = serde_json::to_vec(&document).unwrap();
    let codes = loss_codes(&bytes);
    let unread = codes
        .iter()
        .filter(|code| code.as_str() == "solidcraft/document.pending-change-unread")
        .count();
    assert_eq!(unread, 1, "one loss names the unread changes: {codes:?}");
    let result = SolidcraftCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let report = serde_json::to_value(result.report()).unwrap();
    assert_eq!(
        report["identity"]["dialects"]["primary"]["dialect"],
        "solidcraft:json-v58-pending"
    );
}

#[test]
fn body_and_face_looks_colour_the_cached_body() {
    let bytes = crate::test_support::cached_design_editing(|document| {
        document["appearances"] = json!({
            "bodies": {"Body1": {"name": "Custom", "color": [255, 0, 0], "opacity": 0.5}},
            "faces": [{"body": "Body1", "point": [10.0, 0.0, 2.0], "name": "F2:side",
                       "look": {"name": "Blue", "color": [0, 0, 255], "opacity": 1.0,
                                "pbr": {"metallic": 0.25, "roughness": 0.5, "dielectric_ior": 1.5}}}]
        });
        document["materials"] = json!({"Body1": "Resin"});
        document["user_materials"] = json!([{"name": "Resin", "density": 1.2,
            "appearance": {"name": "Resin", "color": [10, 20, 30], "opacity": 1.0}}]);
        document["visibility"] = json!({"hidden_bodies": ["Body1"]});
    });
    let result = SolidcraftCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let geometry = &result.ir().geometry;
    let body = geometry.bodies[0].color.unwrap();
    assert_eq!(
        (body.r(), body.g(), body.b(), body.a()),
        (1.0, 0.0, 0.0, 0.5)
    );
    let coloured: Vec<_> = geometry
        .faces
        .iter()
        .filter_map(|face| face.color)
        .collect();
    assert_eq!(coloured.len(), 1);
    assert_eq!((coloured[0].r(), coloured[0].b()), (0.0, 1.0));
    assert_eq!(geometry.bodies[0].visible, Some(false));
    let model = serde_json::to_value(&result.ir().model).unwrap();
    let bindings = model["appearance_bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 2);
    let body = bindings
        .iter()
        .find(|binding| binding["target"]["entity"]["kind"] == "body")
        .unwrap();
    assert_eq!(body["channels"]["physical_material"], "Resin");
    let face = bindings
        .iter()
        .find(|binding| binding["target"]["entity"]["kind"] == "face")
        .unwrap();
    let look = model["appearances"]
        .as_array()
        .unwrap()
        .iter()
        .find(|look| look["id"] == face["appearance"])
        .unwrap();
    assert_eq!(look["name"], "Blue");
    assert_eq!(look["properties"]["metallic"], 0.25);
    assert_eq!(model["physical_materials"][0]["density"], 1.2);
}

mod replay {
    use std::io::Cursor;

    use cadmpeg_core::decode::DecodeDepth;
    use cadmpeg_ir::codec::write::target::TargetRequest;
    use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_test_support::EditableDecodeResult;

    use crate::test_support::{box_document, container, CACHED_DESIGN};
    use crate::SolidcraftCodec;

    fn full() -> DecodeOptions {
        DecodeOptions {
            depth: DecodeDepth::Full,
            ..DecodeOptions::default()
        }
    }

    fn replayed(bytes: &[u8], request: TargetRequest<'_>) -> Vec<u8> {
        let decoded = EditableDecodeResult::from(
            SolidcraftCodec
                .decode(&mut Cursor::new(bytes), &full())
                .unwrap(),
        );
        let plan = SolidcraftCodec
            .plan(
                EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
                request,
            )
            .unwrap();
        let mut out = Vec::new();
        plan.write_to(&mut out).unwrap();
        out
    }

    #[test]
    fn an_unedited_container_is_written_back_byte_for_byte() {
        assert_eq!(
            replayed(CACHED_DESIGN, TargetRequest::Inherit),
            CACHED_DESIGN
        );
        let plain = container(&box_document("solidcraft/57"), &[]);
        assert_eq!(replayed(&plain, TargetRequest::Inherit), plain);
    }

    #[test]
    fn an_unedited_plain_json_design_is_written_back_byte_for_byte() {
        let json = serde_json::to_vec_pretty(&box_document("solidcraft/57")).unwrap();
        assert_eq!(replayed(&json, TargetRequest::Inherit), json);
    }

    fn plan_bytes(decoded: &EditableDecodeResult, request: TargetRequest<'_>) -> Vec<u8> {
        let plan = SolidcraftCodec
            .plan(
                EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
                request,
            )
            .unwrap();
        let mut out = Vec::new();
        plan.write_to(&mut out).unwrap();
        out
    }

    fn features(bytes: &[u8]) -> Vec<(String, String)> {
        let decoded = EditableDecodeResult::from(
            SolidcraftCodec
                .decode(&mut Cursor::new(bytes), &full())
                .unwrap(),
        );
        decoded
            .ir()
            .model
            .features
            .iter()
            .map(|feature| {
                (
                    feature.name.clone().unwrap_or_default(),
                    serde_json::to_string(feature).unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn an_explicit_dialect_converts_between_the_two_encodings() {
        let json = serde_json::to_vec_pretty(&box_document("solidcraft/57")).unwrap();
        let decoded = EditableDecodeResult::from(
            SolidcraftCodec
                .decode(&mut Cursor::new(&json), &full())
                .unwrap(),
        );
        let packed = plan_bytes(
            &decoded,
            TargetRequest::Explicit("solidcraft:container-1-v57"),
        );
        assert!(packed.starts_with(b"PK"));
        assert_eq!(features(&packed), features(&json));
        assert_eq!(expressions(&packed), expressions(&json));
        let again = EditableDecodeResult::from(
            SolidcraftCodec
                .decode(&mut Cursor::new(&packed), &full())
                .unwrap(),
        );
        let plain = plan_bytes(&again, TargetRequest::Explicit("solidcraft:json-v57"));
        assert_eq!(features(&plain), features(&json));
    }

    #[test]
    fn a_source_less_model_is_generated_and_read_back() {
        let json = serde_json::to_vec_pretty(&box_document("solidcraft/57")).unwrap();
        let mut decoded = EditableDecodeResult::from(
            SolidcraftCodec
                .decode(&mut Cursor::new(&json), &full())
                .unwrap(),
        );
        decoded.ir_mut().source = None;
        let plan = SolidcraftCodec
            .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
            .unwrap();
        let mut out = Vec::new();
        plan.write_to(&mut out).unwrap();
        assert!(out.starts_with(b"PK"));
        assert_eq!(features(&out), features(&json));
        assert_eq!(expressions(&out), expressions(&json));
    }

    #[test]
    fn an_unlisted_dialect_is_refused_with_the_catalog() {
        let json = serde_json::to_vec_pretty(&box_document("solidcraft/57")).unwrap();
        let decoded = EditableDecodeResult::from(
            SolidcraftCodec
                .decode(&mut Cursor::new(&json), &full())
                .unwrap(),
        );
        let error = SolidcraftCodec
            .plan(
                EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
                TargetRequest::Explicit("solidcraft:json-v56"),
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("solidcraft:container-1-v57"),
            "{error}"
        );
    }

    #[test]
    fn a_feature_without_a_serializer_is_refused_by_name() {
        let mut document = box_document("solidcraft/57");
        document["features"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id": 2, "name": "Round", "suppressed": false, "type": "fillet",
                "radius": "1 mm", "edges": [[0.0, 0.0, 2.0]]
            }));
        let json = serde_json::to_vec_pretty(&document).unwrap();
        let mut decoded = EditableDecodeResult::from(
            SolidcraftCodec
                .decode(&mut Cursor::new(&json), &full())
                .unwrap(),
        );
        decoded.ir_mut().source = None;
        let error = SolidcraftCodec
            .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
            .unwrap_err();
        assert!(error.to_string().contains("Round"), "{error}");
    }

    #[test]
    fn an_edited_design_is_refused_instead_of_replayed_stale() {
        let json = serde_json::to_vec_pretty(&box_document("solidcraft/57")).unwrap();
        let mut decoded = EditableDecodeResult::from(
            SolidcraftCodec
                .decode(&mut Cursor::new(&json), &full())
                .unwrap(),
        );
        decoded.ir_mut().source.as_mut().unwrap().attributes.insert(
            cadmpeg_core::nonblank_const!("document_name"),
            "Edited".to_owned(),
        );
        let error = SolidcraftCodec
            .plan(
                EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
                TargetRequest::Inherit,
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("beyond parameter expressions"),
            "{error}"
        );
    }

    fn with_expression(bytes: &[u8], name: &str, expression: &str) -> Vec<u8> {
        let mut decoded = EditableDecodeResult::from(
            SolidcraftCodec
                .decode(&mut Cursor::new(bytes), &full())
                .unwrap(),
        );
        for parameter in &mut decoded.ir_mut().model.parameters {
            if parameter.name == name {
                parameter.expression = expression.to_owned();
            }
        }
        let plan = SolidcraftCodec
            .plan(
                EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
                TargetRequest::Inherit,
            )
            .unwrap();
        let mut out = Vec::new();
        plan.write_to(&mut out).unwrap();
        out
    }

    fn expressions(bytes: &[u8]) -> Vec<(String, String)> {
        let decoded = EditableDecodeResult::from(
            SolidcraftCodec
                .decode(&mut Cursor::new(bytes), &full())
                .unwrap(),
        );
        decoded
            .ir()
            .model
            .parameters
            .iter()
            .map(|parameter| (parameter.name.clone(), parameter.expression.clone()))
            .collect()
    }

    #[test]
    fn an_edited_parameter_is_patched_into_a_plain_json_design() {
        let json = serde_json::to_vec_pretty(&box_document("solidcraft/57")).unwrap();
        let out = with_expression(&json, "w", "35 mm");
        assert_eq!(
            expressions(&out),
            vec![("w".to_owned(), "35 mm".to_owned())]
        );
        assert!(serde_json::from_slice::<serde_json::Value>(&out).is_ok());
    }

    #[test]
    fn an_edited_container_is_repacked_without_its_stale_cache() {
        let source = crate::test_support::cached_design_editing(|document| {
            document["params"] =
                serde_json::json!([{"name": "w", "expr": "20 mm", "unit": "mm", "model": false}]);
        });
        let out = with_expression(&source, "w", "35 mm");
        let before = crate::test_support::members(&source);
        let after = crate::test_support::members(&out);
        assert!(before.iter().any(|(name, _)| name.starts_with("cache/")));
        assert!(!after.iter().any(|(name, _)| name.starts_with("cache/")));
        assert_eq!(
            after.len(),
            before
                .iter()
                .filter(|(name, _)| !name.starts_with("cache/"))
                .count()
        );
        assert!(expressions(&out).iter().any(|(_, text)| text == "35 mm"));
    }

    /// Writes an unedited and an edited container for the native check
    /// (`scripts/verify-solidcraft-cli.py`) into `SOLIDCRAFT_WRITE_DIR`.
    #[test]
    #[ignore = "feeds scripts/verify-solidcraft-cli.py"]
    fn write_files_for_the_native_check() {
        let Some(dir) = std::env::var_os("SOLIDCRAFT_WRITE_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let source = crate::test_support::cached_design_editing(|document| {
            document["params"] =
                serde_json::json!([{"name": "w", "expr": "20 mm", "unit": "mm", "model": false}]);
            document["features"][0]["length"] = serde_json::json!("w");
        });
        std::fs::write(dir.join("source.sc"), &source).unwrap();
        std::fs::write(
            dir.join("edited.sc"),
            with_expression(&source, "w", "35 mm"),
        )
        .unwrap();
        let json = serde_json::to_vec_pretty(&box_document("solidcraft/57")).unwrap();
        std::fs::write(dir.join("source.json.sc"), &json).unwrap();
        std::fs::write(
            dir.join("edited.json.sc"),
            with_expression(&json, "w", "35 mm"),
        )
        .unwrap();
        // Generated from the model alone, without its source.
        let mut decoded = EditableDecodeResult::from(
            SolidcraftCodec
                .decode(&mut Cursor::new(&json), &full())
                .unwrap(),
        );
        decoded.ir_mut().source = None;
        for (name, request) in [
            (
                "generated.sc",
                TargetRequest::Explicit("solidcraft:container-1-v57"),
            ),
            (
                "generated.json.sc",
                TargetRequest::Explicit("solidcraft:json-v57"),
            ),
        ] {
            let plan = SolidcraftCodec
                .plan(EncodeInput::new(decoded.ir(), None), request)
                .unwrap();
            let mut out = Vec::new();
            plan.write_to(&mut out).unwrap();
            std::fs::write(dir.join(name), out).unwrap();
        }
    }
}

#[test]
fn a_pattern_about_a_model_axis_takes_the_axis_it_was_made_with() {
    let document = json!({
        "format": "solidcraft/57", "name": "Ring", "units": "mm", "params": [],
        "features": [
            {"id": 1, "name": "Hole1", "suppressed": false, "type": "cylinder",
             "base": [22, 0, 0], "axis": [0.0, 0.0, 1.0], "radius": "3", "height": "5",
             "operation": "new_body"},
            {"id": 2, "name": "AboutFace", "suppressed": false, "type": "pattern", "features": [1],
             "pattern": {"type": "circular", "origin": [0.0, 0.0, 10.0], "axis": [0.0, 0.0, 1.0],
                         "axis_ref": {"type": "face", "name": "F2:side", "at": [15.0, 0.0, 20.0]},
                         "count": "5", "angle": "360 deg"}},
            {"id": 3, "name": "AboutAxis", "suppressed": false, "type": "pattern", "features": [1],
             "pattern": {"type": "circular", "origin": [0.0, 0.0, 0.0], "axis": [0.0, 1.0, 0.0],
                         "axis_feature": "Axis1", "count": "3", "angle": "360 deg"}}
        ]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let mut features = features(&bytes);
    features.sort_by_key(|feature| feature["ordinal"].as_u64());
    for feature in &features[1..] {
        assert_eq!(
            feature["definition"]["value"]["kind"], "pattern",
            "{feature}"
        );
        assert_eq!(
            feature["definition"]["value"]["value"]["pattern"]["kind"],
            "circular"
        );
    }
    let first = &features[1]["definition"]["value"]["value"]["pattern"];
    assert_eq!(first["count"], 5);
    assert_eq!(first["axis_origin"], json!({"x": 0.0, "y": 0.0, "z": 10.0}));
}

#[test]
fn a_sweep_carries_its_orientation_twist_and_scale_and_keeps_rails_native() {
    let path = json!({"id": 1, "name": "Path", "suppressed": false, "type": "sketch",
        "plane": {"type": "origin", "name": "XZ"},
        "sketch": {"points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true},
                              {"id": "p1", "pos": [0.0, 0.0]}, {"id": "p2", "pos": [0.0, 30.0]}],
                   "curves": [{"id": "l1", "type": "line", "a": 1, "b": 2}], "constraints": []}});
    let section = json!({"id": 2, "name": "Section", "suppressed": false, "type": "sketch",
        "plane": {"type": "origin", "name": "XY"},
        "sketch": {"points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true},
                              {"id": "c", "pos": [0.0, 0.0]}],
                   "curves": [{"id": "c1", "type": "circle", "c": 1, "r": 2.0}], "constraints": []}});
    let sweep = |id: u64, extra: serde_json::Value| {
        let mut record = json!({"id": id, "name": format!("Sweep{id}"), "suppressed": false,
            "type": "sweep", "sketch": 2, "profiles": {"type": "all"}, "path_sketch": 1,
            "path": ["l1"], "operation": "new_body"});
        for (key, value) in extra.as_object().unwrap() {
            record[key] = value.clone();
        }
        record
    };
    let document = json!({"format": "solidcraft/57", "name": "Sweeps", "units": "mm", "params": [],
        "features": [path, section,
            sweep(3, json!({"twist": "90 deg", "scale": "0.5", "orientation": "parallel"})),
            sweep(4, json!({"rails": [{"sketch": 1, "curves": ["l1"]}]})),
            sweep(5, json!({"orientation": {"direction": [1.0, 0.0, 0.0]}}))]});
    let bytes = serde_json::to_vec(&document).unwrap();
    let mut features = features(&bytes);
    features.sort_by_key(|feature| feature["ordinal"].as_u64());
    let value = |index: usize| &features[index]["definition"]["value"];
    assert_eq!(value(2)["kind"], "sweep");
    assert_eq!(value(2)["value"]["orientation"]["kind"], "fixed");
    assert!(
        (value(2)["value"]["twist"].as_f64().unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-9
    );
    assert_eq!(value(2)["value"]["scale"], 0.5);
    assert_eq!(value(3)["kind"], "native");
    assert_eq!(value(4)["kind"], "native");
}

#[test]
fn a_loft_carries_its_rails() {
    let line = |id: u64, name: &str, plane: &str, top: f64| {
        json!({"id": id, "name": name,
        "suppressed": false, "type": "sketch", "plane": {"type": "origin", "name": plane},
        "sketch": {"points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true},
                              {"id": "p1", "pos": [0.0, 0.0]}, {"id": "p2", "pos": [0.0, top]}],
                   "curves": [{"id": "l1", "type": "line", "a": 1, "b": 2}], "constraints": []}})
    };
    let circle = |id: u64, name: &str, radius: f64| {
        json!({"id": id, "name": name,
        "suppressed": false, "type": "sketch", "plane": {"type": "origin", "name": "XY"},
        "sketch": {"points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true},
                              {"id": "c", "pos": [0.0, 0.0]}],
                   "curves": [{"id": "c1", "type": "circle", "c": 1, "r": radius}], "constraints": []}})
    };
    let loft = |id: u64, extra: serde_json::Value| {
        let mut record = json!({"id": id, "name": format!("Loft{id}"), "suppressed": false,
            "type": "loft", "operation": "new_body",
            "sections": [{"sketch": 1, "profiles": {"type": "all"}}, {"sketch": 2, "profiles": {"type": "all"}}]});
        for (key, value) in extra.as_object().unwrap() {
            record[key] = value.clone();
        }
        record
    };
    let document = json!({"format": "solidcraft/57", "name": "Lofts", "units": "mm", "params": [],
        "features": [circle(1, "A", 5.0), circle(2, "B", 3.0), line(3, "Rail", "XZ", 20.0),
            loft(4, json!({"rails": [{"sketch": 3, "curves": ["l1"]}]})),
            loft(5, json!({"rails": [{"sketch": 3, "curves": ["l1"]}], "rail_fit": "stretch"}))]});
    let bytes = serde_json::to_vec(&document).unwrap();
    let mut features = features(&bytes);
    features.sort_by_key(|feature| feature["ordinal"].as_u64());
    let value = |index: usize| &features[index]["definition"]["value"];
    assert_eq!(value(3)["kind"], "loft");
    assert_eq!(
        value(3)["value"]["guidance"]["value"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(value(4)["kind"], "native");
}

#[test]
fn conics_and_nurbs_splines_become_neutral_nurbs_entities() {
    let document = json!({"format": "solidcraft/57", "name": "Curves", "units": "mm", "params": [],
        "features": [{"id": 1, "name": "S", "suppressed": false, "type": "sketch",
            "plane": {"type": "origin", "name": "XY"},
            "sketch": {"points": [
                {"id": "origin", "pos": [0.0, 0.0], "fixed": true},
                {"id": "a", "pos": [0.0, 0.0]}, {"id": "b", "pos": [10.0, 0.0]},
                {"id": "apex", "pos": [5.0, 8.0]},
                {"id": "q1", "pos": [0.0, 20.0]}, {"id": "q2", "pos": [5.0, 30.0]},
                {"id": "q3", "pos": [10.0, 20.0]}],
              "curves": [
                {"id": "k1", "type": "conic", "a": 1, "b": 2, "apex": 3, "rho": 0.5},
                {"id": "s1", "type": "spline", "pts": [4, 5, 6], "control": true, "degree": 2,
                 "nurbs": {"knots": [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], "weights": [1.0, 2.0, 1.0]}}],
              "constraints": []}}]});
    let decoded = EditableDecodeResult::from(
        SolidcraftCodec
            .decode(
                &mut Cursor::new(serde_json::to_vec(&document).unwrap()),
                &DecodeOptions::default(),
            )
            .unwrap(),
    );
    let entities = serde_json::to_value(&decoded.ir().model.sketch_entities).unwrap();
    let entities = entities.as_array().unwrap();
    assert_eq!(entities.len(), 2, "{entities:?}");
    let weights: Vec<f64> = entities[0]["geometry"]["curve"]["poles"]["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pole| pole["weight"].as_f64().unwrap())
        .collect();
    assert_eq!(weights, [1.0, 1.0, 1.0]);
    assert_eq!(
        entities[1]["geometry"]["curve"]["poles"]["points"][1]["weight"],
        2.0
    );
}

#[test]
fn a_point_cloud_decodes_placed_in_the_design() {
    let mut blob = b"SCPNTS01".to_vec();
    blob.extend_from_slice(&2_u64.to_le_bytes());
    blob.extend_from_slice(&1_u64.to_le_bytes());
    for value in [1.0_f32, 0.0, 0.0, 0.0, 2.0, 0.0] {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
    let name = blob_name(&blob, "points");
    let mut document = box_document("solidcraft/57");
    document["point_clouds"] = json!([{
        "id": 1, "name": "Scan", "file": "scan.xyz", "origin": [10.0, 0.0, 0.0],
        "points": "", "points_blob": name, "units": "mm",
        "transform": [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 5.0], [0.0, 0.0, 1.0, 0.0],
                      [0.0, 0.0, 0.0, 1.0]],
    }]);
    let bytes = container(&document, &[(&name, &blob)]);
    let decoded = EditableDecodeResult::from(
        SolidcraftCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap(),
    );
    let clouds = serde_json::to_value(&decoded.ir().model.point_clouds).unwrap();
    let cloud = &clouds[0];
    assert_eq!(cloud["name"], "Scan");
    assert_eq!(
        cloud["positions"][0],
        json!({"x": 11.0, "y": 5.0, "z": 0.0})
    );
    assert_eq!(
        cloud["positions"][1],
        json!({"x": 10.0, "y": 7.0, "z": 0.0})
    );
    assert_eq!(cloud["colors"][1]["b"], 1.0);
}

#[test]
fn a_note_decodes_to_a_text_annotation_at_its_anchor() {
    let mut document = box_document("solidcraft/57");
    document["notes"] = json!([
        {"id": 1, "anchor": [1.0, 2.0, 3.0], "text": "Check clearance\nhere"},
        {"id": 2, "anchor": [0.0, 0.0, 0.0], "text": "Hidden one", "hidden": true},
    ]);
    let bytes = serde_json::to_vec(&document).unwrap();
    let decoded = EditableDecodeResult::from(
        SolidcraftCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap(),
    );
    let notes = serde_json::to_value(&decoded.ir().model.semantic_annotations).unwrap();
    let messages: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .map(|loss| loss.message.clone())
        .collect();
    assert_eq!(notes.as_array().unwrap().len(), 2, "{messages:?}");
    assert_eq!(notes[0]["kind"], "text");
    assert_eq!(notes[0]["text"], json!(["Check clearance", "here"]));
    assert_eq!(notes[1]["parameters"]["hidden"], "true");
}

#[test]
fn a_canvas_image_decodes_to_an_embedded_asset() {
    let png = b"\x89PNG\r\n\x1a\nimage";
    let name = blob_name(png, "png");
    let mut document = box_document("solidcraft/57");
    document["canvases"] = json!([{
        "id": 1, "name": "Front", "format": "png", "data": "", "image_blob": name,
        "pixels": [2, 2], "plane": {"type": "origin", "name": "XZ"}, "center": [0.0, 0.0],
        "width": 10.0, "angle": 0.0, "opacity": 0.5, "visible": true,
    }]);
    let bytes = container(&document, &[(&name, png)]);
    let decoded = EditableDecodeResult::from(
        SolidcraftCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap(),
    );
    let assets = serde_json::to_value(&decoded.ir().model.assets).unwrap();
    assert_eq!(assets.as_array().unwrap().len(), 1, "{assets}");
    assert_eq!(assets[0]["name"], "Front");
    assert_eq!(assets[0]["media_type"], "image/png");
    assert_eq!(assets[0]["content"]["kind"], "embedded");
}

#[test]
fn the_relation_joints_parallel_perpendicular_and_angle_have_neutral_families() {
    let identity = json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ]);
    let snap = json!({"type": "point", "point": [0.0, 0.0, 0.0]});
    let joint = |id: u64, kind: &str, angle: f64| {
        json!({"id": id, "name": kind, "kind": kind,
            "a": {"occurrence": 2, "snap": snap}, "b": {"occurrence": 3, "snap": snap},
            "values": [], "limits": [], "flip": false, "offset": 0.0,
            "offset_xy": [0.0, 0.0], "angle": angle})
    };
    let document = json!({
        "format": "solidcraft/57", "name": "Relations", "units": "mm", "params": [],
        "components": [{"id": 1, "name": "Leaf", "parent": 0, "identification_color_index": 0}],
        "occurrences": [
            {"id": 2, "component": 1, "parent": 0, "name": "A", "transform": identity, "grounded": false},
            {"id": 3, "component": 1, "parent": 0, "name": "B", "transform": identity, "grounded": false}],
        "assembly": {"joints": [
            joint(1, "parallel", 0.0), joint(2, "perpendicular", 0.0), joint(3, "angle", 0.5),
            joint(4, "point_on_plane", 0.0)]},
        "features": []
    });
    let result = SolidcraftCodec
        .decode(
            &mut Cursor::new(serde_json::to_vec(&document).unwrap()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let model = serde_json::to_value(&result.ir().model).unwrap();
    let joints = model["assembly_joints"].as_array().unwrap();
    let kinds: Vec<_> = joints
        .iter()
        .map(|joint| joint["operands"]["value"]["kind"]["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["parallel", "perpendicular", "angle"]);
    assert_eq!(joints[2]["operands"]["value"]["kind"]["angle"], 0.5);
    // The tilt is the kind's, not a turn of the offset frame.
    assert_eq!(
        joints[2]["operands"]["value"]["offset_frames"][1][0],
        json!([1.0, -0.0, 0.0, 0.0])
    );
}
