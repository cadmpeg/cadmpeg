// SPDX-License-Identifier: Apache-2.0
//! Toolpaths and process states (§7.3, §7.8, §7.9): their keys and
//! configurations, process state bases, content, and the design features
//! that process states name.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_core::CodecError;

use super::records::ProcessRecords;
use crate::design::dimension::Dimension;
use crate::design::evaluation::{EvaluationStatus, StateBase};
use crate::design::feature::{FeatureOperation, OperationFamily};
use crate::design::ids::MachiningFeatureId;
use crate::document::CadIr;
use crate::features::ConfigurationId;
use crate::index::ModelIndex;
use crate::process::state::{ProcessBase, ProcessState, ProcessStateOf};
use crate::report::check::Finding;
use crate::validate::design::evaluations::{
    check_position, check_resulting_position, state_chains,
};
use crate::validate::design::records::{DesignRecords, MemberKey, PositionKey};
use crate::validate::design::resolved::visit::{visit_states, StateView};

pub(super) fn check_evaluations(
    records: &ProcessRecords<'_>,
    max_chain_depth: u64,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    check_configuration_states(records, findings);
    check_toolpaths(records, findings);
    check_states(records, findings);
    check_chains(records, max_chain_depth, findings)?;
    check_material_removal(records, findings);
    Ok(())
}

/// Whether `configuration` names a configuration of `scope`. A configuration
/// of the v10 model arena, which states no scope, is taken as one.
fn configuration_of(
    records: &ProcessRecords<'_>,
    configuration: &str,
    scope: Option<&str>,
) -> bool {
    match records.design.configurations.get(configuration) {
        Some(listed) => scope.is_none_or(|scope| listed.scope.as_str() == scope),
        None => records
            .ir
            .model
            .configurations
            .iter()
            .any(|listed| listed.id.as_str() == configuration),
    }
}

/// `Operation.configuration_states` keys name configurations of the setup's
/// scope, and there is none for an operation whose `suppressed` binds a
/// parameter (§7.3).
fn check_configuration_states(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    for operation in &records.ir.process.operations {
        if operation.configuration_states.is_empty() {
            continue;
        }
        let id = operation.id.as_str();
        if matches!(
            operation.suppressed,
            Some(Dimension::Parameter(_) | Dimension::Negated(_))
        ) {
            super::error(
                findings,
                id,
                crate::rule!("process.operation-suppressed-binds-parameter-states-no", 
                    "operation's `suppressed` binds a parameter, so it states no configuration state"
                ),
            );
        }
        let scope = records
            .setup_of(id)
            .and_then(|setup| records.scope_of_setup(setup));
        for configuration in operation.configuration_states.keys() {
            if !configuration_of(records, configuration.as_str(), scope) {
                super::error(
                    findings,
                    id,
                    crate::rule!("process.operation-configuration-state-names-no-configuration", 
                        "operation's configuration state names {}, which is no configuration of its setup's scope",
                        configuration.as_str()
                    ),
                );
            }
        }
    }
}

/// At most one toolpath per (`operation`, `feature`, `configuration`); for
/// one operation, toolpaths keyed `None` and keyed `Some` do not both occur;
/// a configuration names one of the setup's scope (§7.8).
fn check_toolpaths(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    let mut keys = BTreeMap::new();
    let mut keyed: BTreeMap<&str, (bool, bool)> = BTreeMap::new();
    for toolpath in &records.ir.process.toolpaths {
        let id = toolpath.id.as_str();
        let operation = toolpath.operation.as_str();
        let feature = toolpath.feature.as_ref().map(MachiningFeatureId::as_str);
        let configuration = toolpath.configuration.as_ref().map(ConfigurationId::as_str);
        if let Some(first) = keys.insert((operation, feature, configuration), id) {
            super::error(
                findings,
                id,
                crate::rule!("process.toolpath-repeats-operation-feature-configuration-toolpath", 
                    "toolpath repeats the operation, feature, and configuration of toolpath {first}"
                ),
            );
        }
        let seen = keyed.entry(operation).or_default();
        match configuration {
            None => seen.0 = true,
            Some(configuration) => {
                seen.1 = true;
                let scope = records
                    .setup_of(operation)
                    .and_then(|setup| records.scope_of_setup(setup));
                if !configuration_of(records, configuration, scope) {
                    super::error(
                        findings,
                        id,
                        crate::rule!("process.toolpath-names-no-configuration-setup-scope", 
                            "toolpath names {configuration}, which is no configuration of its setup's scope"
                        ),
                    );
                }
            }
        }
    }
    for (operation, (unkeyed, keyed)) in keyed {
        if unkeyed && keyed {
            super::error(
                findings,
                operation,
                crate::rule!(
                    "process.operation-toolpaths-keyed-no-configuration-toolpaths",
                    "operation has toolpaths keyed by no configuration and toolpaths keyed by one"
                ),
            );
        }
    }
}

