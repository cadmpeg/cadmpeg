// SPDX-License-Identifier: Apache-2.0
//! Feature parameter scopes and their operand frames.

use super::{
    design_stream, folded_guid, group_records, reference_member_at, valid_vertex_recipe, Ctx,
    RecordGroups,
};
use crate::design::decode::scopes::extrude::is_class_296_legacy_one_sided_distance_layout;
use crate::design::decode::scopes::extrude::is_class_296_legacy_one_sided_to_face_layout;
use crate::design::decode::scopes::extrude::is_class_296_one_sided_to_face_layout;
use crate::design::decode::scopes::extrude::is_class_296_symmetric_distance_layout;
use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_layout;
use crate::design::decode::scopes::legacy_class_397::Class397SymmetricFrame;
use crate::design::decode::scopes::legacy_class_415;
use crate::layout::assembly_class_307_264_joint_origin_scope as class_307_joint_origin;
use crate::layout::assembly_class_363_264_frame_363_carrier as class_363_carrier;
use crate::layout::assembly_class_363_264_frame_388_identity as class_363_identity;
use crate::layout::assembly_operand_path_locator as path_locator;
use crate::layout::assembly_operand_path_wrapper as path_wrapper;
use crate::layout::assembly_variable_reference_operand_path_locator as variable_path_locator;
use crate::layout::class_296_261_legacy_extrude_prefix_scalar_at_54 as class_296_legacy_prefix;
use crate::layout::class_296_261_legacy_one_sided_distance_tail as class_296_legacy_distance;
use crate::layout::class_296_261_legacy_one_sided_to_face_tail as class_296_legacy_to_face;
use crate::layout::class_296_261_one_sided_to_face_extrude_prefix as class_296_to_face;
use crate::layout::class_296_261_symmetric_distance_extrude_prefix as class_296_symmetric;
use crate::layout::class_296_261_two_sided_to_faces_extrude_prefix as class_296_two_faces;
use crate::layout::legacy_class_397_symmetric_extrude_frame as class_397;
use crate::layout::legacy_class_415_symmetric_extrude_prefix as class_415;
use crate::layout::sketch_profile_region_selection_prefix as region_selection;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::{design, records};
use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::check::{Check, Finding};
use std::collections::{HashMap, HashSet};

const EPS_PATTERN_ROTATION: f64 = 1.0e-10;
const EPS_PATTERN_DISTANCE: f64 = 1.0e-8;

fn valid_assembly_operand_path_link(
    scope: &records::feature::scope::DesignParameterScope,
    path: &records::feature::assembly::DesignAssemblyOperandPath,
    locator_marker_offset: usize,
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
    let variable_reference = design::assembly::variable_reference_assembly_generation(
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    );
    let locator_length = if variable_reference {
        variable_path_locator::LEN
    } else {
        path_locator::LEN
    };
    let Ok(locator_length) = u64::try_from(locator_length) else {
        return false;
    };
    let Some(path_byte_offset) = link.locator_byte_offset.checked_add(locator_length) else {
        return false;
    };
    let scope_backlink = if variable_reference {
        variable_path_locator::SCOPE_BACKLINK + 1
    } else {
        path_locator::SCOPE_BACKLINK + 1
    };
    let Some(locator_scope_reference_offset) = link
        .locator_byte_offset
        .checked_add(u64_from_index(scope_backlink))
    else {
        return false;
    };
    let wrapper_reference = if variable_reference {
        variable_path_locator::WRAPPER_REFERENCE + 1
    } else {
        path_locator::WRAPPER_REFERENCE + 1
    };
    let Some(wrapper_reference_offset) = link
        .locator_byte_offset
        .checked_add(u64_from_index(wrapper_reference))
    else {
        return false;
    };
    let Some(path_reference_offset) = link.wrapper_byte_offset.checked_add(27) else {
        return false;
    };
    link.locator_reference_offset == locator_reference_offset
        && link.locator_record_index.checked_add(1) == Some(path.record_index)
        && if variable_reference {
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

fn valid_class_363_operand_path_link(
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

fn valid_class_307_joint_origin_qualifier(
    ctx: &Ctx<'_, '_>,
    stream: &str,
    frame: &records::feature::assembly::DesignAssemblyOperandFrame,
    qualifier: &records::feature::assembly::DesignAssemblyOperandQualifier,
) -> Result<bool, CodecError> {
    let records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin {
        scope_record_index,
        class_tag,
        byte_offset,
        paired_class_tag,
        paired_byte_offset,
    } = qualifier
    else {
        return Ok(false);
    };
    let decode = ctx.decode;
    Ok(frame.reference_record_index == *scope_record_index
        && (class_tag.as_str() == "307")
        && (paired_class_tag.as_str() == "264")
        && byte_offset.checked_add(u64_from_index(class_307_joint_origin::LEN))
            == Some(*paired_byte_offset)
        && design_header_matches(
            decode,
            &ctx.records_by_index,
            stream,
            *scope_record_index,
            class_tag.as_str(),
            *byte_offset,
        )?
        && decode.fold(
            ctx.scope_group(
                stream,
                *scope_record_index,
                "find F3D joint origin qualifier scopes",
            )?,
            0usize,
            |count, target_scope| {
                let matched = (target_scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::JointOrigin)
                    && (&target_scope.class_tag == class_tag)
                    && target_scope.byte_offset() == *byte_offset
                    && (&target_scope.paired_class_tag == paired_class_tag)
                    && target_scope.paired_byte_offset() == *paired_byte_offset
                    && target_scope.frame_length() == u64_from_index(class_307_joint_origin::LEN)
                    && target_scope.joint_origin_transform() == Some(frame.transform);
                count.checked_add(usize::from(matched)).ok_or_else(|| {
                    decode.refuse_codec_limit(
                        "count F3D joint origin qualifiers",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })
            },
            "scan F3D joint origin qualifier scopes",
        )? == 1)
}

fn valid_sketch_profile_region_selection(
    decode: &DecodeContext<'_>,
    profile: &records::topology::sketch_profile::DesignSketchProfileOperand,
    selection: &records::topology::sketch_profile::DesignSketchProfileRegionSelection,
) -> Result<bool, CodecError> {
    let Some(expected_region_count_offset) = selection
        .byte_offset
        .checked_add(u64_from_index(region_selection::REGION_COUNT))
    else {
        return Ok(false);
    };
    if profile.record_index.checked_add(3) != Some(selection.record_index)
        || selection.byte_offset <= profile.paired_byte_offset()
        || selection.region_count_offset != expected_region_count_offset
        || selection.regions.is_empty()
    {
        return Ok(false);
    }
    let Some(mut cursor) = selection
        .byte_offset
        .checked_add(u64_from_index(region_selection::LEN))
    else {
        return Ok(false);
    };
    let valid = decode.all_by(
        selection.regions.iter().enumerate(),
        |(region_ordinal, region)| {
            if region_ordinal != 0 {
                let Some(next) = cursor.checked_add(1) else {
                    return Ok(false);
                };
                cursor = next;
            }
            if region.member_count_offset != cursor || region.members.is_empty() {
                return Ok(false);
            }
            let Some(next) = cursor.checked_add(4) else {
                return Ok(false);
            };
            cursor = next;
            decode.all_by(&region.members, |member| {
            let (Some(curve_primary_id_offset), Some(incidence_words_offset), Some(next)) = (
                cursor.checked_add(4),
                cursor.checked_add(8),
                cursor.checked_add(40),
            ) else {
                return Ok(false);
            };
            if member.kind_offset != cursor
                || member.curve_primary_id_offset != curve_primary_id_offset
                || member.incidence_words_offset != incidence_words_offset
            {
                return Ok(false);
            }
            cursor = next;
            Ok(true)
        }, "scan F3D profile region members")
        },
        "scan F3D profile regions",
    )?;
    Ok(valid && cursor.checked_add(5) == Some(selection.companion_byte_offset))
}

fn design_header_matches(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    record_index: u32,
    class_tag: &str,
    byte_offset: u64,
) -> Result<bool, CodecError> {
    Ok(
        match decode.get_hash_map(
            records_by_index,
            &(stream, record_index),
            "find F3D assembly target header",
        )? {
            Some(header) => {
                (header.class_tag.as_str() == class_tag) && header.byte_offset == byte_offset
            }
            None => false,
        },
    )
}

fn valid_axial_selector_identity(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    scope: &records::feature::scope::DesignParameterScope,
    selector: &records::feature::assembly::DesignAssemblyAxialSelectorIdentity,
    limit: u64,
) -> Result<bool, CodecError> {
    let utf16_len = |value: &str| {
        Ok::<_, CodecError>(
            u64::try_from(
                decode
                    .admit_iter(value, "count F3D axial selector UTF-16 units")?
                    .encode_utf16()
                    .count(),
            )
            .ok(),
        )
    };
    let text_end = |offset: u64, units: u64| offset.checked_add(units.checked_mul(2)?);
    // Relaxed GUIDs contain at most 38 ASCII code units.
    let Some(selector_asset_end) = text_end(
        selector.selector_asset_id_offset,
        u64_from_index(selector.selector_asset_id.as_str().len()),
    ) else {
        return Ok(false);
    };
    let Some(selector_context_end) = text_end(
        selector.selector_context_id_offset,
        u64_from_index(selector.selector_context_id.as_str().len()),
    ) else {
        return Ok(false);
    };
    let Some(external_asset_end) = text_end(
        selector.external_asset_id_offset,
        u64_from_index(selector.external_asset_id.as_str().len()),
    ) else {
        return Ok(false);
    };
    let Some(external_link_len) = utf16_len(&selector.external_link_name)? else {
        return Ok(false);
    };
    let Some(external_link_end) = text_end(selector.external_link_name_offset, external_link_len)
    else {
        return Ok(false);
    };
    let external_end = match &selector.external_version {
        None => external_link_end.checked_add(1),
        Some(version) => {
            let property_key = version.property_key.value.as_str();
            let property_key_offset = version.property_key.offset;
            let version_urn = version.version_urn.value.as_str();
            let version_urn_offset = version.version_urn.offset;
            let version_len = utf16_len(version_urn)?;
            if external_link_end.checked_add(5) != Some(property_key_offset)
                || !version_len.is_some_and(|length| (1..=256).contains(&length))
                || text_end(property_key_offset, u64_from_index(property_key.len()))
                    .and_then(|end| end.checked_add(4))
                    != Some(version_urn_offset)
            {
                None
            } else {
                version_len.and_then(|length| text_end(version_urn_offset, length))
            }
        }
    };
    let Some(external_end) = external_end else {
        return Ok(false);
    };
    let Some(occurrence_role_end) = text_end(
        selector.occurrence_role_offset,
        u64_from_index(selector.occurrence_role.as_str().len()),
    ) else {
        return Ok(false);
    };
    let (values, located) = scope.reference_members().storage_slices();
    let (axis_count, selector_count, pair_count, _) = decode
        .admit_iter(values, "count F3D axial selector references")?
        .chain(
            decode
                .admit_iter(located, "count F3D axial selector references")?
                .map(|row| &row.value),
        )
        .fold(
            (0usize, 0usize, 0usize, None),
            |(axis_count, selector_count, pair_count, previous), member| {
                (
                    axis_count + usize::from(*member == selector.axis_record_index),
                    selector_count + usize::from(*member == selector.selector_record_index),
                    pair_count
                        + usize::from(
                            previous == Some(selector.axis_record_index)
                                && *member == selector.selector_record_index,
                        ),
                    Some(*member),
                )
            },
        );
    let selector_pair_is_referenced = pair_count == 1;
    let selector_records_are_unique = axis_count == 1 && selector_count == 1;

    Ok(design_header_matches(
        decode,
        records_by_index,
        stream,
        selector.axis_record_index,
        selector.axis_class_tag.as_str(),
        selector.axis_byte_offset,
    )? && design_header_matches(
        decode,
        records_by_index,
        stream,
        selector.selector_record_index,
        selector.selector_class_tag.as_str(),
        selector.selector_byte_offset,
    )? && selector.axis_record_index.checked_add(3) == Some(selector.selector_record_index)
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
        && (folded_guid(selector.external_asset_id.as_str())
            == folded_guid(selector.selector_asset_id.as_str()))
        && selector.occurrence_reference != 0
        && selector.external_object_reference != 0
        && (1..=256).contains(&external_link_len)
        && selector_pair_is_referenced
        && selector_records_are_unique)
}

fn valid_axial_assembly_targets(
    ctx: &Ctx<'_, '_>,
    stream: &str,
    scope: &records::feature::scope::DesignParameterScope,
    frames: &[records::feature::assembly::DesignAssemblyOperandFrame; 2],
    targets: &[&records::feature::assembly::DesignAssemblyAxialOperandTarget; 2],
) -> Result<bool, CodecError> {
    let decode = ctx.decode;
    let records_by_index = &ctx.records_by_index;
    targets
        .iter()
        .copied().zip(frames).try_fold(true, |valid, (target, frame)| {
            if !valid { return Ok(false); }
            Ok(match target {
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
                let component_scopes = decode.fold(ctx.scope_group(stream, *component_insert_scope_record_index, "find F3D axial component scopes")?, 0usize, |count, target_scope| {
                    let matches = (target_scope.kind() == crate::records::feature::scope::DesignFeatureKind::ComponentInsert)
                        && match target_scope.component_insert_construction() {
                            None => false,
                            Some(construction) => folded_guid(&construction.neutron_role) == folded_guid(selectors[0].occurrence_role.as_str()),
                        };
                    count.checked_add(usize::from(matches)).ok_or_else(|| decode.refuse_codec_limit("count F3D axial component scopes", u64::MAX - 1, u64::MAX))
                }, "scan F3D axial component scopes")?;
                frame.reference_record_index == *construction_record_index
                    && decode.admit_iter(scope.reference_members().storage_slices().0, "count F3D axial construction references")?
                        .chain(decode.admit_iter(scope.reference_members().storage_slices().1, "count F3D axial construction references")?.map(|row| &row.value))
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
                    && design_header_matches(decode,
                        records_by_index,
                        stream,
                        *construction_record_index,
                        construction_class_tag.as_str(),
                        *construction_byte_offset,
                    )?
                    && selectors_ordered
                    && valid_axial_selector_identity(decode,
                        records_by_index,
                        stream,
                        scope,
                        &selectors[0],
                        selectors[1].axis_byte_offset,
                    )?
                    && valid_axial_selector_identity(decode,
                        records_by_index,
                        stream,
                        scope,
                        &selectors[1],
                        *construction_byte_offset,
                    )?
                    && selectors[0].selects_same_object(decode, &selectors[1])?
                    && (folded_guid(selectors[0].occurrence_role.as_str()) == folded_guid(selectors[1].occurrence_role.as_str()))
                    && component_scopes == 1
            }
            records::feature::assembly::DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                scope_record_index,
            } => {
                frame.reference_record_index == *scope_record_index
                    && decode.fold(ctx.scope_group(stream, *scope_record_index, "find F3D axial document root scopes")?, 0usize, |count, target_scope| {
                        let matches = (target_scope.kind() == crate::records::feature::scope::DesignFeatureKind::JointOrigin)
                            && target_scope.joint_origin_transform() == Some(frame.transform);
                        count.checked_add(usize::from(matches)).ok_or_else(|| decode.refuse_codec_limit("count F3D axial document root targets", u64::MAX - 1, u64::MAX))
                    }, "scan F3D axial document root scopes")? == 1
            }
            })
        })
}

/// Collects the record indices claimed by an edge-flange operation.
fn collect_edge_flange_claimed_references<'a>(
    decode: &DecodeContext<'_>,
    edges: impl Iterator<Item = &'a records::feature::sheet_metal::DesignEdgeFlangeEdge>,
    owner_indices: impl Iterator<Item = &'a u32>,
    operation: &records::feature::sheet_metal::DesignEdgeFlangeOperation,
) -> Result<Vec<u32>, CodecError> {
    let fixed_references = [
        operation.selection.aggregate_group_record_index(),
        operation.height_owner_record_index,
        operation.angle_owner_record_index,
        operation.settings_record_index,
    ];
    decode.collect_vec(
        edges
            .flat_map(|edge| {
                [
                    edge.wrapper,
                    edge.group_record_index.get(),
                    edge.operand_record_index(),
                    edge.aggregate_operand_record_index,
                ]
            })
            .chain(owner_indices.copied())
            .chain(
                decode
                    .admit_iter(
                        &operation.auxiliary_reference_record_indices,
                        "scan F3D edge flange auxiliary references",
                    )?
                    .copied(),
            )
            .chain(fixed_references),
        "collect F3D edge flange claimed references",
    )
}

/// Assembly scopes indexed by their operand reference and byte offset.
struct AssemblyScopeIndexes<'a> {
    by_frame: RecordGroups<'a, (&'a str, u32), records::feature::scope::DesignParameterScope>,
    by_offset: RecordGroups<'a, (&'a str, u64), records::feature::scope::DesignParameterScope>,
}

