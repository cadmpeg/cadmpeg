// SPDX-License-Identifier: Apache-2.0

use super::{PrimaryRecord, PrimaryRecords};
use crate::records::group::{valid_group_references, GroupSelector};

pub(crate) struct PrimaryGroup {
    pub(crate) xmt: u32,
    pub(crate) node_id: u32,
    pub(crate) references: [u32; 5],
    pub(crate) selector: GroupSelector,
    pub(crate) offset: usize,
    pub(crate) end: usize,
}

impl PrimaryRecords<'_> {
    pub(crate) fn groups(&self) -> Vec<PrimaryGroup> {
        self.records
            .iter()
            .filter_map(|record| self.group_record(record))
            .collect()
    }

    pub(super) fn group_record(&self, record: &PrimaryRecord) -> Option<PrimaryGroup> {
        if record.definition.kind != 90 || !self.matches_base(record) {
            return None;
        }
        let references = [
            self.pointer(record, 1)?,
            self.pointer(record, 2)?,
            self.pointer(record, 3)?,
            self.pointer(record, 4)?,
            self.pointer(record, 6)?,
        ];
        valid_group_references(&references).then_some(())?;
        Some(PrimaryGroup {
            xmt: u32::from(record.xmt),
            node_id: self.field(record, 0)?.u32_be()?,
            references,
            selector: GroupSelector::try_from(self.field(record, 5)?.u8()?).ok()?,
            offset: record.range.start,
            end: record.range.end,
        })
    }
}
