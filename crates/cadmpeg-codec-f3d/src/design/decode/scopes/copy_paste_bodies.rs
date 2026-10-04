// SPDX-License-Identifier: Apache-2.0
//! Exact copy-paste bodies operation scopes.

use super::shared_frames::marked_record_reference;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::feature::body_ops;
use crate::records::feature::body_ops::DesignCopyPasteBodiesOperation;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn exact_copy_paste_bodies_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignCopyPasteBodiesOperation>, CodecError> {
    if scope.kind() != scope::DesignFeatureKind::CopyPasteBodies
        || scope.reference_members().len() < 2
    {
        return Ok(None);
    }
    let body_count = scope.reference_members().len() - 1;
    let mut operands = ctx.vector_storage(body_count, "f3d CopyPasteBodies operands")?;
    let mut bodies = ctx.vector_storage(body_count, "f3d CopyPasteBodies bodies")?;
    let parsed = (|| -> Option<Result<DesignCopyPasteBodiesOperation, CodecError>> {
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let body_group_record_index = marked_record_reference(bytes, start + 29)?;
        let relation_record_index = marked_record_reference(bytes, start + 40)?;
        if *scope.reference_members().values().next()? != body_group_record_index {
            return None;
        }
        let search_at = usize::try_from(scope.paired_byte_offset())
            .ok()?
            .checked_add(1)?;
        let body_group_at = match records.first_at_or_after(ctx, search_at, body_group_record_index)
        {
            Ok(body_group_at) => body_group_at?,
            Err(error) => return Some(Err(error)),
        };
        let (body_group_class_tag, body_group_after_tag) =
            lp_ascii_filtered_view(bytes, body_group_at, 3..=3, u8::is_ascii_digit)?;
        let body_group_after_index = body_group_after_tag.checked_add(4)?;
        if bytes.get(body_group_after_index..body_group_after_index + 10)? != [0; 10] {
            return None;
        }
        let body_group_count_at = body_group_after_index.checked_add(10)?;
        let body_group_count =
            usize::try_from(View::u32_le_at(bytes, body_group_count_at)?).ok()?;
        if body_group_count != scope.reference_members().len().checked_sub(1)? {
            return None;
        }
        let mut body_group_cursor = body_group_count_at.checked_add(4)?;
        {
            let mut validate_body_reference = |expected: u32| -> Option<Result<(), CodecError>> {
                let actual = marked_record_reference(bytes, body_group_cursor)?;
                if actual != expected {
                    return None;
                }
                if let Err(error) = ctx.push_vec(
                    &mut operands,
                    crate::records::identity::Located {
                        value: actual,
                        offset: u64::try_from(body_group_cursor + 1).ok()?,
                    },
                    "f3d CopyPasteBodies operands",
                ) {
                    return Some(Err(error));
                }
                body_group_cursor = body_group_cursor.checked_add(11)?;
                Some(Ok(()))
            };
            let references = scope.reference_members();
            if let Some(values) = references.unlocated_values() {
                let admitted = match ctx
                    .admit_iter(values, "scan F3D CopyPasteBodies scope references")
                {
                    Ok(admitted) => admitted,
                    Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
                };
                for expected in admitted.skip(1) {
                    match validate_body_reference(*expected) {
                        Some(Ok(())) => {}
                        Some(Err(error)) => return Some(Err(error)),
                        None => return None,
                    }
                }
            } else if let Some(rows) = references.located_rows() {
                let admitted = match ctx
                    .admit_iter(rows, "scan F3D CopyPasteBodies located scope references")
                {
                    Ok(admitted) => admitted,
                    Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
                };
                for expected in admitted.skip(1).map(|row| &row.value) {
                    match validate_body_reference(*expected) {
                        Some(Ok(())) => {}
                        Some(Err(error)) => return Some(Err(error)),
                        None => return None,
                    }
                }
            }
        }
        let relation_at = match records.first_at_or_after(ctx, search_at, relation_record_index) {
            Ok(relation_at) => relation_at?,
            Err(error) => return Some(Err(error)),
        };
        let (relation_class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, relation_at, 3..=3, u8::is_ascii_digit)?;
        let after_index = after_tag.checked_add(4)?;
        if bytes.get(after_index..after_index + 8)? != [0; 8] {
            return None;
        }
        let count_at = after_index.checked_add(8)?;
        if bytes.get(count_at) != Some(&1) {
            return None;
        }
        let reference_count = usize::try_from(View::u32_le_at(bytes, count_at + 1)?).ok()?;
        if reference_count != body_count.checked_mul(2)? {
            return None;
        }
        let references_at = count_at.checked_add(5)?;
        let body_reference =
            |at: usize, trailing_zeros: usize| -> Result<Option<u32>, CodecError> {
                if bytes.get(at) != Some(&1) {
                    return Ok(None);
                }
                let Some(zeroes) = bytes.get(at + 5..at + 5 + trailing_zeros) else {
                    return Ok(None);
                };
                if !ctx
                    .admit_iter(zeroes, "validate F3D copied-body reference padding")?
                    .all(|byte| *byte == 0)
                {
                    return Ok(None);
                }
                Ok(View::u32_le_at(bytes, at + 1))
            };
        let operands = match ctx.admit_iter(&operands, "scan F3D CopyPasteBodies operands") {
            Ok(operands) => operands.copied(),
            Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
        };
        for (ordinal, operand) in operands.enumerate() {
            let source_at = references_at.checked_add(ordinal.checked_mul(30)?)?;
            let copied_at = source_at.checked_add(15)?;
            let source_record_index = match body_reference(source_at, 10) {
                Ok(Some(index)) => index,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
            let source_offset = u64::try_from(source_at + 1).ok()?;
            let copied_record_index =
                match body_reference(copied_at, if ordinal + 1 == body_count { 6 } else { 10 }) {
                    Ok(Some(index)) => index,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
            if let Err(error) = ctx.push_vec(
                &mut bodies,
                body_ops::DesignCopiedBody {
                    operand,
                    source: crate::records::identity::Located {
                        value: source_record_index,
                        offset: source_offset,
                    },
                    copied: crate::records::identity::Located {
                        value: copied_record_index,
                        offset: u64::try_from(copied_at + 1).ok()?,
                    },
                },
                "f3d CopyPasteBodies bodies",
            ) {
                return Some(Err(error));
            };
        }
        let body_group_class_tag =
            match crate::design::decode::text::class_tag_from_view(ctx, body_group_class_tag) {
                Ok(Some(class_tag)) => class_tag,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
        let relation_class_tag =
            match crate::design::decode::text::class_tag_from_view(ctx, relation_class_tag) {
                Ok(Some(class_tag)) => class_tag,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
        Some(DesignCopyPasteBodiesOperation::try_new_charged(
            ctx,
            bodies,
            body_ops::CopyPasteRecordLocation {
                record_index: body_group_record_index,
                class_tag: body_group_class_tag,
                byte_offset: u64::try_from(body_group_at).ok()?,
            },
            body_ops::CopyPasteRecordLocation {
                record_index: relation_record_index,
                class_tag: relation_class_tag,
                byte_offset: u64::try_from(relation_at).ok()?,
            },
        ))
    })();
    match parsed.transpose() {
        Err(CodecError::Malformed(_)) => Ok(None),
        result => result,
    }
}
