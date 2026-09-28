// SPDX-License-Identifier: Apache-2.0
//! Draft leading index frames with token positions derived from their encodings.

use super::compact::{
    CompactIndexTarget, CountedIndexMembers, LocatedCompactIndex, PositionedIndex,
};
use super::operation_record::OperationPayload;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::ops::Add;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DraftLeadingLane<T = (), O = usize> {
    offset: O,
    indices: CountedIndexMembers<CompactIndexTarget<T>, 1>,
}

impl<T, O> DraftLeadingLane<T, O> {
    pub(crate) fn declared_count(&self) -> u8 {
        self.indices.declared_count()
    }
    fn byte_len(&self) -> u16 {
        26 + self
            .indices
            .as_slice()
            .iter()
            .map(|token| token.atom.raw().len() as u16)
            .sum::<u16>()
    }
}

impl<T, O: Copy + Add<Output = O> + From<u16>> DraftLeadingLane<T, O> {
    pub(crate) fn indices(&self) -> impl Iterator<Item = PositionedIndex<'_, T, O>> + Clone {
        let mut offset = self.offset + O::from(24);
        self.indices.as_slice().iter().map(move |token| {
            let positioned = PositionedIndex {
                atom: token.atom,
                target: &token.target,
                offset,
            };
            offset = offset + O::from(token.atom.raw().len() as u16);
            positioned
        })
    }
}

macro_rules! checked_origin {
    ($offset:ty) => {
        impl<T> DraftLeadingLane<T, $offset> {
            pub(crate) fn new(
                indices: CountedIndexMembers<CompactIndexTarget<T>, 1>,
                offset: $offset,
            ) -> Option<Self> {
                let lane = Self { offset, indices };
                offset.checked_add(<$offset>::from(lane.byte_len()))?;
                Some(lane)
            }
        }
    };
}
checked_origin!(usize);
checked_origin!(u64);

impl DraftLeadingLane<(), usize> {
    pub(crate) fn into_absolute(self, base: u64) -> Option<DraftLeadingLane<(), u64>> {
        DraftLeadingLane::<(), u64>::new(self.indices, base.checked_add(self.offset as u64)?)
    }
}

impl<O> DraftLeadingLane<(), O> {
    pub(crate) fn resolve<T>(
        self,
        ctx: &DecodeContext<'_>,
        mut resolve: impl FnMut(u32) -> T,
    ) -> Result<DraftLeadingLane<T, O>, CodecError> {
        Ok(DraftLeadingLane {
            offset: self.offset,
            indices: self.indices.map_charged(ctx, |token| CompactIndexTarget {
                atom: token.atom,
                target: resolve(token.atom.value()),
            })?,
        })
    }

    pub(crate) fn try_resolve<T>(
        self,
        mut resolve: impl FnMut(u32) -> Option<T>,
    ) -> Option<DraftLeadingLane<T, O>> {
        Some(DraftLeadingLane {
            offset: self.offset,
            indices: self.indices.try_map(|token| {
                Some(CompactIndexTarget {
                    atom: token.atom,
                    target: resolve(token.atom.value())?,
                })
            })?,
        })
    }
}

