// SPDX-License-Identifier: Apache-2.0
//! Views, drawings, and view-dependent presentation relationships.

use super::geometry::{resolve_transform, ProjectionOutcome};
use super::PropertyTextIndex;

use crate::directory::{DirectoryEntry, Subordinate, UseFlag};
use crate::global::{GlobalTable, ProjectedGlobal};
use crate::loss::IgesLossCode;
use crate::parameter::{ParameterRecord, TrailingPointerAnalysis};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

fn finite_vector(record: &ParameterRecord, start: usize) -> Option<FiniteVector<3>> {
    let values = [
        record.number_or(start, 0.0)?,
        record.number_or(start + 1, 0.0)?,
        record.number_or(start + 2, 0.0)?,
    ];
    FiniteVector::new(values)
}

fn has_in_plane_component(normal: [f64; 3], up: [f64; 3]) -> bool {
    let normal_scale = normal.iter().map(|value| value.abs()).fold(0.0, f64::max);
    let up_scale = up.iter().map(|value| value.abs()).fold(0.0, f64::max);
    if normal_scale == 0.0 || up_scale == 0.0 {
        return false;
    }
    let normal = normal.map(|value| value / normal_scale);
    let up = up.map(|value| value / up_scale);
    let cross = [
        normal[1] * up[2] - normal[2] * up[1],
        normal[2] * up[0] - normal[0] * up[2],
        normal[0] * up[1] - normal[1] * up[0],
    ];
    cross.iter().any(|value| *value != 0.0)
}

fn depth_clipping_valid(value: i64) -> bool {
    matches!(value, 0..=3)
}

fn display_flag_valid(value: i64) -> bool {
    matches!(value, 0..=1)
}

fn standard_line_font_valid(value: i64) -> bool {
    matches!(value, 1..=5)
}

fn standard_color_valid(value: i64) -> bool {
    matches!(value, 0..=8)
}

fn drawing_directory_valid(entry: &DirectoryEntry, global_table: GlobalTable) -> bool {
    entry.entity_type == 404
        && matches!(entry.form, 0 | 1)
        && entry.status.subordinate() == Some(Subordinate::Independent)
        && match global_table {
            GlobalTable::V4_0 | GlobalTable::Legacy => {
                entry.status.use_flag(global_table) != Some(UseFlag::Geometry)
            }
            GlobalTable::V5_0 | GlobalTable::V5Later => {
                entry.status.use_flag(global_table) == Some(UseFlag::Annotation)
                    && entry.structure == 0
                    && entry.line_font == 0
                    && entry.line_weight == 0
                    && entry.color == 0
            }
        }
}

fn view_directory_valid(entry: &DirectoryEntry, global_table: GlobalTable) -> bool {
    entry.entity_type == 410
        && matches!(entry.form, 0 | 1)
        && match global_table {
            GlobalTable::V4_0 | GlobalTable::Legacy => {
                entry.status.use_flag(global_table) != Some(UseFlag::Geometry)
            }
            GlobalTable::V5_0 | GlobalTable::V5Later => {
                entry.status.use_flag(global_table) == Some(UseFlag::Annotation)
            }
        }
}

fn views_visible_directory_valid(entry: &DirectoryEntry, global_table: GlobalTable) -> bool {
    entry.entity_type == 402
        && match global_table {
            GlobalTable::V4_0 => matches!(entry.form, 3 | 4),
            _ => matches!(entry.form, 3 | 4 | 19),
        }
        && entry.status.subordinate() == Some(Subordinate::Independent)
        && (!matches!(global_table, GlobalTable::V5_0 | GlobalTable::V5Later)
            || entry.status.use_flag(global_table) == Some(UseFlag::Annotation))
}

