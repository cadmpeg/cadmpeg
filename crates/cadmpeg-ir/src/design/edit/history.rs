// SPDX-License-Identifier: Apache-2.0
//! The order of a document's histories (§5.3) and the dependencies of its
//! features and parameters (D26).
//!
//! A state at position `P` depends on feature `f` when `f` is at or before
//! `P` in `P`'s container chain, or when a feature at or before `P` depends on
//! `f` transitively. A feature depends on what its definition references, and
//! a parameter on what its expression references, including the states its
//! measures read.

use std::collections::{BTreeMap, BTreeSet};

use super::wire::{record_reads, Reads, StateRead};
use super::{container_members, EditError};
use crate::design::container::Container;
use crate::design::feature::FeatureOperation;
use crate::design::parameter::{ParameterDefinition, ParameterOwner};
use crate::design::refs::{Member, Position};
use crate::document::Design;
use crate::features::{FeatureId, ParameterId};
use crate::ids::ProductDefinitionId;

/// One level of a member's place: the container, whether it is ordered, and
/// the member's index in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Step {
    pub(super) container: String,
    pub(super) ordered: bool,
    pub(super) index: usize,
}

/// Every member's place in the histories of a design.
#[derive(Debug, Default)]
pub(super) struct History {
    /// Each placed member's steps from its scope's root container to itself.
    paths: BTreeMap<Member, Vec<Step>>,
    /// The definition whose scope each root container belongs to.
    roots: BTreeMap<String, ProductDefinitionId>,
    /// The members of each container, in order where it is ordered.
    members: BTreeMap<String, (bool, Vec<Member>)>,
}

impl History {
    /// Indexes the containers of `design`. Part 10 makes the containers a
    /// tree in which each member has one place, which the editor's document
    /// keeps; on any other design a member takes its first place, and a walk
    /// up a cycle of containers stops.
    pub(super) fn new(design: &Design) -> Self {
        let mut history = Self::default();
        let mut places: BTreeMap<Member, Step> = BTreeMap::new();
        let mut nested: BTreeMap<String, Step> = BTreeMap::new();
        for container in &design.containers {
            let id = container.id().as_str().to_owned();
            let ordered = matches!(container, Container::Ordered { .. });
            let members: Vec<Member> = container_members(container).into_iter().cloned().collect();
            for (index, member) in members.iter().enumerate() {
                let step = Step {
                    container: id.clone(),
                    ordered,
                    index,
                };
                if let Member::Container(inner) = member {
                    nested
                        .entry(inner.as_str().to_owned())
                        .or_insert_with(|| step.clone());
                }
                places.entry(member.clone()).or_insert(step);
            }
            history.members.insert(id, (ordered, members));
        }
        for scope in &design.scopes {
            for root in &scope.roots {
                history
                    .roots
                    .insert(root.as_str().to_owned(), scope.definition.clone());
            }
        }
        for member in places.keys() {
            let mut path = Vec::new();
            let mut next = places.get(member);
            while let Some(step) = next {
                if path.len() > design.containers.len() {
                    break;
                }
                path.push(step.clone());
                next = nested.get(&step.container);
            }
            path.reverse();
            history.paths.insert(member.clone(), path);
        }
        history
    }

    /// The member and every member under it: where it is a container, its
    /// members and theirs.
    pub(super) fn members_under(&self, member: &Member) -> Vec<Member> {
        let mut found = Vec::new();
        let mut seen = BTreeSet::new();
        let mut stack = vec![member.clone()];
        while let Some(next) = stack.pop() {
            if !seen.insert(next.clone()) {
                continue;
            }
            if let Member::Container(container) = &next {
                if let Some((_, members)) = self.members.get(container.as_str()) {
                    stack.extend(members.iter().rev().cloned());
                }
            }
            found.push(next);
        }
        found
    }

    /// The member's steps from its root container, where it is placed.
    pub(super) fn path(&self, member: &Member) -> Option<&[Step]> {
        self.paths.get(member).map(Vec::as_slice)
    }

    /// The members of a container, and whether it is ordered.
    pub(super) fn members(&self, container: &str) -> Option<&(bool, Vec<Member>)> {
        self.members.get(container)
    }

    /// The definition whose scope holds a member.
    pub(super) fn scope_of(&self, member: &Member) -> Option<&ProductDefinitionId> {
        let root = match self.path(member)?.first() {
            Some(step) => step.container.as_str(),
            None => match member {
                Member::Container(id) => id.as_str(),
                Member::Feature(_) => return None,
            },
        };
        self.roots.get(root)
    }

