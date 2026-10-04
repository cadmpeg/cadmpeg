// SPDX-License-Identifier: Apache-2.0
//! Terminal datum-plane index lanes with derived token positions.

use super::compact::NullableCompactIndex;
use super::compact::{CompactIndexAtom, CountedIndexMembers, LocatedCompactIndex};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use std::ops::Add;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatumIndexLane<O = usize> {
    offset: O,
    indices: CountedIndexMembers<CompactIndexAtom, 1>,
    trailer: u32,
}

impl<O> DatumIndexLane<O> {
    pub(crate) fn declared_count(&self) -> u8 {
        self.indices.declared_count()
    }
    pub(crate) fn trailer(&self) -> u32 {
        self.trailer
    }
    fn extent<'a, E, I: Iterator<Item = &'a CompactIndexAtom>>(
        indices: &'a [CompactIndexAtom],
        admit: impl FnOnce(&'a [CompactIndexAtom]) -> Result<I, E>,
    ) -> Result<Option<u16>, E> {
        Ok(admit(indices)?.try_fold(7_u16, |length, atom| {
            length.checked_add(u16::from(atom.byte_len()))
        }))
    }

}

impl<O: Copy + Add<Output = O> + From<u16>> DatumIndexLane<O> {
    pub(crate) fn offset(&self) -> O {
        self.offset
    }
    pub(crate) fn indices(&self) -> impl Iterator<Item = LocatedCompactIndex<O>> + Clone + '_ {
        let mut offset = self.offset + O::from(2);
        self.indices.as_slice().iter().map(move |atom| {
            let token = LocatedCompactIndex {
                atom: *atom,
                offset,
            };
            offset = offset + O::from(u16::from(atom.byte_len()));
            token
        })
    }
}

impl DatumIndexLane<usize> {
    fn from_wire(ctx: &DecodeContext<'_>, indices: CountedIndexMembers<CompactIndexAtom, 1>, trailer: u32, offset: usize) -> Result<Option<Self>, CodecError> {
        let width = Self::extent(indices.as_slice(), |indices| ctx.admit_iter(indices, "NX datum index token widths"))?
            .ok_or_else(|| ctx.refuse_codec_limit("NX datum index extent", u64::MAX, u64::MAX))?;
        if offset.checked_add(usize::from(width)).is_none() { return Ok(None); }
        Ok(Some(Self { offset, indices, trailer }))
    }
}

impl DatumIndexLane<u64> {
    pub(crate) fn new(indices: CountedIndexMembers<CompactIndexAtom, 1>, trailer: u32, offset: u64) -> Option<Self> {
        let width = match Self::extent(indices.as_slice(), |indices| {
            Ok::<_, std::convert::Infallible>(indices.iter())
        }) {
            Ok(width) => width?,
            Err(error) => match error {},
        };
        offset.checked_add(u64::from(width))?;
        Some(Self { offset, indices, trailer })
    }
}

impl DatumIndexLane<usize> {
    pub(crate) fn into_u64(self) -> DatumIndexLane<u64> {
        DatumIndexLane {
            offset: cadmpeg_core::decode::u64_from_index(self.offset),
            indices: self.indices,
            trailer: self.trailer,
        }
    }
}

