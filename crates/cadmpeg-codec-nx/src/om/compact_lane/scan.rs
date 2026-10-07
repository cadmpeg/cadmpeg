// SPDX-License-Identifier: Apache-2.0
//! Complete counted and ABR lane admission.

use super::{AbrLane, CountedLane, ABR_TERMINATOR, COUNTED_PREFIX, COUNTED_TERMINATOR};
use crate::om::compact::{CountedIndexMembers, LocatedCompactIndex, NullableCompactIndex};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Decode fixed-width `ABR` block-reference lanes from contiguous column storage.
pub(crate) fn abr_lanes(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<AbrLane>, CodecError> {
    let mut lanes = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        ctx.charge_work(1, "scan NX ABR lanes")?;
        if bytes[start] != 0x11 {
            start += 1;
            continue;
        }
        let mut at = start + 1;
        let tokens = (|| {
            let mut slots = [None; 16];
            for slot in &mut slots {
                let token = NullableCompactIndex::read(bytes, at)?;
                at += token.raw().len();
                *slot = token.atom.map(Into::into);
            }
            Some(slots)
        })();
        let Some(tokens) = tokens else {
            start += 1;
            continue;
        };
        let Some(end) = at.checked_add(ABR_TERMINATOR.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at..end) == Some(&ABR_TERMINATOR) {
            if let Some(lane) = AbrLane::<(), usize>::new(tokens, start) {
                ctx.push_vec(&mut lanes, lane, "NX ABR lanes")?;
            }
            start = end;
        } else {
            start += 1;
        }
    }
    Ok(lanes)
}

/// Decode complete counted compact-index lanes from one bounded store block.
///
/// A lane is `01, count:u8, anchor, member[count-2], 01 11`, with
/// `count >= 3`. Compact indices use the ordinary direct/extended encoding;
/// null indices reject the candidate atomically.
pub(crate) fn counted_lanes(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<CountedLane>, CodecError> {
    let decode = |start: usize| -> Result<Option<(CountedLane, usize)>, CodecError> {
        if bytes.get(start) != Some(&1) {
            return Ok(None);
        }
        let Some(declared_count @ 3..) = bytes.get(start + 1).copied() else {
            return Ok(None);
        };
        let Some(anchor) = LocatedCompactIndex::read(bytes, start + usize::from(COUNTED_PREFIX))
        else {
            return Ok(None);
        };
        let mut at = anchor.offset + anchor.atom.raw().len();
        let mut storage = ctx.reserve_scoped(0, "NX counted lane candidate storage")?;
        let mut members = Vec::new();
        let mut indices = 0..usize::from(declared_count) - 2;
        while let Some(_) = ctx.next_charged(&mut indices, "NX counted lanes row validation")? {
            let Some(token) = LocatedCompactIndex::read(bytes, at) else {
                return Ok(None);
            };
            at += token.atom.raw().len();
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut members,
                    token.atom.into(),
                    "NX counted index lane members",
                )
            })?;
        }
        let Some(end) = at.checked_add(COUNTED_TERMINATOR.len()) else {
            return Ok(None);
        };
        if bytes.get(at..end) != Some(&COUNTED_TERMINATOR) {
            return Ok(None);
        }
        let Ok(members) = CountedIndexMembers::new(members) else {
            return Ok(None);
        };
        let Some(lane) =
            CountedLane::<(), usize>::from_wire(ctx, anchor.atom.into(), members, start)?
        else {
            return Ok(None);
        };
        storage.commit()?;
        Ok(Some((lane, end)))
    };
    let mut lanes = Vec::new();
    let mut start = 0;
    while start + 4 <= bytes.len() {
        ctx.charge_work(1, "scan NX counted index lanes")?;
        if let Some((lane, end)) = decode(start)? {
            ctx.push_vec(&mut lanes, lane, "NX counted index lanes")?;
            start = end;
        } else {
            start += 1;
        }
    }
    Ok(lanes)
}

