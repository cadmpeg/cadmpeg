// SPDX-License-Identifier: Apache-2.0
//! The positions of §5.3 and the instance-target `state` of §8.
//!
//! `After(Container(c))` of an ordered container `c` is the state at `c`'s
//! rollback position, so an evaluation's `of`, a state base (of an
//! evaluation or of a process state), a current state, and a `StateRef` name
//! that state through its member: `After` the nearest feature of `c` before
//! the rollback, else `Start(c)`. A target into a state that `state: None`
//! reaches has `state: None` (§8): with no path, a current state of its
//! scope in the active configuration; with a path, a current state's
//! position in the configuration of that scope that the path's instance
//! selects, which is the current geometry where that configuration is the
//! active one and that configuration's evaluation there otherwise. Where the
//! document does not establish what `None` reaches, the target keeps its
//! `state`, and validation reports neither spelling.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::declared_measures::DeclaredTarget;
use crate::design::container::{Container, Rollback};
use crate::design::edit::effective::PathResolver;
use crate::design::evaluation::StateBase;
use crate::design::ids::ContainerId;
use crate::design::refs::{Evaluated, Member, Position, StateRef};
use crate::design::selection::OccurrencePath;
use crate::document::{CadIr, Design, Model, Process};
use crate::features::ConfigurationId;
use crate::ids::ProductDefinitionId;
use crate::physics::solver::SolverValue;
use crate::pmi::{PmiDefinition, PmiTarget};
use crate::process::state::ProcessBase;
use crate::references::{InstanceTarget, ReferenceTarget};

/// What the position and target rules read of the design, gathered before
/// any record changes.
#[derive(Debug, Default)]
pub(crate) struct States {
    /// The position each ordered container's `After(Container(c))` names,
    /// by the container's id.
    rollbacks: BTreeMap<String, Position>,
    /// The scope that holds each member, by its definition's id.
    scopes: BTreeMap<Member, String>,
    /// The active configuration of each scope and the positions of its
    /// current states, in their one spelling.
    current: BTreeMap<String, (Option<ConfigurationId>, Vec<Position>)>,
    /// Whether every arrangement that a scope or a configuration puts in
    /// force is an arrangement of that scope.
    arrangements_resolve: bool,
}

/// Visits every instance target of the model-class records (§8): the
/// targets of appearance bindings, attributes, PMI, presentation layers,
/// physics and solver records, and the `Entity` arm of drawing and semantic
/// annotation references. `$visit` takes the id of the record that holds
/// the target, its path, and its `state`, borrowed with `$mutability`;
/// `$definition_targets` lists the targets of a PMI definition with the same
/// borrow.
macro_rules! instance_targets {
    ($model:expr, $visit:ident, $definition_targets:ident $(, $mutability:tt)?) => {{
        let model = $model;
        for record in & $($mutability)? model.appearance_bindings {
            $visit(record.id.as_str(), record.target.path.as_ref(), & $($mutability)? record.target.state);
        }
        for record in & $($mutability)? model.attributes {
            $visit(record.id.as_str(), record.target.path.as_ref(), & $($mutability)? record.target.state);
        }
        for annotation in & $($mutability)? model.pmi {
            let owner = annotation.id.as_str();
            for target in & $($mutability)? annotation.targets {
                $visit(owner, target.path.as_ref(), & $($mutability)? target.state);
            }
            for target in $definition_targets(& $($mutability)? annotation.definition) {
                $visit(owner, target.path.as_ref(), & $($mutability)? target.state);
            }
        }
        for layer in & $($mutability)? model.presentation_layers {
            let owner = layer.id.as_str();
            for item in & $($mutability)? layer.items {
                $visit(owner, item.path.as_ref(), & $($mutability)? item.state);
            }
        }
        for record in & $($mutability)? model.mass_properties {
            $visit(record.id.as_str(), record.target.path.as_ref(), & $($mutability)? record.target.state);
        }
        for record in & $($mutability)? model.rigid_bodies {
            $visit(record.id.as_str(), record.target.path.as_ref(), & $($mutability)? record.target.state);
        }
        for record in & $($mutability)? model.colliders {
            $visit(record.id.as_str(), record.target.path.as_ref(), & $($mutability)? record.target.state);
        }
        for record in & $($mutability)? model.articulations {
            $visit(record.id.as_str(), record.root.path.as_ref(), & $($mutability)? record.root.state);
        }
        for settings in & $($mutability)? model.solver_settings {
            let owner = settings.id.as_str();
            $visit(owner, settings.target.path.as_ref(), & $($mutability)? settings.target.state);
            for (_, value) in & $($mutability)? settings.values {
                if let SolverValue::Targets { targets } = value {
                    for target in targets {
                        $visit(owner, target.path.as_ref(), & $($mutability)? target.state);
                    }
                }
            }
        }
        for drawing in & $($mutability)? model.drawings {
            let owner = drawing.id.as_str();
            for (_, selections) in & $($mutability)? drawing.relationships {
                for selection in selections {
                    if let ReferenceTarget::Entity(target) = & $($mutability)? selection.target {
                        $visit(owner, target.path.as_ref(), & $($mutability)? target.state);
                    }
                }
            }
        }
        for annotation in & $($mutability)? model.semantic_annotations {
            let owner = annotation.id.as_str();
            for (_, selections) in & $($mutability)? annotation.references {
                for selection in selections {
                    if let ReferenceTarget::Entity(target) = & $($mutability)? selection.target {
                        $visit(owner, target.path.as_ref(), & $($mutability)? target.state);
                    }
                }
            }
        }
    }};
}

