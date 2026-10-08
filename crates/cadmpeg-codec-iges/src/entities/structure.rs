// SPDX-License-Identifier: Apache-2.0
//! Product definitions, occurrences, and ordered assembly relationships.

use super::curve_conversion::angularly_equal;
use super::geometry::{
    curve_geometry_coplanar, planar_polyline_has_self_intersection, plane_coordinates,
    resolve_transform, ProjectionOutcome, TransformResolutionError,
};
use crate::directory::{DirectoryEntry, Hierarchy, Subordinate, UseFlag};
use crate::global::{GlobalTable, ProjectedGlobal, RealPrecision};
use crate::parameter::{
    connect_node_layout, signal_string_layout, text_node_layout, ParameterRecord, TokenValue,
    TrailingPointerAnalysis,
};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::draft::{CommitSession, ModelDraft};
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, EdgeId, VertexId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Edge, Face, Loop, Region, Sense, Shell};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

const DEFAULT_DIMENSION_UNITS_CHARACTER_SET: i64 = 1;
const LEGACY_PLANE_NORMAL_EPSILON: f64 = 1.0e-10;

fn attribute_list_type_meaning(value: i64, global_table: GlobalTable) -> Option<&'static str> {
    match (global_table, value) {
        (GlobalTable::V4_0, 0) => Some("property-entity-defined"),
        (_, 0) => Some("type406-form15-defined"),
        (_, 1) => Some("general"),
        (_, 2) => Some("electrical"),
        (_, 3) => Some("aec"),
        (_, 4) => Some("process-plant"),
        (GlobalTable::V4_0, 5..=5000) => Some("other-application-area"),
        (GlobalTable::V4_0, 5001..=9999) => Some("user-defined"),
        (_, 5) => Some("electrical-lep-manufacturing"),
        (_, 6..=5000) => Some("other-application-area"),
        (_, 5001..=9999) => Some("implementor-defined"),
        _ => None,
    }
}

fn connect_point_function_code_valid(value: i64, global_table: GlobalTable) -> bool {
    match global_table {
        GlobalTable::V4_0 => matches!(value, 0..=5),
        _ => matches!(value, 0..=49 | 98..=99 | 5001..=9999),
    }
}

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

#[derive(Clone)]
struct NetworkDefinition {
    depth: usize,
    members: Vec<u32>,
    connect_points: Vec<Option<u32>>,
}

#[derive(Clone)]
struct NetworkInstance {
    definition: u32,
    connect_points: Vec<Option<u32>>,
}

fn network_connect_points(
    record: &ParameterRecord,
    count_index: usize,
    first_pointer_index: usize,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<Option<Vec<Option<u32>>>, CodecError> {
    let Some(count) = record.count(count_index) else {
        return Ok(None);
    };
    let mut points = Vec::new();
    let mut input = 0..count;
    while let Some(index) = ctx.next_charged(&mut input, operation)? {
        let value = if matches!(global_table, GlobalTable::V4_0) {
            record.integer(first_pointer_index + index)
        } else {
            record.integer_or(first_pointer_index + index, 0)
        };
        let Some(value) = value else {
            return Ok(None);
        };
        let point = if value == 0 {
            if matches!(global_table, GlobalTable::V4_0) {
                return Ok(None);
            }
            None
        } else {
            let Some(sequence) = u32::try_from(value)
                .ok()
                .filter(|sequence| sequence % 2 == 1)
            else {
                return Ok(None);
            };
            if !ctx
                .get_btree_map(entries, &sequence, "iges network point directory lookup")?
                .is_some_and(|entry| entry.entity_type == 132)
            {
                return Ok(None);
            }
            Some(sequence)
        };
        ctx.reserve_vec(&mut points, 1, operation)?;
        points.push(point);
    }
    Ok(Some(points))
}

fn network_connectivity_valid(
    definition: &[Option<u32>],
    instance: &[Option<u32>],
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if definition.len() != instance.len() {
        return Ok(false);
    }
    let require_points = matches!(global_table, GlobalTable::V4_0);
    ctx.all_by(
        definition.iter().zip(instance),
        |(definition, instance)| {
            Ok((definition.is_some() || instance.is_none())
                && (!require_points || (definition.is_some() && instance.is_some())))
        },
        "iges network connectivity",
    )
}

fn subfigure_definition_directory_fields_valid(
    entry: &DirectoryEntry,
    global_table: GlobalTable,
) -> bool {
    entry.status.use_flag(global_table) == Some(UseFlag::Definition)
        && (!matches!(global_table, GlobalTable::V4_0)
            || (entry.status.subordinate() == Some(Subordinate::Independent)
                && (entry.status.hierarchy() == Some(Hierarchy::GlobalDefer)
                    || entry.line_font != 0)))
}

fn subfigure_definition_label_display_valid(
    entry: &DirectoryEntry,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if entry.label_display == 0 {
        return Ok(true);
    }
    let Some(sequence) = u32::try_from(entry.label_display)
        .ok()
        .filter(|sequence| sequence % 2 == 1)
    else {
        return Ok(false);
    };
    Ok(ctx
        .get_btree_map(entries, &sequence, "iges subfigure label display lookup")?
        .is_some_and(|target| target.entity_type == 402 && target.form == 5))
}

fn subfigure_definition_transform_valid(
    entry: &DirectoryEntry,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    global: &ProjectedGlobal,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    match resolve_transform(
        entry.transform,
        entries,
        records,
        global.length_factor_mm(),
        global.real_precision(),
        &mut BTreeSet::new(),
        ctx,
    ) {
        Ok(_) => Ok(true),
        Err(error) => {
            error.non_resource()?;
            Ok(false)
        }
    }
}

#[derive(Clone)]
struct FlowAssociativity {
    form: i64,
    associated: Vec<u32>,
    continuations: Vec<Option<u32>>,
}

fn single_target_cycle(
    sequence: u32,
    targets: &BTreeMap<u32, u32>,
    visited: &mut BTreeSet<u32>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if ctx.contains_btree_set(visited, &sequence, "iges structure cycle visited nodes")? {
        return Ok(false);
    }

    let mut search_storage = ctx.reserve_scoped(0, "IGES single target cycle search")?;
    let mut path = Vec::new();
    let mut visiting = BTreeSet::new();
    let mut current = Some(sequence);
    while let Some(current_sequence) = current {
        ctx.charge_work(1, "iges structure cycle traversal")?;
        if ctx.contains_btree_set(
            visited,
            &current_sequence,
            "iges structure cycle visited nodes",
        )? {
            for node in ctx.admit_iter(path, "iges structure list traversal")? {
                ctx.insert_btree_set(visited, node, "iges structure visited cycle nodes")?;
            }
            return Ok(false);
        }
        if !search_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut visiting,
                current_sequence,
                "iges structure active cycle nodes",
            )
        })? {
            return Ok(true);
        }
        search_storage
            .with_storage(|| ctx.reserve_vec(&mut path, 1, "iges structure cycle path"))?;
        path.push(current_sequence);
        current = match ctx.get_btree_map(
            targets,
            &current_sequence,
            "iges structure cycle successor lookup",
        )? {
            Some(target)
                if ctx.contains_key_btree_map(
                    targets,
                    target,
                    "iges structure cycle successor lookup",
                )? =>
            {
                Some(*target)
            }
            _ => None,
        };
    }
    for node in ctx.admit_iter(path, "iges structure cycle completion")? {
        ctx.insert_btree_set(visited, node, "iges structure visited cycle nodes")?;
    }
    Ok(false)
}

pub(crate) fn array_base_type(entity_type: i64, form: i64) -> bool {
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

pub(crate) fn signal_string_geometry_target(entity_type: i64, form: i64) -> bool {
    matches!(
        (entity_type, form),
        (100 | 102 | 112 | 116 | 130 | 132, 0)
            | (104, 0..=3)
            | (106, 11 | 12)
            | (110, 0..=2)
            | (126, 0..=5)
    )
}

pub(crate) fn flow_join_target_valid(target: &DirectoryEntry, global_table: GlobalTable) -> bool {
    target.entity_type == 408
        || (target.status.use_flag(global_table) == Some(UseFlag::Geometry)
            && !matches!(
                target.entity_type,
                0 | 132
                    | 134
                    | 136
                    | 138
                    | 146
                    | 148
                    | 180
                    | 182
                    | 184
                    | 202
                    | 204
                    | 206
                    | 208
                    | 210
                    | 212
                    | 213
                    | 214
                    | 216
                    | 218
                    | 220
                    | 222
                    | 228
                    | 230
                    | 302
                    | 304
                    | 306
                    | 308
                    | 310
                    | 312
                    | 314
                    | 316
                    | 320
                    | 322
                    | 402
                    | 404
                    | 406
                    | 410
                    | 412
                    | 414
                    | 416
                    | 418
                    | 420
                    | 422
                    | 430
                    | 502
                    | 504
                    | 508
                    | 510
                    | 514
            ))
}

fn flow_associativity_directory_valid(entry: &DirectoryEntry, global_table: GlobalTable) -> bool {
    if entry.entity_type != 402 {
        return false;
    }
    match global_table {
        GlobalTable::V4_0 => {
            entry.form == 18 && entry.status.use_flag(global_table) == Some(UseFlag::Other)
        }
        _ => matches!(entry.form, 18 | 20),
    }
}

fn flow_connection_target_valid(
    target: &DirectoryEntry,
    form: i64,
    global_table: GlobalTable,
) -> bool {
    if form != 18 {
        return target.entity_type == 132;
    }
    target.entity_type == 132
        || (!matches!(global_table, GlobalTable::V4_0)
            && target.entity_type == 402
            && matches!(target.form, 1 | 7 | 14 | 15))
}

fn flow_display_target_valid(
    target: &DirectoryEntry,
    form: i64,
    global_table: GlobalTable,
) -> bool {
    target.entity_type == 312
        || (form == 18 && !matches!(global_table, GlobalTable::V4_0) && target.entity_type == 212)
}

fn flow_continuation_target_valid(
    target: &DirectoryEntry,
    form: i64,
    global_table: GlobalTable,
) -> bool {
    target.entity_type == 402
        && if form == 18 {
            target.form == 18 || (!matches!(global_table, GlobalTable::V4_0) && target.form == 11)
        } else {
            target.form == 20
        }
}

fn array_mask_valid(
    record: &ParameterRecord,
    count_index: usize,
    flag_index: usize,
    first_position_index: usize,
    total: usize,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(count) =
        record.count_with_stride_at(count_index, first_position_index, 1, record.parameter_end())
    else {
        return Ok(false);
    };
    let Some(flag) = record
        .integer(flag_index)
        .filter(|value| matches!(*value, 0..=1))
    else {
        return Ok(false);
    };
    let mut storage = ctx.reserve_scoped(0, "iges array mask scratch")?;
    let mut positions = BTreeSet::new();
    let mut input = 0..count;
    while let Some(index) = ctx.next_charged(&mut input, "iges structure list traversal")? {
        let Some(position) = record
            .integer(first_position_index + index)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|position| *position >= 1 && *position <= total)
        else {
            return Ok(false);
        };
        if !storage.with_storage(|| {
            ctx.insert_btree_set(&mut positions, position, "iges array mask positions")
        })? {
            return Ok(false);
        }
    }
    let cardinality_valid = count == 0
        || if flag == 0 {
            count <= total / 2
        } else {
            count >= total.div_ceil(2)
        };
    Ok(cardinality_valid)
}

fn has_association_back_pointer(
    record: &ParameterRecord,
    group_sequence: u32,
    association_owners: &BTreeMap<u32, BTreeSet<u32>>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(owners) = ctx.get_btree_map(
        association_owners,
        &group_sequence,
        "iges association owner lookup",
    )? else {
        return Ok(false);
    };
    ctx.contains_btree_set(
        owners,
        &record.directory_sequence,
        "iges association owner membership",
    )
}

fn legacy_primary_end_valid(
    record: &ParameterRecord,
    primary_end: usize,
    trailing_pointer_analysis: &BTreeMap<u32, TrailingPointerAnalysis>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if record.parameter_end() == primary_end {
        return Ok(true);
    }
    Ok(ctx
        .get_btree_map(
            trailing_pointer_analysis,
            &record.directory_sequence,
            "iges trailing pointer analysis lookup",
        )?
        .and_then(|analysis| match analysis {
            TrailingPointerAnalysis::Unambiguous(groups) => Some(groups),
            _ => None,
        })
        .is_some_and(|groups| groups.token_start == primary_end))
}

struct LegacyAssociativityContext<'map, 'data> {
    entry: &'map DirectoryEntry,
    entries: &'map BTreeMap<u32, &'data DirectoryEntry>,
    records: &'map BTreeMap<u32, &'data ParameterRecord>,
    association_owners: &'map BTreeMap<u32, BTreeSet<u32>>,
}

impl LegacyAssociativityContext<'_, '_> {
    fn pointer_list_valid(
        &self,
        record: &ParameterRecord,
        indices: std::ops::Range<usize>,
        accepts: fn(&DirectoryEntry) -> bool,
        back_pointers_required: bool,
        ctx: &DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        ctx.all_by(
            indices,
            |index| {
                let Some((sequence, target)) = existing_pointer(record, index, self.entries, ctx)?
                else {
                    return Ok(false);
                };
                let accepted = accepts(target);
                if !accepted || !back_pointers_required {
                    return Ok(accepted);
                }
                let Some(owner_record) = ctx.get_btree_map(
                    self.records,
                    &sequence,
                    "iges legacy associativity record lookup",
                )? else {
                    return Ok(false);
                };
                has_association_back_pointer(
                    owner_record,
                    self.entry.sequence,
                    self.association_owners,
                    ctx,
                )
            },
            "iges legacy associativity pointer list",
        )
    }
}

fn connect_node_target(target: &DirectoryEntry) -> bool {
    target.entity_type == 402 && target.form == 11
}

fn point_target(target: &DirectoryEntry) -> bool {
    target.entity_type == 116 && target.form == 0
}

fn negative_font_pointer_valid(
    record: &ParameterRecord,
    index: usize,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    Ok(match record.integer_or(index, 1) {
        Some(value) if value > 0 => true,
        Some(value) if value < 0 => match value
            .checked_neg()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|sequence| sequence % 2 == 1)
        {
            Some(sequence) => ctx
                .get_btree_map(entries, &sequence, "iges negative font pointer lookup")?
                .is_some_and(|target| target.entity_type == 310 && target.form == 0),
            None => false,
        },
        _ => false,
    })
}

fn legacy_associativity_valid(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    links: (
        &BTreeMap<u32, TrailingPointerAnalysis>,
        &BTreeMap<u32, BTreeSet<u32>>,
    ),
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let (trailing_pointer_analysis, association_owners) = links;
    let context = LegacyAssociativityContext {
        entry,
        entries,
        records,
        association_owners,
    };
    Ok(match entry.form {
        8 => {
            let Some(layout) = signal_string_layout(record) else {
                return Ok(false);
            };
            let names_valid = ctx.all_by(
                layout.signal_names(),
                |index| {
                    Ok(matches!(
                        record.value(index),
                        Some(TokenValue::String(_) | TokenValue::Omitted)
                    ))
                },
                "iges legacy associativity fields",
            )?;
            names_valid
                && context.pointer_list_valid(
                    record,
                    layout.connections(),
                    connect_node_target,
                    true,
                    ctx,
                )?
                && context.pointer_list_valid(
                    record,
                    layout.schematic(),
                    |target| signal_string_geometry_target(target.entity_type, target.form),
                    true,
                    ctx,
                )?
                && context.pointer_list_valid(
                    record,
                    layout.physical(),
                    |target| signal_string_geometry_target(target.entity_type, target.form),
                    true,
                    ctx,
                )?
                && legacy_primary_end_valid(
                    record,
                    layout.primary_end(),
                    trailing_pointer_analysis,
                    ctx,
                )?
        }
        10 => {
            let Some(layout) = text_node_layout(record) else {
                return Ok(false);
            };
            let description = layout.description_start();
            let numeric_fields_valid = record.number_or(description, 0.0).is_some()
                && record.number_or(description + 1, 0.0).is_some()
                && record
                    .number_or(description + 3, std::f64::consts::FRAC_PI_2)
                    .is_some()
                && record.number_or(description + 4, 0.0).is_some();
            let mirror_valid = record
                .integer_or(description + 5, 0)
                .is_some_and(|value| matches!(value, 0..=2));
            let rotate_internal_valid = record
                .integer_or(description + 6, 0)
                .is_some_and(|value| matches!(value, 0..=1));
            let points_valid =
                context.pointer_list_valid(record, layout.geometry(), point_target, true, ctx)?;
            entry.status.use_flag(global_table) == Some(UseFlag::LogicalPositional)
                && !layout.geometry().is_empty()
                && points_valid
                && numeric_fields_valid
                && negative_font_pointer_valid(record, description + 2, entries, ctx)?
                && mirror_valid
                && rotate_internal_valid
                && legacy_primary_end_valid(
                    record,
                    layout.primary_end(),
                    trailing_pointer_analysis,
                    ctx,
                )?
        }
        11 => {
            let Some(layout) = connect_node_layout(record) else {
                return Ok(false);
            };
            let data_valid = ctx.all_by(
                layout.data(),
                |index| Ok(record.value(index).is_some()),
                "iges legacy associativity fields",
            )?;
            entry.status.use_flag(global_table) == Some(UseFlag::LogicalPositional)
                && !layout.points().is_empty()
                && context.pointer_list_valid(record, layout.points(), point_target, true, ctx)?
                && data_valid
                && legacy_primary_end_valid(
                    record,
                    layout.primary_end(),
                    trailing_pointer_analysis,
                    ctx,
                )?
        }
        _ => false,
    })
}

