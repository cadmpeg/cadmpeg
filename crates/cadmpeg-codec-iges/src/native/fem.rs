// SPDX-License-Identifier: Apache-2.0
//! Typed native records for the IGES finite-element entity family.

use crate::directory::DirectoryEntry;
use crate::graph::expectation::ReferenceExpectation;
use crate::graph::ParameterResolver;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::Serialize;
use std::collections::BTreeMap;

const FEM_NOTE_FORMS: &[i64] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 100, 101, 102, 105];
const FEM_RESULT_FORM_MAX: i64 = 34;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct NativeFemNodeSample {
    identifier: Option<i64>,
    node: Option<String>,
    translations: Vec<[Option<f64>; 3]>,
    rotations: Vec<[Option<f64>; 3]>,
    values: Vec<Option<f64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct NativeFemElementSample {
    identifier: Option<i64>,
    element: Option<String>,
    topology_type: Option<i64>,
    layers: Option<i64>,
    data_layer_flag: Option<i64>,
    report_locations: Vec<Option<i64>>,
    declared_value_count: Option<i64>,
    values: Vec<Option<f64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum NativeFemEntity {
    Node {
        id: String,
        source_entity: String,
        form: i64,
        node_number: i64,
        coordinates: [Option<f64>; 3],
        definition_transformation: Option<String>,
        displacement_transformation: Option<String>,
    },
    FiniteElement {
        id: String,
        source_entity: String,
        form: i64,
        element_number: i64,
        topology_type: Option<i64>,
        declared_node_count: Option<i64>,
        nodes: Vec<Option<String>>,
        element_type: Option<Vec<u8>>,
    },
    NodalDisplacementRotation {
        id: String,
        source_entity: String,
        form: i64,
        declared_case_count: Option<i64>,
        case_descriptions: Vec<Option<String>>,
        declared_node_count: Option<i64>,
        nodes: Vec<NativeFemNodeSample>,
    },
    NodalResults {
        id: String,
        source_entity: String,
        form: i64,
        analysis_case_number: i64,
        analysis_note: Option<String>,
        subcase_number: Option<i64>,
        time: Option<f64>,
        declared_value_count: Option<i64>,
        expected_value_count: Option<i64>,
        declared_node_count: Option<i64>,
        nodes: Vec<NativeFemNodeSample>,
    },
    ElementResults {
        id: String,
        source_entity: String,
        form: i64,
        analysis_case_number: i64,
        analysis_note: Option<String>,
        subcase_number: Option<i64>,
        time: Option<f64>,
        declared_value_count: Option<i64>,
        expected_value_count: Option<i64>,
        result_report_flag: Option<i64>,
        declared_element_count: Option<i64>,
        elements: Vec<NativeFemElementSample>,
    },
    NodalLoadConstraint {
        id: String,
        source_entity: String,
        form: i64,
        declared_case_count: Option<i64>,
        load_constraint_type: Option<i64>,
        node: Option<String>,
        case_references: Vec<Option<String>>,
    },
}

pub(crate) fn build(
    directory: &[DirectoryEntry],
    records: &BTreeMap<u32, &ParameterRecord>,
    resolver: &ParameterResolver<'_>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Vec<NativeFemEntity>, CodecError> {
    let mut result = Vec::new();
    for entry in directory.iter().filter(|entry| is_fem(entry)) {
        let record = records.get(&entry.sequence).copied();
        let native = match entry.entity_type {
            134 => node(entry, record, resolver),
            136 => finite_element(entry, record, resolver, ctx)?,
            138 => nodal_displacement_rotation(entry, record, resolver, ctx)?,
            146 => nodal_results(entry, record, resolver, ctx)?,
            148 => element_results(entry, record, resolver, ctx)?,
            418 => nodal_load_constraint(entry, record, resolver, ctx)?,
            _ => continue,
        };
        result.push(native);
    }
    Ok(result)
}

fn is_fem(entry: &DirectoryEntry) -> bool {
    matches!((entry.entity_type, entry.form), (134 | 136 | 138 | 418, 0))
        || (matches!(entry.entity_type, 146 | 148)
            && (0..=FEM_RESULT_FORM_MAX).contains(&entry.form))
}

fn source_entity(sequence: u32) -> String {
    format!("iges:entity:directory#{sequence}")
}

fn record_integer(record: Option<&ParameterRecord>, index: usize) -> Option<i64> {
    record.and_then(|record| record.integer(index))
}

fn record_number(record: Option<&ParameterRecord>, index: usize) -> Option<f64> {
    record.and_then(|record| record.number(index))
}

fn record_string(record: Option<&ParameterRecord>, index: usize) -> Option<Vec<u8>> {
    record
        .and_then(|record| record.string(index))
        .map(<[u8]>::to_vec)
}

fn record_has_token(record: Option<&ParameterRecord>, index: usize) -> bool {
    record.is_some_and(|record| record.token(index).is_some())
}

fn complete_count(
    record: Option<&ParameterRecord>,
    count_index: usize,
    item_start: usize,
    stride: usize,
) -> Option<usize> {
    let record = record?;
    let count = usize::try_from(record.integer(count_index)?).ok()?;
    let required = count.checked_mul(stride)?;
    let end = item_start.checked_add(required)?;
    (end <= record.parameter_end()).then_some(count)
}

fn charge_items(
    ctx: Option<&DecodeContext<'_>>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.map_or(Ok(()), |ctx| {
        ctx.charge_collection_items(count as u64, operation)
    })
}