/// Each index group keeps arena order.
fn assembly_scope_indexes<'a>(
    ctx: &Ctx<'a, '_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<AssemblyScopeIndexes<'a>, CodecError> {
    const OPERATION: &str = "group F3D assembly scopes";
    let mut by_frame = HashMap::new();
    let mut by_offset = HashMap::new();
    for scope in ctx
        .decode
        .admit_iter(&ctx.native.design_parameter_scopes, OPERATION)?
    {
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Assemble {
            continue;
        }
        let stream = design_stream(&scope.id);
        storage.with_storage(|| {
            ctx.decode.push_hash_group(
                &mut by_offset,
                (stream, scope.byte_offset()),
                scope,
                OPERATION,
                OPERATION,
            )
        })?;
        let Some(frames) = scope
            .assembly_alignment()
            .and_then(records::feature::assembly::DesignAssemblyAlignment::operand_frames)
        else {
            continue;
        };
        for (ordinal, frame) in frames.iter().enumerate() {
            if ordinal == 1 && frame.reference_record_index == frames[0].reference_record_index {
                continue;
            }
            storage.with_storage(|| {
                ctx.decode.push_hash_group(
                    &mut by_frame,
                    (stream, frame.reference_record_index),
                    scope,
                    OPERATION,
                    OPERATION,
                )
            })?;
        }
    }
    Ok(AssemblyScopeIndexes {
        by_frame,
        by_offset,
    })
}

