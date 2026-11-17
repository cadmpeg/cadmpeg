// SPDX-License-Identifier: Apache-2.0
//! A design document that breaks no part 10 rule, and helpers that build
//! and check variations of it.

use cadmpeg_core::decode::ContentScope;
use cadmpeg_core::text::NonBlankString;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use crate::design::configuration::{Arrangement, ArrangementKind, DesignConfiguration, Overrides};
use crate::design::container::{Container, OrderedKind, Rollback};
use crate::design::evaluation::{
    CurrentState, Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use crate::design::parameter::{DesignParameter, ParameterDefinition};
use crate::design::refs::{Evaluated, Member, Position};
use crate::design::scope::DesignScope;
use crate::design::units::ValueKind;
use crate::document::CadIr;
use crate::features::FeatureId;
use crate::ids::BodyId;
use crate::products::{ProductDefinition, ProductDefinitionKind};
use crate::report::check::{Check, Finding};
use crate::report::Severity;

pub(super) fn id<T>(kind: &str, key: &str) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:design:{kind}#{key}")).unwrap()
}

pub(super) fn feature(key: &str) -> FeatureId {
    id("feature", key)
}

pub(super) fn after(key: &str) -> Position {
    Position::After(Member::Feature(feature(key)))
}

pub(super) fn unresolved(key: &str, family: OperationFamily) -> Feature {
    Feature {
        id: feature(key),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation: FeatureOperation::Unresolved { family },
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

pub(super) fn configuration(key: &str, parent: Option<&str>) -> DesignConfiguration {
    DesignConfiguration {
        id: id("configuration", key),
        scope: id("definition", "part"),
        name: key.to_owned(),
        parent: parent.map(|parent| id("configuration", parent)),
        comment: None,
        overrides: Overrides::default(),
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    }
}

pub(super) fn evaluation(
    key: &str,
    configuration: &str,
    of: Evaluated,
    content: Option<StateContent>,
) -> Evaluation {
    Evaluation {
        id: id("evaluation", key),
        scope: id("definition", "part"),
        configuration: Some(id("configuration", configuration)),
        of,
        status: if content.is_some() {
            EvaluationStatus::Current
        } else {
            EvaluationStatus::Stale
        },
        content,
        native_ref: None,
    }
}

pub(super) fn stored(base: StateBase) -> StateContent {
    StateContent {
        base,
        delta: StateDelta::default(),
    }
}

pub(super) fn set_base(ir: &mut CadIr, evaluation: usize, base: StateBase) {
    ir.design.evaluations[evaluation].content = Some(stored(base));
}

pub(super) fn body(ir: &CadIr) -> BodyId {
    ir.geometry.bodies[0].id.clone()
}

pub(super) fn arrangement(key: &str, kind: ArrangementKind) -> Arrangement {
    Arrangement {
        id: id("arrangement", key),
        scope: id("definition", "part"),
        name: NonBlankString::new(key).unwrap(),
        kind,
        configuration: None,
        overrides: Overrides::default(),
        appearances: Vec::new(),
        moves: Vec::new(),
        nested: Vec::new(),
        native_ref: None,
    }
}

pub(super) fn parameter(key: &str, kind: ValueKind) -> DesignParameter {
    DesignParameter {
        id: id("parameter", key),
        scope: id("definition", "part"),
        owner: None,
        name: Some(NonBlankString::new(key).unwrap()),
        kind,
        unit: None,
        definition: ParameterDefinition::Driven,
        value: None,
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    }
}

/// A unit cube whose part has one scope: a timeline of an extrude and a
/// fillet, rolled to its end; two configurations; the current state after the
/// fillet holding the cube; and an evaluation of the base configuration after
/// the extrude, stored against the current state.
pub(super) fn document() -> CadIr {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.set_scope(ContentScope::full()).unwrap();
    let body = body(&ir);
    ir.model.product_definitions.push(ProductDefinition {
        id: id("definition", "part"),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: BTreeMap::new(),
        bodies: vec![body.clone()],
        sketches: Vec::new(),
        native_ref: None,
    });
    let design = &mut ir.design;
    design.scopes.push(DesignScope {
        definition: id("definition", "part"),
        roots: vec![id("container", "timeline")],
        active_configuration: Some(id("configuration", "base")),
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: vec![CurrentState {
            at: after("fillet"),
            bodies: vec![body],
            poses: BTreeMap::new(),
            status: EvaluationStatus::Current,
            native_ref: None,
        }],
    });
    design.containers.push(Container::Ordered {
        id: id("container", "timeline"),
        kind: OrderedKind::Timeline,
        members: vec![
            Member::Feature(feature("extrude")),
            Member::Feature(feature("fillet")),
        ],
        folders: Vec::new(),
        rollback: Rollback::End,
        placement: None,
    });
    design
        .features
        .push(unresolved("extrude", OperationFamily::Extrude));
    design
        .features
        .push(unresolved("fillet", OperationFamily::Fillet));
    design.configurations.push(configuration("base", None));
    design
        .configurations
        .push(configuration("large", Some("base")));
    design.evaluations.push(evaluation(
        "after-extrude",
        "base",
        Evaluated::State(after("extrude")),
        Some(stored(StateBase::Current(after("fillet")))),
    ));
    design.evaluations.push(evaluation(
        "large-after-fillet",
        "large",
        Evaluated::State(after("fillet")),
        None,
    ));
    ir
}

pub(super) fn design_findings(ir: &CadIr) -> Vec<Finding> {
    super::super::validate_neutral(ir, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Design)
        .collect()
}

#[track_caller]
pub(super) fn assert_no_design_finding(ir: &CadIr) {
    let findings = design_findings(ir);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[track_caller]
pub(super) fn assert_finding(ir: &CadIr, fragment: &str) {
    let findings = design_findings(ir);
    assert!(
        findings.iter().any(
            |finding| finding.severity == Severity::Error && finding.message.contains(fragment)
        ),
        "no design error containing {fragment:?} in {findings:#?}"
    );
}
