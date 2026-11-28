// SPDX-License-Identifier: Apache-2.0
//! What a design edit changes outside the design layer (§4, Edit helpers):
//! the toolpaths and process states that depend on it (§7.9), the process
//! caches that read an edited design parameter through `ParamRef::Design`
//! (§7.7), the `Choice` values those caches select (§7), and the fit
//! deviations of a PMI dimension bound to an edited parameter (D58).
//!
//! An operation depends on an edit where a record it machines with (the
//! operation, its tooling, its machining features, its groups and their
//! parents, its setup, and the process parameters these own) selects
//! geometry from a state the edit marks `Stale`, or names an edited design
//! parameter or a process parameter that depends on one. Its toolpaths and
//! states become `Stale`, and so do the states of every later operation in
//! its setup's machining order and the toolpaths of every modifier that
//! takes it.

use std::collections::{BTreeMap, BTreeSet};

use super::eval::{cache_value, evaluate_tree, kind_value};
use super::history::{Context, Currents, Dependents, History};
use super::wire::{record_reads, rewrite, Reads};
use super::EditError;
use crate::design::dimension::{ParamRef, RefTarget};
use crate::design::evaluation::EvaluationStatus;
use crate::design::expr::{Expression, Neutral};
use crate::design::ids::{OperationId, ProcessParameterId, SetupId};
use crate::design::parameter::ParameterValue;
use crate::process::groups::{ProcessDefinition, ProcessOwner};
use crate::process::operation::{OperationKind, Tooling};
use crate::process::setup::Step;
use crate::process::state::ProcessStateOf;
use crate::CadIr;

#[cfg(test)]
mod tests;

/// The outcome of one attempt to recompute a process parameter's cache.
enum Attempt {
    /// A reference names a cache that is itself still to be recomputed.
    Blocked,
    /// The recomputed cache, or `None` where it cannot be recomputed.
    Done(Option<ParameterValue>),
}

/// The operations of each setup in machining order, and the setup of each
/// operation: a step of the setup, or an input of a modifier in it.
#[derive(Default)]
struct Order {
    steps: BTreeMap<SetupId, Vec<OperationId>>,
    setup_of: BTreeMap<OperationId, SetupId>,
}

impl Order {
    fn new(ir: &CadIr) -> Self {
        let mut order = Self::default();
        for setup in &ir.process.setups {
            let mut found = Vec::new();
            let mut stack: Vec<&Step> = setup.steps.iter().rev().collect();
            while let Some(step) = stack.pop() {
                match step {
                    Step::Operation(operation) => found.push(operation.clone()),
                    Step::Folder { steps, .. } => stack.extend(steps.iter().rev()),
                    Step::Function(_) => {}
                }
            }
            let mut pending = found.clone();
            while let Some(operation) = pending.pop() {
                if order.setup_of.contains_key(&operation) {
                    continue;
                }
                order.setup_of.insert(operation.clone(), setup.id.clone());
                if let Some(OperationKind::Modifier { inputs, .. }) = ir
                    .process
                    .operations
                    .iter()
                    .find(|record| record.id == operation)
                    .map(|record| &record.kind)
                {
                    pending.extend(inputs.iter().cloned());
                }
            }
            order.steps.insert(setup.id.clone(), found);
        }
        order
    }
}

/// Whether a wire object is a `Choice` that names one of `dirty` among its
/// `parameters`.
fn selects(object: &serde_json::Map<String, serde_json::Value>, dirty: &BTreeSet<String>) -> bool {
    let only_choice_keys = object
        .keys()
        .all(|key| key == "value" || key == "parameters");
    only_choice_keys
        && object
            .get("parameters")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.as_str().is_some_and(|id| dirty.contains(id)))
            })
}

