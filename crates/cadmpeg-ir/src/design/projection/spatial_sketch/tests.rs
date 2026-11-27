// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;
use serde_json::json;

use super::{restate, spatial_curve};
use crate::design::dimension::Dimension;
use crate::design::ops::sketch::{
    EntityDefinition, EntityForm, Operand, SketchRelation, SpatialCurve,
};
use crate::design::units::Quantity;
use crate::document::Model;
use crate::features::FeatureId;
use crate::geometry::InlineCurve;
use crate::sketches::{SpatialSketch, SpatialSketchConstraint, SpatialSketchEntity};

fn model() -> Model {
    let mut model = Model::default();
    model.spatial_sketches.push(
        json::from_value::<SpatialSketch>(json!({
            "id": "test:model:spatial-sketch#s", "name": "Path", "profiles": []
        }))
        .expect("valid sketch"),
    );
    for (id, geometry) in [
        (
            "a",
            json!({"kind": "point", "position": {"x": 0.0, "y": 0.0, "z": 0.0}}),
        ),
        (
            "l",
            json!({"kind": "line", "start": {"x": 0.0, "y": 0.0, "z": 0.0},
                "end": {"x": 0.0, "y": 3.0, "z": 4.0}}),
        ),
        ("n", json!({"kind": "native", "native_kind": "test:Helix"})),
    ] {
        model.spatial_sketch_entities.push(
            json::from_value::<SpatialSketchEntity>(json!({
                "id": format!("test:model:spatial-sketch-entity#{id}"),
                "sketch": "test:model:spatial-sketch#s",
                "construction": false,
                "endpoint_refs": [],
                "geometry": geometry,
            }))
            .expect("valid entity"),
        );
    }
    for (id, definition) in [
        (
            "length",
            json!({"kind": "line_length", "entity": "test:model:spatial-sketch-entity#l",
                "parameter": "test:model:parameter#length"}),
        ),
        (
            "gap",
            json!({"kind": "point_distance", "first": "test:model:spatial-sketch-entity#a",
                "second": "test:model:spatial-sketch-entity#n",
                "parameter": "test:model:parameter#gap"}),
        ),
        (
            "group",
            json!({"kind": "spline_group", "entities": [
                "test:model:spatial-sketch-entity#l", "test:model:spatial-sketch-entity#n"]}),
        ),
    ] {
        model.spatial_sketch_constraints.push(
            json::from_value::<SpatialSketchConstraint>(json!({
                "id": format!("test:model:sketch-constraint#{id}"),
                "sketch": "test:model:spatial-sketch#s",
                "definition": definition,
            }))
            .expect("valid constraint"),
        );
    }
    model
}

#[test]
fn a_spatial_sketch_states_its_curves_and_relations_and_names_the_rest() {
    let model = model();
    let owner = FeatureId::mint("test:model:feature#path").unwrap();
    let out = restate(
        &cadmpeg_core::nonblank_literal!("test"),
        &owner,
        &model.spatial_sketches[0],
        &model,
        |parameter, quantity| {
            parameter.as_str() == "test:model:parameter#length" && quantity == Quantity::LENGTH
        },
    );
    assert_eq!(out.sketch.owner, owner);
    assert_eq!(out.sketch.name.as_deref(), Some("Path"));
    assert_eq!(out.entities.len(), 3);
    let EntityDefinition::Free(SpatialCurve::Bounded(line)) = &out.entities[1].definition else {
        panic!("a bounded line: {:?}", out.entities[1].definition);
    };
    assert!(matches!(line.carrier, InlineCurve::Line(_)));
    let range = line.range.expect("a range").endpoints();
    assert!(range[0].abs() < 1e-12 && (range[1] - 5.0).abs() < 1e-12);
    let EntityDefinition::Defined { form, .. } = &out.entities[2].definition else {
        panic!("a native entity");
    };
    assert!(matches!(&**form, EntityForm::Native(kind) if kind.as_str() == "test:Helix"));
    assert_eq!(out.native_entities, ["test:model:spatial-sketch-entity#n"]);

    let SketchRelation::Length { of, value } = &out.constraints[0].relation else {
        panic!("a length: {:?}", out.constraints[0].relation);
    };
    assert_eq!(of[0].as_str(), "test:model:spatial-sketch-entity#l");
    assert!(
        matches!(value, Dimension::Parameter(p) if p.as_str() == "test:model:parameter#length")
    );

    assert_eq!(out.unbound, ["test:model:sketch-constraint#gap"]);
    let SketchRelation::Native {
        kind,
        operands,
        values,
    } = &out.constraints[1].relation
    else {
        panic!("a native distance: {:?}", out.constraints[1].relation);
    };
    assert_eq!(kind.as_str(), "test:point_distance");
    assert_eq!(operands.len(), 2);
    assert!(values.is_empty());
    let SketchRelation::Native { kind, operands, .. } = &out.constraints[2].relation else {
        panic!("a native group");
    };
    assert_eq!(kind.as_str(), "test:spline_group");
    assert!(
        matches!(&operands[..], [Operand::Entity(l), Operand::Entity(n)]
        if l.as_str() == "test:model:spatial-sketch-entity#l"
            && n.as_str() == "test:model:spatial-sketch-entity#n")
    );
    assert_eq!(
        out.native_relations,
        [
            "test:model:sketch-constraint#gap",
            "test:model:sketch-constraint#group"
        ]
    );
}

