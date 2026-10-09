// SPDX-License-Identifier: Apache-2.0
//! Directory display attributes and color definitions.

use super::geometry::ProjectionOutcome;
use super::{mirror_flag_valid, vertical_text_flag_valid, PropertyTextIndex};

use crate::directory::{DirectoryEntry, Hierarchy, Subordinate, UseFlag};
use crate::global::{GlobalTable, ProjectedGlobal};
use crate::loss::IgesLossCode;
use crate::parameter::{ParameterRecord, TokenValue, TrailingPointerAnalysis};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::ids::AppearanceId;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::CadIr;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
struct TextFontDefinition {
    supersedes: Option<u32>,
}

fn standard_color(number: i64) -> Option<Color> {
    let (r, g, b) = match number {
        1 => (0.0, 0.0, 0.0),
        2 => (1.0, 0.0, 0.0),
        3 => (0.0, 1.0, 0.0),
        4 => (0.0, 0.0, 1.0),
        5 => (1.0, 1.0, 0.0),
        6 => (1.0, 0.0, 1.0),
        7 => (0.0, 1.0, 1.0),
        8 => (1.0, 1.0, 1.0),
        _ => return None,
    };
    Color::new(r, g, b, 1.0)
}

fn retained_utf8(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<Option<String>, CodecError> {
    let Ok(value) = ctx.validate_utf8(bytes, operation)? else {
        return Ok(None);
    };
    Ok(Some(ctx.copy_retained_text(value, operation)?))
}

fn push_presentation_loss(
    ctx: &DecodeContext<'_>,
    slots: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    entry: &DirectoryEntry,
    reason: &str,
) -> Result<(), CodecError> {
    super::push_attributed_loss_with_scoped_slots(
        ctx,
        slots,
        losses,
        entry,
        IgesLossCode::DisplayDataNotProjected,
        format_args!(
            "IGES entity type {} form {} display data was not projected: {reason}",
            entry.entity_type, entry.form
        ),
    )
}

fn text_font_definition_pointer_valid(
    value: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(sequence) = value
        .checked_neg()
        .and_then(|value| u32::try_from(value).ok())
        .filter(|sequence| sequence % 2 == 1)
    else {
        return Ok(false);
    };
    Ok(ctx
        .get_btree_map(entries, &sequence, "iges presentation font pointer lookup")?
        .is_some_and(|entry| entry.entity_type == 310 && entry.form == 0))
}

pub(super) fn general_note_font_valid_for_global_table(
    value: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let standard = match global_table {
        GlobalTable::V4_0 => {
            matches!(
                value,
                0 | 1 | 2 | 3 | 6 | 12 | 13 | 14 | 17 | 18 | 19 | 1001..=1003
            )
        }
        GlobalTable::V5_0 => matches!(
            value,
            0 | 1 | 2 | 3 | 6 | 12 | 13 | 14 | 17 | 18 | 19 | 1001..=1003 | 2001
        ),
        GlobalTable::Legacy | GlobalTable::V5Later => {
            matches!(
                value,
                0 | 1 | 2 | 3 | 6 | 12 | 13 | 14 | 17 | 18 | 19 | 1001..=1003 | 2001 | 3001
            )
        }
    };
    if standard {
        Ok(true)
    } else {
        text_font_definition_pointer_valid(value, entries, ctx)
    }
}

pub(super) fn new_general_note_font_valid(value: i64) -> bool {
    matches!(value, 1 | 2 | 3 | 6 | 12 | 13 | 14 | 17 | 18 | 19)
}

pub(super) fn new_general_note_charset_valid(
    value: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if matches!(value, 1 | 1001 | 1002 | 1003 | 2001 | 3001) {
        Ok(true)
    } else {
        text_font_definition_pointer_valid(value, entries, ctx)
    }
}

fn line_font_definition_directory_valid(entry: &DirectoryEntry, global_table: GlobalTable) -> bool {
    entry.status.subordinate() == Some(Subordinate::Independent)
        && entry.status.use_flag(global_table) == Some(UseFlag::Definition)
        && (1..=5).contains(&entry.line_font)
}

fn text_template_directory_valid(entry: &DirectoryEntry, global_table: GlobalTable) -> bool {
    match global_table {
        GlobalTable::V4_0 | GlobalTable::V5_0 => {
            entry.status.subordinate() != Some(Subordinate::Independent)
                && entry.status.use_flag(global_table) == Some(UseFlag::Annotation)
                && entry.line_font != 0
        }
        GlobalTable::Legacy | GlobalTable::V5Later => {
            entry.status.subordinate() == Some(Subordinate::Independent)
                && entry.status.use_flag(global_table) == Some(UseFlag::Definition)
                && entry.structure == 0
                && entry.line_font == 0
                && entry.view == 0
                && entry.transform == 0
                && entry.label_display == 0
                && entry.line_weight == 0
                && entry.status.hierarchy() == Some(Hierarchy::GlobalTopDown)
        }
    }
}

fn directory_color_is_semantic(entry: &DirectoryEntry, global_table: GlobalTable) -> bool {
    !(matches!(global_table, GlobalTable::V4_0) && matches!(entry.entity_type, 124 | 406))
}

fn directory_line_weight_is_semantic(entry: &DirectoryEntry, global_table: GlobalTable) -> bool {
    !(matches!(global_table, GlobalTable::V4_0) && matches!(entry.entity_type, 124 | 314 | 406))
}

fn appearance(
    ir: &mut CadIr,
    id: Cow<'_, AppearanceId>,
    name: Option<&[u8]>,
    color: Color,
    ctx: &DecodeContext<'_>,
    (identities, storage): (
        &mut Option<BTreeSet<String>>,
        &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ),
) -> Result<(), CodecError> {
    if identities.is_none() {
        let mut index = BTreeSet::new();
        let mut source_index_entries = ir.model.appearances.iter();
        while source_index_entries.len() != 0 {
            let Some(item) =
                ctx.next_charged(&mut source_index_entries, "iges appearance index traversal")?
            else {
                break;
            };
            let key =
                ctx.copy_scoped_text(item.id.as_str(), storage, "iges appearance index keys")?;
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut index, key, "iges appearance index nodes")
            })?;
        }
        *identities = Some(index);
    }
    let index = identities
        .as_mut()
        .ok_or_else(|| CodecError::malformed("IGES appearance index is absent"))?;
    if !ctx.contains_btree_set(index, id.as_str(), "iges appearance index lookup")? {
        let key = ctx.copy_scoped_text(id.as_str(), storage, "iges appearance index keys")?;
        storage.with_storage(|| ctx.insert_btree_set(index, key, "iges appearance index nodes"))?;
        let name = name
            .map(|name| retained_utf8(ctx, name, "iges color definition name"))
            .transpose()?
            .flatten();
        ctx.reserve_vec(
            &mut ir.model.appearances,
            1,
            "iges neutral appearance slots",
        )?;
        ctx.charge_entities(1, "iges_geometry_presentation")?;
        let id = match id {
            Cow::Borrowed(id) => id.try_clone_for_decode(ctx, "iges appearance ID copy")?,
            Cow::Owned(id) => id,
        };
        ir.model.appearances.push(Appearance {
            id,
            name,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: Some(
                ctx.format_retained(format_args!("IGES color"), "iges appearance schema")?,
            ),
            category: None,
            base_color: Some(color),
            properties: BTreeMap::new(),
            textures: Vec::new(),
        });
    }
    Ok(())
}

