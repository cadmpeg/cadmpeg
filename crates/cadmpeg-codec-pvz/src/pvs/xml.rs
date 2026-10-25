// SPDX-License-Identifier: Apache-2.0
//! The XML encoding of a structure member (§3.14).
//!
//! The mapped subset is the one §3.14 tabulates. Every other element is
//! counted in the residue and reported, not interpreted.

use std::rc::Rc;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use roxmltree::Node;

use super::{ChildOccurrence, Definition, DefinitionRole, Placement, Property, Residue, Structure};

const ROOT: &str = "PV_FILE";
const SECTION_INDEX: &str = "section_index";
const SECTION_STRUCTURE: &str = "section_structure";
const SECTION_PROPERTIES: &str = "section_properties";
const COMPONENT: &str = "component";
const SHAPE_SOURCE: &str = "shape_source";
const COMPONENT_INSTANCE: &str = "component_instance";
const PROPERTY_COMPONENT_REF: &str = "property_component_ref";
const PROPERTY: &str = "property";
/// The general property stream has the empty name (§3.3).
const GENERAL_STREAM: &str = "";
/// Root attribute that names the structure form.
const ATTRIBUTE_TYPE: &[u8] = b"type";
/// Value of [`ATTRIBUTE_TYPE`] on a structure member.
const TYPE_PVS: &[u8] = b"PVS";
/// Root attribute that holds the generation digits.
const ATTRIBUTE_VERSION: &[u8] = b"version";

/// The `version` attribute value of the `<PV_FILE type="PVS">` start tag
/// that opens `bytes`. Reads the start tag only, so a prefix of the member
/// suffices.
pub(super) fn declared_version(bytes: &[u8]) -> Option<&[u8]> {
    let mut rest = bytes.strip_prefix(super::XML_PREFIX)?;
    let mut kind = None;
    let mut version = None;
    loop {
        let trimmed = rest.trim_ascii_start();
        let separated = trimmed.len() < rest.len();
        rest = trimmed;
        match rest {
            [b'>', ..] | [b'/', b'>', ..] => break,
            _ if !separated => return None,
            _ => {}
        }
        let name_len = rest
            .iter()
            .position(|&byte| byte == b'=' || byte.is_ascii_whitespace())?;
        let (name, after) = rest.split_at(name_len);
        let after = after
            .trim_ascii_start()
            .strip_prefix(b"=")?
            .trim_ascii_start();
        let (&quote, after) = after.split_first()?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        let value_len = after.iter().position(|&byte| byte == quote)?;
        let (value, after) = after.split_at(value_len);
        rest = after.get(1..)?;
        match name {
            ATTRIBUTE_TYPE => kind = Some(value),
            ATTRIBUTE_VERSION => version = Some(value),
            _ => {}
        }
    }
    (kind == Some(TYPE_PVS)).then_some(version).flatten()
}