/// The setup a process state's `of` resolves from, where it names an
/// existing setup or an operation in one.
fn setup_of_state<'a>(records: &ProcessRecords<'a>, state: &ProcessState) -> Option<&'a str> {
    match &state.of {
        ProcessStateOf::Stock(setup) => records
            .setups
            .get_key_value(setup.as_str())
            .map(|(id, _)| *id),
        ProcessStateOf::After(operation) | ProcessStateOf::Removal(operation) => {
            records.setup_of(operation.as_str())
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum OfKey<'a> {
    Stock(&'a str),
    After(&'a str),
    Removal(&'a str),
}

impl<'a> OfKey<'a> {
    fn of(of: &'a ProcessStateOf) -> Self {
        match of {
            ProcessStateOf::Stock(setup) => Self::Stock(setup.as_str()),
            ProcessStateOf::After(operation) => Self::After(operation.as_str()),
            ProcessStateOf::Removal(operation) => Self::Removal(operation.as_str()),
        }
    }
}

/// The per-state rules of §7.9: the state's `of` resolves; at most one state
/// per (`of`, configuration), and keyed `None` and `Some` do not both occur
/// for one `of`; a configuration names one of the setup's scope; a base
/// names a state with content; a state with no content is `Stale` or
/// `Failed` or carries a `feature`; `feature` occurs only on a `Removal`
/// state and names a feature of the setup's scope.
fn check_states(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    let ir = records.ir;
    let mut keys = BTreeMap::new();
    let mut keyed: BTreeMap<OfKey<'_>, (bool, bool, &str)> = BTreeMap::new();
    for state in &ir.process.states {
        let id = state.id.as_str();
        let of = OfKey::of(&state.of);
        let (exists, target, what) = match of {
            OfKey::Stock(setup) => (records.setups.contains_key(setup), setup, "setup"),
            OfKey::After(operation) | OfKey::Removal(operation) => (
                records.operations.contains_key(operation),
                operation,
                "operation",
            ),
        };
        if !exists {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.process-state-no",
                    "process state is of {target}, which is no {what}"
                ),
            );
        }
        let setup = setup_of_state(records, state);
        let scope = setup.and_then(|setup| records.scope_of_setup(setup));
        let configuration = state.configuration.as_ref().map(ConfigurationId::as_str);
        if let Some(first) = keys.insert((of, configuration), id) {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.process-state-repeats-configuration-process-state",
                    "process state repeats the `of` and configuration of process state {first}"
                ),
            );
        }
        let seen = keyed.entry(of).or_insert((false, false, id));
        match configuration {
            None => seen.0 = true,
            Some(configuration) => {
                seen.1 = true;
                if !configuration_of(records, configuration, scope) {
                    super::error(
                        findings,
                        id,
                        crate::rule!("process.process-state-names-no-configuration-setup", 
                            "process state names {configuration}, which is no configuration of its setup's scope"
                        ),
                    );
                }
            }
        }
        if let Some(feature) = &state.feature {
            check_state_feature(records, state, feature.as_str(), scope, findings);
        }
        match &state.content {
            None => {
                if state.status == EvaluationStatus::Current && state.feature.is_none() {
                    super::error(
                        findings,
                        id,
                        crate::rule!("process.process-state-no-content-no-feature", 
                            "process state with no content and no feature has status `current`; it is `stale` or `failed`"
                        ),
                    );
                }
            }
            Some(content) => check_base(records, ir, id, &content.base, scope, findings),
        }
    }
    for (_, (unkeyed, keyed, id)) in keyed {
        if unkeyed && keyed {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.process-states-one-keyed-no-configuration",
                    "process states of one `of` are keyed by no configuration and by one"
                ),
            );
        }
    }
}

