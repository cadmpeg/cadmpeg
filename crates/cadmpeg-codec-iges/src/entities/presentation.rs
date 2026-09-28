// SPDX-License-Identifier: Apache-2.0
//! Directory display attributes and color definitions.

use super::geometry::ProjectionOutcome;
use super::{mirror_flag_valid, push_attributed_loss, vertical_text_flag_valid};
use crate::decode_resource::{
    format_retained, insert_optional_btree_map, insert_optional_btree_set, reserve_vec_growth,
};
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
    let Ok(value) = std::str::from_utf8(bytes) else {
        return Ok(None);
    };
    Ok(Some(format_retained(
        ctx,
        format_args!("{value}"),
        operation,
    )?))
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
    name: Option<String>,
    color: Color,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if ir.model.appearances.iter().all(|item| item.id != id) {
        reserve_vec_growth(
            ctx,
            &mut ir.model.appearances,
            1,
            "iges neutral appearance slots",
        )?;
        crate::decode_resource::admit_optional_entities(
            Some(ctx),
            1,
            "iges_geometry_presentation",
        )?;
        ir.model.appearances.push(Appearance {
            id,
            name,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: Some(format_retained(
                ctx,
                format_args!("IGES color"),
                "iges appearance schema",
            )?),
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
) -> Option<TextFontDefinition> {
    let parameter_end = record.parameter_end();
    let directory_valid = entry.status.subordinate() == Some(Subordinate::Independent)
        && entry.status.use_flag(global_table) == Some(UseFlag::Definition);
    if !directory_valid
        || record.integer(1).is_none_or(|value| value < 0)
        || record.string(2).is_none_or(<[u8]>::is_empty)
        || record.integer(4).is_none_or(|scale| scale <= 0)
    {
        return None;
    }
    let supersedes = match record.value(3) {
        None | Some(TokenValue::Omitted) => None,
        Some(TokenValue::Integer(value)) if *value >= 0 => None,
        Some(TokenValue::Integer(value)) => value
            .checked_neg()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|sequence| sequence % 2 == 1)
            .filter(|sequence| {
                entries
                    .get(sequence)
                    .is_some_and(|target| target.entity_type == 310 && target.form == 0)
            }),
        Some(TokenValue::Real(_) | TokenValue::String(_)) => return None,
    };
    if record.integer(3).is_some_and(|value| value < 0) && supersedes.is_none() {
        return None;
    }
    let count = record.count(5).filter(|count| *count > 0)?;
    let mut cursor = 6;
    let mut character_codes = 0_u128;
    for _ in 0..count {
        let character_code = record
            .integer(cursor)
            .filter(|value| matches!(value, 0..=127))?;
        let mask = 1_u128.checked_shl(u32::try_from(character_code).ok()?)?;
        if character_codes & mask != 0 {
            return None;
        }
        character_codes |= mask;
        record.integer(cursor + 1)?;
        record.integer(cursor + 2)?;
        let motion_count = record.count(cursor + 3)?;
        cursor += 4;
        for _ in 0..motion_count {
            record
                .integer_or(cursor, 0)
                .filter(|value| matches!(value, 0..=1))?;
            record.integer(cursor + 1)?;
            record.integer(cursor + 2)?;
            cursor += 3;
        }
    }
    (cursor == parameter_end).then_some(TextFontDefinition { supersedes })
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    trailing_pointer_analysis: &BTreeMap<u32, TrailingPointerAnalysis>,
    global: &ProjectedGlobal,
    ctx: &DecodeContext<'_>,
    sequences: &super::geometry::SourceSequences,
) -> Result<ProjectionOutcome, CodecError> {
    let mut records = BTreeMap::new();
    for record in parameters {
        insert_optional_btree_map(
            Some(ctx),
            &mut records,
            record.directory_sequence,
            record,
            "iges presentation parameter index",
        )?;
    }
    let mut entries = BTreeMap::new();
    for entry in directory {
        insert_optional_btree_map(
            Some(ctx),
            &mut entries,
            entry.sequence,
            entry,
            "iges presentation directory index",
        )?;
    }
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut defined = BTreeMap::new();
    let mut text_fonts = BTreeMap::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 310 && entry.form == 0)
    {
        if let Some(font) = records
            .get(&entry.sequence)
            .copied()
            .and_then(|record| text_font_definition(entry, record, &entries, global.global_table()))
        {
            insert_optional_btree_map(
                Some(ctx),
                &mut text_fonts,
                entry.sequence,
                font,
                "iges presentation font index",
            )?;
        }
    }
    let mut visited_fonts = BTreeSet::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 310 && entry.form == 0)
    {
        let cyclic =
            super::directed_cycle(entry.sequence, &mut visited_fonts, Some(ctx), |sequence| {
                text_fonts
                    .get(&sequence)
                    .and_then(|font| font.supersedes)
                    .into_iter()
            })?;
        let target_valid = text_fonts.get(&entry.sequence).is_some_and(|font| {
            font.supersedes
                .is_none_or(|target| text_fonts.contains_key(&target))
        });
        if target_valid && !cyclic {
            insert_optional_btree_set(
                Some(ctx),
                &mut decoded,
                entry.sequence,
                "iges presentation decoded sequences",
            )?;
        } else {
            push_presentation_loss(ctx, &mut losses, entry, "font header, superseded-font chain, character grammar, pen motions, or Directory fields are invalid")?;
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 312 && matches!(entry.form, 0..=1))
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            push_presentation_loss(ctx, &mut losses, entry, "Parameter Data record is missing")?;
            continue;
        };
        let parameter_end = record.parameter_end();
        let font = record.integer_or(3, 1);
        let font_valid = font.is_some_and(|font| {
            general_note_font_valid_for_global_table(font, &entries, global.global_table())
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
            insert_optional_btree_set(
                Some(ctx),
                &mut decoded,
                entry.sequence,
                "iges presentation decoded sequences",
            )?;
        } else {
            push_presentation_loss(ctx, &mut losses, entry, "text-template metrics, font, orientation, placement, or Directory fields are invalid")?;
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 406 && entry.form == 1)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            push_presentation_loss(ctx, &mut losses, entry, "Parameter Data record is missing")?;
            continue;
        };
        let levels_valid = if let Some(count) = record.count(1).filter(|count| *count > 0) {
            let mut levels = BTreeSet::new();
            let mut valid = true;
            for index in 0..count {
                let Some(level) = record.integer(2 + index).filter(|level| *level >= 0) else {
                    valid = false;
                    break;
                };
                if !insert_optional_btree_set(
                    Some(ctx),
                    &mut levels,
                    level,
                    "iges presentation definition levels",
                )? {
                    valid = false;
                    break;
                }
            }
            valid
        } else {
            false
        };
        if levels_valid {
            insert_optional_btree_set(
                Some(ctx),
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

    for entry in directory
        .iter()
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
            count.is_some_and(|count| {
                let expected_digits = count.div_ceil(4);
                (0..count).all(|index| {
                    record
                        .number(2 + index)
                        .is_some_and(|value| value.is_finite() && value > 0.0)
                }) && record.string(2 + count).is_some_and(|pattern| {
                    pattern.len() == expected_digits
                        && pattern.iter().all(u8::is_ascii_hexdigit)
                        && u8::from_str_radix(
                            std::str::from_utf8(&pattern[..1]).unwrap_or_default(),
                            16,
                        )
                        .is_ok_and(|first| first < (1_u8 << (4 - (expected_digits * 4 - count))))
                })
            })
        };
        if valid {
            insert_optional_btree_set(
                Some(ctx),
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

    for entry in directory
        .iter()
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
                Some(bytes) => retained_utf8(ctx, bytes, "iges color definition name")?,
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
        let Some(color) = Color::new(
            (red / 100.0) as f32,
            (green / 100.0) as f32,
            (blue / 100.0) as f32,
            1.0,
        ) else {
            push_presentation_loss(
                ctx,
                &mut losses,
                entry,
                "color definition components are outside [0, 100]",
            )?;
            continue;
        };
        insert_optional_btree_map(
            Some(ctx),
            &mut defined,
            entry.sequence,
            color,
            "iges presentation defined colors",
        )?;
        appearance(
            ir,
            crate::ids::appearance_color_admitted(
                &crate::ids::Stem::directory(entry.sequence),
                ctx,
            )?,
            name,
            color,
            ctx,
        )?;
        insert_optional_btree_set(
            Some(ctx),
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

    for entry in directory.iter().filter(|entry| {
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
    for entry in directory.iter().filter(|entry| entry.level < 0) {
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
    for entry in directory.iter().filter(|entry| {
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

    for curve in &mut ir.model.curves {
        if let Some(source) = &mut curve.source_object {
            source.color = sequences
                .curve(&curve.id)
                .and_then(|sequence| entries.get(&sequence))
                .and_then(|entry| resolve_color(entry.color));
        }
    }
    for surface in &mut ir.model.surfaces {
        if let Some(source) = &mut surface.source_object {
            source.color = sequences
                .surface(&surface.id)
                .and_then(|sequence| entries.get(&sequence))
                .and_then(|entry| resolve_color(entry.color));
        }
    }

    for index in 0..ir.model.bodies.len() {
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
        let body_id = crate::decode_resource::clone_optional_identity(
            Some(ctx),
            &body.id,
            "iges appearance body ID copy",
        )?;
        body.color = Some(color);
        body.visible = Some(visible);
        appearance(
            ir,
            crate::decode_resource::clone_optional_identity(
                Some(ctx),
                &appearance_id,
                "iges appearance ID copy",
            )?,
            None,
            color,
            ctx,
        )?;
        reserve_vec_growth(
            ctx,
            &mut ir.model.appearance_bindings,
            1,
            "iges appearance binding slots",
        )?;
        crate::decode_resource::admit_optional_entities(
            Some(ctx),
            1,
            "iges_geometry_presentation",
        )?;
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: crate::ids::appearance_binding_admitted(
                &crate::ids::Stem::word_directory(crate::ids::Word::Body, sequence),
                ctx,
            )?,
            target: AppearanceTarget::Body(body_id),
            appearance: appearance_id,
            source_entity_id: None,
            object_type: Some(format_retained(
                ctx,
                format_args!("Body"),
                "iges appearance object type",
            )?),
            visible: None,
            channels: BTreeMap::new(),
        });
    }
    for body in &mut ir.model.bodies {
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
        let mut names = groups.properties().iter().filter_map(|pointer| {
            entries
                .get(pointer)
                .filter(|entry| entry.entity_type == 406 && entry.form == 15)?;
            let record = records.get(pointer)?;
            (record.integer(1) == Some(1))
                .then(|| record.string(2))
                .flatten()
                .filter(|name| !name.is_empty())
                .filter(|name| {
                    name.iter()
                        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
                })
                .and_then(|name| std::str::from_utf8(name).ok())
        });
        let first = names.next();
        if first.is_some_and(|first| names.any(|name| name != first)) {
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
                Some(name) => retained_utf8(ctx, name.as_bytes(), "iges body property name")?,
                None => None,
            };
        }
    }

    for index in 0..ir.model.faces.len() {
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
        let face_id = crate::decode_resource::clone_optional_identity(
            Some(ctx),
            &face.id,
            "iges appearance face ID copy",
        )?;
        face.color = Some(color);
        appearance(
            ir,
            crate::decode_resource::clone_optional_identity(
                Some(ctx),
                &appearance_id,
                "iges appearance ID copy",
            )?,
            None,
            color,
            ctx,
        )?;
        reserve_vec_growth(
            ctx,
            &mut ir.model.appearance_bindings,
            1,
            "iges appearance binding slots",
        )?;
        crate::decode_resource::admit_optional_entities(
            Some(ctx),
            1,
            "iges_geometry_presentation",
        )?;
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: crate::ids::appearance_binding_admitted(
                &crate::ids::Stem::word_directory(crate::ids::Word::Face, sequence),
                ctx,
            )?,
            target: AppearanceTarget::Face(face_id),
            appearance: appearance_id,
            source_entity_id: None,
            object_type: Some(format_retained(
                ctx,
                format_args!("Face"),
                "iges appearance object type",
            )?),
            visible: None,
            channels: BTreeMap::new(),
        });
    }

    Ok(ProjectionOutcome { decoded, losses })
}

#[cfg(test)]
mod tests;
