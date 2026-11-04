// SPDX-License-Identifier: Apache-2.0
//! Record-timestamp evidence for scope references that name deleted members.

use crate::layout::record_timestamp_entry as entry;
use crate::layout::record_timestamp_table_prefix as table;
use crate::records::identity::Located;
use crate::records::references::DesignClassTag;
use serde::{Deserialize, Serialize};

/// The record-timestamp table of a scope and the scope references that name
/// deleted members, located at their table entries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignDeletedMembers {
    /// Record index of the record-timestamp table.
    pub(crate) table_record_index: u32,
    /// Byte offset of the table's indexed header.
    pub(crate) table_byte_offset: u64,
    /// Per-file dynamic class tag of the table.
    pub(crate) table_class_tag: DesignClassTag,
    /// Number of entries before the owner entry.
    pub(crate) entry_count: u32,
    /// Record index named by the owner entry, at the offset of its encoding.
    pub(crate) owner: Located<u32>,
    /// Deleted member record indices, each at the offset of its entry's
    /// record index.
    pub(crate) members: Vec<Located<u32>>,
}

impl DesignDeletedMembers {
    /// Offset of the record index of table entry `ordinal`; the owner entry
    /// follows the counted entries.
    pub(crate) fn entry_index_offset(&self, ordinal: u32) -> Option<u64> {
        let stride = u64::try_from(entry::LEN).ok()?;
        self.table_byte_offset
            .checked_add(u64::try_from(table::LEN).ok()?)?
            .checked_add(u64::from(ordinal).checked_mul(stride)?)?
            .checked_add(u64::try_from(entry::RECORD_INDEX).ok()?)
    }

    /// Whether the owner names `scope_record_index`, the owner and every member
    /// sit at entry offsets of the table, and the members are distinct.
    pub(crate) fn frame_holds(&self, scope_record_index: u32) -> bool {
        let owner_holds = self.owner.value == scope_record_index
            && self.entry_index_offset(self.entry_count) == Some(self.owner.offset);
        let members_hold = !self.members.is_empty()
            && self.members.iter().enumerate().all(|(position, member)| {
                (0..self.entry_count)
                    .any(|ordinal| self.entry_index_offset(ordinal) == Some(member.offset))
                    && self.members[..position]
                        .iter()
                        .all(|earlier| earlier.value != member.value)
            });
        owner_holds && members_hold
    }
}

rewrite_native_record!(DesignDeletedMembers, []; {table_record_index, table_byte_offset, table_class_tag, entry_count, owner, members});
