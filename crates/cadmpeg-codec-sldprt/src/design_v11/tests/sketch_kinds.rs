// SPDX-License-Identifier: Apache-2.0
//! The sketches a timeline feature owns besides a placed planar sketch: a
//! block definition, a spatial sketch, a sketch read from the records of one
//! configuration's lane, a reference dimension whose parameter the tree
//! does not list, a reference-dimension parameter no dimension binds, a
//! relation on a curve locus the sketch does not store, a curve point two
//! point entities hold alike, the center record a circle names, the operands
//! of a native relation, and a coincidence of one point entity with itself
//! (§5.2, §6.1).

use cadmpeg_core::serde_erase::json;

use cadmpeg_ir::design::container::{Container, UnorderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::names::NativeNameValue;
use cadmpeg_ir::design::ops::sketch::{Operand, Planar, SketchPlane, SketchRelation};
use cadmpeg_ir::design::parameter::{ParameterDefinition, ParameterValue};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::selection::{Anchor, PickTarget, Picks};
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

/// A reference-dimension parameter that no sketch dimension binds is a
/// `Driven` parameter of its owner with its stored value and display; one
/// whose owner is no design feature is named in a note.
#[test]
fn a_reference_dimension_parameter_no_sketch_dimension_binds_is_driven() {
    const BOUND: &str = "sldprt:model:parameter#reference:1:2";
    const UNBOUND: &str = "sldprt:model:parameter#reference:1:3";
    const ORPHAN: &str = "sldprt:model:parameter#reference:1:4";
    let (mut ir, native) = sketch_and_extrude();
    let mut parameters = json::to_value(&ir.model.parameters).unwrap();
    let template = parameters[0].clone();
    let owner = template["owner"].as_str().unwrap().to_owned();
    let absent = format!("{}#absent", owner.split_once('#').unwrap().0);
    for (id, name, owner) in [
        (BOUND, "D1@reference", owner.as_str()),
        (UNBOUND, "D2@reference", owner.as_str()),
        (ORPHAN, "D3@reference", absent.as_str()),
    ] {
        let mut reference = template.clone();
        reference["id"] = cadmpeg_core::json!(id);
        reference["owner"] = cadmpeg_core::json!(owner);
        reference["name"] = cadmpeg_core::json!(name);
        reference["value"] = cadmpeg_core::json!({"kind": "length", "value": 6.5});
        reference["expression"] = cadmpeg_core::json!("<MOD-DIAM>6.5mm");
        reference["display"] = cadmpeg_core::json!("diameter");
        reference["properties"] =
            cadmpeg_core::json!({"sldprt_relation_parameter_role": "reference"});
        parameters.as_array_mut().unwrap().push(reference);
    }
    ir.model.parameters = json::from_value(parameters).unwrap();
    let mut constraints = json::to_value(&ir.model.sketch_constraints).unwrap();
    for constraint in constraints.as_array_mut().unwrap() {
        if constraint["id"] == "sldprt:model:sketch-constraint#test:4" {
            constraint["definition"]["parameter"] = cadmpeg_core::json!(BOUND);
            constraint["driving"] = cadmpeg_core::json!(false);
        }
    }
    ir.model.sketch_constraints = json::from_value(constraints).unwrap();
    let losses = project(
        &mut ir,
        &native.feature_histories,
        &native.feature_input_lanes,
    );

    let stated = |id: &str| {
        ir.design
            .parameters
            .iter()
            .find(|parameter| parameter.id.as_str() == id)
    };
    assert_eq!(
        stated(BOUND).map(|parameter| &parameter.definition),
        Some(&ParameterDefinition::Driven)
    );
    let unbound = stated(UNBOUND).expect("the unbound reference parameter is stated");
    assert_eq!(unbound.definition, ParameterDefinition::Driven);
    assert_eq!(
        unbound.owner,
        Some(cadmpeg_ir::design::parameter::ParameterOwner::Feature(
            json::from_value::<FeatureId>(cadmpeg_core::json!(owner)).unwrap()
        ))
    );
    assert_eq!(
        unbound
            .name
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("D2@reference")
    );
    assert_eq!(
        unbound.kind,
        cadmpeg_ir::design::units::ValueKind::Quantity(cadmpeg_ir::design::units::Quantity::LENGTH)
    );
    let Some(ParameterValue::Number(value)) = &unbound.value else {
        panic!("a length: {unbound:#?}");
    };
    assert!((value.get() - 6.5).abs() < 1e-9, "{unbound:#?}");
    assert_eq!(
        unbound.display,
        Some(cadmpeg_ir::design::parameter::DimensionDisplay::Diameter)
    );
    assert!(stated(ORPHAN).is_none());
    let charged = losses
        .iter()
        .filter(|note| note.code == SldprtLossCode::DesignParameterReferenceUnbound.kind())
        .collect::<Vec<_>>();
    let [note] = charged.as_slice() else {
        panic!("one unbound-reference note: {losses:#?}");
    };
    assert!(
        note.message
            .contains(&format!("{ORPHAN} = <MOD-DIAM>6.5mm")),
        "{note:#?}"
    );
    assert!(!note.message.contains(UNBOUND), "{note:#?}");
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

#[test]
fn two_points_at_the_center_of_a_circle_leave_the_center_to_a_minted_point_with_a_loss() {
    let (mut ir, native) = sketch_and_extrude();
    let mut entities = json::to_value(&ir.model.sketch_entities).unwrap();
    for key in ["center0", "center1"] {
        entities.as_array_mut().unwrap().push(
            cadmpeg_core::json!({"id": format!("sldprt:model:sketch-entity#test:{key}"),
                "sketch": SKETCH, "construction": false,
                "native_ref": format!("native:{key}"), "endpoint_refs": [],
                "geometry": {"kind": "point", "position": {"u": 25.0, "v": 15.0}}}),
        );
    }
    ir.model.sketch_entities = json::from_value(entities).unwrap();
    let losses = project(
        &mut ir,
        &native.feature_histories,
        &native.feature_input_lanes,
    );

    let roleless = |key: &str| {
        ir.design
            .sketch_entities
            .iter()
            .find(|entity| entity.id.as_str().ends_with(key))
            .unwrap()
            .roles
            .is_empty()
    };
    assert!(roleless(":center0") && roleless(":center1"));
    assert!(
        losses.iter().any(
            |note| note.code == SldprtLossCode::DesignSketchLocusAmbiguous.kind()
                && note.message.contains("test:hole")
                && note.message.contains("test:center0")
                && note.message.contains("test:center1")
        ),
        "{losses:#?}"
    );
}

#[test]
fn a_circle_that_names_its_center_record_gives_the_center_to_that_point() {
    let (mut ir, native) = sketch_and_extrude();
    let mut entities = json::to_value(&ir.model.sketch_entities).unwrap();
    let list = entities.as_array_mut().unwrap();
    for key in ["center0", "center1"] {
        list.push(
            cadmpeg_core::json!({"id": format!("sldprt:model:sketch-entity#test:{key}"),
                "sketch": SKETCH, "construction": false,
                "native_ref": format!("native:{key}"), "endpoint_refs": [],
                "geometry": {"kind": "point", "position": {"u": 25.0, "v": 15.0}}}),
        );
    }
    let hole = list
        .iter_mut()
        .find(|entity| entity["id"] == "sldprt:model:sketch-entity#test:hole")
        .unwrap();
    hole["center_ref"] = cadmpeg_core::json!("native:center1");
    ir.model.sketch_entities = json::from_value(entities).unwrap();
    let losses = project(
        &mut ir,
        &native.feature_histories,
        &native.feature_input_lanes,
    );

    let roles = |key: &str| {
        ir.design
            .sketch_entities
            .iter()
            .find(|entity| entity.id.as_str().ends_with(key))
            .unwrap()
            .roles
            .clone()
    };
    assert!(roles(":center0").is_empty());
    assert_eq!(
        roles(":center1"),
        vec![cadmpeg_ir::design::ops::sketch::EntityRole {
            of: cadmpeg_ir::sketches::SketchEntityId::mint("sldprt:model:sketch-entity#test:hole")
                .unwrap(),
            role: cadmpeg_ir::design::ops::sketch::DefiningRole::Center,
        }]
    );
    assert!(
        losses
            .iter()
            .all(|note| note.code != SldprtLossCode::DesignSketchLocusAmbiguous.kind()),
        "{losses:#?}"
    );
}

#[test]
fn a_native_relation_states_each_operand_of_its_record() {
    const NATIVE: &str = "sldprt:model:sketch-constraint#test:native";
    let (mut ir, native) = sketch_and_extrude();
    let mut constraints = json::to_value(&ir.model.sketch_constraints).unwrap();
    constraints
        .as_array_mut()
        .unwrap()
        .push(cadmpeg_core::json!({"id": NATIVE, "sketch": SKETCH,
            "definition": {"kind": "native", "native_kind": "sldprt:sketch-relation:9",
                "native_properties": {}, "entities": ["sldprt:model:sketch-entity#test:a"],
                "operands": [
                    {"native_kind": "sldprt:relation-operand:point", "object_index": 0,
                        "native_ref": "native:a"},
                    {"native_kind": "sldprt:relation-operand:point:unresolved",
                        "object_index": 7, "native_ref": "native:gone"},
                    {"native_kind": "sldprt:relation-operand:unknown:unresolved",
                        "object_index": 2}]}}));
    ir.model.sketch_constraints = json::from_value(constraints).unwrap();
    let losses = project(
        &mut ir,
        &native.feature_histories,
        &native.feature_input_lanes,
    );

    let relation = &ir
        .design
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.id.as_str() == NATIVE)
        .unwrap_or_else(|| panic!("{losses:#?}"))
        .relation;
    let SketchRelation::Native { operands, .. } = relation else {
        panic!("a native relation: {relation:#?}");
    };
    let picked = |operand: &Operand<Planar>| {
        let Operand::Model(selection) = operand else {
            return None;
        };
        assert_eq!(selection.at, Anchor::Input);
        let Picks::Named(picks) = &selection.picks else {
            return None;
        };
        let [pick] = picks.as_slice() else {
            return None;
        };
        assert_eq!(pick.target, PickTarget::Native { within: None });
        pick.native.as_ref().map(|name| name.value.clone())
    };
    let [a, gone, unknown] = operands.as_slice() else {
        panic!("three operands: {operands:#?}");
    };
    assert_eq!(
        a,
        &Operand::Entity(
            cadmpeg_ir::sketches::SketchEntityId::mint("sldprt:model:sketch-entity#test:a")
                .unwrap()
        )
    );
    assert_eq!(
        picked(gone),
        Some(NativeNameValue::Text(
            cadmpeg_core::text::NonBlankString::new("native:gone").unwrap()
        ))
    );
    assert_eq!(picked(unknown), Some(NativeNameValue::Integers(vec![2])));
}

#[test]
fn a_coincidence_of_two_ends_one_point_holds_is_charged_and_not_restated() {
    const SAME: &str = "sldprt:model:sketch-constraint#test:0";
    let (mut ir, native) = sketch_and_extrude();
    let losses = project(
        &mut ir,
        &native.feature_histories,
        &native.feature_input_lanes,
    );

    assert!(ir
        .design
        .sketch_constraints
        .iter()
        .all(|constraint| constraint.id.as_str() != SAME));
    assert!(
        losses.iter().any(|note| note.code
            == SldprtLossCode::DesignSketchRelationSinglePoint.kind()
            && note.message.contains(SAME)),
        "{losses:#?}"
    );
}
