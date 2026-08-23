// SPDX-License-Identifier: Apache-2.0
//! Directory display attributes and color definitions.

use super::geometry::ProjectionOutcome;
use crate::directory::DirectoryEntry;
use crate::global::{Dialect, ProjectedGlobal};
use crate::loss::IgesLossCode;
use crate::parameter::{ParameterRecord, TokenValue};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::ids::AppearanceId;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
struct TextFontDefinition {
    supersedes: Option<u32>,
}

fn loss(entry: &DirectoryEntry, message: impl Into<String>) -> LossNote {
    IgesLossCode::DisplayDataNotProjected
        .note(format!(
            "IGES entity type {} form {} display data was not projected: {}",
            entry.entity_type,
            entry.form,
            message.into()
        ))
        .with_provenance(entry.loss_provenance())
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
    Some(Color { r, g, b, a: 1.0 })
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

pub(super) fn general_note_font_valid_for_dialect(
    value: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    dialect: Dialect,
) -> bool {
    let standard = match dialect {
        Dialect::V4_0 => {
            matches!(
                value,
                0 | 1 | 2 | 3 | 6 | 12 | 13 | 14 | 17 | 18 | 19 | 1001..=1003
            )
        }
        Dialect::V5_0 => matches!(
            value,
            0 | 1 | 2 | 3 | 6 | 12 | 13 | 14 | 17 | 18 | 19 | 1001..=1003 | 2001
        ),
        Dialect::Legacy | Dialect::V5_1 | Dialect::V5_2 | Dialect::V5_3 => matches!(
            value,
            0 | 1 | 2 | 3 | 6 | 12 | 13 | 14 | 17 | 18 | 19 | 1001..=1003 | 2001 | 3001
        ),
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

fn mirror_flag_valid(value: i64) -> bool {
    matches!(value, 0..=2)
}

fn vertical_text_flag_valid(value: i64) -> bool {
    matches!(value, 0..=1)
}

fn source_sequence(id: &str) -> Option<u32> {
    let marker = id.rfind("#D").into_iter().chain(id.rfind(":D")).max()? + 2;
    let digits = id[marker..].bytes().take_while(u8::is_ascii_digit).count();
    id.get(marker..marker.checked_add(digits)?)?.parse().ok()
}

fn appearance(ir: &mut CadIr, id: AppearanceId, name: Option<String>, color: Color) {
    if ir.model.appearances.iter().all(|item| item.id != id) {
        ir.model.appearances.push(Appearance {
            id,
            name,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("IGES color".into()),
            category: None,
            base_color: Some(color),
            properties: BTreeMap::new(),
            textures: Vec::new(),
        });
    }
}

fn text_font_definition(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> Option<TextFontDefinition> {
    let parameter_end = record.parameter_end();
    let directory_valid = entry.status.use_flag == 2
        && entry.structure == 0
        && entry.line_font == 0
        && entry.level == 0
        && entry.view == 0
        && entry.transform == 0
        && entry.label_display == 0
        && entry.line_weight == 0
        && entry.color == 0;
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
    let mut character_codes = BTreeSet::new();
    for _ in 0..count {
        let character_code = record
            .integer(cursor)
            .filter(|value| matches!(value, 0..=127))?;
        if !character_codes.insert(character_code) {
            return None;
        }
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
    global: &ProjectedGlobal,
    _ctx: Option<&DecodeContext<'_>>,
) -> ProjectionOutcome {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut defined = BTreeMap::new();
    let mut names = BTreeMap::new();
    let text_fonts = directory
        .iter()
        .filter(|entry| entry.entity_type == 310 && entry.form == 0)
        .filter_map(|entry| {
            let record = records.get(&entry.sequence).copied()?;
            text_font_definition(entry, record, &entries).map(|font| (entry.sequence, font))
        })
        .collect::<BTreeMap<_, _>>();
    let mut visited_fonts = BTreeSet::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 310 && entry.form == 0)
    {
        let cyclic = super::directed_cycle(entry.sequence, &mut visited_fonts, |sequence| {
            text_fonts
                .get(&sequence)
                .and_then(|font| font.supersedes)
                .into_iter()
                .collect()
        });
        let target_valid = text_fonts.get(&entry.sequence).is_some_and(|font| {
            font.supersedes
                .is_none_or(|target| text_fonts.contains_key(&target))
        });
        if target_valid && !cyclic {
            decoded.insert(entry.sequence);
        } else {
            losses.push(loss(
                entry,
                "font header, superseded-font chain, character grammar, pen motions, or Directory fields are invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 312 && matches!(entry.form, 0..=1))
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let parameter_end = record.parameter_end();
        let font = record.integer_or(3, 1);
        let font_valid = font.is_some_and(|font| {
            general_note_font_valid_for_dialect(font, &entries, global.dialect())
        });
        let directory_valid = entry.status.use_flag == 2
            && entry.structure == 0
            && entry.line_font == 0
            && entry.view == 0
            && entry.transform == 0
            && entry.label_display == 0
            && entry.line_weight == 0;
        let fields_valid = parameter_end <= 11
            && (1..=2).all(|index| {
                record
                    .number_or(index, 0.0)
                    .is_some_and(|value| value.is_finite() && value >= 0.0)
            })
            && font_valid
            && record
                .number_or(4, std::f64::consts::FRAC_PI_2)
                .is_some_and(f64::is_finite)
            && record.number_or(5, 0.0).is_some_and(f64::is_finite)
            && record.integer_or(6, 0).is_some_and(mirror_flag_valid)
            && record
                .integer_or(7, 0)
                .is_some_and(vertical_text_flag_valid)
            && (8..=10).all(|index| record.number_or(index, 0.0).is_some_and(f64::is_finite));
        if directory_valid && fields_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(loss(
                entry,
                "text-template metrics, font, orientation, placement, or Directory fields are invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 406 && entry.form == 1)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let levels = record
            .count(1)
            .filter(|count| *count > 0)
            .and_then(|count| {
                (0..count)
                    .map(|index| record.integer(2 + index).filter(|level| *level >= 0))
                    .collect::<Option<Vec<_>>>()
            });
        if levels.is_some_and(|levels| levels.iter().collect::<BTreeSet<_>>().len() == levels.len())
        {
            decoded.insert(entry.sequence);
        } else {
            losses.push(loss(
                entry,
                "definition-level count, value, or uniqueness is invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 304 && matches!(entry.form, 1 | 2))
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(loss(entry, "Parameter Data record is missing"));
            continue;
        };
        if entry.status.use_flag != 2 || !(1..=5).contains(&entry.line_font) {
            losses.push(loss(
                entry,
                "line-font definition use flag or fallback pattern is invalid",
            ));
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
            decoded.insert(entry.sequence);
        } else {
            losses.push(loss(entry, "line-font definition parameters are invalid"));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 314 && entry.form == 0)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(components) = (1..=3)
            .map(|index| {
                record
                    .number(index)
                    .filter(|value| (0.0..=100.0).contains(value))
            })
            .collect::<Option<Vec<_>>>()
        else {
            losses.push(loss(entry, "RGB percentage is outside 0 through 100"));
            continue;
        };
        let name = match record.value(4) {
            None | Some(crate::parameter::TokenValue::Omitted) => None,
            Some(crate::parameter::TokenValue::String(_)) => record
                .string(4)
                .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok()),
            Some(crate::parameter::TokenValue::Integer(0))
                if matches!(global.dialect(), Dialect::V4_0) =>
            {
                None
            }
            Some(
                crate::parameter::TokenValue::Integer(_) | crate::parameter::TokenValue::Real(_),
            ) => {
                losses.push(loss(entry, "optional color name is not a string"));
                continue;
            }
        };
        let color = Color {
            r: (components[0] / 100.0) as f32,
            g: (components[1] / 100.0) as f32,
            b: (components[2] / 100.0) as f32,
            a: 1.0,
        };
        defined.insert(entry.sequence, color);
        names.insert(entry.sequence, name.clone());
        appearance(
            ir,
            AppearanceId(format!("iges:appearance:color#D{}", entry.sequence)),
            name,
            color,
        );
        decoded.insert(entry.sequence);
    }

    let resolve = |value: i64| -> Option<(AppearanceId, Color)> {
        match value.cmp(&0) {
            std::cmp::Ordering::Greater => Some((
                AppearanceId(format!("iges:appearance:standard#{value}")),
                standard_color(value)?,
            )),
            std::cmp::Ordering::Less => {
                let sequence = u32::try_from(value.checked_neg()?).ok()?;
                let entry = entries.get(&sequence)?;
                if entry.entity_type != 314 || entry.form != 0 {
                    return None;
                }
                Some((
                    AppearanceId(format!("iges:appearance:color#D{sequence}")),
                    *defined.get(&sequence)?,
                ))
            }
            std::cmp::Ordering::Equal => None,
        }
    };

    for entry in directory.iter().filter(|entry| entry.color != 0) {
        if resolve(entry.color).is_none() {
            losses.push(loss(
                entry,
                "Directory color number or definition pointer is invalid",
            ));
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
            losses.push(loss(
                entry,
                "negative Directory level does not reference a decoded Definition Levels property",
            ));
        }
    }
    for entry in directory.iter().filter(|entry| entry.line_weight != 0) {
        if !global.line_weight_number_is_valid(entry.line_weight) {
            losses.push(loss(
                entry,
                "line-weight number is outside the Global gradation range",
            ));
        }
    }

    for curve in &mut ir.model.curves {
        if let Some(source) = &mut curve.source_object {
            source.color = source_sequence(&source.object_id)
                .and_then(|sequence| entries.get(&sequence))
                .and_then(|entry| resolve(entry.color))
                .map(|(_, color)| color);
        }
    }
    for surface in &mut ir.model.surfaces {
        if let Some(source) = &mut surface.source_object {
            source.color = source_sequence(&source.object_id)
                .and_then(|sequence| entries.get(&sequence))
                .and_then(|entry| resolve(entry.color))
                .map(|(_, color)| color);
        }
    }

    let body_assignments = ir
        .model
        .bodies
        .iter()
        .filter_map(|body| {
            let sequence = source_sequence(&body.id.0)?;
            let entry = entries.get(&sequence)?;
            resolve(entry.color)
                .map(|appearance| (body.id.clone(), sequence, appearance, entry.status.blank))
        })
        .collect::<Vec<_>>();
    for (body_id, sequence, (appearance_id, color), blank) in body_assignments {
        appearance(ir, appearance_id.clone(), None, color);
        let Some(body) = ir.model.bodies.iter_mut().find(|body| body.id == body_id) else {
            continue;
        };
        body.color = Some(color);
        body.visible = Some(blank == 0);
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: format!("iges:model:appearance-binding#body-D{sequence}"),
            target: AppearanceTarget::Body(body_id),
            appearance: appearance_id,
            source_entity_id: None,
            object_type: Some("Body".into()),
            visible: None,
            channels: BTreeMap::new(),
        });
    }
    for body in &mut ir.model.bodies {
        if body.visible.is_none() {
            body.visible = source_sequence(&body.id.0)
                .and_then(|sequence| entries.get(&sequence))
                .map(|entry| entry.status.blank == 0);
        }
    }

    let face_assignments = ir
        .model
        .faces
        .iter()
        .filter_map(|face| {
            let sequence = source_sequence(&face.id.0)?;
            let entry = entries.get(&sequence)?;
            resolve(entry.color).map(|appearance| (face.id.clone(), sequence, appearance))
        })
        .collect::<Vec<_>>();
    for (face_id, sequence, (appearance_id, color)) in face_assignments {
        appearance(ir, appearance_id.clone(), None, color);
        let Some(face) = ir.model.faces.iter_mut().find(|face| face.id == face_id) else {
            continue;
        };
        face.color = Some(color);
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: format!("iges:model:appearance-binding#face-D{sequence}"),
            target: AppearanceTarget::Face(face_id),
            appearance: appearance_id,
            source_entity_id: None,
            object_type: Some("Face".into()),
            visible: None,
            channels: BTreeMap::new(),
        });
    }

    ProjectionOutcome { decoded, losses }
}

#[cfg(test)]
mod tests;
