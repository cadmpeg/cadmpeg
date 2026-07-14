// SPDX-License-Identifier: Apache-2.0
//! Product definitions, occurrences, and ordered assembly relationships.

use super::geometry::{entity_loss, resolve_transform, Projection};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::{ParameterRecord, TokenValue};
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

fn single_target_cycle(
    sequence: u32,
    targets: &BTreeMap<u32, u32>,
    visiting: &mut BTreeSet<u32>,
    visited: &mut BTreeSet<u32>,
) -> bool {
    if visited.contains(&sequence) {
        return false;
    }
    if !visiting.insert(sequence) {
        return true;
    }
    if targets.get(&sequence).is_some_and(|target| {
        targets.contains_key(target) && single_target_cycle(*target, targets, visiting, visited)
    }) {
        return true;
    }
    visiting.remove(&sequence);
    visited.insert(sequence);
    false
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
    let Some(count) = record
        .integer(count_index)
        .and_then(|value| usize::try_from(value).ok())
    else {
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
    (1..record.tokens.len()).any(|association_count_index| {
        let Some(association_count) = record
            .integer(association_count_index)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|count| *count > 0)
        else {
            return false;
        };
        let association_start = association_count_index + 1;
        let property_count_index = association_start + association_count;
        let Some(property_count) = record
            .integer(property_count_index)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return false;
        };
        if property_count_index + 1 + property_count != record.tokens.len() {
            return false;
        }
        let associations = (0..association_count)
            .map(|index| record.integer(association_start + index))
            .collect::<Option<Vec<_>>>();
        let properties_valid = (0..property_count).all(|index| {
            record
                .integer(property_count_index + 1 + index)
                .and_then(|value| u32::try_from(value).ok())
                .and_then(|sequence| entries.get(&sequence))
                .is_some_and(|target| matches!(target.entity_type, 322 | 406 | 422))
        });
        properties_valid
            && associations.is_some_and(|associations| {
                associations
                    .iter()
                    .any(|value| *value == i64::from(group_sequence))
                    && associations.iter().all(|value| {
                        u32::try_from(*value)
                            .ok()
                            .and_then(|sequence| entries.get(&sequence))
                            .is_some_and(|target| matches!(target.entity_type, 212 | 312 | 402))
                    })
            })
    })
}

fn has_property_pointer(
    record: &ParameterRecord,
    property_sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    (1..record.tokens.len()).any(|association_count_index| {
        let Some(association_count) = record
            .integer(association_count_index)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return false;
        };
        let association_start = association_count_index + 1;
        let property_count_index = association_start + association_count;
        let Some(property_count) = record
            .integer(property_count_index)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|count| *count > 0)
        else {
            return false;
        };
        if property_count_index + 1 + property_count != record.tokens.len() {
            return false;
        }
        let associations_valid = (0..association_count).all(|index| {
            record
                .integer(association_start + index)
                .and_then(|value| u32::try_from(value).ok())
                .and_then(|sequence| entries.get(&sequence))
                .is_some_and(|target| matches!(target.entity_type, 212 | 312 | 402))
        });
        associations_valid
            && (0..property_count).all(|index| {
                record
                    .integer(property_count_index + 1 + index)
                    .and_then(|value| u32::try_from(value).ok())
                    .and_then(|sequence| entries.get(&sequence))
                    .is_some_and(|target| matches!(target.entity_type, 322 | 406 | 422))
            })
            && (0..property_count).any(|index| {
                record.integer(property_count_index + 1 + index)
                    == Some(i64::from(property_sequence))
            })
    })
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
    let mut attribute_shapes = BTreeMap::<u32, Vec<(i64, usize)>>::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 406 && matches!(entry.form, 7 | 15))
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
        let fields_valid =
            record.integer(1) == Some(1) && record.string(2).is_some_and(|value| !value.is_empty());
        let attachment_valid = entry.status.subordinate == 0 || !owners.is_empty();
        let reference_designator_valid = entry.form != 7
            || owners.iter().all(|owner| {
                entries
                    .get(owner)
                    .is_some_and(|owner| owner.entity_type != 420)
            });
        if fields_valid && attachment_valid && reference_designator_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "product property value, attachment, or owner kind is invalid",
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
        let attribute_count = record
            .integer(3)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|count| *count > 0);
        let mut cursor = 4;
        let mut attributes_valid = attribute_count.is_some();
        let mut shape = Vec::new();
        for _ in 0..attribute_count.unwrap_or_default() {
            let attribute_type_valid = record.integer(cursor).is_some();
            let data_type = record
                .integer(cursor + 1)
                .filter(|value| matches!(value, 1..=6));
            let value_count =
                integer_or(record, cursor + 2, 1).and_then(|value| usize::try_from(value).ok());
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
        let row_count = if entry.form == 0 {
            Some(1)
        } else {
            record
                .integer(1)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|count| *count > 0)
        };
        let value_start = if entry.form == 0 { 1 } else { 2 };
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
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 1 | 7 | 14 | 15))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let count = record
            .integer(1)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|count| *count > 0);
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
            _ => unreachable!("filtered external-reference form"),
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
        let cyclic = single_target_cycle(
            entry.sequence,
            &array_targets,
            &mut BTreeSet::new(),
            &mut visited_arrays,
        );
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
        let cyclic = single_target_cycle(
            *sequence,
            &solid_instances,
            &mut BTreeSet::new(),
            &mut visited_instances,
        );
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
        let count = record
            .integer(3)
            .and_then(|value| usize::try_from(value).ok());
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
        let member_count = record
            .integer(3)
            .and_then(|value| usize::try_from(value).ok());
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
        let connect_count = record
            .integer(7 + member_count)
            .and_then(|value| usize::try_from(value).ok());
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
        let connect_count = record
            .integer(11)
            .and_then(|value| usize::try_from(value).ok());
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
                _ => unreachable!(),
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
