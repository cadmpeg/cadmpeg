// SPDX-License-Identifier: Apache-2.0
//! Tests of the planar sketches of a Fusion design.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::design::construction::InputReference;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::sketch::{
    DistanceAxis, EntityDefinition, EntityForm, Operand, RadialMeasure, SketchPlane, SketchRelation,
};
use cadmpeg_ir::design::selection::Anchor;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::parameters::ParameterValue as V10;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureContent, FeatureDefinition as LegacyDefinition, FeatureId,
    FeatureOperation as LegacyOperation, ParameterId, SketchFeatureBinding,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::SketchId;

use crate::loss::F3dLossCode;

const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";
const OWNER: &str = "f3d:model:feature#sketch:20";
const SKETCH: &str = "f3d:model:sketch#20";

fn entity(key: &str) -> String {
    format!("f3d:model:sketch-entity#20:{key}")
}

fn constraint(key: &str) -> String {
    format!("f3d:model:sketch-constraint#20:{key}")
}

fn parameter_id(key: &str) -> String {
    format!("f3d:model:parameter#{key}")
}

/// The v10 feature `id` whose operation is the planar sketch `sketch`.
fn sketch_feature(id: &str, sketch: &str) -> LegacyFeature {
    LegacyFeature {
        product_definition: None,
        id: FeatureId::mint(id).unwrap(),
        ordinal: 0,
        name: Some("Sketch1".to_owned()),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(LegacyDefinition::Operation(
            LegacyOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(SketchId::mint(sketch).unwrap())),
            },
        )),
        native_ref: Some(format!("{STREAM}:design-parameter-scope#20")),
    }
}

/// A length parameter `key` of the sketch feature that the source lists
/// under a linear dimension.
fn length(key: &str, value: f64) -> cadmpeg_ir::features::parameters::DesignParameter {
    cadmpeg_ir::features::parameters::DesignParameter {
        product_definition: None,
        id: ParameterId::mint(parameter_id(key)).unwrap(),
        owner: Some(FeatureId::mint(OWNER).unwrap()),
        ordinal: 0,
        name: key.to_owned(),
        expression: format!("{value} mm"),
        display: None,
        value: Some(V10::Length(Length::new(value).unwrap())),
        dependencies: DistinctMembers::default(),
        properties: [(
            cadmpeg_core::text::NonBlankString::new("source_kind".to_owned()).unwrap(),
            "Linear Dimension-3".to_owned(),
        )]
        .into_iter()
        .collect(),
        pmi: None,
        native_ref: Some(format!("{STREAM}:design-parameter#{key}")),
    }
}

/// A sketch on the XY plane with two points 30 mm apart along `u`, a circle
/// of radius 5 mm, and a text entity, constrained by a horizontal distance
/// bound to `width`, a radius bound to `radius`, a distance bound to
/// `absent`, which no record stores, and a horizontal relation.
fn model() -> Model {
    let mut model = Model::default();
    model.features = vec![sketch_feature(OWNER, SKETCH)];
    model.parameters = vec![length("width", 30.0), length("radius", 5.0)];
    model.sketches = json::from_value(cadmpeg_core::json!([{
        "id": SKETCH, "name": "Sketch1",
        "placement": {"kind": "resolved", "frame": {"origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0}, "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}}},
        "profiles": []
    }]))
    .unwrap();
    let point = |key: &str, u: f64| {
        cadmpeg_core::json!({"id": entity(key), "sketch": SKETCH, "construction": false,
            "endpoint_refs": [], "geometry": {"kind": "point", "position": {"u": u, "v": 0.0}}})
    };
    model.sketch_entities = json::from_value(cadmpeg_core::json!([
        point("p0", 0.0),
        point("p1", 30.0),
        {"id": entity("line"), "sketch": SKETCH, "construction": false, "endpoint_refs": [],
            "geometry": {"kind": "line", "start": {"u": 0.0, "v": 0.0}, "end": {"u": 30.0, "v": 0.0}}},
        {"id": entity("circle"), "sketch": SKETCH, "construction": true, "endpoint_refs": [],
            "geometry": {"kind": "circle", "center": {"u": 15.0, "v": 10.0}, "radius": 5.0}},
        {"id": entity("text"), "sketch": SKETCH, "construction": false, "endpoint_refs": [],
            "geometry": {"kind": "native", "native_kind": "SketchText"}}
    ]))
    .unwrap();
    model.sketch_constraints = json::from_value(cadmpeg_core::json!([
        {"id": constraint("width"), "sketch": SKETCH,
            "definition": {"kind": "horizontal_distance",
                "first": {"kind": "entity", "value": entity("p1")},
                "second": {"kind": "entity", "value": entity("p0")},
                "parameter": parameter_id("width")}},
        {"id": constraint("radius"), "sketch": SKETCH,
            "definition": {"kind": "radius", "entity": entity("circle"),
                "parameter": parameter_id("radius")}},
        {"id": constraint("absent"), "sketch": SKETCH,
            "definition": {"kind": "distance", "entities": [entity("p0"), entity("line")],
                "parameter": parameter_id("absent")}},
        {"id": constraint("horizontal"), "sketch": SKETCH,
            "definition": {"kind": "horizontal", "entity": entity("line")}}
    ]))
    .unwrap();
    model
}

