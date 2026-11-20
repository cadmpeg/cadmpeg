// SPDX-License-Identifier: Apache-2.0
//! The resolvers of an entity's or an instance's effective display: its
//! colour, its visibility, and its placement in document coordinates (§4,
//! Shapes; D19, D20, D25).
//!
//! Each occurrence of a path resolves, outermost first, through the
//! configuration that applies to it: the one its parent occurrence selects
//! for the prototype that owns it, else its scope's active configuration.
//! Its suppression, visibility, and placement are its base values resolved
//! through that configuration, then through the arrangements in force in it,
//! then through the nested overrides of the occurrences above it, applied
//! innermost occurrence first so that the outer occurrence's value holds.
//! Its placement is then its scope's current pose for it, where that
//! configuration is the active one (§5.5, §5.11).

use std::borrow::Cow;

use super::EditError;
use crate::appearance::AppearanceTarget;
use crate::design::configuration::{Arrangement, ArrangementRef, OccurrenceOverride, Overrides};
use crate::design::refs::{ConfigurationRef, StateRef};
use crate::design::scope::DesignScope;
use crate::design::selection::{OccurrencePath, StepRef};
use crate::document::{Design, Topology};
use crate::features::ConfigurationId;
use crate::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use crate::presentation::PresentationItem;
use crate::products::{NestedOverride, Occurrence, ProductDefinition};
use crate::references::InstanceTarget;
use crate::topology::Color;
use crate::transform::Transform;
use crate::CadIr;

#[cfg(test)]
mod tests;

/// One occurrence of a path, resolved (§4, §5.11).
#[derive(Debug, Clone, Default, PartialEq)]
struct Resolved {
    transform: Option<Transform>,
    visible: Option<bool>,
}

impl Resolved {
    /// Takes each field an override states that this level does not hold
    /// yet: an earlier entry wins.
    fn take(&mut self, entry: &OccurrenceOverride) {
        self.transform = self.transform.or(entry.transform);
        self.visible = self.visible.or(entry.visible);
    }

    /// As [`Resolved::take`], for a nested override.
    fn take_nested(&mut self, entry: &NestedOverride) {
        self.transform = self.transform.or(entry.transform);
        self.visible = self.visible.or(entry.visible);
    }
}

/// One step of a path being resolved: its occurrence, the scope that states
/// its design, and the configuration of that scope that applies to it.
struct Level<'a> {
    occurrence: &'a OccurrenceId,
    record: &'a Occurrence,
    scope: Option<&'a DesignScope>,
    configuration: Option<ConfigurationId>,
}

/// The records an occurrence path resolves through: the design, and the
/// occurrences and product definitions of the document (§4, §5.1, §5.11).
pub(crate) struct PathResolver<'a> {
    pub(crate) design: &'a Design,
    pub(crate) occurrences: &'a [Occurrence],
    pub(crate) definitions: &'a [ProductDefinition],
}

impl<'a> PathResolver<'a> {
    fn of(ir: &'a CadIr) -> Self {
        Self {
            design: &ir.design,
            occurrences: &ir.model.occurrences,
            definitions: &ir.model.product_definitions,
        }
    }
}

/// The occurrence ids of a path, outermost first; `None` where a step is
/// `Native`, an occurrence the document does not hold.
fn occurrence_ids(path: &OccurrencePath) -> Option<Vec<&OccurrenceId>> {
    path.0
        .iter()
        .map(|step| match &step.occurrence {
            StepRef::Occurrence(occurrence) => Some(occurrence),
            StepRef::Native(_) => None,
        })
        .collect()
}

/// Whether a nested override's path is `below`, by occurrence.
fn names_path(entry: &NestedOverride, below: &[&OccurrenceId]) -> bool {
    occurrence_ids(&entry.path).is_some_and(|ids| ids == below)
}

