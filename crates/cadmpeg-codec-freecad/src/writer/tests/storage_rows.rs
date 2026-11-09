// SPDX-License-Identifier: Apache-2.0
//! `FreeCAD` storage normalization rows: a value `FreeCAD` cannot hold
//! re-decodes as the row's stored value, and the row's loss names the record
//! (#238 §5.5, #232 §5.2).

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use serde_json::{json, Value};

use crate::FcstdCodec;

fn decode(name: &str) -> Value {
    let path = format!(
        "{}/../../corpus/freecad_fcstd/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(path).expect("fixture bytes");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("fixture decodes");
    serde_json::to_value(decoded.ir()).expect("CADIR JSON")
}

/// The losses of writing `document` with `pointer` set to `value`, and the
/// re-decoded document.
fn write_edit(document: &Value, pointer: &str, value: Value) -> (Vec<(String, String)>, Value) {
    let mut edited = document.clone();
    let (parent, key) = pointer.rsplit_once('/').expect("edited path");
    let parent = edited.pointer_mut(parent).expect("edited parent");
    match parent {
        Value::Object(fields) => {
            fields.insert(key.to_owned(), value);
        }
        Value::Array(items) => items[key.parse::<usize>().expect("index")] = value,
        _ => panic!("edited parent is a container"),
    }
    let ir = cadmpeg_ir::CadIr::from_json(&edited.to_string()).expect("edited document");
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .expect("edited document plans");
    let losses = plan
        .report()
        .losses
        .iter()
        .map(|loss| (loss.code.to_string(), loss.message.clone()))
        .collect();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("edited document writes");
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("output re-decodes");
    (
        losses,
        serde_json::to_value(output.ir()).expect("CADIR JSON"),
    )
}

fn charged(losses: &[(String, String)], code: &str, record: &str) -> bool {
    losses
        .iter()
        .any(|(charged, message)| charged == code && message.contains(record))
}

/// The record of `arena` with the identity of `document`'s record `index`.
fn record<'a>(output: &'a Value, document: &Value, arena: &str, index: usize) -> &'a Value {
    let id = &document["model"][arena][index]["id"];
    output["model"][arena]
        .as_array()
        .expect("arena")
        .iter()
        .find(|row| &row["id"] == id)
        .expect("record re-decodes")
}

#[test]
fn a_non_rigid_container_placement_keeps_its_translation() {
    let document = decode("core_design_product.FCStd");
    let original = document["model"]["occurrences"][0]["transform"].clone();
    let id = document["model"]["occurrences"][0]["id"].as_str().unwrap();
    let (losses, output) = write_edit(&document, "/model/occurrences/0/transform/0/1", json!(1.0));
    assert!(charged(&losses, "fcstd/product.placement-not-rigid", id));
    let stored = &record(&output, &document, "occurrences", 0)["transform"];
    for row in 0..3 {
        for column in 0..3 {
            assert_eq!(
                stored[row][column],
                json!(f64::from(u8::from(row == column)))
            );
        }
        assert_eq!(stored[row][3], original[row][3]);
    }
}

#[test]
fn a_scaled_container_placement_drops_its_scale() {
    let document = decode("core_design_product.FCStd");
    let original = document["model"]["occurrences"][0]["transform"].clone();
    let id = document["model"]["occurrences"][0]["id"].as_str().unwrap();
    let (losses, output) = write_edit(&document, "/model/occurrences/0/transform/0/0", json!(2.0));
    assert!(charged(&losses, "fcstd/product.placement-not-rigid", id));
    assert_eq!(
        record(&output, &document, "occurrences", 0)["transform"],
        original
    );
}

