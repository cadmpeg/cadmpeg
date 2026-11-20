// SPDX-License-Identifier: Apache-2.0
use super::{HistoricalRecordArchive, Record, Token};
use crate::history_records::AsmHistoricalTransition;
use std::collections::{BTreeMap, BTreeSet};

/// Edge record chunks: start vertex, end vertex, coedge, and curve.
const EDGE_START_VERTEX: usize = 3;
const EDGE_END_VERTEX: usize = 5;
const EDGE_COEDGE: usize = 7;
const EDGE_CURVE: usize = 8;

pub(crate) fn unchanged(
    transition: &AsmHistoricalTransition,
    older: &BTreeMap<i64, i64>,
    newer: &BTreeMap<i64, i64>,
    archive: &HistoricalRecordArchive,
) -> bool {
    let topology = &transition.topology;
    let families = [
        &topology.bodies,
        &topology.regions,
        &topology.shells,
        &topology.faces,
        &topology.loops,
        &topology.coedges,
        &topology.edges,
        &topology.vertices,
        &topology.surfaces,
        &topology.curves,
        &topology.pcurves,
        &topology.points,
    ];
    let record = |versions: &BTreeMap<i64, i64>, slot: &i64| {
        versions
            .get(slot)
            .and_then(|revision| archive.get(revision))
    };
    let same = |slot: &i64| {
        record(older, slot)
            .zip(record(newer, slot))
            .is_some_and(|(before, after)| same_content(before, after))
    };
    let Some(free_edges) = inserted_saved_free_edges(transition, newer, archive) else {
        return false;
    };
    transition.previous_state_id.is_some()
        && families.iter().all(|family| {
            family.inserted.iter().all(|slot| free_edges.contains(slot))
                && family.deleted.is_empty()
                && family.updated.iter().all(same)
        })
        && transition
            .records
            .inserted
            .iter()
            .all(|slot| free_edges.contains(slot) || record(newer, slot).is_some_and(attribute))
        && transition
            .records
            .deleted
            .iter()
            .all(|slot| record(older, slot).is_some_and(attribute))
        && transition.records.updated.iter().all(|slot| {
            record(older, slot)
                .zip(record(newer, slot))
                .is_some_and(|(before, after)| {
                    (attribute(before) && attribute(after)) || same_content(before, after)
                })
        })
}

/// The slots of the saved free edges that a transition inserts, with their
/// curves, vertices, and points. A saved free edge is an edge record with a
/// null coedge reference, so no loop, face, shell, or solid uses it. The stable
/// topology holds each one as a wire body, region, and shell on the edge's own
/// slot. Returns `None` when the inserted topology is not exactly such a set.
fn inserted_saved_free_edges(
    transition: &AsmHistoricalTransition,
    newer: &BTreeMap<i64, i64>,
    archive: &HistoricalRecordArchive,
) -> Option<BTreeSet<i64>> {
    let topology = &transition.topology;
    let edges = &topology.edges.inserted;
    if topology.bodies.inserted != *edges
        || topology.regions.inserted != *edges
        || topology.shells.inserted != *edges
    {
        return None;
    }
    let record = |slot: i64| newer.get(&slot).and_then(|revision| archive.get(revision));
    let mut vertices = BTreeSet::new();
    let mut curves = BTreeSet::new();
    for &slot in edges {
        let edge = record(slot)?;
        if !matches!(edge.head(), "edge" | "tedge")
            || !matches!(edge.chunk(EDGE_COEDGE), Some(Token::Ref(-1)))
        {
            return None;
        }
        vertices.extend([
            edge.ref_at(EDGE_START_VERTEX)?,
            edge.ref_at(EDGE_END_VERTEX)?,
        ]);
        curves.insert(edge.ref_at(EDGE_CURVE)?);
    }
    let mut points = BTreeSet::new();
    for &slot in &vertices {
        let vertex = record(slot)?;
        if !matches!(vertex.head(), "vertex" | "tvertex") {
            return None;
        }
        points.extend(vertex.chunks().filter_map(|token| match token {
            Token::Ref(reference) if *reference >= 0 => Some(*reference),
            _ => None,
        }));
    }
    let within =
        |inserted: &[i64], used: &BTreeSet<i64>| inserted.iter().all(|slot| used.contains(slot));
    (within(&topology.vertices.inserted, &vertices)
        && within(&topology.curves.inserted, &curves)
        && within(&topology.points.inserted, &points))
    .then(|| {
        edges
            .iter()
            .chain(&topology.vertices.inserted)
            .chain(&topology.curves.inserted)
            .chain(&topology.points.inserted)
            .copied()
            .collect()
    })
}

fn attribute(record: &Record) -> bool {
    record.name.ends_with("-attrib")
}

fn same_content(before: &Record, after: &Record) -> bool {
    if before.name != after.name {
        return false;
    }
    match (before.tokens.split_first(), after.tokens.split_first()) {
        (Some((Token::Ref(_), before)), Some((Token::Ref(_), after))) => before == after,
        _ => before.tokens == after.tokens,
    }
}

#[cfg(test)]
mod tests;