fn attribute_value_valid(
    record: &ParameterRecord,
    index: usize,
    data_type: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    Ok(match (data_type, record.value(index)) {
        (0 | 5, Some(TokenValue::Omitted))
        | (1, Some(TokenValue::Integer(_)))
        | (3, Some(TokenValue::String(_))) => true,
        (2, Some(TokenValue::Integer(_) | TokenValue::Real(_))) => record.number(index).is_some(),
        (4, Some(TokenValue::Integer(value))) => u32::try_from(*value)
            .ok()
            .filter(|sequence| sequence % 2 == 1)
            .map(|sequence| {
                ctx.contains_key_btree_map(
                    entries,
                    &sequence,
                    "iges attribute directory lookup",
                )
            })
            .transpose()?
            .unwrap_or(false),
        (6, Some(TokenValue::Integer(value))) => matches!(*value, 0..=1),
        _ => false,
    })
}

struct AttributeShape<'ctx> {
    descriptors: Vec<(i64, usize)>,
    _storage: ScopedReservation<'ctx>,
}

fn attribute_definition_valid_and_shape<'ctx>(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    global_table: GlobalTable,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<(bool, AttributeShape<'ctx>), CodecError> {
    let name_valid = matches!(
        record.value(1),
        Some(TokenValue::String(_) | TokenValue::Omitted)
    );
    let list_type_valid = record
        .integer(2)
        .is_some_and(|value| attribute_list_type_meaning(value, global_table).is_some());
    let attribute_count = record.count(3).filter(|count| *count > 0);
    let mut cursor = 4;
    let mut attributes_valid = attribute_count.is_some();
    let mut attribute_type_storage = ctx.reserve_scoped(0, "iges attribute type nodes")?;
    let mut attribute_types = BTreeSet::new();
    let mut shape_storage = ctx.reserve_scoped(0, "iges attribute shape descriptors")?;
    let mut descriptors = Vec::new();
    for _ in ctx.admit_iter(
        0..attribute_count.unwrap_or_default(),
        "iges structure list traversal",
    )? {
        let attribute_type_valid = match record.integer(cursor) {
            Some(value) if (0..=9999).contains(&value) => attribute_type_storage
                .with_storage(|| {
                    ctx.insert_btree_set(&mut attribute_types, value, "iges attribute type nodes")
                })?,
            _ => false,
        };
        let data_type = record
            .integer(cursor + 1)
            .filter(|value| matches!(value, 0..=6));
        let value_count = match record.value(cursor + 2) {
            None | Some(TokenValue::Omitted) => record.integer_or(cursor + 2, 1).map(|_| 1),
            Some(TokenValue::Integer(value)) => {
                usize::try_from(*value).ok().and_then(|count| {
                    (entry.form == 0
                        || cursor
                            .checked_add(3)
                            .and_then(|start| record.parameter_end().checked_sub(start))
                            .is_some_and(|available| count <= available))
                    .then_some(count)
                })
            }
            Some(TokenValue::Real(_) | TokenValue::String(_)) => None,
        };
        cursor += 3;
        attributes_valid &=
            attribute_type_valid && data_type.is_some() && value_count.is_some();
        if let Some(descriptor) = data_type
            .zip(value_count)
            .filter(|(_, count)| entry.form == 0 && *count > 0)
        {
            ctx.push_scoped_vec(
                &mut shape_storage,
                &mut descriptors,
                descriptor,
                "iges attribute shape descriptors",
            )?;
        }
        if entry.form != 0 {
            for _ in ctx.admit_iter(
                0..value_count.unwrap_or_default(),
                "iges structure list traversal",
            )? {
                let value_valid = match data_type {
                    Some(data_type) => {
                        attribute_value_valid(record, cursor, data_type, entries, ctx)?
                    }
                    None => false,
                };
                attributes_valid &= value_valid;
                cursor += 1;
                if entry.form == 2 {
                    let display_valid = match record.integer_or(cursor, 0) {
                        Some(0) => true,
                        Some(value) => match u32::try_from(value).ok() {
                            Some(sequence) => ctx
                                .get_btree_map(
                                    entries,
                                    &sequence,
                                    "iges attribute display directory lookup",
                                )?
                                .is_some_and(|target| target.entity_type == 312),
                            None => false,
                        },
                        None => false,
                    };
                    attributes_valid &= display_valid;
                    cursor += 1;
                }
            }
        }
    }
    Ok((
        name_valid && list_type_valid && attributes_valid,
        AttributeShape {
            descriptors,
            _storage: shape_storage,
        },
    ))
}

