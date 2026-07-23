// SPDX-License-Identifier: Apache-2.0
//! Decode source attribute chains into sketch links, persistent design and
//! subentity references, colors, names, and transforms.

use crate::nurbs::reader::LEN_TO_MM;
use crate::records::{
    CreationTimestamp, PersistentDesignLink, PersistentSubentityTag, SketchCurveLink,
};
use crate::sab::{Record, Token};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::wire::cursor::bounded_len;
use std::collections::{HashMap, HashSet};

pub(crate) fn sketch_curve_link(attribute: &SourceAttribute) -> Option<SketchCurveLink> {
    let AttributeTarget::Coedge(coedge) = &attribute.target else {
        return None;
    };
    let family = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "sketch_attrib_def"),
    )?;
    let fields = attribute.values[family + 1..]
        .iter()
        .filter_map(|value| match value {
            AttributeValue::String(payload) => Some(
                payload
                    .split_ascii_whitespace()
                    .map(str::parse::<i64>)
                    .collect::<Result<Vec<_>, _>>()
                    .ok(),
            ),
            _ => None,
        })
        .flatten()
        .find(|values| values.len() == 6)
        .unwrap_or_else(|| {
            attribute.values[family + 1..]
                .iter()
                .filter_map(|value| match value {
                    AttributeValue::Integer(value) => Some(*value),
                    _ => None,
                })
                .take(6)
                .collect()
        });
    let [sketch_curve_id, 0, signed_reference, 0, role, closure] = fields.as_slice() else {
        return None;
    };
    Some(SketchCurveLink {
        id: format!("f3d:design:sketch-curve-link#{}", attribute_key(attribute)),
        coedge: coedge.clone(),
        sketch_curve_id: *sketch_curve_id,
        signed_reference: (*signed_reference != -1).then_some(*signed_reference),
        role: *role,
        closure: *closure,
    })
}

pub(crate) fn persistent_design_links(attribute: &SourceAttribute) -> Vec<PersistentDesignLink> {
    let AttributeTarget::Body(_) = &attribute.target else {
        return Vec::new();
    };
    let Some(family) = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "generic_tag_attrib_def"),
    ) else {
        return Vec::new();
    };
    let values = &attribute.values[family + 1..];
    let [AttributeValue::Integer(3), AttributeValue::Integer(3), AttributeValue::Integer(-1), AttributeValue::String(marker), AttributeValue::Integer(group_count), rest @ ..] =
        values
    else {
        return Vec::new();
    };
    if marker != "generic_tag_attrib_def " || *group_count < 0 {
        return Vec::new();
    }
    let Ok(group_count) = usize::try_from(*group_count) else {
        return Vec::new();
    };
    if rest.len() != group_count.saturating_mul(5) {
        return Vec::new();
    }
    let groups = rest
        .chunks_exact(5)
        .filter_map(|values| match values {
            [
                AttributeValue::Integer(entity_kind),
                AttributeValue::String(design_id),
                AttributeValue::Integer(design_reference),
                AttributeValue::Integer(0),
                AttributeValue::Integer(0),
            ] if !design_id.is_empty()
                && design_id.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                Some((*entity_kind, design_id.clone(), *design_reference))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if groups.len() != group_count {
        return Vec::new();
    }
    let groups = groups
        .into_iter()
        .filter(|(entity_kind, _, _)| *entity_kind == 3)
        .collect::<Vec<_>>();
    let last = groups.len().saturating_sub(1);
    groups
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (entity_kind, design_id, design_reference))| PersistentDesignLink {
                id: format!(
                    "f3d:design:persistent-design-link#{}:{ordinal}",
                    attribute_key(attribute)
                ),
                target: attribute.target.clone(),
                design_id,
                entity_kind,
                design_reference,
                ordinal: ordinal as u32,
                is_current: ordinal == last,
            },
        )
        .collect()
}

