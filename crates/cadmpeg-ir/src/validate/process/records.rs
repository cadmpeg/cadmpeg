// SPDX-License-Identifier: Apache-2.0
//! Lookups over the process layer that the part 10 rules share.
//!
//! Every lookup keeps the first record of an identity; a repeated identity is
//! reported by the identity rules and does not stop the others.

use std::collections::{BTreeMap, BTreeSet};

use crate::design::ids::{ProcessGroupId, ProcessParameterId, SetupId};
use crate::document::CadIr;
use crate::process::feature::MachiningFeature;
use crate::process::groups::{ProcessGroup, ProcessOwner, ProcessParameter};
use crate::process::operation::{Operation, OperationKind, Tooling};
use crate::process::setup::{Program, ProgramStep, Setup, Step};
use crate::process::state::ProcessState;
use crate::process::tool::{Machine, Preset, Tool};
use crate::validate::design::records::DesignRecords;

/// The process records of one document, by identity, with the setups each
/// record resolves from (part 7, Scope).
pub(super) struct ProcessRecords<'a> {
    pub(super) ir: &'a CadIr,
    pub(super) design: &'a DesignRecords<'a>,
    pub(super) setups: BTreeMap<&'a str, &'a Setup>,
    pub(super) operations: BTreeMap<&'a str, &'a Operation>,
    pub(super) features: BTreeMap<&'a str, &'a MachiningFeature>,
    pub(super) groups: BTreeMap<&'a str, &'a ProcessGroup>,
    pub(super) tools: BTreeMap<&'a str, &'a Tool>,
    pub(super) presets: BTreeMap<&'a str, &'a Preset>,
    pub(super) machines: BTreeMap<&'a str, &'a Machine>,
    pub(super) programs: BTreeMap<&'a str, &'a Program>,
    pub(super) parameters: BTreeMap<&'a str, &'a ProcessParameter>,
    pub(super) states: BTreeMap<&'a str, &'a ProcessState>,
    /// The setups whose steps list each operation, once per listing.
    pub(super) listings: BTreeMap<&'a str, Vec<&'a str>>,
    /// The setups each operation is in: the setups whose steps list it, and
    /// the setups of every modifier that takes it as an input.
    pub(super) memberships: BTreeMap<&'a str, BTreeSet<&'a str>>,
    /// The setups that use each record no setup holds, by its owner form.
    users: BTreeMap<OwnerKey<'a>, BTreeSet<&'a str>>,
}