/// The presentation item that names the same record as an appearance
/// target, where one does.
fn same_record(item: &PresentationItem, entity: &AppearanceTarget) -> bool {
    match (item, entity) {
        (PresentationItem::Body { body }, AppearanceTarget::Body(id)) => body == id,
        (PresentationItem::Face { face }, AppearanceTarget::Face(id)) => face == id,
        (PresentationItem::Edge { edge }, AppearanceTarget::Edge(id)) => edge == id,
        (PresentationItem::Vertex { vertex }, AppearanceTarget::Vertex(id)) => vertex == id,
        (PresentationItem::Point { point }, AppearanceTarget::Point(id)) => point == id,
        (PresentationItem::Curve { curve }, AppearanceTarget::Curve(id)) => curve == id,
        (PresentationItem::Surface { surface }, AppearanceTarget::Surface(id)) => surface == id,
        (PresentationItem::Tessellation { tessellation }, AppearanceTarget::Tessellation(id)) => {
            tessellation.as_str() == id
        }
        _ => false,
    }
}

impl CadIr {
    /// The placement of the instance at `path` in document coordinates: the
    /// placements of its occurrences, outermost first, each resolved as §4
    /// states, composed. `None` where an occurrence states no placement, or a
    /// step is `Native`.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::UnknownConfiguration`] for a configuration the
    /// document does not hold, and [`EditError::ConfigurationCycle`] where a
    /// configuration's parents form a cycle.
    pub fn effective_transform(
        &self,
        path: &OccurrencePath,
    ) -> Result<Option<Transform>, EditError> {
        let Some(resolved) = PathResolver::of(self).resolve_path(path)? else {
            return Ok(None);
        };
        let mut world = Transform::identity();
        for occurrence in resolved {
            let Some(placement) = occurrence.transform else {
                return Ok(None);
            };
            let Ok(composed) = world.compose(placement) else {
                return Ok(None);
            };
            world = composed;
        }
        Ok(Some(world))
    }

    /// Whether `target` is shown: every explicit visibility gate that applies
    /// to it, combined with AND (D20). The gates are the visibility of each
    /// occurrence of its path, resolved as §4 states; the `visible` of the
    /// body that holds it, or of the body itself; the `visible` of each
    /// appearance binding on it or on that body, at its instance and in its
    /// state; and the `visible` of each presentation layer that lists it,
    /// that body, or an occurrence of its path. A gate that states nothing
    /// does not hide it.
    ///
    /// # Errors
    ///
    /// As [`CadIr::effective_transform`], and as
    /// [`CadIr::parameter_cache`] where `target.state` names an evaluation
    /// whose chain of bases does not resolve.
    pub fn effective_visibility(
        &self,
        target: &InstanceTarget<AppearanceTarget>,
    ) -> Result<bool, EditError> {
        if let Some(path) = &target.path {
            if let Some(resolved) = PathResolver::of(self).resolve_path(path)? {
                if resolved
                    .iter()
                    .any(|occurrence| occurrence.visible == Some(false))
                {
                    return Ok(false);
                }
            }
        }
        let topology = self.state_topology(target.state.as_ref())?;
        let body = holding_body(&topology, &target.entity);
        let own = body.as_ref().and_then(|body| {
            topology
                .bodies
                .iter()
                .find(|record| &record.id == body)
                .and_then(|record| record.visible)
        });
        if own == Some(false) {
            return Ok(false);
        }
        let mut entities = vec![target.entity.clone()];
        if let Some(body) = body {
            let whole = AppearanceTarget::Body(body);
            if whole != target.entity {
                entities.push(whole);
            }
        }
        let at_instance = |bound: &InstanceTarget<AppearanceTarget>| {
            entities.contains(&bound.entity)
                && bound.state == target.state
                && (bound.path.is_none() || bound.path == target.path)
        };
        if self
            .model
            .appearance_bindings
            .iter()
            .any(|binding| at_instance(&binding.target) && binding.visible == Some(false))
        {
            return Ok(false);
        }
        let on_path: Vec<OccurrenceId> = target
            .path
            .as_ref()
            .and_then(occurrence_ids)
            .unwrap_or_default()
            .into_iter()
            .cloned()
            .collect();
        let listed = |item: &InstanceTarget<PresentationItem>| match &item.entity {
            PresentationItem::Occurrence { occurrence } => {
                item.state.is_none() && on_path.contains(occurrence)
            }
            entity => {
                entities.iter().any(|named| same_record(entity, named))
                    && item.state == target.state
                    && (item.path.is_none() || item.path == target.path)
            }
        };
        Ok(!self
            .model
            .presentation_layers
            .iter()
            .any(|layer| layer.visible == Some(false) && layer.items.iter().any(listed)))
    }

