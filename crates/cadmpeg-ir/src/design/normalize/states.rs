// SPDX-License-Identifier: Apache-2.0
//! The positions of §5.3 and the instance-target `state` of §8.
//!
//! `After(Container(c))` of an ordered container `c` is the state at `c`'s
//! rollback position, so an evaluation's `of`, a state base (of an
//! evaluation or of a process state), a current state, and a `StateRef` name
//! that state through its member: `After` the nearest feature of `c` before
//! the rollback, else `Start(c)`. A target into a state that `state: None`
//! provably reaches has `state: None`; every other target keeps its `state`.

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::declared_measures::DeclaredTarget;
use crate::design::container::{Container, Rollback};
use crate::design::evaluation::StateBase;
use crate::design::ids::ContainerId;
use crate::design::refs::{Evaluated, Member, Position, StateRef};
use crate::design::selection::{OccurrencePath, StepRef};
use crate::document::{Design, Model, Process};
use crate::features::ConfigurationId;
use crate::ids::{OccurrenceId, ProductDefinitionId};
use crate::physics::solver::SolverValue;
use crate::pmi::PmiDefinition;
use crate::process::state::ProcessBase;
use crate::products::{Occurrence, ProductDefinition};
use crate::references::InstanceTarget;

/// What the position and target rules read of the design, gathered before
/// any record changes.
#[derive(Debug, Default)]
pub(super) struct States {
    /// The position each ordered container's `After(Container(c))` names,
    /// by the container's id.
    rollbacks: BTreeMap<String, Position>,
    /// The scope that holds each member, by its definition's id.
    scopes: BTreeMap<Member, String>,
    /// The active configuration of each scope and the positions of its
    /// current states, in their one spelling.
    current: BTreeMap<String, (Option<ConfigurationId>, Vec<Position>)>,
    /// Whether the configuration that applies where no record selects one is
    /// known everywhere: each scope's active configuration is `None` or one
    /// of its own configurations whose parents resolve, and no scope,
    /// configuration, or scope entry puts an arrangement in force.
    settled: bool,
    /// The occurrences a record selects a configuration or a prototype for:
    /// a scope entry's `configuration`, or a configuration override's
    /// `configuration` or `prototype`.
    selected: BTreeSet<OccurrenceId>,
}

/// Lazily built index over `occurrences` and `definitions` for path checks.
struct PlacementIndex<'a> {
    /// `Some(prototype)` when one occurrence has this id; `None` on duplicate ids.
    unique_prototype: HashMap<&'a str, Option<&'a ProductDefinitionId>>,
    /// Definitions that list each occurrence id.
    owners: HashMap<&'a str, HashSet<&'a str>>,
}

impl<'a> PlacementIndex<'a> {
    fn build(occurrences: &'a [Occurrence], definitions: &'a [ProductDefinition]) -> Self {
        let mut unique_prototype: HashMap<&'a str, Option<&'a ProductDefinitionId>> =
            HashMap::with_capacity(occurrences.len());
        for record in occurrences {
            unique_prototype
                .entry(record.id.as_str())
                .and_modify(|entry| *entry = None)
                .or_insert(Some(&record.prototype));
        }
        let mut owners: HashMap<&'a str, HashSet<&'a str>> = HashMap::new();
        for definition in definitions {
            let def_id = definition.id.as_str();
            for occurrence in &definition.occurrences {
                owners
                    .entry(occurrence.as_str())
                    .or_default()
                    .insert(def_id);
            }
        }
        Self {
            unique_prototype,
            owners,
        }
    }
}

/// The occurrences and product definitions a target's path names.
struct Placements<'a> {
    occurrences: &'a [Occurrence],
    definitions: &'a [ProductDefinition],
    index: OnceCell<PlacementIndex<'a>>,
}

impl<'a> Placements<'a> {
    const fn new(occurrences: &'a [Occurrence], definitions: &'a [ProductDefinition]) -> Self {
        Self {
            occurrences,
            definitions,
            index: OnceCell::new(),
        }
    }

    fn index(&self) -> &PlacementIndex<'a> {
        self.index
            .get_or_init(|| PlacementIndex::build(self.occurrences, self.definitions))
    }
}

