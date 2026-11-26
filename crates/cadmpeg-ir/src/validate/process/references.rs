// SPDX-License-Identifier: Apache-2.0
//! References between process records (§7.3 to §7.8): tooling, machines,
//! machining features, groups, parameter owners, and units.

use std::collections::{BTreeMap, BTreeSet};
use std::mem::discriminant;

use super::records::{OwnerKey, ProcessRecords};
use crate::design::ids::{MachineId, MachiningFeatureId};
use crate::design::units::UnitSymbol;
use crate::index::ModelIndex;
use crate::process::feature::MachiningFeatureClass;
use crate::process::groups::GroupKind;
use crate::process::operation::{ProcessGeometry, Tooling};
use crate::process::setup::{FunctionStep, ProgramStep, Step};
use crate::report::check::Finding;
use crate::validate::cycles::on_cycles;

pub(super) fn check_references(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    check_tooling(records, findings);
    check_machining_features(records, findings);
    check_groups(records, findings);
    check_owners(records, findings);
    check_units(records, findings);
}

/// `Tooling`, `Preset.tool`, `ToolPocket.tool`, `Setup.machine`, and
/// `Program.machine` resolve; no machine has two pockets holding one tool.
fn check_tooling(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    let process = &records.ir.process;
    for operation in &process.operations {
        let (exists, what, target) = match &operation.tooling {
            Some(Tooling::Tool(tool)) => (
                records.tools.contains_key(tool.as_str()),
                "tool",
                tool.as_str(),
            ),
            Some(Tooling::Preset(preset)) => (
                records.presets.contains_key(preset.as_str()),
                "preset",
                preset.as_str(),
            ),
            None => continue,
        };
        if !exists {
            super::error(
                findings,
                operation.id.as_str(),
                crate::rule!(
                    "process.operation-tooling-names-no",
                    "operation's tooling names {target}, which is no {what}"
                ),
            );
        }
    }
    for preset in &process.presets {
        if !records.tools.contains_key(preset.tool.as_str()) {
            super::error(
                findings,
                preset.id.as_str(),
                crate::rule!(
                    "process.preset-tool-no-tool",
                    "preset's tool {} is no tool",
                    preset.tool.as_str()
                ),
            );
        }
    }
    for machine in &process.machines {
        let mut held = BTreeSet::new();
        for pocket in &machine.pockets {
            let tool = pocket.tool.as_str();
            if !records.tools.contains_key(tool) {
                super::error(
                    findings,
                    machine.id.as_str(),
                    crate::rule!(
                        "process.machine-pocket-holds-no-tool",
                        "machine pocket holds {tool}, which is no tool"
                    ),
                );
            }
            if !held.insert(tool) {
                super::error(
                    findings,
                    machine.id.as_str(),
                    crate::rule!(
                        "process.machine-two-pockets-holding",
                        "machine has two pockets holding {tool}"
                    ),
                );
            }
        }
    }
    let machine = |holder: &str, machine: Option<&MachineId>, findings: &mut Vec<Finding>| {
        if let Some(machine) = machine {
            if !records.machines.contains_key(machine.as_str()) {
                super::error(
                    findings,
                    holder,
                    crate::rule!(
                        "process.machine-no-machine",
                        "machine {} is no machine",
                        machine.as_str()
                    ),
                );
            }
        }
    };
    for setup in &process.setups {
        machine(setup.id.as_str(), setup.machine.as_ref(), findings);
    }
    for program in &process.programs {
        machine(program.id.as_str(), program.machine.as_ref(), findings);
    }
}

/// `Operation.features` and `Toolpath.feature` name machining features, a
/// toolpath's feature is one its operation lists, and `Replicate.base` and
/// `Compound.members` resolve and form no cycle (§7.4, §7.8).
fn check_machining_features(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    let process = &records.ir.process;
    for operation in &process.operations {
        for feature in &operation.features {
            if !records.features.contains_key(feature.as_str()) {
                super::error(
                    findings,
                    operation.id.as_str(),
                    crate::rule!(
                        "process.operation-lists-no-machining-feature",
                        "operation lists {}, which is no machining feature",
                        feature.as_str()
                    ),
                );
            }
        }
    }
    for toolpath in &process.toolpaths {
        let id = toolpath.id.as_str();
        let operation = records.operations.get(toolpath.operation.as_str());
        if operation.is_none() {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.toolpath-operation-no-operation",
                    "toolpath's operation {} is no operation",
                    toolpath.operation.as_str()
                ),
            );
        }
        let Some(feature) = &toolpath.feature else {
            continue;
        };
        if !records.features.contains_key(feature.as_str()) {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.toolpath-feature-no-machining-feature",
                    "toolpath's feature {} is no machining feature",
                    feature.as_str()
                ),
            );
        } else if operation.is_some_and(|operation| !operation.features.contains(feature)) {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.toolpath-feature-not-one-operation-lists",
                    "toolpath's feature {} is not one its operation lists",
                    feature.as_str()
                ),
            );
        }
    }
    let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for feature in &process.features {
        let id = feature.id.as_str();
        let named: Vec<&str> = match &feature.class {
            MachiningFeatureClass::Replicate { base } => vec![base.as_str()],
            MachiningFeatureClass::Compound { members } => {
                members.iter().map(MachiningFeatureId::as_str).collect()
            }
            _ => continue,
        };
        for &target in &named {
            if !records.features.contains_key(target) {
                super::error(
                    findings,
                    id,
                    crate::rule!(
                        "process.machining-feature-names-no-machining-feature",
                        "machining feature names {target}, which is no machining feature"
                    ),
                );
            }
        }
        graph.insert(id, named.into_iter().collect());
    }
    for id in on_cycles(&graph) {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.machining-feature-contains-itself-through-replicate",
                "machining feature contains itself through `Replicate` and `Compound`"
            ),
        );
    }
}

