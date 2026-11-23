// SPDX-License-Identifier: Apache-2.0
//! Validation of body recipe references, component instances, and assembly operands.

use super::{design_stream, push_constant_finding, Ctx};
use crate::layout::{
    assembly_class_307_264_joint_origin_scope as class_307_joint_origin,
    assembly_class_363_264_frame_363_carrier as class_363_carrier,
    assembly_class_363_264_frame_388_identity as class_363_identity,
    assembly_operand_path_locator as path_locator,
    assembly_variable_reference_operand_path_locator as variable_path_locator,
    combine_operand_list_operation_prefix as combine_lists,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::{design, native, records};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::report::check::{Check, Finding};
use std::collections::{HashMap, HashSet};

pub(super) fn body_recipe_reference_table_is_admitted(
    scope: Option<&DesignParameterScope>,
    operand: &records::topology::body_recipe::DesignBodyRecipeOperand,
) -> bool {
    if operand.has_legacy_cut_frame() {
        return operand.owner.group().is_some_and(|(group, _)| {
            scope.and_then(|scope| {
                scope.legacy_class_330_body_recipe_close(group, operand.record_index())
            }) == Some(operand.next_record_index())
        });
    }
    !operand.references().is_empty()
        || matches!(
            operand.owner,
            records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
        ) && scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Combine
                && scope.combine_operation().is_some_and(|operation| {
                    operation
                        .tools
                        .iter()
                        .any(|tool| tool.record_index == operand.record_index())
                })
        })
}

/// An operand-list `Combine` stores its operation after the presence byte, or
/// after an empty property block, and its keep-tools Boolean after the
/// companion list. A `Combine` with a companion consumes its tools.
pub(super) fn operand_list_combine_offsets_hold(
    scope_offset: u64,
    operation: &records::feature::combine::DesignCombineOperation,
) -> bool {
    const EMPTY_PROPERTY_BLOCK_LEN: u64 = 4;
    const COMPANION_REFERENCE_LEN: u64 = 11;
    const TYPED_COMPANION_REFERENCE_LEN: u64 = 51;
    let operation_at = u64_from_index(combine_lists::OPERATION);
    let keep_tools_after_operation =
        u64_from_index(combine_lists::KEEP_TOOLS - combine_lists::OPERATION);
    let property_block = operation
        .operation_offset
        .checked_sub(scope_offset)
        .and_then(|relative| relative.checked_sub(operation_at));
    let companion_bytes = operation
        .keep_tools_offset
        .checked_sub(operation.operation_offset)
        .and_then(|distance| distance.checked_sub(keep_tools_after_operation));
    matches!(property_block, Some(0 | EMPTY_PROPERTY_BLOCK_LEN))
        && companion_bytes.is_some_and(|bytes| {
            (bytes == 0 || !operation.keep_tools)
                && (0..COMPANION_REFERENCE_LEN).any(|typed| {
                    bytes
                        .checked_sub(typed * TYPED_COMPANION_REFERENCE_LEN)
                        .is_some_and(|remaining| remaining.is_multiple_of(COMPANION_REFERENCE_LEN))
                })
        })
}

/// A rigid `As-built` alignment has no parameter owners, so its angle and
/// offset are zero.
pub(super) fn rigid_alignment_scalars_hold(
    alignment: &records::feature::assembly::DesignAssemblyAlignment,
    rigid_as_built: bool,
) -> bool {
    !rigid_as_built
        || !alignment.owners.is_empty()
        || (alignment.angle() == 0.0 && alignment.offset() == [0.0; 3])
}

pub(super) fn valid_component_insert_role(role: &str) -> bool {
    role.encode_utf16().count() <= 256
        && (crate::bytes::is_guid_relaxed(role)
            || role.starts_with("urn:")
            || (crate::bytes::is_guid_prefix(role)
                && role.as_bytes().get(36) == Some(&b'_')
                && role
                    .get(37..)
                    .is_some_and(|suffix| suffix.starts_with("urn:"))))
}

