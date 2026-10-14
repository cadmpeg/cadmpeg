// SPDX-License-Identifier: Apache-2.0
//! Prefix-anchored FSET compact-index lanes, without a target namespace.

use super::compact::{CompactIndexAtom, CountedIndexMembers, LocatedCompactIndex};
use super::operation_record::OperationPayload;
use crate::layout::fset_parameter_prefix as prefix;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::num::NonZeroU8;

const INDEX_START: usize = prefix::LEN + 2;
const SUFFIX: [u8; 3] = [0, 3, 0];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FsetIndexValues {
    Empty,
    Counted(CountedIndexMembers<LocatedCompactIndex<u64>, 1>),
}

/// A complete class-prefixed lane and exact file positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FsetIndexLane {
    parameter_class_index: NonZeroU8,
    source_offset: u64,
    values: FsetIndexValues,
}

impl FsetIndexLane {
    pub(crate) fn new(
        parameter_class_index: u8,
        source_offset: u64,
        values: FsetIndexValues,
    ) -> Result<Self, &'static str> {
        let parameter_class_index = NonZeroU8::new(parameter_class_index)
            .ok_or("parameter_class_index: requires a nonzero one-based index")?;
        let mut at = source_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(INDEX_START))
            .ok_or("source_offset: FSET prefix overflows")?;
        match &values {
            FsetIndexValues::Empty => {
                source_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(prefix::LEN + 4))
                    .ok_or("source_offset: empty FSET lane overflows")?;
            }
            FsetIndexValues::Counted(members) => {
                for member in members.as_slice() {
                    if member.offset != at {
                        return Err("members.source_offset: requires serialized index order");
                    }
                    at = at
                        .checked_add(u64::from(member.atom.byte_len()))
                        .ok_or("members.source_offset: FSET index overflows")?;
                }
                at.checked_add(3)
                    .ok_or("source_offset: FSET suffix overflows")?;
            }
        }
        Ok(Self {
            parameter_class_index,
            source_offset,
            values,
        })
    }

    pub(crate) fn parameter_class_index(&self) -> u8 {
        self.parameter_class_index.get()
    }

    pub(crate) fn source_offset(&self) -> u64 {
        self.source_offset
    }

    pub(crate) fn values(&self) -> &FsetIndexValues {
        &self.values
    }

    pub(crate) fn read(
        ctx: &DecodeContext<'_>,
        record: OperationPayload<'_>,
        entry_offset: u64,
    ) -> Result<Option<Self>, CodecError> {
        if record.name() != "FSET" {
            return Ok(None);
        }
        let bytes = record.payload();
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(bytes.len()),
            "NX FSET index lane",
        )?;
        let Some(class) = bytes
            .get(prefix::CLASS_INDEX)
            .copied()
            .filter(|value| *value != 0)
        else {
            return Ok(None);
        };
        if bytes.get(prefix::FIXED_PREFIX..prefix::ZEROS) != Some(&prefix::FIXED_PREFIX_VALUE)
            || bytes.get(prefix::ZEROS..prefix::LEN) != Some(&prefix::ZEROS_VALUE)
        {
            return Ok(None);
        }
        let Some(source_offset) = entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(
            record.payload_offset(),
        )) else {
            return Ok(None);
        };
        if bytes.get(prefix::LEN..prefix::LEN + 4) == Some(&[0, 0, 3, 0]) {
            return Ok(Self::new(class, source_offset, FsetIndexValues::Empty).ok());
        }
        if bytes.get(prefix::LEN) != Some(&1) {
            return Ok(None);
        }
        let Some(count) = bytes
            .get(prefix::LEN + 1)
            .copied()
            .filter(|count| *count >= 2)
        else {
            return Ok(None);
        };
        let mut at = INDEX_START;
        for _ in 1..count {
            let Some(atom) = bytes.get(at..).and_then(CompactIndexAtom::read) else {
                return Ok(None);
            };
            at += usize::from(atom.byte_len());
        }
        if bytes.get(at..at + 3) != Some(&SUFFIX) {
            return Ok(None);
        }
        let mut members = ctx.collection_vec(usize::from(count - 1), "NX FSET compact indices")?;
        at = INDEX_START;
        for _ in 1..count {
            let Some(atom) = bytes.get(at..).and_then(CompactIndexAtom::read) else {
                return Err(CodecError::malformed("NX validated FSET lane changed"));
            };
            let Some(offset) = source_offset.checked_add(cadmpeg_core::decode::u64_from_index(at))
            else {
                return Ok(None);
            };
            members.push(LocatedCompactIndex { atom, offset });
            at += usize::from(atom.byte_len());
        }
        let members = CountedIndexMembers::new(members)
            .map_err(|_| CodecError::malformed("NX validated FSET count changed"))?;
        Ok(Self::new(class, source_offset, FsetIndexValues::Counted(members)).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::{prefix, FsetIndexLane, FsetIndexValues};
    use crate::om::operation_record::OperationPayload;

    fn payload(lane: &[u8]) -> Vec<u8> {
        let mut bytes = vec![49];
        bytes.extend_from_slice(&prefix::FIXED_PREFIX_VALUE);
        bytes.extend_from_slice(&prefix::ZEROS_VALUE);
        bytes.extend_from_slice(lane);
        bytes
    }

    #[test]
    fn fset_counted_lane_keeps_compact_width_and_absolute_positions() {
        let bytes = payload(&[1, 3, 0x85, 0x17, 94, 0, 3, 0]);
        crate::test_support::with_decode_context(|ctx| {
            let record = OperationPayload::new(&bytes, 100, "FSET").unwrap();
            let lane = FsetIndexLane::read(ctx, record, 200).unwrap().unwrap();
            let FsetIndexValues::Counted(members) = lane.values() else {
                panic!("counted lane");
            };
            assert_eq!(lane.source_offset(), 300);
            assert_eq!(lane.parameter_class_index(), 49);
            assert_eq!(members.declared_count(), 3);
            assert_eq!(members.as_slice()[0].atom.value(), 1303);
            assert_eq!(members.as_slice()[0].atom.raw(), &[0x85, 0x17]);
            assert_eq!(members.as_slice()[0].offset, 323);
            assert_eq!(members.as_slice()[1].atom.value(), 94);
            assert_eq!(members.as_slice()[1].offset, 325);
        });
    }

    #[test]
    fn fset_empty_spelling_has_no_declared_count() {
        let bytes = payload(&[0, 0, 3, 0]);
        crate::test_support::with_decode_context(|ctx| {
            let record = OperationPayload::new(&bytes, 100, "FSET").unwrap();
            let lane = FsetIndexLane::read(ctx, record, 200).unwrap().unwrap();
            assert_eq!(lane.values(), &FsetIndexValues::Empty);
        });
    }

    #[test]
    fn fset_lane_rejects_bad_count_null_indices_suffix_and_prefix() {
        let good = payload(&[1, 3, 0x85, 0x17, 94, 0, 3, 0]);
        crate::test_support::with_decode_context(|ctx| {
            for (offset, value) in [(0, 0), (1, 1), (22, 4), (23, 0xff), (28, 1)] {
                let mut bytes = good.clone();
                bytes[offset] = value;
                let record = OperationPayload::new(&bytes, 100, "FSET").unwrap();
                assert!(FsetIndexLane::read(ctx, record, 200).unwrap().is_none());
            }
            let record = OperationPayload::new(&good[..28], 100, "FSET").unwrap();
            assert!(FsetIndexLane::read(ctx, record, 200).unwrap().is_none());
        });
    }

    #[test]
    fn fset_counted_lane_propagates_collection_refusal() {
        let bytes = payload(&[1, 3, 0x85, 0x17, 94, 0, 3, 0]);
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_collection_items = 0,
            |ctx| {
                let record = OperationPayload::new(&bytes, 100, "FSET").unwrap();
                assert!(matches!(
                    FsetIndexLane::read(ctx, record, 200),
                    Err(cadmpeg_core::CodecError::ResourceLimit(_))
                ));
            },
        );
    }
}
