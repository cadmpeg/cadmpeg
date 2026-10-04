// SPDX-License-Identifier: Apache-2.0
//! Scalar dictionary wire projection with positional indices and derived extent.

use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};

use crate::container::ModelDoubleXarTable;
use crate::scalar::DoubleXarSlot;

pub(in crate::decode) struct CreoDoubleXarTableRecord<'a> {
    pub(in crate::decode) id: String,
    pub(in crate::decode) table: &'a ModelDoubleXarTable,
}

impl Serialize for CreoDoubleXarTableRecord<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("CreoDoubleXarTableRecord", 6)?;
        record.serialize_field("id", &self.id)?;
        record.serialize_field("section_name", &self.table.section_name)?;
        record.serialize_field("section_source_offset", &self.table.section_source_offset)?;
        record.serialize_field("expanded_offset", &self.table.expanded_offset)?;
        record.serialize_field("count", &self.table.entries.len())?;
        record.serialize_field("entries", &Entries(&self.table.entries))?;
        record.end()
    }
}

struct Entries<'a>(&'a [DoubleXarSlot]);

impl Serialize for Entries<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Entry<'a> {
            index: usize,
            raw: &'a [u8],
            value: Option<f64>,
            kind: &'static str,
        }

        let mut entries = serializer.serialize_seq(Some(self.0.len()))?;
        for (index, entry) in self.0.iter().enumerate() {
            entries.serialize_element(&Entry {
                index,
                raw: entry.raw(),
                value: entry.value(),
                kind: entry.kind(),
            })?;
        }
        entries.end()
    }
}