/// Validate feature parameter scopes and their paired feature-operation frames.
pub(super) fn validate_parameter_scopes(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let placements_by_scope = &ctx.placements_by_scope;
    let mut storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D parameter scope validation indexes")?;
    let mut scope_indices = HashSet::new();
    let mut bindings_by_entity = None;
    let mut xrefs_by_role = None;
    let mut assemblies = None;
    for scope in ctx.decode.admit_iter(
        &native.design_parameter_scopes,
        "scan F3D design parameter scopes",
    )? {
        let native_stream = design_stream(&scope.id);
        let unique_index = storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut scope_indices,
                (native_stream, scope.record_index),
                "index F3D parameter scope records",
            )
        })?;
        let entity_link = scope
            .sketch_entity()
            .map(|binding| -> Result<bool, CodecError> {
                Ok(
                    match ctx.decode.get_hash_map(
                        entities_by_suffix,
                        &(native_stream, binding.entity_id.suffix()),
                        "find F3D parameter scope entity",
                    )? {
                        Some(entity) => {
                            ctx.decode.equal(
                                &entity.entity_id,
                                &binding.entity_id,
                                "compare F3D parameter scope entity",
                            )? && binding.entity_reference_offset > scope.byte_offset()
                                && binding.entity_reference_offset < scope.paired_byte_offset()
                        }
                        None => false,
                    },
                )
            })
            .transpose()?;
        let valid_sketch_profile =
            |profile: &records::topology::sketch_profile::DesignSketchProfileOperand| -> Result<bool, CodecError> {
                let header = ctx.decode.get_hash_map(records_by_index, &(native_stream, profile.record_index), "find F3D profile record header")?;
                let entity = ctx.decode.get_hash_map(entities_by_suffix, &(native_stream, profile.entity_id.suffix()), "find F3D profile entity")?;
                let reference_member = usize::try_from(profile.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| reference_member_at(scope.reference_members(), ordinal));
                Ok(reference_member == Some(&profile.record_index)
                    && match header {
                        Some(header) => header.byte_offset == profile.byte_offset()
                            && (header.class_tag == profile.class_tag),
                        None => false,
                    }
                    && match entity {
                        Some(entity) => entity.in_sketch_module()
                            && ctx.decode.equal(&entity.entity_id, &profile.entity_id, "compare F3D profile entity IDs")?,
                        None => false,
                    }
                    && match profile.region_selection.as_ref() {
                        Some(selection) => valid_sketch_profile_region_selection(ctx.decode, profile, selection)?,
                        None => true,
                    })
            };
        let extrude_profile_link = match scope.extrude_profile() {
            Some(profile) => valid_sketch_profile(profile)?,
            None => true,
        };
        let sweep_profile_link = match scope.sweep_profile() {
            Some(profile) => valid_sketch_profile(profile)?,
            None => true,
        };
        let is_base_flange =
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::BaseFlange;
        let base_flange_profile_link = match scope.base_flange_profile() {
            Some(profile) => valid_sketch_profile(profile)?,
            None => !is_base_flange,
        };
        let base_flange_link = match scope.base_flange_operation() {
            None => scope.kind() != crate::records::feature::scope::DesignFeatureKind::BaseFlange,
            Some(operation) => {
                scope.reference_members().values().copied().eq([
                    operation.profile_group_record_index,
                    operation.profile_record_index,
                    operation.thickness_record_index,
                    operation.settings_record_index,
                ]) && scope.base_flange_profile().is_some_and(|profile| {
                    profile.record_index == operation.profile_record_index
                        && profile.scope_reference_ordinal == 1
                }) && scope
                    .byte_offset()
                    .checked_add(123)
                    .is_some_and(|expected_offset| operation.thickness_offset == expected_offset)
                    && operation.thickness_offset < scope.paired_byte_offset()
            }
        };
        let edge_flange_link = match scope.edge_flange_operation() {
            None => true,
            Some(operation) => {
                // The ordered reference table is in record-index order, so the
                // check is that every role names a distinct table entry and that
                // the entries no role claims are exactly the width owners.
                let shape = operation.selection.shape();
                let edge_count = match shape {
                    records::feature::sheet_metal::DesignEdgeFlangeShape::FullEdge {
                        edges,
                        ..
                    }
                    | records::feature::sheet_metal::DesignEdgeFlangeShape::Symmetric {
                        edges,
                        ..
                    }
                    | records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSides {
                        edges,
                        ..
                    } => edges.len(),
                    records::feature::sheet_metal::DesignEdgeFlangeShape::SymmetricPerEdge(
                        edges,
                    ) => edges.len(),
                    records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSidesPerEdge {
                        edges,
                        ..
                    } => edges.len(),
                };
                let mut claimed_storage = ctx
                    .decode
                    .reserve_scoped(0, "hold F3D edge flange claimed references")?;
                let mut claimed =
                    claimed_storage.with_storage(|| -> Result<Vec<u32>, CodecError> {
                        Ok(match shape {
                        records::feature::sheet_metal::DesignEdgeFlangeShape::FullEdge {
                            edges, ..
                        } => collect_edge_flange_claimed_references(
                            ctx.decode,
                            ctx.decode
                                .admit_iter(edges, "scan F3D edge flange selected edges")?,
                            std::iter::empty(),
                            operation,
                        )?,
                        records::feature::sheet_metal::DesignEdgeFlangeShape::Symmetric {
                            edges,
                            owner,
                        } => collect_edge_flange_claimed_references(
                            ctx.decode,
                            ctx.decode
                                .admit_iter(edges, "scan F3D edge flange selected edges")?,
                            std::iter::once(owner),
                            operation,
                        )?,
                        records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSides {
                            edges,
                            owners,
                        } => collect_edge_flange_claimed_references(
                            ctx.decode,
                            ctx.decode
                                .admit_iter(edges, "scan F3D edge flange selected edges")?,
                            owners.iter(),
                            operation,
                        )?,
                        records::feature::sheet_metal::DesignEdgeFlangeShape::SymmetricPerEdge(
                            edges,
                        ) => collect_edge_flange_claimed_references(
                            ctx.decode,
                            ctx.decode
                                .admit_iter(edges, "scan F3D edge flange selected edges")?
                                .map(|row| &row.edge),
                            ctx.decode
                                .admit_iter(edges, "scan F3D edge flange width owners")?
                                .map(|row| &row.owners),
                            operation,
                        )?,
                        records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSidesPerEdge {
                            edges,
                            ..
                        } => collect_edge_flange_claimed_references(
                            ctx.decode,
                            ctx.decode
                                .admit_iter(edges, "scan F3D edge flange selected edges")?
                                .map(|row| &row.edge),
                            ctx.decode
                                .admit_iter(edges, "scan F3D edge flange width owners")?
                                .flat_map(|row| row.owners.iter()),
                            operation,
                        )?,
                    })
                    })?;
                if let records::feature::sheet_metal::DesignEdgeFlangeHeightExtent::ToObject {
                    target_group_record_index,
                    target_operand_record_index,
                    offset_owner_record_index,
                    ..
                } = operation.selection.shape().height()
                {
                    for index in [
                        target_group_record_index,
                        target_operand_record_index,
                        offset_owner_record_index,
                    ] {
                        claimed_storage.with_storage(|| {
                            ctx.decode.push_vec(
                                &mut claimed,
                                index,
                                "collect F3D edge flange target references",
                            )
                        })?;
                    }
                }
                let unique_claimed = claimed_storage.with_storage(|| {
                    ctx.decode.collect_hash_set(
                        claimed.iter().copied(),
                        "index F3D edge flange claimed references",
                    )
                })?;
                edge_count > 0
                    && claimed.len() == scope.reference_members().len()
                    && unique_claimed.len() == claimed.len()
                    && {
                        let references = claimed_storage.with_storage(|| {
                            ctx.decode.collect_hash_set(
                                scope.reference_members().values().copied(),
                                "index F3D edge flange scope references",
                            )
                        })?;
                        ctx.decode.all_by(
                            &claimed,
                            |index| {
                                ctx.decode.contains_hash_set(
                                    &references,
                                    index,
                                    "find F3D flange claimed reference",
                                )
                            },
                            "validate F3D flange claimed references",
                        )?
                    }
                    && operation.bend_radius_offset > scope.byte_offset()
                    && operation.bend_radius_offset < scope.paired_byte_offset()
            }
        };
        let hem_link = match scope.hem_operation() {
            None => true,
            Some(operation) => {
                let mut claimed_storage = ctx
                    .decode
                    .reserve_scoped(0, "hold F3D Hem claimed references")?;
                let mut claimed = claimed_storage.with_storage(|| {
                    ctx.decode.collect_vec(
                        [
                            operation.edge_wrapper_record_index,
                            operation.edge_group_record_index.get(),
                            operation.edge_operand_record_index(),
                            operation.aggregate_group_record_index.get(),
                            operation.aggregate_operand_record_index(),
                            operation.settings_record_index,
                        ],
                        "collect F3D Hem claimed references",
                    )
                })?;
                match &operation.parameter_owners {
                    crate::records::feature::sheet_metal::DesignHemParameterOwners::GapLength {
                        gap_owner_record_index,
                        length_owner_record_index,
                    } => {
                        ctx.decode.push_scoped_vec(&mut claimed_storage, &mut claimed, *gap_owner_record_index, "collect F3D Hem gap owner")?;
                        ctx.decode.push_scoped_vec(&mut claimed_storage, &mut claimed, *length_owner_record_index, "collect F3D Hem length owner")?;
                    }
                    crate::records::feature::sheet_metal::DesignHemParameterOwners::RadiusAngle {
                        radius_owner_record_index,
                        angle_owner_record_index,
                    } => {
                        ctx.decode.push_scoped_vec(&mut claimed_storage, &mut claimed, *radius_owner_record_index, "collect F3D Hem radius owner")?;
                        ctx.decode.push_scoped_vec(&mut claimed_storage, &mut claimed, *angle_owner_record_index, "collect F3D Hem angle owner")?;
                    }
                    crate::records::feature::sheet_metal::DesignHemParameterOwners::GapLengthRadius {
                        gap_owner_record_index,
                        length_owner_record_index,
                        radius_owner_record_index,
                    } => {
                        ctx.decode.push_scoped_vec(&mut claimed_storage, &mut claimed, *gap_owner_record_index, "collect F3D Hem gap owner")?;
                        ctx.decode.push_scoped_vec(&mut claimed_storage, &mut claimed, *length_owner_record_index, "collect F3D Hem length owner")?;
                        ctx.decode.push_scoped_vec(&mut claimed_storage, &mut claimed, *radius_owner_record_index, "collect F3D Hem radius owner")?;
                    }
                }
                let unique_claimed = claimed_storage.with_storage(|| {
                    ctx.decode.collect_hash_set(
                        claimed.iter().copied(),
                        "index F3D Hem claimed references",
                    )
                })?;
                unique_claimed.len() == claimed.len()
                    && claimed.len() == scope.reference_members().len()
                    && ctx.decode.all_by(
                        &claimed,
                        |index| {
                            let (plain, located) = scope.reference_members().storage_slices();
                            Ok(ctx.decode.any_by(
                                plain,
                                |value| Ok(value == index),
                                "find F3D Hem reference",
                            )? || ctx.decode.any_by(
                                located,
                                |row| Ok(&row.value == index),
                                "find F3D located Hem reference",
                            )?)
                        },
                        "check F3D Hem claimed references",
                    )?
                    && operation.bend_radius_offset > scope.byte_offset()
                    && operation.bend_radius_offset < scope.paired_byte_offset()
            }
        };
        let copy_paste_link = match scope.copy_paste_bodies_operation() {
            None => {
                scope.kind() != crate::records::feature::scope::DesignFeatureKind::CopyPasteBodies
            }
            Some(operation) => {
                let group_header = ctx.decode.get_hash_map(
                    records_by_index,
                    &(native_stream, operation.body_group_record_index),
                    "find F3D validation linked record",
                )?;
                let relation_header = ctx.decode.get_hash_map(
                    records_by_index,
                    &(native_stream, operation.relation_record_index),
                    "find F3D validation linked record",
                )?;
                scope.reference_members().values().next()
                    == Some(&operation.body_group_record_index)
                    && scope.reference_members().len().checked_sub(1)
                        == Some(operation.bodies().len())
                    && ctx.decode.all_by(
                        scope
                            .reference_members()
                            .values()
                            .skip(1)
                            .zip(operation.bodies()),
                        |(record_index, body)| Ok(*record_index == body.operand.value),
                        "compare F3D copied body references",
                    )?
                    && match group_header {
                        Some(header) => {
                            header.byte_offset == operation.body_group_byte_offset()
                                && (header.class_tag == operation.body_group_class_tag)
                        }
                        None => false,
                    }
                    && match relation_header {
                        Some(header) => {
                            header.byte_offset == operation.relation_byte_offset()
                                && (header.class_tag == operation.relation_class_tag)
                        }
                        None => false,
                    }
                    && {
                        let bindings_by_entity = match &mut bindings_by_entity {
                            Some(bindings) => bindings,
                            None => bindings_by_entity.insert(group_records(
                                ctx.decode,
                                &mut storage,
                                &native.design_body_bindings,
                                |binding| {
                                    Some((design_stream(binding.id()), binding.entity_suffix))
                                },
                                "group F3D copied body bindings",
                            )?),
                        };
                        let bindings_at = |entity_suffix: u32| {
                            Ok::<_, CodecError>(
                                ctx.decode
                                    .get_hash_map(
                                        bindings_by_entity,
                                        &(native_stream, u64::from(entity_suffix)),
                                        "find F3D copied body bindings",
                                    )?
                                    .map_or(&[][..], Vec::as_slice),
                            )
                        };
                        ctx.decode.all_by(
                            operation.bodies(),
                            |body| Ok(!bindings_at(body.source.value)?.is_empty()),
                            "validate F3D copied body sources",
                        )? && ctx.decode.all_by(
                            operation.bodies(),
                            |body| {
                                ctx.decode.any_by(
                                    bindings_at(body.copied.value)?,
                                    |binding| Ok(binding.body.is_some()),
                                    "find F3D copied body target binding",
                                )
                            },
                            "validate F3D copied body targets",
                        )?
                    }
            }
        };
        let rectangular_pattern_link = match scope.rectangular_pattern_construction() {
            None => {
                design::design_feature_family(&scope.kind())
                    != Some(design::DesignFeatureFamily::RectangularPattern)
            }
            Some(construction) => {
                let instances_link = construction.instances().map(|instances| -> Result<bool, CodecError> {
                    let active = [
                        (construction.u_count(), construction.u_extent()),
                        (construction.v_count(), construction.v_extent()),
                    ];
                    let (count, extent) = match active {
                        [(count, extent), (other_count, _)] if count > 1 && other_count <= 1 => (count, extent),
                        [(other_count, _), (count, extent)] if count > 1 && other_count <= 1 => (count, extent),
                        _ => return Ok(false),
                    };
                    let Ok(count) = usize::try_from(count) else {
                        return Ok(false);
                    };
                    let Some(reference_end) = count.checked_add(5) else {
                        return Ok(false);
                    };
                    let mut expected_records = scope
                        .reference_members()
                        .values()
                        .next()
                        .into_iter()
                        .chain(
                            scope
                                .reference_members()
                                .values_in(6..reference_end)
                                .into_iter()
                                .flatten(),
                        )
                        .copied();
                    let Some(first) = instances
                        .frames()
                        .next()
                        .map(|frame| &frame.transform.value)
                    else {
                        return Ok(false);
                    };
                    let Some(last) = instances
                        .frames()
                        .next_back()
                        .map(|frame| &frame.transform.value)
                    else {
                        return Ok(false);
                    };
                    let delta = [
                        last[0][3] - first[0][3],
                        last[1][3] - first[1][3],
                        last[2][3] - first[2][3],
                    ];
                    let distance = cadmpeg_ir::math::Vector3::from(delta).norm();
                    let (body_frames, seed_frame, generated_frames): (
                        &[records::feature::patterns::DesignPatternInstance],
                        Option<&records::feature::patterns::DesignPatternInstance>,
                        &[records::feature::patterns::DesignPatternComponentInstance],
                    ) = match instances {
                        records::feature::patterns::DesignRectangularPatternInstances::Bodies(rows) => (rows, None, &[]),
                        records::feature::patterns::DesignRectangularPatternInstances::Components { seed, generated, .. } => (&[], Some(&seed.instance), generated),
                    };
                    let rotation_matches = |frame: &records::feature::patterns::DesignPatternInstance| {
                        let transform = &frame.transform.value;
                        (0..3).all(|row| (0..3).all(|column| {
                            (transform[row][column] - first[row][column]).abs() <= EPS_PATTERN_ROTATION
                        }))
                    };
                    let position_matches = |ordinal: usize, frame: &records::feature::patterns::DesignPatternInstance| {
                        let transform = &frame.transform.value;
                        let (Some(ordinal), Some(divisor)) = (f64_from_index(ordinal), f64_from_index(count - 1)) else { return false; };
                        let fraction = ordinal / divisor;
                        (0..3).all(|axis| (transform[axis][3] - first[axis][3] - delta[axis] * fraction).abs() <= EPS_PATTERN_DISTANCE)
                    };
                    let frame_follows_header = |frame: &records::feature::patterns::DesignPatternInstance| -> Result<bool, CodecError> {
                        Ok(ctx.decode.get_hash_map(records_by_index, &(native_stream, frame.record_index), "find F3D rectangular pattern frame header")?
                            .is_some_and(|header| frame.transform.offset > header.byte_offset))
                    };
                    let component_link =
                        valid_component_pattern_occurrences(ctx, native_stream, instances)?;
                    Ok(ctx.decode.all_by(
                        instances.frames(),
                        |frame| Ok(expected_records.next() == Some(frame.record_index)),
                        "compare F3D rectangular pattern frame records",
                    )? && expected_records.next().is_none()
                        && instances.instance_count() == count
                        && ctx.decode.all_by(body_frames, |frame| Ok(rotation_matches(frame)), "validate F3D rectangular pattern body rotations")?
                        && seed_frame.is_none_or(rotation_matches)
                        && ctx.decode.all_by(generated_frames, |row| Ok(rotation_matches(&row.instance)), "validate F3D rectangular pattern component rotations")?
                        && (distance - extent.abs()).abs()
                            <= EPS_PATTERN_DISTANCE
                        && ctx.decode.all_by(body_frames.iter().enumerate(), |(ordinal, frame)| Ok(position_matches(ordinal, frame)), "validate F3D rectangular pattern body positions")?
                        && seed_frame.is_none_or(|frame| position_matches(0, frame))
                        && ctx.decode.all_by(generated_frames.iter().enumerate(), |(ordinal, row)| {
                                let ordinal = ordinal.checked_add(1).ok_or_else(|| ctx.decode.refuse_codec_limit("count F3D rectangular pattern component positions", u64::MAX - 1, u64::MAX))?;
                                Ok(position_matches(ordinal, &row.instance))
                            }, "validate F3D rectangular pattern component positions")?
                        && ctx.decode.all_by(body_frames, frame_follows_header, "validate F3D rectangular pattern body frame headers")?
                        && seed_frame.map(frame_follows_header).transpose()?.unwrap_or(true)
                        && ctx.decode.all_by(generated_frames, |row| frame_follows_header(&row.instance), "validate F3D rectangular pattern component frame headers")?
                        && component_link)
                }).transpose()?.unwrap_or(true);
                instances_link
                    && ctx
                        .scope_owners(
                            native_stream,
                            scope.record_index,
                            "find F3D rectangular pattern parameter owners",
                        )?
                        .len()
                        == 4
                    && construction.owner_record_indices.iter().try_fold(
                        true,
                        |valid, record_index| -> Result<bool, CodecError> {
                            if !valid {
                                return Ok(false);
                            }
                            let (plain, located) = scope.reference_members().storage_slices();
                            Ok((ctx.decode.contains(
                                plain,
                                record_index,
                                "find F3D rectangular pattern owner reference",
                            )? || ctx.decode.any_by(
                                located,
                                |row| Ok(row.value == *record_index),
                                "find F3D located rectangular pattern owner reference",
                            )?) && ctx.decode.contains_key_hash_map(
                                records_by_index,
                                &(native_stream, *record_index),
                                "find F3D rectangular pattern owner header",
                            )?)
                        },
                    )?
                    && construction
                        .owner_record_indices
                        .iter()
                        .zip(construction.value_offsets)
                        .zip([
                            f64::from(construction.u_count()),
                            f64::from(construction.v_count()),
                            construction.u_extent(),
                            construction.v_extent(),
                        ])
                        .enumerate()
                        .try_fold(
                            true,
                            |valid,
                             (ordinal, ((record_index, value_offset), value))|
                             -> Result<bool, CodecError> {
                                if !valid {
                                    return Ok(false);
                                }
                                ctx.decode.any_by(
                                    ctx.owner_group(
                                        native_stream,
                                        *record_index,
                                        "find F3D rectangular pattern lane owner records",
                                    )?,
                                    |owner| {
                                        Ok(owner.scope_record_index() == scope.record_index
                                            && u32::try_from(ordinal) == Ok(owner.local_ordinal())
                                            && owner.evaluated_value().get() == value
                                            && owner.evaluated_value_offset() == value_offset)
                                    },
                                    "find F3D rectangular pattern lane owner",
                                )
                            },
                        )?
            }
        };
        let assembly_alignment_link = match scope.assembly_alignment() {
            None => {
                design::design_feature_family(&scope.kind())
                    != Some(design::DesignFeatureFamily::Assemble)
            }
            Some(alignment) => {
                let values_storage = if alignment.owners.len() == 2 {
                    [alignment.angle(), alignment.offset()[2], 0.0, 0.0]
                } else {
                    [
                        alignment.angle(),
                        alignment.offset()[0],
                        alignment.offset()[1],
                        alignment.offset()[2],
                    ]
                };
                let values = &values_storage[..if alignment.owners.len() == 2 { 2 } else { 4 }];
                let generation = design::assembly::AssemblyScopeGeneration::new(
                    scope.frame_length(),
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                );
                let operand_frame_variant = generation.operand_frame_variant();
                let variable_reference = design::assembly::variable_reference_assembly_generation(
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                );
                let compact_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::Compact)
                );
                let axial_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::Axial)
                );
                let as_built_frames = (scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::AsBuilt)
                    && scope.frame_length() == 399;
                let as_built_421_generation = design::assembly::legacy_as_built_421_generation(
                    scope.frame_length(),
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                );
                let as_built_421 = as_built_421_generation.is_some();
                let operand_paths = alignment.operand_path_refs();
                let frame_reference_offsets = if axial_frames {
                    [29, 168]
                } else if compact_frames {
                    [25, 165]
                } else {
                    [29, 169]
                };
                let frame_transform_offsets = if axial_frames {
                    [39, 178]
                } else if compact_frames {
                    [36, 176]
                } else {
                    [40, 180]
                };
                let assembly_owner_count = ctx
                    .scope_owners(
                        native_stream,
                        scope.record_index,
                        "find F3D assembly parameter owners",
                    )?
                    .len();
                let alignment_lane_bounds = generation.alignment_lane_bounds(assembly_owner_count);
                let operand_frames_link = if let Some(
                    records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                        carriers,
                        ..
                    },
                ) = &alignment.form
                {
                    carriers.references().into_iter().enumerate().try_fold(
                        true,
                        |valid, (ordinal, reference)| -> Result<bool, CodecError> {
                            if !valid {
                                return Ok(false);
                            }
                            let reference_ordinal = ordinal * 2;
                            let (_, located) = scope.reference_members().storage_slices();
                            Ok(
                                reference_member_at(scope.reference_members(), reference_ordinal)
                                    .copied()
                                    == Some(reference.value)
                                    && located.get(reference_ordinal).map(|row| row.offset)
                                        == Some(reference.offset)
                                    && ctx.decode.contains_key_hash_map(
                                        records_by_index,
                                        &(native_stream, reference.value),
                                        "find F3D legacy assembly reference header",
                                    )?,
                            )
                        },
                    )?
                } else {
                    alignment
                        .operand_frames()
                        .map(|frames| -> Result<bool, CodecError> {
                            Ok(
                                frames[0].reference_record_index
                                    != frames[1].reference_record_index
                                    && frames.iter().enumerate().try_fold(
                                        true,
                                        |valid, (ordinal, frame)| -> Result<bool, CodecError> {
                                            if !valid {
                                                return Ok(false);
                                            }
                                            let offsets_match = if as_built_frames {
                                                operand_paths.as_ref().is_some_and(|paths| {
                                                    paths[ordinal]
                                                        .link()
                                                        .locator_byte_offset
                                                        .checked_add(22)
                                                        == Some(frame.reference_offset)
                                                        && paths[ordinal]
                                                            .link()
                                                            .locator_byte_offset
                                                            .checked_add(33)
                                                            == Some(frame.transform_offset)
                                                })
                                            } else {
                                                Some(frame.reference_offset)
                                                    == scope.byte_offset().checked_add(
                                                        frame_reference_offsets[ordinal],
                                                    )
                                                    && Some(frame.transform_offset)
                                                        == scope.byte_offset().checked_add(
                                                            frame_transform_offsets[ordinal],
                                                        )
                                            };
                                            let reference_exists = if as_built_frames {
                                                frame.reference_record_index != 0
                                            } else {
                                                ctx.decode.contains_key_hash_map(
                                                    records_by_index,
                                                    &(native_stream, frame.reference_record_index),
                                                    "find F3D assembly operand frame header",
                                                )?
                                            };
                                            Ok(offsets_match && reference_exists)
                                        },
                                    )?,
                            )
                        })
                        .transpose()?
                        .unwrap_or(true)
                };
                let solved_frame_link = alignment
                    .solved_frame()
                    .map(|frame| -> Result<bool, CodecError> {
                        let Some(generation) = as_built_421_generation else {
                            return Ok(false);
                        };
                        let Some(header) = ctx.decode.get_hash_map(
                            records_by_index,
                            &(native_stream, frame.reference_record_index),
                            "find F3D solved assembly frame header",
                        )?
                        else {
                            return Ok(false);
                        };
                        let (_, located) = scope.reference_members().storage_slices();
                        Ok(as_built_421
                            && reference_member_at(scope.reference_members(), 8).copied()
                                == Some(frame.reference_record_index)
                            && located.get(8).map(|row| row.offset) == Some(frame.reference_offset)
                            && (header.class_tag.as_str() == generation.frame_class_tag())
                            && (frame.class_tag.as_str() == header.class_tag.as_str())
                            && frame.record_byte_offset == header.byte_offset
                            && Some(frame.transform_offset)
                                == frame
                                    .record_byte_offset
                                    .checked_add(u64_from_index(generation.matrix_offset())))
                    })
                    .transpose()?
                    .unwrap_or(true);
                let operand_qualifiers_link = match alignment.form.as_ref() {
                    Some(records::feature::assembly::DesignAssemblyAlignmentForm::Qualified(
                        operands,
                    )) => {
                        let frames = operands.each_ref().map(|operand| operand.frame.clone());
                        match (&operands[0].qualifier, &operands[1].qualifier) {
                            (records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path: first },
                             records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path: second }) => {
                                let paths = [first, second];

                            let class_363_carriers = paths
                                .iter()
                                .all(|path| path.link().locator_class_tag.as_str() == "363");
                            if class_363_carriers {
                                !axial_frames
                                    && paths[0].link().locator_record_index
                                        != paths[1].link().locator_record_index
                                    && paths.iter().zip(&frames).all(|(path, frame)| {
                                        valid_class_363_operand_path_link(scope, frame, path)
                                    })
                            } else {
                                let locator_offsets =
                                    generation.operand_path_locator_offsets();
                                let first_start = paths[0].link().locator_byte_offset;
                                let second_start = paths[1].link().locator_byte_offset;
                                let envelope_ends = paths.each_ref().map(|path| {
                                    let continuation_count = if variable_reference {
                                        path.link()
                                            .wrapper_record_index
                                            .checked_sub(path.link().locator_record_index)?
                                            .checked_sub(2)?
                                    } else {
                                        0
                                    };
                                    u64::try_from(path_wrapper::LEN)
                                        .ok()?
                                        .checked_add(u64::from(continuation_count).checked_mul(11)?)
                                        .and_then(|length| {
                                            path.link().wrapper_byte_offset.checked_add(length)
                                        })
                                });
                                !axial_frames
                                    && locator_offsets.is_some_and(|offsets| {
                                        paths.iter().zip(offsets).all(|(path, offset)| {
                                            valid_assembly_operand_path_link(scope, path, offset)
                                        })
                                    })
                                    && paths[0].link().locator_record_index
                                        != paths[1].link().locator_record_index
                                    && matches!(envelope_ends, [Some(first_end), Some(second_end)]
                                if !(first_start < second_end && second_start < first_end))
                                    && paths.iter().try_fold(true, |valid, path| -> Result<bool, CodecError> {
                                        if !valid { return Ok(false); }
                                        Ok(matches!(path.class_tag().as_str(), "294" | "299" | "307" | "329" | "330" | "386" | "390")
                                            && !matches!(path.link().locator_class_tag.as_str(), "363" | "378")
                                            && (matches!(
                                                path.class_tag().as_str(),
                                                "294" | "299" | "307" | "330" | "386"
                                            ) || path.occurrence_guids().first().map(|guid| -> Result<bool, CodecError> {
                                                Ok(ctx.occurrences_with_guid(native_stream, guid.value.as_str(), "find F3D assembly path occurrences")?.len() == 1)
                                            }).transpose()?.unwrap_or(false)))
                                    })?
                            }

                            }
                            (records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target: first },
                             records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target: second }) => {
                                axial_frames && valid_axial_assembly_targets(ctx, native_stream, scope, &frames, &[first, second])?
                            }
                            _ if variable_reference && operands.iter().any(|operand| matches!(operand.qualifier, records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin { .. })) => {
                                frames[0].reference_record_index != frames[1].reference_record_index
                                    && operands.iter().try_fold(true, |valid, operand| -> Result<bool, CodecError> {
                                        if !valid { return Ok(false); }
                                        Ok(match &operand.qualifier {
                                        records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path } => valid_class_363_operand_path_link(scope, &operand.frame, path),
                                        qualifier @ records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin { .. } => valid_class_307_joint_origin_qualifier(ctx, native_stream, &operand.frame, qualifier)?,
                                        records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { .. } => false,
                                    } )
                                    })?
                            }
                            _ => false,
                        }
                    }
                    Some(records::feature::assembly::DesignAssemblyAlignmentForm::Frames {
                        ..
                    }) => axial_frames,
                    Some(records::feature::assembly::DesignAssemblyAlignmentForm::LimitsOnly {
                        ..
                    }) => as_built_421,
                    None
                    | Some(
                        records::feature::assembly::DesignAssemblyAlignmentForm::DatumEnvelope {
                            ..
                        }
                        | records::feature::assembly::DesignAssemblyAlignmentForm::SolvedOnly {
                            ..
                        }
                        | records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                            ..
                        },
                    ) => true,
                };
                let joint_origin_envelope_link = alignment
                    .joint_origin_scope_record_index()
                    .map(|record_index| -> Result<bool, CodecError> {
                        Ok(scope.class_tag.as_str() == "276"
                            && scope.paired_class_tag.as_str() == "258"
                            && scope.frame_length() == 604
                            && ctx.decode.any_by(ctx.scope_group(native_stream, record_index, "find F3D joint origin target scopes")?, |target| {
                                Ok((target.kind() == crate::records::feature::scope::DesignFeatureKind::JointOrigin)
                                    && scope.byte_offset().checked_add(36).is_some_and(|offset| target.joint_origin_transform_offset() == Some(offset)))
                            }, "find F3D joint origin envelope target")?)
                    }).transpose()?.unwrap_or(true);
                let alignment_scalars_link = if let Some(generation) = as_built_421_generation {
                    match (alignment.limits(), alignment.owners.as_slice()) {
                        (Some(limits), [angle, offset_x, offset_y, offset_z]) => {
                            let alignment_lanes = [
                                (
                                    offset_x.value,
                                    offset_x.offset,
                                    alignment.offset()[0],
                                    0_u32,
                                ),
                                (
                                    offset_y.value,
                                    offset_y.offset,
                                    alignment.offset()[1],
                                    1_u32,
                                ),
                                (
                                    offset_z.value,
                                    offset_z.offset,
                                    alignment.offset()[2],
                                    2_u32,
                                ),
                                (angle.value, angle.offset, alignment.angle(), 3_u32),
                            ];
                            let limit_order = if generation.reverse_limit_order() {
                                [1, 0]
                            } else {
                                [0, 1]
                            };
                            let limit_lanes = limit_order.into_iter().zip([4_u32, 5]).map(
                                |(index, local_ordinal)| {
                                    (
                                        limits.owner_record_indices[index],
                                        limits.value_offsets[index],
                                        [limits.minimum(), limits.maximum()][index],
                                        local_ordinal,
                                    )
                                },
                            );
                            limits.kind == generation.limit_kind()
                                && alignment_lanes.into_iter().chain(limit_lanes).try_fold(true,
                                    |valid, (record_index, value_offset, value, local_ordinal)| -> Result<bool, CodecError> {
                                        if !valid { return Ok(false); }
                                        ctx.decode.any_by(ctx.owner_group(native_stream, record_index, "find F3D legacy alignment lane owner records")?, |owner| {
                                            Ok(owner.scope_record_index() == scope.record_index
                                                && owner.local_ordinal() == local_ordinal
                                                && owner.evaluated_value().get() == value
                                                && owner.evaluated_value_offset() == value_offset)
                                        }, "find F3D legacy alignment lane owner")
                                    },
                                )?
                        }
                        _ => false,
                    }
                } else {
                    alignment_lane_bounds
                        .map(
                            |(alignment_start, alignment_end)| -> Result<bool, CodecError> {
                                Ok(alignment_end.checked_sub(alignment_start).is_some_and(
                                    |expected_offset| alignment.owners.len() == expected_offset,
                                ) && ctx.decode.all_by(
                                    alignment.owners.iter().zip(values.iter()).enumerate(),
                                    |(ordinal, (lane, value))| -> Result<bool, CodecError> {
                                        let ordinal = alignment_start
                                            .checked_add(ordinal)
                                            .ok_or_else(|| {
                                                ctx.decode.refuse_codec_limit(
                                                    "count F3D alignment owner ordinal",
                                                    u64::MAX - 1,
                                                    u64::MAX,
                                                )
                                            })?;
                                        ctx.decode.any_by(
                                            ctx.owner_group(
                                                native_stream,
                                                lane.value,
                                                "find F3D alignment lane owner records",
                                            )?,
                                            |owner| {
                                                Ok(owner.scope_record_index() == scope.record_index
                                                    && u32::try_from(ordinal)
                                                        == Ok(owner.local_ordinal())
                                                    && owner.evaluated_value().get() == *value
                                                    && owner.evaluated_value_offset()
                                                        == lane.offset)
                                            },
                                            "find F3D alignment lane owner",
                                        )
                                    },
                                    "validate F3D assembly alignment owners",
                                )?)
                            },
                        )
                        .transpose()?
                        .unwrap_or(false)
                };
                let alignment_reference_link = if let Some(generation) = as_built_421_generation {
                    alignment.limits().is_some_and(|limits| {
                        let limit_reference_indices = if generation.reverse_limit_order() {
                            [
                                limits.owner_record_indices[1],
                                limits.owner_record_indices[0],
                            ]
                        } else {
                            limits.owner_record_indices
                        };
                        alignment.owners.len() == 4
                            && scope
                                .reference_members()
                                .values()
                                .skip(4)
                                .take(4)
                                .copied()
                                .eq([
                                    alignment.owners[1].value,
                                    alignment.owners[2].value,
                                    alignment.owners[3].value,
                                    alignment.owners[0].value,
                                ])
                            && scope
                                .reference_members()
                                .values()
                                .skip(9)
                                .take(2)
                                .eq(limit_reference_indices.iter())
                    })
                } else if design::assembly::variable_reference_assembly_generation(
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                ) {
                    let references = scope.reference_members();
                    let (reference_values, located_reference_values) = references.storage_slices();
                    let matching_windows = ctx
                        .decode
                        .admit_iter(
                            reference_values,
                            "scan F3D variable alignment reference-window starts",
                        )?
                        .chain(
                            ctx.decode
                                .admit_iter(
                                    located_reference_values,
                                    "scan F3D variable alignment reference-window starts",
                                )?
                                .map(|row| &row.value),
                        )
                        .enumerate()
                        .try_fold(
                            0usize,
                            |count, (start, first_reference)| -> Result<usize, CodecError> {
                                if alignment.owners.is_empty() {
                                    return count.checked_add(1).ok_or_else(|| {
                                        ctx.decode.refuse_codec_limit(
                                            "count F3D variable alignment reference windows",
                                            u64::MAX - 1,
                                            u64::MAX,
                                        )
                                    });
                                }
                                let end =
                                    start.checked_add(alignment.owners.len()).ok_or_else(|| {
                                        ctx.decode.refuse_codec_limit(
                                            "compare F3D variable alignment reference window",
                                            u64::MAX - 1,
                                            u64::MAX,
                                        )
                                    })?;
                                if end > references.len() {
                                    return Ok(count);
                                }

                                let Some(first_owner) = alignment.owners.first() else {
                                    return Ok(count);
                                };
                                if first_owner.value != *first_reference {
                                    return Ok(count);
                                }

                                let suffix_start = start + 1;
                                let suffix_matches =
                                    if let Some(values) = reference_values.get(suffix_start..end) {
                                        ctx.decode.all_by(
                                            alignment.owners[1..].iter().zip(values),
                                            |(owner, value)| Ok(owner.value == *value),
                                            "compare F3D variable alignment reference window",
                                        )?
                                    } else if let Some(rows) =
                                        located_reference_values.get(suffix_start..end)
                                    {
                                        ctx.decode.all_by(
                                            alignment.owners[1..].iter().zip(rows),
                                            |(owner, row)| Ok(owner.value == row.value),
                                            "compare F3D variable alignment reference window",
                                        )?
                                    } else {
                                        false
                                    };
                                if suffix_matches {
                                    count.checked_add(1).ok_or_else(|| {
                                        ctx.decode.refuse_codec_limit(
                                            "count F3D variable alignment reference windows",
                                            u64::MAX - 1,
                                            u64::MAX,
                                        )
                                    })
                                } else {
                                    Ok(count)
                                }
                            },
                        )?;
                    matching_windows == 1
                } else {
                    scope.reference_members().len() >= alignment.owners.len()
                        && ctx.decode.all_by(
                            scope
                                .reference_members()
                                .values()
                                .rev()
                                .zip(alignment.owners.iter().rev()),
                            |(record_index, owner)| Ok(*record_index == owner.value),
                            "compare F3D alignment trailing references",
                        )?
                };
                operand_frames_link
                    && solved_frame_link
                    && operand_qualifiers_link
                    && joint_origin_envelope_link
                    && alignment_reference_link
                    && alignment_scalars_link
            }
        };
        let component_insert_link = match scope.component_insert_construction() {
            None => {
                scope.kind() != crate::records::feature::scope::DesignFeatureKind::ComponentInsert
            }
            Some(construction) => {
                let relation = ctx.decode.get_hash_map(
                    records_by_index,
                    &(native_stream, construction.relation_record_index),
                    "find F3D validation linked record",
                )?;
                let frame_matches_transform = match (
                    scope.frame_length(),
                    scope.paired_class_tag.as_str(),
                    construction.placement.as_ref(),
                ) {
                    (261, "263", None) if scope.class_tag.as_str() == "296" => true,
                    (261, "261", None) if scope.class_tag.as_str() == "410" => true,
                    (261, "258", None) if scope.class_tag.as_str() == "426" => true,
                    (261, "266", None) if scope.class_tag.as_str() == "434" => true,
                    (257 | 261 | 267, "264", None) if scope.class_tag.as_str() == "414" => true,
                    (257, "262", None) if scope.class_tag.as_str() == "283" => true,
                    (385, "262", Some(matrix)) if scope.class_tag.as_str() == "283" => {
                        scope
                            .byte_offset()
                            .checked_add(46)
                            .is_some_and(|expected_offset| matrix.scope.offset == expected_offset)
                            && matrix.carrier_offset.is_none()
                    }
                    (404, _, Some(matrix)) => {
                        scope
                            .byte_offset()
                            .checked_add(54)
                            .is_some_and(|expected_offset| matrix.scope.offset == expected_offset)
                            && matrix
                                .carrier_offset
                                .is_some_and(|offset| offset < construction.neutron_role_offset)
                    }
                    (frame_length, paired, Some(matrix)) => {
                        let scope_delta = match (frame_length, paired) {
                            (399, "259") => Some(50),
                            (381, "261") => Some(49),
                            (395, "258") => Some(46),
                            (389, "264") if scope.class_tag.as_str() == "414" => Some(50),
                            _ => None,
                        };
                        scope_delta.is_some_and(|delta| {
                            scope
                                .byte_offset()
                                .checked_add(delta)
                                .is_some_and(|expected_offset| {
                                    matrix.scope.offset == expected_offset
                                })
                        }) && matrix
                            .carrier_offset
                            .is_some_and(|offset| construction.neutron_role_offset < offset)
                    }
                    _ => false,
                };
                let role_valid = crate::bytes::is_guid_relaxed(&construction.neutron_role)
                    || (crate::bytes::is_guid_prefix(&construction.neutron_role)
                        && construction.neutron_role.as_bytes().get(36) == Some(&b'_')
                        && construction
                            .neutron_role
                            .get(37..)
                            .is_some_and(|suffix| suffix.starts_with("urn:")));
                scope
                    .reference_members()
                    .values()
                    .copied()
                    .eq([construction.relation_record_index])
                    && construction.carrier_record_index != construction.relation_record_index
                    && role_valid
                    && frame_matches_transform
                    && relation.is_some_and(|relation| {
                        construction
                            .carrier_transform_offset()
                            .is_none_or(|offset| offset < relation.byte_offset)
                    })
                    && (native.xref_references.is_empty() || {
                        let xrefs_by_role = match &mut xrefs_by_role {
                            Some(xrefs) => xrefs,
                            None => xrefs_by_role.insert(group_records(
                                ctx.decode,
                                &mut storage,
                                &native.xref_references,
                                |reference| Some(reference.neutron_role.as_str()),
                                "group F3D component insert xrefs",
                            )?),
                        };
                        ctx.decode.any_by(
                            ctx.decode
                                .get_hash_map(
                                    xrefs_by_role,
                                    construction.neutron_role.as_str(),
                                    "find F3D component insert xrefs",
                                )?
                                .map_or(&[][..], Vec::as_slice),
                            |reference| {
                                Ok(reference
                                    .transform
                                    .map(records::xref::XrefPlacementTransform::rows)
                                    == Some((*construction.transform()).into()))
                            },
                            "find F3D component insert xref",
                        )?
                    })
            }
        };
        let copy_paste_component_link = match scope.copy_paste_component_operation() {
            None => scope.kind() != crate::records::feature::scope::DesignFeatureKind::CopyPaste,
            Some(operation) => {
                let source = ctx
                    .occurrence_records(
                        native_stream,
                        operation.source_occurrence_record_index,
                        "find F3D source component occurrence",
                    )?
                    .first()
                    .copied();
                let copied = ctx
                    .occurrence_records(
                        native_stream,
                        operation.copied_occurrence_record_index,
                        "find F3D copied component occurrence",
                    )?
                    .first()
                    .copied();
                let source_at = match scope.frame_length() {
                    529 => 38,
                    525 => 34,
                    _ => 0,
                };
                source_at != 0
                    && scope
                        .reference_members()
                        .values()
                        .copied()
                        .eq([operation.relation_record_index])
                    && operation.source_occurrence_record_index
                        != operation.copied_occurrence_record_index
                    && Some(operation.source_transform_offset)
                        == scope.byte_offset().checked_add(source_at)
                    && Some(operation.copied_transform_offset)
                        == scope.byte_offset().checked_add(source_at + 156)
                    && source
                        .map(|source| -> Result<bool, CodecError> {
                            Ok((folded_guid(source.component_guid.as_str())
                                == folded_guid(operation.component_guid.as_str()))
                                && (folded_guid(source.occurrence_guid.as_str())
                                    == folded_guid(operation.source_occurrence_guid.as_str()))
                                && source.transform().is_none())
                        })
                        .transpose()?
                        .unwrap_or(false)
                    && copied
                        .map(|copied| -> Result<bool, CodecError> {
                            Ok((folded_guid(copied.component_guid.as_str())
                                == folded_guid(operation.component_guid.as_str()))
                                && (folded_guid(copied.occurrence_guid.as_str())
                                    == folded_guid(operation.copied_occurrence_guid.as_str()))
                                && copied.transform().map(|frame| frame.value)
                                    == Some(operation.copied_transform))
                        })
                        .transpose()?
                        .unwrap_or(false)
            }
        };
        let draft_link = match scope.draft_operation() {
            None => {
                design::design_feature_family(&scope.kind())
                    != Some(design::DesignFeatureFamily::Draft)
            }
            Some(operation) => {
                scope.reference_members().len() >= 6
                    && {
                        let (plain, located) = scope.reference_members().storage_slices();
                        ctx.decode.contains(
                            plain,
                            &operation.angle_record_index,
                            "find F3D Draft scope reference",
                        )? || ctx.decode.any_by(
                            located,
                            |row| Ok(row.value == operation.angle_record_index),
                            "find F3D located Draft scope reference",
                        )?
                    }
                    && {
                        let (plain, located) = scope.reference_members().storage_slices();
                        ctx.decode.contains(
                            plain,
                            &operation.opposite_angle_record_index,
                            "find F3D Draft scope reference",
                        )? || ctx.decode.any_by(
                            located,
                            |row| Ok(row.value == operation.opposite_angle_record_index),
                            "find F3D located Draft scope reference",
                        )?
                    }
                    && operation.angle_record_index != operation.opposite_angle_record_index
                    && operation.angle_offset > scope.paired_byte_offset()
                    && operation.opposite_angle_offset > operation.angle_offset
                    && ctx.decode.contains_key_hash_map(
                        records_by_index,
                        &(native_stream, operation.angle_record_index),
                        "find F3D Draft parameter header",
                    )?
                    && ctx.decode.contains_key_hash_map(
                        records_by_index,
                        &(native_stream, operation.opposite_angle_record_index),
                        "find F3D Draft parameter header",
                    )?
            }
        };
        let combine_link = match scope.combine_operation() {
            None => true,
            Some(operation) => {
                let mut selection_storage = ctx
                    .decode
                    .reserve_scoped(0, "hold F3D Combine selection indexes")?;
                let (plain, located) = scope.reference_members().storage_slices();
                let expected_selections = selection_storage.with_storage(|| {
                    ctx.decode.collect_btree_set(
                        ctx.decode
                            .admit_iter(plain, "scan F3D Combine expected selections")?
                            .chain(
                                ctx.decode
                                    .admit_iter(
                                        located,
                                        "scan F3D located Combine expected selections",
                                    )?
                                    .map(|row| &row.value),
                            )
                            .skip(1)
                            .step_by(2)
                            .copied(),
                        "index F3D Combine expected selections",
                    )
                })?;
                let selections = selection_storage.with_storage(|| {
                    ctx.decode.collect_vec(
                        std::iter::once(operation.target_record_index).chain(
                            std::iter::once(&operation.tools.first)
                                .chain(ctx.decode.admit_iter(
                                    &operation.tools.additional,
                                    "scan F3D Combine tool selections",
                                )?)
                                .map(|tool| tool.record_index),
                        ),
                        "collect F3D Combine selections",
                    )
                })?;
                let actual_selections = selection_storage.with_storage(|| {
                    ctx.decode.collect_btree_set(
                        selections.iter().copied(),
                        "index F3D Combine actual selections",
                    )
                })?;
                let valid_external =
                    |selection: &records::feature::combine::DesignCombineBodySelection| -> Result<bool, CodecError> {
                        let Some(identity) = selection.external_identity.as_ref() else {
                            return Ok(true);
                        };
                        let Some(header) = ctx.decode.get_hash_map(
                            records_by_index,
                            &(native_stream, selection.record_index),
                            "find F3D Combine selection header",
                        )? else {
                            return Ok(false);
                        };
                        Ok(header.byte_offset.checked_add(44) == Some(identity.selector_asset_id_offset()))
                    };
                let compact_scope = scope.class_tag.as_str() == "387"
                    && scope.paired_class_tag.as_str() == "258"
                    && design::decode::scopes::parameter_scope::parameter_scope_payload_length(
                        ctx.decode, scope,
                    )? == Some(314);
                let extended_reference_scope = scope.class_tag.as_str() == "329"
                    && scope.paired_class_tag.as_str() == "261"
                    && scope.frame_length() == 363;
                scope.reference_members().len() >= 4
                    && scope.reference_members().len().is_multiple_of(2)
                    && selections.len() == scope.reference_members().len() / 2
                    && actual_selections.len() == selections.len()
                    && actual_selections.len() == expected_selections.len()
                    && ctx.decode.is_subset_btree_set(
                        &actual_selections,
                        &expected_selections,
                        "compare F3D Combine selections",
                    )?
                    && valid_external(&operation.tools.first)?
                    && ctx.decode.all_by(
                        &operation.tools.additional,
                        &valid_external,
                        "check F3D Combine external selections",
                    )?
                    && match operation.form {
                        records::feature::combine::DesignCombineForm::Standard => {
                            !compact_scope
                                && !extended_reference_scope
                                && scope.byte_offset().checked_add(20).is_some_and(
                                    |expected_offset| operation.operation_offset == expected_offset,
                                )
                                && scope.byte_offset().checked_add(25).is_some_and(
                                    |expected_offset| {
                                        operation.keep_tools_offset == expected_offset
                                    },
                                )
                        }
                        records::feature::combine::DesignCombineForm::Compact => {
                            compact_scope
                                && scope.byte_offset().checked_add(21).is_some_and(
                                    |expected_offset| operation.operation_offset == expected_offset,
                                )
                                && scope.byte_offset().checked_add(25).is_some_and(
                                    |expected_offset| {
                                        operation.keep_tools_offset == expected_offset
                                    },
                                )
                        }
                        records::feature::combine::DesignCombineForm::ExtendedReference => {
                            extended_reference_scope
                                && scope.byte_offset().checked_add(31).is_some_and(
                                    |expected_offset| operation.operation_offset == expected_offset,
                                )
                                && scope.byte_offset().checked_add(30).is_some_and(
                                    |expected_offset| {
                                        operation.keep_tools_offset == expected_offset
                                    },
                                )
                        }
                    }
            }
        };
        let thread_link = match scope.thread_construction() {
            None => true,
            Some(construction) => {
                let (reference_values, located_reference_values) =
                    scope.reference_members().storage_slices();
                let mut expected_groups_storage = ctx
                    .decode
                    .reserve_scoped(0, "hold F3D thread expected groups")?;
                let expected_groups: Vec<_> =
                    expected_groups_storage.with_storage(|| -> Result<Vec<_>, CodecError> {
                        Ok(match construction.form {
                            records::feature::thread::DesignThreadForm::Standard
                            | records::feature::thread::DesignThreadForm::StandardLegacy => {
                                ctx.decode.collect_vec(
                                    reference_member_at(scope.reference_members(), 0).copied(),
                                    "collect F3D standard thread face groups",
                                )?
                            }
                            records::feature::thread::DesignThreadForm::Compact(_)
                            | records::feature::thread::DesignThreadForm::CompactLegacy => {
                                ctx.decode.collect_vec(
                                    ctx.decode
                                        .admit_iter(
                                            reference_values,
                                            "scan F3D compact thread references",
                                        )?
                                        .chain(
                                            ctx.decode
                                                .admit_iter(
                                                    located_reference_values,
                                                    "scan F3D compact thread located references",
                                                )?
                                                .map(|row| &row.value),
                                        )
                                        .step_by(2)
                                        .copied(),
                                    "collect F3D compact thread face groups",
                                )?
                            }
                        })
                    })?;
                scope.reference_members().len() >= 2
                    && scope.reference_members().len().is_multiple_of(2)
                    && match construction.form {
                        records::feature::thread::DesignThreadForm::StandardLegacy => {
                            scope.class_tag.as_str() == "334"
                                && scope.paired_class_tag.as_str() == "262"
                        }
                        records::feature::thread::DesignThreadForm::CompactLegacy => {
                            scope.class_tag.as_str() == "414"
                                && scope.paired_class_tag.as_str() == "263"
                        }
                        records::feature::thread::DesignThreadForm::Standard
                        | records::feature::thread::DesignThreadForm::Compact(_) => true,
                    }
                    && ctx.decode.equal(
                        &construction.face_group_record_indices,
                        &expected_groups,
                        "compare F3D thread expected groups",
                    )?
                    && matches!(
                        construction
                            .designation_offset
                            .checked_sub(scope.byte_offset()),
                        Some(38 | 42)
                    )
                    && match construction.form {
                        records::feature::thread::DesignThreadForm::Compact(Some(reference)) => {
                            reference.offset > construction.designation_offset
                                && reference.offset < scope.paired_byte_offset()
                                && ctx.decode.contains_key_hash_map(
                                    records_by_index,
                                    &(native_stream, reference.value.get()),
                                    "find F3D compact thread reference header",
                                )?
                        }
                        records::feature::thread::DesignThreadForm::Compact(None)
                        | records::feature::thread::DesignThreadForm::Standard
                        | records::feature::thread::DesignThreadForm::StandardLegacy
                        | records::feature::thread::DesignThreadForm::CompactLegacy => true,
                    }
                    && ctx.decode.all_by(
                        construction.face_group_record_indices.iter().enumerate(),
                        |(group_ordinal, record_index)| -> Result<bool, CodecError> {
                            let compact_member = if matches!(
                                construction.form,
                                records::feature::thread::DesignThreadForm::Compact(_)
                                    | records::feature::thread::DesignThreadForm::CompactLegacy
                            ) {
                                let Some(reference_ordinal) = group_ordinal.checked_mul(2) else {
                                    return Ok(false);
                                };
                                let Some(member_ordinal) = reference_ordinal.checked_add(1) else {
                                    return Ok(false);
                                };
                                let Some(member_record_index) =
                                    reference_member_at(scope.reference_members(), member_ordinal)
                                        .copied()
                                else {
                                    return Ok(false);
                                };
                                let Ok(scope_reference_ordinal) = u32::try_from(reference_ordinal)
                                else {
                                    return Ok(false);
                                };
                                Some((scope_reference_ordinal, member_record_index))
                            } else {
                                None
                            };
                            let mut found = false;
                            let duplicate = ctx.decode.any_by(
                                ctx.operand_group_records(
                                    native_stream,
                                    *record_index,
                                    "find F3D thread operand group records",
                                )?,
                                |group| {
                                    if group.scope_record_index == scope.record_index
                                        && group.role() == DesignOperandRole::ROLE_0X10
                                        && compact_member.is_none_or(
                                            |(scope_reference_ordinal, member_record_index)| {
                                                group.scope_reference_ordinal
                                                    == scope_reference_ordinal
                                                    && group
                                                        .members()
                                                        .iter()
                                                        .map(|member| member.value)
                                                        .eq([member_record_index])
                                            },
                                        )
                                    {
                                        if found {
                                            return Ok(true);
                                        }
                                        found = true;
                                    }

                                    Ok(false)
                                },
                                "find F3D thread operand groups",
                            )?;
                            Ok(found && !duplicate)
                        },
                        "validate F3D thread face groups",
                    )?
            }
        };
        let joint_origin_link = scope
            .joint_origin_frame()
            .map(|origin| -> Result<bool, CodecError> {
                let transform = origin.joint_origin_transform;
                let transform_offset = origin.joint_origin_transform_offset;
                let inline = match (scope.frame_length(), &origin.reference) {
                    (385, None) => Some(transform_offset) == scope.byte_offset().checked_add(49),
                    (336 | 347, Some(reference)) => {
                        let (plain, located) = scope.reference_members().storage_slices();
                        Some(transform_offset) == scope.byte_offset().checked_add(60)
                            && Some(reference.joint_origin_reference_offset)
                                == scope.byte_offset().checked_add(46)
                            && (ctx.decode.contains(
                                plain,
                                &reference.joint_origin_reference,
                                "find F3D joint origin scope reference",
                            )? || ctx.decode.any_by(
                                located,
                                |row| Ok(row.value == reference.joint_origin_reference),
                                "find F3D located joint origin scope reference",
                            )?)
                    }
                    _ => false,
                };
                let AssemblyScopeIndexes {
                    by_frame: assemblies_by_frame,
                    by_offset: assemblies_by_offset,
                } = match &mut assemblies {
                    Some(assemblies) => assemblies,
                    None => assemblies.insert(assembly_scope_indexes(ctx, &mut storage)?),
                };
                let assembly_operand = origin.reference.is_none()
                    && ctx.decode.any_by(
                        ctx.decode
                            .get_hash_map(
                                assemblies_by_frame,
                                &(native_stream, scope.record_index),
                                "find F3D joint origin assembly operands",
                            )?
                            .map_or(&[][..], Vec::as_slice),
                        |assembly| {
                            Ok(assembly.assembly_alignment().is_some_and(|alignment| {
                                alignment.operand_frames().is_some_and(|frames| {
                                    frames.iter().any(|frame| {
                                        frame.reference_record_index == scope.record_index
                                            && frame.transform == transform
                                            && frame.transform_offset == transform_offset
                                    })
                                })
                            }))
                        },
                        "find F3D joint origin assembly operand",
                    )?;
                let single_operand_assembly = origin
                    .reference
                    .as_ref()
                    .map(|reference| -> Result<bool, CodecError> {
                        let candidates = match transform_offset.checked_sub(36) {
                            Some(byte_offset) => ctx
                                .decode
                                .get_hash_map(
                                    assemblies_by_offset,
                                    &(native_stream, byte_offset),
                                    "find F3D single operand joint assemblies",
                                )?
                                .map_or(&[][..], Vec::as_slice),
                            None => &[],
                        };
                        ctx.decode.any_by(
                            candidates,
                            |assembly| {
                                let (plain, located) =
                                    assembly.reference_members().storage_slices();
                                Ok(assembly.class_tag.as_str() == "276"
                                    && assembly.paired_class_tag.as_str() == "258"
                                    && assembly.frame_length() == 604
                                    && Some(transform_offset)
                                        == assembly.byte_offset().checked_add(36)
                                    && (ctx.decode.contains(
                                        plain,
                                        &reference.joint_origin_reference,
                                        "find F3D single assembly joint reference",
                                    )? || ctx.decode.any_by(
                                        located,
                                        |row| Ok(row.value == reference.joint_origin_reference),
                                        "find F3D located single assembly joint reference",
                                    )?)
                                    && Some(reference.joint_origin_reference_offset)
                                        == assembly.byte_offset().checked_add(25))
                            },
                            "find F3D single operand joint assembly",
                        )
                    })
                    .transpose()?
                    .unwrap_or(false);
                Ok(inline || assembly_operand || single_operand_assembly)
            })
            .transpose()?
            .unwrap_or(true);
        let work_point_link = valid_work_point_construction(ctx, scope, native_stream)?;
        let work_plane_link = valid_work_plane_construction(ctx, scope, native_stream)?;
        let valid = match scope.extrude_prologue() {
            Some(records::feature::extrude::DesignExtrudePrologue::LegacyDistance {
                prefix_zero_offset,
                operation_offset,
                extent_kind_offset,
                direction_reversed_offset,
                solid_operation_offset,
                ..
            }) => {
                let marker_offset = scope.byte_offset().checked_add(20);
                let prefix_valid =
                    match prefix_zero_offset {
                        None => {
                            marker_offset
                                .and_then(|offset| offset.checked_add(1))
                                .is_some_and(|expected_offset| operation_offset == expected_offset)
                                && scope.byte_offset().checked_add(208).is_some_and(
                                    |expected_offset| {
                                        scope.reference_count_offset() == expected_offset
                                    },
                                )
                        }
                        Some(offset) => {
                            marker_offset
                                .and_then(|offset| offset.checked_add(1))
                                .is_some_and(|expected_offset| offset == expected_offset)
                                && offset.checked_add(4).is_some_and(|expected_offset| {
                                    operation_offset == expected_offset
                                })
                                && scope.byte_offset().checked_add(212).is_some_and(
                                    |expected_offset| {
                                        scope.reference_count_offset() == expected_offset
                                    },
                                )
                        }
                    };
                prefix_valid
                    && operation_offset
                        .checked_add(4)
                        .is_some_and(|expected_offset| extent_kind_offset == expected_offset)
                    && extent_kind_offset
                        .checked_add(4)
                        .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                    && direction_reversed_offset
                        .checked_add(1)
                        .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
            }
            Some(records::feature::extrude::DesignExtrudePrologue::ShiftedReferenceAware {
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed_offset,
                solid_operation_offset,
                start_offset,
                ..
            }) => {
                let expected_layout =
                    match (scope.class_tag.as_str(), scope.paired_class_tag.as_str()) {
                        ("357", "258") | ("275" | "361", "262") => Some((
                            538_u64,
                            292_u64,
                            13_usize,
                            [2, 1],
                            [2, 0],
                            records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces,
                            288_u64,
                        )),
                        ("349", "266") => Some((
                            538_u64,
                            292_u64,
                            13_usize,
                            [2, 1],
                            [2, 0],
                            records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces,
                            288_u64,
                        )),
                        ("323", "263")
                            if scope.byte_offset().checked_add(292).is_some_and(
                                |expected_offset| scope.reference_count_offset() == expected_offset,
                            ) =>
                        {
                            Some((
                                516_u64,
                                292_u64,
                                11_usize,
                                [2, 1],
                                [2, 0],
                                records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces,
                                288_u64,
                            ))
                        }
                        ("323", "263")
                            if scope.byte_offset().checked_add(272).is_some_and(
                                |expected_offset| scope.reference_count_offset() == expected_offset,
                            ) =>
                        {
                            Some((
                                485_u64,
                                272_u64,
                                10_usize,
                                [3, 0],
                                [4, 4],
                                records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll,
                                129_u64,
                            ))
                        }
                        _ => None,
                    };
                expected_layout.is_some_and(
                    |(
                        frame_length,
                        reference_count_offset,
                        reference_member_count,
                        expected_direction_face_extend_values,
                        expected_side_extent_discriminators,
                        expected_extent,
                        second_side_extent_offset,
                    )| {
                        scope.frame_length() == frame_length
                            && scope.byte_offset().checked_add(frame_length).is_some_and(
                                |expected_offset| scope.paired_byte_offset() == expected_offset,
                            )
                            && scope
                                .byte_offset()
                                .checked_add(reference_count_offset)
                                .is_some_and(|expected_offset| {
                                    scope.reference_count_offset() == expected_offset
                                })
                            && scope.reference_members().len() == reference_member_count
                            && scope
                                .byte_offset()
                                .checked_add(27)
                                .is_some_and(|expected_offset| operation_offset == expected_offset)
                            && direction_face_extend_values == expected_direction_face_extend_values
                            && side_extent_discriminators == expected_side_extent_discriminators
                            && extent == expected_extent
                            && scope
                                .byte_offset()
                                .checked_add(116)
                                .zip(scope.byte_offset().checked_add(second_side_extent_offset))
                                .map(|(first, second)| [first, second])
                                .is_some_and(|expected_offset| {
                                    side_extent_discriminator_offsets == expected_offset
                                })
                            && scope
                                .byte_offset()
                                .checked_add(31)
                                .zip(scope.byte_offset().checked_add(35))
                                .map(|(first, second)| [first, second])
                                .is_some_and(|expected_offset| {
                                    direction_face_extend_offsets == expected_offset
                                })
                            && scope
                                .byte_offset()
                                .checked_add(39)
                                .is_some_and(|expected_offset| {
                                    direction_reversed_offset == expected_offset
                                })
                            && scope
                                .byte_offset()
                                .checked_add(40)
                                .is_some_and(|expected_offset| {
                                    solid_operation_offset == expected_offset
                                })
                            && scope
                                .byte_offset()
                                .checked_add(41)
                                .is_some_and(|expected_offset| start_offset == expected_offset)
                    },
                )
            }
            Some(records::feature::extrude::DesignExtrudePrologue::ReferenceAware {
                reference,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                first_side_target_ordinal,
                extent,
                direction_face_extend_offsets,
                direction_reversed_offset,
                solid_operation_offset,
                start_offset,
                ..
            }) => {
                let prefix_valid = reference
                    .map(|reference| -> Result<bool, CodecError> {
                        let padding_end =
                            reference
                                .record_index_offset
                                .checked_add(4)
                                .and_then(|offset| {
                                    offset.checked_add(u64::from(reference.trailing_zero_count))
                                });
                        let marker_valid = match reference.operation_prefix_marker_offset {
                            None => Some(operation_offset) == padding_end,
                            Some(marker_offset) => {
                                Some(marker_offset) == padding_end
                                    && marker_offset.checked_add(1).is_some_and(|expected_offset| {
                                        operation_offset == expected_offset
                                    })
                            }
                        };
                        Ok(scope
                            .byte_offset()
                            .checked_add(26)
                            .is_some_and(|expected_offset| {
                                reference.record_index_offset == expected_offset
                            })
                            && matches!(reference.trailing_zero_count, 7 | 8)
                            && marker_valid
                            && {
                                let (plain, located) = scope.reference_members().storage_slices();
                                ctx.decode.contains(
                                    plain,
                                    &reference.record_index,
                                    "find F3D extent prefix reference",
                                )? || ctx.decode.any_by(
                                    located,
                                    |row| Ok(row.value == reference.record_index),
                                    "find F3D located extent prefix reference",
                                )?
                            })
                    })
                    .transpose()?
                    .unwrap_or(
                        scope
                            .byte_offset()
                            .checked_add(28)
                            .is_some_and(|expected_offset| operation_offset == expected_offset),
                    );
                let target_ordinal_valid = first_side_target_ordinal
                    .map(|target| -> Result<bool, CodecError> {
                        let Ok(ordinal) = usize::try_from(target.scope_reference_ordinal) else {
                            return Ok(false);
                        };
                        let Some(record_index) =
                            reference_member_at(scope.reference_members(), ordinal).copied()
                        else {
                            return Ok(false);
                        };
                        if target.scope_reference_ordinal_offset.checked_add(5)
                            != Some(side_extent_discriminator_offsets[0])
                        {
                            return Ok(false);
                        }
                        let mut found = false;
                        let duplicate = ctx.decode.any_by(
                            ctx.operand_group_records(
                                native_stream,
                                record_index,
                                "find F3D extent target operand group records",
                            )?,
                            |group| {
                                if group.scope_record_index == scope.record_index
                                    && group.scope_reference_ordinal
                                        == target.scope_reference_ordinal
                                    && group.role() == DesignOperandRole::ROLE_0X5
                                    && group.extrude_role().is_none()
                                {
                                    if found {
                                        return Ok(true);
                                    }
                                    found = true;
                                }

                                Ok(false)
                            },
                            "find F3D extent target operand groups",
                        )?;
                        Ok(found && !duplicate)
                    })
                    .transpose()?
                    .unwrap_or(true);
                let target_prefix_length = if first_side_target_ordinal.is_some() {
                    5
                } else {
                    0
                };
                let legacy_class_415_layout = scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_delta| {
                        legacy_class_415::is_symmetric_distance_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_delta,
                            scope.reference_members().len(),
                        )
                    });
                let legacy_class_415_one_sided_layout = scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_delta| {
                        legacy_class_415::is_one_sided_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_delta,
                            scope.reference_members().len(),
                        )
                    });
                let legacy_class_415_extent = legacy_class_415_layout
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                    && direction_face_extend_values == [3, 2]
                    && side_extent_discriminators == [1, 1]
                    && extent == records::feature::extrude::DesignExtrudeExtent::SymmetricDistance
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::FIRST_SIDE_EXTENT))
                        .zip(
                            scope
                                .byte_offset()
                                .checked_add(u64_from_index(class_415::SECOND_SIDE_EXTENT)),
                        )
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            side_extent_discriminator_offsets == expected_offset
                        })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::DIRECTION))
                        .zip(
                            scope
                                .byte_offset()
                                .checked_add(u64_from_index(class_415::FACE_EXTEND)),
                        )
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            direction_face_extend_offsets == expected_offset
                        })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::DIRECTION_REVERSED))
                        .is_some_and(|expected_offset| {
                            direction_reversed_offset == expected_offset
                        })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::GEOMETRY_KIND))
                        .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::START_SUPPORT))
                        .is_some_and(|expected_offset| start_offset == expected_offset);
                let first_side_offset_valid = operation_offset
                    .checked_add(49)
                    .and_then(|offset| offset.checked_add(target_prefix_length))
                    .and_then(|offset| side_extent_discriminator_offsets[0].checked_sub(offset))
                    .is_some_and(|slot_expansion| {
                        slot_expansion <= 70 && slot_expansion.is_multiple_of(10)
                    });
                let second_side_offset_valid = (if side_extent_discriminators[0] == 2 {
                    scope.reference_count_offset().checked_sub(4)
                } else {
                    side_extent_discriminator_offsets[0].checked_add(13)
                })
                .is_some_and(|expected_offset| {
                    side_extent_discriminator_offsets[1] == expected_offset
                }) || (legacy_class_415_one_sided_layout
                    && scope.reference_count_offset().checked_sub(4).is_some_and(
                        |expected_offset| side_extent_discriminator_offsets[1] == expected_offset,
                    ));
                let standard_extent = matches!(
                    (
                        direction_face_extend_values[0],
                        side_extent_discriminators,
                        extent,
                    ),
                    (
                        1,
                        [1, 0],
                        records::feature::extrude::DesignExtrudeExtent::OneSidedDistance
                    ) | (
                        1,
                        [2, 0],
                        records::feature::extrude::DesignExtrudeExtent::OneSidedToFace
                    ) | (
                        1,
                        [3, 0],
                        records::feature::extrude::DesignExtrudeExtent::OneSidedThroughNext
                    ) | (
                        1,
                        [4, 0],
                        records::feature::extrude::DesignExtrudeExtent::OneSidedThroughAll
                    ) | (
                        2,
                        [2, 0],
                        records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces
                    ) | (
                        2,
                        [1, 1],
                        records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance
                    ) | (
                        3,
                        [1, 0],
                        records::feature::extrude::DesignExtrudeExtent::SymmetricDistance
                    ) | (
                        3,
                        [4, 4],
                        records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll
                    )
                );
                prefix_valid
                    && matches!(direction_face_extend_values[0], 1..=3)
                    && (standard_extent || legacy_class_415_extent)
                    && first_side_target_ordinal.is_none_or(|_| side_extent_discriminators[0] == 2)
                    && target_ordinal_valid
                    && first_side_offset_valid
                    && second_side_offset_valid
                    && operation_offset
                        .checked_add(4)
                        .zip(operation_offset.checked_add(8))
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            direction_face_extend_offsets == expected_offset
                        })
                    && operation_offset
                        .checked_add(14)
                        .is_some_and(|expected_offset| start_offset == expected_offset)
                    && operation_offset
                        .checked_add(13)
                        .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
                    && operation_offset
                        .checked_add(12)
                        .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                    && side_extent_discriminator_offsets[1]
                        .checked_add(4)
                        .is_some_and(|end| end <= scope.reference_count_offset())
            }
            Some(records::feature::extrude::DesignExtrudePrologue::LegacyShifted {
                operation_prefix_marker_offset,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed_offset,
                solid_operation_offset,
                start_offset,
                ..
            }) => {
                let field_shift =
                    match operation_prefix_marker_offset {
                        None if scope
                            .byte_offset()
                            .checked_add(27)
                            .is_some_and(|expected_offset| operation_offset == expected_offset) =>
                        {
                            Some(0)
                        }
                        Some(marker_offset)
                            if scope.byte_offset().checked_add(27).is_some_and(
                                |expected_offset| marker_offset == expected_offset,
                            ) && marker_offset.checked_add(1).is_some_and(
                                |expected_offset| operation_offset == expected_offset,
                            ) =>
                        {
                            Some(1)
                        }
                        _ => None,
                    };
                let compact_extent_offsets = if operation_prefix_marker_offset.is_none()
                    && scope
                        .byte_offset()
                        .checked_add(26)
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    scope
                        .reference_count_offset()
                        .checked_sub(scope.byte_offset())
                        .and_then(|offset| match offset {
                            251 => scope
                                .byte_offset()
                                .checked_add(105)
                                .zip(scope.byte_offset().checked_add(109))
                                .map(|(first, second)| [first, second]),
                            281 => scope
                                .byte_offset()
                                .checked_add(124)
                                .zip(scope.byte_offset().checked_add(128))
                                .map(|(first, second)| [first, second]),
                            _ => None,
                        })
                } else {
                    None
                };
                let class_296_extent_offsets = if scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_offset| {
                        is_class_296_one_sided_to_face_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_to_face::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_to_face::FIRST_SIDE_EXTENT))
                        .zip(
                            scope
                                .byte_offset()
                                .checked_add(u64_from_index(class_296_to_face::SECOND_SIDE_EXTENT)),
                        )
                        .map(|(first, second)| [first, second])
                } else {
                    None
                };
                let class_296_symmetric_extent_offsets =
                    if scope
                        .reference_count_offset()
                        .checked_sub(scope.byte_offset())
                        .is_some_and(|reference_count_offset| {
                            is_class_296_symmetric_distance_layout(
                                scope.class_tag.as_str(),
                                scope.paired_class_tag.as_str(),
                                scope.frame_length(),
                                reference_count_offset,
                                scope.reference_members().len(),
                            )
                        })
                        && scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_symmetric::OPERATION))
                            .is_some_and(|expected_offset| operation_offset == expected_offset)
                    {
                        scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_symmetric::FIRST_SIDE_EXTENT))
                            .zip(scope.byte_offset().checked_add(u64_from_index(
                                class_296_symmetric::SECOND_SIDE_EXTENT,
                            )))
                            .map(|(first, second)| [first, second])
                    } else {
                        None
                    };
                let class_296_two_faces_extent_offsets =
                    if scope
                        .reference_count_offset()
                        .checked_sub(scope.byte_offset())
                        .is_some_and(|reference_count_offset| {
                            is_class_296_two_sided_to_faces_layout(
                                scope.class_tag.as_str(),
                                scope.paired_class_tag.as_str(),
                                scope.frame_length(),
                                reference_count_offset,
                                scope.reference_members().len(),
                            )
                        })
                        && scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_two_faces::OPERATION))
                            .is_some_and(|expected_offset| operation_offset == expected_offset)
                    {
                        scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_two_faces::FIRST_SIDE_EXTENT))
                            .zip(scope.byte_offset().checked_add(u64_from_index(
                                class_296_two_faces::SECOND_SIDE_EXTENT,
                            )))
                            .map(|(first, second)| [first, second])
                    } else {
                        None
                    };
                let class_296_legacy_to_face_extent_offsets = if scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_offset| {
                        is_class_296_legacy_one_sided_to_face_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::FIRST_SIDE_EXTENT))
                        .zip(scope.byte_offset().checked_add(u64_from_index(
                            class_296_legacy_to_face::SECOND_SIDE_EXTENT,
                        )))
                        .map(|(first, second)| [first, second])
                } else {
                    None
                };
                let class_296_legacy_distance_extent_offsets = if scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_offset| {
                        is_class_296_legacy_one_sided_distance_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::FIRST_SIDE_EXTENT))
                        .zip(scope.byte_offset().checked_add(u64_from_index(
                            class_296_legacy_distance::SECOND_SIDE_EXTENT,
                        )))
                        .map(|(first, second)| [first, second])
                } else {
                    None
                };
                let class_397_frame = scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .and_then(|reference_count_offset| {
                        Class397SymmetricFrame::new(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    });
                let extent_valid = if let Some(frame) = class_397_frame {
                    operation_prefix_marker_offset.is_none()
                        && scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_397::OPERATION))
                            .is_some_and(|expected_offset| operation_offset == expected_offset)
                        && direction_face_extend_values
                            == [class_397::DIRECTION_VALUE, class_397::FACE_EXTEND_VALUE]
                        && extent.is_some()
                        && extent
                            == frame
                                .extent(direction_face_extend_values[0], side_extent_discriminators)
                } else if compact_extent_offsets.is_some() {
                    matches!(
                        (
                            direction_face_extend_values,
                            side_extent_discriminators,
                            extent,
                        ),
                        (
                            [1, _],
                            [1, 0],
                            Some(records::feature::extrude::DesignExtrudeExtent::OneSidedDistance)
                        ) | (
                            [3, _],
                            [1, 0],
                            Some(records::feature::extrude::DesignExtrudeExtent::SymmetricDistance)
                        ) | (
                            [2, 0],
                            [1, 2],
                            Some(records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace)
                        )
                    )
                } else if class_296_extent_offsets.is_some() {
                    direction_face_extend_values[0] == 1
                        && matches!(direction_face_extend_values[1], 1 | 2)
                        && side_extent_discriminators == [2, 0]
                        && extent
                            == Some(records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                } else if class_296_symmetric_extent_offsets.is_some() {
                    direction_face_extend_values == [3, 2]
                        && side_extent_discriminators == [1, 0]
                        && extent
                            == Some(
                                records::feature::extrude::DesignExtrudeExtent::SymmetricDistance,
                            )
                } else if class_296_two_faces_extent_offsets.is_some() {
                    direction_face_extend_values[0] == 2
                        && matches!(direction_face_extend_values[1], 1 | 2)
                        && side_extent_discriminators == [2, 0]
                        && extent
                            == Some(records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces)
                } else if class_296_legacy_to_face_extent_offsets.is_some() {
                    direction_face_extend_values == [1, 1]
                        && side_extent_discriminators == [2, 0]
                        && extent
                            == Some(records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                } else if class_296_legacy_distance_extent_offsets.is_some() {
                    direction_face_extend_values == [1, 2]
                        && side_extent_discriminators == [1, 0]
                        && extent
                            == Some(
                                records::feature::extrude::DesignExtrudeExtent::OneSidedDistance,
                            )
                } else {
                    matches!(direction_face_extend_values[0], 1..=3)
                        && matches!(
                            (
                                direction_face_extend_values[0],
                                side_extent_discriminators,
                                extent,
                            ),
                            (
                                1,
                                [1, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::OneSidedDistance)
                            ) | (
                                1,
                                [2, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                            ) | (
                                1,
                                [3, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::OneSidedThroughNext),
                            ) | (
                                1,
                                [4, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::OneSidedThroughAll)
                            ) | (
                                2,
                                [1, 1],
                                Some(records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance)
                            ) | (
                                3,
                                [1, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::SymmetricDistance)
                            ) | (
                                3,
                                [4, 4],
                                Some(records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll)
                            )
                        )
                };
                let side_offsets_valid = if class_397_frame.is_some() {
                    scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_397::FIRST_SIDE_EXTENT))
                        .zip(
                            scope
                                .byte_offset()
                                .checked_add(u64_from_index(class_397::SECOND_SIDE_EXTENT)),
                        )
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            side_extent_discriminator_offsets == expected_offset
                        })
                } else {
                    compact_extent_offsets
                        .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_symmetric_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_two_faces_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_legacy_to_face_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_legacy_distance_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || field_shift.is_some_and(|field_shift| {
                            (if direction_face_extend_values[0] == 2 {
                                if scope
                                    .reference_count_offset()
                                    .checked_sub(scope.byte_offset())
                                    .and_then(|offset| offset.checked_sub(field_shift))
                                    == Some(283)
                                {
                                    scope
                                        .byte_offset()
                                        .checked_add(166 + field_shift)
                                        .zip(scope.byte_offset().checked_add(181 + field_shift))
                                        .map(|(first, second)| [first, second])
                                } else {
                                    scope
                                        .byte_offset()
                                        .checked_add(155 + field_shift)
                                        .zip(scope.byte_offset().checked_add(178 + field_shift))
                                        .map(|(first, second)| [first, second])
                                }
                            } else if side_extent_discriminators[0] == 2 {
                                let first_offset = side_extent_discriminator_offsets[0];
                                if matches!(
                                    first_offset
                                        .checked_sub(scope.byte_offset())
                                        .and_then(|offset| offset.checked_sub(field_shift)),
                                    Some(106 | 116)
                                ) {
                                    scope
                                        .reference_count_offset()
                                        .checked_sub(4)
                                        .map(|second| [first_offset, second])
                                } else {
                                    Some([0, 0])
                                }
                            } else if scope
                                .byte_offset()
                                .checked_add(116 + field_shift)
                                .zip(scope.byte_offset().checked_add(129 + field_shift))
                                .map(|(first, second)| [first, second])
                                .is_some_and(|expected_offset| {
                                    side_extent_discriminator_offsets == expected_offset
                                })
                            {
                                Some(side_extent_discriminator_offsets)
                            } else if scope
                                .byte_offset()
                                .checked_add(116 + field_shift)
                                .is_some_and(|expected_offset| {
                                    side_extent_discriminator_offsets[0] == expected_offset
                                })
                            {
                                scope
                                    .byte_offset()
                                    .checked_add(116 + field_shift)
                                    .zip(scope.byte_offset().checked_add(130 + field_shift))
                                    .map(|(first, second)| [first, second])
                            } else {
                                scope
                                    .byte_offset()
                                    .checked_add(106 + field_shift)
                                    .zip(scope.byte_offset().checked_add(110 + field_shift))
                                    .map(|(first, second)| [first, second])
                            })
                            .is_some_and(|expected_offset| {
                                side_extent_discriminator_offsets == expected_offset
                            })
                        })
                };
                (field_shift.is_some()
                    || compact_extent_offsets.is_some()
                    || class_296_extent_offsets.is_some()
                    || class_296_symmetric_extent_offsets.is_some()
                    || class_296_two_faces_extent_offsets.is_some()
                    || class_296_legacy_to_face_extent_offsets.is_some()
                    || class_296_legacy_distance_extent_offsets.is_some())
                    && extent_valid
                    && side_offsets_valid
                    && operation_offset
                        .checked_add(4)
                        .zip(operation_offset.checked_add(8))
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            direction_face_extend_offsets == expected_offset
                        })
                    && operation_offset
                        .checked_add(14)
                        .is_some_and(|expected_offset| start_offset == expected_offset)
                    && operation_offset
                        .checked_add(13)
                        .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
                    && operation_offset
                        .checked_add(12)
                        .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                    && direction_face_extend_offsets[1] < scope.reference_count_offset()
            }
            None => true,
        } && match &scope.payload() {
            records::feature::scope::DesignScopePayload::SurfaceStitch(operation) => {
                operation.gap_tolerance_offset > scope.paired_byte_offset()
                    && scope.reference_members().len() >= 4
                    && scope.reference_members().len().is_multiple_of(2)
                    && reference_member_at(
                        scope.reference_members(),
                        scope.reference_members().len() - 2,
                    ) == Some(&operation.tolerance_record_index)
                    && scope.reference_members().values().next_back()
                        == Some(&operation.settings_record_index)
            }
            _ => true,
        } && match &scope.payload() {
            records::feature::scope::DesignScopePayload::SurfaceRuled(operation) => {
                scope
                    .byte_offset()
                    .checked_add(20)
                    .is_some_and(|expected_offset| operation.method_offset == expected_offset)
                    && scope
                        .byte_offset()
                        .checked_add(27)
                        .is_some_and(|expected_offset| {
                            operation.alternate_face_offset == expected_offset
                        })
                    && scope
                        .byte_offset()
                        .checked_add(50)
                        .is_some_and(|expected_offset| operation.corner_offset == expected_offset)
                    && scope.reference_members().values().next()
                        == Some(&operation.distance_owner_record_index)
                    && reference_member_at(scope.reference_members(), 1)
                        == Some(&operation.angle_owner_record_index)
                    && operation.distance_owner_record_index != operation.angle_owner_record_index
                    && !operation.edge_group_record_indices.is_empty()
                    && {
                        let (references, _references_storage) = ctx.decode.with_scoped_storage(
                            "index F3D ruled surface references",
                            || {
                                ctx.decode.collect_hash_set(
                                    scope.reference_members().values().copied(),
                                    "index F3D ruled surface references",
                                )
                            },
                        )?;
                        ctx.decode.all_by(
                            &operation.edge_group_record_indices,
                            |record_index| {
                                ctx.decode.contains_hash_set(
                                    &references,
                                    record_index,
                                    "find F3D ruled surface edge reference",
                                )
                            },
                            "validate F3D ruled surface edge groups",
                        )?
                    }
                    && match operation.method {
                        records::feature::surface_ops::DesignRuledSurfaceMethod::Direction => {
                            operation.direction_entity_id.is_some()
                        }
                        records::feature::surface_ops::DesignRuledSurfaceMethod::Normal
                        | records::feature::surface_ops::DesignRuledSurfaceMethod::Tangent => {
                            operation.direction_entity_id.is_none()
                        }
                    }
            }
            _ => true,
        } && {
            let (plain, located) = scope.reference_members().storage_slices();
            ctx.decode.all_by(
                plain,
                |record_index| {
                    ctx.decode.contains_key_hash_map(
                        records_by_index,
                        &(native_stream, *record_index),
                        "find F3D scope reference header",
                    )
                },
                "validate F3D scope reference headers",
            )? && ctx.decode.all_by(
                located,
                |row| {
                    ctx.decode.contains_key_hash_map(
                        records_by_index,
                        &(native_stream, row.value),
                        "find F3D scope reference header",
                    )
                },
                "validate F3D located scope reference headers",
            )?
        } && ctx.decode.contains_key_hash_map(
            records_by_index,
            &(native_stream, scope.record_index),
            "find F3D parameter scope header",
        )? && entity_link
            .unwrap_or(scope.kind() != crate::records::feature::scope::DesignFeatureKind::Sketch)
            && extrude_profile_link
            && sweep_profile_link
            && base_flange_profile_link
            && base_flange_link
            && edge_flange_link
            && hem_link
            && copy_paste_link
            && rectangular_pattern_link
            && assembly_alignment_link
            && component_insert_link
            && copy_paste_component_link
            && draft_link
            && combine_link
            && thread_link
            && joint_origin_link
            && work_point_link
            && work_plane_link
            && (scope.kind() != crate::records::feature::scope::DesignFeatureKind::Sketch
                || ctx.decode.contains_key_hash_map(
                    placements_by_scope,
                    &(native_stream, scope.record_index),
                    "find F3D sketch scope placement",
                )?)
            && unique_index;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter scope has an invalid paired frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

