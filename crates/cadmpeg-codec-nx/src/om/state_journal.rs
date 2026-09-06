// SPDX-License-Identifier: Apache-2.0
//! Complete state-journal rows with derived extents.

use super::state_index::StateIndexToken;
use super::state_tagged_value::StateTaggedValue;
use cadmpeg_core::decode::View;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JournalRow<O = u64> {
    offset: O,
    timestamp: u32,
    value: StateTaggedValue,
    schema: StateIndexToken,
    ordinal: StateIndexToken,
}

impl<O: Copy> JournalRow<O> {
    pub(crate) fn offset(self) -> O {
        self.offset
    }
    pub(crate) fn timestamp(self) -> u32 {
        self.timestamp
    }
    pub(crate) fn value(self) -> StateTaggedValue {
        self.value
    }
    pub(crate) fn schema(self) -> StateIndexToken {
        self.schema
    }
    pub(crate) fn ordinal(self) -> StateIndexToken {
        self.ordinal
    }
    pub(crate) fn byte_len(self) -> usize {
        6 + self.value.raw().len() + self.schema.raw().len() + self.ordinal.raw().len()
    }
}

impl JournalRow<usize> {
    pub(crate) fn read(bytes: &[u8], at: usize, end: usize, base: usize) -> Option<Self> {
        let tail = bytes.get(at..end)?;
        if tail.first() != Some(&0xe0) {
            return None;
        }
        let timestamp = View::u32_be_at(tail, 1)?;
        let value = StateTaggedValue::read_at(tail, 5)?;
        let schema_at = 5 + value.raw().len();
        let schema = StateIndexToken::read_at(tail, schema_at)?;
        let ordinal_at = schema_at + schema.raw().len();
        let ordinal = StateIndexToken::read_at(tail, ordinal_at)?;
        let terminator_at = ordinal_at + ordinal.raw().len();
        if tail.get(terminator_at) != Some(&0x13) {
            return None;
        }
        let offset = base.checked_add(at)?;
        offset.checked_add(terminator_at + 1)?;
        Some(Self {
            offset,
            timestamp,
            value,
            schema,
            ordinal,
        })
    }

    pub(crate) fn into_absolute(self, base: u64) -> Option<JournalRow> {
        JournalRow::new(
            base.checked_add(u64::try_from(self.offset).ok()?)?,
            self.timestamp,
            self.value,
            self.schema,
            self.ordinal,
        )
        .ok()
    }
}

impl JournalRow {
    pub(crate) fn new(
        offset: u64,
        timestamp: u32,
        value: StateTaggedValue,
        schema: StateIndexToken,
        ordinal: StateIndexToken,
    ) -> Result<Self, &'static str> {
        let row = Self {
            offset,
            timestamp,
            value,
            schema,
            ordinal,
        };
        offset
            .checked_add(row.byte_len() as u64)
            .ok_or("source_offset: journal row extent overflows")?;
        Ok(row)
    }

    pub(crate) fn end_offset(self) -> u64 {
        self.offset + self.byte_len() as u64
    }
}

#[derive(Serialize, Deserialize)]
struct Wire {
    timestamp: u32,
    #[serde(flatten)]
    value: StateTaggedValue,
    schema_id: u32,
    raw_schema_id: Vec<u8>,
    state_ordinal: u32,
    raw_state_ordinal: Vec<u8>,
    source_offset: u64,
    end_offset: u64,
}

impl Serialize for JournalRow {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Wire {
            timestamp: self.timestamp(),
            value: self.value(),
            schema_id: self.schema().value(),
            raw_schema_id: self.schema().raw().to_vec(),
            state_ordinal: self.ordinal().value(),
            raw_state_ordinal: self.ordinal().raw().to_vec(),
            source_offset: self.offset(),
            end_offset: self.end_offset(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JournalRow {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        let schema =
            StateIndexToken::from_wire(wire.schema_id, &wire.raw_schema_id).map_err(|error| {
                serde::de::Error::custom(format!("schema_id/raw_schema_id: {error}"))
            })?;
        let ordinal = StateIndexToken::from_wire(wire.state_ordinal, &wire.raw_state_ordinal)
            .map_err(|error| {
                serde::de::Error::custom(format!("state_ordinal/raw_state_ordinal: {error}"))
            })?;
        let row = Self::new(
            wire.source_offset,
            wire.timestamp,
            wire.value,
            schema,
            ordinal,
        )
        .map_err(serde::de::Error::custom)?;
        if wire.end_offset != row.end_offset() {
            return Err(serde::de::Error::custom(
                "end_offset: disagrees with journal row extent",
            ));
        }
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::JournalRow;

    #[test]
    fn journal_row_wire_preserves_tokens_and_derives_end() {
        let json = r#"{"timestamp":0,"value_marker":255,"value":0,"raw_value":[255,0,0,0,0],"schema_id":0,"raw_schema_id":[144,0,0],"state_ordinal":0,"raw_state_ordinal":[160,0,0],"source_offset":100,"end_offset":117}"#;
        let row: JournalRow = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&row).unwrap(), json);
        let wire: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut mismatch = wire.clone();
        mismatch["end_offset"] = 116.into();
        assert!(serde_json::from_value::<JournalRow>(mismatch)
            .unwrap_err()
            .to_string()
            .contains("end_offset"));
        let mut boundary = wire.clone();
        boundary["source_offset"] = (u64::MAX - 17).into();
        boundary["end_offset"] = u64::MAX.into();
        assert!(serde_json::from_value::<JournalRow>(boundary).is_ok());
        let mut overflow = wire;
        overflow["source_offset"] = (u64::MAX - 16).into();
        overflow["end_offset"] = u64::MAX.into();
        assert!(serde_json::from_value::<JournalRow>(overflow)
            .unwrap_err()
            .to_string()
            .contains("source_offset"));
    }

    #[test]
    fn journal_source_row_bounds_complete_framing() {
        let bytes = [
            0xe0, 0, 0, 0, 0, 0xff, 0, 0, 0, 0, 0x90, 0, 0, 0xa0, 0, 0, 0x13,
        ];
        let row = JournalRow::read(&bytes, 0, bytes.len(), 100).unwrap();
        assert_eq!(row.offset(), 100);
        assert_eq!(row.byte_len(), bytes.len());
        assert_eq!(row.into_absolute(200).unwrap().end_offset(), 317);
        for end in 0..bytes.len() {
            assert!(JournalRow::read(&bytes, 0, end, 100).is_none());
        }
        assert!(JournalRow::read(&bytes, 0, bytes.len(), usize::MAX - 16).is_none());
    }
}