/// Visits every instance target of `model` (see [`instance_targets`]).
pub(crate) fn each_target(
    model: &Model,
    mut visit: impl FnMut(&str, Option<&OccurrencePath>, &Option<StateRef>),
) {
    instance_targets!(model, visit, definition_targets);
}

/// Visits every instance target of `model`, its `state` mutably.
fn each_target_mut(
    model: &mut Model,
    mut visit: impl FnMut(&str, Option<&OccurrencePath>, &mut Option<StateRef>),
) {
    instance_targets!(model, visit, definition_targets_mut, mut);
}

/// The instance targets a PMI definition holds: a datum target's basis and
/// the boundaries and end of each tolerance-zone projection.
fn definition_targets(definition: &PmiDefinition) -> Vec<&InstanceTarget<PmiTarget>> {
    match definition {
        PmiDefinition::DatumTarget { basis, .. } => basis.iter().collect(),
        PmiDefinition::GeometricTolerance { zones, .. } => zones
            .iter()
            .flat_map(|zone| &zone.projections)
            .flat_map(|projection| projection.boundaries.iter().chain([&projection.end]))
            .collect(),
        PmiDefinition::Datum { .. }
        | PmiDefinition::DatumSystem { .. }
        | PmiDefinition::Dimension(_)
        | PmiDefinition::Presentation { .. } => Vec::new(),
    }
}

/// [`definition_targets`], borrowed mutably.
fn definition_targets_mut(definition: &mut PmiDefinition) -> Vec<&mut InstanceTarget<PmiTarget>> {
    match definition {
        PmiDefinition::DatumTarget { basis, .. } => basis.iter_mut().collect(),
        PmiDefinition::GeometricTolerance { zones, .. } => zones
            .iter_mut()
            .flat_map(|zone| &mut zone.projections)
            .flat_map(|projection| {
                projection
                    .boundaries
                    .iter_mut()
                    .chain([&mut projection.end])
            })
            .collect(),
        PmiDefinition::Datum { .. }
        | PmiDefinition::DatumSystem { .. }
        | PmiDefinition::Dimension(_)
        | PmiDefinition::Presentation { .. } => Vec::new(),
    }
}

/// What `state: None` reaches for an instance target of one document.
pub(crate) struct Reach<'a> {
    states: &'a States,
    ir: &'a CadIr,
    resolver: PathResolver<'a>,
    /// The definitions each scope spans (§5.1), by the scope's definition,
    /// as far as they were read.
    spans: RefCell<HashMap<String, BTreeSet<String>>>,
}

impl<'a> Reach<'a> {
    pub(crate) fn new(states: &'a States, ir: &'a CadIr) -> Self {
        Self {
            states,
            ir,
            resolver: PathResolver::of(ir),
            spans: RefCell::new(HashMap::new()),
        }
    }