/// Decodes an XML structure member.
pub(super) fn read(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Structure, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::malformed("XML structure member is not valid UTF-8"))?;
    let document = ctx.parse_xml(text, "PVZ XML structure member")?;
    let root = document.root_element();
    if root.tag_name().name() != ROOT || root.attribute("type") != Some("PVS") {
        return Err(CodecError::malformed(
            "XML structure member root is not <PV_FILE type=\"PVS\">",
        ));
    }
    let mut residue = Residue::default();
    let general: Rc<str> = Rc::from(GENERAL_STREAM);
    let mut definitions = Vec::new();
    let mut property_sets = Vec::new();
    for section in root.children().filter(Node::is_element) {
        match section.tag_name().name() {
            SECTION_INDEX => {}
            SECTION_STRUCTURE => {
                for component in section.children().filter(Node::is_element) {
                    if component.tag_name().name() == COMPONENT {
                        definitions.push(component_definition(component, &mut residue)?);
                    } else {
                        residue.xml_element(component.tag_name().name());
                    }
                }
            }
            SECTION_PROPERTIES => {
                for set in section.children().filter(Node::is_element) {
                    if set.tag_name().name() == PROPERTY_COMPONENT_REF {
                        property_sets.push(set);
                    } else {
                        residue.xml_element(set.tag_name().name());
                    }
                }
            }
            other => residue.xml_element(other),
        }
    }
    let count = definitions.len();
    if let Some(occurrence) = definitions
        .iter()
        .flat_map(|definition| &definition.occurrences)
        .find(|occurrence| occurrence.definition >= count)
    {
        return Err(CodecError::malformed(format_args!(
            "XML component_instance places component {} of {count}",
            occurrence.definition
        )));
    }
    let mut cursor = 0_usize;
    for set in property_sets {
        let step = match set.attribute("index_offset") {
            Some(text) => index(text, "index_offset")?,
            None => 1,
        };
        cursor = cursor
            .checked_add(step)
            .filter(|next| (1..=count).contains(next))
            .ok_or_else(|| {
                CodecError::malformed("XML property_component_ref advances past the components")
            })?;
        for property in set.children().filter(Node::is_element) {
            if property.tag_name().name() != PROPERTY {
                residue.xml_element(property.tag_name().name());
                continue;
            }
            if let Some(definition) = definitions.get_mut(cursor - 1) {
                definition.properties.push(Property {
                    stream: Rc::clone(&general),
                    name: required(property, "name")?.into(),
                    value: required(property, "value")?.into(),
                });
            }
        }
    }
    Ok(Structure {
        definitions,
        residue,
    })
}

fn component_definition(
    component: Node<'_, '_>,
    residue: &mut Residue,
) -> Result<Definition, CodecError> {
    let mut definition = Definition {
        name: component.attribute("name").unwrap_or_default().into(),
        role: DefinitionRole::Component,
        geometry: None,
        occurrences: Vec::new(),
        properties: Vec::new(),
    };
    for child in component.children().filter(Node::is_element) {
        match child.tag_name().name() {
            SHAPE_SOURCE => {
                let member: Rc<str> = required(child, "file_name")?.into();
                if child.attribute("bbox").is_some() {
                    residue.bounds += 1;
                }
                if definition.geometry.replace(member).is_some() {
                    return Err(CodecError::malformed(format_args!(
                        "XML component {:?} has more than one shape_source",
                        definition.name
                    )));
                }
            }
            COMPONENT_INSTANCE => {
                let translation = child
                    .attribute("translation")
                    .map(|text| reals::<3>(text, "translation"))
                    .transpose()?;
                let basis = child
                    .attribute("orientation")
                    .map(|text| reals::<9>(text, "orientation"))
                    .transpose()?;
                definition.occurrences.push(ChildOccurrence {
                    definition: index(required(child, "index")?, "index")?,
                    placement: Placement { translation, basis },
                    feature_id: child.attribute("id").unwrap_or_default().into(),
                });
            }
            other => residue.xml_element(other),
        }
    }
    Ok(definition)
}

fn required<'a>(node: Node<'a, '_>, attribute: &str) -> Result<&'a str, CodecError> {
    node.attribute(attribute).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "XML <{}> has no {attribute} attribute",
            node.tag_name().name()
        ))
    })
}

fn index(text: &str, attribute: &str) -> Result<usize, CodecError> {
    text.trim().parse().map_err(|_| {
        CodecError::malformed(format_args!(
            "XML {attribute} {text:?} is not a non-negative integer"
        ))
    })
}

/// Parses exactly `N` comma-separated reals.
fn reals<const N: usize>(text: &str, attribute: &str) -> Result<[f64; N], CodecError> {
    let invalid = || {
        CodecError::malformed(format_args!(
            "XML {attribute} {text:?} is not {N} comma-separated reals"
        ))
    };
    let mut values = [0.0; N];
    let mut parts = text.split(',');
    for value in &mut values {
        *value = parts
            .next()
            .and_then(|part| part.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(invalid)?;
    }
    if parts.next().is_some() {
        return Err(invalid());
    }
    Ok(values)
}
