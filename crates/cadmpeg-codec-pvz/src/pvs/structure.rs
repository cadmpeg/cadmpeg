// SPDX-License-Identifier: Apache-2.0
//! The structure stream: definitions (§3.6, §3.7), geometry elements (§3.8),
//! and child occurrences with their placements (§3.9, §3.10).

use std::rc::Rc;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use super::atom::{uleb_index, StringPool};
use super::element::Element;
use super::{family, ChildOccurrence, Definition, DefinitionRole, Placement, Residue};
use crate::layout::token;

/// Type of a descriptor-`0x92` definition that roots illustrations.
const DEFINITION_ILLUSTRATION_ROOT: u16 = 0x01;
/// Type of a descriptor-`0x12` geometry element that carries bounds.
const GEOMETRY_WITH_BOUNDS: u16 = 0x01;
/// `f32` bounds of a geometry element: a minimum and a maximum corner.
const BOUNDS_VALUES: usize = 6;
/// Low four bits of a placement kind: the placement encoding.
const PLACEMENT_ENCODING: u16 = 0x0f;
/// Placement kind bits above the encoding nibble.
const PLACEMENT_FLAGS: u16 = 0xfff0;
/// Placement encoding: identity.
const PLACEMENT_IDENTITY: u16 = 0x8;
/// Placement encoding: translation.
const PLACEMENT_TRANSLATION: u16 = 0xa;
/// Placement encoding: basis.
const PLACEMENT_BASIS: u16 = 0xc;
/// Placement encoding: translation, then basis.
const PLACEMENT_TRANSLATION_BASIS: u16 = 0xe;

/// Bytes after the name atom of a typed definition (§3.7).
fn definition_attribute_len(kind: u16) -> Option<usize> {
    match kind {
        0x01 | 0x02 => Some(1),
        0x06 => Some(2),
        0x1e => Some(4),
        _ => None,
    }
}

/// Reads the definitions of the structure stream root.
pub(super) fn read(
    root: &Element<'_>,
    pool: &StringPool,
    residue: &mut Residue,
) -> Result<Vec<Definition>, CodecError> {
    if !root.is(family::STRUCTURE, token::DESC_CHILDREN, None) {
        return Err(CodecError::malformed(format_args!(
            "structure stream root is family {:#04x} descriptor {:#04x}, not 0x02 0x80",
            root.family, root.descriptor
        )));
    }
    let mut definitions = Vec::new();
    for child in root.children() {
        match child.family {
            family::DEFINITION => definitions.push(read_definition(child, pool, residue)?),
            _ => residue.element(child),
        }
    }
    let count = definitions.len();
    for (index, definition) in definitions.iter().enumerate() {
        if let Some(occurrence) = definition
            .occurrences
            .iter()
            .find(|occurrence| occurrence.definition >= count)
        {
            return Err(CodecError::malformed(format_args!(
                "definition {index} places definition {} of {count}",
                occurrence.definition
            )));
        }
    }
    Ok(definitions)
}

fn read_definition(
    element: &Element<'_>,
    pool: &StringPool,
    residue: &mut Residue,
) -> Result<Definition, CodecError> {
    let typed = match (element.descriptor, element.kind) {
        (token::DESC_U8_LENGTH | token::DESC_U8_LENGTH_CHILDREN, None) => None,
        (token::DESC_U8_LENGTH_U8_TYPE_CHILDREN, Some(kind)) => Some(kind),
        _ => {
            return Err(CodecError::malformed(format_args!(
                "definition at offset {} has descriptor {:#04x}",
                element.payload.start(),
                element.descriptor
            )))
        }
    };
    let mut payload = element.payload;
    let name = pool.atom(&mut payload)?;
    let expected = typed.map_or(Some(0), definition_attribute_len);
    if expected != Some(payload.remaining()) {
        return Err(CodecError::malformed(format_args!(
            "definition {name:?} of type {typed:?} has {} bytes after its name",
            payload.remaining()
        )));
    }
    if typed.is_some() {
        residue.definition_attributes += 1;
    }
    let role = if typed == Some(DEFINITION_ILLUSTRATION_ROOT) {
        DefinitionRole::IllustrationRoot
    } else {
        DefinitionRole::Component
    };
    let mut definition = Definition {
        name,
        role,
        geometry: None,
        occurrences: Vec::new(),
        properties: Vec::new(),
    };
    for child in element.children() {
        match child.family {
            family::GEOMETRY => {
                let member = read_geometry(child, pool, residue)?;
                if definition.geometry.replace(member).is_some() {
                    return Err(CodecError::malformed(format_args!(
                        "definition {:?} has more than one geometry element",
                        definition.name
                    )));
                }
            }
            family::OCCURRENCE => {
                if let Some(occurrence) = read_occurrence(child, pool, residue)? {
                    definition.occurrences.push(occurrence);
                }
            }
            _ => residue.element(child),
        }
    }
    Ok(definition)
}