    /// Whether a target at `path` reaches the state `state` names, spelled
    /// through its member, with `state: None` (§8): `state` names a current
    /// state's position in the scope that holds it, and its configuration
    /// (where it states none, the scope's active one) is the configuration
    /// that applies there: the scope's active one with no path, else the one
    /// that the path's instance selects (see [`Reach::shown`]).
    pub(crate) fn reached(&self, path: Option<&OccurrencePath>, state: &StateRef) -> bool {
        let Evaluated::State(position) = &state.of else {
            return false;
        };
        let member = match position {
            Position::Start(container) => Member::Container(container.clone()),
            Position::After(member) => member.clone(),
        };
        let Some((holder, (active, positions))) = self
            .states
            .scopes
            .get(&member)
            .and_then(|scope| Some((scope, self.states.current.get(scope)?)))
        else {
            return false;
        };
        if !positions.contains(position) {
            return false;
        }
        let named = state.configuration.as_ref().or(active.as_ref());
        match path {
            None => named == active.as_ref(),
            Some(path) => self.shows(path, holder, named),
        }
    }

    /// Whether the instance at `path` shows `configuration` of scope
    /// `holder`, where the document establishes what it shows: every
    /// arrangement put in force resolves, the path resolves as
    /// [`PathResolver::selected`] states, and the prototype in force at its
    /// last occurrence is `holder`'s definition, or a definition that
    /// `holder`'s scope spans and that has no scope of its own.
    fn shows(
        &self,
        path: &OccurrencePath,
        holder: &str,
        configuration: Option<&ConfigurationId>,
    ) -> bool {
        if !self.states.arrangements_resolve {
            return false;
        }
        let Some(selected) = self.resolver.selected(path) else {
            return false;
        };
        selected.scope.definition.as_str() == holder
            && (selected.prototype == &selected.scope.definition
                || self.spans(&selected.scope.definition, selected.prototype))
            && selected.configuration.as_ref() == configuration
    }

    /// Whether the scope of `scope` spans `definition`.
    fn spans(&self, scope: &ProductDefinitionId, definition: &ProductDefinitionId) -> bool {
        self.spans
            .borrow_mut()
            .entry(scope.as_str().to_owned())
            .or_insert_with(|| {
                self.ir
                    .spanned_definitions(scope)
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            })
            .contains(definition.as_str())
    }
}

impl States {
    pub(crate) fn new(design: &Design) -> Self {
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
        let arrangements_resolve = design
            .scopes
            .iter()
            .flat_map(|scope| {
                scope
                    .active_arrangements
                    .iter()
                    .map(move |id| (id, &scope.definition))
            })
            .chain(design.configurations.iter().flat_map(|configuration| {
                configuration
                    .active_arrangements
                    .iter()
                    .map(move |id| (id, &configuration.scope))
            }))
            .all(|(id, scope)| {
                design
                    .arrangements
                    .iter()
                    .any(|arrangement| &arrangement.id == id && &arrangement.scope == scope)
            });
        let mut states = Self {
            rollbacks,
            scopes: BTreeMap::new(),
            current: BTreeMap::new(),
            arrangements_resolve,
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

    pub(crate) fn state_ref(&self, state: &mut StateRef) {
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
    /// clears each instance target's `state` that `state: None` reaches
    /// (see [`Reach::reached`]).
    pub(super) fn normalize_model(&self, ir: &mut CadIr) {
        if self.rollbacks.is_empty() && self.current.is_empty() {
            return;
        }
        each_target_mut(&mut ir.model, |_, _, state| {
            if let Some(state) = state {
                self.state_ref(state);
            }
        });
        for tessellation in &mut ir.model.tessellations {
            if let Some(state) = &mut tessellation.state {
                self.state_ref(state);
            }
        }
        for measure in &mut ir.model.declared_measures {
            if let DeclaredTarget::State(state) = &mut measure.target {
                self.state_ref(state);
            }
        }
        let reached = {
            let reach = Reach::new(self, ir);
            let mut reached = Vec::new();
            each_target(&ir.model, |_, path, state| {
                reached.push(
                    state
                        .as_ref()
                        .is_some_and(|state| reach.reached(path, state)),
                );
            });
            reached
        };
        let mut reached = reached.into_iter();
        each_target_mut(&mut ir.model, |_, _, state| {
            if reached.next() == Some(true) {
                *state = None;
            }
        });
    }
}

/// The position an ordered container's `After(Container(c))` names: the
/// state entering the member at its rollback position, which is `After` the
/// nearest member before it that is a feature, or `Start(c)` where no feature
/// of `c` precedes it (§5.3). A member that is a container is passed over:
/// the state leaving it is the state entering it. `None` for an unordered
/// container, for an unstated rollback, whose one spelling is
/// `After(Container(c))` itself, and for a rollback that names no member of
/// its container.
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
        None => return None,
        Some(Rollback::End) => members.len(),
        Some(Rollback::Before(member)) => members.iter().position(|listed| listed == member)?,
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
