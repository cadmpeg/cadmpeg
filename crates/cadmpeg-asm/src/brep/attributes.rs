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
#[allow(clippy::implicit_hasher)]
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
        current = attribute_next(record);
    }
}

fn is_integer(token: Option<&Token>) -> bool {
    matches!(
        token,
        Some(Token::Char(_) | Token::Short(_) | Token::Long(_) | Token::Enum(_) | Token::Int64(_))
    )
}

#[derive(Clone, Copy)]
struct AttributeBase {
    next: usize,
    owner: Option<usize>,
    payload: usize,
}

fn attribute_base(record: &Record) -> Option<AttributeBase> {
    let current = matches!(
        (
            record.chunk(0),
            record.chunk(2),
            record.chunk(3),
            record.chunk(4),
        ),
        (
            Some(Token::Ref(_)),
            Some(Token::Ref(_)),
            Some(Token::Ref(_)),
            Some(Token::Ref(_)),
        )
    ) && is_integer(record.chunk(1));
    if current {
        return Some(AttributeBase {
            next: 2,
            owner: Some(4),
            payload: 5,
        });
    }
    let legacy = matches!(
        (
            record.chunk(0),
            record.chunk(1),
            record.chunk(2),
            record.chunk(3),
        ),
        (
            Some(Token::Ref(_)),
            Some(Token::Ref(_)),
            Some(Token::Ref(_)),
            Some(Token::Ref(_)),
        )
    );
    if legacy {
        return Some(AttributeBase {
            next: 1,
            owner: Some(3),
            payload: 4,
        });
    }
    matches!(record.chunk(0), Some(Token::Ref(_))).then_some(AttributeBase {
        next: 0,
        owner: None,
        payload: 1,
    })
}

/// The next record in an attribute chain.
///
/// A current ASM attribute starts with `reserved, marker, next, previous,
/// owner`; a legacy attribute omits `marker`. Source-less streams written by
/// older cadmpeg versions used a compact record whose first field was `next`;
/// retain read compatibility with all three forms.
pub(crate) fn attribute_next(record: &Record) -> Option<i64> {
    record.ref_at(attribute_base(record)?.next)
}

/// The topology or parent-attribute owner of a current or legacy attribute.
pub(crate) fn attribute_owner(record: &Record) -> Option<i64> {
    record.ref_at(attribute_base(record)?.owner?)
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
        .unwrap_or(attribute.id.as_str())
}

/// Serialize one attribute record's value chunks as a [`SourceAttribute`]
/// bound to `target`.
pub fn source_attribute(
    record: &Record,
    target: AttributeTarget,
    format: IdFormat<'_>,
) -> SourceAttribute {
    SourceAttribute {
        id: AttributeId::mint(format!("{format}:brep:attribute#{}", record.index))
            .expect("identity grammar"),
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
    Some(
        cadmpeg_ir::transform::Transform::from_rows([
            [x[0], y[0], z[0], translation[0] * header_scale * LEN_TO_MM],
            [x[1], y[1], z[1], translation[1] * header_scale * LEN_TO_MM],
            [x[2], y[2], z[2], translation[2] * header_scale * LEN_TO_MM],
            [0.0, 0.0, 0.0, scale],
        ])
        .expect("affine transform"),
    )
}

/// Storage form and payload-field location of an exact direct-color attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectColorCarrier {
    /// Three normalized f64 channel fields.
    NormalizedRgb {
        /// Payload-field indices of red, green, and blue.
        fields: [usize; 3],
    },
    /// One Autodesk method-and-color packed integer field.
    AutodeskTrueColor {
        /// Payload-field index of the packed integer.
        field: usize,
    },
    /// One decimal-text packed RGB field.
    DecimalRgb {
        /// Payload-field index of the decimal text.
        field: usize,
    },
}

/// A decoded exact direct-color attribute and the carrier that supplied it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectAttributeColor {
    /// Opaque neutral RGB color.
    pub color: Color,
    /// Native payload form and field location.
    pub carrier: DirectColorCarrier,
}

fn packed_u32(value: i64) -> Option<u32> {
    u32::try_from(value)
        .ok()
        .or_else(|| i32::try_from(value).ok().map(|value| value as u32))
}

