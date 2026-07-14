// SPDX-License-Identifier: Apache-2.0
//! Product definitions, occurrences, and ordered assembly relationships.

use super::geometry::{entity_loss, resolve_transform, Projection};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::ParameterRecord;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct SolidAssembly {
    form: i64,
    items: Vec<(u32, u32)>,
}

fn assembly_cycle(
    sequence: u32,
    definitions: &BTreeMap<u32, SolidAssembly>,
    visiting: &mut BTreeSet<u32>,
    visited: &mut BTreeSet<u32>,
) -> bool {
    if visited.contains(&sequence) {
        return false;
    }
    if !visiting.insert(sequence) {
        return true;
    }
    if definitions.get(&sequence).is_some_and(|definition| {
        definition.items.iter().any(|(item, _)| {
            definitions.contains_key(item) && assembly_cycle(*item, definitions, visiting, visited)
        })
    }) {
        return true;
    }
    visiting.remove(&sequence);
    visited.insert(sequence);
    false
}

pub(super) fn project(
    _ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
) -> Projection {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut handled = BTreeSet::new();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut assemblies = BTreeMap::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 184 && matches!(entry.form, 0 | 1))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(count) = record
            .integer(1)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|count| *count > 0)
        else {
            losses.push(entity_loss(
                entry,
                "solid-assembly item count is not positive",
            ));
            continue;
        };
        let items = (0..count)
            .map(|index| {
                let item = record.integer(2 + index).and_then(|value| {
                    let sequence = u32::try_from(value).ok()?;
                    (sequence % 2 == 1).then_some(sequence)
                })?;
                let transformation = record
                    .integer(2 + count + index)
                    .and_then(|value| u32::try_from(value).ok())?;
                (transformation == 0 || transformation % 2 == 1).then_some((item, transformation))
            })
            .collect::<Option<Vec<_>>>();
        let Some(items) = items else {
            losses.push(entity_loss(entry, "solid-assembly item tuple is invalid"));
            continue;
        };
        assemblies.insert(
            entry.sequence,
            SolidAssembly {
                form: entry.form,
                items,
            },
        );
    }

    let mut visited = BTreeSet::new();
    for (sequence, assembly) in &assemblies {
        let entry = entries[sequence];
        let has_brep = assembly.items.iter().any(|(item, _)| {
            entries
                .get(item)
                .is_some_and(|target| target.entity_type == 186)
        });
        let items_valid = assembly.items.iter().all(|(item, transformation)| {
            let item_valid = entries.get(item).is_some_and(|target| {
                matches!(
                    target.entity_type,
                    150 | 152 | 154 | 156 | 158 | 160 | 162 | 164 | 168 | 180 | 184 | 430
                ) || (assembly.form == 1 && target.entity_type == 186)
            });
            let transform_valid = *transformation == 0
                || entries.get(transformation).is_some_and(|target| {
                    target.entity_type == 124
                        && resolve_transform(
                            *transformation as i64,
                            &entries,
                            &records,
                            global.length_factor_mm().unwrap_or_default(),
                            &mut BTreeSet::new(),
                        )
                        .is_ok()
                });
            item_valid && transform_valid
        });
        let cyclic = assembly_cycle(*sequence, &assemblies, &mut BTreeSet::new(), &mut visited);
        let own_transform_valid = global.length_factor_mm().is_some_and(|factor| {
            resolve_transform(
                entry.transform,
                &entries,
                &records,
                factor,
                &mut BTreeSet::new(),
            )
            .is_ok()
        });
        if entry.status.use_flag != 2
            || (assembly.form == 1) != has_brep
            || !items_valid
            || cyclic
            || !own_transform_valid
        {
            losses.push(entity_loss(
                entry,
                "solid-assembly use flag, form, members, transforms, or acyclicity is invalid",
            ));
            continue;
        }
        decoded.insert(*sequence);
    }

    Projection {
        handled,
        decoded,
        losses,
    }
}
