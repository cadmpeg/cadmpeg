// SPDX-License-Identifier: Apache-2.0

use super::PrimaryRecords;
use crate::records::group::GroupSelector;

pub struct PrimaryGroup {
    pub xmt: u32,
    pub node_id: u32,
    pub references: [u32; 5],
    pub selector: GroupSelector,
    pub offset: usize,
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