fn text_font_definition(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
) -> Result<Option<TextFontDefinition>, CodecError> {
    let parameter_end = record.parameter_end();
    let directory_valid = entry.status.subordinate() == Some(Subordinate::Independent)
        && entry.status.use_flag(global_table) == Some(UseFlag::Definition);
    if !directory_valid
        || record.integer(1).is_none_or(|value| value < 0)
        || record.string(2).is_none_or(<[u8]>::is_empty)
        || record.integer(4).is_none_or(|scale| scale <= 0)
    {
        return Ok(None);
    }
    let supersedes = match record.value(3) {
        None | Some(TokenValue::Omitted) => None,
        Some(TokenValue::Integer(value)) if *value >= 0 => None,
        Some(TokenValue::Integer(value)) => {
            let Some(sequence) = value
                .checked_neg()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|sequence| sequence % 2 == 1)
            else {
                return Ok(None);
            };
            ctx.get_btree_map(
                entries,
                &sequence,
                "iges presentation font pointer lookup",
            )?
            .filter(|target| target.entity_type == 310 && target.form == 0)
            .map(|_| sequence)
        }
        Some(TokenValue::Real(_) | TokenValue::String(_)) => return Ok(None),
    };
    if record.integer(3).is_some_and(|value| value < 0) && supersedes.is_none() {
        return Ok(None);
    }
    let Some(count) = record.count(5).filter(|count| *count > 0) else {
        return Ok(None);
    };
    let mut cursor = 6;
    let mut character_codes = 0_u128;
    let mut characters = 0..count;
    while ctx
        .next_charged(&mut characters, "iges text font character traversal")?
        .is_some()
    {
        let Some(code) = record
            .integer(cursor)
            .filter(|value| matches!(value, 0..=127))
        else {
            return Ok(None);
        };
        let Some(mask) = u32::try_from(code)
            .ok()
            .and_then(|code| 1_u128.checked_shl(code))
        else {
            return Ok(None);
        };
        if character_codes & mask != 0 {
            return Ok(None);
        }
        character_codes |= mask;
        if record.integer(cursor + 1).is_none() || record.integer(cursor + 2).is_none() {
            return Ok(None);
        }
        let Some(count) = record.count(cursor + 3) else {
            return Ok(None);
        };
        cursor += 4;
        let mut motions = 0..count;
        while ctx
            .next_charged(&mut motions, "iges text font motion traversal")?
            .is_some()
        {
            if record
                .integer_or(cursor, 0)
                .is_none_or(|value| !matches!(value, 0..=1))
                || record.integer(cursor + 1).is_none()
                || record.integer(cursor + 2).is_none()
            {
                return Ok(None);
            }
            cursor += 3;
        }
    }
    Ok((cursor == parameter_end).then_some(TextFontDefinition { supersedes }))
}

