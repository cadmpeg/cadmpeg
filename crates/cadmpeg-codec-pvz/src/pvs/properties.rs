// SPDX-License-Identifier: Apache-2.0
//! Property streams (§3.13): property sets applied to definitions by a
//! cursor over definition indexes.

use std::rc::Rc;

use cadmpeg_core::CodecError;

use super::atom::{uleb_index, StringPool};
use super::element::Element;
use super::{family, Definition, Property, Residue};
use crate::layout::token;

/// Type of a descriptor-`0x92` property set whose payload is the cursor step.
const SET_WITH_STEP: u16 = 0x01;
/// Type of a descriptor-`0x12` property with bytes after its two atoms.
const PROPERTY_TYPED: u16 = 0x05;
/// Bytes after the two atoms of a typed property.
const PROPERTY_TYPED_TAIL: usize = 3;

/// Applies the property sets of one property stream to `definitions`.
///
/// The stream either applies whole or not at all: a grammar failure leaves
/// every definition unchanged.
pub(super) fn apply(
    stream: &str,
    root: &Element<'_>,
    pool: &StringPool,
    definitions: &mut [Definition],
    residue: &mut Residue,
) -> Result<(), CodecError> {
    if !root.is(family::PROPERTIES, token::DESC_CHILDREN, None) {
        return Err(CodecError::malformed(format_args!(
            "property stream {stream:?} root is family {:#04x} descriptor {:#04x}, not 0x03 0x80",
            root.family, root.descriptor
        )));
    }
    let stream_name: Rc<str> = Rc::from(stream);
    let mut assigned: Vec<(usize, Property)> = Vec::new();
    let mut local = Residue::default();
    // The cursor is one more than the definition index it selects, so the
    // initial index −1 is 0.
    let mut cursor = 0_usize;
    for set in root.children() {
        let step = if set.is(family::PROPERTY_SET, token::DESC_CHILDREN, None) {
            1
        } else if set.is(
            family::PROPERTY_SET,
            token::DESC_U8_LENGTH_U8_TYPE_CHILDREN,
            Some(SET_WITH_STEP),
        ) {
            let mut payload = set.payload;
            let step = uleb_index(&mut payload)?;
            if !payload.is_empty() {
                return Err(CodecError::malformed(format_args!(
                    "property set in stream {stream:?} leaves {} bytes after its step",
                    payload.remaining()
                )));
            }
            step
        } else {
            local.element(set);
            continue;
        };
        cursor = cursor
            .checked_add(step)
            .filter(|next| (1..=definitions.len()).contains(next))
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "property set in stream {stream:?} advances past the {} definitions",
                    definitions.len()
                ))
            })?;
        let index = cursor - 1;
        for property in set.children() {
            let typed = if property.is(family::PROPERTY, token::DESC_U8_LENGTH, None) {
                false
            } else if property.is(
                family::PROPERTY,
                token::DESC_U8_LENGTH_U8_TYPE,
                Some(PROPERTY_TYPED),
            ) {
                true
            } else {
                local.element(property);
                continue;
            };
            let mut payload = property.payload;
            let name = pool.atom(&mut payload)?;
            let value = pool.atom(&mut payload)?;
            let tail = if typed { PROPERTY_TYPED_TAIL } else { 0 };
            if payload.remaining() != tail {
                return Err(CodecError::malformed(format_args!(
                    "property {name:?} in stream {stream:?} has {} bytes after its value",
                    payload.remaining()
                )));
            }
            if typed {
                local.typed_properties += 1;
            }
            assigned.push((
                index,
                Property {
                    stream: Rc::clone(&stream_name),
                    name,
                    value,
                },
            ));
        }
    }
    for (index, property) in assigned {
        if let Some(definition) = definitions.get_mut(index) {
            definition.properties.push(property);
        }
    }
    residue.absorb(local);
    Ok(())
}