impl States {
    pub(super) fn new(design: &Design) -> Self {
        let containers: BTreeMap<&str, &Container> = design
            .containers
            .iter()
            .map(|container| (container.id().as_str(), container))
            .collect();
        let rollbacks = design
            .containers
            .iter()
            .filter_map(|container| {
                rollback_position(container)
                    .map(|position| (container.id().as_str().to_owned(), position))
            })
            .collect();
        let settled = design
            .configurations
            .iter()
            .all(|configuration| configuration.active_arrangements.is_empty())
            && design.scopes.iter().all(|scope| {
                scope.active_arrangements.is_empty()
                    && scope
                        .occurrences
                        .values()
                        .all(|entry| entry.arrangements.is_empty())
                    && scope.active_configuration.as_ref().is_none_or(|active| {
                        design.configurations.iter().any(|configuration| {
                            &configuration.id == active && configuration.scope == scope.definition
                        }) && design.configuration_chain(active).is_ok()
                    })
            });
        let entries = design
            .scopes
            .iter()
            .flat_map(|scope| &scope.occurrences)
            .filter(|(_, entry)| entry.configuration.is_some());
        let overrides = design
            .configurations
            .iter()
            .flat_map(|configuration| &configuration.overrides.occurrences)
            .filter(|(_, entry)| entry.configuration.is_some() || entry.prototype.is_some());
        let selected = entries
            .map(|(occurrence, _)| occurrence)
            .chain(overrides.map(|(occurrence, _)| occurrence))
            .cloned()
            .collect();
        let mut states = Self {
            rollbacks,
            scopes: BTreeMap::new(),
            current: BTreeMap::new(),
            settled,
            selected,
        };
        for scope in &design.scopes {
            let definition = scope.definition.as_str();
            let mut stack: Vec<&str> = scope.roots.iter().map(ContainerId::as_str).collect();
            let mut seen = BTreeSet::new();
            while let Some(id) = stack.pop() {
                if !seen.insert(id) {
                    continue;
                }
                let Some(container) = containers.get(id) else {
                    continue;
                };
                states
                    .scopes
                    .entry(Member::Container(container.id().clone()))
                    .or_insert_with(|| definition.to_owned());
                let members: Vec<&Member> = match container {
                    Container::Ordered { members, .. } => members.iter().collect(),
                    Container::Unordered { members, .. } => members.iter().collect(),
                };
                for member in members {
                    states
                        .scopes
                        .entry(member.clone())
                        .or_insert_with(|| definition.to_owned());
                    if let Member::Container(inner) = member {
                        stack.push(inner.as_str());
                    }
                }
            }
            let positions = scope
                .current
                .iter()
                .map(|current| {
                    let mut at = current.at.clone();
                    states.position(&mut at);
                    at
                })
                .collect();
            states.current.insert(
                definition.to_owned(),
                (scope.active_configuration.clone(), positions),
            );
        }
        states
    }

    /// Spells a position through its member.
    fn position(&self, position: &mut Position) {
        if let Position::After(Member::Container(container)) = position {
            if let Some(rollback) = self.rollbacks.get(container.as_str()) {
                *position = rollback.clone();
            }
        }
    }

    fn state_ref(&self, state: &mut StateRef) {
        if let Evaluated::State(position) = &mut state.of {
            self.position(position);
        }
    }

    /// Spells every evaluation's `of`, every state base, and every current
    /// state's position through its member.
    pub(super) fn normalize_design(&self, design: &mut Design) {
        for evaluation in &mut design.evaluations {
            if let Evaluated::State(position) = &mut evaluation.of {
                self.position(position);
            }
            if let Some(content) = &mut evaluation.content {
                if let StateBase::Current(position) = &mut content.base {
                    self.position(position);
                }
            }
        }
        for scope in &mut design.scopes {
            for current in &mut scope.current {
                self.position(&mut current.at);
            }
        }
    }

    /// Spells the design state base of every process state through its
    /// member.
    pub(super) fn normalize_process(&self, process: &mut Process) {
        for state in &mut process.states {
            if let Some(content) = &mut state.content {
                if let ProcessBase::Design(StateBase::Current(position)) = &mut content.base {
                    self.position(position);
                }
            }
        }
    }

    /// Spells every `StateRef` of the model records through its member, and
    /// clears each instance target's `state` that provably names what `None`
    /// names (see [`States::target`]).
    pub(super) fn normalize_model(&self, model: &mut Model) {
        if self.rollbacks.is_empty() && self.current.is_empty() {
            return;
        }
        let Model {
            occurrences,
            product_definitions,
            appearance_bindings,
            attributes,
            pmi,
            presentation_layers,
            mass_properties,
            rigid_bodies,
            colliders,
            articulations,
            solver_settings,
            tessellations,
            declared_measures,
            ..
        } = model;
        let placements = Placements::new(occurrences, product_definitions);
        for binding in appearance_bindings {
            self.target(&placements, &mut binding.target);
        }
        for attribute in attributes {
            self.target(&placements, &mut attribute.target);
        }
        for annotation in pmi {
            for target in &mut annotation.targets {
                self.target(&placements, target);
            }
            match &mut annotation.definition {
                PmiDefinition::DatumTarget { basis, .. } => {
                    for target in basis {
                        self.target(&placements, target);
                    }
                }
                PmiDefinition::GeometricTolerance { zones, .. } => {
                    for projection in zones.iter_mut().flat_map(|zone| &mut zone.projections) {
                        for target in &mut projection.boundaries {
                            self.target(&placements, target);
                        }
                        self.target(&placements, &mut projection.end);
                    }
                }
                PmiDefinition::Datum { .. }
                | PmiDefinition::DatumSystem { .. }
                | PmiDefinition::Dimension(_)
                | PmiDefinition::Presentation { .. } => {}
            }
        }
        for layer in presentation_layers {
            for item in &mut layer.items {
                self.target(&placements, item);
            }
        }
        for properties in mass_properties {
            self.target(&placements, &mut properties.target);
        }
        for body in rigid_bodies {
            self.target(&placements, &mut body.target);
        }
        for collider in colliders {
            self.target(&placements, &mut collider.target);
        }
        for articulation in articulations {
            self.target(&placements, &mut articulation.root);
        }
        for settings in solver_settings {
            self.target(&placements, &mut settings.target);
            for value in settings.values.values_mut() {
                if let SolverValue::Targets { targets } = value {
                    for target in targets {
                        self.target(&placements, target);
                    }
                }
            }
        }
        for tessellation in tessellations {
            if let Some(state) = &mut tessellation.state {
                self.state_ref(state);
            }
        }
        for measure in declared_measures {
            if let DeclaredTarget::State(state) = &mut measure.target {
                self.state_ref(state);
            }
        }
    }

