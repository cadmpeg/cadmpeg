// SPDX-License-Identifier: Apache-2.0
//! Counted extrusion profile tokens with a shared duplicate-list witness.

use super::branch_items::BranchItems;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
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

    pub(crate) fn relocate(
        mut self,
        ctx: &DecodeContext<'_>,
        base: u64,
    ) -> Result<Option<Self>, CodecError> {
        let width = ctx
            .admit_iter(
                self.references.as_slice(),
                "NX extrude profile token widths",
            )?
            .map(|token| u64_from_index(token.raw().len()))
            .sum::<u64>();
        let Some(primary_offset) = base.checked_add(self.primary_offset).filter(|offset| {
            offset
                .checked_add(width)
                .and_then(|end| end.checked_add(3))
                .is_some()
        }) else {
            return Ok(None);
        };
        self.primary_offset = primary_offset;
        if let Some(offset) = self.witness_offset {
            let Some(offset) = base.checked_add(offset).filter(|offset| {
                offset
                    .checked_add(width)
                    .and_then(|end| end.checked_add(2))
                    .is_some()
            }) else {
                return Ok(None);
            };
            self.witness_offset = Some(offset);
        }
        Ok(Some(self))
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
pub(crate) fn extrude_profile_references<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<
    (
        Option<ExtrudeProfileReferenceField>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("NX extrude profile scratch", || {
        read_extrude_profile_references(ctx, record)
    })
}

fn read_extrude_profile_references(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<ExtrudeProfileReferenceField>, CodecError> {
    if record.name() != "EXTRUDE" {
        return Ok(None);
    }
    let mut shape = None;
    if let Some(range_end) = record.payload().len().checked_sub(6) {
        let mut starts = 0..range_end;
        while let Some(start) =
            ctx.next_charged(&mut starts, "NX extrude profile candidate search")?
        {
            if record.payload().get(start..start + 2) != Some(&[0x01, 0x02])
                || record.payload().get(start + 3) != Some(&0x01)
            {
                continue;
            }
            if let Some(candidate) = extrude_profile_reference_shape(ctx, record, start)? {
                if shape.is_some() {
                    return Ok(None);
                }
                shape = Some(candidate);
            }
        }
    }
    let Some((start, count, references_start, references_end)) = shape else {
        return Ok(None);
    };
    let encoded_references = &record.payload()[references_start..references_end];
    let witness_len = 4 + encoded_references.len();
    let mut windows = record.payload().windows(witness_len).enumerate();
    let mut witness_start = None;
    while let Some((offset, candidate)) =
        ctx.next_charged(&mut windows, "NX extrusion witness windows")?
    {
        if candidate.starts_with(&[0x01, count])
            && candidate.ends_with(&[0x00, 0x00])
            && ctx.equal_bytes(
                &candidate[2..2 + encoded_references.len()],
                encoded_references,
                "NX extrusion witness token equality",
            )?
        {
            if witness_start.is_some() {
                witness_start = None;
                break;
            }
            witness_start = Some(offset);
        }
    }
    let count = usize::from(count - 1);

    let mut references = ctx.collection_vec(count, "NX extrude profile references")?;
    let mut at = references_start;
    let mut rows = 0..count;
    while ctx
        .next_charged(&mut rows, "NX extrude profile reference materialization")?
        .is_some()
    {
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
) -> Result<Option<(usize, u8, usize, usize)>, CodecError> {
    let Some(&count @ 2..) = record.payload().get(start + 4) else {
        return Ok(None);
    };
    let references_start = start + 5;
    let mut at = references_start;
    let mut rows = 1..count;
    while ctx
        .next_charged(&mut rows, "NX extrude profile reference shape tokens")?
        .is_some()
    {
        let Some(token) = record.payload().get(at..).and_then(PayloadIndexToken::read) else {
            return Ok(None);
        };
        at += token.raw().len();
    }
    if record.payload().get(at..at + 3) != Some(&[0x01, 0x03, 0x79])
        || record
            .payload_offset()
            .checked_add(references_start)
            .is_none()
    {
        return Ok(None);
    }
    Ok(Some((start, count, references_start, at)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn extrusion_rejects_two_shapes_before_searching_their_witnesses() {
        let shape = b"\x01\x02\x00\x01\x02\xf0\x00\x01\x03\x79";
        let mut bytes = shape.to_vec();
        bytes.extend(shape);
        bytes.resize(4096, 0);
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_work_units = 20;
            },
            |ctx| {
                let record = super::OperationPayload::new(&bytes, 0, "EXTRUDE").unwrap();
                assert!(super::extrude_profile_references(ctx, record)
                    .map(|(value, _storage)| value)
                    .unwrap()
                    .is_none());
            },
        );
    }

    #[test]
    fn extrusion_keeps_unique_and_ambiguous_witness_rules() {
        let shape = b"\x01\x02\x00\x01\x02\xf0\x00\x01\x03\x79";
        let witness = b"\x01\x02\xf0\x00\x00\x00";
        let mut bytes = shape.to_vec();
        for witness_count in 0..=2 {
            let field = crate::test_support::with_decode_context(|ctx| {
                super::extrude_profile_references(
                    ctx,
                    super::OperationPayload::new(&bytes, 100, "EXTRUDE").unwrap(),
                )
                .map(|(value, _storage)| value)
            })
            .unwrap()
            .unwrap();
            let rows: Vec<_> = field.references().collect();
            assert_eq!(rows[0].0.value(), 0);
            assert_eq!(rows[0].1, 105);
            assert_eq!(
                rows[0].2,
                if witness_count == 1 {
                    Some(100 + cadmpeg_core::decode::u64_from_index(shape.len()) + 2)
                } else {
                    None
                }
            );
            bytes.extend(witness);
        }
        crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "NX extrusion witness windows",
            |ctx| {
                super::extrude_profile_references(
                    ctx,
                    super::OperationPayload::new(shape, 0, "EXTRUDE").unwrap(),
                )
                .map(|(value, _storage)| value)
            },
        );
    }

    fn extrude_profile_references_test(
        record: super::OperationPayload<'_>,
    ) -> Option<super::ExtrudeProfileReferenceField> {
        crate::test_support::with_decode_context(|ctx| {
            super::extrude_profile_references(ctx, record).map(|(value, _storage)| value)
        })
        .unwrap()
    }

    use super::super::operation_record::OperationPayload;

    #[test]
    fn extrude_profile_references_refuse_collection_limit() {
        let bytes = b"\x01\x02\x00\x01\x03\xf0\x00\xf1\x01\x00\x01\x03\x79\x01\x03\xf0\x00\xf1\x01\x00\x00\x00";
        let record = OperationPayload::new(bytes, 100, "EXTRUDE").unwrap();

        let error = crate::test_support::resource_refusal_at(
            bytes,
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "NX extrude profile references",
            |ctx| super::extrude_profile_references(ctx, record).map(|(value, _storage)| value),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn relocation_preserves_the_shared_witness_and_checks_both_complete_spans() {
        let bytes = b"\x01\x02\x00\x01\x03\xf0\x00\xf1\x01\x00\x01\x03\x79\x01\x03\xf0\x00\xf1\x01\x00\x00\x00";
        let field =
            extrude_profile_references_test(OperationPayload::new(bytes, 100, "EXTRUDE").unwrap())
                .unwrap();
        let relocated =
            crate::test_support::with_decode_context(|ctx| field.clone().relocate(ctx, 1000))
                .unwrap()
                .unwrap();
        let rows: Vec<_> = relocated.references().collect();
        assert_eq!((rows[0].1, rows[0].2), (1105, Some(1115)));
        assert_eq!((rows[1].1, rows[1].2), (1107, Some(1117)));
        let maximum_base = u64::MAX - 100 - cadmpeg_core::decode::u64_from_index(bytes.len());
        assert!(crate::test_support::with_decode_context(|ctx| field
            .clone()
            .relocate(ctx, maximum_base))
        .unwrap()
        .is_some());
        assert!(crate::test_support::with_decode_context(|ctx| field
            .clone()
            .relocate(ctx, maximum_base + 1))
        .unwrap()
        .is_none());
        let no_witness = extrude_profile_references_test(
            OperationPayload::new(&bytes[..13], 100, "EXTRUDE").unwrap(),
        )
        .unwrap();
        assert!(no_witness.references().all(|row| row.2.is_none()));
        assert!(crate::test_support::with_decode_context(|ctx| no_witness
            .clone()
            .relocate(ctx, u64::MAX - 113))
        .unwrap()
        .is_some());
        assert!(crate::test_support::with_decode_context(|ctx| no_witness
            .clone()
            .relocate(ctx, u64::MAX - 112))
        .unwrap()
        .is_none());
    }
}