/// A rigid group whose operands were read keeps one member after its first
/// two for each locator its frame stores, each taking the locator reference
/// that follows the previous one, with the frame its locator carries. A
/// group whose operands were not read, and every other scope, has no further
/// members.
pub(super) fn rigid_group_members_link(
    scope: &records::feature::scope::DesignParameterScope,
    alignment: &records::feature::assembly::DesignAssemblyAlignment,
    rigid_as_built: bool,
    stream_types: &std::collections::HashMap<u64, (&str, u32)>,
) -> bool {
    if rigid_as_built && alignment.form.is_some() {
        let Some(layout) = design::assembly::RigidGroupLayout::for_scope(scope) else {
            return false;
        };
        Some(layout.members) == alignment.additional_members.len().checked_add(2)
            && alignment.additional_members.iter().enumerate().all(|(ordinal, member)| {
                let records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path } = &member.qualifier else {
                    return false;
                };
                let Some(locator_offset) = ordinal.checked_add(2)
                    .and_then(|member| member.checked_mul(layout.stride))
                    .and_then(|offset| offset.checked_add(layout.first_reference)) else {
                    return false;
                };
                valid_assembly_operand_path_link(scope, path, locator_offset, stream_types)
                    && as_built_operand_frame_link(scope, path, &member.frame, stream_types)
            })
    } else {
        alignment.additional_members.is_empty()
    }
}

pub(super) fn as_built_operand_frame_link(
    scope: &records::feature::scope::DesignParameterScope,
    path: &records::feature::assembly::DesignAssemblyOperandPath,
    frame: &records::feature::assembly::DesignAssemblyOperandFrame,
    stream_types: &std::collections::HashMap<u64, (&str, u32)>,
) -> bool {
    use crate::layout::assembly_wide_operand_path_locator as wide_locator;
    let locator = path.link().locator_byte_offset;
    let identity = frame.transform.rows() == cadmpeg_ir::transform::Transform::identity().rows();
    let length = path
        .byte_offset()
        .checked_sub(locator)
        .and_then(|length| usize::try_from(length).ok());
    if design::assembly::rigid_as_built_scope(scope)
        || design::assembly::compact_placement_as_built_scope(scope)
    {
        let Some(layout) = as_built_locator_layout(scope, path, stream_types) else {
            return false;
        };
        let reference = if layout.null_construction {
            crate::layout::indexed_design_record_header::RECORD_INDEX
        } else {
            layout.construction_reference + 1
        };
        let transform = layout.transform.unwrap_or(layout.identity_flag);
        return locator.checked_add(u64_from_index(reference)) == Some(frame.reference_offset)
            && locator.checked_add(u64_from_index(transform)) == Some(frame.transform_offset)
            && (layout.transform.is_some() || identity)
            && frame.reference_record_index != 0
            && (!layout.null_construction
                || frame.reference_record_index == path.link().locator_record_index);
    }
    let (reference, flag, matrix) = if design::assembly::wide_as_built_scope(scope) {
        if !matches!(
            length,
            Some(
                crate::layout::assembly_wide_operand_path_identity_locator::LEN
                    | crate::layout::assembly_wide_operand_path_matrix_locator::LEN
            )
        ) {
            return false;
        }
        (
            wide_locator::NONZERO_RECORD_REFERENCE + 1,
            Some(wide_locator::IDENTITY_FLAG),
            wide_locator::TRANSFORM,
        )
    } else {
        (
            path_locator::NONZERO_RECORD_REFERENCE + 1,
            None,
            path_locator::TRANSFORM,
        )
    };
    locator.checked_add(u64_from_index(reference)) == Some(frame.reference_offset)
        && (locator.checked_add(u64_from_index(matrix)) == Some(frame.transform_offset)
            || identity
                && flag.is_some_and(|flag| {
                    locator.checked_add(u64_from_index(flag)) == Some(frame.transform_offset)
                }))
        && frame.reference_record_index != 0
}

pub(super) fn as_built_locator_layout(
    scope: &records::feature::scope::DesignParameterScope,
    path: &records::feature::assembly::DesignAssemblyOperandPath,
    stream_types: &std::collections::HashMap<u64, (&str, u32)>,
) -> Option<design::assembly::RigidGroupLocatorLayout> {
    let link = path.link();
    let registered = stream_types
        .get(&u64::from(link.locator_record_index))
        .copied();
    let length = usize::try_from(path.byte_offset().checked_sub(link.locator_byte_offset)?).ok()?;
    if design::assembly::compact_placement_as_built_scope(scope) {
        design::assembly::RigidGroupLocatorLayout::admit_compact_as_built(registered, length)
    } else {
        design::assembly::RigidGroupLocatorLayout::admit(registered, length)
    }
}

