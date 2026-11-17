// SPDX-License-Identifier: Apache-2.0
//! Positions, evaluations, and current geometry (§5.3, §5.4, §5.5, §5.10).

use std::collections::{BTreeMap, BTreeSet};

use super::index::{DesignIndex, MemberKey, OfKey, PositionKey};
use crate::design::container::Container;
use crate::design::dimension::Dimension;
use crate::design::evaluation::{EvaluationStatus, StateBase};
use crate::design::feature::{Feature, FeatureOperation, OperationFamily};
use crate::design::ops::body::PatternInstances;
use crate::design::ops::surface::MeshKind;
use crate::design::parameter::ParameterValue;
use crate::design::refs::Position;
use crate::design::selection::{OccurrencePath, StepRef};
use crate::features::{ConfigurationId, ParameterId};
use crate::report::check::Finding;

/// A position names an existing member of a container, or the start of an
/// existing ordered container (§5.3). Returns whether it does.
pub(super) fn check_position(
    index: &DesignIndex<'_>,
    position: PositionKey<'_>,
    entity: &str,
    findings: &mut Vec<Finding>,
) -> bool {
    match position {
        PositionKey::Start(id) => {
            let ordered = matches!(index.containers.get(id), Some(Container::Ordered { .. }));
            if !ordered {
                super::error(
                    findings,
                    entity,
                    format_args!("{position} names no ordered container of the design"),
                );
            }
            ordered
        }
        PositionKey::After(member) => {
            let listed = index.member_exists(member) && index.parents.contains_key(&member);
            if !listed {
                super::error(
                    findings,
                    entity,
                    format_args!("{position} names no member of a container"),
                );
            }
            listed
        }
    }
}

/// Whether `position` is `After(Container(c))` for an ordered container `c`.
fn after_ordered_container(index: &DesignIndex<'_>, position: PositionKey<'_>) -> bool {
    matches!(
        position,
        PositionKey::After(MemberKey::Container(id))
            if matches!(index.containers.get(id), Some(Container::Ordered { .. }))
    )
}