fn entity_id(kind: &str, sequence: u32) -> String {
    format!("iges:fem:{kind}#D{sequence}")
}

fn resolved_id(sequence: u32) -> String {
    source_entity(sequence)
}

fn resolve_type(
    resolver: &ParameterResolver<'_>,
    source: u32,
    index: usize,
    raw_pointer: Option<i64>,
    entity_type: i64,
    forms: &[i64],
) -> Option<String> {
    resolver
        .resolve_type(source, index, raw_pointer?, entity_type, forms)
        .map(resolved_id)
}

fn resolve_note(
    resolver: &ParameterResolver<'_>,
    source: u32,
    index: usize,
    raw_pointer: Option<i64>,
) -> Option<String> {
    resolver
        .resolve(
            source,
            index,
            raw_pointer?,
            ReferenceExpectation::Type212GeneralNote,
            |target| target.entity_type == 212 && FEM_NOTE_FORMS.contains(&target.form),
        )
        .map(resolved_id)
}

fn resolve_transformation(
    resolver: &ParameterResolver<'_>,
    source: u32,
    index: usize,
    raw_pointer: i64,
) -> Option<String> {
    resolver
        .resolve(
            source,
            index,
            raw_pointer,
            ReferenceExpectation::Type124Transformation,
            |target| target.entity_type == 124,
        )
        .map(resolved_id)
}

fn node(
    entry: &DirectoryEntry,
    record: Option<&ParameterRecord>,
    resolver: &ParameterResolver<'_>,
) -> NativeFemEntity {
    let sequence = entry.sequence;
    NativeFemEntity::Node {
        id: entity_id("node", sequence),
        source_entity: source_entity(sequence),
        form: entry.form,
        node_number: entry.subscript,
        coordinates: [
            record_number(record, 1),
            record_number(record, 2),
            record_number(record, 3),
        ],
        definition_transformation: resolve_transformation(resolver, sequence, 7, entry.transform),
        displacement_transformation: resolve_type(
            resolver,
            sequence,
            4,
            record_integer(record, 4),
            124,
            &[10, 11, 12],
        ),
    }
}

