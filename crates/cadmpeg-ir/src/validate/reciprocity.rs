// SPDX-License-Identifier: Apache-2.0
//! Parent pointers and ordered child lists describe the same topology.

use std::collections::{BTreeMap, BTreeSet};

use crate::report::{
    check::{Check, Finding},
    Severity,
};

/// Check both directions of every topology relationship that stores both.
pub(super) fn check(model: &crate::document::Model, findings: &mut Vec<Finding>) {
    check_pair(
        findings,
        "body",
        "region",
        model.bodies.iter().map(|row| {
            (
                row.id.as_str(),
                row.regions.iter().map(|id| id.as_str()).collect(),
            )
        }),
        model
            .regions
            .iter()
            .map(|row| (row.id.as_str(), row.body.as_str())),
    );
    check_pair(
        findings,
        "region",
        "shell",
        model.regions.iter().map(|row| {
            (
                row.id.as_str(),
                row.shells.iter().map(|id| id.as_str()).collect(),
            )
        }),
        model
            .shells
            .iter()
            .map(|row| (row.id.as_str(), row.region.as_str())),
    );
    check_pair(
        findings,
        "shell",
        "face",
        model.shells.iter().map(|row| {
            (
                row.id.as_str(),
                row.faces().iter().map(|id| id.as_str()).collect(),
            )
        }),
        model
            .faces
            .iter()
            .map(|row| (row.id.as_str(), row.shell.as_str())),
    );
    check_pair(
        findings,
        "face",
        "loop",
        model.faces.iter().map(|row| {
            (
                row.id.as_str(),
                row.loops.iter().map(|id| id.as_str()).collect(),
            )
        }),
        model
            .loops
            .iter()
            .map(|row| (row.id.as_str(), row.face.as_str())),
    );
    check_pair(
        findings,
        "loop",
        "coedge",
        model.loops.iter().map(|row| {
            (
                row.id.as_str(),
                row.coedges().iter().map(|id| id.as_str()).collect(),
            )
        }),
        model
            .coedges
            .iter()
            .map(|row| (row.id.as_str(), row.owner_loop.as_str())),
    );
}

fn check_pair<'a>(
    findings: &mut Vec<Finding>,
    parent_kind: &str,
    child_kind: &str,
    parents: impl Iterator<Item = (&'a str, Vec<&'a str>)>,
    children: impl Iterator<Item = (&'a str, &'a str)>,
) {
    let children: BTreeMap<_, _> = children.collect();
    let mut lists = BTreeMap::new();
    for (parent, members) in parents {
        let mut distinct = BTreeSet::new();
        for child in members {
            if !distinct.insert(child) {
                super::record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    Some(parent),
                    format_args!("{parent_kind} lists {child_kind} `{child}` more than once"),
                );
            }
            if let Some(owner) = children.get(child) {
                if *owner != parent {
                    super::record_related_finding(findings, Check::ReferentialIntegrity, Severity::Error,
                        parent, &[child, owner], format_args!(
                            "{parent_kind} lists {child_kind} `{child}`, but its parent pointer names `{owner}`"));
                }
            }
        }
        lists.insert(parent, distinct);
    }
    for (child, parent) in children {
        // Missing references already have their specific reference finding.
        if lists
            .get(parent)
            .is_some_and(|members| !members.contains(child))
        {
            super::record_related_finding(findings, Check::ReferentialIntegrity, Severity::Error,
                child, &[parent], format_args!(
                    "{child_kind} parent pointer names {parent_kind} `{parent}`, but its child list omits `{child}`"));
        }
    }
}

#[cfg(test)]
mod tests;
