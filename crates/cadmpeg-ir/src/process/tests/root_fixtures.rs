// SPDX-License-Identifier: Apache-2.0
//! Hand-built values of the process root's types, one or more per variant.

use std::collections::BTreeMap;

use super::fixtures::{
    self, feature_id, id, operation_id, parameter, process_expressions, program_id, real, setup_id,
    text,
};
use crate::design::evaluation::{EvaluationStatus, StateBase, StateDelta};
use crate::design::ids::{ProcessStateId, ToolpathId};
use crate::design::parameter::ParameterValue;
use crate::design::refs::{Member, Position};
use crate::design::units::{Quantity, ValueKind};
use crate::document::Process;
use crate::features::FeatureId;
use crate::process::groups::{ProcessDefinition, ProcessOwner, ProcessParameter};
use crate::process::state::{ProcessBase, ProcessContent, ProcessState, ProcessStateOf};
use crate::process::toolpath::{Toolpath, ToolpathContent};
use crate::transform::Transform;

pub(super) fn process_parameters() -> Vec<ProcessParameter> {
    vec![
        ProcessParameter {
            id: parameter(1),
            owner: ProcessOwner::Operation(operation_id(1)),
            name: text("stepdown"),
            kind: Some(ValueKind::Quantity(Quantity::LENGTH)),
            definition: ProcessDefinition::Expression(process_expressions().remove(0)),
            value: Some(ParameterValue::Number(real(2.5))),
            native_ref: Some("f3d:parameter#stepdown".to_owned()),
        },
        ProcessParameter {
            id: parameter(2),
            owner: ProcessOwner::Setup(setup_id(1)),
            name: text("stockZHigh"),
            kind: None,
            definition: ProcessDefinition::Computed,
            value: Some(ParameterValue::Number(real(12.0))),
            native_ref: None,
        },
        ProcessParameter {
            id: parameter(3),
            owner: ProcessOwner::Program(program_id(1)),
            name: text("creo:29"),
            kind: Some(ValueKind::Text),
            definition: ProcessDefinition::Computed,
            value: None,
            native_ref: None,
        },
    ]
}

pub(super) fn toolpath_id(key: u32) -> ToolpathId {
    id("toolpath", key)
}

pub(super) fn toolpaths() -> Vec<Toolpath> {
    vec![
        Toolpath {
            id: toolpath_id(1),
            operation: operation_id(1),
            feature: Some(feature_id(1)),
            configuration: Some(id("configuration", 1)),
            status: EvaluationStatus::Current,
            content: ToolpathContent::Motions {
                frame: Transform::identity(),
                motions: fixtures::motions(),
            },
        },
        Toolpath {
            id: toolpath_id(2),
            operation: operation_id(2),
            feature: None,
            configuration: None,
            status: EvaluationStatus::Stale,
            content: ToolpathContent::Native("toolpath-2".to_owned()),
        },
    ]
}

pub(super) fn state_id(key: u32) -> ProcessStateId {
    id("process-state", key)
}

pub(super) fn process_bases() -> Vec<ProcessBase> {
    vec![
        ProcessBase::Design(StateBase::Empty),
        ProcessBase::Design(StateBase::Current(Position::After(Member::Feature(id(
            "feature", 3,
        ))))),
        ProcessBase::Design(StateBase::Evaluation(id("evaluation", 1))),
        ProcessBase::State(state_id(2)),
    ]
}

fn state_delta() -> StateDelta {
    StateDelta {
        owners: BTreeMap::from([
            (id("body", 1), Some(id("product-definition", 1))),
            (id("body", 2), None),
        ]),
        members: BTreeMap::from([(id("occurrence", 1), None)]),
        ..StateDelta::default()
    }
}

pub(super) fn process_contents() -> Vec<ProcessContent> {
    vec![
        ProcessContent {
            base: ProcessBase::State(state_id(2)),
            delta: state_delta(),
        },
        ProcessContent {
            base: ProcessBase::Design(StateBase::Empty),
            delta: StateDelta::default(),
        },
    ]
}

pub(super) fn process_states() -> Vec<ProcessState> {
    let mut contents = process_contents();
    vec![
        ProcessState {
            id: state_id(1),
            of: ProcessStateOf::After(operation_id(1)),
            configuration: Some(id("configuration", 1)),
            status: EvaluationStatus::Current,
            content: Some(contents.remove(0)),
            feature: None,
        },
        ProcessState {
            id: state_id(2),
            of: ProcessStateOf::Stock(setup_id(1)),
            configuration: None,
            status: EvaluationStatus::Current,
            content: Some(contents.remove(0)),
            feature: None,
        },
        ProcessState {
            id: state_id(3),
            of: ProcessStateOf::Removal(operation_id(2)),
            configuration: None,
            status: EvaluationStatus::Stale,
            content: None,
            feature: Some(id::<FeatureId>("feature", 9)),
        },
    ]
}

/// An empty process root, and one with a populated record in every arena.
pub(super) fn processes() -> Vec<Process> {
    vec![
        Process::default(),
        Process {
            setups: fixtures::setups(),
            operations: fixtures::operations(),
            features: fixtures::machining_features(),
            groups: fixtures::process_groups(),
            tools: fixtures::tools(),
            presets: fixtures::presets(),
            machines: fixtures::machines(),
            programs: fixtures::programs(),
            parameters: process_parameters(),
            toolpaths: toolpaths(),
            states: process_states(),
        },
    ]
}
