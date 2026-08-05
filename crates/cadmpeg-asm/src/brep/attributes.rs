// SPDX-License-Identifier: Apache-2.0
//! Decode source attribute chains into typed attribute values, colors, names,
//! and transforms.

use crate::ids::IdFormat;
use crate::nurbs::reader::LEN_TO_MM;
use crate::sab::{Record, Token};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::topology::Color;
use std::collections::{HashMap, HashSet};

/// Follow `entity`'s attribute chain, emitting each record not yet in
/// `emitted` as a [`SourceAttribute`] bound to `target`.
#[allow(
    clippy::implicit_hasher,
    reason = "Callers pass default-hasher collections; a hasher parameter adds generic noise for one call shape."
)]
pub fn collect_attributes(
    entity: &Record,
    target: &AttributeTarget,
    by_index: &HashMap<i64, &Record>,
    emitted: &mut HashSet<i64>,
    out: &mut Vec<SourceAttribute>,
    format: IdFormat<'_>,
) {
    let mut current = entity.ref_at(0);
    let mut chain = HashSet::new();
    while let Some(index) = current.filter(|index| chain.insert(*index)) {
        let Some(record) = by_index.get(&index) else {
            break;
        };
        if emitted.insert(index) {
            out.push(source_attribute(record, target.clone(), format));
        }
        current = record.ref_at(0);
    }
}

/// The numeric record-index key of an attribute id
/// (`<format>:brep:attribute#<index>`), used to key records derived from that
/// attribute.
pub fn attribute_key(attribute: &SourceAttribute) -> &str {
    attribute
        .id
        .0
        .rsplit('#')
        .next()
        .unwrap_or(attribute.id.0.as_str())
}

/// Serialize one attribute record's value chunks as a [`SourceAttribute`]
/// bound to `target`.
pub fn source_attribute(
    record: &Record,
    target: AttributeTarget,
    format: IdFormat<'_>,
) -> SourceAttribute {
    SourceAttribute {
        id: AttributeId(format!("{format}:brep:attribute#{}", record.index)),
        target,
        name: record.name.clone(),
        // Chunks, not raw tokens: the serialized value list is defined over the
        // value tokens, and a payload identifier names an embedded construction
        // rather than carrying an attribute value.
        values: record
            .chunks()
            .map(|token| attribute_value(token, format))
            .collect(),
    }
}

fn attribute_value(token: &Token, format: IdFormat<'_>) -> AttributeValue {
    match token {
        Token::Char(value) => AttributeValue::Integer(i64::from(*value)),
        Token::Short(value) => AttributeValue::Integer(i64::from(*value)),
        Token::Long(value) | Token::Enum(value) | Token::Int64(value) => {
            AttributeValue::Integer(*value)
        }
        Token::Float(value) => AttributeValue::Float(f64::from(*value)),
        Token::Double(value) => AttributeValue::Float(*value),
        Token::Str(value) => AttributeValue::String(value.clone()),
        Token::True => AttributeValue::Boolean(true),
        Token::False => AttributeValue::Boolean(false),
        Token::Ref(value) => AttributeValue::Reference(format!("{format}:brep:entity#{value}")),
        Token::SubtypeOpen => AttributeValue::String("subtype_open".into()),
        Token::SubtypeClose => AttributeValue::String("subtype_close".into()),
        Token::Position(value) | Token::Vector3(value) => AttributeValue::Vector(value.to_vec()),
        Token::Vector2(value) => AttributeValue::Vector(value.to_vec()),
        // Not reachable through `source_attribute`, which maps chunks; kept
        // total so a payload identifier still carries its name if a future
        // caller maps raw tokens.
        Token::Ident(value) | Token::SubIdent(value) => AttributeValue::String(value.clone()),
    }
}

