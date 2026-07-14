// SPDX-License-Identifier: Apache-2.0
//! Views, drawings, and view-dependent presentation relationships.

use super::geometry::{entity_loss, resolve_transform, Projection};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::ParameterRecord;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

fn finite_vector(record: &ParameterRecord, start: usize) -> Option<[f64; 3]> {
    let values = [
        record.number(start)?,
        record.number(start + 1)?,
        record.number(start + 2)?,
    ];
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(values)
}

fn norm_squared(vector: [f64; 3]) -> f64 {
    vector.iter().map(|value| value * value).sum()
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

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 410 && matches!(entry.form, 0 | 1))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let view_number_valid = record.integer(1).is_some();
        let scale_valid = match record.tokens.get(2).map(|token| &token.value) {
            None | Some(crate::parameter::TokenValue::Omitted) => entry.form == 0,
            _ => record
                .number(2)
                .is_some_and(|value| value.is_finite() && value > 0.0),
        };
        let form_valid = if entry.form == 0 {
            let transform_valid = if entry.transform == 0 {
                true
            } else {
                u32::try_from(entry.transform).ok().is_some_and(|sequence| {
                    entries.get(&sequence).is_some_and(|target| {
                        target.entity_type == 124
                            && target.form == 0
                            && global.length_factor_mm().is_some_and(|factor| {
                                resolve_transform(
                                    entry.transform,
                                    &entries,
                                    &records,
                                    factor,
                                    &mut BTreeSet::new(),
                                )
                                .is_ok()
                            })
                    })
                })
            };
            let clipping_valid = (3..=8).all(|index| {
                record.integer(index).is_some_and(|value| {
                    value == 0
                        || u32::try_from(value).ok().is_some_and(|sequence| {
                            entries
                                .get(&sequence)
                                .is_some_and(|target| target.entity_type == 108)
                        })
                })
            });
            transform_valid && clipping_valid
        } else {
            let normal = finite_vector(record, 3);
            let reference = finite_vector(record, 6);
            let center = finite_vector(record, 9);
            let up = finite_vector(record, 12);
            let vectors_valid = normal.zip(up).is_some_and(|(normal, up)| {
                let normal_norm = norm_squared(normal);
                let up_norm = norm_squared(up);
                let dot = (0..3).map(|index| normal[index] * up[index]).sum::<f64>();
                normal_norm > 0.0 && up_norm > 0.0 && up_norm - dot * dot / normal_norm > 1.0e-20
            });
            let window_valid = (15..=19)
                .all(|index| record.number(index).is_some_and(f64::is_finite))
                && record
                    .number(16)
                    .zip(record.number(17))
                    .is_some_and(|(min, max)| min < max)
                && record
                    .number(18)
                    .zip(record.number(19))
                    .is_some_and(|(min, max)| min < max);
            let depth = record.integer(20).filter(|value| matches!(value, 0..=3));
            let depth_values_valid = (21..=22)
                .all(|index| record.number(index).is_some_and(f64::is_finite))
                && (depth != Some(3)
                    || record
                        .number(21)
                        .zip(record.number(22))
                        .is_some_and(|(min, max)| min < max));
            entry.transform == 0
                && reference.is_some()
                && center.is_some()
                && vectors_valid
                && window_valid
                && depth.is_some()
                && depth_values_valid
        };
        if view_number_valid && scale_valid && form_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "view number, projection, transform, scale, or clipping fields are invalid",
            ));
        }
    }

    Projection {
        handled,
        decoded,
        losses,
    }
}