#[test]
fn a_non_rigid_joint_frame_keeps_its_translation() {
    let document = decode("product_assembly.FCStd");
    let original =
        document["model"]["assembly_joints"][0]["operands"]["connector"]["frame"].clone();
    let id = document["model"]["assembly_joints"][0]["id"]
        .as_str()
        .unwrap();
    let (losses, output) = write_edit(
        &document,
        "/model/assembly_joints/0/operands/connector/frame/0/1",
        json!(1.0),
    );
    assert!(charged(&losses, "fcstd/joint.frame-not-rigid", id));
    assert_eq!(
        record(&output, &document, "assembly_joints", 0)["operands"]["connector"]["frame"],
        original
    );
}

#[test]
fn a_sketch_frame_stores_unit_directions() {
    let document = decode("core_design_product.FCStd");
    let original = document["model"]["sketches"][0]["placement"].clone();
    let id = document["model"]["sketches"][0]["id"].as_str().unwrap();
    let (losses, output) = write_edit(
        &document,
        "/model/sketches/0/placement/frame/normal/z",
        json!(2.0),
    );
    assert!(charged(&losses, "fcstd/sketch.placement-normalized", id));
    assert_eq!(
        record(&output, &document, "sketches", 0)["placement"],
        original
    );
}

#[test]
fn a_one_entity_profile_takes_its_geometry_orientation() {
    let document = decode("core_design_product.FCStd");
    let id = document["model"]["sketches"][0]["id"].as_str().unwrap();
    let (losses, output) = write_edit(
        &document,
        "/model/sketches/0/profiles/0/0/reversed",
        json!(true),
    );
    assert!(charged(
        &losses,
        "fcstd/sketch.profile-orientation-derived",
        id
    ));
    assert_eq!(
        record(&output, &document, "sketches", 0)["profiles"][0][0]["reversed"],
        json!(false)
    );
}

#[test]
fn a_gui_document_is_stored_in_schema_one_with_consecutive_states() {
    let document = decode("gui_appearance.FCStd");
    let id = document["model"]["presentation_documents"][0]["id"]
        .as_str()
        .unwrap();
    let (losses, output) = write_edit(
        &document,
        "/model/presentation_documents/0/schema_version",
        json!(2),
    );
    assert!(charged(&losses, "fcstd/presentation.schema-normalized", id));
    assert_eq!(
        record(&output, &document, "presentation_documents", 0)["schema_version"],
        json!(1)
    );
    let (losses, output) = write_edit(
        &document,
        "/model/presentation_documents/0/states/1/order",
        json!(2),
    );
    assert!(charged(
        &losses,
        "fcstd/presentation.state-order-normalized",
        id
    ));
    assert_eq!(
        record(&output, &document, "presentation_documents", 0)["states"][1]["order"],
        json!(1)
    );
}

#[test]
fn a_view_mode_that_is_no_enumeration_index_is_omitted() {
    let document = decode("gui_appearance.FCStd");
    let id = document["model"]["view_presentations"][0]["id"]
        .as_str()
        .unwrap();
    let (losses, output) = write_edit(
        &document,
        "/model/view_presentations/0/display_mode",
        json!("0x"),
    );
    assert!(charged(
        &losses,
        "fcstd/presentation.view-mode-not-transferred",
        id
    ));
    assert_eq!(
        record(&output, &document, "view_presentations", 0).get("display_mode"),
        None
    );
}

#[test]
fn a_binding_channel_is_derived_from_its_color_lane() {
    let document = decode("gui_appearance.FCStd");
    let original = document["model"]["appearance_bindings"][0]["channels"].clone();
    let id = document["model"]["appearance_bindings"][0]["id"]
        .as_str()
        .unwrap();
    let (losses, output) = write_edit(
        &document,
        "/model/appearance_bindings/0/channels/precedence",
        json!("over"),
    );
    assert!(charged(&losses, "fcstd/appearance.channel-derived", id));
    assert_eq!(
        record(&output, &document, "appearance_bindings", 0)["channels"],
        original
    );
}