    /// Spells a target's `state` through its member, and clears it where it
    /// provably names what `state: None` names: a current state of the scope
    /// that holds it, in that scope's active configuration (a `configuration`
    /// of `None` is the configuration in use), reached with no path, or with a
    /// path that [`States::reaches_unselected`] accepts. Every other `state`
    /// stays as decoded: a kept `state` that `None` could spell leaves two
    /// spellings of one meaning, while a cleared `state` that `None` does not
    /// spell would change the geometry the target names (§8, part 9).
    fn target<E>(&self, placements: &Placements<'_>, target: &mut InstanceTarget<E>) {
        let Some(state) = &mut target.state else {
            return;
        };
        self.state_ref(state);
        let Evaluated::State(position) = &state.of else {
            return;
        };
        let member = match position {
            Position::Start(container) => Member::Container(container.clone()),
            Position::After(member) => member.clone(),
        };
        let Some((holder, (active, positions))) = self
            .scopes
            .get(&member)
            .and_then(|scope| Some((scope, self.current.get(scope)?)))
        else {
            return;
        };
        let reads = positions.contains(position)
            && (state.configuration.is_none() || state.configuration == *active)
            && target
                .path
                .as_ref()
                .is_none_or(|path| self.reaches_unselected(placements, path, holder));
        if reads {
            target.state = None;
        }
    }

    /// Whether the instance at `path` is provably an instance of `holder` in
    /// `holder`'s active configuration: the document is `settled`, the path
    /// is not empty, and each step names, with no configuration of its own,
    /// the one occurrence record of its id, which no record selects for
    /// (`selected`), and which a definition owns, the previous step's
    /// prototype for every step after the first; the last step's prototype
    /// is `holder`. No record selects a configuration or a prototype for an
    /// occurrence of such a path, and no arrangement is in force anywhere, so
    /// every occurrence on it resolves through an active configuration and
    /// the last one reads `holder`'s (§5.1, §5.11). Any other path is not
    /// established here.
    fn reaches_unselected(
        &self,
        placements: &Placements<'_>,
        path: &OccurrencePath,
        holder: &str,
    ) -> bool {
        if !self.settled {
            return false;
        }
        let index = placements.index();
        let mut above: Option<&ProductDefinitionId> = None;
        for step in &path.0 {
            let StepRef::Occurrence(occurrence) = &step.occurrence else {
                return false;
            };
            if step.configuration.is_some() || self.selected.contains(occurrence) {
                return false;
            }
            let Some(Some(prototype)) = index.unique_prototype.get(occurrence.as_str()).copied()
            else {
                return false;
            };
            let owned = index.owners.get(occurrence.as_str()).is_some_and(|owners| {
                above.map_or_else(
                    || !owners.is_empty(),
                    |parent| owners.contains(parent.as_str()),
                )
            });
            if !owned {
                return false;
            }
            above = Some(prototype);
        }
        above.is_some_and(|prototype| prototype.as_str() == holder)
    }
}

/// The position an ordered container's `After(Container(c))` names: the
/// state entering the member at its rollback position, which is `After` the
/// nearest member before it that is a feature, or `Start(c)` where no feature
/// of `c` precedes it (§5.3). A member that is a container is passed over:
/// the state leaving it is the state entering it. `None` for an unordered
/// container, and for a rollback that names no member of its container.
fn rollback_position(container: &Container) -> Option<Position> {
    let Container::Ordered {
        id,
        members,
        rollback,
        ..
    } = container
    else {
        return None;
    };
    let at = match rollback {
        Rollback::End => members.len(),
        Rollback::Before(member) => members.iter().position(|listed| listed == member)?,
    };
    Some(
        members[..at]
            .iter()
            .rev()
            .find(|member| matches!(member, Member::Feature(_)))
            .map_or_else(
                || Position::Start(id.clone()),
                |feature| Position::After(feature.clone()),
            ),
    )
}
