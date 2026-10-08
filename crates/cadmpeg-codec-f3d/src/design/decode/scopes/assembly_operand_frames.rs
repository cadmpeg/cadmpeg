// SPDX-License-Identifier: Apache-2.0
//! Exact assembly operand frames, including as-built frames.

use cadmpeg_core::decode::u64_from_index;

use super::legacy_operand_paths::exact_legacy_class_388_scope;
use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::design::assembly::{AssemblyOperandFrameVariant, AssemblyScopeGeneration};
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::layout::assembly_class_383_258_scope_1011 as class_383_scope;
use crate::layout::assembly_class_388_266_scope_968 as class_388_assemble;
use crate::layout::assembly_class_406_261_scope_671 as class_406_assemble;
use crate::layout::assembly_operand_path_locator as path_locator;
use crate::records::feature::assembly::DesignAssemblyOperandFrame;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::scope::DesignParameterScope;

/// The two operand frames an assembly scope embeds. A legacy class-388 scope
/// also passes its full scope check.
pub(super) fn exact_assembly_operand_frames(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<[DesignAssemblyOperandFrame; 2]> {
    let frame_variant = AssemblyScopeGeneration::new(
        scope.frame_length(),
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    )
    .operand_frame_variant()?;
    let frames = operand_frames_at(bytes, scope, frame_variant)?;
    if frame_variant == AssemblyOperandFrameVariant::LegacyClass388
        && exact_legacy_class_388_scope(bytes, scope).is_none()
    {
        return None;
    }
    Some(frames)
}

/// The fixed-layout operand frames of `scope` under `frame_variant`.
fn operand_frames_at(
    bytes: &[u8],
    scope: &DesignParameterScope,
    frame_variant: AssemblyOperandFrameVariant,
) -> Option<[DesignAssemblyOperandFrame; 2]> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let class_tags = (scope.class_tag.as_str(), scope.paired_class_tag.as_str());
    let frame_offsets = match frame_variant {
        AssemblyOperandFrameVariant::LegacyClass388 => (
            class_388_assemble::FIRST_OPERAND_REFERENCE,
            class_388_assemble::FIRST_OPERAND_TRANSFORM,
            class_388_assemble::SECOND_OPERAND_REFERENCE,
            class_388_assemble::SECOND_OPERAND_TRANSFORM,
        ),
        AssemblyOperandFrameVariant::Standard if class_tags == ("383", "258") => (
            class_383_scope::FIRST_OPERAND_REFERENCE,
            class_383_scope::FIRST_OPERAND_TRANSFORM,
            class_383_scope::SECOND_OPERAND_REFERENCE,
            class_383_scope::SECOND_OPERAND_TRANSFORM,
        ),
        AssemblyOperandFrameVariant::Standard if class_tags == ("406", "261") => (
            class_406_assemble::FIRST_OPERAND_REFERENCE,
            class_406_assemble::FIRST_OPERAND_TRANSFORM,
            class_406_assemble::SECOND_OPERAND_REFERENCE,
            class_406_assemble::SECOND_OPERAND_TRANSFORM,
        ),
        AssemblyOperandFrameVariant::Standard => (28, 40, 168, 180),
        AssemblyOperandFrameVariant::Compact => (24, 36, 164, 176),
        AssemblyOperandFrameVariant::Axial => (28, 39, 167, 178),
    };
    if usize::try_from(scope.paired_byte_offset()).ok()?
        != start.checked_add(usize::try_from(scope.frame_length()).ok()?)?
        || !zeros_at::<9>(bytes, start + 11)
    {
        return None;
    }
    let prologue_matches = match frame_variant {
        AssemblyOperandFrameVariant::LegacyClass388 => {
            zeros_at::<{ class_388_assemble::SCOPE_FLAGS - 11 }>(bytes, start + 11)
                && bytes_at::<6>(bytes, start + class_388_assemble::SCOPE_FLAGS)
                    == Some(&class_388_assemble::SCOPE_FLAGS_VALUE)
                && zeros_at::<{ class_388_assemble::FIRST_OPERAND_REFERENCE - 26 }>(
                    bytes,
                    start + 26,
                )
                && bytes.get(start + 39) == Some(&0)
                && zeros_at::<{ class_388_assemble::SECOND_OPERAND_TRANSFORM - (168 + 11) }>(
                    bytes,
                    start + 168 + 11,
                )
        }
        AssemblyOperandFrameVariant::Standard => {
            let standard_tail_marker_offset = if scope.frame_length()
                == u64_from_index(class_383_scope::LEN)
                && class_tags == ("383", "258")
            {
                class_383_scope::STANDARD_TAIL_MARKER
            } else {
                308
            };
            bytes_at::<5>(bytes, start + 20) == Some(&[1, 0, 0, 0, 0])
                && matches!(bytes.get(start + 25), Some(0 | 1))
                && zeros_at::<2>(bytes, start + 26)
                && zeros_at::<7>(bytes, start + 33)
                && zeros_at::<7>(bytes, start + 173)
                && (crate::design::assembly::variable_reference_assembly_generation(
                    class_tags.0,
                    class_tags.1,
                ) || zeros_at::<4>(bytes, start + standard_tail_marker_offset))
        }
        AssemblyOperandFrameVariant::Compact => {
            matches!(bytes_at::<4>(bytes, start + 20), Some([0, 0 | 1, 0, 0]))
                && zeros_at::<7>(bytes, start + 29)
                && zeros_at::<7>(bytes, start + 169)
                && zeros_at::<4>(bytes, start + 304)
        }
        AssemblyOperandFrameVariant::Axial => {
            bytes_at::<5>(bytes, start + 20) == Some(&[1, 0, 0, 0, 0])
                && matches!(bytes.get(start + 25), Some(0 | 1))
                && zeros_at::<2>(bytes, start + 26)
                && zeros_at::<6>(bytes, start + 33)
                && zeros_at::<6>(bytes, start + 172)
                && zeros_at::<4>(bytes, start + 306)
        }
    };
    if !prologue_matches {
        return None;
    }
    let frame = |reference_at: usize, transform_at: usize| {
        Some(DesignAssemblyOperandFrame {
            reference_record_index: marked_record_reference(bytes, reference_at)?,
            reference_offset: u64_from_index(reference_at + 1),
            transform: rigid_transform_at(bytes, transform_at)?,
            transform_offset: u64_from_index(transform_at),
        })
    };
    let first = frame(start + frame_offsets.0, start + frame_offsets.1)?;
    let second = frame(start + frame_offsets.2, start + frame_offsets.3)?;
    (first.reference_record_index != second.reference_record_index).then_some([first, second])
}

pub(super) fn exact_as_built_operand_frames(
    bytes: &[u8],
    paths: &[DesignAssemblyOperandPath; 2],
) -> Option<[DesignAssemblyOperandFrame; 2]> {
    let frames = paths.each_ref().map(|path| {
        let locator_at = usize::try_from(path.link().locator_byte_offset).ok()?;
        let reference_at = locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?;
        let transform_at = locator_at.checked_add(path_locator::TRANSFORM)?;
        Some(DesignAssemblyOperandFrame {
            reference_record_index: marked_record_reference(bytes, reference_at)?,
            reference_offset: u64::try_from(reference_at.checked_add(1)?).ok()?,
            transform: rigid_transform_at(bytes, transform_at)?,
            transform_offset: u64::try_from(transform_at).ok()?,
        })
    });
    let [Some(first), Some(second)] = frames else {
        return None;
    };
    (first.reference_record_index != second.reference_record_index).then_some([first, second])
}
