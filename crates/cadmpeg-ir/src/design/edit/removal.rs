// SPDX-License-Identifier: Apache-2.0
//! Removing a member from its container (§4).
//!
//! Removal deletes the member with what it owns: the members under a
//! container, the sketches, sheets, parameters, and programs their features
//! and containers own, and the overrides keyed by a deleted record. It
//! refuses where a record it keeps names a deleted record by id. It marks
//! `Stale` the states that depend on the removal, deletes the evaluations
//! keyed by a deleted member, and rebases what was based on them. In an
//! ordered container every other position that names a deleted member
//! retargets to the position before the member; in an unordered container
//! the result only a deleted member produced is removed with the model-class
//! records that target only it.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value as Json;

use super::history::{currents, Graph, History, Node, Step};
use super::members::shift_for_remove;
use super::states::{compose, ids, reached, remove};
use super::wire::{clear_resolved, names_deleted, positions, re_anchor, retarget, rewrite, Target};
use super::{malformed, EditError};
use crate::design::container::{Container, Rollback};
use crate::design::evaluation::{StateBase, StateContent, StateDelta};
use crate::design::feature::FeatureOperation;
use crate::design::ids::{ContainerId, EvaluationId};
use crate::design::names::TopoName;
use crate::design::parameter::ParameterOwner;
use crate::design::refs::{Evaluated, Member, Position, StateRef};
use crate::features::FeatureId;
use crate::ids::{BodyId, ProductDefinitionId};
use crate::process::state::{ProcessBase, ProcessContent};
use crate::CadIr;

