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
    fn from_wire(
        ctx: &DecodeContext<'_>,
        indices: CountedIndexMembers<CompactIndexAtom, 1>,
        trailer: u32,
        offset: usize,
    ) -> Result<Option<Self>, CodecError> {
        let width = Self::extent(indices.as_slice(), |indices| {
            ctx.admit_iter(indices, "NX datum index token widths")
        })?
        .ok_or_else(|| ctx.refuse_codec_limit("NX datum index extent", u64::MAX, u64::MAX))?;
        if offset.checked_add(usize::from(width)).is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            offset,
            indices,
            trailer,
        }))
    }
}

impl DatumIndexLane<u64> {
    pub(crate) fn new(
        indices: CountedIndexMembers<CompactIndexAtom, 1>,
        trailer: u32,
        offset: u64,
    ) -> Option<Self> {
        let width = match Self::extent(indices.as_slice(), |indices| {
            Ok::<_, std::convert::Infallible>(indices.iter())
        }) {
            Ok(width) => width?,
            Err(error) => match error {},
        };
        offset.checked_add(u64::from(width))?;
        Some(Self {
            offset,
            indices,
            trailer,
        })
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
pub(crate) fn scan<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
) -> Result<
    (
        Vec<DatumIndexLane>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut indices_storage = ctx.reserve_scoped(0, "NX datum index token storage")?;
    let mut lanes_storage = ctx.reserve_scoped(0, "NX datum lane scratch")?;
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
            let mut rows = 1..declared_count;
            while ctx
                .next_charged(&mut rows, "scan NX datum index members")?
                .is_some()
            {
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
            let mut indices =
                indices_storage.with_storage(|| ctx.collection_vec(member_count, operation))?;
            let mut at = start + 2;
            let mut rows = 0..member_count;
            while ctx
                .next_charged(&mut rows, "NX datum index member materialization")?
                .is_some()
            {
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
                ctx.push_scoped_vec(&mut lanes_storage, &mut lanes, lane, "NX datum index lanes")?;
            }
        }
    }
    Ok((lanes, indices_storage, lanes_storage))
}

#[cfg(test)]
mod tests {
    use super::{scan, DatumIndexLane};

    fn datum_index_limit_error(
        dimension: cadmpeg_core::decode::ResourceDimension,
        operation: &str,
    ) -> cadmpeg_core::CodecError {
        let bytes = [
            0x80, 0xab, 0x01, 0x04, 0x81, 0x01, 0x01, 0x01, 0x00, 0x12, 0x34, 0x56, 0x78,
        ];

        crate::test_support::resource_refusal_at(&bytes, dimension, operation, |ctx| {
            scan(ctx, &bytes).map(|(value, _indices_storage, _lanes_storage)| value)
        })
    }

    #[test]
    fn om_datum_index_route_refuses_collection_limit() {
        assert!(
            matches!(datum_index_limit_error(cadmpeg_core::decode::ResourceDimension::CollectionItems, "NX datum index members"), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn om_datum_index_route_refuses_scoped_limit() {
        assert!(
            matches!(datum_index_limit_error(cadmpeg_core::decode::ResourceDimension::MaterializedBytes, "NX datum index members"), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
        );
    }

    #[test]
    fn om_datum_index_route_refuses_work_limit() {
        assert!(
            matches!(datum_index_limit_error(cadmpeg_core::decode::ResourceDimension::WorkUnits, "scan NX datum index members"), cadmpeg_core::CodecError::ResourceLimit(limit)
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
            let lane = crate::test_support::with_decode_context(|ctx| {
                scan(ctx, &bytes).map(|(value, _indices_storage, _lanes_storage)| value)
            })
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
        let lanes = crate::test_support::with_decode_context(|ctx| {
            scan(ctx, &bytes).map(|(value, _indices_storage, _lanes_storage)| value)
        })
        .unwrap();
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
        assert!(crate::test_support::with_decode_context(
            |ctx| scan(ctx, &trailing).map(|(value, _indices_storage, _lanes_storage)| value)
        )
        .unwrap()
        .is_empty());
    }
    #[test]
    fn datum_lane_vector_slots_do_not_use_the_retained_token_budget() {
        let bytes = [0x80, 0xab, 1, 4, 0x81, 1, 1, 1, 0, 0x12, 0x34, 0x56, 0x78];
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes =
                    u64::try_from(3 * std::mem::size_of::<super::CompactIndexAtom>()).unwrap();
            },
            |ctx| {
                let (lanes, indices_storage, lanes_storage) = super::scan(ctx, &bytes).unwrap();
                assert_eq!(lanes.len(), 1);
                let [lane] = <[_; 1]>::try_from(lanes).unwrap();
                assert_eq!(lane.indices().count(), 3);
                drop(lanes_storage);
                let _lane = indices_storage.commit_value(lane).unwrap();
                assert_eq!(ctx.resource_refusal(), None);
            },
        );
    }

    #[test]
    fn ambiguous_datum_lanes_need_no_retained_storage() {
        let bytes = [1, 4, 1, 2, 42, 0, 0x12, 0x34, 0x56, 0x78];
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_retained_bytes = 0,
            |ctx| {
                let (lanes, indices_storage, lanes_storage) = super::scan(ctx, &bytes).unwrap();
                assert_eq!(lanes.len(), 2);
                assert_eq!(lanes[0].offset(), 0);
                assert_eq!(lanes[1].offset(), 2);
                assert!(<[_; 1]>::try_from(lanes).is_err());
                drop((indices_storage, lanes_storage));
                assert_eq!(ctx.resource_refusal(), None);
            },
        );
    }
}
