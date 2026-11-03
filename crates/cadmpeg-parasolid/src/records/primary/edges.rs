// SPDX-License-Identifier: Apache-2.0

use super::PrimaryRecords;
use crate::xmt::NonNullXmt;
use std::collections::BTreeSet;
use std::ops::Range;

/// The frame of one type-16 EDGE node.
pub struct EdgeFrame {
    /// The node's encoded index.
    pub xmt: NonNullXmt,
    /// The node's byte range in the stream.
    pub range: Range<usize>,
    /// Byte offset of the fixed fields of a unique base-schema EDGE node.
    pub fixed_fields_at: Option<usize>,
}

impl PrimaryRecords<'_> {
    /// Frames of the type-16 EDGE nodes in stream order.
    pub fn edge_frames(&self) -> Vec<EdgeFrame> {
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
