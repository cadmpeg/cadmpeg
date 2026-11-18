// SPDX-License-Identifier: Apache-2.0
//! Codec-wide inspect and decode over code-built designs.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{archive, RawEntry};
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
    assert_eq!(unknowns, 4);
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
    assert_eq!(features[2]["definition"]["value"]["kind"], "native");
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
fn body_and_face_looks_colour_the_cached_body() {
    let bytes = crate::test_support::cached_design_editing(|document| {
        document["appearances"] = json!({
            "bodies": {"Body1": {"name": "Custom", "color": [255, 0, 0], "opacity": 0.5}},
            "faces": [{"body": "Body1", "point": [10.0, 0.0, 2.0], "name": "F2:side",
                       "look": {"name": "Blue", "color": [0, 0, 255], "opacity": 1.0}}]
        });
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
}
