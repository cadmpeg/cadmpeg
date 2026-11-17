// SPDX-License-Identifier: Apache-2.0
//! Inserting a member into a container and removing one (§4).
//!
//! Both mark `Stale` every evaluation and current state at or after the
//! member's position, and every one that depends on a feature at or after
//! it. Removing member `m` also deletes the evaluations keyed by `m` and
//! rebases those based on them, retargets every other position that names
//! `m` to the position before it, and moves a rollback before `m` to the
//! member after it.

use std::collections::BTreeSet;

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::history::{currents, Graph, History, Node, Step};
use super::states::{compose, ids};
use super::wire::{names_after, retarget, rewrite};
use super::{malformed, EditError};
use crate::design::container::{Container, Folder, Rollback};
use crate::design::evaluation::{StateBase, StateContent};
use crate::design::ids::{ContainerId, EvaluationId};
use crate::design::refs::{Evaluated, Member, Position};
use crate::CadIr;

impl CadIr {
    /// The features whose states change with an edit at a member's position:
    /// the features at or after it and every feature that depends on one.
    fn features_after_edit(
        &self,
        history: &History,
        member: &Member,
    ) -> Result<BTreeSet<crate::features::FeatureId>, EditError> {
        let graph = Graph::new(&self.design, history)?;
        let seeds = history.features_from(member).into_iter().map(Node::Feature);
        Ok(graph
            .dependents(history, &currents(&self.design), seeds)
            .features)
    }

    /// Inserts `member` into `container`, at `index` where the container is
    /// ordered, and marks `Stale` every evaluation and current state at or
    /// after its position, including every one that depends on a feature at
    /// or after it. A folder that holds the index grows to hold the member,
    /// and a later folder moves with the members after it.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::UnknownContainer`] for a container the design
    /// does not hold, [`EditError::AlreadyPlaced`] for a member already in a
    /// container or a scope root, and [`EditError::IndexOutOfRange`] for an
    /// index past the end of an ordered container.
    pub fn insert_member(
        &mut self,
        container: &ContainerId,
        index: usize,
        member: Member,
    ) -> Result<(), EditError> {
        let history = History::new(&self.design)?;
        let is_root = match &member {
            Member::Container(id) => self
                .design
                .scopes
                .iter()
                .any(|scope| scope.roots.contains(id)),
            Member::Feature(_) => false,
        };
        if is_root || history.path(&member).is_some() {
            return Err(EditError::AlreadyPlaced(member));
        }
        let record = self
            .design
            .containers
            .iter_mut()
            .find(|candidate| candidate.id() == container)
            .ok_or_else(|| EditError::UnknownContainer(container.clone()))?;
        match record {
            Container::Ordered {
                members, folders, ..
            } => {
                if index > members.len() {
                    return Err(EditError::IndexOutOfRange {
                        container: container.clone(),
                        index,
                    });
                }
                let at = u32::try_from(index).map_err(malformed)?;
                for folder in folders.iter_mut() {
                    shift_for_insert(folder, at)?;
                }
                members.insert(index, member.clone());
            }
            Container::Unordered { members, .. } => {
                members.insert(member.clone());
            }
        }
        let history = History::new(&self.design)?;
        let features = self.features_after_edit(&history, &member)?;
        self.mark_states_stale(&history, &features);
        Ok(())
    }

    /// Removes `member` from its container. It marks `Stale` every evaluation
    /// and current state at or after the member's position, and every one
    /// that depends on a feature at or after it; deletes the evaluations
    /// keyed by the member (`of` is `State(After(m))` or `Tool(m)`) and
    /// rebases those based on them so their resolved states are unchanged;
    /// retargets every other position that names the member (a
    /// `Pinned(After(m))` anchor, a current state at `After(m)`, a
    /// `StateRef`) to the position before it; and retargets
    /// `Rollback::Before(m)` to the member after it, or to `End` where it is
    /// the last member. A folder loses the member, and one left empty is
    /// removed. The member's own records stay in their arenas.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::NotAMember`] for a member in no container, and
    /// [`EditError::NoPositionBefore`] where a position names a member of an
    /// unordered container, which has no position before it.
    pub fn remove_member(&mut self, member: &Member) -> Result<(), EditError> {
        let history = History::new(&self.design)?;
        let step = history
            .path(member)
            .and_then(<[Step]>::last)
            .cloned()
            .ok_or_else(|| EditError::NotAMember(member.clone()))?;
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
        let keyed = |of: &Evaluated| match (of, member) {
            (Evaluated::State(Position::After(at)), _) => at == member,
            (Evaluated::Tool(feature), Member::Feature(removed)) => feature == removed,
            (Evaluated::Tool(_), Member::Container(_)) => false,
            (Evaluated::State(Position::Start(_)), _) => false,
        };
        if before.is_none() && self.names_member(member, &keyed)? {
            return Err(EditError::NoPositionBefore(member.clone()));
        }
        let features = self.features_after_edit(&history, member)?;
        self.mark_states_stale(&history, &features);
        let deleted: Vec<EvaluationId> = self
            .design
            .evaluations
            .iter()
            .filter(|evaluation| keyed(&evaluation.of))
            .map(|evaluation| evaluation.id.clone())
            .collect();
        for id in deleted {
            self.delete_evaluation(&id)?;
        }
        if let Some(before) = &before {
            self.retarget_positions(member, before)?;
        }
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

    /// Deletes an evaluation, and rebases every evaluation based on it onto
    /// its base, composing the two deltas, so their resolved states are
    /// unchanged.
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
        for evaluation in &mut self.design.evaluations {
            let Some(StateContent { base, delta }) = &mut evaluation.content else {
                continue;
            };
            if *base == StateBase::Evaluation(id.clone()) {
                *delta = compose(&content.delta, delta, &base_ids);
                base.clone_from(&content.base);
            }
        }
        Ok(())
    }

