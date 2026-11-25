// SPDX-License-Identifier: Apache-2.0
//! The occurrence membership of states (§5.4, §6.7): an occurrence that a
//! feature creates exists in a state where its creating feature is at or
//! before the state's position, and in every state of its definition where
//! that feature is a member of an unordered container. An occurrence no
//! feature creates exists in every state.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::design::refs::Evaluated;
use crate::design::selection::{OccurrencePath, StepRef};
use crate::ids::ProductDefinitionId;
use crate::report::check::Finding;

use super::index::{DesignIndex, StateAt};
use super::records::MemberKey;
use super::selections::moved_occurrences;
use super::states::States;

/// Member entries and pose paths of a state name occurrences existing in it;
/// a resolved state's instance-body and pose paths resolve their first step
/// against the state's membership.
pub(super) fn check(index: &DesignIndex<'_>, states: &States<'_>, findings: &mut Vec<Finding>) {
    let ir = index.records.ir;
    let creators: BTreeMap<&str, Vec<&str>> = index
        .records
        .occurrence_namers()
        .into_iter()
        .map(|(occurrence, namers)| {
            let features = namers.into_iter().map(|(feature, _, _)| feature);
            (occurrence, features.collect())
        })
        .collect();
    let exists = |scope: &str, features: &HashSet<&str>, occurrence: &str| {
        creators.get(occurrence).is_none_or(|creating| {
            creating.iter().any(|&feature| {
                let key = MemberKey::Feature(feature);
                features.contains(feature)
                    || !index.in_ordered(key)
                    || index.scope_of(key).map(ProductDefinitionId::as_str) != Some(scope)
            })
        })
    };
    for scope in &ir.design.scopes {
        let id = scope.definition.as_str();
        for current in scope
            .current
            .iter()
            .filter(|current| !current.poses.is_empty())
        {
            let Some(features) = index.state_features(StateAt::Position(&current.at)) else {
                continue;
            };
            for path in current.poses.keys() {
                check_pose(path, |o| exists(id, &features, o), id, findings);
            }
        }
    }
    let moved = moved_occurrences(ir);
    for evaluation in &ir.design.evaluations {
        let Evaluated::State(position) = &evaluation.of else {
            continue;
        };
        if evaluation.content.is_none() {
            continue;
        }
        let Some(features) = index.state_features(StateAt::Position(position)) else {
            continue;
        };
        let at = evaluation.id.as_str();
        let scope = evaluation.scope.as_str();
        let members = states.members(at);
        for &occurrence in members.keys() {
            if !exists(scope, &features, occurrence) {
                super::error(
                    findings,
                    at,
                    crate::rule!(
                        "design.member-entry-occurrence-not-exist-state",
                        "member entry names {occurrence}, which does not exist in the state"
                    ),
                );
            }
        }
        let poses = states.pose_paths(at);
        for path in &poses {
            check_pose(path, |o| exists(scope, &features, o), at, findings);
        }
        let paths: BTreeSet<&OccurrencePath> =
            poses.into_iter().chain(states.instance_paths(at)).collect();
        for occurrence in paths.into_iter().filter_map(first_occurrence) {
            if !moved.contains(occurrence) {
                continue;
            }
            let owner = members
                .get(occurrence)
                .map(|owner| owner.as_str())
                .or_else(|| index.records.occurrence_owner(occurrence));
            if let Some(owner) = owner.filter(|&owner| owner != scope) {
                super::error(
                    findings,
                    at,
                    crate::rule!(
                        "design.state-path-step-not-occurrence-membership",
                        "a path of the state begins at {occurrence}, which the state's membership places in {owner}"
                    ),
                );
            }
        }
    }
}

/// A pose path's first occurrence exists in the state.
fn check_pose(
    path: &OccurrencePath,
    exists: impl Fn(&str) -> bool,
    at: &str,
    findings: &mut Vec<Finding>,
) {
    if let Some(occurrence) = first_occurrence(path).filter(|&o| !exists(o)) {
        super::error(
            findings,
            at,
            crate::rule!(
                "design.pose-names-instance-not-exist-state",
                "a pose names an instance of {occurrence}, which does not exist in the state"
            ),
        );
    }
}

/// The occurrence a path's first step names.
fn first_occurrence(path: &OccurrencePath) -> Option<&str> {
    match &path.0.first()?.occurrence {
        StepRef::Occurrence(occurrence) => Some(occurrence.as_str()),
        StepRef::Native(_) => None,
    }
}

#[cfg(test)]
mod tests;
