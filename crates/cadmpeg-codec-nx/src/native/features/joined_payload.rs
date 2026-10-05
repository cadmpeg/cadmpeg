// SPDX-License-Identifier: Apache-2.0
//! Joined payload bytes and the source spans that produced them.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

/// One source block occupying `start..end` of the joined bytes.
struct SourceSpan {
    end: u64,
    start: u64,
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
        count: usize,
        blocks: &BTreeMap<String, (&[u8], u64)>,
    ) -> Result<Option<Self>, CodecError> {
        const OPERATION: &str = "join NX feature payload";
        let mut reservation = ctx.reserve_scoped(0, OPERATION)?;
        let mut bytes = Vec::new();
        let mut sources = Vec::new();
        let mut ids = ids;
        for _ in 0_usize..count {
            ctx.charge_work(1, "copy NX feature payload blocks")?;
            let Some(id) = ids.next() else {
                return Ok(None);
            };
            let Some(&(fragment, source_offset)) =
                ctx.get_btree_map(blocks, id.as_str(), "copy NX feature payload blocks")?
            else {
                return Ok(None);
            };
            let fragment_len = u64_from_index(fragment.len());
            if source_offset.checked_add(fragment_len).is_none() {
                return Ok(None);
            }
            let start = u64_from_index(bytes.len());
            let end = start
                .checked_add(fragment_len)
                .ok_or_else(|| ctx.refuse_codec_limit("join NX feature payload bytes", 0, 1))?;
            reservation.with_storage(|| {
                ctx.extend_retained_bytes(&mut bytes, fragment, "copy NX feature payload bytes")
            })?;
            ctx.reserve_scoped_vec(
                &mut reservation,
                &mut sources,
                1,
                "NX feature payload spans",
            )?;
            sources.push(SourceSpan {
                end,
                start,
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

    /// Map a joined-byte position to its source file offset.
    pub(super) fn source_at(
        &self,
        ctx: &DecodeContext<'_>,
        relative: usize,
    ) -> Result<Option<u64>, CodecError> {
        self.source_offset(ctx, u64_from_index(relative))
    }

    /// Map a joined-byte offset to its source file offset.
    pub(super) fn source_offset(
        &self,
        ctx: &DecodeContext<'_>,
        relative: u64,
    ) -> Result<Option<u64>, CodecError> {
        // Spans are contiguous, so their ends do not decrease; the first span
        // ending after the offset is the only one that can contain it.
        let index = ctx.partition_point(
            &self.sources,
            |span| Ok(span.end <= relative),
            "map NX feature payload source offset",
        )?;
        Ok(self
            .sources
            .get(index)
            .filter(|span| span.start <= relative)
            .and_then(|span| span.source_offset.checked_add(relative - span.start)))
    }
}

#[cfg(test)]
mod tests {
    use super::JoinedPayload;
    use std::collections::BTreeMap;

    #[test]
    fn source_locations_follow_fragment_boundaries_and_skip_empty_blocks() {
        crate::test_support::with_decode_context(|ctx| {
            let ids = ["a".to_owned(), "empty".to_owned(), "b".to_owned()];
            let blocks = BTreeMap::from([
                ("a".to_owned(), (&[1, 2][..], 10)),
                ("empty".to_owned(), (&[][..], u64::MAX)),
                ("b".to_owned(), (&[3, 4, 5][..], 100)),
            ]);
            let joined = JoinedPayload::from_source(ctx, ids.iter(), ids.len(), &blocks)
                .unwrap()
                .unwrap();
            assert_eq!(joined.bytes(), [1, 2, 3, 4, 5]);
            assert_eq!(
                [0, 1, 2, 4, 5, u64::MAX].map(|offset| joined.source_offset(ctx, offset).unwrap()),
                [Some(10), Some(11), Some(100), Some(102), None, None]
            );
        });
    }

    #[test]
    fn source_extent_overflow_is_rejected_at_construction() {
        crate::test_support::with_decode_context(|ctx| {
            let ids = ["a".to_owned()];
            let blocks = BTreeMap::from([("a".to_owned(), (&[1, 2][..], u64::MAX))]);
            assert!(
                JoinedPayload::from_source(ctx, ids.iter(), ids.len(), &blocks)
                    .unwrap()
                    .is_none()
            );
        });
    }

    #[test]
    fn source_offset_propagates_work_refusal() {
        use cadmpeg_core::decode::ResourceDimension;

        let ids = ["block#2".to_string(), "block#3".to_string()];
        let blocks = BTreeMap::from([
            ("block#2".to_string(), (&[0x30, 0x43][..], 120_u64)),
            (
                "block#3".to_string(),
                (&[0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72][..], 900_u64),
            ),
        ]);
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "map NX feature payload source offset",
            |ctx| {
                let Some(joined) = JoinedPayload::from_source(ctx, ids.iter(), ids.len(), &blocks)?
                else {
                    return Ok(());
                };
                assert_eq!(joined.source_offset(ctx, 0)?, Some(120));
                Ok(())
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "map NX feature payload source offset"
        ));
    }

    #[test]
    fn nx_sketch_payload_join_preserves_order_and_cross_block_values() {
        crate::test_support::with_decode_context(|ctx| {
            let ids = ["block#2".to_string(), "block#3".to_string()];
            let blocks = std::collections::BTreeMap::from([
                ("block#2".to_string(), (&[0x30, 0x43][..], 120_u64)),
                (
                    "block#3".to_string(),
                    (&[0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72][..], 900_u64),
                ),
            ]);
            let joined = crate::native::features::joined_payload::JoinedPayload::from_source(
                ctx,
                ids.iter(),
                ids.len(),
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
                    .sources
                    .iter()
                    .map(|span| span.start)
                    .collect::<Vec<_>>(),
                [0, 2]
            );
            assert_eq!(
                joined
                    .sources
                    .iter()
                    .map(|span| span.end - span.start)
                    .collect::<Vec<_>>(),
                [2, 6]
            );
            assert_eq!(
                joined
                    .sources
                    .iter()
                    .map(|span| span.source_offset)
                    .collect::<Vec<_>>(),
                [120, 900]
            );

            let missing = ["block#2".to_string(), "missing".to_string()];
            assert!(
                crate::native::features::joined_payload::JoinedPayload::from_source(
                    ctx,
                    missing.iter(),
                    missing.len(),
                    &blocks
                )
                .unwrap()
                .is_none()
            );
        });
    }
}
