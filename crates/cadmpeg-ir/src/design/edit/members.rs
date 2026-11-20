// SPDX-License-Identifier: Apache-2.0
//! Inserting a new member into a container (§4).
//!
//! Insertion adds a member with the records it owns, and marks `Stale` every
//! evaluation and current state at or after the member's position, and every
//! one that depends on a feature at or after it.

use std::collections::BTreeSet;

use serde_json::Value as Json;

use super::history::{currents, Graph, History, Node};
use super::{malformed, EditError};
use crate::design::container::{Container, Folder};
use crate::design::feature::FeatureOperation;
use crate::design::ids::ContainerId;
use crate::design::parameter::ParameterOwner;
use crate::design::refs::Member;
use crate::document::Design;
use crate::CadIr;

/// The id of every record of a design's arenas.
pub(super) fn design_ids(design: &Design) -> Result<BTreeSet<String>, EditError> {
    let json = serde_json::to_value(design).map_err(malformed)?;
    let mut found = BTreeSet::new();
    for records in json
        .as_object()
        .into_iter()
        .flat_map(|arenas| arenas.values())
    {
        for record in records.as_array().into_iter().flatten() {
            if let Some(id) = record.get("id").and_then(Json::as_str) {
                found.insert(id.to_owned());
            }
        }
    }
    Ok(found)
}

/// The one member `records` adds: the feature or container that no
/// container of `records` holds. Every other record is one of its members,
/// or is owned by one of them.
fn new_member(records: &Design) -> Result<Member, EditError> {
    let refuse = |reason: String| Err(EditError::NotOneMember(reason));
    let others = [
        ("scopes", records.scopes.is_empty()),
        ("sheet_metal_rules", records.sheet_metal_rules.is_empty()),
        ("configurations", records.configurations.is_empty()),
        ("arrangements", records.arrangements.is_empty()),
        ("publications", records.publications.is_empty()),
        ("evaluations", records.evaluations.is_empty()),
        ("names", records.names.entries().is_empty()),
    ];
    if let Some((arena, _)) = others.iter().find(|(_, empty)| !empty) {
        return refuse(format!("{arena} hold records no member owns"));
    }
    let features: BTreeSet<&str> = records
        .features
        .iter()
        .map(|feature| feature.id.as_str())
        .collect();
    let containers: BTreeSet<&str> = records
        .containers
        .iter()
        .map(|container| container.id().as_str())
        .collect();
    let mut held = BTreeSet::new();
    for container in &records.containers {
        for member in super::container_members(container) {
            let known = match member {
                Member::Feature(feature) => features.contains(feature.as_str()),
                Member::Container(inner) => containers.contains(inner.as_str()),
            };
            if !known {
                return refuse(format!(
                    "{member:?} is a member of a container and not a record"
                ));
            }
            held.insert(member.clone());
        }
    }
    let mut tops = records
        .features
        .iter()
        .map(|feature| Member::Feature(feature.id.clone()))
        .chain(
            records
                .containers
                .iter()
                .map(|container| Member::Container(container.id().clone())),
        )
        .filter(|member| !held.contains(member));
    let (Some(member), None) = (tops.next(), tops.next()) else {
        return refuse("not exactly one feature or container is in no container".into());
    };
    let sheets: BTreeSet<&str> = records
        .features
        .iter()
        .filter_map(|feature| match &feature.definition.operation {
            FeatureOperation::Spreadsheet(sheet) => Some(sheet.as_str()),
            _ => None,
        })
        .collect();
    let sketches: BTreeSet<&str> = records
        .sketches
        .iter()
        .filter(|sketch| features.contains(sketch.owner.as_str()))
        .map(|sketch| sketch.id.as_str())
        .chain(
            records
                .spatial_sketches
                .iter()
                .filter(|sketch| features.contains(sketch.owner.as_str()))
                .map(|sketch| sketch.id.as_str()),
        )
        .collect();
    let unowned = records.sketches.len() + records.spatial_sketches.len() != sketches.len()
        || records
            .sketch_entities
            .iter()
            .any(|entity| !sketches.contains(entity.sketch.as_str()))
        || records
            .sketch_constraints
            .iter()
            .any(|constraint| !sketches.contains(constraint.sketch.as_str()))
        || records
            .spatial_sketch_entities
            .iter()
            .any(|entity| !sketches.contains(entity.sketch.as_str()))
        || records
            .spatial_sketch_constraints
            .iter()
            .any(|constraint| !sketches.contains(constraint.sketch.as_str()))
        || records
            .spreadsheets
            .iter()
            .any(|sheet| !sheets.contains(sheet.id.as_str()))
        || records
            .parameters
            .iter()
            .any(|parameter| match &parameter.owner {
                Some(ParameterOwner::Feature(feature)) => !features.contains(feature.as_str()),
                Some(ParameterOwner::Container(container)) => {
                    !containers.contains(container.as_str())
                }
                Some(ParameterOwner::Attribute(_)) | None => true,
            })
        || records.programs.iter().any(|program| {
            !program
                .owner
                .as_ref()
                .is_some_and(|feature| features.contains(feature.as_str()))
        });
    if unowned {
        return refuse("a record is owned by no feature or container of the records".into());
    }
    Ok(member)
}

impl CadIr {
    /// The features whose states change with an edit at a member's position:
    /// the features at or after it and every feature that depends on one.
    pub(super) fn features_after_edit(
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

    /// [`Editor::insert_member`](super::Editor::insert_member) on the
    /// document itself.
    pub(super) fn insert_member(
        &mut self,
        container: &ContainerId,
        index: usize,
        records: Design,
    ) -> Result<(), EditError> {
        let member = new_member(&records)?;
        let taken = design_ids(&self.design)?;
        if let Some(id) = design_ids(&records)?
            .into_iter()
            .find(|id| taken.contains(id))
        {
            return Err(EditError::DuplicateId(id));
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
        let design = &mut self.design;
        design.containers.extend(records.containers);
        design.features.extend(records.features);
        design.sketches.extend(records.sketches);
        design.sketch_entities.extend(records.sketch_entities);
        design.sketch_constraints.extend(records.sketch_constraints);
        design.spatial_sketches.extend(records.spatial_sketches);
        design
            .spatial_sketch_entities
            .extend(records.spatial_sketch_entities);
        design
            .spatial_sketch_constraints
            .extend(records.spatial_sketch_constraints);
        design.parameters.extend(records.parameters);
        design.spreadsheets.extend(records.spreadsheets);
        design.programs.extend(records.programs);
        design.finalize();
        let history = History::new(&self.design);
        let features = self.features_after_edit(&history, &member)?;
        self.mark_states_stale(&history, &features);
        Ok(())
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
pub(super) fn shift_for_remove(folder: &mut Folder, at: u32) -> bool {
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
