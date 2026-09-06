// SPDX-License-Identifier: Apache-2.0
//! Entity 51 wire counts are derived from the bounded reference collection.
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use crate::framing::xmt_reference::NonNullXmt;
use super::ParasolidEntity51Record;
use crate::parasolid::entity_references::EntityReferences;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Entity51Wire {
    id: String,
    stream_ordinal: u32,
    xmt: u32,
    flags: u32,
    sequence: u32,
    definition_xmt: u32,
    leading_references: [u32; 5],
    trailing_references: Vec<u32>,
    byte_len: u64,
    inflated_offset: u64,
}
impl From<ParasolidEntity51Record> for Entity51Wire {
    fn from(value: ParasolidEntity51Record) -> Self {
        Self {
            id: value.id, stream_ordinal: value.stream_ordinal, xmt: value.xmt.into(),
            flags: value.trailing_references.values().len() as u32,
            sequence: value.sequence.get(), definition_xmt: value.definition_xmt,
            leading_references: value.leading_references,
            trailing_references: value.trailing_references.into_values(),
            byte_len: value.byte_len, inflated_offset: value.inflated_offset,
        }
    }
}
impl TryFrom<Entity51Wire> for ParasolidEntity51Record {
    type Error = &'static str;
    fn try_from(wire: Entity51Wire) -> Result<Self, Self::Error> {
        let trailing_references = EntityReferences::new(wire.trailing_references)?;
        if wire.flags != trailing_references.values().len() as u32 {
            return Err("flags: must equal trailing_references length");
        }
        Ok(Self {
            id: wire.id, stream_ordinal: wire.stream_ordinal, xmt: NonNullXmt::try_from(wire.xmt).map_err(|_| "xmt must exceed one")?,
            sequence: NonZeroU32::new(wire.sequence).ok_or("sequence must be nonzero")?, definition_xmt: wire.definition_xmt,
            leading_references: wire.leading_references, trailing_references,
            byte_len: wire.byte_len, inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ParasolidEntity51Record;

    #[test]
    fn entity51_wire_derives_count_and_rejects_mismatched_or_unbounded_counts() {
        let json = r#"{"id":"entity","stream_ordinal":0,"xmt":50,"flags":2,"sequence":7,"definition_xmt":34,"leading_references":[60,61,70,71,72],"trailing_references":[70,71],"byte_len":28,"inflated_offset":200}"#;
        let record: ParasolidEntity51Record = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
        for (field, invalid) in [("xmt", 0), ("xmt", 1), ("sequence", 0)] {
            let mut wire = serde_json::to_value(&record).unwrap();
            wire[field] = invalid.into();
            let error = serde_json::from_value::<ParasolidEntity51Record>(wire).unwrap_err();
            assert!(error.to_string().contains(field));
        }
        let mut wire = serde_json::to_value(&record).unwrap();
        wire["flags"] = 1.into();
        assert!(serde_json::from_value::<ParasolidEntity51Record>(wire).unwrap_err().to_string().contains("flags"));
        for count in [0, 1, 32, 33] {
            let mut wire = serde_json::to_value(&record).unwrap();
            wire["flags"] = count.into();
            wire["trailing_references"] = (0..count).map(serde_json::Value::from).collect();
            let decoded = serde_json::from_value::<ParasolidEntity51Record>(wire);
            if matches!(count, 1 | 32) {
                assert_eq!(decoded.unwrap().trailing_references.values().len(), count as usize);
            } else {
                assert!(decoded.unwrap_err().to_string().contains("trailing_references"));
            }
        }
    }
}