fn finite_element(
    entry: &DirectoryEntry,
    record: Option<&ParameterRecord>,
    resolver: &ParameterResolver<'_>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<NativeFemEntity, CodecError> {
    let sequence = entry.sequence;
    let declared_node_count = record_integer(record, 2);
    let count = complete_count(record, 2, 3, 1).filter(|count| {
        count
            .checked_add(3)
            .is_some_and(|index| record_has_token(record, index))
    });
    let nodes = if let Some(count) = count {
        charge_items(ctx, count, "iges_fem_element_nodes")?;
        (0..count)
            .map(|offset| {
                let index = 3 + offset;
                resolve_type(
                    resolver,
                    sequence,
                    index,
                    record_integer(record, index),
                    134,
                    &[0],
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    let element_type = count.and_then(|count| record_string(record, 3 + count));
    Ok(NativeFemEntity::FiniteElement {
        id: entity_id("element", sequence),
        source_entity: source_entity(sequence),
        form: entry.form,
        element_number: entry.subscript,
        topology_type: record_integer(record, 1),
        declared_node_count,
        nodes,
        element_type,
    })
}

fn nodal_displacement_layout(
    record: Option<&ParameterRecord>,
) -> Option<(usize, usize, usize, usize)> {
    let record = record?;
    let case_count = usize::try_from(record.integer(1)?).ok()?;
    let node_count_index = 2usize.checked_add(case_count)?;
    let node_count = usize::try_from(record.integer(node_count_index)?).ok()?;
    let start = node_count_index.checked_add(1)?;
    let stride = case_count.checked_mul(6)?.checked_add(2)?;
    let end = start.checked_add(node_count.checked_mul(stride)?)?;
    (end <= record.parameter_end()).then_some((case_count, node_count, start, stride))
}

fn nodal_displacement_rotation(
    entry: &DirectoryEntry,
    record: Option<&ParameterRecord>,
    resolver: &ParameterResolver<'_>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<NativeFemEntity, CodecError> {
    let sequence = entry.sequence;
    let declared_case_count = record_integer(record, 1);
    let declared_node_count = record.and_then(|record| {
        let case_count = usize::try_from(record.integer(1)?).ok()?;
        record.integer(2 + case_count)
    });
    let layout = nodal_displacement_layout(record);
    let case_descriptions = if let Some((case_count, _, _, _)) = layout {
        charge_items(ctx, case_count, "iges_fem_displacement_cases")?;
        (0..case_count)
            .map(|offset| {
                let index = 2 + offset;
                resolve_note(resolver, sequence, index, record_integer(record, index))
            })
            .collect()
    } else {
        Vec::new()
    };
    let nodes = if let Some((case_count, node_count, start, stride)) = layout {
        charge_items(ctx, node_count, "iges_fem_displacement_nodes")?;
        charge_items(
            ctx,
            node_count.saturating_mul(case_count),
            "iges_fem_displacement_values",
        )?;
        (0..node_count)
            .map(|node_offset| {
                let base = start + node_offset * stride;
                let identifier = record_integer(record, base);
                let node = resolve_type(
                    resolver,
                    sequence,
                    base + 1,
                    record_integer(record, base + 1),
                    134,
                    &[0],
                );
                let mut translations = Vec::with_capacity(case_count);
                let mut rotations = Vec::with_capacity(case_count);
                for case in 0..case_count {
                    let values = base + 2 + case * 6;
                    translations.push([
                        record_number(record, values),
                        record_number(record, values + 1),
                        record_number(record, values + 2),
                    ]);
                    rotations.push([
                        record_number(record, values + 3),
                        record_number(record, values + 4),
                        record_number(record, values + 5),
                    ]);
                }
                NativeFemNodeSample {
                    identifier,
                    node,
                    translations,
                    rotations,
                    values: Vec::new(),
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(NativeFemEntity::NodalDisplacementRotation {
        id: entity_id("nodal-displacement-rotation", sequence),
        source_entity: source_entity(sequence),
        form: entry.form,
        declared_case_count,
        case_descriptions,
        declared_node_count,
        nodes,
    })
}

fn result_value_count(form: i64) -> Option<i64> {
    match form {
        0 => None,
        1 | 2 | 10 | 11 | 13 | 14 | 16 => Some(1),
        3 | 5 | 6 | 7 | 8 | 9 | 12 | 15 | 17..=22 => Some(3),
        4 | 23..=28 => Some(6),
        29..=34 => Some(9),
        _ => None,
    }
}

fn nodal_results(
    entry: &DirectoryEntry,
    record: Option<&ParameterRecord>,
    resolver: &ParameterResolver<'_>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<NativeFemEntity, CodecError> {
    let sequence = entry.sequence;
    let declared_value_count = record_integer(record, 4);
    let declared_node_count = record_integer(record, 5);
    let layout = match (
        declared_value_count.and_then(|value| usize::try_from(value).ok()),
        declared_node_count.and_then(|value| usize::try_from(value).ok()),
    ) {
        (Some(value_count), Some(node_count)) => {
            let stride = value_count.checked_add(2);
            let start: usize = 6;
            stride
                .and_then(|stride| node_count.checked_mul(stride))
                .and_then(|span| start.checked_add(span))
                .filter(|end| record.is_some_and(|record| *end <= record.parameter_end()))
                .and_then(|_| stride.map(|stride| (value_count, node_count, start, stride)))
        }
        _ => None,
    };
    let nodes = if let Some((value_count, node_count, start, stride)) = layout {
        charge_items(ctx, node_count, "iges_fem_nodal_result_nodes")?;
        charge_items(
            ctx,
            node_count.saturating_mul(value_count),
            "iges_fem_nodal_result_values",
        )?;
        (0..node_count)
            .map(|offset| {
                let base = start + offset * stride;
                NativeFemNodeSample {
                    identifier: record_integer(record, base),
                    node: resolve_type(
                        resolver,
                        sequence,
                        base + 1,
                        record_integer(record, base + 1),
                        134,
                        &[0],
                    ),
                    translations: Vec::new(),
                    rotations: Vec::new(),
                    values: (0..value_count)
                        .map(|value| record_number(record, base + 2 + value))
                        .collect(),
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(NativeFemEntity::NodalResults {
        id: entity_id("nodal-results", sequence),
        source_entity: source_entity(sequence),
        form: entry.form,
        analysis_case_number: entry.subscript,
        analysis_note: resolve_note(resolver, sequence, 1, record_integer(record, 1)),
        subcase_number: record_integer(record, 2),
        time: record_number(record, 3),
        declared_value_count,
        expected_value_count: result_value_count(entry.form),
        declared_node_count,
        nodes,
    })
}

fn element_results_layout(record: Option<&ParameterRecord>) -> Option<(usize, usize, usize)> {
    let record = record?;
    let value_count = usize::try_from(record.integer(4)?).ok()?;
    let element_count = usize::try_from(record.integer(6)?).ok()?;
    let mut cursor = 7usize;
    for _ in 0..element_count {
        cursor = element_result_item_layout(record, cursor)?.4;
    }
    (cursor <= record.parameter_end()).then_some((value_count, element_count, 7))
}

fn element_result_item_layout(
    record: &ParameterRecord,
    cursor: usize,
) -> Option<(usize, usize, usize, usize, usize)> {
    let report_location_count_index = cursor.checked_add(5)?;
    let report_location_count =
        usize::try_from(record.integer(report_location_count_index)?).ok()?;
    let report_start = cursor.checked_add(6)?;
    let value_count_index = report_start.checked_add(report_location_count)?;
    let result_count = usize::try_from(record.integer(value_count_index)?).ok()?;
    let next = value_count_index
        .checked_add(1)?
        .checked_add(result_count)?;
    (next <= record.parameter_end()).then_some((
        report_location_count,
        report_start,
        value_count_index,
        result_count,
        next,
    ))
}

fn element_results(
    entry: &DirectoryEntry,
    record: Option<&ParameterRecord>,
    resolver: &ParameterResolver<'_>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<NativeFemEntity, CodecError> {
    let sequence = entry.sequence;
    let declared_value_count = record_integer(record, 4);
    let declared_element_count = record_integer(record, 6);
    let layout = element_results_layout(record);
    let elements = if let Some((_, element_count, mut cursor)) = layout {
        charge_items(ctx, element_count, "iges_fem_element_result_elements")?;
        let mut elements = Vec::with_capacity(element_count);
        for _ in 0..element_count {
            let Some((report_location_count, report_start, value_count_index, result_count, next)) =
                record.and_then(|record| element_result_item_layout(record, cursor))
            else {
                break;
            };
            charge_items(
                ctx,
                report_location_count,
                "iges_fem_element_result_locations",
            )?;
            charge_items(ctx, result_count, "iges_fem_element_result_values")?;
            let report_locations = (0..report_location_count)
                .map(|offset| record_integer(record, report_start + offset))
                .collect();
            let values = (0..result_count)
                .map(|offset| record_number(record, value_count_index + 1 + offset))
                .collect();
            elements.push(NativeFemElementSample {
                identifier: record_integer(record, cursor),
                element: resolve_type(
                    resolver,
                    sequence,
                    cursor + 1,
                    record_integer(record, cursor + 1),
                    136,
                    &[0],
                ),
                topology_type: record_integer(record, cursor + 2),
                layers: record_integer(record, cursor + 3),
                data_layer_flag: record_integer(record, cursor + 4),
                report_locations,
                declared_value_count: record_integer(record, value_count_index),
                values,
            });
            cursor = next;
        }
        elements
    } else {
        Vec::new()
    };
    Ok(NativeFemEntity::ElementResults {
        id: entity_id("element-results", sequence),
        source_entity: source_entity(sequence),
        form: entry.form,
        analysis_case_number: entry.subscript,
        analysis_note: resolve_note(resolver, sequence, 1, record_integer(record, 1)),
        subcase_number: record_integer(record, 2),
        time: record_number(record, 3),
        declared_value_count,
        expected_value_count: result_value_count(entry.form),
        result_report_flag: record_integer(record, 5),
        declared_element_count,
        elements,
    })
}

fn nodal_load_constraint(
    entry: &DirectoryEntry,
    record: Option<&ParameterRecord>,
    resolver: &ParameterResolver<'_>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<NativeFemEntity, CodecError> {
    let sequence = entry.sequence;
    let declared_case_count = record_integer(record, 1);
    let case_references = if let Some(count) = complete_count(record, 1, 4, 1) {
        charge_items(ctx, count, "iges_fem_load_constraint_cases")?;
        (0..count)
            .map(|offset| {
                let index = 4 + offset;
                resolver
                    .resolve(
                        sequence,
                        index,
                        record_integer(record, index)?,
                        ReferenceExpectation::Type406Form11OrType212GeneralNote,
                        |target| {
                            (target.entity_type == 406 && target.form == 11)
                                || (target.entity_type == 212
                                    && FEM_NOTE_FORMS.contains(&target.form))
                        },
                    )
                    .map(resolved_id)
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(NativeFemEntity::NodalLoadConstraint {
        id: entity_id("nodal-load-constraint", sequence),
        source_entity: source_entity(sequence),
        form: entry.form,
        declared_case_count,
        load_constraint_type: record_integer(record, 2),
        node: resolve_type(resolver, sequence, 3, record_integer(record, 3), 134, &[0]),
        case_references,
    })
}

#[cfg(test)]
mod tests {
    use super::result_value_count;

    #[test]
    fn result_forms_use_the_standard_value_arities() {
        assert_eq!(result_value_count(0), None);
        for form in [1, 2, 10, 11, 13, 14, 16] {
            assert_eq!(result_value_count(form), Some(1), "Form {form}");
        }
        for form in [3, 5, 6, 7, 8, 9, 12, 15, 17, 18, 19, 20, 21, 22] {
            assert_eq!(result_value_count(form), Some(3), "Form {form}");
        }
        for form in [4, 23, 24, 25, 26, 27, 28] {
            assert_eq!(result_value_count(form), Some(6), "Form {form}");
        }
        for form in [29, 30, 31, 32, 33, 34] {
            assert_eq!(result_value_count(form), Some(9), "Form {form}");
        }
        assert_eq!(result_value_count(35), None);
    }
}
