// SPDX-License-Identifier: Apache-2.0
//! The sketches a timeline feature owns besides a placed planar sketch: a
//! block definition, a spatial sketch, a sketch read from the records of one
//! configuration's lane, a reference dimension whose parameter the tree
//! does not list, and a relation on a curve locus the sketch does not store
//! (§5.2, §6.1).

use cadmpeg_core::serde_erase::json;

use cadmpeg_ir::design::container::{Container, UnorderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sketch::{SketchPlane, SketchRelation};
use cadmpeg_ir::design::parameter::{ParameterDefinition, ParameterValue};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::scalar::FiniteReal;

use super::outside_history::project;
use super::{sketch_and_extrude, sketch_and_extrude_from};
use crate::loss::SldprtLossCode;

const SKETCH: &str = "sldprt:model:sketch#test";

fn set_operation(ir: &mut CadIr, name: &str, operation: &serde_json::Value) -> FeatureId {
    let feature = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap();
    let definition =
        json::from_value(cadmpeg_core::json!({"kind": "operation", "value": operation.clone()}))
            .unwrap();
    feature.evaluation.edit(|current, _| *current = definition);
    feature.id.clone()
}

fn has_group(ir: &CadIr) -> bool {
    ir.design.containers.iter().any(|container| {
        matches!(
            container,
            Container::Unordered {
                kind: UnorderedKind::Group,
                ..
            }
        )
    })
}

fn in_timeline(ir: &CadIr, feature: &FeatureId) -> bool {
    ir.design.containers.iter().any(|container| {
        matches!(container, Container::Ordered { members, .. }
            if members.contains(&Member::Feature(feature.clone())))
    })
}

#[test]
fn a_block_definition_is_a_sketch_without_a_plane_that_its_timeline_feature_owns() {
    let (mut ir, native) = sketch_and_extrude();
    let owner = set_operation(
        &mut ir,
        "Sketch1",
        &cadmpeg_core::json!({"kind": "sketch_block_definition", "value": {"sketch": SKETCH}}),
    );
    project(
        &mut ir,
        &native.feature_histories,
        &native.feature_input_lanes,
    );

    let sketch = ir
        .design
        .sketches
        .iter()
        .find(|sketch| sketch.id.as_str() == SKETCH)
        .unwrap();
    assert_eq!(sketch.plane, SketchPlane::Block);
    assert_eq!(sketch.owner, owner);
    let feature = ir
        .design
        .features
        .iter()
        .find(|feature| feature.id == owner)
        .unwrap();
    assert_eq!(
        feature.definition.operation,
        FeatureOperation::Sketch(sketch.id.clone())
    );
    assert!(in_timeline(&ir, &owner), "{:#?}", ir.design.containers);
    assert!(!has_group(&ir), "{:#?}", ir.design.containers);
}

#[test]
fn a_sketch_of_one_configuration_lane_is_the_sketch_of_its_timeline_feature() {
    const LANE: &str = "sldprt:model:sketch#markers:2:8";
    let (mut ir, native) = sketch_and_extrude();
    let quoted = format!("\"{SKETCH}\"");
    let renamed = format!("\"{LANE}\"");
    let rename = |value: serde_json::Value| {
        serde_json::from_str::<serde_json::Value>(
            &serde_json::to_string(&value)
                .unwrap()
                .replace(&quoted, &renamed),
        )
        .unwrap()
    };
    let mut sketches = rename(json::to_value(&ir.model.sketches).unwrap());
    sketches[0]["configuration"] = cadmpeg_core::json!("Other");
    ir.model.sketches = json::from_value(sketches).unwrap();
    ir.model.sketch_entities =
        json::from_value(rename(json::to_value(&ir.model.sketch_entities).unwrap())).unwrap();
    ir.model.sketch_constraints = json::from_value(rename(
        json::to_value(&ir.model.sketch_constraints).unwrap(),
    ))
    .unwrap();
    let owner = set_operation(
        &mut ir,
        "Sketch1",
        &cadmpeg_core::json!({"kind": "sketch",
            "value": {"sketch": {"kind": "planar", "sketch": LANE}}}),
    );
    project(
        &mut ir,
        &native.feature_histories,
        &native.feature_input_lanes,
    );

    let [sketch] = ir.design.sketches.as_slice() else {
        panic!("one sketch: {:#?}", ir.design.sketches);
    };
    assert_eq!(sketch.id.as_str(), LANE);
    assert_eq!(sketch.owner, owner);
    assert!(in_timeline(&ir, &owner));
    assert!(!has_group(&ir), "{:#?}", ir.design.containers);
}

#[test]
fn a_spatial_sketch_is_the_operation_of_its_timeline_feature() {
    const SPATIAL: &str = "sldprt:model:spatial-sketch#path";
    let (mut ir, native) = sketch_and_extrude_from(
        br#"<Keywords Name="Block">
            <Feature Name="Front" id="2"/>
            <Feature Name="Top" id="3"/>
            <Feature Name="Right" id="4"/>
            <Sketch Name="Origin" Type="OriginProfileFeature" id="106"/>
            <Sketch Name="Sketch1" Type="3DSketch" id="8">
                <Dimension Name="D1">50</Dimension>
                <Dimension Name="D2">30</Dimension>
                <Dimension Name="D3">&lt;MOD-DIAM&gt;10</Dimension>
            </Sketch>
            <Extrusion Name="Boss" Type="BossExtrude" id="7">
                <Dimension Name="D1">10</Dimension>
            </Extrusion>
        </Keywords>"#,
    );
    let length =
        json::to_value(&ir.model.sketch_constraints).unwrap()[1]["definition"]["parameter"].clone();
    ir.model.sketches.clear();
    ir.model.sketch_entities.clear();
    ir.model.sketch_constraints.clear();
    ir.model.spatial_sketches = json::from_value(cadmpeg_core::json!([
        {"id": SPATIAL, "name": "Sketch1", "profiles": []}
    ]))
    .unwrap();
    ir.model.spatial_sketch_entities = json::from_value(cadmpeg_core::json!([
        {"id": "sldprt:model:spatial-sketch-entity#path:l", "sketch": SPATIAL,
            "construction": false, "endpoint_refs": [],
            "geometry": {"kind": "line", "start": {"x": 0.0, "y": 0.0, "z": 0.0},
                "end": {"x": 0.0, "y": 30.0, "z": 40.0}}},
        {"id": "sldprt:model:spatial-sketch-entity#path:h", "sketch": SPATIAL,
            "construction": false, "endpoint_refs": [],
            "geometry": {"kind": "native", "native_kind": "sldprt:helix"}}
    ]))
    .unwrap();
    ir.model.spatial_sketch_constraints = json::from_value(cadmpeg_core::json!([
        {"id": "sldprt:model:sketch-constraint#path:0", "sketch": SPATIAL,
            "definition": {"kind": "line_length",
                "entity": "sldprt:model:spatial-sketch-entity#path:l", "parameter": length}},
        {"id": "sldprt:model:sketch-constraint#path:1", "sketch": SPATIAL,
            "definition": {"kind": "spline_group", "entities": [
                "sldprt:model:spatial-sketch-entity#path:l",
                "sldprt:model:spatial-sketch-entity#path:h"]}}
    ]))
    .unwrap();
    let owner = set_operation(
        &mut ir,
        "Sketch1",
        &cadmpeg_core::json!({"kind": "spatial_sketch", "value": {"sketch": SPATIAL}}),
    );
    let losses = project(
        &mut ir,
        &native.feature_histories,
        &native.feature_input_lanes,
    );

    let [sketch] = ir.design.spatial_sketches.as_slice() else {
        panic!("one spatial sketch: {:#?}", ir.design.spatial_sketches);
    };
    assert_eq!(sketch.id.as_str(), SPATIAL);
    assert_eq!(sketch.owner, owner);
    let feature = ir
        .design
        .features
        .iter()
        .find(|feature| feature.id == owner)
        .unwrap();
    assert_eq!(
        feature.definition.operation,
        FeatureOperation::SpatialSketch(sketch.id.clone())
    );
    assert!(in_timeline(&ir, &owner));
    assert!(!has_group(&ir), "{:#?}", ir.design.containers);
    assert_eq!(ir.design.spatial_sketch_entities.len(), 2);
    let [constraint, group] = ir.design.spatial_sketch_constraints.as_slice() else {
        panic!("two constraints");
    };
    assert!(
        matches!(&group.relation, SketchRelation::Native { .. }),
        "{:?}",
        group.relation
    );
    assert!(
        losses.iter().any(
            |note| note.code == SldprtLossCode::DesignSketchRelationNative.kind()
                && note.message.contains(group.id.as_str())
        ),
        "{losses:#?}"
    );
    let SketchRelation::Length {
        value: Dimension::Parameter(parameter),
        ..
    } = &constraint.relation
    else {
        panic!("a length bound to D1: {:?}", constraint.relation);
    };
    assert_eq!(parameter.as_str(), length.as_str().unwrap());
    assert!(
        losses.iter().any(
            |note| note.code == SldprtLossCode::DesignSketchEntityNative.kind()
                && note
                    .message
                    .contains("sldprt:model:spatial-sketch-entity#path:h")
        ),
        "{losses:#?}"
    );
}

#[test]
fn a_reference_dimension_of_a_parameter_the_tree_omits_binds_a_driven_parameter() {
    const REFERENCE: &str = "sldprt:model:parameter#reference";
    let (mut ir, native) = sketch_and_extrude();
    let mut parameters = json::to_value(&ir.model.parameters).unwrap();
    let mut reference = parameters[0].clone();
    reference["id"] = cadmpeg_core::json!(REFERENCE);
    reference["name"] = cadmpeg_core::json!("RD1");
    reference.as_object_mut().unwrap().remove("owner");
    reference["value"] = cadmpeg_core::json!({"kind": "length", "value": 10.0});
    parameters.as_array_mut().unwrap().push(reference);
    ir.model.parameters = json::from_value(parameters).unwrap();
    let mut constraints = json::to_value(&ir.model.sketch_constraints).unwrap();
    for constraint in constraints.as_array_mut().unwrap() {
        if constraint["id"] == "sldprt:model:sketch-constraint#test:4" {
            constraint["definition"]["parameter"] = cadmpeg_core::json!(REFERENCE);
            constraint["driving"] = cadmpeg_core::json!(false);
        }
    }
    ir.model.sketch_constraints = json::from_value(constraints).unwrap();
    project(
        &mut ir,
        &native.feature_histories,
        &native.feature_input_lanes,
    );

    let parameter = ir
        .design
        .parameters
        .iter()
        .find(|parameter| parameter.id.as_str() == REFERENCE)
        .unwrap();
    assert_eq!(parameter.definition, ParameterDefinition::Driven);
    assert_eq!(
        parameter
            .name
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("RD1")
    );
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Number(FiniteReal::new(10.0).unwrap()))
    );
    let diameter = ir
        .design
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.id.as_str() == "sldprt:model:sketch-constraint#test:4")
        .unwrap();
    assert!(
        matches!(&diameter.relation, SketchRelation::Radial { value: Dimension::Parameter(bound), .. }
            if bound.as_str() == REFERENCE),
        "{:?}",
        diameter.relation
    );
}

#[test]
fn a_relation_on_the_start_of_a_circle_states_the_circle_and_is_charged() {
    const ON: &str = "sldprt:model:sketch-constraint#test:on";
    let (mut ir, native) = sketch_and_extrude();
    let mut constraints = json::to_value(&ir.model.sketch_constraints).unwrap();
    constraints
        .as_array_mut()
        .unwrap()
        .push(cadmpeg_core::json!({"id": ON, "sketch": SKETCH,
            "definition": {"kind": "point_on_object",
                "point": {"kind": "start", "value": "sldprt:model:sketch-entity#test:hole"},
                "entity": "sldprt:model:sketch-entity#test:l0"}}));
    ir.model.sketch_constraints = json::from_value(constraints).unwrap();
    let losses = project(
        &mut ir,
        &native.feature_histories,
        &native.feature_input_lanes,
    );

    assert!(
        ir.design
            .sketch_constraints
            .iter()
            .any(|constraint| constraint.id.as_str() == ON),
        "{:#?}",
        ir.design.sketch_constraints
    );
    assert!(
        losses.iter().any(
            |note| note.code == SldprtLossCode::DesignSketchLocusUnplaced.kind()
                && note.message.contains(ON)
        ),
        "{losses:#?}"
    );
}
