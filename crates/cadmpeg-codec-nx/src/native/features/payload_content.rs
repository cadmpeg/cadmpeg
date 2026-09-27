// SPDX-License-Identifier: Apache-2.0
//! Ordered source-block metadata for reconstructed feature payloads.

use serde::{
    ser::{SerializeSeq, SerializeStruct},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FeaturePayloadBlock {
    pub(super) id: String,
    pub(super) byte_len: u64,
    pub(super) source_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FeaturePayloadContent<B> {
    blocks: B,
    sha256: crate::native::hex::Sha256Hex,
}

impl<B: AsRef<[FeaturePayloadBlock]>> FeaturePayloadContent<B> {
    pub(super) fn new(blocks: B, sha256: crate::native::hex::Sha256Hex) -> Result<Self, String> {
        blocks
            .as_ref()
            .iter()
            .try_fold(0u64, |total, block| total.checked_add(block.byte_len))
            .ok_or_else(|| "block_byte_lengths overflow byte_len".to_owned())?;
        Ok(Self { blocks, sha256 })
    }

    pub(super) fn byte_len(&self) -> u64 {
        self.blocks().iter().map(|block| block.byte_len).sum()
    }

    pub(super) fn blocks(&self) -> &[FeaturePayloadBlock] {
        self.blocks.as_ref()
    }

    pub(super) fn block_ids(&self) -> impl ExactSizeIterator<Item = &String> + Clone {
        self.blocks().iter().map(|block| &block.id)
    }
}

impl<B: AsRef<[FeaturePayloadBlock]> + TryFrom<Vec<FeaturePayloadBlock>>> FeaturePayloadContent<B> {
    pub(super) fn from_source(
        ids: impl IntoIterator<Item = String>,
        blocks: &BTreeMap<String, (&[u8], u64)>,
    ) -> Option<(Vec<u8>, Self)> {
        let sources = ids
            .into_iter()
            .map(|id| {
                let (bytes, offset) = *blocks.get(&id)?;
                Some((id, bytes, offset))
            })
            .collect::<Option<Vec<_>>>()?;
        let byte_len = sources.iter().try_fold(0usize, |total, (_, bytes, _)| {
            total.checked_add(bytes.len())
        })?;
        let mut payload = Vec::new();
        payload.try_reserve_exact(byte_len).ok()?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(sources.len()).ok()?;
        for (id, bytes, source_offset) in sources {
            payload.extend_from_slice(bytes);
            rows.push(FeaturePayloadBlock {
                id,
                byte_len: bytes.len() as u64,
                source_offset,
            });
        }
        let content = Self::new(
            B::try_from(rows).ok()?,
            crate::native::hex::Sha256Hex::digest(&payload),
        )
        .ok()?;
        Some((payload, content))
    }
}

#[derive(Deserialize)]
struct PayloadContentWire {
    data_blocks: Vec<String>,
    byte_len: u64,
    sha256: crate::native::hex::Sha256Hex,
    block_payload_offsets: Vec<u64>,
    block_byte_lengths: Vec<u64>,
    block_source_offsets: Vec<u64>,
}

struct BlockIds<'a>(&'a [FeaturePayloadBlock]);

impl Serialize for BlockIds<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut items = serializer.serialize_seq(Some(self.0.len()))?;
        for block in self.0 {
            items.serialize_element(&block.id)?;
        }
        items.end()
    }
}

struct BlockPayloadOffsets<'a>(&'a [FeaturePayloadBlock]);

impl Serialize for BlockPayloadOffsets<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut items = serializer.serialize_seq(Some(self.0.len()))?;
        let mut offset = 0_u64;
        for block in self.0 {
            items.serialize_element(&offset)?;
            offset = offset
                .checked_add(block.byte_len)
                .ok_or_else(|| serde::ser::Error::custom("block_byte_lengths overflow byte_len"))?;
        }
        items.end()
    }
}

struct BlockByteLengths<'a>(&'a [FeaturePayloadBlock]);

impl Serialize for BlockByteLengths<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut items = serializer.serialize_seq(Some(self.0.len()))?;
        for block in self.0 {
            items.serialize_element(&block.byte_len)?;
        }
        items.end()
    }
}

struct BlockSourceOffsets<'a>(&'a [FeaturePayloadBlock]);

impl Serialize for BlockSourceOffsets<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut items = serializer.serialize_seq(Some(self.0.len()))?;
        for block in self.0 {
            items.serialize_element(&block.source_offset)?;
        }
        items.end()
    }
}

impl<B: AsRef<[FeaturePayloadBlock]>> Serialize for FeaturePayloadContent<B> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let blocks = self.blocks();
        let byte_len = blocks
            .iter()
            .try_fold(0_u64, |total, block| total.checked_add(block.byte_len))
            .ok_or_else(|| serde::ser::Error::custom("block_byte_lengths overflow byte_len"))?;
        let mut fields = serializer.serialize_struct("PayloadContentWire", 6)?;
        fields.serialize_field("data_blocks", &BlockIds(blocks))?;
        fields.serialize_field("byte_len", &byte_len)?;
        fields.serialize_field("sha256", &self.sha256)?;
        fields.serialize_field("block_payload_offsets", &BlockPayloadOffsets(blocks))?;
        fields.serialize_field("block_byte_lengths", &BlockByteLengths(blocks))?;
        fields.serialize_field("block_source_offsets", &BlockSourceOffsets(blocks))?;
        fields.end()
    }
}

