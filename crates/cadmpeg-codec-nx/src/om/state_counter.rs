// SPDX-License-Identifier: Apache-2.0
//! Framed operation-state counter rows.

use super::discriminators::OperationStateCounterKind;
use super::state_index::StateIndexToken;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StateCounter<O = u64> {
    offset: O,
    kind: OperationStateCounterKind,
    object: StateIndexToken,
    introduced: u8,
    modified: u8,
}

impl<O: Copy> StateCounter<O> {
    pub(crate) fn offset(self) -> O {
        self.offset
    }
    pub(crate) fn kind(self) -> OperationStateCounterKind {
        self.kind
    }
    pub(crate) fn object(self) -> StateIndexToken {
        self.object
    }
    pub(crate) fn introduced(self) -> u8 {
        self.introduced
    }
    pub(crate) fn modified(self) -> u8 {
        self.modified
    }
    fn byte_len(self) -> usize {
        5 + self.object.raw().len()
    }
}

impl StateCounter<usize> {
    fn read(bytes: &[u8], at: usize, base: usize) -> Option<Self> {
        let tail = bytes.get(at..)?;
        if tail.first() != Some(&0x05) {
            return None;
        }
        let kind = OperationStateCounterKind::try_from(*tail.get(1)?).ok()?;
        let object = StateIndexToken::read_at(tail, 2)?;
        let state_at = 2 + object.raw().len();
        let introduced = *tail.get(state_at)?;
        let modified = *tail.get(state_at + 1)?;
        if tail.get(state_at + 2) != Some(&0x4e) {
            return None;
        }
        let offset = base.checked_add(at)?;
        offset.checked_add(state_at + 3)?;
        Some(Self {
            offset,
            kind,
            object,
            introduced,
            modified,
        })
    }

    pub(crate) fn into_absolute(self, base: u64) -> Option<StateCounter> {
        StateCounter::new(
            base.checked_add(u64::try_from(self.offset).ok()?)?,
            self.kind,
            self.object,
            self.introduced,
            self.modified,
        )
        .ok()
    }
}

impl StateCounter {
    pub(crate) fn new(
        offset: u64,
        kind: OperationStateCounterKind,
        object: StateIndexToken,
        introduced: u8,
        modified: u8,
    ) -> Result<Self, &'static str> {
        offset
            .checked_add(5 + object.raw().len() as u64)
            .ok_or("source_offset: counter row extent overflows")?;
        Ok(Self {
            offset,
            kind,
            object,
            introduced,
            modified,
        })
    }

    pub(crate) fn object_offset(self) -> u64 {
        self.offset + 2
    }
}

/// Contiguous map with at least two complete rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StateCounterMap {
    first: [StateCounter<usize>; 2],
    rest: Vec<StateCounter<usize>>,
}

impl StateCounterMap {
    pub(super) fn offset(&self) -> usize {
        self.first[0].offset()
    }

    pub(crate) fn into_rows(self) -> impl Iterator<Item = StateCounter<usize>> {
        self.first.into_iter().chain(self.rest)
    }

