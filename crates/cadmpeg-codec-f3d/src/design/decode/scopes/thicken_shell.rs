// SPDX-License-Identifier: Apache-2.0
//! Exact legacy thicken and shell scope frames.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::{fixed_guid_end, fixed_utf16_ascii_eq};
use crate::layout::shell_class_369_261_scope_frame as shell_369_261;
use crate::layout::thicken_class_347_scope_frame as thicken_347;
use crate::records::feature::direct_face;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

/// Decode class-347/258 Thicken, whose group precedes its scalar. The class
/// pair and 291-byte frame are part of admission for this distinct grammar.
pub(super) fn exact_legacy_thicken_class_347(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignDirectFaceOperation>, CodecError> {
    if scope.class_tag.as_str() != "347"
        || scope.paired_class_tag.as_str() != "258"
        || scope.frame_length() != u64_from_index(thicken_347::LEN)
    {
        return Ok(None);
    }
    let (Some(references), Ok(start)) = (
        scope.reference_members().values_array::<3>(),
        usize::try_from(scope.byte_offset()),
    ) else {
        return Ok(None);
    };
    let [group_record_index, _, thickness_record_index] = references.map(|value| *value);
    if !zeros_at::<10>(bytes, start + thicken_347::ZERO_RUN_10)
        || View::u32_le_at(bytes, start + thicken_347::FEATURE_FORM) != Some(4)
        || View::u32_le_at(bytes, start + thicken_347::GROUP_FORM) != Some(1)
        || marked_record_reference(bytes, start + thicken_347::GROUP_REFERENCE)
            != Some(group_record_index)
        || bytes_at::<2>(bytes, start + thicken_347::SCALAR_PREFIX) != Some(&[1, 1])
        || View::u32_le_at(bytes, start + thicken_347::AUXILIARY_COUNT) != Some(1)
        || marked_record_reference(bytes, start + thicken_347::AUXILIARY_REFERENCE).is_none()
        || !zeros_at::<8>(bytes, start + thicken_347::ZERO_RUN_8)
        || View::u32_le_at(bytes, start + thicken_347::GUID_CODE_UNIT_COUNT) != Some(36)
        || !zeros_at::<3>(bytes, start + thicken_347::ZERO_RUN_3)
        || View::u32_le_at(bytes, start + thicken_347::REFERENCE_COUNT) != Some(3)
        || View::u32_le_at(bytes, start + thicken_347::KIND_CODE_UNIT_COUNT) != Some(7)
    {
        return Ok(None);
    }
    if fixed_guid_end(bytes, start + thicken_347::GUID_CODE_UNIT_COUNT)
        != Some(start + thicken_347::ZERO_RUN_3)
        || fixed_utf16_ascii_eq(bytes, start + thicken_347::KIND_CODE_UNIT_COUNT, "Thicken")
            != Some(start + thicken_347::FEATURE_ORDINAL)
    {
        return Ok(None);
    }
    let reference_entries = [
        thicken_347::GROUP_REFERENCE_ENTRY,
        thicken_347::MEMBER_REFERENCE_ENTRY,
        thicken_347::SCALAR_REFERENCE_ENTRY,
    ];
    if reference_entries
        .into_iter()
        .zip(references)
        .any(|(offset, expected)| marked_record_reference(bytes, start + offset) != Some(*expected))
        || marked_record_reference(bytes, start + thicken_347::SCALAR_REFERENCE)
            != Some(thickness_record_index)
    {
        return Ok(None);
    }
    let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, thickness_record_index)? else {
        return Ok(None);
    };
    Ok(
        (scalar.value.get() != 0.0).then_some(DesignDirectFaceOperation::Thicken(
            direct_face::DesignThickenOperation {
                signed_thickness: scalar.value,
                thickness_record_index,
                thickness_offset: scalar.value_offset,
            },
        )),
    )
}

pub(super) fn exact_shell_class_369_261(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignDirectFaceOperation>, CodecError> {
    if scope.class_tag.as_str() != "369"
        || scope.paired_class_tag.as_str() != "261"
        || scope.frame_length() != u64_from_index(shell_369_261::LEN)
    {
        return Ok(None);
    }
    let (Some(references), Ok(start)) = (
        scope.reference_members().values_array::<3>(),
        usize::try_from(scope.byte_offset()),
    ) else {
        return Ok(None);
    };
    let thickness_record_index = *references[0];
    if !zeros_at::<9>(bytes, start + shell_369_261::ZERO_RUN_9)
        || bytes.get(start + shell_369_261::FEATURE_FORM)
            != Some(&shell_369_261::FEATURE_FORM_VALUE)
        || !zeros_at::<3>(bytes, start + shell_369_261::ZERO_RUN_3)
        || bytes.get(start + shell_369_261::SCALAR_MARKER)
            != Some(&shell_369_261::SCALAR_MARKER_VALUE)
        || !zeros_at::<9>(bytes, start + shell_369_261::ZERO_RUN_9_AFTER_SCALAR)
        || bytes.get(start + shell_369_261::GROUP_FORM) != Some(&shell_369_261::GROUP_FORM_VALUE)
        || !zeros_at::<3>(bytes, start + 47)
        || !zeros_at::<3>(bytes, start + shell_369_261::ZERO_RUN_3_BEFORE_REFERENCES)
        || View::u32_le_at(bytes, start + shell_369_261::GUID_CODE_UNIT_COUNT)
            != Some(shell_369_261::GUID_CODE_UNIT_COUNT_VALUE)
        || View::u32_le_at(bytes, start + shell_369_261::REFERENCE_COUNT)
            != Some(shell_369_261::REFERENCE_COUNT_VALUE)
        || View::u32_le_at(bytes, start + shell_369_261::KIND_CODE_UNIT_COUNT)
            != Some(shell_369_261::KIND_CODE_UNIT_COUNT_VALUE)
    {
        return Ok(None);
    }
    let outward = match bytes.get(start + shell_369_261::OUTWARD) {
        Some(0) => false,
        Some(1) => true,
        _ => return Ok(None),
    };
    if fixed_utf16_ascii_eq(
        bytes,
        start + shell_369_261::GUID_CODE_UNIT_COUNT,
        "00000000-0000-0000-0000-000000000000",
    ) != Some(start + shell_369_261::ZERO_RUN_3_BEFORE_REFERENCES)
        || fixed_utf16_ascii_eq(bytes, start + shell_369_261::KIND_CODE_UNIT_COUNT, "Shell")
            != Some(start + shell_369_261::FEATURE_ORDINAL)
    {
        return Ok(None);
    }
    let reference_entries = [
        shell_369_261::SCALAR_REFERENCE,
        shell_369_261::GROUP_REFERENCE,
        shell_369_261::REFERENCE_ENTRY_2,
    ];
    if reference_entries
        .into_iter()
        .zip(references)
        .any(|(offset, expected)| marked_record_reference(bytes, start + offset) != Some(*expected))
    {
        return Ok(None);
    }
    let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, thickness_record_index)? else {
        return Ok(None);
    };
    let Some(thickness) = cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get()) else {
        return Ok(None);
    };
    Ok(Some(DesignDirectFaceOperation::Shell(
        direct_face::DesignShellOperation {
            thickness,
            thickness_record_index,
            thickness_offset: scalar.value_offset,
            outward,
            outward_offset: u64_from_index(start + shell_369_261::OUTWARD),
        },
    )))
}