/// Decode a native transform record into an IR affine transform, scaling the
/// translation into millimetres.
pub fn decode_transform(
    record: &Record,
    header_scale: f64,
) -> Option<cadmpeg_ir::transform::Transform> {
    let vectors: Vec<[f64; 3]> = record
        .tokens
        .iter()
        .filter_map(|token| match token {
            Token::Position(value) | Token::Vector3(value) => Some(*value),
            _ => None,
        })
        .collect();
    let scale = record
        .tokens
        .iter()
        .filter_map(|token| match token {
            Token::Double(value) => Some(*value),
            _ => None,
        })
        .next_back()?;
    let [x, y, z, translation] = vectors.as_slice() else {
        return None;
    };
    Some(cadmpeg_ir::transform::Transform {
        rows: [
            [x[0], y[0], z[0], translation[0] * header_scale * LEN_TO_MM],
            [x[1], y[1], z[1], translation[1] * header_scale * LEN_TO_MM],
            [x[2], y[2], z[2], translation[2] * header_scale * LEN_TO_MM],
            [0.0, 0.0, 0.0, scale],
        ],
    })
}

/// The first decodable color record on `entity`'s attribute chain.
#[allow(
    clippy::implicit_hasher,
    reason = "Callers pass default-hasher collections; a hasher parameter adds generic noise for one call shape."
)]
pub fn attribute_chain_color(entity: &Record, by_index: &HashMap<i64, &Record>) -> Option<Color> {
    let mut current = entity.ref_at(0)?;
    let mut seen = HashSet::new();
    while seen.insert(current) {
        let record = by_index.get(&current)?;
        if record.name.contains("rgb_color") {
            let values: Vec<f64> = record
                .tokens
                .iter()
                .filter_map(|t| match t {
                    Token::Double(value) => Some(*value),
                    _ => None,
                })
                .collect();
            if let [r, g, b, ..] = values.as_slice() {
                if [*r, *g, *b].iter().all(|value| (0.0..=1.0).contains(value)) {
                    return Some(Color {
                        r: *r as f32,
                        g: *g as f32,
                        b: *b as f32,
                        a: 1.0,
                    });
                }
            }
        } else if record.name.contains("truecolor") {
            let packed = record.tokens.iter().find_map(|token| match token {
                Token::Int64(value) | Token::Long(value) => Some(*value as u32),
                _ => None,
            });
            if let Some(packed) = packed {
                return Some(Color {
                    r: ((packed >> 16) & 0xff) as f32 / 255.0,
                    g: ((packed >> 8) & 0xff) as f32 / 255.0,
                    b: (packed & 0xff) as f32 / 255.0,
                    a: ((packed >> 24) & 0xff) as f32 / 255.0,
                });
            }
        } else if record.name == "entatt_color-bt-attrib" {
            let packed = record.tokens.iter().find_map(|token| match token {
                Token::Str(value) => value
                    .parse::<u32>()
                    .ok()
                    .filter(|value| *value <= 0xff_ffff),
                _ => None,
            });
            if let Some(packed) = packed {
                return Some(Color {
                    r: ((packed >> 16) & 0xff) as f32 / 255.0,
                    g: ((packed >> 8) & 0xff) as f32 / 255.0,
                    b: (packed & 0xff) as f32 / 255.0,
                    a: 1.0,
                });
            }
        }
        current = record.ref_at(0)?;
    }
    None
}

/// The first non-empty name attribute on `entity`'s attribute chain.
#[allow(
    clippy::implicit_hasher,
    reason = "Callers pass default-hasher collections; a hasher parameter adds generic noise for one call shape."
)]
pub fn attribute_chain_name(entity: &Record, by_index: &HashMap<i64, &Record>) -> Option<String> {
    let mut current = entity.ref_at(0)?;
    let mut seen = HashSet::new();
    while seen.insert(current) {
        let record = by_index.get(&current)?;
        if record.name == "string_attrib-name_attrib-gen-attrib" {
            let values = record
                .tokens
                .iter()
                .filter_map(|token| match token {
                    Token::Str(value) => Some(value.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if let [.., "name", value] = values.as_slice() {
                if !value.is_empty() {
                    return Some((*value).to_owned());
                }
            }
        }
        current = record.ref_at(0)?;
    }
    None
}

/// The `UnknownId` for a preserved carrier record. Shared by the passthrough
/// `UnknownRecord` and any `SurfaceGeometry::Unknown` that links to it, so the
/// reference resolves under validation.
pub fn unknown_record_id(rec: &Record, format: IdFormat<'_>) -> String {
    format!("{format}:brep:{}#{}", rec.head, rec.index)
}
