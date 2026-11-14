// SPDX-License-Identifier: Apache-2.0
//! The start condition of an extrusion, from its `moFromEndSpec_c` object.
//!
//! The object follows the extrusion's block of 72 or 76 bytes. Its leading u32
//! is the start type. Type `0` starts on the profile plane. Every other type
//! starts elsewhere, and the definition keeps that start unresolved: the
//! offset plane of type `5` has no proved direction relative to the profile
//! normal, and the other types name no plane at all. The decoder and the
//! native validator both reach the definition through [`condition`], so a
//! start that is not type `0` cannot be reported as the profile plane.

use super::is_class_token;
use cadmpeg_core::decode::View;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::ExtrudeStart;
use std::collections::BTreeMap;

/// The start type that names the profile plane.
const START_PROFILE_PLANE: u32 = 0;
/// The first eight bytes of the extrusion's block.
const BLOCK_HEADER: [u8; 8] = [0, 0, 1, 0, 0, 1, 0, 0];
/// The length of the block without its u32 word.
const SHORT_BLOCK_LEN: usize = 72;
/// The length of the block with its u32 word: the u32 follows 56 zero bytes and
/// eight zero bytes follow the u32.
const WORD_BLOCK_LEN: usize = 76;
/// Where the u32 word of the longer block starts, from the block start.
const BLOCK_WORD_AT: usize = 64;
/// The direct class declaration of a `moFromEndSpec_c` object.
const DECLARATION: &[u8] = b"\xff\xff\x01\x00\x0f\x00moFromEndSpec_c";
/// The zero bytes a profile-plane start writes in place of its type word.
const PROFILE_PLANE_BODY_LEN: usize = 20;

/// History property naming a start that is not the profile plane.
pub(crate) const START_CONDITION: &str = "StartCondition";
/// The value of [`START_CONDITION`] for a start with no supported reading.
const UNRESOLVED: &str = "Unresolved";

/// A start condition read from the lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartCondition {
    /// The profile plane.
    ProfilePlane,
    /// A start with no supported reading.
    Unresolved,
}

/// The condition of one start type: type `0` is the profile plane, every
/// other type is unresolved.
pub(crate) fn condition(kind: u32) -> StartCondition {
    if kind == START_PROFILE_PLANE {
        StartCondition::ProfilePlane
    } else {
        StartCondition::Unresolved
    }
}

/// The condition shared by the starts of one extrusion. A start other than
/// the profile plane outweighs a profile-plane reading of the same object.
pub(crate) fn common_condition(conditions: &[StartCondition]) -> StartCondition {
    if conditions.contains(&StartCondition::Unresolved) {
        StartCondition::Unresolved
    } else {
        StartCondition::ProfilePlane
    }
}

/// Whether `offset` holds a class token or the marker of a class declaration.
fn object_pointer_at(payload: &[u8], offset: usize) -> bool {
    payload
        .get(offset..offset + 2)
        .and_then(|bytes| View::u16_le_at(bytes, 0))
        .is_some_and(|word| is_class_token(word) || word == u16::MAX)
}

/// Where the object after the block at `block` starts, when the block has one
/// of its two written forms.
fn object_after_block(payload: &[u8], block: usize) -> Option<usize> {
    let bytes = payload.get(block..block.checked_add(WORD_BLOCK_LEN)?)?;
    if bytes[..BLOCK_HEADER.len()] != BLOCK_HEADER {
        return None;
    }
    let zeros = |range: std::ops::Range<usize>| bytes[range].iter().all(|byte| *byte == 0);
    if zeros(BLOCK_HEADER.len()..SHORT_BLOCK_LEN) {
        let object = block + SHORT_BLOCK_LEN;
        if object_pointer_at(payload, object) {
            return Some(object);
        }
    }
    (zeros(BLOCK_HEADER.len()..BLOCK_WORD_AT)
        && View::u32_le_at(bytes, BLOCK_WORD_AT).is_some_and(|word| word <= 1)
        && zeros(BLOCK_WORD_AT + 4..WORD_BLOCK_LEN))
    .then_some(block + WORD_BLOCK_LEN)
}

/// The start type of the object after the block at `block`, when the block
/// and the object both have their written form: a class token or the class
/// declaration, then either twenty zero bytes (type `0`) or a nonzero type
/// word and the object pointer that follows it.
fn start_type_at(payload: &[u8], block: usize) -> Option<u32> {
    let object = object_after_block(payload, block)?;
    let body = if payload.get(object..object + DECLARATION.len()) == Some(DECLARATION) {
        object + DECLARATION.len()
    } else if payload
        .get(object..object + 2)
        .and_then(|bytes| View::u16_le_at(bytes, 0))
        .is_some_and(is_class_token)
    {
        object + 2
    } else {
        return None;
    };
    let kind = View::u32_le_at(payload, body)?;
    if kind == START_PROFILE_PLANE {
        return payload
            .get(body..body + PROFILE_PLANE_BODY_LEN)
            .is_some_and(|zeros| zeros.iter().all(|byte| *byte == 0))
            .then_some(kind);
    }
    object_pointer_at(payload, body + 4).then_some(kind)
}

/// The condition of the extrusion object spanning `span` of the lane payload:
/// the common condition of the start objects written inside it. `None` when no
/// start object has its written form there.
pub(crate) fn object_condition(
    payload: &[u8],
    span: std::ops::Range<usize>,
) -> Option<StartCondition> {
    let end = span.end.min(payload.len());
    let conditions = (span.start..end)
        .filter(|&offset| payload.get(offset..offset + BLOCK_HEADER.len()) == Some(&BLOCK_HEADER))
        .filter_map(|block| start_type_at(payload, block))
        .map(condition)
        .collect::<Vec<_>>();
    (!conditions.is_empty()).then(|| common_condition(&conditions))
}

/// Record `condition` on the history properties of an extrusion. The profile
/// plane leaves them unchanged.
pub(crate) fn insert_condition(
    properties: &mut BTreeMap<NonBlankString, String>,
    condition: StartCondition,
) -> Result<(), cadmpeg_core::CodecError> {
    if condition == StartCondition::Unresolved {
        let key = NonBlankString::new(START_CONDITION.to_owned())
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("blank start field name"))?;
        properties.insert(key, UNRESOLVED.to_owned());
    }
    Ok(())
}

/// The definition start of an extrusion from its history properties: the
/// profile plane when no other start was recorded.
pub(crate) fn extrude_start(properties: &BTreeMap<NonBlankString, String>) -> ExtrudeStart {
    if properties.contains_key(START_CONDITION) {
        ExtrudeStart::Unresolved {}
    } else {
        ExtrudeStart::ProfilePlane {}
    }
}

#[cfg(test)]
mod tests;
