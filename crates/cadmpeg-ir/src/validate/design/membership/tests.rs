// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::design::feature::FeatureOperation;
use crate::design::ops::assembly::PlaceSet;
use crate::design::refs::Evaluated;
use crate::design::selection::{OccurrencePath, PathStep, StepRef};
use crate::document::CadIr;
use crate::ids::OccurrenceId;
use crate::products::{Occurrence, ProductDefinition, Storage};
use crate::transform::Transform;
use crate::validate::design::fixture::{after, assert_finding, design_findings, document, id};

const PART: &str = "test:design:definition#part";
const EVALUATION: &str = "test:design:evaluation#after-extrude";

/// The fixture document, whose fillet places occurrence `bolt` of the part.
fn placed() -> CadIr {
    let mut ir = document();
    let bolt: OccurrenceId = id("occurrence", "bolt");
    ir.model.occurrences.push(Occurrence::new(
        bolt.clone(),
        id("definition", "part"),
        None,
    ));
    ir.model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.id.as_str() == PART)
        .unwrap()
        .occurrences
        .push(bolt.clone());
    ir.design.features[1].definition.operation = FeatureOperation::PlaceSet(Box::new(PlaceSet {
        occurrences: vec![bolt],
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    ir
}

fn bolt_path() -> OccurrencePath {
    OccurrencePath(vec![PathStep {
        occurrence: StepRef::Occurrence(id("occurrence", "bolt")),
        configuration: None,
    }])
}

/// Whether a finding on `entity` holds `fragment`.
fn reports(ir: &CadIr, entity: &str, fragment: &str) -> bool {
    design_findings(ir)
        .iter()
        .any(|f| f.entity.as_deref() == Some(entity) && f.message.contains(fragment))
}

#[test]
fn a_member_entry_names_an_occurrence_existing_in_its_state() {
    let entry = "member entry names test:design:occurrence#bolt, which does not exist in the state";
    let mut ir = placed();
    let delta = &mut ir.design.evaluations[0].content.as_mut().unwrap().delta;
    delta
        .members
        .insert(id("occurrence", "bolt"), Some(id("definition", "part")));
    assert_finding(&ir, entry);
    ir.design.evaluations[0].of = Evaluated::State(after("fillet"));
    assert!(!reports(
        &ir,
        EVALUATION,
        "which does not exist in the state"
    ));
}

#[test]
fn a_pose_names_an_instance_existing_in_its_state() {
    let pose = "a pose names an instance of test:design:occurrence#bolt";
    let mut ir = placed();
    ir.design.scopes[0].current[0]
        .poses
        .insert(bolt_path(), Transform::identity());
    assert!(!reports(&ir, PART, pose));
    assert!(reports(&ir, EVALUATION, pose));
    let delta = &mut ir.design.evaluations[0].content.as_mut().unwrap().delta;
    delta.poses.insert(bolt_path(), None);
    assert!(!reports(&ir, EVALUATION, pose));
    ir.design.scopes[0].current[0].at = after("extrude");
    assert!(reports(&ir, PART, pose));
}

#[test]
fn a_state_path_begins_at_an_occurrence_its_membership_places_in_the_scope() {
    let moved = "begins at test:design:occurrence#bolt, which the state's membership places in test:design:definition#other";
    let mut ir = placed();
    ir.model.product_definitions.push(ProductDefinition::new(
        id("definition", "other"),
        Storage::Embedded,
    ));
    ir.design.evaluations[0].of = Evaluated::State(after("fillet"));
    let delta = &mut ir.design.evaluations[0].content.as_mut().unwrap().delta;
    delta.poses.insert(bolt_path(), Some(Transform::identity()));
    assert!(!reports(
        &ir,
        EVALUATION,
        "which the state's membership places in"
    ));
    let delta = &mut ir.design.evaluations[0].content.as_mut().unwrap().delta;
    delta
        .members
        .insert(id("occurrence", "bolt"), Some(id("definition", "other")));
    assert!(reports(&ir, EVALUATION, moved));
    let delta = &mut ir.design.evaluations[0].content.as_mut().unwrap().delta;
    delta
        .members
        .insert(id("occurrence", "bolt"), Some(id("definition", "part")));
    assert!(!reports(
        &ir,
        EVALUATION,
        "which the state's membership places in"
    ));
}