/// Reads a geometry element: the member name and, when present, its bounds.
fn read_geometry(
    element: &Element<'_>,
    pool: &StringPool,
    residue: &mut Residue,
) -> Result<Rc<str>, CodecError> {
    let mut payload = element.payload;
    let member = pool.atom(&mut payload)?;
    if element.is(
        family::GEOMETRY,
        token::DESC_U8_LENGTH_U8_TYPE,
        Some(GEOMETRY_WITH_BOUNDS),
    ) {
        for _ in 0..BOUNDS_VALUES {
            payload.req_f32_le()?;
        }
        residue.bounds += 1;
    } else if !element.is(family::GEOMETRY, token::DESC_U8_LENGTH, None) {
        return Err(CodecError::malformed(format_args!(
            "geometry element for {member:?} has descriptor {:#04x}",
            element.descriptor
        )));
    }
    if !payload.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "geometry element for {member:?} leaves {} bytes",
            payload.remaining()
        )));
    }
    Ok(member)
}

/// Reads a child occurrence. An occurrence whose placement encoding is not
/// defined is counted in `residue` and yields `None`.
fn read_occurrence(
    element: &Element<'_>,
    pool: &StringPool,
    residue: &mut Residue,
) -> Result<Option<ChildOccurrence>, CodecError> {
    let (kind, extended) = match (element.descriptor, element.kind) {
        (token::DESC_U8_LENGTH_U8_TYPE | token::DESC_U8_LENGTH_U8_TYPE_CHILDREN, Some(kind)) => {
            (kind, false)
        }
        (token::DESC_U8_LENGTH_U16_TYPE, Some(kind)) => (kind, true),
        _ => {
            residue.element(element);
            return Ok(None);
        }
    };
    let mut payload = element.payload;
    let definition = uleb_index(&mut payload)?;
    let placement = match kind & PLACEMENT_ENCODING {
        PLACEMENT_IDENTITY => Placement::default(),
        PLACEMENT_TRANSLATION => Placement {
            translation: Some(f64s(&mut payload)?),
            basis: None,
        },
        PLACEMENT_BASIS => Placement {
            translation: None,
            basis: Some(f64s(&mut payload)?),
        },
        PLACEMENT_TRANSLATION_BASIS => Placement {
            translation: Some(f64s(&mut payload)?),
            basis: Some(f64s(&mut payload)?),
        },
        _ => {
            residue.occurrences_undecoded += 1;
            return Ok(None);
        }
    };
    let feature_id = pool.atom(&mut payload)?;
    if extended {
        if kind & PLACEMENT_FLAGS != 0 || !payload.is_empty() {
            residue.occurrence_attributes += 1;
        }
    } else {
        if !payload.is_empty() {
            return Err(CodecError::malformed(format_args!(
                "occurrence {feature_id:?} leaves {} bytes after its feature identifier",
                payload.remaining()
            )));
        }
        if kind & PLACEMENT_FLAGS != 0 {
            residue.occurrence_attributes += 1;
        }
    }
    for child in element.children() {
        residue.element(child);
    }
    Ok(Some(ChildOccurrence {
        definition,
        placement,
        feature_id,
    }))
}

/// Reads `N` little-endian `f64` values.
fn f64s<const N: usize>(view: &mut View<'_>) -> Result<[f64; N], CodecError> {
    let mut values = [0.0; N];
    for value in &mut values {
        *value = view.req_f64_le()?;
    }
    Ok(values)
}