#[cfg(test)]
mod tests {
    fn counted_lane_limit_error(
        adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> cadmpeg_core::CodecError {
        let bytes = [0x01, 0x03, 0x42, 0x62, 0x01, 0x11];

        crate::test_support::with_decode_context_over(&bytes, adjust, |ctx| {
            super::counted_lanes(ctx, &bytes).expect_err("counted lane resource refusal")
        })
    }

    fn abr_lane_limit_error(
        adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> cadmpeg_core::CodecError {
        let mut bytes = vec![0x11];
        bytes.extend_from_slice(&[0xff; 16]);
        bytes.extend_from_slice(&super::ABR_TERMINATOR);

        crate::test_support::with_decode_context_over(&bytes, adjust, |ctx| {
            super::abr_lanes(ctx, &bytes).expect_err("ABR lane resource refusal")
        })
    }

    #[test]
    fn om_abr_lane_route_refuses_collection_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_collection_items = 0;
        };
        assert!(
            matches!(abr_lane_limit_error(adjust_policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn om_abr_lane_route_refuses_retained_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_retained_bytes = 0;
        };
        assert!(
            matches!(abr_lane_limit_error(adjust_policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn om_abr_lane_route_refuses_work_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_work_units = 0;
        };
        assert!(
            matches!(abr_lane_limit_error(adjust_policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
    }

    #[test]
    fn om_counted_lane_route_refuses_collection_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_collection_items = 0;
        };
        assert!(
            matches!(counted_lane_limit_error(adjust_policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn om_counted_lane_route_refuses_retained_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_retained_bytes = 0;
        };
        assert!(
            matches!(counted_lane_limit_error(adjust_policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn om_counted_lane_route_refuses_work_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_work_units = 0;
        };
        assert!(
            matches!(counted_lane_limit_error(adjust_policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
    }

    #[test]
    fn om_offset_store_counted_index_lane_requires_complete_non_null_members() {
        let bytes = [
            0xaa, 0x01, 0x06, 0x42, 0x62, 0x80, 0x48, 0x80, 0x50, 0x7c, 0x01, 0x11, 0xbb,
        ];
        let lanes =
            crate::test_support::with_decode_context(|ctx| super::counted_lanes(ctx, &bytes))
                .unwrap();
        assert_eq!(lanes.len(), 1);
        assert_eq!(lanes[0].offset(), 1);
        assert_eq!(lanes[0].declared_count(), 6);
        assert_eq!(lanes[0].anchor().atom.value(), 0x42);
        assert_eq!(lanes[0].anchor().atom.raw(), [0x42]);
        assert_eq!(lanes[0].anchor().offset, 3);
        assert_eq!(
            lanes[0]
                .members()
                .map(|token| (token.atom.value(), token.offset))
                .collect::<Vec<_>>(),
            vec![(0x62, 4), (0x48, 5), (0x50, 7), (0x7c, 9)]
        );
        assert_eq!(
            lanes[0]
                .members()
                .map(|token| token.atom.raw().to_vec())
                .collect::<Vec<_>>(),
            [vec![0x62], vec![0x80, 0x48], vec![0x80, 0x50], vec![0x7c]]
        );

        assert!(
            crate::test_support::with_decode_context(|ctx| super::counted_lanes(
                ctx,
                &[0x01, 0x03, 0x42, 0xff, 0x01, 0x11,]
            ))
            .unwrap()
            .is_empty()
        );
        assert!(
            crate::test_support::with_decode_context(|ctx| super::counted_lanes(
                ctx,
                &[0x01, 0x03, 0x42, 0x80, 0x01, 0x11,]
            ))
            .unwrap()
            .is_empty()
        );
        assert!(
            crate::test_support::with_decode_context(|ctx| super::counted_lanes(
                ctx,
                &[0x01, 0x03, 0x42, 0x62, 0x01, 0x10,]
            ))
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn om_offset_store_abr_lane_requires_sixteen_slots_and_exact_terminator() {
        let mut bytes = vec![0xaa, 0x11];
        bytes.extend_from_slice(&[0xff; 6]);
        bytes.extend_from_slice(&[0x82, 0x83]);
        bytes.extend_from_slice(&[0xff; 9]);
        bytes.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03, 0xbb]);

        let lanes =
            crate::test_support::with_decode_context(|ctx| super::abr_lanes(ctx, &bytes)).unwrap();
        assert_eq!(lanes.len(), 1);
        assert_eq!(lanes[0].offset(), 1);
        assert_eq!(lanes[0].slots().len(), 16);
        assert_eq!(
            (
                lanes[0].slots()[6].atom.map(|index| index.atom.value()),
                lanes[0].slots()[6].offset
            ),
            (Some(643), 8)
        );
        assert_eq!(lanes[0].slots()[6].atom.unwrap().atom.raw(), [0x82, 0x83]);
        assert!(lanes[0]
            .slots()
            .iter()
            .enumerate()
            .all(|(slot, token)| slot == 6
                || token.atom.map_or(&[0xff][..], |index| index.atom.raw()) == [0xff]));
        assert!(lanes[0]
            .slots()
            .iter()
            .enumerate()
            .all(|(slot, token)| slot == 6 || token.atom.is_none()));

        bytes[23] = b'X';
        assert!(
            crate::test_support::with_decode_context(|ctx| super::abr_lanes(ctx, &bytes))
                .unwrap()
                .is_empty()
        );
        bytes[23] = b'R';
        bytes.remove(18);
        assert!(
            crate::test_support::with_decode_context(|ctx| super::abr_lanes(ctx, &bytes))
                .unwrap()
                .is_empty()
        );
    }
}