fn clipping_plane_valid(entry: &DirectoryEntry, global_table: GlobalTable) -> bool {
    entry.entity_type == 108
        && match global_table {
            GlobalTable::V4_0 => matches!(
                entry.status.use_flag(global_table),
                Some(
                    UseFlag::Geometry
                        | UseFlag::Annotation
                        | UseFlag::Definition
                        | UseFlag::Parametric
                )
            ),
            _ => entry.status.use_flag(global_table) == Some(UseFlag::Annotation),
        }
}

#[derive(Debug, PartialEq)]
pub(crate) enum DrawingPropertyValue<'a> {
    Name(&'a [u8]),
    Size(FiniteVector<2>),
    Units(i64, &'a [u8]),
}

pub(crate) fn drawing_property_value(
    form: i64,
    record: &ParameterRecord,
) -> Option<DrawingPropertyValue<'_>> {
    match form {
        15 => (record.integer(1) == Some(1))
            .then(|| record.string(2).filter(|value| !value.is_empty()))
            .flatten()
            .map(DrawingPropertyValue::Name),
        16 => {
            if record.integer(1) != Some(2) {
                return None;
            }
            let size = [record.number(2)?, record.number(3)?];
            FiniteVector::new(size).map(DrawingPropertyValue::Size)
        }
        17 => {
            let units = record.integer(2).filter(|value| (1..=11).contains(value))?;
            let name = record.string(3).filter(|value| !value.is_empty())?;
            (record.integer(1) == Some(2)).then_some(DrawingPropertyValue::Units(units, name))
        }
        _ => None,
    }
}

fn conflicting_drawing_property_forms<'text>(
    record: &ParameterRecord,
    form: i64,
    directory: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &'text ParameterRecord>,
    trailing_pointer_analysis: &BTreeMap<u32, TrailingPointerAnalysis>,
    texts: &mut PropertyTextIndex<'text, '_>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(TrailingPointerAnalysis::Unambiguous(groups)) = ctx.get_btree_map(
        trailing_pointer_analysis,
        &record.directory_sequence,
        "iges drawing trailing analysis lookup",
    )?
    else {
        return Ok(false);
    };
    let mut first: Option<(u32, DrawingPropertyValue<'text>)> = None;
    let mut properties = groups.properties().iter();
    while properties.len() != 0 || ctx.resource_refusal().is_some() {
        let Some(sequence) =
            ctx.next_charged(&mut properties, "iges drawing property traversal")?
        else {
            break;
        };
        if ctx
            .get_btree_map(directory, sequence, "iges drawing directory lookup")?
            .is_none_or(|entry| entry.entity_type != 406 || entry.form != form)
        {
            continue;
        }
        let Some(value) = ctx
            .get_btree_map(records, sequence, "iges drawing parameter lookup")?
            .and_then(|&record| drawing_property_value(form, record))
        else {
            continue;
        };
        if let Some((first_sequence, first_value)) = &first {
            let agrees = match (&value, first_value) {
                (DrawingPropertyValue::Name(left), DrawingPropertyValue::Name(right)) => {
                    sequence == first_sequence
                        || (left.len() == right.len()
                            && texts.id(
                                *first_sequence,
                                right,
                                ctx,
                                "iges drawing property name agreement",
                            )? == texts.id(
                                *sequence,
                                left,
                                ctx,
                                "iges drawing property name agreement",
                            )?)
                }
                (DrawingPropertyValue::Size(left), DrawingPropertyValue::Size(right)) => {
                    left == right
                }
                (
                    DrawingPropertyValue::Units(left_unit, left_name),
                    DrawingPropertyValue::Units(right_unit, right_name),
                ) => {
                    left_unit == right_unit
                        && (sequence == first_sequence
                            || (left_name.len() == right_name.len()
                                && texts.id(
                                    *first_sequence,
                                    right_name,
                                    ctx,
                                    "iges drawing unit name agreement",
                                )? == texts.id(
                                    *sequence,
                                    left_name,
                                    ctx,
                                    "iges drawing unit name agreement",
                                )?))
                }
                _ => false,
            };
            if !agrees {
                return Ok(true);
            }
        } else {
            first = Some((*sequence, value));
        }
    }
    Ok(false)
}

