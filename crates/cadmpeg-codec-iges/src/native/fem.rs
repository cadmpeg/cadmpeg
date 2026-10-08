// SPDX-License-Identifier: Apache-2.0
//! Typed native records for the IGES finite-element entity family.

use crate::directory::DirectoryEntry;
use crate::graph::expectation::{ExpectationLabel, ReferenceExpectation};
use crate::graph::ParameterResolver;
use crate::parameter::{record_by_sequence, ParameterRecord};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::Serialize;

const FEM_NOTE_FORMS: &[i64] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 100, 101, 102, 105];
const FEM_RESULT_FORM_MAX: i64 = 34;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(in crate::native) struct NativeFemNodeSample {
    identifier: Option<i64>,
    node: Option<String>,
    translations: Vec<[Option<f64>; 3]>,
    rotations: Vec<[Option<f64>; 3]>,
    values: Vec<Option<f64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(in crate::native) struct NativeFemElementSample {
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
pub(in crate::native) enum NativeFemEntity {
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

pub(super) fn build(
    directory: &[DirectoryEntry],
    records: &[ParameterRecord],
    resolver: &ParameterResolver<'_, '_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<NativeFemEntity>, CodecError> {
    let mut result = Vec::new();
    for entry in ctx
        .admit_iter(directory, "iges FEM directory scan")?
        .filter(|entry| is_fem(entry))
    {
        ctx.reserve_vec(&mut result, 1, "iges FEM native entities")?;
        let record = record_by_sequence(records, entry.sequence, ctx)?;
        let native = match entry.entity_type {
            134 => node(entry, record, resolver, ctx)?,
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

fn source_entity(ctx: &DecodeContext<'_>, sequence: u32) -> Result<String, CodecError> {
    ctx.format_retained(
        format_args!("iges:entity:directory#{sequence}"),
        "iges FEM source entity id",
    )
}

fn record_integer(record: Option<&ParameterRecord>, index: usize) -> Option<i64> {
    record.and_then(|record| record.integer(index))
}

fn record_number(record: Option<&ParameterRecord>, index: usize) -> Option<f64> {
    record.and_then(|record| record.number(index))
}

fn record_string(
    ctx: &DecodeContext<'_>,
    record: Option<&ParameterRecord>,
    index: usize,
) -> Result<Option<Vec<u8>>, CodecError> {
    record
        .and_then(|record| record.string(index))
        .map(|bytes| ctx.copy_retained(bytes, "iges FEM parameter string"))
        .transpose()
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

fn entity_id(ctx: &DecodeContext<'_>, kind: &str, sequence: u32) -> Result<String, CodecError> {
    ctx.format_retained(
        format_args!("iges:fem:{kind}#D{sequence}"),
        "iges FEM entity id",
    )
}

fn resolved_id(
    ctx: &DecodeContext<'_>,
    resolved: Option<u32>,
) -> Result<Option<String>, CodecError> {
    resolved
        .map(|sequence| source_entity(ctx, sequence))
        .transpose()
}

fn resolve_type(
    ctx: &DecodeContext<'_>,
    resolver: &ParameterResolver<'_, '_>,
    source: u32,
    index: usize,
    raw_pointer: Option<i64>,
    entity_type: i64,
    forms: &[i64],
) -> Result<Option<String>, CodecError> {
    let Some(raw_pointer) = raw_pointer else {
        return Ok(None);
    };
    resolved_id(
        ctx,
        resolver.resolve_type(source, index, raw_pointer, entity_type, forms)?,
    )
}

fn resolve_note(
    ctx: &DecodeContext<'_>,
    resolver: &ParameterResolver<'_, '_>,
    source: u32,
    index: usize,
    raw_pointer: Option<i64>,
) -> Result<Option<String>, CodecError> {
    let Some(raw_pointer) = raw_pointer else {
        return Ok(None);
    };
    resolved_id(
        ctx,
        resolver.resolve(
            source,
            index,
            raw_pointer,
            ReferenceExpectation::Named(ExpectationLabel::Type212GeneralNote),
            |target| target.entity_type == 212 && FEM_NOTE_FORMS.contains(&target.form),
        )?,
    )
}

fn resolve_transformation(
    ctx: &DecodeContext<'_>,
    resolver: &ParameterResolver<'_, '_>,
    source: u32,
    index: usize,
    raw_pointer: i64,
) -> Result<Option<String>, CodecError> {
    resolved_id(
        ctx,
        resolver.resolve(
            source,
            index,
            raw_pointer,
            ReferenceExpectation::Named(ExpectationLabel::Type124Transformation),
            |target| target.entity_type == 124,
        )?,
    )
}

fn node(
    entry: &DirectoryEntry,
    record: Option<&ParameterRecord>,
    resolver: &ParameterResolver<'_, '_>,
    ctx: &DecodeContext<'_>,
) -> Result<NativeFemEntity, CodecError> {
    let sequence = entry.sequence;
    Ok(NativeFemEntity::Node {
        id: entity_id(ctx, "node", sequence)?,
        source_entity: source_entity(ctx, sequence)?,
        form: entry.form,
        node_number: entry.subscript,
        coordinates: [
            record_number(record, 1),
            record_number(record, 2),
            record_number(record, 3),
        ],
        definition_transformation: resolve_transformation(
            ctx,
            resolver,
            sequence,
            7,
            entry.transform,
        )?,
        displacement_transformation: resolve_type(
            ctx,
            resolver,
            sequence,
            4,
            record_integer(record, 4),
            124,
            &[10, 11, 12],
        )?,
    })
}

fn finite_element(
    entry: &DirectoryEntry,
    record: Option<&ParameterRecord>,
    resolver: &ParameterResolver<'_, '_>,
    ctx: &DecodeContext<'_>,
) -> Result<NativeFemEntity, CodecError> {
    let sequence = entry.sequence;
    let declared_node_count = record_integer(record, 2);
    let count = complete_count(record, 2, 3, 1).filter(|count| {
        count
            .checked_add(3)
            .is_some_and(|index| record_has_token(record, index))
    });
    let nodes = if let Some(count) = count {
        ctx.collect_indexed_vec(count, "iges_fem_element_nodes", |offset| {
            let index = 3 + offset;
            resolve_type(
                ctx,
                resolver,
                sequence,
                index,
                record_integer(record, index),
                134,
                &[0],
            )
        })?
    } else {
        Vec::new()
    };
    let element_type = count
        .map(|count| record_string(ctx, record, 3 + count))
        .transpose()?
        .flatten();
    Ok(NativeFemEntity::FiniteElement {
        id: entity_id(ctx, "element", sequence)?,
        source_entity: source_entity(ctx, sequence)?,
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
    resolver: &ParameterResolver<'_, '_>,
    ctx: &DecodeContext<'_>,
) -> Result<NativeFemEntity, CodecError> {
    let sequence = entry.sequence;
    let declared_case_count = record_integer(record, 1);
    let declared_node_count = record.and_then(|record| {
        let case_count = usize::try_from(record.integer(1)?).ok()?;
        record.integer(2 + case_count)
    });
    let layout = nodal_displacement_layout(record);
    let case_descriptions = if let Some((case_count, _, _, _)) = layout {
        ctx.collect_indexed_vec(case_count, "iges_fem_displacement_cases", |offset| {
            let index = 2 + offset;
            resolve_note(
                ctx,
                resolver,
                sequence,
                index,
                record_integer(record, index),
            )
        })?
    } else {
        Vec::new()
    };
    let nodes = if let Some((case_count, node_count, start, stride)) = layout {
        ctx.collect_indexed_vec(
            node_count,
            "iges_fem_displacement_nodes",
            |node_offset| -> Result<NativeFemNodeSample, CodecError> {
                let base = start + node_offset * stride;
                let identifier = record_integer(record, base);
                let node = resolve_type(
                    ctx,
                    resolver,
                    sequence,
                    base + 1,
                    record_integer(record, base + 1),
                    134,
                    &[0],
                )?;
                let mut translations = ctx.collection_vec(case_count, "iges FEM translations")?;
                let mut rotations = ctx.collection_vec(case_count, "iges FEM rotations")?;
                for case in ctx.admit_iter(0..case_count, "iges FEM displacement case scan")? {
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
                Ok(NativeFemNodeSample {
                    identifier,
                    node,
                    translations,
                    rotations,
                    values: Vec::new(),
                })
            },
        )?
    } else {
        Vec::new()
    };
    Ok(NativeFemEntity::NodalDisplacementRotation {
        id: entity_id(ctx, "nodal-displacement-rotation", sequence)?,
        source_entity: source_entity(ctx, sequence)?,
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
    resolver: &ParameterResolver<'_, '_>,
    ctx: &DecodeContext<'_>,
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
        ctx.collect_indexed_vec(
            node_count,
            "iges_fem_nodal_result_nodes",
            |offset| -> Result<NativeFemNodeSample, CodecError> {
                let base = start + offset * stride;
                Ok(NativeFemNodeSample {
                    identifier: record_integer(record, base),
                    node: resolve_type(
                        ctx,
                        resolver,
                        sequence,
                        base + 1,
                        record_integer(record, base + 1),
                        134,
                        &[0],
                    )?,
                    translations: Vec::new(),
                    rotations: Vec::new(),
                    values: ctx.collect_indexed_vec(
                        value_count,
                        "iges_fem_nodal_result_values",
                        |value| Ok(record_number(record, base + 2 + value)),
                    )?,
                })
            },
        )?
    } else {
        Vec::new()
    };
    Ok(NativeFemEntity::NodalResults {
        id: entity_id(ctx, "nodal-results", sequence)?,
        source_entity: source_entity(ctx, sequence)?,
        form: entry.form,
        analysis_case_number: entry.subscript,
        analysis_note: resolve_note(ctx, resolver, sequence, 1, record_integer(record, 1))?,
        subcase_number: record_integer(record, 2),
        time: record_number(record, 3),
        declared_value_count,
        expected_value_count: result_value_count(entry.form),
        declared_node_count,
        nodes,
    })
}

fn element_results(
    entry: &DirectoryEntry,
    record: Option<&ParameterRecord>,
    resolver: &ParameterResolver<'_, '_>,
    ctx: &DecodeContext<'_>,
) -> Result<NativeFemEntity, CodecError> {
    let sequence = entry.sequence;
    let declared_value_count = record_integer(record, 4);
    let declared_element_count = record_integer(record, 6);
    let (mut layouts, mut layout_storage) =
        ctx.temporary_vec(0, "iges FEM element result layouts")?;
    let element_count = declared_value_count
        .and_then(|count| usize::try_from(count).ok())
        .and(declared_element_count.and_then(|count| usize::try_from(count).ok()));
    let mut cursor = 7_usize;
    let complete = if let Some(element_count) = element_count {
        ctx.all_by(
            0..element_count,
            |_| {
                let layout = record.and_then(|record| {
                    let count_index = cursor.checked_add(5)?;
                    let report_count = usize::try_from(record.integer(count_index)?).ok()?;
                    let report_start = cursor.checked_add(6)?;
                    let value_count_index = report_start.checked_add(report_count)?;
                    let value_count = usize::try_from(record.integer(value_count_index)?).ok()?;
                    let next = value_count_index.checked_add(1)?.checked_add(value_count)?;
                    (next <= record.parameter_end()).then_some((
                        report_count,
                        report_start,
                        value_count_index,
                        value_count,
                        next,
                    ))
                });
                let Some((report_count, report_start, value_count_index, value_count, next)) =
                    layout
                else {
                    return Ok(false);
                };
                ctx.push_scoped_vec(
                    &mut layout_storage,
                    &mut layouts,
                    (
                        cursor,
                        report_count,
                        report_start,
                        value_count_index,
                        value_count,
                    ),
                    "iges FEM element result layouts",
                )?;
                cursor = next;
                Ok(true)
            },
            "iges FEM element result layout scan",
        )? && record.is_some_and(|record| cursor <= record.parameter_end())
    } else {
        false
    };
    let elements = if complete {
        ctx.try_collect_retained_with::<_, _, CodecError>(
            layouts,
            "iges_fem_element_result_elements",
            |(cursor, report_count, report_start, value_count_index, value_count)| {
                let report_locations = ctx.collect_indexed_vec(
                    report_count,
                    "iges_fem_element_result_locations",
                    |offset| Ok(record_integer(record, report_start + offset)),
                )?;
                let values = ctx.collect_indexed_vec(
                    value_count,
                    "iges_fem_element_result_values",
                    |offset| Ok(record_number(record, value_count_index + 1 + offset)),
                )?;
                Ok(NativeFemElementSample {
                    identifier: record_integer(record, cursor),
                    element: resolve_type(
                        ctx,
                        resolver,
                        sequence,
                        cursor + 1,
                        record_integer(record, cursor + 1),
                        136,
                        &[0],
                    )?,
                    topology_type: record_integer(record, cursor + 2),
                    layers: record_integer(record, cursor + 3),
                    data_layer_flag: record_integer(record, cursor + 4),
                    report_locations,
                    declared_value_count: record_integer(record, value_count_index),
                    values,
                })
            },
        )?
    } else {
        Vec::new()
    };
    Ok(NativeFemEntity::ElementResults {
        id: entity_id(ctx, "element-results", sequence)?,
        source_entity: source_entity(ctx, sequence)?,
        form: entry.form,
        analysis_case_number: entry.subscript,
        analysis_note: resolve_note(ctx, resolver, sequence, 1, record_integer(record, 1))?,
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
    resolver: &ParameterResolver<'_, '_>,
    ctx: &DecodeContext<'_>,
) -> Result<NativeFemEntity, CodecError> {
    let sequence = entry.sequence;
    let declared_case_count = record_integer(record, 1);
    let case_references = if let Some(count) = complete_count(record, 1, 4, 1) {
        ctx.collect_indexed_vec(
            count,
            "iges_fem_load_constraint_cases",
            |offset| -> Result<Option<String>, CodecError> {
                let index = 4 + offset;
                let Some(raw_pointer) = record_integer(record, index) else {
                    return Ok(None);
                };
                resolved_id(
                    ctx,
                    resolver.resolve(
                        sequence,
                        index,
                        raw_pointer,
                        ReferenceExpectation::Named(
                            ExpectationLabel::Type406Form11OrType212GeneralNote,
                        ),
                        |target| {
                            (target.entity_type == 406 && target.form == 11)
                                || (target.entity_type == 212
                                    && FEM_NOTE_FORMS.contains(&target.form))
                        },
                    )?,
                )
            },
        )?
    } else {
        Vec::new()
    };
    Ok(NativeFemEntity::NodalLoadConstraint {
        id: entity_id(ctx, "nodal-load-constraint", sequence)?,
        source_entity: source_entity(ctx, sequence)?,
        form: entry.form,
        declared_case_count,
        load_constraint_type: record_integer(record, 2),
        node: resolve_type(
            ctx,
            resolver,
            sequence,
            3,
            record_integer(record, 3),
            134,
            &[0],
        )?,
        case_references,
    })
}

#[cfg(test)]
mod tests {
    use super::result_value_count;

    fn integer_record(sequence: u32, values: &[i64]) -> crate::parameter::ParameterRecord {
        use crate::parameter::{Token, TokenValue};

        crate::parameter::ParameterRecord::from_test_tokens(
            sequence,
            1..2,
            Vec::new(),
            values.len(),
            values
                .iter()
                .copied()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            Vec::new(),
        )
    }

    fn refusal_at(
        directory: &[crate::directory::DirectoryEntry],
        records: &[crate::parameter::ParameterRecord],
        dimension: cadmpeg_core::decode::ResourceDimension,
        operation: &str,
    ) -> cadmpeg_core::CodecError {
        use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
        cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                _ => panic!("test dimension"),
            }
            crate::test_support::with_policy_context(&[], &policy, |ctx| {
                let resolver = crate::graph::ParameterResolver::new(directory, ctx)?;
                super::build(directory, records, &resolver, ctx)
            })
        })
    }

    #[test]
    fn fem_directory_scan_refuses_before_filtering_a_non_fem_entry() {
        use crate::graph::ParameterResolver;
        use crate::test_support::directory_target;
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;

        let directory = [directory_target(1, 116)];
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "iges FEM directory scan",
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_work_units = cap;
                crate::test_support::with_policy_context(&[], &policy, |ctx| {
                    let resolver = ParameterResolver::new(&[], ctx)?;
                    super::build(&directory, &[], &resolver, ctx)
                })
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "iges FEM directory scan"
                && limit.used == 0 && limit.additional == 1));
        crate::test_support::with_service_context(&[], |ctx| {
            let resolver = ParameterResolver::new(&[], ctx).expect("empty parameter resolver");
            assert!(super::build(&directory, &[], &resolver, ctx)
                .expect("non-FEM directory entry")
                .is_empty());
        });
    }

    #[test]
    fn fem_element_nodes_refuse_collection_limit_before_allocating_list() {
        use super::build;
        use crate::graph::ParameterResolver;
        use crate::test_support::directory_target;
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let directory = [directory_target(1, 134), directory_target(3, 136)];
        let element = integer_record(3, &[136, 1, 1, 1, 0]);
        let records = [element];
        let result = refusal_at(
            &directory,
            &records,
            ResourceDimension::CollectionItems,
            "iges_fem_element_nodes",
        );
        assert!(matches!(
            result,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.additional == 1
                    && limit.operation == "iges_fem_element_nodes"
        ));

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let resolver = ParameterResolver::new(&directory, &ctx).expect("directory index");
        assert_eq!(
            build(&directory, &records, &resolver, &ctx)
                .expect("FEM entities")
                .len(),
            2
        );
    }

    #[test]
    fn fem_rotation_lane_has_its_own_collection_admission() {
        use super::build;
        use crate::graph::ParameterResolver;
        use crate::test_support::directory_target;
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let directory = [directory_target(1, 138)];
        let displacement = integer_record(1, &[138, 1, 0, 1, 1, 0, 1, 2, 3, 4, 5, 6]);
        let records = [displacement];
        let result = refusal_at(
            &directory,
            &records,
            ResourceDimension::CollectionItems,
            "iges FEM rotations",
        );
        assert!(matches!(
            result,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.additional == 1
                    && limit.operation == "iges FEM rotations"
        ));

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let resolver = ParameterResolver::new(&directory, &ctx).expect("directory index");
        assert_eq!(
            build(&directory, &records, &resolver, &ctx)
                .expect("FEM displacement")
                .len(),
            1
        );
    }

    #[test]
    fn fem_entity_id_refuses_retained_limit_before_formatting() {
        use super::build;
        use crate::graph::ParameterResolver;
        use crate::test_support::directory_target;
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let directory = [directory_target(1, 134)];
        let result = refusal_at(
            &directory,
            &[],
            ResourceDimension::RetainedBytes,
            "iges FEM entity id",
        );
        assert!(matches!(
            result,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.additional == cadmpeg_core::decode::u64_from_index(b"iges:fem:node#D1".len())
                    && limit.operation == "iges FEM entity id"
        ));

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let resolver = ParameterResolver::new(&directory, &ctx).expect("directory index");
        assert_eq!(
            build(&directory, &[], &resolver, &ctx)
                .expect("FEM entity")
                .len(),
            1
        );
    }

    #[test]
    fn fem_parameter_string_refuses_retained_limit_before_copy() {
        use super::build;
        use crate::graph::ParameterResolver;
        use crate::parameter::{ParameterRecord, Token, TokenValue};
        use crate::test_support::directory_target;
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let directory = [directory_target(1, 136)];
        let tokens = [
            TokenValue::Integer(136),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::String(b"BEAM".to_vec()),
        ]
        .into_iter()
        .map(|value| Token { value, span: 0..0 })
        .collect();
        let element = ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), 4, tokens, Vec::new());
        let records = [element];
        let result = refusal_at(
            &directory,
            &records,
            ResourceDimension::RetainedBytes,
            "iges FEM parameter string",
        );
        assert!(matches!(
            result,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.additional == 4
                    && limit.operation == "iges FEM parameter string"
        ));

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let resolver = ParameterResolver::new(&directory, &ctx).expect("directory index");
        assert_eq!(
            build(&directory, &records, &resolver, &ctx)
                .expect("FEM entity")
                .len(),
            1
        );
    }

    #[test]
    fn fem_native_slot_refuses_collection_limit_before_building_entity() {
        use super::build;
        use crate::graph::ParameterResolver;
        use crate::test_support::directory_target;
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let directory = [directory_target(1, 134)];
        let result = refusal_at(
            &directory,
            &[],
            ResourceDimension::CollectionItems,
            "iges FEM native entities",
        );
        assert!(matches!(
            result,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.additional == 1
                    && limit.operation == "iges FEM native entities"
        ));

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let resolver = ParameterResolver::new(&directory, &ctx).expect("directory index");
        assert_eq!(
            build(&directory, &[], &resolver, &ctx)
                .expect("FEM entity")
                .len(),
            1
        );
    }

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