pub(super) fn valid_assembly_operand_path_link(
    scope: &records::feature::scope::DesignParameterScope,
    path: &records::feature::assembly::DesignAssemblyOperandPath,
    locator_marker_offset: usize,
    stream_types: &std::collections::HashMap<u64, (&str, u32)>,
) -> bool {
    let link = &path.link();
    let Ok(locator_marker_offset) = u64::try_from(locator_marker_offset) else {
        return false;
    };
    let Some(locator_reference_offset) = scope
        .byte_offset()
        .checked_add(locator_marker_offset)
        .and_then(|offset| offset.checked_add(1))
    else {
        return false;
    };
    let compact_as_built = design::assembly::compact_placement_as_built_scope(scope);
    let variable_reference = !compact_as_built
        && !design::assembly::rigid_as_built_scope(scope)
        && design::assembly::variable_reference_assembly_generation(
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        );
    let wide = design::assembly::AssemblyScopeGeneration::for_scope(scope)
        .operand_frame_variant()
        .is_some_and(design::assembly::AssemblyOperandFrameVariant::is_wide_family)
        || design::assembly::wide_as_built_scope(scope);
    let (locator_length, scope_backlink, wrapper_reference) =
        if compact_as_built || design::assembly::rigid_as_built_scope(scope) {
            let Some(layout) = as_built_locator_layout(scope, path, stream_types) else {
                return false;
            };
            if compact_as_built && !path.admitted_by_type() {
                return false;
            }
            let (length, backlink, wrapper, _, _) = layout.offsets();
            (
                u64_from_index(length),
                u64_from_index(backlink + 1),
                u64_from_index(wrapper + 1),
            )
        } else if wide {
            use crate::layout::assembly_wide_operand_path_identity_locator as identity;
            use crate::layout::assembly_wide_operand_path_matrix_locator as matrix;
            let (length, backlink, wrapper) = match path
                .byte_offset()
                .checked_sub(link.locator_byte_offset)
                .and_then(|length| usize::try_from(length).ok())
            {
                Some(identity::LEN) => (
                    identity::LEN,
                    identity::SCOPE_BACKLINK,
                    identity::WRAPPER_REFERENCE,
                ),
                Some(matrix::LEN) => (
                    matrix::LEN,
                    matrix::SCOPE_BACKLINK,
                    matrix::WRAPPER_REFERENCE,
                ),
                _ => return false,
            };
            (
                u64_from_index(length),
                u64_from_index(backlink + 1),
                u64_from_index(wrapper + 1),
            )
        } else if variable_reference {
            (
                u64_from_index(variable_path_locator::LEN),
                u64_from_index(variable_path_locator::SCOPE_BACKLINK + 1),
                u64_from_index(variable_path_locator::WRAPPER_REFERENCE + 1),
            )
        } else {
            (
                u64_from_index(path_locator::LEN),
                u64_from_index(path_locator::SCOPE_BACKLINK + 1),
                u64_from_index(path_locator::WRAPPER_REFERENCE + 1),
            )
        };
    let Some(path_byte_offset) = link.locator_byte_offset.checked_add(locator_length) else {
        return false;
    };
    let Some(locator_scope_reference_offset) = link.locator_byte_offset.checked_add(scope_backlink)
    else {
        return false;
    };
    let Some(wrapper_reference_offset) = link.locator_byte_offset.checked_add(wrapper_reference)
    else {
        return false;
    };
    let Some(path_reference_offset) = link.wrapper_byte_offset.checked_add(27) else {
        return false;
    };
    link.locator_reference_offset == locator_reference_offset
        && link.locator_record_index.checked_add(1) == Some(path.record_index)
        // A variable-reference or wide locator's path can continue over
        // further records before its wrapper.
        && if variable_reference || wide || compact_as_built {
            link.locator_record_index
                .checked_add(2)
                .zip(link.locator_record_index.checked_add(65))
                .is_some_and(|(first, last)| (first..=last).contains(&link.wrapper_record_index))
        } else {
            link.locator_record_index.checked_add(2) == Some(link.wrapper_record_index)
        }
        && path.byte_offset() == path_byte_offset
        && link.locator_scope_reference_offset == locator_scope_reference_offset
        && link.wrapper_reference_offset == wrapper_reference_offset
        && link.wrapper_byte_offset > path.byte_offset()
        && link.path_reference_offset == path_reference_offset
}