fn unresolved_sketch(id: &str) -> Feature {
    Feature {
        id: FeatureId::mint(id).unwrap(),
        name: Some("Sketch1".to_owned()),
        suppressed: None,
        definition: FeatureDefinition {
            operation: FeatureOperation::Unresolved {
                family: OperationFamily::Sketch,
            },
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

fn names(losses: &[LossNote], code: F3dLossCode, record: &str) -> bool {
    losses
        .iter()
        .any(|loss| loss.code == code.kind() && loss.message.contains(record))
}

#[test]
fn a_sketch_whose_owner_has_no_timeline_item_is_owned_by_its_v10_feature_or_a_minted_one() {
    let mut model = model();
    let mut loose = model.sketches[0].clone();
    loose.id = SketchId::mint("f3d:model:sketch#21").unwrap();
    loose.name = Some("Loose".to_owned());
    model.sketches.push(loose);

    let outside = super::outside(&model, &BTreeSet::new()).unwrap();
    let owners = outside.get("").unwrap();
    let [owned, minted] = owners.as_slice() else {
        panic!("two owners: {owners:#?}");
    };
    assert_eq!(owned.id.as_str(), OWNER);
    assert_eq!(
        owned.definition.operation,
        FeatureOperation::Sketch(SketchId::mint(SKETCH).unwrap())
    );
    assert!(minted.id.as_str().starts_with("f3d:design:sketch-owner#"));
    assert_eq!(minted.name.as_deref(), Some("Loose"));
    assert_eq!(
        minted.definition.operation,
        FeatureOperation::Sketch(SketchId::mint("f3d:model:sketch#21").unwrap())
    );

    let timeline = BTreeSet::from([FeatureId::mint(OWNER).unwrap()]);
    let outside = super::outside(&model, &timeline).unwrap();
    let [minted] = outside.get("").unwrap().as_slice() else {
        panic!("one owner: {outside:#?}");
    };
    assert_eq!(minted.name.as_deref(), Some("Loose"));
}

#[test]
fn the_group_and_a_minted_owner_of_an_archive_member_take_the_member_key() {
    assert_eq!(
        super::group_id("").unwrap().as_str(),
        "f3d:design:container#sketches"
    );
    assert_eq!(
        super::group_id("member.f3d@").unwrap().as_str(),
        "f3d:member.f3d@design:container#sketches"
    );

    let mut model = model();
    model.features.clear();
    model.sketches[0].id = SketchId::mint("f3d:member.f3d@model:sketch#21").unwrap();
    let outside = super::outside(&model, &BTreeSet::new()).unwrap();
    let [minted] = outside.values().flatten().collect::<Vec<_>>()[..] else {
        panic!("one owner: {outside:#?}");
    };
    assert!(minted
        .id
        .as_str()
        .starts_with("f3d:member.f3d@design:sketch-owner#"));
}

#[test]
fn a_sketch_states_its_entities_and_binds_its_dimensions_to_parameters_of_its_scope() {
    let model = model();
    let definitions = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
    let mut losses = Vec::new();
    let parameters = super::super::parameters::project(
        &model.parameters,
        &definitions,
        &BTreeSet::new(),
        &BTreeSet::from([OWNER]),
        &mut losses,
    );
    let in_scope = parameters
        .iter()
        .filter(|parameter| parameter.id.as_str() == parameter_id("width"))
        .collect::<Vec<_>>();
    assert_eq!(in_scope.len(), 1, "{parameters:#?}");

    let mut features = [unresolved_sketch(OWNER)];
    let mut out = super::Sketches::default();
    super::project(
        &model,
        &mut features,
        &BTreeSet::new(),
        &in_scope,
        &mut out,
        &mut losses,
    );

    let sketch = SketchId::mint(SKETCH).unwrap();
    assert_eq!(
        features[0].definition.operation,
        FeatureOperation::Sketch(sketch.clone())
    );
    let [record] = out.records.as_slice() else {
        panic!("one sketch: {:#?}", out.records);
    };
    assert_eq!(record.owner.as_str(), OWNER);
    let SketchPlane::Placed(plane) = &record.plane else {
        panic!("a placed plane");
    };
    let Some(InputReference::Picks(selection)) = &plane.inputs[0].reference else {
        panic!("an unread input");
    };
    assert_eq!(selection.at, Anchor::Input);
    assert!(plane.resolved.is_some());

    assert_eq!(out.entities.len(), 5);
    let text = out
        .entities
        .iter()
        .find(|candidate| candidate.id.as_str() == entity("text"))
        .unwrap();
    let EntityDefinition::Defined { form, solved: None } = &text.definition else {
        panic!("a native entity: {:#?}", text.definition);
    };
    assert!(matches!(form.as_ref(), EntityForm::Native(kind) if kind.as_str() == "f3d:SketchText"));
    assert!(names(
        &losses,
        F3dLossCode::DesignSketchEntityNative,
        &entity("text")
    ));
    let circle = out
        .entities
        .iter()
        .find(|candidate| candidate.id.as_str() == entity("circle"))
        .unwrap();
    assert_eq!(circle.construction, Some(true));

    let relation = |key: &str| {
        &out.constraints
            .iter()
            .find(|candidate| candidate.id.as_str() == constraint(key))
            .unwrap()
            .relation
    };
    let point = |key: &str| {
        Operand::Entity(cadmpeg_ir::sketches::SketchEntityId::mint(entity(key)).unwrap())
    };
    assert_eq!(
        relation("width"),
        &SketchRelation::Distance {
            a: point("p0"),
            b: point("p1"),
            along: Some(DistanceAxis::Horizontal),
            doubled: None,
            value: Dimension::Parameter(ParameterId::mint(parameter_id("width")).unwrap()),
        }
    );
    assert!(!names(
        &losses,
        F3dLossCode::DesignSketchDimensionUnbound,
        &constraint("width")
    ));
    let SketchRelation::Radial {
        measure: RadialMeasure::Radius,
        value: Dimension::Literal(radius),
        ..
    } = relation("radius")
    else {
        panic!("a radius of the stored value: {:#?}", relation("radius"));
    };
    assert_eq!(radius.get(), 5.0);
    assert!(names(
        &losses,
        F3dLossCode::DesignSketchDimensionUnbound,
        &constraint("radius")
    ));
    assert!(
        matches!(relation("absent"), SketchRelation::Native { kind, values, .. }
            if kind.as_str() == "f3d:distance" && values.is_empty()),
        "{:#?}",
        relation("absent")
    );
    assert!(names(
        &losses,
        F3dLossCode::DesignSketchDimensionUnbound,
        &constraint("absent")
    ));
    assert!(matches!(
        relation("horizontal"),
        SketchRelation::Horizontal { b: None, .. }
    ));
}

#[test]
fn a_sketch_of_an_unordered_member_anchors_its_plane_in_the_current_state() {
    let model = model();
    let mut features = [unresolved_sketch(OWNER)];
    let mut out = super::Sketches::default();
    super::project(
        &model,
        &mut features,
        &BTreeSet::from([FeatureId::mint(OWNER).unwrap()]),
        &[],
        &mut out,
        &mut Vec::new(),
    );
    let SketchPlane::Placed(plane) = &out.records[0].plane else {
        panic!("a placed plane");
    };
    let Some(InputReference::Picks(selection)) = &plane.inputs[0].reference else {
        panic!("an unread input");
    };
    assert_eq!(selection.at, Anchor::Current);
}
