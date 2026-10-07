// SPDX-License-Identifier: Apache-2.0
//! Byte-counted pattern references with derived contiguous token positions.

use super::branch_items::BranchItems;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

const TRAILER: [u8; 19] = [
    0, 0, 0, 0x37, 0xff, 0xff, 1, 0, 0, 0, 0x38, 0xff, 1, 0xff, 0xff, 0xff, 0xff, 1, 0xff,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CountedPatternReferences<B> {
    offset: u64,
    entries: BranchItems<(PayloadIndexToken, B)>,
}

impl<B> CountedPatternReferences<B> {
    pub(crate) fn new(
        offset: u64,
        entries: BranchItems<(PayloadIndexToken, B)>,
    ) -> Result<Self, &'static str> {
        match Self::validate(offset, entries.as_slice(), |entries| {
            Ok::<_, std::convert::Infallible>(entries.iter())
        }) {
            Ok(valid) => valid?,
            Err(error) => match error {},
        }
        Ok(Self { offset, entries })
    }

    fn validate<'a, E, I: Iterator<Item = &'a (PayloadIndexToken, B)>>(
        offset: u64,
        entries: &'a [(PayloadIndexToken, B)],
        admit: impl FnOnce(&'a [(PayloadIndexToken, B)]) -> Result<I, E>,
    ) -> Result<Result<(), &'static str>, E>
    where
        B: 'a,
    {
        let end = admit(entries)?
            .fold(Some(offset), |end, (token, _)| {
                end.and_then(|end| end.checked_add(u64_from_index(token.raw().len())))
            })
            .and_then(|end| end.checked_add(2))
            .and_then(|end| end.checked_add(u64_from_index(TRAILER.len())));
        Ok(end
            .map(|_| ())
            .ok_or("source_offset: counted reference frame overflows"))
    }

    fn from_wire(
        ctx: &DecodeContext<'_>,
        offset: u64,
        entries: BranchItems<(PayloadIndexToken, B)>,
    ) -> Result<Option<Self>, CodecError> {
        if Self::validate(offset, entries.as_slice(), |entries| {
            ctx.admit_iter(entries, "NX counted pattern token widths")
        })?
        .is_err()
        {
            return Ok(None);
        }
        Ok(Some(Self { offset, entries }))
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }

    pub(crate) fn declared_count(&self) -> u8 {
        self.entries.declared_count()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (u64, PayloadIndexToken, &B)> + Clone {
        let mut at = self.offset + 2;
        self.entries.as_slice().iter().map(move |(token, target)| {
            let offset = at;
            at += cadmpeg_core::decode::u64_from_index(token.raw().len());
            (offset, *token, target)
        })
    }
}

impl CountedPatternReferences<()> {
    pub(crate) fn read(
        ctx: &DecodeContext<'_>,
        record: OperationPayload<'_>,
    ) -> Result<Option<Self>, CodecError> {
        if record.name() != "Pattern Feature" {
            return Ok(None);
        }
        let bytes = record.payload();
        let shape = |start: usize| -> Result<Option<(usize, u8)>, CodecError> {
            if bytes.get(start) != Some(&1) {
                return Ok(None);
            }
            let Some(count @ 1..) = bytes.get(start + 1).and_then(|value| value.checked_sub(1))
            else {
                return Ok(None);
            };
            let Some(at) = start.checked_add(2) else {
                return Ok(None);
            };
            let Some(remaining) = bytes.len().checked_sub(at) else {
                return Ok(None);
            };
            if cadmpeg_core::decode::bounded_len(u64::from(count), 2, remaining).is_none() {
                return Ok(None);
            }
            let mut scan_at = at;
            let mut rows = 0..count;
            while ctx
                .next_charged(&mut rows, "NX counted pattern reference validation")?
                .is_some()
            {
                let Some(token) = bytes.get(scan_at..).and_then(PayloadIndexToken::read) else {
                    return Ok(None);
                };
                scan_at += token.raw().len();
            }
            let Some(end) = scan_at.checked_add(TRAILER.len()) else {
                return Ok(None);
            };
            if bytes.get(scan_at..end) != Some(&TRAILER)
                || record.payload_offset().checked_add(end).is_none()
            {
                return Ok(None);
            }
            Ok(Some((start, count)))
        };
        let mut candidate = None;
        let mut starts = 0..bytes.len();
        while let Some(start) =
            ctx.next_charged(&mut starts, "scan NX counted pattern references")?
        {
            if let Some(next) = shape(start)? {
                if candidate.is_some() {
                    return Ok(None);
                }
                candidate = Some(next);
            }
        }
        let Some((start, count)) = candidate else {
            return Ok(None);
        };
        let count = usize::from(count);

        let mut entries = ctx.collection_vec(count, "NX counted pattern references")?;
        let Some(mut at) = start.checked_add(2) else {
            return Ok(None);
        };
        let mut rows = 0..count;
        while ctx
            .next_charged(&mut rows, "NX counted pattern reference materialization")?
            .is_some()
        {
            let Some(token) = bytes.get(at..).and_then(PayloadIndexToken::read) else {
                return Ok(None);
            };
            at += token.raw().len();
            entries.push((token, ()));
        }
        let Some(offset) = record.payload_offset().checked_add(start) else {
            return Ok(None);
        };
        let Ok(entries) = BranchItems::new(entries) else {
            return Ok(None);
        };
        Self::from_wire(ctx, u64_from_index(offset), entries)
    }

    pub(crate) fn resolve<B>(
        self,
        ctx: &DecodeContext<'_>,
        file_base: u64,
        mut target: impl FnMut(PayloadIndexToken) -> Result<B, CodecError>,
    ) -> Result<Option<CountedPatternReferences<B>>, CodecError> {
        let Some(offset) = self.offset.checked_add(file_base) else {
            return Ok(None);
        };
        let entries = self
            .entries
            .try_map_indexed_charged(ctx, |_, (token, ())| Ok((token, target(token)?)))?;
        CountedPatternReferences::from_wire(ctx, offset, entries)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn pattern_reference_width_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "NX counted pattern token widths",
            |ctx| {
                let entries = super::BranchItems::new(vec![(
                    super::PayloadIndexToken::read(&[0xf0, 1]).unwrap(),
                    (),
                )])
                .unwrap();
                super::CountedPatternReferences::from_wire(ctx, 0, entries)
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX counted pattern token widths"));
    }
}