fn packed_rgb(packed: u32) -> Color {
    Color {
        r: ((packed >> 16) & 0xff) as f32 / 255.0,
        g: ((packed >> 8) & 0xff) as f32 / 255.0,
        b: (packed & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

fn direct_payload_start(record: &Record) -> Option<usize> {
    attribute_base(record).map(|base| base.payload)
}

/// Decode one well-formed exact direct-color attribute.
///
/// Palette, material-library, inherited truecolor, and malformed records do
/// not define a neutral RGB color.
pub(crate) fn direct_attribute_color(record: &Record) -> Option<DirectAttributeColor> {
    let payload = direct_payload_start(record)?;
    match record.name.as_str() {
        "rgb_color-st-attrib" => {
            let channels = record
                .chunks()
                .enumerate()
                .skip(payload)
                .filter_map(|(field, token)| match token {
                    Token::Double(value) => Some((field, *value)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let [(r_field, r), (g_field, g), (b_field, b)] = match channels.as_slice() {
                [red, green, blue] => [*red, *green, *blue],
                [red, green, blue, (_, 1.0)] => [*red, *green, *blue],
                _ => return None,
            };
            if ![r, g, b]
                .into_iter()
                .all(|value| (0.0..=1.0).contains(&value))
            {
                return None;
            }
            Some(DirectAttributeColor {
                color: Color {
                    r: r as f32,
                    g: g as f32,
                    b: b as f32,
                    a: 1.0,
                },
                carrier: DirectColorCarrier::NormalizedRgb {
                    fields: [r_field, g_field, b_field],
                },
            })
        }
        "truecolor-adesk-attrib" => {
            let (field, packed) = record
                .chunks()
                .enumerate()
                .skip(payload)
                .filter_map(|(field, token)| match token {
                    Token::Int64(value) | Token::Long(value) => Some((field, *value)),
                    _ => None,
                })
                .last()?;
            let packed = packed_u32(packed)?;
            // AcCmColor stores its color method in the high byte. Only
            // kByColor carries self-contained RGB channels.
            if packed >> 24 != 0xc2 {
                return None;
            }
            Some(DirectAttributeColor {
                color: packed_rgb(packed),
                carrier: DirectColorCarrier::AutodeskTrueColor { field },
            })
        }
        "entatt_color-bt-attrib" => {
            let (field, text) = record
                .chunks()
                .enumerate()
                .skip(payload)
                .filter_map(|(field, token)| match token {
                    Token::Str(value) => Some((field, value.as_str())),
                    _ => None,
                })
                .last()?;
            if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let packed = text
                .parse::<u32>()
                .ok()
                .filter(|value| *value <= 0xff_ffff)?;
            Some(DirectAttributeColor {
                color: packed_rgb(packed),
                carrier: DirectColorCarrier::DecimalRgb { field },
            })
        }
        _ => None,
    }
}

/// The first well-formed exact direct-color carrier on an attribute chain.
pub fn attribute_chain_color_carrier<'a>(
    entity: &Record,
    mut by_index: impl FnMut(i64) -> Option<&'a Record>,
) -> Option<(&'a Record, DirectAttributeColor)> {
    let mut current = entity.ref_at(0)?;
    let mut seen = HashSet::new();
    while seen.insert(current) {
        let record = by_index(current)?;
        if let Some(color) = direct_attribute_color(record) {
            return Some((record, color));
        }
        current = attribute_next(record)?;
    }
    None
}

/// The first well-formed exact direct color on `entity`'s attribute chain.
#[allow(clippy::implicit_hasher)]
pub fn attribute_chain_color(entity: &Record, by_index: &HashMap<i64, &Record>) -> Option<Color> {
    attribute_chain_color_carrier(entity, |index| by_index.get(&index).copied())
        .map(|(_, decoded)| decoded.color)
}

/// The first non-empty name attribute on `entity`'s attribute chain.
#[allow(clippy::implicit_hasher)]
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
        current = attribute_next(record)?;
    }
    None
}

/// The `UnknownId` for a preserved carrier record. Shared by the passthrough
/// `UnknownRecord` and any `SurfaceGeometry::Unknown` that links to it, so the
/// reference resolves under validation.
pub fn unknown_record_id(rec: &Record, format: IdFormat<'_>) -> String {
    format!("{format}:brep:{}#{}", rec.head, rec.index)
}
