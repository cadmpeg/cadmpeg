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
    let Some(count) = record.count(1).filter(|count| *count > 0) else {
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
    let Some(count) = record.count(12).filter(|count| *count > 0) else {
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

fn leader_valid(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    let Some(count) = record.count(1).filter(|count| *count > 0) else {
        return false;
    };
    let dimensions_valid = record
        .number(2)
        .zip(record.number(3))
        .is_some_and(|(height, width)| {
            height.is_finite()
                && width.is_finite()
                && match entry.form {
                    4 => height == 0.0 && width == 0.0,
                    5 | 6 | 12 => height > 0.0 && height == width,
                    1..=3 | 7..=11 => height > 0.0 && width > 0.0,
                    _ => false,
                }
        });
    exact_parameter_count(record, 7 + count * 2, entries)
        && dimensions_valid
        && (4..=6 + count * 2).all(|index| finite(record, index))
}

fn pointer(
    record: &ParameterRecord,
    index: usize,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> Option<u32> {
    record
        .integer(index)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|sequence| sequence % 2 == 1)
        .filter(|sequence| entries.contains_key(sequence))
}

fn child_valid(
    sequence: u32,
    entity_type: i64,
    forms: impl Fn(i64) -> bool,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> bool {
    entries.get(&sequence).is_some_and(|entry| {
        entry.entity_type == entity_type
            && forms(entry.form)
            && entry.status.subordinate == 1
            && entry.status.use_flag == 1
            && records
                .get(&sequence)
                .is_some_and(|record| match entity_type {
                    212 => general_note_valid(record, entries),
                    214 => leader_valid(entry, record, entries),
                    106 => witness_valid(record, entries),
                    _ => false,
                })
    })
}

fn dimension_children_valid(
    parent: &DirectoryEntry,
    children: &[u32],
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    let Some(first_transform) = children
        .first()
        .and_then(|sequence| entries.get(sequence))
        .map(|entry| entry.transform)
    else {
        return false;
    };
    (parent.transform == 0 || first_transform == 0)
        && children.iter().all(|sequence| {
            entries
                .get(sequence)
                .is_some_and(|entry| entry.transform == first_transform)
        })
}

fn witness_valid(record: &ParameterRecord, entries: &BTreeMap<u32, &DirectoryEntry>) -> bool {
    let Some(count) = record
        .count(2)
        .filter(|count| *count >= 3 && *count % 2 == 1)
    else {
        return false;
    };
    record.integer(1) == Some(1)
        && exact_parameter_count(record, 4 + count * 2, entries)
        && (3..4 + count * 2).all(|index| finite(record, index))
}

fn parameterized_curve_type(entry: &DirectoryEntry) -> bool {
    matches!(
        entry.entity_type,
        100 | 102 | 104 | 106 | 110 | 112 | 126 | 130 | 142
    )
}

fn dimension_valid(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> bool {
    let note = pointer(record, 1, entries);
    let note_valid =
        note.is_some_and(|sequence| child_valid(sequence, 212, |form| form == 0, entries, records));
    let mut children = note.into_iter().collect::<Vec<_>>();
    let fields_valid = match (entry.entity_type, entry.form) {
        (202, 0) => {
            let witnesses = [record.integer(2), record.integer(3)];
            let leaders = [pointer(record, 7, entries), pointer(record, 8, entries)];
            let witnesses_valid = witnesses.iter().enumerate().all(|(offset, raw)| match raw {
                Some(0) => true,
                Some(_) => pointer(record, 2 + offset, entries).is_some_and(|sequence| {
                    child_valid(sequence, 106, |form| form == 40, entries, records)
                }),
                None => false,
            });
            let leaders_valid = leaders.iter().all(|leader| {
                leader.is_some_and(|sequence| {
                    child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                    )
                })
            });
            children.extend((2..=3).filter_map(|index| pointer(record, index, entries)));
            children.extend(leaders.into_iter().flatten());
            exact_parameter_count(record, 9, entries)
                && witnesses_valid
                && (4..=5).all(|index| finite(record, index))
                && record
                    .number(6)
                    .is_some_and(|value| value.is_finite() && value > 0.0)
                && leaders_valid
        }
        (204, 0) => {
            let curves = [pointer(record, 2, entries), pointer(record, 3, entries)];
            let curve_entries =
                curves.map(|curve| curve.and_then(|sequence| entries.get(&sequence).copied()));
            let curves_valid = curve_entries[0].is_some_and(|curve| {
                parameterized_curve_type(curve)
                    && curve.status.subordinate == 1
                    && curve.status.use_flag == 1
            }) && match record.integer(3) {
                Some(0) => true,
                Some(_) => curve_entries[1].is_some_and(|curve| {
                    parameterized_curve_type(curve)
                        && curve.status.subordinate == 1
                        && curve.status.use_flag == 1
                        && !(curve.entity_type == 110
                            && curve_entries[0].is_some_and(|first| first.entity_type == 110))
                }),
                None => false,
            };
            let leaders = [pointer(record, 4, entries), pointer(record, 5, entries)];
            let leaders_valid = leaders.iter().all(|leader| {
                leader.is_some_and(|sequence| {
                    child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                    )
                })
            });
            let witnesses_valid = (6..=7).all(|index| match record.integer(index) {
                Some(0) => true,
                Some(_) => pointer(record, index, entries).is_some_and(|sequence| {
                    child_valid(sequence, 106, |form| form == 40, entries, records)
                }),
                None => false,
            });
            children.extend((2..=7).filter_map(|index| pointer(record, index, entries)));
            exact_parameter_count(record, 8, entries)
                && curves_valid
                && leaders_valid
                && witnesses_valid
        }
        (206, 0) => {
            let first = pointer(record, 2, entries);
            let second = pointer(record, 3, entries);
            let leaders_valid = first.is_some_and(|sequence| {
                child_valid(
                    sequence,
                    214,
                    |form| matches!(form, 1..=12),
                    entries,
                    records,
                )
            }) && match record.integer(3) {
                Some(0) => true,
                Some(_) => second.is_some_and(|sequence| {
                    child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                    )
                }),
                None => false,
            };
            children.extend(first);
            children.extend(second);
            exact_parameter_count(record, 6, entries)
                && leaders_valid
                && (4..=5).all(|index| finite(record, index))
        }
        (216, 0..=2) => {
            let leaders = [pointer(record, 2, entries), pointer(record, 3, entries)];
            let witnesses = [record.integer(4), record.integer(5)];
            let leaders_valid = leaders.iter().all(|sequence| {
                sequence.is_some_and(|sequence| {
                    child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                    )
                })
            });
            let witnesses_valid = witnesses.iter().enumerate().all(|(offset, raw)| match raw {
                Some(0) => true,
                Some(_) => pointer(record, 4 + offset, entries).is_some_and(|sequence| {
                    child_valid(sequence, 106, |form| form == 40, entries, records)
                }),
                None => false,
            });
            children.extend(leaders.into_iter().flatten());
            children.extend((4..=5).filter_map(|index| pointer(record, index, entries)));
            exact_parameter_count(record, 6, entries) && leaders_valid && witnesses_valid
        }
        (218, 0) => {
            let ordinate = pointer(record, 2, entries);
            let valid = ordinate.is_some_and(|sequence| {
                child_valid(sequence, 106, |form| form == 40, entries, records)
                    || child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                    )
            });
            children.extend(ordinate);
            exact_parameter_count(record, 3, entries) && valid
        }
        (218, 1) => {
            let witness = pointer(record, 2, entries);
            let leader = pointer(record, 3, entries);
            let valid = witness.is_some_and(|sequence| {
                child_valid(sequence, 106, |form| form == 40, entries, records)
            }) && leader.is_some_and(|sequence| {
                child_valid(
                    sequence,
                    214,
                    |form| matches!(form, 1..=12),
                    entries,
                    records,
                )
            });
            children.extend(witness);
            children.extend(leader);
            exact_parameter_count(record, 4, entries) && valid
        }
        (220, 0) => {
            let leader = pointer(record, 2, entries);
            let enclosure_raw = record.integer(3);
            let enclosure = pointer(record, 3, entries);
            let leader_valid = leader.is_some_and(|sequence| {
                child_valid(
                    sequence,
                    214,
                    |form| matches!(form, 1..=12),
                    entries,
                    records,
                ) && records.get(&sequence).and_then(|record| record.integer(1)) == Some(3)
            });
            let enclosure_valid = match enclosure_raw {
                Some(0) => true,
                Some(_) => enclosure.is_some_and(|sequence| {
                    entries.get(&sequence).is_some_and(|entry| {
                        matches!((entry.entity_type, entry.form), (100 | 102, 0) | (106, 63))
                            && entry.status.subordinate == 1
                            && entry.status.use_flag == 1
                    })
                }),
                None => false,
            };
            children.extend(leader);
            children.extend(enclosure);
            exact_parameter_count(record, 4, entries) && leader_valid && enclosure_valid
        }
        (222, 0..=1) => {
            let first = pointer(record, 2, entries);
            let first_valid = first.is_some_and(|sequence| {
                child_valid(
                    sequence,
                    214,
                    |form| matches!(form, 1..=12),
                    entries,
                    records,
                )
            });
            let center_valid = finite(record, 3) && finite(record, 4);
            let second_raw = (entry.form == 1).then(|| record.integer(5)).flatten();
            let second = (entry.form == 1)
                .then(|| pointer(record, 5, entries))
                .flatten();
            let second_valid = entry.form == 0
                || match second_raw {
                    Some(0) => true,
                    Some(_) => second.is_some_and(|sequence| {
                        child_valid(sequence, 214, |form| form == 4, entries, records)
                    }),
                    None => false,
                };
            children.extend(first);
            children.extend(second);
            exact_parameter_count(record, if entry.form == 0 { 5 } else { 6 }, entries)
                && first_valid
                && center_valid
                && second_valid
        }
        _ => false,
    };
    note_valid && fields_valid && dimension_children_valid(entry, &children, entries)
}