/// A process record that can own parameters, by identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum OwnerKey<'a> {
    Setup(&'a str),
    Operation(&'a str),
    Feature(&'a str),
    Group(&'a str),
    Tool(&'a str),
    Preset(&'a str),
    Machine(&'a str),
    Program(&'a str),
}

impl<'a> OwnerKey<'a> {
    pub(super) fn of(owner: &'a ProcessOwner) -> Self {
        match owner {
            ProcessOwner::Setup(id) => Self::Setup(id.as_str()),
            ProcessOwner::Operation(id) => Self::Operation(id.as_str()),
            ProcessOwner::Feature(id) => Self::Feature(id.as_str()),
            ProcessOwner::Group(id) => Self::Group(id.as_str()),
            ProcessOwner::Tool(id) => Self::Tool(id.as_str()),
            ProcessOwner::Preset(id) => Self::Preset(id.as_str()),
            ProcessOwner::Machine(id) => Self::Machine(id.as_str()),
            ProcessOwner::Program(id) => Self::Program(id.as_str()),
        }
    }

    pub(super) const fn id(self) -> &'a str {
        match self {
            Self::Setup(id)
            | Self::Operation(id)
            | Self::Feature(id)
            | Self::Group(id)
            | Self::Tool(id)
            | Self::Preset(id)
            | Self::Machine(id)
            | Self::Program(id) => id,
        }
    }
}

fn by_id<'a, T>(records: &'a [T], id: impl Fn(&'a T) -> &'a str) -> BTreeMap<&'a str, &'a T> {
    let mut index = BTreeMap::new();
    for record in records {
        index.entry(id(record)).or_insert(record);
    }
    index
}

/// Every operation a list of setup steps names, folders included, in order,
/// walked without recursion.
pub(super) fn step_operations(steps: &[Step]) -> Vec<&str> {
    let mut found = Vec::new();
    let mut pending: Vec<&Step> = steps.iter().rev().collect();
    while let Some(step) = pending.pop() {
        match step {
            Step::Operation(operation) => found.push(operation.as_str()),
            Step::Folder { steps, .. } => pending.extend(steps.iter().rev()),
            Step::Function(_) => {}
        }
    }
    found
}

/// Every setup and operation a list of program steps names, groups
/// included, in order, walked without recursion.
pub(super) fn program_targets(steps: &[ProgramStep]) -> Vec<ProgramTarget<'_>> {
    let mut found = Vec::new();
    let mut pending: Vec<&ProgramStep> = steps.iter().rev().collect();
    while let Some(step) = pending.pop() {
        match step {
            ProgramStep::Setup(setup) => found.push(ProgramTarget::Setup(setup)),
            ProgramStep::Operation(operation) => {
                found.push(ProgramTarget::Operation(operation.as_str()));
            }
            ProgramStep::Group { steps, .. } => pending.extend(steps.iter().rev()),
            ProgramStep::Function(_) => {}
        }
    }
    found
}

/// A setup or operation that a program outputs.
#[derive(Debug, Clone, Copy)]
pub(super) enum ProgramTarget<'a> {
    Setup(&'a SetupId),
    Operation(&'a str),
}

impl<'a> ProcessRecords<'a> {
    pub(super) fn new(design: &'a DesignRecords<'a>) -> Self {
        let ir = design.ir;
        let process = &ir.process;
        let mut records = Self {
            ir,
            design,
            setups: by_id(&process.setups, |setup| setup.id.as_str()),
            operations: by_id(&process.operations, |operation| operation.id.as_str()),
            features: by_id(&process.features, |feature| feature.id.as_str()),
            groups: by_id(&process.groups, |group| group.id.as_str()),
            tools: by_id(&process.tools, |tool| tool.id.as_str()),
            presets: by_id(&process.presets, |preset| preset.id.as_str()),
            machines: by_id(&process.machines, |machine| machine.id.as_str()),
            programs: by_id(&process.programs, |program| program.id.as_str()),
            parameters: by_id(&process.parameters, |parameter| parameter.id.as_str()),
            states: by_id(&process.states, |state| state.id.as_str()),
            listings: BTreeMap::new(),
            memberships: BTreeMap::new(),
            users: BTreeMap::new(),
        };
        records.index_memberships();
        records.index_users();
        records
    }

    /// Lists the setups of each operation: the setups whose steps list it,
    /// then, through each modifier, the setups of the modifier for each of
    /// its inputs, until no set grows.
    fn index_memberships(&mut self) {
        for setup in &self.ir.process.setups {
            for operation in step_operations(&setup.steps) {
                self.listings
                    .entry(operation)
                    .or_default()
                    .push(setup.id.as_str());
                self.memberships
                    .entry(operation)
                    .or_default()
                    .insert(setup.id.as_str());
            }
        }
        let mut pending: Vec<&str> = self.memberships.keys().copied().collect();
        while let Some(modifier) = pending.pop() {
            let Some(Operation {
                kind: OperationKind::Modifier { inputs, .. },
                ..
            }) = self.operations.get(modifier).copied()
            else {
                continue;
            };
            let setups = self.memberships.get(modifier).cloned().unwrap_or_default();
            for input in inputs {
                let held = self.memberships.entry(input.as_str()).or_default();
                let before = held.len();
                held.extend(setups.iter().copied());
                if held.len() > before {
                    pending.push(input.as_str());
                }
            }
        }
    }

    /// The one setup an operation is in, where it is in exactly one.
    pub(super) fn setup_of(&self, operation: &str) -> Option<&'a str> {
        let setups = self.memberships.get(operation)?;
        match setups.len() {
            1 => setups.first().copied(),
            _ => None,
        }
    }

    fn index_users(&mut self) {
        let mut users: BTreeMap<OwnerKey<'a>, BTreeSet<&'a str>> = BTreeMap::new();
        for operation in &self.ir.process.operations {
            let Some(setup) = self.setup_of(operation.id.as_str()) else {
                continue;
            };
            for feature in &operation.features {
                users
                    .entry(OwnerKey::Feature(feature.as_str()))
                    .or_default()
                    .insert(setup);
            }
            for group in &operation.groups {
                for ancestor in self.group_chain(group.as_str()) {
                    users
                        .entry(OwnerKey::Group(ancestor))
                        .or_default()
                        .insert(setup);
                }
            }
            match &operation.tooling {
                Some(Tooling::Tool(tool)) => {
                    users
                        .entry(OwnerKey::Tool(tool.as_str()))
                        .or_default()
                        .insert(setup);
                }
                Some(Tooling::Preset(preset)) => {
                    users
                        .entry(OwnerKey::Preset(preset.as_str()))
                        .or_default()
                        .insert(setup);
                    if let Some(preset) = self.presets.get(preset.as_str()) {
                        users
                            .entry(OwnerKey::Tool(preset.tool.as_str()))
                            .or_default()
                            .insert(setup);
                    }
                }
                None => {}
            }
        }
        for setup in &self.ir.process.setups {
            if let Some(machine) = &setup.machine {
                users
                    .entry(OwnerKey::Machine(machine.as_str()))
                    .or_default()
                    .insert(setup.id.as_str());
            }
        }
        for program in &self.ir.process.programs {
            for target in program_targets(&program.steps) {
                let setup = match target {
                    ProgramTarget::Setup(setup) => {
                        self.setups.get_key_value(setup.as_str()).map(|(id, _)| *id)
                    }
                    ProgramTarget::Operation(operation) => self.setup_of(operation),
                };
                if let Some(setup) = setup {
                    users
                        .entry(OwnerKey::Program(program.id.as_str()))
                        .or_default()
                        .insert(setup);
                }
            }
        }
        self.users = users;
    }

    /// A group and its ancestors through `parent`, nearest first, stopping
    /// before a group already listed.
    pub(super) fn group_chain(&self, group: &'a str) -> Vec<&'a str> {
        let mut chain = Vec::new();
        let mut next = Some(group);
        while let Some(id) = next {
            if chain.contains(&id) {
                break;
            }
            chain.push(id);
            next = self
                .groups
                .get(id)
                .and_then(|group| group.parent.as_ref())
                .map(ProcessGroupId::as_str);
        }
        chain
    }

    /// The setups a record resolves from (part 7, Scope): a setup itself;
    /// an operation's setup; and, for a record no setup holds, the setups
    /// that use it.
    pub(super) fn setups_of(&self, owner: OwnerKey<'_>) -> BTreeSet<&'a str> {
        match owner {
            OwnerKey::Setup(id) => self
                .setups
                .get_key_value(id)
                .map(|(id, _)| *id)
                .into_iter()
                .collect(),
            OwnerKey::Operation(id) => self.setup_of(id).into_iter().collect(),
            _ => self.users.get(&owner).cloned().unwrap_or_default(),
        }
    }

    /// The design scope of a setup: its `scope` definition.
    pub(super) fn scope_of_setup(&self, setup: &str) -> Option<&'a str> {
        self.setups.get(setup).map(|setup| setup.scope.as_str())
    }

    /// The one scope the setups of a record machine, where they machine one.
    pub(super) fn scope_of(&self, owner: OwnerKey<'_>) -> Option<&'a str> {
        let scopes: BTreeSet<&str> = self
            .setups_of(owner)
            .into_iter()
            .filter_map(|setup| self.scope_of_setup(setup))
            .collect();
        match scopes.len() {
            1 => scopes.first().copied(),
            _ => None,
        }
    }

    /// Whether the process parameter exists.
    pub(super) fn has_parameter(&self, id: &ProcessParameterId) -> bool {
        self.parameters.contains_key(id.as_str())
    }
}