    /// Whether any record of the design, other than an evaluation keyed by
    /// the member, names `After(member)`.
    fn names_member(
        &self,
        member: &Member,
        keyed: &dyn Fn(&Evaluated) -> bool,
    ) -> Result<bool, EditError> {
        fn any<'a, T: Serialize + 'a>(
            records: impl IntoIterator<Item = &'a T>,
            member: &Member,
        ) -> Result<bool, EditError> {
            for record in records {
                if names_after(&serde_json::to_value(record).map_err(malformed)?, member)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        let design = &self.design;
        Ok(any(&design.features, member)?
            || any(&design.sketches, member)?
            || any(&design.sketch_entities, member)?
            || any(&design.sketch_constraints, member)?
            || any(&design.spatial_sketches, member)?
            || any(&design.spatial_sketch_entities, member)?
            || any(&design.spatial_sketch_constraints, member)?
            || any(&design.parameters, member)?
            || any(&design.spreadsheets, member)?
            || any(&design.sheet_metal_rules, member)?
            || any(&design.programs, member)?
            || any(&design.configurations, member)?
            || any(&design.arrangements, member)?
            || any(&design.publications, member)?
            || any(&design.scopes, member)?
            || any(
                design
                    .evaluations
                    .iter()
                    .filter(|evaluation| !keyed(&evaluation.of)),
                member,
            )?)
    }

    /// Replaces every `After(member)` position of the design with `to`.
    fn retarget_positions(&mut self, member: &Member, to: &Position) -> Result<(), EditError> {
        fn all<T: Serialize + DeserializeOwned>(
            records: &mut [T],
            member: &Member,
            to: &Position,
        ) -> Result<(), EditError> {
            for record in records {
                rewrite(record, |json| retarget(json, member, to))?;
            }
            Ok(())
        }
        let design = &mut self.design;
        all(&mut design.features, member, to)?;
        all(&mut design.sketches, member, to)?;
        all(&mut design.sketch_entities, member, to)?;
        all(&mut design.sketch_constraints, member, to)?;
        all(&mut design.spatial_sketches, member, to)?;
        all(&mut design.spatial_sketch_entities, member, to)?;
        all(&mut design.spatial_sketch_constraints, member, to)?;
        all(&mut design.parameters, member, to)?;
        all(&mut design.spreadsheets, member, to)?;
        all(&mut design.sheet_metal_rules, member, to)?;
        all(&mut design.programs, member, to)?;
        all(&mut design.configurations, member, to)?;
        all(&mut design.arrangements, member, to)?;
        all(&mut design.publications, member, to)?;
        all(&mut design.scopes, member, to)?;
        all(&mut design.evaluations, member, to)
    }
}

/// Moves a folder for a member inserted at index `at`: a folder that holds
/// the index grows, and a later one moves.
fn shift_for_insert(folder: &mut Folder, at: u32) -> Result<(), EditError> {
    let next = |value: u32| {
        value
            .checked_add(1)
            .ok_or_else(|| malformed("a folder index overflows"))
    };
    if folder.first >= at {
        folder.first = next(folder.first)?;
        folder.last = next(folder.last)?;
    } else if folder.last >= at {
        folder.last = next(folder.last)?;
    }
    Ok(())
}

/// Moves a folder for the member removed at index `at`, and reports whether
/// it still holds a member.
fn shift_for_remove(folder: &mut Folder, at: u32) -> bool {
    if folder.first > at {
        folder.first -= 1;
        folder.last -= 1;
        true
    } else if folder.last >= at {
        if folder.first == folder.last {
            return false;
        }
        folder.last -= 1;
        true
    } else {
        true
    }
}
