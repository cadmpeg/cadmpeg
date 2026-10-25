// SPDX-License-Identifier: Apache-2.0
//! Source layout of the assembly-alignment scopes: the operand frames, the
//! solved frame, the qualifiers and the axial selectors that a scope capture
//! locates.

use cadmpeg_core::decode::u64_from_index;

use super::scopes::LocatedScopes;
use crate::design;
use crate::layout::assembly_class_307_264_joint_origin_scope as class_307_joint_origin;
use crate::records::feature::assembly::{
    DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyAxialOperandTarget,
    DesignAssemblyAxialSelectorIdentity, DesignAssemblyOperandQualifier,
};
use crate::records::feature::scope::{DesignParameterScope, LocatedScopeFrame};

/// Whether the assembly alignment of a located scope has the offsets its
/// frame layout places.
pub(super) fn alignment_layout_holds(
    scopes: &LocatedScopes<'_>,
    scope: &DesignParameterScope,
    frame: LocatedScopeFrame,
    alignment: &DesignAssemblyAlignment,
) -> bool {
    let generation = design::assembly::AssemblyScopeGeneration::of_scope(scope);
    let operand_frame_variant = generation.operand_frame_variant();
    let axial_frames = matches!(
        operand_frame_variant,
        Some(design::assembly::AssemblyOperandFrameVariant::Axial)
    );
    let compact_frames = matches!(
        operand_frame_variant,
        Some(design::assembly::AssemblyOperandFrameVariant::Compact)
    );
    let as_built_frames = scope.kind()
        == crate::records::feature::scope::DesignFeatureKind::AsBuilt
        && (scope.assembly_frame()
            == Some(crate::records::feature::scope::generation::DesignAssemblyFrame::Frame399)
            || design::assembly::scope_is_wide_as_built(scope));
    let wide_family_offsets =
        operand_frame_variant.and_then(crate::design::decode::scopes::wide_family_frame_offsets);
    let frame_reference_offsets = if let Some([first, _, second, _]) = wide_family_offsets {
        [u64_from_index(first + 1), u64_from_index(second + 1)]
    } else if axial_frames {
        [29, 168]
    } else if compact_frames {
        [25, 165]
    } else {
        [29, 169]
    };
    let frame_transform_offsets = if let Some([_, first, _, second]) = wide_family_offsets {
        [u64_from_index(first), u64_from_index(second)]
    } else if axial_frames {
        [39, 178]
    } else if compact_frames {
        [36, 176]
    } else {
        [40, 180]
    };
    let operand_frames =
        if let Some(DesignAssemblyAlignmentForm::LegacyAsBuilt421 { carriers, .. }) =
            &alignment.form
        {
            carriers
                .references()
                .into_iter()
                .enumerate()
                .all(|(ordinal, reference)| {
                    reference.offset.is_some()
                        && scope
                            .reference_members()
                            .offsets()
                            .nth(ordinal * 2)
                            .copied()
                            == reference.offset
                })
        } else {
            // An `As-built` frame places its operands by its source parser.
            as_built_frames
                || alignment.operand_frames().is_none_or(|frames| {
                    frames.iter().enumerate().all(|(ordinal, operand)| {
                        operand.reference_offset
                            == frame
                                .byte_offset()
                                .checked_add(frame_reference_offsets[ordinal])
                            && operand.transform_offset
                                == frame
                                    .byte_offset()
                                    .checked_add(frame_transform_offsets[ordinal])
                    })
                })
        };
    let solved_frame = alignment.solved_frame().is_none_or(|solved| {
        let Some(generation) = design::assembly::scope_legacy_as_built_421_generation(scope) else {
            return false;
        };
        solved.reference_offset.is_some()
            && scope.reference_members().offsets().nth(8).copied() == solved.reference_offset
            && solved.transform_offset
                == solved.record_byte_offset.and_then(|offset| {
                    offset.checked_add(u64_from_index(generation.matrix_offset()))
                })
            && solved.transform_offset.is_some()
    });
    let joint_origin_envelope =
        alignment
            .joint_origin_scope_record_index()
            .is_none_or(|record_index| {
                scopes.joint_origins(scope, record_index).any(|target| {
                    target.joint_origin_transform_offset() == frame.byte_offset().checked_add(36)
                })
            });
    let qualifiers = match alignment.form.as_ref() {
        Some(DesignAssemblyAlignmentForm::Qualified(operands)) => {
            operands.iter().all(|operand| match &operand.qualifier {
                DesignAssemblyOperandQualifier::OccurrencePath { .. } => true,
                DesignAssemblyOperandQualifier::JointOrigin {
                    scope_record_index,
                    byte_offset,
                    paired_byte_offset,
                    ..
                } => joint_origin_qualifier_layout_holds(
                    scopes,
                    scope,
                    *scope_record_index,
                    *byte_offset,
                    *paired_byte_offset,
                ),
                DesignAssemblyOperandQualifier::AxialTarget { target } => {
                    axial_target_layout_holds(frame, target)
                }
            })
        }
        _ => true,
    };
    operand_frames && solved_frame && joint_origin_envelope && qualifiers
}