#[test]
fn an_arc_that_crosses_its_reference_direction_has_an_increasing_range() {
    let arc = json::from_value(json!({"kind": "arc",
        "center": {"x": 0.0, "y": 0.0, "z": 0.0},
        "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
        "reference_direction": {"x": 1.0, "y": 0.0, "z": 0.0},
        "radius": 2.0, "start_angle": 5.0, "end_angle": 1.0}))
    .expect("valid arc");
    let Some(SpatialCurve::Bounded(curve)) = spatial_curve(&arc) else {
        panic!("a bounded arc");
    };
    assert!(matches!(curve.carrier, InlineCurve::Circle(_)));
    let range = curve.range.expect("a range").endpoints();
    assert!((range[0] - 5.0).abs() < 1e-12);
    assert!((range[1] - (1.0 + std::f64::consts::TAU)).abs() < 1e-12);
}

#[test]
fn a_native_spatial_relation_keeps_its_operands_and_state() {
    let mut model = model();
    for entity in &mut model.spatial_sketch_entities {
        entity.native_ref = Some(format!("record:{}", entity.id().as_str()));
    }
    model.spatial_sketch_constraints.push(
        json::from_value::<SpatialSketchConstraint>(json!({
            "id": "test:model:sketch-constraint#bend",
            "sketch": "test:model:spatial-sketch#s",
            "definition": {"kind": "native", "native_kind": "test:Bend", "native_state": 3,
                "operands": [
                    {"native_kind": "test:curve",
                        "native_ref": "record:test:model:spatial-sketch-entity#l"},
                    {"native_kind": "test:curve", "native_ref": "record:elsewhere"}]},
        }))
        .expect("valid constraint"),
    );
    let out = restate(
        &cadmpeg_core::nonblank_literal!("test"),
        &FeatureId::mint("test:model:feature#path").unwrap(),
        &model.spatial_sketches[0],
        &model,
        |_, _| false,
    );
    let bend = out
        .constraints
        .iter()
        .find(|constraint| constraint.id.as_str() == "test:model:sketch-constraint#bend")
        .expect("the native relation");
    let SketchRelation::Native { kind, operands, .. } = &bend.relation else {
        panic!("a native relation: {:?}", bend.relation);
    };
    assert_eq!(kind.as_str(), "test:Bend");
    assert!(
        matches!(&operands[..], [Operand::Entity(l), Operand::Model(unread)]
        if l.as_str() == "test:model:spatial-sketch-entity#l"
            && matches!(unread.picks, crate::design::selection::Picks::Unread))
    );
    assert_eq!(
        bend.native_options
            .iter()
            .map(|(name, value)| (name.as_str(), value.clone()))
            .collect::<Vec<_>>(),
        [(
            "test:native_state",
            crate::design::options::NativeOption::Integer(Dimension::Literal(3))
        )]
    );
    assert!(out
        .native_relations
        .contains(&"test:model:sketch-constraint#bend".to_owned()));
}
