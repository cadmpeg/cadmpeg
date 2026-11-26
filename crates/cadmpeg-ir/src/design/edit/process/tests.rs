// SPDX-License-Identifier: Apache-2.0
//! What a design edit marks and refreshes in the process and PMI layers.

use cadmpeg_core::serde_erase::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use cadmpeg_core::json;
use cadmpeg_core::text::NonBlankString;

use crate::design::configuration::{DesignConfiguration, Overrides};
use crate::design::container::{Container, OrderedKind, Rollback};
use crate::design::dimension::{Dimension, ParamRef, ParameterRef, RefTarget};
use crate::design::evaluation::{
    CurrentState, Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use crate::design::expr::{parse, Expression, Neutral};
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use crate::design::ids::{OperationId, ProcessParameterId, SetupId};
use crate::design::options::{NativeOption, NativeOptions};
use crate::design::parameter::{
    DesignParameter, Deviations, FitDeviations, ParameterDefinition, ParameterValue, Tolerance,
};
use crate::design::refs::{Evaluated, Member, Position};
use crate::design::scope::DesignScope;
use crate::design::selection::{Anchor, Picks, Selection};
use crate::design::units::{Quantity, ValueKind};
use crate::features::{ConfigurationId, FeatureId, ParameterId};
use crate::ids::ProductDefinitionId;
use crate::pmi::{
    FitZone, LimitsAndFits, PmiAnnotation, PmiDefinition, PmiDimension, PmiDimensionKind,
};
use crate::process::groups::{ProcessDefinition, ProcessOwner, ProcessParameter};
use crate::process::operation::Operation;
use crate::process::setup::Setup;
use crate::process::state::{ProcessBase, ProcessContent, ProcessState, ProcessStateOf};
use crate::process::toolpath::{Toolpath, ToolpathContent};
use crate::scalar::FiniteReal;
use crate::CadIr;

fn id<T>(kind: &str, key: u32) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:process-edit:{kind}#{key}")).unwrap()
}

fn feature(key: u32) -> FeatureId {
    id("feature", key)
}

fn parameter_id(key: u32) -> ParameterId {
    id("parameter", key)
}

fn operation_id(key: u32) -> OperationId {
    id("operation", key)
}

fn setup_id(key: u32) -> SetupId {
    id("setup", key)
}

fn process_parameter_id(key: u32) -> ProcessParameterId {
    id("process-parameter", key)
}

fn definition() -> ProductDefinitionId {
    id("product-definition", 1)
}

fn real(value: f64) -> FiniteReal {
    FiniteReal::new(value).unwrap()
}

fn design_ref(key: u32) -> ParameterRef {
    ParameterRef {
        target: RefTarget::Parameter(parameter_id(key)),
        through: None,
    }
}

fn after(key: u32) -> Position {
    Position::After(Member::Feature(feature(key)))
}

fn length(key: u32, text: &str, refs: &[ParameterRef], value: f64) -> DesignParameter {
    let mut parameter = blank_parameter(key);
    parameter.definition = ParameterDefinition::Expression(Expression::Neutral(Neutral(
        parse(text, refs, &[]).unwrap(),
    )));
    parameter.value = Some(ParameterValue::Number(real(value)));
    parameter
}

fn blank_parameter(key: u32) -> DesignParameter {
    DesignParameter {
        id: parameter_id(key),
        scope: definition(),
        owner: None,
        name: Some(NonBlankString::new(format!("p{key}")).unwrap()),
        kind: ValueKind::Quantity(Quantity::LENGTH),
        unit: None,
        definition: ParameterDefinition::Driven,
        value: None,
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: None,
    }
}