/// A class-307 `JointOrigin` qualifier locates the 366-byte frame of the
/// scope it names.
fn joint_origin_qualifier_layout_holds(
    scopes: &LocatedScopes<'_>,
    scope: &DesignParameterScope,
    record_index: u32,
    byte_offset: Option<u64>,
    paired_byte_offset: Option<u64>,
) -> bool {
    let Some(byte_offset) = byte_offset else {
        return false;
    };
    byte_offset.checked_add(u64_from_index(class_307_joint_origin::LEN)) == paired_byte_offset
        && scopes
            .joint_origins(scope, record_index)
            .filter(|target| {
                target.byte_offset() == Some(byte_offset)
                    && target.paired_byte_offset() == paired_byte_offset
            })
            .count()
            == 1
}

/// An axial operand target in a `Component Insert` occurrence follows the
/// scope frame, with its selectors in source order before its carrier.
fn axial_target_layout_holds(
    frame: LocatedScopeFrame,
    target: &DesignAssemblyAxialOperandTarget,
) -> bool {
    let DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
        construction_byte_offset,
        construction_transform_offset,
        axis_record_index_offsets,
        construction_paired_byte_offset,
        selectors,
        ..
    } = target
    else {
        return true;
    };
    let (Some(construction_byte_offset), Some(axis_record_index_offsets)) =
        (*construction_byte_offset, *axis_record_index_offsets)
    else {
        return false;
    };
    let (
        Some(first_axis),
        Some(first_selector),
        Some(first_role),
        Some(second_axis),
        Some(second_selector),
        Some(second_role),
    ) = (
        selectors[0].axis_byte_offset,
        selectors[0].selector_byte_offset,
        selectors[0].role_byte_offset,
        selectors[1].axis_byte_offset,
        selectors[1].selector_byte_offset,
        selectors[1].role_byte_offset,
    )
    else {
        return false;
    };
    first_axis < first_selector
        && first_selector < first_role
        && first_role < second_axis
        && second_axis < second_selector
        && second_selector < second_role
        && second_role < construction_byte_offset
        && construction_byte_offset > frame.paired_byte_offset()
        && construction_byte_offset.checked_add(48) == *construction_transform_offset
        && construction_byte_offset.checked_add(193) == Some(axis_record_index_offsets[0])
        && construction_byte_offset.checked_add(209) == Some(axis_record_index_offsets[1])
        && construction_byte_offset.checked_add(380) == *construction_paired_byte_offset
        && axial_selector_layout_holds(&selectors[0], second_axis)
        && axial_selector_layout_holds(&selectors[1], construction_byte_offset)
}

