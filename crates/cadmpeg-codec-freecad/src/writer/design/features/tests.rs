// SPDX-License-Identifier: Apache-2.0
use super::super::super::xml::{self, Object};
use super::write;
use crate::test_support::test_archive::{archive, archive_entries};
use crate::FcstdCodec;
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation,
};
use cadmpeg_ir::{Codec, DecodeOptions};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Cursor;

fn feature(value: Value) -> Feature {
    let operation: FeatureOperation =
        serde_json::from_value(value).expect("admitted neutral operation");
    Feature {
        product_definition: None,
        id: FeatureId::mint("test:feature:operation#0").expect("identity"),
        ordinal: 0,
        name: Some("Operation".into()),
        suppressed: Some(false),
        needs_recompute: false,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(operation),
            DistinctMembers::default(),
        ),
        native_ref: None,
    }
}
fn sketch() -> Object {
    let mut sketch = Object::new("Sketch".into(), "Sketcher::SketchObject", "Sketch");
    sketch.set(
        "Geometry",
        xml::property(
            "Geometry",
            "Part::PropertyGeometryList",
            "<GeometryList count=\"0\"/>",
        ),
    );
    sketch.set(
        "Placement",
        xml::property(
            "Placement",
            "App::PropertyPlacement",
            "<PropertyPlacement Px=\"0\" Py=\"0\" Pz=\"0\" Q0=\"0\" Q1=\"0\" Q2=\"0\" Q3=\"1\"/>",
        ),
    );
    sketch
}
fn decode_operation(mut objects: Vec<Object>, output: super::FeatureOutput) -> Value {
    objects.extend(output.objects);
    let document = xml::document(&objects, &std::collections::BTreeMap::default(), "4");
    let mut entries = vec![("Document.xml".to_owned(), document.into_bytes())];
    entries.extend(output.entries);
    let entries = entries
        .iter()
        .map(|(name, data)| (name.as_str(), data.as_slice()))
        .collect::<Vec<_>>();
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&entries)),
            &DecodeOptions::default(),
        )
        .expect("read synthesized construction");
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Operation"))
        .expect("generated operation");
    let value = decoded
        .ir()
        .resolved_feature_definition(feature.evaluation.definition())
        .expect("evaluated operation dimensions");
    if value["definition"] == "post_process" {
        value["operation"].clone()
    } else {
        value
    }
}

#[test]
fn a_neutral_pad_writes_profile_length_and_direction_through_the_reader() {
    let feature = feature(
        json!({"definition":"extrude","profile":{"kind":"sketch","value":"test:sketch:profile#0"},"direction":{"kind":"explicit","vector":{"x":0.0,"y":0.0,"z":1.0}},"start":{"kind":"profile_plane"},"extent":{"kind":"one_sided","side":{"termination":{"kind":"blind","length":{"kind":"literal","value":23.0}}}},"op":"join","solid":true}),
    );
    let names = [("test:sketch:profile#0".into(), "Sketch".into())]
        .into_iter()
        .collect();
    let mut object = Object::new("Operation".into(), "PartDesign::Feature", "Operation");
    let output =
        write(&cadmpeg_ir::CadIr::empty(), &feature, &mut object, &names).expect("pad mapping");
    let value = decode_operation(vec![sketch(), object], output);
    assert_eq!(value["definition"], "extrude");
    assert_eq!(value["extent"]["side"]["termination"]["length"], 23.0);
    assert_eq!(value["direction"]["vector"]["z"], 1.0);
}

#[test]
fn a_simple_hole_writes_bore_depth_bottom_and_profile_filter() {
    let feature = feature(
        json!({"definition":"hole","profile":{"kind":"sketch","value":"test:sketch:profile#0"},"profile_filter":"all","shape":{"construction":{"construction":"form","kind":{"kind":"simple"}},"diameter":{"kind":"literal","value":6.0}},"extent":{"kind":"blind","length":{"kind":"literal","value":19.0}},"bottom":{"kind":"flat"}}),
    );
    let names = [("test:sketch:profile#0".into(), "Sketch".into())]
        .into_iter()
        .collect();
    let mut object = Object::new("Operation".into(), "PartDesign::Feature", "Operation");
    let output =
        write(&cadmpeg_ir::CadIr::empty(), &feature, &mut object, &names).expect("hole mapping");
    let value = decode_operation(vec![sketch(), object], output);
    assert_eq!(value["definition"], "hole");
    assert_eq!(value["shape"]["diameter"], 6.0);
    assert_eq!(value["extent"]["length"], 19.0);
    assert_eq!(value["bottom"]["kind"], "flat");
    assert_eq!(value["profile_filter"], "all");
}

