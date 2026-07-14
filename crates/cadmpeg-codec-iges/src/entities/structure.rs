// SPDX-License-Identifier: Apache-2.0
//! Product definitions, occurrences, and ordered assembly relationships.

use super::geometry::{entity_loss, resolve_transform, Projection};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::{trailing_pointer_groups, ParameterRecord, TokenValue};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct SolidAssembly {
    form: i64,
    items: Vec<(u32, u32)>,
}

#[derive(Clone)]
struct SubfigureDefinition {
    depth: usize,
    members: Vec<u32>,
}

#[derive(Clone, Copy)]
struct SubfigureInstance {
    definition: u32,
    valid_fields: bool,
}

#[derive(Clone)]
struct NetworkDefinition {
    depth: usize,
    members: Vec<u32>,
    connect_count: usize,
}

#[derive(Clone, Copy)]
struct NetworkInstance {
    definition: u32,
    connect_count: usize,
    valid_fields: bool,
}

#[derive(Clone)]
struct FlowAssociativity {
    form: i64,
    associated: Vec<u32>,
    continuations: Vec<Option<u32>>,
}

pub(crate) fn single_target_cycle(
    sequence: u32,
    targets: &BTreeMap<u32, u32>,
    visited: &mut BTreeSet<u32>,
) -> bool {
    if visited.contains(&sequence) {
        return false;
    }

    let mut path = Vec::new();
    let mut visiting = BTreeSet::new();
    let mut current = sequence;
    loop {
        if visited.contains(&current) {
            visited.extend(path);
            return false;
        }
        if !visiting.insert(current) {
            return true;
        }
        path.push(current);
        let Some(target) = targets
            .get(&current)
            .copied()
            .filter(|target| targets.contains_key(target))
        else {
            visited.extend(path);
            return false;
        };
        current = target;
    }
}

fn array_base_type(entity_type: i64, form: i64) -> bool {
    matches!(
        entity_type,
        100 | 104
            | 110
            | 112
            | 116
            | 126
            | 202
            | 206
            | 208
            | 210
            | 212
            | 214
            | 216
            | 218
            | 220
            | 222
            | 228
            | 308
            | 412
            | 414
    ) || (entity_type == 402 && matches!(form, 1 | 7 | 14 | 15))
}

fn array_mask_valid(
    record: &ParameterRecord,
    count_index: usize,
    flag_index: usize,
    first_position_index: usize,
    total: usize,
) -> bool {
    let Some(count) = record.count(count_index) else {
        return false;
    };
    let Some(flag) = record
        .integer(flag_index)
        .filter(|value| matches!(*value, 0..=1))
    else {
        return false;
    };
    let positions = (0..count)
        .map(|index| {
            record
                .integer(first_position_index + index)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|position| *position >= 1 && *position <= total)
        })
        .collect::<Option<Vec<_>>>();
    let Some(positions) = positions else {
        return false;
    };
    let unique = positions.iter().copied().collect::<BTreeSet<_>>().len() == positions.len();
    let cardinality_valid = count == 0
        || if flag == 0 {
            count <= total / 2
        } else {
            count >= total.div_ceil(2)
        };
    unique && cardinality_valid
}

fn has_association_back_pointer(
    record: &ParameterRecord,
    group_sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    trailing_pointer_groups(record, entries)
        .is_some_and(|groups| groups.associations.contains(&group_sequence))
}

fn has_property_pointer(
    record: &ParameterRecord,
    property_sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    trailing_pointer_groups(record, entries)
        .is_some_and(|groups| groups.properties.contains(&property_sequence))
}

fn attribute_value_valid(
    record: &ParameterRecord,
    index: usize,
    data_type: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    match (
        data_type,
        record.tokens.get(index).map(|token| &token.value),
    ) {
        (1, Some(TokenValue::Integer(_)))
        | (3, Some(TokenValue::String(_)))
        | (5, Some(TokenValue::Omitted)) => true,
        (2, Some(TokenValue::Integer(_) | TokenValue::Real(_))) => {
            record.number(index).is_some_and(f64::is_finite)
        }
        (4, Some(TokenValue::Integer(value))) => u32::try_from(*value)
            .ok()
            .filter(|sequence| sequence % 2 == 1)
            .is_some_and(|sequence| entries.contains_key(&sequence)),
        (6, Some(TokenValue::Integer(value))) => matches!(*value, 0..=1),
        _ => false,
    }
}

fn unit_value_valid(unit_type: &[u8], value: &[u8]) -> bool {
    match unit_type {
        b"LENGTH" => matches!(
            value,
            b"A" | b"AU" | b"FT" | b"IN" | b"LY" | b"M" | b"UM" | b"MIL" | b"MI" | b"KN" | b"Y"
        ),
        b"MASS" => matches!(
            value,
            b"C" | b"DR" | b"GA" | b"KG" | b"MT" | b"OU" | b"LB" | b"S"
        ),
        b"TIME" => matches!(value, b"D" | b"HR" | b"M" | b"S" | b"W" | b"Y"),
        b"CURRENT" => value == b"A",
        b"TEMPERATURE" => matches!(value, b"C" | b"F" | b"K" | b"R"),
        b"AMOUNT" => value == b"M",
        b"INTENSITY" => value == b"C",
        b"PLANE" => matches!(value, b"D" | b"G" | b"M" | b"R" | b"REV" | b"S"),
        b"SOLID" => value == b"C",
        _ => false,
    }
}

fn number_or(record: &ParameterRecord, index: usize, default: f64) -> Option<f64> {
    match record.tokens.get(index).map(|token| &token.value) {
        None | Some(TokenValue::Omitted) => Some(default),
        Some(TokenValue::Integer(_) | TokenValue::Real(_)) => record.number(index),
        Some(TokenValue::String(_)) => None,
    }
}

fn integer_or(record: &ParameterRecord, index: usize, default: i64) -> Option<i64> {
    match record.tokens.get(index).map(|token| &token.value) {
        None | Some(TokenValue::Omitted) => Some(default),
        Some(TokenValue::Integer(value)) => Some(*value),
        Some(TokenValue::Real(_) | TokenValue::String(_)) => None,
    }
}