fn check_state_feature(
    records: &ProcessRecords<'_>,
    state: &ProcessState,
    feature: &str,
    scope: Option<&str>,
    findings: &mut Vec<Finding>,
) {
    let id = state.id.as_str();
    if !matches!(state.of, ProcessStateOf::Removal(_)) {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.process-state-names-feature-only-removal",
                "process state names feature {feature}, but only a `removal` state names one"
            ),
        );
    }
    let in_scope = if records.design.features.contains_key(feature) {
        scope
            .is_none_or(|scope| records.design.scope_of(MemberKey::Feature(feature)) == Some(scope))
    } else {
        records
            .ir
            .model
            .features
            .iter()
            .any(|listed| listed.id.as_str() == feature)
    };
    if !in_scope {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.process-state-names-no-feature-setup",
                "process state names {feature}, which is no feature of its setup's scope"
            ),
        );
    }
    if let Some(record) = records.design.features.get(feature) {
        let configuration = state.configuration.as_ref().map(ConfigurationId::as_str);
        let removal = matches!(
            records
                .design
                .definition_in(record, configuration, None)
                .operation,
            FeatureOperation::MaterialRemoval
                | FeatureOperation::Unresolved {
                    family: OperationFamily::MaterialRemoval,
                }
        );
        if !removal {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.process-state-names-no-materialremoval-feature",
                    "process state names {feature}, which is no `MaterialRemoval` feature"
                ),
            );
        }
    }
}

fn check_base(
    records: &ProcessRecords<'_>,
    ir: &CadIr,
    id: &str,
    base: &ProcessBase,
    scope: Option<&str>,
    findings: &mut Vec<Finding>,
) {
    match base {
        ProcessBase::State(base) => match records.states.get(base.as_str()) {
            None if ir.fragment => super::error(
                findings,
                id,
                crate::rule!(
                    "process.fragment-does-not-hold-process-state",
                    "the fragment does not hold the process state's base {}",
                    base.as_str()
                ),
            ),
            None => super::error(
                findings,
                id,
                crate::rule!(
                    "process.state-base-no-process-state",
                    "state base {} is no process state",
                    base.as_str()
                ),
            ),
            Some(base) if base.content.is_none() => super::error(
                findings,
                id,
                crate::rule!(
                    "process.state-base-process-state-no-content",
                    "state base {} is a process state with no content",
                    base.id.as_str()
                ),
            ),
            Some(_) => {}
        },
        ProcessBase::Design(StateBase::Empty) => {}
        ProcessBase::Design(StateBase::Evaluation(base)) => {
            match records.design.evaluations.get(base.as_str()) {
                None if ir.fragment => super::error(
                    findings,
                    id,
                    crate::rule!(
                        "process.fragment-does-not-hold-process-state",
                        "the fragment does not hold the process state's base {}",
                        base.as_str()
                    ),
                ),
                None => super::error(
                    findings,
                    id,
                    crate::rule!(
                        "process.state-base-no-evaluation-design",
                        "state base {} is no evaluation of the design",
                        base.as_str()
                    ),
                ),
                Some(base) if base.content.is_none() => super::error(
                    findings,
                    id,
                    crate::rule!(
                        "process.state-base-evaluation-no-content",
                        "state base {} is an evaluation with no content",
                        base.id.as_str()
                    ),
                ),
                Some(_) => {}
            }
        }
        ProcessBase::Design(StateBase::Current(position)) => {
            let position = PositionKey::of(position);
            let exists = super::as_process(findings, |findings| {
                check_position(&records.design, position, id, findings)
                    && check_resulting_position(&records.design, position, id, findings)
            });
            if !exists {
                return;
            }
            if matches!(position, PositionKey::After(MemberKey::Container(_)))
                && !records.design.at_unstated_rollback(position)
            {
                super::error(
                    findings,
                    id,
                    crate::rule!("process.state-base-names-stated-rollback-position", 
                        "state base names {position}; a stated rollback position is spelled through its member"
                    ),
                );
            }
            let Some(scope) = scope else { return };
            let current = records.design.scopes.get(scope).is_some_and(|scope| {
                scope
                    .current
                    .iter()
                    .any(|current| PositionKey::of(&current.at) == position)
            });
            if !current {
                super::error(
                    findings,
                    id,
                    crate::rule!("process.state-base-names-position-no-current", 
                        "state base names {position}, which is the position of no current state of the setup's scope"
                    ),
                );
            }
        }
    }
}