/// Removes the `value` of every `Choice` that names one of `dirty`, and
/// reports whether it removed one.
fn clear_choice_values(value: &mut serde_json::Value, dirty: &BTreeSet<String>) -> bool {
    let mut changed = false;
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        match node {
            serde_json::Value::Array(items) => stack.extend(items.iter_mut()),
            serde_json::Value::Object(object) => {
                if selects(object, dirty) && object.remove("value").is_some() {
                    changed = true;
                }
                stack.extend(object.values_mut());
            }
            _ => {}
        }
    }
    changed
}

impl CadIr {
    /// Marks and refreshes what the process and PMI records hold that
    /// depends on an edit's dependents.
    pub(super) fn refresh_process(
        &mut self,
        history: &History,
        currents: &Currents,
        dependents: &Dependents,
    ) -> Result<(), EditError> {
        let design: BTreeSet<String> = dependents
            .parameters
            .iter()
            .map(|parameter| parameter.as_str().to_owned())
            .collect();
        let dirty = self.dirty_process_parameters(history, &design)?;
        self.recompute_process_values(&dirty);
        let named: BTreeSet<String> = dirty
            .iter()
            .map(|parameter| parameter.as_str().to_owned())
            .chain(design)
            .collect();
        self.clear_choices(&named)?;
        self.mark_process_stale(history, currents, dependents, &named)
    }

    /// The process parameters whose definition names a design parameter of
    /// `design`, or a process parameter that does, transitively.
    fn dirty_process_parameters(
        &self,
        history: &History,
        design: &BTreeSet<String>,
    ) -> Result<BTreeSet<ProcessParameterId>, EditError> {
        let is_container = |id: &str| history.members(id).is_some();
        let mut mentions = Vec::with_capacity(self.process.parameters.len());
        for parameter in &self.process.parameters {
            mentions.push(record_reads(&parameter.definition, &is_container)?.mentions);
        }
        let mut dirty: BTreeSet<String> = BTreeSet::new();
        loop {
            let before = dirty.len();
            for (parameter, named) in self.process.parameters.iter().zip(&mentions) {
                if named
                    .iter()
                    .any(|id| design.contains(id) || dirty.contains(id))
                {
                    dirty.insert(parameter.id.as_str().to_owned());
                }
            }
            if dirty.len() == before {
                break;
            }
        }
        Ok(self
            .process
            .parameters
            .iter()
            .filter(|parameter| dirty.contains(parameter.id.as_str()))
            .map(|parameter| parameter.id.clone())
            .collect())
    }

    /// Recomputes the value of every process parameter of `dirty` whose
    /// definition is a neutral expression of a declared kind, in dependency
    /// order, and clears the value of every other one. A design reference
    /// reads its parameter's cache in the active configuration of the
    /// parameter's scope, as a bound dimension does (§5.14).
    fn recompute_process_values(&mut self, dirty: &BTreeSet<ProcessParameterId>) {
        let index: BTreeMap<ProcessParameterId, usize> = self
            .process
            .parameters
            .iter()
            .enumerate()
            .map(|(at, parameter)| (parameter.id.clone(), at))
            .collect();
        let mut pending = dirty.clone();
        loop {
            let mut done = Vec::new();
            for parameter in &pending {
                let record = &self.process.parameters[index[parameter]];
                let outcome = match (&record.definition, &record.kind) {
                    (
                        ProcessDefinition::Expression(Expression::Neutral(Neutral(node))),
                        Some(kind),
                    ) => {
                        let mut blocked = false;
                        let result =
                            evaluate_tree(node, &mut |reference: &ParamRef| match reference {
                                ParamRef::Process(target) if pending.contains(target) => {
                                    blocked = true;
                                    None
                                }
                                ParamRef::Process(target) => {
                                    let held = &self.process.parameters[*index.get(target)?];
                                    cache_value(held.kind.as_ref()?, held.value.as_ref()?)
                                }
                                ParamRef::Design(design) => {
                                    let RefTarget::Parameter(target) = &design.target else {
                                        return None;
                                    };
                                    if design.through.is_some() {
                                        return None;
                                    }
                                    let held = self.design_parameter(target).ok()?;
                                    let configuration = self.active_configuration_of(&held.scope);
                                    let cache =
                                        self.parameter_cache(target, configuration).ok()??;
                                    cache_value(&held.kind, cache)
                                }
                            });
                        if blocked {
                            Attempt::Blocked
                        } else {
                            Attempt::Done(
                                result.ok().and_then(|value| kind_value(kind, value).ok()),
                            )
                        }
                    }
                    _ => Attempt::Done(None),
                };
                if let Attempt::Done(value) = outcome {
                    done.push((parameter.clone(), value));
                }
            }
            if done.is_empty() {
                break;
            }
            for (parameter, value) in done {
                self.process.parameters[index[&parameter]].value = value;
                pending.remove(&parameter);
            }
        }
        for parameter in pending {
            self.process.parameters[index[&parameter]].value = None;
        }
    }