impl<'de, B> Deserialize<'de> for FeaturePayloadContent<B>
where
    B: AsRef<[FeaturePayloadBlock]> + TryFrom<Vec<FeaturePayloadBlock>>,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = PayloadContentWire::deserialize(deserializer)?;
        let count = wire.data_blocks.len();
        if wire.block_payload_offsets.len() != count
            || wire.block_byte_lengths.len() != count
            || wire.block_source_offsets.len() != count
        {
            return Err(serde::de::Error::custom("data_blocks, block_payload_offsets, block_byte_lengths and block_source_offsets must have equal lengths"));
        }
        let mut byte_len = 0u64;
        for (offset, length) in wire
            .block_payload_offsets
            .iter()
            .zip(&wire.block_byte_lengths)
        {
            if *offset != byte_len {
                return Err(serde::de::Error::custom(
                    "block_payload_offsets must equal cumulative block_byte_lengths",
                ));
            }
            byte_len = byte_len
                .checked_add(*length)
                .ok_or_else(|| serde::de::Error::custom("block_byte_lengths overflow byte_len"))?;
        }
        if byte_len != wire.byte_len {
            return Err(serde::de::Error::custom(
                "byte_len must equal the sum of block_byte_lengths",
            ));
        }
        let rows = wire
            .data_blocks
            .into_iter()
            .zip(wire.block_byte_lengths)
            .zip(wire.block_source_offsets)
            .map(|((id, byte_len), source_offset)| FeaturePayloadBlock {
                id,
                byte_len,
                source_offset,
            })
            .collect();
        let blocks = B::try_from(rows).map_err(|_| {
            serde::de::Error::custom("data_blocks count does not match the payload lane")
        })?;
        Self::new(blocks, wire.sha256).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{FeaturePayloadBlock, FeaturePayloadContent};

    #[test]
    fn payload_content_columns_stream_once_with_native_retained_limit() {
        #[derive(serde::Serialize)]
        struct Row<'a> {
            id: &'static str,
            content: &'a FeaturePayloadContent<Vec<FeaturePayloadBlock>>,
        }

        let content: FeaturePayloadContent<Vec<FeaturePayloadBlock>> =
            serde_json::from_str(TWO_BLOCKS).unwrap();
        let record = Row {
            id: "nx:feature-history:payload#1",
            content: &content,
        };
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({
                "id": record.id,
                "content": serde_json::from_str::<serde_json::Value>(TWO_BLOCKS).unwrap(),
            }),
        );
    }

    const TWO_BLOCKS: &str = r#"{"data_blocks":["first","second"],"byte_len":8,"sha256":"d04b98f48e8f8bcc15c6ae5ac050801cd6dcfd428fb5f9e65c4e16e7807340fa","block_payload_offsets":[0,3],"block_byte_lengths":[3,5],"block_source_offsets":[10,100]}"#;

    #[test]
    fn payload_content_preserves_ordered_metadata_wire() {
        let content: FeaturePayloadContent<[FeaturePayloadBlock; 2]> =
            serde_json::from_str(TWO_BLOCKS).unwrap();
        assert_eq!(content.byte_len(), 8);
        assert_eq!(serde_json::to_string(&content).unwrap(), TWO_BLOCKS);
        let variable: FeaturePayloadContent<Vec<FeaturePayloadBlock>> =
            serde_json::from_str(TWO_BLOCKS).unwrap();
        assert_eq!(serde_json::to_string(&variable).unwrap(), TWO_BLOCKS);
    }

    #[test]
    fn payload_content_rejects_inconsistent_metadata() {
        for (before, after, field) in [
            ("\"first\",\"second\"", "\"first\"", "data_blocks"),
            ("[0,3]", "[0]", "block_payload_offsets"),
            ("[3,5]", "[3]", "block_byte_lengths"),
            ("[10,100]", "[10]", "block_source_offsets"),
            ("[0,3]", "[1,3]", "block_payload_offsets"),
            ("[0,3]", "[0,4]", "block_payload_offsets"),
            ("\"byte_len\":8", "\"byte_len\":9", "byte_len"),
        ] {
            let invalid = TWO_BLOCKS.replace(before, after);
            let error =
                serde_json::from_str::<FeaturePayloadContent<Vec<FeaturePayloadBlock>>>(&invalid)
                    .unwrap_err();
            assert!(error.to_string().contains(field));
        }
        let error =
            serde_json::from_str::<FeaturePayloadContent<[FeaturePayloadBlock; 5]>>(TWO_BLOCKS)
                .unwrap_err();
        assert!(error.to_string().contains("data_blocks"));
    }

    #[test]
    fn payload_content_rejects_total_length_overflow() {
        let json = TWO_BLOCKS
            .replace("[3,5]", "[18446744073709551615,1]")
            .replace("[0,3]", "[0,18446744073709551615]");
        let error = serde_json::from_str::<FeaturePayloadContent<Vec<FeaturePayloadBlock>>>(&json)
            .unwrap_err();
        assert!(error.to_string().contains("block_byte_lengths"));
        let blocks = [
            FeaturePayloadBlock {
                id: "first".to_owned(),
                byte_len: u64::MAX,
                source_offset: 0,
            },
            FeaturePayloadBlock {
                id: "second".to_owned(),
                byte_len: 1,
                source_offset: 0,
            },
        ];
        assert!(
            FeaturePayloadContent::new(blocks, crate::native::hex::Sha256Hex::digest(b"hash"))
                .is_err()
        );
    }
}
