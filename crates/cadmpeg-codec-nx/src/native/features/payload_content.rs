// SPDX-License-Identifier: Apache-2.0
//! Ordered source-block metadata for reconstructed feature payloads.

use cadmpeg_core::decode::scan::AdmittedIter;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use serde::{
    ser::{SerializeSeq, SerializeStruct},
    Deserialize, Deserializer, Serialize, Serializer,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::convert::Infallible;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FeaturePayloadBlock {
    pub(super) id: String,
    pub(super) byte_len: u64,
    pub(super) source_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FeaturePayloadContent<B> {
    blocks: B,
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
}

impl<B: AsRef<[FeaturePayloadBlock]>> FeaturePayloadContent<B> {
    pub(super) fn new<A: FeaturePayloadBlockAdmission>(
        blocks: B,
        sha256: cadmpeg_ir::hash::digest::Sha256Digest,
        admission: &A,
    ) -> Result<Result<Self, String>, A::Error> {
        let total = admission
            .admit(blocks.as_ref())?
            .try_fold(0u64, |total, block| total.checked_add(block.byte_len))
            .ok_or_else(|| "block_byte_lengths overflow byte_len".to_owned());
        Ok(total.map(|_| Self { blocks, sha256 }))
    }

    pub(super) fn byte_len(&self, ctx: &DecodeContext<'_>) -> Result<u64, CodecError> {
        ctx.admit_iter(self.blocks(), "sum NX feature payload block lengths")?
            .try_fold(0u64, |total, block| {
                total.checked_add(block.byte_len).ok_or_else(|| {
                    ctx.refuse_codec_limit("sum NX feature payload block lengths", 0, 1)
                })
            })
    }

    pub(super) fn blocks(&self) -> &[FeaturePayloadBlock] {
        self.blocks.as_ref()
    }

    pub(super) fn block_ids(&self) -> impl ExactSizeIterator<Item = &String> + Clone {
        self.blocks().iter().map(|block| &block.id)
    }
}

pub(super) trait FeaturePayloadBlockAdmission {
    type Error;
    type Iter<'a>: Iterator<Item = &'a FeaturePayloadBlock>
    where
        Self: 'a;

    fn admit<'a>(
        &'a self,
        blocks: &'a [FeaturePayloadBlock],
    ) -> Result<Self::Iter<'a>, Self::Error>;
}

struct DecodeFeaturePayloadBlockAdmission<'ctx, 'decode> {
    ctx: &'ctx DecodeContext<'decode>,
}

impl FeaturePayloadBlockAdmission for DecodeFeaturePayloadBlockAdmission<'_, '_> {
    type Error = CodecError;
    type Iter<'a>
        = AdmittedIter<std::slice::Iter<'a, FeaturePayloadBlock>>
    where
        Self: 'a;

    fn admit<'a>(
        &'a self,
        blocks: &'a [FeaturePayloadBlock],
    ) -> Result<Self::Iter<'a>, Self::Error> {
        Ok(self
            .ctx
            .admit_iter(blocks, "validate NX feature payload block lengths")?)
    }
}

pub(super) struct ContextFreeFeaturePayloadBlockAdmission;

impl FeaturePayloadBlockAdmission for ContextFreeFeaturePayloadBlockAdmission {
    type Error = Infallible;
    type Iter<'a>
        = std::slice::Iter<'a, FeaturePayloadBlock>
    where
        Self: 'a;

    fn admit<'a>(
        &'a self,
        blocks: &'a [FeaturePayloadBlock],
    ) -> Result<Self::Iter<'a>, Self::Error> {
        Ok(blocks.iter())
    }
}

impl<B: AsRef<[FeaturePayloadBlock]> + TryFrom<Vec<FeaturePayloadBlock>>> FeaturePayloadContent<B> {
    pub(super) fn from_source<I>(
        ctx: &DecodeContext<'_>,
        ids: I,
        blocks: &BTreeMap<String, (&[u8], u64)>,
    ) -> Result<Option<Self>, CodecError>
    where
        I: IntoIterator<Item = String>,
        I::IntoIter: ExactSizeIterator,
    {
        let mut hash = Sha256::new();
        let mut rows = Vec::new();

        let mut byte_len = 0u64;
        let mut ids = ids.into_iter();
        for _ in ctx.admit_iter(&(0..ids.len()), "scan NX feature payload block ids")? {
            let Some(id) = ids.next() else {
                return Err(ctx.refuse_codec_limit("scan NX feature payload block ids", 0, 1));
            };
            let Some((bytes, source_offset)) = ctx
                .get_btree_map(blocks, &id, "find NX feature payload block")?
                .copied()
            else {
                return Ok(None);
            };
            let length = u64_from_index(bytes.len());
            byte_len = byte_len
                .checked_add(length)
                .ok_or_else(|| ctx.refuse_codec_limit("count NX feature payload bytes", 0, 1))?;
            ctx.charge_work(
                length
                    .checked_add(1)
                    .ok_or_else(|| ctx.refuse_codec_limit("hash NX feature payload bytes", 0, 1))?,
                "hash NX feature payload bytes",
            )?;

            ctx.reserve_vec(&mut rows, 1, "NX feature payload blocks")?;
            hash.update(bytes);
            rows.push(FeaturePayloadBlock {
                id,
                byte_len: length,
                source_offset,
            });
        }
        let Ok(blocks) = B::try_from(rows) else {
            return Ok(None);
        };

        let digest = cadmpeg_ir::hash::digest::Sha256Digest::from_bytes_for_decode(
            ctx,
            hash.finalize().into(),
            "retain NX feature payload digest",
        )?;
        let content = match Self::new(blocks, digest, &DecodeFeaturePayloadBlockAdmission { ctx })?
        {
            Ok(content) => content,
            Err(error) => return Err(CodecError::Malformed(error)),
        };
        Ok(Some(content))
    }
}