fn entity_parameter_end(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> usize {
    trailing_pointer_groups(record, entries)
        .map_or(record.tokens.len(), |groups| groups.token_start)
}

fn generic_property_value_valid(
    record: &ParameterRecord,
    index: usize,
    data_type: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    match data_type {
        0 => matches!(
            record.tokens.get(index).map(|token| &token.value),
            Some(TokenValue::Omitted)
        ),
        1 => record.integer(index).is_some(),
        2 => record.number(index).is_some_and(f64::is_finite),
        3 => record.string(index).is_some(),
        4 => existing_pointer(record, index, entries).is_some(),
        6 => record
            .integer(index)
            .is_some_and(|value| matches!(value, 0..=1)),
        _ => false,
    }
}

fn dimension_entity_type(entity_type: i64) -> bool {
    matches!(entity_type, 202 | 206 | 216 | 218 | 220 | 222)
}

fn closure_owner_dimension(entity_type: i64) -> Option<usize> {
    if matches!(
        entity_type,
        100 | 102 | 104 | 106 | 110 | 112 | 126 | 130 | 142
    ) {
        Some(1)
    } else if matches!(
        entity_type,
        108 | 114 | 118 | 120 | 122 | 128 | 140 | 190 | 192 | 194 | 196 | 198
    ) {
        Some(2)
    } else {
        None
    }
}

fn iges_datetime_valid(value: &[u8]) -> bool {
    let dot = match value.len() {
        13 => 6,
        15 => 8,
        _ => return false,
    };
    value.get(dot) == Some(&b'.')
        && value
            .iter()
            .enumerate()
            .all(|(index, byte)| index == dot || byte.is_ascii_digit())
}

fn property_fields_valid(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    end: usize,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    let exact = |count: i64| record.integer(1) == Some(count) && end == count as usize + 2;
    let integer_range = |index, range: std::ops::RangeInclusive<i64>| {
        record
            .integer(index)
            .is_some_and(|value| range.contains(&value))
    };
    match entry.form {
        2 => exact(3) && (2..=4).all(|index| integer_range(index, 0..=2)),
        3 => exact(2) && record.integer(2).is_some() && record.string(3).is_some(),
        5 => {
            exact(5)
                && record
                    .number(2)
                    .is_some_and(|value| value.is_finite() && value >= 0.0)
                && integer_range(3, 0..=1)
                && integer_range(4, 0..=2)
                && integer_range(5, 0..=2)
                && record.number(6).is_some_and(f64::is_finite)
        }
        6 => {
            exact(5)
                && (2..=3).all(|index| {
                    record
                        .number(index)
                        .is_some_and(|value| value.is_finite() && value >= 0.0)
                })
                && integer_range(4, 0..=1)
                && record
                    .integer(5)
                    .zip(record.integer(6))
                    .is_some_and(|(lower, upper)| lower >= 0 && upper >= lower)
        }
        7 | 8 | 15 => exact(1) && record.string(2).is_some_and(|value| !value.is_empty()),
        9 => exact(4) && (2..=5).all(|index| record.string(index).is_some()),
        10 => exact(6) && (2..=7).all(|index| integer_range(index, 0..=1)),
        11 => {
            let dependent_count = record.count(3).filter(|count| *count > 0 && *count <= end);
            let independent_count = record.count(4).filter(|count| *count <= end);
            let Some((dependent_count, independent_count)) = dependent_count.zip(independent_count)
            else {
                return false;
            };
            let types_valid = (0..independent_count).all(|offset| integer_range(5 + offset, 1..=8));
            let counts = (0..independent_count)
                .map(|offset| {
                    record
                        .integer(5 + independent_count + offset)
                        .and_then(|value| usize::try_from(value).ok())
                        .filter(|count| *count > 0)
                })
                .collect::<Option<Vec<_>>>();
            let Some(counts) = counts else {
                return false;
            };
            let independent_values = counts
                .iter()
                .try_fold(0_usize, |total, count| total.checked_add(*count));
            let point_count = counts
                .iter()
                .try_fold(1_usize, |total, count| total.checked_mul(*count));
            let expected_end = independent_values.zip(point_count).and_then(
                |(independent_values, point_count)| {
                    dependent_count
                        .checked_mul(point_count)
                        .and_then(|dependent_values| {
                            5_usize
                                .checked_add(2 * independent_count)?
                                .checked_add(independent_values)?
                                .checked_add(dependent_values)
                        })
                },
            );
            record
                .integer(2)
                .is_some_and(|value| matches!(value, 1..=9999))
                && types_valid
                && expected_end == Some(end)
                && record.integer(1) == i64::try_from(end - 2).ok()
                && (5 + 2 * independent_count..end)
                    .all(|index| record.number(index).is_some_and(f64::is_finite))
        }
        12 | 14 => record.count(1).is_some_and(|count| {
            count > 0
                && end == count + 2
                && (0..count).all(|offset| record.string(2 + offset).is_some())
        }),
        13 => {
            matches!(record.integer(1), Some(2 | 3))
                && end == record.integer(1).unwrap_or_default() as usize + 2
                && record.number(2).is_some_and(f64::is_finite)
                && record.string(3).is_some()
                && (record.integer(1) == Some(2) || record.string(4).is_some())
        }
        18 => {
            exact(1)
                && record
                    .number(2)
                    .is_some_and(|value| (0.0..=100.0).contains(&value))
        }
        19 => exact(1) && record.integer(2).is_some(),
        20 | 21 => exact(1) && integer_range(2, 0..=1),
        22 => {
            exact(9)
                && (2..=4).all(|index| integer_range(index, 0..=1))
                && (5..=6).all(|index| record.number(index).is_some_and(f64::is_finite))
                && (7..=8).all(|index| {
                    record
                        .number(index)
                        .is_some_and(|value| value.is_finite() && value > 0.0)
                })
                && (9..=10).all(|index| record.integer(index).is_some_and(|value| value >= 0))
                && (record.integer(2) != Some(1)
                    || (9..=10).all(|index| record.integer(index).is_some_and(|value| value > 0)))
        }
        23 => {
            exact(2)
                && integer_range(2, 1..=9999)
                && record.string(3).is_some_and(|value| !value.is_empty())
        }
        24 => record
            .count(2)
            .filter(|count| *count > 0)
            .is_some_and(|count| {
                exact(i64::try_from(1 + 4 * count).unwrap_or_default())
                    && (0..count).all(|offset| {
                        let start = 3 + offset * 4;
                        record.integer(start).is_some_and(|value| value >= 0)
                            && record.string(start + 1).is_some()
                            && record.integer(start + 2).is_some_and(|value| value >= 0)
                            && record
                                .string(start + 3)
                                .is_some_and(|value| !value.is_empty())
                    })
            }),
        25 => record
            .count(3)
            .filter(|count| *count > 0 && *count <= end)
            .is_some_and(|count| {
                exact(i64::try_from(2 + count).unwrap_or_default())
                    && record.string(2).is_some_and(|value| !value.is_empty())
                    && (0..count)
                        .all(|offset| record.integer(4 + offset).is_some_and(|value| value >= 0))
            }),
        26 => {
            exact(3)
                && (2..=3).all(|index| {
                    record
                        .number(index)
                        .is_some_and(|value| value.is_finite() && value > 0.0)
                })
                && record
                    .integer(4)
                    .is_some_and(|value| matches!(value, 1..=5 | 5001..=9999))
        }
        27 => record
            .count(3)
            .filter(|count| *count > 0)
            .is_some_and(|count| {
                exact(i64::try_from(2 + 2 * count).unwrap_or_default())
                    && record.string(2).is_some_and(|value| !value.is_empty())
                    && (0..count).all(|offset| {
                        let index = 4 + offset * 2;
                        record.integer(index).is_some_and(|data_type| {
                            generic_property_value_valid(record, index + 1, data_type, entries)
                        })
                    })
            }),
        28 => {
            let units_valid = record
                .integer(3)
                .is_some_and(|value| matches!(value, 0..=11 | 100..=106));
            let charset_valid = record
                .integer(4)
                .is_some_and(|value| matches!(value, 1 | 1001..=1003));
            let fraction = record.integer(6);
            exact(6)
                && integer_range(2, 0..=4)
                && units_valid
                && charset_valid
                && record.string(5).is_some()
                && fraction.is_some_and(|value| matches!(value, 0..=1))
                && record
                    .integer(7)
                    .is_some_and(|value| value >= 0 && (fraction != Some(1) || value > 0))
        }
        29 => {
            let fraction = record.integer(8);
            exact(8)
                && integer_range(2, 0..=2)
                && integer_range(3, 1..=10)
                && integer_range(4, 1..=4)
                && (5..=6).all(|index| record.number(index).is_some_and(f64::is_finite))
                && integer_range(7, 0..=1)
                && fraction.is_some_and(|value| matches!(value, 0..=2))
                && record
                    .integer(9)
                    .is_some_and(|value| value >= 0 && (fraction == Some(0) || value > 0))
        }
        30 => record
            .count(13)
            .filter(|count| *count <= end)
            .is_some_and(|count| {
                exact(i64::try_from(12 + count * 3).unwrap_or_default())
                    && integer_range(2, 0..=2)
                    && integer_range(3, 0..=4)
                    && integer_or(record, 4, 1)
                        .is_some_and(|value| matches!(value, 1 | 1001..=1003))
                    && record.string(5).is_some()
                    && integer_range(6, 0..=1)
                    && number_or(record, 7, std::f64::consts::FRAC_PI_2).is_some_and(f64::is_finite)
                    && integer_range(8, 0..=1)
                    && integer_range(9, 0..=2)
                    && integer_range(10, 0..=2)
                    && integer_range(11, 0..=1)
                    && record.number(12).is_some_and(f64::is_finite)
                    && (0..count).all(|offset| {
                        let start = 14 + offset * 3;
                        integer_range(start, 1..=4)
                            && record
                                .integer(start + 1)
                                .zip(record.integer(start + 2))
                                .is_some_and(|(first, last)| first > 0 && last >= first)
                    })
            }),
        31 => exact(8) && (2..=9).all(|index| record.number(index).is_some_and(f64::is_finite)),
        32 => {
            exact(3)
                && record.string(2).is_some_and(|value| !value.is_empty())
                && record.string(3).is_some()
                && record.string(4).is_some_and(iges_datetime_valid)
        }
        33 => {
            exact(2)
                && record.integer(2).is_some_and(|value| value > 0)
                && record.string(3).is_some_and(|value| !value.is_empty())
        }
        34 | 35 => record
            .count(2)
            .filter(|count| *count > 0 && *count <= end)
            .is_some_and(|count| {
                exact(i64::try_from(1 + count * 3).unwrap_or_default())
                    && (0..count).all(|offset| {
                        let start = 3 + offset * 3;
                        record.integer(start).is_some_and(|value| value > 0)
                            && record
                                .integer(start + 1)
                                .zip(record.integer(start + 2))
                                .is_some_and(|(first, last)| first > 0 && last >= first)
                    })
            }),
        36 => {
            matches!(record.integer(1), Some(1 | 2))
                && end == record.integer(1).unwrap_or_default() as usize + 2
                && integer_range(2, 0..=2)
                && (record.integer(1) == Some(1) || integer_range(3, 0..=2))
        }
        _ => false,
    }
}

fn existing_pointer(
    record: &ParameterRecord,
    index: usize,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> Option<u32> {
    record
        .integer(index)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|sequence| sequence % 2 == 1 && entries.contains_key(sequence))
}

fn predefined_associativity_valid(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> bool {
    let end = entity_parameter_end(record, entries);
    match entry.form {
        5 => {
            let Some(count) = record.count(1).filter(|count| *count > 0) else {
                return false;
            };
            end == 2 + count * 7
                && (0..count).all(|offset| {
                    let start = 2 + offset * 7;
                    existing_pointer(record, start, entries).is_some_and(|sequence| {
                        entries
                            .get(&sequence)
                            .is_some_and(|target| target.entity_type == 410)
                    }) && (start + 1..=start + 3)
                        .all(|index| record.number(index).is_some_and(f64::is_finite))
                        && existing_pointer(record, start + 4, entries).is_some_and(|sequence| {
                            entries
                                .get(&sequence)
                                .is_some_and(|target| target.entity_type == 214)
                        })
                        && record.integer(start + 5).is_some_and(|level| level >= 0)
                        && existing_pointer(record, start + 6, entries).is_some()
                })
        }
        6 => {
            let visible_count = record.count(1);
            let view = existing_pointer(record, 2, entries);
            let visible = visible_count.and_then(|count| {
                (0..count)
                    .map(|offset| existing_pointer(record, 3 + offset, entries))
                    .collect::<Option<Vec<_>>>()
            });
            visible_count.is_some_and(|count| end == 3 + count)
                && view.is_some_and(|sequence| {
                    entries
                        .get(&sequence)
                        .is_some_and(|target| target.entity_type == 410)
                        && records.get(&sequence).is_some_and(|record| {
                            has_association_back_pointer(record, entry.sequence, entries)
                        })
                })
                && visible.is_some_and(|visible| {
                    visible.iter().all(|sequence| {
                        records.get(sequence).is_some_and(|record| {
                            has_association_back_pointer(record, entry.sequence, entries)
                        })
                    })
                })
        }
        9 => {
            let child_count = record.count(2).filter(|count| *count > 0);
            let members = child_count.and_then(|count| {
                (3..4 + count)
                    .map(|index| existing_pointer(record, index, entries))
                    .collect::<Option<Vec<_>>>()
            });
            record.integer(1) == Some(1)
                && child_count.is_some_and(|count| end == 4 + count)
                && members.is_some_and(|members| {
                    members.iter().all(|sequence| {
                        records.get(sequence).is_some_and(|member| {
                            has_association_back_pointer(member, entry.sequence, entries)
                        })
                    })
                })
        }
        12 => {
            let count = record.count(1).filter(|count| *count > 0);
            count.is_some_and(|count| {
                end == 2 + count * 2
                    && (0..count).all(|offset| {
                        let start = 2 + offset * 2;
                        record.string(start).is_some_and(|name| !name.is_empty())
                            && existing_pointer(record, start + 1, entries).is_some()
                    })
            })
        }
        13 => {
            let geometry_count = record.count(2).filter(|count| *count > 0);
            let dimension = existing_pointer(record, 3, entries);
            record.integer(1) == Some(1)
                && geometry_count.is_some_and(|count| {
                    end == 4 + count
                        && (0..count)
                            .all(|offset| existing_pointer(record, 4 + offset, entries).is_some())
                })
                && dimension.is_some_and(|sequence| {
                    entries.get(&sequence).is_some_and(|target| {
                        matches!(target.entity_type, 202 | 206 | 216 | 218 | 220 | 222)
                    }) && records.get(&sequence).is_some_and(|member| {
                        has_association_back_pointer(member, entry.sequence, entries)
                    })
                })
        }
        16 => {
            let count = record.count(2).filter(|count| *count > 0);
            let transform_valid = match record.integer(3) {
                Some(0) => true,
                Some(_) => existing_pointer(record, 3, entries).is_some_and(|sequence| {
                    entries
                        .get(&sequence)
                        .is_some_and(|target| target.entity_type == 124 && target.form == 0)
                }),
                None => false,
            };
            record.integer(1) == Some(1)
                && transform_valid
                && count.is_some_and(|count| {
                    end == 4 + count
                        && (0..count)
                            .all(|offset| existing_pointer(record, 4 + offset, entries).is_some())
                })
        }
        21 => {
            let geometry_count = record.count(2).filter(|count| *count > 0);
            let dimension = existing_pointer(record, 3, entries);
            let dimension_entry = dimension.and_then(|sequence| entries.get(&sequence).copied());
            let orientation_valid = record.integer(4).is_some_and(|orientation| {
                dimension_entry.is_some_and(|dimension| match dimension.entity_type {
                    202 => matches!(orientation, 0..=3),
                    216 => matches!(orientation, 4..=8),
                    218 => matches!(orientation, 6..=7),
                    206 | 220 | 222 => orientation == 0,
                    _ => false,
                })
            });
            let angle_valid = record.number(5).is_some_and(f64::is_finite);
            let geometry_valid = geometry_count.is_some_and(|count| {
                end == 6 + count * 5
                    && (0..count).all(|offset| {
                        let start = 6 + offset * 5;
                        let pointer_valid = match record.integer(start) {
                            Some(0) => offset + 1 == count,
                            Some(_) => existing_pointer(record, start, entries).is_some(),
                            None => false,
                        };
                        pointer_valid
                            && record
                                .integer(start + 1)
                                .is_some_and(|location| matches!(location, 0..=5))
                            && (start + 2..=start + 4)
                                .all(|index| record.number(index).is_some_and(f64::is_finite))
                    })
            });
            let arrow_cardinality_valid = dimension_entry.is_none_or(|dimension| {
                if dimension.entity_type != 216 {
                    return true;
                }
                let arrow_count = records
                    .get(&dimension.sequence)
                    .map(|dimension_record| {
                        [2, 3]
                            .into_iter()
                            .filter_map(|index| existing_pointer(dimension_record, index, entries))
                            .filter(|sequence| {
                                entries.get(sequence).is_some_and(|leader| leader.form != 4)
                            })
                            .count()
                    })
                    .unwrap_or_default();
                arrow_count != 2 || geometry_count == Some(2)
            });
            let back_pointer_owners = records
                .iter()
                .filter_map(|(sequence, owner)| {
                    has_association_back_pointer(owner, entry.sequence, entries)
                        .then_some(*sequence)
                })
                .collect::<Vec<_>>();
            record.integer(1) == Some(1)
                && orientation_valid
                && angle_valid
                && geometry_valid
                && arrow_cardinality_valid
                && dimension.is_some_and(|dimension| back_pointer_owners == [dimension])
                && entry.status.subordinate == 1
        }
        _ => false,
    }
}

fn flow_associativity(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> Option<FlowAssociativity> {
    let form = entry.form;
    let context_count = if form == 18 { 2 } else { 1 };
    if record.integer(1) != Some(context_count) {
        return None;
    }
    let counts = (2..=7)
        .map(|index| record.count(index))
        .collect::<Option<Vec<_>>>()?;
    let _type_flag = record.integer(8).filter(|value| matches!(value, 0..=2))?;
    let function_flag = (form == 18)
        .then(|| record.integer(9).filter(|value| matches!(value, 0..=2)))
        .flatten();
    if form == 18 && function_flag.is_none() {
        return None;
    }
    let mut cursor = if form == 18 { 10 } else { 9 };
    let pointers = |cursor: &mut usize, count: usize, nullable: bool| {
        (0..count)
            .map(|_| {
                let raw = record.integer(*cursor)?;
                *cursor += 1;
                if nullable && raw == 0 {
                    return Some(None);
                }
                existing_pointer(record, *cursor - 1, entries).map(Some)
            })
            .collect::<Option<Vec<_>>>()
    };
    let associated = pointers(&mut cursor, counts[0], false)?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let connections = pointers(&mut cursor, counts[1], false)?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let joins = pointers(&mut cursor, counts[2], false)?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let _names = (0..counts[3])
        .map(|_| {
            let name = record
                .string(cursor)
                .filter(|name| !name.is_empty())?
                .to_vec();
            cursor += 1;
            Some(name)
        })
        .collect::<Option<Vec<_>>>()?;
    let displays = pointers(&mut cursor, counts[4], false)?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let continuations = pointers(&mut cursor, counts[5], true)?;
    let associated_valid = associated.iter().all(|sequence| {
        entries
            .get(sequence)
            .is_some_and(|target| target.entity_type == 402 && target.form == form)
    });
    let connections_valid = connections.iter().all(|sequence| {
        entries.get(sequence).is_some_and(|target| {
            if form == 18 {
                target.entity_type == 132
                    || (target.entity_type == 402 && matches!(target.form, 1 | 7 | 14 | 15))
            } else {
                target.entity_type == 132
            }
        }) && (form == 20
            || records.get(sequence).is_some_and(|member| {
                has_association_back_pointer(member, entry.sequence, entries)
            }))
    });
    let joins_valid = joins.iter().all(|sequence| {
        (form == 20
            || records.get(sequence).is_some_and(|member| {
                has_association_back_pointer(member, entry.sequence, entries)
            }))
            && entries
                .get(sequence)
                .is_some_and(|target| target.entity_type != 402 || target.form == 7)
    });
    let displays_valid = displays.iter().all(|sequence| {
        entries.get(sequence).is_some_and(|target| {
            target.entity_type == 312 || (form == 18 && target.entity_type == 212)
        })
    });
    let continuations_valid = continuations.iter().flatten().all(|sequence| {
        entries.get(sequence).is_some_and(|target| {
            target.entity_type == 402
                && if form == 18 {
                    matches!(target.form, 11 | 18)
                } else {
                    target.form == 20
                }
        }) && (form == 20
            || records.get(sequence).is_some_and(|member| {
                has_association_back_pointer(member, entry.sequence, entries)
            }))
    });
    (cursor == entity_parameter_end(record, entries)
        && associated_valid
        && connections_valid
        && joins_valid
        && displays_valid
        && continuations_valid)
        .then_some(FlowAssociativity {
            form,
            associated,
            continuations,
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
    let mut assemblies = BTreeMap::new();
    let mut attribute_shapes = BTreeMap::<u32, Vec<(i64, usize)>>::new();
    let flows = directory
        .iter()
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 18 | 20))
        .filter_map(|entry| {
            let record = records.get(&entry.sequence).copied()?;
            flow_associativity(entry, record, &entries, &records).map(|flow| (entry.sequence, flow))
        })
        .collect::<BTreeMap<_, _>>();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 406 && matches!(entry.form, 2 | 3 | 5..=15 | 18..=36))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let owners = records
            .iter()
            .filter_map(|(sequence, owner_record)| {
                (*sequence != entry.sequence
                    && has_property_pointer(owner_record, entry.sequence, &entries))
                .then_some(*sequence)
            })
            .collect::<Vec<_>>();
        let fields_valid = property_fields_valid(
            entry,
            record,
            entity_parameter_end(record, &entries),
            &entries,
        );
        let attachment_valid = entry.status.subordinate == 0 || !owners.is_empty();
        let reference_designator_valid = entry.form != 7
            || owners.iter().all(|owner| {
                entries
                    .get(owner)
                    .is_some_and(|owner| owner.entity_type != 420)
            });
        let owner_kind_valid = match entry.form {
            22 => {
                !owners.is_empty()
                    && owners.iter().all(|owner| {
                        entries
                            .get(owner)
                            .is_some_and(|owner| owner.entity_type == 404)
                    })
            }
            23 => {
                !owners.is_empty()
                    && owners.iter().all(|owner| {
                        entries.get(owner).is_some_and(|owner| {
                            owner.entity_type == 402 && matches!(owner.form, 1 | 7 | 14 | 15)
                        })
                    })
            }
            26 => {
                !owners.is_empty()
                    && owners.iter().all(|owner| {
                        entries
                            .get(owner)
                            .is_some_and(|owner| matches!(owner.entity_type, 116 | 132))
                    })
            }
            27 => entry.status.subordinate == 1 && owners.len() == 1,
            28 | 29 => {
                !owners.is_empty()
                    && owners.iter().all(|owner| {
                        entries
                            .get(owner)
                            .is_some_and(|owner| dimension_entity_type(owner.entity_type))
                    })
            }
            30 => {
                let note_count = record.count(13).unwrap_or_default();
                let owners_valid = !owners.is_empty()
                    && (note_count == 0 || owners.len() == 1)
                    && owners.iter().all(|owner| {
                        let Some(owner_entry) = entries.get(owner) else {
                            return false;
                        };
                        if !dimension_entity_type(owner_entry.entity_type) {
                            return false;
                        }
                        let Some(owner_record) = records.get(owner) else {
                            return false;
                        };
                        let groups = trailing_pointer_groups(owner_record, &entries);
                        let has_basic = groups.as_ref().is_some_and(|groups| {
                            groups.properties.iter().any(|sequence| {
                                entries.get(sequence).is_some_and(|property| {
                                    property.entity_type == 406 && property.form == 31
                                })
                            })
                        });
                        let display_count = groups.as_ref().map_or(0, |groups| {
                            groups
                                .properties
                                .iter()
                                .filter(|sequence| {
                                    entries.get(sequence).is_some_and(|property| {
                                        property.entity_type == 406 && property.form == 30
                                    })
                                })
                                .count()
                        });
                        let basic_consistent = (record.integer(2) == Some(2)) == has_basic;
                        let notes_valid = note_count == 0
                            || existing_pointer(owner_record, 1, &entries).is_some_and(|note| {
                                entries
                                    .get(&note)
                                    .is_some_and(|note| note.entity_type == 212)
                                    && records
                                        .get(&note)
                                        .and_then(|note| note.integer(1))
                                        .is_some_and(|text_count| {
                                            (0..note_count).all(|offset| {
                                                record
                                                    .integer(16 + offset * 3)
                                                    .is_some_and(|last| last <= text_count)
                                            })
                                        })
                            });
                        display_count == 1 && basic_consistent && notes_valid
                    });
                owners_valid
            }
            31 => {
                entry.status.subordinate == 1
                    && owners.len() == 1
                    && entries
                        .get(&owners[0])
                        .is_some_and(|owner| dimension_entity_type(owner.entity_type))
            }
            32 => {
                !owners.is_empty()
                    && owners.iter().all(|owner| {
                        entries
                            .get(owner)
                            .is_some_and(|owner| owner.entity_type == 404)
                    })
            }
            33 => {
                let identity = record.integer(2).zip(record.string(3));
                let unique_identity = identity.is_some_and(|identity| {
                    directory
                        .iter()
                        .filter(|candidate| candidate.entity_type == 406 && candidate.form == 33)
                        .filter(|candidate| {
                            records.get(&candidate.sequence).is_some_and(|candidate| {
                                candidate.integer(2).zip(candidate.string(3)) == Some(identity)
                            })
                        })
                        .count()
                        == 1
                });
                let sole_sheet_id = owners.first().is_some_and(|owner| {
                    records.get(owner).is_some_and(|owner_record| {
                        trailing_pointer_groups(owner_record, &entries).is_some_and(|groups| {
                            groups
                                .properties
                                .iter()
                                .filter(|sequence| {
                                    entries.get(sequence).is_some_and(|property| {
                                        property.entity_type == 406 && property.form == 33
                                    })
                                })
                                .count()
                                == 1
                        })
                    })
                });
                owners.len() == 1
                    && entries
                        .get(&owners[0])
                        .is_some_and(|owner| owner.entity_type == 404)
                    && unique_identity
                    && sole_sheet_id
            }
            34 => {
                owners.len() == 1
                    && entries
                        .get(&owners[0])
                        .is_some_and(|owner| owner.entity_type == 212)
            }
            35 => {
                owners.len() == 1
                    && entries
                        .get(&owners[0])
                        .is_some_and(|owner| matches!(owner.entity_type, 212 | 312))
            }
            36 => {
                let arity = record
                    .integer(1)
                    .and_then(|value| usize::try_from(value).ok());
                !owners.is_empty()
                    && owners.iter().all(|owner| {
                        entries
                            .get(owner)
                            .and_then(|owner| closure_owner_dimension(owner.entity_type))
                            == arity
                    })
            }
            _ => true,
        };
        if fields_valid && attachment_valid && reference_designator_valid && owner_kind_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "property value layout, attachment, or owner kind is invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 322 && matches!(entry.form, 0..=2))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let name_valid = matches!(
            record.tokens.get(1).map(|token| &token.value),
            Some(TokenValue::String(_) | TokenValue::Omitted)
        );
        let list_type_valid = record
            .integer(2)
            .is_some_and(|value| matches!(value, 0..=9999));
        let attribute_count = record.count(3).filter(|count| *count > 0);
        let mut cursor = 4;
        let mut attributes_valid = attribute_count.is_some();
        let mut shape = Vec::new();
        for _ in 0..attribute_count.unwrap_or_default() {
            let attribute_type_valid = record.integer(cursor).is_some();
            let data_type = record
                .integer(cursor + 1)
                .filter(|value| matches!(value, 1..=6));
            let value_count = match record.tokens.get(cursor + 2).map(|token| &token.value) {
                None | Some(TokenValue::Omitted) => Some(1),
                Some(TokenValue::Integer(value)) => {
                    usize::try_from(*value).ok().and_then(|count| {
                        (entry.form == 0 || count <= record.tokens.len().saturating_sub(cursor + 3))
                            .then_some(count)
                    })
                }
                Some(TokenValue::Real(_) | TokenValue::String(_)) => None,
            };
            cursor += 3;
            attributes_valid &=
                attribute_type_valid && data_type.is_some() && value_count.is_some();
            if let Some(descriptor) = data_type.zip(value_count) {
                shape.push(descriptor);
            }
            if entry.form != 0 {
                for _ in 0..value_count.unwrap_or_default() {
                    attributes_valid &= data_type.is_some_and(|data_type| {
                        attribute_value_valid(record, cursor, data_type, &entries)
                    });
                    cursor += 1;
                    if entry.form == 2 {
                        attributes_valid &= integer_or(record, cursor, 0).is_some_and(|value| {
                            value == 0
                                || u32::try_from(value).ok().is_some_and(|sequence| {
                                    entries
                                        .get(&sequence)
                                        .is_some_and(|target| target.entity_type == 312)
                                })
                        });
                        cursor += 1;
                    }
                }
            }
        }
        if name_valid && list_type_valid && attributes_valid {
            decoded.insert(entry.sequence);
            if entry.form == 0 {
                attribute_shapes.insert(entry.sequence, shape);
            }
        } else {
            losses.push(entity_loss(
                entry,
                "attribute-table definition header, value type, value, or display link is invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 422 && matches!(entry.form, 0..=1))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let definition = entry
            .structure
            .checked_neg()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|sequence| sequence % 2 == 1);
        let shape = definition.and_then(|sequence| attribute_shapes.get(&sequence));
        let declared_row_count = if entry.form == 0 {
            Some(1)
        } else {
            record.count(1).filter(|count| *count > 0)
        };
        let value_start = if entry.form == 0 { 1 } else { 2 };
        let values_per_row = shape.and_then(|shape| {
            shape
                .iter()
                .try_fold(0_usize, |total, (_, count)| total.checked_add(*count))
        });
        let row_count = declared_row_count
            .zip(values_per_row)
            .and_then(|(rows, width)| {
                let available = record.tokens.len().saturating_sub(value_start);
                (width == 0 || rows <= available / width).then_some(rows)
            });
        let mut cursor = value_start;
        let mut values_valid = shape.is_some() && row_count.is_some();
        for _ in 0..row_count.unwrap_or_default() {
            for (data_type, count) in shape.into_iter().flatten() {
                for _ in 0..*count {
                    values_valid &= attribute_value_valid(record, cursor, *data_type, &entries);
                    cursor += 1;
                }
            }
        }
        if values_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "attribute-table instance definition, row count, or typed value is invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 316 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let count = record.count(1).filter(|count| *count > 0);
        let mut types = BTreeSet::<Vec<u8>>::new();
        let units_valid = count.is_some_and(|count| {
            entity_parameter_end(record, &entries) == 2 + count * 3
                && (0..count).all(|offset| {
                    let start = 2 + offset * 3;
                    record
                        .string(start)
                        .zip(record.string(start + 1))
                        .is_some_and(|(unit_type, value)| {
                            unit_value_valid(unit_type, value) && types.insert(unit_type.to_vec())
                        })
                        && record
                            .number(start + 2)
                            .is_some_and(|scale| scale.is_finite() && scale > 0.0)
                })
        });
        let directory_valid = entry.status.use_flag == 2
            && entry.structure == 0
            && entry.line_font == 0
            && entry.level == 0
            && entry.view == 0
            && entry.transform == 0
            && entry.label_display == 0
            && entry.line_weight == 0
            && entry.color == 0;
        if units_valid && directory_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "units count, type/value pair, scale factor, uniqueness, or Directory fields are invalid",
            ));
        }
    }

    for entry in directory.iter().filter(|entry| entry.entity_type == 302) {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let class_count = record.count(1).filter(|count| *count > 0);
        let mut cursor = 2;
        let mut classes_valid = class_count.is_some();
        for _ in 0..class_count.unwrap_or_default() {
            classes_valid &= record
                .integer(cursor)
                .is_some_and(|value| matches!(value, 1..=2));
            classes_valid &= record
                .integer(cursor + 1)
                .is_some_and(|value| matches!(value, 1..=2));
            let item_count = record.count(cursor + 2).filter(|count| *count > 0);
            cursor += 3;
            for _ in 0..item_count.unwrap_or_default() {
                classes_valid &= record
                    .integer(cursor)
                    .is_some_and(|value| matches!(value, 1..=3));
                cursor += 1;
            }
            classes_valid &= item_count.is_some();
        }
        let directory_valid = matches!(entry.form, 5001..=9999)
            && entry.status.use_flag == 2
            && entry.structure == 0
            && entry.line_font == 0
            && entry.level == 0
            && entry.view == 0
            && entry.transform == 0
            && entry.label_display == 0
            && entry.line_weight == 0
            && entry.color == 0;
        if directory_valid && classes_valid && cursor == entity_parameter_end(record, &entries) {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "associativity form, class count, class flags, item layout, or Directory fields are invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 1 | 7 | 14 | 15))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let count = record.count(1).filter(|count| *count > 0);
        let members = count.and_then(|count| {
            (0..count)
                .map(|index| {
                    record.integer(2 + index).and_then(|value| {
                        let sequence = u32::try_from(value).ok()?;
                        (sequence % 2 == 1 && entries.contains_key(&sequence)).then_some(sequence)
                    })
                })
                .collect::<Option<Vec<_>>>()
        });
        let back_pointers_valid = members.as_ref().is_some_and(|members| {
            !matches!(entry.form, 1 | 14)
                || members.iter().all(|member| {
                    records.get(member).is_some_and(|member_record| {
                        has_association_back_pointer(member_record, entry.sequence, &entries)
                    })
                })
        });
        if members.is_some() && back_pointers_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "group member list or required association back pointer is invalid",
            ));
        }
    }

    for entry in directory.iter().filter(|entry| {
        entry.entity_type == 402 && matches!(entry.form, 5 | 6 | 9 | 12 | 13 | 16 | 21)
    }) {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        if entry.structure == 0 && predefined_associativity_valid(entry, record, &entries, &records)
        {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "predefined associativity counts, class layout, links, back pointers, or structure are invalid",
            ));
        }
    }

    let mut visited_flows = BTreeSet::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 18 | 20))
    {
        handled.insert(entry.sequence);
        let flow = flows.get(&entry.sequence);
        let flow_targets_valid = flow.is_some_and(|flow| {
            flow.form == entry.form
                && flow.associated.iter().all(|target| {
                    flows
                        .get(target)
                        .is_some_and(|target_flow| target_flow.form == flow.form)
                })
                && flow.continuations.iter().flatten().all(|target| {
                    entries.get(target).is_some_and(|target_entry| {
                        (flow.form == 18 && target_entry.form == 11)
                            || flows
                                .get(target)
                                .is_some_and(|target_flow| target_flow.form == flow.form)
                    })
                })
        });
        let cyclic = super::directed_cycle(entry.sequence, &mut visited_flows, |sequence| {
            flows
                .get(&sequence)
                .into_iter()
                .flat_map(|flow| flow.continuations.iter().flatten().copied())
                .filter(|target| flows.contains_key(target))
                .collect()
        });
        if entry.structure == 0 && flow_targets_valid && !cyclic {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "flow class counts, flags, typed links, required back pointers, continuation tree, or structure are invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 416 && matches!(entry.form, 0..=4))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let nonempty_string = |index| record.string(index).is_some_and(|value| !value.is_empty());
        let fields_valid = match entry.form {
            0 | 2 | 4 => nonempty_string(1) && nonempty_string(2),
            1 | 3 => nonempty_string(1),
            _ => false,
        };
        if fields_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "external-reference file, symbolic, or library identifier is empty or invalid",
            ));
        }
    }

    let array_targets = directory
        .iter()
        .filter(|entry| matches!(entry.entity_type, 412 | 414) && entry.form == 0)
        .filter_map(|entry| {
            records
                .get(&entry.sequence)
                .and_then(|record| record.integer(1))
                .and_then(|value| u32::try_from(value).ok())
                .map(|target| (entry.sequence, target))
        })
        .collect::<BTreeMap<_, _>>();
    let mut visited_arrays = BTreeSet::new();
    for entry in directory
        .iter()
        .filter(|entry| matches!(entry.entity_type, 412 | 414) && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let target_valid = array_targets.get(&entry.sequence).is_some_and(|target| {
            target % 2 == 1
                && entries
                    .get(target)
                    .is_some_and(|base| array_base_type(base.entity_type, base.form))
        });
        let cyclic = single_target_cycle(entry.sequence, &array_targets, &mut visited_arrays);
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
        let fields_valid = if entry.entity_type == 412 {
            let scale_valid =
                number_or(record, 2, 1.0).is_some_and(|value| value.is_finite() && value > 0.0);
            let coordinates_valid =
                (3..=5).all(|index| record.number(index).is_some_and(f64::is_finite));
            let columns = record
                .integer(6)
                .and_then(|value| usize::try_from(value).ok());
            let rows = record
                .integer(7)
                .and_then(|value| usize::try_from(value).ok());
            let dimensions = columns.zip(rows).and_then(|(columns, rows)| {
                (columns > 0 && rows > 0)
                    .then(|| columns.checked_mul(rows))
                    .flatten()
            });
            scale_valid
                && coordinates_valid
                && (8..=10).all(|index| record.number(index).is_some_and(f64::is_finite))
                && dimensions.is_some_and(|total| array_mask_valid(record, 11, 12, 13, total))
        } else {
            let locations = record
                .integer(2)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|count| *count > 0);
            (3..=5).all(|index| record.number(index).is_some_and(f64::is_finite))
                && record
                    .number(6)
                    .is_some_and(|value| value.is_finite() && value > 0.0)
                && (7..=8).all(|index| record.number(index).is_some_and(f64::is_finite))
                && locations.is_some_and(|total| array_mask_valid(record, 9, 10, 11, total))
        };
        if target_valid && !cyclic && transform_valid && fields_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "array base, dimensions, selection mask, transform, or acyclicity is invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 132 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let position_valid = (1..=3).all(|index| record.number(index).is_some_and(f64::is_finite));
        let optional_pointer_valid = |index: usize, entity_type: Option<i64>| {
            integer_or(record, index, 0).is_some_and(|value| {
                value == 0
                    || u32::try_from(value).ok().is_some_and(|sequence| {
                        sequence % 2 == 1
                            && entries.get(&sequence).is_some_and(|target| {
                                entity_type.is_none_or(|expected| target.entity_type == expected)
                            })
                    })
            })
        };
        let type_flag_valid = integer_or(record, 5, 0)
            .is_some_and(|value| matches!(value, 0..=2 | 101..=104 | 201..=203 | 5001..=9999));
        let function_flag_valid =
            integer_or(record, 6, 0).is_some_and(|value| matches!(value, 0..=2));
        let strings_valid = record.string(7).is_some() && record.string(9).is_some();
        let identifier_valid = record.integer(11).is_some();
        let function_code_valid = integer_or(record, 12, 0)
            .is_some_and(|value| matches!(value, 0..=49 | 98..=99 | 5001..=9999));
        let swap_valid = integer_or(record, 13, 0).is_some_and(|value| matches!(value, 0..=1));
        let owner_valid = integer_or(record, 14, 0).is_some_and(|value| {
            value == 0
                || u32::try_from(value).ok().is_some_and(|sequence| {
                    sequence % 2 == 1
                        && entries
                            .get(&sequence)
                            .is_some_and(|target| matches!(target.entity_type, 320 | 420))
                })
        });
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
        if position_valid
            && optional_pointer_valid(4, None)
            && type_flag_valid
            && function_flag_valid
            && strings_valid
            && optional_pointer_valid(8, Some(312))
            && optional_pointer_valid(10, Some(312))
            && identifier_valid
            && function_code_valid
            && swap_valid
            && owner_valid
            && transform_valid
            && entry.status.use_flag == 4
        {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "connect-point fields, references, placement, or use flag are invalid",
            ));
        }
    }

    let mut solid_instances = BTreeMap::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 430 && matches!(entry.form, 0 | 1))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let target = record.integer(1).and_then(|value| {
            let sequence = u32::try_from(value).ok()?;
            (sequence % 2 == 1).then_some(sequence)
        });
        if let Some(target) = target {
            solid_instances.insert(entry.sequence, target);
        } else {
            losses.push(entity_loss(
                entry,
                "solid-instance target pointer is invalid",
            ));
        }
    }

    let mut visited_instances = BTreeSet::new();
    for (sequence, target) in &solid_instances {
        let entry = entries[sequence];
        let target_valid = entries.get(target).is_some_and(|target_entry| {
            if entry.form == 1 {
                target_entry.entity_type == 186
            } else {
                matches!(
                    target_entry.entity_type,
                    150 | 152 | 154 | 156 | 158 | 160 | 162 | 164 | 168 | 180 | 184 | 430
                )
            }
        });
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
        let cyclic = single_target_cycle(*sequence, &solid_instances, &mut visited_instances);
        if target_valid && transform_valid && !cyclic {
            decoded.insert(*sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "solid-instance form, target, transform, or acyclicity is invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 184 && matches!(entry.form, 0 | 1))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(count) = record.count(1).filter(|count| *count > 0) else {
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
        let cyclic = super::directed_cycle(*sequence, &mut visited, |sequence| {
            assemblies
                .get(&sequence)
                .into_iter()
                .flat_map(|definition| definition.items.iter().map(|(item, _)| *item))
                .filter(|item| assemblies.contains_key(item))
                .collect()
        });
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

    let mut definitions = BTreeMap::new();
    let mut definition_fields_valid = BTreeSet::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 308 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let depth = record
            .integer(1)
            .and_then(|value| usize::try_from(value).ok());
        let name_valid = record.string(2).is_some_and(|name| !name.is_empty());
        let count = record.count(3);
        let members = count.and_then(|count| {
            (0..count)
                .map(|index| {
                    record.integer(4 + index).and_then(|value| {
                        let sequence = u32::try_from(value).ok()?;
                        (sequence % 2 == 1 && entries.contains_key(&sequence)).then_some(sequence)
                    })
                })
                .collect::<Option<Vec<_>>>()
        });
        let (Some(depth), Some(members)) = (depth, members) else {
            losses.push(entity_loss(
                entry,
                "subfigure depth, member count, or member pointer is invalid",
            ));
            continue;
        };
        definitions.insert(entry.sequence, SubfigureDefinition { depth, members });
        if name_valid && entry.status.use_flag == 2 && entry.transform == 0 {
            definition_fields_valid.insert(entry.sequence);
        }
    }

    let mut instances = BTreeMap::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 408 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let definition = record.integer(1).and_then(|value| {
            let sequence = u32::try_from(value).ok()?;
            (sequence % 2 == 1 && definitions.contains_key(&sequence)).then_some(sequence)
        });
        let translation_valid =
            (2..=4).all(|index| number_or(record, index, 0.0).is_some_and(f64::is_finite));
        let scale_valid =
            number_or(record, 5, 1.0).is_some_and(|value| value.is_finite() && value > 0.0);
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
        let Some(definition) = definition else {
            losses.push(entity_loss(
                entry,
                "subfigure-instance definition pointer is invalid",
            ));
            continue;
        };
        instances.insert(
            entry.sequence,
            SubfigureInstance {
                definition,
                valid_fields: translation_valid && scale_valid && transform_valid,
            },
        );
    }

    let mut network_definitions = BTreeMap::new();
    let mut network_definition_fields_valid = BTreeSet::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 320 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let depth = record
            .integer(1)
            .and_then(|value| usize::try_from(value).ok());
        let name_valid = record.string(2).is_some_and(|name| !name.is_empty());
        let member_count = record.count(3);
        let members = member_count.and_then(|count| {
            (0..count)
                .map(|index| {
                    record.integer(4 + index).and_then(|value| {
                        let sequence = u32::try_from(value).ok()?;
                        (sequence % 2 == 1 && entries.contains_key(&sequence)).then_some(sequence)
                    })
                })
                .collect::<Option<Vec<_>>>()
        });
        let Some((depth, member_count, members)) = depth
            .zip(member_count)
            .zip(members)
            .map(|((depth, member_count), members)| (depth, member_count, members))
        else {
            losses.push(entity_loss(
                entry,
                "network definition header or member list is invalid",
            ));
            continue;
        };
        let type_flag_valid = record
            .integer(4 + member_count)
            .is_some_and(|value| matches!(value, 0..=2));
        let designator_valid = record.string(5 + member_count).is_some();
        let display_valid = record.integer(6 + member_count).is_some_and(|value| {
            value == 0
                || u32::try_from(value).ok().is_some_and(|sequence| {
                    entries
                        .get(&sequence)
                        .is_some_and(|target| target.entity_type == 312)
                })
        });
        let connect_count = record.count(7 + member_count);
        let connect_points_valid = connect_count.is_some_and(|count| {
            (0..count).all(|index| {
                record
                    .integer(8 + member_count + index)
                    .is_some_and(|value| {
                        value == 0
                            || u32::try_from(value).ok().is_some_and(|sequence| {
                                entries
                                    .get(&sequence)
                                    .is_some_and(|target| target.entity_type == 132)
                            })
                    })
            })
        });
        let Some(connect_count) = connect_count else {
            losses.push(entity_loss(
                entry,
                "network definition connect-point count is invalid",
            ));
            continue;
        };
        network_definitions.insert(
            entry.sequence,
            NetworkDefinition {
                depth,
                members,
                connect_count,
            },
        );
        if name_valid
            && type_flag_valid
            && designator_valid
            && display_valid
            && connect_points_valid
            && entry.status.use_flag == 2
            && entry.transform == 0
        {
            network_definition_fields_valid.insert(entry.sequence);
        }
    }

    let mut network_instances = BTreeMap::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 420 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let definition = record.integer(1).and_then(|value| {
            let sequence = u32::try_from(value).ok()?;
            network_definitions
                .contains_key(&sequence)
                .then_some(sequence)
        });
        let translation_valid =
            (2..=4).all(|index| number_or(record, index, 0.0).is_some_and(f64::is_finite));
        let x_scale = number_or(record, 5, 1.0);
        let scales_valid = x_scale.is_some_and(|x_scale| {
            x_scale.is_finite()
                && x_scale > 0.0
                && number_or(record, 6, x_scale)
                    .is_some_and(|value| value.is_finite() && value > 0.0)
                && number_or(record, 7, x_scale)
                    .is_some_and(|value| value.is_finite() && value > 0.0)
        });
        let type_flag_valid = record.integer(8).is_none_or(|value| matches!(value, 0..=2));
        let designator_valid = record.string(9).is_some();
        let display_valid = record.integer(10).is_some_and(|value| {
            value == 0
                || u32::try_from(value).ok().is_some_and(|sequence| {
                    entries
                        .get(&sequence)
                        .is_some_and(|target| target.entity_type == 312)
                })
        });
        let connect_count = record.count(11);
        let connect_points_valid = connect_count.is_some_and(|count| {
            (0..count).all(|index| {
                record.integer(12 + index).is_some_and(|value| {
                    value == 0
                        || u32::try_from(value).ok().is_some_and(|sequence| {
                            entries
                                .get(&sequence)
                                .is_some_and(|target| target.entity_type == 132)
                        })
                })
            })
        });
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
        let (Some(definition), Some(connect_count)) = (definition, connect_count) else {
            losses.push(entity_loss(
                entry,
                "network instance definition or count is invalid",
            ));
            continue;
        };
        network_instances.insert(
            entry.sequence,
            NetworkInstance {
                definition,
                connect_count,
                valid_fields: translation_valid
                    && scales_valid
                    && type_flag_valid
                    && designator_valid
                    && display_valid
                    && connect_points_valid
                    && transform_valid,
            },
        );
    }

    let valid_instances = instances
        .iter()
        .filter_map(|(sequence, instance)| {
            (instance.valid_fields && definition_fields_valid.contains(&instance.definition))
                .then_some(*sequence)
        })
        .collect::<BTreeSet<_>>();
    for (sequence, definition) in &definitions {
        let entry = entries[sequence];
        let nesting_valid = definition.members.iter().all(|member| {
            let Some(member_entry) = entries.get(member) else {
                return false;
            };
            if !matches!(member_entry.entity_type, 408 | 420) {
                return true;
            }
            match member_entry.entity_type {
                408 => instances.get(member).is_some_and(|instance| {
                    valid_instances.contains(member)
                        && definitions
                            .get(&instance.definition)
                            .is_some_and(|child| child.depth < definition.depth)
                }),
                420 => network_instances.get(member).is_some_and(|instance| {
                    instance.valid_fields
                        && network_definition_fields_valid.contains(&instance.definition)
                        && network_definitions
                            .get(&instance.definition)
                            .is_some_and(|child| child.depth < definition.depth)
                }),
                _ => false,
            }
        });
        if definition_fields_valid.contains(sequence) && nesting_valid {
            decoded.insert(*sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "subfigure definition fields or nesting depth is invalid",
            ));
        }
    }
    for (sequence, instance) in &instances {
        let entry = entries[sequence];
        if valid_instances.contains(sequence) && decoded.contains(&instance.definition) {
            decoded.insert(*sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "subfigure-instance placement or decoded definition is invalid",
            ));
        }
    }
    for (sequence, definition) in &network_definitions {
        let entry = entries[sequence];
        let nesting_valid = definition.members.iter().all(|member| {
            let Some(member_entry) = entries.get(member) else {
                return false;
            };
            match member_entry.entity_type {
                408 => instances.get(member).is_some_and(|instance| {
                    instance.valid_fields
                        && definition_fields_valid.contains(&instance.definition)
                        && definitions
                            .get(&instance.definition)
                            .is_some_and(|child| child.depth < definition.depth)
                }),
                420 => network_instances.get(member).is_some_and(|instance| {
                    instance.valid_fields
                        && network_definition_fields_valid.contains(&instance.definition)
                        && network_definitions
                            .get(&instance.definition)
                            .is_some_and(|child| child.depth < definition.depth)
                }),
                _ => true,
            }
        });
        if network_definition_fields_valid.contains(sequence) && nesting_valid {
            decoded.insert(*sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "network definition fields or nesting depth is invalid",
            ));
        }
    }
    for (sequence, instance) in &network_instances {
        let entry = entries[sequence];
        let definition_valid = network_definitions
            .get(&instance.definition)
            .is_some_and(|definition| definition.connect_count == instance.connect_count);
        if instance.valid_fields && definition_valid && decoded.contains(&instance.definition) {
            decoded.insert(*sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "network instance placement or connection list is invalid",
            ));
        }
    }

    Projection {
        handled,
        decoded,
        losses,
    }
}