pub(super) fn valid_class_363_operand_path_link(
    scope: &records::feature::scope::DesignParameterScope,
    frame: &records::feature::assembly::DesignAssemblyOperandFrame,
    path: &records::feature::assembly::DesignAssemblyOperandPath,
) -> bool {
    let link = &path.link();
    link.locator_class_tag.as_str() == "363"
        && link.wrapper_class_tag.as_str() == "388"
        && path.class_tag().as_str() == "386"
        && link.locator_record_index == frame.reference_record_index
        && link.locator_reference_offset == frame.reference_offset
        && link
            .locator_byte_offset
            .checked_add(u64_from_index(class_363_carrier::SCOPE_REFERENCE + 1))
            == Some(link.locator_scope_reference_offset)
        && link
            .wrapper_byte_offset
            .checked_add(u64_from_index(class_363_identity::OCCURRENCE_GUID + 4))
            == Some(link.path_reference_offset)
        && link.wrapper_reference_offset < link.wrapper_byte_offset
        && link.locator_scope_reference_offset > link.locator_byte_offset
        && link.locator_reference_offset >= scope.byte_offset()
        && link.locator_reference_offset < scope.paired_byte_offset()
        && path.byte_offset() < link.locator_byte_offset
        && path.occurrence_guids()[0].offset == link.path_reference_offset
        && path.identity_guids()[0].offset > path.occurrence_guids()[0].offset
}

pub(super) fn valid_class_307_joint_origin_qualifier(
    native: &native::F3dNative,
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    frame: &records::feature::assembly::DesignAssemblyOperandFrame,
    qualifier: &records::feature::assembly::DesignAssemblyOperandQualifier,
) -> bool {
    let records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin {
        scope_record_index,
        class_tag,
        byte_offset,
        paired_class_tag,
        paired_byte_offset,
    } = qualifier
    else {
        return false;
    };
    frame.reference_record_index == *scope_record_index
        && class_tag.as_str() == "307"
        && paired_class_tag.as_str() == "264"
        && byte_offset.checked_add(u64_from_index(class_307_joint_origin::LEN))
            == Some(*paired_byte_offset)
        && design_header_matches(
            records_by_index,
            stream,
            *scope_record_index,
            class_tag.as_str(),
            *byte_offset,
        )
        && native
            .design_parameter_scopes
            .iter()
            .filter(|target_scope| {
                design_stream(&target_scope.id) == stream
                    && target_scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::JointOrigin
                    && target_scope.record_index == *scope_record_index
                    && target_scope.class_tag == *class_tag
                    && target_scope.byte_offset() == *byte_offset
                    && target_scope.paired_class_tag == *paired_class_tag
                    && target_scope.paired_byte_offset() == *paired_byte_offset
                    && target_scope.frame_length() == u64_from_index(class_307_joint_origin::LEN)
                    && target_scope.joint_origin_transform() == Some(frame.transform)
            })
            .count()
            == 1
}

pub(super) fn design_header_matches(
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    record_index: u32,
    class_tag: &str,
    byte_offset: u64,
) -> bool {
    records_by_index
        .get(&(stream, record_index))
        .is_some_and(|header| {
            header.class_tag.as_str() == class_tag && header.byte_offset == byte_offset
        })
}