/// Every `Operation.groups` entry resolves, and an operation lists at most
/// one group of each kind; group parents resolve, form no cycle, and are of
/// their child's kind; `combine` occurs only on an operation's own geometry,
/// of an operation that lists a `Geometry` group (§7.7).
fn check_groups(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    let process = &records.ir.process;
    let combined = |geometry: &[ProcessGeometry]| geometry.iter().any(|g| g.combine.is_some());
    for operation in &process.operations {
        let id = operation.id.as_str();
        let mut kinds = Vec::new();
        let mut geometry_group = false;
        for group in &operation.groups {
            let Some(listed) = records.groups.get(group.as_str()) else {
                super::error(
                    findings,
                    id,
                    crate::rule!(
                        "process.operation-lists-no-group",
                        "operation lists {}, which is no group",
                        group.as_str()
                    ),
                );
                continue;
            };
            geometry_group |= matches!(listed.kind, GroupKind::Geometry { .. });
            let kind = discriminant(&listed.kind);
            if kinds.contains(&kind) {
                super::error(
                    findings,
                    id,
                    crate::rule!(
                        "process.operation-lists-second-group-one-kind",
                        "operation lists {}, a second group of one kind",
                        group.as_str()
                    ),
                );
            }
            kinds.push(kind);
        }
        if combined(&operation.geometry) && !geometry_group {
            super::error(
                findings,
                id,
                crate::rule!("process.operation-geometry-states-combine-operation-lists", 
                    "operation's geometry states `combine`, but the operation lists no geometry group"
                ),
            );
        }
    }
    let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for group in &process.groups {
        let id = group.id.as_str();
        if let GroupKind::Geometry { geometry } = &group.kind {
            if combined(geometry) {
                super::error(
                    findings,
                    id,
                    crate::rule!("process.group-geometry-states-combine-only-operation", "group's geometry states `combine`, which only an operation's own geometry states"),
                );
            }
        }
        let Some(parent) = &group.parent else {
            continue;
        };
        let parent = parent.as_str();
        match records.groups.get(parent) {
            None => super::error(
                findings,
                id,
                crate::rule!(
                    "process.group-parent-no-group",
                    "group's parent {parent} is no group"
                ),
            ),
            Some(listed) if discriminant(&listed.kind) != discriminant(&group.kind) => {
                super::error(
                    findings,
                    id,
                    crate::rule!(
                        "process.group-parent-another-kind",
                        "group's parent {parent} is of another kind"
                    ),
                );
            }
            Some(_) => {}
        }
        graph.insert(id, BTreeSet::from([parent]));
    }
    for id in on_cycles(&graph) {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.group-own-ancestor-through-parent",
                "group is its own ancestor through `parent`"
            ),
        );
    }
    for feature in &process.features {
        if combined(&feature.geometry) {
            super::error(
                findings,
                feature.id.as_str(),
                crate::rule!("process.machining-feature-geometry-states-combine-only", "machining feature's geometry states `combine`, which only an operation's own geometry states"),
            );
        }
    }
}

/// `ProcessParameter.owner` resolves; names are unique per owner (§7.7).
fn check_owners(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    let mut names: BTreeSet<(OwnerKey<'_>, &str)> = BTreeSet::new();
    for parameter in &records.ir.process.parameters {
        let id = parameter.id.as_str();
        let owner = OwnerKey::of(&parameter.owner);
        let (exists, what) = match owner {
            OwnerKey::Setup(target) => (records.setups.contains_key(target), "setup"),
            OwnerKey::Operation(target) => (records.operations.contains_key(target), "operation"),
            OwnerKey::Feature(target) => {
                (records.features.contains_key(target), "machining feature")
            }
            OwnerKey::Group(target) => (records.groups.contains_key(target), "group"),
            OwnerKey::Tool(target) => (records.tools.contains_key(target), "tool"),
            OwnerKey::Preset(target) => (records.presets.contains_key(target), "preset"),
            OwnerKey::Machine(target) => (records.machines.contains_key(target), "machine"),
            OwnerKey::Program(target) => (records.programs.contains_key(target), "program"),
        };
        if !exists {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.parameter-owner-no",
                    "parameter's owner {} is no {what}",
                    owner.id()
                ),
            );
        }
        if !names.insert((owner, parameter.name.as_str())) {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.parameter-name-repeats-among-parameters",
                    "parameter name `{}` repeats among the parameters of {}",
                    parameter.name.as_str(),
                    owner.id()
                ),
            );
        }
    }
}

