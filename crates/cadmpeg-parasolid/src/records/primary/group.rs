// SPDX-License-Identifier: Apache-2.0

use super::PrimaryRecords;
use crate::records::group::GroupSelector;

/// One type-90 GROUP node with base-schema fields.
pub struct PrimaryGroup {
    /// The node's encoded index.
    pub xmt: u32,
    /// The node's `node_id`.
    pub node_id: u32,
    /// The pointer fields at base-schema positions 1 through 4 and 6.
    pub references: [u32; 5],
    /// The member class that the type byte at base-schema position 5 selects.
    pub selector: GroupSelector,
    /// Byte offset of the node in the stream.
    pub offset: usize,
    /// Byte offset just past the node.
    pub end: usize,
}

impl PrimaryRecords<'_> {
    /// Type-90 GROUP nodes in stream order.
    pub fn groups(&self) -> Vec<PrimaryGroup> {
        (self.records.iter().filter_map(|record| {
            if record.definition.kind != 90 {
                return None;
            }
            if !self.matches_base(record) {
                return None;
            }
            Some(PrimaryGroup {
                xmt: u32::from(record.xmt),
                node_id: self.field(record, 0)?.u32_be()?,
                references: [
                    self.pointer(record, 1)?,
                    self.pointer(record, 2)?,
                    self.pointer(record, 3)?,
                    self.pointer(record, 4)?,
                    self.pointer(record, 6)?,
                ],
                selector: GroupSelector::try_from(self.field(record, 5)?.u8()?).ok()?,
                offset: record.range.start,
                end: record.range.end,
            })
        }))
        .collect::<Vec<_>>()
    }
}