    /// Clears the `value` of every `Choice` one of whose `parameters` is in
    /// `named` (§7).
    fn clear_choices(&mut self, named: &BTreeSet<String>) -> Result<(), EditError> {
        fn all<T: serde::Serialize + serde::de::DeserializeOwned>(
            records: &mut [T],
            named: &BTreeSet<String>,
        ) -> Result<(), EditError> {
            for record in records {
                rewrite(record, |json| Ok(clear_choice_values(json, named)))?;
            }
            Ok(())
        }
        if named.is_empty() {
            return Ok(());
        }
        let process = &mut self.process;
        all(&mut process.setups, named)?;
        all(&mut process.operations, named)?;
        all(&mut process.features, named)?;
        all(&mut process.groups, named)?;
        all(&mut process.tools, named)?;
        all(&mut process.presets, named)?;
        all(&mut process.machines, named)?;
        all(&mut process.programs, named)
    }

    /// The ids of the records an operation machines with: the operation, its
    /// tooling, its machining features, its groups and their parents, and
    /// its setup.
    fn machining_records(
        &self,
        operation: &OperationId,
        setup: Option<&SetupId>,
    ) -> BTreeSet<String> {
        let mut found = BTreeSet::from([operation.as_str().to_owned()]);
        if let Some(setup) = setup {
            found.insert(setup.as_str().to_owned());
        }
        let Some(record) = self
            .process
            .operations
            .iter()
            .find(|record| &record.id == operation)
        else {
            return found;
        };
        match &record.tooling {
            Some(Tooling::Tool(tool)) => {
                found.insert(tool.as_str().to_owned());
            }
            Some(Tooling::Preset(preset)) => {
                found.insert(preset.as_str().to_owned());
                if let Some(held) = self.process.presets.iter().find(|held| &held.id == preset) {
                    found.insert(held.tool.as_str().to_owned());
                }
            }
            None => {}
        }
        found.extend(
            record
                .features
                .iter()
                .map(|feature| feature.as_str().to_owned()),
        );
        let mut groups: Vec<_> = record.groups.iter().collect();
        while let Some(group) = groups.pop() {
            if !found.insert(group.as_str().to_owned()) {
                continue;
            }
            if let Some(parent) = self
                .process
                .groups
                .iter()
                .find(|held| &held.id == group)
                .and_then(|held| held.parent.as_ref())
            {
                groups.push(parent);
            }
        }
        found
    }