pub(super) fn valid_axial_selector_identity(
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    scope: &records::feature::scope::DesignParameterScope,
    selector: &records::feature::assembly::DesignAssemblyAxialSelectorIdentity,
    limit: u64,
) -> bool {
    let utf16_len = |value: &str| u64::try_from(value.encode_utf16().count()).ok();
    let utf16_end =
        |offset: u64, value: &str| utf16_len(value)?.checked_mul(2)?.checked_add(offset);
    let Some(selector_asset_end) = utf16_end(
        selector.selector_asset_id_offset,
        selector.selector_asset_id.as_str(),
    ) else {
        return false;
    };
    let Some(selector_context_end) = utf16_end(
        selector.selector_context_id_offset,
        selector.selector_context_id.as_str(),
    ) else {
        return false;
    };
    let Some(external_asset_end) = utf16_end(
        selector.external_asset_id_offset,
        selector.external_asset_id.as_str(),
    ) else {
        return false;
    };
    let Some(external_link_end) = utf16_end(
        selector.external_link_name_offset,
        &selector.external_link_name,
    ) else {
        return false;
    };
    let Some(external_link_len) = utf16_len(&selector.external_link_name) else {
        return false;
    };
    let external_end = match &selector.external_version {
        None => external_link_end.checked_add(1),
        Some(version) => {
            let property_key = version.property_key.value.as_str();
            let property_key_offset = version.property_key.offset;
            let version_urn = version.version_urn.value.as_str();
            let version_urn_offset = version.version_urn.offset;
            let version_len = utf16_len(version_urn);
            if external_link_end.checked_add(5) != Some(property_key_offset)
                || !version_len.is_some_and(|length| (1..=256).contains(&length))
                || utf16_end(property_key_offset, property_key).and_then(|end| end.checked_add(4))
                    != Some(version_urn_offset)
            {
                None
            } else {
                utf16_end(version_urn_offset, version_urn)
            }
        }
    };
    let Some(external_end) = external_end else {
        return false;
    };
    let Some(occurrence_role_end) = utf16_end(
        selector.occurrence_role_offset,
        selector.occurrence_role.as_str(),
    ) else {
        return false;
    };
    let selector_pair_is_referenced = scope
        .reference_members()
        .values()
        .zip(scope.reference_members().values().skip(1))
        .filter(|(first, second)| {
            [**first, **second] == [selector.axis_record_index, selector.selector_record_index]
        })
        .count()
        == 1;
    let selector_records_are_unique = [selector.axis_record_index, selector.selector_record_index]
        .iter()
        .all(|record_index| {
            scope
                .reference_members()
                .values()
                .filter(|member| *member == record_index)
                .count()
                == 1
        });

    design_header_matches(
        records_by_index,
        stream,
        selector.axis_record_index,
        selector.axis_class_tag.as_str(),
        selector.axis_byte_offset,
    ) && design_header_matches(
        records_by_index,
        stream,
        selector.selector_record_index,
        selector.selector_class_tag.as_str(),
        selector.selector_byte_offset,
    ) && selector.axis_record_index.checked_add(3) == Some(selector.selector_record_index)
        && selector.selector_record_index.checked_add(3) == Some(selector.nested_record_index)
        && selector.selector_record_index.checked_add(5) == Some(selector.role_record_index)
        && selector.axis_byte_offset < selector.axis_paired_byte_offset
        && selector.axis_paired_byte_offset < selector.selector_byte_offset
        && selector.selector_byte_offset < selector.selector_paired_byte_offset
        && external_end <= selector.selector_paired_byte_offset
        && selector.selector_paired_byte_offset < selector.role_byte_offset
        && occurrence_role_end <= limit
        && selector.selector_byte_offset.checked_add(23)
            == Some(selector.nested_record_index_offset)
        && selector.selector_byte_offset.checked_add(41) == Some(selector.selector_asset_id_offset)
        && selector_asset_end.checked_add(4) == Some(selector.selector_context_id_offset)
        && selector_context_end.checked_add(13) == Some(selector.occurrence_reference_offset)
        && selector.occurrence_reference_offset.checked_add(15)
            == Some(selector.external_object_reference_offset)
        && selector.external_object_reference_offset.checked_add(9)
            == Some(selector.external_segment_offset)
        && selector.external_segment_offset.checked_add(8)
            == Some(selector.external_asset_id_offset)
        && external_asset_end.checked_add(5) == Some(selector.external_link_name_offset)
        && selector.role_byte_offset.checked_add(29) == Some(selector.occurrence_role_offset)
        && selector
            .external_asset_id
            .as_str()
            .eq_ignore_ascii_case(selector.selector_asset_id.as_str())
        && selector.occurrence_reference != 0
        && selector.external_object_reference != 0
        && (1..=256).contains(&external_link_len)
        && selector_pair_is_referenced
        && selector_records_are_unique
}

