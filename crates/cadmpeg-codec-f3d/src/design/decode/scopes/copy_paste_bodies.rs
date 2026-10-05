// SPDX-License-Identifier: Apache-2.0
//! Exact copy-paste bodies operation scopes.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::marked_record_reference;
use crate::design::decode::byte_fields::zeros_at;
use crate::design::decode::reference_runs::admit_reference_values;
use crate::design::decode::sketch::{indexed_record_header_at, IndexedRecordOffsets};
use crate::design::decode::text::retain_class_tag;
use crate::records::feature::body_ops;
use crate::records::feature::body_ops::DesignCopyPasteBodiesOperation;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::identity::Located;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn exact_copy_paste_bodies_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignCopyPasteBodiesOperation>, CodecError> {
    let references = scope.reference_members();
    if !matches!(scope.payload(), DesignScopePayload::CopyPasteBodies(_)) || references.len() < 2 {
        return Ok(None);
    }
    let body_count = references.len() - 1;
    let Ok(start) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    let (Some(body_group_record_index), Some(relation_record_index)) = (
        marked_record_reference(bytes, start + 29),
        marked_record_reference(bytes, start + 40),
    ) else {
        return Ok(None);
    };
    if references.values().next() != Some(&body_group_record_index) {
        return Ok(None);
    }
    let Some(search_at) = usize::try_from(scope.paired_byte_offset())
        .ok()
        .and_then(|paired| paired.checked_add(1))
    else {
        return Ok(None);
    };
    let Some(body_group_at) = records.first_at_or_after(ctx, search_at, body_group_record_index)?
    else {
        return Ok(None);
    };
    let Some(relation_at) = records.first_at_or_after(ctx, search_at, relation_record_index)?
    else {
        return Ok(None);
    };
    let (Some(body_group), Some(relation)) = (
        indexed_record_header_at(bytes, body_group_at),
        indexed_record_header_at(bytes, relation_at),
    ) else {
        return Ok(None);
    };
    let Some(operands_at) = body_group_operands_at(bytes, body_group_at, body_count) else {
        return Ok(None);
    };
    let Some(relations_at) = relation_pairs_at(bytes, relation_at, body_count) else {
        return Ok(None);
    };
    let mut bodies = ctx.vector_storage(body_count, "f3d CopyPasteBodies bodies")?;
    // The body group lists every reference after the first, one eleven-byte
    // marked reference each; the relation pairs each with its source and copy.
    for (ordinal, operand) in
        admit_reference_values(ctx, references, "scan F3D CopyPasteBodies scope references")?
            .skip(1)
            .enumerate()
    {
        let Some(body) = copied_body(
            bytes,
            operands_at + ordinal * 11,
            relations_at + ordinal * 30,
            *operand,
            ordinal + 1 == body_count,
        ) else {
            return Ok(None);
        };
        ctx.push_vec(&mut bodies, body, "f3d CopyPasteBodies bodies")?;
    }
    let body_group_location = body_ops::CopyPasteRecordLocation {
        record_index: body_group_record_index,
        class_tag: retain_class_tag(ctx, *body_group.class_tag, "copy F3D class tag")?,
        byte_offset: u64_from_index(body_group_at),
    };
    let relation_location = body_ops::CopyPasteRecordLocation {
        record_index: relation_record_index,
        class_tag: retain_class_tag(ctx, *relation.class_tag, "copy F3D class tag")?,
        byte_offset: u64_from_index(relation_at),
    };
    match DesignCopyPasteBodiesOperation::try_new_charged(
        ctx,
        bodies,
        body_group_location,
        relation_location,
    ) {
        Ok(operation) => Ok(Some(operation)),
        Err(CodecError::Malformed(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The offset of the first operand reference of the body-group record at
/// `at`, whose count names `body_count` operands. The end of the operand run
/// is checked, so every operand offset below it is representable.
fn body_group_operands_at(bytes: &[u8], at: usize, body_count: usize) -> Option<usize> {
    let count_at = at.checked_add(21)?;
    if !zeros_at::<10>(bytes, at + 11)
        || usize::try_from(View::u32_le_at(bytes, count_at)?).ok()? != body_count
    {
        return None;
    }
    let operands_at = count_at.checked_add(4)?;
    operands_at.checked_add(body_count.checked_mul(11)?)?;
    Some(operands_at)
}

/// The offset of the first source/copy pair of the relation record at `at`,
/// whose count names two references per body. The end of the pair run is
/// checked, so every pair offset below it is representable.
fn relation_pairs_at(bytes: &[u8], at: usize, body_count: usize) -> Option<usize> {
    let count_at = at.checked_add(19)?;
    if !zeros_at::<8>(bytes, at + 11)
        || bytes.get(count_at) != Some(&1)
        || usize::try_from(View::u32_le_at(bytes, count_at + 1)?).ok()?
            != body_count.checked_mul(2)?
    {
        return None;
    }
    let pairs_at = count_at.checked_add(5)?;
    pairs_at.checked_add(body_count.checked_mul(30)?)?;
    Some(pairs_at)
}

/// One copied body: the operand reference at `operand_at`, which must name
/// `operand`, and the source and copy references of the pair at `pair_at`.
/// The last pair's copy carries a shorter padding.
fn copied_body(
    bytes: &[u8],
    operand_at: usize,
    pair_at: usize,
    operand: u32,
    last: bool,
) -> Option<body_ops::DesignCopiedBody> {
    if marked_record_reference(bytes, operand_at)? != operand {
        return None;
    }
    let source = padded_reference::<10>(bytes, pair_at)?;
    let copied_at = pair_at + 15;
    let copied = if last {
        padded_reference::<6>(bytes, copied_at)?
    } else {
        padded_reference::<10>(bytes, copied_at)?
    };
    Some(body_ops::DesignCopiedBody {
        operand: Located {
            value: operand,
            offset: u64_from_index(operand_at + 1),
        },
        source: Located {
            value: source,
            offset: u64_from_index(pair_at + 1),
        },
        copied: Located {
            value: copied,
            offset: u64_from_index(copied_at + 1),
        },
    })
}

/// The record index of the marked reference at `at` followed by `PADDING`
/// zero bytes.
fn padded_reference<const PADDING: usize>(bytes: &[u8], at: usize) -> Option<u32> {
    if bytes.get(at) != Some(&1) || !zeros_at::<PADDING>(bytes, at + 5) {
        return None;
    }
    View::u32_le_at(bytes, at + 1)
}
