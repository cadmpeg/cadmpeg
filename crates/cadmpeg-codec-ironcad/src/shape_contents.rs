// SPDX-License-Identifier: Apache-2.0
//! Scene objects of the `Shape Contents` stream (`ironcad.md` §6).

use cadmpeg_core::decode::View;

use crate::layout::{shape_object_header as header, shape_object_item as item};
use crate::placement::{find, name_at, NAME_MARKER};

/// Item kind that a scene object with a body holds.
pub(crate) const BODY_ITEM: u16 = 20;

/// Item kind that a part and an assembly hold.
pub(crate) const MEMBER_ITEM: u16 = 25;

/// One scene object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SceneObject {
    /// Identifier, distinct among the children of one parent.
    pub(crate) id: u32,
    pub(crate) name: String,
    /// Offset in the `Brep Data` payload where the kernel data of the object
    /// ends.
    pub(crate) end: u64,
    pub(crate) kind: ObjectKind,
}

/// What a scene object is, by the items it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectKind {
    /// A part or a feature: the object holds a body item.
    Body,
    /// An assembly: the object holds a member item and no body item.
    Assembly,
    Other,
}

/// Reads the scene objects of a `Shape Contents` payload in stored order.
pub(crate) fn objects(payload: &[u8]) -> Vec<SceneObject> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = find(payload, &NAME_MARKER, at) {
        at = found + NAME_MARKER.len();
        if let Some(object) = object_at(payload, found) {
            out.push(object);
        }
    }
    out
}

/// The scene object whose name marker is at `name`.
fn object_at(payload: &[u8], name: usize) -> Option<SceneObject> {
    let start = name.checked_sub(header::LEN)?;
    if View::u32_le_at(payload, start + header::RESERVED)? != 0 {
        return None;
    }
    let text = name_at(payload, name)?;
    let after = name + NAME_MARKER.len() + 1 + 2 * text.encode_utf16().count();
    if View::u32_le_at(payload, after)? != 0 {
        return None;
    }
    let end = View::u64_le_at(payload, start + header::END)?;
    let kinds = item_kinds(payload, after + 4)?;
    let kind = if kinds.contains(&BODY_ITEM) {
        ObjectKind::Body
    } else if kinds.contains(&MEMBER_ITEM) {
        ObjectKind::Assembly
    } else {
        ObjectKind::Other
    };
    Some(SceneObject {
        id: View::u32_le_at(payload, start + header::ID)?,
        name: text,
        end,
        kind,
    })
}

/// The kinds of the items from `at` to the terminating item, or `None` when
/// an item breaks its form before the terminator.
fn item_kinds(payload: &[u8], mut at: usize) -> Option<Vec<u16>> {
    let mut kinds = Vec::new();
    loop {
        payload.get(at..at + item::LEN)?;
        if View::u32_le_at(payload, at + item::TAG)? != item::TAG_VALUE {
            return None;
        }
        if View::u64_le_at(payload, at + item::HANDLE)? == u64::MAX {
            return Some(kinds);
        }
        if View::u16_le_at(payload, at + item::COUNT)? != item::COUNT_VALUE {
            return None;
        }
        kinds.push(View::u16_le_at(payload, at + item::KIND)?);
        at += item::LEN;
    }
}

#[cfg(test)]
mod tests;
