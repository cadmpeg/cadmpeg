// SPDX-License-Identifier: Apache-2.0
//! PSM object storage: the space map, the cluster objects and the type table
//! (`docs/formats/solidedge.md` §3).
//!
//! Every reader returns `None` for a stream that does not parse to the
//! grammar the specification states; the caller turns that into a loss. A
//! count read from the stream is checked against the bytes that remain before
//! any loop runs, and no collection is sized from a parsed count.

use std::collections::BTreeMap;

use cadmpeg_core::decode::View;

use crate::layout::{
    cluster_header, object_frame, space_map_header, space_map_item, type_compact_record,
    type_declaration, type_table_section_header as section_header,
};

/// The space map of cluster 0.
pub(crate) const SPACE_MAP_STREAM: &str = "PSMspacemap/0x00000000";
/// The type table.
pub(crate) const TYPE_TABLE_STREAM: &str = "PSMtypetable";

/// Bit of a space-map item's flags that selects the class rule's declaration
/// branch (§3.1, §3.3).
const DECLARATION_FLAG: u16 = 0x2000;
/// The compact-record value that names no declaration (§3.3).
const NO_DECLARATION: u16 = 0xFFFF;
/// Bits below the form in an item's form word (§3.1).
const FORM_SHIFT: u32 = 6;

/// One item of a space map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpaceItem {
    /// The object's offset in the cluster stream.
    pub(crate) offset: usize,
    /// The object identifier.
    pub(crate) id: u32,
    /// The item's form word; the object's form is this value shifted right by
    /// six bits.
    pub(crate) form_word: u16,
    /// The item's flags.
    pub(crate) flags: u16,
}

/// A parsed space map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpaceMap {
    items: Vec<SpaceItem>,
}

impl SpaceMap {
    /// Parse a space-map stream. The items end at the stream end.
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.get(..space_map_header::MAGIC_VALUE.len())? != space_map_header::MAGIC_VALUE {
            return None;
        }
        let count = usize::from(View::u16_le_at(bytes, space_map_header::ITEM_COUNT)?);
        let free = usize::from(View::u16_le_at(bytes, space_map_header::FREE_COUNT)?);
        let mut at = space_map_header::LEN.checked_add(free.checked_mul(2)?)?;
        if count > bytes.len().checked_sub(at)? / space_map_item::LEN {
            return None;
        }
        let mut items = Vec::new();
        for _ in 0..count {
            let length = usize::from(View::u16_le_at(
                bytes,
                at.checked_add(space_map_item::LENGTH)?,
            )?);
            // The length word does not count itself.
            let size = length.checked_add(2)?;
            if size < space_map_item::LEN {
                return None;
            }
            let end = at.checked_add(size)?;
            if end > bytes.len() {
                return None;
            }
            items.push(SpaceItem {
                offset: usize::try_from(View::u32_le_at(
                    bytes,
                    at.checked_add(space_map_item::OBJECT_OFFSET)?,
                )?)
                .ok()?,
                id: View::u32_le_at(bytes, at.checked_add(space_map_item::OBJECT_ID)?)?,
                form_word: View::u16_le_at(bytes, at.checked_add(space_map_item::FORM_WORD)?)?,
                flags: View::u16_le_at(bytes, at.checked_add(space_map_item::FLAGS)?)?,
            });
            at = end;
        }
        (at == bytes.len()).then_some(Self { items })
    }

    /// The items in stored order.
    #[cfg(test)]
    pub(crate) fn items(&self) -> &[SpaceItem] {
        &self.items
    }
}

/// One object of a cluster: its frame fields and its bytes, frame included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Object<'a> {
    pub(crate) id: u32,
    pub(crate) form: u16,
    pub(crate) flags: u16,
    pub(crate) bytes: &'a [u8],
}

/// A cluster stream read through its space map.
#[derive(Debug, Clone)]
pub(crate) struct Cluster<'a> {
    bytes: &'a [u8],
    map: &'a SpaceMap,
    by_id: BTreeMap<u32, usize>,
}

impl<'a> Cluster<'a> {
    /// Check the cluster header against `map`. An object whose frame does not
    /// match its item is unreadable on its own; the cluster stays readable.
    pub(crate) fn parse(bytes: &'a [u8], map: &'a SpaceMap) -> Option<Self> {
        if View::u32_le_at(bytes, cluster_header::MAGIC)? != cluster_header::MAGIC_VALUE {
            return None;
        }
        let count = usize::try_from(View::u32_le_at(bytes, cluster_header::OBJECT_COUNT)?).ok()?;
        if count != map.items.len() {
            return None;
        }
        let mut by_id = BTreeMap::new();
        for (index, item) in map.items.iter().enumerate() {
            by_id.entry(item.id).or_insert(index);
        }
        Some(Self { bytes, map, by_id })
    }

