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
    byte_len: u16,
}

impl<T, O> DraftLeadingLane<T, O> {
    pub(crate) fn declared_count(&self) -> u8 {
        self.indices.declared_count()
    }
    fn extent<'a, E, I: Iterator<Item = &'a CompactIndexTarget<T>>>(
        indices: &'a [CompactIndexTarget<T>],
        admit: impl FnOnce(&'a [CompactIndexTarget<T>]) -> Result<I, E>,
    ) -> Result<Option<u16>, E>
    where
        T: 'a,
    {
        Ok(admit(indices)?.try_fold(26_u16, |length, token| {
            length.checked_add(u16::from(token.atom.byte_len()))
        }))
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
            offset = offset + O::from(u16::from(token.atom.byte_len()));
            positioned
        })
    }
}

impl<T> DraftLeadingLane<T, usize> {
    fn from_wire(
        ctx: &DecodeContext<'_>,
        indices: CountedIndexMembers<CompactIndexTarget<T>, 1>,
        offset: usize,
    ) -> Result<Option<Self>, CodecError> {
        let byte_len = Self::extent(indices.as_slice(), |indices| {
            ctx.admit_iter(indices, "NX draft leading token widths")
        })?
        .ok_or_else(|| ctx.refuse_codec_limit("NX draft leading extent", u64::MAX, u64::MAX))?;
        if offset.checked_add(usize::from(byte_len)).is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            offset,
            indices,
            byte_len,
        }))
    }
}

impl<T> DraftLeadingLane<T, u64> {
    pub(crate) fn new(
        indices: CountedIndexMembers<CompactIndexTarget<T>, 1>,
        offset: u64,
    ) -> Option<Self> {
        let byte_len = match Self::extent(indices.as_slice(), |indices| {
            Ok::<_, std::convert::Infallible>(indices.iter())
        }) {
            Ok(length) => length?,
            Err(error) => match error {},
        };
        offset.checked_add(u64::from(byte_len))?;
        Some(Self {
            offset,
            indices,
            byte_len,
        })
    }
}

impl DraftLeadingLane<(), usize> {
    pub(crate) fn into_absolute(self, base: u64) -> Option<DraftLeadingLane<(), u64>> {
        let offset = base.checked_add(u64_from_index(self.offset))?;
        offset.checked_add(u64::from(self.byte_len))?;
        Some(DraftLeadingLane {
            offset,
            indices: self.indices,
            byte_len: self.byte_len,
        })
    }
}

impl<O> DraftLeadingLane<(), O> {
    pub(crate) fn resolve<T>(
        self,
        ctx: &DecodeContext<'_>,
        mut resolve: impl FnMut(u32) -> Result<T, CodecError>,
    ) -> Result<DraftLeadingLane<T, O>, CodecError> {
        Ok(DraftLeadingLane {
            offset: self.offset,
            byte_len: self.byte_len,
            indices: self.indices.map_charged(ctx, |token| {
                Ok(CompactIndexTarget {
                    atom: token.atom,
                    target: resolve(token.atom.value())?,
                })
            })?,
        })
    }

    pub(crate) fn try_resolve<T>(
        self,
        mut resolve: impl FnMut(u32) -> Option<T>,
    ) -> Option<DraftLeadingLane<T, O>> {
        Some(DraftLeadingLane {
            offset: self.offset,
            byte_len: self.byte_len,
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
pub(crate) fn scan(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<DraftLeadingLane>, CodecError> {
    const PREFIX: [u8; 22] = [
        0x67, 0x00, 0x00, 0x01, 0x00, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0x03, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ];
    if record.name() != "DRAFT" || record.payload().get(..PREFIX.len()) != Some(&PREFIX) {
        return Ok(None);
    }
    let mut at = PREFIX.len();
    if record.payload().get(at) != Some(&0x01) {
        return Ok(None);
    }
    let Some(&declared_count) = record.payload().get(at + 1) else {
        return Ok(None);
    };
    if declared_count < 2 {
        return Ok(None);
    }
    at += 2;
    let member_count = usize::from(declared_count - 1);
    let mut scan_at = at;
    for _ in ctx.admit_iter(&(1..declared_count), "scan NX draft leading indices")? {
        let Some(token) = LocatedCompactIndex::read(record.payload(), scan_at) else {
            return Ok(None);
        };
        scan_at += token.atom.raw().len();
    }
    if record.payload().get(scan_at..scan_at + 2) != Some(&[0x01, 0x02]) {
        return Ok(None);
    }
    let operation = "NX draft leading index members";
    let mut indices = ctx.collection_vec(member_count, operation)?;
    for _ in ctx.admit_iter(&(1..declared_count), "scan NX draft leading indices")? {
        let Some(token) = LocatedCompactIndex::read(record.payload(), at) else {
            return Ok(None);
        };
        at += token.atom.raw().len();
        indices.push(token.atom.into());
    }

    let Ok(indices) = CountedIndexMembers::new(indices) else {
        return Ok(None);
    };
    DraftLeadingLane::from_wire(ctx, indices, record.payload_offset())
}

#[cfg(test)]
mod tests {
    use super::super::operation_record::OperationPayload;
    use super::scan;

    fn draft_leading_limit_error(
        adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> cadmpeg_core::CodecError {
        let bytes = [
            0x67, 0, 0, 1, 0, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 3, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0xff, 1, 2, 8, 1, 2,
        ];

        crate::test_support::with_decode_context_over(&bytes, adjust, |ctx| {
            scan(ctx, OperationPayload::new(&bytes, 100, "DRAFT").unwrap())
                .expect_err("draft leading resource refusal")
        })
    }

    #[test]
    fn om_draft_leading_route_refuses_collection_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_collection_items = 0;
        };
        assert!(
            matches!(draft_leading_limit_error(adjust_policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn om_draft_leading_route_refuses_retained_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_retained_bytes = 0;
        };
        assert!(
            matches!(draft_leading_limit_error(adjust_policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn om_draft_leading_route_refuses_work_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_work_units = 0;
        };
        assert!(
            matches!(draft_leading_limit_error(adjust_policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
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
                u8::try_from(count + 1).expect("fixture value fits u8"),
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
            })
            .unwrap()
            .unwrap();
            assert_eq!(usize::from(frame.declared_count()), count + 1);
            assert_eq!(
                frame
                    .indices()
                    .map(|token| token.offset)
                    .collect::<Vec<_>>(),
                positions
            );
            let base = u64::MAX - 100 - cadmpeg_core::decode::u64_from_index(bytes.len());
            let absolute = frame.clone().into_absolute(base).unwrap();
            assert_eq!(
                absolute
                    .indices()
                    .map(|token| token.offset)
                    .collect::<Vec<_>>(),
                positions
                    .iter()
                    .map(|offset| base + cadmpeg_core::decode::u64_from_index(*offset))
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

    #[test]
    fn draft_leading_width_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "NX draft leading token widths",
            |ctx| {
                let indices = crate::om::compact::CountedIndexMembers::new(vec![
                    crate::om::compact::CompactIndexAtom::from_wire(1, &[1])
                        .unwrap()
                        .into(),
                ])
                .unwrap();
                super::DraftLeadingLane::<()>::from_wire(ctx, indices, 0)
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX draft leading token widths"));
    }
}
