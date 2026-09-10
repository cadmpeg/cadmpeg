// SPDX-License-Identifier: Apache-2.0
//! Unlabeled operation records with one checked header and payload extent.

use crate::om::header_references::{HeaderReferences, OperationHeader};
use crate::om::reference_index::FeatureReferenceToken;
use crate::om::UnlabeledOperationRecord;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "UnlabeledRecordWire", into = "UnlabeledRecordWire")]
pub(crate) struct FeatureUnlabeledOperationRecord {
    pub(crate) id: String,
    ordinal: u32,
    header: OperationHeader<u64>,
    sha256: crate::native::hex::Sha256Hex,
    payload_byte_len: u64,
    payload_sha256: crate::native::hex::Sha256Hex,
}

impl FeatureUnlabeledOperationRecord {
    pub(crate) fn from_source(
        id: String,
        ordinal: u32,
        entry_offset: u64,
        record: UnlabeledOperationRecord<'_>,
    ) -> Option<Self> {
        let header = OperationHeader::<u64>::new(
            entry_offset.checked_add(record.header().offset() as u64)?,
            record.header().objects(),
        )?;
        let payload_byte_len = record.payload().len() as u64;
        header.end_offset().checked_add(payload_byte_len)?;
        Some(Self {
            id,
            ordinal,
            header,
            sha256: crate::native::hex::Sha256Hex::digest(record.bytes()),
            payload_byte_len,
            payload_sha256: crate::native::hex::Sha256Hex::digest(record.payload()),
        })
    }

    pub(crate) fn source_offset(&self) -> u64 {
        self.header.offset()
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UnlabeledRecordWire {
    id: String,
    ordinal: u32,
    object_indices: [Option<u32>; 4],
    object_index_source_offsets: [u64; 4],
    byte_len: u64,
    sha256: crate::native::hex::Sha256Hex,
    payload_byte_len: u64,
    payload_sha256: crate::native::hex::Sha256Hex,
    payload_source_offset: u64,
    source_offset: u64,
}

impl From<FeatureUnlabeledOperationRecord> for UnlabeledRecordWire {
    fn from(value: FeatureUnlabeledOperationRecord) -> Self {
        Self {
            id: value.id,
            ordinal: value.ordinal,
            object_indices: value.header.objects().values(),
            object_index_source_offsets: value.header.object_offsets(),
            byte_len: u64::from(value.header.byte_len()) + value.payload_byte_len,
            sha256: value.sha256,
            payload_byte_len: value.payload_byte_len,
            payload_sha256: value.payload_sha256,
            payload_source_offset: value.header.end_offset(),
            source_offset: value.header.offset(),
        }
    }
}

impl TryFrom<UnlabeledRecordWire> for FeatureUnlabeledOperationRecord {
    type Error = String;

    fn try_from(wire: UnlabeledRecordWire) -> Result<Self, Self::Error> {
        let mut tokens = [None; 4];
        for (slot, token) in tokens.iter_mut().enumerate() {
            let end = wire
                .object_index_source_offsets
                .get(slot + 1)
                .copied()
                .unwrap_or(wire.payload_source_offset);
            let width = end
                .checked_sub(wire.object_index_source_offsets[slot])
                .ok_or_else(|| {
                    format!("object_index_source_offsets[{slot}]: reversed token extent")
                })?;
            *token = match wire.object_indices[slot] {
                None if width == 1 => None,
                None => return Err(format!("object_index_source_offsets[{slot}]: null requires one byte")),
                Some(value) => Some(FeatureReferenceToken::with_width(value, width)
                    .ok_or_else(|| format!("object_indices/object_index_source_offsets[{slot}]: invalid value or token width"))?),
            };
        }
        let header = OperationHeader::<u64>::new(wire.source_offset, HeaderReferences(tokens))
            .ok_or("source_offset: operation header end overflows")?;
        if header.object_offsets() != wire.object_index_source_offsets {
            return Err("object_index_source_offsets: positions disagree with header".into());
        }
        if header.end_offset() != wire.payload_source_offset {
            return Err("payload_source_offset: position disagrees with header".into());
        }
        header
            .end_offset()
            .checked_add(wire.payload_byte_len)
            .ok_or("payload_byte_len: record end overflows")?;
        if u64::from(header.byte_len()) + wire.payload_byte_len != wire.byte_len {
            return Err("byte_len: length disagrees with header and payload".into());
        }
        Ok(Self {
            id: wire.id,
            ordinal: wire.ordinal,
            header,
            sha256: wire.sha256,
            payload_byte_len: wire.payload_byte_len,
            payload_sha256: wire.payload_sha256,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::FeatureUnlabeledOperationRecord;

    const WIRE: &str = r#"{"id":"record","ordinal":0,"object_indices":[null,0,0,0],"object_index_source_offsets":[115,116,117,119],"byte_len":25,"sha256":"e3435e1ec46c3583cddf3562de1ac4b15f5cf950be3f42d3dd273d6f5b756b95","payload_byte_len":3,"payload_sha256":"47ac2ba87d3f6c174479809b0a1ea8f32a654ec0044301278e6c822375d33e75","payload_source_offset":122,"source_offset":100}"#;

    #[test]
    fn unlabeled_record_wire_retains_header_widths_and_payload_extent() {
        let record: FeatureUnlabeledOperationRecord = serde_json::from_str(WIRE).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), WIRE);
        assert_eq!(
            record
                .header
                .objects()
                .0
                .map(|token| token.map(|token| token.raw().to_vec())),
            [
                None,
                Some(vec![0]),
                Some(vec![128, 0]),
                Some(vec![144, 0, 0])
            ]
        );
        for (field, value) in [
            ("object_indices", serde_json::json!([null, 128, 0, 0])),
            ("object_indices", serde_json::json!([null, 0, 4096, 0])),
            ("object_indices", serde_json::json!([null, 0, 0, 65536])),
            ("object_indices", serde_json::json!([null, 0, 0, null])),
            (
                "object_index_source_offsets",
                serde_json::json!([114, 116, 117, 119]),
            ),
            (
                "object_index_source_offsets",
                serde_json::json!([115, 116, 118, 117]),
            ),
            (
                "object_index_source_offsets",
                serde_json::json!([114, 115, 116, 118]),
            ),
            ("payload_source_offset", serde_json::json!(123)),
            ("byte_len", serde_json::json!(24)),
            ("payload_byte_len", serde_json::json!(u64::MAX)),
            ("source_offset", serde_json::json!(u64::MAX)),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(WIRE).unwrap();
            invalid[field] = value;
            assert!(
                serde_json::from_value::<FeatureUnlabeledOperationRecord>(invalid).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn unlabeled_record_wire_accepts_the_last_representable_end() {
        let mut wire: serde_json::Value = serde_json::from_str(WIRE).unwrap();
        let start = u64::MAX - 25;
        wire["source_offset"] = serde_json::json!(start);
        wire["object_index_source_offsets"] =
            serde_json::json!([start + 15, start + 16, start + 17, start + 19]);
        wire["payload_source_offset"] = serde_json::json!(start + 22);
        let record: FeatureUnlabeledOperationRecord = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
        wire["payload_byte_len"] = serde_json::json!(4);
        assert!(
            serde_json::from_value::<FeatureUnlabeledOperationRecord>(wire)
                .unwrap_err()
                .to_string()
                .contains("payload_byte_len")
        );
    }
}
