// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::serde_erase::json;
use std::collections::BTreeMap;

use cadmpeg_ir::annotations::{AnnotationKey, AnnotationTarget, Annotations, StreamHandle};
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding};
use cadmpeg_ir::design::evaluation::{
    Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use cadmpeg_ir::design::ids::EvaluationId;
use cadmpeg_ir::design::refs::Evaluated;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::parameters::{
    ConfigurationEvaluation, ConfigurationFeatureState, DesignConfiguration,
};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{ConfigurationId, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::ids::{BodyId, PointId, ProductDefinitionId, UnknownId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use serde_json::json;

use crate::loss::SldprtLossCode;

const FACE: &str = "sldprt:brep:face#37@block@1";
const KEPT_FACE: &str = "sldprt:brep:face#38@block@2";

fn face_binding(id: &str, face: &str, appearance: &str) -> AppearanceBinding {
    json::from_value(json!({
        "id": id,
        "target": {"entity": {"kind": "face", "value": face}},
        "appearance": appearance,
        "channels": {},
    }))
    .unwrap()
}

fn appearance(id: &str) -> Appearance {
    json::from_value(json!({
        "id": id,
        "schema": "entity-53",
        "base_color": {"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0},
        "properties": {},
        "textures": [],
    }))
    .unwrap()
}

#[test]
fn the_appearance_on_a_retired_face_goes_with_a_loss_naming_its_configuration() {
    let configuration = ConfigurationId::mint("sldprt:model:configuration#7:0").unwrap();
    let mut ir = CadIr::empty();
    ir.model
        .appearances
        .push(appearance("sldprt:appearance:entity53#1@block@1"));
    ir.model
        .appearances
        .push(appearance("sldprt:appearance:entity53#2@block@2"));
    ir.model.appearance_bindings.push(face_binding(
        "sldprt:appearance:binding#face:37:1@block@1",
        FACE,
        "sldprt:appearance:entity53#1@block@1",
    ));
    ir.model.appearance_bindings.push(face_binding(
        "sldprt:appearance:binding#face:38:2@block@2",
        KEPT_FACE,
        "sldprt:appearance:entity53#2@block@2",
    ));
    let mut unknowns = vec![UnknownRecord::retained(
        UnknownId::mint("sldprt:file:block#1").unwrap(),
        0,
        Vec::new(),
        vec![FACE.to_owned(), KEPT_FACE.to_owned()],
    )];
    let retired = BTreeMap::from([(FACE.to_owned(), configuration.clone())]);
    let mut losses = Vec::new();
    super::retire(
        &mut ir,
        &mut Annotations::default(),
        &mut unknowns,
        &retired,
        &mut losses,
    );

    let [binding] = ir.model.appearance_bindings.as_slice() else {
        panic!("one binding: {:#?}", ir.model.appearance_bindings);
    };
    assert_eq!(
        binding.appearance.as_str(),
        "sldprt:appearance:entity53#2@block@2"
    );
    let [kept] = ir.model.appearances.as_slice() else {
        panic!("one appearance: {:#?}", ir.model.appearances);
    };
    assert_eq!(kept.id, binding.appearance);
    assert_eq!(unknowns[0].links(), [KEPT_FACE.to_owned()]);
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:#?}");
    };
    assert_eq!(
        loss.code,
        SldprtLossCode::ConfigStatePresentationUnstated.kind()
    );
    assert!(loss.message.contains(configuration.as_str()), "{loss:#?}");
    assert!(
        loss.message
            .contains("1 appearance binding(s) on the records of its state"),
        "{loss:#?}"
    );
}

fn active_state(outputs: &[&BodyId]) -> ConfigurationFeatureState {
    ConfigurationFeatureState {
        evaluation: ConfigurationEvaluation::Active {
            outputs: outputs.iter().map(|body| (*body).clone()).collect(),
        },
        dependencies: DistinctMembers::default(),
        definition: FeatureDefinition::Operation(FeatureOperation::DatumPoint {
            position: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            construction: None,
        }),
    }
}

/// The feature states of a configuration that name the bodies its
/// evaluation holds lose those outputs, with one loss that counts those whose
/// feature no face name in the evaluation names: here every one, since the
/// configuration has no evaluation.
#[test]
fn the_outputs_on_a_retired_body_go_with_a_loss_counting_them() {
    let configuration = ConfigurationId::mint("sldprt:model:configuration#7:0").unwrap();
    let stored = BodyId::mint("sldprt:brep:body#5@block@1").unwrap();
    let kept = BodyId::mint("sldprt:brep:body#6@block@2").unwrap();
    let boss = FeatureId::mint("sldprt:history:feature#boss").unwrap();
    let cut = FeatureId::mint("sldprt:history:feature#cut").unwrap();
    let mut ir = CadIr::empty();
    ir.model.configurations.push(DesignConfiguration {
        id: configuration.clone(),
        ordinal: 0,
        active: false,
        source_index: Some(0),
        name: None,
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::from([
            (boss.clone(), active_state(&[&stored, &kept])),
            (cut.clone(), active_state(&[&stored])),
        ]),
        native_ref: None,
    });
    let retired = BTreeMap::from([(stored.as_str().to_owned(), configuration.clone())]);
    let mut losses = Vec::new();
    super::retire(
        &mut ir,
        &mut Annotations::default(),
        &mut [],
        &retired,
        &mut losses,
    );

    let states = &ir.model.configurations[0].feature_states;
    assert_eq!(states[&boss].evaluation.outputs(), [kept]);
    assert!(states[&cut].evaluation.outputs().is_empty());
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:#?}");
    };
    assert_eq!(loss.code, SldprtLossCode::ConfigStateOutputsUnstated.kind());
    assert!(loss.message.contains(configuration.as_str()), "{loss:#?}");
    assert!(loss.message.contains("2 feature output(s)"), "{loss:#?}");
}

/// A record that moves into the delta of a configuration's evaluation keeps
/// its annotations under its key in that evaluation (§3.2), and its
/// top-level annotations go with it.
#[test]
fn a_retired_record_keeps_its_annotations_under_its_evaluation() {
    let configuration = ConfigurationId::mint("sldprt:model:configuration#7:0").unwrap();
    let evaluation = EvaluationId::mint("sldprt:design:evaluation#configuration:7:0").unwrap();
    let id = PointId::mint("sldprt:brep:point#4@block@1").unwrap();
    let point = Point::new(
        id.clone(),
        FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
        None,
    );
    let mut ir = CadIr::empty();
    ir.geometry.points.push(point.clone());
    let mut records = Topology::default();
    records.points.push(point);
    ir.design.evaluations.push(Evaluation {
        id: evaluation.clone(),
        scope: ProductDefinitionId::mint("sldprt:model:definition#part").unwrap(),
        configuration: Some(configuration.clone()),
        of: Evaluated::Tool(FeatureId::mint("sldprt:history:feature#boss").unwrap()),
        status: EvaluationStatus::Current,
        content: Some(StateContent {
            base: StateBase::Empty,
            delta: StateDelta {
                records,
                ..StateDelta::default()
            },
        }),
        native_ref: None,
    });
    let stream = StreamHandle::new(cadmpeg_ir::stream_name!("Config-0-Partition"));
    let mut builder = AnnotationBuilder::new();
    builder.note(&id, &stream, 40, Some("point"));
    builder.exactness(&id, Exactness::Inferred);
    let mut annotations = builder.build();

    let retired = BTreeMap::from([(id.as_str().to_owned(), configuration)]);
    super::retire(
        &mut ir,
        &mut annotations,
        &mut [],
        &retired,
        &mut Vec::new(),
    );

    let key = AnnotationKey::in_delta(&evaluation, &id);
    assert_eq!(
        key.target(),
        AnnotationTarget::Delta {
            qualifier: evaluation.as_str(),
            record: id.as_str(),
        }
    );
    assert_eq!(annotations.provenance[&key].offset, 40);
    assert_eq!(annotations.exactness()[&key].entity(), Exactness::Inferred);
    assert!(!annotations.provenance.contains_key(id.as_str()));
    assert!(!annotations.exactness().contains_key(id.as_str()));
}