    /// The steps and inclusiveness of the state at a position: `After(m)`
    /// holds `m` and what precedes it; `Start(c)` holds only what precedes
    /// `c`.
    fn state_steps(&self, at: &Position) -> Option<(Vec<Step>, bool)> {
        match at {
            Position::After(member) => Some((self.path(member)?.to_vec(), true)),
            Position::Start(container) => {
                let mut steps = self
                    .path(&Member::Container(container.clone()))
                    .map(<[Step]>::to_vec)
                    .unwrap_or_default();
                let ordered = self.members.get(container.as_str())?.0;
                steps.push(Step {
                    container: container.as_str().to_owned(),
                    ordered,
                    index: 0,
                });
                Some((steps, false))
            }
        }
    }

    /// Whether member `item` is at or before the state given by `steps`.
    fn holds(&self, steps: &[Step], inclusive: bool, item: &Member) -> bool {
        let Some(own) = self.path(item) else {
            return false;
        };
        let Some(last) = steps.len().checked_sub(1) else {
            return false;
        };
        for (level, step) in steps.iter().enumerate() {
            let Some(mine) = own.get(level) else {
                return false;
            };
            if mine.container != step.container {
                return false;
            }
            if level == last {
                return if step.ordered {
                    mine.index < step.index || (inclusive && mine.index == step.index)
                } else {
                    inclusive && mine.index == step.index
                };
            }
            if mine.index != step.index {
                return mine.index < step.index && steps[level..last].iter().all(|up| up.ordered);
            }
        }
        false
    }

    /// Whether the state at `at` depends on any feature of `features`.
    pub(super) fn state_depends(&self, at: &Position, features: &BTreeSet<FeatureId>) -> bool {
        let Some((steps, inclusive)) = self.state_steps(at) else {
            return false;
        };
        features
            .iter()
            .any(|feature| self.holds(&steps, inclusive, &Member::Feature(feature.clone())))
    }

    /// Whether the state entering `member` depends on any feature of
    /// `features`.
    pub(super) fn entering_depends(&self, member: &Member, features: &BTreeSet<FeatureId>) -> bool {
        let Some(steps) = self.path(member) else {
            return false;
        };
        features
            .iter()
            .any(|feature| self.holds(steps, false, &Member::Feature(feature.clone())))
    }

    /// Every feature of a member: the feature itself, or the features of a
    /// container and its nested containers.
    pub(super) fn features_of(&self, member: &Member) -> Vec<FeatureId> {
        let mut found = Vec::new();
        let mut seen = BTreeSet::new();
        let mut stack = vec![member.clone()];
        while let Some(next) = stack.pop() {
            match next {
                Member::Feature(feature) => found.push(feature),
                Member::Container(container) => {
                    if !seen.insert(container.as_str().to_owned()) {
                        continue;
                    }
                    if let Some((_, members)) = self.members.get(container.as_str()) {
                        stack.extend(members.iter().rev().cloned());
                    }
                }
            }
        }
        found
    }

    /// Every feature at or after a member's position in its container chain:
    /// the member and the later members of its container, and, through each
    /// ordered container above, the members after the container that holds
    /// it.
    pub(super) fn features_from(&self, member: &Member) -> Vec<FeatureId> {
        let mut found = self.features_of(member);
        let Some(path) = self.path(member) else {
            return found;
        };
        for step in path.iter().rev() {
            let Some((ordered, members)) = self.members.get(&step.container) else {
                continue;
            };
            if !ordered {
                break;
            }
            let later = step
                .index
                .checked_add(1)
                .and_then(|start| members.get(start..));
            for member in later.into_iter().flatten() {
                found.extend(self.features_of(member));
            }
        }
        found
    }
}

/// A feature or a parameter of the dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Node {
    Feature(FeatureId),
    Parameter(ParameterId),
}

/// Where a record reads states from: the feature whose `Input` state an
/// `Input` anchor reads, and the scope whose current geometry `Current`
/// reads.
#[derive(Debug, Clone, Default)]
pub(super) struct Context {
    pub(super) feature: Option<Member>,
    pub(super) scope: Option<ProductDefinitionId>,
}

/// The positions of each scope's current states.
pub(super) type Currents = BTreeMap<ProductDefinitionId, Vec<Position>>;

/// The positions of the current states of every scope of `design`.
pub(super) fn currents(design: &Design) -> Currents {
    let mut found = Currents::new();
    for scope in &design.scopes {
        found
            .entry(scope.definition.clone())
            .or_default()
            .extend(scope.current.iter().map(|current| current.at.clone()));
    }
    found
}