    /// The display colour of `target`: its own explicit display colour
    /// (`Face.color`, `Body.color`), else the base colour of the appearance
    /// bound to it, a binding at its instance standing before one on every
    /// placement, else its body's colour, resolved the same way (D19).
    /// `None` where none of them states a colour.
    ///
    /// # Errors
    ///
    /// As [`CadIr::effective_visibility`].
    pub fn effective_display_color(
        &self,
        target: &InstanceTarget<AppearanceTarget>,
    ) -> Result<Option<Color>, EditError> {
        let topology = self.state_topology(target.state.as_ref())?;
        let own = |entity: &AppearanceTarget| match entity {
            AppearanceTarget::Body(id) => topology
                .bodies
                .iter()
                .find(|record| &record.id == id)
                .and_then(|record| record.color),
            AppearanceTarget::Face(id) => topology
                .faces
                .iter()
                .find(|record| &record.id == id)
                .and_then(|record| record.color),
            _ => None,
        };
        let bound = |entity: &AppearanceTarget| {
            let mut found = None;
            for binding in &self.model.appearance_bindings {
                let on = &binding.target;
                if &on.entity != entity || on.state != target.state {
                    continue;
                }
                let rank = match &on.path {
                    Some(path) if Some(path) == target.path.as_ref() => 0,
                    None => 1,
                    Some(_) => continue,
                };
                if found.as_ref().is_some_and(|(held, _)| *held <= rank) {
                    continue;
                }
                let color = self
                    .model
                    .appearances
                    .iter()
                    .find(|appearance| appearance.id == binding.appearance)
                    .and_then(|appearance| appearance.base_color);
                found = Some((rank, color));
            }
            found.and_then(|(_, color)| color)
        };
        if let Some(color) = own(&target.entity).or_else(|| bound(&target.entity)) {
            return Ok(Some(color));
        }
        Ok(holding_body(&topology, &target.entity)
            .map(AppearanceTarget::Body)
            .filter(|body| body != &target.entity)
            .and_then(|body| own(&body).or_else(|| bound(&body))))
    }

    /// The topology of a state: the current geometry for `None`, else the
    /// resolved records of the evaluation `state` names, empty where it
    /// names none with content.
    fn state_topology(&self, state: Option<&StateRef>) -> Result<Cow<'_, Topology>, EditError> {
        let Some(state) = state else {
            return Ok(Cow::Borrowed(&self.geometry));
        };
        let Some(evaluation) = self.design.evaluations.iter().find(|evaluation| {
            evaluation.configuration == state.configuration
                && evaluation.of == state.of
                && evaluation.content.is_some()
        }) else {
            return Ok(Cow::Owned(Topology::default()));
        };
        Ok(Cow::Owned(self.resolved(&evaluation.id)?.records))
    }
}

