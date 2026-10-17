// SPDX-License-Identifier: Apache-2.0

use super::PrimaryRecords;
use crate::records::group::GroupSelector;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(crate) struct PrimaryGroup {
    pub(crate) xmt: u32,
    pub(crate) node_id: u32,
    pub(crate) references: [u32; 5],
    pub(crate) selector: GroupSelector,
    pub(crate) offset: usize,
    pub(crate) end: usize,
}

impl PrimaryRecords<'_> {
    pub(crate) fn groups(&self, ctx: &DecodeContext<'_>) -> Result<Vec<PrimaryGroup>, CodecError> {
        ctx.collect_retained_vec(
            self.records.iter().filter_map(|record| {
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
            }),
            "NX primary GROUP projections",
        )
    }
}