    /// Decode the contiguous operation-state counter-map suffix of a bounded area.
    ///
    /// The map is selected by the longest run of complete `05, row_kind, index,
    /// state, state, 4e` rows whose remaining bounded tail is small enough to be
    /// an area footer. This end anchor prevents a syntactically valid short lane in
    /// an operation payload from becoming a state map.
    pub(super) fn read(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        base_offset: usize,
    ) -> Result<Option<Self>, CodecError> {
        const MAX_COUNTER_TAIL_BYTES: usize = 64;
        ctx.charge_work(
            u64_from_index(bytes.len()),
            "scan NX operation-state counter map",
        )?;
        let mut best: Option<(usize, usize, usize)> = None;
        let mut run_start = 0;
        let mut run_end = 0;
        let mut run_len = 0;
        for at in 0..bytes.len().saturating_sub(2) {
            if bytes.get(at) != Some(&0x05) || !matches!(bytes.get(at + 1), Some(0x01 | 0x02)) {
                continue;
            }
            let Some(row) = StateCounter::read(bytes, at, base_offset) else {
                continue;
            };
            let Some(row_end) = at.checked_add(row.byte_len()) else {
                return Ok(None);
            };
            if at == run_end {
                run_end = row_end;
                run_len += 1;
            } else {
                run_start = at;
                run_end = row_end;
                run_len = 1;
            }
            if run_len >= 2
                && bytes.len().saturating_sub(run_end) <= MAX_COUNTER_TAIL_BYTES
                && best.is_none_or(|(_, _, current_len)| run_len > current_len)
            {
                best = Some((run_start, run_end, run_len));
            }
        }
        let Some((start, end, row_count)) = best else {
            return Ok(None);
        };
        let Some(first) = StateCounter::read(bytes, start, base_offset) else {
            return Ok(None);
        };
        let Some(second_at) = start.checked_add(first.byte_len()) else {
            return Ok(None);
        };
        let Some(second) = StateCounter::read(bytes, second_at, base_offset) else {
            return Ok(None);
        };
        let rest_count = row_count - 2;
        let operation = "NX operation-state counter rows";
        let count = u64_from_index(rest_count);
        ctx.charge_collection_items(count, operation)?;
        let retained = count
            .checked_mul(u64_from_index(std::mem::size_of::<StateCounter<usize>>()))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, count))?;
        ctx.charge_retained(retained, operation)?;
        let mut rest = Vec::new();
        rest.try_reserve_exact(rest_count)
            .map_err(|_| ctx.refuse_codec_limit(operation, 0, count))?;
        let Some(mut cursor) = second_at.checked_add(second.byte_len()) else {
            return Ok(None);
        };
        while cursor < end {
            let Some(row) = StateCounter::read(bytes, cursor, base_offset) else {
                return Ok(None);
            };
            let Some(next) = cursor.checked_add(row.byte_len()) else {
                return Ok(None);
            };
            cursor = next;
            rest.push(row);
        }
        Ok((cursor == end).then_some(Self {
            first: [first, second],
            rest,
        }))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn operation_state_counter_map_anchors_to_the_longest_bounded_suffix() {
        let mut bytes = vec![0x41, 0x83, 0x20, 0x3f];
        bytes.extend([
            0x05, 0x01, 0x90, 0x12, 0x34, 0x56, 0x57, 0x4e, 0x05, 0x02, 0xa3, 0x1f, 0x85, 0x2a,
            0x2b, 0x4e, 0x05, 0x01, 0x7d, 0x63, 0x63, 0x4e,
        ]);
        bytes.extend([
            0xb8, 0x6e, 0x58, 0x81, 0xd8, 0xb9, 0x96, 0x62, 0xdf, 0x59, 0xb8, 0x59, 0xc0, 0xd1,
            0xf1, 0xed,
        ]);

        let map = crate::test_support::with_decode_context(|ctx| {
            crate::om::state_counter::StateCounterMap::read(ctx, &bytes, 1000)
        })
        .unwrap()
        .expect("counter-map suffix");
        assert_eq!(map.offset(), 1004);
        let rows: Vec<_> = map.into_rows().collect();
        assert_eq!(rows.len(), 3);
        let end_offset = rows[2].offset() + rows[2].byte_len();
        assert_eq!(end_offset, 1004 + 8 + 8 + 6);
        assert_eq!(bytes.len() - (end_offset - 1000), 16);
        assert_eq!(u8::from(rows[0].kind()), 1);
        assert_eq!(Some(rows[0].object().value()), Some(0x1234));
        assert_eq!(rows[0].introduced(), 0x56);
        assert_eq!(rows[0].modified(), 0x57);
        assert_eq!(u8::from(rows[1].kind()), 2);
        assert_eq!(Some(rows[1].object().value()), Some(0x31f85));
        assert_eq!(rows[1].introduced(), 0x2a);
        assert_eq!(rows[1].modified(), 0x2b);
        assert_eq!(rows[2].object().raw().len(), 1);
    }

    #[test]
    fn operation_state_counter_map_rejects_a_short_non_suffix_lane() {
        let bytes = [
            0x05, 0x01, 0x12, 0x34, 0x56, 0x4e, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
            0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
            0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
            0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
            0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
        ];
        assert!(crate::test_support::with_decode_context(|ctx| {
            crate::om::state_counter::StateCounterMap::read(ctx, &bytes, 0)
        })
        .unwrap()
        .is_none());
    }

    #[test]
    fn operation_state_counter_map_refuses_collection_limit() {
        let bytes = [
            0x05, 0x01, 0x90, 0x12, 0x34, 0x56, 0x57, 0x4e, 0x05, 0x02, 0xa3, 0x1f, 0x85, 0x2a,
            0x2b, 0x4e, 0x05, 0x01, 0x7d, 0x63, 0x63, 0x4e,
        ];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let error = super::StateCounterMap::read(&ctx, &bytes, 0).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn operation_state_counter_map_refuses_retained_limit() {
        let bytes = [
            0x05, 0x01, 0x90, 0x12, 0x34, 0x56, 0x57, 0x4e, 0x05, 0x02, 0xa3, 0x1f, 0x85, 0x2a,
            0x2b, 0x4e, 0x05, 0x01, 0x7d, 0x63, 0x63, 0x4e,
        ];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let error = super::StateCounterMap::read(&ctx, &bytes, 0).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn operation_state_counter_map_refuses_work_limit() {
        let bytes = [
            0x05, 0x01, 0x90, 0x12, 0x34, 0x56, 0x57, 0x4e, 0x05, 0x02, 0xa3, 0x1f, 0x85, 0x2a,
            0x2b, 0x4e, 0x05, 0x01, 0x7d, 0x63, 0x63, 0x4e,
        ];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let error = super::StateCounterMap::read(&ctx, &bytes, 0).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
    }
}
