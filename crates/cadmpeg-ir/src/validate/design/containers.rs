// SPDX-License-Identifier: Apache-2.0
//! The container tree and membership (§5.2).

use std::collections::BTreeMap;

use super::records::{members, DesignRecords, MemberKey};
use crate::design::container::{Container, Folder, Rollback};
use crate::report::check::Finding;

pub(super) fn check_containers(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    check_scope_contents(index, findings);
    let design = &index.ir.design;
    let mut listings: BTreeMap<MemberKey<'_>, usize> = BTreeMap::new();
    for container in &design.containers {
        let id = container.id().as_str();
        for member in members(container) {
            let key = MemberKey::of(member);
            *listings.entry(key).or_default() += 1;
            if !index.member_exists(key) {
                super::error(
                    findings,
                    id,
                    format_args!("container lists {key}, which is no record of the design"),
                );
            }
        }
        check_order(container, findings);
    }
    for scope in &design.scopes {
        for root in &scope.roots {
            *listings
                .entry(MemberKey::Container(root.as_str()))
                .or_default() += 1;
            if !index.containers.contains_key(root.as_str()) {
                super::error(
                    findings,
                    scope.definition.as_str(),
                    format_args!("scope root {} is no container of the design", root.as_str()),
                );
            }
        }
    }
    let records = design
        .features
        .iter()
        .map(|feature| MemberKey::Feature(feature.id.as_str()))
        .chain(
            design
                .containers
                .iter()
                .map(|container| MemberKey::Container(container.id().as_str())),
        );
    for key in records {
        let count = listings.get(&key).copied().unwrap_or_default();
        if count != 1 {
            super::error(
                findings,
                key.id(),
                format_args!(
                    "{key} is a member or scope root {count} times; it is a member of exactly one container or a root of exactly one scope"
                ),
            );
        }
    }
    for container in &design.containers {
        let id = container.id().as_str();
        if is_own_ancestor(index, id) {
            super::error(
                findings,
                id,
                format_args!(
                    "container is its own ancestor through membership; containers form a tree"
                ),
            );
        }
    }
}

/// A scope holds a root container that has a member, a parameter, program,
/// sheet-metal rule, configuration, arrangement, or publication that names
/// it, an `occurrences` entry, or a setup that names it (§5.1).
fn check_scope_contents(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let ir = index.ir;
    let design = &ir.design;
    let mut named: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    named.extend(design.parameters.iter().map(|record| record.scope.as_str()));
    named.extend(design.programs.iter().map(|record| record.scope.as_str()));
    named.extend(
        design
            .sheet_metal_rules
            .iter()
            .map(|record| record.scope.as_str()),
    );
    named.extend(
        design
            .configurations
            .iter()
            .map(|record| record.scope.as_str()),
    );
    named.extend(
        design
            .arrangements
            .iter()
            .map(|record| record.scope.as_str()),
    );
    named.extend(
        design
            .publications
            .iter()
            .map(|record| record.scope.as_str()),
    );
    named.extend(ir.process.setups.iter().map(|setup| setup.scope.as_str()));
    for scope in &design.scopes {
        let definition = scope.definition.as_str();
        let filled_root = scope.roots.iter().any(|root| {
            index
                .containers
                .get(root.as_str())
                .is_some_and(|container| !members(container).is_empty())
        });
        if !filled_root && scope.occurrences.is_empty() && !named.contains(definition) {
            super::error(
                findings,
                definition,
                format_args!("design scope holds no member, parameter, program, rule, configuration, arrangement, publication, occurrence design, or setup"),
            );
        }
    }
}

fn is_own_ancestor(index: &DesignRecords<'_>, id: &str) -> bool {
    let mut container = id;
    for _ in 0..index.containers.len() {
        let Some(parent) = index.parents.get(&MemberKey::Container(container)) else {
            return false;
        };
        if *parent == id {
            return true;
        }
        container = parent;
    }
    false
}

/// An ordered container's rollback names one of its members, and its folders
/// are runs of its members that nest or are disjoint.
fn check_order(container: &Container, findings: &mut Vec<Finding>) {
    let Container::Ordered {
        id,
        members,
        folders,
        rollback,
        ..
    } = container
    else {
        return;
    };
    let id = id.as_str();
    if let Some(Rollback::Before(member)) = rollback {
        if !members.contains(member) {
            super::error(
                findings,
                id,
                format_args!(
                    "rollback names {}, which is no member of the container",
                    MemberKey::of(member)
                ),
            );
        }
    }
    let count = members.len();
    for folder in folders {
        let in_range = usize::try_from(folder.last).is_ok_and(|last| last < count);
        if folder.first > folder.last || !in_range {
            super::error(
                findings,
                id,
                format_args!(
                    "folder {}..={} is no run of the container's {count} members",
                    folder.first, folder.last
                ),
            );
        }
    }
    for (at, first) in folders.iter().enumerate() {
        for second in &folders[at + 1..] {
            if overlap_without_nesting(first, second) {
                super::error(
                    findings,
                    id,
                    format_args!(
                        "folders {}..={} and {}..={} neither nest nor are disjoint",
                        first.first, first.last, second.first, second.last
                    ),
                );
            }
        }
    }
}

fn overlap_without_nesting(first: &Folder, second: &Folder) -> bool {
    let overlap = first.first <= second.last && second.first <= first.last;
    let first_holds = first.first <= second.first && second.last <= first.last;
    let second_holds = second.first <= first.first && first.last <= second.last;
    overlap && !first_holds && !second_holds
}

#[cfg(test)]
mod tests;