pub(super) fn project<'ctx>(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    (entries, records): (
        &BTreeMap<u32, &DirectoryEntry>,
        &BTreeMap<u32, &ParameterRecord>,
    ),
    trailing_pointer_analysis: &BTreeMap<u32, TrailingPointerAnalysis>,
    global: &ProjectedGlobal,
    ctx: &'ctx DecodeContext<'_>,
    sequences: &super::geometry::SourceSequences<'_>,
) -> Result<ProjectionOutcome<'ctx>, CodecError> {
    let mut decoded_storage = ctx.reserve_scoped(0, "iges presentation decoded sequences")?;
    let mut decoded = BTreeSet::new();
    let mut loss_slots_storage = ctx.reserve_scoped(0, "iges entity loss slots")?;
    let mut losses = Vec::new();
    let mut scratch = ctx.reserve_scoped(0, "iges presentation scratch")?;
    let mut appearances = None;
    let mut defined = BTreeMap::new();
    let mut text_fonts = BTreeMap::new();
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges presentation directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 310 && entry.form == 0) {
            continue;
        }
        let record = ctx.get_btree_map(
            records,
            &entry.sequence,
            "iges presentation Parameter Data lookup",
        )?;
        if let Some(font) = record
            .copied()
            .map(|record| text_font_definition(entry, record, entries, global.global_table(), ctx))
            .transpose()?
            .flatten()
        {
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut text_fonts,
                    entry.sequence,
                    font,
                    "iges presentation font index",
                )
            })?;
        }
    }
    let mut cyclic_fonts = BTreeMap::new();

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges presentation directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 310 && entry.form == 0) {
            continue;
        }
        let mut cycle_storage = ctx.reserve_scoped(0, "iges font cycle scratch")?;
        let mut active = BTreeSet::new();
        let mut next_sequence = Some(entry.sequence);
        let cyclic = loop {
            let mut current = next_sequence.into_iter();
            let Some(sequence) = ctx.next_charged(&mut current, "iges font cycle traversal")? else {
                break false;
            };
            if let Some(cyclic) = ctx.get_btree_map(
                &cyclic_fonts,
                &sequence,
                "iges font cycle result lookup",
            )? {
                break *cyclic;
            }
            if !cycle_storage.with_storage(|| {
                ctx.insert_btree_set(&mut active, sequence, "iges font cycle active")
            })? {
                break true;
            }
            next_sequence = ctx
                .get_btree_map(
                    &text_fonts,
                    &sequence,
                    "iges text font chain lookup",
                )?
                .and_then(|font| font.supersedes);
        };
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut source_values = IntoIterator::into_iter(active);
        while source_values.len() != 0 {
            let Some(sequence) = ctx.next_charged(&mut source_values, "iges font cycle result traversal")? else {
                break;
            };
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut cyclic_fonts,
                    sequence,
                    cyclic,
                    "iges font cycle results",
                )
            })?;
        }
        let target_valid = match ctx.get_btree_map(
            &text_fonts,
            &entry.sequence,
            "iges text font target lookup",
        )? {
            Some(font) => match font.supersedes {
                Some(target) => ctx.contains_key_btree_map(
                    &text_fonts,
                    &target,
                    "iges text font target lookup",
                )?,
                None => true,
            },
            None => false,
        };
        if target_valid && !cyclic {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges presentation decoded sequences",
                "iges presentation decoded sequences",
            )?;
        } else {
            push_presentation_loss(ctx, &mut loss_slots_storage, &mut losses, entry, "font header, superseded-font chain, character grammar, pen motions, or Directory fields are invalid")?;
        }
    }

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges presentation directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 312 && matches!(entry.form, 0..=1)) {
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(
                records,
                &entry.sequence,
                "iges presentation Parameter Data lookup",
            )?
            .copied()
        else {
            push_presentation_loss(ctx, &mut loss_slots_storage, &mut losses, entry, "Parameter Data record is missing")?;
            continue;
        };
        let parameter_end = record.parameter_end();
        let font = record.integer_or(3, 1);
        let font_valid = match font {
            Some(font) => {
                general_note_font_valid_for_global_table(
                    font,
                    entries,
                    global.global_table(),
                    ctx,
                )?
            }
            None => false,
        };
        let directory_valid = text_template_directory_valid(entry, global.global_table());
        let fields_valid = parameter_end <= 11
            && (1..=2).all(|index| {
                record
                    .number_or(index, 0.0)
                    .is_some_and(|value| value.is_finite() && value >= 0.0)
            })
            && font_valid
            && record.number_or(4, std::f64::consts::FRAC_PI_2).is_some()
            && record.number_or(5, 0.0).is_some()
            && record.integer_or(6, 0).is_some_and(mirror_flag_valid)
            && record
                .integer_or(7, 0)
                .is_some_and(vertical_text_flag_valid)
            && (8..=10).all(|index| record.number_or(index, 0.0).is_some());
        if directory_valid && fields_valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges presentation decoded sequences",
                "iges presentation decoded sequences",
            )?;
        } else {
            push_presentation_loss(ctx, &mut loss_slots_storage, &mut losses, entry, "text-template metrics, font, orientation, placement, or Directory fields are invalid")?;
        }
    }

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges presentation directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 406 && entry.form == 1) {
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(
                records,
                &entry.sequence,
                "iges presentation Parameter Data lookup",
            )?
            .copied()
        else {
            push_presentation_loss(ctx, &mut loss_slots_storage, &mut losses, entry, "Parameter Data record is missing")?;
            continue;
        };
        let levels_valid = if let Some(count) = record.count(1).filter(|count| *count > 0) {
            let mut level_storage = ctx.reserve_scoped(0, "iges presentation level scratch")?;
            let mut levels = BTreeSet::new();
            ctx.all_by(
                0..count,
                |index| {
                    let Some(level) = record.integer(2 + index).filter(|level| *level >= 0) else {
                        return Ok(false);
                    };
                    level_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut levels,
                            level,
                            "iges presentation definition levels",
                        )
                    })
                },
                "iges definition level traversal",
            )?
        } else {
            false
        };
        if levels_valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges presentation decoded sequences",
                "iges presentation decoded sequences",
            )?;
        } else {
            push_presentation_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "definition-level count, value, or uniqueness is invalid",
            )?;
        }
    }

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges presentation directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 304 && matches!(entry.form, 1 | 2)) {
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(
                records,
                &entry.sequence,
                "iges presentation Parameter Data lookup",
            )?
            .copied()
        else {
            push_presentation_loss(ctx, &mut loss_slots_storage, &mut losses, entry, "Parameter Data record is missing")?;
            continue;
        };
        if !line_font_definition_directory_valid(entry, global.global_table()) {
            push_presentation_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "line-font definition use flag or fallback pattern is invalid",
            )?;
            continue;
        }
        let valid = if entry.form == 1 {
            let template = record
                .integer(2)
                .and_then(|value| u32::try_from(value).ok());
            matches!(record.integer(1), Some(0 | 1))
                && match template {
                    Some(sequence) if sequence % 2 == 1 => ctx
                        .get_btree_map(
                            entries,
                            &sequence,
                            "iges presentation line font template lookup",
                        )?
                        .is_some_and(|target| target.entity_type == 308 && target.form == 0),
                    _ => false,
                }
                && record
                    .number(3)
                    .is_some_and(|value| value.is_finite() && value > 0.0)
                && record
                    .number(4)
                    .is_some_and(|value| value.is_finite() && value > 0.0)
        } else {
            let count = record.count(1).filter(|count| *count > 0);
            if let Some(count) = count {
                let expected_digits = count.div_ceil(4);
                ctx.all_by(
                    0..count,
                    |index| {
                        Ok(record
                            .number(2 + index)
                            .is_some_and(|value| value.is_finite() && value > 0.0))
                    },
                    "iges line font segment traversal",
                )? && if let Some(pattern) = record.string(2 + count) {
                    pattern.len() == expected_digits
                        && ctx.all_by(
                            pattern,
                            |byte| Ok(byte.is_ascii_hexdigit()),
                            "iges line font pattern validation",
                        )?
                        && {
                            let first = match pattern[0] {
                                b'0'..=b'9' => pattern[0] - b'0',
                                b'A'..=b'F' => pattern[0] - b'A' + 10,
                                b'a'..=b'f' => pattern[0] - b'a' + 10,
                                _ => 255,
                            };
                            first < (1_u8 << (4 - (expected_digits * 4 - count)))
                        }
                } else {
                    false
                }
            } else {
                false
            }
        };
        if valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges presentation decoded sequences",
                "iges presentation decoded sequences",
            )?;
        } else {
            push_presentation_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "line-font definition parameters are invalid",
            )?;
        }
    }

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges presentation directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 314 && entry.form == 0) {
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(
                records,
                &entry.sequence,
                "iges presentation Parameter Data lookup",
            )?
            .copied()
        else {
            push_presentation_loss(ctx, &mut loss_slots_storage, &mut losses, entry, "Parameter Data record is missing")?;
            continue;
        };
        let components = [1, 2, 3].map(|index| {
            record
                .number(index)
                .filter(|value| (0.0..=100.0).contains(value))
        });
        let [Some(red), Some(green), Some(blue)] = components else {
            push_presentation_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "RGB percentage is outside 0 through 100",
            )?;
            continue;
        };
        let name = match record.value(4) {
            None | Some(crate::parameter::TokenValue::Omitted) => None,
            Some(crate::parameter::TokenValue::String(_)) => record.string(4),
            Some(crate::parameter::TokenValue::Integer(0))
                if matches!(global.global_table(), GlobalTable::V4_0) =>
            {
                None
            }
            Some(
                crate::parameter::TokenValue::Integer(_) | crate::parameter::TokenValue::Real(_),
            ) => {
                push_presentation_loss(
                    ctx,
                    &mut loss_slots_storage,
                    &mut losses,
                    entry,
                    "optional color name is not a string",
                )?;
                continue;
            }
        };
        let directory_valid = entry.status.subordinate() == Some(Subordinate::Independent)
            && entry.status.use_flag(global.global_table()) == Some(UseFlag::Definition)
            && matches!(entry.color, 0..=8);
        if !directory_valid {
            push_presentation_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "color definition Directory fields are invalid",
            )?;
            continue;
        }
        let color = cadmpeg_core::convert::f32_from_f64(red / 100.0)
            .zip(cadmpeg_core::convert::f32_from_f64(green / 100.0))
            .zip(cadmpeg_core::convert::f32_from_f64(blue / 100.0))
            .and_then(|((red, green), blue)| Color::new(red, green, blue, 1.0));
        let Some(color) = color else {
            push_presentation_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "color definition components are outside [0, 100]",
            )?;
            continue;
        };
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut defined,
                entry.sequence,
                color,
                "iges presentation defined colors",
            )
        })?;
        let appearance_id = crate::ids::appearance_color_admitted(
            &crate::ids::Stem::directory(entry.sequence),
            ctx,
        )?;
        appearance(
            ir,
            Cow::Owned(appearance_id),
            name,
            color,
            ctx,
            (&mut appearances, &mut scratch),
        )?;
        ctx.insert_scoped_btree_set(
            &mut decoded_storage,
            &mut decoded,
            entry.sequence,
            "iges presentation decoded sequences",
            "iges presentation decoded sequences",
        )?;
    }

    let resolve_color = |value: i64| -> Result<Option<Color>, CodecError> {
        Ok(match value.cmp(&0) {
            std::cmp::Ordering::Greater => standard_color(value),
            std::cmp::Ordering::Less => {
                let Some(sequence) = value
                    .checked_neg()
                    .and_then(|value| u32::try_from(value).ok())
                else {
                    return Ok(None);
                };
                let Some(entry) = ctx.get_btree_map(
                    entries,
                    &sequence,
                    "iges presentation color definition lookup",
                )? else {
                    return Ok(None);
                };
                if entry.entity_type != 314 || entry.form != 0 {
                    return Ok(None);
                }
                ctx.get_btree_map(
                    &defined,
                    &sequence,
                    "iges presentation defined color lookup",
                )?
                .copied()
            }
            std::cmp::Ordering::Equal => None,
        })
    };
    let resolve = |value: i64| -> Result<Option<(AppearanceId, Color)>, CodecError> {
        let Some(color) = resolve_color(value)? else {
            return Ok(None);
        };
        let id = if value > 0 {
            crate::ids::appearance_standard_admitted(&crate::ids::Stem::number(value), ctx)?
        } else {
            let Some(sequence) = value
                .checked_neg()
                .and_then(|value| u32::try_from(value).ok())
            else {
                return Ok(None);
            };
            crate::ids::appearance_color_admitted(&crate::ids::Stem::directory(sequence), ctx)?
        };
        Ok(Some((id, color)))
    };

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges presentation directory traversal")?
        else {
            break;
        };
        if !(entry.color != 0 && directory_color_is_semantic(entry, global.global_table())) {
            continue;
        }
        if resolve_color(entry.color)?.is_none() {
            push_presentation_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "Directory color number or definition pointer is invalid",
            )?;
        }
    }
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges presentation directory traversal")?
        else {
            break;
        };
        if !(entry.level < 0) {
            continue;
        }
        let sequence = entry.level.unsigned_abs();
        let target_valid = match u32::try_from(sequence) {
            Ok(sequence) => {
                ctx.contains_btree_set(&decoded, &sequence, "iges presentation decoded lookup")?
                    && ctx
                        .get_btree_map(
                            entries,
                            &sequence,
                            "iges presentation definition level target lookup",
                        )?
                        .is_some_and(|target| target.entity_type == 406 && target.form == 1)
            }
            Err(_) => false,
        };
        if !target_valid {
            push_presentation_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "negative Directory level does not reference a decoded Definition Levels property",
            )?;
        }
    }
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges presentation directory traversal")?
        else {
            break;
        };
        if !(entry.line_weight != 0 && directory_line_weight_is_semantic(entry, global.global_table())) {
            continue;
        }
        if !global.line_weight_number_is_valid(entry.line_weight) {
            push_presentation_loss(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                "line-weight number is outside the Global gradation range",
            )?;
        }
    }

    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(&mut ir.model.curves);
    while source_values.len() != 0 {
        let Some(curve) = ctx.next_charged(&mut source_values, "iges curve display traversal")? else {
            break;
        };
        if let Some(source) = &mut curve.source_object {
            source.color = match sequences.curve(&curve.id, ctx)? {
                Some(sequence) => match ctx.get_btree_map(
                    entries,
                    &sequence,
                    "iges presentation source color entry lookup",
                )? {
                    Some(entry) => resolve_color(entry.color)?,
                    None => None,
                },
                None => None,
            };
        }
    }
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(&mut ir.model.surfaces);
    while source_values.len() != 0 {
        let Some(surface) = ctx.next_charged(&mut source_values, "iges surface display traversal")? else {
            break;
        };
        if let Some(source) = &mut surface.source_object {
            source.color = match sequences.surface(&surface.id, ctx)? {
                Some(sequence) => match ctx.get_btree_map(
                    entries,
                    &sequence,
                    "iges presentation source color entry lookup",
                )? {
                    Some(entry) => resolve_color(entry.color)?,
                    None => None,
                },
                None => None,
            };
        }
    }

    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(0..ir.model.bodies.len());
    while source_values.len() != 0 {
        let Some(index) = ctx.next_charged(&mut source_values, "iges body display traversal")? else {
            break;
        };
        let body = &ir.model.bodies[index];
        let Some(sequence) = sequences.body(&body.id, ctx)? else {
            continue;
        };
        let Some((sequence, color_number, visible)) = ctx
            .get_btree_map(
                entries,
                &sequence,
                "iges presentation body directory lookup",
            )?
            .map(|entry| (sequence, entry.color, entry.status.is_visible()))
        else {
            continue;
        };
        let Some((appearance_id, color)) = resolve(color_number)? else {
            continue;
        };
        let body = &mut ir.model.bodies[index];
        let body_id = body
            .id
            .try_clone_for_decode(ctx, "iges appearance body ID copy")?;
        body.color = Some(color);
        body.visible = Some(visible);
        appearance(
            ir,
            Cow::Borrowed(&appearance_id),
            None,
            color,
            ctx,
            (&mut appearances, &mut scratch),
        )?;
        ctx.reserve_vec(
            &mut ir.model.appearance_bindings,
            1,
            "iges appearance binding slots",
        )?;
        ctx.charge_entities(1, "iges_geometry_presentation")?;
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: crate::ids::appearance_binding_admitted(
                &crate::ids::Stem::word_directory(crate::ids::Word::Body, sequence),
                ctx,
            )?,
            target: AppearanceTarget::Body(body_id),
            appearance: appearance_id,
            source_entity_id: None,
            object_type: Some(
                ctx.format_retained(format_args!("Body"), "iges appearance object type")?,
            ),
            visible: None,
            channels: BTreeMap::new(),
        });
    }
    let mut name_texts = PropertyTextIndex::new(ctx)?;
    let mut body_names = BTreeMap::<u32, Option<&str>>::new();
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(&mut ir.model.bodies);
    while source_values.len() != 0 {
        let Some(body) = ctx.next_charged(&mut source_values, "iges body name traversal")? else {
            break;
        };
        if body.visible.is_none() {
            body.visible = match sequences.body(&body.id, ctx)? {
                Some(sequence) => ctx
                    .get_btree_map(
                        entries,
                        &sequence,
                        "iges presentation body visibility lookup",
                    )?
                    .map(|entry| entry.status.is_visible()),
                None => None,
            };
        }
        let Some(sequence) = sequences.body(&body.id, ctx)? else {
            continue;
        };
        let Some(TrailingPointerAnalysis::Unambiguous(groups)) = ctx.get_btree_map(
            trailing_pointer_analysis,
            &sequence,
            "iges presentation body property analysis lookup",
        )?
        else {
            continue;
        };
        let mut first: Option<(u32, &str)> = None;
        let mut conflicting = false;
        let mut properties = groups.properties().iter();
        while let Some(pointer) =
            ctx.next_charged(&mut properties, "iges body property traversal")?
        {
            let Some(entry) = ctx.get_btree_map(
                entries,
                pointer,
                "iges body property Directory lookup",
            )? else {
                continue;
            };
            if entry.entity_type != 406 || entry.form != 15 {
                continue;
            }
            let Some(record) = ctx.get_btree_map(
                records,
                pointer,
                "iges body property Parameter Data lookup",
            )? else {
                continue;
            };
            if record.integer(1) != Some(1) {
                continue;
            }
            let Some(name) = record.string(2).filter(|name| !name.is_empty()) else {
                continue;
            };
            let name = if let Some(name) = ctx.get_btree_map(
                &body_names,
                pointer,
                "iges body property name cache lookup",
            )? {
                *name
            } else {
                let name = if ctx.all_by(
                    name,
                    |byte| Ok(byte.is_ascii_graphic() || *byte == b' '),
                    "iges body property name characters",
                )? {
                    ctx.validate_utf8(name, "iges body property name validation")?
                        .ok()
                } else {
                    None
                };
                scratch.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut body_names,
                        *pointer,
                        name,
                        "iges body property name cache",
                    )
                })?;
                name
            };
            let Some(name) = name else {
                continue;
            };
            match first {
                Some((first_sequence, first_name))
                    if *pointer != first_sequence
                        && (name.len() != first_name.len()
                            || name_texts.id(
                                first_sequence,
                                first_name.as_bytes(),
                                ctx,
                                "iges body property name agreement",
                            )? != name_texts.id(
                                *pointer,
                                name.as_bytes(),
                                ctx,
                                "iges body property name agreement",
                            )?) =>
                {
                    conflicting = true;
                    break;
                }
                None => first = Some((*pointer, name)),
                Some(_) => {}
            }
        }
        if conflicting {
            if let Some(entry) = ctx.get_btree_map(
                entries,
                &sequence,
                "iges body name owner lookup",
            )? {
                super::push_attributed_loss_with_scoped_slots(
                    ctx,
                    &mut loss_slots_storage,
                    &mut losses,
                    entry,
                    IgesLossCode::BodyNameAmbiguous,
                    format_args!(
                        "IGES body owner D{sequence} has conflicting valid Type 406 Form 15 names"
                    ),
                )?;
            }
        } else {
            body.name = match first {
                Some((_, name)) => Some(ctx.copy_retained_text(name, "iges body property name")?),
                None => None,
            };
        }
    }

    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(0..ir.model.faces.len());
    while source_values.len() != 0 {
        let Some(index) = ctx.next_charged(&mut source_values, "iges face display traversal")? else {
            break;
        };
        let face = &ir.model.faces[index];
        let Some(sequence) = sequences.face(&face.id, ctx)? else {
            continue;
        };
        let Some((sequence, color_number)) = ctx
            .get_btree_map(
                entries,
                &sequence,
                "iges presentation face directory lookup",
            )?
            .map(|entry| (sequence, entry.color))
        else {
            continue;
        };
        let Some((appearance_id, color)) = resolve(color_number)? else {
            continue;
        };
        let face = &mut ir.model.faces[index];
        let face_id = face
            .id
            .try_clone_for_decode(ctx, "iges appearance face ID copy")?;
        face.color = Some(color);
        appearance(
            ir,
            Cow::Borrowed(&appearance_id),
            None,
            color,
            ctx,
            (&mut appearances, &mut scratch),
        )?;
        ctx.reserve_vec(
            &mut ir.model.appearance_bindings,
            1,
            "iges appearance binding slots",
        )?;
        ctx.charge_entities(1, "iges_geometry_presentation")?;
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: crate::ids::appearance_binding_admitted(
                &crate::ids::Stem::word_directory(crate::ids::Word::Face, sequence),
                ctx,
            )?,
            target: AppearanceTarget::Face(face_id),
            appearance: appearance_id,
            source_entity_id: None,
            object_type: Some(
                ctx.format_retained(format_args!("Face"), "iges appearance object type")?,
            ),
            visible: None,
            channels: BTreeMap::new(),
        });
    }

    Ok(ProjectionOutcome { decoded, decoded_storage, losses, loss_slots_storage })
}

#[cfg(test)]
mod tests;