/// Every `native_ref` of a process record resolves (part 10, Scope and
/// linkage), the steps of a setup's or a program's function steps included.
pub(super) fn check_native_refs(
    records: &ProcessRecords<'_>,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    let ir = records.ir;
    let mut check = |owner: &str, native_ref: Option<&String>| {
        let Some(target) = native_ref else { return };
        let resolves = crate::validate::annotations_native::native_link_resolves(
            ir,
            |target| ids.native_record_owns(target),
            target,
        );
        if !resolves {
            super::error(
                findings,
                owner,
                crate::rule!(
                    "process.native-ref-does-not-resolve",
                    "native_ref `{target}` does not resolve"
                ),
            );
        }
    };
    let process = &ir.process;
    for setup in &process.setups {
        check(setup.id.as_str(), setup.native_ref.as_ref());
        for step in setup_functions(&setup.steps) {
            check(setup.id.as_str(), step.native_ref.as_ref());
        }
    }
    for operation in &process.operations {
        check(operation.id.as_str(), operation.native_ref.as_ref());
    }
    for feature in &process.features {
        check(feature.id.as_str(), feature.native_ref.as_ref());
    }
    for group in &process.groups {
        check(group.id.as_str(), group.native_ref.as_ref());
    }
    for tool in &process.tools {
        check(tool.id.as_str(), tool.native_ref.as_ref());
    }
    for preset in &process.presets {
        check(preset.id.as_str(), preset.native_ref.as_ref());
    }
    for machine in &process.machines {
        check(machine.id.as_str(), machine.native_ref.as_ref());
    }
    for program in &process.programs {
        check(program.id.as_str(), program.native_ref.as_ref());
        for step in program_functions(&program.steps) {
            check(program.id.as_str(), step.native_ref.as_ref());
        }
    }
    for parameter in &process.parameters {
        check(parameter.id.as_str(), parameter.native_ref.as_ref());
    }
}

/// The function steps of a setup's steps, folders included, walked without
/// recursion.
fn setup_functions(steps: &[Step]) -> Vec<&FunctionStep> {
    let mut found = Vec::new();
    let mut pending: Vec<&Step> = steps.iter().collect();
    while let Some(step) = pending.pop() {
        match step {
            Step::Function(function) => found.push(function),
            Step::Folder { steps, .. } => pending.extend(steps),
            Step::Operation(_) => {}
        }
    }
    found
}

/// The function steps of a program's steps, groups included, walked without
/// recursion.
fn program_functions(steps: &[ProgramStep]) -> Vec<&FunctionStep> {
    let mut found = Vec::new();
    let mut pending: Vec<&ProgramStep> = steps.iter().collect();
    while let Some(step) = pending.pop() {
        match step {
            ProgramStep::Function(function) => found.push(function),
            ProgramStep::Group { steps, .. } => pending.extend(steps),
            ProgramStep::Setup(_) | ProgramStep::Operation(_) => {}
        }
    }
    found
}

/// A unit named outside an expression is a length symbol where it states a
/// length unit.
fn check_units(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    let process = &records.ir.process;
    for tool in &process.tools {
        if let Some(unit) = tool.unit {
            length_unit(tool.id.as_str(), "tool's unit", unit, findings);
        }
    }
    for program in &process.programs {
        if let Some(unit) = program.output.unit.as_ref().and_then(|unit| unit.value) {
            length_unit(program.id.as_str(), "program's output unit", unit, findings);
        }
    }
}

fn length_unit(holder: &str, what: &str, unit: UnitSymbol, findings: &mut Vec<Finding>) {
    let length = matches!(
        unit,
        UnitSymbol::Nm
            | UnitSymbol::Um
            | UnitSymbol::Mm
            | UnitSymbol::Cm
            | UnitSymbol::M
            | UnitSymbol::Km
            | UnitSymbol::In
            | UnitSymbol::Ft
            | UnitSymbol::Yd
            | UnitSymbol::Mi
    );
    if !length {
        super::error(
            findings,
            holder,
            crate::rule!(
                "process.no-length-unit",
                "{what} `{}` is no length unit",
                unit.name()
            ),
        );
    }
}

#[cfg(test)]
mod tests;
