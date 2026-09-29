// SPDX-License-Identifier: Apache-2.0
//! Joined payload bytes and the source spans that produced them.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

struct SourceSpan {
    byte_len: u64,
    source_offset: u64,
}

pub(super) struct JoinedPayload<'ctx> {
    bytes: Vec<u8>,
    sources: Vec<SourceSpan>,
    _reservation: ScopedReservation<'ctx>,
}

impl<'ctx> JoinedPayload<'ctx> {
    pub(super) fn from_source<'a>(
        ctx: &'ctx DecodeContext<'_>,
        ids: impl Iterator<Item = &'a String> + Clone,
        blocks: &BTreeMap<String, (&[u8], u64)>,
    ) -> Result<Option<Self>, CodecError> {
        let count = ids
            .clone()
            .try_fold(0usize, |count, _| count.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit("count NX feature payload blocks", 0, 1))?;
        let mut byte_len = 0usize;
        for id in ids.clone() {
            let Some((fragment, _)) = blocks.get(id).copied() else {
                return Ok(None);
            };
            byte_len = byte_len
                .checked_add(fragment.len())
                .ok_or_else(|| ctx.refuse_codec_limit("join NX feature payload bytes", 0, 1))?;
        }
        let span_bytes = count
            .checked_mul(std::mem::size_of::<SourceSpan>())
            .ok_or_else(|| ctx.refuse_codec_limit("reserve NX feature payload spans", 0, 1))?;
        let reserved = byte_len
            .checked_add(span_bytes)
            .ok_or_else(|| ctx.refuse_codec_limit("reserve NX joined feature payload", 0, 1))?;
        ctx.charge_work(u64_from_index(count), "scan NX feature payload blocks")?;
        ctx.charge_work(u64_from_index(byte_len), "copy NX feature payload bytes")?;
        ctx.charge_collection_items(u64_from_index(count), "NX feature payload spans")?;
        let reservation =
            ctx.reserve_scoped(u64_from_index(reserved), "join NX feature payload")?;
        let mut bytes = Vec::new();
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(&mut bytes, byte_len, "allocate NX feature payload bytes")?;
        let mut sources = Vec::new();
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(&mut sources, count, "allocate NX feature payload spans")?;
        for id in ids {
            let Some((fragment, source_offset)) = blocks.get(id).copied() else {
                return Ok(None);
            };
            if source_offset
                .checked_add(u64_from_index(fragment.len()))
                .is_none()
            {
                return Ok(None);
            }
            bytes.extend_from_slice(fragment);
            sources.push(SourceSpan {
                byte_len: u64_from_index(fragment.len()),
                source_offset,
            });
        }
        Ok(Some(Self {
            bytes,
            sources,
            _reservation: reservation,
        }))
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn source_spans(&self) -> impl Iterator<Item = (u64, u64, u64)> + '_ {
        let mut at = 0;
        self.sources.iter().map(move |span| {
            let start = at;
            at += span.byte_len;
            (start, span.byte_len, span.source_offset)
        })
    }

    pub(super) fn source_offset(&self, relative: u64) -> Option<u64> {
        self.source_spans().find_map(|(start, length, source)| {
            (relative >= start && relative - start < length).then(|| source + (relative - start))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::JoinedPayload;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::BTreeMap;

    #[test]
    fn source_locations_follow_fragment_boundaries_and_skip_empty_blocks() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
        let ids = ["a".to_owned(), "empty".to_owned(), "b".to_owned()];
        let blocks = BTreeMap::from([
            ("a".to_owned(), (&[1, 2][..], 10)),
            ("empty".to_owned(), (&[][..], u64::MAX)),
            ("b".to_owned(), (&[3, 4, 5][..], 100)),
        ]);
        let joined = JoinedPayload::from_source(&ctx, ids.iter(), &blocks)
            .unwrap()
            .unwrap();
        assert_eq!(joined.bytes(), [1, 2, 3, 4, 5]);
        assert_eq!(
            [0, 1, 2, 4, 5, u64::MAX].map(|offset| joined.source_offset(offset)),
            [Some(10), Some(11), Some(100), Some(102), None, None]
        );
    }

    #[test]
    fn source_extent_overflow_is_rejected_at_construction() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
        let ids = ["a".to_owned()];
        let blocks = BTreeMap::from([("a".to_owned(), (&[1, 2][..], u64::MAX))]);
        assert!(JoinedPayload::from_source(&ctx, ids.iter(), &blocks)
            .unwrap()
            .is_none());
    }

    #[test]
    fn nx_sketch_payload_join_preserves_order_and_cross_block_values() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
        let ids = ["block#2".to_string(), "block#3".to_string()];
        let blocks = std::collections::BTreeMap::from([
            ("block#2".to_string(), (&[0x30, 0x43][..], 120_u64)),
            (
                "block#3".to_string(),
                (&[0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72][..], 900_u64),
            ),
        ]);
        let joined = crate::native::features::joined_payload::JoinedPayload::from_source(
            &ctx,
            ids.iter(),
            &blocks,
        )
        .expect("required invariant")
        .expect("required invariant");
        assert_eq!(
            joined.bytes(),
            [0x30, 0x43, 0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72]
        );
        assert_eq!(
            joined
                .source_spans()
                .map(|(start, _, _)| start)
                .collect::<Vec<_>>(),
            [0, 2]
        );
        assert_eq!(
            joined
                .source_spans()
                .map(|(_, length, _)| length)
                .collect::<Vec<_>>(),
            [2, 6]
        );
        assert_eq!(
            joined
                .source_spans()
                .map(|(_, _, source)| source)
                .collect::<Vec<_>>(),
            [120, 900]
        );

        let missing = ["block#2".to_string(), "missing".to_string()];
        assert!(
            crate::native::features::joined_payload::JoinedPayload::from_source(
                &ctx,
                missing.iter(),
                &blocks
            )
            .unwrap()
            .is_none()
        );
    }
}