impl Context {
    /// Whether any of `states`, read in this context, depends on any of
    /// `features`.
    pub(super) fn reads_depend(
        &self,
        states: &[StateRead],
        history: &History,
        currents: &Currents,
        features: &BTreeSet<FeatureId>,
    ) -> bool {
        states.iter().any(|read| match read {
            StateRead::At(at) => history.state_depends(at, features),
            StateRead::Input => self
                .feature
                .as_ref()
                .is_some_and(|member| history.entering_depends(member, features)),
            StateRead::Current => self
                .scope
                .as_ref()
                .and_then(|scope| currents.get(scope))
                .is_some_and(|positions| {
                    positions
                        .iter()
                        .any(|at| history.state_depends(at, features))
                }),
        })
    }
}

struct Entry {
    node: Node,
    context: Context,
    mentions: BTreeSet<usize>,
    states: Vec<StateRead>,
}

/// The features and parameters of a design, with what each references.
pub(super) struct Graph {
    entries: Vec<Entry>,
    index: BTreeMap<Node, usize>,
}

/// The features and parameters that depend on an edit.
#[derive(Debug, Default)]
pub(super) struct Dependents {
    pub(super) features: BTreeSet<FeatureId>,
    pub(super) parameters: BTreeSet<ParameterId>,
}

impl Graph {
    /// Reads what every feature and parameter of `design` references. A
    /// feature references what its definition, its definitions in
    /// configurations, and the sketch or sheet records it owns reference; a
    /// parameter what its definition, its overrides, and the program that
    /// assigns it reference. A mention of a sketch, sketch entity, sketch
    /// constraint, or sheet is a mention of the feature that owns it.
    pub(super) fn new(design: &Design, history: &History) -> Result<Self, EditError> {
        let is_container = |id: &str| history.members(id).is_some();
        let mut entries = Vec::new();
        let mut index = BTreeMap::new();
        let mut owners: BTreeMap<String, usize> = BTreeMap::new();
        for feature in &design.features {
            let member = Member::Feature(feature.id.clone());
            let node = Node::Feature(feature.id.clone());
            index.insert(node.clone(), entries.len());
            owners.insert(feature.id.as_str().to_owned(), entries.len());
            if let FeatureOperation::Spreadsheet(sheet) = &feature.definition.operation {
                owners.insert(sheet.as_str().to_owned(), entries.len());
            }
            entries.push(Entry {
                node,
                context: Context {
                    scope: history.scope_of(&member).cloned(),
                    feature: Some(member),
                },
                mentions: BTreeSet::new(),
                states: Vec::new(),
            });
        }
        for parameter in &design.parameters {
            let node = Node::Parameter(parameter.id.clone());
            index.insert(node.clone(), entries.len());
            owners.insert(parameter.id.as_str().to_owned(), entries.len());
            let feature = match &parameter.owner {
                Some(ParameterOwner::Feature(feature)) => Some(Member::Feature(feature.clone())),
                _ => None,
            };
            entries.push(Entry {
                node,
                context: Context {
                    feature,
                    scope: Some(parameter.scope.clone()),
                },
                mentions: BTreeSet::new(),
                states: Vec::new(),
            });
        }
        let feature_at = |id: &FeatureId| index.get(&Node::Feature(id.clone())).copied();
        let parameter_at = |id: &ParameterId| index.get(&Node::Parameter(id.clone())).copied();
        for sketch in &design.sketches {
            if let Some(at) = feature_at(&sketch.owner) {
                owners.insert(sketch.id.as_str().to_owned(), at);
            }
        }
        for sketch in &design.spatial_sketches {
            if let Some(at) = feature_at(&sketch.owner) {
                owners.insert(sketch.id.as_str().to_owned(), at);
            }
        }
        let mut reads: Vec<(usize, Reads)> = Vec::new();
        let mut owned =
            |owners: &mut BTreeMap<String, usize>, sketch: &str, id: &str, record: Reads| {
                if let Some(&at) = owners.get(sketch) {
                    owners.insert(id.to_owned(), at);
                    reads.push((at, record));
                }
            };
        for entity in &design.sketch_entities {
            let found = record_reads(entity, &is_container)?;
            owned(
                &mut owners,
                entity.sketch.as_str(),
                entity.id.as_str(),
                found,
            );
        }
        for constraint in &design.sketch_constraints {
            let found = record_reads(constraint, &is_container)?;
            owned(
                &mut owners,
                constraint.sketch.as_str(),
                constraint.id.as_str(),
                found,
            );
        }
        for entity in &design.spatial_sketch_entities {
            let found = record_reads(entity, &is_container)?;
            owned(
                &mut owners,
                entity.sketch.as_str(),
                entity.id.as_str(),
                found,
            );
        }
        for constraint in &design.spatial_sketch_constraints {
            let found = record_reads(constraint, &is_container)?;
            owned(
                &mut owners,
                constraint.sketch.as_str(),
                constraint.id.as_str(),
                found,
            );
        }
        for sketch in &design.sketches {
            let found = record_reads(sketch, &is_container)?;
            owned(&mut owners, sketch.id.as_str(), sketch.id.as_str(), found);
        }
        for sketch in &design.spatial_sketches {
            let found = record_reads(sketch, &is_container)?;
            owned(&mut owners, sketch.id.as_str(), sketch.id.as_str(), found);
        }
        for sheet in &design.spreadsheets {
            let found = record_reads(sheet, &is_container)?;
            owned(&mut owners, sheet.id.as_str(), sheet.id.as_str(), found);
        }
        for feature in &design.features {
            if let Some(at) = feature_at(&feature.id) {
                reads.push((at, record_reads(feature, &is_container)?));
            }
        }
        let overrides = design
            .configurations
            .iter()
            .map(|configuration| &configuration.overrides)
            .chain(
                design
                    .arrangements
                    .iter()
                    .map(|arrangement| &arrangement.overrides),
            );
        for overrides in overrides {
            for (parameter, value) in &overrides.parameters {
                if let Some(at) = parameter_at(parameter) {
                    reads.push((at, record_reads(value, &is_container)?));
                }
            }
            for (feature, state) in &overrides.features {
                if let Some(at) = feature_at(feature) {
                    reads.push((at, record_reads(state, &is_container)?));
                }
            }
        }
        for scope in &design.scopes {
            for occurrence in scope.occurrences.values() {
                for (parameter, value) in &occurrence.parameters {
                    if let Some(at) = parameter_at(parameter) {
                        reads.push((at, record_reads(value, &is_container)?));
                    }
                }
            }
        }
        for parameter in &design.parameters {
            let Some(at) = parameter_at(&parameter.id) else {
                continue;
            };
            reads.push((at, record_reads(&parameter.definition, &is_container)?));
            if let ParameterDefinition::Assigned(program) = &parameter.definition {
                if let Some(record) = design.programs.iter().find(|record| &record.id == program) {
                    reads.push((at, record_reads(&record.body, &is_container)?));
                }
            }
        }
        for (at, found) in reads {
            let entry = &mut entries[at];
            for mention in found.mentions {
                if let Some(&target) = owners.get(&mention) {
                    if target != at {
                        entry.mentions.insert(target);
                    }
                }
            }
            entry.states.extend(found.states);
        }
        Ok(Self { entries, index })
    }