pub(crate) fn persistent_subentity_tags(
    attribute: &SourceAttribute,
) -> Vec<PersistentSubentityTag> {
    if !matches!(
        attribute.target,
        AttributeTarget::Face(_) | AttributeTarget::Edge(_)
    ) {
        return Vec::new();
    }
    let Some(family) = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "generic_tag_attrib_def"),
    ) else {
        return Vec::new();
    };
    let values = &attribute.values[family + 1..];
    let [AttributeValue::Integer(3), AttributeValue::Integer(3), AttributeValue::Integer(-1), AttributeValue::String(marker), AttributeValue::Integer(group_count), rest @ ..] =
        values
    else {
        return Vec::new();
    };
    if marker != "generic_tag_attrib_def " || *group_count < 0 {
        return Vec::new();
    }
    let Ok(group_count) = usize::try_from(*group_count) else {
        return Vec::new();
    };
    // Each group consumes at least four leading attribute values from `rest`.
    let Some(group_count) = bounded_len(group_count as u64, 4, rest.len()) else {
        return Vec::new();
    };
    let mut position: usize = 0;
    let mut groups = Vec::with_capacity(group_count);
    for ordinal in 0..group_count {
        let Some(
            [AttributeValue::Integer(selector), AttributeValue::String(token), AttributeValue::Integer(0), AttributeValue::Integer(reference_count)],
        ) = rest.get(position..position.saturating_add(4))
        else {
            return Vec::new();
        };
        if token.is_empty() || *reference_count < 0 {
            return Vec::new();
        }
        let Ok(reference_count) = usize::try_from(*reference_count) else {
            return Vec::new();
        };
        let reference_start = position + 4;
        let reference_end = reference_start.saturating_add(reference_count);
        let Some(reference_values) = rest.get(reference_start..reference_end) else {
            return Vec::new();
        };
        let references = reference_values
            .iter()
            .map(|value| match value {
                AttributeValue::Integer(value) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>();
        let Some(design_references) = references else {
            return Vec::new();
        };
        if !matches!(rest.get(reference_end), Some(AttributeValue::Integer(0))) {
            return Vec::new();
        }
        groups.push(PersistentSubentityTag {
            id: format!(
                "f3d:design:persistent-subentity-tag#{}:{ordinal}",
                attribute_key(attribute)
            ),
            target: attribute.target.clone(),
            selector: *selector,
            token: token.clone(),
            design_references,
            ordinal: ordinal as u32,
        });
        position = reference_end + 1;
    }
    if position != rest.len() {
        return Vec::new();
    }
    groups
}

pub(crate) fn creation_timestamp(attribute: &SourceAttribute) -> Option<CreationTimestamp> {
    let family = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "Timestamp_attrib_def"),
    )?;
    let marker = attribute.values.get(family + 1)?;
    if !matches!(marker, AttributeValue::Integer(1)) {
        return None;
    }
    let AttributeValue::Float(unix_microseconds) = attribute.values.get(family + 2)? else {
        return None;
    };
    if !unix_microseconds.is_finite() {
        return None;
    }
    Some(CreationTimestamp {
        id: format!("f3d:design:creation-timestamp#{}", attribute_key(attribute)),
        target: attribute.target.clone(),
        record_index: attribute_key(attribute).parse().ok()?,
        unix_microseconds: *unix_microseconds,
    })
}

pub(crate) fn collect_attributes(
    entity: &Record,
    target: &AttributeTarget,
    by_index: &HashMap<i64, &Record>,
    emitted: &mut HashSet<i64>,
    out: &mut Vec<SourceAttribute>,
) {
    let mut current = entity.ref_at(0);
    let mut chain = HashSet::new();
    while let Some(index) = current.filter(|index| chain.insert(*index)) {
        let Some(record) = by_index.get(&index) else {
            break;
        };
        if emitted.insert(index) {
            out.push(source_attribute(record, target.clone()));
        }
        current = record.ref_at(0);
    }
}

/// The numeric record-index key of an attribute id
/// (`f3d:brep:attribute#<index>`), used to key records derived from that
/// attribute.
fn attribute_key(attribute: &SourceAttribute) -> &str {
    attribute
        .id
        .0
        .rsplit('#')
        .next()
        .unwrap_or(attribute.id.0.as_str())
}

pub(crate) fn source_attribute(record: &Record, target: AttributeTarget) -> SourceAttribute {
    SourceAttribute {
        id: AttributeId(format!("f3d:brep:attribute#{}", record.index)),
        target,
        name: record.name.clone(),
        values: record.tokens.iter().map(attribute_value).collect(),
    }
}

fn attribute_value(token: &Token) -> AttributeValue {
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
        Token::Ref(value) => AttributeValue::Reference(format!("f3d:brep:entity#{value}")),
        Token::SubtypeOpen => AttributeValue::String("subtype_open".into()),
        Token::SubtypeClose => AttributeValue::String("subtype_close".into()),
        Token::Position(value) | Token::Vector3(value) => AttributeValue::Vector(value.to_vec()),
        Token::Vector2(value) => AttributeValue::Vector(value.to_vec()),
    }
}

pub(crate) fn decode_transform(
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

pub(crate) fn attribute_chain_color(
    entity: &Record,
    by_index: &HashMap<i64, &Record>,
) -> Option<Color> {
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

pub(crate) fn attribute_chain_name(
    entity: &Record,
    by_index: &HashMap<i64, &Record>,
) -> Option<String> {
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

/// The raw bytes of a record within the decompressed stream.
pub(crate) fn record_slice<'a>(rec: &Record, bytes: &'a [u8]) -> &'a [u8] {
    let end = (rec.offset + rec.len).min(bytes.len());
    &bytes[rec.offset..end]
}

/// The `UnknownId` for a preserved carrier record. Shared by the passthrough
/// `UnknownRecord` and any `SurfaceGeometry::Unknown` that links to it, so the
/// reference resolves under validation.
pub(crate) fn unknown_record_id(rec: &Record) -> String {
    format!("f3d:brep:{}#{}", rec.head, rec.index)
}
