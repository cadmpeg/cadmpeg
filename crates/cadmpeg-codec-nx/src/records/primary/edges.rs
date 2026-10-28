// SPDX-License-Identifier: Apache-2.0

use super::PrimaryRecords;
use crate::framing::xmt_reference::NonNullXmt;
use std::collections::BTreeSet;
use std::ops::Range;

pub(crate) struct EdgeFrame {
    pub(crate) xmt: NonNullXmt,
    pub(crate) range: Range<usize>,
    pub(crate) fixed_fields_at: Option<usize>,
}

impl PrimaryRecords<'_> {
    pub(crate) fn edge_frames(&self) -> Vec<EdgeFrame> {
        let mut seen = BTreeSet::new();
        let mut duplicates = BTreeSet::new();
        for record in self.records.iter().filter(|r| r.definition.kind == 16) {
            let xmt = u32::from(record.xmt);
            if !seen.insert(xmt) {
                duplicates.insert(xmt);
            }
        }
        self.records
            .iter()
            .filter(|record| record.definition.kind == 16)
            .map(|record| EdgeFrame {
                xmt: record.xmt,
                range: record.range.clone(),
                fixed_fields_at: if self.matches_base(record)
                    && !duplicates.contains(&u32::from(record.xmt))
                {
                    record.fields.first().map(|field| field.start)
                } else {
                    None
                },
            })
            .collect()
    }
}