fn valid_work_point_construction(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
) -> Result<bool, CodecError> {
    let Some(construction) = scope.work_point_construction() else {
        return Ok(true);
    };
    let native = ctx.native;
    let (plain, located) = scope.reference_members().storage_slices();
    if construction.point_record_byte_offset >= construction.position_offset
        || construction.position_offset >= construction.reference_type_offset
        || !(ctx.decode.contains(
            plain,
            &construction.point_record_index,
            "find F3D work-point scope reference",
        )? || ctx.decode.any_by(
            located,
            |value| Ok(value.value == construction.point_record_index),
            "find F3D located work-point scope reference",
        )?)
        || ctx
            .decode
            .get_hash_map(
                &ctx.records_by_index,
                &(native_stream, construction.point_record_index),
                "find F3D work-point record",
            )?
            .is_none_or(|header| header.byte_offset != construction.point_record_byte_offset)
    {
        return Ok(false);
    }

    ctx.decode.all_by(
        construction.rule.inputs(),
        |input| {
            let header = ctx.decode.get_hash_map(
                &ctx.records_by_index,
                &(native_stream, input.record_index()),
                "find F3D work-point input record",
            )?;
            Ok((ctx.decode.contains(
                plain,
                &input.record_index(),
                "find F3D work-point input reference",
            )? || ctx.decode.any_by(
                located,
                |value| Ok(value.value == input.record_index()),
                "find F3D located work-point input reference",
            )?) && input.reference_offset > construction.reference_type_offset
                && header.is_some() && match input.carrier() {
                None => true,
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::EdgeRecipe {
                        operand_id,
                    },
                ) => ctx.decode.any_by(
                    &native.design_edge_operands,
                    |operand| {
                        Ok(ctx.decode.equal(
                            &operand.id,
                            operand_id,
                            "compare F3D work-point edge recipe identity",
                        )? && ctx.decode.equal(
                            design_stream(&operand.id),
                            native_stream,
                            "compare F3D work-point edge recipe stream",
                        )? && operand.scope_record_index == scope.record_index
                            && operand.record_index() == input.record_index())
                    },
                    "find F3D work-point edge recipe",
                )?,
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::VertexRecipe {
                        recipe: vertex,
                    },
                ) => valid_vertex_recipe(ctx, scope, native_stream, input.record_index(), vertex)?,
                Some(records::feature::work_geometry::DesignWorkPointInputCarrier::WorkPlane {
                    selection,
                }) => {
                    (match header {
                        Some(header) => {
                            (header.class_tag == selection.class_tag)
                                && selection.asset_id_offset() > header.byte_offset
                        }
                        None => false,
                    }) && u32::try_from(selection.primary_identity)
                        .ok()
                        .and_then(|identity| identity.checked_add(1))
                        == Some(selection.work_plane_scope_record_index)
                        && ctx.decode.any_by(
                            &native.design_parameter_scopes,
                            |plane| {
                                Ok(ctx.decode.equal(
                                    design_stream(&plane.id),
                                    native_stream,
                                    "compare F3D work-point plane stream",
                                )? && (plane.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::WorkPlane)
                                    && plane.record_index
                                        == selection.work_plane_scope_record_index)
                            },
                            "find F3D work-point plane scope",
                        )?
                }
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::SketchPoint {
                        selection,
                    },
                ) => {
                    (match header {
                        Some(header) => {
                            (header.class_tag == selection.class_tag)
                                && selection.asset_id_offset() > header.byte_offset
                        }
                        None => false,
                    }) && u32::try_from(selection.point_persistent_id).is_ok()
                        && !ctx
                            .decode
                            .trim_text(
                                &selection.point_native_id,
                                "validate F3D work-point sketch identity",
                            )?
                            .is_empty()
                        && ctx.decode.any_by(
                            &native.sketch_points,
                            |point| {
                                Ok(ctx.decode.equal(
                                    &point.id,
                                    &selection.point_native_id,
                                    "compare F3D work-point sketch identity",
                                )? && ctx.decode.equal(
                                    design_stream(&point.id),
                                    native_stream,
                                    "compare F3D work-point sketch stream",
                                )? && point.owner_reference
                                    == Some(selection.sketch_record_index)
                                    && point.persistent_id() == Some(selection.point_persistent_id))
                            },
                            "find F3D work-point sketch point",
                        )?
                }
            })
        },
        "validate F3D work-point inputs",
    )
}