fn unit_values_valid(
    record: &ParameterRecord,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let count = record.count(1).filter(|count| *count > 0);
    let mut type_storage = ctx.reserve_scoped(0, "iges unit type nodes")?;
    let mut types = BTreeSet::<&[u8]>::new();
    let mut units_valid = count.is_some_and(|count| record.parameter_end() == 2 + count * 3);
    if let Some(count) = count.filter(|_| units_valid) {
        let mut input = 0..count;
        while let Some(offset) = ctx.next_charged(&mut input, "iges structure list traversal")? {
            let start = 2 + offset * 3;
            let valid = if let Some((unit_type, value)) =
                record.string(start).zip(record.string(start + 1))
            {
                unit_value_valid(unit_type, value)
                    && type_storage.with_storage(|| {
                        ctx.insert_btree_set(&mut types, unit_type, "iges unit type nodes")
                    })?
                    && record
                        .number(start + 2)
                        .is_some_and(|scale| scale.is_finite() && scale > 0.0)
            } else {
                false
            };
            if !valid {
                units_valid = false;
                break;
            }
        }
    }
    Ok(units_valid)
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

fn line_font_property_code_valid(value: i64) -> bool {
    matches!(
        value,
        12 | 14
            | 16
            | 18
            | 22
            | 42
            | 44
            | 46
            | 48
            | 52
            | 54
            | 152
            | 154
            | 156
            | 162
            | 164
            | 166
            | 172
            | 174
            | 176
            | 178
            | 192
            | 194
            | 198
            | 200
            | 203
            | 206
            | 223
            | 227
            | 230
            | 232
            | 237
            | 239
            | 240
            | 253
            | 270
            | 330
            | 355
            | 360
            | 380
            | 385
            | 390
            | 395
            | 400
            | 405
            | 410
            | 415
            | 420
            | 425
            | 430
            | 445
            | 485
    )
}

const FUNCTIONAL_LEVEL_IDENTIFIERS: &[&[u8]] = &[
    b"Annotation",
    b"Drilled Holes",
    b"Errors",
    b"Panel_Outline",
    b"Placement_Keepin",
    b"Placement_Keepout",
    b"PRD_ID",
    b"Routing_Keepin",
    b"Routing_Keepout",
    b"Signal_Guide",
    b"Substrate_Outline",
    b"Trace_Keepin",
    b"Trace_Keepout",
    b"Undefined",
    b"Unplaced_Components",
    b"Via_Keepin",
    b"Via_Keepout",
    b"Via_Placement",
];

const SIDE_QUALIFIED_FUNCTIONAL_LEVEL_IDENTIFIERS: &[&[u8]] = &[
    b"Bond_Pad",
    b"Breakout",
    b"Chip_Pad",
    b"Component_Outline",
    b"Component_Placement",
    b"Crossover",
    b"Deposition_Components",
    b"Dielectric",
    b"Glue_Mask",
    b"Ground",
    b"Hole_Fill",
    b"Laser-Trim-Path",
    b"Pad",
    b"Pin_ID",
    b"Pin_Placement",
    b"Power",
    b"Sheet_Dielectric",
    b"Signal",
    b"Signal_ID",
    b"Silkscreen",
    b"Solder_Mask",
    b"Solder_Paste-Mask",
    b"Thermal_Outline",
    b"Wire-Bond",
];

fn functional_level_identifier_valid(
    value: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if FUNCTIONAL_LEVEL_IDENTIFIERS
        .iter()
        .any(|identifier| value.eq_ignore_ascii_case(identifier))
    {
        return Ok(true);
    }

    if SIDE_QUALIFIED_FUNCTIONAL_LEVEL_IDENTIFIERS
        .iter()
        .any(|identifier| value.eq_ignore_ascii_case(identifier))
    {
        return Ok(true);
    }

    for identifier in SIDE_QUALIFIED_FUNCTIONAL_LEVEL_IDENTIFIERS {
        let identifier_length = identifier.len();
        if value.len() > identifier_length + 1
            && value[..identifier_length].eq_ignore_ascii_case(identifier)
            && value[identifier_length] == b'_'
            && functional_level_suffix_valid(&value[identifier_length + 1..], ctx)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn functional_level_suffix_valid(
    value: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if value.eq_ignore_ascii_case(b"T") || value.eq_ignore_ascii_case(b"B") {
        return Ok(true);
    }
    if value.is_empty() || value.first() == Some(&b'+') {
        return Ok(false);
    }
    let Ok(text) = ctx.validate_utf8(value, "iges functional level suffix UTF-8")? else {
        return Ok(false);
    };
    Ok(ctx
        .parse_text::<u64>(text, "iges functional level suffix number")?
        .is_ok_and(|number| number >= 2))
}

fn generic_property_value_valid(
    record: &ParameterRecord,
    index: usize,
    data_type: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    Ok(match data_type {
        0 => matches!(record.value(index), Some(TokenValue::Omitted)),
        1 => record.integer(index).is_some(),
        2 => record.number(index).is_some(),
        3 => record.string(index).is_some(),
        4 => existing_pointer(record, index, entries, ctx)?.is_some(),
        6 => record
            .integer(index)
            .is_some_and(|value| matches!(value, 0..=1)),
        _ => false,
    })
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
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    // The property count is stated in `u64`: every caller derives it from an
    // in-memory token count, and the record's own declaration is compared in
    // that width. A declaration a `u64` cannot state — a negative one — equals
    // no count, so it is refused by the comparison rather than folded.
    let exact = |count: u64| {
        record
            .integer(1)
            .and_then(|declared| u64::try_from(declared).ok())
            == Some(count)
            && u64_from_index(end) == count + 2
    };
    let integer_range = |index, range: std::ops::RangeInclusive<i64>| {
        record
            .integer(index)
            .is_some_and(|value| range.contains(&value))
    };
    let fields_valid = match entry.form {
        2 => exact(3) && (2..=4).all(|index| integer_range(index, 0..=2)),
        3 => exact(2) && record.integer(2).is_some() && record.string(3).is_some(),
        4 => {
            exact(2)
                && integer_range(2, 0..=2)
                && (match record.integer(3) {
                    Some(0) => true,
                    Some(_) => existing_pointer(record, 3, entries, ctx)?.is_some(),
                    None => false,
                })
        }
        5 => {
            exact(5)
                && record
                    .number(2)
                    .is_some_and(|value| value.is_finite() && value >= 0.0)
                && integer_range(3, 0..=1)
                && integer_range(4, 0..=2)
                && integer_range(5, 0..=2)
                && record.number(6).is_some()
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
                return Ok(false);
            };
            let types_valid = ctx.all_by(
                0..independent_count,
                |offset| Ok(integer_range(5 + offset, 1..=8)),
                "iges property fields",
            )?;
            let mut counts = Some((0_usize, 1_usize));
            let mut offsets = 0..independent_count;
            while let Some(offset) =
                ctx.next_charged(&mut offsets, "iges property independent counts")?
            {
                counts = counts.and_then(|(sum, product)| {
                    let count = record
                        .integer(5 + independent_count + offset)
                        .and_then(|value| usize::try_from(value).ok())
                        .filter(|count| *count > 0)?;
                    Some((sum.checked_add(count)?, product.checked_mul(count)?))
                });
                if counts.is_none() {
                    break;
                }
            }
            let Some((independent_values, point_count)) = counts else {
                return Ok(false);
            };
            let expected_end =
                dependent_count
                    .checked_mul(point_count)
                    .and_then(|dependent_values| {
                        5_usize
                            .checked_add(2 * independent_count)?
                            .checked_add(independent_values)?
                            .checked_add(dependent_values)
                    });
            record
                .integer(2)
                .is_some_and(|value| matches!(value, 1..=9999))
                && types_valid
                && expected_end == Some(end)
                && record.integer(1) == i64::try_from(end - 2).ok()
                && ctx.all_by(
                    5 + 2 * independent_count..end,
                    |index| Ok(record.number(index).is_some()),
                    "iges property fields",
                )?
        }
        12 | 14 => record
            .count(1)
            .map(|count| -> Result<bool, CodecError> {
                Ok(count > 0
                    && end == count + 2
                    && ctx.all_by(
                        0..count,
                        |offset| Ok(record.string(2 + offset).is_some()),
                        "iges property fields",
                    )?)
            })
            .transpose()?
            .unwrap_or(false),
        13 => {
            matches!(record.integer(1), Some(2 | 3))
                && record
                    .integer(1)
                    .and_then(|value| usize::try_from(value).ok())
                    .is_some_and(|value| end == value + 2)
                && record.number(2).is_some()
                && record.string(3).is_some()
                && (record.integer(1) == Some(2) || record.string(4).is_some())
        }
        18 => {
            exact(1)
                && record
                    .number(2)
                    .is_some_and(|value| (0.0..=100.0).contains(&value))
        }
        19 => exact(1) && record.integer(2).is_some_and(line_font_property_code_valid),
        20 | 21 => exact(1) && integer_range(2, 0..=1),
        22 => {
            exact(9)
                && (2..=4).all(|index| integer_range(index, 0..=1))
                && (5..=6).all(|index| record.number(index).is_some())
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
            .map(|count| -> Result<bool, CodecError> {
                Ok(exact(1 + 4 * u64_from_index(count))
                    && ctx.all_by(
                        0..count,
                        |offset| {
                            let start = 3 + offset * 4;
                            Ok(record.integer(start).is_some_and(|value| value >= 0)
                                && record.string(start + 1).is_some()
                                && record.integer(start + 2).is_some_and(|value| value >= 0)
                                && record
                                    .string(start + 3)
                                    .map(|value| -> Result<bool, CodecError> {
                                        functional_level_identifier_valid(value, ctx)
                                    })
                                    .transpose()?
                                    .unwrap_or(false))
                        },
                        "iges property fields",
                    )?)
            })
            .transpose()?
            .unwrap_or(false),
        25 => record
            .count(3)
            .filter(|count| *count > 0 && *count <= end)
            .map(|count| -> Result<bool, CodecError> {
                Ok(exact(2 + u64_from_index(count))
                    && record.string(2).is_some_and(|value| !value.is_empty())
                    && ctx.all_by(
                        0..count,
                        |offset| Ok(record.integer(4 + offset).is_some_and(|value| value >= 0)),
                        "iges property fields",
                    )?)
            })
            .transpose()?
            .unwrap_or(false),
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
            .map(|count| -> Result<bool, CodecError> {
                Ok(exact(2 + 2 * u64_from_index(count))
                    && record.string(2).is_some_and(|value| !value.is_empty())
                    && ctx.all_by(
                        0..count,
                        |offset| {
                            let index = 4 + offset * 2;
                            let Some(data_type) = record.integer(index) else {
                                return Ok(false);
                            };
                            generic_property_value_valid(record, index + 1, data_type, entries, ctx)
                        },
                        "iges property fields",
                    )?)
            })
            .transpose()?
            .unwrap_or(false),
        28 => {
            let units_valid = record
                .integer(3)
                .is_some_and(|value| matches!(value, 0..=11 | 100..=106));
            let charset_valid = record
                .integer_or(4, DEFAULT_DIMENSION_UNITS_CHARACTER_SET)
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
                && record
                    .integer_or(4, 2)
                    .is_some_and(|value| (1..=4).contains(&value))
                && (5..=6).all(|index| record.number(index).is_some())
                && integer_range(7, 0..=1)
                && fraction.is_some_and(|value| matches!(value, 0..=2))
                && record
                    .integer(9)
                    .is_some_and(|value| value >= 0 && (fraction == Some(0) || value > 0))
        }
        30 => record
            .count(13)
            .filter(|count| *count <= end)
            .map(|count| -> Result<bool, CodecError> {
                Ok(record.integer(1) == Some(14)
                    && count.checked_mul(3).and_then(|span| span.checked_add(14)) == Some(end)
                    && integer_range(2, 0..=2)
                    && integer_range(3, 0..=4)
                    && record
                        .integer_or(4, 1)
                        .is_some_and(|value| matches!(value, 1 | 1001..=1003))
                    && record.string(5).is_some()
                    && integer_range(6, 0..=1)
                    && record.number_or(7, std::f64::consts::FRAC_PI_2).is_some()
                    && integer_range(8, 0..=1)
                    && integer_range(9, 0..=2)
                    && integer_range(10, 0..=2)
                    && integer_range(11, 0..=1)
                    && record.number(12).is_some()
                    && ctx.all_by(
                        0..count,
                        |offset| {
                            let start = 14 + offset * 3;
                            Ok(integer_range(start, 1..=4)
                                && record
                                    .integer(start + 1)
                                    .zip(record.integer(start + 2))
                                    .is_some_and(|(first, last)| first > 0 && last >= first))
                        },
                        "iges property fields",
                    )?)
            })
            .transpose()?
            .unwrap_or(false),
        31 => exact(8) && (2..=9).all(|index| record.number(index).is_some()),
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
            .map(|count| -> Result<bool, CodecError> {
                Ok(exact(1 + u64_from_index(count) * 3)
                    && ctx.all_by(
                        0..count,
                        |offset| {
                            let start = 3 + offset * 3;
                            Ok(record.integer(start).is_some_and(|value| value > 0)
                                && record
                                    .integer(start + 1)
                                    .zip(record.integer(start + 2))
                                    .is_some_and(|(first, last)| first > 0 && last >= first))
                        },
                        "iges property fields",
                    )?)
            })
            .transpose()?
            .unwrap_or(false),
        36 => {
            matches!(record.integer(1), Some(1 | 2))
                && record
                    .integer(1)
                    .and_then(|value| usize::try_from(value).ok())
                    .is_some_and(|value| end == value + 2)
                && integer_range(2, 0..=2)
                && (record.integer(1) == Some(1) || integer_range(3, 0..=2))
        }
        _ => false,
    };
    Ok(fields_valid)
}

fn existing_pointer<'entries>(
    record: &ParameterRecord,
    index: usize,
    entries: &'entries BTreeMap<u32, &DirectoryEntry>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(u32, &'entries DirectoryEntry)>, CodecError> {
    let Some(sequence) = record
        .integer(index)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|sequence| sequence % 2 == 1)
    else {
        return Ok(None);
    };
    Ok(ctx
        .get_btree_map(entries, &sequence, "iges structure pointer lookup")?
        .map(|entry| (sequence, *entry)))
}

fn type402_structure_valid(entry: &DirectoryEntry, global_table: GlobalTable) -> bool {
    matches!(global_table, GlobalTable::V4_0) || entry.structure == 0
}

fn predefined_associativity_valid(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    association_owners: &BTreeMap<u32, BTreeSet<u32>>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let end = record.parameter_end();
    match entry.form {
        5 => {
            let Some(count) = record.count(1).filter(|count| *count > 0) else {
                return Ok(false);
            };
            if end != 2 + count * 7 {
                return Ok(false);
            }
            ctx.all_by(
                0..count,
                |offset| {
                    let start = 2 + offset * 7;
                    let Some((_, view)) = existing_pointer(record, start, entries, ctx)? else {
                        return Ok(false);
                    };
                    if view.entity_type != 410
                        || !(start + 1..=start + 3).all(|index| record.number(index).is_some())
                    {
                        return Ok(false);
                    }
                    let Some((_, leader)) = existing_pointer(record, start + 4, entries, ctx)?
                    else {
                        return Ok(false);
                    };
                    Ok(leader.entity_type == 214
                        && record.integer(start + 5).is_some_and(|level| level >= 0)
                        && existing_pointer(record, start + 6, entries, ctx)?.is_some())
                },
                "iges predefined associativity fields",
            )
        }
        6 => {
            if record.integer(1) != Some(1) {
                return Ok(false);
            }
            let Some(visible_count) = record.count(2) else {
                return Ok(false);
            };
            if end != 4 + visible_count {
                return Ok(false);
            }
            let view = existing_pointer(record, 3, entries, ctx)?;
            let visible_valid = ctx.all_by(
                0..visible_count,
                |offset| {
                    let Some((sequence, _)) =
                        existing_pointer(record, 4 + offset, entries, ctx)?
                    else {
                        return Ok(false);
                    };
                    let Some(owner) = ctx.get_btree_map(
                        records,
                        &sequence,
                        "iges predefined associativity record lookup",
                    )? else {
                        return Ok(false);
                    };
                    has_association_back_pointer(owner, entry.sequence, association_owners, ctx)
                },
                "iges predefined associativity fields",
            )?;
            let view_back_pointer_valid = match view {
                Some((sequence, target)) if target.entity_type == 410 => {
                    match ctx.get_btree_map(
                        records,
                        &sequence,
                        "iges predefined associativity record lookup",
                    )? {
                        Some(record) => has_association_back_pointer(
                            record,
                            entry.sequence,
                            association_owners,
                            ctx,
                        )?,
                        None => false,
                    }
                }
                _ => false,
            };
            Ok(view_back_pointer_valid && visible_valid)
        }
        9 => {
            if record.integer(1) != Some(1) {
                return Ok(false);
            }
            let Some(child_count) = record.count(2).filter(|count| *count > 0) else {
                return Ok(false);
            };
            if end != 4 + child_count {
                return Ok(false);
            }
            ctx.all_by(
                3..4 + child_count,
                |index| {
                    let Some((sequence, _)) = existing_pointer(record, index, entries, ctx)? else {
                        return Ok(false);
                    };
                    let Some(member) = ctx.get_btree_map(
                        records,
                        &sequence,
                        "iges predefined associativity record lookup",
                    )? else {
                        return Ok(false);
                    };
                    has_association_back_pointer(member, entry.sequence, association_owners, ctx)
                },
                "iges predefined associativity fields",
            )
        }
        2 | 12 => {
            let count = record.count(1).filter(|count| *count > 0);
            Ok(count
                .map(|count| -> Result<bool, CodecError> {
                    Ok(end == 2 + count * 2
                        && ctx.all_by(
                            0..count,
                            |offset| {
                                let start = 2 + offset * 2;
                                Ok(record.string(start).is_some_and(|name| !name.is_empty())
                                    && existing_pointer(record, start + 1, entries, ctx)?.is_some())
                            },
                            "iges predefined associativity fields",
                        )?)
                })
                .transpose()?
                .unwrap_or(false))
        }
        13 => {
            if record.integer(1) != Some(1) {
                return Ok(false);
            }
            let Some(geometry_count) = record.count(2).filter(|count| *count > 0) else {
                return Ok(false);
            };
            if end != 4 + geometry_count {
                return Ok(false);
            }
            let dimension = existing_pointer(record, 3, entries, ctx)?;
            let geometry_valid = ctx.all_by(
                0..geometry_count,
                |offset| Ok(existing_pointer(record, 4 + offset, entries, ctx)?.is_some()),
                "iges predefined associativity fields",
            )?;
            if !geometry_valid {
                return Ok(false);
            }
            let Some((sequence, target)) = dimension else {
                return Ok(false);
            };
            if !matches!(target.entity_type, 202 | 206 | 216 | 218 | 220 | 222) {
                return Ok(false);
            }
            let Some(member) = ctx.get_btree_map(
                records,
                &sequence,
                "iges predefined associativity record lookup",
            )? else {
                return Ok(false);
            };
            has_association_back_pointer(member, entry.sequence, association_owners, ctx)
        }
        16 => {
            if record.integer(1) != Some(1) {
                return Ok(false);
            }
            let Some(count) = record.count(2).filter(|count| *count > 0) else {
                return Ok(false);
            };
            if end != 4 + count {
                return Ok(false);
            }
            let transform_valid = match record.integer(3) {
                Some(0) => true,
                Some(_) => existing_pointer(record, 3, entries, ctx)?
                    .is_some_and(|(_, target)| target.entity_type == 124 && target.form == 0),
                None => false,
            };
            if !transform_valid {
                return Ok(false);
            }
            ctx.all_by(
                0..count,
                |offset| Ok(existing_pointer(record, 4 + offset, entries, ctx)?.is_some()),
                "iges predefined associativity fields",
            )
        }
        21 => {
            if record.integer(1) != Some(1) {
                return Ok(false);
            }
            let Some(geometry_count) = record.count(2).filter(|count| *count > 0) else {
                return Ok(false);
            };
            if end != 6 + geometry_count * 5 {
                return Ok(false);
            }
            let Some(orientation) = record
                .integer(4)
                .filter(|orientation| (0..=7).contains(orientation))
            else {
                return Ok(false);
            };
            let angle_valid = record.number(5).is_some();
            if !angle_valid || !entry.status.is_physically_dependent() {
                return Ok(false);
            }
            let dimension = existing_pointer(record, 3, entries, ctx)?;
            let orientation_valid = dimension.is_some_and(|(_, dimension)| {
                match dimension.entity_type {
                    202 => matches!(orientation, 0..=3),
                    216 => matches!(orientation, 4..=7),
                    218 => matches!(orientation, 6..=7),
                    206 | 220 | 222 => orientation == 0,
                    _ => false,
                }
            });
            let geometry_valid = ctx.all_by(
                0..geometry_count,
                |offset| {
                    let start = 6 + offset * 5;
                    let pointer_valid = match record.integer(start) {
                        Some(0) => offset + 1 == geometry_count,
                        Some(_) => existing_pointer(record, start, entries, ctx)?.is_some(),
                        None => false,
                    };
                    Ok(pointer_valid
                        && record
                            .integer(start + 1)
                            .is_some_and(|location| matches!(location, 0..=5))
                        && (start + 2..=start + 4)
                            .all(|index| record.number(index).is_some()))
                },
                "iges predefined associativity fields",
            )?;
            let arrow_count = match dimension {
                Some((sequence, dimension)) if dimension.entity_type == 216 => {
                    match ctx.get_btree_map(
                        records,
                        &sequence,
                        "iges predefined associativity record lookup",
                    )? {
                        Some(dimension_record) => {
                            let mut arrows = 0_usize;
                            for index in [2, 3] {
                                if existing_pointer(dimension_record, index, entries, ctx)?
                                    .is_some_and(|(_, leader)| leader.form != 4)
                                {
                                    arrows += 1;
                                }
                            }
                            arrows
                        }
                        None => 0,
                    }
                }
                _ => 0,
            };
            let arrow_cardinality_valid = !dimension
                .is_some_and(|(_, dimension)| dimension.entity_type == 216)
                || arrow_count != 2
                || geometry_count == 2;
            let back_pointer_owners = ctx.get_btree_map(
                association_owners,
                &entry.sequence,
                "iges association owner lookup",
            )?;
            let dimension_back_pointer_valid = match (dimension, back_pointer_owners) {
                (Some((sequence, _)), Some(owners)) => {
                    owners.len() == 1 && owners.first() == Some(&sequence)
                }
                _ => false,
            };
            Ok(orientation_valid
                && geometry_valid
                && arrow_cardinality_valid
                && dimension_back_pointer_valid)
        }
        _ => Ok(false),
    }
}

fn vertex_position(
    index: &ModelIndex<'_>,
    vertex: &VertexId,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Point3>, CodecError> {
    let Some(vertex) = index.vertices(vertex.as_str(), ctx)? else {
        return Ok(None);
    };
    Ok(index
        .points(vertex.point.as_str(), ctx)?
        .map(|point| point.position().get()))
}

fn plane_carrier(
    index: &ModelIndex<'_>,
    sequence: u32,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(Point3, Vector3)>, CodecError> {
    let mut key_storage = [0_u8; 64];
    let Some(key) =
        crate::ids::directory_lookup_key("iges:model:surface#D", sequence, &mut key_storage)
    else {
        return Ok(None);
    };
    let Some(surface) = index.surfaces(key, ctx)? else {
        return Ok(None);
    };
    Ok(match surface.geometry.solved() {
        Some(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            Some((origin, *normal))
        }
        _ => None,
    })
}

fn planes_are_coplanar(
    parent: (Point3, Vector3),
    child: (Point3, Vector3),
    resolution: f64,
) -> bool {
    let (parent_origin, parent_normal) = parent;
    let (child_origin, child_normal) = child;
    parent_normal.cross(child_normal).norm() <= LEGACY_PLANE_NORMAL_EPSILON
        && child_origin
            .vector_from(parent_origin)
            .dot(parent_normal)
            .abs()
            <= resolution
}

fn points_coincident(left: Point3, right: Point3, resolution: f64) -> bool {
    let distance = left.distance(right);
    distance == 0.0 || (resolution > 0.0 && distance < resolution)
}

fn linear_nurbs_boundary_points(
    nurbs: &NurbsCurve,
    parameter_range: [f64; 2],
    transform: Transform,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Point3>>, CodecError> {
    let knots = nurbs.knots().as_slice();
    let control_count = nurbs.pole_count();
    if nurbs.periodic()
        || nurbs.degree() != 1
        || control_count < 2
        || control_count.checked_add(2) != Some(knots.len())
        || !parameter_range[0].is_finite()
        || !parameter_range[1].is_finite()
        || parameter_range[0] >= parameter_range[1]
    {
        return Ok(None);
    }
    if parameter_range[0] < knots[1] || parameter_range[1] > knots[control_count] {
        return Ok(None);
    }
    if let cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } = nurbs.pole_rows() {
        if ctx.any_by(
            points.iter(),
            |pole| Ok(pole.weight.get() <= 0.0),
            "iges plane NURBS weights",
        )? {
            return Ok(None);
        }
    }
    if !ctx.all_by(
        knots.windows(2),
        |pair| {
            Ok(pair[0].is_finite()
                && pair[1].is_finite()
                && pair[0] <= pair[1]
                && !(pair[0] == pair[1]
                    && parameter_range[0] < pair[0]
                    && pair[0] < parameter_range[1]))
        },
        "iges plane NURBS knot validation",
    )? {
        return Ok(None);
    }
    let mut points = Vec::new();
    let mut append_sample = |parameter| -> Result<bool, CodecError> {
        let Some(point) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, nurbs, parameter),
        )?)?
        else {
            return Ok(false);
        };
        let Some(point) = transform.apply_point(point.get()) else {
            return Ok(false);
        };
        ctx.push_vec(&mut points, point.get(), "iges plane NURBS boundary points")?;
        Ok(true)
    };
    if !append_sample(parameter_range[0])? {
        return Ok(None);
    }
    let mut interior = knots.iter();
    while let Some(knot) = ctx.next_charged(&mut interior, "iges plane NURBS interior knots")? {
        if parameter_range[0] < *knot && *knot < parameter_range[1] && !append_sample(*knot)? {
            return Ok(None);
        }
    }
    if !append_sample(parameter_range[1])? {
        return Ok(None);
    }
    Ok(Some(points))
}

fn linear_nurbs_is_simple_closed(
    nurbs: &NurbsCurve,
    parameter_range: [f64; 2],
    plane: (Point3, Vector3),
    resolution: f64,
    transform: Transform,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "iges plane boundary samples")?;
    let Some(points) = storage
        .with_storage(|| linear_nurbs_boundary_points(nurbs, parameter_range, transform, ctx))?
    else {
        return Ok(false);
    };
    if points.len() < 3
        || !points_coincident(points[0], *points.last().unwrap_or(&points[0]), resolution)
        || super::geometry::closed_polyline_has_duplicate(
            &points,
            |left, right| points_coincident(*left, *right, resolution),
            ctx,
        )?
    {
        return Ok(false);
    }
    let Some(projected) = storage.with_storage(|| plane_coordinates(&points, plane, ctx))? else {
        return Ok(false);
    };
    Ok(!planar_polyline_has_self_intersection(&projected, ctx)?)
}

fn analytic_curve_is_simple_closed(
    geometry: &SolvedCurveGeometry,
    parameter_range: [f64; 2],
) -> bool {
    let period = parameter_range[1] - parameter_range[0];
    if !period.is_finite() || period <= 0.0 || !angularly_equal(period, std::f64::consts::TAU) {
        return false;
    }
    matches!(
        geometry,
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)
    )
}

#[derive(Clone, Copy)]
struct PlaneBoundarySimplicity<'ir, 'ctx> {
    index: &'ir ModelIndex<'ir>,
    plane: (Point3, Vector3),
    resolution: f64,
    transform: Transform,
    ctx: &'ctx DecodeContext<'ctx>,
}