    /// The features and parameters that depend, transitively, on the seeds,
    /// the seeds included.
    pub(super) fn dependents(
        &self,
        history: &History,
        currents: &Currents,
        seeds: impl IntoIterator<Item = Node>,
    ) -> Dependents {
        let mut reverse: Vec<Vec<usize>> = vec![Vec::new(); self.entries.len()];
        for (at, entry) in self.entries.iter().enumerate() {
            for &target in &entry.mentions {
                reverse[target].push(at);
            }
        }
        let mut found = Dependents::default();
        let mut held = vec![false; self.entries.len()];
        let mut queue = Vec::new();
        let hold =
            |at: usize, held: &mut [bool], found: &mut Dependents, queue: &mut Vec<usize>| {
                if std::mem::replace(&mut held[at], true) {
                    return;
                }
                found.insert(self.entries[at].node.clone());
                queue.push(at);
            };
        for seed in seeds {
            match self.index.get(&seed) {
                Some(&at) => hold(at, &mut held, &mut found, &mut queue),
                None => found.insert(seed),
            }
        }
        loop {
            while let Some(at) = queue.pop() {
                for &dependent in &reverse[at] {
                    hold(dependent, &mut held, &mut found, &mut queue);
                }
            }
            for (at, entry) in self.entries.iter().enumerate() {
                if !held[at]
                    && entry
                        .context
                        .reads_depend(&entry.states, history, currents, &found.features)
                {
                    hold(at, &mut held, &mut found, &mut queue);
                }
            }
            if queue.is_empty() {
                return found;
            }
        }
    }
}

impl Dependents {
    fn insert(&mut self, node: Node) {
        match node {
            Node::Feature(feature) => {
                self.features.insert(feature);
            }
            Node::Parameter(parameter) => {
                self.parameters.insert(parameter);
            }
        }
    }
}
