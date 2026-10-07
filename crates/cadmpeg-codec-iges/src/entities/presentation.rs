// SPDX-License-Identifier: Apache-2.0
//! Directory display attributes and color definitions.

use super::geometry::ProjectionOutcome;
use super::{mirror_flag_valid, push_attributed_loss, vertical_text_flag_valid};

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
    Ok(Some(
        ctx.copy_retained_text(value, operation)?,
    ))
}

fn push_presentation_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    entry: &DirectoryEntry,
    reason: &str,
) -> Result<(), CodecError> {
    push_attributed_loss(
        ctx,
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
) -> bool {
    value
        .checked_neg()
        .and_then(|value| u32::try_from(value).ok())
        .is_some_and(|sequence| {
            sequence % 2 == 1
                && entries
                    .get(&sequence)
                    .is_some_and(|entry| entry.entity_type == 310 && entry.form == 0)
        })
}

pub(super) fn general_note_font_valid_for_global_table(
    value: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    global_table: GlobalTable,
) -> bool {
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
    standard || text_font_definition_pointer_valid(value, entries)
}

pub(super) fn new_general_note_font_valid(value: i64) -> bool {
    matches!(value, 1 | 2 | 3 | 6 | 12 | 13 | 14 | 17 | 18 | 19)
}

pub(super) fn new_general_note_charset_valid(
    value: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    matches!(value, 1 | 1001 | 1002 | 1003 | 2001 | 3001)
        || text_font_definition_pointer_valid(value, entries)
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
    id: AppearanceId,
    name: Option<&[u8]>,
    color: Color,
    ctx: &DecodeContext<'_>,
    (identities, storage): (&mut Option<BTreeSet<String>>, &mut cadmpeg_core::decode::ScopedReservation<'_>),
) -> Result<(), CodecError> {
    if identities.is_none() {
        let mut index = BTreeSet::new();
        for item in ctx.admit_iter(&ir.model.appearances, "iges appearance index traversal")? {
            let key = ctx.copy_scoped_text(item.id.as_str(), storage, "iges appearance index keys")?;
            storage.with_storage(|| ctx.insert_btree_set(&mut index, key, "iges appearance index nodes"))?;
        }
        *identities = Some(index);
    }
    let index = identities.as_mut().ok_or_else(|| CodecError::malformed("IGES appearance index is absent"))?;
    if !ctx.contains_btree_set(index, id.as_str(), "iges appearance index lookup")? {
        let key = ctx.copy_scoped_text(id.as_str(), storage, "iges appearance index keys")?;
        storage.with_storage(|| ctx.insert_btree_set(index, key, "iges appearance index nodes"))?;
        let name = name.map(|name| retained_utf8(ctx, name, "iges color definition name")).transpose()?.flatten();
        ctx.reserve_vec(
            &mut ir.model.appearances,
            1,
            "iges neutral appearance slots",
        )?;
        ctx.charge_entities(1, "iges_geometry_presentation")?;
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
    if !directory_valid || record.integer(1).is_none_or(|value| value < 0)
        || record.string(2).is_none_or(<[u8]>::is_empty)
        || record.integer(4).is_none_or(|scale| scale <= 0) {
        return Ok(None);
    }
    let supersedes = match record.value(3) {
        None | Some(TokenValue::Omitted) => None,
        Some(TokenValue::Integer(value)) if *value >= 0 => None,
        Some(TokenValue::Integer(value)) => value.checked_neg()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|sequence| sequence % 2 == 1)
            .filter(|sequence| entries.get(sequence).is_some_and(|target| target.entity_type == 310 && target.form == 0)),
        Some(TokenValue::Real(_) | TokenValue::String(_)) => return Ok(None),
    };
    if record.integer(3).is_some_and(|value| value < 0) && supersedes.is_none() { return Ok(None); }
    let Some(count) = record.count(5).filter(|count| *count > 0) else { return Ok(None); };
    let mut cursor = 6;
    let mut character_codes = 0_u128;
    let mut characters = 0..count;
    while ctx.next_charged(&mut characters, "iges text font character traversal")?.is_some() {
        let Some(code) = record.integer(cursor).filter(|value| matches!(value, 0..=127)) else { return Ok(None); };
        let Some(mask) = u32::try_from(code).ok().and_then(|code| 1_u128.checked_shl(code)) else { return Ok(None); };
        if character_codes & mask != 0 { return Ok(None); }
        character_codes |= mask;
        if record.integer(cursor + 1).is_none() || record.integer(cursor + 2).is_none() { return Ok(None); }
        let Some(count) = record.count(cursor + 3) else { return Ok(None); };
        cursor += 4;
        let mut motions = 0..count;
        while ctx.next_charged(&mut motions, "iges text font motion traversal")?.is_some() {
            if record.integer_or(cursor, 0).is_none_or(|value| !matches!(value, 0..=1))
                || record.integer(cursor + 1).is_none() || record.integer(cursor + 2).is_none() {
                return Ok(None);
            }
            cursor += 3;
        }
    }
    Ok((cursor == parameter_end).then_some(TextFontDefinition { supersedes }))
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    (entries, records): (&BTreeMap<u32, &DirectoryEntry>, &BTreeMap<u32, &ParameterRecord>),
    trailing_pointer_analysis: &BTreeMap<u32, TrailingPointerAnalysis>,
    global: &ProjectedGlobal,
    ctx: &DecodeContext<'_>,
    sequences: &super::geometry::SourceSequences,
) -> Result<ProjectionOutcome, CodecError> {
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut scratch = ctx.reserve_scoped(0, "iges presentation scratch")?;
    let mut appearances = None;
    let mut defined = BTreeMap::new();
    let mut text_fonts = BTreeMap::new();
    for entry in ctx.admit_iter(directory, "iges presentation directory traversal")?
        .filter(|entry| entry.entity_type == 310 && entry.form == 0)
    {
        if let Some(font) = records
            .get(&entry.sequence)
            .copied()
            .map(|record| text_font_definition(entry, record, entries, global.global_table(), ctx)).transpose()?.flatten()
        {
            scratch.with_storage(|| ctx.insert_btree_map(
                &mut text_fonts,
                entry.sequence,
                font,
                "iges presentation font index",
            ))?;
        }
    }
    let mut cyclic_fonts = BTreeMap::new();

    for entry in ctx.admit_iter(directory, "iges presentation directory traversal")?
        .filter(|entry| entry.entity_type == 310 && entry.form == 0)
    {
        let mut cycle_storage = ctx.reserve_scoped(0, "iges font cycle scratch")?;
        let mut active = BTreeSet::new();
        let mut chain = std::iter::successors(Some(entry.sequence), |sequence| text_fonts.get(sequence).and_then(|font| font.supersedes));
        let cyclic = loop {
            let Some(sequence) = ctx.next_charged(&mut chain, "iges font cycle traversal")? else { break false; };
            if let Some(cyclic) = cyclic_fonts.get(&sequence) { break *cyclic; }
            if !cycle_storage.with_storage(|| ctx.insert_btree_set(&mut active, sequence, "iges font cycle active"))? { break true; }
        };
        for sequence in ctx.admit_iter(active, "iges font cycle result traversal")? {
            scratch.with_storage(|| ctx.insert_btree_map(&mut cyclic_fonts, sequence, cyclic, "iges font cycle results"))?;
        }
        let target_valid = text_fonts.get(&entry.sequence).is_some_and(|font| {
            font.supersedes
                .is_none_or(|target| text_fonts.contains_key(&target))
        });
        if target_valid && !cyclic {
            ctx.insert_btree_set(
                &mut decoded,
                entry.sequence,
                "iges presentation decoded sequences",
            )?;
        } else {
            push_presentation_loss(ctx, &mut losses, entry, "font header, superseded-font chain, character grammar, pen motions, or Directory fields are invalid")?;
        }
    }

    for entry in ctx.admit_iter(directory, "iges presentation directory traversal")?
        .filter(|entry| entry.entity_type == 312 && matches!(entry.form, 0..=1))
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            push_presentation_loss(ctx, &mut losses, entry, "Parameter Data record is missing")?;
            continue;
        };
        let parameter_end = record.parameter_end();
        let font = record.integer_or(3, 1);
        let font_valid = font.is_some_and(|font| {
            general_note_font_valid_for_global_table(font, entries, global.global_table())
        });
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
            ctx.insert_btree_set(
                &mut decoded,
                entry.sequence,
                "iges presentation decoded sequences",
            )?;
        } else {
            push_presentation_loss(ctx, &mut losses, entry, "text-template metrics, font, orientation, placement, or Directory fields are invalid")?;
        }
    }

    for entry in ctx.admit_iter(directory, "iges presentation directory traversal")?
        .filter(|entry| entry.entity_type == 406 && entry.form == 1)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            push_presentation_loss(ctx, &mut losses, entry, "Parameter Data record is missing")?;
            continue;
        };
        let levels_valid = if let Some(count) = record.count(1).filter(|count| *count > 0) {
            let mut level_storage = ctx.reserve_scoped(0, "iges presentation level scratch")?;
            let mut levels = BTreeSet::new();
            ctx.all_by(0..count, |index| {
                let Some(level) = record.integer(2 + index).filter(|level| *level >= 0) else {
                    return Ok(false);
                };
                level_storage.with_storage(|| ctx.insert_btree_set(
                    &mut levels,
                    level,
                    "iges presentation definition levels",
                ))
            }, "iges definition level traversal")?
        } else {
            false
        };
        if levels_valid {
            ctx.insert_btree_set(
                &mut decoded,
                entry.sequence,
                "iges presentation decoded sequences",
            )?;
        } else {
            push_presentation_loss(
                ctx,
                &mut losses,
                entry,
                "definition-level count, value, or uniqueness is invalid",
            )?;
        }
    }

    for entry in ctx.admit_iter(directory, "iges presentation directory traversal")?
        .filter(|entry| entry.entity_type == 304 && matches!(entry.form, 1 | 2))
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            push_presentation_loss(ctx, &mut losses, entry, "Parameter Data record is missing")?;
            continue;
        };
        if !line_font_definition_directory_valid(entry, global.global_table()) {
            push_presentation_loss(
                ctx,
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
                && template.is_some_and(|sequence| {
                    sequence % 2 == 1
                        && entries
                            .get(&sequence)
                            .is_some_and(|target| target.entity_type == 308 && target.form == 0)
                })
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
                ctx.all_by(0..count, |index| Ok(record.number(2 + index).is_some_and(|value| value.is_finite() && value > 0.0)), "iges line font segment traversal")?
                    && if let Some(pattern) = record.string(2 + count) {
                        pattern.len() == expected_digits
                            && ctx.all_by(pattern, |byte| Ok(byte.is_ascii_hexdigit()), "iges line font pattern validation")?
                            && {
                                let first = match pattern[0] {
                                    b'0'..=b'9' => pattern[0] - b'0',
                                    b'A'..=b'F' => pattern[0] - b'A' + 10,
                                    b'a'..=b'f' => pattern[0] - b'a' + 10,
                                    _ => 255,
                                };
                                first < (1_u8 << (4 - (expected_digits * 4 - count)))
                            }
                    } else { false }
            } else { false }
        };
        if valid {
            ctx.insert_btree_set(
                &mut decoded,
                entry.sequence,
                "iges presentation decoded sequences",
            )?;
        } else {
            push_presentation_loss(
                ctx,
                &mut losses,
                entry,
                "line-font definition parameters are invalid",
            )?;
        }
    }

    for entry in ctx.admit_iter(directory, "iges presentation directory traversal")?
        .filter(|entry| entry.entity_type == 314 && entry.form == 0)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            push_presentation_loss(ctx, &mut losses, entry, "Parameter Data record is missing")?;
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
                &mut losses,
                entry,
                "RGB percentage is outside 0 through 100",
            )?;
            continue;
        };
        let name = match record.value(4) {
            None | Some(crate::parameter::TokenValue::Omitted) => None,
            Some(crate::parameter::TokenValue::String(_)) => match record.string(4) {
                Some(bytes) => Some(bytes),
                None => None,
            },
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
                &mut losses,
                entry,
                "color definition components are outside [0, 100]",
            )?;
            continue;
        };
        scratch.with_storage(|| ctx.insert_btree_map(
            &mut defined,
            entry.sequence,
            color,
            "iges presentation defined colors",
        ))?;
        appearance(
            ir,
            crate::ids::appearance_color_admitted(
                &crate::ids::Stem::directory(entry.sequence),
                ctx,
            )?,
            name,
            color,
            ctx,
            (&mut appearances, &mut scratch),
        )?;
        ctx.insert_btree_set(
            &mut decoded,
            entry.sequence,
            "iges presentation decoded sequences",
        )?;
    }

    let resolve_color = |value: i64| -> Option<Color> {
        match value.cmp(&0) {
            std::cmp::Ordering::Greater => standard_color(value),
            std::cmp::Ordering::Less => {
                let sequence = u32::try_from(value.checked_neg()?).ok()?;
                let entry = entries.get(&sequence)?;
                if entry.entity_type != 314 || entry.form != 0 {
                    return None;
                }
                defined.get(&sequence).copied()
            }
            std::cmp::Ordering::Equal => None,
        }
    };
    let resolve = |value: i64| -> Result<Option<(AppearanceId, Color)>, CodecError> {
        let Some(color) = resolve_color(value) else {
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

    for entry in ctx.admit_iter(directory, "iges presentation directory traversal")?.filter(|entry| {
        entry.color != 0 && directory_color_is_semantic(entry, global.global_table())
    }) {
        if resolve_color(entry.color).is_none() {
            push_presentation_loss(
                ctx,
                &mut losses,
                entry,
                "Directory color number or definition pointer is invalid",
            )?;
        }
    }
    for entry in ctx.admit_iter(directory, "iges presentation directory traversal")?.filter(|entry| entry.level < 0) {
        let sequence = entry.level.unsigned_abs();
        if u32::try_from(sequence).ok().is_none_or(|sequence| {
            !decoded.contains(&sequence)
                || entries
                    .get(&sequence)
                    .is_none_or(|target| target.entity_type != 406 || target.form != 1)
        }) {
            push_presentation_loss(
                ctx,
                &mut losses,
                entry,
                "negative Directory level does not reference a decoded Definition Levels property",
            )?;
        }
    }
    for entry in ctx.admit_iter(directory, "iges presentation directory traversal")?.filter(|entry| {
        entry.line_weight != 0 && directory_line_weight_is_semantic(entry, global.global_table())
    }) {
        if !global.line_weight_number_is_valid(entry.line_weight) {
            push_presentation_loss(
                ctx,
                &mut losses,
                entry,
                "line-weight number is outside the Global gradation range",
            )?;
        }
    }

    for curve in ctx.admit_iter(&mut ir.model.curves, "iges curve display traversal")? {
        if let Some(source) = &mut curve.source_object {
            source.color = sequences
                .curve(&curve.id)
                .and_then(|sequence| entries.get(&sequence))
                .and_then(|entry| resolve_color(entry.color));
        }
    }
    for surface in ctx.admit_iter(&mut ir.model.surfaces, "iges surface display traversal")? {
        if let Some(source) = &mut surface.source_object {
            source.color = sequences
                .surface(&surface.id)
                .and_then(|sequence| entries.get(&sequence))
                .and_then(|entry| resolve_color(entry.color));
        }
    }

    for index in ctx.admit_iter(0..ir.model.bodies.len(), "iges body display traversal")? {
        let Some((sequence, color_number, visible)) = (|| {
            let body = &ir.model.bodies[index];
            let sequence = sequences.body(&body.id)?;
            let entry = entries.get(&sequence)?;
            Some((sequence, entry.color, entry.status.is_visible()))
        })() else {
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
            appearance_id.try_clone_for_decode(ctx, "iges appearance ID copy")?,
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
    for body in ctx.admit_iter(&mut ir.model.bodies, "iges body name traversal")? {
        if body.visible.is_none() {
            body.visible = sequences
                .body(&body.id)
                .and_then(|sequence| entries.get(&sequence))
                .map(|entry| entry.status.is_visible());
        }
        let Some(sequence) = sequences.body(&body.id) else {
            continue;
        };
        let Some(TrailingPointerAnalysis::Unambiguous(groups)) =
            trailing_pointer_analysis.get(&sequence)
        else {
            continue;
        };
        let mut first: Option<&str> = None;
        let mut conflicting = false;
        let mut properties = groups.properties().iter();
        while let Some(pointer) = ctx.next_charged(&mut properties, "iges body property traversal")? {
            if entries.get(pointer).is_none_or(|entry| entry.entity_type != 406 || entry.form != 15) { continue; }
            let Some(record) = records.get(pointer) else { continue; };
            if record.integer(1) != Some(1) { continue; }
            let Some(name) = record.string(2).filter(|name| !name.is_empty()) else { continue; };
            if !ctx.all_by(name, |byte| Ok(byte.is_ascii_graphic() || *byte == b' '), "iges body property name characters")? { continue; }
            let Ok(name) = ctx.validate_utf8(name, "iges body property name validation")? else { continue; };
            match first {
                Some(first) if !ctx.equal_bytes(name.as_bytes(), first.as_bytes(), "iges body property name agreement")? => {
                    conflicting = true;
                    break;
                }
                None => first = Some(name),
                Some(_) => {}
            }
        }
        if conflicting {
            if let Some(entry) = entries.get(&sequence) {
                push_attributed_loss(
                    ctx,
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
                Some(name) => Some(ctx.copy_retained_text(name, "iges body property name")?),
                None => None,
            };
        }
    }

    for index in ctx.admit_iter(0..ir.model.faces.len(), "iges face display traversal")? {
        let Some((sequence, color_number)) = (|| {
            let face = &ir.model.faces[index];
            let sequence = sequences.face(&face.id)?;
            let entry = entries.get(&sequence)?;
            Some((sequence, entry.color))
        })() else {
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
            appearance_id.try_clone_for_decode(ctx, "iges appearance ID copy")?,
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

    Ok(ProjectionOutcome { decoded, losses })
}

#[cfg(test)]
mod tests;