fn bounded_plane_curve_is_simple(
    geometry: &SolvedCurveGeometry,
    context: PlaneBoundarySimplicity<'_, '_>,
    source_is_certified_simple: bool,
    parameter_range: Option<[f64; 2]>,
    active: &mut BTreeSet<CurveId>,
) -> Result<bool, CodecError> {
    let _nested = context.ctx.enter_nested("iges plane boundary simplicity")?;
    match geometry {
        SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Line(_)
        | SolvedCurveGeometry::Parabola(_)
        | SolvedCurveGeometry::Hyperbola(_)
        | SolvedCurveGeometry::Unknown { .. } => Ok(false),
        SolvedCurveGeometry::Composite {
            segments,
            self_intersect,
        } => {
            if self_intersect != &Some(false) {
                return Ok(false);
            }
            let mut input = segments.iter();
            while let Some(segment) = context
                .ctx
                .next_charged(&mut input, "iges plane boundary segment traversal")?
            {
                let Some(curve) = context.index.curves(segment.curve.as_str(), context.ctx)? else {
                    return Ok(false);
                };
                if context.ctx.contains_btree_set(
                    active,
                    &segment.curve,
                    "iges plane boundary active lookup",
                )? {
                    return Ok(false);
                }
                let mut active_storage = context
                    .ctx
                    .reserve_scoped(0, "iges plane boundary active scratch")?;
                let active_id = active_storage.with_storage(|| {
                    segment
                        .curve
                        .try_clone_for_decode(context.ctx, "iges plane boundary child curve ID")
                })?;
                active_storage.with_storage(|| {
                    context.ctx.insert_btree_set(
                        active,
                        active_id,
                        "iges plane boundary active curve",
                    )
                })?;
                let Some(geometry) = curve.geometry.solved() else {
                    context.ctx.remove_btree_set(
                        active,
                        &segment.curve,
                        "iges plane boundary active removal",
                    )?;
                    return Ok(false);
                };
                let valid = bounded_plane_curve_is_simple(geometry, context, false, None, active);
                context.ctx.remove_btree_set(
                    active,
                    &segment.curve,
                    "iges plane boundary active removal",
                )?;
                if !valid? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        SolvedCurveGeometry::Transformed(placed) => {
            let Ok(transform) = context.transform.compose(*placed.transform()) else {
                return Ok(false);
            };
            bounded_plane_curve_is_simple(
                placed.basis(),
                PlaneBoundarySimplicity {
                    transform,
                    ..context
                },
                source_is_certified_simple,
                parameter_range,
                active,
            )
        }
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => {
            Ok(parameter_range
                .is_some_and(|range| analytic_curve_is_simple_closed(geometry, range)))
        }
        SolvedCurveGeometry::Nurbs(nurbs) => {
            if source_is_certified_simple {
                return Ok(true);
            }
            let Some(range) = parameter_range else {
                return Ok(false);
            };
            linear_nurbs_is_simple_closed(
                nurbs,
                range,
                context.plane,
                context.resolution,
                context.transform,
                context.ctx,
            )
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            let active_range_matches = parameter_range.is_none_or(|range| {
                polyline.parameters().is_some_and(|_| {
                    let first = polyline.parameter_at(0);
                    let last = polyline
                        .point_count()
                        .checked_sub(1)
                        .and_then(|index| polyline.parameter_at(index))
                        .or(first);
                    first.map(cadmpeg_ir::scalar::FiniteReal::get) == Some(range[0])
                        && last.map(cadmpeg_ir::scalar::FiniteReal::get) == Some(range[1])
                })
            });
            if !active_range_matches {
                return Ok(false);
            }
            let mut storage = context
                .ctx
                .reserve_scoped(0, "iges plane polyline samples")?;
            let points = storage.with_storage(|| {
                context.ctx.collect_options(
                    polyline.points().map(|point| {
                        context
                            .transform
                            .apply_point(point.get())
                            .map(cadmpeg_ir::features::FinitePoint3::get)
                    }),
                    "iges plane polyline points",
                )
            })?;
            let Some(points) = points else {
                return Ok(false);
            };
            if !(points.len() >= 3
                && points_coincident(
                    points[0],
                    *points.last().unwrap_or(&points[0]),
                    context.resolution,
                )
                && !super::geometry::closed_polyline_has_duplicate(
                    &points,
                    |left, right| points_coincident(*left, *right, context.resolution),
                    context.ctx,
                )?)
            {
                return Ok(false);
            }
            let Some(projected) =
                storage.with_storage(|| plane_coordinates(&points, context.plane, context.ctx))?
            else {
                return Ok(false);
            };
            Ok(!planar_polyline_has_self_intersection(
                &projected,
                context.ctx,
            )?)
        }
    }
}

enum PlaneBoundaryError {
    MissingEdge,
    MissingCurve,
    MissingCurveCarrier,
    NotSimple,
    NotCoplanar,
    MissingStart,
    MissingEnd,
    NotClosed,
    Resource(CodecError),
}

impl PlaneBoundaryError {
    fn message(self) -> Result<&'static str, CodecError> {
        Ok(match self {
            Self::MissingEdge => "plane boundary curve was not projected as a bounded edge",
            Self::MissingCurve => "plane boundary edge has no curve carrier",
            Self::MissingCurveCarrier => "plane boundary edge curve carrier is missing",
            Self::NotSimple => {
                "plane boundary curve is not proven simple, degenerate, cyclic, or self-intersecting"
            }
            Self::NotCoplanar => "plane boundary curve does not lie in the plane",
            Self::MissingStart => "plane boundary start vertex is missing",
            Self::MissingEnd => "plane boundary end vertex is missing",
            Self::NotClosed => "plane boundary curve is not closed",
            Self::Resource(error) => return Err(error),
        })
    }

    fn legacy_message(self) -> Result<&'static str, CodecError> {
        Ok(match self {
            Self::MissingEdge => "legacy single-parent plane boundary was not projected",
            Self::MissingCurve | Self::MissingCurveCarrier => {
                "legacy single-parent plane boundary carrier is invalid"
            }
            Self::NotSimple => {
                "legacy single-parent plane boundary is not proven simple, degenerate, cyclic, or self-intersecting"
            }
            Self::NotCoplanar => "legacy single-parent plane boundary does not lie in the plane",
            Self::MissingStart => "legacy single-parent boundary start vertex is missing",
            Self::MissingEnd => "legacy single-parent boundary end vertex is missing",
            Self::NotClosed => "legacy single-parent plane boundary is not closed",
            Self::Resource(error) => return Err(error),
        })
    }
}

impl From<CodecError> for PlaneBoundaryError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

type PlaneBoundaryKey = (u32, [u64; 7]);

/// Successful boundary proofs for one immutable model.
struct PlaneBoundaryProofs<'ir, 'ctx> {
    proven: BTreeMap<PlaneBoundaryKey, &'ir Edge>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn plane_boundary_edge<'ir>(
    index: &ModelIndex<'ir>,
    plane: (Point3, Vector3),
    boundary_sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    resolution: f64,
    ctx: &DecodeContext<'_>,
    proofs: &mut PlaneBoundaryProofs<'ir, '_>,
) -> Result<&'ir Edge, PlaneBoundaryError> {
    let proof_key = (
        boundary_sequence,
        [
            plane.0.x.to_bits(),
            plane.0.y.to_bits(),
            plane.0.z.to_bits(),
            plane.1.x.to_bits(),
            plane.1.y.to_bits(),
            plane.1.z.to_bits(),
            resolution.to_bits(),
        ],
    );
    if let Some(edge) = ctx.get_btree_map(
        &proofs.proven,
        &proof_key,
        "iges plane boundary proof cache lookup",
    )? {
        return Ok(*edge);
    }
    let mut key_storage = [0_u8; 64];
    let key =
        crate::ids::directory_lookup_key("iges:model:edge#D", boundary_sequence, &mut key_storage)
            .ok_or(PlaneBoundaryError::MissingEdge)?;
    let source_edge = index
        .edges(key, ctx)
        .map_err(CodecError::from)?
        .ok_or(PlaneBoundaryError::MissingEdge)?;
    let curve_id = source_edge
        .curve()
        .ok_or(PlaneBoundaryError::MissingCurve)?;
    let curve = index
        .curves(curve_id.as_str(), ctx)
        .map_err(CodecError::from)?
        .ok_or(PlaneBoundaryError::MissingCurveCarrier)?;
    let Some(geometry) = curve.geometry.solved() else {
        return Err(PlaneBoundaryError::MissingCurveCarrier);
    };
    let source_is_certified_simple = ctx
        .get_btree_map(
            entries,
            &boundary_sequence,
            "iges plane boundary directory lookup",
        )?
        .is_some_and(|entry| entry.entity_type == 106 && entry.form == 63);
    let mut active_storage = ctx.reserve_scoped(0, "iges plane boundary active scratch")?;
    let mut active = BTreeSet::new();
    let active_id = active_storage.with_storage(|| {
        curve_id.try_clone_for_decode(ctx, "iges plane boundary active curve ID")
    })?;
    if !active_storage.with_storage(|| {
        ctx.insert_btree_set(&mut active, active_id, "iges plane boundary active curve")
    })? || !bounded_plane_curve_is_simple(
        geometry,
        PlaneBoundarySimplicity {
            index,
            plane,
            resolution,
            transform: Transform::identity(),
            ctx,
        },
        source_is_certified_simple,
        source_edge
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::get),
        &mut active,
    )? {
        return Err(PlaneBoundaryError::NotSimple);
    }
    if !curve_geometry_coplanar(
        geometry,
        index,
        Transform::identity(),
        plane,
        resolution,
        &mut BTreeSet::new(),
        ctx,
    )? {
        return Err(PlaneBoundaryError::NotCoplanar);
    }
    let start =
        vertex_position(index, &source_edge.start, ctx)?.ok_or(PlaneBoundaryError::MissingStart)?;
    let end =
        vertex_position(index, &source_edge.end, ctx)?.ok_or(PlaneBoundaryError::MissingEnd)?;
    if start.distance(end) > resolution {
        return Err(PlaneBoundaryError::NotClosed);
    }
    proofs.storage.with_storage(|| {
        ctx.insert_btree_map(
            &mut proofs.proven,
            proof_key,
            source_edge,
            "iges plane boundary proof cache",
        )
    })?;
    Ok(source_edge)
}

fn closed_plane_boundary_edge(
    source: &Edge,
    id: EdgeId,
    ctx: &DecodeContext<'_>,
) -> Result<Edge, CodecError> {
    let curve = source
        .curve()
        .ok_or_else(|| CodecError::malformed("validated plane edge has no curve"))?;
    Ok(Edge {
        id,
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(
            Some(curve.try_clone_for_decode(ctx, "iges selected edge curve ID")?),
            source
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::get),
        )
        .map_err(CodecError::malformed)?,
        start: source
            .start
            .try_clone_for_decode(ctx, "iges selected edge start ID")?,
        end: source
            .start
            .try_clone_for_decode(ctx, "iges structure identity copy")?,
        tolerance: source.tolerance,
    })
}