pub(super) fn valid_axial_assembly_targets(
    native: &native::F3dNative,
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    scope: &records::feature::scope::DesignParameterScope,
    frames: &[records::feature::assembly::DesignAssemblyOperandFrame; 2],
    targets: &[&records::feature::assembly::DesignAssemblyAxialOperandTarget; 2],
) -> bool {
    targets
        .iter()
        .copied()
        .zip(frames)
        .all(|(target, frame)| match target {
            records::feature::assembly::DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                component_insert_scope_record_index,
                construction_record_index,
                construction_class_tag,
                construction_byte_offset,
                construction_transform_offset,
                axis_record_index_offsets,
                construction_paired_class_tag: _,
                construction_paired_byte_offset,
                selectors,
            } => {
                let selectors_ordered = selectors[0].axis_byte_offset
                    < selectors[0].selector_byte_offset
                    && selectors[0].selector_byte_offset < selectors[0].role_byte_offset
                    && selectors[0].role_byte_offset < selectors[1].axis_byte_offset
                    && selectors[1].axis_byte_offset < selectors[1].selector_byte_offset
                    && selectors[1].selector_byte_offset < selectors[1].role_byte_offset
                    && selectors[1].role_byte_offset < *construction_byte_offset;
                let component_scopes = native
                    .design_parameter_scopes
                    .iter()
                    .filter(|target_scope| {
                        design_stream(&target_scope.id) == stream
                            && target_scope.kind()
                                == crate::records::feature::scope::DesignFeatureKind::ComponentInsert
                            && target_scope.record_index == *component_insert_scope_record_index
                            && target_scope.component_insert_construction().is_some_and(
                                |construction| {
                                    construction
                                        .neutron_role
                                        .eq_ignore_ascii_case(selectors[0].occurrence_role.as_str())
                                },
                            )
                    })
                    .count();
                frame.reference_record_index == *construction_record_index
                    && scope
                        .reference_members()
                        .values()
                        .filter(|record_index| **record_index == *construction_record_index)
                        .count()
                        == 1
                    && *construction_byte_offset > scope.paired_byte_offset()
                    && construction_byte_offset.checked_add(48)
                        == Some(*construction_transform_offset)
                    && construction_byte_offset.checked_add(193)
                        == Some(axis_record_index_offsets[0])
                    && construction_byte_offset.checked_add(209)
                        == Some(axis_record_index_offsets[1])
                    && construction_byte_offset.checked_add(380)
                        == Some(*construction_paired_byte_offset)
                    && design_header_matches(
                        records_by_index,
                        stream,
                        *construction_record_index,
                        construction_class_tag.as_str(),
                        *construction_byte_offset,
                    )
                    && selectors_ordered
                    && valid_axial_selector_identity(
                        records_by_index,
                        stream,
                        scope,
                        &selectors[0],
                        selectors[1].axis_byte_offset,
                    )
                    && valid_axial_selector_identity(
                        records_by_index,
                        stream,
                        scope,
                        &selectors[1],
                        *construction_byte_offset,
                    )
                    && selectors[0].selects_same_object(&selectors[1])
                    && selectors[0]
                        .occurrence_role
                        .as_str()
                        .eq_ignore_ascii_case(selectors[1].occurrence_role.as_str())
                    && component_scopes == 1
            }
            records::feature::assembly::DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                scope_record_index,
            } => {
                frame.reference_record_index == *scope_record_index
                    && native
                        .design_parameter_scopes
                        .iter()
                        .filter(|target_scope| {
                            design_stream(&target_scope.id) == stream
                                && target_scope.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::JointOrigin
                                && target_scope.record_index == *scope_record_index
                                && target_scope.joint_origin_transform() == Some(frame.transform)
                        })
                        .count()
                        == 1
            }
        })
}

