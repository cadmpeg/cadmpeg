// SPDX-License-Identifier: Apache-2.0
//! Bounded framing for text and binary exact-shape side entries.

use std::collections::BTreeMap;

use cadmpeg_ir::codec::CodecError;
use serde::{Deserialize, Serialize};

use crate::native::{EntryRecord, PropertyFamily, PropertyRecord};

/// Exact-shape side-entry form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapePayloadForm {
    /// Compact text shape-set grammar.
    Text,
    /// Binary shape-set grammar.
    Binary,
}

/// One exact-shape property bound to its side entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapePayloadRecord {
    /// Stable payload identity.
    pub id: String,
    /// Owning property identity.
    pub property: String,
    /// Side-entry identity.
    pub entry: String,
    /// Carrier form.
    pub form: ShapePayloadForm,
    /// Text shape-set facts, when applicable.
    pub text: Option<TextFacts>,
}

/// Framing facts from a text shape set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFacts {
    /// Topology grammar version.
    pub topology_version: u8,
    /// Declared table counts by section name.
    pub section_counts: BTreeMap<String, usize>,
    /// Shape-type token census.
    pub shape_types: BTreeMap<String, usize>,
}

/// Bind every exact-shape property to and frame its payload.
pub fn parse_payloads(
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<Vec<ShapePayloadRecord>, CodecError> {
    let entries = entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    properties
        .iter()
        .filter(|property| property.family == PropertyFamily::Geometry)
        .flat_map(|property| {
            property
                .side_entries
                .iter()
                .map(move |name| (property, name))
        })
        .map(|(property, name)| {
            let entry = entries.get(name.as_str()).ok_or_else(|| {
                CodecError::Malformed(format!("missing exact-shape entry {name}"))
            })?;
            let form = if name.to_ascii_lowercase().ends_with(".bin") {
                ShapePayloadForm::Binary
            } else {
                ShapePayloadForm::Text
            };
            let text = match form {
                ShapePayloadForm::Text => Some(parse_text(&entry.data)?),
                ShapePayloadForm::Binary => None,
            };
            Ok(ShapePayloadRecord {
                id: format!("{}:shape-payload", property.id),
                property: property.id.clone(),
                entry: entry.id.clone(),
                form,
                text,
            })
        })
        .collect()
}

fn parse_text(bytes: &[u8]) -> Result<TextFacts, CodecError> {
    const MAX_TEXT_BREP_BYTES: usize = 256 * 1024 * 1024;
    if bytes.len() > MAX_TEXT_BREP_BYTES {
        return Err(CodecError::Malformed(
            "text B-rep size limit exceeded".into(),
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("text B-rep is not UTF-8".into()))?;
    let topology_version = if text.contains("CASCADE Topology V1, (c) Matra-Datavision") {
        1
    } else if text.contains("CASCADE Topology V2, (c) Matra-Datavision") {
        2
    } else if text.contains("CASCADE Topology V3, (c) Open Cascade") {
        3
    } else {
        return Err(CodecError::Malformed(
            "text B-rep has no supported topology header".into(),
        ));
    };
    let tokens = text.split_ascii_whitespace().collect::<Vec<_>>();
    let mut section_counts = BTreeMap::new();
    for section in [
        "Locations",
        "Curve2ds",
        "Curves",
        "Polygon3D",
        "PolygonOnTriangulations",
        "Surfaces",
        "Triangulations",
        "TShapes",
    ] {
        let index = tokens
            .iter()
            .position(|token| *token == section)
            .ok_or_else(|| CodecError::Malformed(format!("text B-rep has no {section} table")))?;
        let count = tokens
            .get(index + 1)
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| CodecError::Malformed(format!("invalid {section} count")))?;
        section_counts.insert(section.to_owned(), count);
    }
    let tshapes = tokens
        .iter()
        .position(|token| *token == "TShapes")
        .expect("TShapes was validated");
    let mut shape_types = BTreeMap::new();
    for token in &tokens[tshapes + 2..] {
        let name = match *token {
            "Ve" => "vertex",
            "Ed" => "edge",
            "Wi" => "wire",
            "Fa" => "face",
            "Sh" => "shell",
            "So" => "solid",
            "CS" => "compsolid",
            "Co" => "compound",
            _ => continue,
        };
        *shape_types.entry(name.to_owned()).or_insert(0) += 1;
    }
    let declared_shapes = section_counts.get("TShapes").copied().unwrap_or(0);
    if shape_types.values().sum::<usize>() != declared_shapes {
        return Err(CodecError::Malformed(format!(
            "TShapes declares {declared_shapes} records but the shape-type census found {}",
            shape_types.values().sum::<usize>()
        )));
    }
    Ok(TextFacts {
        topology_version,
        section_counts,
        shape_types,
    })
}