/// The positions of a scope's current states.
fn current_positions<'a>(index: &DesignIndex<'a>, scope: &str) -> BTreeSet<PositionKey<'a>> {
    index
        .scopes
        .get(scope)
        .map(|scope| {
            scope
                .current
                .iter()
                .map(|current| PositionKey::of(&current.at))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn check_evaluations(index: &DesignIndex<'_>, findings: &mut Vec<Finding>) {
    let cyclic = cyclic_evaluations(index);
    let mut keys = BTreeMap::new();
    for evaluation in &index.ir.design.evaluations {
        let id = evaluation.id.as_str();
        let scope = evaluation.scope.as_str();
        let design_scope = index.scopes.get(scope).copied();
        if design_scope.is_none() {
            super::error(
                findings,
                id,
                format_args!("evaluation's scope {scope} has no design scope"),
            );
        }
        let configuration = evaluation
            .configuration
            .as_ref()
            .map(ConfigurationId::as_str);
        match (configuration, index.has_configurations(scope)) {
            (None, true) => super::error(
                findings,
                id,
                format_args!(
                    "evaluation states no configuration, but its scope has configurations"
                ),
            ),
            (Some(configuration), false) => super::error(
                findings,
                id,
                format_args!(
                    "evaluation names configuration {configuration}, but its scope has none"
                ),
            ),
            (Some(configuration), true) => {
                let of_scope = index
                    .configurations
                    .get(configuration)
                    .is_some_and(|c| c.scope.as_str() == scope);
                if !of_scope {
                    super::error(
                        findings,
                        id,
                        format_args!("evaluation names {configuration}, which is no configuration of its scope"),
                    );
                }
            }
            (None, false) => {}
        }
        let of = OfKey::of(&evaluation.of);
        let feature = match of {
            OfKey::State(position) => {
                check_position(index, position, id, findings);
                if after_ordered_container(index, position) {
                    super::error(
                        findings,
                        id,
                        format_args!("evaluation evaluates {position}, which is the state at that ordered container's rollback position"),
                    );
                }
                let active = design_scope.is_some_and(|s| {
                    s.active_configuration.as_ref().map(ConfigurationId::as_str) == configuration
                });
                if active && current_positions(index, scope).contains(&position) {
                    super::error(
                        findings,
                        id,
                        format_args!("evaluation of the active configuration repeats the current state at {position}"),
                    );
                }
                match position {
                    PositionKey::After(MemberKey::Feature(feature)) => Some(feature),
                    PositionKey::After(MemberKey::Container(_)) | PositionKey::Start(_) => None,
                }
            }
            OfKey::Tool(feature) => {
                if !index.features.contains_key(feature) {
                    super::error(
                        findings,
                        id,
                        format_args!("evaluation evaluates the tool body of {feature}, which is no feature of the design"),
                    );
                }
                Some(feature)
            }
        };
        if let Some(feature) = feature.and_then(|feature| index.features.get(feature)) {
            if is_stateless(index, feature, configuration) {
                super::error(
                    findings,
                    id,
                    format_args!(
                        "feature {} is stateless in the evaluation's configuration and has no evaluation there",
                        feature.id.as_str()
                    ),
                );
            }
        }
        let key = (scope, configuration, of);
        if let Some(first) = keys.insert(key, id) {
            super::error(
                findings,
                id,
                format_args!(
                    "evaluation repeats the scope, configuration, and `of` of evaluation {first}"
                ),
            );
        }
        let Some(content) = &evaluation.content else {
            if evaluation.status == EvaluationStatus::Current {
                super::error(
                    findings,
                    id,
                    format_args!("evaluation with no content has status `current`; it is `stale` or `failed`"),
                );
            }
            continue;
        };
        match &content.base {
            StateBase::Empty => {}
            StateBase::Current(position) => {
                let position = PositionKey::of(position);
                if check_position(index, position, id, findings) {
                    if matches!(position, PositionKey::After(MemberKey::Container(_))) {
                        super::error(
                            findings,
                            id,
                            format_args!("state base names {position}; a rollback position is spelled through its member"),
                        );
                    }
                    if !current_positions(index, scope).contains(&position) {
                        super::error(
                            findings,
                            id,
                            format_args!("state base names {position}, which is the position of no current state of the evaluation's scope"),
                        );
                    }
                }
            }
            StateBase::Evaluation(base) => match index.evaluations.get(base.as_str()) {
                None if index.ir.fragment => super::error(
                    findings,
                    id,
                    format_args!(
                        "the fragment does not hold the evaluation's base {}",
                        base.as_str()
                    ),
                ),
                None => super::error(
                    findings,
                    id,
                    format_args!(
                        "state base {} is no evaluation of the design",
                        base.as_str()
                    ),
                ),
                Some(base) if base.content.is_none() => super::error(
                    findings,
                    id,
                    format_args!(
                        "state base {} is an evaluation with no content",
                        base.id.as_str()
                    ),
                ),
                Some(_) => {}
            },
        }
        if cyclic.contains(id) {
            super::error(
                findings,
                id,
                format_args!(
                    "evaluation is its own ancestor through its state bases; bases form a forest"
                ),
            );
        }
    }
}

/// The evaluations on a cycle of state bases.
fn cyclic_evaluations<'a>(index: &DesignIndex<'a>) -> BTreeSet<&'a str> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mark {
        OnChain,
        Done,
    }
    let base_of = |id: &str| -> Option<&'a str> {
        match &index.evaluations.get(id)?.content.as_ref()?.base {
            StateBase::Evaluation(base) => index
                .evaluations
                .get_key_value(base.as_str())
                .map(|(key, _)| *key),
            StateBase::Empty | StateBase::Current(_) => None,
        }
    };
    let mut marks: BTreeMap<&str, Mark> = BTreeMap::new();
    let mut cyclic = BTreeSet::new();
    for &start in index.evaluations.keys() {
        let mut chain = Vec::new();
        let mut next = Some(start);
        while let Some(id) = next {
            match marks.get(id) {
                Some(Mark::Done) => break,
                Some(Mark::OnChain) => {
                    let from = chain
                        .iter()
                        .position(|listed| *listed == id)
                        .unwrap_or_default();
                    cyclic.extend(chain[from..].iter().copied());
                    break;
                }
                None => {}
            }
            marks.insert(id, Mark::OnChain);
            chain.push(id);
            next = base_of(id);
        }
        for id in chain {
            marks.insert(id, Mark::Done);
        }
    }
    cyclic
}