/// Separator between a data-block store and the block ordinal in a block id.
const BLOCK_MARKER: &str = ":block#";

/// The store that owns a block id of the form `{store}:block#{ordinal}`.
pub(super) fn block_store<'t>(
    ctx: &DecodeContext<'_>,
    block: &'t str,
    operation: &'static str,
) -> Result<Option<&'t str>, CodecError> {
    Ok(ctx
        .rsplit_once(block, BLOCK_MARKER, operation)?
        .map(|(store, _)| store))
}

/// Copies block ids into scoped storage; `None` when any block id is absent.
///
/// The returned reservation keeps the id vector's storage accounted until the
/// caller drops it. `blocks` must be an admitted iteration.
pub(super) fn copy_block_ids<'ctx, 'b>(
    ctx: &'ctx DecodeContext<'_>,
    blocks: impl Iterator<Item = Option<&'b str>>,
    operation: &'static str,
) -> Result<Option<(Vec<String>, ScopedReservation<'ctx>)>, CodecError> {
    let mut reservation = ctx.reserve_scoped(0, operation)?;
    let mut ids = Vec::new();
    for block in blocks {
        let Some(block) = block else {
            return Ok(None);
        };
        let copy = ctx.copy_retained_text(block, operation)?;
        ctx.push_scoped_vec(&mut reservation, &mut ids, copy, operation)?;
    }
    Ok(Some((ids, reservation)))
}

/// True when every block id names `store` as its owner.
pub(super) fn blocks_in_store(
    ctx: &DecodeContext<'_>,
    blocks: &[String],
    store: &str,
    operation: &'static str,
) -> Result<bool, CodecError> {
    let outside = ctx.any_by(
        blocks,
        |block| match block_store(ctx, block, operation)? {
            Some(owner) => Ok(!ctx.equal(owner, store, operation)?),
            None => Ok(true),
        },
        operation,
    )?;
    Ok(!outside)
}

/// The store shared by every block id, or `None` when there are no blocks, a
/// block id has no store, or two ids name different stores.
pub(super) fn shared_block_store<'b>(
    ctx: &DecodeContext<'_>,
    blocks: &'b [String],
    operation: &'static str,
) -> Result<Option<&'b str>, CodecError> {
    let Some((first, rest)) = blocks.split_first() else {
        return Ok(None);
    };
    let Some(store) = block_store(ctx, first, operation)? else {
        return Ok(None);
    };
    Ok(blocks_in_store(ctx, rest, store, operation)?.then_some(store))
}

/// The operation key after the last `#` of an operation-label identity.
pub(super) fn operation_key<'t>(
    ctx: &DecodeContext<'_>,
    label: &'t str,
    operation: &'static str,
) -> Result<Option<&'t str>, CodecError> {
    Ok(ctx.rsplit_once(label, "#", operation)?.map(|(_, key)| key))
}

/// A malformed-input error carrying a copy of a constant reason.
pub(super) fn malformed_reason(
    ctx: &DecodeContext<'_>,
    reason: &str,
    operation: &'static str,
) -> CodecError {
    match ctx.copy_retained_text(reason, operation) {
        Ok(reason) => CodecError::Malformed(reason),
        Err(error) => error,
    }
}

#[derive(Deserialize)]
struct PayloadContentWire {
    data_blocks: Vec<String>,
    byte_len: u64,
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
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
        match Self::new(
            blocks,
            wire.sha256,
            &ContextFreeFeaturePayloadBlockAdmission,
        )
        .map_err(|error| match error {})?
        {
            Ok(content) => Ok(content),
            Err(message) => Err(serde::de::Error::custom(message)),
        }
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
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(content.byte_len(ctx)?, 8);
            Ok::<_, cadmpeg_core::CodecError>(())
        })
        .unwrap();
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
        assert!(FeaturePayloadContent::new(
            blocks,
            cadmpeg_ir::hash::digest::Sha256Digest::digest(b"hash"),
            &super::ContextFreeFeaturePayloadBlockAdmission,
        )
        .unwrap()
        .is_err());
    }

    #[test]
    fn shared_block_store_requires_one_owner_for_every_block() {
        let ids = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
        crate::test_support::with_decode_context(|ctx| {
            let op = "test shared store";
            let same = ids(&["s:a:block#1", "s:a:block#2"]);
            assert_eq!(super::shared_block_store(ctx, &same, op)?, Some("s:a"));
            let mixed = ids(&["s:a:block#1", "s:b:block#2"]);
            assert_eq!(super::shared_block_store(ctx, &mixed, op)?, None);
            let unowned = ids(&["s:a:block#1", "plain"]);
            assert_eq!(super::shared_block_store(ctx, &unowned, op)?, None);
            assert_eq!(super::shared_block_store(ctx, &ids(&["plain"]), op)?, None);
            assert_eq!(super::shared_block_store(ctx, &[], op)?, None);
            assert!(super::blocks_in_store(ctx, &same, "s:a", op)?);
            assert!(!super::blocks_in_store(ctx, &mixed, "s:a", op)?);
            Ok::<_, cadmpeg_core::CodecError>(())
        })
        .unwrap();
    }
}
