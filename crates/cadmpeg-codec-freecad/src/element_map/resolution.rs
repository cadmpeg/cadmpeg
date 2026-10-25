// SPDX-License-Identifier: Apache-2.0
//! Resolve current canonical names without source reads or neutral backlinks.

use crate::native::element_map::{ElementMapGroup, ElementMapNodes};
use crate::topology_transfer::TopologyOccurrence;

pub(crate) fn topology_ids<'a>(
    nodes: &'a ElementMapNodes,
    group: &'a ElementMapGroup,
    occurrences: &'a [TopologyOccurrence],
    property: &'a str,
    source_index: usize,
) -> impl Iterator<Item = &'a str> {
    let mapped = has_name(nodes, group, source_index);
    occurrences
        .iter()
        .filter(move |occurrence| {
            mapped
                && occurrence.property == property
                && occurrence.indexed_name == group.indexed_name
                && occurrence.source_index == source_index
        })
        .map(|occurrence| occurrence.topology_id.as_str())
        .scan(std::collections::BTreeSet::new(), |seen, id| {
            Some(seen.insert(id).then_some(id))
        })
        .flatten()
}

pub(crate) fn topology_indices<'a>(
    nodes: &'a ElementMapNodes,
    group: &'a ElementMapGroup,
    occurrences: &'a [TopologyOccurrence],
    property: &'a str,
) -> impl Iterator<Item = usize> + 'a {
    occurrences
        .iter()
        .filter(move |occurrence| {
            occurrence.property == property
                && occurrence.indexed_name == group.indexed_name
                && has_name(nodes, group, occurrence.source_index)
        })
        .map(|occurrence| occurrence.source_index)
        .scan(std::collections::BTreeSet::new(), |seen, index| {
            Some(seen.insert(index).then_some(index))
        })
        .flatten()
}

pub(crate) fn topology_index_count(group: &ElementMapGroup) -> usize {
    let names = match group.names.len() {
        0 => 0,
        length => length - 1,
    };
    group
        .children
        .iter()
        .flatten()
        .filter_map(|child| {
            let fields = child.split_ascii_whitespace().collect::<Vec<_>>();
            let index = fields.first()?.parse::<usize>().ok()?;
            let offset = fields.get(1)?.parse::<usize>().ok()?;
            let count = fields.get(2)?.parse::<usize>().ok()?;
            if count == 0 {
                return None;
            }
            index
                .checked_add(offset)?
                .checked_add(count)?
                .checked_sub(1)
        })
        .fold(names, usize::max)
}

fn has_name<'a>(
    nodes: &'a ElementMapNodes,
    mut group: &'a ElementMapGroup,
    mut index: usize,
) -> bool {
    let mut node_index = nodes.slot_count();
    loop {
        match group.names.get(index) {
            Some(None) => return false,
            Some(Some(chain)) if !chain.is_empty() => return true,
            _ => {}
        }
        // FreeCAD selects children.upper_bound(index), keyed by end index.
        // Equal end keys use the last authored descriptor, as std::map does.
        let mut selected = None;
        for child in group.children.iter().flatten() {
            let fields = child.split_ascii_whitespace().collect::<Vec<_>>();
            let parsed = fields
                .first()
                .and_then(|value| value.parse::<usize>().ok())
                .zip(fields.get(1).and_then(|value| value.parse::<usize>().ok()))
                .zip(fields.get(2).and_then(|value| value.parse::<usize>().ok()))
                .zip(fields.get(4).and_then(|value| value.parse::<usize>().ok()));
            let Some((((local, offset), count), target)) = parsed else {
                continue;
            };
            let Some(start) = local.checked_add(offset) else {
                continue;
            };
            let Some(end) = start.checked_add(count) else {
                continue;
            };
            if end > index && selected.as_ref().is_none_or(|(best, _, _, _)| end <= *best) {
                selected = Some((end, start, offset, target));
            }
        }
        let Some((_, start, offset, target)) = selected else {
            return false;
        };
        if index < start {
            return false;
        }
        if target == 0 {
            return true;
        }
        if target >= node_index {
            return false;
        }
        let Some(node) = nodes.node(target) else {
            return false;
        };
        let mut groups = node
            .groups
            .iter()
            .flatten()
            .filter(|child| child.indexed_name == group.indexed_name);
        let Some(child) = groups.next() else {
            return false;
        };
        if groups.next().is_some() {
            return false;
        }
        group = child;
        index -= offset;
        node_index = target;
    }
}