/// `CadIr::is_stateless(f, c)` (§5.4, D34, D35), decided where the document
/// states what decides it: the feature's `suppressed` resolves to true in
/// `configuration`, or the operation of the definition that applies there
/// cannot change a state. A suppression bound to a parameter resolves through
/// the parameter's cached value.
pub(super) fn is_stateless(
    index: &DesignIndex<'_>,
    feature: &Feature,
    configuration: Option<&str>,
) -> bool {
    let chain = index.configuration_chain(configuration);
    let overrides = chain
        .iter()
        .copied()
        .filter_map(|configuration| configuration.overrides.features.get(&feature.id));
    let suppressed = overrides
        .clone()
        .find_map(|state| state.suppressed)
        .or_else(|| match feature.suppressed.as_ref()? {
            Dimension::Literal(value) => Some(*value),
            Dimension::Parameter(parameter) => parameter_bool(index, &chain, parameter),
            Dimension::Negated(parameter) => {
                parameter_bool(index, &chain, parameter).map(|value| !value)
            }
        });
    if suppressed == Some(true) {
        return true;
    }
    let definition = { overrides }
        .find_map(|state| state.definition.as_ref())
        .unwrap_or(&feature.definition);
    stateless_operation(&definition.operation)
}

fn parameter_bool(
    index: &DesignIndex<'_>,
    chain: &[&crate::design::configuration::DesignConfiguration],
    parameter: &ParameterId,
) -> Option<bool> {
    let value = chain
        .iter()
        .find_map(|configuration| {
            configuration
                .overrides
                .parameters
                .get(parameter)?
                .value
                .as_ref()
        })
        .or_else(|| index.parameters.get(parameter.as_str())?.value.as_ref())?;
    match value {
        ParameterValue::Boolean(value) => Some(*value),
        ParameterValue::Number(_) | ParameterValue::Integer(_) | ParameterValue::Text(_) => None,
    }
}

/// An operation whose kind cannot change a state: a sketch, a sheet, a datum,
/// a pattern whose instances are features of their own, a template instance,
/// a decal, and a mesh section.
fn stateless_operation(operation: &FeatureOperation) -> bool {
    match operation {
        FeatureOperation::Sketch(_)
        | FeatureOperation::SpatialSketch(_)
        | FeatureOperation::Spreadsheet(_)
        | FeatureOperation::Datum(_)
        | FeatureOperation::TemplateInstance(_)
        | FeatureOperation::Decal(_) => true,
        FeatureOperation::Pattern(pattern) => {
            matches!(pattern.instances, PatternInstances::Members(_))
        }
        FeatureOperation::Mesh(mesh) => matches!(mesh.kind, MeshKind::Section { .. }),
        FeatureOperation::Unresolved { family } => matches!(
            family,
            OperationFamily::Sketch
                | OperationFamily::SpatialSketch
                | OperationFamily::Spreadsheet
                | OperationFamily::Datum
                | OperationFamily::TemplateInstance
                | OperationFamily::Decal
        ),
        _ => false,
    }
}

/// Every `Occurrence` step of the path names an occurrence of the model.
pub(super) fn check_path_steps(
    index: &DesignIndex<'_>,
    path: &OccurrencePath,
    entity: &str,
    what: &str,
    findings: &mut Vec<Finding>,
) {
    for step in &path.0 {
        if let StepRef::Occurrence(occurrence) = &step.occurrence {
            if !index.occurrences.contains_key(occurrence.as_str()) {
                super::error(
                    findings,
                    entity,
                    format_args!(
                        "{what} path names {}, which is no occurrence of the model",
                        occurrence.as_str()
                    ),
                );
            }
        }
    }
}