    /// What every process record and the parameters it owns read, keyed by
    /// the record's id.
    fn process_reads(&self, history: &History) -> Result<BTreeMap<String, Vec<Reads>>, EditError> {
        let is_container = |id: &str| history.members(id).is_some();
        let mut found: BTreeMap<String, Vec<Reads>> = BTreeMap::new();
        macro_rules! read {
            ($($arena:ident),*) => {
                $(for record in &self.process.$arena {
                    found
                        .entry(record.id.as_str().to_owned())
                        .or_default()
                        .push(record_reads(record, &is_container)?);
                })*
            };
        }
        read!(setups, operations, features, groups, tools, presets);
        for parameter in &self.process.parameters {
            let owner = match &parameter.owner {
                ProcessOwner::Setup(id) => id.as_str(),
                ProcessOwner::Operation(id) => id.as_str(),
                ProcessOwner::Feature(id) => id.as_str(),
                ProcessOwner::Group(id) => id.as_str(),
                ProcessOwner::Tool(id) => id.as_str(),
                ProcessOwner::Preset(id) => id.as_str(),
                ProcessOwner::Machine(id) => id.as_str(),
                ProcessOwner::Program(id) => id.as_str(),
            };
            found
                .entry(owner.to_owned())
                .or_default()
                .push(record_reads(parameter, &is_container)?);
        }
        Ok(found)
    }

    /// Marks `Stale` the toolpaths and process states that depend on an
    /// edit, and those §7.9 names after them.
    fn mark_process_stale(
        &mut self,
        history: &History,
        currents: &Currents,
        dependents: &Dependents,
        named: &BTreeSet<String>,
    ) -> Result<(), EditError> {
        if self.process.operations.is_empty() && self.process.setups.is_empty() {
            return Ok(());
        }
        let order = Order::new(self);
        let reads = self.process_reads(history)?;
        let scope_of = |setup: Option<&SetupId>| {
            setup.and_then(|setup| {
                self.process
                    .setups
                    .iter()
                    .find(|record| &record.id == setup)
                    .map(|record| record.scope.clone())
            })
        };
        let depends = |records: &BTreeSet<String>, setup: Option<&SetupId>| {
            let context = Context {
                feature: None,
                scope: scope_of(setup),
            };
            records
                .iter()
                .filter_map(|record| reads.get(record))
                .flatten()
                .any(|read| {
                    read.mentions.iter().any(|id| named.contains(id))
                        || context.reads_depend(
                            &read.states,
                            history,
                            currents,
                            &dependents.features,
                        )
                })
        };
        let stale_setups: BTreeSet<SetupId> = self
            .process
            .setups
            .iter()
            .filter(|setup| {
                depends(
                    &BTreeSet::from([setup.id.as_str().to_owned()]),
                    Some(&setup.id),
                )
            })
            .map(|setup| setup.id.clone())
            .collect();
        let mut affected: BTreeSet<OperationId> = self
            .process
            .operations
            .iter()
            .filter(|operation| {
                let setup = order.setup_of.get(&operation.id);
                depends(&self.machining_records(&operation.id, setup), setup)
            })
            .map(|operation| operation.id.clone())
            .collect();
        let mut later: BTreeSet<OperationId> = affected.clone();
        for steps in order.steps.values() {
            if let Some(first) = steps
                .iter()
                .position(|operation| affected.contains(operation))
            {
                later.extend(steps[first..].iter().cloned());
            }
        }
        loop {
            let takes: Vec<OperationId> = self
                .process
                .operations
                .iter()
                .filter(|operation| !affected.contains(&operation.id))
                .filter(|operation| match &operation.kind {
                    OperationKind::Modifier { inputs, .. } => {
                        inputs.iter().any(|input| affected.contains(input))
                    }
                    OperationKind::Machining { .. } => false,
                })
                .map(|operation| operation.id.clone())
                .collect();
            if takes.is_empty() {
                break;
            }
            affected.extend(takes);
        }
        for toolpath in &mut self.process.toolpaths {
            if affected.contains(&toolpath.operation) {
                toolpath.status = EvaluationStatus::Stale;
            }
        }
        for state in &mut self.process.states {
            let stale = match &state.of {
                ProcessStateOf::Stock(setup) => stale_setups.contains(setup),
                ProcessStateOf::After(operation) | ProcessStateOf::Removal(operation) => {
                    later.contains(operation)
                }
            };
            if stale {
                state.status = EvaluationStatus::Stale;
            }
        }
        Ok(())
    }
}