/// Process bases form a forest (§7.9). A chain of bases, counted through the
/// evaluations it rests on, longer than the caller's ceiling refuses the
/// session (§2.5).
fn check_chains(
    records: &ProcessRecords<'_>,
    max_chain_depth: u64,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    #[derive(Clone, Copy)]
    enum Mark {
        OnChain,
        Depth(u64),
    }
    let design = state_chains(&records.design)
        .map(|chains| chains.depths)
        .unwrap_or_default();
    let below = |state: &ProcessState| -> Result<&str, u64> {
        let Some(content) = &state.content else {
            return Err(0);
        };
        match &content.base {
            ProcessBase::State(base) => match records.states.get_key_value(base.as_str()) {
                Some((key, _)) => Ok(*key),
                None => Err(1),
            },
            ProcessBase::Design(StateBase::Evaluation(base)) => {
                Err(1 + design.get(base.as_str()).copied().unwrap_or(0))
            }
            ProcessBase::Design(StateBase::Empty | StateBase::Current(_)) => Err(1),
        }
    };
    let mut marks: BTreeMap<&str, Mark> = BTreeMap::new();
    let mut cyclic: BTreeSet<&str> = BTreeSet::new();
    for &start in records.states.keys() {
        let mut chain: Vec<&str> = Vec::new();
        let mut at = start;
        let tail = loop {
            match marks.get(at) {
                Some(Mark::Depth(depth)) => break Some(*depth),
                Some(Mark::OnChain) => {
                    let from = chain
                        .iter()
                        .position(|&listed| listed == at)
                        .unwrap_or_default();
                    cyclic.extend(chain[from..].iter().copied());
                    break None;
                }
                None => {}
            }
            match below(records.states[at]) {
                Err(depth) => {
                    marks.insert(at, Mark::Depth(depth));
                    break Some(depth);
                }
                Ok(base) => {
                    marks.insert(at, Mark::OnChain);
                    chain.push(at);
                    at = base;
                }
            }
        };
        let mut depth = tail.unwrap_or_default();
        for id in chain.into_iter().rev() {
            depth = match tail.map(|_| depth.checked_add(1)) {
                Some(Some(next)) => next,
                Some(None) => {
                    super::error(
                        findings,
                        id,
                        crate::rule!(
                            "process.depth-process-state-chain-state-bases",
                            "the depth of the process state's chain of state bases overflows"
                        ),
                    );
                    0
                }
                None => 0,
            };
            marks.insert(id, Mark::Depth(depth));
        }
    }
    let mut deepest = 0;
    for (id, mark) in marks {
        if cyclic.contains(id) {
            super::error(
                findings,
                id,
                crate::rule!("process.process-state-own-ancestor-through-state", "process state is its own ancestor through its state bases; bases form a forest"),
            );
        } else if let Mark::Depth(depth) = mark {
            deepest = deepest.max(depth);
        }
    }
    match crate::validate::state_chain_refusal(max_chain_depth, deepest) {
        Some(refusal) => Err(CodecError::ResourceLimit(refusal)),
        None => Ok(()),
    }
}

/// The `Removal` process states whose `feature` names one `MaterialRemoval`
/// feature are states of one operation (§7.9, D49).
fn check_material_removal(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    let mut named: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for state in &records.ir.process.states {
        if let (ProcessStateOf::Removal(operation), Some(feature)) = (&state.of, &state.feature) {
            named
                .entry(feature.as_str())
                .or_default()
                .insert(operation.as_str());
        }
    }
    for (feature, operations) in named {
        let removal = records.design.features.get(feature).is_some_and(|listed| {
            matches!(
                listed.definition.operation,
                FeatureOperation::MaterialRemoval
            )
        });
        if removal && operations.len() > 1 {
            super::error(
                findings,
                feature,
                crate::rule!(
                    "process.material-removal-feature-named-removal-states",
                    "material-removal feature is named by removal states of {} operations: {}",
                    operations.len(),
                    operations.into_iter().collect::<Vec<_>>().join(", ")
                ),
            );
        }
    }
}

/// Each stored process state, resolved through its chain of bases, is
/// closed, and its entries name what it holds (§5.4, §7.9).
pub(super) fn check_resolved_states(
    ctx: &DecodeContext<'_>,
    records: &ProcessRecords<'_>,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    crate::validate::design::resolved::check_process_states(
        ctx,
        &records.design,
        ids,
        |state| setup_of_state(records, state).and_then(|setup| records.scope_of_setup(setup)),
        findings,
    )
}

/// Visits the resolved state of each evaluation and each process state of
/// `ir` with content, as the checks of resolved states resolve it
/// ([`visit_states`]).
pub(crate) fn visit_resolved_states(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    visit: &mut dyn FnMut(StateView<'_, '_>) -> Result<(), ResourceLimit>,
) -> Result<(), ResourceLimit> {
    let design = std::rc::Rc::new(DesignRecords::new(ir));
    let records = ProcessRecords::new(design.clone());
    visit_states(
        ctx,
        &design,
        &ModelIndex::build(ir),
        |state| setup_of_state(&records, state).and_then(|setup| records.scope_of_setup(setup)),
        visit,
    )
}

#[cfg(test)]
mod tests;