    /// The object at item `index`, when its bytes lie in the stream and its
    /// frame matches its item.
    fn object_at(&self, index: usize) -> Option<Object<'a>> {
        let item = self.map.items.get(index)?;
        let end = match self.map.items.get(index.checked_add(1)?) {
            Some(next) => next.offset,
            None => self.bytes.len(),
        };
        if item.offset < cluster_header::LEN || end > self.bytes.len() {
            return None;
        }
        let bytes = self.bytes.get(item.offset..end)?;
        if bytes.len() < object_frame::LEN {
            return None;
        }
        let form = item.form_word >> FORM_SHIFT;
        let framed = View::u16_le_at(bytes, object_frame::TAG)? == object_frame::TAG_VALUE
            && View::u16_le_at(bytes, object_frame::FORM)? == form
            && View::u32_le_at(bytes, object_frame::OBJECT_ID)? == item.id;
        framed.then_some(Object {
            id: item.id,
            form,
            flags: item.flags,
            bytes,
        })
    }

    /// The object with identifier `id`.
    pub(crate) fn object(&self, id: u32) -> Option<Object<'a>> {
        self.object_at(*self.by_id.get(&id)?)
    }

    /// Every readable object in stored order.
    pub(crate) fn objects(&self) -> impl Iterator<Item = Object<'a>> + '_ {
        (0..self.map.items.len()).filter_map(|index| self.object_at(index))
    }
}

/// A parsed type table: the class names and the form-to-declaration links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeTable {
    names: Vec<String>,
    links: Vec<u16>,
}

impl TypeTable {
    /// Parse a `PSMtypetable` stream: the named declarations, then the
    /// compact records to the stream end.
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        let declared = section_count(bytes, 0)?;
        let mut at = section_header::LEN;
        if declared > bytes.len().checked_sub(at)? / type_declaration::LEN {
            return None;
        }
        let mut names = Vec::new();
        for expected in 0..declared {
            let (name, end) = declaration(bytes, at, expected)?;
            names.push(name);
            at = end;
        }
        let compact = section_count(bytes, at)?;
        let first = at.checked_add(section_header::LEN)?;
        let end = first.checked_add(compact.checked_mul(type_compact_record::LEN)?)?;
        if end != bytes.len() {
            return None;
        }
        let mut links = Vec::new();
        for index in 0..compact {
            let record = first.checked_add(index.checked_mul(type_compact_record::LEN)?)?;
            links.push(View::u16_le_at(
                bytes,
                record.checked_add(type_compact_record::LINK)?,
            )?);
        }
        Some(Self { names, links })
    }

    /// The class name of an object of form `form` whose space-map item has
    /// flags `flags` (§3.3).
    pub(crate) fn class_name(&self, form: u16, flags: u16) -> Option<&str> {
        let index = usize::from(form);
        let declaration = match self.links.get(index) {
            Some(&link) if flags & DECLARATION_FLAG == 0 && link != NO_DECLARATION => {
                usize::from(link)
            }
            _ => index,
        };
        self.names.get(declaration).map(String::as_str)
    }
}

/// The record count of the section header at `at`, when its two counts are
/// equal.
fn section_count(bytes: &[u8], at: usize) -> Option<usize> {
    let magic = bytes.get(at..at.checked_add(section_header::MAGIC_VALUE.len())?)?;
    if magic != section_header::MAGIC_VALUE {
        return None;
    }
    let count = View::u16_le_at(bytes, at.checked_add(section_header::COUNT)?)?;
    let repeat = View::u16_le_at(bytes, at.checked_add(section_header::COUNT_REPEAT)?)?;
    (count == repeat).then_some(usize::from(count))
}

/// The class name of the named declaration at `at` and the offset after it.
fn declaration(bytes: &[u8], at: usize, expected: usize) -> Option<(String, usize)> {
    let index = usize::try_from(View::u32_le_at(
        bytes,
        at.checked_add(type_declaration::INDEX)?,
    )?)
    .ok()?;
    let fixed = View::u16_le_at(bytes, at.checked_add(type_declaration::MARKER_ONE)?)?
        == type_declaration::MARKER_ONE_VALUE;
    if index != expected || !fixed {
        return None;
    }
    let name_len = usize::from(View::u16_le_at(
        bytes,
        at.checked_add(type_declaration::NAME_LEN)?,
    )?);
    let name_at = at.checked_add(type_declaration::LEN)?;
    let name_end = name_at.checked_add(name_len)?;
    let (terminator, name) = bytes.get(name_at..name_end)?.split_last()?;
    if *terminator != 0 || !name.is_ascii() {
        return None;
    }
    let text_len = usize::from(View::u16_le_at(bytes, name_end)?);
    let end = name_end.checked_add(2)?.checked_add(text_len)?;
    if end > bytes.len() {
        return None;
    }
    Some((String::from_utf8(name.to_vec()).ok()?, end))
}

#[cfg(test)]
mod tests;
