// SPDX-License-Identifier: Apache-2.0
//! Counted extrusion profile tokens with a shared duplicate-list witness.

use super::branch_items::BranchItems;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use std::num::NonZeroUsize;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtrudeProfileReferenceField {
    field_tag: u8,
    references: BranchItems<PayloadIndexToken>,
    primary_offset: u64,
    witness_offset: Option<u64>,
}

impl ExtrudeProfileReferenceField {
    pub(crate) fn field_tag(&self) -> u8 {
        self.field_tag
    }

    pub(crate) fn relocate(mut self, ctx: &DecodeContext<'_>, base: u64) -> Result<Option<Self>, CodecError> {
        (|| {
        let width = propagate_resource!(ctx.admit_iter(self.references.as_slice(), "NX extrude profile token widths").map_err(CodecError::from))
            .try_fold(0_u64, |width, token| width.checked_add(u64_from_index(token.raw().len())));
        let width = propagate_resource!(width.ok_or_else(|| ctx.refuse_codec_limit("NX extrude profile extent", u64::MAX, u64::MAX)));
        self.primary_offset = base.checked_add(self.primary_offset)?;
        self.primary_offset.checked_add(width)?.checked_add(3)?;
        if let Some(offset) = self.witness_offset {
            let offset = base.checked_add(offset)?;
            offset.checked_add(width)?.checked_add(2)?;
            self.witness_offset = Some(offset);
        }
        Some(Ok(self))
        })().transpose()
    }

    pub(crate) fn references(
        &self,
    ) -> impl Iterator<Item = (PayloadIndexToken, u64, Option<u64>)> + '_ {
        let mut relative = 0;
        self.references.as_slice().iter().map(move |token| {
            let position = relative;
            relative += cadmpeg_core::decode::u64_from_index(token.raw().len());
            (
                *token,
                self.primary_offset + position,
                self.witness_offset.map(|offset| offset + position),
            )
        })
    }
}