#[test]
fn an_appearance_name_has_no_gui_home() {
    let document = decode("gui_appearance.FCStd");
    let id = document["model"]["appearances"][0]["id"].as_str().unwrap();
    let (losses, output) = write_edit(&document, "/model/appearances/0/name", json!("Named"));
    assert!(charged(
        &losses,
        "fcstd/appearance.name-not-transferred",
        id
    ));
    assert_eq!(
        record(&output, &document, "appearances", 0).get("name"),
        None
    );
}

#[test]
fn a_dimension_parameter_that_does_not_parse_stores_its_restored_value() {
    let document = decode("techdraw-dimensions.FCStd");
    let id = document["model"]["semantic_annotations"][0]["id"]
        .as_str()
        .unwrap();
    let (losses, output) = write_edit(
        &document,
        "/model/semantic_annotations/0/parameters/AngleOverride",
        json!("maybe"),
    );
    assert!(charged(
        &losses,
        "fcstd/drawing.parameter-not-transferred",
        id
    ));
    assert_eq!(
        record(&output, &document, "semantic_annotations", 0)["parameters"]["AngleOverride"],
        json!("false")
    );
}

#[test]
fn a_techdraw_annotation_position_lies_on_the_page() {
    let document = decode("techdraw-dimensions.FCStd");
    let original = document["model"]["semantic_annotations"][0]["position"].clone();
    let id = document["model"]["semantic_annotations"][0]["id"]
        .as_str()
        .unwrap();
    let (losses, output) = write_edit(
        &document,
        "/model/semantic_annotations/0/position/2",
        json!(1.0),
    );
    assert!(charged(&losses, "fcstd/annotation.position-planar", id));
    assert_eq!(
        record(&output, &document, "semantic_annotations", 0)["position"],
        original
    );
}

#[test]
fn a_dimension_value_its_saved_geometry_contradicts_is_omitted() {
    let document = decode("techdraw-dimensions.FCStd");
    let index = document["model"]["semantic_annotations"]
        .as_array()
        .unwrap()
        .iter()
        .position(|annotation| annotation.get("value").is_some())
        .expect("measured dimension");
    let id = document["model"]["semantic_annotations"][index]["id"]
        .as_str()
        .unwrap();
    let measured = document["model"]["semantic_annotations"][index]["value"]
        .as_f64()
        .unwrap();
    let (losses, output) = write_edit(
        &document,
        &format!("/model/semantic_annotations/{index}/value"),
        json!(measured + 1.0),
    );
    assert!(charged(&losses, "fcstd/annotation.value-derived", id));
    assert_eq!(
        record(&output, &document, "semantic_annotations", index).get("value"),
        None
    );
    let (losses, _) = write_edit(
        &document,
        &format!("/model/semantic_annotations/{index}/value"),
        json!(measured),
    );
    assert!(!charged(&losses, "fcstd/annotation.value-derived", id));
}

#[test]
fn a_two_use_edge_links_its_uses_to_each_other() {
    let document = decode("core_design_product.FCStd");
    let coedges = document["model"]["coedges"].as_array().unwrap();
    let index = coedges
        .iter()
        .position(|coedge| coedge["radial_next"] != coedge["id"])
        .expect("a two-use edge");
    let id = coedges[index]["id"].as_str().unwrap();
    let (losses, output) = write_edit(
        &document,
        &format!("/model/coedges/{index}/radial_next"),
        json!(id),
    );
    assert!(charged(
        &losses,
        "fcstd/topology.radial-order-not-stored",
        id
    ));
    let stored = output["model"]["coedges"].as_array().unwrap();
    assert_eq!(stored.len(), coedges.len());
    for coedge in stored {
        let uses: Vec<&Value> = stored
            .iter()
            .filter(|other| other["edge"] == coedge["edge"])
            .collect();
        let expected = match uses.as_slice() {
            [first, second] if first["id"] == coedge["id"] => &second["id"],
            [first, _] => &first["id"],
            _ => &coedge["id"],
        };
        assert_eq!(&coedge["radial_next"], expected);
    }
}