pub(super) fn validate_component_occurrences(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    struct LowerAscii<'a>(&'a str);
    impl std::fmt::Display for LowerAscii<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for character in self.0.chars() {
                write!(formatter, "{}", character.to_ascii_lowercase())?;
            }
            Ok(())
        }
    }
    let mut identities = HashSet::new();
    let mut record_indices = HashSet::new();
    for occurrence in &ctx.native.design_component_occurrences {
        let stream = design_stream(&occurrence.id);
        let key = format!("{}", LowerAscii(occurrence.occurrence_guid.as_str()));
        let unique_identity = identities.insert((stream, key));
        let unique_record = record_indices.insert((stream, occurrence.record_index));
        let placement_admitted = !occurrence.is_class_256_first_matrix();
        // The duplicated references must agree within one carrier, which
        // the decoder checks. The component-record reference names the
        // definition; carriers of one definition can store different
        // carrier GUIDs.
        for (holds, message) in [
            (
                unique_identity,
                "Fusion Design component occurrence GUID repeats in its Design stream",
            ),
            (
                unique_record,
                "Fusion Design component occurrence record index repeats in its Design stream",
            ),
            (
                placement_admitted,
                "Fusion Design class-256 component occurrence has a placed frame at ordinal one",
            ),
        ] {
            if !holds {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    message,
                    Some(occurrence.id.clone()),
                );
            }
        }
    }
}

pub(super) fn valid_component_pattern_occurrences(
    native: &native::F3dNative,
    stream: &str,
    instances: &records::feature::patterns::DesignRectangularPatternInstances,
) -> bool {
    let records::feature::patterns::DesignRectangularPatternInstances::Components {
        component_guid,
        seed,
        generated,
    } = instances
    else {
        return true;
    };
    native
        .design_component_occurrences
        .iter()
        .any(|occurrence| {
            design_stream(&occurrence.id) == stream
                && occurrence
                    .component_guid
                    .as_str()
                    .eq_ignore_ascii_case(component_guid.as_str())
                && occurrence
                    .occurrence_guid
                    .as_str()
                    .eq_ignore_ascii_case(seed.occurrence_guid.as_str())
                && matches!(
                    occurrence.placement(),
                    crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Base
                )
        })
        && generated.iter().enumerate().all(|(ordinal, row)| {
            native
                .design_component_occurrences
                .iter()
                .any(|occurrence| {
                    design_stream(&occurrence.id) == stream
                        && occurrence
                            .component_guid
                            .as_str()
                            .eq_ignore_ascii_case(component_guid.as_str())
                        && occurrence
                            .occurrence_guid
                            .as_str()
                            .eq_ignore_ascii_case(row.occurrence_guid.as_str())
                        && u32::try_from(ordinal).ok().and_then(|ordinal| ordinal.checked_add(2)) == Some(occurrence.occurrence_ordinal())
                        && occurrence.transform() == Some(row.instance.transform)
                })
        })
}

/// Every element of a joint operand `occurrence_path` equals the final
/// identity segment of exactly one child of the occurrence the path has
/// reached, starting at the container occurrence. A path that reaches an
/// occurrence with no child in the model continues inside the document of
/// that occurrence, which the model does not hold.
pub(super) fn validate_joint_operand_paths(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    use cadmpeg_ir::products::OperandContainer;
    let model = &ctx.ir.model;
    if model.assembly_joints.is_empty() {
        return;
    }
    let mut children =
        HashMap::<&cadmpeg_ir::ids::OccurrenceId, Vec<&cadmpeg_ir::ids::OccurrenceId>>::new();
    let parents = crate::tree::parent_ids(&model.product_definitions, &model.occurrences);
    for occurrence in &model.occurrences {
        if let Some(parent) = parents.get(&occurrence.id) {
            children.entry(parent).or_default().push(&occurrence.id);
        }
    }
    for joint in &model.assembly_joints {
        for connector in joint.connectors() {
            let OperandContainer::Occurrence {
                occurrence,
                occurrence_path,
            } = &connector.operand.container
            else {
                continue;
            };
            let mut reached = occurrence;
            for element in occurrence_path {
                let Some(candidates) = children.get(reached) else {
                    break;
                };
                let mut matches = candidates.iter().filter(|child| {
                    crate::tree::identity_segment(child).eq_ignore_ascii_case(element)
                });
                let (Some(child), None) = (matches.next(), matches.next()) else {
                    push_constant_finding(
                        findings,
                        Check::ReferentialIntegrity,
                        "Fusion joint operand occurrence path names no single child occurrence",
                        Some(joint.id.as_str().to_owned()),
                    );
                    break;
                };
                reached = child;
            }
        }
    }
}
