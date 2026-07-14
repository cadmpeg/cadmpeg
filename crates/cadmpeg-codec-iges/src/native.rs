// SPDX-License-Identifier: Apache-2.0
//! Versioned `native.iges` physical cards and entity records.

use crate::card::CardScan;
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::graph::ReferenceEdge;
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::CadIr;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct NativeCard {
    id: String,
    offset: u64,
    payload: Vec<u8>,
    line_ending: Vec<u8>,
    section: Option<String>,
    sequence: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum NativeTokenValue {
    Omitted,
    Integer(i64),
    Real(f64),
    String(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeToken {
    start: usize,
    end: usize,
    value: NativeTokenValue,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeDirection {
    id: String,
    source_entity: String,
    components: Vec<Option<f64>>,
    physically_dependent: bool,
    has_transform: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeTransformation {
    id: String,
    source_entity: String,
    form: i64,
    coefficients: Vec<Option<f64>>,
    parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeCopiousData {
    id: String,
    source_entity: String,
    form: i64,
    interpretation: Option<i64>,
    declared_tuple_count: Option<i64>,
    common_z: Option<f64>,
    tuples: Vec<Vec<Option<f64>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeColorDefinition {
    id: String,
    source_entity: String,
    red_percent: Option<f64>,
    green_percent: Option<f64>,
    blue_percent: Option<f64>,
    name: Option<Vec<u8>>,
    fallback_color_number: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeDisplayAttributes {
    id: String,
    source_entity: String,
    visible: bool,
    line_font_number: i64,
    line_font_definition: Option<String>,
    level_number: i64,
    level_definition: Option<String>,
    view: i64,
    line_weight_number: i64,
    line_weight_mm: Option<f64>,
    color_number: i64,
    color_definition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NativeLineFontDefinition {
    Template {
        id: String,
        source_entity: String,
        fallback_line_font_number: i64,
        tangent_oriented: Option<bool>,
        template: Option<String>,
        spacing: Option<f64>,
        scale: Option<f64>,
    },
    VisibleBlankPattern {
        id: String,
        source_entity: String,
        fallback_line_font_number: i64,
        segment_count: Option<i64>,
        lengths: Vec<Option<f64>>,
        hexadecimal_pattern: Option<Vec<u8>>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeDefinitionLevels {
    id: String,
    source_entity: String,
    declared_count: Option<i64>,
    levels: Vec<Option<i64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct NativeEntity {
    id: String,
    directory_sequence: u32,
    entity_type: i64,
    form: i64,
    parameter_start: i64,
    parameter_line_count: i64,
    structure: i64,
    line_font: i64,
    level: i64,
    view: i64,
    transform: i64,
    label_display: i64,
    blank_status: u8,
    subordinate_status: u8,
    use_flag: u8,
    hierarchy_status: u8,
    line_weight: i64,
    color: i64,
    reserved: Vec<Vec<u8>>,
    label: Vec<u8>,
    subscript: i64,
    parameter_line_start: Option<u32>,
    parameter_line_end: Option<u32>,
    parameter_bytes: Vec<u8>,
    parameters: Vec<NativeToken>,
    comment: Vec<u8>,
    links: Vec<String>,
    references: Vec<ReferenceEdge>,
}

fn token(token: &Token) -> NativeToken {
    NativeToken {
        start: token.span.start,
        end: token.span.end,
        value: match &token.value {
            TokenValue::Omitted => NativeTokenValue::Omitted,
            TokenValue::Integer(value) => NativeTokenValue::Integer(*value),
            TokenValue::Real(value) => NativeTokenValue::Real(*value),
            TokenValue::String(value) => NativeTokenValue::String(value.clone()),
        },
    }
}

pub(crate) fn store(
    ir: &mut CadIr,
    scan: &CardScan,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    references: &BTreeMap<u32, Vec<ReferenceEdge>>,
    global: &Global,
) -> Result<(), CodecError> {
    let cards = scan
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| NativeCard {
            id: format!("iges:physical:card#{}", index + 1),
            offset: line.offset,
            payload: line.payload.clone(),
            line_ending: line.line_ending().to_vec(),
            section: line
                .section
                .map(|section| format!("{section:?}").to_lowercase()),
            sequence: line.sequence,
        })
        .collect::<Vec<_>>();
    let by_directory = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entities = directory
        .iter()
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            NativeEntity {
                id: format!("iges:entity:directory#{}", entry.sequence),
                directory_sequence: entry.sequence,
                entity_type: entry.entity_type,
                form: entry.form,
                parameter_start: entry.parameter_start,
                parameter_line_count: entry.parameter_line_count,
                structure: entry.structure,
                line_font: entry.line_font,
                level: entry.level,
                view: entry.view,
                transform: entry.transform,
                label_display: entry.label_display,
                blank_status: entry.status.blank,
                subordinate_status: entry.status.subordinate,
                use_flag: entry.status.use_flag,
                hierarchy_status: entry.status.hierarchy,
                line_weight: entry.line_weight,
                color: entry.color,
                reserved: entry.reserved.iter().map(|value| value.to_vec()).collect(),
                label: entry.label.to_vec(),
                subscript: entry.subscript,
                parameter_line_start: parameters.map(|record| record.line_range.start),
                parameter_line_end: parameters.map(|record| record.line_range.end),
                parameter_bytes: parameters
                    .map(|record| record.bytes.clone())
                    .unwrap_or_default(),
                parameters: parameters
                    .into_iter()
                    .flat_map(|record| record.tokens.iter().map(token))
                    .collect(),
                comment: parameters
                    .map(|record| record.comment.clone())
                    .unwrap_or_default(),
                links: references
                    .get(&entry.sequence)
                    .into_iter()
                    .flatten()
                    .filter_map(ReferenceEdge::target)
                    .map(str::to_owned)
                    .collect(),
                references: references.get(&entry.sequence).cloned().unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    let directions = directory
        .iter()
        .filter(|entry| entry.entity_type == 123 && entry.form == 0)
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            NativeDirection {
                id: format!("iges:native:direction#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                components: (1..=3)
                    .map(|index| parameters.and_then(|record| record.number(index)))
                    .collect(),
                physically_dependent: entry.status.subordinate == 1,
                has_transform: entry.transform != 0,
            }
        })
        .collect::<Vec<_>>();
    let transforms = directory
        .iter()
        .filter(|entry| entry.entity_type == 124 && matches!(entry.form, 0 | 1 | 10 | 11 | 12))
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            NativeTransformation {
                id: format!("iges:native:transformation#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                coefficients: (1..=12)
                    .map(|index| parameters.and_then(|record| record.number(index)))
                    .collect(),
                parent: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
        })
        .collect::<Vec<_>>();
    let copious_data = directory
        .iter()
        .filter(|entry| entry.entity_type == 106)
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            let interpretation = parameters.and_then(|record| record.integer(1));
            let declared_tuple_count = parameters.and_then(|record| record.integer(2));
            let common_z = (interpretation == Some(1))
                .then(|| parameters.and_then(|record| record.number(3)))
                .flatten();
            let (start, width) = match interpretation {
                Some(1) => (4, 2),
                Some(2) => (3, 3),
                Some(3) => (3, 6),
                _ => (3, 1),
            };
            let count = declared_tuple_count
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            let tuples = parameters
                .map(|record| {
                    let available = record.tokens.len().saturating_sub(start) / width;
                    (0..count.min(available))
                        .map(|tuple| {
                            (0..width)
                                .map(|component| {
                                    tuple
                                        .checked_mul(width)
                                        .and_then(|offset| offset.checked_add(start))
                                        .and_then(|offset| offset.checked_add(component))
                                        .and_then(|index| record.number(index))
                                })
                                .collect()
                        })
                        .collect()
                })
                .unwrap_or_default();
            NativeCopiousData {
                id: format!("iges:native:copious-data#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                interpretation,
                declared_tuple_count,
                common_z,
                tuples,
            }
        })
        .collect::<Vec<_>>();
    let colors = directory
        .iter()
        .filter(|entry| entry.entity_type == 314 && entry.form == 0)
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            NativeColorDefinition {
                id: format!("iges:presentation:color#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                red_percent: parameters.and_then(|record| record.number(1)),
                green_percent: parameters.and_then(|record| record.number(2)),
                blue_percent: parameters.and_then(|record| record.number(3)),
                name: parameters
                    .and_then(|record| record.string(4))
                    .map(<[u8]>::to_vec),
                fallback_color_number: entry.color,
            }
        })
        .collect::<Vec<_>>();
    let display_attributes = directory
        .iter()
        .map(|entry| NativeDisplayAttributes {
            id: format!("iges:presentation:display-attributes#D{}", entry.sequence),
            source_entity: format!("iges:entity:directory#{}", entry.sequence),
            visible: entry.status.blank == 0,
            line_font_number: entry.line_font,
            line_font_definition: (entry.line_font < 0)
                .then(|| format!("iges:entity:directory#{}", entry.line_font.unsigned_abs())),
            level_number: entry.level,
            level_definition: (entry.level < 0).then(|| {
                format!(
                    "iges:presentation:definition-levels#D{}",
                    entry.level.unsigned_abs()
                )
            }),
            view: entry.view,
            line_weight_number: entry.line_weight,
            line_weight_mm: global.line_weight_mm(entry.line_weight),
            color_number: entry.color,
            color_definition: (entry.color < 0)
                .then(|| format!("iges:presentation:color#D{}", entry.color.unsigned_abs())),
        })
        .collect::<Vec<_>>();
    let line_fonts = directory
        .iter()
        .filter(|entry| entry.entity_type == 304 && matches!(entry.form, 1 | 2))
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            if entry.form == 1 {
                NativeLineFontDefinition::Template {
                    id: format!("iges:presentation:line-font#D{}", entry.sequence),
                    source_entity: format!("iges:entity:directory#{}", entry.sequence),
                    fallback_line_font_number: entry.line_font,
                    tangent_oriented: parameters.and_then(|record| record.integer(1)).and_then(
                        |value| match value {
                            0 => Some(false),
                            1 => Some(true),
                            _ => None,
                        },
                    ),
                    template: parameters
                        .and_then(|record| record.integer(2))
                        .map(|sequence| format!("iges:entity:directory#{sequence}")),
                    spacing: parameters.and_then(|record| record.number(3)),
                    scale: parameters.and_then(|record| record.number(4)),
                }
            } else {
                let count = parameters
                    .and_then(|record| record.integer(1))
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or_default();
                NativeLineFontDefinition::VisibleBlankPattern {
                    id: format!("iges:presentation:line-font#D{}", entry.sequence),
                    source_entity: format!("iges:entity:directory#{}", entry.sequence),
                    fallback_line_font_number: entry.line_font,
                    segment_count: parameters.and_then(|record| record.integer(1)),
                    lengths: (0..count)
                        .map(|index| parameters.and_then(|record| record.number(2 + index)))
                        .collect(),
                    hexadecimal_pattern: parameters
                        .and_then(|record| record.string(2 + count))
                        .map(<[u8]>::to_vec),
                }
            }
        })
        .collect::<Vec<_>>();
    let definition_levels = directory
        .iter()
        .filter(|entry| entry.entity_type == 406 && entry.form == 1)
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            let count = parameters
                .and_then(|record| record.integer(1))
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            NativeDefinitionLevels {
                id: format!("iges:presentation:definition-levels#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                declared_count: parameters.and_then(|record| record.integer(1)),
                levels: (0..count)
                    .map(|index| parameters.and_then(|record| record.integer(2 + index)))
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let namespace = ir.native.namespace_mut("iges");
    namespace.version = 2;
    namespace.set_arena("cards", &cards)?;
    namespace.set_arena("entities", &entities)?;
    namespace.set_arena("directions", &directions)?;
    namespace.set_arena("transformations", &transforms)?;
    namespace.set_arena("copious_data", &copious_data)?;
    namespace.set_arena("colors", &colors)?;
    namespace.set_arena("display_attributes", &display_attributes)?;
    namespace.set_arena("line_fonts", &line_fonts)?;
    namespace.set_arena("definition_levels", &definition_levels)?;
    Ok(())
}