/// Decode the unique witnessed profile-reference field in an `EXTRUDE` payload.
pub(crate) fn extrude_profile_references(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<ExtrudeProfileReferenceField>, CodecError> {
    if record.name() != "EXTRUDE" {
        return Ok(None);
    }
    let mut shape = None;
    if let Some(range_end) = record.payload().len().checked_sub(6) {
            for start in ctx.admit_iter(&(0..range_end), "NX extrude profile references range traversal")? {
        if record.payload().get(start..start + 2) != Some(&[0x01, 0x02])
            || record.payload().get(start + 3) != Some(&0x01) { continue; }
        if let Some(candidate) = extrude_profile_reference_shape(ctx, record, start)? {
            if shape.is_some() { return Ok(None); }
            shape = Some(candidate);
        }
    }
        }
    let Some((start, count, references_start, witness_start)) = shape else {
        return Ok(None);
    };
    let count = usize::from(count - 1);

    let mut references = ctx.collection_vec(count, "NX extrude profile references")?;
    let mut at = references_start;
    for _ in ctx.admit_iter(&(0..count), "NX extrude profile references range traversal")? {
        let Some(token) = record.payload().get(at..).and_then(PayloadIndexToken::read) else {
            return Ok(None);
        };
        at += token.raw().len();
        references.push(token);
    }
    let Some(primary_offset) = record.payload_offset().checked_add(references_start) else {
        return Ok(None);
    };
    let witness_offset =
        witness_start.and_then(|start| record.payload_offset().checked_add(start)?.checked_add(2));
    Ok(BranchItems::new(references)
        .ok()
        .map(|references| ExtrudeProfileReferenceField {
            field_tag: record.payload()[start + 2],
            references,
            primary_offset: u64_from_index(primary_offset),
            witness_offset: witness_offset.map(u64_from_index),
        }))
}

fn extrude_profile_reference_shape(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
    start: usize,
) -> Result<Option<(usize, u8, usize, Option<usize>)>, CodecError> {
    (|| {
    let count = *record.payload().get(start + 4)?;
    if count < 2 {
        return None;
    }
    let references_start = start + 5;
    let mut at = references_start;
    for _ in propagate_resource!(ctx.admit_iter(&(1..count), "NX extrude profile reference shape range traversal").map_err(CodecError::from)) {
        let token = PayloadIndexToken::read(record.payload().get(at..)?)?;
        at += token.raw().len();
    }
    if record.payload().get(at..at + 3) != Some(&[0x01, 0x03, 0x79]) {
        return None;
    }
    record.payload_offset().checked_add(references_start)?;
    let encoded_references = record.payload().get(references_start..at)?;
    let witness_len = 2 + encoded_references.len() + 2;
    let width = NonZeroUsize::new(witness_len)?;
    let mut witness_start = None;
    for (witness_offset, candidate) in propagate_resource!(ctx.admit_iter(record.payload(), "NX extrusion witness windows").map_err(CodecError::from))
        .windows(width).enumerate() {
        if candidate.starts_with(&[0x01, count])
            && propagate_resource!(ctx.equal(&(candidate.get(2..2 + encoded_references.len())), &(Some(encoded_references)), "NX extrude profile reference shape equality"))
            && candidate.ends_with(&[0x00, 0x00]) {
            if witness_start.is_some() {
                return Some(Ok(Some((start, count, references_start, None))));
            }
            witness_start = Some(witness_offset);
        }
    }
    Some(Ok(Some((start, count, references_start, witness_start))))
    })().transpose().map(Option::flatten)
}

#[cfg(test)]
mod tests {
    fn extrude_profile_references_test(
        record: super::OperationPayload<'_>,
    ) -> Option<super::ExtrudeProfileReferenceField> {
        crate::test_support::with_decode_context(|ctx| {
            super::extrude_profile_references(ctx, record)
        })
        .unwrap()
    }

    use super::super::operation_record::OperationPayload;

    #[test]
    fn extrude_profile_references_refuse_collection_limit() {
        let bytes = b"\x01\x02\x00\x01\x03\xf0\x00\xf1\x01\x00\x01\x03\x79\x01\x03\xf0\x00\xf1\x01\x00\x00\x00";
        let record = OperationPayload::new(bytes, 100, "EXTRUDE").unwrap();

        crate::test_support::with_decode_context_over(
            bytes,
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                let error = super::extrude_profile_references(ctx, record).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
                );
            },
        );
    }

    #[test]
    fn relocation_preserves_the_shared_witness_and_checks_both_complete_spans() {
        let bytes = b"\x01\x02\x00\x01\x03\xf0\x00\xf1\x01\x00\x01\x03\x79\x01\x03\xf0\x00\xf1\x01\x00\x00\x00";
        let field =
            extrude_profile_references_test(OperationPayload::new(bytes, 100, "EXTRUDE").unwrap())
                .unwrap();
        let relocated = crate::test_support::with_decode_context(|ctx| field.clone().relocate(ctx, 1000)).unwrap().unwrap();
        let rows: Vec<_> = relocated.references().collect();
        assert_eq!((rows[0].1, rows[0].2), (1105, Some(1115)));
        assert_eq!((rows[1].1, rows[1].2), (1107, Some(1117)));
        let maximum_base = u64::MAX - 100 - cadmpeg_core::decode::u64_from_index(bytes.len());
        assert!(crate::test_support::with_decode_context(|ctx| field.clone().relocate(ctx, maximum_base)).unwrap().is_some());
        assert!(crate::test_support::with_decode_context(|ctx| field.clone().relocate(ctx, maximum_base + 1)).unwrap().is_none());
        let no_witness = extrude_profile_references_test(
            OperationPayload::new(&bytes[..13], 100, "EXTRUDE").unwrap(),
        )
        .unwrap();
        assert!(no_witness.references().all(|row| row.2.is_none()));
        assert!(crate::test_support::with_decode_context(|ctx| no_witness.clone().relocate(ctx, u64::MAX - 113)).unwrap().is_some());
        assert!(crate::test_support::with_decode_context(|ctx| no_witness.clone().relocate(ctx, u64::MAX - 112)).unwrap().is_none());
    }
}