#[test]
fn a_polygon_generates_a_typed_node_side_entry() {
    let feature = feature(
        json!({"definition":"polyline","chain":{"points":[{"x":0.0,"y":0.0,"z":0.0},{"x":2.0,"y":3.0,"z":4.0}],"closed":false}}),
    );
    let mut object = Object::new("Operation".into(), "PartDesign::Feature", "Operation");
    let output = write(
        &cadmpeg_ir::CadIr::empty(),
        &feature,
        &mut object,
        &HashMap::new(),
    )
    .expect("polygon mapping");
    assert_eq!(output.entries.len(), 1);
    let value = decode_operation(vec![object], output);
    assert_eq!(value["definition"], "polyline");
    assert_eq!(value["chain"]["points"][1]["z"], 4.0);
}

#[test]
fn a_body_tree_writes_ordered_group_and_tip_membership() {
    let feature = feature(
        json!({"definition":"tree_node","role":"solid_bodies","children":{"children":["test:feature:child#0"],"active_child":"test:feature:child#0"}}),
    );
    let names = [("test:feature:child#0".into(), "Child".into())]
        .into_iter()
        .collect();
    let mut object = Object::new("Operation".into(), "App::DocumentObjectGroup", "Operation");
    let output =
        write(&cadmpeg_ir::CadIr::empty(), &feature, &mut object, &names).expect("body mapping");
    let mut child = Object::new("Child".into(), "Part::Box", "Child");
    for dimension in ["Length", "Width", "Height"] {
        child.set(
            dimension,
            xml::scalar(dimension, "App::PropertyLength", "Float", 1.0),
        );
    }
    let value = decode_operation(vec![child, object], output);
    assert_eq!(value["definition"], "tree_node");
    assert_eq!(value["role"], "solid_bodies");
    assert_eq!(
        value["children"]["children"]
            .as_array()
            .expect("children")
            .len(),
        1
    );
    assert!(value["children"]["active_child"].is_string());
}

#[test]
fn an_unresolved_profile_declines_the_feature_without_writing_hidden_source_content() {
    let source = r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="Operation" type="PartDesign::Pad"/></Objects><ObjectData Count="1"><Object name="Operation"><Properties Count="1"><Property name="Length" type="App::PropertyLength"><Float value="4"/></Property></Properties></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(archive(source)), &DecodeOptions::default())
        .expect("read native-only pad");
    let feature = &decoded.ir().model.features[0];
    let mut object = Object::new("Operation".into(), "PartDesign::Feature", "Operation");
    // A Native operation is emitted by the orchestrator's typed-property path;
    // this helper supplies no replacement values from retained XML.
    let output =
        write(decoded.ir(), feature, &mut object, &HashMap::new()).expect("native dispatch");
    assert!(output.objects.is_empty());
    assert!(output.entries.is_empty());
    assert!(!object.properties.contains_key("Length"));
}

#[test]
fn compatible_standalone_boolean_source_tags_keep_their_native_operation() {
    for (kind, operation) in [
        ("Part::Cut", "cut"),
        ("Part::Fuse", "join"),
        ("Part::Common", "intersect"),
    ] {
        let mut feature = feature(
            json!({"definition":"combine","operands":{"target":{"kind":"native","value":"test:native:property#base"},"tools":{"kind":"native","value":"test:native:property#tool"}},"op":operation,"keep_tools":false}),
        );
        feature.source_tag = Some(kind.into());
        let names = HashMap::from([
            ("test:native:property#base".into(), "Base".into()),
            ("test:native:property#tool".into(), "Tool".into()),
        ]);
        let mut object = Object::new("Operation".into(), "PartDesign::Feature", "Operation");
        let output = write(&cadmpeg_ir::CadIr::empty(), &feature, &mut object, &names)
            .expect("standalone boolean inverse");
        assert_eq!(object.kind, kind);
        assert!(object.properties.contains_key("Base"));
        assert!(object.properties.contains_key("Tool"));
        let value = decode_operation(
            vec![
                Object::new("Base".into(), "Part::Feature", "Base"),
                Object::new("Tool".into(), "Part::Feature", "Tool"),
                object,
            ],
            output,
        );
        assert_eq!(value["definition"], "combine");
        assert_eq!(value["op"], operation);
    }
}