fn valid_work_plane_construction(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
) -> Result<bool, CodecError> {
    let Some(frame) = scope.work_plane_frame() else {
        return Ok(true);
    };
    let Some(construction) = &frame.work_plane_construction else {
        return Ok(true);
    };
    let placement_record_index = &construction.placement_record_index;
    let inputs = construction.inputs();
    let Some([placement, first, second, third, extra_offset]) =
        scope.reference_members().values_array()
    else {
        return Ok(false);
    };
    let Some(placement_header) = ctx.decode.get_hash_map(
        &ctx.records_by_index,
        &(native_stream, *placement_record_index),
        "find F3D work-plane placement record",
    )?
    else {
        return Ok(false);
    };
    let transform_offset = frame.work_plane_transform_offset;
    let Some(owner) = ctx.decode.find_by(
        ctx.owner_group(
            native_stream,
            *extra_offset,
            "find F3D work-plane owner records",
        )?,
        |owner| {
            Ok(owner.scope_record_index() == scope.record_index
                && owner.evaluated_value().get() == 0.0)
        },
        "find F3D work-plane parameter owner",
    )?
    else {
        return Ok(false);
    };

    Ok(placement == placement_record_index
        && [
            inputs[0].record_index(),
            inputs[1].record_index(),
            inputs[2].record_index(),
        ] == [*first, *second, *third]
        && scope.work_plane_reference() == Some(*extra_offset)
        && scope
            .work_plane_frame()
            .and_then(|frame| frame.reference.as_ref())
            .is_some()
        && transform_offset > placement_header.byte_offset
        && ctx.decode.all_by(
            inputs,
            |input| valid_vertex_recipe(ctx, scope, native_stream, input.record_index(), input),
            "validate F3D work-plane vertex inputs",
        )?
        && ctx.decode.any_by(
            ctx.parameter_group(
                native_stream,
                owner.parameter_record_index(),
                "find F3D work-plane parameter records",
            )?,
            |parameter| {
                Ok(parameter.owner_record_index() == Some(owner.record_index())
                    && ctx.decode.equal(
                        parameter.source_kind(),
                        "ExtraOffset",
                        "compare F3D work-plane parameter kind",
                    )?
                    && parameter.evaluated_value().get() == 0.0)
            },
            "find F3D work-plane ExtraOffset parameter",
        )?)
}