/// Decode unique datum-plane index lanes ending at the logical payload boundary.
pub(crate) fn scan(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<DatumIndexLane>, CodecError> {
    let mut lanes = Vec::new();
    if let Some(last) = bytes.len().checked_sub(7) {
    for start in ctx.admit_iter(&(0..last), "scan NX datum index lanes")? {
        if bytes[start] != 0x01 {
            continue;
        }
        let declared_count = bytes[start + 1];
        if declared_count < 2 {
            continue;
        }
        let mut scan_at = start + 2;
        let mut complete = true;
        for _ in ctx.admit_iter(&(1..declared_count), "scan NX datum index members")? {
            let Some(token) =
                NullableCompactIndex::read(bytes, scan_at).filter(|token| token.atom.is_some())
            else {
                complete = false;
                break;
            };
            scan_at += token.raw().len();
        }
        if !complete || bytes.get(scan_at) != Some(&0x00) || scan_at + 5 != bytes.len() {
            continue;
        }
        let member_count = usize::from(declared_count - 1);
        let operation = "NX datum index members";
        let mut indices = ctx.collection_vec(member_count, operation)?;
        let mut at = start + 2;
        for _ in ctx.admit_iter(&(0..member_count), "NX datum index member materialization")? {
            let Some(token) = LocatedCompactIndex::read(&bytes[..scan_at], at) else {
                break;
            };
            at += token.atom.raw().len();
            indices.push(token.atom);
        }
        if indices.len() != member_count {
            continue;
        }
        let Some(indices) = CountedIndexMembers::new(indices).ok() else {
            continue;
        };
        let Some(trailer) = View::u32_be_at(bytes, scan_at + 1) else {
            continue;
        };
        if let Some(lane) = DatumIndexLane::<usize>::from_wire(ctx, indices, trailer, start)? {
            ctx.reserve_vec(&mut lanes, 1, "NX datum index lanes")?;
            lanes.push(lane);
        }
    }
    }
    Ok(lanes)
}

#[cfg(test)]
mod tests {
    use super::{scan, DatumIndexLane};

    fn datum_index_limit_error(
        adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> cadmpeg_core::CodecError {
        let bytes = [
            0x80, 0xab, 0x01, 0x04, 0x81, 0x01, 0x01, 0x01, 0x00, 0x12, 0x34, 0x56, 0x78,
        ];

        crate::test_support::with_decode_context_over(&bytes, adjust, |ctx| {
            scan(ctx, &bytes).expect_err("datum index resource refusal")
        })
    }

    #[test]
    fn om_datum_index_route_refuses_collection_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_collection_items = 0;
        };
        assert!(
            matches!(datum_index_limit_error(adjust_policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn om_datum_index_route_refuses_retained_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_retained_bytes = 0;
        };
        assert!(
            matches!(datum_index_limit_error(adjust_policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn om_datum_index_route_refuses_work_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_work_units = 0;
        };
        assert!(
            matches!(datum_index_limit_error(adjust_policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
    }

    #[test]
    fn datum_terminal_positions_follow_mixed_token_widths_and_checked_extent() {
        for count in [1, 254] {
            let mut bytes = vec![
                0x7f,
                0x01,
                u8::try_from(count + 1).expect("fixture value fits u8"),
            ];
            let mut offsets = Vec::new();
            for slot in 0..count {
                offsets.push(bytes.len());
                if slot % 2 == 0 {
                    bytes.extend_from_slice(&[0x80, 1]);
                } else {
                    bytes.push(2);
                }
            }
            bytes.extend_from_slice(&[0, 0x12, 0x34, 0x56, 0x78]);
            let lane = crate::test_support::with_decode_context(|ctx| scan(ctx, &bytes))
                .unwrap()
                .into_iter()
                .find(|lane| lane.offset() == 1)
                .unwrap();
            assert_eq!(
                lane.indices().map(|token| token.offset).collect::<Vec<_>>(),
                offsets
            );
            assert_eq!(usize::from(lane.declared_count()), count + 1);
            assert_eq!(lane.trailer(), 0x1234_5678);
            let origin = u64::MAX - cadmpeg_core::decode::u64_from_index(bytes.len() - 1);
            let last =
                DatumIndexLane::<u64>::new(lane.indices.clone(), lane.trailer, origin).unwrap();
            assert_eq!(
                last.indices().map(|token| token.offset).collect::<Vec<_>>(),
                offsets
                    .iter()
                    .map(|offset| origin + cadmpeg_core::decode::u64_from_index(*offset - 1))
                    .collect::<Vec<_>>()
            );
            assert!(DatumIndexLane::<u64>::new(lane.indices, lane.trailer, origin + 1).is_none());
        }
    }

    #[test]
    fn om_datum_plane_object_index_lane_ends_at_logical_payload_boundary() {
        let bytes = [
            0x80, 0xab, 0x01, 0x04, 0x81, 0x01, 0x01, 0x01, 0x00, 0x12, 0x34, 0x56, 0x78,
        ];
        let lanes = crate::test_support::with_decode_context(|ctx| scan(ctx, &bytes)).unwrap();
        assert_eq!(lanes.len(), 1);
        assert_eq!(lanes[0].offset(), 2);
        assert_eq!(usize::from(lanes[0].declared_count()), 4);
        assert_eq!(
            lanes[0]
                .indices()
                .map(|token| (token.atom.value(), token.offset))
                .collect::<Vec<_>>(),
            [(257, 4), (1, 6), (1, 7)]
        );
        assert_eq!(
            lanes[0]
                .indices()
                .map(|token| token.atom.raw().to_vec())
                .collect::<Vec<_>>(),
            [vec![0x81, 0x01], vec![1], vec![1]]
        );
        assert_eq!(lanes[0].trailer(), 0x1234_5678);

        let mut trailing = bytes.to_vec();
        trailing.push(0);
        assert!(
            crate::test_support::with_decode_context(|ctx| scan(ctx, &trailing))
                .unwrap()
                .is_empty()
        );
    }
}