pub(super) fn check_current_geometry(index: &DesignIndex<'_>, findings: &mut Vec<Finding>) {
    let ir = index.ir;
    let bodies: BTreeSet<&str> = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect();
    let mut listed: BTreeMap<&str, usize> = BTreeMap::new();
    for scope in &ir.design.scopes {
        let entity = scope.definition.as_str();
        let mut posed: BTreeMap<&OccurrencePath, usize> = BTreeMap::new();
        for current in &scope.current {
            check_current_position(index, &current.at, entity, findings);
            for body in &current.bodies {
                *listed.entry(body.as_str()).or_default() += 1;
                if !bodies.contains(body.as_str()) {
                    super::error(
                        findings,
                        entity,
                        format_args!(
                            "current state lists {}, which is no body of the current geometry",
                            body.as_str()
                        ),
                    );
                }
            }
            for path in current.poses.keys() {
                *posed.entry(path).or_default() += 1;
                check_path_steps(index, path, entity, "pose", findings);
            }
        }
        for count in posed.into_values() {
            if count > 1 {
                super::error(
                    findings,
                    entity,
                    format_args!("an instance path has a pose in {count} current states of the scope; it has one in at most one"),
                );
            }
        }
    }
    for (body, count) in &listed {
        if *count > 1 {
            super::error(
                findings,
                body,
                format_args!("body belongs to {count} current states; it belongs to exactly one"),
            );
        }
    }
    for scope in &ir.design.scopes {
        let Some(definition) = index.definitions.get(scope.definition.as_str()) else {
            continue;
        };
        for body in &definition.bodies {
            if !listed.contains_key(body.as_str()) {
                super::error(
                    findings,
                    body.as_str(),
                    format_args!(
                        "body of scoped definition {} belongs to no current state",
                        scope.definition.as_str()
                    ),
                );
            }
        }
    }
    check_stored_features(index, findings);
}

/// A current state names an existing position through its member, and one
/// inside an ordered container is at that container's rollback position.
fn check_current_position(
    index: &DesignIndex<'_>,
    at: &Position,
    entity: &str,
    findings: &mut Vec<Finding>,
) {
    let at = PositionKey::of(at);
    if !check_position(index, at, entity, findings) {
        return;
    }
    if matches!(at, PositionKey::After(MemberKey::Container(_))) {
        super::error(
            findings,
            entity,
            format_args!(
                "current state is at {at}; a rollback position is spelled through its member"
            ),
        );
        return;
    }
    let Some(container) = index.container_of(at) else {
        return;
    };
    if let Some(rollback) = DesignIndex::rollback_position(container) {
        if rollback != at {
            super::error(
                findings,
                entity,
                format_args!(
                    "current state is at {at}, not at the rollback position of container {}, {rollback}",
                    container.id().as_str()
                ),
            );
        }
    }
}

/// A stored-geometry feature's tessellations exist, and a current state after
/// it holds each body it names (§5.10).
fn check_stored_features(index: &DesignIndex<'_>, findings: &mut Vec<Finding>) {
    let ir = index.ir;
    let tessellations: BTreeSet<&str> = ir
        .model
        .tessellations
        .iter()
        .map(|tessellation| tessellation.id.as_str())
        .collect();
    for feature in &ir.design.features {
        let FeatureOperation::Stored {
            bodies,
            tessellations: named,
            ..
        } = &feature.definition.operation
        else {
            continue;
        };
        let id = feature.id.as_str();
        for tessellation in named {
            if !tessellations.contains(tessellation.as_str()) {
                super::error(
                    findings,
                    id,
                    format_args!(
                        "stored feature names {}, which is no tessellation of the model",
                        tessellation.as_str()
                    ),
                );
            }
        }
        let after = PositionKey::After(MemberKey::Feature(id));
        let Some(scope) = index
            .scope_of(MemberKey::Feature(id))
            .and_then(|scope| index.scopes.get(scope))
        else {
            continue;
        };
        for current in scope
            .current
            .iter()
            .filter(|current| PositionKey::of(&current.at) == after)
        {
            for body in bodies.iter().flatten() {
                if !current.bodies.contains(body) {
                    super::error(
                        findings,
                        id,
                        format_args!("stored feature names {}, which is no body of the current state after it", body.as_str()),
                    );
                }
            }
        }
    }
}