fn valid_component_pattern_occurrences(
    ctx: &Ctx<'_, '_>,
    stream: &str,
    instances: &records::feature::patterns::DesignRectangularPatternInstances,
) -> Result<bool, CodecError> {
    let records::feature::patterns::DesignRectangularPatternInstances::Components {
        component_guid,
        seed,
        generated,
    } = instances
    else {
        return Ok(true);
    };
    Ok(ctx.decode.any_by(
        ctx.occurrences_with_guid(
            stream,
            seed.occurrence_guid.as_str(),
            "find F3D pattern seed occurrence group",
        )?,
        |occurrence| {
            Ok(folded_guid(occurrence.component_guid.as_str())
                == folded_guid(component_guid.as_str())
                && matches!(
                    occurrence.placement(),
                    records::feature::assembly_features::DesignComponentOccurrencePlacement::Base
                ))
        },
        "find F3D pattern seed occurrence",
    )? && ctx.decode.all_by(
        generated.iter().enumerate(),
        |(ordinal, row)| {
            ctx.decode.any_by(
                ctx.occurrences_with_guid(
                    stream,
                    row.occurrence_guid.as_str(),
                    "find F3D generated component occurrence group",
                )?,
                |occurrence| {
                    Ok(folded_guid(occurrence.component_guid.as_str())
                        == folded_guid(component_guid.as_str())
                        && u32::try_from(ordinal)
                            .ok()
                            .and_then(|ordinal| ordinal.checked_add(2))
                            == Some(occurrence.occurrence_ordinal())
                        && occurrence.transform() == Some(row.instance.transform))
                },
                "find F3D generated component occurrence",
            )
        },
        "validate F3D generated component occurrences",
    )?)
}

#[cfg(test)]
mod tests;