fn flag_or_label_valid(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> bool {
    let (note_index, count_index, leader_start) = if entry.entity_type == 208 {
        (5, 6, 7)
    } else {
        (1, 2, 3)
    };
    let note = pointer(record, note_index, entries);
    let note_valid =
        note.is_some_and(|sequence| child_valid(sequence, 212, |form| form == 0, entries, records));
    let count = record.count(count_index);
    let leaders_valid = count.is_some_and(|count| {
        (0..count).all(|offset| {
            pointer(record, leader_start + offset, entries).is_some_and(|sequence| {
                child_valid(
                    sequence,
                    214,
                    |form| matches!(form, 1..=12),
                    entries,
                    records,
                )
            })
        })
    });
    let shape_valid = if entry.entity_type == 208 {
        count.is_some_and(|count| exact_parameter_count(record, 7 + count, entries))
            && (1..=4).all(|index| finite(record, index))
            && note
                .and_then(|sequence| records.get(&sequence))
                .and_then(|note| note.count(1))
                .is_some_and(|strings| {
                    (0..strings)
                        .map(|offset| {
                            note.and_then(|sequence| records.get(&sequence))
                                .and_then(|note| note.integer(2 + offset * 12))
                                .unwrap_or_default()
                        })
                        .sum::<i64>()
                        <= 10
                })
    } else {
        count.is_some_and(|count| count > 0 && exact_parameter_count(record, 3 + count, entries))
    };
    note_valid && leaders_valid && shape_valid
}

fn general_symbol_valid(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> bool {
    let note_valid = match record.integer(1) {
        Some(0) => true,
        Some(_) => pointer(record, 1, entries)
            .is_some_and(|sequence| child_valid(sequence, 212, |form| form == 0, entries, records)),
        None => false,
    };
    let Some(geometry_count) = record.count(2).filter(|count| *count > 0) else {
        return false;
    };
    let geometry_valid = (0..geometry_count).all(|offset| {
        pointer(record, 3 + offset, entries).is_some_and(|sequence| {
            entries
                .get(&sequence)
                .is_some_and(|target| target.status.subordinate == 1 && target.status.use_flag == 1)
        })
    });
    let leader_count_index = 3 + geometry_count;
    let Some(leader_count) = record.count(leader_count_index) else {
        return false;
    };
    let leaders_valid = (0..leader_count).all(|offset| {
        pointer(record, leader_count_index + 1 + offset, entries).is_some_and(|sequence| {
            child_valid(
                sequence,
                214,
                |form| matches!(form, 1..=12),
                entries,
                records,
            )
        })
    });
    note_valid
        && geometry_valid
        && leaders_valid
        && exact_parameter_count(record, leader_count_index + 1 + leader_count, entries)
}

fn section_boundary_type(entry: &DirectoryEntry) -> bool {
    matches!(
        (entry.entity_type, entry.form),
        (100 | 102 | 112 | 126, 0) | (104, 1) | (106, 63)
    )
}

fn fill_pattern_valid(pattern: i64) -> bool {
    matches!(
        pattern,
        0..=20 | 22 | 26 | 28..=29 | 32 | 34 | 36 | 38 | 40..=42 | 46 | 50 | 60
            | 70 | 72 | 80 | 82 | 84 | 86 | 90 | 92 | 94 | 110 | 124 | 134 | 136
            | 140 | 142 | 152 | 154 | 156..=159 | 172 | 174 | 178 | 210 | 220 | 224
            | 226 | 234 | 236 | 240 | 244 | 246 | 252 | 254 | 256 | 262 | 264..=266
            | 268
    )
}

fn zero_or_omitted(record: &ParameterRecord, index: usize) -> bool {
    match record.tokens.get(index).map(|token| &token.value) {
        None | Some(crate::parameter::TokenValue::Omitted) => true,
        _ => record.number(index) == Some(0.0),
    }
}

fn finite_or_omitted(record: &ParameterRecord, index: usize) -> bool {
    match record.tokens.get(index).map(|token| &token.value) {
        None | Some(crate::parameter::TokenValue::Omitted) => true,
        _ => finite(record, index),
    }
}

fn sectioned_area_valid(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    let boundary_valid = pointer(record, 1, entries)
        .and_then(|sequence| entries.get(&sequence).copied())
        .is_some_and(section_boundary_type);
    let pattern = record.integer(2).filter(|value| fill_pattern_valid(*value));
    let pattern_parameters_valid = pattern.is_some_and(|pattern| {
        if matches!(pattern, 0 | 19) || pattern > 19 {
            (3..=7).all(|index| zero_or_omitted(record, index))
        } else {
            finite_or_omitted(record, 3)
                && finite_or_omitted(record, 4)
                && finite(record, 5)
                && record
                    .number(6)
                    .is_some_and(|distance| distance.is_finite() && distance > 0.0)
                && finite_or_omitted(record, 7)
        }
    });
    let Some(island_count) = record.count(8) else {
        return false;
    };
    let islands_valid = (0..island_count).all(|offset| {
        pointer(record, 9 + offset, entries)
            .and_then(|sequence| entries.get(&sequence).copied())
            .is_some_and(section_boundary_type)
    });
    boundary_valid
        && pattern_parameters_valid
        && islands_valid
        && exact_parameter_count(record, 9 + island_count, entries)
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

    for entry in directory.iter().filter(|entry| {
        (matches!(entry.entity_type, 202 | 204 | 206 | 208 | 210 | 212 | 213) && entry.form == 0)
            || (entry.entity_type == 214 && matches!(entry.form, 1..=12))
            || matches!(
                (entry.entity_type, entry.form),
                (216, 0..=2) | (218 | 222, 0..=1) | (220, 0)
            )
            || matches!((entry.entity_type, entry.form), (228 | 230, 0))
    }) {
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
                && match entry.entity_type {
                    202 | 204 | 206 => dimension_valid(entry, record, &entries, &records),
                    208 | 210 => flag_or_label_valid(entry, record, &entries, &records),
                    212 => general_note_valid(record, &entries),
                    213 => new_general_note_valid(record, &entries),
                    214 => leader_valid(entry, record, &entries),
                    216 | 218 | 220 | 222 => dimension_valid(entry, record, &entries, &records),
                    228 => general_symbol_valid(record, &entries, &records),
                    230 => sectioned_area_valid(record, &entries),
                    _ => false,
                }
        });
        if valid {
            decoded.insert(entry.sequence);
        } else {
            let message = match entry.entity_type {
                202 | 204 | 206 | 216 | 218 | 220 | 222 => {
                    "dimension components, role types, transforms, or Directory status are invalid"
                }
                228 => "symbol note, defining geometry, or leader list is invalid",
                230 => "section boundary, fill pattern, hatch geometry, or island list is invalid",
                _ => "text count, presentation metrics, encoding, placement, or Directory use flag is invalid",
            };
            losses.push(entity_loss(entry, message));
        }
    }

    Projection {
        handled,
        decoded,
        losses,
    }
}
