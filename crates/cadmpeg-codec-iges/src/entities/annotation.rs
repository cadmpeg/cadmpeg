// SPDX-License-Identifier: Apache-2.0
//! Text annotation entities.

use super::geometry::{entity_loss, resolve_transform, Projection};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::{trailing_pointer_groups, ParameterRecord};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

fn finite(record: &ParameterRecord, index: usize) -> bool {
    record.number(index).is_some_and(f64::is_finite)
}

fn exact_parameter_count(
    record: &ParameterRecord,
    expected: usize,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    trailing_pointer_groups(record, entries)
        .map_or(record.tokens.len(), |groups| groups.token_start)
        == expected
}

fn font_valid(value: i64, entries: &BTreeMap<u32, &DirectoryEntry>) -> bool {
    value >= 0
        || value
            .checked_neg()
            .and_then(|value| u32::try_from(value).ok())
            .and_then(|sequence| entries.get(&sequence).copied())
            .is_some_and(|entry| entry.entity_type == 310)
}

fn general_note_valid(record: &ParameterRecord, entries: &BTreeMap<u32, &DirectoryEntry>) -> bool {
    let Some(count) = record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return false;
    };
    exact_parameter_count(record, 2 + count * 12, entries)
        && (0..count).all(|index| {
            let start = 2 + index * 12;
            let text = record.string(start + 11);
            record
                .integer(start)
                .and_then(|value| usize::try_from(value).ok())
                .zip(text)
                .is_some_and(|(declared, text)| declared == text.len())
                && (start + 1..=start + 2).all(|field| {
                    record
                        .number(field)
                        .is_some_and(|value| value.is_finite() && value >= 0.0)
                })
                && record
                    .integer(start + 3)
                    .is_some_and(|value| font_valid(value, entries))
                && (start + 4..=start + 5).all(|field| finite(record, field))
                && record
                    .integer(start + 6)
                    .is_some_and(|value| matches!(value, 0..=2))
                && record
                    .integer(start + 7)
                    .is_some_and(|value| matches!(value, 0..=1))
                && (start + 8..=start + 10).all(|field| finite(record, field))
        })
}

fn new_general_note_valid(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    let Some(count) = record
        .integer(12)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return false;
    };
    exact_parameter_count(record, 13 + count * 20, entries)
        && (1..=2).all(|index| {
            record
                .number(index)
                .is_some_and(|value| value.is_finite() && value >= 0.0)
        })
        && record
            .integer(3)
            .is_some_and(|value| matches!(value, 0..=3))
        && (4..=11).all(|index| finite(record, index))
        && (0..count).all(|index| {
            let start = 13 + index * 20;
            let fixed = record.integer(start);
            let character_width = record.number(start + 1);
            let character_height = record.number(start + 2);
            let spacing = record.number(start + 3);
            let text = record.string(start + 19);
            let metrics_valid = character_width
                .zip(character_height)
                .zip(spacing)
                .is_some_and(|((width, height), spacing)| {
                    width.is_finite()
                        && width > 0.0
                        && height.is_finite()
                        && height > 0.0
                        && spacing.is_finite()
                        && match fixed {
                            Some(0) => spacing >= -width,
                            Some(1) => spacing >= 0.0,
                            _ => false,
                        }
                });
            metrics_valid
                && finite(record, start + 4)
                && record.integer(start + 5).is_some()
                && record.number(start + 6).is_some_and(|value| {
                    value.is_finite() && (0.0..=std::f64::consts::TAU).contains(&value)
                })
                && record.string(start + 7).is_some()
                && record
                    .integer(start + 8)
                    .and_then(|value| usize::try_from(value).ok())
                    .zip(text)
                    .is_some_and(|(declared, text)| declared == text.len())
                && (start + 9..=start + 10).all(|field| {
                    record
                        .number(field)
                        .is_some_and(|value| value.is_finite() && value >= 0.0)
                })
                && record
                    .integer(start + 11)
                    .is_some_and(|value| font_valid(value, entries))
                && (start + 12..=start + 13).all(|field| finite(record, field))
                && record
                    .integer(start + 14)
                    .is_some_and(|value| matches!(value, 0..=2))
                && record
                    .integer(start + 15)
                    .is_some_and(|value| matches!(value, 0..=1))
                && (start + 16..=start + 18).all(|field| finite(record, field))
        })
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
        .filter(|entry| matches!(entry.entity_type, 212 | 213) && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let valid = records.get(&entry.sequence).is_some_and(|record| {
            let transform_valid = global.length_factor_mm().is_some_and(|factor| {
                resolve_transform(
                    entry.transform,
                    &entries,
                    &records,
                    factor,
                    &mut BTreeSet::new(),
                )
                .is_ok()
            });
            entry.status.use_flag == 1
                && transform_valid
                && if entry.entity_type == 212 {
                    general_note_valid(record, &entries)
                } else {
                    new_general_note_valid(record, &entries)
                }
        });
        if valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "text count, presentation metrics, encoding, placement, or Directory use flag is invalid",
            ));
        }
    }

    Projection {
        handled,
        decoded,
        losses,
    }
}
