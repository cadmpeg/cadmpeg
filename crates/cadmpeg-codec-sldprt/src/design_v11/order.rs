// SPDX-License-Identifier: Apache-2.0
//! The timeline's members in regeneration order.
//!
//! The tree walk lists members in Keywords serialization order, with nested
//! nodes before their parent. The active partition's persistent marks order
//! its recorded features. Dependency references order the other members,
//! with serialization order breaking ties. A folder stays over the run its
//! members form in that order; a folder whose members no longer form one run
//! is dropped.

use std::collections::{BTreeSet, HashMap, HashSet};

use cadmpeg_ir::design::container::Folder;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::report::loss::LossNote;

use super::tree::Timeline;
use crate::loss::SldprtLossCode;

/// Apply the active partition's regeneration order and the dependency
/// references. Returns the note of the folders that no longer form a run.
pub(super) fn regeneration_order(
    timeline: &mut Timeline,
    model: &Model,
    marked: &[FeatureId],
) -> Option<LossNote> {
    let order = order(timeline, model, marked)?;
    let mut position = vec![0; order.len()];
    for (at, &index) in order.iter().enumerate() {
        position[index] = at;
    }
    let mut slots = std::mem::take(&mut timeline.features)
        .into_iter()
        .map(Some)
        .collect::<Vec<_>>();
    timeline.features = order
        .iter()
        .filter_map(|&index| slots[index].take())
        .collect();
    let mut dropped = Vec::new();
    let folders = std::mem::take(&mut timeline.folders)
        .into_iter()
        .zip(std::mem::take(&mut timeline.folder_records))
        .zip(std::mem::take(&mut timeline.folder_children));
    for ((folder, record), children) in folders {
        match run(&folder, &position) {
            Some((first, last)) => {
                timeline.folders.push(Folder {
                    first,
                    last,
                    ..folder
                });
                timeline.folder_records.push(record);
                timeline.folder_children.push(children);
            }
            None => dropped.push(folder.name.unwrap_or_default()),
        }
    }
    (!dropped.is_empty()).then(|| {
        SldprtLossCode::DesignFolderDropped.note(format!(
            "{} feature folders hold members that regeneration order does not keep together: {}",
            dropped.len(),
            dropped.join(", ")
        ))
    })
}

/// The member indices in regeneration order, or `None` where that is the
/// present order or the dependencies form a cycle.
fn order(timeline: &Timeline, model: &Model, marked: &[FeatureId]) -> Option<Vec<usize>> {
    let index = timeline
        .features
        .iter()
        .enumerate()
        .map(|(at, feature)| (&feature.id, at))
        .collect::<HashMap<_, _>>();
    let marks = marked
        .iter()
        .filter_map(|id| index.get(id).copied())
        .collect::<Vec<_>>();
    let marked_edges = marks
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .collect::<HashSet<_>>();
    let mut edges = marked_edges.clone();
    for feature in &model.features {
        let Some(&consumer) = index.get(&feature.id) else {
            continue;
        };
        let configured = model
            .configurations
            .iter()
            .filter_map(|configuration| configuration.feature_states.get(&feature.id))
            .flat_map(|state| &state.dependencies);
        let sources = feature
            .dependencies
            .iter()
            .chain(configured)
            .filter_map(|dependency| index.get(dependency).copied());
        for source in sources {
            if source != consumer {
                edges.insert((source, consumer));
            }
        }
    }
    let serialized = (0..timeline.features.len()).collect::<Vec<_>>();
    let order = sorted(&edges, &serialized).or_else(|| {
        if marked_edges.is_empty() {
            return None;
        }
        let dependencies = edges.difference(&marked_edges).copied().collect();
        let preferred = sorted(&dependencies, &serialized).unwrap_or_else(|| serialized.clone());
        let mut priority = vec![0; preferred.len()];
        for (rank, index) in preferred.into_iter().enumerate() {
            priority[index] = rank;
        }
        sorted(&marked_edges, &priority)
    })?;
    let moved = order.iter().enumerate().any(|(at, &index)| at != index);
    moved.then_some(order)
}

fn sorted(edges: &HashSet<(usize, usize)>, priority: &[usize]) -> Option<Vec<usize>> {
    let mut outgoing = vec![Vec::new(); priority.len()];
    let mut indegree = vec![0usize; priority.len()];
    for &(source, consumer) in edges {
        outgoing[source].push(consumer);
        indegree[consumer] += 1;
    }
    let mut ready = (0..priority.len())
        .filter(|&at| indegree[at] == 0)
        .map(|at| (priority[at], at))
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(priority.len());
    while let Some((_, at)) = ready.pop_first() {
        order.push(at);
        for &consumer in &outgoing[at] {
            indegree[consumer] -= 1;
            if indegree[consumer] == 0 {
                ready.insert((priority[consumer], consumer));
            }
        }
    }
    (order.len() == priority.len()).then_some(order)
}

/// The run that the members of `folder` form after the reorder, if they form
/// one.
fn run(folder: &Folder, position: &[usize]) -> Option<(u32, u32)> {
    let first = usize::try_from(folder.first).ok()?;
    let last = usize::try_from(folder.last).ok()?;
    let moved = position.get(first..=last)?;
    let low = *moved.iter().min()?;
    let high = *moved.iter().max()?;
    if high - low != last - first {
        return None;
    }
    Some((u32::try_from(low).ok()?, u32::try_from(high).ok()?))
}
