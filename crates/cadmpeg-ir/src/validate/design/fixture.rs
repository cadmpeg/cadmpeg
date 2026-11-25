// SPDX-License-Identifier: Apache-2.0
//! A design document that breaks no part 10 rule, and helpers that build
//! and check variations of it.

use cadmpeg_core::decode::{ContentScope, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
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
use crate::index::ModelIndex;
use crate::products::{ProductDefinition, ProductDefinitionKind, Storage};
use crate::report::check::{Check, Finding};
use crate::report::Severity;

pub(crate) fn id<T>(kind: &str, key: &str) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:design:{kind}#{key}")).unwrap()
}

pub(in crate::validate) fn feature(key: &str) -> FeatureId {
    id("feature", key)
}

pub(in crate::validate) fn after(key: &str) -> Position {
    Position::After(Member::Feature(feature(key)))
}

pub(in crate::validate) fn unresolved(key: &str, family: OperationFamily) -> Feature {
    Feature {
        id: feature(key),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation: FeatureOperation::Unresolved { family },
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

pub(in crate::validate) fn configuration(key: &str, parent: Option<&str>) -> DesignConfiguration {
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

pub(in crate::validate) fn evaluation(
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

pub(in crate::validate) fn stored(base: StateBase) -> StateContent {
    StateContent {
        base,
        delta: StateDelta::default(),
    }
}

pub(in crate::validate) fn set_base(ir: &mut CadIr, evaluation: usize, base: StateBase) {
    ir.design.evaluations[evaluation].content = Some(stored(base));
}

pub(in crate::validate) fn body(ir: &CadIr) -> BodyId {
    ir.geometry.bodies[0].id.clone()
}

pub(in crate::validate) fn arrangement(key: &str, kind: ArrangementKind) -> Arrangement {
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

pub(crate) fn parameter(key: &str, kind: ValueKind) -> DesignParameter {
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
pub(crate) fn document() -> CadIr {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.model.product_definitions.clear();
    ir.set_scope(ContentScope::full()).unwrap();
    let body = body(&ir);
    ir.model.product_definitions.push(ProductDefinition {
        kind: Some(ProductDefinitionKind::Part),
        bodies: vec![body.clone()],
        ..ProductDefinition::new(id("definition", "part"), Storage::Embedded)
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
        unproduced: BTreeSet::default(),
    });
    design.containers.push(Container::Ordered {
        id: id("container", "timeline"),
        kind: OrderedKind::Timeline,
        members: vec![
            Member::Feature(feature("extrude")),
            Member::Feature(feature("fillet")),
        ],
        folders: Vec::new(),
        rollback: Some(Rollback::End),
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
    crate::examples::add_document_root(&mut ir);
    ir
}

/// The findings of one family of checks, run as `validate_neutral` runs it.
pub(in crate::validate) fn family_findings(
    ir: &CadIr,
    family: Check,
    check: impl FnOnce(
        &DecodeContext<'_>,
        &CadIr,
        &ModelIndex<'_>,
        &mut Vec<Finding>,
    ) -> Result<(), CodecError>,
) -> Vec<Finding> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    let index = ModelIndex::build(ir);
    let mut findings = Vec::new();
    {
        let _depth = ctx.enter_nested("IR neutral validation").unwrap();
        check(&ctx, ir, &index, &mut findings).unwrap();
    }
    ctx.finish_session().unwrap();
    findings.retain(|finding| finding.check == family);
    findings
}

pub(in crate::validate) fn design_findings(ir: &CadIr) -> Vec<Finding> {
    family_findings(ir, Check::Design, super::check_design)
}

#[track_caller]
pub(in crate::validate) fn assert_no_design_finding(ir: &CadIr) {
    let findings = design_findings(ir);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[track_caller]
pub(in crate::validate) fn assert_finding(ir: &CadIr, fragment: &str) {
    let findings = design_findings(ir);
    assert!(
        findings.iter().any(
            |finding| finding.severity == Severity::Error && finding.message.contains(fragment)
        ),
        "no design error containing {fragment:?} in {findings:#?}"
    );
}

/// Runs `work` on a 1 MiB stack, the smallest default stack of a supported
/// target (a Windows main thread). The design walk and the canonical writer
/// recurse once per JSON level, and a Windows debug frame is larger than a
/// Linux one.
pub(in crate::validate) fn on_the_smallest_default_stack<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> T {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(work)
        .unwrap()
        .join()
        .unwrap()
}