/// The fields of one axial selector are in their fixed order, each string
/// at the end of the one before it, with the selector ending before `limit`.
fn axial_selector_layout_holds(selector: &DesignAssemblyAxialSelectorIdentity, limit: u64) -> bool {
    let utf16_len = |value: &str| u64::try_from(value.encode_utf16().count()).ok();
    let utf16_end =
        |offset: u64, value: &str| utf16_len(value)?.checked_mul(2)?.checked_add(offset);
    let (
        Some(axis_byte_offset),
        Some(axis_paired_byte_offset),
        Some(selector_byte_offset),
        Some(selector_paired_byte_offset),
        Some(role_byte_offset),
        Some(selector_asset_id_offset),
        Some(selector_context_id_offset),
        Some(occurrence_reference_offset),
    ) = (
        selector.axis_byte_offset,
        selector.axis_paired_byte_offset,
        selector.selector_byte_offset,
        selector.selector_paired_byte_offset,
        selector.role_byte_offset,
        selector.selector_asset_id_offset,
        selector.selector_context_id_offset,
        selector.occurrence_reference_offset,
    )
    else {
        return false;
    };
    let (
        Some(external_object_reference_offset),
        Some(external_segment_offset),
        Some(external_asset_id_offset),
        Some(external_link_name_offset),
        Some(occurrence_role_offset),
        Some(nested_record_index_offset),
    ) = (
        selector.external_object_reference_offset,
        selector.external_segment_offset,
        selector.external_asset_id_offset,
        selector.external_link_name_offset,
        selector.occurrence_role_offset,
        selector.nested_record_index_offset,
    )
    else {
        return false;
    };
    let Some(selector_asset_end) = utf16_end(
        selector_asset_id_offset,
        selector.selector_asset_id.as_str(),
    ) else {
        return false;
    };
    let Some(selector_context_end) = utf16_end(
        selector_context_id_offset,
        selector.selector_context_id.as_str(),
    ) else {
        return false;
    };
    let Some(external_asset_end) = utf16_end(
        external_asset_id_offset,
        selector.external_asset_id.as_str(),
    ) else {
        return false;
    };
    let Some(external_link_end) =
        utf16_end(external_link_name_offset, &selector.external_link_name)
    else {
        return false;
    };
    let external_end = match &selector.external_version {
        None => external_link_end.checked_add(1),
        Some(version) => {
            let (Some(property_key_offset), Some(version_urn_offset)) =
                (version.property_key.offset, version.version_urn.offset)
            else {
                return false;
            };
            if external_link_end.checked_add(5) != Some(property_key_offset)
                || utf16_end(property_key_offset, version.property_key.value.as_str())
                    .and_then(|end| end.checked_add(4))
                    != Some(version_urn_offset)
            {
                None
            } else {
                utf16_end(version_urn_offset, version.version_urn.value.as_str())
            }
        }
    };
    let Some(external_end) = external_end else {
        return false;
    };
    let Some(occurrence_role_end) =
        utf16_end(occurrence_role_offset, selector.occurrence_role.as_str())
    else {
        return false;
    };
    axis_byte_offset < axis_paired_byte_offset
        && axis_paired_byte_offset < selector_byte_offset
        && selector_byte_offset < selector_paired_byte_offset
        && external_end <= selector_paired_byte_offset
        && selector_paired_byte_offset < role_byte_offset
        && occurrence_role_end <= limit
        && selector_byte_offset.checked_add(23) == Some(nested_record_index_offset)
        && selector_byte_offset.checked_add(41) == Some(selector_asset_id_offset)
        && selector_asset_end.checked_add(4) == Some(selector_context_id_offset)
        && selector_context_end.checked_add(13) == Some(occurrence_reference_offset)
        && occurrence_reference_offset.checked_add(15) == Some(external_object_reference_offset)
        && external_object_reference_offset.checked_add(9) == Some(external_segment_offset)
        && external_segment_offset.checked_add(8) == Some(external_asset_id_offset)
        && external_asset_end.checked_add(5) == Some(external_link_name_offset)
        && role_byte_offset.checked_add(29) == Some(occurrence_role_offset)
}