fn push_drawing_loss(
    ctx: &DecodeContext<'_>,
    slots: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    entry: &DirectoryEntry,
    code: IgesLossCode,
    message: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    ctx.reserve_scoped_vec(slots, losses, 1, "iges drawing loss slots")?;
    losses.push(super::attributed_loss_payload(
        ctx,
        entry,
        code,
        message,
        "iges drawing loss message",
        "iges drawing loss kind",
    )?);
    Ok(())
}

fn push_drawing_entity_loss(
    ctx: &DecodeContext<'_>,
    slots: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    entry: &DirectoryEntry,
    reason: &str,
) -> Result<(), CodecError> {
    push_drawing_loss(
        ctx,
        slots,
        losses,
        entry,
        IgesLossCode::EntityNotProjected,
        format_args!(
            "IGES entity type {} form {} was not projected: {reason}",
            entry.entity_type, entry.form
        ),
    )
}

pub(super) fn project<'ctx>(
    _ir: &mut CadIr,
    directory: &[DirectoryEntry],
    (entries, records): (
        &BTreeMap<u32, &DirectoryEntry>,
        &BTreeMap<u32, &ParameterRecord>,
    ),
    trailing_pointer_analysis: &BTreeMap<u32, TrailingPointerAnalysis>,
    global: &ProjectedGlobal,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<ProjectionOutcome<'ctx>, CodecError> {
    let mut property_texts = PropertyTextIndex::new(ctx)?;
    let mut decoded_storage = ctx.reserve_scoped(0, "iges drawing decoded sequences")?;
    let mut decoded = BTreeSet::new();
    let mut loss_slots_storage = ctx.reserve_scoped(0, "iges drawing loss slots")?;
    let mut losses = Vec::new();

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges drawing directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 406 && matches!(entry.form, 16 | 17)) {
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(records, &entry.sequence, "iges drawing parameter lookup")?
            .copied()
        else {
            push_drawing_entity_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "Parameter Data record is missing",
            )?;
            continue;
        };
        let valid = if entry.form == 16 {
            record.integer(1) == Some(2) && (2..=3).all(|index| record.number(index).is_some())
        } else {
            record.integer(1) == Some(2)
                && record
                    .integer(2)
                    .is_some_and(|value| matches!(value, 1..=11))
                && record.string(3).is_some_and(|value| !value.is_empty())
        };
        if valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges drawing decoded sequences",
                "iges drawing decoded sequences",
            )?;
        } else {
            push_drawing_entity_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "drawing size or unit property fields are invalid",
            )?;
        }
    }

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges drawing directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 404 && matches!(entry.form, 0 | 1)) {
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(records, &entry.sequence, "iges drawing parameter lookup")?
            .copied()
        else {
            push_drawing_entity_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "Parameter Data record is missing",
            )?;
            continue;
        };
        for form in [15, 16, 17] {
            if conflicting_drawing_property_forms(
                record,
                form,
                entries,
                records,
                trailing_pointer_analysis,
                &mut property_texts,
                ctx,
            )? {
                push_drawing_loss(
                    ctx,
                    &mut loss_slots_storage,
                    &mut losses,
                    entry,
                    IgesLossCode::DrawingPropertyAmbiguous,
                    format_args!(
                        "IGES drawing has conflicting valid Type 406 Form {form} properties"
                    ),
                )?;
            }
        }
        let view_count = record.count(1);
        let width = if entry.form == 0 { 3 } else { 4 };
        let views_valid = view_count
            .map(|count| -> Result<bool, CodecError> {
                ctx.all_by(
                    0..count,
                    |index| {
                        let start = 2 + index * width;
                        Ok(record
                            .integer(start)
                            .and_then(|value| u32::try_from(value).ok())
                            .map(|sequence| {
                                ctx.get_btree_map(
                                    entries,
                                    &sequence,
                                    "iges drawing directory lookup",
                                )
                                .map(Option::<&&DirectoryEntry>::copied)
                            })
                            .transpose()?
                            .flatten()
                            .is_some_and(|view| {
                                view.entity_type == 410 && view.status.is_logically_dependent()
                            })
                            && (start + 1..=start + 2).all(|index| record.number(index).is_some())
                            && (entry.form == 0
                                || match record.value(start + 3) {
                                    None | Some(crate::parameter::TokenValue::Omitted) => true,
                                    _ => record.number(start + 3).is_some(),
                                }))
                    },
                    "iges drawing reference traversal",
                )
            })
            .transpose()?
            .unwrap_or(false);
        let annotation_count_index = 2 + view_count.unwrap_or_default() * width;
        let annotation_count = record.count(annotation_count_index);
        let annotations_valid = annotation_count
            .map(|count| -> Result<bool, CodecError> {
                ctx.all_by(
                    0..count,
                    |index| {
                        Ok(record
                            .integer(annotation_count_index + 1 + index)
                            .and_then(|value| u32::try_from(value).ok())
                            .map(|sequence| {
                                ctx.get_btree_map(
                                    entries,
                                    &sequence,
                                    "iges drawing directory lookup",
                                )
                                .map(Option::<&&DirectoryEntry>::copied)
                            })
                            .transpose()?
                            .flatten()
                            .is_some_and(|annotation| {
                                annotation.status.use_flag(global.global_table())
                                    == Some(UseFlag::Annotation)
                                    && annotation.status.is_physically_dependent()
                            }))
                    },
                    "iges drawing reference traversal",
                )
            })
            .transpose()?
            .unwrap_or(false);
        if drawing_directory_valid(entry, global.global_table()) && views_valid && annotations_valid
        {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges drawing decoded sequences",
                "iges drawing decoded sequences",
            )?;
        } else {
            push_drawing_entity_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "drawing view placements or drawing-space annotations are invalid",
            )?;
        }
    }

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges drawing directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 410 && matches!(entry.form, 0 | 1)) {
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(records, &entry.sequence, "iges drawing parameter lookup")?
            .copied()
        else {
            push_drawing_entity_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "Parameter Data record is missing",
            )?;
            continue;
        };
        let view_number_valid = record.integer_or(1, 0).is_some();
        let scale_valid = record
            .number_or(2, if entry.form == 0 { 1.0 } else { 0.0 })
            .is_some_and(|value| value.is_finite() && value > 0.0);
        let form_valid = if entry.form == 0 {
            let transform_valid = if entry.transform == 0 {
                true
            } else {
                let target_valid = u32::try_from(entry.transform)
                    .ok()
                    .map(|sequence| {
                        ctx.get_btree_map(entries, &sequence, "iges drawing directory lookup")
                            .map(|entry| {
                                entry.is_some_and(|target| {
                                    target.entity_type == 124 && target.form == 0
                                })
                            })
                    })
                    .transpose()?
                    .unwrap_or(false);
                if target_valid {
                    let mut transform_storage =
                        ctx.reserve_scoped(0, "iges drawing transform scratch")?;
                    match transform_storage.with_storage(|| {
                        resolve_transform(
                            entry.transform,
                            entries,
                            records,
                            global.length_factor_mm(),
                            global.real_precision(),
                            &mut BTreeSet::new(),
                            ctx,
                        )
                    }) {
                        Ok(_) => true,
                        Err(error) => {
                            error.non_resource()?;
                            false
                        }
                    }
                } else {
                    false
                }
            };
            let mut clipping_valid = true;
            for index in 3..=8 {
                let valid = match record.integer_or(index, 0) {
                    Some(0) => true,
                    Some(value) => match u32::try_from(value).ok() {
                        Some(sequence) => ctx
                            .get_btree_map(entries, &sequence, "iges drawing directory lookup")?
                            .is_some_and(|target| {
                                clipping_plane_valid(target, global.global_table())
                            }),
                        None => false,
                    },
                    None => false,
                };
                if !valid {
                    clipping_valid = false;
                    break;
                }
            }
            transform_valid && clipping_valid
        } else {
            let normal = finite_vector(record, 3);
            let reference = finite_vector(record, 6);
            let center = finite_vector(record, 9);
            let up = finite_vector(record, 12);
            let vectors_valid = normal
                .zip(up)
                .is_some_and(|(normal, up)| has_in_plane_component(normal.get(), up.get()));
            let window_valid = (15..=19).all(|index| record.number_or(index, 0.0).is_some())
                && record
                    .number_or(16, 0.0)
                    .zip(record.number_or(17, 0.0))
                    .is_some_and(|(min, max)| min < max)
                && record
                    .number_or(18, 0.0)
                    .zip(record.number_or(19, 0.0))
                    .is_some_and(|(min, max)| min < max);
            let depth = record
                .integer_or(20, 0)
                .filter(|value| depth_clipping_valid(*value));
            let depth_values_valid = (21..=22).all(|index| record.number_or(index, 0.0).is_some())
                && (depth != Some(3)
                    || record
                        .number_or(21, 0.0)
                        .zip(record.number_or(22, 0.0))
                        .is_some_and(|(min, max)| min < max));
            entry.transform == 0
                && reference.is_some()
                && center.is_some()
                && vectors_valid
                && window_valid
                && depth.is_some()
                && depth_values_valid
        };
        if view_directory_valid(entry, global.global_table())
            && view_number_valid
            && scale_valid
            && form_valid
        {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges drawing decoded sequences",
                "iges drawing decoded sequences",
            )?;
        } else {
            push_drawing_entity_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "view number, projection, transform, scale, or clipping fields are invalid",
            )?;
        }
    }

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges drawing directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 402 && entry.form == 19) {
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(records, &entry.sequence, "iges drawing parameter lookup")?
            .copied()
        else {
            push_drawing_entity_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "Parameter Data record is missing",
            )?;
            continue;
        };
        let count = record.count(1).filter(|count| *count > 0);
        let mut last_view = None;
        let mut closed_view_storage = ctx.reserve_scoped(0, "iges drawing closed view scratch")?;
        let mut closed_views = BTreeSet::new();
        let mut last_breakpoint: Option<FiniteReal> = None;
        let blocks_valid = if let Some(count) = count {
            ctx.all_by(
                0..count,
                |index| {
                    let start = 2 + index * 6;
                    let view = record
                        .integer(start)
                        .and_then(|value| u32::try_from(value).ok())
                        .map(|sequence| {
                            ctx.get_btree_map(entries, &sequence, "iges drawing directory lookup")
                                .map(|target| {
                                    target
                                        .filter(|target| target.entity_type == 410)
                                        .map(|_| sequence)
                                })
                        })
                        .transpose()?
                        .flatten();
                    if view != last_view {
                        if let Some(previous) = last_view {
                            closed_view_storage.with_storage(|| {
                                ctx.insert_btree_set(
                                    &mut closed_views,
                                    previous,
                                    "iges drawing closed views",
                                )
                            })?;
                        }
                        last_breakpoint = None;
                    }
                    let view_order_valid = match view {
                        Some(view) => !ctx.contains_btree_set(
                            &closed_views,
                            &view,
                            "iges drawing closed view lookup",
                        )?,
                        None => false,
                    };
                    let breakpoint = record.number(start + 1).and_then(FiniteReal::new);
                    let breakpoint_order_valid = breakpoint.is_some_and(|value| {
                        last_breakpoint.is_none_or(|previous| value.get() > previous.get())
                    });
                    last_view = view;
                    last_breakpoint = breakpoint;
                    let display_valid = record.integer(start + 2).is_some_and(display_flag_valid);
                    let color_valid = match record.value(start + 3) {
                        None | Some(crate::parameter::TokenValue::Omitted) => true,
                        _ => match record.integer(start + 3) {
                            Some(value) if standard_color_valid(value) => true,
                            Some(value) => match value
                                .checked_neg()
                                .and_then(|value| u32::try_from(value).ok())
                            {
                                Some(sequence) => ctx
                                    .get_btree_map(
                                        entries,
                                        &sequence,
                                        "iges drawing directory lookup",
                                    )?
                                    .is_some_and(|target| target.entity_type == 314),
                                None => false,
                            },
                            None => false,
                        },
                    };
                    let font_valid = match record.value(start + 4) {
                        None | Some(crate::parameter::TokenValue::Omitted) => true,
                        _ => match record.integer(start + 4) {
                            Some(value) if value == 0 || standard_line_font_valid(value) => true,
                            Some(value) => match value
                                .checked_neg()
                                .and_then(|value| u32::try_from(value).ok())
                            {
                                Some(sequence) => ctx
                                    .get_btree_map(
                                        entries,
                                        &sequence,
                                        "iges drawing directory lookup",
                                    )?
                                    .is_some_and(|target| target.entity_type == 304),
                                None => false,
                            },
                            None => false,
                        },
                    };
                    let weight_valid = match record.value(start + 5) {
                        None | Some(crate::parameter::TokenValue::Omitted) => true,
                        _ => record.integer(start + 5).is_some_and(|value| value >= 0),
                    };
                    Ok(view_order_valid
                        && breakpoint_order_valid
                        && display_valid
                        && color_valid
                        && font_valid
                        && weight_valid)
                },
                "iges segmented view traversal",
            )?
        } else {
            false
        };
        if views_visible_directory_valid(entry, global.global_table()) && blocks_valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges drawing decoded sequences",
                "iges drawing decoded sequences",
            )?;
        } else {
            push_drawing_entity_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "segmented-view blocks, grouping, breakpoints, or display fields are invalid",
            )?;
        }
    }

    let mut association_storage = ctx.reserve_scoped(0, "iges view association index")?;
    let mut associations = BTreeMap::<u32, BTreeSet<u32>>::new();

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges drawing directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 402 && matches!(entry.form, 3 | 4)) {
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(records, &entry.sequence, "iges drawing parameter lookup")?
            .copied()
        else {
            push_drawing_entity_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "Parameter Data record is missing",
            )?;
            continue;
        };
        let view_count = record.count(1).filter(|count| *count > 0);
        let entity_count =
            crate::parameter::view_visibility_entity_count(record, global.global_table());
        let block_width = if entry.form == 3 { 1 } else { 5 };
        let views_valid = view_count
            .map(|count| -> Result<bool, CodecError> {
                ctx.all_by(
                    0..count,
                    |index| {
                        let start = 3 + index * block_width;
                        let view = match record
                            .integer(start)
                            .and_then(|value| u32::try_from(value).ok())
                        {
                            Some(sequence) => ctx
                                .get_btree_map(entries, &sequence, "iges drawing directory lookup")?
                                .copied(),
                            None => None,
                        };
                        let back_pointer_valid = if let Some(view) =
                            view.filter(|view| view.entity_type == 410)
                        {
                            if !ctx.contains_key_btree_map(
                                &associations,
                                &view.sequence,
                                "iges view association index lookup",
                            )? {
                                let groups = match ctx.get_btree_map(
                                    records,
                                    &view.sequence,
                                    "iges drawing parameter lookup",
                                )? {
                                    Some(view_record) => ctx.get_btree_map(
                                        trailing_pointer_analysis,
                                        &view_record.directory_sequence,
                                        "iges drawing trailing analysis lookup",
                                    )?,
                                    None => None,
                                };
                                let mut members = BTreeSet::new();
                                if let Some(TrailingPointerAnalysis::Unambiguous(groups)) = groups {
                                    let mut source_index_entries = groups.associations().iter();
                                    while source_index_entries.len() != 0 {
                                        let Some(sequence) = ctx.next_charged(
                                            &mut source_index_entries,
                                            "iges view association index traversal",
                                        )?
                                        else {
                                            break;
                                        };
                                        association_storage.with_storage(|| {
                                            ctx.insert_btree_set(
                                                &mut members,
                                                *sequence,
                                                "iges view association index members",
                                            )
                                        })?;
                                    }
                                }
                                association_storage.with_storage(|| {
                                    ctx.insert_btree_map(
                                        &mut associations,
                                        view.sequence,
                                        members,
                                        "iges view association index views",
                                    )
                                })?;
                            }
                            let members = ctx
                                .get_btree_map(
                                    &associations,
                                    &view.sequence,
                                    "iges view association index lookup",
                                )?
                                .ok_or_else(|| {
                                    CodecError::malformed("IGES view association index is absent")
                                })?;
                            ctx.contains_btree_set(
                                members,
                                &entry.sequence,
                                "iges view association search",
                            )?
                        } else {
                            false
                        };
                        Ok(back_pointer_valid
                            && (entry.form == 3 || {
                                let line_font = record.integer(start + 1);
                                let definition = record.integer(start + 2);
                                let color = record.integer_or(start + 3, 0);
                                let weight = record.integer(start + 4);
                                line_font.is_some_and(|value| {
                                    value == 0 || standard_line_font_valid(value)
                                }) && match definition {
                                    Some(value) if line_font == Some(0) => {
                                        match u32::try_from(value).ok() {
                                            Some(sequence) => ctx
                                                .get_btree_map(
                                                    entries,
                                                    &sequence,
                                                    "iges drawing directory lookup",
                                                )?
                                                .is_some_and(|target| target.entity_type == 304),
                                            None => false,
                                        }
                                    }
                                    Some(value) => value == 0,
                                    None => false,
                                } && match color {
                                    Some(value) if standard_color_valid(value) => true,
                                    Some(value) => match value
                                        .checked_neg()
                                        .and_then(|value| u32::try_from(value).ok())
                                    {
                                        Some(sequence) => ctx
                                            .get_btree_map(
                                                entries,
                                                &sequence,
                                                "iges drawing directory lookup",
                                            )?
                                            .is_some_and(|target| target.entity_type == 314),
                                        None => false,
                                    },
                                    None => false,
                                } && weight.is_some_and(|value| value >= 0)
                            }))
                    },
                    "iges drawing reference traversal",
                )
            })
            .transpose()?
            .unwrap_or(false);
        let entities_valid = view_count
            .zip(entity_count)
            .map(|(views, count)| -> Result<bool, CodecError> {
                ctx.all_by(
                    0..count,
                    |index| {
                        Ok(record
                            .integer(3 + views * block_width + index)
                            .and_then(|value| u32::try_from(value).ok())
                            .filter(|sequence| sequence % 2 == 1)
                            .map(|sequence| {
                                ctx.contains_key_btree_map(
                                    entries,
                                    &sequence,
                                    "iges drawing directory lookup",
                                )
                            })
                            .transpose()?
                            .unwrap_or(false))
                    },
                    "iges drawing reference traversal",
                )
            })
            .transpose()?
            .unwrap_or(false);
        if views_visible_directory_valid(entry, global.global_table())
            && views_valid
            && entities_valid
        {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges drawing decoded sequences",
                "iges drawing decoded sequences",
            )?;
        } else {
            push_drawing_entity_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "view-visibility blocks, display overrides, entities, or back pointers are invalid",
            )?;
        }
    }

    Ok(ProjectionOutcome {
        decoded,
        decoded_storage,
        losses,
        loss_slots_storage,
    })
}

#[cfg(test)]
mod tests;