impl PathResolver<'_> {
    /// The scope that states an occurrence's design: the scope of the
    /// definition that owns it, else the scope of the step above it, which
    /// spans that definition (§5.1).
    fn owning_scope<'a>(
        &'a self,
        occurrence: &OccurrenceId,
        above: Option<&'a DesignScope>,
    ) -> Option<&'a DesignScope> {
        crate::products::owner(self.definitions, occurrence)
            .and_then(|definition| self.design.design_scope(&definition.id))
            .or(above)
    }

    /// The configuration of its prototype that an occurrence selects,
    /// resolved in the configuration of its scope that applies to it: the
    /// path step's own, else its `OccurrenceDesign.configuration` resolved
    /// through that configuration and then through the arrangements in
    /// force there (§5.1, §5.11). `None` where it selects none, or a
    /// configuration by name, which the document does not hold.
    fn selected_configuration(
        &self,
        level: &Level<'_>,
        stated: Option<&ConfigurationRef>,
    ) -> Result<Option<ConfigurationId>, EditError> {
        let selected = match (stated, level.scope) {
            (Some(stated), _) => Some(stated.clone()),
            (None, None) => None,
            (None, Some(scope)) => {
                let from = |overrides: &Overrides| {
                    overrides
                        .occurrences
                        .get(level.occurrence)
                        .and_then(|state| state.configuration.clone())
                };
                let arranged = self
                    .in_force_at(level)?
                    .into_iter()
                    .find_map(|arrangement| from(&arrangement.overrides));
                let configured = match &level.configuration {
                    Some(configuration) => self
                        .design
                        .configuration_chain(configuration)?
                        .into_iter()
                        .find_map(|state| from(&state.overrides)),
                    None => None,
                };
                arranged.or(configured).or_else(|| {
                    scope
                        .occurrences
                        .get(level.occurrence)
                        .and_then(|design| design.configuration.clone())
                })
            }
        };
        Ok(match selected {
            Some(ConfigurationRef::Local(configuration)) => Some(configuration),
            Some(ConfigurationRef::Named(_)) | None => None,
        })
    }

    /// The arrangements in force in the configuration of an occurrence's
    /// scope that applies to it.
    fn in_force_at(&self, level: &Level<'_>) -> Result<Vec<&Arrangement>, EditError> {
        let Some(scope) = level.scope else {
            return Ok(Vec::new());
        };
        self.design
            .arrangements_in_force(&scope.definition, level.configuration.as_ref())
    }

    /// Resolves every occurrence of a path, outermost first; `None` where a
    /// step is `Native` or names an occurrence the document does not hold.
    fn resolve_path(&self, path: &OccurrencePath) -> Result<Option<Vec<Resolved>>, EditError> {
        let Some(levels) = self.levels(path)? else {
            return Ok(None);
        };
        let ids: Vec<&OccurrenceId> = levels.iter().map(|level| level.occurrence).collect();
        let mut resolved = Vec::with_capacity(levels.len());
        for at in 0..levels.len() {
            resolved.push(self.resolve_level(&levels, &ids, at)?);
        }
        Ok(Some(resolved))
    }

    /// The definition of the scope that states the design of what the
    /// instance at `path` holds, and the configuration of that scope that
    /// applies to it: its prototype's scope, in the configuration the path's
    /// last occurrence selects for it, else the scope of that occurrence,
    /// which spans the prototype, in that occurrence's configuration (§5.1,
    /// part 9). `None` where the path is empty, a step is `Native` or names
    /// an occurrence the document does not hold, an override in force
    /// replaces the prototype of one of its occurrences, or no scope states
    /// that design.
    ///
    /// # Errors
    ///
    /// As [`CadIr::effective_transform`].
    pub(crate) fn instance_configuration(
        &self,
        path: &OccurrencePath,
    ) -> Result<Option<(ProductDefinitionId, Option<ConfigurationId>)>, EditError> {
        let Some(levels) = self.levels(path)? else {
            return Ok(None);
        };
        for at in 0..levels.len() {
            if self.replaced(&levels, at)? {
                return Ok(None);
            }
        }
        let Some(last) = levels.last() else {
            return Ok(None);
        };
        let Some(scope) = self
            .design
            .design_scope(&last.record.prototype)
            .or(last.scope)
        else {
            return Ok(None);
        };
        let stated = path.0.last().and_then(|step| step.configuration.as_ref());
        let configuration = self.configuration_below(Some(last), Some(scope), stated)?;
        Ok(Some((scope.definition.clone(), configuration)))
    }

    /// Whether an override in force for the occurrence at `at` of a path
    /// whose levels are `levels` replaces its prototype.
    fn replaced(&self, levels: &[Level<'_>], at: usize) -> Result<bool, EditError> {
        let level = &levels[at];
        let replaces = |overrides: &Overrides| {
            overrides
                .occurrences
                .get(level.occurrence)
                .is_some_and(|entry| entry.prototype.is_some())
        };
        if self
            .arranged_for(levels, at)?
            .into_iter()
            .any(|arrangement| replaces(&arrangement.overrides))
        {
            return Ok(true);
        }
        Ok(match &level.configuration {
            Some(configuration) => self
                .design
                .configuration_chain(configuration)?
                .into_iter()
                .any(|state| replaces(&state.overrides)),
            None => false,
        })
    }

    /// The levels of a path, outermost first; `None` where a step is
    /// `Native` or names an occurrence the document does not hold.
    fn levels<'a>(&'a self, path: &'a OccurrencePath) -> Result<Option<Vec<Level<'a>>>, EditError> {
        let Some(ids) = occurrence_ids(path) else {
            return Ok(None);
        };
        let mut levels: Vec<Level<'a>> = Vec::with_capacity(ids.len());
        for (at, occurrence) in ids.into_iter().enumerate() {
            let Some(record) = self
                .occurrences
                .iter()
                .find(|record| &record.id == occurrence)
            else {
                return Ok(None);
            };
            let parent = levels.last();
            let scope = self.owning_scope(occurrence, parent.and_then(|level| level.scope));
            let stated = at
                .checked_sub(1)
                .and_then(|above| path.0[above].configuration.as_ref());
            let configuration = self.configuration_below(parent, scope, stated)?;
            levels.push(Level {
                occurrence,
                record,
                scope,
                configuration,
            });
        }
        Ok(Some(levels))
    }

    /// The configuration of `scope` that applies below `parent`, the
    /// occurrence whose prototype holds what `scope` states, where `parent`'s
    /// path step states `stated`: with no parent, the scope's active
    /// configuration; where `scope` is the prototype's own, the configuration
    /// of it that `parent` selects, else its active one; where `scope` spans
    /// the prototype, `parent`'s own configuration.
    fn configuration_below(
        &self,
        parent: Option<&Level<'_>>,
        scope: Option<&DesignScope>,
        stated: Option<&ConfigurationRef>,
    ) -> Result<Option<ConfigurationId>, EditError> {
        let Some(parent) = parent else {
            return Ok(scope.and_then(|scope| scope.active_configuration.clone()));
        };
        let Some(scope) = scope.filter(|scope| scope.definition == parent.record.prototype) else {
            return Ok(parent.configuration.clone());
        };
        let selected = self.selected_configuration(parent, stated)?;
        Ok(selected
            .filter(|selected| {
                self.design.configurations.iter().any(|candidate| {
                    &candidate.id == selected && candidate.scope == scope.definition
                })
            })
            .or_else(|| scope.active_configuration.clone()))
    }

    /// Resolves the occurrence at `at` of a path whose levels are `levels`.
    fn resolve_level(
        &self,
        levels: &[Level<'_>],
        ids: &[&OccurrenceId],
        at: usize,
    ) -> Result<Resolved, EditError> {
        let level = &levels[at];
        let mut configured = Resolved::default();
        if let Some(configuration) = &level.configuration {
            for state in self.design.configuration_chain(configuration)? {
                if let Some(entry) = state.overrides.occurrences.get(level.occurrence) {
                    configured.take(entry);
                }
            }
        }
        let mut arranged = Resolved::default();
        for arrangement in self.arranged_for(levels, at)? {
            if let Some(entry) = arrangement.overrides.occurrences.get(level.occurrence) {
                arranged.take(entry);
            }
        }
        let nested = self.nested_for(levels, ids, at)?;
        Ok(Resolved {
            transform: self
                .current_pose(levels, ids, at)
                .or(nested.transform)
                .or(arranged.transform)
                .or(configured.transform)
                .or(level.record.transform),
            visible: nested
                .visible
                .or(arranged.visible)
                .or(configured.visible)
                .or(level.record.visible),
        })
    }

    /// The nested overrides that the occurrences above the one at `at` state
    /// for it, per field: for one occurrence above, its configuration's entry
    /// before its parent's resolved entry before its base list's; across
    /// them the outermost occurrence's value holds (§4, §5.11).
    fn nested_for(
        &self,
        levels: &[Level<'_>],
        ids: &[&OccurrenceId],
        at: usize,
    ) -> Result<Resolved, EditError> {
        let mut nested = Resolved::default();
        for above in 0..at {
            let below = &ids[above + 1..=at];
            let outer = &levels[above];
            if let Some(configuration) = &outer.configuration {
                for state in self.design.configuration_chain(configuration)? {
                    let entries = state
                        .overrides
                        .occurrences
                        .get(outer.occurrence)
                        .map(|entry| entry.nested.as_slice())
                        .unwrap_or_default();
                    for entry in entries.iter().filter(|entry| names_path(entry, below)) {
                        nested.take_nested(entry);
                    }
                }
            }
            for entry in outer
                .record
                .overrides
                .iter()
                .filter(|entry| names_path(entry, below))
            {
                nested.take_nested(entry);
            }
        }
        Ok(nested)
    }

    /// The arrangements in force for the occurrence at `at`: per kind, the
    /// one its parent occurrence uses for the prototype where its scope is
    /// that prototype's own, else the one in force in the configuration that
    /// applies to it (§5.1, §5.11).
    fn arranged_for<'a>(
        &'a self,
        levels: &[Level<'a>],
        at: usize,
    ) -> Result<Vec<&'a Arrangement>, EditError> {
        let level = &levels[at];
        let mut in_force: Vec<&Arrangement> = Vec::new();
        if let (Some(parent), Some(scope)) = (at.checked_sub(1).map(|up| &levels[up]), level.scope)
        {
            if scope.definition == parent.record.prototype {
                let named = parent
                    .scope
                    .and_then(|holder| holder.occurrences.get(parent.occurrence))
                    .map(|design| design.arrangements.as_slice())
                    .unwrap_or_default();
                for reference in named {
                    let ArrangementRef::Local(id) = reference else {
                        continue;
                    };
                    if let Some(arrangement) = self.design.arrangements.iter().find(|candidate| {
                        &candidate.id == id && candidate.scope == scope.definition
                    }) {
                        if !in_force.iter().any(|held| held.kind == arrangement.kind) {
                            in_force.push(arrangement);
                        }
                    }
                }
            }
        }
        for arrangement in self.in_force_at(level)? {
            if !in_force.iter().any(|held| held.kind == arrangement.kind) {
                in_force.push(arrangement);
            }
        }
        Ok(in_force)
    }

    /// The pose a current state of the occurrence's scope states for it, by
    /// its path from the scope's definition, where the configuration that
    /// applies to it is the scope's active one (§5.5).
    fn current_pose(
        &self,
        levels: &[Level<'_>],
        ids: &[&OccurrenceId],
        at: usize,
    ) -> Option<Transform> {
        let level = &levels[at];
        let scope = level.scope?;
        if level.configuration != scope.active_configuration {
            return None;
        }
        let from = (0..=at)
            .rev()
            .find(|&start| self.owned_by(ids[start], &scope.definition))?;
        let key = &ids[from..=at];
        scope
            .current
            .iter()
            .flat_map(|current| &current.poses)
            .find(|(path, _)| occurrence_ids(path).is_some_and(|found| found == key))
            .map(|(_, pose)| *pose)
    }

    /// Whether `definition` lists `occurrence` among the occurrences it owns.
    fn owned_by(&self, occurrence: &OccurrenceId, definition: &ProductDefinitionId) -> bool {
        crate::products::owner(self.definitions, occurrence)
            .is_some_and(|owner| &owner.id == definition)
    }
}

/// The body that holds an entity: the body itself, or the body a face, an
/// edge, or a vertex belongs to through the parent pointers and the coedges
/// of `topology`.
fn holding_body(topology: &Topology, entity: &AppearanceTarget) -> Option<BodyId> {
    let of_face = |face: &crate::ids::FaceId| {
        let shell = &topology
            .faces
            .iter()
            .find(|record| &record.id == face)?
            .shell;
        let region = &topology
            .shells
            .iter()
            .find(|record| &record.id == shell)?
            .region;
        topology
            .regions
            .iter()
            .find(|record| &record.id == region)
            .map(|record| record.body.clone())
    };
    let of_edge = |edge: &crate::ids::EdgeId| {
        let coedge = topology
            .coedges
            .iter()
            .find(|record| &record.edge == edge)?;
        let face = &topology
            .loops
            .iter()
            .find(|record| record.id == coedge.owner_loop)?
            .face;
        of_face(face)
    };
    match entity {
        AppearanceTarget::Body(body) => Some(body.clone()),
        AppearanceTarget::Face(face) => of_face(face),
        AppearanceTarget::Edge(edge) => of_edge(edge),
        AppearanceTarget::Vertex(vertex) => topology
            .edges
            .iter()
            .filter(|record| record.vertices().any(|end| end == vertex))
            .find_map(|record| of_edge(&record.id)),
        _ => None,
    }
}