fn plane_face_draft(
    surface_sequence: u32,
    source_sequence: u32,
    stem: &crate::ids::Stem,
    boundary_edges: Vec<Edge>,
    resolution: f64,
    sequences: &mut super::geometry::SourceSequences<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<ModelDraft, LegacyPlaneError> {
    let tolerance = if resolution > 0.0 {
        Some(
            cadmpeg_ir::scalar::PositiveReal::new(resolution)
                .ok_or("face tolerance must be finite")?,
        )
    } else {
        None
    };
    let body_id = crate::ids::body_admitted(stem, ctx)?;
    sequences.record_body(&body_id, source_sequence, stem, ctx)?;
    let region_id = crate::ids::region_admitted(stem, ctx)?;
    let shell_id = crate::ids::shell_admitted(stem, ctx)?;
    let face_id = crate::ids::face_admitted(stem, ctx)?;
    sequences.record_face(&face_id, source_sequence, ctx)?;
    let mut candidate = ModelDraft::new();
    let mut outer_loop = None;
    let mut loop_ids = ctx.collection_vec(
        boundary_edges.len().saturating_sub(1),
        "iges legacy plane loop IDs",
    )?;
    let mut input = boundary_edges.into_iter().enumerate();
    while let Some((boundary_index, edge)) =
        ctx.next_charged(&mut input, "iges structure list traversal")?
    {
        let edge_id = edge
            .id
            .try_clone_for_decode(ctx, "iges structure identity copy")?;
        ctx.reserve_vec(
            &mut candidate.model_mut().edges,
            1,
            "iges legacy plane edge slots",
        )?;
        ctx.charge_entities(1, "iges_geometry_structure")?;
        candidate.model_mut().edges.push(edge);
        let loop_id = crate::ids::loop_admitted(&stem.slot(boundary_index), ctx)?;
        let coedge_id = crate::ids::coedge_admitted(&stem.slot(boundary_index), ctx)?;
        ctx.reserve_vec(
            &mut candidate.model_mut().coedges,
            1,
            "iges legacy plane coedge slots",
        )?;
        ctx.charge_entities(1, "iges_geometry_structure")?;
        candidate.model_mut().coedges.push(Coedge {
            id: coedge_id.try_clone_for_decode(ctx, "iges structure identity copy")?,
            owner_loop: loop_id.try_clone_for_decode(ctx, "iges structure identity copy")?,
            edge: edge_id,
            radial_next: coedge_id.try_clone_for_decode(ctx, "iges structure identity copy")?,
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        });
        let mut ring_coedges = ctx.collection_vec(1, "iges legacy plane ring coedges")?;
        ring_coedges.push(coedge_id);
        let ring = match cadmpeg_ir::topology::LoopRing::new(ctx, ring_coedges, Vec::new())
            .map_err(cadmpeg_core::CodecError::from)
        {
            Ok(Ok(ring)) => ring,
            Ok(Err(_)) => return Err("legacy plane loop ring is invalid".into()),
            Err(error) => return Err(error.into()),
        };
        ctx.reserve_vec(
            &mut candidate.model_mut().loops,
            1,
            "iges legacy plane loop slots",
        )?;
        ctx.charge_entities(1, "iges_geometry_structure")?;
        candidate.model_mut().loops.push(Loop {
            id: loop_id.try_clone_for_decode(ctx, "iges structure identity copy")?,
            face: face_id.try_clone_for_decode(ctx, "iges structure identity copy")?,
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
        });
        if boundary_index == 0 {
            outer_loop = Some(loop_id);
        } else {
            loop_ids.push(loop_id);
        }
    }
    let face_loops = match outer_loop {
        Some(outer) => cadmpeg_ir::topology::FaceLoops::classified(outer, loop_ids),
        None => cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
    };
    ctx.reserve_vec(
        &mut candidate.model_mut().faces,
        1,
        "iges legacy plane face slots",
    )?;
    ctx.charge_entities(1, "iges_geometry_structure")?;
    candidate.model_mut().faces.push(Face {
        id: face_id.try_clone_for_decode(ctx, "iges structure identity copy")?,
        shell: shell_id.try_clone_for_decode(ctx, "iges structure identity copy")?,
        surface: crate::ids::surface_admitted(&crate::ids::Stem::directory(surface_sequence), ctx)?,
        sense: Sense::Forward,
        loops: face_loops,
        name: None,
        color: None,
        tolerance,
    });
    let mut shell_faces = ctx.collection_vec(1, "iges legacy plane shell faces")?;
    shell_faces.push(face_id);
    let shell = Shell::new(
        shell_id.try_clone_for_decode(ctx, "iges structure identity copy")?,
        region_id.try_clone_for_decode(ctx, "iges structure identity copy")?,
        shell_faces,
        Vec::new(),
        Vec::new(),
    )
    .map_err(|_| LegacyPlaneError::Invalid("legacy plane shell is empty"))?;
    ctx.reserve_vec(
        &mut candidate.model_mut().shells,
        1,
        "iges legacy plane shell slots",
    )?;
    ctx.charge_entities(1, "iges_geometry_structure")?;
    candidate.model_mut().shells.push(shell);
    let mut region_shells = ctx.collection_vec(1, "iges legacy plane region shells")?;
    region_shells.push(shell_id);
    ctx.reserve_vec(
        &mut candidate.model_mut().regions,
        1,
        "iges legacy plane region slots",
    )?;
    ctx.charge_entities(1, "iges_geometry_structure")?;
    candidate.model_mut().regions.push(Region {
        id: region_id.try_clone_for_decode(ctx, "iges structure identity copy")?,
        body: body_id.try_clone_for_decode(ctx, "iges structure identity copy")?,
        shells: region_shells,
    });
    let mut body_regions = ctx.collection_vec(1, "iges legacy plane body regions")?;
    body_regions.push(region_id);
    ctx.reserve_vec(
        &mut candidate.model_mut().bodies,
        1,
        "iges legacy plane body slots",
    )?;
    ctx.charge_entities(1, "iges_geometry_structure")?;
    candidate.model_mut().bodies.push(Body {
        id: body_id,
        kind: BodyKind::Sheet,
        regions: body_regions,
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    candidate.model_mut().finalize(ctx)?;
    Ok(candidate)
}

enum LegacyPlaneError {
    Invalid(&'static str),
    Resource(CodecError),
}

impl From<&'static str> for LegacyPlaneError {
    fn from(message: &'static str) -> Self {
        Self::Invalid(message)
    }
}

impl From<CodecError> for LegacyPlaneError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

impl LegacyPlaneError {
    fn non_resource(self) -> Result<&'static str, CodecError> {
        match self {
            Self::Invalid(message) => Ok(message),
            Self::Resource(error) => Err(error),
        }
    }
}

#[derive(Clone, Copy)]
struct LegacyPlaneSource<'a> {
    entry: &'a DirectoryEntry,
    record: &'a ParameterRecord,
}

fn legacy_single_parent_face<'ir, 'ctx>(
    proof_context: (&ModelIndex<'ir>, &mut PlaneBoundaryProofs<'ir, '_>),
    source: LegacyPlaneSource<'_>,
    parent: (u32, &DirectoryEntry),
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    global: &ProjectedGlobal,
    ctx: &'ctx DecodeContext<'_>,
    sequences: &mut super::geometry::SourceSequences<'_>,
) -> Result<
    Option<(
        ModelDraft,
        Vec<u32>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
    LegacyPlaneError,
> {
    let (index, proofs) = proof_context;
    let LegacyPlaneSource { entry, record } = source;
    let (parent_sequence, parent_entry) = parent;
    if parent_entry.entity_type != 108 || parent_entry.form != 1 {
        return Ok(None);
    }
    let Some(child_count) = record.count(2).filter(|count| *count > 0) else {
        return Err("legacy single-parent plane hole has no children".into());
    };
    let mut child_storage;
    let (mut children, result_child_storage) =
        ctx.temporary_vec(child_count, "iges legacy plane child pointers")?;
    child_storage = result_child_storage;
    let mut children_are_type_108 = true;
    let mut children_have_negative_physical_status = true;
    let mut input = 0..child_count;
    while let Some(offset) = ctx.next_charged(&mut input, "iges structure list traversal")? {
        let Some((child_sequence, child_entry)) =
            existing_pointer(record, 4 + offset, entries, ctx)?
        else {
            return Err("legacy single-parent plane hole has an invalid child pointer".into());
        };
        children_are_type_108 &= child_entry.entity_type == 108;
        if child_entry.entity_type == 108 {
            children_have_negative_physical_status &=
                child_entry.form == -1 && child_entry.status.is_physically_dependent();
        }
        children.push(child_sequence);
    }
    if !children_are_type_108 {
        return Ok(None);
    }
    if !children_have_negative_physical_status {
        return Err(
            "legacy single-parent plane hole requires negative, physically dependent Type 108 children"
                .into(),
        );
    }

    let boundary_count = child_count
        .checked_add(1)
        .ok_or("legacy single-parent plane hole has an invalid child pointer")?;
    let _boundary_storage;
    let (mut boundary_sequences, result_boundary_storage) =
        ctx.temporary_vec(boundary_count, "iges legacy plane boundary pointers")?;
    _boundary_storage = result_boundary_storage;
    for sequence in std::iter::once(parent_sequence).chain(
        ctx.admit_iter(&children, "iges legacy plane children traversal")
            .map_err(CodecError::from)?
            .copied(),
    ) {
        let Some(plane) =
            ctx.get_btree_map(records, &sequence, "iges legacy plane record lookup")?
        else {
            return Err("legacy single-parent plane has an invalid boundary pointer".into());
        };
        let Some((boundary, _)) = existing_pointer(plane, 5, entries, ctx)? else {
            return Err("legacy single-parent plane has an invalid boundary pointer".into());
        };
        boundary_sequences.push(boundary);
    }
    let parent_plane = plane_carrier(index, parent_sequence, ctx)?
        .ok_or("legacy single-parent parent plane was not projected")?;
    let resolution = global.minimum_resolution_mm();
    let _edge_storage;
    let (mut boundary_edges, result_edge_storage) =
        ctx.temporary_vec(boundary_sequences.len(), "iges legacy plane boundary edges")?;
    _edge_storage = result_edge_storage;
    let mut input = std::iter::once(parent_sequence)
        .chain(children.iter().copied())
        .zip(boundary_sequences.iter().copied())
        .enumerate();
    while let Some((boundary_index, (plane_sequence, boundary_sequence))) =
        ctx.next_charged(&mut input, "iges structure list traversal")?
    {
        let plane = plane_carrier(index, plane_sequence, ctx)?
            .ok_or("legacy single-parent child plane was not projected")?;
        if !planes_are_coplanar(parent_plane, plane, resolution) {
            return Err("legacy single-parent plane boundaries are not coplanar".into());
        }
        let edge = plane_boundary_edge(
            index,
            plane,
            boundary_sequence,
            entries,
            resolution,
            ctx,
            proofs,
        )
        .map_err(|error| match error.legacy_message() {
            Ok(message) => LegacyPlaneError::Invalid(message),
            Err(resource) => LegacyPlaneError::Resource(resource),
        })?;
        let edge_id = crate::ids::edge_admitted(
            &crate::ids::Stem::word_directory(crate::ids::Word::LegacySingleParent, entry.sequence)
                .tail_index(boundary_index),
            ctx,
        )?;
        let edge = closed_plane_boundary_edge(edge, edge_id, ctx)?;
        boundary_edges.push(edge);
    }
    let stem =
        crate::ids::Stem::word_directory(crate::ids::Word::LegacySingleParent, entry.sequence);
    child_storage.with_storage(|| {
        ctx.insert_vec(
            &mut children,
            0,
            parent_sequence,
            "iges legacy plane sequence list",
        )
    })?;
    Ok(Some((
        plane_face_draft(
            parent_sequence,
            entry.sequence,
            &stem,
            boundary_edges,
            resolution,
            sequences,
            ctx,
        )?,
        children,
        child_storage,
    )))
}

enum FlowPointer {
    Null,
    Sequence(u32),
}

fn read_flow_pointer(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    cursor: &mut usize,
    nullable: bool,
    ctx: &DecodeContext<'_>,
) -> Result<Option<FlowPointer>, CodecError> {
    let Some(raw) = record.integer(*cursor) else {
        return Ok(None);
    };
    let index = *cursor;
    let Some(next) = cursor.checked_add(1) else {
        return Ok(None);
    };
    *cursor = next;
    if nullable && raw == 0 {
        return Ok(Some(FlowPointer::Null));
    }
    Ok(existing_pointer(record, index, entries, ctx)?
        .map(|(sequence, _)| FlowPointer::Sequence(sequence)))
}

fn read_flow_required_pointers(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    cursor: &mut usize,
    count: usize,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<Option<Vec<u32>>, CodecError> {
    let mut pointers = ctx.collection_vec(count, operation)?;
    let mut input = 0..count;
    while ctx.next_charged(&mut input, operation)?.is_some() {
        let Some(FlowPointer::Sequence(sequence)) =
            read_flow_pointer(record, entries, cursor, false, ctx)?
        else {
            return Ok(None);
        };
        pointers.push(sequence);
    }
    Ok(Some(pointers))
}

fn read_flow_optional_pointers(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    cursor: &mut usize,
    count: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Option<u32>>>, CodecError> {
    let mut pointers = ctx.collection_vec(count, "iges flow continuation pointers")?;
    let mut input = 0..count;
    while ctx
        .next_charged(&mut input, "iges structure list traversal")?
        .is_some()
    {
        let Some(pointer) = read_flow_pointer(record, entries, cursor, true, ctx)? else {
            return Ok(None);
        };
        pointers.push(match pointer {
            FlowPointer::Null => None,
            FlowPointer::Sequence(sequence) => Some(sequence),
        });
    }
    Ok(Some(pointers))
}

fn definition_members(
    record: &ParameterRecord,
    count: usize,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<Option<Vec<u32>>, CodecError> {
    let mut members = ctx.collection_vec(count, operation)?;
    let mut input = 0..count;
    while let Some(index) = ctx.next_charged(&mut input, operation)? {
        let Some(sequence) = record
            .integer(4 + index)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|sequence| sequence % 2 == 1)
        else {
            return Ok(None);
        };
        if ctx
            .get_btree_map(entries, &sequence, "iges subfigure member directory lookup")?
            .is_none()
        {
            return Ok(None);
        }
        members.push(sequence);
    }
    Ok(Some(members))
}

fn flow_associativity(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    association_owners: &BTreeMap<u32, BTreeSet<u32>>,
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
) -> Result<Option<FlowAssociativity>, CodecError> {
    if !flow_associativity_directory_valid(entry, global_table) {
        return Ok(None);
    }
    let form = entry.form;
    let context_count = if form == 18 { 2 } else { 1 };
    if record.integer(1) != Some(context_count) {
        return Ok(None);
    }
    let Some(counts) = (|| {
        Some([
            record.count(2)?,
            record.count(3)?,
            record.count(4)?,
            record.count(5)?,
            record.count(6)?,
            record.count(7)?,
        ])
    })() else {
        return Ok(None);
    };
    if record
        .integer(8)
        .is_none_or(|value| !matches!(value, 0..=2))
    {
        return Ok(None);
    }
    let function_flag = (form == 18)
        .then(|| record.integer(9).filter(|value| matches!(value, 0..=2)))
        .flatten();
    if form == 18 && function_flag.is_none() {
        return Ok(None);
    }
    let mut cursor = if form == 18 { 10 } else { 9 };
    let Some(associated) = read_flow_required_pointers(
        record,
        entries,
        &mut cursor,
        counts[0],
        ctx,
        "iges flow associated pointers",
    )?
    else {
        return Ok(None);
    };
    let mut local_storage = ctx.reserve_scoped(0, "iges flow validation scratch")?;
    let Some(connections) = local_storage.with_storage(|| {
        read_flow_required_pointers(
            record,
            entries,
            &mut cursor,
            counts[1],
            ctx,
            "iges flow connection pointers",
        )
    })?
    else {
        return Ok(None);
    };
    let Some(joins) = local_storage.with_storage(|| {
        read_flow_required_pointers(
            record,
            entries,
            &mut cursor,
            counts[2],
            ctx,
            "iges flow join pointers",
        )
    })?
    else {
        return Ok(None);
    };
    let mut input = 0..counts[3];
    while ctx
        .next_charged(&mut input, "iges structure list traversal")?
        .is_some()
    {
        if record.string(cursor).is_none_or(<[u8]>::is_empty) {
            return Ok(None);
        }
        let Some(next) = cursor.checked_add(1) else {
            return Ok(None);
        };
        cursor = next;
    }
    let Some(displays) = local_storage.with_storage(|| {
        read_flow_required_pointers(
            record,
            entries,
            &mut cursor,
            counts[4],
            ctx,
            "iges flow display pointers",
        )
    })?
    else {
        return Ok(None);
    };
    let Some(continuations) =
        read_flow_optional_pointers(record, entries, &mut cursor, counts[5], ctx)?
    else {
        return Ok(None);
    };
    let associated_valid = ctx.all_by(
        associated.iter(),
        |sequence| {
            Ok(ctx
                .get_btree_map(entries, sequence, "iges flow target lookup")?
                .is_some_and(|target| target.entity_type == 402 && target.form == form))
        },
        "iges structure list validation",
    )?;
    let connections_valid = ctx.all_by(
        connections.iter(),
        |sequence| {
            let Some(target) =
                ctx.get_btree_map(entries, sequence, "iges flow target lookup")?
            else {
                return Ok(false);
            };
            if !flow_connection_target_valid(target, form, global_table) {
                return Ok(false);
            }
            if form == 20 {
                return Ok(true);
            }
            let Some(member) =
                ctx.get_btree_map(records, sequence, "iges flow parameter record lookup")?
            else {
                return Ok(false);
            };
            has_association_back_pointer(member, entry.sequence, association_owners, ctx)
        },
        "iges structure list validation",
    )?;
    let joins_valid = ctx.all_by(
        joins.iter(),
        |sequence| {
            if form != 20 {
                let Some(member) =
                    ctx.get_btree_map(records, sequence, "iges flow parameter record lookup")?
                else {
                    return Ok(false);
                };
                if !has_association_back_pointer(member, entry.sequence, association_owners, ctx)? {
                    return Ok(false);
                }
            }
            Ok(ctx
                .get_btree_map(entries, sequence, "iges flow target lookup")?
                .is_some_and(|target| flow_join_target_valid(target, global_table)))
        },
        "iges structure list validation",
    )?;
    let displays_valid = ctx.all_by(
        displays.iter(),
        |sequence| {
            Ok(ctx
                .get_btree_map(entries, sequence, "iges flow target lookup")?
                .is_some_and(|target| flow_display_target_valid(target, form, global_table)))
        },
        "iges structure list validation",
    )?;
    let continuations_valid = ctx.all_by(
        &continuations,
        |slot| {
            let Some(sequence) = slot else {
                return Ok(true);
            };
            let Some(target) =
                ctx.get_btree_map(entries, sequence, "iges flow target lookup")?
            else {
                return Ok(false);
            };
            if !flow_continuation_target_valid(target, form, global_table) {
                return Ok(false);
            }
            if form == 20 {
                return Ok(true);
            }
            let Some(member) =
                ctx.get_btree_map(records, sequence, "iges flow parameter record lookup")?
            else {
                return Ok(false);
            };
            has_association_back_pointer(member, entry.sequence, association_owners, ctx)
        },
        "iges structure list validation",
    )?;
    Ok((cursor == record.parameter_end()
        && associated_valid
        && connections_valid
        && joins_valid
        && displays_valid
        && continuations_valid)
        .then_some(FlowAssociativity {
            form,
            associated,
            continuations,
        }))
}

/// The structure admission failure for one product instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlacementRejection {
    MissingRecord,
    InvalidDefinition,
    InvalidPlacement,
    InvalidMetadata { definition: u32 },
}

/// The local affine placement of a subfigure or network instance.
pub(crate) enum PlacementAffineError {
    Invalid,
    Resource(CodecError),
}

impl From<()> for PlacementAffineError {
    fn from((): ()) -> Self {
        Self::Invalid
    }
}

impl From<TransformResolutionError> for PlacementAffineError {
    fn from(error: TransformResolutionError) -> Self {
        match error {
            TransformResolutionError::Invalid(_) => Self::Invalid,
            TransformResolutionError::Resource(error) => Self::Resource(error),
        }
    }
}

impl PlacementAffineError {
    pub(crate) fn non_resource(self) -> Result<(), CodecError> {
        match self {
            Self::Invalid => Ok(()),
            Self::Resource(error) => Err(error),
        }
    }
}

pub(crate) fn placement_affine(
    instance: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: f64,
    precision: RealPrecision,
    ctx: &DecodeContext<'_>,
) -> Result<(u32, Transform), PlacementAffineError> {
    let definition = u32::try_from(record.integer(1).ok_or(())?).map_err(|_| ())?;
    let translation_component = |index| {
        record
            .number_or(index, 0.0)
            .and_then(FiniteReal::new)
            .ok_or(())
    };
    let scale_component = |index, default| {
        record
            .number_or(index, default)
            .and_then(PositiveReal::new)
            .ok_or(())
    };
    let x_scale = scale_component(5, 1.0)?;
    let scales = if instance.entity_type == 420 {
        [
            x_scale,
            scale_component(6, x_scale.get())?,
            scale_component(7, x_scale.get())?,
        ]
    } else {
        [x_scale; 3]
    };
    let translation = Transform::affine([
        [
            1.0,
            0.0,
            0.0,
            translation_component(2)?.get() * length_factor,
        ],
        [
            0.0,
            1.0,
            0.0,
            translation_component(3)?.get() * length_factor,
        ],
        [
            0.0,
            0.0,
            1.0,
            translation_component(4)?.get() * length_factor,
        ],
    ])
    .ok_or(())?;
    let scale = Transform::affine([
        [scales[0].get(), 0.0, 0.0, 0.0],
        [0.0, scales[1].get(), 0.0, 0.0],
        [0.0, 0.0, scales[2].get(), 0.0],
    ])
    .ok_or(())?;
    let directory = if instance.transform == 0 {
        Transform::identity()
    } else {
        resolve_transform(
            instance.transform,
            entries,
            records,
            length_factor,
            precision,
            &mut std::collections::BTreeSet::new(),
            ctx,
        )
        .map_err(PlacementAffineError::from)?
    };
    Ok((
        definition,
        directory
            .compose(translation.compose(scale).map_err(|_| ())?)
            .map_err(|_| ())?,
    ))
}

pub(super) fn project<'ctx>(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    indexes: (
        &BTreeMap<u32, &DirectoryEntry>,
        &BTreeMap<u32, &ParameterRecord>,
    ),
    trailing_pointer_analysis: &BTreeMap<u32, TrailingPointerAnalysis>,
    global: &ProjectedGlobal,
    ctx: &'ctx DecodeContext<'_>,
    sequences: &mut super::geometry::SourceSequences<'_>,
) -> Result<(ProjectionOutcome<'ctx>, BTreeMap<u32, PlacementRejection>), CodecError> {
    let (entries, records) = indexes;
    let mut scratch = ctx.reserve_scoped(0, "iges structure scratch")?;
    let mut decoded_storage = ctx.reserve_scoped(0, "iges structure decoded sequences")?;
    let mut decoded = BTreeSet::new();
    let mut loss_slots_storage = ctx.reserve_scoped(0, "iges entity loss slots")?;
    let mut losses = Vec::new();
    let mut placement_rejections = BTreeMap::new();
    let mut assemblies = BTreeMap::new();
    let mut attribute_shape_storage =
        ctx.reserve_scoped(0, "iges attribute shape index nodes")?;
    let mut attribute_shapes = BTreeMap::new();
    let mut legacy_face_candidates = Vec::<(&DirectoryEntry, ModelDraft)>::new();
    let mut legacy_plane_sequences = BTreeSet::new();
    let mut flows = BTreeMap::new();
    let mut property_owners = BTreeMap::<u32, Vec<u32>>::new();
    let mut association_owners = BTreeMap::<u32, BTreeSet<u32>>::new();
    for (owner, record) in ctx.admit_iter(records, "iges structure ownership records")? {
        let Some(TrailingPointerAnalysis::Unambiguous(groups)) =
            ctx.get_btree_map(
                trailing_pointer_analysis,
                &record.directory_sequence,
                "iges trailing pointer analysis lookup",
            )?
        else {
            continue;
        };
        for target in ctx.admit_iter(groups.properties(), "iges structure property references")? {
            if target != owner
                && ctx
                    .get_btree_map(&property_owners, target, "iges property owner lookup")?
                    .and_then(|owners| owners.last())
                    != Some(owner)
            {
                ctx.push_scoped_btree_group(
                    &mut scratch,
                    &mut property_owners,
                    *target,
                    || *owner,
                    0,
                    "iges property owner sequences",
                )?;
            }
        }
        for target in ctx.admit_iter(
            groups.associations(),
            "iges structure association references",
        )? {
            scratch.with_storage(|| {
                ctx.insert_btree_group_set(
                    &mut association_owners,
                    *target,
                    *owner,
                    "iges association owner groups",
                    "iges association owner sequences",
                )
            })?;
        }
    }
    let mut sheet_identities = None;
    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 18 | 20))
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            continue;
        };
        if let Some(flow) = scratch.with_storage(|| {
            flow_associativity(
                entry,
                record,
                entries,
                records,
                &association_owners,
                global.global_table(),
                ctx,
            )
        })? {
            scratch.with_storage(|| {
                ctx.insert_btree_map(&mut flows, entry.sequence, flow, "iges flow index nodes")
            })?;
        }
    }

    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 406 && matches!(entry.form, 2..=15 | 18..=36))
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let owners = ctx
            .get_btree_map(
                &property_owners,
                &entry.sequence,
                "iges property owner lookup",
            )?
            .map_or(&[][..], Vec::as_slice);
        let fields_valid =
            property_fields_valid(entry, record, record.parameter_end(), entries, ctx)?;
        let attachment_valid =
            entry.status.subordinate() == Some(Subordinate::Independent) || !owners.is_empty();
        let reference_designator_valid = entry.form != 7
            || ctx.all_by(
                owners.iter(),
                |owner| {
                    Ok(ctx
                        .get_btree_map(entries, owner, "iges property owner directory lookup")?
                        .is_some_and(|owner| owner.entity_type != 420))
                },
                "iges structure list validation",
            )?;
        let owner_kind_valid = match entry.form {
            22 => {
                !owners.is_empty()
                    && ctx.all_by(
                        owners.iter(),
                        |owner| {
                            Ok(ctx
                                .get_btree_map(
                                    entries,
                                    owner,
                                    "iges property owner directory lookup",
                                )?
                                .is_some_and(|owner| owner.entity_type == 404))
                        },
                        "iges structure list validation",
                    )?
            }
            23 => {
                !owners.is_empty()
                    && ctx.all_by(
                        owners.iter(),
                        |owner| {
                            Ok(ctx.get_btree_map(entries, owner, "iges property owner directory lookup")?.is_some_and(|owner| {
                                owner.entity_type == 402 && matches!(owner.form, 1 | 7 | 14 | 15)
                            }))
                        },
                        "iges structure list validation",
                    )?
            }
            26 => {
                !owners.is_empty()
                    && ctx.all_by(
                        owners.iter(),
                        |owner| {
                            Ok(ctx
                                .get_btree_map(
                                    entries,
                                    owner,
                                    "iges property owner directory lookup",
                                )?
                                .is_some_and(|owner| matches!(owner.entity_type, 116 | 132)))
                        },
                        "iges structure list validation",
                    )?
            }
            27 => entry.status.is_physically_dependent() && owners.len() == 1,
            28 | 29 => {
                !owners.is_empty()
                    && ctx.all_by(
                        owners.iter(),
                        |owner| {
                            Ok(ctx
                                .get_btree_map(
                                    entries,
                                    owner,
                                    "iges property owner directory lookup",
                                )?
                                .is_some_and(|owner| dimension_entity_type(owner.entity_type)))
                        },
                        "iges structure list validation",
                    )?
            }
            30 => {
                let note_count = record.count(13).unwrap_or_default();
                if owners.is_empty() || (note_count != 0 && owners.len() != 1) {
                    false
                } else {
                    ctx.all_by(
                        owners.iter(),
                        |owner| {
                            let Some(owner_entry) = ctx.get_btree_map(
                                entries,
                                owner,
                                "iges property owner directory lookup",
                            )? else {
                                return Ok(false);
                            };
                            if !dimension_entity_type(owner_entry.entity_type) {
                                return Ok(false);
                            }
                            let Some(owner_record) = ctx.get_btree_map(
                                records,
                                owner,
                                "iges property owner record lookup",
                            )? else {
                                return Ok(false);
                            };
                            let groups = ctx
                                .get_btree_map(
                                    trailing_pointer_analysis,
                                    &owner_record.directory_sequence,
                                    "iges trailing pointer analysis lookup",
                                )?
                                .and_then(|analysis| match analysis {
                                    TrailingPointerAnalysis::Unambiguous(groups) => Some(groups),
                                    _ => None,
                                });
                            let has_basic = match groups {
                                Some(groups) => ctx.any_by(
                                    groups.properties().iter(),
                                    |sequence| {
                                        Ok(ctx
                                            .get_btree_map(
                                                entries,
                                                sequence,
                                                "iges property reference lookup",
                                            )?
                                            .is_some_and(|property| {
                                                property.entity_type == 406 && property.form == 31
                                            }))
                                    },
                                    "iges structure list validation",
                                )?,
                                None => false,
                            };
                            let mut display_count = 0_usize;
                            if let Some(groups) = groups {
                                for sequence in ctx.admit_iter(
                                    groups.properties(),
                                    "iges dimension display property references",
                                )? {
                                    if ctx
                                        .get_btree_map(
                                            entries,
                                            sequence,
                                            "iges property reference lookup",
                                        )?
                                        .is_some_and(|property| {
                                            property.entity_type == 406 && property.form == 30
                                        })
                                    {
                                        display_count += 1;
                                    }
                                }
                            }
                            let basic_consistent =
                                (record.integer(2) == Some(2)) == has_basic;
                            let notes_valid = if note_count == 0 {
                                true
                            } else {
                                match existing_pointer(owner_record, 1, entries, ctx)? {
                                    Some((note, note_entry)) if note_entry.entity_type == 212 => {
                                        match ctx.get_btree_map(
                                            records,
                                            &note,
                                            "iges note parameter record lookup",
                                        )? {
                                            Some(note_record) => {
                                                match note_record.integer(1) {
                                                    Some(text_count) => ctx.all_by(
                                                        0..note_count,
                                                        |offset| {
                                                            Ok(record
                                                                .integer(16 + offset * 3)
                                                                .is_some_and(|last| {
                                                                    last <= text_count
                                                                }))
                                                        },
                                                        "iges structure list validation",
                                                    )?,
                                                    None => false,
                                                }
                                            }
                                            None => false,
                                        }
                                    }
                                    _ => false,
                                }
                            };
                            Ok(display_count == 1 && basic_consistent && notes_valid)
                        },
                        "iges structure list validation",
                    )?
                }
            }
            31 => {
                entry.status.is_physically_dependent()
                    && owners.len() == 1
                    && ctx
                        .get_btree_map(
                            entries,
                            &owners[0],
                            "iges property owner directory lookup",
                        )?
                        .is_some_and(|owner| dimension_entity_type(owner.entity_type))
            }
            32 => {
                !owners.is_empty()
                    && ctx.all_by(
                        owners.iter(),
                        |owner| {
                            Ok(ctx
                                .get_btree_map(
                                    entries,
                                    owner,
                                    "iges property owner directory lookup",
                                )?
                                .is_some_and(|owner| owner.entity_type == 404))
                        },
                        "iges structure list validation",
                    )?
            }
            33 => {
                let identity = record.integer(2).zip(record.string(3));
                if sheet_identities.is_none() {
                    let mut identity_storage;
                    let (mut identities, result_identity_storage) =
                        ctx.temporary_vec(0, "iges sheet identity index inputs")?;
                    identity_storage = result_identity_storage;
                    for candidate in
                        ctx.admit_iter(directory, "iges sheet identity directory")?
                    {
                        if candidate.entity_type != 406 || candidate.form != 33 {
                            continue;
                        }
                        let Some(identity_record) = ctx.get_btree_map(
                            records,
                            &candidate.sequence,
                            "iges sheet identity record lookup",
                        )? else {
                            continue;
                        };
                        if let Some(identity) =
                            identity_record.integer(2).zip(identity_record.string(3))
                        {
                            ctx.push_scoped_vec(
                                &mut identity_storage,
                                &mut identities,
                                (identity, candidate.sequence),
                                "iges sheet identity index inputs",
                            )?;
                        }
                    }
                    sheet_identities = Some(ctx.unique_index(
                        ctx.admit_iter(&identities, "iges sheet identity index inputs")?
                            .copied(),
                        "iges sheet identity index",
                    )?);
                }
                let unique_identity = match identity {
                    Some(identity) => match &sheet_identities {
                        Some((identities, _)) => ctx
                            .get_hash_map(identities, &identity, "iges sheet identity uniqueness")?
                            .and_then(Option::as_ref)
                            .is_some(),
                        None => false,
                    },
                    None => false,
                };
                let sole_sheet_id = match owners.first() {
                    Some(owner) => match ctx.get_btree_map(
                        records,
                        owner,
                        "iges property owner record lookup",
                    )? {
                        Some(owner_record) => match ctx
                            .get_btree_map(
                                trailing_pointer_analysis,
                                &owner_record.directory_sequence,
                                "iges trailing pointer analysis lookup",
                            )?
                            .and_then(|analysis| match analysis {
                                TrailingPointerAnalysis::Unambiguous(groups) => Some(groups),
                                _ => None,
                            }) {
                            Some(groups) => {
                                let mut sheet_ids = 0_usize;
                                for sequence in ctx.admit_iter(
                                    groups.properties(),
                                    "iges sheet owner property references",
                                )? {
                                    if ctx
                                        .get_btree_map(
                                            entries,
                                            sequence,
                                            "iges property reference lookup",
                                        )?
                                        .is_some_and(|property| {
                                            property.entity_type == 406 && property.form == 33
                                        })
                                    {
                                        sheet_ids += 1;
                                    }
                                }
                                sheet_ids == 1
                            }
                            None => false,
                        },
                        None => false,
                    },
                    None => false,
                };
                owners.len() == 1
                    && ctx
                        .get_btree_map(
                            entries,
                            &owners[0],
                            "iges property owner directory lookup",
                        )?
                        .is_some_and(|owner| owner.entity_type == 404)
                    && unique_identity
                    && sole_sheet_id
            }
            34 => {
                owners.len() == 1
                    && ctx
                        .get_btree_map(
                            entries,
                            &owners[0],
                            "iges property owner directory lookup",
                        )?
                        .is_some_and(|owner| owner.entity_type == 212)
            }
            35 => {
                owners.len() == 1
                    && ctx
                        .get_btree_map(
                            entries,
                            &owners[0],
                            "iges property owner directory lookup",
                        )?
                        .is_some_and(|owner| matches!(owner.entity_type, 212 | 312))
            }
            36 => {
                let arity = record
                    .integer(1)
                    .and_then(|value| usize::try_from(value).ok());
                !owners.is_empty()
                    && ctx.all_by(
                        owners.iter(),
                        |owner| {
                            Ok(ctx
                                .get_btree_map(
                                    entries,
                                    owner,
                                    "iges property owner directory lookup",
                                )?
                                .and_then(|owner| closure_owner_dimension(owner.entity_type))
                                == arity)
                        },
                        "iges structure list validation",
                    )?
            }
            _ => true,
        };
        if fields_valid && attachment_valid && reference_designator_valid && owner_kind_valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "property value layout, attachment, or owner kind is invalid"
                ),
            )?;
        }
    }

    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 322 && matches!(entry.form, 0..=2))
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let (definition_valid, shape) = attribute_definition_valid_and_shape(
            entry,
            record,
            entries,
            global.global_table(),
            ctx,
        )?;
        if definition_valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
            if entry.form == 0 {
                attribute_shape_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut attribute_shapes,
                        entry.sequence,
                        shape,
                        "iges attribute shape index nodes",
                    )
                })?;
            }
        } else {
            drop(shape);
            super::push_entity_loss_with_scoped_slots(ctx, &mut loss_slots_storage, &mut losses, entry, format_args!("{}", "attribute-table definition header, value type, value, or display link is invalid"))?;
        }
    }

    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 422 && matches!(entry.form, 0..=1))
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let definition = entry
            .structure
            .checked_neg()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|sequence| sequence % 2 == 1);
        let shape = match definition {
            Some(sequence) => ctx.get_btree_map(
                &attribute_shapes,
                &sequence,
                "iges attribute shape lookup",
            )?,
            None => None,
        };
        let declared_row_count = if entry.form == 0 {
            Some(1)
        } else {
            record.count(1).filter(|count| *count > 0)
        };
        let value_start = if entry.form == 0 { 1 } else { 2 };
        let mut values_per_row = shape.map(|_| 0_usize);
        if let Some(shape) = shape {
            let mut descriptors = shape.descriptors.iter();
            while let Some((_, count)) =
                ctx.next_charged(&mut descriptors, "iges attribute row width")?
            {
                values_per_row = values_per_row.and_then(|total| total.checked_add(*count));
                if values_per_row.is_none() {
                    break;
                }
            }
        }
        let row_count = declared_row_count
            .zip(values_per_row)
            .and_then(|(rows, width)| {
                record
                    .parameter_end()
                    .checked_sub(value_start)
                    .and_then(|available| (width == 0 || rows <= available / width).then_some(rows))
            });
        let mut cursor = value_start;
        let mut values_valid = shape.is_some() && row_count.is_some();
        for _ in ctx.admit_iter(
            0..row_count.unwrap_or_default(),
            "iges structure list traversal",
        )? {
            for (data_type, count) in ctx.admit_iter(
                shape.map_or(&[][..], |shape| shape.descriptors.as_slice()),
                "iges attribute shape traversal",
            )? {
                for _ in ctx.admit_iter(0..*count, "iges structure list traversal")? {
                    values_valid &= attribute_value_valid(record, cursor, *data_type, entries, ctx)?;
                    cursor += 1;
                }
            }
        }
        if values_valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "attribute-table instance definition, row count, or typed value is invalid"
                ),
            )?;
        }
    }
    drop(attribute_shapes);
    drop(attribute_shape_storage);

    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 316 && entry.form == 0)
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let units_valid = unit_values_valid(record, ctx)?;
        let directory_valid = entry.status.subordinate() == Some(Subordinate::Independent)
            && entry.status.use_flag(global.global_table()) == Some(UseFlag::Definition);
        if units_valid && directory_valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            super::push_entity_loss_with_scoped_slots(ctx, &mut loss_slots_storage, &mut losses, entry, format_args!("{}", "units count, type/value pair, scale factor, uniqueness, or Directory fields are invalid"))?;
        }
    }

    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 302)
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let class_count = record.count(1).filter(|count| *count > 0);
        let mut cursor = 2;
        let mut classes_valid = class_count.is_some();
        for _ in ctx.admit_iter(
            0..class_count.unwrap_or_default(),
            "iges structure list traversal",
        )? {
            classes_valid &= record
                .integer(cursor)
                .is_some_and(|value| matches!(value, 1..=2));
            classes_valid &= record
                .integer(cursor + 1)
                .is_some_and(|value| matches!(value, 1..=2));
            let item_count = record.count(cursor + 2).filter(|count| *count > 0);
            cursor += 3;
            for _ in ctx.admit_iter(
                0..item_count.unwrap_or_default(),
                "iges structure list traversal",
            )? {
                classes_valid &= record
                    .integer(cursor)
                    .is_some_and(|value| matches!(value, 1..=3));
                cursor += 1;
            }
            classes_valid &= item_count.is_some();
        }
        let directory_valid = matches!(entry.form, 5001..=9999)
            && entry.status.subordinate() == Some(Subordinate::Independent)
            && entry.status.use_flag(global.global_table()) == Some(UseFlag::Definition);
        if directory_valid && classes_valid && cursor == record.parameter_end() {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            super::push_entity_loss_with_scoped_slots(ctx, &mut loss_slots_storage, &mut losses, entry, format_args!("{}", "associativity form, class count, class flags, item layout, or Directory fields are invalid"))?;
        }
    }

    let mut plane_index = None;
    let mut plane_proofs = PlaneBoundaryProofs {
        proven: BTreeMap::new(),
        storage: ctx.reserve_scoped(0, "iges plane boundary proof cache")?,
    };
    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 1 | 7 | 14 | 15))
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let count = record.count(1).filter(|count| *count > 0);
        let members_valid = count
            .map(|count| -> Result<bool, CodecError> {
                ctx.all_by(
                0..count,
                |index| {
                    let Some((sequence, _)) = existing_pointer(record, 2 + index, entries, ctx)?
                    else {
                        return Ok(false);
                    };
                    if !matches!(entry.form, 1 | 14) {
                        return Ok(true);
                    }
                    let Some(member_record) = ctx.get_btree_map(
                        records,
                        &sequence,
                        "iges group member record lookup",
                    )? else {
                        return Ok(false);
                    };
                    has_association_back_pointer(
                        member_record,
                        entry.sequence,
                        &association_owners,
                        ctx,
                    )
                },
                    "iges structure list validation",
                )
            })
            .transpose()?
            .unwrap_or(false);
        if members_valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "group member list or required association back pointer is invalid"
                ),
            )?;
        }
    }

    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| {
            entry.entity_type == 402
                && matches!(entry.form, 2 | 5 | 6 | 8 | 9 | 10 | 11 | 12 | 13 | 16 | 21)
        })
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let valid = type402_structure_valid(entry, global.global_table())
            && if matches!(entry.form, 8 | 10 | 11) {
                legacy_associativity_valid(
                    entry,
                    record,
                    entries,
                    records,
                    (trailing_pointer_analysis, &association_owners),
                    global.global_table(),
                    ctx,
                )?
            } else {
                predefined_associativity_valid(
                    entry,
                    record,
                    entries,
                    records,
                    &association_owners,
                    ctx,
                )?
            };
        if valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
            let legacy_parent = if entry.form == 9 {
                existing_pointer(record, 3, entries, ctx)?
                    .filter(|(_, parent)| parent.entity_type == 108 && parent.form == 1)
            } else {
                None
            };
            if let Some(parent) = legacy_parent {
                let index = match &mut plane_index {
                    Some(index) => index,
                    slot @ None => {
                        slot.insert(ModelIndex::new_model_only(ir, ctx).map_err(CodecError::from)?)
                    }
                };
                match legacy_single_parent_face(
                    (index, &mut plane_proofs),
                    LegacyPlaneSource { entry, record },
                    parent,
                    entries,
                    records,
                    global,
                    ctx,
                    sequences,
                ) {
                    Ok(Some(result)) => {
                        let _plane_storage;
                        let (candidate, plane_sequences, result_plane_storage) = result;
                        _plane_storage = result_plane_storage;
                        for sequence in
                            ctx.admit_iter(plane_sequences, "iges structure list traversal")?
                        {
                            scratch.with_storage(|| {
                                ctx.insert_btree_set(
                                    &mut legacy_plane_sequences,
                                    sequence,
                                    "iges legacy plane sequence nodes",
                                )
                            })?;
                        }
                        scratch.with_storage(|| {
                            ctx.reserve_vec(
                                &mut legacy_face_candidates,
                                1,
                                "iges legacy face candidates",
                            )
                        })?;
                        legacy_face_candidates.push((entry, candidate));
                    }
                    Ok(None) => {}
                    Err(reason) => super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", reason.non_resource()?),
                    )?,
                }
            }
        } else {
            super::push_entity_loss_with_scoped_slots(ctx, &mut loss_slots_storage, &mut losses, entry, format_args!("{}", "predefined associativity counts, class layout, links, back pointers, or structure are invalid"))?;
        }
    }

    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 108 && matches!(entry.form, -1 | 1))
    {
        if ctx.contains_btree_set(
            &legacy_plane_sequences,
            &entry.sequence,
            "iges legacy plane sequence lookup",
        )? {
            continue;
        }
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            continue;
        };
        let index = match &mut plane_index {
            Some(index) => index,
            slot @ None => {
                slot.insert(ModelIndex::new_model_only(ir, ctx).map_err(CodecError::from)?)
            }
        };
        let Some(plane) = plane_carrier(index, entry.sequence, ctx)? else {
            continue;
        };
        let Some((boundary_sequence, _)) = existing_pointer(record, 5, entries, ctx)? else {
            continue;
        };
        match entry.form {
            1 => match plane_boundary_edge(
                index,
                plane,
                boundary_sequence,
                entries,
                global.minimum_resolution_mm(),
                ctx,
                &mut plane_proofs,
            ) {
                Ok(edge) => {
                    let edge_id = crate::ids::edge_admitted(
                        &crate::ids::Stem::word_directory(
                            crate::ids::Word::BoundedPlane,
                            entry.sequence,
                        ),
                        ctx,
                    )?;
                    let edge = closed_plane_boundary_edge(edge, edge_id, ctx)?;
                    let stem = crate::ids::Stem::word_directory(
                        crate::ids::Word::BoundedPlane,
                        entry.sequence,
                    );
                    let _edge_storage;
                    let (mut boundary_edges, result_edge_storage) =
                        ctx.temporary_vec(1, "iges bounded plane boundary edges")?;
                    _edge_storage = result_edge_storage;
                    boundary_edges.push(edge);
                    let candidate = plane_face_draft(
                        entry.sequence,
                        entry.sequence,
                        &stem,
                        boundary_edges,
                        global.minimum_resolution_mm(),
                        sequences,
                        ctx,
                    );
                    match candidate {
                        Ok(candidate) => {
                            scratch.with_storage(|| {
                                ctx.reserve_vec(
                                    &mut legacy_face_candidates,
                                    1,
                                    "iges legacy face candidates",
                                )
                            })?;
                            legacy_face_candidates.push((entry, candidate));
                        }
                        Err(reason) => super::push_entity_loss_with_scoped_slots(
                            ctx,
                            &mut loss_slots_storage,
                            &mut losses,
                            entry,
                            format_args!("{}", reason.non_resource()?),
                        )?,
                    }
                }
                Err(reason) => super::push_entity_loss_with_scoped_slots(
                    ctx,
                    &mut loss_slots_storage,
                    &mut losses,
                    entry,
                    format_args!("{}", reason.message()?),
                )?,
            },
            -1 => match plane_boundary_edge(
                index,
                plane,
                boundary_sequence,
                entries,
                global.minimum_resolution_mm(),
                ctx,
                &mut plane_proofs,
            ) {
                Ok(_) => super::push_entity_loss_with_scoped_slots(
                    ctx,
                    &mut loss_slots_storage,
                    &mut losses,
                    entry,
                    format_args!(
                        "negative bounded plane requires an enclosing positive plane face"
                    ),
                )?,
                Err(reason) => super::push_entity_loss_with_scoped_slots(
                    ctx,
                    &mut loss_slots_storage,
                    &mut losses,
                    entry,
                    format_args!("{}", reason.message()?),
                )?,
            },
            _ => {}
        }
    }

    drop(plane_proofs);
    drop(plane_index);
    let mut commit_session = CommitSession::new(ir, ctx, None)?;
    for (entry, candidate) in
        ctx.admit_iter(legacy_face_candidates, "iges structure list traversal")?
    {
        if commit_session.commit_model(candidate)?.is_err() {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "legacy single-parent plane hole failed neutral topology validation"
                ),
            )?;
        }
    }

    let mut flow_graph = BTreeMap::new();
    for (sequence, flow) in ctx.admit_iter(&flows, "iges flow graph definitions")? {
        let targets = scratch.with_storage(|| -> Result<Vec<u32>, CodecError> {
            let mut targets = Vec::new();
            let mut input = flow.continuations.iter();
            while let Some(slot) =
                ctx.next_charged(&mut input, "iges flow graph continuation slots")?
            {
                let Some(target) = slot else {
                    continue;
                };
                if ctx.contains_key_btree_map(
                    &flows,
                    target,
                    "iges flow graph target lookup",
                )? {
                    ctx.push_vec(&mut targets, *target, "iges flow graph targets")?;
                }
            }
            Ok(targets)
        })?;
        scratch.with_storage(|| {
            ctx.insert_btree_map(&mut flow_graph, *sequence, targets, "iges flow graph nodes")
        })?;
    }
    let mut visited_flows = BTreeSet::new();
    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 18 | 20))
    {
        let flow = ctx.get_btree_map(
            &flows,
            &entry.sequence,
            "iges flow definition lookup",
        )?;
        let flow_targets_valid = flow
            .map(|flow| -> Result<bool, CodecError> {
                Ok(flow.form == entry.form
                    && ctx.all_by(
                        flow.associated.iter(),
                        |target| {
                            Ok(ctx
                                .get_btree_map(
                                    &flows,
                                    target,
                                    "iges flow associated definition lookup",
                                )?
                                .is_some_and(|target_flow| target_flow.form == flow.form))
                        },
                        "iges structure list validation",
                    )?
                    && ctx.all_by(
                        &flow.continuations,
                        |slot| {
                            let Some(target) = slot else { return Ok(true) };
                            let Some(target_entry) = ctx.get_btree_map(
                                entries,
                                target,
                                "iges flow continuation directory lookup",
                            )? else {
                                return Ok(false);
                            };
                            Ok((flow.form == 18 && target_entry.form == 11)
                                || ctx
                                    .get_btree_map(
                                        &flows,
                                        target,
                                        "iges flow continuation definition lookup",
                                    )?
                                    .is_some_and(|target_flow| target_flow.form == flow.form))
                        },
                        "iges structure list validation",
                    )?)
            })
            .transpose()?
            .unwrap_or(false);
        let cyclic = scratch.with_storage(|| {
            super::directed_cycle(entry.sequence, &mut visited_flows, ctx, |sequence| {
                let adjacent = ctx.get_btree_map(
                    &flow_graph,
                    &sequence,
                    "iges flow graph successor lookup",
                )?;
                Ok(adjacent.into_iter().flatten().copied())
            })
        })?;
        if flow_targets_valid && !cyclic {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            super::push_entity_loss_with_scoped_slots(ctx, &mut loss_slots_storage, &mut losses, entry, format_args!("{}", "flow class counts, flags, typed links, required back pointers, continuation tree, or directory status is invalid"))?;
        }
    }

    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 416 && matches!(entry.form, 0..=4))
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let nonempty_string = |index| record.string(index).is_some_and(|value| !value.is_empty());
        let fields_valid = match entry.form {
            0 | 2 | 4 => nonempty_string(1) && nonempty_string(2),
            1 | 3 => nonempty_string(1),
            _ => false,
        };
        if fields_valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "external-reference file, symbolic, or library identifier is empty or invalid"
                ),
            )?;
        }
    }

    let mut array_targets = BTreeMap::new();
    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| matches!(entry.entity_type, 412 | 414) && entry.form == 0)
    {
        if let Some(target) = ctx
            .get_btree_map(
                records,
                &entry.sequence,
                "iges array parameter record lookup",
            )?
            .and_then(|record| record.integer(1))
            .and_then(|value| u32::try_from(value).ok())
        {
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut array_targets,
                    entry.sequence,
                    target,
                    "iges array target index nodes",
                )
            })?;
        }
    }
    let mut visited_arrays = BTreeSet::new();
    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| matches!(entry.entity_type, 412 | 414) && entry.form == 0)
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let target_valid = match ctx.get_btree_map(
            &array_targets,
            &entry.sequence,
            "iges array target lookup",
        )? {
            Some(target) if target % 2 == 1 => ctx
                .get_btree_map(entries, target, "iges array base directory lookup")?
                .is_some_and(|base| array_base_type(base.entity_type, base.form)),
            _ => false,
        };
        let cyclic = scratch.with_storage(|| {
            single_target_cycle(entry.sequence, &array_targets, &mut visited_arrays, ctx)
        })?;
        let transform_valid =
            subfigure_definition_transform_valid(entry, entries, records, global, ctx)?;
        let fields_valid = if entry.entity_type == 412 {
            let scale_valid = record
                .number_or(2, 1.0)
                .is_some_and(|value| value.is_finite() && value > 0.0);
            let coordinates_valid = (3..=5).all(|index| record.number(index).is_some());
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
                && (8..=10).all(|index| record.number(index).is_some())
                && match dimensions {
                    Some(total) => array_mask_valid(record, 11, 12, 13, total, ctx)?,
                    None => false,
                }
        } else {
            let locations = record
                .integer(2)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|count| *count > 0);
            (3..=5).all(|index| record.number(index).is_some())
                && record
                    .number(6)
                    .is_some_and(|value| value.is_finite() && value > 0.0)
                && (7..=8).all(|index| record.number(index).is_some())
                && match locations {
                    Some(total) => array_mask_valid(record, 9, 10, 11, total, ctx)?,
                    None => false,
                }
        };
        if target_valid && !cyclic && transform_valid && fields_valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "array base, dimensions, selection mask, transform, or acyclicity is invalid"
                ),
            )?;
        }
    }

    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 132 && entry.form == 0)
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let position_valid = (1..=3).all(|index| record.number(index).is_some());
        let optional_pointer_valid =
            |index: usize, entity_type: Option<i64>| -> Result<bool, CodecError> {
                let Some(value) = record.integer_or(index, 0) else {
                    return Ok(false);
                };
                if value == 0 {
                    return Ok(true);
                }
                let Some(sequence) = u32::try_from(value)
                    .ok()
                    .filter(|sequence| sequence % 2 == 1)
                else {
                    return Ok(false);
                };
                Ok(ctx
                    .get_btree_map(
                        entries,
                        &sequence,
                        "iges connect point directory lookup",
                    )?
                    .is_some_and(|target| {
                        entity_type.is_none_or(|expected| target.entity_type == expected)
                    }))
        };
        let type_flag_valid =
            record
                .integer_or(5, 0)
                .is_some_and(|value| match global.global_table() {
                    GlobalTable::V4_0 => matches!(value, 0..=2),
                    _ => matches!(value, 0..=2 | 101..=104 | 201..=203 | 5001..=9999),
                });
        let function_flag_valid = record
            .integer_or(6, 0)
            .is_some_and(|value| matches!(value, 0..=2));
        let strings_valid = record.string(7).is_some() && record.string(9).is_some();
        let identifier_valid = record.integer(11).is_some();
        let function_code_valid = record
            .integer_or(12, 0)
            .is_some_and(|value| connect_point_function_code_valid(value, global.global_table()));
        let swap_valid = record
            .integer_or(13, 0)
            .is_some_and(|value| matches!(value, 0..=1));
        let owner_valid = match record.integer_or(14, 0) {
            Some(0) => true,
            Some(value) => match u32::try_from(value)
                .ok()
                .filter(|sequence| sequence % 2 == 1)
            {
                Some(sequence) => ctx
                    .get_btree_map(
                        entries,
                        &sequence,
                        "iges connect point owner directory lookup",
                    )?
                    .is_some_and(|target| matches!(target.entity_type, 320 | 420)),
                None => false,
            },
            None => false,
        };
        let transform_valid =
            subfigure_definition_transform_valid(entry, entries, records, global, ctx)?;
        if position_valid
            && optional_pointer_valid(4, None)?
            && type_flag_valid
            && function_flag_valid
            && strings_valid
            && optional_pointer_valid(8, Some(312))?
            && optional_pointer_valid(10, Some(312))?
            && identifier_valid
            && function_code_valid
            && swap_valid
            && owner_valid
            && transform_valid
            && entry.status.use_flag(global.global_table()) == Some(UseFlag::LogicalPositional)
        {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "connect-point fields, references, placement, or use flag are invalid"
                ),
            )?;
        }
    }

    let mut solid_instances = BTreeMap::new();
    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 430 && matches!(entry.form, 0 | 1))
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let target = record.integer(1).and_then(|value| {
            let sequence = u32::try_from(value).ok()?;
            (sequence % 2 == 1).then_some(sequence)
        });
        if let Some(target) = target {
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut solid_instances,
                    entry.sequence,
                    target,
                    "iges solid instance index nodes",
                )
            })?;
        } else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "solid-instance target pointer is invalid"),
            )?;
        }
    }

    let mut visited_instances = BTreeSet::new();
    for (sequence, target) in ctx.admit_iter(&solid_instances, "iges structure list traversal")? {
        let entry = ctx
            .get_btree_map(entries, sequence, "iges solid instance directory lookup")?
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "solid instance index refers to a missing directory entry"
                ))
            })?;
        let target_valid = ctx
            .get_btree_map(entries, target, "iges solid instance target lookup")?
            .is_some_and(|target_entry| {
            if entry.form == 1 {
                target_entry.entity_type == 186
            } else {
                matches!(
                    target_entry.entity_type,
                    150 | 152 | 154 | 156 | 158 | 160 | 162 | 164 | 168 | 180 | 184 | 430
                )
            }
            });
        let transform_valid =
            subfigure_definition_transform_valid(entry, entries, records, global, ctx)?;
        let cyclic = scratch.with_storage(|| {
            single_target_cycle(*sequence, &solid_instances, &mut visited_instances, ctx)
        })?;
        if target_valid && transform_valid && !cyclic {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                *sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "solid-instance form, target, transform, or acyclicity is invalid"
                ),
            )?;
        }
    }

    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 184 && matches!(entry.form, 0 | 1))
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(count) = record.count(1).filter(|count| *count > 0) else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "solid-assembly item count is not positive"),
            )?;
            continue;
        };
        let mut items =
            scratch.with_storage(|| ctx.collection_vec(count, "iges solid assembly items"))?;
        let mut items_valid = true;
        let mut input = 0..count;
        while let Some(index) = ctx.next_charged(&mut input, "iges structure list traversal")? {
            let Some(item) = (|| {
                let item = record.integer(2 + index).and_then(|value| {
                    let sequence = u32::try_from(value).ok()?;
                    (sequence % 2 == 1).then_some(sequence)
                })?;
                let transformation = record
                    .integer(2 + count + index)
                    .and_then(|value| u32::try_from(value).ok())?;
                (transformation == 0 || transformation % 2 == 1).then_some((item, transformation))
            })() else {
                items_valid = false;
                break;
            };
            items.push(item);
        }
        if !items_valid {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "solid-assembly item tuple is invalid"),
            )?;
            continue;
        }
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut assemblies,
                entry.sequence,
                SolidAssembly {
                    form: entry.form,
                    items,
                },
                "iges solid assembly index nodes",
            )
        })?;
    }

    let mut assembly_graph = BTreeMap::new();
    for (sequence, definition) in ctx.admit_iter(&assemblies, "iges assembly graph definitions")? {
        let targets = scratch.with_storage(|| -> Result<Vec<u32>, CodecError> {
            let mut targets = Vec::new();
            let mut input = definition.items.iter();
            while let Some((item, _)) = ctx.next_charged(&mut input, "iges assembly graph items")? {
                if ctx.contains_key_btree_map(
                    &assemblies,
                    item,
                    "iges assembly graph target lookup",
                )? {
                    ctx.push_vec(&mut targets, *item, "iges assembly graph targets")?;
                }
            }
            Ok(targets)
        })?;
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut assembly_graph,
                *sequence,
                targets,
                "iges assembly graph nodes",
            )
        })?;
    }
    let mut visited = BTreeSet::new();
    let mut input = assemblies.iter();
    while let Some((sequence, assembly)) =
        ctx.next_charged(&mut input, "iges structure list traversal")?
    {
        let entry = ctx
            .get_btree_map(entries, sequence, "iges assembly directory lookup")?
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "assembly index refers to a missing directory entry"
                ))
            })?;
        let mut has_brep = false;
        let mut items_valid = true;
        let mut input = assembly.items.iter();
        while let Some((item, transformation)) =
            ctx.next_charged(&mut input, "iges structure list traversal")?
        {
            let item_entry = ctx.get_btree_map(
                entries,
                item,
                "iges assembly item directory lookup",
            )?;
            has_brep |= item_entry.is_some_and(|target| target.entity_type == 186);
            let item_valid = item_entry.is_some_and(|target| {
                matches!(
                    target.entity_type,
                    150 | 152 | 154 | 156 | 158 | 160 | 162 | 164 | 168 | 180 | 184 | 430
                ) || (assembly.form == 1 && target.entity_type == 186)
            });
            let transform_valid = if *transformation == 0 {
                true
            } else if ctx
                .get_btree_map(
                    entries,
                    transformation,
                    "iges assembly transformation directory lookup",
                )?
                .is_some_and(|target| target.entity_type == 124)
            {
                match resolve_transform(
                    i64::from(*transformation),
                    entries,
                    records,
                    global.length_factor_mm(),
                    global.real_precision(),
                    &mut BTreeSet::new(),
                    ctx,
                ) {
                    Ok(_) => true,
                    Err(error) => {
                        error.non_resource()?;
                        false
                    }
                }
            } else {
                false
            };
            if !(item_valid && transform_valid) {
                items_valid = false;
                break;
            }
        }
        let cyclic = scratch.with_storage(|| {
            super::directed_cycle(*sequence, &mut visited, ctx, |sequence| {
                let adjacent = ctx.get_btree_map(
                    &assembly_graph,
                    &sequence,
                    "iges assembly graph successor lookup",
                )?;
                Ok(adjacent.into_iter().flatten().copied())
            })
        })?;
        let own_transform_valid =
            subfigure_definition_transform_valid(entry, entries, records, global, ctx)?;
        if entry.status.use_flag(global.global_table()) != Some(UseFlag::Definition)
            || (assembly.form == 1) != has_brep
            || !items_valid
            || cyclic
            || !own_transform_valid
        {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "solid-assembly use flag, form, members, transforms, or acyclicity is invalid"
                ),
            )?;
            continue;
        }
        ctx.insert_scoped_btree_set(
            &mut decoded_storage,
            &mut decoded,
            *sequence,
            "iges structure decoded sequences",
            "iges structure decoded sequences",
        )?;
    }

    let mut definitions = BTreeMap::new();
    let mut definition_fields_valid = BTreeSet::new();
    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 308 && entry.form == 0)
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let depth = record
            .integer(1)
            .and_then(|value| usize::try_from(value).ok());
        let name_valid = record.string(2).is_some_and(|name| !name.is_empty());
        let count = record.count(3);
        let members = match count {
            Some(count) => scratch.with_storage(|| {
                definition_members(
                    record,
                    count,
                    entries,
                    ctx,
                    "iges subfigure definition members",
                )
            })?,
            None => None,
        };
        let (Some(depth), Some(members)) = (depth, members) else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "subfigure depth, member count, or member pointer is invalid"
                ),
            )?;
            continue;
        };
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut definitions,
                entry.sequence,
                SubfigureDefinition { depth, members },
                "iges subfigure definition index nodes",
            )
        })?;
        if name_valid
            && subfigure_definition_directory_fields_valid(entry, global.global_table())
            && subfigure_definition_label_display_valid(entry, entries, ctx)?
            && subfigure_definition_transform_valid(entry, entries, records, global, ctx)?
        {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut definition_fields_valid,
                    entry.sequence,
                    "iges subfigure valid-definition nodes",
                )
            })?;
        }
    }

    let mut instances = BTreeMap::new();
    let mut instance_fields_valid = BTreeSet::new();
    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 408 && entry.form == 0)
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            ctx.insert_btree_map(
                &mut placement_rejections,
                entry.sequence,
                PlacementRejection::MissingRecord,
                "iges placement rejection nodes",
            )?;
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let definition = match record
            .integer(1)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|sequence| sequence % 2 == 1)
        {
            Some(sequence)
                if ctx.contains_key_btree_map(
                    &definitions,
                    &sequence,
                    "iges subfigure instance definition lookup",
                )? =>
            {
                Some(sequence)
            }
            _ => None,
        };
        let placement_valid = match placement_affine(
            entry,
            record,
            entries,
            records,
            global.length_factor_mm(),
            global.real_precision(),
            ctx,
        ) {
            Ok(_) => true,
            Err(error) => {
                error.non_resource()?;
                false
            }
        };
        if !placement_valid {
            ctx.insert_btree_map(
                &mut placement_rejections,
                entry.sequence,
                PlacementRejection::InvalidPlacement,
                "iges placement rejection nodes",
            )?;
        }
        let Some(definition) = definition else {
            if !ctx.contains_key_btree_map(
                &placement_rejections,
                &entry.sequence,
                "iges placement rejection lookup",
            )? {
                ctx.insert_btree_map(
                    &mut placement_rejections,
                    entry.sequence,
                    PlacementRejection::InvalidDefinition,
                    "iges placement rejection nodes",
                )?;
            }
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "subfigure-instance definition pointer is invalid"),
            )?;
            continue;
        };
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut instances,
                entry.sequence,
                definition,
                "iges subfigure instance nodes",
            )
        })?;
        if placement_valid {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut instance_fields_valid,
                    entry.sequence,
                    "iges valid subfigure instance nodes",
                )
            })?;
        }
    }

    let mut network_definitions = BTreeMap::new();
    let mut network_definition_fields_valid = BTreeSet::new();
    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 320 && entry.form == 0)
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let depth = record
            .integer(1)
            .and_then(|value| usize::try_from(value).ok());
        let name_valid = record.string(2).is_some_and(|name| !name.is_empty());
        let member_count = record.count(3);
        let members = match member_count {
            Some(count) => scratch.with_storage(|| {
                definition_members(
                    record,
                    count,
                    entries,
                    ctx,
                    "iges network definition members",
                )
            })?,
            None => None,
        };
        let Some((depth, member_count, members)) = depth
            .zip(member_count)
            .zip(members)
            .map(|((depth, member_count), members)| (depth, member_count, members))
        else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "network definition header or member list is invalid"),
            )?;
            continue;
        };
        let type_flag_valid = record
            .integer(4 + member_count)
            .is_some_and(|value| matches!(value, 0..=2));
        let designator_valid = record.string_or_empty(5 + member_count).is_some();
        let display_valid = match record.integer_or(6 + member_count, 0) {
            Some(0) => true,
            Some(value) => match u32::try_from(value).ok() {
                Some(sequence) => ctx
                    .get_btree_map(
                        entries,
                        &sequence,
                        "iges network definition display lookup",
                    )?
                    .is_some_and(|target| target.entity_type == 312),
                None => false,
            },
            None => false,
        };
        let Some(connect_points) = scratch.with_storage(|| {
            network_connect_points(
                record,
                7 + member_count,
                8 + member_count,
                entries,
                global.global_table(),
                ctx,
                "iges network definition connect points",
            )
        })?
        else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "network definition connect-point count is invalid"),
            )?;
            continue;
        };
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut network_definitions,
                entry.sequence,
                NetworkDefinition {
                    depth,
                    members,
                    connect_points,
                },
                "iges network definition index nodes",
            )
        })?;
        if name_valid
            && type_flag_valid
            && designator_valid
            && display_valid
            && subfigure_definition_directory_fields_valid(entry, global.global_table())
            && subfigure_definition_label_display_valid(entry, entries, ctx)?
            && subfigure_definition_transform_valid(entry, entries, records, global, ctx)?
        {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut network_definition_fields_valid,
                    entry.sequence,
                    "iges network valid-definition nodes",
                )
            })?;
        }
    }

    let mut network_instances = BTreeMap::new();
    let mut network_instance_fields_valid = BTreeSet::new();
    for entry in ctx
        .admit_iter(directory, "iges structure directory traversal")?
        .filter(|entry| entry.entity_type == 420 && entry.form == 0)
    {
        let Some(record) = ctx.get_btree_map(records, &entry.sequence, "iges structure parameter record lookup")?.copied() else {
            ctx.insert_btree_map(
                &mut placement_rejections,
                entry.sequence,
                PlacementRejection::MissingRecord,
                "iges placement rejection nodes",
            )?;
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let definition = match record
            .integer(1)
            .and_then(|value| u32::try_from(value).ok())
        {
            Some(sequence)
                if ctx.contains_key_btree_map(
                    &network_definitions,
                    &sequence,
                    "iges network instance definition lookup",
                )? =>
            {
                Some(sequence)
            }
            _ => None,
        };
        let type_flag_valid = record
            .integer_or(8, 0)
            .is_some_and(|value| matches!(value, 0..=2));
        let designator_valid = record.string_or_empty(9).is_some();
        let display_valid = match record.integer_or(10, 0) {
            Some(0) => true,
            Some(value) => match u32::try_from(value).ok() {
                Some(sequence) => ctx
                    .get_btree_map(
                        entries,
                        &sequence,
                        "iges network instance display lookup",
                    )?
                    .is_some_and(|target| target.entity_type == 312),
                None => false,
            },
            None => false,
        };
        let connect_points = scratch.with_storage(|| {
            network_connect_points(
                record,
                11,
                12,
                entries,
                global.global_table(),
                ctx,
                "iges network instance connect points",
            )
        })?;
        let placement_valid = match placement_affine(
            entry,
            record,
            entries,
            records,
            global.length_factor_mm(),
            global.real_precision(),
            ctx,
        ) {
            Ok(_) => true,
            Err(error) => {
                error.non_resource()?;
                false
            }
        };
        if !placement_valid {
            ctx.insert_btree_map(
                &mut placement_rejections,
                entry.sequence,
                PlacementRejection::InvalidPlacement,
                "iges placement rejection nodes",
            )?;
        }
        let (Some(definition), Some(connect_points)) = (definition, connect_points) else {
            if !ctx.contains_key_btree_map(
                &placement_rejections,
                &entry.sequence,
                "iges placement rejection lookup",
            )? {
                let rejection = match definition {
                    Some(definition) => PlacementRejection::InvalidMetadata { definition },
                    None => PlacementRejection::InvalidDefinition,
                };
                ctx.insert_btree_map(
                    &mut placement_rejections,
                    entry.sequence,
                    rejection,
                    "iges placement rejection nodes",
                )?;
            }
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "network instance definition or count is invalid"),
            )?;
            continue;
        };
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut network_instances,
                entry.sequence,
                NetworkInstance {
                    definition,
                    connect_points,
                },
                "iges network instance nodes",
            )
        })?;
        if placement_valid && type_flag_valid && designator_valid && display_valid {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut network_instance_fields_valid,
                    entry.sequence,
                    "iges valid network instance nodes",
                )
            })?;
        }
    }

    let mut input = definitions.iter();
    while let Some((sequence, definition)) =
        ctx.next_charged(&mut input, "iges structure list traversal")?
    {
        let entry = ctx
            .get_btree_map(entries, sequence, "iges subfigure definition directory lookup")?
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "subfigure definition index refers to a missing directory entry"
                ))
            })?;
        let nesting_valid = ctx.all_by(
            definition.members.iter(),
            |member| {
                let Some(member_entry) =
                    ctx.get_btree_map(entries, member, "iges subfigure member directory lookup")?
                else {
                    return Ok(false);
                };
                if !matches!(member_entry.entity_type, 408 | 420) {
                    return Ok(true);
                }
                Ok(match member_entry.entity_type {
                    408 => match ctx.get_btree_map(
                        &instances,
                        member,
                        "iges subfigure member instance lookup",
                    )? {
                        Some(definition_sequence) => {
                            ctx.contains_btree_set(
                                &instance_fields_valid,
                                member,
                                "iges subfigure instance field lookup",
                            )? && ctx.contains_btree_set(
                                &definition_fields_valid,
                                definition_sequence,
                                "iges subfigure definition field lookup",
                            )? && ctx
                                .get_btree_map(
                                    &definitions,
                                    definition_sequence,
                                    "iges nested subfigure definition lookup",
                                )?
                                .is_some_and(|child| child.depth < definition.depth)
                        }
                        None => false,
                    },
                    420 => match ctx.get_btree_map(
                        &network_instances,
                        member,
                        "iges network member instance lookup",
                    )? {
                        Some(instance) => {
                            ctx.contains_btree_set(
                                &network_instance_fields_valid,
                                member,
                                "iges network instance field lookup",
                            )? && ctx.contains_btree_set(
                                &network_definition_fields_valid,
                                &instance.definition,
                                "iges network definition field lookup",
                            )? && ctx
                                .get_btree_map(
                                    &network_definitions,
                                    &instance.definition,
                                    "iges nested network definition lookup",
                                )?
                                .is_some_and(|child| child.depth < definition.depth)
                        }
                        None => false,
                    },
                    _ => false,
                })
            },
            "iges structure list validation",
        )?;
        if ctx.contains_btree_set(
            &definition_fields_valid,
            sequence,
            "iges subfigure definition field lookup",
        )? && nesting_valid
        {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                *sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "subfigure definition fields or nesting depth is invalid"
                ),
            )?;
        }
    }
    for (sequence, definition_sequence) in
        ctx.admit_iter(&instances, "iges structure list traversal")?
    {
        let entry = ctx
            .get_btree_map(entries, sequence, "iges subfigure instance directory lookup")?
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "subfigure instance index refers to a missing directory entry"
                ))
            })?;
        if ctx.contains_btree_set(
            &instance_fields_valid,
            sequence,
            "iges subfigure instance field lookup",
        )? && ctx.contains_btree_set(
            &definition_fields_valid,
            definition_sequence,
            "iges subfigure definition field lookup",
        )? && ctx.contains_btree_set(
            &decoded,
            definition_sequence,
            "iges decoded subfigure definition lookup",
        )?
        {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                *sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            if !ctx.contains_key_btree_map(
                &placement_rejections,
                sequence,
                "iges placement rejection lookup",
            )? {
                ctx.insert_btree_map(
                    &mut placement_rejections,
                    *sequence,
                    PlacementRejection::InvalidDefinition,
                    "iges placement rejection nodes",
                )?;
            }
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "subfigure-instance placement or decoded definition is invalid"
                ),
            )?;
        }
    }
    let mut input = network_definitions.iter();
    while let Some((sequence, definition)) =
        ctx.next_charged(&mut input, "iges structure list traversal")?
    {
        let entry = ctx
            .get_btree_map(entries, sequence, "iges network definition directory lookup")?
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "network definition index refers to a missing directory entry"
                ))
            })?;
        let nesting_valid = ctx.all_by(
            definition.members.iter(),
            |member| {
                let Some(member_entry) =
                    ctx.get_btree_map(entries, member, "iges network member directory lookup")?
                else {
                    return Ok(false);
                };
                Ok(match member_entry.entity_type {
                    408 => match ctx.get_btree_map(
                        &instances,
                        member,
                        "iges subfigure member instance lookup",
                    )? {
                        Some(definition_sequence) => {
                            ctx.contains_btree_set(
                                &instance_fields_valid,
                                member,
                                "iges subfigure instance field lookup",
                            )? && ctx.contains_btree_set(
                                &definition_fields_valid,
                                definition_sequence,
                                "iges subfigure definition field lookup",
                            )? && ctx
                                .get_btree_map(
                                    &definitions,
                                    definition_sequence,
                                    "iges nested subfigure definition lookup",
                                )?
                                .is_some_and(|child| child.depth < definition.depth)
                        }
                        None => false,
                    },
                    420 => match ctx.get_btree_map(
                        &network_instances,
                        member,
                        "iges network member instance lookup",
                    )? {
                        Some(instance) => {
                            ctx.contains_btree_set(
                                &network_instance_fields_valid,
                                member,
                                "iges network instance field lookup",
                            )? && ctx.contains_btree_set(
                                &network_definition_fields_valid,
                                &instance.definition,
                                "iges network definition field lookup",
                            )? && ctx
                                .get_btree_map(
                                    &network_definitions,
                                    &instance.definition,
                                    "iges nested network definition lookup",
                                )?
                                .is_some_and(|child| child.depth < definition.depth)
                        }
                        None => false,
                    },
                    _ => true,
                })
            },
            "iges structure list validation",
        )?;
        if ctx.contains_btree_set(
            &network_definition_fields_valid,
            sequence,
            "iges network definition field lookup",
        )? && nesting_valid
        {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                *sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "network definition fields or nesting depth is invalid"
                ),
            )?;
        }
    }
    for (sequence, instance) in
        ctx.admit_iter(&network_instances, "iges structure list traversal")?
    {
        let entry = ctx
            .get_btree_map(entries, sequence, "iges network instance directory lookup")?
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "network instance index refers to a missing directory entry"
                ))
            })?;
        let definition_valid = match ctx.get_btree_map(
            &network_definitions,
            &instance.definition,
            "iges network instance definition lookup",
        )? {
            Some(definition) => network_connectivity_valid(
                &definition.connect_points,
                &instance.connect_points,
                global.global_table(),
                ctx,
            )?,
            None => false,
        };
        let instance_fields_valid = ctx.contains_btree_set(
            &network_instance_fields_valid,
            sequence,
            "iges network instance field lookup",
        )?;
        let definition_decoded = if instance_fields_valid && definition_valid {
            Some(ctx.contains_btree_set(
                &decoded,
                &instance.definition,
                "iges decoded network definition lookup",
            )?)
        } else {
            None
        };
        if instance_fields_valid && definition_valid && definition_decoded == Some(true) {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                *sequence,
                "iges structure decoded sequences",
                "iges structure decoded sequences",
            )?;
        } else {
            if !ctx.contains_key_btree_map(
                &placement_rejections,
                sequence,
                "iges placement rejection lookup",
            )? {
                let definition_decoded = match definition_decoded {
                    Some(definition_decoded) => definition_decoded,
                    None => ctx.contains_btree_set(
                        &decoded,
                        &instance.definition,
                        "iges decoded network definition lookup",
                    )?,
                };
                ctx.insert_btree_map(
                    &mut placement_rejections,
                    *sequence,
                    if definition_decoded {
                        PlacementRejection::InvalidMetadata {
                            definition: instance.definition,
                        }
                    } else {
                        PlacementRejection::InvalidDefinition
                    },
                    "iges placement rejection nodes",
                )?;
            }
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "network instance placement or connection list is invalid"
                ),
            )?;
        }
    }

    Ok((ProjectionOutcome { decoded, decoded_storage, losses, loss_slots_storage }, placement_rejections))
}

#[cfg(test)]
mod tests;
