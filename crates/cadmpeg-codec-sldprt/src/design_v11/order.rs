// SPDX-License-Identifier: Apache-2.0
//! The timeline's members in regeneration order.
//!
//! The tree walk lists members in Keywords serialization order, with nested
//! nodes before their parent. Regeneration order is the stable topological
//! order of the dependency references, in which unrelated features keep
//! their serialization order (`docs/formats/sldprt.md`, Keywords element
//! order): a feature follows every feature it depends on in the v10 records,
//! in the base history or in a configuration. A folder stays over the run its
//! members form in that order; a folder whose members no longer form one run
//! is dropped.

use std::collections::{BTreeSet, HashMap, HashSet};

use cadmpeg_ir::design::container::Folder;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::report::loss::LossNote;

use super::tree::Timeline;
use crate::loss::SldprtLossCode;

/// Order the members of `timeline` after the members they depend on. A
/// timeline whose dependencies form a cycle keeps its order. Returns the note
/// of the folders dropped.
pub(super) fn regeneration_order(timeline: &mut Timeline, model: &Model) -> Option<LossNote> {
    let order = order(timeline, model)?;
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
fn order(timeline: &Timeline, model: &Model) -> Option<Vec<usize>> {
    let index = timeline
        .features
        .iter()
        .enumerate()
        .map(|(at, feature)| (&feature.id, at))
        .collect::<HashMap<_, _>>();
    let mut edges = HashSet::new();
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
    let count = timeline.features.len();
    let mut outgoing = vec![Vec::new(); timeline.features.len()];
    let mut indegree = vec![0usize; timeline.features.len()];
    for &(source, consumer) in &edges {
        outgoing[source].push(consumer);
        indegree[consumer] += 1;
    }
    let mut ready = (0..count)
        .filter(|&at| indegree[at] == 0)
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(count);
    while let Some(at) = ready.pop_first() {
        order.push(at);
        for &consumer in &outgoing[at] {
            indegree[consumer] -= 1;
            if indegree[consumer] == 0 {
                ready.insert(consumer);
            }
        }
    }
    let moved = order.iter().enumerate().any(|(at, &index)| at != index);
    (order.len() == count && moved).then_some(order)
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
