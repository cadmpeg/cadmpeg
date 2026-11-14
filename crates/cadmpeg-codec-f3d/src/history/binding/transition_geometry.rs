// SPDX-License-Identifier: Apache-2.0
use super::{HistoricalRecordArchive, Record, Token};
use crate::history_records::AsmHistoricalTransition;
use std::collections::BTreeMap;

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
    transition.previous_state_id.is_some()
        && families.iter().all(|family| {
            family.inserted.is_empty()
                && family.deleted.is_empty()
                && family.updated.iter().all(same)
        })
        && transition
            .records
            .inserted
            .iter()
            .all(|slot| record(newer, slot).is_some_and(attribute))
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