/// Decode the exactly positioned counted compact-index lane preceding a `DRAFT` graph.
pub(crate) fn scan(ctx: &DecodeContext<'_>, record: OperationPayload<'_>) -> Result<Option<DraftLeadingLane>, CodecError> {
    const PREFIX: [u8; 22] = [
        0x67, 0x00, 0x00, 0x01, 0x00, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0x03, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ];
    if record.name() != "DRAFT" || record.payload().get(..PREFIX.len()) != Some(&PREFIX) {
        return Ok(None);
    }
    ctx.charge_work(u64_from_index(record.payload().len()), "scan NX draft leading indices")?;
    let mut at = PREFIX.len();
    if record.payload().get(at) != Some(&0x01) { return Ok(None); }
    let Some(&declared_count) = record.payload().get(at + 1) else { return Ok(None); };
    if declared_count < 2 { return Ok(None); }
    at += 2;
    let member_count = usize::from(declared_count - 1);
    let mut scan_at = at;
    for _ in 1..declared_count {
        let Some(token) = LocatedCompactIndex::read(record.payload(), scan_at) else { return Ok(None); };
        scan_at += token.atom.raw().len();
    }
    if record.payload().get(scan_at..scan_at + 2) != Some(&[0x01, 0x02]) { return Ok(None); }
    let count = u64_from_index(member_count);
    let operation = "NX draft leading index members";
    ctx.charge_collection_items(count, operation)?;
    let bytes = count.checked_mul(u64_from_index(std::mem::size_of::<CompactIndexTarget<()>>()))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, count))?;
    ctx.charge_retained(bytes, operation)?;
    let mut indices = Vec::new();
    indices.try_reserve_exact(member_count)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, count))?;
    for _ in 1..declared_count {
        let Some(token) = LocatedCompactIndex::read(record.payload(), at) else { return Ok(None); };
        at += token.atom.raw().len();
        indices.push(token.atom.into());
    }

    Ok(CountedIndexMembers::new(indices).ok().and_then(|indices| {
        DraftLeadingLane::<(), usize>::new(indices, record.payload_offset())
    }))
}

#[cfg(test)]
mod tests {
    use super::super::operation_record::OperationPayload;
    use super::scan;

    fn draft_leading_limit_error(policy: &cadmpeg_core::decode::DecodePolicy) -> cadmpeg_core::CodecError {
        let bytes = [
            0x67, 0, 0, 1, 0, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 3,
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1, 2, 8, 1, 2,
        ];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, policy).unwrap();
        scan(&ctx, OperationPayload::new(&bytes, 100, "DRAFT").unwrap())
            .expect_err("draft leading resource refusal")
    }

    #[test]
    fn om_draft_leading_route_refuses_collection_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        assert!(matches!(draft_leading_limit_error(&policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
    }

    #[test]
    fn om_draft_leading_route_refuses_retained_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        assert!(matches!(draft_leading_limit_error(&policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
    }

    #[test]
    fn om_draft_leading_route_refuses_work_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = 0;
        assert!(matches!(draft_leading_limit_error(&policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
    }

    #[test]
    fn draft_leading_positions_follow_token_widths_and_checked_frame_extent() {
        for count in [1, 254] {
            let mut bytes = vec![
                0x67,
                0,
                0,
                1,
                0,
                0x2f,
                0xa4,
                0x7a,
                0xe1,
                0x47,
                0xae,
                0x14,
                0x7b,
                3,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                1,
                (count + 1) as u8,
            ];
            let mut positions = Vec::new();
            for slot in 0..count {
                positions.push(bytes.len() + 100);
                if slot % 2 == 0 {
                    bytes.extend_from_slice(&[0x80, 7]);
                } else {
                    bytes.push(8);
                }
            }
            bytes.extend_from_slice(&[1, 2]);
            let frame = crate::test_support::with_decode_context(|ctx| {
                scan(ctx, OperationPayload::new(&bytes, 100, "DRAFT").unwrap())
            }).unwrap().unwrap();
            assert_eq!(usize::from(frame.declared_count()), count + 1);
            assert_eq!(
                frame
                    .indices()
                    .map(|token| token.offset)
                    .collect::<Vec<_>>(),
                positions
            );
            let base = u64::MAX - 100 - bytes.len() as u64;
            let absolute = frame.clone().into_absolute(base).unwrap();
            assert_eq!(
                absolute
                    .indices()
                    .map(|token| token.offset)
                    .collect::<Vec<_>>(),
                positions
                    .iter()
                    .map(|offset| base + *offset as u64)
                    .collect::<Vec<_>>()
            );
            assert!(frame.clone().into_absolute(base + 1).is_none());
            let resolved = absolute
                .try_resolve(|index| Some(index.to_string()))
                .unwrap();
            assert!(resolved
                .indices()
                .all(|token| *token.target == token.atom.value().to_string()));
            assert!(frame.try_resolve(|_| None::<String>).is_none());
        }
    }
}