fn native_feature(key: u32, operation: FeatureOperation) -> Feature {
    Feature {
        id: feature(key),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation,
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

fn operation(key: u32, kind: &serde_json::Value) -> Operation {
    json::from_value(json!({
        "id": operation_id(key).as_str(),
        "kind": kind,
        "native_kind": {"codec": "test", "value": {"kind": "text", "value": "op"}},
        "features": [],
        "groups": [],
        "geometry": [],
        "heights": {},
        "radii": {},
        "axial": {},
        "technology": {"feeds": []},
        "configuration_states": {},
    }))
    .unwrap()
}

fn machining(key: u32) -> Operation {
    operation(
        key,
        &json!({"kind": "machining", "class": {"kind": "probing"}}),
    )
}

fn setup(key: u32, steps: &[u32]) -> Setup {
    json::from_value(json!({
        "id": setup_id(key).as_str(),
        "scope": definition().as_str(),
        "steps": steps
            .iter()
            .map(|step| json!({"kind": "operation", "value": operation_id(*step).as_str()}))
            .collect::<Vec<_>>(),
        "workpieces": [],
        "wcs": {},
        "work_offsets": [],
    }))
    .unwrap()
}

fn toolpath(key: u32, operation: u32) -> Toolpath {
    Toolpath {
        id: id("toolpath", key),
        operation: operation_id(operation),
        feature: None,
        configuration: None,
        status: EvaluationStatus::Current,
        content: ToolpathContent::Native(format!("toolpath-{key}")),
    }
}

fn state(key: u32, of: ProcessStateOf) -> ProcessState {
    ProcessState {
        id: id("process-state", key),
        of,
        configuration: None,
        status: EvaluationStatus::Current,
        content: None,
        feature: None,
    }
}

/// One scope whose timeline holds feature 1, bound to design parameter 1
/// (10 mm), and feature 2, unbound; one current state after feature 2.
/// Design parameter 2 is independent (5 mm). Process parameter 1, owned by
/// operation 1, is three times design parameter 1 (30). Setup 1 machines
/// operations 1, 2, and 3 in that order, where 3 is a modifier of 1; setup
/// 2 machines operation 4, which depends on nothing. Each operation has one
/// toolpath (keys match) and one `After` state (keys match); each setup
/// has a `Stock` state (10 and 20).
fn machined() -> CadIr {
    let mut document = CadIr::empty();
    let container: crate::design::ids::ContainerId = id("container", 1);
    document.design.containers.push(Container::Ordered {
        id: container.clone(),
        kind: OrderedKind::Timeline,
        members: (1..=2).map(|key| Member::Feature(feature(key))).collect(),
        folders: Vec::new(),
        rollback: Some(Rollback::End),
        placement: None,
    });
    document.design.scopes.push(DesignScope {
        definition: definition(),
        roots: vec![container],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: vec![CurrentState {
            at: after(2),
            bodies: Vec::new(),
            poses: BTreeMap::new(),
            status: EvaluationStatus::Current,
            native_ref: None,
        }],
        unproduced: BTreeSet::new(),
    });
    document.design.features = vec![
        native_feature(
            1,
            FeatureOperation::Native {
                kind: crate::codec_name!("test:op"),
                native_options: NativeOptions::from([(
                    crate::codec_name!("test:depth"),
                    NativeOption::Length(Dimension::Parameter(parameter_id(1))),
                )]),
                source_settings: NativeOptions::new(),
            },
        ),
        native_feature(
            2,
            FeatureOperation::Unresolved {
                family: OperationFamily::Extrude,
            },
        ),
    ];
    document.design.parameters = vec![length(1, "10[mm]", &[], 10.0), length(2, "5[mm]", &[], 5.0)];
    let process = &mut document.process;
    process.setups = vec![setup(1, &[1, 2, 3]), setup(2, &[4])];
    process.operations = vec![
        machining(1),
        machining(2),
        operation(
            3,
            &json!({"kind": "modifier", "modifier": "dogbone", "inputs": [operation_id(1).as_str()]}),
        ),
        machining(4),
    ];
    process.parameters = vec![ProcessParameter {
        id: process_parameter_id(1),
        owner: ProcessOwner::Operation(operation_id(1)),
        name: NonBlankString::new("depth").unwrap(),
        kind: Some(ValueKind::Quantity(Quantity::LENGTH)),
        definition: ProcessDefinition::Expression(Expression::Neutral(Neutral(
            parse("$0 * 3", &[ParamRef::Design(design_ref(1))], &[]).unwrap(),
        ))),
        value: Some(ParameterValue::Number(real(30.0))),
        native_ref: None,
    }];
    process.toolpaths = (1..=4).map(|key| toolpath(key, key)).collect();
    process.states = (1..=4)
        .map(|key| state(key, ProcessStateOf::After(operation_id(key))))
        .chain([
            state(10, ProcessStateOf::Stock(setup_id(1))),
            state(20, ProcessStateOf::Stock(setup_id(2))),
        ])
        .collect();
    document
}

fn toolpath_status(document: &CadIr, key: u32) -> EvaluationStatus {
    document
        .process
        .toolpaths
        .iter()
        .find(|toolpath| toolpath.id == id("toolpath", key))
        .unwrap()
        .status
}

fn state_status(document: &CadIr, key: u32) -> EvaluationStatus {
    document
        .process
        .states
        .iter()
        .find(|state| state.id == id("process-state", key))
        .unwrap()
        .status
}

fn literal(value: f64) -> super::super::Literal {
    super::super::Literal::Number {
        value: real(value),
        unit: crate::design::units::Unit(vec![crate::design::units::UnitFactor {
            symbol: crate::design::units::UnitSymbol::Mm,
            power: std::num::NonZeroI8::new(1).unwrap(),
        }]),
    }
}

#[test]
fn a_design_edit_marks_the_operations_that_depend_on_it_and_what_follows_them() {
    use EvaluationStatus::{Current, Stale};
    let mut document = machined();
    document
        .set_dimension(&parameter_id(1), literal(20.0))
        .unwrap();
    assert_eq!(
        (1..=4)
            .map(|key| toolpath_status(&document, key))
            .collect::<Vec<_>>(),
        [Stale, Current, Stale, Current],
        "operation 1, and modifier 3 that takes it",
    );
    assert_eq!(
        (1..=4)
            .map(|key| state_status(&document, key))
            .collect::<Vec<_>>(),
        [Stale, Stale, Stale, Current],
        "operation 1 and every later operation of setup 1",
    );
    assert_eq!(
        [state_status(&document, 10), state_status(&document, 20)],
        [Current, Current]
    );
}

#[test]
fn a_design_edit_recomputes_a_process_value_that_reads_it() {
    let mut document = machined();
    document
        .set_dimension(&parameter_id(1), literal(20.0))
        .unwrap();
    assert_eq!(
        document.process.parameters[0].value,
        Some(ParameterValue::Number(real(60.0)))
    );
    document.process.parameters[0].definition = ProcessDefinition::Expression(Expression::Source {
        dialect: crate::design::expr::ExpressionDialect::FusionCam,
        tokens: vec![crate::design::expr::ExpressionToken::Reference(
            ParamRef::Design(design_ref(1)),
        )],
    });
    document
        .set_dimension(&parameter_id(1), literal(30.0))
        .unwrap();
    assert_eq!(document.process.parameters[0].value, None);
}

#[test]
fn an_unrelated_design_edit_leaves_the_process_alone() {
    let mut document = machined();
    let before = document.process.clone();
    document
        .set_dimension(&parameter_id(2), literal(6.0))
        .unwrap();
    assert_eq!(document.process, before);
}

#[test]
fn a_choice_selected_by_a_dependent_parameter_loses_its_value() {
    let mut document = machined();
    document.process.operations[1].cycle = Some(
        json::from_value(json!({
            "kind": {"value": {"kind": "drill"}, "parameters": [process_parameter_id(1).as_str()]},
        }))
        .unwrap(),
    );
    let constant = document.process.operations[3].clone();
    document
        .set_dimension(&parameter_id(1), literal(20.0))
        .unwrap();
    let cycle = document.process.operations[1].cycle.as_ref().unwrap();
    let kind = cycle.kind.as_ref().unwrap();
    assert_eq!(kind.value, None);
    assert_eq!(kind.parameters, BTreeSet::from([process_parameter_id(1)]));
    assert_eq!(document.process.operations[3], constant);
    assert_eq!(
        toolpath_status(&document, 2),
        EvaluationStatus::Stale,
        "operation 2 now names a dependent parameter"
    );
}

#[test]
fn an_operation_that_selects_from_a_stale_state_is_stale() {
    let mut document = machined();
    document.process.setups[1].fixtures = Some(Selection {
        at: Anchor::Current,
        picks: Picks::Unread,
        native: None,
    });
    document
        .set_dimension(&parameter_id(1), literal(20.0))
        .unwrap();
    assert_eq!(toolpath_status(&document, 4), EvaluationStatus::Stale);
    assert_eq!(state_status(&document, 20), EvaluationStatus::Stale);
}

#[test]
fn a_bound_pmi_dimension_loses_its_cached_fit_deviations() {
    let mut document = machined();
    let deviations = Deviations {
        upper: real(0.1),
        lower: real(0.0),
    };
    let fit = |cached: FitDeviations| Tolerance::Fit {
        fit: LimitsAndFits {
            form_variance: "H".into(),
            zone: FitZone::Hole,
            grade: "7".into(),
            source: None,
        },
        mate: None,
        deviations: Some(cached),
    };
    let annotation = |key: u32, nominal: Dimension<_>, tolerance: Tolerance| {
        let mut dimension = PmiDimension::new(PmiDimensionKind::Diameter);
        dimension.nominal = Some(nominal);
        dimension.tolerance = Some(tolerance);
        PmiAnnotation {
            id: id("pmi", key),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::Dimension(dimension),
            native_ref: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        }
    };
    document.model.pmi = vec![
        annotation(
            1,
            Dimension::Parameter(parameter_id(1)),
            fit(FitDeviations::Cell(deviations.clone())),
        ),
        annotation(
            2,
            Dimension::Parameter(parameter_id(1)),
            fit(FitDeviations::Own(deviations.clone())),
        ),
        annotation(
            3,
            Dimension::Parameter(parameter_id(2)),
            fit(FitDeviations::Unchecked(deviations.clone())),
        ),
    ];
    document
        .set_dimension(&parameter_id(1), literal(20.0))
        .unwrap();
    let cached = |key: usize| match &document.model.pmi[key].definition {
        PmiDefinition::Dimension(dimension) => match &dimension.tolerance {
            Some(Tolerance::Fit { deviations, .. }) => deviations.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    assert_eq!(cached(0), None);
    assert_eq!(cached(1), Some(FitDeviations::Own(deviations.clone())));
    assert_eq!(
        cached(2),
        Some(FitDeviations::Unchecked(deviations.clone()))
    );
}

#[test]
fn switching_the_active_configuration_rebases_process_states() {
    let mut document = machined();
    let scope = definition();
    let configuration = |key: u32| -> ConfigurationId { id("configuration", key) };
    for key in [1, 2] {
        document.design.configurations.push(DesignConfiguration {
            id: configuration(key),
            scope: scope.clone(),
            name: format!("c{key}"),
            parent: None,
            comment: None,
            overrides: Overrides::default(),
            active_arrangements: BTreeSet::new(),
            materials: Vec::new(),
            native_ref: None,
        });
    }
    document.design.scopes[0].active_configuration = Some(configuration(1));
    document.design.evaluations.push(Evaluation {
        id: id("evaluation", 2),
        scope: scope.clone(),
        configuration: Some(configuration(2)),
        of: Evaluated::State(after(2)),
        status: EvaluationStatus::Current,
        content: Some(StateContent {
            base: StateBase::Empty,
            delta: StateDelta::default(),
        }),
        native_ref: None,
    });
    let based = |base: StateBase| {
        Some(ProcessContent {
            base: ProcessBase::Design(base),
            delta: StateDelta::default(),
        })
    };
    document.process.states[0].content = based(StateBase::Current(after(2)));
    document.process.states[1].content = based(StateBase::Evaluation(id("evaluation", 2)));
    document
        .set_active_configuration(&scope, &configuration(2))
        .unwrap();
    let written = document
        .design
        .evaluations
        .iter()
        .find(|evaluation| evaluation.configuration == Some(configuration(1)))
        .unwrap()
        .id
        .clone();
    assert_eq!(
        document.process.states[0].content,
        based(StateBase::Evaluation(written))
    );
    assert_eq!(
        document.process.states[1].content,
        based(StateBase::Current(after(2)))
    );
}