/// Lists the model-class arenas of the `model` home.
macro_rules! model_class_arenas {
    ($($field:ident: $ty:ty, $class:ident, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        &[$((stringify!($field), stringify!($class), stringify!($home))),*]
    };
}

/// Whether a `Model` arena holds model-class records, as opposed to the
/// design records of the earlier layer.
fn model_class(arena: &str) -> bool {
    const ARENAS: &[(&str, &str, &str)] =
        crate::document::classified_arena_registry!(model_class_arenas);
    ARENAS
        .iter()
        .any(|(field, class, home)| *field == arena && *class == "model" && *home == "model")
}

/// What a removal deletes from the design.
struct Removal {
    /// The features among the removed member and the members under it.
    features: BTreeSet<FeatureId>,
    /// The id of every design record it deletes, evaluations aside.
    records: BTreeSet<String>,
    /// Every position that names a deleted member, or the `Start` of a
    /// deleted container.
    positions: Vec<Position>,
}

impl Removal {
    fn names(&self, at: &Position) -> bool {
        self.positions.contains(at)
    }
}

/// Whether a name mentions a record of `records`.
fn mentions(name: &TopoName, records: &BTreeSet<String>) -> Result<bool, EditError> {
    let json = serde_json::to_value(name).map_err(malformed)?;
    Ok(names_deleted(&json, records, &BTreeSet::new()))
}

/// The wire form of every record of every arena of a home, with its arena.
fn arena_records<T: Serialize>(home: &T) -> Result<Vec<(String, Json)>, EditError> {
    let json = serde_json::to_value(home).map_err(malformed)?;
    let mut found = Vec::new();
    if let Json::Object(arenas) = json {
        for (arena, records) in arenas {
            if let Json::Array(records) = records {
                found.extend(records.into_iter().map(|record| (arena.clone(), record)));
            }
        }
    }
    Ok(found)
}

/// How a referring record is reported: its id, a scope's definition, or a
/// name entry's entity.
fn label(arena: &str, record: &Json) -> String {
    let named = record
        .get("id")
        .or_else(|| record.get("definition"))
        .or_else(|| record.get(0).and_then(|entity| entity.get("value")))
        .and_then(Json::as_str);
    named.map_or_else(|| arena.to_owned(), str::to_owned)
}

impl CadIr {
    /// What removing `member` deletes from the design.
    fn removal(&self, history: &History, member: &Member) -> Removal {
        let members: BTreeSet<Member> = history.members_under(member).into_iter().collect();
        let mut features = BTreeSet::new();
        let mut containers: BTreeSet<ContainerId> = BTreeSet::new();
        for member in &members {
            match member {
                Member::Feature(feature) => {
                    features.insert(feature.clone());
                }
                Member::Container(container) => {
                    containers.insert(container.clone());
                }
            }
        }
        let design = &self.design;
        let mut records: BTreeSet<String> = features
            .iter()
            .map(|feature| feature.as_str().to_owned())
            .chain(
                containers
                    .iter()
                    .map(|container| container.as_str().to_owned()),
            )
            .collect();
        let sketches: BTreeSet<String> = design
            .sketches
            .iter()
            .filter(|sketch| features.contains(&sketch.owner))
            .map(|sketch| sketch.id.as_str().to_owned())
            .chain(
                design
                    .spatial_sketches
                    .iter()
                    .filter(|sketch| features.contains(&sketch.owner))
                    .map(|sketch| sketch.id.as_str().to_owned()),
            )
            .collect();
        let owned = |sketch: &str| sketches.contains(sketch);
        records.extend(
            design
                .sketch_entities
                .iter()
                .filter(|entity| owned(entity.sketch.as_str()))
                .map(|entity| entity.id.as_str().to_owned()),
        );
        records.extend(
            design
                .sketch_constraints
                .iter()
                .filter(|constraint| owned(constraint.sketch.as_str()))
                .map(|constraint| constraint.id.as_str().to_owned()),
        );
        records.extend(
            design
                .spatial_sketch_entities
                .iter()
                .filter(|entity| owned(entity.sketch.as_str()))
                .map(|entity| entity.id.as_str().to_owned()),
        );
        records.extend(
            design
                .spatial_sketch_constraints
                .iter()
                .filter(|constraint| owned(constraint.sketch.as_str()))
                .map(|constraint| constraint.id.as_str().to_owned()),
        );
        records.extend(sketches.iter().cloned());
        records.extend(design.features.iter().filter_map(|feature| {
            match &feature.definition.operation {
                FeatureOperation::Spreadsheet(sheet) if features.contains(&feature.id) => {
                    Some(sheet.as_str().to_owned())
                }
                _ => None,
            }
        }));
        records.extend(
            design
                .parameters
                .iter()
                .filter(|parameter| match &parameter.owner {
                    Some(ParameterOwner::Feature(feature)) => features.contains(feature),
                    Some(ParameterOwner::Container(container)) => containers.contains(container),
                    Some(ParameterOwner::Attribute(_)) | None => false,
                })
                .map(|parameter| parameter.id.as_str().to_owned()),
        );
        records.extend(
            design
                .programs
                .iter()
                .filter(|program| {
                    program
                        .owner
                        .as_ref()
                        .is_some_and(|owner| features.contains(owner))
                })
                .map(|program| program.id.as_str().to_owned()),
        );
        let positions = members
            .iter()
            .map(|member| Position::After(member.clone()))
            .chain(
                containers
                    .iter()
                    .map(|container| Position::Start(container.clone())),
            )
            .collect();
        Removal {
            features,
            records,
            positions,
        }
    }

    /// [`Editor::remove_member`](super::Editor::remove_member) on the
    /// document itself.
    pub(super) fn remove_member(&mut self, member: &Member) -> Result<(), EditError> {
        let history = History::new(&self.design);
        let step = history
            .path(member)
            .and_then(<[Step]>::last)
            .cloned()
            .ok_or_else(|| EditError::NotAMember(member.clone()))?;
        let removal = self.removal(&history, member);
        let stale = if step.ordered {
            self.features_after_edit(&history, member)?
        } else {
            Graph::new(&self.design, &history)?
                .dependents(
                    &history,
                    &currents(&self.design),
                    removal.features.iter().cloned().map(Node::Feature),
                )
                .features
        };
        self.mark_states_stale(&history, &stale);
        let before = if step.ordered {
            Some(match step.index.checked_sub(1) {
                Some(previous) => {
                    let (_, members) = history
                        .members(&step.container)
                        .ok_or_else(|| EditError::NotAMember(member.clone()))?;
                    Position::After(members[previous].clone())
                }
                None => {
                    Position::Start(ContainerId::mint(step.container.clone()).map_err(malformed)?)
                }
            })
        } else {
            None
        };
        let own = Position::After(member.clone());
        let doomed: Vec<(ProductDefinitionId, Position)> = self
            .design
            .scopes
            .iter()
            .flat_map(|scope| {
                scope
                    .current
                    .iter()
                    .map(move |current| (scope.definition.clone(), current.at.clone()))
            })
            .filter(|(_, at)| removal.names(at) && (before.is_none() || *at != own))
            .collect();
        for (scope, at) in &doomed {
            self.rebase_off_current(scope, at)?;
        }
        let keyed: Vec<(EvaluationId, StateRef)> = self
            .design
            .evaluations
            .iter()
            .filter(|evaluation| match &evaluation.of {
                Evaluated::State(at) => removal.names(at),
                Evaluated::Tool(feature) => removal.features.contains(feature),
            })
            .map(|evaluation| {
                (
                    evaluation.id.clone(),
                    StateRef {
                        configuration: evaluation.configuration.clone(),
                        of: evaluation.of.clone(),
                    },
                )
            })
            .collect();
        let mut gone = removal.records.clone();
        let mut states = Vec::new();
        for (id, state) in keyed {
            self.delete_evaluation(&id)?;
            gone.insert(id.as_str().to_owned());
            states.push(serde_json::to_value(state).map_err(malformed)?);
        }
        let lost = self.delete_current_states(&doomed)?;
        let from = positions(&removal.positions)?;
        match &before {
            Some(before) => {
                let to = serde_json::to_value(before).map_err(malformed)?;
                rewrite(&mut self.design, |json| Ok(retarget(json, &from, &to)))?;
                rewrite(&mut self.process, |json| Ok(retarget(json, &from, &to)))?;
                rewrite(&mut self.model, |json| Ok(retarget(json, &from, &to)))?;
            }
            None => {
                rewrite(&mut self.design, |json| Ok(re_anchor(json, &from)))?;
                rewrite(&mut self.process, |json| Ok(re_anchor(json, &from)))?;
            }
        }
        let stored: BTreeSet<String> = if step.ordered {
            BTreeSet::new()
        } else {
            self.design
                .features
                .iter()
                .filter(|feature| removal.features.contains(&feature.id))
                .filter_map(|feature| match &feature.definition.operation {
                    FeatureOperation::Stored { tessellations, .. } => Some(tessellations),
                    _ => None,
                })
                .flatten()
                .map(|tessellation| tessellation.as_str().to_owned())
                .collect()
        };
        gone.extend(self.delete_model_targets(&lost, &states, &stored)?);
        let cleared: BTreeSet<String> = lost.union(&removal.records).cloned().collect();
        rewrite(&mut self.design, |json| Ok(clear_resolved(json, &cleared)))?;
        rewrite(&mut self.process, |json| Ok(clear_resolved(json, &cleared)))?;
        self.clean_names(&removal.records)?;
        self.delete_records(&removal);
        self.leave_container(member, &step)?;
        let referring = self.referring(&gone, &lost)?;
        if !referring.is_empty() {
            return Err(EditError::Referenced(referring));
        }
        Ok(())
    }

    /// Rebases every evaluation and process state based on the current
    /// state at `at` onto `Empty`, its delta composed with that state's
    /// extent, so its resolved state is unchanged.
    fn rebase_off_current(
        &mut self,
        scope: &ProductDefinitionId,
        at: &Position,
    ) -> Result<(), EditError> {
        let Some(current) = self.current_state(scope, at) else {
            return Ok(());
        };
        let extent = self.extent(scope, current)?.delta();
        let based = StateBase::Current(at.clone());
        let empty = BTreeSet::new();
        for evaluation in &mut self.design.evaluations {
            if &evaluation.scope != scope {
                continue;
            }
            if let Some(StateContent { base, delta }) = &mut evaluation.content {
                if *base == based {
                    *delta = compose(&extent, delta, &empty);
                    *base = StateBase::Empty;
                }
            }
        }
        for state in &mut self.process.states {
            if let Some(ProcessContent { base, delta }) = &mut state.content {
                if *base == ProcessBase::Design(based.clone()) {
                    *delta = compose(&extent, delta, &empty);
                    *base = ProcessBase::Design(StateBase::Empty);
                }
            }
        }
        Ok(())
    }

    /// Deletes an evaluation, and rebases every evaluation and process state
    /// based on it onto its base, composing the two deltas, so their resolved
    /// states are unchanged.
    fn delete_evaluation(&mut self, id: &EvaluationId) -> Result<(), EditError> {
        let Some(at) = self
            .design
            .evaluations
            .iter()
            .position(|evaluation| &evaluation.id == id)
        else {
            return Ok(());
        };
        let removed = self.design.evaluations.remove(at);
        let Some(content) = removed.content else {
            return Ok(());
        };
        let base_ids = ids(&self.base_state(&removed.scope, &content.base)?.records);
        let based = StateBase::Evaluation(id.clone());
        for evaluation in &mut self.design.evaluations {
            let Some(StateContent { base, delta }) = &mut evaluation.content else {
                continue;
            };
            if *base == based {
                *delta = compose(&content.delta, delta, &base_ids);
                base.clone_from(&content.base);
            }
        }
        for state in &mut self.process.states {
            let Some(ProcessContent { base, delta }) = &mut state.content else {
                continue;
            };
            if *base == ProcessBase::Design(based.clone()) {
                *delta = compose(&content.delta, delta, &base_ids);
                *base = ProcessBase::Design(content.base.clone());
            }
        }
        Ok(())
    }

    /// Deletes the current states at `doomed` with the part of their extents
    /// that no other body of the current geometry holds: their bodies and
    /// the records only they reach, the name, owner, and instance-body
    /// entries of those, and their poses. Returns the deleted records' ids.
    fn delete_current_states(
        &mut self,
        doomed: &[(ProductDefinitionId, Position)],
    ) -> Result<BTreeSet<String>, EditError> {
        if doomed.is_empty() {
            return Ok(BTreeSet::new());
        }
        let mut leaving_bodies: Vec<BodyId> = Vec::new();
        for scope in &mut self.design.scopes {
            scope.current.retain(|current| {
                let deleted = doomed
                    .iter()
                    .any(|(definition, at)| *definition == scope.definition && *at == current.at);
                if deleted {
                    leaving_bodies.extend(current.bodies.iter().cloned());
                }
                !deleted
            });
        }
        let staying: Vec<BodyId> = self
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .filter(|body| !leaving_bodies.contains(body))
            .collect();
        let kept = reached(&self.geometry, &staying)?;
        let leaving: BTreeSet<String> = reached(&self.geometry, &leaving_bodies)?
            .into_iter()
            .filter(|record| !kept.contains(record))
            .collect();
        remove(&mut self.geometry, &leaving);
        let mut names = self.design.names.entries().clone();
        names.retain(|entity, _| !leaving.contains(entity.id()));
        self.design.names = names.into();
        for definition in &mut self.model.product_definitions {
            definition
                .bodies
                .retain(|body| !leaving.contains(body.as_str()));
            for entry in &mut definition.instance_bodies {
                entry.bodies.retain(|body| !leaving.contains(body.as_str()));
            }
            definition
                .instance_bodies
                .retain(|entry| !entry.bodies.is_empty());
        }
        Ok(leaving)
    }

    /// Deletes each model-class record all of whose targets lie in the
    /// deleted extent (`lost`) or in a deleted evaluation (`states`), and
    /// each tessellation of `stored`; removes those targets from each
    /// presentation layer's items. Returns the deleted records' ids.
    fn delete_model_targets(
        &mut self,
        lost: &BTreeSet<String>,
        states: &[Json],
        stored: &BTreeSet<String>,
    ) -> Result<BTreeSet<String>, EditError> {
        let mut gone = BTreeSet::new();
        if lost.is_empty() && states.is_empty() && stored.is_empty() {
            return Ok(gone);
        }
        rewrite(&mut self.model, |json| {
            let mut changed = false;
            let Some(arenas) = json.as_object_mut() else {
                return Ok(false);
            };
            for (arena, records) in arenas.iter_mut() {
                if !model_class(arena) {
                    continue;
                }
                let Some(records) = records.as_array_mut() else {
                    continue;
                };
                if arena == "presentation_layers" {
                    for layer in records.iter_mut() {
                        let Some(items) = layer.get_mut("items").and_then(Json::as_array_mut)
                        else {
                            continue;
                        };
                        let count = items.len();
                        items.retain(|item| {
                            !item
                                .as_object()
                                .and_then(super::wire::target)
                                .is_some_and(|target| target.deleted(lost, states))
                        });
                        changed |= items.len() != count;
                    }
                    continue;
                }
                records.retain(|record| {
                    let id = record.get("id").and_then(Json::as_str).unwrap_or_default();
                    let found = if arena == "tessellations" {
                        let mut ids: Vec<String> = record
                            .get("body")
                            .and_then(Json::as_str)
                            .map(str::to_owned)
                            .into_iter()
                            .collect();
                        ids.extend(
                            record
                                .get("faces")
                                .and_then(Json::as_array)
                                .into_iter()
                                .flatten()
                                .filter_map(Json::as_str)
                                .map(str::to_owned),
                        );
                        vec![Target {
                            ids,
                            state: record
                                .get("state")
                                .filter(|state| !state.is_null())
                                .cloned(),
                        }]
                    } else {
                        super::wire::targets(record)
                    };
                    let deleted = stored.contains(id)
                        || (!found.is_empty()
                            && found.iter().all(|target| target.deleted(lost, states)));
                    if deleted {
                        gone.insert(id.to_owned());
                        changed = true;
                    }
                    !deleted
                });
            }
            Ok(changed)
        })?;
        Ok(gone)
    }

    /// In the current geometry's table and in every kept state's table, a
    /// name entry whose name mentions a deleted record loses the name and
    /// keeps its `native`; an entry left with neither is removed (§5.7).
    fn clean_names(&mut self, records: &BTreeSet<String>) -> Result<(), EditError> {
        let mut table = BTreeMap::new();
        for (entity, entry) in self.design.names.entries() {
            let mut entry = entry.clone();
            if let Some(name) = &entry.name {
                if mentions(name, records)? {
                    entry.name = None;
                }
            }
            if entry.name.is_some() || entry.native.is_some() {
                table.insert(entity.clone(), entry);
            }
        }
        self.design.names = table.into();
        let deltas =
            self.design
                .evaluations
                .iter_mut()
                .filter_map(|evaluation| {
                    evaluation
                        .content
                        .as_mut()
                        .map(|content| &mut content.delta)
                })
                .chain(
                    self.process.states.iter_mut().filter_map(|state| {
                        state.content.as_mut().map(|content| &mut content.delta)
                    }),
                );
        for delta in deltas {
            clean_delta_names(delta, records)?;
        }
        Ok(())
    }

    /// Deletes the removal's design records, and every override keyed by
    /// one of them.
    fn delete_records(&mut self, removal: &Removal) {
        let gone = |id: &str| removal.records.contains(id);
        let design = &mut self.design;
        design
            .containers
            .retain(|container| !gone(container.id().as_str()));
        design.features.retain(|feature| !gone(feature.id.as_str()));
        design.sketches.retain(|sketch| !gone(sketch.id.as_str()));
        design
            .sketch_entities
            .retain(|entity| !gone(entity.id.as_str()));
        design
            .sketch_constraints
            .retain(|constraint| !gone(constraint.id.as_str()));
        design
            .spatial_sketches
            .retain(|sketch| !gone(sketch.id.as_str()));
        design
            .spatial_sketch_entities
            .retain(|entity| !gone(entity.id.as_str()));
        design
            .spatial_sketch_constraints
            .retain(|constraint| !gone(constraint.id.as_str()));
        design
            .parameters
            .retain(|parameter| !gone(parameter.id.as_str()));
        design.spreadsheets.retain(|sheet| !gone(sheet.id.as_str()));
        design.programs.retain(|program| !gone(program.id.as_str()));
        let overrides = design
            .configurations
            .iter_mut()
            .map(|configuration| &mut configuration.overrides)
            .chain(
                design
                    .arrangements
                    .iter_mut()
                    .map(|arrangement| &mut arrangement.overrides),
            );
        for overrides in overrides {
            overrides
                .parameters
                .retain(|parameter, _| !gone(parameter.as_str()));
            overrides
                .features
                .retain(|feature, _| !gone(feature.as_str()));
        }
        for scope in &mut design.scopes {
            for occurrence in scope.occurrences.values_mut() {
                occurrence
                    .parameters
                    .retain(|parameter, _| !gone(parameter.as_str()));
            }
        }
    }

    /// Takes the member out of its container. In an ordered container a
    /// rollback before it moves to the member after it, or to `End` where it
    /// is the last member, and a folder loses it; one left with no member is
    /// removed.
    fn leave_container(&mut self, member: &Member, step: &Step) -> Result<(), EditError> {
        let record = self
            .design
            .containers
            .iter_mut()
            .find(|candidate| candidate.id().as_str() == step.container)
            .ok_or_else(|| EditError::NotAMember(member.clone()))?;
        match record {
            Container::Ordered {
                members,
                folders,
                rollback,
                ..
            } => {
                if *rollback == Rollback::Before(member.clone()) {
                    *rollback = match members.get(step.index + 1) {
                        Some(next) => Rollback::Before(next.clone()),
                        None => Rollback::End,
                    };
                }
                members.remove(step.index);
                let at = u32::try_from(step.index).map_err(malformed)?;
                folders.retain_mut(|folder| shift_for_remove(folder, at));
            }
            Container::Unordered { members, .. } => {
                members.remove(member);
            }
        }
        Ok(())
    }

    /// The kept records that name a deleted record by id (`gone`), or a
    /// record of the deleted part of the current geometry (`lost`).
    fn referring(
        &self,
        gone: &BTreeSet<String>,
        lost: &BTreeSet<String>,
    ) -> Result<Vec<String>, EditError> {
        let mut found = BTreeSet::new();
        let none = BTreeSet::new();
        for (arena, record) in arena_records(&self.design)?
            .into_iter()
            .chain(arena_records(&self.process)?)
        {
            if names_deleted(&record, gone, &none) {
                found.insert(label(&arena, &record));
            }
        }
        for (arena, record) in arena_records(&self.model)? {
            if model_class(&arena) && names_deleted(&record, gone, lost) {
                found.insert(label(&arena, &record));
            }
        }
        Ok(found.into_iter().collect())
    }
}

/// Cleans the names a state delta sets: a name mentioning a deleted record
/// goes, and an entry left with neither name nor `native` removes the
/// entity's entry.
fn clean_delta_names(delta: &mut StateDelta, records: &BTreeSet<String>) -> Result<(), EditError> {
    for entry in delta.names.values_mut() {
        let Some(set) = entry else {
            continue;
        };
        if let Some(name) = &set.name {
            if mentions(name, records)? {
                set.name = None;
            }
        }
        if set.name.is_none() && set.native.is_none() {
            *entry = None;
        }
    }
    Ok(())
}
