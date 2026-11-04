// SPDX-License-Identifier: Apache-2.0
//! Scalar dictionary wire projection with positional indices and derived extent.

use cadmpeg_ir::native::bytes::NativeBytes;

use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};

use crate::container::ModelDoubleXarTable;
use crate::double_xar::DoubleXarSlot;

pub(in crate::decode) struct CreoDoubleXarTableRecord<'a> {
    pub(in crate::decode) id: String,
    pub(in crate::decode) table: &'a ModelDoubleXarTable,
}

impl Serialize for CreoDoubleXarTableRecord<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("CreoDoubleXarTableRecord", 7)?;
        record.serialize_field("id", &self.id)?;
        record.serialize_field("section_name", &self.table.section_name)?;
        record.serialize_field("section_source_offset", &self.table.section_source_offset)?;
        record.serialize_field("expanded_offset", &self.table.expanded_offset)?;
        record.serialize_field("body_offset", &self.table.body_offset)?;
        record.serialize_field("count", &self.table.logical_count)?;
        record.serialize_field(
            "entries",
            &Entries {
                slots: &self.table.entries,
                body_offset: self.table.body_offset,
            },
        )?;
        record.end()
    }
}

struct Entries<'a> {
    slots: &'a [DoubleXarSlot],
    body_offset: usize,
}

impl Serialize for Entries<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Entry<'a> {
            index: usize,
            offset: usize,
            logical_width: usize,
            raw: NativeBytes<&'a [u8]>,
            value: Option<f64>,
            kind: &'static str,
        }

        let mut entries = serializer.serialize_seq(Some(self.slots.len()))?;
        let mut index = 0;
        let mut offset = self.body_offset;
        for entry in self.slots {
            entries.serialize_element(&Entry {
                index,
                offset,
                logical_width: entry.logical_width(),
                raw: (entry.raw()).into(),
                value: entry.value(),
                kind: entry.kind(),
            })?;
            index += entry.logical_width();
            offset += entry.raw().len();
        }
        entries.end()
    }
}
