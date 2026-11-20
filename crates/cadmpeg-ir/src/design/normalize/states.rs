// SPDX-License-Identifier: Apache-2.0
//! The positions of §5.3 and the instance-target `state` of §8.
//!
//! `After(Container(c))` of an ordered container `c` is the state at `c`'s
//! rollback position, so an evaluation's `of`, a state base (of an
//! evaluation or of a process state), a current state, and a `StateRef` name
//! that state through its member: `After` the nearest feature of `c` before
//! the rollback, else `Start(c)`. A target into a state that `state: None`
//! reaches has `state: None`.

use std::collections::{BTreeMap, BTreeSet};

use crate::declared_measures::DeclaredTarget;
use crate::design::container::{Container, Rollback};
use crate::design::evaluation::StateBase;
use crate::design::ids::ContainerId;
use crate::design::refs::{ConfigurationRef, Evaluated, Member, Position, StateRef};
use crate::design::selection::{OccurrencePath, StepRef};
use crate::document::{Design, Model, Process};
use crate::features::ConfigurationId;
use crate::ids::OccurrenceId;
use crate::physics::solver::SolverValue;
use crate::pmi::PmiDefinition;
use crate::process::state::ProcessBase;
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
    /// What the design states of the configuration each occurrence uses, by
    /// the occurrence's id.
    occurrences: BTreeMap<OccurrenceId, Uses>,
}

/// The statements of the configuration of its prototype that one
/// occurrence uses (§5.1, §5.11).
#[derive(Debug, Default)]
struct Uses {
    /// The base value of each scope entry that lists the occurrence; `None`
    /// is the prototype's active configuration.
    bases: Vec<Option<ConfigurationRef>>,
    /// The value each configuration or arrangement override states.
    overrides: Vec<ConfigurationRef>,
    /// Whether an override replaces the occurrence's prototype.
    replaced: bool,
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
        let mut states = Self {
            rollbacks,
            scopes: BTreeMap::new(),
            current: BTreeMap::new(),
            occurrences: occurrence_uses(design),
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
    /// clears each instance target's `state` that names what `None` names.
    pub(super) fn normalize_model(&self, model: &mut Model) {
        for binding in &mut model.appearance_bindings {
            self.target(&mut binding.target);
        }
        for attribute in &mut model.attributes {
            self.target(&mut attribute.target);
        }
        for annotation in &mut model.pmi {
            for target in &mut annotation.targets {
                self.target(target);
            }
            match &mut annotation.definition {
                PmiDefinition::DatumTarget { basis, .. } => {
                    for target in basis {
                        self.target(target);
                    }
                }
                PmiDefinition::GeometricTolerance { zones, .. } => {
                    for projection in zones.iter_mut().flat_map(|zone| &mut zone.projections) {
                        for target in &mut projection.boundaries {
                            self.target(target);
                        }
                        self.target(&mut projection.end);
                    }
                }
                PmiDefinition::Datum { .. }
                | PmiDefinition::DatumSystem { .. }
                | PmiDefinition::Dimension(_)
                | PmiDefinition::Presentation { .. } => {}
            }
        }
        for layer in &mut model.presentation_layers {
            for item in &mut layer.items {
                self.target(item);
            }
        }
        for properties in &mut model.mass_properties {
            self.target(&mut properties.target);
        }
        for body in &mut model.rigid_bodies {
            self.target(&mut body.target);
        }
        for collider in &mut model.colliders {
            self.target(&mut collider.target);
        }
        for articulation in &mut model.articulations {
            self.target(&mut articulation.root);
        }
        for settings in &mut model.solver_settings {
            self.target(&mut settings.target);
            for value in settings.values.values_mut() {
                if let SolverValue::Targets { targets } = value {
                    for target in targets {
                        self.target(target);
                    }
                }
            }
        }
        for tessellation in &mut model.tessellations {
            if let Some(state) = &mut tessellation.state {
                self.state_ref(state);
            }
        }
        for measure in &mut model.declared_measures {
            if let DeclaredTarget::State(state) = &mut measure.target {
                self.state_ref(state);
            }
        }
    }

    /// Spells a target's `state` through its member, and clears it where it
    /// names a current state of its scope in the configuration that
    /// `state: None` reads: the configuration in use (`configuration` of
    /// `None`), or the scope's active configuration where the target has no
    /// path or its path's instance selects that configuration (§8).
    fn target<E>(&self, target: &mut InstanceTarget<E>) {
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
        let Some((active, positions)) = self
            .scopes
            .get(&member)
            .and_then(|scope| self.current.get(scope))
        else {
            return;
        };
        let configuration_reads = state.configuration.is_none()
            || (state.configuration == *active
                && target
                    .path
                    .as_ref()
                    .is_none_or(|path| self.selects(path, active.as_ref())));
        if configuration_reads && positions.contains(position) {
            target.state = None;
        }
    }

    /// Whether the instance a path places selects `active`, the active
    /// configuration of the scope that holds the target's state, whatever
    /// configuration the occurrences above it read: its last step states
    /// it, or every statement of the configuration its occurrence uses is
    /// `active`, an unstated base being the prototype's active configuration
    /// (§5.1). A `Named` configuration, a `Native` step, and an occurrence
    /// whose prototype an override replaces are not known to select it.
    fn selects(&self, path: &OccurrencePath, active: Option<&ConfigurationId>) -> bool {
        let is_active = |configuration: &ConfigurationRef| matches!(configuration, ConfigurationRef::Local(id) if Some(id) == active);
        let Some(last) = path.0.last() else {
            return false;
        };
        if let Some(configuration) = &last.configuration {
            return is_active(configuration);
        }
        let StepRef::Occurrence(occurrence) = &last.occurrence else {
            return false;
        };
        self.occurrences.get(occurrence).is_none_or(|uses| {
            !uses.replaced
                && uses
                    .bases
                    .iter()
                    .all(|base| base.as_ref().is_none_or(is_active))
                && uses.overrides.iter().all(is_active)
        })
    }
}

/// Every statement of the configuration each occurrence uses: the base
/// entries of the scopes, and the overrides of every configuration and
/// arrangement.
fn occurrence_uses(design: &Design) -> BTreeMap<OccurrenceId, Uses> {
    let mut uses: BTreeMap<OccurrenceId, Uses> = BTreeMap::new();
    for scope in &design.scopes {
        for (occurrence, entry) in &scope.occurrences {
            uses.entry(occurrence.clone())
                .or_default()
                .bases
                .push(entry.configuration.clone());
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
        for (occurrence, stated) in &overrides.occurrences {
            let entry = uses.entry(occurrence.clone()).or_default();
            entry.replaced |= stated.prototype.is_some();
            entry.overrides.extend(stated.configuration.clone());
        }
    }
    uses
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
