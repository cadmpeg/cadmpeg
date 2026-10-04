// SPDX-License-Identifier: Apache-2.0
//! Semantic validation of the Fusion `f3d` native namespace.
//!
//! [`validate_native_charged`] loads the `f3d` native namespace from a decoded
//! [`CadIr`] and checks the settled byte frames and cross-record relationships
//! of every Fusion Design record family: body maps and bounds, parameter
//! scopes and their feature operands, sketch geometry and relations, dimension
//! loci, persistent identity links, and the ASM history graph. It returns the
//! [`Finding`] values in a fixed emission order; callers append them to the
//! generic IR validation report.

use cadmpeg_core::convert::f64_from_index;

use crate::design::decode::scopes::extrude::is_class_296_legacy_one_sided_distance_layout;
use crate::design::decode::scopes::extrude::is_class_296_legacy_one_sided_to_face_layout;
use crate::design::decode::scopes::extrude::is_class_296_one_sided_to_face_layout;
use crate::design::decode::scopes::extrude::is_class_296_symmetric_distance_layout;
use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_layout;
use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_scope;
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
use crate::{design, history, ids, native, records};
use cadmpeg_core::decode::id_from_index;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::{
    check::{Check, Finding},
    Severity,
};

const EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E10: f64 = 1.0e-10;
const EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8: f64 = 1.0e-8;

/// Resolve the native design stream that owns a record `id`, defaulting to the
/// primary design stream when the id carries no stream qualifier.
fn design_stream(id: &str) -> &str {
    ids::native_stream(id).unwrap_or(ids::DEFAULT_STREAM)
}

/// Report whether a native `stream` scope contains the design `entry`, either
/// directly or through an `f3d:xref/` qualifier.
fn design_stream_contains_entry(stream: &str, entry: &str) -> bool {
    ids::native_scope_matches(stream, entry)
}

/// Admit the empty reference table used by a legacy Combine tool operand.
fn body_recipe_reference_table_is_admitted(
    decode: &DecodeContext<'_>,
    scope: Option<&records::feature::scope::DesignParameterScope>,
    operand: &records::topology::body_recipe::DesignBodyRecipeOperand,
) -> Result<bool, CodecError> {
    if !operand.references().is_empty() {
        return Ok(true);
    }
    if !matches!(operand.owner, records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }) {
        return Ok(false);
    }
    let Some(scope) = scope else {
        return Ok(false);
    };
    if !decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::Combine, "compare F3D body recipe owner kind")? {
        return Ok(false);
    }
    let Some(operation) = scope.combine_operation() else {
        return Ok(false);
    };
    Ok(decode.any_by(std::slice::from_ref(&operation.tools.first), |tool| Ok(tool.record_index == operand.record_index()), "find F3D body recipe Combine tool")?
        || decode.any_by(&operation.tools.additional, |tool| Ok(tool.record_index == operand.record_index()), "find F3D body recipe Combine tool")?)
}

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
    for (region_ordinal, region) in decode.admit_iter(&selection.regions, "scan F3D profile regions")?.enumerate() {
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
        for member in decode.admit_iter(&region.members, "scan F3D profile region members")? {
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
        }
    }
    Ok(cursor.checked_add(5) == Some(selection.companion_byte_offset))
}

fn design_header_matches(
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

fn valid_axial_selector_identity(
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

fn valid_axial_assembly_targets(
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

use crate::records::topology::extrude_selection::DesignOperandRole;
use std::collections::{HashMap, HashSet};

fn reload_native_arena<'ctx, T: serde::de::DeserializeOwned>(
    decode: &'ctx DecodeContext<'_>,
    ir: &CadIr,
    name: &str,
) -> Result<(Vec<T>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
    decode.with_scoped_storage("reload F3D validation records", || {
        let Some(namespace) = ir.native.namespace("f3d") else {
            return Ok(Vec::new());
        };
        namespace
            .arena_as_for_decode(decode, name)
            .map_err(Into::into)
    })
}

/// Read-only indexes over the loaded `f3d` native namespace, shared by the
/// per-family validators. Every map is derived purely from the namespace and
/// borrows it for the duration of a [`validate_native`] call.
struct Ctx<'a, 'd> {
    decode: &'a DecodeContext<'d>,
    /// The decoded document, for model-side body, face, and edge identity.
    ir: &'a CadIr,
    /// The loaded native namespace.
    native: &'a native::F3dNative,
    /// Design record headers keyed by `(stream, record_index)`.
    records_by_index: HashMap<(&'a str, u32), &'a records::decal::DesignRecordHeader>,
    /// Construction recipes keyed by recipe id.
    recipes_by_id: HashMap<&'a str, &'a records::recipes::ConstructionRecipe>,
    /// Parameters keyed by `(stream, record_index)`.
    parameters_by_index: HashMap<(&'a str, u32), &'a records::parameters::DesignParameter>,
    /// Parameter owners keyed by `(stream, record_index)`.
    owners_by_index: HashMap<(&'a str, u32), &'a records::parameters::DesignParameterOwner>,
    /// Parameter companions keyed by `(stream, record_index)`.
    companions_by_index: HashMap<(&'a str, u32), &'a records::parameters::DesignParameterCompanion>,
    /// Parameter scopes keyed by `(stream, record_index)`.
    scopes_by_index: HashMap<(&'a str, u32), &'a records::feature::scope::DesignParameterScope>,
    /// Entity headers keyed by `(stream, entity_suffix)`.
    entities_by_suffix: HashMap<(&'a str, u64), &'a records::entity_header::DesignEntityHeader>,
    /// Sketch geometry record indices keyed by `(stream, record_index)`.
    sketch_geometry_indices: HashSet<(&'a str, u32)>,
    /// Sketch placements keyed by `(stream, scope_record_index)`.
    placements_by_scope:
        HashMap<(&'a str, u32), &'a records::sketch_placement::DesignSketchPlacement>,
    /// Extrude selection groups keyed by `(stream, record_index)`.
    groups_by_index: HashMap<
        (&'a str, u32),
        &'a records::topology::extrude_selection::DesignExtrudeSelectionGroup,
    >,
    /// Construction operand groups keyed by `(stream, record_index)`.
    operand_groups_by_index: HashMap<
        (&'a str, u32),
        &'a records::topology::construction::DesignConstructionOperandGroup,
    >,
    /// Extrude selection members keyed by `(stream, group_record_index, ordinal)`.
    members_by_slot: HashMap<
        (&'a str, u32, u32),
        &'a records::topology::extrude_selection::DesignExtrudeSelectionMember,
    >,
    /// Sketch owner entity ids keyed by `(stream, suffix)`.
    sketch_owner_ids: HashMap<(&'a str, u32), &'a str>,
}

impl<'a, 'd> Ctx<'a, 'd> {
    fn push_constant_finding(
        &self,
        findings: &mut Vec<Finding>,
        check: Check,
        message: &'static str,
        entity: Option<String>,
    ) -> Result<(), CodecError> {
        self.decode.push_vec(
            findings,
            Finding {
                check,
                severity: Severity::Error,
                message: message.into(),
                entity,
            },
            "collect F3D native validation findings",
        )?;
        Ok(())
    }

    /// Build every shared index over `native` up front. All builds are pure and
    /// emit no findings, so their eager construction does not affect the
    /// observable finding order.
    fn new(
        ir: &'a CadIr,
        native: &'a native::F3dNative,
        decode: &'a DecodeContext<'d>,
    ) -> Result<Self, CodecError> {
        let records_by_index = decode.collect_hash_map(
            native
                .design_record_headers
                .iter()
                .map(|record| ((design_stream(&record.id), record.record_index), record)),
            "index F3D design headers",
        )?;
        let recipes_by_id = decode.collect_hash_map(
            native
                .construction_recipes
                .iter()
                .map(|recipe| (recipe.id.as_str(), recipe)),
            "index F3D construction recipes",
        )?;
        let parameters_by_index = decode.collect_hash_map(
            native.design_parameters.iter().map(|parameter| {
                (
                    (design_stream(&parameter.id), parameter.record_index),
                    parameter,
                )
            }),
            "index F3D design parameters",
        )?;
        let owners_by_index = decode.collect_hash_map(
            native
                .design_parameter_owners
                .iter()
                .map(|owner| ((design_stream(owner.id()), owner.record_index()), owner)),
            "index F3D parameter owners",
        )?;
        let companions_by_index = decode.collect_hash_map(
            native.design_parameter_companions.iter().map(|companion| {
                (
                    (design_stream(companion.id()), companion.record_index()),
                    companion,
                )
            }),
            "index F3D parameter companions",
        )?;
        let scopes_by_index = decode.collect_hash_map(
            native
                .design_parameter_scopes
                .iter()
                .map(|scope| ((design_stream(&scope.id), scope.record_index), scope)),
            "index F3D parameter scopes",
        )?;
        let entities_by_suffix = decode.collect_hash_map(
            native.design_entity_headers.iter().map(|entity| {
                (
                    (design_stream(&entity.id), entity.entity_id.suffix()),
                    entity,
                )
            }),
            "index F3D entity suffixes",
        )?;
        let sketch_geometry_indices = decode.collect_hash_set(
            decode.admit_iter(&native.sketch_points, "admit source for index F3D sketch geometry")?
                .map(|point| (design_stream(&point.id), point.record_index))
                .chain(
                    decode.admit_iter(&native.sketch_curve_identities, "admit source for index F3D sketch geometry")?
                        .map(|curve| (design_stream(&curve.id), curve.record_index)),
                ),
            "index F3D sketch geometry",
        )?;
        let placements_by_scope = decode.collect_hash_map(
            decode.admit_iter(&native.design_sketch_placements, "admit source for index F3D sketch placements")?
                .filter_map(|placement| {
                    Some((
                        (design_stream(&placement.id), placement.scope_record_index?),
                        placement,
                    ))
                }),
            "index F3D sketch placements",
        )?;
        let groups_by_index = decode.collect_hash_map(
            native
                .design_extrude_selection_groups
                .iter()
                .map(|group| ((design_stream(&group.id), group.record_index), group)),
            "index F3D extrude selection groups",
        )?;
        let operand_groups_by_index = decode.collect_hash_map(
            native
                .design_construction_operand_groups
                .iter()
                .map(|group| ((design_stream(&group.id), group.record_index), group)),
            "index F3D construction operand groups",
        )?;
        let members_by_slot = decode.collect_hash_map(
            native
                .design_extrude_selection_members
                .iter()
                .map(|member| {
                    (
                        (
                            design_stream(&member.id),
                            member.group_record_index,
                            member.group_member_ordinal,
                        ),
                        member,
                    )
                }),
            "index F3D extrude selection members",
        )?;
        let sketch_owner_ids = decode.collect_hash_map(
            decode.admit_iter(&native.design_entity_headers, "admit source for index F3D sketch owner ids")?
                .filter(|header| header.in_sketch_module())
                .filter_map(|header| {
                    Some((
                        (
                            design_stream(&header.id),
                            u32::try_from(header.entity_id.suffix()).ok()?,
                        ),
                        header.entity_id.as_str(),
                    ))
                }),
            "index F3D sketch owner ids",
        )?;
        Ok(Ctx {
            decode,
            ir,
            native,
            records_by_index,
            recipes_by_id,
            parameters_by_index,
            owners_by_index,
            companions_by_index,
            scopes_by_index,
            entities_by_suffix,
            sketch_geometry_indices,
            placements_by_scope,
            groups_by_index,
            operand_groups_by_index,
            members_by_slot,
            sketch_owner_ids,
        })
    }
}

/// Validate native records using the source decode budget.
pub(crate) fn validate_native_charged(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Vec<Finding>, CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(Vec::new());
    };
    let (native, _native_storage) = match decode
        .with_scoped_storage("load F3D validation records", || {
            native::F3dNative::load_charged(decode, namespace)
        }) {
        Ok(native) => native,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return Ok(vec![Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion native namespace does not match the expected arena shape".into(),
                entity: None,
            }]);
        }
    };
    validate_loaded(decode, ir, &native)
}

fn validate_loaded(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
    native: &native::F3dNative,
) -> Result<Vec<Finding>, CodecError> {
    let ctx = Ctx::new(ir, native, decode)?;
    let mut findings = Vec::new();
    let (mut expected_face_operands, _expected_face_operands_storage) =
        reload_native_arena(decode, ir, "design_face_operands")?;
    let scope_histories = history::bind_scope_histories(
        decode,
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    )?;
    history::bind_face_operand_history_candidates(
        decode,
        &mut expected_face_operands,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.construction_recipes,
        &native.asm_histories,
        &scope_histories,
    )?;
    let decoded_profile_face_groups = decode.collect_hash_set(
        decode.admit_iter(&native.design_face_operands, "admit source for index F3D decoded profile face groups")?.filter_map(|operand| {
            Some((design_stream(&operand.id), operand.group_record_index()?))
        }),
        "index F3D decoded profile face groups",
    )?;
    let mut face_group_members_storage = decode.reserve_scoped(0, "index F3D face group members")?;
    let mut face_group_members = HashSet::new();
    for group in decode.admit_iter(&native.design_construction_operand_groups, "scan F3D face groups")? {
        let selected = group.extrude_role().is_some_and(|role| {
            matches!(role, records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_))
        }) || (group.extrude_role()
            == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Profile)
            && decode.contains_hash_set(&decoded_profile_face_groups,
                &(design_stream(&group.id), group.record_index), "find F3D profile face group")?);
        if !selected { continue; }
        let native_stream = design_stream(&group.id);
        for member in decode.admit_iter(group.members(), "scan F3D face group members")? {
            face_group_members_storage.with_storage(|| decode.insert_hash_set(
                &mut face_group_members, (native_stream, group.scope_record_index, member.value),
                "index F3D face group members"))?;
        }
    }
    validate_act(&ctx, &mut findings)?;
    validate_body_bindings(&ctx, &mut findings)?;
    validate_body_bounds(&ctx, &mut findings)?;
    validate_canvas_images(&ctx, &mut findings)?;
    validate_decal_images(&ctx, &mut findings)?;
    validate_mesh_features(&ctx, &mut findings)?;
    validate_component_occurrences(&ctx, &mut findings)?;
    validate_configurations(&ctx, &mut findings)?;
    validate_feature_timelines(&ctx, &mut findings)?;
    validate_parameter_scopes(&ctx, &mut findings)?;
    validate_extrude_selection_groups(&ctx, &mut findings)?;
    validate_construction_operand_groups(&ctx, &mut findings)?;
    validate_path_feature_operand_roles(&ctx, &mut findings)?;
    validate_extrude_parameter_operands(&ctx, &mut findings)?;
    let fillet_radius_group_records = validate_fillet_radius_groups(&ctx, &mut findings)?;
    validate_fillet_operand_groups(&ctx, &mut findings, &fillet_radius_group_records)?;
    let operand_identity_groups = validate_construction_operand_identities(&ctx, &mut findings)?;
    let edge_identity_records =
        validate_edge_identity_operands(decode, &ctx, &mut findings, &expected_face_operands)?;
    let body_recipe_operand_records = validate_body_recipe_operands(decode, &ctx, &mut findings)?;
    let edge_operand_records = validate_edge_operands(decode, &ctx, &mut findings)?;
    let edge_treatment_vertex_records =
        validate_edge_treatment_vertex_operands(decode, &ctx, &mut findings)?;
    validate_operand_group_carriers(
        &ctx,
        &mut findings,
        &operand_identity_groups,
        &edge_identity_records,
        &body_recipe_operand_records,
        &edge_operand_records,
        &edge_treatment_vertex_records,
    )?;
    validate_extrude_selection_members(&ctx, &mut findings)?;
    validate_entity_selection_operands(&ctx, &mut findings)?;
    validate_extrude_selection_group_members(&ctx, &mut findings)?;
    validate_edge_treatment_groups(
        &ctx,
        &mut findings,
        &edge_operand_records,
        &edge_identity_records,
        &edge_treatment_vertex_records,
    )?;
    let face_operand_records =
        validate_face_operands(&ctx, &mut findings, &expected_face_operands)?;
    validate_face_group_member_resolution(
        &ctx,
        &mut findings,
        face_group_members,
        &face_operand_records,
        &native.design_entity_selection_operands,
    )?;
    validate_face_source_groups(&ctx, &mut findings)?;
    validate_sketch_placements(&ctx, &mut findings)?;
    validate_parameter_owners(&ctx, &mut findings)?;
    validate_parameter_companions(&ctx, &mut findings)?;
    let dimension_recipe_ids = validate_dimension_recipe_records(&ctx, &mut findings)?;
    validate_dimension_companion_recipes(&ctx, &mut findings, &dimension_recipe_ids)?;
    let locus_pair_companions = validate_dimension_locus_pairs(&ctx, &mut findings)?;
    validate_dimension_annotation_frames(&ctx, &mut findings)?;
    validate_dimension_presentation_frames(&ctx, &mut findings)?;
    let locus_group_companions = validate_dimension_locus_groups(&ctx, &mut findings)?;
    validate_dimension_null_locus_pairs(
        &ctx,
        &mut findings,
        &locus_pair_companions,
        &locus_group_companions,
    )?;
    validate_parameters(&ctx, &mut findings)?;
    validate_entity_headers(&ctx, &mut findings)?;
    validate_sketch_relations(&ctx, &mut findings)?;
    validate_sketch_geometry_identities(&ctx, &mut findings)?;
    validate_sketch_relation_owners(decode, &ctx, &mut findings)?;
    validate_body_links(&ctx, &mut findings)?;
    validate_subentity_tags(&ctx, &mut findings)?;
    validate_history_graphs(decode, &ctx, &mut findings)?;
    Ok(findings)
}

/// Validate ACT record identity, table/group joins, ordered registries, and the
/// stored document-root discriminator.
fn validate_act(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut stream_indexes_storage = ctx.decode.reserve_scoped(0, "index F3D ACT validation streams")?;
    let mut streams = std::collections::BTreeMap::<&str, &str>::new();
    let mut record_indices = HashSet::new();
    for entity in ctx.decode.admit_iter(&native.act_entities, "scan F3D act entities")? {
        let stream = entity.stream();
        stream_indexes_storage.with_storage(|| ctx.decode.entry_btree_map(
            &mut streams, stream, "index F3D ACT streams"))?.or_insert(entity.id().as_str());
        let unique_index = ctx.decode.insert_hash_set(
            &mut record_indices,
            (stream, entity.record_index()),
            "index F3D ACT record indices",
        )?;
        if !unique_index {
            ctx.push_constant_finding(findings, Check::NativeLinks,
                "Fusion ACT entity has an invalid identity, table membership, or change-group frame",
                Some(ctx.decode.copy_retained_text(entity.id(), "retain F3D validation entity")?))?;
        }
    }

    let mut guid_ordinals = std::collections::BTreeMap::<&str, (HashSet<u32>, &str)>::new();
    let mut guid_offsets = HashSet::new();
    for guid in ctx.decode.admit_iter(&native.act_guids, "scan F3D act guids")? {
        let stream = guid.stream();
        stream_indexes_storage.with_storage(|| ctx.decode.entry_btree_map(
            &mut streams, stream, "index F3D ACT streams"))?.or_insert(guid.id().as_str());
        stream_indexes_storage.with_storage(|| ctx.decode.entry_btree_map(
            &mut guid_ordinals, stream, "index F3D ACT GUID streams"))?
            .or_insert((HashSet::new(), guid.id().as_str()));
        let unique_ordinal = ctx.decode.insert_hash_set(
            &mut ctx.decode.get_mut_btree_map(&mut guid_ordinals, stream, "find F3D ACT GUID stream ordinals")?
                .ok_or_else(|| CodecError::malformed("F3D ACT GUID stream index missing"))?
                .0,
            guid.ordinal,
            "index F3D ACT GUID ordinals",
        )?;
        let unique_offset = ctx.decode.insert_hash_set(
            &mut guid_offsets,
            (stream, guid.byte_offset()),
            "index F3D ACT GUID offsets",
        )?;
        let valid = unique_offset && unique_ordinal;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT GUID-pool entry has an invalid identity, ordinal, offset, or GUID",
                Some(
                    ctx.decode
                        .copy_retained_text(guid.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }

    let mut table_reference_ordinals =
        std::collections::BTreeMap::<&str, (HashSet<u32>, &str)>::new();
    let mut table_reference_offsets = HashSet::new();
    for reference in ctx.decode.admit_iter(&native.act_table_references, "scan F3D act table references")? {
        let stream = reference.stream();
        stream_indexes_storage.with_storage(|| ctx.decode.entry_btree_map(
            &mut streams, stream, "index F3D ACT streams"))?.or_insert(reference.id().as_str());
        stream_indexes_storage.with_storage(|| ctx.decode.entry_btree_map(
            &mut table_reference_ordinals, stream, "index F3D ACT table streams"))?
            .or_insert((HashSet::new(), reference.id().as_str()));
        let unique_ordinal = ctx.decode.insert_hash_set(
            &mut ctx.decode.get_mut_btree_map(&mut table_reference_ordinals, stream, "find F3D ACT table stream ordinals")?
                .ok_or_else(|| CodecError::malformed("F3D ACT table stream index missing"))?
                .0,
            reference.ordinal,
            "index F3D ACT table ordinals",
        )?;
        let unique_offset = ctx.decode.insert_hash_set(
            &mut table_reference_offsets,
            (stream, reference.byte_offset()),
            "index F3D ACT table offsets",
        )?;
        let valid = unique_ordinal && unique_offset;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT table reference has an invalid identity, ordinal, or offset",
                Some(
                    ctx.decode
                        .copy_retained_text(reference.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }

    let mut registry_ordinals = std::collections::BTreeMap::<&str, (HashSet<u32>, &str)>::new();
    let mut registry_offsets = HashSet::new();
    let mut registry_names = HashSet::new();
    for channel in ctx.decode.admit_iter(&native.act_registry_channels, "scan F3D act registry channels")? {
        let stream = channel.stream();
        stream_indexes_storage.with_storage(|| ctx.decode.entry_btree_map(
            &mut streams, stream, "index F3D ACT streams"))?.or_insert(channel.id().as_str());
        stream_indexes_storage.with_storage(|| ctx.decode.entry_btree_map(
            &mut registry_ordinals, stream, "index F3D ACT registry streams"))?
            .or_insert((HashSet::new(), channel.id().as_str()));
        let unique_ordinal = ctx.decode.insert_hash_set(
            &mut ctx.decode.get_mut_btree_map(&mut registry_ordinals, stream, "find F3D ACT registry stream ordinals")?
                .ok_or_else(|| CodecError::malformed("F3D ACT registry stream index missing"))?
                .0,
            channel.ordinal,
            "index F3D ACT registry ordinals",
        )?;
        let unique_offset = ctx.decode.insert_hash_set(
            &mut registry_offsets,
            (stream, channel.byte_offset()),
            "index F3D ACT registry offsets",
        )?;
        let unique_name = ctx.decode.insert_hash_set(
            &mut registry_names,
            (stream, channel.name()),
            "index F3D ACT registry names",
        )?;
        let valid = unique_offset && unique_name && unique_ordinal;
        if !valid {
            ctx.push_constant_finding(findings, Check::NativeLinks,
                "Fusion ACT channel-registry entry has an invalid identity, ordinal, offset, name, or GUID",
                Some(ctx.decode.copy_retained_text(channel.id(), "retain F3D validation entity")?))?;
        }
    }

    let mut root_counts = HashMap::<&str, usize>::new();
    for root in ctx.decode.admit_iter(&native.act_root_components, "scan F3D act root components")? {
        let stream = root.stream();
        stream_indexes_storage.with_storage(|| ctx.decode.entry_btree_map(
            &mut streams, stream, "index F3D ACT streams"))?.or_insert(root.id().as_str());
        let root_count = stream_indexes_storage.with_storage(|| ctx.decode.entry_hash_map(
            &mut root_counts, stream, "index F3D ACT root counts"))?.or_default();
        *root_count = root_count.checked_add(1).ok_or_else(||
            ctx.decode.refuse_codec_limit("count F3D ACT roots", u64::MAX - 1, u64::MAX))?;
        let unique_record_index = ctx.decode.insert_hash_set(
            &mut record_indices,
            (stream, root.record_index),
            "index F3D ACT record indices",
        )?;
        if !unique_record_index {
            ctx.push_constant_finding(findings, Check::NativeLinks,
                "Fusion ACT root component has an invalid identity, frame, or tracked-entity reference",
                Some(ctx.decode.copy_retained_text(root.id(), "retain F3D validation entity")?))?;
        }
    }

    for (stream, witness) in ctx.decode.admit_iter(&streams, "scan F3D ACT stream witnesses")?.map(|(stream, witness)| (*stream, *witness)) {
        if ctx.decode.get_hash_map(&root_counts, stream, "find F3D ACT root count")?.copied() != Some(1) {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT stream does not have exactly one document-root component link",
                Some(
                    ctx.decode
                        .copy_retained_text(witness, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    for (ordinals, witness, family) in ctx.decode.admit_iter(&guid_ordinals, "scan F3D ACT GUID ordinal sets")?
        .map(|(_, (ordinals, witness))| (ordinals, *witness, "GUID pool"))
        .chain(ctx.decode.admit_iter(&table_reference_ordinals, "scan F3D ACT table ordinal sets")?
            .map(|(_, (ordinals, witness))| (ordinals, *witness, "table reference")))
        .chain(ctx.decode.admit_iter(&registry_ordinals, "scan F3D ACT registry ordinal sets")?
            .map(|(_, (ordinals, witness))| (ordinals, *witness, "channel registry")))
    {
        let contiguous = match id_from_index(ordinals.len()) {
            Some(length) => {
                let mut maximum: Option<&u32> = None;
                for ordinal in ctx.decode.admit_iter(ordinals, "scan F3D ACT ordinal maximum")? {
                    if let Some(previous) = maximum {
                        if ctx.decode.compare(previous, ordinal, "compare F3D ACT ordinal maximum")?
                            == std::cmp::Ordering::Greater { continue; }
                    }
                    maximum = Some(ordinal);
                }
                maximum.and_then(|maximum| maximum.checked_add(1)) == Some(length)
            }
            None => false,
        };
        if !contiguous {
            let message = match family {
                "GUID pool" => "Fusion ACT GUID pool ordinals are not contiguous from zero",
                "table reference" => {
                    "Fusion ACT table reference ordinals are not contiguous from zero"
                }
                _ => "Fusion ACT channel registry ordinals are not contiguous from zero",
            };
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                message,
                Some(
                    ctx.decode
                        .copy_retained_text(witness, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate unique configuration entries and a single authored table.
fn validate_configurations(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut entry_names_storage = ctx.decode.reserve_scoped(0, "index F3D configuration entry names")?;
    let mut entry_names = HashSet::new();
    for configuration in ctx.decode.admit_iter(&ctx.native.design_configurations, "scan F3D design configurations")? {
        let name = configuration.entry_name().as_str();
        if !entry_names_storage.with_storage(|| ctx.decode.insert_hash_set(&mut entry_names, name, "index F3D configuration entry names"))? {
            let id = {
                let decode = ctx.decode;
                configuration.id_charged(decode)?
            };
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design configuration entry name is duplicated",
                Some(id),
            )?;
        }
    }
    let mut nonempty_tables = ctx
        .native
        .design_configurations
        .iter()
        .filter(|configuration| !configuration.variants().is_empty());
    let first = nonempty_tables.next();
    if nonempty_tables.next().is_some() {
        let id = first
            .map(|table| {
                let decode = ctx.decode;
                table.id_charged(decode)
            })
            .transpose()?;
        ctx.push_constant_finding(
            findings,
            Check::NativeLinks,
            "Fusion Design configurations have no single authored table order",
            id,
        )?;
    }
    Ok(())
}

/// Validate authored Design timeline order and its exact type and scope joins.
fn validate_feature_timelines(ctx: &Ctx, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut timeline_indexes_storage = ctx.decode.reserve_scoped(0, "hold F3D feature timeline indexes")?;
    let mut type_ordinals = HashMap::<&str, u32>::new();
    let mut timeline_ordinals = HashMap::<&str, u32>::new();
    let mut entity_type_counts = HashMap::<(&str, u64), usize>::new();
    let mut expected = HashMap::<(&str, u64), (String, u32, bool, &str)>::new();
    let mut design_types = ctx.decode.collect_vec(
        native.design_types.iter(),
        "order F3D feature timeline types",
    )?;
    ctx.decode.stable_sort_by_key(
        &mut design_types,
        |value| {
            (
                ids::native_stream(value.id()).unwrap_or_default(),
                value.byte_offset,
            )
        },
        Ord::cmp,
        "f3d feature timeline types sort",
    )?;
    for design_type in ctx.decode.admit_iter(&design_types, "scan F3D design types")? {
        let Some(meta_stream) = ids::native_stream(design_type.id()) else {
            continue;
        };
        let Some(segment) = ids::design_segment(design_type.id()) else {
            continue;
        };
        let type_ordinal = timeline_indexes_storage.with_storage(|| ctx.decode.entry_hash_map(
            &mut type_ordinals, meta_stream, "index F3D feature timeline type ordinals"))?.or_default();
        let class_tag = type_ordinal.checked_add(256).map(|tag| tag.to_string());
        *type_ordinal = type_ordinal.checked_add(1).ok_or_else(|| {
            CodecError::Malformed("F3D feature timeline type ordinal overflows".into())
        })?;
        let (entity_ids, located_entity_ids) = design_type.entities.storage_slices();
        for entity_id in ctx.decode.admit_iter(entity_ids, "scan F3D timeline entity IDs")?
            .chain(ctx.decode.admit_iter(located_entity_ids, "scan F3D timeline located entity IDs")?.map(|row| &row.value)) {
            let count = timeline_indexes_storage.with_storage(|| ctx.decode.entry_hash_map(
                &mut entity_type_counts, (segment, *entity_id), "index F3D feature timeline entity types"))?.or_default();
            *count = count.checked_add(1).ok_or_else(|| ctx.decode.refuse_codec_limit("count F3D feature timeline entity types", u64::MAX - 1, u64::MAX))?;
        }
        if !ctx.decode.eq_ignore_ascii_case(design_type.type_guid.as_str(), crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID, "match F3D timeline type GUID")?
        {
            continue;
        }
        let source_ordinal = timeline_indexes_storage.with_storage(|| ctx.decode.entry_hash_map(
            &mut timeline_ordinals, segment, "index F3D feature timeline source ordinals"))?.or_default();
        for entity_id in ctx.decode.admit_iter(entity_ids, "scan F3D timeline entity IDs")?
            .chain(ctx.decode.admit_iter(located_entity_ids, "scan F3D timeline located entity IDs")?.map(|row| &row.value)) {
            let valid_type =
                crate::design::decode::meta::is_supported_feature_timeline_type(design_type)
                    && match class_tag.as_ref() {
                        Some(tag) => {
                            let mut validation_storage = ctx.decode.reserve_scoped(0, "hold F3D timeline class tag validation")?;
                            let valid = records::references::DesignClassTag::try_from(validation_storage.with_storage(|| ctx.decode.copy_retained_text(tag, "copy F3D timeline type class tag"))?).is_ok();
                            valid
                        }
                        None => false,
                    };
            let class_tag = match class_tag.as_ref() {
                Some(tag) => timeline_indexes_storage.with_storage(|| ctx.decode.copy_retained_text(tag, "copy F3D expected timeline class tag"))?,
                None => continue,
            };
            if timeline_indexes_storage.with_storage(|| ctx.decode.insert_hash_map(
                &mut expected,
                (segment, *entity_id),
                (class_tag, *source_ordinal, valid_type, design_type.id().as_str()),
                "index F3D expected feature timelines",
            ))?.is_some() {
                ctx.push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design feature-timeline type repeats an entity identity",
                    Some(
                        ctx.decode
                            .copy_retained_text(design_type.id(), "retain F3D validation entity")?,
                    ),
                )?;
            }
            *source_ordinal = source_ordinal.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("F3D feature timeline source ordinal overflows".into())
            })?;
        }
    }

    let mut actual = ctx.decode.collect_vec(
        native.design_feature_timelines.iter(),
        "order F3D feature timeline records",
    )?;
    ctx.decode.stable_sort_by_key(
        &mut actual,
        |value| (value.segment(), value.source_ordinal),
        Ord::cmp,
        "f3d feature timeline records sort",
    )?;
    let mut actual_records = HashSet::<(&str, u64)>::new();
    let mut item_records = HashSet::<(&str, u64)>::new();
    for timeline in ctx.decode.admit_iter(&actual, "scan F3D feature timeline records")? {
        let segment = timeline.segment();
        let expected_type = ctx.decode.get_hash_map(&expected, &(segment, timeline.record_index.get()), "find F3D expected timeline type")?;
        let unique_record = ctx.decode.insert_hash_set(
            &mut actual_records,
            (segment, timeline.record_index.get()),
            "index F3D feature timeline record identities",
        )?;
        let record_valid = match expected_type {
            Some((class_tag, source_ordinal, valid_type, _)) => *valid_type
                && ctx.decode.equal(timeline.class_tag.as_str(), class_tag.as_str(), "compare F3D timeline class tag")?
                && timeline.source_ordinal == *source_ordinal,
            None => false,
        } && ctx.decode.get_hash_map(&entity_type_counts, &(segment, timeline.record_index.get()), "find F3D timeline record type count")? == Some(&1)
            && ctx.decode.get_hash_map(&entity_type_counts, &(segment, timeline.context_record_index.get()), "find F3D timeline context type count")? == Some(&1)
            && unique_record;
        let mut items_valid = true;
        for item in ctx.decode.admit_iter(timeline.frame().items(), "scan F3D timeline items")?.map(|item| item.value) {
            items_valid &= ctx.decode.get_hash_map(&entity_type_counts, &(segment, item), "find F3D timeline item type count")? == Some(&1)
                && ctx.decode.insert_hash_set(
                    &mut item_records,
                    (segment, item),
                    "index F3D feature timeline item identities",
                )?;
        }
        if !record_valid || !items_valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design feature timeline has an invalid typed frame",
                Some(
                    ctx.decode
                        .copy_retained_text(timeline.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    for ((segment, entity_id), (_, _, _, type_id)) in ctx.decode.admit_iter(&expected, "scan F3D expected feature timelines")? {
        if !ctx.decode.contains_hash_set(&actual_records, &(*segment, *entity_id), "find F3D actual timeline record")? {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design feature-timeline type has no decoded record",
                Some(
                    ctx.decode
                        .copy_retained_text(type_id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }

    let mut scope_positions = HashMap::<&str, u64>::new();
    match crate::design::feature_project::authored_scope_ordinals_per_stream(
        ctx.decode,
        &native.design_parameter_scopes,
        &native.design_feature_timelines,
    ) {
        Ok(authored) => {
            for scope in ctx.decode.admit_iter(&native.design_parameter_scopes, "scan F3D design parameter scopes")? {
                let stream = design_stream(&scope.id);
                let Some(position) = ctx.decode.get_hash_map(&authored, &(stream, scope.record_index), "find F3D authored scope ordinal")? else {
                    continue;
                };
                timeline_indexes_storage.with_storage(|| ctx.decode.insert_hash_map(
                    &mut scope_positions, scope.id.as_str(), *position, "index F3D feature timeline scope positions"))?;
            }
        }
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            let entity = native
                .design_parameter_scopes
                .first()
                .map(|scope| {
                    ctx.decode
                        .copy_retained_text(&scope.id, "retain F3D validation entity")
                })
                .transpose()?;
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design scopes have no complete authored order",
                entity,
            )?;
        }
    }

    let scope_history = crate::design::feature_project::ScopeHistoryGraph::new(
        ctx.decode,
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.design_component_naming_spaces,
        &native.asm_histories,
    )?;
    for scope in ctx.decode.admit_iter(&native.design_parameter_scopes, "scan F3D design parameter scopes")? {
        let Some(position) = ctx.decode.get_hash_map(&scope_positions, scope.id.as_str(), "find F3D scope timeline position")?.copied() else {
            continue;
        };
        match scope_history.predecessor(ctx.decode, scope, |candidate| {
            ctx.decode.contains_key_hash_map(&scope_positions, candidate.id.as_str(), "find F3D projected predecessor scope")
        }) {
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::Scope(predecessor)) => {
                if ctx.decode.get_hash_map(&scope_positions, predecessor.id.as_str(), "find F3D predecessor timeline position")?
                    .is_some_and(|predecessor| *predecessor >= position)
                {
                    ctx.push_constant_finding(
                        findings,
                        Check::NativeLinks,
                        "Fusion Design history edge runs forward in its feature timeline",
                        Some(
                            ctx.decode
                                .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                        ),
                    )?;
                }
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design scope history-state dependency is cyclic",
                Some(
                    ctx.decode
                        .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                ),
            )?,
            Ok(
                crate::design::feature_project::ScopeHistoryPredecessor::None
                | crate::design::feature_project::ScopeHistoryPredecessor::Ambiguous,
            ) => {}
        }
    }
    Ok(())
}

fn mesh_record_offset_is(
    record: &records::mesh::DesignMeshRecordIdentity,
    relative: u64,
    offset: u64,
) -> bool {
    record.byte_offset().checked_add(relative) == Some(offset)
}

/// Validate complete `Base Mesh Feature` record graphs and their neutral links.
fn validate_mesh_features(ctx: &Ctx, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let mut feature_ids = HashSet::new();
    let mut scope_records = HashSet::new();
    let mut collection_records = HashSet::new();
    let mut body_records = HashSet::new();
    let mut entry_records = HashSet::new();
    let mut guid_records = HashSet::new();
    let mut wrapper_records = HashSet::new();
    let mut state_records = HashSet::new();
    let mut node_records = HashSet::new();
    let mut auxiliary_records = HashSet::new();
    let mut collection_owner_records = HashSet::new();
    let mut body_owner_records = HashMap::new();
    let mut texture_table_records = HashSet::new();
    let mut record_indexes_storage = ctx.decode.reserve_scoped(0, "hold F3D mesh record indexes")?;
    let mut filename_records = HashMap::new();
    let mut projected_tessellations = HashSet::new();
    let asset_ids = ctx.decode.collect_hash_set(
        ctx.ir.model.assets.iter().map(|asset| &asset.id),
        "index F3D mesh asset IDs",
    )?;
    let tessellation_ids = ctx.decode.collect_hash_set(
        ctx.ir
            .model
            .tessellations
            .iter()
            .map(|tessellation| tessellation.id.as_str()),
        "index F3D mesh tessellation IDs",
    )?;
    for feature in ctx.decode.admit_iter(&ctx.native.design_mesh_features, "scan F3D design mesh features")? {
        let stream = design_stream(&feature.id);
        let scope = ctx.decode.get_hash_map(&ctx.scopes_by_index,
            &(stream, feature.scope().record().record_index()), "find F3D mesh scope")?;
        let mut valid = ctx.decode.insert_hash_set(
            &mut feature_ids,
            feature.id.as_str(),
            "index F3D mesh feature IDs",
        )? && ctx.decode.insert_hash_set(
            &mut scope_records,
            (stream, feature.scope().record().record_index()),
            "index F3D mesh scope records",
        )? && ctx.decode.insert_hash_set(
            &mut collection_records,
            (stream, feature.collection().record().record_index()),
            "index F3D mesh collection records",
        )? && ctx.decode.insert_hash_set(
            &mut texture_table_records,
            (stream, feature.texture_table.record().record_index()),
            "index F3D mesh texture tables",
        )? && ctx.decode.insert_hash_set(
            &mut collection_owner_records,
            (stream, feature.collection_owner.record().record_index()),
            "index F3D mesh collection owners",
        )? && feature
            .scope()
            .record()
            .byte_offset()
            .checked_add(scope.map_or(0, |scope| scope.frame_length()))
            == Some(feature.scope().base_record().byte_offset())
            && mesh_record_offset_is(
                feature.collection_owner.record(),
                262,
                feature.collection_owner.backlink_offset(),
            )
            && match scope {
                Some(scope) => ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::BaseMeshFeature, "compare F3D mesh scope kind")?
                    && scope.byte_offset() == feature.scope().record().byte_offset()
                    && scope.paired_byte_offset() == feature.scope().base_record().byte_offset(),
                None => false,
            };

        let mut resources = ctx.decode.collect_vec(
            feature.texture_table.resources().iter(),
            "collect F3D mesh texture resources",
        )?;
        ctx.decode.stable_sort_by(
            &mut resources,
            |value| &value.filename_ordinal,
            Ord::cmp,
            "f3d mesh texture resources sort",
        )?;
        let mut resources_valid = true;
        for resource in ctx.decode.admit_iter(&resources, "scan F3D mesh texture resources")? {
            let filename_key = (stream, resource.file.record().record_index());
            let filename_record_consistent = match ctx.decode.get_hash_map(&filename_records, &filename_key, "find F3D mesh filename record")? {
                Some(record) => ctx.decode.equal(*record, resource.file.record(), "compare F3D mesh filename records")?,
                None => true,
            };
            record_indexes_storage.with_storage(|| ctx.decode.entry_hash_map(&mut filename_records, filename_key, "index F3D mesh filename records"))?
                .or_insert(resource.file.record());
            resources_valid = filename_record_consistent
                && ctx.decode.contains_hash_set(&asset_ids, &resource.asset, "find F3D mesh resource asset")?;
            if !resources_valid {
                break;
            }
        }
        valid &= resources_valid;

        for body in ctx.decode.admit_iter(feature.bodies(), "scan F3D mesh feature bodies")? {
            let owner_key = (stream, body.owner_record.record_index());
            let owner_consistent = match ctx.decode.get_hash_map(&body_owner_records, &owner_key, "find F3D mesh owner record")? {
                Some(record) => ctx.decode.equal(*record, &body.owner_record, "compare F3D mesh owner records")?,
                None => true,
            };
            record_indexes_storage.with_storage(|| ctx.decode.entry_hash_map(&mut body_owner_records, owner_key, "index F3D mesh body owner records"))?
                .or_insert(&body.owner_record);
            let body_valid = ctx.decode.insert_hash_set(
                &mut body_records,
                (stream, body.placement.record().record_index()),
                "index F3D mesh body records",
            )? && ctx.decode.insert_hash_set(
                &mut entry_records,
                (stream, body.entry.record().record_index()),
                "index F3D mesh entry records",
            )? && ctx.decode.insert_hash_set(
                &mut guid_records,
                (stream, body.guid.record().record_index()),
                "index F3D mesh GUID records",
            )? && ctx.decode.insert_hash_set(
                &mut wrapper_records,
                (stream, body.wrapper_record.record_index()),
                "index F3D mesh wrapper records",
            )? && ctx.decode.insert_hash_set(
                &mut state_records,
                (stream, body.scene_state.record().record_index()),
                "index F3D mesh scene states",
            )? && ctx.decode.insert_hash_set(
                &mut node_records,
                (stream, body.scene_node.record_index()),
                "index F3D mesh scene nodes",
            )? && ctx.decode.insert_hash_set(
                &mut auxiliary_records,
                (stream, body.scene_auxiliary_record.record_index()),
                "index F3D mesh scene auxiliary records",
            )? && owner_consistent
                && body.scene_node.frame_length() == 133;
            let projection_valid = if body_valid {
                match body.tessellation_id.as_deref() {
                    Some(id) => {
                        ctx.decode.contains_hash_set(&tessellation_ids, id, "find F3D mesh tessellation")?
                            && ctx.decode.insert_hash_set(
                                &mut projected_tessellations,
                                id,
                                "index F3D mesh projected tessellations",
                            )?
                    }
                    None => true,
                }
            } else {
                false
            };
            valid &= body_valid && projection_valid;
        }
        let projected = || {
            feature
                .bodies()
                .iter()
                .filter_map(|body| body.tessellation_id.as_deref())
        };
        if projected().next().is_some() {
            valid &= match scope {
                Some(scope) => ctx.decode.any_by(&ctx.ir.model.features, |neutral| {
                    Ok(ctx.decode.equal(&neutral.native_ref.as_deref(), &Some(scope.id.as_str()), "compare F3D mesh native reference")?
                        && matches!(
                            neutral.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::MeshImport { tessellations })
                                if tessellations.iter().map(String::as_str).eq(projected())
                        ))
                }, "find F3D neutral mesh feature")?,
                None => false,
            };
        }

        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design mesh feature has an invalid frame or object graph",
                Some(
                    ctx.decode
                        .copy_retained_text(&feature.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate Canvas scope and Design object joins.
fn validate_canvas_images(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut scope_bindings = HashSet::new();
    let mut geometry_records = HashSet::new();
    let mut geometry_entities_storage = ctx.decode.reserve_scoped(0, "index F3D Canvas geometry entities")?;
    let mut geometry_entities = HashSet::new();
    for design_type in ctx.decode.admit_iter(&native.design_types, "scan F3D Canvas geometry entities types")? {
        if !matches!(design_type.module.as_str(), records::entity_header::DESIGN_MODULE_BODY | records::entity_header::DESIGN_MODULE_GEOMETRY) { continue; }
        let segment = ids::design_segment(design_type.id());
        let (entity_values, located_entity_values) = design_type.entities.storage_slices();
        for suffix in ctx.decode.admit_iter(entity_values, "scan F3D Canvas geometry entities members")?
            .chain(ctx.decode.admit_iter(located_entity_values, "scan F3D Canvas geometry entities located members")?.map(|row| &row.value)) {
            geometry_entities_storage.with_storage(|| ctx.decode.insert_hash_set(
                &mut geometry_entities, (segment, *suffix), "index F3D Canvas geometry entities"))?;
        }
    }
    let mut component_entities_storage = ctx.decode.reserve_scoped(0, "index F3D Canvas component entities")?;
    let mut component_entities = HashSet::new();
    for design_type in ctx.decode.admit_iter(&native.design_types, "scan F3D Canvas component entities types")? {
        if !matches!(design_type.module.as_str(), records::entity_header::DESIGN_MODULE_FUSION | records::entity_header::DESIGN_MODULE_COMPONENT) { continue; }
        let segment = ids::design_segment(design_type.id());
        let (entity_values, located_entity_values) = design_type.entities.storage_slices();
        for suffix in ctx.decode.admit_iter(entity_values, "scan F3D Canvas component entities members")?
            .chain(ctx.decode.admit_iter(located_entity_values, "scan F3D Canvas component entities located members")?.map(|row| &row.value)) {
            component_entities_storage.with_storage(|| ctx.decode.insert_hash_set(
                &mut component_entities, (segment, *suffix), "index F3D Canvas component entities"))?;
        }
    }
    for image in ctx.decode.admit_iter(&native.design_canvas_images, "scan F3D design canvas images")? {
        let native_stream = design_stream(&image.id);
        let design_segment = ids::design_segment(&image.id);
        let scope = ctx.decode.get_hash_map(&ctx.scopes_by_index, &(native_stream, image.scope_record_index), "find F3D Canvas scope")?;
        let scope_valid = match scope {
            Some(scope) => ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::Canvas, "compare F3D Canvas scope kind")?,
            None => false,
        };
        let scope_unique = if scope_valid {
            ctx.decode.insert_hash_set(
                &mut scope_bindings,
                (native_stream, image.scope_record_index),
                "index F3D Canvas scopes",
            )?
        } else {
            false
        };
        let geometry_unique = if scope_unique {
            ctx.decode.insert_hash_set(
                &mut geometry_records,
                (native_stream, image.geometry().record_index()),
                "index F3D Canvas geometry records",
            )?
        } else {
            false
        };
        let valid = scope_valid
            && scope_unique
            && geometry_unique
            && scope.is_some_and(|scope| scope.byte_offset() == image.scope_byte_offset())
            && ctx.decode.contains_hash_set(&geometry_entities, &(design_segment, u64::from(image.plane_entity_suffix)), "find F3D Canvas geometry entity")?
            && ctx.decode.contains_hash_set(&component_entities, &(design_segment, u64::from(image.component_entity_suffix)), "find F3D Canvas component entity")?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Canvas image has an invalid frame or Design object join",
                Some(
                    ctx.decode
                        .copy_retained_text(&image.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate Decal native and neutral object joins.
fn validate_decal_images(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    const TARGET_ROLE: DesignOperandRole = DesignOperandRole::BODIES_A;
    let mut scope_bindings = HashSet::new();
    let mut asset_records = HashSet::new();
    let mut fusion_entities_storage = ctx.decode.reserve_scoped(0, "index F3D Decal fusion entities")?;
    let mut fusion_entities = HashSet::new();
    for design_type in ctx.decode.admit_iter(&ctx.native.design_types, "scan F3D Decal fusion entities types")? {
        if !matches!(design_type.module.as_str(), records::entity_header::DESIGN_MODULE_FUSION) { continue; }
        let segment = ids::design_segment(design_type.id());
        let (entity_values, located_entity_values) = design_type.entities.storage_slices();
        for suffix in ctx.decode.admit_iter(entity_values, "scan F3D Decal fusion entities members")?
            .chain(ctx.decode.admit_iter(located_entity_values, "scan F3D Decal fusion entities located members")?.map(|row| &row.value)) {
            fusion_entities_storage.with_storage(|| ctx.decode.insert_hash_set(
                &mut fusion_entities, (segment, *suffix), "index F3D Decal fusion entities"))?;
        }
    }
    for image in ctx.decode.admit_iter(&ctx.native.design_decal_images, "scan F3D design decal images")? {
        let native_stream = design_stream(&image.id);
        let design_segment = ids::design_segment(&image.id);
        let scope = ctx.decode.get_hash_map(&ctx.scopes_by_index, &(native_stream, image.scope_record_index()), "find F3D Decal scope")?;
        let group = ctx.decode.get_hash_map(&ctx.operand_groups_by_index, &(native_stream, image.target_group_record_index), "find F3D Decal target group")?;
        let operand = match group.and_then(|group| group.members().first().map(|member| (group, member.value))) {
            Some((group, member)) => ctx.decode.find_by(&ctx.native.design_body_recipe_operands, |operand| {
                Ok(ctx.decode.equal(design_stream(&operand.id), native_stream, "compare F3D Decal operand stream")?
                    && operand.scope_record_index == image.scope_record_index()
                    && operand.record_index() == member
                    && operand.owner.group() == Some((group.record_index, 0)))
            }, "find F3D Decal operand")?,
            None => None,
        };
        let mut projected_faces_storage = ctx.decode.reserve_scoped(0, "collect F3D Decal projected faces")?;
        let projected =
            if image.mapping_mode == crate::records::decal::DesignDecalMappingMode::FitToFaces {
                if let Some(operand) = operand {
                    let mut faces = Vec::new();
                    for reference in ctx.decode.admit_iter(operand.references(), "scan F3D Decal projected references")? {
                        for id in ctx.decode.admit_iter(&reference.candidate_faces, "scan F3D Decal projected faces")? {
                            projected_faces_storage.with_storage(|| {
                                let face = id.try_clone_for_decode(ctx.decode, "collect F3D Decal projected faces")?;
                                ctx.decode.push_vec(&mut faces, face, "collect F3D Decal projected faces")
                            })?;
                        }
                    }
                    ctx.decode.stable_sort_by(
                        &mut faces,
                        |value| value.as_str(),
                        Ord::cmp,
                        "f3d decal projected faces sort",
                    )?;
                    ctx.decode.dedup_vec(&mut faces, "deduplicate F3D Decal projected faces")?;
                    (!faces.is_empty()).then_some((operand, faces))
                } else {
                    None
                }
            } else {
                None
            };
        let neutral_is_valid = match projected {
            None => true,
            Some((operand, expected_faces)) => match scope {
                None => false,
                Some(scope) => ctx.decode.any_by(&ctx.ir.model.features, |feature| {
                    if !ctx.decode.equal(&feature.native_ref.as_deref(), &Some(scope.id.as_str()), "compare F3D Decal native reference")? {
                        return Ok(false);
                    }
                    let cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Decal {
                        asset,
                        faces: cadmpeg_ir::features::FaceSelection::Resolved { faces, native },
                        mapping: cadmpeg_ir::features::DecalMapping::FitToFaces,
                        opacity: None,
                    }) = feature.evaluation.definition() else {
                        return Ok(false);
                    };
                    Ok(ctx.decode.equal(faces, &expected_faces, "compare F3D Decal projected face IDs")?
                        && ctx.decode.equal(native, &operand.id, "compare F3D Decal operand identity")?
                        && ctx.decode.any_by(&ctx.ir.model.assets, |candidate| {
                            Ok(ctx.decode.equal(&candidate.id, asset, "compare F3D Decal asset identity")?
                                && ctx.decode.equal(&candidate.name.as_ref().map(cadmpeg_core::text::NonBlankString::as_str), &Some(image.asset.name()), "compare F3D Decal asset name")?)
                        }, "find F3D Decal projected asset")?)
                }, "find F3D Decal neutral feature")?,
            },
        };
        drop(projected_faces_storage);
        let scope_valid = match scope {
            Some(scope) => ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::Decal, "compare F3D Decal scope kind")?,
            None => false,
        };
        let scope_unique = if scope_valid {
            ctx.decode.insert_hash_set(
                &mut scope_bindings,
                (native_stream, image.scope_record_index()),
                "index F3D Decal scopes",
            )?
        } else {
            false
        };
        let asset_unique = if scope_unique {
            ctx.decode.insert_hash_set(
                &mut asset_records,
                (native_stream, image.asset.record_index()),
                "index F3D Decal assets",
            )?
        } else {
            false
        };
        let valid = scope_valid
            && scope_unique
            && asset_unique
            && scope.is_some_and(|scope| scope.byte_offset() == image.scope_byte_offset())
            && ctx.decode.contains_hash_set(&fusion_entities, &(design_segment, u64::from(image.asset.entity_suffix())), "find F3D Decal fusion entity")?
            && group.is_some_and(|group| {
                group.scope_record_index == image.scope_record_index()
                    && group.role() == TARGET_ROLE
                    && group.members().len() == 1
            })
            && operand.is_some()
            && neutral_is_valid;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Decal image has an invalid frame or Design object join",
                Some(
                    ctx.decode
                        .copy_retained_text(&image.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate the ordered Design body-map binding entries and their pair runs.
fn validate_body_bindings(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut binding_offsets = HashSet::new();
    let mut binding_groups =
        std::collections::HashMap::<(&str, u64), Vec<&records::bodies::DesignBodyBinding>>::new();
    for binding in ctx.decode.admit_iter(&native.design_body_bindings, "scan F3D design body bindings")? {
        let native_stream = design_stream(binding.id());
        let resolved_valid = if let Some(body) = &binding.body {
            let has_named_source = ctx.decode.any_by(&native.body_native_keys, |key| {
                Ok(ids::same_native_occurrence(key.source_namespace.as_str(), binding.id())
                    && ctx.decode.equal(&key.source_brep.as_deref(), &Some(binding.blob_name()), "compare F3D body source name")?)
            }, "find F3D named body sources")?;
            let mut source_keys_storage = ctx.decode.reserve_scoped(0, "hold F3D body source keys")?;
            let mut source_keys = Vec::new();
            for key in ctx.decode.admit_iter(&native.body_native_keys, "select F3D body source keys")? {
                if ids::same_native_occurrence(key.source_namespace.as_str(), binding.id())
                    && if has_named_source {
                        ctx.decode.equal(&key.source_brep.as_deref(), &Some(binding.blob_name()), "compare F3D selected body source name")?
                    } else {
                        key.source_brep.is_none()
                    }
                {
                    ctx.decode.push_scoped_vec(&mut source_keys_storage, &mut source_keys, key, "collect F3D body source keys")?;
                }
            }
            match crate::brep::resolve_body_selector(ctx.decode, source_keys.iter().copied(), binding.asm_body_key)
            {
                Ok(Some(resolved)) => ctx.decode.equal(resolved, body, "compare F3D resolved body identity")?,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Ok(None) | Err(_) => false,
            }
        } else {
            true
        };
        let valid = design_stream_contains_entry(native_stream, binding.stream())
            && resolved_valid
            && ctx.decode.insert_hash_set(
                &mut binding_offsets,
                (native_stream, binding.asm_body_key_offset()),
                "index F3D body binding offsets",
            )?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body binding has an invalid ordered map entry",
                Some(
                    ctx.decode
                        .copy_retained_text(binding.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
        ctx.decode.push_hash_group(
            &mut binding_groups,
            (native_stream, binding.blob_name_offset()),
            binding,
            "index F3D body binding groups",
            "collect F3D body binding group members",
        )?;
    }
    let mut bindings_keys_storage = ctx.decode.reserve_scoped(0, "scan F3D bindings groups")?;
    let bindings_keys = bindings_keys_storage.with_storage(|| {
        ctx.decode.collect_vec(binding_groups.keys().copied(), "stage F3D bindings group keys")
    })?;
    for key in ctx.decode.admit_iter(&bindings_keys, "scan F3D bindings groups")? {
        let bindings = ctx.decode.get_mut_hash_map(&mut binding_groups, key, "find F3D bindings group")?
            .ok_or_else(|| CodecError::malformed("F3D validation group is absent"))?;
        ctx.decode.stable_sort_by_key(
            bindings,
            |value| value.pair_ordinal(),
            Ord::cmp,
            "f3d body binding pair run sort",
        )?;
        let mut complete = bindings.first().is_some_and(|first| usize::try_from(first.pair_count()).ok() == Some(bindings.len()));
        if complete {
            for (ordinal, binding) in ctx.decode.admit_iter(&*bindings, "check F3D ordered body pairs")?.enumerate() {
                if usize::try_from(binding.pair_ordinal()).ok() != Some(ordinal)
                    || binding.pair_count() != bindings[0].pair_count()
                    || !ctx.decode.equal(binding.blob_name(), bindings[0].blob_name(), "compare F3D body pair names")?
                    || !ctx.decode.equal(binding.stream(), bindings[0].stream(), "compare F3D body pair streams")?
                {
                    complete = false;
                    break;
                }
            }
        }
        if !complete {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body map has an incomplete ordered pair run",
                bindings
                    .first()
                    .map(|binding| {
                        ctx.decode
                            .copy_retained_text(binding.id(), "retain F3D validation entity")
                    })
                    .transpose()?,
            )?;
        }
    }
    Ok(())
}

/// Validate each Design body-bounds repeated record frame.
fn validate_body_bounds(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut bounded_bodies = HashSet::new();
    for bounds in ctx.decode.admit_iter(&native.design_body_bounds, "scan F3D design body bounds")? {
        let native_stream = design_stream(bounds.id());
        let mut expected_bindings = ctx.decode.collect_vec(
            ctx.decode.admit_iter(&native.design_body_bindings, "admit source for collect F3D expected body bounds bindings")?.filter(|binding| {
                design_stream_contains_entry(native_stream, binding.stream())
                    && binding.entity_suffix == bounds.entity_suffix()
            }),
            "collect F3D expected body bounds bindings",
        )?;
        ctx.decode.stable_sort_by_key(
            &mut expected_bindings,
            |value| value.asm_body_key_offset(),
            Ord::cmp,
            "f3d body bounds binding sort",
        )?;
        let valid_frame = ctx.decode
            .get_hash_map(&ctx.entities_by_suffix, &(native_stream, bounds.entity_suffix()), "find F3D body bounds entity")?
            .is_some_and(|entity| {
                entity.module() == Some(records::entity_header::DESIGN_MODULE_BODY)
                    && entity.byte_offset == bounds.entity_byte_offset()
            })
            && bounds.body_binding_ids().eq(expected_bindings
                .iter()
                .map(|binding| binding.id().as_str()));
        let valid = if valid_frame {
            ctx.decode.insert_hash_set(
                &mut bounded_bodies,
                (native_stream, bounds.entity_suffix()),
                "index F3D bounded bodies",
            )?
        } else {
            false
        };
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body bounds have an invalid repeated record frame",
                Some(
                    ctx.decode
                        .copy_retained_text(bounds.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate feature parameter scopes and their paired feature-operation frames.
fn validate_parameter_scopes(ctx: &Ctx, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let placements_by_scope = &ctx.placements_by_scope;
    let mut scope_indices = HashSet::new();
    for scope in ctx.decode.admit_iter(&native.design_parameter_scopes, "scan F3D design parameter scopes")? {
        let native_stream = design_stream(&scope.id);
        let unique_index = ctx.decode.insert_hash_set(
            &mut scope_indices,
            (native_stream, scope.record_index),
            "index F3D parameter scope records",
        )?;
        let entity_link = scope.sketch_entity().map(|binding| {
            entities_by_suffix
                .get(&(native_stream, binding.entity_id.suffix()))
                .is_some_and(|entity| {
                    entity.entity_id == binding.entity_id
                        && binding.entity_reference_offset > scope.byte_offset()
                        && binding.entity_reference_offset < scope.paired_byte_offset()
                })
        });
        let valid_sketch_profile =
            |profile: &records::topology::sketch_profile::DesignSketchProfileOperand| -> Result<bool, CodecError> {
                let header = ctx.decode.get_hash_map(records_by_index, &(native_stream, profile.record_index), "find F3D profile record header")?;
                let entity = ctx.decode.get_hash_map(entities_by_suffix, &(native_stream, profile.entity_id.suffix()), "find F3D profile entity")?;
                let reference_member = match usize::try_from(profile.scope_reference_ordinal) {
                    Ok(ordinal) => {
                        let (values, located) = scope.reference_members().storage_slices();
                        ctx.decode.admit_iter(values, "scan F3D profile scope references")?
                            .chain(ctx.decode.admit_iter(located, "scan F3D profile located scope references")?.map(|row| &row.value))
                            .nth(ordinal)
                    }
                    Err(_) => None,
                };
                Ok(reference_member == Some(&profile.record_index)
                    && match header {
                        Some(header) => header.byte_offset == profile.byte_offset()
                            && ctx.decode.equal(&header.class_tag, &profile.class_tag, "compare F3D profile class tags")?,
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
                let edge_count = operation.selection.shape().edges().count();
                let claimed = ctx.decode.collect_vec(
                    operation
                        .selection
                        .shape()
                        .edges()
                        .flat_map(|edge| {
                            [
                                edge.wrapper,
                                edge.group_record_index.get(),
                                edge.operand_record_index(),
                                edge.aggregate_operand_record_index,
                            ]
                        })
                        .chain(operation.selection.shape().owner_indices().copied())
                        .chain(operation.auxiliary_reference_record_indices.iter().copied())
                        .chain([
                            operation.selection.aggregate_group_record_index(),
                            operation.height_owner_record_index,
                            operation.angle_owner_record_index,
                            operation.settings_record_index,
                        ]),
                    "collect F3D edge flange claimed references",
                )?;
                let mut claimed = claimed;
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
                        ctx.decode.reserve_vec(
                            &mut claimed,
                            1,
                            "collect F3D edge flange target references",
                        )?;
                        claimed.push(index);
                    }
                }
                let unique_claimed = ctx.decode.collect_hash_set(
                    claimed.iter().copied(),
                    "index F3D edge flange claimed references",
                )?;
                edge_count > 0
                    && claimed.len() == scope.reference_members().len()
                    && unique_claimed.len() == claimed.len()
                    && claimed.iter().all(|index| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == index)
                    })
                    && operation.bend_radius_offset > scope.byte_offset()
                    && operation.bend_radius_offset < scope.paired_byte_offset()
            }
        };
        let hem_link = match scope.hem_operation() {
            None => true,
            Some(operation) => {
                let mut claimed_storage = ctx.decode.reserve_scoped(0, "hold F3D Hem claimed references")?;
                let mut claimed = claimed_storage.with_storage(|| ctx.decode.collect_vec([
                    operation.edge_wrapper_record_index,
                    operation.edge_group_record_index.get(),
                    operation.edge_operand_record_index(),
                    operation.aggregate_group_record_index.get(),
                    operation.aggregate_operand_record_index(),
                    operation.settings_record_index,
                ], "collect F3D Hem claimed references"))?;
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
                let unique_claimed = claimed_storage.with_storage(|| ctx.decode.collect_hash_set(claimed.iter().copied(), "index F3D Hem claimed references"))?;
                unique_claimed.len() == claimed.len()
                    && claimed.len() == scope.reference_members().len()
                    && ctx.decode.all_by(&claimed, |index| {
                        let (plain, located) = scope.reference_members().storage_slices();
                        Ok(ctx.decode.any_by(plain, |value| Ok(value == index), "find F3D Hem reference")?
                            || ctx.decode.any_by(located, |row| Ok(&row.value == index), "find F3D located Hem reference")?)
                    }, "check F3D Hem claimed references")?
                    && operation.bend_radius_offset > scope.byte_offset()
                    && operation.bend_radius_offset < scope.paired_byte_offset()
            }
        };
        let copy_paste_link = match scope.copy_paste_bodies_operation() {
            None => {
                scope.kind() != crate::records::feature::scope::DesignFeatureKind::CopyPasteBodies
            }
            Some(operation) => {
                let group_header =
                    ctx.decode.get_hash_map(records_by_index, &(native_stream, operation.body_group_record_index), "find F3D validation linked record")?;
                let relation_header =
                    ctx.decode.get_hash_map(records_by_index, &(native_stream, operation.relation_record_index), "find F3D validation linked record")?;
                scope.reference_members().values().next()
                    == Some(&operation.body_group_record_index)
                    && scope
                        .reference_members()
                        .values()
                        .skip(1)
                        .copied()
                        .eq(operation.bodies().iter().map(|body| body.operand.value))
                    && group_header.is_some_and(|header| {
                        header.byte_offset == operation.body_group_byte_offset()
                            && header.class_tag == operation.body_group_class_tag
                    })
                    && relation_header.is_some_and(|header| {
                        header.byte_offset == operation.relation_byte_offset()
                            && header.class_tag == operation.relation_class_tag
                    })
                    && operation
                        .bodies()
                        .iter()
                        .map(|body| body.source.value)
                        .all(|suffix| {
                            native.design_body_bindings.iter().any(|binding| {
                                design_stream(binding.id()) == native_stream
                                    && binding.entity_suffix == u64::from(suffix)
                            })
                        })
                    && operation
                        .bodies()
                        .iter()
                        .map(|body| body.copied.value)
                        .all(|suffix| {
                            native.design_body_bindings.iter().any(|binding| {
                                design_stream(binding.id()) == native_stream
                                    && binding.entity_suffix == u64::from(suffix)
                                    && binding.body.is_some()
                            })
                        })
            }
        };
        let rectangular_pattern_link = match scope.rectangular_pattern_construction() {
            None => {
                design::design_feature_family(&scope.kind())
                    != Some(design::DesignFeatureFamily::RectangularPattern)
            }
            Some(construction) => {
                let instances_link = construction.instances().is_none_or(|instances| {
                    let active = [
                        (construction.u_count(), construction.u_extent()),
                        (construction.v_count(), construction.v_extent()),
                    ]
                    .into_iter()
                    .filter(|(count, _)| *count > 1)
                    .collect::<Vec<_>>();
                    let [(count, extent)] = active.as_slice() else {
                        return false;
                    };
                    let Ok(count) = usize::try_from(*count) else {
                        return false;
                    };
                    let Some(reference_end) = count.checked_add(5) else {
                        return false;
                    };
                    let expected_records = scope
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
                        return false;
                    };
                    let Some(last) = instances
                        .frames()
                        .next_back()
                        .map(|frame| &frame.transform.value)
                    else {
                        return false;
                    };
                    let delta = [
                        last[0][3] - first[0][3],
                        last[1][3] - first[1][3],
                        last[2][3] - first[2][3],
                    ];
                    let distance = cadmpeg_ir::math::Vector3::from(delta).norm();
                    let component_link =
                        valid_component_pattern_occurrences(native, native_stream, instances);
                    instances
                        .frames()
                        .map(|frame| frame.record_index)
                        .eq(expected_records)
                        && instances.instance_count() == count
                        && instances.frames().all(|frame| {
                            let transform = &frame.transform.value;
                            (0..3).all(|row| {
                                (0..3).all(|column| {
                                    (transform[row][column] - first[row][column]).abs()
                                        <= EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E10
                                })
                            })
                        })
                        && (distance - extent.abs()).abs()
                            <= EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8
                        && instances.frames().enumerate().all(|(ordinal, frame)| {
                            let transform = &frame.transform.value;
                            let (Some(ordinal), Some(divisor)) =
                                (f64_from_index(ordinal), f64_from_index(count - 1))
                            else {
                                return false;
                            };
                            let fraction = ordinal / divisor;
                            (0..3).all(|axis| {
                                (transform[axis][3] - first[axis][3] - delta[axis] * fraction).abs()
                                    <= EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8
                            })
                        })
                        && instances.frames().all(|frame| {
                            records_by_index
                                .get(&(native_stream, frame.record_index))
                                .is_some_and(|header| frame.transform.offset > header.byte_offset)
                        })
                        && component_link
                });
                instances_link
                    && native
                        .design_parameter_owners
                        .iter()
                        .filter(|owner| {
                            design_stream(owner.id()) == native_stream
                                && owner.scope_record_index() == scope.record_index
                        })
                        .count()
                        == 4
                    && construction
                        .owner_record_indices
                        .iter()
                        .all(|record_index| {
                            scope
                                .reference_members()
                                .values()
                                .any(|value| value == record_index)
                                && records_by_index.contains_key(&(native_stream, *record_index))
                        })
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
                        .all(|(ordinal, ((record_index, value_offset), value))| {
                            native.design_parameter_owners.iter().any(|owner| {
                                design_stream(owner.id()) == native_stream
                                    && owner.record_index() == *record_index
                                    && owner.scope_record_index() == scope.record_index
                                    && u32::try_from(ordinal) == Ok(owner.local_ordinal())
                                    && owner.evaluated_value().get() == value
                                    && owner.evaluated_value_offset() == value_offset
                            })
                        })
            }
        };
        let assembly_alignment_link = match scope.assembly_alignment() {
            None => {
                design::design_feature_family(&scope.kind())
                    != Some(design::DesignFeatureFamily::Assemble)
            }
            Some(alignment) => {
                let values = if alignment.owners.len() == 2 {
                    vec![alignment.angle(), alignment.offset()[2]]
                } else {
                    vec![
                        alignment.angle(),
                        alignment.offset()[0],
                        alignment.offset()[1],
                        alignment.offset()[2],
                    ]
                };
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
                let as_built_frames = scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::AsBuilt
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
                let assembly_owner_count = native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == scope.record_index
                    })
                    .count();
                let alignment_lane_bounds = generation.alignment_lane_bounds(assembly_owner_count);
                let operand_frames_link = if let Some(
                    records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                        carriers,
                        ..
                    },
                ) = &alignment.form
                {
                    carriers
                        .references()
                        .into_iter()
                        .enumerate()
                        .all(|(ordinal, reference)| {
                            let reference_ordinal = ordinal * 2;
                            scope
                                .reference_members()
                                .values()
                                .nth(reference_ordinal)
                                .copied()
                                == Some(reference.value)
                                && scope
                                    .reference_members()
                                    .offsets()
                                    .nth(reference_ordinal)
                                    .copied()
                                    == Some(reference.offset)
                                && records_by_index.contains_key(&(native_stream, reference.value))
                        })
                } else {
                    alignment.operand_frames().is_none_or(|frames| {
                        frames[0].reference_record_index != frames[1].reference_record_index
                            && frames.iter().enumerate().all(|(ordinal, frame)| {
                                let offsets_match = if as_built_frames {
                                    operand_paths.as_ref().is_some_and(|paths| {
                                        paths[ordinal].link().locator_byte_offset.checked_add(22)
                                            == Some(frame.reference_offset)
                                            && paths[ordinal]
                                                .link()
                                                .locator_byte_offset
                                                .checked_add(33)
                                                == Some(frame.transform_offset)
                                    })
                                } else {
                                    Some(frame.reference_offset)
                                        == scope
                                            .byte_offset()
                                            .checked_add(frame_reference_offsets[ordinal])
                                        && Some(frame.transform_offset)
                                            == scope
                                                .byte_offset()
                                                .checked_add(frame_transform_offsets[ordinal])
                                };
                                let reference_exists = if as_built_frames {
                                    frame.reference_record_index != 0
                                } else {
                                    records_by_index.contains_key(&(
                                        native_stream,
                                        frame.reference_record_index,
                                    ))
                                };
                                offsets_match && reference_exists
                            })
                    })
                };
                let solved_frame_link = alignment.solved_frame().is_none_or(|frame| {
                    let Some(generation) = as_built_421_generation else {
                        return false;
                    };
                    let Some(header) =
                        records_by_index.get(&(native_stream, frame.reference_record_index))
                    else {
                        return false;
                    };
                    as_built_421
                        && scope.reference_members().values().nth(8).copied()
                            == Some(frame.reference_record_index)
                        && scope.reference_members().offsets().nth(8).copied()
                            == Some(frame.reference_offset)
                        && header.class_tag.as_str() == generation.frame_class_tag()
                        && frame.class_tag == header.class_tag
                        && frame.record_byte_offset == header.byte_offset
                        && Some(frame.transform_offset)
                            == frame
                                .record_byte_offset
                                .checked_add(u64_from_index(generation.matrix_offset()))
                });
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
                                    && paths.iter().all(|path| {
                                        matches!(path.class_tag().as_str(), "294" | "299" | "307" | "329" | "330" | "386" | "390")
                                            && !matches!(path.link().locator_class_tag.as_str(), "363" | "378")
                                            && (matches!(
                                                path.class_tag().as_str(),
                                                "294" | "299" | "307" | "330" | "386"
                                            ) || path.occurrence_guids().first().is_some_and(
                                                |guid| {
                                                    native
                                                        .design_component_occurrences
                                                        .iter()
                                                        .filter(|occurrence| {
                                                            design_stream(&occurrence.id)
                                                                == native_stream
                                                                && occurrence
                                                                    .occurrence_guid.as_str()
                                                                    .eq_ignore_ascii_case(guid.value.as_str())
                                                        })
                                                        .count()
                                                        == 1
                                                },
                                            ))
                                    })
                            }

                            }
                            (records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target: first },
                             records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target: second }) => {
                                axial_frames && valid_axial_assembly_targets(native, records_by_index, native_stream, scope, &frames, &[first, second])
                            }
                            _ if variable_reference && operands.iter().any(|operand| matches!(operand.qualifier, records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin { .. })) => {
                                frames[0].reference_record_index != frames[1].reference_record_index
                                    && operands.iter().all(|operand| match &operand.qualifier {
                                        records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path } => valid_class_363_operand_path_link(scope, &operand.frame, path),
                                        qualifier @ records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin { .. } => valid_class_307_joint_origin_qualifier(native, records_by_index, native_stream, &operand.frame, qualifier),
                                        records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { .. } => false,
                                    })
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
                    .is_none_or(|record_index| {
                        scope.class_tag.as_str() == "276"
                            && scope.paired_class_tag.as_str() == "258"
                            && scope.frame_length() == 604
                            && native.design_parameter_scopes.iter().any(|target| {
                                design_stream(&target.id) == native_stream
                                    && target.kind()
                                        == crate::records::feature::scope::DesignFeatureKind::JointOrigin
                                    && target.record_index == record_index
                                    && scope
                                        .byte_offset()
                                        .checked_add(36)
                                        .is_some_and(|offset| {
                                            target.joint_origin_transform_offset() == Some(offset)
                                        })
                            })
                    });
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
                                && alignment_lanes.into_iter().chain(limit_lanes).all(
                                    |(record_index, value_offset, value, local_ordinal)| {
                                        native.design_parameter_owners.iter().any(|owner| {
                                            design_stream(owner.id()) == native_stream
                                                && owner.record_index() == record_index
                                                && owner.scope_record_index() == scope.record_index
                                                && owner.local_ordinal() == local_ordinal
                                                && owner.evaluated_value().get() == value
                                                && owner.evaluated_value_offset() == value_offset
                                        })
                                    },
                                )
                        }
                        _ => false,
                    }
                } else {
                    alignment_lane_bounds.is_some_and(|(alignment_start, alignment_end)| {
                        alignment_end
                            .checked_sub(alignment_start)
                            .is_some_and(|expected_offset| {
                                alignment.owners.len() == expected_offset
                            })
                            && alignment.owners.iter().zip(&values).enumerate().all(
                                |(ordinal, (lane, value))| {
                                    native.design_parameter_owners.iter().any(|owner| {
                                        design_stream(owner.id()) == native_stream
                                            && owner.record_index() == lane.value
                                            && owner.scope_record_index() == scope.record_index
                                            && u32::try_from(alignment_start + ordinal)
                                                == Ok(owner.local_ordinal())
                                            && owner.evaluated_value().get() == *value
                                            && owner.evaluated_value_offset() == lane.offset
                                    })
                                },
                            )
                    })
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
                    (0..scope.reference_members().len())
                        .filter(|&start| {
                            scope
                                .reference_members()
                                .values_in(start..start + alignment.owners.len())
                                .is_some_and(|values| {
                                    values.eq(alignment.owners.iter().map(|owner| &owner.value))
                                })
                        })
                        .count()
                        == 1
                } else {
                    scope
                        .reference_members()
                        .values()
                        .rev()
                        .take(alignment.owners.len())
                        .eq(alignment.owners.iter().map(|owner| &owner.value).rev())
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
                let relation =
                    ctx.decode.get_hash_map(records_by_index, &(native_stream, construction.relation_record_index), "find F3D validation linked record")?;
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
                    && (native.xref_references.is_empty()
                        || native.xref_references.iter().any(|reference| {
                            reference.neutron_role.as_str() == construction.neutron_role
                                && reference
                                    .transform
                                    .map(records::xref::XrefPlacementTransform::rows)
                                    == Some((*construction.transform()).into())
                        }))
            }
        };
        let copy_paste_component_link = match scope.copy_paste_component_operation() {
            None => scope.kind() != crate::records::feature::scope::DesignFeatureKind::CopyPaste,
            Some(operation) => {
                let source = native
                    .design_component_occurrences
                    .iter()
                    .find(|occurrence| {
                        design_stream(&occurrence.id) == native_stream
                            && occurrence.record_index == operation.source_occurrence_record_index
                    });
                let copied = native
                    .design_component_occurrences
                    .iter()
                    .find(|occurrence| {
                        design_stream(&occurrence.id) == native_stream
                            && occurrence.record_index == operation.copied_occurrence_record_index
                    });
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
                    && source.is_some_and(|source| {
                        source
                            .component_guid
                            .as_str()
                            .eq_ignore_ascii_case(operation.component_guid.as_str())
                            && source
                                .occurrence_guid
                                .as_str()
                                .eq_ignore_ascii_case(operation.source_occurrence_guid.as_str())
                            && source.transform().is_none()
                    })
                    && copied.is_some_and(|copied| {
                        copied
                            .component_guid
                            .as_str()
                            .eq_ignore_ascii_case(operation.component_guid.as_str())
                            && copied
                                .occurrence_guid
                                .as_str()
                                .eq_ignore_ascii_case(operation.copied_occurrence_guid.as_str())
                            && copied.transform().map(|frame| frame.value)
                                == Some(operation.copied_transform)
                    })
            }
        };
        let draft_link = match scope.draft_operation() {
            None => {
                design::design_feature_family(&scope.kind())
                    != Some(design::DesignFeatureFamily::Draft)
            }
            Some(operation) => {
                scope.reference_members().len() >= 6
                    && scope
                        .reference_members()
                        .values()
                        .any(|value| value == &operation.angle_record_index)
                    && scope
                        .reference_members()
                        .values()
                        .any(|value| value == &operation.opposite_angle_record_index)
                    && operation.angle_record_index != operation.opposite_angle_record_index
                    && operation.angle_offset > scope.paired_byte_offset()
                    && operation.opposite_angle_offset > operation.angle_offset
                    && records_by_index.contains_key(&(native_stream, operation.angle_record_index))
                    && records_by_index
                        .contains_key(&(native_stream, operation.opposite_angle_record_index))
            }
        };
        let combine_link = match scope.combine_operation() {
            None => true,
            Some(operation) => {
                let mut selection_storage = ctx.decode.reserve_scoped(0, "hold F3D Combine selection indexes")?;
                let (plain, located) = scope.reference_members().storage_slices();
                let expected_selections = selection_storage.with_storage(|| {
                    ctx.decode.collect_hash_set(
                        ctx.decode.admit_iter(plain, "scan F3D Combine expected selections")?
                            .chain(ctx.decode.admit_iter(located, "scan F3D located Combine expected selections")?.map(|row| &row.value))
                            .skip(1).step_by(2).copied(),
                        "index F3D Combine expected selections",
                    )
                })?;
                let selections = selection_storage.with_storage(|| {
                    ctx.decode.collect_vec(
                        std::iter::once(operation.target_record_index)
                            .chain(std::iter::once(&operation.tools.first).chain(ctx.decode.admit_iter(&operation.tools.additional, "scan F3D Combine tool selections")?).map(|tool| tool.record_index)),
                        "collect F3D Combine selections",
                    )
                })?;
                let actual_selections = selection_storage.with_storage(|| {
                    ctx.decode.collect_hash_set(selections.iter().copied(), "index F3D Combine actual selections")
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
                        scope,
                    ) == Some(314);
                let extended_reference_scope = scope.class_tag.as_str() == "329"
                    && scope.paired_class_tag.as_str() == "261"
                    && scope.frame_length() == 363;
                scope.reference_members().len() >= 4
                    && scope.reference_members().len().is_multiple_of(2)
                    && selections.len() == scope.reference_members().len() / 2
                    && actual_selections.len() == selections.len()
                    && actual_selections.len() == expected_selections.len()
                    && ctx.decode.is_subset_hash_set(&actual_selections, &expected_selections, "compare F3D Combine selections")?
                    && ctx.decode.all_by(std::slice::from_ref(&operation.tools.first), &valid_external, "check F3D Combine external selections")?
                    && ctx.decode.all_by(&operation.tools.additional, &valid_external, "check F3D Combine external selections")?
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
                let (reference_values, located_reference_values) = scope.reference_members().storage_slices();
                let expected_groups: Vec<_> = match construction.form {
                    records::feature::thread::DesignThreadForm::Standard
                    | records::feature::thread::DesignThreadForm::StandardLegacy => {
                        ctx.decode.collect_vec(
                            ctx.decode.admit_iter(reference_values, "scan F3D standard thread references")?
                                .chain(ctx.decode.admit_iter(located_reference_values, "scan F3D standard thread located references")?.map(|row| &row.value))
                                .next().copied(),
                            "collect F3D standard thread face groups",
                        )?
                    }
                    records::feature::thread::DesignThreadForm::Compact(_)
                    | records::feature::thread::DesignThreadForm::CompactLegacy => {
                        ctx.decode.collect_vec(
                            ctx.decode.admit_iter(reference_values, "scan F3D compact thread references")?
                                .chain(ctx.decode.admit_iter(located_reference_values, "scan F3D compact thread located references")?.map(|row| &row.value))
                                .step_by(2).copied(),
                            "collect F3D compact thread face groups",
                        )?
                    }
                };
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
                    && construction.face_group_record_indices == expected_groups
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
                                && records_by_index
                                    .contains_key(&(native_stream, reference.value.get()))
                        }
                        records::feature::thread::DesignThreadForm::Compact(None)
                        | records::feature::thread::DesignThreadForm::Standard
                        | records::feature::thread::DesignThreadForm::StandardLegacy
                        | records::feature::thread::DesignThreadForm::CompactLegacy => true,
                    }
                    && construction
                        .face_group_record_indices
                        .iter()
                        .enumerate()
                        .all(|(group_ordinal, record_index)| {
                            let compact_member = if matches!(
                                construction.form,
                                records::feature::thread::DesignThreadForm::Compact(_)
                                    | records::feature::thread::DesignThreadForm::CompactLegacy
                            ) {
                                let Some(reference_ordinal) = group_ordinal.checked_mul(2) else {
                                    return false;
                                };
                                let Some(member_ordinal) = reference_ordinal.checked_add(1) else {
                                    return false;
                                };
                                let Some(member_record_index) =
                                    scope.reference_members().values().nth(member_ordinal)
                                else {
                                    return false;
                                };
                                let Ok(scope_reference_ordinal) = u32::try_from(reference_ordinal)
                                else {
                                    return false;
                                };
                                Some((scope_reference_ordinal, *member_record_index))
                            } else {
                                None
                            };
                            let mut groups = native
                                .design_construction_operand_groups
                                .iter()
                                .filter(|group| {
                                    design_stream(&group.id) == native_stream
                                        && group.scope_record_index == scope.record_index
                                        && group.record_index == *record_index
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
                                });
                            groups.next().is_some() && groups.next().is_none()
                        })
            }
        };
        let joint_origin_link = scope.joint_origin_frame().is_none_or(|origin| {
            let transform = origin.joint_origin_transform;
            let transform_offset = origin.joint_origin_transform_offset;
            let inline = match (scope.frame_length(), &origin.reference) {
                (385, None) => Some(transform_offset) == scope.byte_offset().checked_add(49),
                (336 | 347, Some(reference)) => {
                    Some(transform_offset) == scope.byte_offset().checked_add(60)
                        && Some(reference.joint_origin_reference_offset)
                            == scope.byte_offset().checked_add(46)
                        && scope
                            .reference_members()
                            .values()
                            .any(|value| value == &reference.joint_origin_reference)
                }
                _ => false,
            };
            let assembly_operand = origin.reference.is_none()
                && native.design_parameter_scopes.iter().any(|assembly| {
                    design_stream(&assembly.id) == native_stream
                        && assembly.kind()
                            == crate::records::feature::scope::DesignFeatureKind::Assemble
                        && assembly.assembly_alignment().is_some_and(|alignment| {
                            alignment.operand_frames().is_some_and(|frames| {
                                frames.iter().any(|frame| {
                                    frame.reference_record_index == scope.record_index
                                        && frame.transform == transform
                                        && frame.transform_offset == transform_offset
                                })
                            })
                        })
                });
            let single_operand_assembly = origin.reference.as_ref().is_some_and(|reference| {
                native.design_parameter_scopes.iter().any(|assembly| {
                    design_stream(&assembly.id) == native_stream
                        && assembly.kind()
                            == crate::records::feature::scope::DesignFeatureKind::Assemble
                        && assembly.class_tag.as_str() == "276"
                        && assembly.paired_class_tag.as_str() == "258"
                        && assembly.frame_length() == 604
                        && Some(transform_offset) == assembly.byte_offset().checked_add(36)
                        && assembly
                            .reference_members()
                            .values()
                            .any(|value| value == &reference.joint_origin_reference)
                        && Some(reference.joint_origin_reference_offset)
                            == assembly.byte_offset().checked_add(25)
                })
            });
            inline || assembly_operand || single_operand_assembly
        });
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
                let prefix_valid = reference.map_or(
                    scope
                        .byte_offset()
                        .checked_add(28)
                        .is_some_and(|expected_offset| operation_offset == expected_offset),
                    |reference| {
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
                        scope
                            .byte_offset()
                            .checked_add(26)
                            .is_some_and(|expected_offset| {
                                reference.record_index_offset == expected_offset
                            })
                            && matches!(reference.trailing_zero_count, 7 | 8)
                            && marker_valid
                            && scope
                                .reference_members()
                                .values()
                                .any(|value| value == &reference.record_index)
                    },
                );
                let target_ordinal_valid = first_side_target_ordinal.is_none_or(|target| {
                    usize::try_from(target.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| {
                            scope.reference_members().values().nth(ordinal).copied()
                        })
                        .is_some_and(|record_index| {
                            let mut groups = native
                                .design_construction_operand_groups
                                .iter()
                                .filter(|group| {
                                    design_stream(&group.id) == native_stream
                                        && group.scope_record_index == scope.record_index
                                        && group.record_index == record_index
                                        && group.scope_reference_ordinal
                                            == target.scope_reference_ordinal
                                        && group.role() == DesignOperandRole::ROLE_0X5
                                        && group.extrude_role().is_none()
                                });
                            target.scope_reference_ordinal_offset.checked_add(5)
                                == Some(side_extent_discriminator_offsets[0])
                                && groups.next().is_some()
                                && groups.next().is_none()
                        })
                });
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
                    && scope.reference_members().values().rev().nth(1)
                        == Some(&operation.tolerance_record_index)
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
                    && scope.reference_members().values().nth(1)
                        == Some(&operation.angle_owner_record_index)
                    && operation.distance_owner_record_index != operation.angle_owner_record_index
                    && !operation.edge_group_record_indices.is_empty()
                    && operation
                        .edge_group_record_indices
                        .iter()
                        .all(|record_index| {
                            scope
                                .reference_members()
                                .values()
                                .any(|value| value == record_index)
                        })
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
        } && scope
            .reference_members()
            .values()
            .all(|record_index| records_by_index.contains_key(&(native_stream, *record_index)))
            && records_by_index.contains_key(&(native_stream, scope.record_index))
            && entity_link.unwrap_or(
                scope.kind() != crate::records::feature::scope::DesignFeatureKind::Sketch,
            )
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
                || placements_by_scope.contains_key(&(native_stream, scope.record_index)))
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
        || !(ctx.decode.contains(plain, &construction.point_record_index, "find F3D work-point scope reference")?
            || ctx.decode.any_by(located, |value| Ok(value.value == construction.point_record_index), "find F3D located work-point scope reference")?)
        || ctx.decode.get_hash_map(&ctx.records_by_index, &(native_stream, construction.point_record_index), "find F3D work-point record")?
            .is_none_or(|header| header.byte_offset != construction.point_record_byte_offset)
    {
        return Ok(false);
    }

    ctx.decode.all_by(construction.rule.inputs(), |input| {
            let header = ctx.decode.get_hash_map(&ctx.records_by_index, &(native_stream, input.record_index()), "find F3D work-point input record")?;
            Ok((ctx.decode.contains(plain, &input.record_index(), "find F3D work-point input reference")?
                || ctx.decode.any_by(located, |value| Ok(value.value == input.record_index()), "find F3D located work-point input reference")?)
                && input.reference_offset > construction.reference_type_offset
                && header.is_some() && match input.carrier() {
                None => true,
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::EdgeRecipe {
                        operand_id,
                    },
                ) => ctx.decode.any_by(&native.design_edge_operands, |operand| {
                    Ok(ctx.decode.equal(&operand.id, operand_id, "compare F3D work-point edge recipe identity")?
                        && ctx.decode.equal(design_stream(&operand.id), native_stream, "compare F3D work-point edge recipe stream")?
                        && operand.scope_record_index == scope.record_index
                        && operand.record_index() == input.record_index())
                }, "find F3D work-point edge recipe")?,
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::VertexRecipe {
                        recipe: vertex,
                    },
                ) => valid_vertex_recipe(ctx, scope, native_stream, input.record_index(), vertex)?,
                Some(records::feature::work_geometry::DesignWorkPointInputCarrier::WorkPlane {
                    selection,
                }) => {
                    (match header {
                        Some(header) => ctx.decode.equal(&header.class_tag, &selection.class_tag, "compare F3D work-point selection class tags")?
                            && selection.asset_id_offset() > header.byte_offset,
                        None => false,
                    }) && u32::try_from(selection.primary_identity)
                        .ok()
                        .and_then(|identity| identity.checked_add(1))
                        == Some(selection.work_plane_scope_record_index)
                        && ctx.decode.any_by(&native.design_parameter_scopes, |plane| {
                            Ok(ctx.decode.equal(design_stream(&plane.id), native_stream, "compare F3D work-point plane stream")?
                                && ctx.decode.equal(&plane.kind(), &crate::records::feature::scope::DesignFeatureKind::WorkPlane, "compare F3D work-point plane kind")?
                                && plane.record_index == selection.work_plane_scope_record_index)
                        }, "find F3D work-point plane scope")?
                }
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::SketchPoint {
                        selection,
                    },
                ) => {
                    (match header {
                        Some(header) => ctx.decode.equal(&header.class_tag, &selection.class_tag, "compare F3D work-point selection class tags")?
                            && selection.asset_id_offset() > header.byte_offset,
                        None => false,
                    }) && u32::try_from(selection.point_persistent_id).is_ok()
                        && !ctx.decode.trim_text(&selection.point_native_id, "validate F3D work-point sketch identity")?.is_empty()
                        && ctx.decode.any_by(&native.sketch_points, |point| {
                            Ok(ctx.decode.equal(&point.id, &selection.point_native_id, "compare F3D work-point sketch identity")?
                                && ctx.decode.equal(design_stream(&point.id), native_stream, "compare F3D work-point sketch stream")?
                                && point.owner_reference == Some(selection.sketch_record_index)
                                && point.persistent_id() == Some(selection.point_persistent_id))
                        }, "find F3D work-point sketch point")?
                }
            })
        }, "validate F3D work-point inputs")
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
    let Some(placement_header) = ctx.decode.get_hash_map(&ctx.records_by_index, &(native_stream, *placement_record_index), "find F3D work-plane placement record")?
    else {
        return Ok(false);
    };
    let transform_offset = frame.work_plane_transform_offset;
    let Some(owner) = ctx.decode.find_by(&ctx.native.design_parameter_owners, |owner| {
        Ok(ctx.decode.equal(design_stream(owner.id()), native_stream, "compare F3D work-plane owner stream")?
            && owner.record_index() == *extra_offset
            && owner.scope_record_index() == scope.record_index
            && owner.evaluated_value().get() == 0.0)
    }, "find F3D work-plane parameter owner")? else {
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
        && ctx.decode.all_by(inputs, |input| valid_vertex_recipe(ctx, scope, native_stream, input.record_index(), input), "validate F3D work-plane vertex inputs")?
        && ctx.decode.any_by(&ctx.native.design_parameters, |parameter| {
            Ok(ctx.decode.equal(design_stream(&parameter.id), native_stream, "compare F3D work-plane parameter stream")?
                && parameter.record_index == owner.parameter_record_index()
                && parameter.owner_record_index() == Some(owner.record_index())
                && ctx.decode.equal(parameter.source_kind(), "ExtraOffset", "compare F3D work-plane parameter kind")?
                && parameter.evaluated_value().get() == 0.0)
        }, "find F3D work-plane ExtraOffset parameter")?)
}

fn valid_vertex_recipe(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
    record_index: u32,
    vertex: &records::feature::work_geometry::DesignVertexRecipe,
) -> Result<bool, CodecError> {
    let native = ctx.native;
    let header = ctx.decode.get_hash_map(&ctx.records_by_index, &(native_stream, record_index), "find F3D validation record index")?;
    let recipe = ctx.decode.get_hash_map(&ctx.recipes_by_id, vertex.recipe_id.as_str(), "find F3D validation record index")?;
    let mut expected_reference_storage = ctx.decode.reserve_scoped(0, "hold F3D vertex recipe expected references")?;
    let mut expected_references = expected_reference_storage.with_storage(|| {
        design::decode::dimension_frames::decode_recipe_references_charged(
            ctx.decode,
            &vertex.recipe_prefix_bytes,
            vertex.recipe_prefix_offset(),
        )
    })?;
    let mut reference_positions_storage = ctx.decode.reserve_scoped(0, "scan F3D recipe references")?;
    let reference_positions = reference_positions_storage.with_storage(|| {
        ctx.decode.collect_vec(expected_references.iter().enumerate().map(|(index, _)| index), "scan F3D recipe references")
    })?;
    for position in ctx.decode.admit_iter(&reference_positions, "scan F3D recipe references")? {
        let reference = &mut expected_references[*position];
        expected_reference_storage.with_storage(|| {
            design::decode::dimension_frames::bind_recipe_reference_candidates_charged(
                ctx.decode,
                reference,
                &native.persistent_subentity_tags,
                Some(&scope.id),
            )
        })?;
    }
    let prefix_length = u64::try_from(vertex.recipe_prefix_bytes.len()).ok();
    let family_name_length = u64::try_from(design::construction_recipe_family_name_len(
        records::recipes::ConstructionRecipeKind::Vertex,
    ))
    .ok();
    let program_byte_length = u64::try_from(vertex.recipe_program.len())
        .ok()
        .and_then(|length| length.checked_mul(4));
    let resolution_is_valid = match vertex.resolution {
        None => true,
        Some(resolution) => {
            let state_id = resolution.state_id;
            let vertex_slot = resolution.vertex_slot();
            let mut selected = None;
            let mut ambiguous = false;
            'histories: for source in ctx.decode.admit_iter(&native.asm_histories, "find F3D vertex recipe history")? {
                for state in ctx.decode.admit_iter(&source.states, "find F3D vertex recipe state")? {
                    if state.state_id != state_id { continue; }
                    if selected.replace(state).is_some() {
                        ambiguous = true;
                        break 'histories;
                    }
                }
            }
            match selected {
                Some(state) if !ambiguous => match state.topology() {
                    Some(topology) => ctx.decode.contains(&topology.vertices, &vertex_slot, "find F3D vertex recipe historical vertex")?,
                    None => history::projection_was_finalized(ctx.decode, &native.asm_histories)?,
                },
                _ => false,
            }
        }
    };
    Ok(vertex.record_index() == record_index
        && (match header {
            Some(header) => header.byte_offset == vertex.byte_offset()
                && ctx.decode.equal(&header.class_tag, &vertex.class_tag, "compare F3D vertex recipe class tags")?,
            None => false,
        })
        && prefix_length.is_some_and(|prefix_length| {
            vertex
                .recipe_prefix_offset()
                .checked_add(prefix_length)
                .zip(recipe.and_then(|recipe| recipe.byte_offset.checked_sub(4)))
                .is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end)
        })
        && ctx.decode.equal(&vertex.recipe_references, &expected_references, "compare F3D vertex recipe references")?
        && resolution_is_valid
        && match recipe {
            None => false,
            Some(recipe) => {
            ctx.decode.equal(design_stream(&recipe.id), native_stream, "compare F3D vertex recipe stream")?
                && recipe.kind == records::recipes::ConstructionRecipeKind::Vertex
                && recipe.byte_offset > vertex.recipe_record_byte_offset()
                && recipe.byte_offset < vertex.next_byte_offset()
                && family_name_length.is_some_and(|family_name_length| {
                    recipe
                        .byte_offset
                        .checked_add(family_name_length)
                        .is_some_and(|expected_offset| {
                            vertex.recipe_program_offset == expected_offset
                        })
                })
            }
        }
        && program_byte_length.is_some_and(|program_byte_length| {
            program_byte_length != 0
                && vertex
                    .recipe_program_offset
                    .checked_add(program_byte_length)
                    .is_some_and(|expected_offset| expected_offset == vertex.next_byte_offset())
        }))
}

fn validate_component_occurrences(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
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
    for occurrence in ctx.decode.admit_iter(&ctx.native.design_component_occurrences, "scan F3D design component occurrences")? {
        let stream = design_stream(&occurrence.id);
        let key = {
            let decode = ctx.decode;
            decode.format_retained(
                format_args!("{}", LowerAscii(occurrence.occurrence_guid.as_str())),
                "retain F3D occurrence GUID index key",
            )?
        };
        let unique_identity = ctx.decode.insert_hash_set(
            &mut identities,
            (stream, key),
            "index F3D occurrence GUIDs",
        )?;
        let unique_record = if unique_identity {
            ctx.decode.insert_hash_set(
                &mut record_indices,
                (stream, occurrence.record_index),
                "index F3D occurrence record indices",
            )?
        } else {
            false
        };
        let valid = unique_identity && unique_record && match occurrence.placement() {
            records::feature::assembly_features::DesignComponentOccurrencePlacement::Base => true,
            records::feature::assembly_features::DesignComponentOccurrencePlacement::Explicit {
                ordinal,
                ..
            } => occurrence.class_tag.as_str() == "327" || ordinal.get() > 1,
        };
        // The duplicated references must agree within one carrier, which
        // the decoder checks. The component GUID is the reusable-definition
        // identity; a different carrier-local component-record reference
        // does not contradict it.
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design component occurrence has an invalid fixed frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&occurrence.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

fn valid_component_pattern_occurrences(
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

/// Validate Extrude selection groups and their counted member frames.
fn validate_extrude_selection_groups(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut group_slots = HashSet::new();
    for group in ctx.decode.admit_iter(&native.design_extrude_selection_groups, "scan F3D design extrude selection groups")? {
        let native_stream = design_stream(&group.id);
        let scope = ctx.decode.get_hash_map(scopes_by_index, &(native_stream, group.scope_record_index), "find F3D validation record index")?;
        let header = ctx.decode.get_hash_map(records_by_index, &(native_stream, group.record_index), "find F3D validation record index")?;
        let frame_valid = match scope {
            None => false,
            Some(scope) => {
            design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Extrude)
                && match usize::try_from(group.scope_reference_ordinal) {
                    Ok(ordinal) => {
                        let (plain, located) = scope.reference_members().storage_slices();
                        ctx.decode.admit_iter(plain, "scan F3D Extrude group scope references")?
                            .chain(ctx.decode.admit_iter(located, "scan F3D Extrude group located scope references")?.map(|member| &member.value))
                            .nth(ordinal) == Some(&group.record_index)
                    }
                    Err(_) => false,
                }
            }
        } && match header {
            Some(header) => header.byte_offset == group.byte_offset()
                && ctx.decode.equal(&header.class_tag, &group.class_tag, "compare F3D Extrude group class tags")?,
            None => false,
        } && ctx.decode.all_by(group.members(), |member| {
            ctx.decode.contains_key_hash_map(records_by_index, &(native_stream, member.value), "find F3D Extrude group member record")
        }, "validate F3D Extrude group member records")?;
        let valid = if frame_valid {
            ctx.decode.insert_hash_set(
                &mut group_slots,
                (
                    native_stream,
                    group.scope_record_index,
                    group.scope_reference_ordinal,
                ),
                "index F3D Extrude selection group slots",
            )?
        } else {
            false
        };
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection group has an invalid counted frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate construction operand groups and their role discriminators.
fn validate_construction_operand_groups(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut operand_group_slots = HashSet::new();
    for group in ctx.decode.admit_iter(&native.design_construction_operand_groups, "scan F3D design construction operand groups")? {
        let native_stream = design_stream(&group.id);
        let scope = ctx.decode.get_hash_map(scopes_by_index, &(native_stream, group.scope_record_index), "find F3D validation record index")?;
        let header = ctx.decode.get_hash_map(records_by_index, &(native_stream, group.record_index), "find F3D validation record index")?;
        let frame = &group.frame;
        let member_run_end = group.members().last().map_or_else(
            || frame.member_count_offset.checked_add(4),
            |member| member.offset.checked_add(10),
        );
        let frame_valid = group
            .byte_offset
            .checked_add(
                if match scope {
                    None => false,
                    Some(scope) => {
                    ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::SurfaceStitch, "compare F3D construction scope kind")?
                        || (ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::SplitFace, "compare F3D construction scope kind")?
                            && group.role() == DesignOperandRole::ROLE_0X21)
                        || (ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::Split, "compare F3D construction scope kind")?
                            && matches!(
                                group.role(),
                                DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X21
                            ))
                    }
                } {
                    88
                } else {
                    21
                },
            )
            .is_some_and(|expected_offset| frame.member_count_offset == expected_offset)
            && group.members().first().is_none_or(|member| {
                frame
                    .member_count_offset
                    .checked_add(5)
                    .is_some_and(|expected_offset| member.offset == expected_offset)
            })
            && frame.trailing_records().first().is_none_or(|record| {
                group
                    .role_offset()
                    .checked_sub(10)
                    .is_some_and(|expected_offset| record.offset == expected_offset)
            })
            && member_run_end.is_some_and(|end| group.role_offset() >= end)
            && group.role().raw().trailing_zeros() >= 32
            && frame
                .opaque_scalar_offset()
                .checked_add(8)
                .is_some_and(|expected_offset| group.paired_byte_offset > expected_offset)
            && ctx.decode.admit_iter(&frame.auxiliary_records, "scan F3D construction auxiliary records")?
                .chain(ctx.decode.admit_iter(frame.trailing_records(), "scan F3D construction trailing records")?)
                .try_fold(true, |valid, record| {
                    if !valid { return Ok(false); }
                    ctx.decode.contains_key_hash_map(records_by_index, &(native_stream, record.value), "find F3D construction tail record")
                })?
            && ctx.decode.all_by(frame.trailing_transforms(), |transform| {
                Ok(
                ctx.decode.any_by(frame.trailing_records(), |record| Ok(record.value == transform.record_index()), "find F3D construction tail reference")?
                    && match ctx.decode.get_hash_map(records_by_index, &(native_stream, transform.record_index()), "find F3D construction tail record")? {
                        Some(header) => header.byte_offset == transform.byte_offset()
                            && ctx.decode.equal(&header.class_tag, &transform.class_tag, "compare F3D construction tail class tags")?,
                        None => false,
                    }
                    && match ctx.decode.get_hash_map(records_by_index, &(native_stream, transform.following_record_index()), "find F3D construction tail record")? {
                        Some(header) => header.byte_offset == transform.following_byte_offset()
                            && ctx.decode.equal(&header.class_tag, &transform.following_class_tag, "compare F3D construction tail class tags")?,
                        None => false,
                    }
                )
            }, "validate F3D construction trailing_transforms")?
            && ctx.decode.all_by(frame.trailing_dual_transforms(), |transform| {
                Ok(
                ctx.decode.any_by(frame.trailing_records(), |record| Ok(record.value == transform.record_index), "find F3D construction tail reference")?
                    && match ctx.decode.get_hash_map(records_by_index, &(native_stream, transform.record_index), "find F3D construction tail record")? {
                        Some(header) => header.byte_offset == transform.byte_offset
                            && ctx.decode.equal(&header.class_tag, &transform.class_tag, "compare F3D construction tail class tags")?,
                        None => false,
                    }
                    && transform
                        .byte_offset
                        .checked_add(21)
                        .is_some_and(|expected_offset| {
                            transform.first_transform_offset == expected_offset
                        })
                    && transform
                        .byte_offset
                        .checked_add(149)
                        .is_some_and(|expected_offset| {
                            transform.second_transform_offset == expected_offset
                        })
                )
            }, "validate F3D construction trailing_dual_transforms")?
            && ctx.decode.all_by(frame.trailing_flags(), |flag| {
                Ok(
                ctx.decode.any_by(frame.trailing_records(), |record| Ok(record.value == flag.record_index), "find F3D construction tail reference")?
                    && match ctx.decode.get_hash_map(records_by_index, &(native_stream, flag.record_index), "find F3D construction tail record")? {
                        Some(header) => header.byte_offset == flag.byte_offset
                            && ctx.decode.equal(&header.class_tag, &flag.class_tag, "compare F3D construction tail class tags")?,
                        None => false,
                    }
                    && flag
                        .byte_offset
                        .checked_add(22)
                        .is_some_and(|expected_offset| flag.value_offset == expected_offset)
                )
            }, "validate F3D construction trailing_flags")?
            && ctx.decode.all_by(frame.auxiliary_paths(), |path| {
                Ok(
                ctx.decode.any_by(&frame.auxiliary_records, |record| Ok(record.value == path.record_index()), "find F3D construction tail reference")?
                    && match ctx.decode.get_hash_map(records_by_index, &(native_stream, path.record_index()), "find F3D construction tail record")? {
                        Some(header) => header.byte_offset == path.byte_offset()
                            && ctx.decode.equal(&header.class_tag, &path.class_tag, "compare F3D construction tail class tags")?,
                        None => false,
                    }
                    && path.scope_record_index == group.scope_record_index
                    && ctx.decode.contains_key_hash_map(records_by_index, &(native_stream, path.nested_record_index()), "find F3D nested construction record")?
                    && match ctx.decode.get_hash_map(records_by_index, &(native_stream, path.following_record_index()), "find F3D construction tail record")? {
                        Some(header) => header.byte_offset == path.following_byte_offset()
                            && ctx.decode.equal(&header.class_tag, &path.following_class_tag, "compare F3D construction tail class tags")?,
                        None => false,
                    }
                )
            }, "validate F3D construction auxiliary_paths")?;
        let valid = match scope {
            None => false,
            Some(scope) => {
            let role_is_valid = match design::design_feature_family(&scope.kind()) {
                Some(design::DesignFeatureFamily::Extrude) => {
                    match group.operand_role {
                        records::topology::construction::DesignConstructionOperandRole::ExtrudeBodiesA
                        | records::topology::construction::DesignConstructionOperandRole::ExtrudeBodiesB => true,
                        records::topology::construction::DesignConstructionOperandRole::ExtrudeProfile => {
                            scope.extrude_profile().is_none_or(|profile| {
                                group.members().first().map(|member| &member.value)
                                    == Some(&profile.record_index)
                            })
                        }
                        records::topology::construction::DesignConstructionOperandRole::ExtrudeFaces {
                            encoding,
                            ..
                        } => match encoding {
                            records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces => true,
                            records::topology::extrude_selection::DesignExtrudeFaceEncoding::LegacyTermination => {
                                scope.extrude_prologue().and_then(
                                    records::feature::extrude::DesignExtrudePrologue::extent,
                                ) == Some(
                                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace,
                                ) || is_class_296_two_sided_to_faces_scope(scope)
                            }
                            records::topology::extrude_selection::DesignExtrudeFaceEncoding::SelectedStart => {
                                scope
                                    .extrude_prologue()
                                    .map(records::feature::extrude::DesignExtrudePrologue::start)
                                    == Some(records::feature::extrude::DesignExtrudeStart::FromFace)
                            }
                        },
                        records::topology::construction::DesignConstructionOperandRole::Other(role) => {
                            role == DesignOperandRole::ROLE_0X5
                        }
                    }
                }
                Some(
                    design::DesignFeatureFamily::Fillet | design::DesignFeatureFamily::Chamfer,
                ) => group.extrude_role().is_none(),
                Some(design::DesignFeatureFamily::Coil) => {
                    group.role()
                        == if ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::CoilPrimitive, "compare F3D construction scope kind")?
                            && scope.reference_members().len() == 10
                            && scope.coil_operation_offset() == scope.byte_offset().checked_add(22)
                        {
                            DesignOperandRole::BODIES_A
                        } else {
                            DesignOperandRole::BODIES_B
                        }
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Move) => {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::OffsetFaces) => {
                    group.role() == DesignOperandRole::ROLE_0X10 && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Draft) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X10 | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::ReplaceFace) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X10
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Revolve) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X21
                            | DesignOperandRole::PROFILE
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Shell) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X10
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Thicken) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X12
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Loft) => {
                    (!scope.has_path_construction()
                        || matches!(
                            group.role(),
                            DesignOperandRole::BODIES_A
                                | DesignOperandRole::ROLE_0X5
                                | DesignOperandRole::PROFILE
                                | DesignOperandRole::ROLE_0X43
                                | DesignOperandRole::ROLE_0X7
                        ))
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Sweep) => {
                    (!scope.has_path_construction()
                        || matches!(
                            group.role(),
                            DesignOperandRole::BODIES_A
                                | DesignOperandRole::ROLE_0X5
                                | DesignOperandRole::FACES
                                | DesignOperandRole::PROFILE
                        ))
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Pipe) => {
                    group.role() == DesignOperandRole::ROLE_0X5 && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::CircularPattern) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::RectangularPattern) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Mirror) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::ROLE_0X5
                            | DesignOperandRole::BODIES_B
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfacePatch) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::PROFILE
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfaceOffset) => {
                    group.role() == DesignOperandRole::PROFILE && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfaceRuled) => {
                    group.role() == DesignOperandRole::BODIES_B
                        && group.extrude_role().is_none()
                        && match scope.ruled_surface_operation() {
                            Some(operation) => ctx.decode.contains(&operation.edge_group_record_indices, &group.record_index, "find F3D construction operand role group")?,
                            None => false,
                        }
                }
                Some(design::DesignFeatureFamily::BoundaryFill) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X5
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Hole) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X5
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfaceTrim) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Split) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::ROLE_0X9
                            | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Scale) => {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Thread) => {
                    group.role() == DesignOperandRole::ROLE_0X10
                        && group.extrude_role().is_none()
                        && match scope.thread_construction() {
                            Some(construction) => ctx.decode.contains(&construction.face_group_record_indices, &group.record_index, "find F3D construction operand role group")?,
                            None => false,
                        }
                }
                Some(design::DesignFeatureFamily::SheetMetalEdgeFlange) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X21
                            | DesignOperandRole::ROLE_0X43
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SheetMetalHem) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_B | DesignOperandRole::ROLE_0X43
                    ) && group.extrude_role().is_none()
                }
                Some(_) => false,
                None if ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::RemoveBody, "compare F3D construction scope kind")? =>
                {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                None if ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::SurfaceStitch, "compare F3D construction scope kind")? =>
                {
                    group.role() == DesignOperandRole::ROLE_0X5 && group.extrude_role().is_none()
                }
                None if ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::SplitFace, "compare F3D construction scope kind")? =>
                {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X10 | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                None if matches!(
                    scope.kind(),
                    crate::records::feature::scope::DesignFeatureKind::DeleteFace
                        | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                ) =>
                {
                    group.role() == DesignOperandRole::ROLE_0X10 && group.extrude_role().is_none()
                }
                None if ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::Decal, "compare F3D construction scope kind")? =>
                {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                None if ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::BaseFlange, "compare F3D construction scope kind")? =>
                {
                    group.role() == DesignOperandRole::PROFILE
                        && group.extrude_role().is_none()
                        && match scope.base_flange_profile().as_ref() {
                            Some(profile) => match group.members() {
                                [member] => ctx.decode.equal(&member.value, &profile.record_index, "compare F3D BaseFlange profile member")?,
                                _ => false,
                            },
                            None => false,
                        }
                }
                None if ctx.decode.equal(&scope.kind(), &crate::records::feature::scope::DesignFeatureKind::Hem, "compare F3D construction scope kind")? => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_B | DesignOperandRole::ROLE_0X43
                    ) && group.extrude_role().is_none()
                }
                None => false,
            };
            (design::design_feature_family(&scope.kind()).is_some()
                || matches!(
                    scope.kind(),
                    crate::records::feature::scope::DesignFeatureKind::RemoveBody
                        | crate::records::feature::scope::DesignFeatureKind::SurfaceStitch
                        | crate::records::feature::scope::DesignFeatureKind::SplitFace
                        | crate::records::feature::scope::DesignFeatureKind::DeleteFace
                        | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                        | crate::records::feature::scope::DesignFeatureKind::Decal
                        | crate::records::feature::scope::DesignFeatureKind::BaseFlange
                        | crate::records::feature::scope::DesignFeatureKind::EdgeFlange
                        | crate::records::feature::scope::DesignFeatureKind::Hem
                ))
                && role_is_valid
                && match usize::try_from(group.scope_reference_ordinal) {
                    Ok(ordinal) => {
                        let (plain, located) = scope.reference_members().storage_slices();
                        ctx.decode.admit_iter(plain, "scan F3D construction scope references")?
                            .chain(ctx.decode.admit_iter(located, "scan F3D construction located scope references")?.map(|member| &member.value))
                            .nth(ordinal) == Some(&group.record_index)
                    }
                    Err(_) => false,
                }
                && ctx.decode.all_by(group.members(), |member| {
                    let (plain, located) = scope.reference_members().storage_slices();
                    Ok(ctx.decode.contains(plain, &member.value, "find F3D construction scope member")?
                        || ctx.decode.any_by(located, |value| Ok(value.value == member.value), "find F3D located construction scope member")?)
                }, "validate F3D construction scope members")?
            }
        } && match header {
            Some(header) => header.byte_offset == group.byte_offset
                && ctx.decode.equal(&header.class_tag, &group.class_tag, "compare F3D construction group class tags")?,
            None => false,
        } && frame_valid
            && !group.members().is_empty()
            && {
                let mut seen = HashSet::new();
                for member in ctx.decode.admit_iter(group.members(), "scan F3D construction operand members")? {
                    ctx.decode.insert_hash_set(&mut seen, member.value, "index F3D construction operand group members")?;
                }
                seen.len() == group.members().len()
            }
            && ctx.decode.all_by(group.members(), |member| {
                ctx.decode.contains_key_hash_map(records_by_index, &(native_stream, member.value), "find F3D construction group member record")
            }, "validate F3D construction group member records")?
            && ctx.decode.insert_hash_set(&mut operand_group_slots, (native_stream, group.scope_record_index, group.scope_reference_ordinal), "index F3D construction operand group slots")?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has an invalid frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate path-feature operand roles against the scope construction.
///
/// The role grammar is shared with the fixed Loft projector. Keeping the
/// predicate independent of native byte offsets lets validation reject a
/// malformed role combination without rejecting a valid section/guide mix.
pub(crate) fn loft_operand_roles_are_valid(
    operation: records::feature::extrude::DesignExtrudeOperation,
    groups: &[(DesignOperandRole, usize)],
) -> bool {
    const BODY: DesignOperandRole = DesignOperandRole::BODIES_A;
    const SECTION: DesignOperandRole = DesignOperandRole::PROFILE;
    const FACE_SECTION: DesignOperandRole = DesignOperandRole::ROLE_0X43;
    const GUIDE: DesignOperandRole = DesignOperandRole::ROLE_0X5;
    const CENTERLINE: DesignOperandRole = DesignOperandRole::ROLE_0X7;

    let body_count = groups.iter().filter(|(role, _)| *role == BODY).count();
    let expected_body_count =
        usize::from(operation != records::feature::extrude::DesignExtrudeOperation::NewBody);
    if body_count != expected_body_count {
        return false;
    }
    let operands = || groups.iter().filter(|(role, _)| *role != BODY);
    let section_count = operands()
        .filter(|(role, _)| matches!(*role, SECTION | FACE_SECTION))
        .count();
    let guide_count = operands().filter(|(role, _)| *role == GUIDE).count();
    let centerline_count = operands().filter(|(role, _)| *role == CENTERLINE).count();

    if section_count >= 2 {
        let roles_are_known = operands()
            .all(|(role, _)| matches!(*role, SECTION | FACE_SECTION | GUIDE | CENTERLINE));
        return roles_are_known
            && centerline_count <= 1
            && !(guide_count > 0 && centerline_count > 0)
            && operands().count() == section_count + guide_count + centerline_count;
    }

    if operation != records::feature::extrude::DesignExtrudeOperation::NewBody {
        return false;
    }

    if section_count == 1 && operands().all(|(role, _)| matches!(*role, FACE_SECTION | GUIDE)) {
        let mut point_ordinals = operands()
            .enumerate()
            .filter(|(_, (role, member_count))| *role == GUIDE && *member_count == 1)
            .map(|(ordinal, _)| ordinal);
        let Some(point_ordinal) = point_ordinals.next() else {
            return false;
        };
        return point_ordinals.next().is_none()
            && (point_ordinal == 0 || point_ordinal + 1 == operands().count())
            && operands()
                .enumerate()
                .all(|(ordinal, (role, member_count))| {
                    ordinal == point_ordinal || *role != GUIDE || *member_count != 1
                });
    }

    if section_count == 0 && operands().count() >= 2 {
        let all_sections = operands().all(|(role, _)| *role == SECTION);
        let all_guides = operands().all(|(role, _)| *role == GUIDE);
        return all_sections || all_guides;
    }

    false
}

fn validate_path_feature_operand_roles(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    for scope in ctx.decode.admit_iter(&native.design_parameter_scopes, "scan F3D path feature scopes")?
        .filter(|scope| scope.has_path_construction())
    {
        let native_stream = design_stream(&scope.id);
        let groups = ctx.decode.collect_vec(
            ctx.decode.admit_iter(&native.design_construction_operand_groups, "admit source for collect F3D path-feature operand groups")?
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                }),
            "collect F3D path-feature operand groups",
        )?;
        let role_count = |role| groups.iter().filter(|group| group.role() == role).count();
        let group_roles = ctx.decode.collect_vec(
            groups
                .iter()
                .map(|group| (group.role(), group.members().len())),
            "collect F3D path-feature operand roles",
        )?;
        let valid = match &scope.payload() {
            records::feature::scope::DesignScopePayload::Revolve(Some(
                crate::records::feature::path_features::DesignRevolveConstruction {
                    operation,
                    angle_record_index,
                    opposite_angle,
                    ..
                },
            )) => {
                let body_count = role_count(DesignOperandRole::BODIES_A)
                    + role_count(DesignOperandRole::BODIES_B);
                let expected_body_count = usize::from(
                    *operation != records::feature::extrude::DesignExtrudeOperation::NewBody,
                );
                scope
                    .reference_members()
                    .values()
                    .any(|value| value == angle_record_index)
                    && opposite_angle.is_none_or(|located| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == &located.value)
                    })
                    && groups.len() == 2 + expected_body_count
                    && role_count(DesignOperandRole::ROLE_0X21) == 1
                    && role_count(DesignOperandRole::PROFILE) == 1
                    && body_count == expected_body_count
            }
            records::feature::scope::DesignScopePayload::Loft(Some(
                crate::records::feature::path_features::DesignLoftConstruction {
                    operation, ..
                },
            )) => loft_operand_roles_are_valid(*operation, &group_roles),
            records::feature::scope::DesignScopePayload::Sweep(Some(
                records::feature::scope::DesignSweepScope {
                    construction:
                        Some(records::feature::path_features::DesignSweepConstruction {
                            operation, ..
                        }),
                    ..
                },
            )) => {
                let path_count = role_count(DesignOperandRole::ROLE_0X5);
                let profile_count = role_count(DesignOperandRole::PROFILE);
                let guide_surface_count = role_count(DesignOperandRole::FACES);
                let guide_profile_frame = scope.sweep_profile().is_some_and(|profile| {
                    let profile_groups = || {
                        groups
                            .iter()
                            .filter(|group| group.role() == DesignOperandRole::PROFILE)
                    };
                    profile_groups()
                        .filter(|group| {
                            group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([profile.record_index])
                        })
                        .count()
                        == 1
                        && profile_groups()
                            .filter(|group| {
                                !group
                                    .members()
                                    .iter()
                                    .map(|member| member.value)
                                    .eq([profile.record_index])
                            })
                            .filter(|group| {
                                !group.members().is_empty()
                                    && group.members().iter().map(|member| &member.value).all(
                                        |member| {
                                            native.design_entity_selection_operands.iter().any(
                                                |operand| {
                                                    design_stream(&operand.id) == native_stream
                                                        && operand.scope_record_index
                                                            == scope.record_index
                                                        && operand.group_record_index
                                                            == group.record_index
                                                        && operand.record_index() == *member
                                                },
                                            )
                                        },
                                    )
                            })
                            .count()
                            == 1
                });
                let common_roles =
                    (profile_count == 1 && guide_surface_count == 0 && matches!(path_count, 1 | 2))
                        || (profile_count == 2
                            && guide_surface_count == 1
                            && path_count == 1
                            && guide_profile_frame);
                common_roles
                    && match operation {
                        records::feature::extrude::DesignExtrudeOperation::NewBody => {
                            groups.len() == path_count + profile_count + guide_surface_count
                                && role_count(DesignOperandRole::BODIES_A) == 0
                        }
                        records::feature::extrude::DesignExtrudeOperation::Join
                        | records::feature::extrude::DesignExtrudeOperation::Cut
                        | records::feature::extrude::DesignExtrudeOperation::Intersect => {
                            guide_surface_count == 0
                                && groups.len() == path_count + 2
                                && role_count(DesignOperandRole::BODIES_A) == 1
                        }
                    }
            }
            records::feature::scope::DesignScopePayload::Pipe(Some(
                crate::records::feature::path_features::DesignPipeConstruction {
                    operation, ..
                },
            )) => {
                *operation == records::feature::extrude::DesignExtrudeOperation::NewBody
                    && groups.len() == 1
                    && role_count(DesignOperandRole::ROLE_0X5) == 1
                    && !groups[0].members().is_empty()
            }
            _ => false,
        };
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design path-feature operand roles conflict with its construction",
                Some(
                    ctx.decode
                        .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate Extrude profile, operation, start, and extent operand agreement.
fn validate_extrude_parameter_operands(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    for scope in ctx.decode.admit_iter(&native.design_parameter_scopes, "scan F3D feature parameter scopes")?.filter(|scope| {
        matches!(
            design::design_feature_family(&scope.kind()),
            Some(
                design::DesignFeatureFamily::Extrude
                    | design::DesignFeatureFamily::Fillet
                    | design::DesignFeatureFamily::Chamfer
            )
        )
    }) {
        let native_stream = design_stream(&scope.id);
        if design::design_feature_family(&scope.kind())
            == Some(design::DesignFeatureFamily::Extrude)
        {
            let mut profile_groups = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role()
                            == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Profile)
                });
            let first_profile_group = profile_groups.next();
            let second_profile_group = profile_groups.next();
            let profile_matches_operand = scope.extrude_profile().is_none_or(|profile| {
                match (first_profile_group, second_profile_group) {
                    (None, _) => {
                        usize::try_from(profile.scope_reference_ordinal)
                            .ok()
                            .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                            == Some(&profile.record_index)
                    }
                    (Some(group), None) => {
                        group.members().first().map(|member| &member.value)
                            == Some(&profile.record_index)
                    }
                    (Some(_), Some(_)) => false,
                }
            });
            if !profile_matches_operand {
                ctx.push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Extrude profile conflicts with its profile operand group",
                    Some(
                        ctx.decode
                            .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                    ),
                )?;
            }
            let has_body_operands = native
                .design_construction_operand_groups
                .iter()
                .any(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role()
                            == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Bodies)
                });
            let face_operand_group_count = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role().is_some_and(|role| {
                            matches!(role, records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_))
                        })
                })
                .count();
            let target_shape_group_count = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.role() == DesignOperandRole::ROLE_0X5
                        && group.extrude_role().is_none()
                        && !group.members().is_empty()
                        && group
                            .members()
                            .iter()
                            .map(|member| &member.value)
                            .enumerate()
                            .all(|(ordinal, record_index)| {
                                id_from_index(ordinal).is_some_and(|ordinal| {
                                    native.design_body_recipe_operands.iter().any(|operand| {
                                        design_stream(&operand.id) == native_stream
                                            && operand.scope_record_index == scope.record_index
                                            && operand.owner.group()
                                                == Some((group.record_index, ordinal))
                                            && operand.record_index() == *record_index
                                    })
                                })
                            })
                })
                .count();
            let operation_matches_operands = match scope
                .extrude_prologue()
                .map(records::feature::extrude::DesignExtrudePrologue::operation)
            {
                Some(records::feature::extrude::DesignExtrudeOperation::NewBody) => {
                    !has_body_operands
                }
                Some(
                    records::feature::extrude::DesignExtrudeOperation::Join
                    | records::feature::extrude::DesignExtrudeOperation::Cut
                    | records::feature::extrude::DesignExtrudeOperation::Intersect,
                ) => has_body_operands,
                None => true,
            };
            if !operation_matches_operands {
                ctx.push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Extrude operation conflicts with its body operands",
                    Some(
                        ctx.decode
                            .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                    ),
                )?;
            }
            let Some(prologue) = scope.extrude_prologue() else {
                continue;
            };
            let Some(extrude_extent) = prologue.extent() else {
                continue;
            };
            let parameter_kind_count = |source_kind: &str| {
                native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == scope.record_index
                    })
                    .filter_map(|owner| {
                        parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                    })
                    .filter(|parameter| parameter.source_kind() == source_kind)
                    .count()
            };
            let parameter_kind_values = |source_kind: &'static str| {
                native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == scope.record_index
                    })
                    .filter_map(|owner| {
                        parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                    })
                    .filter(move |parameter| parameter.source_kind() == source_kind)
                    .map(|parameter| parameter.evaluated_value().get())
            };
            let along_count = parameter_kind_count("AlongDistance");
            let against_count = parameter_kind_count("AgainstDistance");
            let profile_offset_count = parameter_kind_count("ProfileOffset");
            let side_one_offset_count = parameter_kind_count("Side1Offset");
            let omitted_zero_side_one_offset =
                crate::design::face_resolve::extrude_omits_zero_side_one_offset(
                    scope,
                    &prologue,
                    side_one_offset_count,
                );
            let mut side_one_offsets = parameter_kind_values("Side1Offset");
            let side_one_offset_is_absent = match side_one_offsets.next() {
                None => true,
                Some(0.0) => side_one_offsets.next().is_none(),
                Some(_) => false,
            };
            let side_two_offset_count = parameter_kind_count("Side2Offset");
            let has_fixed_extrude_parameters = scope.fixed_extrude_parameters().is_some();
            let has_fixed_along = scope
                .fixed_extrude_parameters()
                .as_ref()
                .is_some_and(|fixed| fixed.along_distance.is_some());
            let fixed_along_uses_reversal = scope
                .fixed_extrude_parameters()
                .as_ref()
                .and_then(|fixed| fixed.along_distance.as_ref())
                .is_some_and(|distance| {
                    matches!(
                        distance,
                        records::feature::fixed_parameters::DesignFixedExtrudeDistance::DistanceConstruction(_)
                    )
                });
            let has_one_along_carrier = along_count <= 1 && (along_count == 1 || has_fixed_along);
            let class_296_two_faces_layout = scope
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
                });
            let extent_matches_operands = match extrude_extent {
                records::feature::extrude::DesignExtrudeExtent::OneSidedDistance => {
                    has_one_along_carrier
                        && against_count == 0
                        && side_one_offset_is_absent
                        && (!prologue.direction_reversed() || fixed_along_uses_reversal)
                }
                records::feature::extrude::DesignExtrudeExtent::OneSidedToFace => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && if target_shape_group_count == 1 {
                            side_one_offset_is_absent
                        } else {
                            side_one_offset_count == 1 || omitted_zero_side_one_offset
                        }
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_count == 1
                        && side_two_offset_count == 1
                        && (!prologue.direction_reversed() || class_296_two_faces_layout)
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance => {
                    along_count == 1
                        && !has_fixed_extrude_parameters
                        && against_count == 1
                        && side_one_offset_count == 0
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace => {
                    along_count == 1
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_count == 0
                        && side_two_offset_count == 1
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::SymmetricDistance => {
                    has_one_along_carrier
                        && against_count == 0
                        && side_one_offset_is_absent
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_is_absent
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::OneSidedThroughNext
                | records::feature::extrude::DesignExtrudeExtent::OneSidedThroughAll => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_is_absent
                }
            };
            let extrude_start = prologue.start();
            let start_matches_operands = match extrude_start {
                records::feature::extrude::DesignExtrudeStart::ProfilePlane => {
                    profile_offset_count == 0
                }
                records::feature::extrude::DesignExtrudeStart::OffsetProfilePlane
                | records::feature::extrude::DesignExtrudeStart::FromFace => {
                    profile_offset_count == 1
                }
            };
            let expected_face_group_count = usize::from(
                matches!(
                    extrude_extent,
                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace
                ) && target_shape_group_count == 0,
            ) + 2 * usize::from(matches!(
                extrude_extent,
                records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces
            )) + usize::from(matches!(
                extrude_extent,
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace
            )) + usize::from(matches!(
                extrude_start,
                records::feature::extrude::DesignExtrudeStart::FromFace
            ));
            let mut face_groups = ctx.decode.collect_vec(ctx.decode.admit_iter(&native.design_construction_operand_groups, "scan F3D Extrude face operand groups")?.filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role().is_some_and(|role| {
                            matches!(role, records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_))
                        })
                }), "collect F3D Extrude face operand groups")?;
            ctx.decode.stable_sort_by(
                &mut face_groups,
                |value| &value.scope_reference_ordinal,
                Ord::cmp,
                "f3d extrude face operand groups sort",
            )?;
            let expected_face_roles = match (extrude_start, extrude_extent) {
                (
                    records::feature::extrude::DesignExtrudeStart::FromFace,
                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace,
                ) if target_shape_group_count == 0 => vec![
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Start,
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                ],
                (records::feature::extrude::DesignExtrudeStart::FromFace, _) => {
                    vec![records::topology::extrude_selection::DesignExtrudeFaceRole::Start]
                }
                (_, records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                    if target_shape_group_count == 0 =>
                {
                    vec![records::topology::extrude_selection::DesignExtrudeFaceRole::Termination]
                }
                (_, records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces) => vec![
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                ],
                (_, records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace) => {
                    vec![records::topology::extrude_selection::DesignExtrudeFaceRole::Termination]
                }
                _ => Vec::new(),
            };
            let invalid_face_groups_hide_extent_conflict =
                class_296_two_faces_layout && face_operand_group_count == 0;
            if !invalid_face_groups_hide_extent_conflict
                && (!extent_matches_operands
                    || !start_matches_operands
                    || face_operand_group_count != expected_face_group_count
                    || face_groups
                        .iter()
                        .map(|group| group.extrude_face_role())
                        .ne(expected_face_roles.iter().copied().map(Some)))
            {
                ctx.push_constant_finding(findings, Check::NativeLinks,
                    "Fusion Design Extrude start or extent conflicts with its parameters and face operands",
                    Some(ctx.decode.copy_retained_text(&scope.id, "retain F3D validation entity")?))?;
            }
        }
        if design::design_feature_family(&scope.kind()) == Some(design::DesignFeatureFamily::Sweep)
        {
            let mut profile_groups =
                native
                    .design_construction_operand_groups
                    .iter()
                    .filter(|group| {
                        design_stream(&group.id) == native_stream
                            && group.scope_record_index == scope.record_index
                            && group.role() == DesignOperandRole::PROFILE
                    });
            let profile_group = profile_groups.next();
            let profile_matches_operand = profile_groups.next().is_none()
                && scope.sweep_profile().is_none_or(|profile| {
                    profile_group.is_some_and(|group| {
                        group
                            .members()
                            .iter()
                            .map(|member| member.value)
                            .eq([profile.record_index])
                    })
                });
            if !profile_matches_operand {
                ctx.push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Sweep profile conflicts with its profile operand group",
                    Some(
                        ctx.decode
                            .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                    ),
                )?;
            }
        }
    }
    Ok(())
}

/// Validate Fillet radius-law parameter assignments; returns the assigned groups.
fn validate_fillet_radius_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let construction_groups_by_index = &ctx.operand_groups_by_index;
    let mut fillet_radius_group_records = HashSet::new();
    let mut fillet_radius_group_slots = HashSet::new();
    for assignment in ctx.decode.admit_iter(&native.design_fillet_radius_groups, "scan F3D design fillet radius groups")? {
        let native_stream = design_stream(&assignment.id);
        let scope = ctx.decode.get_hash_map(scopes_by_index, &(native_stream, assignment.scope_record_index), "find F3D validation record index")?;
        let group =
            ctx.decode.get_hash_map(construction_groups_by_index, &(native_stream, assignment.group_record_index), "find F3D validation linked record")?;
        let assignment_parameter = |record_index: u32| -> Result<Option<&records::parameters::DesignParameter>, CodecError> {
            let Some(&parameter) = ctx.decode.get_hash_map(parameters_by_index, &(native_stream, record_index), "find F3D Fillet assignment parameter")? else { return Ok(None); };
            let Some(owner_record_index) = parameter.owner_record_index() else { return Ok(None); };
            let Some(&owner) = ctx.decode.get_hash_map(owners_by_index, &(native_stream, owner_record_index), "find F3D Fillet assignment parameter owner")? else { return Ok(None); };
            Ok((owner.scope_record_index() == assignment.scope_record_index
                && owner.parameter_record_index() == record_index)
                .then_some(parameter))
        };
        let tangency_weight = assignment
            .tangency_weight_parameter_record_index
            .map(&assignment_parameter).transpose()?.flatten();
        let is_fillet = |scope: &&records::feature::scope::DesignParameterScope| {
            design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Fillet)
        };
        let valid = scope.is_some_and(is_fillet)
            && group.is_some_and(|group| {
                group.scope_record_index == assignment.scope_record_index
                    && group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq(assignment.edge_operand_record_indices.iter().copied())
            })
            && match &assignment.law {
                records::topology::fillet::DesignFilletRadiusLaw::Constant {
                    radius_parameter_record_index,
                } => {
                    match assignment_parameter(*radius_parameter_record_index)? { None => false, Some(parameter) => {
                        parameter.source_kind() == "Radius"
                            && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    }}
                }
                records::topology::fillet::DesignFilletRadiusLaw::Chordal {
                    chord_length_parameter_record_index,
                } => match assignment_parameter(*chord_length_parameter_record_index)? { None => false, Some(parameter) => {
                        ctx.decode.equal(parameter.source_kind(), "ChordLen", "compare F3D Fillet parameter kind")?
                            && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    }},
                records::topology::fillet::DesignFilletRadiusLaw::Asymmetric {
                    offset_one_parameter_record_index,
                    offset_two_parameter_record_index,
                } => ctx.decode.all_by(&[
                    (*offset_one_parameter_record_index, "EdgeOffset1"),
                    (*offset_two_parameter_record_index, "EdgeOffset2"),
                ], |(record_index, kind)| {
                    Ok(match assignment_parameter(*record_index)? { None => false, Some(parameter) => {
                        ctx.decode.equal(parameter.source_kind(), *kind, "compare F3D Fillet parameter kind")?
                            && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    }})
                }, "validate F3D asymmetric Fillet parameters")?,
                records::topology::fillet::DesignFilletRadiusLaw::Variable {
                    start_radius_parameter_record_index,
                    end_radius_parameter_record_index,
                    middle: midpoint_records,
                } => {
                    let radius = |record_index: u32, kind: &str| -> Result<Option<f64>, CodecError> {
                        let Some(parameter) = assignment_parameter(record_index)? else { return Ok(None); };
                        Ok((parameter.source_kind() == kind
                                    && parameter
                                        .unit()
                                        .map(|field| field.value.as_str())
                                        .is_some_and(design::feature_project::design_length_unit)
                                    && parameter.evaluated_value().get() >= 0.0)
                            .then_some(parameter.evaluated_value().get()))
                    };
                    let start = radius(*start_radius_parameter_record_index, "StartRadius")?;
                    let end = radius(*end_radius_parameter_record_index, "EndRadius")?;
                    let mut middle_positive = false;
                    let mut previous_position = None;
                    let mut middle_valid = true;
                    for row in ctx.decode.admit_iter(midpoint_records, "scan F3D midpoint records")? {
                        let Some(middle_radius) =
                            radius(row.radius_parameter_record_index, "MidRadius")?
                        else {
                            middle_valid = false;
                            break;
                        };
                        let position = match assignment_parameter(row.parameter_record_index)? { None => None, Some(parameter) => {
                                (parameter.source_kind() == "MidParams"
                                    && parameter.unit().is_none()
                                    && (0.0..1.0).contains(&parameter.evaluated_value().get()))
                            .then_some(parameter.evaluated_value().get())
                        }};
                        let Some(position) = position else {
                            middle_valid = false;
                            break;
                        };
                        if previous_position.is_some_and(|previous| previous >= position) {
                            middle_valid = false;
                            break;
                        }
                        middle_positive |= middle_radius > 0.0;
                        previous_position = Some(position);
                    }
                    start.zip(end).is_some_and(|(start, end)| {
                        middle_valid && (start > 0.0 || end > 0.0 || middle_positive)
                    })
                }
            }
            && (assignment.tangency_weight_parameter_record_index.is_none()
                || match tangency_weight { None => false, Some(parameter) => {
                    parameter.source_kind() == "TangencyWeight" && parameter.unit().is_none()
                }})
            && ctx.decode.insert_hash_set(
                &mut fillet_radius_group_records,
                (native_stream, assignment.group_record_index),
                "index F3D Fillet radius group records",
            )?
            && ctx.decode.insert_hash_set(
                &mut fillet_radius_group_slots,
                (
                    native_stream,
                    assignment.scope_record_index,
                    assignment.group_ordinal,
                ),
                "index F3D Fillet radius group slots",
            )?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Fillet radius group has an invalid parameter assignment",
                Some(
                    ctx.decode
                        .copy_retained_text(&assignment.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(fillet_radius_group_records)
}

/// Report Fillet operand groups that carry no radius assignment.
fn validate_fillet_operand_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    fillet_radius_group_records: &HashSet<(&'a str, u32)>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let scopes_by_index = &ctx.scopes_by_index;
    for group in ctx.decode.admit_iter(&native.design_construction_operand_groups, "scan F3D design construction operand groups")? {
        let native_stream = design_stream(&group.id);
        let scope = ctx.decode.get_hash_map(scopes_by_index, &(native_stream, group.scope_record_index), "find F3D validation record index")?;
        let is_fillet = scope.is_some_and(|scope| {
            design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Fillet)
        });
        let mut fixed_edge_group_count = 0usize;
        let mut is_fixed_edge_group = false;
        if let Some(scope) = scope {
            for candidate in ctx.decode.admit_iter(&native.design_construction_operand_groups, "scan F3D design construction operand groups")? {
                if ctx.decode.equal(design_stream(&candidate.id), native_stream, "compare F3D Fillet operand streams")?
                    && candidate.scope_record_index == scope.record_index
                    && !candidate.members().is_empty()
                    && ctx.decode.all_by(candidate.members(), |member| {
                            let member = &member.value;

                            ctx.decode.any_by(&native.design_edge_operands, |operand| {
                                Ok(
                                ctx.decode.equal(design_stream(&operand.id), native_stream, "compare F3D Fillet operand streams")?
                                    && operand.scope_record_index == scope.record_index
                                    && operand.record_index() == *member)
                            }, "find F3D fixed Fillet edge operand")
                        }, "validate F3D fixed Fillet group members")?
                {
                    fixed_edge_group_count = fixed_edge_group_count.checked_add(1).ok_or_else(|| ctx.decode.refuse_codec_limit("count F3D fixed Fillet edge groups", u64::MAX - 1, u64::MAX))?;
                    is_fixed_edge_group |= candidate.record_index == group.record_index;
                }
            }
        }
        let has_radius_assignment =
            ctx.decode.contains_hash_set(fillet_radius_group_records, &(native_stream, group.record_index), "find F3D Fillet radius assignment")?;
        let has_parameter_owner = ctx.decode.any_by(&native.design_parameter_owners, |owner| {
            Ok(ctx.decode.equal(design_stream(owner.id()), native_stream, "compare F3D Fillet operand streams")?
                && owner.scope_record_index() == group.scope_record_index)
        }, "find F3D Fillet parameter owner")?;
        let sole_compact_group_shape = match scope { None => false, Some(scope) => {
            ctx.decode.fold(&native.design_construction_operand_groups, 0usize, |count, candidate| {
                    if ctx.decode.equal(design_stream(&candidate.id), native_stream, "compare F3D Fillet operand streams")?
                        && candidate.scope_record_index == scope.record_index {
                        count.checked_add(1).ok_or_else(|| ctx.decode.refuse_codec_limit("count F3D Fillet scope operand groups", u64::MAX - 1, u64::MAX))
                    } else { Ok(count) }
                }, "count F3D Fillet scope operand groups")?
                == 1
                && ctx.decode.all_by(group.members(), |member| {
                        ctx.decode.any_by(&native.design_edge_identity_operands, |operand| {
                            Ok(
                            ctx.decode.equal(design_stream(&operand.id), native_stream, "compare F3D Fillet operand streams")?
                                && operand.scope_record_index == scope.record_index
                                && operand.group_record_index == group.record_index
                                && operand.record_index() == member.value)
                        }, "find F3D compact Fillet edge identity operand")
                    }, "validate F3D compact Fillet group members")?
        }};
        let full_round_group_shape = is_fillet
            && group.role() == DesignOperandRole::BODIES_A
            && !has_radius_assignment
            && !has_parameter_owner
            && match scope { None => false, Some(scope) => {
                ctx.decode.fold(&native.design_construction_operand_groups, 0usize, |count, candidate| {
                    if ctx.decode.equal(design_stream(&candidate.id), native_stream, "compare F3D Fillet operand streams")?
                            && candidate.scope_record_index == scope.record_index {
                        count.checked_add(1).ok_or_else(|| ctx.decode.refuse_codec_limit("count F3D Fillet scope operand groups", u64::MAX - 1, u64::MAX))
                    } else { Ok(count) }
                }, "count F3D Fillet scope operand groups")?
                    == 1
                    && group.members().len() == 1
                    && ctx.decode.all_by(&native.design_edge_operands, |operand| { Ok(
                        !ctx.decode.equal(design_stream(&operand.id), native_stream, "compare F3D Fillet operand streams")?
                            || operand.scope_record_index != scope.record_index
                            || operand.record_index() != group.members()[0].value)
                    }, "exclude F3D full-round Fillet edge operand")?
                    && ctx.decode.any_by(&native.design_face_operands, |operand| { Ok(
                        ctx.decode.equal(design_stream(&operand.id), native_stream, "compare F3D Fillet operand streams")?
                            && operand.scope_record_index == scope.record_index
                            && operand.group_record_index() == Some(group.record_index)
                            && operand.group_member_ordinal() == Some(0)
                            && operand.record_index() == group.members()[0].value
                            && operand.recipe_kind
                                == records::recipes::ConstructionRecipeKind::BoundedFace)
                    }, "find F3D full-round Fillet bounded face operand")?
            }};
        let valid_full_round_group = full_round_group_shape
            && !group.frame.variant
            && group.frame.trailing_records().len() == 1
            && group.frame.trailing_flags().len() == 1
            && group.frame.trailing_records()[0].value
                == group.frame.trailing_flags()[0].record_index
            && group.frame.trailing_flags()[0].value
            && ctx.decode.any_by(&native.design_face_operands, |operand| { Ok(
                ctx.decode.equal(design_stream(&operand.id), native_stream, "compare F3D Fillet operand streams")?
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index() == Some(group.record_index)
                    && operand.group_member_ordinal() == Some(0)
                    && operand.record_index() == group.members()[0].value
                    && !operand.resolved_face_slots.is_empty())
            }, "find F3D full-round Fillet resolved face operand")?;
        if full_round_group_shape {
            if !valid_full_round_group {
                ctx.push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Fillet full-round face group is invalid",
                    Some(
                        ctx.decode
                            .copy_retained_text(&group.id, "retain F3D validation entity")?,
                    ),
                )?;
            }
            continue;
        }
        let has_fixed_assignment = match scope.and_then(|scope| scope.fixed_fillet_parameters().map(|fixed| (scope, fixed))) {
            None => false,
            Some((scope, fixed)) => {
                let unique_reference = |record_index: u32| -> Result<bool, CodecError> {
                    let (plain, located) = scope.reference_members().storage_slices();
                    Ok(ctx.decode.admit_iter(plain, "count F3D fixed Fillet scope references")?
                        .chain(ctx.decode.admit_iter(located, "count F3D located fixed Fillet scope references")?.map(|member| &member.value))
                        .filter(|member| **member == record_index).count() == 1)
                };
                ctx.decode.all_by(&native.design_parameter_owners, |owner| {
                    Ok(!ctx.decode.equal(design_stream(owner.id()), native_stream, "compare F3D fixed Fillet owner stream")?
                        || owner.scope_record_index() != scope.record_index)
                }, "exclude F3D fixed Fillet parameter owners")?
                && ctx.decode.all_by(&fixed.groups, |fixed_group| {
                    if let Some(tangency) = fixed_group.tangency_weight() {
                        if !unique_reference(tangency.record_index)? { return Ok(false); }
                    }
                    let radii_valid = match fixed_group.law() {
                        records::feature::fixed_parameters::DesignFixedFilletLaw::Constant(radius) => unique_reference(radius.record_index)?,
                        records::feature::fixed_parameters::DesignFixedFilletLaw::Variable { start, end, intermediate } => {
                            unique_reference(start.record_index)? && unique_reference(end.record_index)?
                                && ctx.decode.all_by(intermediate, |row| unique_reference(row.radius.record_index), "validate F3D fixed Fillet intermediate radius references")?
                        }
                    };
                    Ok(radii_valid && ctx.decode.all_by(fixed_group.law().intermediate(), |row| unique_reference(row.parameter.record_index), "validate F3D fixed Fillet intermediate parameter references")?)
                }, "validate F3D fixed Fillet radius groups")?
                && ((fixed_edge_group_count == fixed.groups.len() && is_fixed_edge_group)
                    || (fixed.groups.len() == 1 && sole_compact_group_shape))
            }
        };
        if is_fillet
            && (group.role() == DesignOperandRole::BODIES_B || sole_compact_group_shape)
            && !has_fixed_assignment
            && !has_radius_assignment
        {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Fillet operand group has no radius assignment",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate construction operand identity chains; returns identity-backed groups.
fn validate_construction_operand_identities<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut operand_identity_groups = HashSet::new();
    for identity in ctx.decode.admit_iter(&native.design_construction_operand_identities, "scan F3D design construction operand identities")? {
        let native_stream = design_stream(&identity.id);
        let group = ctx.decode.get_hash_map(operand_groups_by_index, &(native_stream, identity.group_record_index), "find F3D validation record index")?;
        let selected_scope = match group {
            Some(group) => ctx.decode.get_hash_map(scopes_by_index,
                &(native_stream, group.scope_record_index), "find F3D selected profile scope")?,
            None => None,
        };
        let selected_profile = selected_scope.and_then(|scope| scope.extrude_profile());
        let wrapper_shape = identity.wrappers().iter().all(|wrapper| {
            records_by_index
                .get(&(native_stream, wrapper.record_index))
                .is_some_and(|header| {
                    header.byte_offset == wrapper.byte_offset
                        && header.class_tag == wrapper.class_tag
                })
        });
        let transform = group.and_then(|group| group.frame.trailing_transforms().first());
        let tracking_shape = identity.tracking_path().is_none_or(|path| {
            records_by_index
                .get(&(native_stream, path.wrapper_record_index()))
                .is_some_and(|header| {
                    header.byte_offset == path.wrapper_byte_offset()
                        && header.class_tag == path.wrapper_class_tag
                })
                && records_by_index
                    .get(&(native_stream, path.carrier_record_index()))
                    .is_some_and(|header| {
                        header.byte_offset == path.carrier_byte_offset()
                            && header.class_tag == path.carrier_class_tag
                    })
                && records_by_index
                    .get(&(native_stream, path.following_record_index()))
                    .is_some_and(|header| {
                        header.byte_offset == path.following_byte_offset()
                            && header.class_tag == path.following_class_tag
                    })
        });
        let chain_entry_shape = if let Some(path) = identity.tracking_path() {
            !identity.wrappers().is_empty()
                || (identity.wrappers().is_empty()
                    && transform.is_some_and(|transform| {
                        path.wrapper_record_index() == transform.following_record_index()
                            && path.wrapper_byte_offset() == transform.following_byte_offset()
                            && path.wrapper_class_tag == transform.following_class_tag
                    }))
                || (identity.wrappers().is_empty()
                    && transform.is_none()
                    && group.is_some_and(|group| {
                        group
                            .frame
                            .trailing_records()
                            .first()
                            .map(|record| &record.value)
                            == Some(&path.wrapper_record_index())
                    }))
        } else {
            true
        };
        let following_shape =
            if identity.tracking_path().is_some() || !identity.wrappers().is_empty() {
                true
            } else {
                transform.is_some_and(|transform| {
                    identity.following_record_index() == transform.following_record_index()
                        && identity.following_byte_offset() == transform.following_byte_offset()
                        && *identity.following_class_tag() == transform.following_class_tag
                })
            } && records_by_index
                .get(&(native_stream, identity.following_record_index()))
                .is_some_and(|header| {
                    header.byte_offset == identity.following_byte_offset()
                        && header.class_tag == *identity.following_class_tag()
                });
        let persistent_shape = identity.persistent_identity().is_none_or(|persistent| {
            selected_profile.is_none_or(|profile| profile.asset_id == persistent.asset_id)
                && (persistent.next_record_index != 0
                    || (identity
                        .following_byte_offset()
                        .checked_add(190)
                        .is_some_and(|expected_offset| {
                            persistent.next_byte_offset() == expected_offset
                        })
                        && !records_by_index.values().any(|header| {
                            design_stream(&header.id) == native_stream
                                && header.byte_offset == persistent.next_byte_offset()
                        })))
                && records_by_index
                    .get(&(native_stream, persistent.next_record_index))
                    // The header arena indexes records named by Design entity
                    // reference lists. A nested identity can terminate at a
                    // structurally parsed record that no entity names, so the
                    // arena is not an exhaustive index of terminal records.
                    .is_none_or(|header| header.byte_offset == persistent.next_byte_offset())
        });
        let valid = group.is_some_and(|group| {
            let trailing = group
                .frame
                .trailing_records()
                .first()
                .map(|record| record.value);
            identity
                .wrappers()
                .first()
                .map(|wrapper| wrapper.record_index)
                .or_else(|| {
                    group
                        .frame
                        .trailing_transforms()
                        .first()
                        .map(super::records::topology::construction::DesignConstructionOperandTransform::record_index)
                })
                .or_else(|| {
                    identity
                        .tracking_path()
                        .map(super::records::topology::construction::DesignConstructionTrackingPath::wrapper_record_index)
                })
                == trailing
        }) && wrapper_shape
            && tracking_shape
            && chain_entry_shape
            && following_shape
            && persistent_shape
            && ctx.decode.insert_hash_set(&mut operand_identity_groups, (native_stream, identity.group_record_index), "index F3D construction operand identity groups")?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand identity has an invalid nested frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&identity.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(operand_identity_groups)
}

/// Validate edge identity operands; returns their backing record set.
fn validate_edge_identity_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected_face_operands: &[records::topology::face::DesignFaceOperand],
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let (mut expected_edge_identity_operands, _expected_edge_identity_operands_storage) =
        reload_native_arena(decode, ctx.ir, "design_edge_identity_operands")?;
    let scope_histories = history::bind_scope_histories(
        decode,
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    )?;
    history::selection::bind_edge_identity_history(
        decode,
        &mut expected_edge_identity_operands,
        &native.design_construction_operand_identities,
        &native.design_parameter_scopes,
        &native.asm_histories,
        &scope_histories,
    )?;
    history::selection::bind_edge_identity_bounded_face_rules(
        decode,
        &mut expected_edge_identity_operands,
        expected_face_operands,
    )?;
    let expected_edge_identity_operands = decode.collect_hash_map(
        expected_edge_identity_operands
            .iter()
            .map(|operand| (operand.id.as_str(), operand)),
        "index F3D expected edge identity operands",
    )?;
    let mut edge_identity_slots = HashSet::new();
    let mut edge_identity_records = HashSet::new();
    for operand in ctx.decode.admit_iter(&native.design_edge_identity_operands, "scan F3D design edge identity operands")? {
        let native_stream = design_stream(&operand.id);
        let scope = ctx.decode.get_hash_map(scopes_by_index, &(native_stream, operand.scope_record_index), "find F3D validation record index")?;
        let group = ctx.decode.get_hash_map(operand_groups_by_index, &(native_stream, operand.group_record_index), "find F3D validation record index")?;
        let header = ctx.decode.get_hash_map(records_by_index, &(native_stream, operand.record_index()), "find F3D validation record index")?;
        let valid = scope.is_some_and(|scope| {
            matches!(
                design::design_feature_family(&scope.kind()),
                Some(design::DesignFeatureFamily::Fillet | design::DesignFeatureFamily::Chamfer)
            )
        }) && group.is_some_and(|group| {
            group.scope_record_index == operand.scope_record_index
                && usize::try_from(operand.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                    == Some(&operand.record_index())
        }) && header.is_some_and(|header| {
            header.byte_offset == operand.byte_offset() && header.class_tag == operand.class_tag
        }) && ctx.decode.get_hash_map(&expected_edge_identity_operands, operand.id.as_str(), "find F3D expected operand")?
            == Some(&operand)
            && ctx.decode.insert_hash_set(
                &mut edge_identity_slots,
                (
                    native_stream,
                    operand.group_record_index,
                    operand.group_member_ordinal,
                ),
                "index F3D edge identity slots",
            )?
            && ctx.decode.insert_hash_set(
                &mut edge_identity_records,
                (native_stream, operand.record_index()),
                "index F3D edge identity records",
            )?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge identity operand has an invalid fixed frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&operand.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(edge_identity_records)
}

/// Validate whole-body recipe operands; returns their backing record set.
fn validate_body_recipe_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let (mut expected_operands, _expected_operands_storage) =
        reload_native_arena(decode, ctx.ir, "design_body_recipe_operands")?;
    if let Err(error) = design::decode::operands::bind_body_recipe_operand_candidates(
        ctx.decode,
        &mut expected_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
        &native.design_parameter_scopes,
    ) {
        if matches!(error, CodecError::ResourceLimit(_)) {
            return Err(error);
        }
        let message = ctx.decode.format_retained(
            format_args!("Fusion Design body-recipe candidate binding failed: {error}"),
            "retain F3D body-recipe candidate binding finding",
        )?;
        ctx.decode.push_vec(
            findings,
            Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message,
                entity: None,
            },
            "collect F3D native validation findings",
        )?;
    }
    history::bind_body_recipe_operand_history_candidates(
        decode,
        &mut expected_operands,
        &native.construction_recipes,
        &native.design_parameter_scopes,
        &native.asm_histories,
    )?;
    let expected_operands = decode.collect_hash_map(
        expected_operands
            .iter()
            .map(|operand| (operand.id.as_str(), operand)),
        "index F3D expected body recipe operands",
    )?;
    let mut member_slots = HashSet::new();
    let mut operand_records = HashSet::new();
    for operand in ctx.decode.admit_iter(&native.design_body_recipe_operands, "scan F3D design body recipe operands")? {
        let native_stream = design_stream(&operand.id);
        let scope = ctx.decode.get_hash_map(scopes_by_index, &(native_stream, operand.scope_record_index), "find F3D validation record index")?;
        let header = ctx.decode.get_hash_map(records_by_index, &(native_stream, operand.record_index()), "find F3D validation record index")?;
        let recipe = ctx.decode.get_hash_map(recipes_by_id, operand.recipe_id.as_str(), "find F3D validation record index")?;
        let valid_owner = scope.is_some_and(|scope| match operand.owner {
            records::topology::body_recipe::DesignOperandOwner::Group {
                group_record_index,
                group_member_ordinal,
            } => operand_groups_by_index
                .get(&(native_stream, group_record_index))
                .is_some_and(|group| {
                    group.scope_record_index == operand.scope_record_index
                        && usize::try_from(group_member_ordinal)
                            .ok()
                            .and_then(|ordinal| {
                                group.members().get(ordinal).map(|member| &member.value)
                            })
                            == Some(&operand.record_index())
                }),
            records::topology::body_recipe::DesignOperandOwner::ScopeReference {
                scope_reference_ordinal,
            } => {
                (scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole
                    || (!scope_reference_ordinal.is_multiple_of(2)
                        && scope.combine_operation().is_some_and(|operation| {
                            operation.target_record_index == operand.record_index()
                                || operation
                                    .tools
                                    .iter()
                                    .any(|tool| tool.record_index == operand.record_index())
                        })))
                    && usize::try_from(scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                        == Some(&operand.record_index())
            }
        });
        let valid = valid_owner
            && header.is_some_and(|header| {
                header.byte_offset == operand.byte_offset() && header.class_tag == operand.class_tag
            })
            && body_recipe_reference_table_is_admitted(decode, scope.copied(), operand)?
            && recipe.is_some_and(|recipe| {
                let selector_is_valid = recipe.design.as_ref().is_some_and(|design| {
                    let design_id = &design.id;
                    let design_id_offset = design_id.offset;
                    let Some(selector) = design.selector else {
                        return false;
                    };
                    u64::try_from(design_id.value.len())
                        .ok()
                        .is_some_and(|length| {
                            let selector_follows_id =
                                design_id_offset.checked_add(length) == Some(selector.byte_offset);
                            let prefix_frame =
                                selector.byte_offset.checked_add(20) == Some(recipe.byte_offset);
                            let body_suffix_frame = recipe
                                .byte_offset
                                .checked_add(u64_from_index(b"body_recipe_data".len()))
                                .and_then(|offset| offset.checked_add(12))
                                == Some(design_id_offset)
                                && selector.value == operand.next_record_index();
                            selector_follows_id
                                && selector.value != 0
                                && (prefix_frame || body_suffix_frame)
                        })
                });
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == records::recipes::ConstructionRecipeKind::Body
                    && recipe.byte_offset > operand.context_id_offset()
                    && recipe.byte_offset < operand.next_byte_offset()
                    && selector_is_valid
            })
            && ctx.decode.get_hash_map(&expected_operands, operand.id.as_str(), "find F3D expected operand")? == Some(&operand)
            && ctx.decode.insert_hash_set(
                &mut member_slots,
                (native_stream, operand.scope_record_index, operand.owner),
                "index F3D body recipe member slots",
            )?
            && ctx.decode.insert_hash_set(
                &mut operand_records,
                (native_stream, operand.record_index()),
                "index F3D body recipe records",
            )?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body recipe operand has an invalid nested frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&operand.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(operand_records)
}

/// Report operand groups lacking a typed member carrier.
fn validate_operand_group_carriers<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    operand_identity_groups: &HashSet<(&'a str, u32)>,
    edge_identity_records: &HashSet<(&'a str, u32)>,
    body_recipe_operand_records: &HashSet<(&'a str, u32)>,
    edge_operand_records: &HashSet<(&'a str, u32)>,
    edge_treatment_vertex_records: &HashSet<(&'a str, u32)>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    for group in ctx.decode.admit_iter(&native.design_construction_operand_groups, "scan F3D design construction operand groups")? {
        let native_stream = design_stream(&group.id);
        let mut identity_members = ctx.decode.collect_vec(
            ctx.decode.admit_iter(&native.design_edge_identity_operands, "admit source for collect F3D operand group identity members")?
                .filter(|operand| {
                    design_stream(&operand.id) == native_stream
                        && operand.scope_record_index == group.scope_record_index
                        && operand.group_record_index == group.record_index
                }),
            "collect F3D operand group identity members",
        )?;
        ctx.decode.stable_sort_by(
            &mut identity_members,
            |value| &value.group_member_ordinal,
            Ord::cmp,
            "f3d operand group identity members sort",
        )?;
        let has_exact_identity_members = !group.members().is_empty()
            && identity_members.len() == group.members().len()
            && identity_members
                .iter()
                .enumerate()
                .all(|(ordinal, operand)| {
                    usize::try_from(operand.group_member_ordinal) == Ok(ordinal)
                        && group.members().get(ordinal).map(|member| &member.value)
                            == Some(&operand.record_index())
                        && edge_identity_records.contains(&(native_stream, operand.record_index()))
                });
        let has_exact_entity_selection_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .enumerate()
                .all(|(ordinal, record_index)| {
                    id_from_index(ordinal).is_some_and(|ordinal| {
                        native
                            .design_entity_selection_operands
                            .iter()
                            .any(|operand| {
                                design_stream(&operand.id) == native_stream
                                    && operand.scope_record_index == group.scope_record_index
                                    && operand.group_record_index == group.record_index
                                    && operand.group_member_ordinal == ordinal
                                    && operand.record_index() == *record_index
                            })
                    })
                });
        let has_exact_face_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .enumerate()
                .all(|(ordinal, record_index)| {
                    id_from_index(ordinal).is_some_and(|ordinal| {
                        native.design_face_operands.iter().any(|operand| {
                            design_stream(&operand.id) == native_stream
                                && operand.scope_record_index == group.scope_record_index
                                && operand.group_record_index() == Some(group.record_index)
                                && operand.group_member_ordinal() == Some(ordinal)
                                && operand.record_index() == *record_index
                        })
                    })
                });
        let has_exact_body_recipe_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .enumerate()
                .all(|(ordinal, record_index)| {
                    id_from_index(ordinal).is_some_and(|ordinal| {
                        native.design_body_recipe_operands.iter().any(|operand| {
                            design_stream(&operand.id) == native_stream
                                && operand.scope_record_index == group.scope_record_index
                                && operand.owner.group() == Some((group.record_index, ordinal))
                                && operand.record_index() == *record_index
                                && body_recipe_operand_records
                                    .contains(&(native_stream, operand.record_index()))
                        })
                    })
                });
        let has_exact_topology_recipe_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .all(|record_index| {
                    edge_operand_records.contains(&(native_stream, *record_index))
                        || edge_treatment_vertex_records.contains(&(native_stream, *record_index))
                });
        let has_exact_sketch_profile_member = group.members().len() == 1
            && ctx
                .scopes_by_index
                .get(&(native_stream, group.scope_record_index))
                .is_some_and(|scope| {
                    scope
                        .extrude_profile()
                        .or(scope.sweep_profile())
                        .or(scope.base_flange_profile())
                        .is_some_and(|profile| {
                            group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([profile.record_index])
                        })
                });
        let has_exact_group_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .all(|record_index| {
                    native
                        .design_construction_operand_groups
                        .iter()
                        .any(|member| {
                            design_stream(&member.id) == native_stream
                                && member.scope_record_index == group.scope_record_index
                                && member.scope_reference_ordinal > group.scope_reference_ordinal
                                && member.record_index == *record_index
                        })
                });
        let has_exact_trailing_carrier = group.frame.trailing_records().is_empty()
            || operand_identity_groups.contains(&(native_stream, group.record_index))
            || (group.frame.trailing_transforms().len()
                + group.frame.trailing_dual_transforms().len()
                + group.frame.trailing_flags().len()
                == group.frame.trailing_records().len()
                && group
                    .frame
                    .trailing_records()
                    .iter()
                    .map(|record| &record.value)
                    .all(|record_index| {
                        group
                            .frame
                            .trailing_transforms()
                            .iter()
                            .any(|transform| transform.record_index() == *record_index)
                            || group
                                .frame
                                .trailing_dual_transforms()
                                .iter()
                                .any(|transform| transform.record_index == *record_index)
                            || group
                                .frame
                                .trailing_flags()
                                .iter()
                                .any(|flag| flag.record_index == *record_index)
                    }));
        let has_exact_member_carrier = operand_identity_groups
            .contains(&(native_stream, group.record_index))
            || has_exact_identity_members
            || has_exact_entity_selection_members
            || has_exact_face_members
            || has_exact_body_recipe_members
            || has_exact_topology_recipe_members
            || has_exact_sketch_profile_member
            || has_exact_group_members;
        if !has_exact_member_carrier {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has no exact typed member",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
        if !has_exact_trailing_carrier {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has no exact trailing carrier",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate Extrude selection members against their resolved sketch geometry.
fn validate_extrude_selection_members(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let groups_by_index = &ctx.groups_by_index;
    let mut member_slots = HashSet::new();
    let mut member_records = HashSet::new();
    for member in ctx.decode.admit_iter(&native.design_extrude_selection_members, "scan F3D design extrude selection members")? {
        let native_stream = design_stream(&member.id);
        let group = ctx.decode.get_hash_map(groups_by_index, &(native_stream, member.group_record_index), "find F3D validation record index")?;
        let header = ctx.decode.get_hash_map(records_by_index, &(native_stream, member.record_index()), "find F3D validation record index")?;
        let selected_scope = match group {
            Some(group) => ctx.decode.get_hash_map(scopes_by_index,
                &(native_stream, group.scope_record_index), "find F3D selected profile scope")?,
            None => None,
        };
        let selected_profile = selected_scope.and_then(|scope| scope.extrude_profile());
        let selected_sketch =
            selected_profile.and_then(|profile| u32::try_from(profile.entity_id.suffix()).ok());
        let point_targets = native.sketch_points.iter().filter_map(|point| {
            (selected_sketch.is_some()
                && design_stream(&point.id) == native_stream
                && point.owner_reference == selected_sketch
                && point.persistent_id() == Some(member.local_id))
            .then_some(records::sketch_relations::SketchRelationOperand::Point {
                record_index: point.record_index,
                persistent_id: point.persistent_id(),
            })
        });
        let curve_targets = native.sketch_curve_identities.iter().filter_map(|curve| {
            (selected_sketch.is_some()
                && design_stream(&curve.id) == native_stream
                && curve.owner_reference == selected_sketch
                && (curve.primary_id.get() == member.local_id
                    || curve.secondary_id != 0 && curve.secondary_id == member.local_id))
                .then_some(records::sketch_relations::SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id.get(),
                    secondary_id: curve.secondary_id,
                })
        });
        let mut targets = point_targets.chain(curve_targets);
        let first_target = targets.next();
        let expected_target = if targets.next().is_none() {
            first_target
        } else {
            None
        };
        let mut expected_identities = ctx.decode.collect_vec(
            ctx.decode.admit_iter(&native.design_construction_operand_identities, "admit source for collect F3D Extrude selection identities")?
                .filter(|identity| {
                    design_stream(&identity.id) == native_stream
                        && identity.following_record_index() == member.record_index()
                        && identity.following_byte_offset() == member.byte_offset()
                        && identity.persistent_identity().is_some_and(|persistent| {
                            persistent.local_id == member.local_id
                                && persistent.asset_id == member.asset_id
                                && persistent.context_id == member.context_id
                        })
                }),
            "collect F3D Extrude selection identities",
        )?;
        ctx.decode.stable_sort_by_key(
            &mut expected_identities,
            |value| value.wrappers().first().map(|wrapper| wrapper.byte_offset),
            Ord::cmp,
            "f3d extrude selection identities sort",
        )?;
        let expected_history = history::selection::historical_extrude_selection_identity_kind(
            ctx.decode,
            member,
            &native.design_component_naming_spaces,
            &native.design_body_bindings,
            &native.asm_histories,
        )?;
        let history_matches = if history::projection_was_finalized(ctx.decode, &native.asm_histories)? {
            if let Some(binding) = member.historical.as_ref() {
                ctx.decode
                    .collect_hash_set(
                        binding.state_ids.iter().copied(),
                        "index F3D Extrude selection history states",
                    )?
                    .len()
                    == binding.state_ids.len()
                    && binding.state_ids.iter().all(|state_id| {
                        native
                            .asm_histories
                            .iter()
                            .flat_map(|history| &history.states)
                            .any(|state| state.state_id == *state_id)
                    })
            } else {
                true
            }
        } else {
            expected_history
                .as_ref()
                .map(|(kind, entity_ref, states)| (*kind, *entity_ref, states.as_slice()))
                == member.historical.as_ref().map(|binding| {
                    (
                        binding.kind,
                        binding.entity_ref,
                        binding.state_ids.as_slice(),
                    )
                })
        };
        let terminal_next = member.next_record_index == 0
            && !records_by_index.values().any(|header| {
                design_stream(&header.id) == native_stream
                    && header.byte_offset == member.next_byte_offset()
            });
        let valid = group.is_some_and(|group| {
            usize::try_from(member.group_member_ordinal)
                .ok()
                .and_then(|ordinal| group.members().get(ordinal))
                .map(|reference| reference.value)
                == Some(member.record_index())
        }) && header.is_some_and(|header| {
            header.byte_offset == member.byte_offset() && header.class_tag == member.class_tag
        }) && selected_profile
            .is_none_or(|profile| profile.asset_id == member.asset_id)
            && member.resolved_geometry == expected_target
            && member
                .operand_identity_ids
                .iter()
                .map(String::as_str)
                .eq(expected_identities
                    .iter()
                    .map(|identity| identity.id.as_str()))
            && history_matches
            && (member.next_record_index != 0 || terminal_next)
            && ctx.decode.insert_hash_set(
                &mut member_slots,
                (
                    native_stream,
                    member.group_record_index,
                    member.group_member_ordinal,
                ),
                "index F3D Extrude selection member slots",
            )?
            && ctx.decode.insert_hash_set(
                &mut member_records,
                (native_stream, member.record_index()),
                "index F3D Extrude selection member records",
            )?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection member has an invalid fixed frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&member.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate entity-selection operand nested frames.
fn validate_entity_selection_operands(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut entity_selection_slots = HashSet::new();
    for operand in ctx.decode.admit_iter(&native.design_entity_selection_operands, "scan F3D design entity selection operands")? {
        let native_stream = design_stream(&operand.id);
        let group = ctx.decode.get_hash_map(operand_groups_by_index, &(native_stream, operand.group_record_index), "find F3D validation record index")?;
        let header = ctx.decode.get_hash_map(records_by_index, &(native_stream, operand.record_index()), "find F3D validation record index")?;
        let valid = group.is_some_and(|group| {
            group.scope_record_index == operand.scope_record_index
                && usize::try_from(operand.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                    == Some(&operand.record_index())
        }) && header.is_some_and(|header| {
            header.byte_offset == operand.byte_offset() && header.class_tag == *operand.class_tag()
        }) && ctx.decode.insert_hash_set(
            &mut entity_selection_slots,
            (
                native_stream,
                operand.group_record_index,
                operand.group_member_ordinal,
            ),
            "index F3D entity selection slots",
        )?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design entity-selection operand has an invalid nested frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&operand.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Report Extrude selection groups with missing or inconsistent members.
fn validate_extrude_selection_group_members(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let members_by_slot = &ctx.members_by_slot;
    for group in ctx.decode.admit_iter(&native.design_extrude_selection_groups, "scan F3D design extrude selection groups")? {
        let native_stream = design_stream(&group.id);
        let complete = (0..group.members().len()).all(|ordinal| {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return false;
            };
            let Some(member) = members_by_slot.get(&(native_stream, group.record_index, ordinal))
            else {
                return false;
            };
            let next = usize::try_from(ordinal)
                .ok()
                .and_then(|ordinal| group.members().get(ordinal + 1));
            next.is_none_or(|next_record_index| {
                let Some(next_ordinal) = ordinal.checked_add(1) else {
                    return false;
                };
                let next_member =
                    members_by_slot.get(&(native_stream, group.record_index, next_ordinal));
                member.next_record_index == next_record_index.value
                    && next_member.is_some_and(|next_member| {
                        member.next_byte_offset() == next_member.byte_offset()
                    })
            })
        });
        let context_id = ctx.decode.get_hash_map(members_by_slot,
            &(native_stream, group.record_index, 0), "find F3D first group member")?
            .map(|member| member.context_id.as_str());
        let context_consistent = context_id.is_some_and(|context_id| {
            (0..group.members().len()).all(|ordinal| {
                u32::try_from(ordinal)
                    .ok()
                    .and_then(|ordinal| {
                        members_by_slot.get(&(native_stream, group.record_index, ordinal))
                    })
                    .is_some_and(|member| member.context_id.as_str() == context_id)
            })
        });
        if !(complete && context_consistent) {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection group has missing members",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Bytes from a face recipe header to the recipe program, by recipe kind.
///
/// `None` states a recipe kind that carries no face operand: it states no
/// program offset at all.
fn recipe_program_operand_length(kind: records::recipes::ConstructionRecipeKind) -> Option<u64> {
    match kind {
        records::recipes::ConstructionRecipeKind::Face => Some(16),
        records::recipes::ConstructionRecipeKind::BoundedFace => Some(24),
        records::recipes::ConstructionRecipeKind::Body
        | records::recipes::ConstructionRecipeKind::Edge
        | records::recipes::ConstructionRecipeKind::Vertex => None,
    }
}

fn recipe_reference_frames_match(
    decode: &DecodeContext<'_>,
    actual: &[records::dimensions::DesignRecipeReference],
    expected: &[records::dimensions::DesignRecipeReference],
    ignore_derived_candidates: bool,
) -> Result<bool, CodecError> {
    if !ignore_derived_candidates {
        return decode.equal(actual, expected, "compare F3D recipe references");
    }
    if actual.len() != expected.len() {
        return Ok(false);
    }
    for (actual, expected) in decode.admit_iter(actual, "scan F3D actual recipe reference frames")?
        .zip(decode.admit_iter(expected, "scan F3D expected recipe reference frames")?)
    {
        if !(actual.selector == expected.selector
            && actual.selector_offset == expected.selector_offset
            && decode.equal(&actual.token, &expected.token, "compare F3D recipe reference token")?
            && actual.token_offset == expected.token_offset
            && actual.design_reference == expected.design_reference
            && actual.design_reference_offset == expected.design_reference_offset)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Validate edge operands and their recipe frames; returns their record set.
fn validate_edge_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let scopes_by_index = &ctx.scopes_by_index;
    let historical_candidates_retained = history::projection_was_finalized(decode, &native.asm_histories)?;
    let mut edge_operand_slots = HashSet::new();
    let mut edge_operand_records = HashSet::new();
    let (mut expected_edge_operands, _expected_edge_operands_storage) =
        reload_native_arena(decode, ctx.ir, "design_edge_operands")?;
    let scope_histories = history::bind_scope_histories(
        decode,
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    )?;
    history::bind_edge_operand_history_candidates(
        decode,
        &mut expected_edge_operands,
        &native.design_parameter_scopes,
        &native.construction_recipes,
        &native.asm_histories,
        &scope_histories,
    )?;
    let expected_edge_operands = decode.collect_hash_map(
        expected_edge_operands
            .iter()
            .map(|operand| (operand.id.as_str(), operand)),
        "index F3D expected edge operands",
    )?;
    for operand in ctx.decode.admit_iter(&native.design_edge_operands, "scan F3D design edge operands")? {
        let native_stream = design_stream(&operand.id);
        let scope = ctx.decode.get_hash_map(scopes_by_index, &(native_stream, operand.scope_record_index), "find F3D validation record index")?;
        let header = ctx.decode.get_hash_map(records_by_index, &(native_stream, operand.record_index()), "find F3D validation record index")?;
        let recipe = ctx.decode.get_hash_map(recipes_by_id, operand.recipe_id.as_str(), "find F3D validation record index")?;
        let design_reference = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0);
        let expected_faces = match design_reference {
            Some(design_reference) => design::decode::operands::edge_operand_candidate_faces(
                ctx.decode,
                design_reference,
                &native.persistent_subentity_tags,
                Some(&operand.id),
            )?,
            None => Vec::new(),
        };
        let mut expected_references =
            design::decode::dimension_frames::decode_recipe_references_charged(
                ctx.decode,
                &operand.recipe_prefix_bytes,
                operand.recipe_prefix_offset(),
            )?;
        if !historical_candidates_retained {
            let mut reference_positions_storage = ctx.decode.reserve_scoped(0, "scan F3D recipe references")?;
            let reference_positions = reference_positions_storage.with_storage(|| {
                ctx.decode.collect_vec(expected_references.iter().enumerate().map(|(index, _)| index), "scan F3D recipe references")
            })?;
            for position in ctx.decode.admit_iter(&reference_positions, "scan F3D recipe references")? {
                let reference = &mut expected_references[*position];
                design::decode::dimension_frames::bind_recipe_reference_candidates_charged(
                    ctx.decode,
                    reference,
                    &native.persistent_subentity_tags,
                    Some(&operand.id),
                )?;
            }
        }
        let expected_surface_patch_recipe_structure = scope
            .filter(|scope| {
                scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
            })
            .map(|_| {
                design::decode::operands::surface_patch_recipe_structure_with_context(
                    ctx.decode,
                    &operand.recipe_program,
                    operand.recipe_references.len(),
                )
            })
            .transpose()?
            .flatten();
        let terminal_group_member = native
            .design_construction_operand_groups
            .iter()
            .any(|group| {
                design_stream(&group.id) == native_stream
                    && group.scope_record_index == operand.scope_record_index
                    && group.members().last().map(|member| &member.value)
                        == Some(&operand.record_index())
            });
        let valid = scope.is_some_and(|scope| {
            design::decode::operands::has_edge_recipe_operands(&scope.kind())
                && usize::try_from(operand.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&operand.record_index())
        }) && header.is_some_and(|header| {
            header.byte_offset == operand.byte_offset() && header.class_tag == operand.class_tag
        }) && (operand
            .record_index()
            .checked_add(scope.map_or(4, |scope| {
                design::decode::operands::edge_recipe_terminal_delta(&scope.kind())
            }))
            .is_some_and(|expected_offset| operand.next_record_index == expected_offset)
            || terminal_group_member)
            && operand
                .recipe_prefix_offset()
                .checked_add(u64_from_index(operand.recipe_prefix_bytes.len()))
                .zip(recipe.and_then(|recipe| recipe.byte_offset.checked_sub(4)))
                .is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end)
            && recipe_reference_frames_match(
                decode,
                &operand.recipe_references,
                &expected_references,
                historical_candidates_retained,
            )?
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == crate::records::recipes::ConstructionRecipeKind::Edge
                    && recipe.byte_offset > operand.recipe_record_byte_offset()
                    && recipe.byte_offset < operand.next_byte_offset()
            })
            && design::decode::operands::edge_recipe_structure_with_context(
                ctx.decode,
                &operand.recipe_program,
            )? == operand.recipe_structure
            && expected_surface_patch_recipe_structure == operand.surface_patch_recipe_structure
            && (historical_candidates_retained || expected_faces == operand.candidate_faces)
            && ctx.decode.get_hash_map(&expected_edge_operands, operand.id.as_str(), "find F3D expected operand")? == Some(&operand);
        let valid = valid
            && ctx.decode.insert_hash_set(
                &mut edge_operand_slots,
                (
                    native_stream,
                    operand.scope_record_index,
                    operand.scope_reference_ordinal,
                ),
                "index F3D edge operand slots",
            )?
            && ctx.decode.insert_hash_set(
                &mut edge_operand_records,
                (native_stream, operand.record_index()),
                "index F3D edge operand records",
            )?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge operand has an invalid scope or recipe frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&operand.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(edge_operand_records)
}

fn validate_edge_treatment_vertex_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let (mut expected, _expected_storage) =
        reload_native_arena::<records::feature::work_geometry::DesignEdgeTreatmentVertexOperand>(
            decode,
            ctx.ir,
            "design_edge_treatment_vertex_operands",
        )?;
    let mut operand_positions_storage = ctx.decode.reserve_scoped(0, "scan F3D expected vertex operands")?;
    let operand_positions = operand_positions_storage.with_storage(|| {
        ctx.decode.collect_vec(expected.iter().enumerate().map(|(index, _)| index), "scan F3D expected vertex operands")
    })?;
    for position in ctx.decode.admit_iter(&operand_positions, "scan F3D expected vertex operands")? {
        let operand = &mut expected[*position];
        let mut reference_positions_storage = ctx.decode.reserve_scoped(0, "scan F3D recipe references")?;
        let reference_positions = reference_positions_storage.with_storage(|| {
            ctx.decode.collect_vec(operand.recipe.recipe_references.iter().enumerate().map(|(index, _)| index), "scan F3D recipe references")
        })?;
        for position in ctx.decode.admit_iter(&reference_positions, "scan F3D recipe references")? {
            let reference = &mut operand.recipe.recipe_references[*position];
            design::decode::dimension_frames::bind_recipe_reference_candidates_charged(
                ctx.decode,
                reference,
                &native.persistent_subentity_tags,
                Some(&operand.id),
            )?;
        }
    }
    let scope_histories = history::bind_scope_histories(
        decode,
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    )?;
    history::bind_edge_treatment_vertex_history(
        decode,
        &mut expected,
        &native.design_parameter_scopes,
        &native.asm_histories,
        &scope_histories,
    )?;
    let expected = decode.collect_hash_map(
        expected
            .iter()
            .map(|operand| (operand.id.as_str(), operand)),
        "index F3D expected edge treatment vertex operands",
    )?;
    let mut records = HashSet::new();
    for operand in ctx.decode.admit_iter(&native.design_edge_treatment_vertex_operands, "scan F3D design edge treatment vertex operands")? {
        let stream = design_stream(&operand.id);
        let scope = ctx.decode.get_hash_map(&ctx.scopes_by_index, &(stream, operand.scope_record_index), "find F3D validation linked record")?;
        let mut groups = native
            .design_construction_operand_groups
            .iter()
            .filter(|group| {
                design_stream(&group.id) == stream
                    && group.scope_record_index == operand.scope_record_index
                    && group.record_index == operand.group_record_index
            });
        let group = groups.next();
        let expected_id = crate::ids::native_scoped_id_charged(
            decode,
            stream,
            "edge-treatment-vertex-operand",
            operand.recipe.byte_offset(),
        )?;
        let valid = operand.id == expected_id
            && scope.is_some_and(|scope| {
                design::decode::operands::has_edge_recipe_operands(&scope.kind())
                    && usize::try_from(operand.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                        == Some(&operand.recipe.record_index())
            })
            && group.is_some_and(|group| {
                usize::try_from(operand.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                    == Some(&operand.recipe.record_index())
            })
            && groups.next().is_none()
            && ctx.decode.get_hash_map(&expected, operand.id.as_str(), "find F3D expected operand")? == Some(&operand);
        let valid = valid
            && ctx.decode.insert_hash_set(
                &mut records,
                (stream, operand.recipe.record_index()),
                "index F3D edge treatment vertex records",
            )?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion edge-treatment vertex operand has an invalid group or recipe frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&operand.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(records)
}

/// Report Fillet/Chamfer edge groups with incomplete selection operands.
fn validate_edge_treatment_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    edge_operand_records: &HashSet<(&'a str, u32)>,
    edge_identity_records: &HashSet<(&'a str, u32)>,
    edge_treatment_vertex_records: &HashSet<(&'a str, u32)>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    for scope in ctx.decode.admit_iter(&native.design_parameter_scopes, "scan F3D feature parameter scopes")?.filter(|scope| {
        matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::Fillet
                | crate::records::feature::scope::DesignFeatureKind::Chamfer
        )
    }) {
        let native_stream = design_stream(&scope.id);
        let mut groups = native
            .design_construction_operand_groups
            .iter()
            .filter(|group| {
                design_stream(&group.id) == native_stream
                    && group.scope_record_index == scope.record_index
            })
            .peekable();
        let complete = groups.peek().is_some()
            && groups.all(|group| {
                let recipe_backed =
                    group
                        .members()
                        .iter()
                        .map(|member| &member.value)
                        .all(|member| {
                            edge_operand_records.contains(&(native_stream, *member))
                                || edge_treatment_vertex_records.contains(&(native_stream, *member))
                        });
                let identity_backed = group
                    .members()
                    .iter()
                    .map(|member| &member.value)
                    .all(|member| edge_identity_records.contains(&(native_stream, *member)));
                recipe_backed || identity_backed
            });
        if !complete {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge-treatment group has incomplete selection operands",
                Some(
                    ctx.decode
                        .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate face operands and their recipe frames; returns their record set.
fn validate_face_operands<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected_face_operands: &[records::topology::face::DesignFaceOperand],
) -> Result<HashSet<(&'a str, u32, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let scopes_by_index = &ctx.scopes_by_index;
    let historical_candidates_retained = history::projection_was_finalized(ctx.decode, &native.asm_histories)?;
    let face_groups_by_index = (ctx.decode).collect_hash_map(
        native
            .design_construction_operand_groups
            .iter()
            .map(|group| ((design_stream(&group.id), group.record_index), group)),
        "index F3D face operand groups",
    )?;
    let expected_face_operands = (ctx.decode).collect_hash_map(
        expected_face_operands
            .iter()
            .map(|operand| (operand.id.as_str(), operand)),
        "index F3D expected face operands",
    )?;
    let mut face_operand_records = HashSet::new();
    for operand in ctx.decode.admit_iter(&native.design_face_operands, "scan F3D design face operands")? {
        let native_stream = design_stream(&operand.id);
        let scope = ctx.decode.get_hash_map(scopes_by_index, &(native_stream, operand.scope_record_index), "find F3D validation record index")?;
        let header = ctx.decode.get_hash_map(records_by_index, &(native_stream, operand.record_index()), "find F3D validation record index")?;
        let recipe = ctx.decode.get_hash_map(recipes_by_id, operand.recipe_id.as_str(), "find F3D validation record index")?;
        let mut expected_faces = ctx.decode.collect_vec(
            recipe
                .and_then(|recipe| recipe.record_index)
                .map(|record_index| i64::from(record_index.value))
                .filter(|value| *value >= 0)
                .into_iter()
                .flat_map(|design_reference| {
                    native
                        .persistent_subentity_tags
                        .iter()
                        .filter(move |tag| {
                            crate::ids::same_native_occurrence(&tag.id, &operand.id)
                                && tag.design_references.contains(&design_reference)
                        })
                        .filter_map(|tag| match &tag.target {
                            cadmpeg_ir::attributes::AttributeTarget::Face(id) => Some(id),
                            _ => None,
                        })
                }),
            "collect F3D expected operand faces",
        )?;
        ctx.decode.stable_sort_by(
            &mut expected_faces,
            |value| value.as_str(),
            Ord::cmp,
            "f3d face operand expected faces sort",
        )?;
        expected_faces.dedup();
        let mut expected_references =
            design::decode::dimension_frames::decode_recipe_references_charged(
                ctx.decode,
                &operand.recipe_prefix_bytes,
                operand.recipe_prefix_offset(),
            )?;
        if !historical_candidates_retained {
            let mut reference_positions_storage = ctx.decode.reserve_scoped(0, "scan F3D recipe references")?;
            let reference_positions = reference_positions_storage.with_storage(|| {
                ctx.decode.collect_vec(expected_references.iter().enumerate().map(|(index, _)| index), "scan F3D recipe references")
            })?;
            for position in ctx.decode.admit_iter(&reference_positions, "scan F3D recipe references")? {
                let reference = &mut expected_references[*position];
                design::decode::dimension_frames::bind_recipe_reference_candidates_charged(
                    ctx.decode,
                    reference,
                    &native.persistent_subentity_tags,
                    Some(&operand.id),
                )?;
            }
        }
        let recipe_design_reference = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0);
        let mut referenced_faces_storage = ctx.decode.reserve_scoped(0, "index F3D referenced operand faces")?;
        let mut referenced_faces = HashSet::new();
        for reference in ctx.decode.admit_iter(&expected_references, "scan F3D referenced face operands")? {
            if Some(reference.design_reference) != recipe_design_reference { continue; }
            for face in ctx.decode.admit_iter(&reference.candidate_faces, "scan F3D referenced operand faces")? {
                referenced_faces_storage.with_storage(|| ctx.decode.insert_hash_set(
                    &mut referenced_faces, face, "index F3D referenced operand faces"))?;
            }
        }
        let mut expected_unreferenced_faces_storage = ctx.decode.reserve_scoped(0, "collect F3D unreferenced operand faces")?;
        let mut expected_unreferenced_faces = Vec::new();
        for face in ctx.decode.admit_iter(&expected_faces, "scan F3D unreferenced operand faces")? {
            if !ctx.decode.contains_hash_set(&referenced_faces, face, "find F3D referenced operand face")? {
                ctx.decode.push_scoped_vec(&mut expected_unreferenced_faces_storage,
                    &mut expected_unreferenced_faces, *face, "collect F3D unreferenced operand faces")?;
            }
        }
        let mut expected_alternate_selector_faces_storage = ctx.decode.reserve_scoped(0, "collect F3D alternate selector operand faces")?;
        let mut expected_alternate_selector_faces = Vec::new();
        for reference in ctx.decode.admit_iter(&expected_references, "scan F3D alternate selector face operands")? {
            if Some(reference.design_reference) != recipe_design_reference { continue; }
            for face in ctx.decode.admit_iter(&reference.alternate_selector_faces, "scan F3D alternate selector operand faces")? {
                ctx.decode.push_scoped_vec(&mut expected_alternate_selector_faces_storage,
                    &mut expected_alternate_selector_faces, face, "collect F3D alternate selector operand faces")?;
            }
        }
        ctx.decode.stable_sort_by(
            &mut expected_alternate_selector_faces,
            |value| value.as_str(),
            Ord::cmp,
            "f3d face operand alternate selector faces sort",
        )?;
        expected_alternate_selector_faces.dedup();
        let Some(recipe_window) = std::num::NonZeroUsize::new(3) else {
            return Err(CodecError::Malformed("F3D face recipe window is empty".into()));
        };
        let mut expected_node_offsets_storage = ctx.decode.reserve_scoped(0, "collect F3D face recipe node offsets")?;
        let mut expected_node_offsets = Vec::new();
        for (index, values) in ctx.decode.admit_iter(&operand.recipe_program,
            "scan F3D face recipe node windows")?.windows(recipe_window).enumerate() {
            if !ctx.decode.equal(values, &[-1, -1, 2][..], "compare F3D face recipe node marker")? { continue; }
            let offset = u64_from_index(index)
                .checked_mul(4)
                .and_then(|delta| operand.recipe_program_offset.checked_add(delta))
                .ok_or_else(|| CodecError::Malformed("F3D face recipe node offset overflows".into()))?;
            ctx.decode.push_scoped_vec(&mut expected_node_offsets_storage, &mut expected_node_offsets,
                offset, "collect F3D face recipe node offsets")?;
        }
        let expected_nodes = ctx.decode.collect_vec(
            ctx.decode.admit_iter(&expected_node_offsets, "admit source for collect F3D face recipe nodes")?.copied().zip(
                ctx.decode.admit_iter(&expected_node_offsets, "admit source for collect F3D face recipe nodes")?
                    .copied()
                    .skip(1)
                    .chain(std::iter::once(operand.next_byte_offset())),
            ),
            "collect F3D face recipe nodes",
        )?;
        let valid_program =
            match design::decode::operands::face_recipe_program_kind(&operand.recipe_program) {
                Some(design::decode::operands::FaceRecipeProgramKind::Terminal) => {
                    operand.recipe_nodes.is_empty()
                }
                Some(design::decode::operands::FaceRecipeProgramKind::Counted { .. }) => {
                    operand.recipe_nodes.len() == expected_nodes.len()
                        && operand.recipe_nodes.iter().zip(expected_nodes).try_fold(true,
                            |valid, (node, (start, end))| -> Result<bool, CodecError> {
 if !valid { return Ok(false); }
 Ok(
                                node.byte_offset == start
                                    && node.end_byte_offset == end
                                    && node.program.get(0..3) == Some(&[-1, -1, 2])
                                    && node.recipe_structure
                                        == node.program.get(3..).map(
                                            |program| design::decode::operands::face_recipe_structure_with_context(ctx.decode, program),
                                        ).transpose()?.flatten()
                                    && u64_from_index(node.program.len()).checked_mul(4).and_then(|delta| start.checked_add(delta)).is_some_and(|expected_offset| expected_offset == end)
                            )},
                        )?
                        && operand.recipe_nodes.first().is_none_or(|first_node| {
                            first_node
                                .byte_offset
                                .checked_sub(operand.recipe_program_offset)
                                .and_then(|byte_offset| usize::try_from(byte_offset / 4).ok())
                                .is_some_and(|first_node_index| {
                                    operand
                                        .recipe_nodes
                                        .iter()
                                        .flat_map(|node| node.program.iter().copied())
                                        .eq(operand
                                            .recipe_program
                                            .iter()
                                            .copied()
                                            .skip(first_node_index))
                                })
                        })
                }
                None => false,
            };
        let expected_history = ctx.decode.get_hash_map(&expected_face_operands, operand.id.as_str(), "find F3D validation record index")?;
        let valid = (match scope {
            None => false,
            Some(scope) => {
            let family = design::design_feature_family(&scope.kind());
            match (operand.group_record_index(), operand.group_member_ordinal()) {
                (Some(group_record_index), Some(group_member_ordinal)) => {
                    let group = ctx.decode.get_hash_map(&face_groups_by_index,
                        &(native_stream, group_record_index), "find F3D face operand group")?
                        .copied();
                    let exact_group_member = group.is_some_and(|group| {
                        group.scope_record_index == operand.scope_record_index
                            && usize::try_from(operand.scope_reference_ordinal)
                                .ok()
                                .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                                == Some(&group_record_index)
                            && usize::try_from(group_member_ordinal)
                                .ok()
                                .and_then(|ordinal| {
                                    group.members().get(ordinal).map(|member| &member.value)
                                })
                                == Some(&operand.record_index())
                    });
                    exact_group_member
                        && match family {
                            Some(
                                design::DesignFeatureFamily::Extrude
                                | design::DesignFeatureFamily::OffsetFaces
                                | design::DesignFeatureFamily::Shell
                                | design::DesignFeatureFamily::Thicken
                                | design::DesignFeatureFamily::Split,
                            ) => true,
                            Some(design::DesignFeatureFamily::ReplaceFace) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::Loft) => {
                                group.is_some_and(|group| {
                                    matches!(
                                        group.role(),
                                        DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
                                    )
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::Sweep) => {
                                group.is_some_and(|group| group.role() == DesignOperandRole::FACES)
                                    && operand.recipe_kind
                                        == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::SurfaceOffset) => {
                                group
                                    .is_some_and(|group| group.role() == DesignOperandRole::PROFILE)
                                    && operand.recipe_kind
                                        == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::Draft) => {
                                group.is_some_and(|group| match group.role() {
                                    DesignOperandRole::ROLE_0X10 => {
                                        operand.recipe_kind
                                            == records::recipes::ConstructionRecipeKind::BoundedFace
                                    }
                                    DesignOperandRole::ROLE_0X21 => {
                                        operand.recipe_kind
                                            == records::recipes::ConstructionRecipeKind::Face
                                    }
                                    _ => false,
                                })
                            }
                            Some(design::DesignFeatureFamily::Revolve) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X21
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::Face
                            }
                            Some(design::DesignFeatureFamily::CircularPattern) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::BODIES_B
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::Face
                            }
                            Some(design::DesignFeatureFamily::Mirror) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::BODIES_B
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::Face
                            }
                            Some(design::DesignFeatureFamily::Thread) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                        && scope.thread_construction().is_some_and(|construction| {
                                            construction
                                                .face_group_record_indices
                                                .contains(&group.record_index)
                                        })
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(
                                design::DesignFeatureFamily::Fillet
                                | design::DesignFeatureFamily::Chamfer,
                            ) => {
                                operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                                    && native.design_edge_identity_operands.iter().any(|identity| {
                                        design_stream(&identity.id) == native_stream
                                            && identity.scope_record_index
                                                == operand.scope_record_index
                                            && identity.group_record_index == group_record_index
                                            && identity.group_member_ordinal == group_member_ordinal
                                            && identity.record_index() == operand.record_index()
                                            && identity.class_tag == operand.class_tag
                                    })
                            }
                            None if scope.kind()
                                == crate::records::feature::scope::DesignFeatureKind::SplitFace =>
                            {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            None if matches!(
                                scope.kind(),
                                crate::records::feature::scope::DesignFeatureKind::DeleteFace
                                    | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                            ) =>
                            {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            _ => false,
                        }
                }
                (None, None) => {
                    let direct_member = usize::try_from(operand.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                        == Some(&operand.record_index());
                    direct_member
                        && match family {
                            Some(
                                design::DesignFeatureFamily::OffsetFaces
                                | design::DesignFeatureFamily::Shell
                                | design::DesignFeatureFamily::Thicken,
                            ) => true,
                            Some(design::DesignFeatureFamily::Split) => {
                                operand.scope_reference_ordinal == 1
                            }
                            Some(design::DesignFeatureFamily::Hole) => {
                                operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::Assemble)
                                if scope.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::AsBuilt
                                    && design::assembly::legacy_as_built_421_generation(
                                        scope.frame_length(),
                                        scope.class_tag.as_str(),
                                        scope.paired_class_tag.as_str(),
                                    )
                                    .is_some() =>
                            {
                                matches!(
                                    (operand.scope_reference_ordinal, operand.recipe_kind),
                                    (1, records::recipes::ConstructionRecipeKind::BoundedFace)
                                        | (3, records::recipes::ConstructionRecipeKind::Face)
                                )
                            }
                            _ => false,
                        }
                }
                _ => false,
            }
        }}) && header.is_some_and(|header| {
            header.byte_offset == operand.byte_offset() && header.class_tag == operand.class_tag
        }) && operand
            .recipe_prefix_offset().checked_add(u64_from_index(operand.recipe_prefix_bytes.len())).zip(recipe.and_then(|recipe| recipe.byte_offset.checked_sub(4))).is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end)
            && recipe_reference_frames_match(
                ctx.decode,
                &operand.recipe_references,
                &expected_references,
                historical_candidates_retained,
            )?
            && valid_program
            && recipe_program_operand_length(operand.recipe_kind).is_some_and(|operand_length| {
                recipe.is_some_and(|recipe| {
                    recipe.byte_offset.checked_add(operand_length).is_some_and(|expected_offset| operand.recipe_program_offset == expected_offset)
                })
            })
            && u64_from_index(operand.recipe_program.len()).checked_mul(4).and_then(|delta| operand
                    .recipe_program_offset.checked_add(delta)).is_some_and(|expected_offset| operand.next_byte_offset() == expected_offset)
            && (historical_candidates_retained
                || face_ids_match_refs(ctx.decode, &operand.candidate_faces, &expected_faces)?)
            && (historical_candidates_retained
                || face_ids_match_refs(ctx.decode, &operand.unreferenced_candidate_faces, &expected_unreferenced_faces)?)
            && (historical_candidates_retained
                || face_ids_match_refs(ctx.decode, &operand.alternate_selector_candidate_faces, &expected_alternate_selector_faces)?)
            && expected_history.is_some_and(|expected| {
                operand.preceding_candidate_faces == expected.preceding_candidate_faces
                    && operand.changed_candidate_faces == expected.changed_candidate_faces
                    && operand.historical_support_contexts == expected.historical_support_contexts
            })
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == operand.recipe_kind
                    && recipe.byte_offset > operand.recipe_record_byte_offset()
                    && recipe.byte_offset < operand.next_byte_offset()
            });
        let valid = valid
            && ctx.decode.insert_hash_set(
                &mut face_operand_records,
                (
                    native_stream,
                    operand.scope_record_index,
                    operand.record_index(),
                ),
                "index F3D face operand records",
            )?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design face operand has an invalid scope or recipe frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&operand.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(face_operand_records)
}

fn face_ids_match_refs(
    decode: &DecodeContext<'_>,
    actual: &[cadmpeg_ir::ids::FaceId],
    expected: &[&cadmpeg_ir::ids::FaceId],
) -> Result<bool, CodecError> {
    if actual.len() != expected.len() {
        return Ok(false);
    }
    for (actual, expected) in decode.admit_iter(actual, "scan F3D actual face identities")?
        .zip(decode.admit_iter(expected, "scan F3D expected face identities")?)
    {
        if !decode.equal(actual, *expected, "compare F3D face identity")? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Report face-group members with no resolved recipe operand.
fn validate_face_group_member_resolution(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
    face_group_members: HashSet<(&str, u32, u32)>,
    face_operand_records: &HashSet<(&str, u32, u32)>,
    entity_selection_operands: &[records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), CodecError> {
    let entity_selection_records = ctx.decode.collect_hash_set(
        entity_selection_operands.iter().map(|operand| {
            (
                design_stream(&operand.id),
                operand.scope_record_index,
                operand.record_index(),
            )
        }),
        "index F3D face group entity selections",
    )?;
    for member in ctx.decode.admit_iter(&face_group_members, "scan F3D face group members")?.copied() {
        if !ctx.decode.contains_hash_set(face_operand_records, &member, "find F3D face group operand")? && !ctx.decode.contains_hash_set(&entity_selection_records, &member, "find F3D face group entity selection")? {
            let entity = {
                let decode = ctx.decode;
                decode.format_retained(
                    format_args!(
                        "{}:design-face-group-member#{}:{}",
                        member.0, member.1, member.2
                    ),
                    "retain F3D face group member identity",
                )?
            };
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude face group has an unresolved recipe operand",
                Some(entity),
            )?;
        }
    }
    Ok(())
}

/// Validate retained Face source carriers and their persistent identities.
fn validate_face_source_groups(ctx: &Ctx, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut carrier_records = HashSet::new();
    for group in ctx.decode.admit_iter(&native.design_face_source_groups, "scan F3D design face source groups")? {
        let native_stream = design_stream(&group.id);
        let scope = ctx.decode.get_hash_map(&ctx.scopes_by_index, &(native_stream, group.scope_record_index), "find F3D validation linked record")?;
        let carrier_header = ctx.decode.get_hash_map(&ctx.records_by_index, &(native_stream, group.carrier_record_index), "find F3D validation linked record")?;
        let paired_header = ctx.decode.get_hash_map(&ctx.records_by_index, &(native_stream, group.paired_record_index), "find F3D validation linked record")?;
        let carrier_ordinal = usize::try_from(group.carrier_reference_ordinal).ok();
        let source_spec = design::decode::operands::face_source_carrier_spec(
            group.carrier_class_tag.as_str(),
            group.paired_class_tag.as_str(),
        );
        let scope_links_valid = scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Face
                && carrier_ordinal
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.carrier_record_index)
                && carrier_ordinal
                    .and_then(|ordinal| ordinal.checked_add(1))
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.paired_record_index)
        });
        let headers_valid = carrier_header.is_some_and(|header| {
            header.byte_offset == group.carrier_span.start()
                && header.class_tag == group.carrier_class_tag
        }) && paired_header.is_some_and(|header| {
            header.byte_offset == group.carrier_span.end()
                && header.class_tag == group.paired_class_tag
        });
        let source_offsets_valid = source_spec.is_some_and(|layout| {
            let Ok(source_reference_offset) = u64::try_from(layout.source_reference_offset) else {
                return false;
            };
            group
                .source_members
                .iter()
                .enumerate()
                .all(|(ordinal, member)| {
                    let Ok(ordinal) = u64::try_from(ordinal) else {
                        return false;
                    };
                    group
                        .carrier_span
                        .start()
                        .checked_add(source_reference_offset)
                        .and_then(|offset| offset.checked_add(ordinal.checked_mul(11)?))
                        == Some(member.offset)
                })
        });
        let mut source_records = HashSet::new();
        let source_members_valid = if let Some(layout) = source_spec {
            if group.source_members.len() == layout.source_count {
                let mut valid = true;
                for member in ctx.decode.admit_iter(&group.source_members, "scan F3D face source members")?.map(|member| &member.value) {
                    let unique_record = ctx.decode.insert_hash_set(
                        &mut source_records,
                        member.record_index,
                        "index F3D Face source member records",
                    )?;
                    let persistent = &member.persistent_identity;
                    let local_id_offset = member.byte_offset.checked_add(21);
                    let asset_id_offset = member.byte_offset.checked_add(33);
                    let member_valid = unique_record
                        && member.byte_offset > group.carrier_span.start()
                        && local_id_offset == Some(persistent.local_id_offset())
                        && asset_id_offset == Some(persistent.asset_id_offset())
                        && persistent.context_id_offset() > persistent.asset_id_offset()
                        && persistent.tail_slot_offset() > persistent.context_id_offset()
                        && persistent.next_byte_offset() > member.byte_offset;
                    if !member_valid {
                        valid = false;
                        break;
                    }
                }
                valid
            } else {
                false
            }
        } else {
            false
        };
        let valid = ctx.decode.insert_hash_set(
            &mut carrier_records,
            (native_stream, group.carrier_record_index),
            "index F3D Face source carriers",
        )? && scope_links_valid
            && headers_valid
            && source_offsets_valid
            && source_members_valid;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Face source carrier has invalid links or offsets",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate sketch placement frames and their scope links.
fn validate_sketch_placements(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut placement_records = HashSet::new();
    let mut placement_scopes = HashSet::new();
    let mut visibility_offsets = HashSet::new();
    let mut visibility_ordinals = HashSet::new();
    for placement in ctx.decode.admit_iter(&native.design_sketch_placements, "scan F3D design sketch placements")? {
        let native_stream = design_stream(&placement.id);
        let unique_record = ctx.decode.insert_hash_set(
            &mut placement_records,
            (native_stream, placement.record_index),
            "index F3D sketch placement records",
        )?;
        let unique_scope = match placement.scope_record_index {
            Some(index) => ctx.decode.insert_hash_set(
                &mut placement_scopes,
                (native_stream, index),
                "index F3D sketch placement scopes",
            )?,
            None => true,
        };
        let scope = match placement.scope_record_index {
            Some(index) => ctx.decode.get_hash_map(scopes_by_index,
                &(native_stream, index), "find F3D placement scope")?,
            None => None,
        };
        let visibility_valid = if let Some(visibility) = placement.visibility.as_ref() {
            let header_valid = ctx
                .entities_by_suffix
                .get(&(native_stream, placement.entity_id.suffix()))
                .is_some_and(|entity| visibility.stream_ordinal_offset() > entity.byte_offset);
            if header_valid
                && ctx.decode.insert_hash_set(
                    &mut visibility_ordinals,
                    (native_stream, visibility.stream_ordinal.get()),
                    "index F3D sketch visibility ordinals",
                )?
            {
                ctx.decode.insert_hash_set(
                    &mut visibility_offsets,
                    (native_stream, visibility.visible_offset()),
                    "index F3D sketch visibility offsets",
                )?
            } else {
                false
            }
        } else {
            true
        };
        let scope_valid = if placement.member_run_head() {
            scope.is_none_or(|scope| {
                design::design_feature_family(&scope.kind())
                    == Some(design::DesignFeatureFamily::Sketch)
            })
        } else {
            scope.is_some_and(|scope| {
                design::design_feature_family(&scope.kind())
                    == Some(design::DesignFeatureFamily::Sketch)
                    && scope
                        .sketch_entity()
                        .is_some_and(|binding| binding.entity_id == placement.entity_id)
            })
        };
        let valid = scope_valid && unique_record && unique_scope && visibility_valid;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design sketch placement has an invalid frame or scope link",
                Some(
                    ctx.decode
                        .copy_retained_text(&placement.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    let mut visibility_ranges_storage = ctx.decode.reserve_scoped(0, "hold F3D visibility ordinal ranges")?;
    let mut visibility_ordinal_ranges = HashMap::<&str, (usize, u32)>::new();
    for (stream, ordinal) in ctx.decode.admit_iter(&visibility_ordinals, "scan F3D visibility ordinals")?.copied() {
        let (count, maximum) = visibility_ranges_storage.with_storage(|| ctx.decode.entry_hash_map(
            &mut visibility_ordinal_ranges, stream, "index F3D visibility stream ordinals"))?.or_default();
        *count = count.checked_add(1).ok_or_else(|| ctx.decode.refuse_codec_limit("count F3D visibility stream ordinals", u64::MAX - 1, u64::MAX))?;
        *maximum = (*maximum).max(ordinal);
    }
    for (stream, (count, maximum)) in ctx.decode.admit_iter(&visibility_ordinal_ranges, "scan F3D visibility ordinal ranges")?.map(|(stream, range)| (*stream, *range)) {
        if usize::try_from(maximum).ok() != Some(count) {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design sketch Geometry member ordinals are not contiguous",
                Some(
                    ctx.decode
                        .copy_retained_text(stream, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate parameter owner frames and their indexed parameter links.
fn validate_parameter_owners(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let parameters_by_index = &ctx.parameters_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let mut owner_indices = HashSet::new();
    let mut owner_local_ordinals = HashSet::new();
    for owner in ctx.decode.admit_iter(&native.design_parameter_owners, "scan F3D design parameter owners")? {
        let native_stream = design_stream(owner.id());
        let unique_index = ctx.decode.insert_hash_set(
            &mut owner_indices,
            (native_stream, owner.record_index()),
            "index F3D parameter owners",
        )?;
        let parameter = ctx.decode.get_hash_map(parameters_by_index, &(native_stream, owner.parameter_record_index()), "find F3D validation record index")?;
        let legacy_68_frame = owner.frame_length() == 68;
        let frame_layout = !matches!(owner.frame_length(), 68 | 88)
            || parameter.is_some_and(|parameter| {
                owner.evaluated_value_offset() == parameter.evaluated_value_offset()
            });
        let scope_resolves = legacy_68_frame
            || records_by_index.contains_key(&(native_stream, owner.scope_record_index()));
        let unique_local_ordinal = if legacy_68_frame {
            true
        } else {
            ctx.decode.insert_hash_set(
                &mut owner_local_ordinals,
                (
                    native_stream,
                    owner.scope_record_index(),
                    owner.local_ordinal(),
                ),
                "index F3D parameter owner local ordinals",
            )?
        };
        let valid = frame_layout
            && scope_resolves
            && records_by_index.contains_key(&(native_stream, owner.parameter_record_index()))
            && records_by_index.contains_key(&(native_stream, owner.companion_record_index()))
            && companions_by_index
                .get(&(native_stream, owner.companion_record_index()))
                .is_some_and(|companion| companion.owner_record_index() == owner.record_index())
            && parameter.is_some_and(|parameter| {
                parameter.owner_record_index() == Some(owner.record_index())
                    && parameter.evaluated_value().get().to_bits()
                        == owner.evaluated_value().get().to_bits()
            })
            && unique_index
            && unique_local_ordinal;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter owner has an invalid frame or indexed link",
                Some(
                    ctx.decode
                        .copy_retained_text(owner.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate parameter companion prefixes and owned recipe runs.
fn validate_parameter_companions(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let mut companion_indices = HashSet::new();
    let mut companion_owners = HashSet::new();
    for companion in ctx.decode.admit_iter(&native.design_parameter_companions, "scan F3D design parameter companions")? {
        let native_stream = design_stream(companion.id());
        let payload = companion.payload();
        let payload_end =
            payload.and_then(|payload| payload.byte_offset().checked_add(payload.byte_length()));
        let mut expected_recipes = ctx.decode.collect_vec(
            ctx.decode.admit_iter(&native.construction_recipes, "admit source for collect F3D companion expected recipes")?.filter(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && payload.is_some_and(|payload| {
                        payload_end.is_some_and(|end| {
                            recipe.byte_offset >= payload.byte_offset() && recipe.byte_offset < end
                        })
                    })
            }),
            "collect F3D companion expected recipes",
        )?;
        ctx.decode.stable_sort_by(
            &mut expected_recipes,
            |value| &value.byte_offset,
            Ord::cmp,
            "f3d parameter companion recipes sort",
        )?;
        let unique_index = ctx.decode.insert_hash_set(
            &mut companion_indices,
            (native_stream, companion.record_index()),
            "index F3D parameter companions",
        )?;
        let unique_owner = ctx.decode.insert_hash_set(
            &mut companion_owners,
            (native_stream, companion.owner_record_index()),
            "index F3D companion owners",
        )?;
        let owner = ctx.decode.get_hash_map(owners_by_index, &(native_stream, companion.owner_record_index()), "find F3D validation record index")?;
        let valid = companion
            .byte_offset()
            .checked_add(42)
            .is_some_and(|expected_offset| companion.timestamp_micros_offset() == expected_offset)
            && payload.is_none_or(|payload| {
                companion
                    .byte_offset()
                    .checked_add(58)
                    .is_some_and(|expected_offset| payload.byte_offset() == expected_offset)
            })
            && (payload.is_none() || payload_end.is_some())
            && payload
                .map_or(
                    &[][..],
                    crate::records::parameters::DesignCompanionPayload::owned_recipe_ids,
                )
                .iter()
                .map(String::as_str)
                .eq(expected_recipes.iter().map(|recipe| recipe.id.as_str()))
            && ctx.decode.contains_key_hash_map(records_by_index, &(native_stream, companion.record_index()), "find F3D parameter companion record")?
            && owner
                .is_some_and(|owner| owner.companion_record_index() == companion.record_index())
            && unique_index
            && unique_owner;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter companion has an invalid prefix or owner link",
                Some(
                    ctx.decode
                        .copy_retained_text(companion.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate dimension recipe records; returns the owned recipe ids.
fn validate_dimension_recipe_records<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, &'a str)>, CodecError> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let mut dimension_recipe_ids = HashSet::new();
    for record in ctx.decode.admit_iter(&native.design_dimension_recipe_records, "scan F3D design dimension recipe records")? {
        let native_stream = design_stream(&record.id);
        let companion = ctx.decode.get_hash_map(companions_by_index, &(native_stream, record.companion_record_index), "find F3D validation record index")?;
        let dimension_companion = match companion {
            Some(companion) => match ctx.decode.get_hash_map(
                owners_by_index,
                &(native_stream, companion.owner_record_index()),
                "find F3D dimension recipe owner",
            )? {
                Some(owner) => ctx.decode.get_hash_map(
                    parameters_by_index,
                    &(native_stream, owner.parameter_record_index()),
                    "find F3D dimension recipe parameter",
                )?.is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                }),
                None => false,
            },
            None => false,
        };
        let recipe = ctx.decode.find_by(
            &native.construction_recipes,
            |recipe| ctx.decode.equal(&recipe.id, &record.recipe_id, "compare F3D dimension recipe identity"),
            "find F3D dimension construction recipe",
        )?;
        let companion_order_matches = match companion {
            Some(companion) => {
                let expected = usize::try_from(record.recipe_ordinal)
                    .ok()
                    .and_then(|ordinal| companion.payload().and_then(|payload| payload.owned_recipe_ids().get(ordinal)));
                ctx.decode.equal(&expected, &Some(&record.recipe_id), "compare F3D dimension companion recipe order")?
            }
            None => false,
        };
        let frame_end = record.byte_offset.checked_add(record.frame_length);
        let prefix_end = record
            .prefix_offset
            .checked_add(u64_from_index(record.prefix_bytes.len()));
        let program_end = u64_from_index(record.program.len())
            .checked_mul(4)
            .and_then(|length| record.program_offset.checked_add(length));
        let mut decoded_references =
            design::decode::dimension_frames::decode_recipe_references_charged(
                ctx.decode,
                &record.prefix_bytes,
                record.prefix_offset,
            )?;
        let mut reference_positions_storage = ctx.decode.reserve_scoped(0, "scan F3D recipe references")?;
        let reference_positions = reference_positions_storage.with_storage(|| {
            ctx.decode.collect_vec(decoded_references.iter().enumerate().map(|(index, _)| index), "scan F3D recipe references")
        })?;
        for position in ctx.decode.admit_iter(&reference_positions, "scan F3D recipe references")? {
            let reference = &mut decoded_references[*position];
            design::decode::dimension_frames::bind_recipe_reference_candidates_charged(
                ctx.decode,
                reference,
                &native.persistent_subentity_tags,
                Some(&record.id),
            )?;
        }
        let references_match = ctx.decode.equal(&decoded_references, &record.references, "compare F3D dimension recipe references")?;
        let matching_edge_operands =
            design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
                ctx.decode,
                record,
                &native.design_edge_operands,
            )?;
        let edge_operands_match = ctx.decode.equal(
            &matching_edge_operands,
            &record.matching_edge_operand_ids,
            "compare F3D dimension matching edge operands",
        )?;
        let recipe_frame_matches = match recipe {
            Some(recipe) => ctx.decode.equal(&design_stream(&recipe.id), &native_stream, "compare F3D dimension recipe stream")?
                && record
                    .byte_offset
                    .checked_add(11)
                    .is_some_and(|expected_offset| recipe.byte_offset >= expected_offset)
                && frame_end.is_some_and(|end| recipe.byte_offset < end)
                && prefix_end == recipe.byte_offset.checked_sub(4)
                && recipe
                    .byte_offset
                    .checked_add(u64_from_index(design::construction_recipe_family_name_len(
                        recipe.kind,
                    )))
                    .is_some_and(|expected_offset| record.program_offset == expected_offset),
            None => false,
        };
        let valid = record.frame_length >= 11
            && !record.prefix_bytes.is_empty()
            && references_match
            && edge_operands_match
            && record
                .byte_offset
                .checked_add(11)
                .is_some_and(|expected_offset| record.prefix_offset == expected_offset)
            && !record.program.is_empty()
            && record
                .byte_offset
                .checked_add(11)
                .is_some_and(|expected_offset| record.program_offset >= expected_offset)
            && program_end == frame_end
            && dimension_companion
            && companion_order_matches
            && recipe_frame_matches
            && ctx.decode.insert_hash_set(
                &mut dimension_recipe_ids,
                (native_stream, record.recipe_id.as_str()),
                "index F3D dimension recipe IDs",
            )?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension recipe has an invalid indexed-record owner",
                Some(
                    ctx.decode
                        .copy_retained_text(&record.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(dimension_recipe_ids)
}

/// Report dimension companions owning an unresolved construction recipe.
fn validate_dimension_companion_recipes<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    dimension_recipe_ids: &HashSet<(&'a str, &'a str)>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    for companion in ctx.decode.admit_iter(&native.design_parameter_companions, "scan F3D design parameter companions")? {
        let native_stream = design_stream(companion.id());
        let dimension_companion = match ctx.decode.get_hash_map(
            owners_by_index,
            &(native_stream, companion.owner_record_index()),
            "find F3D dimension companion owner",
        )? {
            Some(owner) => ctx.decode.get_hash_map(
                parameters_by_index,
                &(native_stream, owner.parameter_record_index()),
                "find F3D dimension companion parameter",
            )?.is_some_and(|parameter| parameter.kind() == records::parameters::DesignParameterKind::Dimension),
            None => false,
        };
        if dimension_companion
            && match companion.payload() {
                Some(payload) => ctx.decode.any_by(
                    payload.owned_recipe_ids(),
                    |recipe_id| Ok(!ctx.decode.contains_hash_set(
                        dimension_recipe_ids,
                        &(native_stream, recipe_id.as_str()),
                        "find F3D owned dimension recipe identity",
                    )?),
                    "scan F3D dimension companion recipe identities",
                )?,
                None => false,
            }
        {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension companion has an unowned construction recipe",
                Some(
                    ctx.decode
                        .copy_retained_text(companion.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate dimension locus pairs; returns their companion set.
fn validate_dimension_locus_pairs<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut locus_pair_indices = HashSet::new();
    let mut locus_pair_companions = HashSet::new();
    for pair in ctx.decode.admit_iter(&native.design_dimension_locus_pairs[..], "scan F3D design dimension locus pairs")? {
        let native_stream = design_stream(&pair.id);
        let unique_index = ctx.decode.insert_hash_set(
            &mut locus_pair_indices,
            (native_stream, pair.record_index),
            "index F3D dimension locus pairs",
        )?;
        let unique_companion = ctx.decode.insert_hash_set(
            &mut locus_pair_companions,
            (native_stream, pair.companion_record_index),
            "index F3D dimension locus pair companions",
        )?;
        let companion = ctx.decode.get_hash_map(companions_by_index, &(native_stream, pair.companion_record_index), "find F3D validation record index")?;
        let companion_contains_frame = match companion {
            Some(companion) => companion.byte_offset().checked_add(58)
                .is_some_and(|expected_offset| pair.byte_offset() >= expected_offset)
                && !ctx.decode.any_by(&native.design_parameter_owners, |owner| {
                    Ok(ctx.decode.equal(&design_stream(owner.id()), &native_stream, "compare F3D dimension locus owner stream")?
                        && owner.byte_offset() > companion.byte_offset()
                        && owner.byte_offset() <= pair.byte_offset())
                }, "scan F3D dimension locus owner intervals")?,
            None => false,
        };
        let dimension_companion = match companion {
            Some(companion) => match ctx.decode.get_hash_map(owners_by_index, &(native_stream, companion.owner_record_index()), "find F3D locus companion owner")? {
                Some(owner) => ctx.decode.get_hash_map(parameters_by_index, &(native_stream, owner.parameter_record_index()), "find F3D locus companion parameter")?
                    .is_some_and(|parameter| parameter.kind() == records::parameters::DesignParameterKind::Dimension),
                None => false,
            },
            None => false,
        };
        let governs_following_dimension =
            design::decode::dimension_frames::following_dimension_companion_record_index(
                &pair.id,
                pair.paired_byte_offset(),
                &native.design_parameter_owners,
                &native.design_parameters,
            ) == Some(pair.governing_companion_record_index);
        let valid = companion_contains_frame
            && dimension_companion
            && governs_following_dimension
            && ctx.decode.contains_hash_set(sketch_geometry_indices, &(native_stream, pair.loci()[0].geometry_index()), "find F3D dimension locus geometry")?
            && ctx.decode.contains_hash_set(sketch_geometry_indices, &(native_stream, pair.loci()[1].geometry_index()), "find F3D dimension locus geometry")?
            && unique_index
            && unique_companion;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension locus pair has an invalid frame or geometry link",
                Some(
                    ctx.decode
                        .copy_retained_text(&pair.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(locus_pair_companions)
}

/// Validate dimension annotation frames and their operand runs.
fn validate_dimension_annotation_frames(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut annotation_frame_indices = HashSet::new();
    for frame in ctx.decode.admit_iter(&native.design_dimension_annotation_frames, "scan F3D design dimension annotation frames")? {
        let native_stream = design_stream(&frame.id);
        let unique_index = ctx.decode.insert_hash_set(
            &mut annotation_frame_indices,
            (native_stream, frame.record_index),
            "index F3D dimension annotation frames",
        )?;
        let governing_owner = ctx.decode.get_hash_map(owners_by_index, &(native_stream, frame.governing_owner_record_index), "find F3D dimension annotation owner")?.copied();
        let physical_interval_valid = match frame.companion_record_index {
            Some(record_index) => ctx.decode.get_hash_map(companions_by_index, &(native_stream, record_index), "find F3D annotation companion interval")?
                .is_some_and(|companion| {
                    companion.byte_offset().checked_add(58).is_some_and(|expected_offset| frame.byte_offset() >= expected_offset)
                        && companion.payload().is_some_and(|payload| {
                            companion.byte_offset().checked_add(58).and_then(|offset| offset.checked_add(payload.byte_length()))
                                .is_some_and(|expected_offset| frame.paired_byte_offset() < expected_offset)
                        })
                }),
            None => match governing_owner {
                Some(owner) => {
                    let scope_contains_frame = ctx.decode.get_hash_map(scopes_by_index, &(native_stream, owner.scope_record_index()), "find F3D annotation interval scope")?
                        .is_some_and(|scope| frame.byte_offset() >= scope.byte_offset());
                    if scope_contains_frame {
                        let end = ctx.decode.fold(&native.design_parameter_owners, None::<u64>, |end, candidate| {
                            if !ctx.decode.equal(&design_stream(candidate.id()), &native_stream, "compare F3D annotation interval stream")?
                                || candidate.scope_record_index() != owner.scope_record_index() {
                                return Ok(end);
                            }
                            let Some(companion) = ctx.decode.get_hash_map(companions_by_index, &(native_stream, candidate.companion_record_index()), "find F3D annotation interval companion")? else {
                                return Ok(end);
                            };
                            let offset = companion.byte_offset();
                            Ok(Some(match end { Some(end) => end.min(offset), None => offset }))
                        }, "find F3D annotation interval end")?;
                        end.is_some_and(|end| frame.paired_byte_offset() < end)
                    } else { false }
                }
                None => false,
            },
        };
        let governing_link_valid = match governing_owner {
            Some(owner) => owner.companion_record_index() == frame.governing_companion_record_index
                && ctx.decode.get_hash_map(parameters_by_index, &(native_stream, owner.parameter_record_index()), "find F3D annotation governing parameter")?
                    .is_some_and(|parameter| parameter.kind() == records::parameters::DesignParameterKind::Dimension),
            None => false,
        };
        let operands_valid = ctx.decode.all_by(frame.operands(), |operand| {
            match operand.geometry_record_index {
                Some(index) => ctx.decode.contains_hash_set(sketch_geometry_indices, &(native_stream, index.get()), "find F3D annotation operand geometry"),
                None => Ok(true),
            }
        }, "scan F3D annotation operands")?;
        let owner_is_sketch = ctx.decode.get_hash_map(entities_by_suffix, &(native_stream, u64::from(frame.owner_reference)), "find F3D annotation sketch owner")?
            .is_some_and(|entity| entity.in_sketch_module());
        let valid = unique_index
            && physical_interval_valid
            && governing_link_valid
            && operands_valid
            && owner_is_sketch;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension annotation frame has invalid links or offsets",
                Some(
                    ctx.decode
                        .copy_retained_text(&frame.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate direct dimension presentation frames and their sketch-owner joins.
fn validate_dimension_presentation_frames(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let sketch_scope_by_entity = (ctx.decode).collect_hash_map(
        ctx.decode.admit_iter(&native.design_sketch_placements, "admit source for index F3D dimension presentation sketch scopes")?
            .filter_map(|placement| {
                Some((
                    (design_stream(&placement.id), placement.entity_id.suffix()),
                    placement.scope_record_index?,
                ))
            }),
        "index F3D dimension presentation sketch scopes",
    )?;
    let mut presentation_frame_indices = HashSet::new();
    for frame in ctx.decode.admit_iter(&native.design_dimension_presentation_frames, "scan F3D design dimension presentation frames")? {
        let native_stream = design_stream(&frame.id);
        let unique_index = ctx.decode.insert_hash_set(
            &mut presentation_frame_indices,
            (native_stream, frame.record_index),
            "index F3D dimension presentation frames",
        )?;
        let owner = ctx.decode.get_hash_map(owners_by_index, &(native_stream, frame.governing_owner_record_index), "find F3D validation record index")?;
        let parameter = ctx.decode.get_hash_map(parameters_by_index, &(native_stream, frame.governing_parameter_record_index), "find F3D presentation governing parameter")?;
        let companion = ctx.decode.get_hash_map(companions_by_index, &(native_stream, frame.governing_companion_record_index), "find F3D presentation governing companion")?;
        let owner_link_valid = owner.is_some_and(|owner| {
            owner.parameter_record_index() == frame.governing_parameter_record_index
                && owner.companion_record_index() == frame.governing_companion_record_index
                && parameter.is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
                && companion
                    .is_some_and(|companion| companion.owner_record_index() == owner.record_index())
        });
        let nearest_owner = ctx.decode.fold(&native.design_parameter_owners, None::<&records::parameters::DesignParameterOwner>, |nearest, candidate| {
            if !ctx.decode.equal(&design_stream(candidate.id()), &native_stream, "compare F3D presentation owner stream")? {
                return Ok(nearest);
            }
            if !ctx.decode.get_hash_map(&sketch_scope_by_entity, &(native_stream, u64::from(frame.owner_reference)), "find F3D presentation sketch scope")?
                .is_some_and(|scope_record_index| candidate.scope_record_index() == *scope_record_index)
                || candidate.byte_offset() <= frame.paired_byte_offset {
                return Ok(nearest);
            }
            if !ctx.decode.get_hash_map(parameters_by_index, &(native_stream, candidate.parameter_record_index()), "find F3D presentation owner parameter")?
                .is_some_and(|parameter| parameter.kind() == records::parameters::DesignParameterKind::Dimension) {
                return Ok(nearest);
            }
            Ok(Some(match nearest {
                Some(previous) if previous.byte_offset() <= candidate.byte_offset() => previous,
                _ => candidate,
            }))
        }, "find F3D nearest presentation owner")?;
        let governing_owner_is_nearest = nearest_owner.is_some_and(|candidate| {
            candidate.record_index() == frame.governing_owner_record_index
        });
        let operand_start = frame.byte_offset.checked_add(24);
        let operands_valid = if frame.operands.is_empty() {
            false
        } else {
            let mut valid = true;
            for (ordinal, operand) in ctx.decode.admit_iter(&frame.operands, "scan F3D presentation operands")?.enumerate() {
                let Some(start) = u64_from_index(ordinal).checked_mul(15)
                    .and_then(|delta| operand_start.and_then(|offset| offset.checked_add(delta))) else {
                    valid = false;
                    break;
                };
                if !(start.checked_add(1).is_some_and(|expected_offset| operand.geometry_reference_offset == expected_offset)
                    && start.checked_add(11).is_some_and(|expected_offset| operand.role_offset == expected_offset)
                    && ctx.decode.contains_hash_set(sketch_geometry_indices, &(native_stream, operand.geometry_record_index.get()), "find F3D presentation operand geometry")?) {
                    valid = false;
                    break;
                }
            }
            valid
        };
        let owner_is_sketch = ctx.decode.get_hash_map(entities_by_suffix, &(native_stream, u64::from(frame.owner_reference)), "find F3D presentation sketch owner")?
            .is_some_and(|entity| entity.in_sketch_module());
        let valid = unique_index
            && frame.paired_byte_offset > frame.byte_offset
            && frame
                .paired_byte_offset
                .checked_sub(frame.byte_offset)
                .is_some_and(|expected_offset| frame.frame_length == expected_offset)
            && (u64_from_index(frame.operands.len()))
                .checked_mul(15)
                .and_then(|delta| operand_start.and_then(|offset| offset.checked_add(delta)))
                .is_some_and(|expected_offset| frame.presentation_byte_offset == expected_offset)
            && frame
                .presentation_byte_offset
                .checked_add(u64_from_index(frame.presentation_bytes.len()))
                .is_some_and(|expected_offset| expected_offset == frame.paired_byte_offset)
            && frame
                .paired_byte_offset
                .checked_add(20)
                .is_some_and(|expected_offset| frame.owner_reference_offset == expected_offset)
            && owner_link_valid
            && governing_owner_is_nearest
            && operands_valid
            && owner_is_sketch;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension presentation frame has invalid links or offsets",
                Some(
                    ctx.decode
                        .copy_retained_text(&frame.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate dimension locus groups; returns their companion set.
fn validate_dimension_locus_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let decode: &DecodeContext<'_> = ctx.decode;
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut locus_group_indices = HashSet::new();
    let mut locus_group_companions = HashSet::new();
    for group in ctx.decode.admit_iter(&native.design_dimension_locus_groups, "scan F3D design dimension locus groups")? {
        let native_stream = design_stream(&group.id);
        let unique_index = ctx.decode.insert_hash_set(
            &mut locus_group_indices,
            (native_stream, group.record_index),
            "index F3D dimension locus groups",
        )?;
        ctx.decode.insert_hash_set(
            &mut locus_group_companions,
            (native_stream, group.companion_record_index),
            "index F3D dimension locus group companions",
        )?;
        let companion = ctx.decode.get_hash_map(companions_by_index, &(native_stream, group.companion_record_index), "find F3D validation record index")?;
        let companion_contains_frame = match companion {
            Some(companion) => companion.byte_offset().checked_add(58)
                .is_some_and(|expected_offset| group.byte_offset >= expected_offset)
                && !ctx.decode.any_by(&native.design_parameter_owners, |owner| {
                    Ok(ctx.decode.equal(&design_stream(owner.id()), &native_stream, "compare F3D dimension locus owner stream")?
                        && owner.byte_offset() > companion.byte_offset()
                        && owner.byte_offset() <= group.byte_offset)
                }, "scan F3D dimension locus owner intervals")?,
            None => false,
        };
        let dimension_companion = match companion {
            Some(companion) => match ctx.decode.get_hash_map(owners_by_index, &(native_stream, companion.owner_record_index()), "find F3D locus companion owner")? {
                Some(owner) => ctx.decode.get_hash_map(parameters_by_index, &(native_stream, owner.parameter_record_index()), "find F3D locus companion parameter")?
                    .is_some_and(|parameter| parameter.kind() == records::parameters::DesignParameterKind::Dimension),
                None => false,
            },
            None => false,
        };
        let count = group.loci.len();
        let loci_start = group.byte_offset.checked_add(24);
        let loci_offsets_valid = {
            let mut valid = true;
            for (ordinal, locus) in ctx.decode.admit_iter(&group.loci, "scan F3D dimension locus offsets")?.enumerate() {
                let Some(start) = u64_from_index(ordinal).checked_mul(15)
                    .and_then(|delta| loci_start.and_then(|offset| offset.checked_add(delta))) else {
                    valid = false;
                    break;
                };
                if !(start.checked_add(1).is_some_and(|expected_offset| locus.geometry_reference_offset == expected_offset)
                    && start.checked_add(11).is_some_and(|expected_offset| locus.role_offset == expected_offset)
                    && ctx.decode.contains_hash_set(sketch_geometry_indices, &(native_stream, locus.geometry_record_index), "find F3D counted dimension locus geometry")?) {
                    valid = false;
                    break;
                }
            }
            valid
        };
        let owner_start = u64_from_index(count)
            .checked_mul(15)
            .and_then(|delta| loci_start.and_then(|offset| offset.checked_add(delta)));
        let returns_start = owner_start.and_then(|offset| offset.checked_add(24));
        let returns_valid = {
            let mut valid = true;
            for (ordinal, locus) in ctx.decode.admit_iter(&group.loci, "scan F3D dimension locus returns")?.enumerate() {
                if !(u64_from_index(ordinal).checked_mul(11)
                    .and_then(|delta| returns_start.and_then(|offset| offset.checked_add(delta)))
                    .and_then(|offset| offset.checked_add(1))
                    .is_some_and(|expected_offset| locus.returned.offset == expected_offset)
                    && ctx.decode.contains_hash_set(sketch_geometry_indices, &(native_stream, locus.returned.value), "find F3D returned dimension locus geometry")?) {
                    valid = false;
                    break;
                }
            }
            valid
        };
        let mut locus_members = ctx.decode.collect_vec(
            group.loci.iter().map(|locus| locus.geometry_record_index),
            "collect F3D dimension locus members",
        )?;
        let mut return_members = ctx.decode.collect_vec(
            group.loci.iter().map(|locus| locus.returned.value),
            "collect F3D dimension return members",
        )?;
        decode.sort_unstable_by(
            &mut locus_members,
            |value| value,
            Ord::cmp,
            "f3d dimension locus members sort",
        )?;
        decode.sort_unstable_by(
            &mut return_members,
            |value| value,
            Ord::cmp,
            "f3d dimension return members sort",
        )?;
        let owner_is_sketch = ctx.decode.get_hash_map(entities_by_suffix, &(native_stream, u64::from(group.owner_reference)), "find F3D counted locus sketch owner")?
            .is_some_and(|entity| entity.in_sketch_module());
        let frame_does_not_overlap = ctx.decode.all_by(&native.design_dimension_locus_groups, |other| {
            Ok(!ctx.decode.equal(&design_stream(&other.id), &native_stream, "compare F3D counted locus frame stream")?
                || other.companion_record_index != group.companion_record_index
                || other.record_index == group.record_index
                || group.next_byte_offset <= other.byte_offset
                || other.next_byte_offset <= group.byte_offset)
        }, "scan F3D counted locus frame overlap")?;
        let valid = companion_contains_frame
            && dimension_companion
            && (1..=64).contains(&count)
            && loci_offsets_valid
            && owner_start
                .and_then(|offset| offset.checked_add(2))
                .is_some_and(|expected_offset| group.owner_reference_offset == expected_offset)
            && owner_start
                .and_then(|offset| offset.checked_add(12))
                .is_some_and(|expected_offset| group.owner_role_offset == expected_offset)
            && owner_start
                .and_then(|offset| offset.checked_add(16))
                .is_some_and(|expected_offset| group.state_offset == expected_offset)
            && owner_is_sketch
            && returns_valid
            && ctx.decode.equal(&locus_members, &return_members, "compare F3D dimension locus return members")?
            && (u64_from_index(count))
                .checked_mul(11)
                .and_then(|delta| returns_start.and_then(|offset| offset.checked_add(delta)))
                .and_then(|offset| offset.checked_add(1))
                .is_some_and(|expected_offset| group.next_byte_offset == expected_offset)
            && group
                .next_byte_offset
                .checked_sub(group.byte_offset)
                .is_some_and(|expected_offset| group.frame_length == expected_offset)
            && unique_index
            && frame_does_not_overlap;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension locus group has an invalid counted frame or geometry link",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(locus_group_companions)
}

/// Validate null-locus dimension pairs against typed companions.
fn validate_dimension_null_locus_pairs<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    locus_pair_companions: &HashSet<(&'a str, u32)>,
    locus_group_companions: &HashSet<(&'a str, u32)>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut null_locus_pair_indices = HashSet::new();
    let mut null_locus_pair_companions = HashSet::new();
    for pair in ctx.decode.admit_iter(&native.design_dimension_null_locus_pairs[..], "scan F3D design dimension null locus pairs")? {
        let native_stream = design_stream(&pair.id);
        let unique_index = ctx.decode.insert_hash_set(
            &mut null_locus_pair_indices,
            (native_stream, pair.record_index),
            "index F3D null-locus dimension pairs",
        )?;
        let unique_companion = ctx.decode.insert_hash_set(
            &mut null_locus_pair_companions,
            (native_stream, pair.companion_record_index),
            "index F3D null-locus dimension companions",
        )?;
        let companion = ctx.decode.get_hash_map(companions_by_index, &(native_stream, pair.companion_record_index), "find F3D validation record index")?;
        let companion_contains_frame = match companion {
            Some(companion) => companion.byte_offset().checked_add(58)
                .is_some_and(|expected_offset| pair.byte_offset() >= expected_offset)
                && !ctx.decode.any_by(&native.design_parameter_owners, |owner| {
                    Ok(ctx.decode.equal(&design_stream(owner.id()), &native_stream, "compare F3D dimension locus owner stream")?
                        && owner.byte_offset() > companion.byte_offset()
                        && owner.byte_offset() <= pair.byte_offset())
                }, "scan F3D dimension locus owner intervals")?,
            None => false,
        };
        let dimension_companion = match companion {
            Some(companion) => match ctx.decode.get_hash_map(owners_by_index, &(native_stream, companion.owner_record_index()), "find F3D locus companion owner")? {
                Some(owner) => ctx.decode.get_hash_map(parameters_by_index, &(native_stream, owner.parameter_record_index()), "find F3D locus companion parameter")?
                    .is_some_and(|parameter| parameter.kind() == records::parameters::DesignParameterKind::Dimension),
                None => false,
            },
            None => false,
        };
        let governs_following_dimension =
            design::decode::dimension_frames::following_dimension_companion_record_index(
                &pair.id,
                pair.paired_byte_offset(),
                &native.design_parameter_owners,
                &native.design_parameters,
            ) == Some(pair.governing_companion_record_index);
        let companion_has_typed_frame = ctx.decode.contains_hash_set(locus_pair_companions, &(native_stream, pair.companion_record_index), "find F3D typed dimension locus pair")?
            || ctx.decode.contains_hash_set(locus_group_companions, &(native_stream, pair.companion_record_index), "find F3D typed dimension locus group")?;
        let valid = companion_contains_frame
            && dimension_companion
            && governs_following_dimension
            && !companion_has_typed_frame
            && ctx.decode.contains_hash_set(sketch_geometry_indices, &(native_stream, pair.loci()[1].geometry_index()), "find F3D null dimension locus geometry")?
            && unique_index
            && unique_companion;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design null-locus dimension pair has an invalid frame or geometry link",
                Some(
                    ctx.decode
                        .copy_retained_text(&pair.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate parameter record identity uniqueness.
fn validate_parameters(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut parameter_indices_storage = ctx.decode.reserve_scoped(0, "index F3D validation parameters")?;
    let mut parameter_indices = HashSet::new();
    for parameter in ctx.decode.admit_iter(&native.design_parameters, "scan F3D design parameters")? {
        let native_stream = design_stream(&parameter.id);
        let key = (native_stream, parameter.record_index);
        if !parameter_indices_storage.with_storage(|| ctx.decode.insert_hash_set(&mut parameter_indices, key, "index F3D validation parameters"))? {
            let id = {
                let decode = ctx.decode;
                decode.format_retained(
                    format_args!("{}", parameter.id),
                    "retain F3D parameter finding entity",
                )?
            };
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter has an invalid frame, family discriminator, or owner",
                Some(id),
            )?;
        }
    }
    Ok(())
}

/// Validate design entity reference runs and suffix uniqueness.
fn validate_entity_headers(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let mut entity_suffixes_storage = ctx.decode.reserve_scoped(0, "index F3D design entity suffixes")?;
    let mut entity_suffixes = HashSet::new();
    for header in ctx.decode.admit_iter(&native.design_entity_headers, "scan F3D design entity headers")? {
        let native_stream = design_stream(&header.id);
        let references_resolve = header
            .reference_values()
            .all(|index| records_by_index.contains_key(&(native_stream, *index)));
        if !references_resolve {
            ctx.push_constant_finding(
                findings,
                Check::ReferentialIntegrity,
                "Fusion design entity has an invalid reference run",
                Some(ctx.decode.copy_retained_text(
                    header.entity_id.as_str(),
                    "retain F3D validation entity",
                )?),
            )?;
        }
        let key = (native_stream, header.entity_id.suffix());
        if !entity_suffixes_storage.with_storage(|| ctx.decode.insert_hash_set(&mut entity_suffixes, key, "index F3D design entity suffixes"))? {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design entity suffix is duplicated within its stream",
                Some(ctx.decode.copy_retained_text(
                    header.entity_id.as_str(),
                    "retain F3D validation entity",
                )?),
            )?;
        }
    }
    Ok(())
}

/// Validate sketch relation owners and byte frames.
fn validate_sketch_relations(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let sketch_owner_ids = &ctx.sketch_owner_ids;
    for relation in ctx.decode.admit_iter(&native.sketch_relations, "scan F3D sketch relations")? {
        let native_stream = design_stream(&relation.id);
        let owner_matches = matches!(
            (
                ctx.decode.get_hash_map(sketch_owner_ids, &(native_stream, relation.owner_reference), "find F3D sketch relation owner")?,
                relation.owner_entity_id.as_ref().map(cadmpeg_core::text::NonBlankString::as_str),
            ),
            (Some(expected), Some(actual)) if *expected == actual
        );
        if !owner_matches {
            ctx.push_constant_finding(
                findings,
                Check::ReferentialIntegrity,
                "Fusion sketch relation has an invalid owner or byte frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&relation.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate sketch point, curve, and surface persistent identities.
fn validate_sketch_geometry_identities(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut sketch_point_identities = HashSet::new();
    let mut sketch_geometry_records = HashSet::new();
    // An unresolved owner is not one shared sketch. Enforce uniqueness only
    // when the owning sketch reference is known.
    for point in ctx.decode.admit_iter(&native.sketch_points, "scan F3D sketch points")? {
        let duplicate = if let (Some(persistent_id), Some(owner_reference)) =
            (point.persistent_id(), point.owner_reference)
        {
            !ctx.decode.insert_hash_set(
                &mut sketch_point_identities,
                (design_stream(&point.id), owner_reference, persistent_id),
                "index F3D sketch point identities",
            )?
        } else {
            false
        };
        if duplicate {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch point has an invalid persistent identity",
                Some(
                    ctx.decode
                        .copy_retained_text(&point.id, "retain F3D validation entity")?,
                ),
            )?;
        }
        if !ctx.decode.insert_hash_set(
            &mut sketch_geometry_records,
            (design_stream(&point.id), point.record_index),
            "index F3D sketch geometry records",
        )? {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(
                    ctx.decode
                        .copy_retained_text(&point.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    let mut sketch_curve_identities = HashSet::new();
    for curve in ctx.decode.admit_iter(&native.sketch_curve_identities, "scan F3D sketch curve identities")? {
        let duplicate = if let Some(owner_reference) = curve.owner_reference {
            !ctx.decode.insert_hash_set(
                &mut sketch_curve_identities,
                (
                    design_stream(&curve.id),
                    owner_reference,
                    curve.primary_id.get(),
                    curve.secondary_id,
                ),
                "index F3D sketch curve identities",
            )?
        } else {
            false
        };
        if duplicate {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch curve has an invalid persistent identity",
                Some(
                    ctx.decode
                        .copy_retained_text(&curve.id, "retain F3D validation entity")?,
                ),
            )?;
        }
        if !ctx.decode.insert_hash_set(
            &mut sketch_geometry_records,
            (design_stream(&curve.id), curve.record_index),
            "index F3D sketch geometry records",
        )? {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(
                    ctx.decode
                        .copy_retained_text(&curve.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    let mut sketch_surface_identities = HashSet::new();
    for surface in ctx.decode.admit_iter(&native.sketch_surfaces, "scan F3D sketch surfaces")? {
        let duplicate = if let Some(owner_reference) = surface.owner_reference {
            !ctx.decode.insert_hash_set(
                &mut sketch_surface_identities,
                (
                    design_stream(&surface.id),
                    owner_reference,
                    surface.persistent_id.get(),
                ),
                "index F3D sketch surface identities",
            )?
        } else {
            false
        };
        if duplicate {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch surface has an invalid persistent identity",
                Some(
                    ctx.decode
                        .copy_retained_text(&surface.id, "retain F3D validation entity")?,
                ),
            )?;
        }
        if !ctx.decode.insert_hash_set(
            &mut sketch_geometry_records,
            (design_stream(&surface.id), surface.record_index),
            "index F3D sketch geometry records",
        )? {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(
                    ctx.decode
                        .copy_retained_text(&surface.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

fn emit_sketch_relation_finding(
    decode: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    entity: &str,
    message: &'static str,
) -> Result<(), CodecError> {
    decode.reserve_vec(findings, 1, "collect F3D sketch owner findings")?;
    let entity = decode.copy_retained_text(entity, "retain F3D sketch owner finding ID")?;
    findings.push(Finding {
        check: Check::NativeLinks,
        severity: Severity::Error,
        message: decode.copy_retained_text(message, "retain F3D sketch owner finding message")?,
        entity: Some(entity),
    });
    Ok(())
}

fn validate_sketch_relation_owners(
    decode: &DecodeContext<'_>,
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let placements_by_scope = &ctx.placements_by_scope;
    let sketch_owner_ids = &ctx.sketch_owner_ids;
    let typed_sketch_records = decode.collect_hash_set(
        decode.admit_iter(&native.sketch_points, "admit source for index F3D typed sketch records")?
            .map(|point| (design_stream(&point.id), point.record_index))
            .chain(
                decode.admit_iter(&native.sketch_curve_identities, "admit source for index F3D typed sketch records")?
                    .map(|curve| (design_stream(&curve.id), curve.record_index)),
            )
            .chain(
                decode.admit_iter(&native.sketch_surfaces, "admit source for index F3D typed sketch records")?
                    .map(|surface| (design_stream(&surface.id), surface.record_index)),
            ),
        "index F3D typed sketch records",
    )?;
    let sketch_operands = decode.collect_hash_map(
        decode.admit_iter(&native.sketch_points, "admit source for index F3D sketch operands")?
            .map(|point| {
                (
                    (design_stream(&point.id), point.record_index),
                    records::sketch_relations::SketchRelationOperand::Point {
                        record_index: point.record_index,
                        persistent_id: point.persistent_id(),
                    },
                )
            })
            .chain(decode.admit_iter(&native.sketch_curve_identities, "admit source for index F3D sketch operands")?.map(|curve| {
                (
                    (design_stream(&curve.id), curve.record_index),
                    records::sketch_relations::SketchRelationOperand::Curve {
                        record_index: curve.record_index,
                        primary_id: curve.primary_id.get(),
                        secondary_id: curve.secondary_id,
                    },
                )
            }))
            .chain(decode.admit_iter(&native.sketch_surfaces, "admit source for index F3D sketch operands")?.map(|surface| {
                (
                    (design_stream(&surface.id), surface.record_index),
                    records::sketch_relations::SketchRelationOperand::Surface {
                        record_index: surface.record_index,
                        persistent_id: surface.persistent_id.get(),
                    },
                )
            })),
        "index F3D sketch operands",
    )?;
    let mut relation_owners = std::collections::HashMap::new();
    for (id, record_index, owner_reference) in decode.admit_iter(&native.sketch_points, "scan F3D sketch point owners")?
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            decode.admit_iter(&native.sketch_curve_identities, "scan F3D sketch curve owners")?
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
        .chain(
            decode.admit_iter(&native.sketch_surfaces, "scan F3D sketch surface owners")?
                .map(|surface| (&surface.id, surface.record_index, surface.owner_reference)),
        )
    {
        let Some(owner_reference) = owner_reference else {
            continue;
        };
        let native_stream = design_stream(id);
        if decode.contains_key_hash_map(sketch_owner_ids, &(native_stream, owner_reference), "find F3D sketch owner identity")? {
            decode.insert_hash_map(
                &mut relation_owners,
                (native_stream, record_index),
                owner_reference,
                "index F3D sketch relation owners",
            )?;
        }
    }
    for relation in ctx.decode.admit_iter(&native.sketch_relations, "scan F3D sketch relations")? {
        let native_stream = design_stream(&relation.id);
        let agrees = |reference: &records::sketch_relations::SketchRelationReference| {
            let record_index = reference.record_index();
            match sketch_operands.get(&(native_stream, record_index)) {
                Some(expected) => reference.resolved() == Some(expected),
                None => {
                    reference.resolved()
                        == Some(&records::sketch_relations::SketchRelationOperand::Record {
                            record_index,
                        })
                }
            }
        };
        if !relation
            .members()
            .iter()
            .all(|member| agrees(&member.reference))
            || !relation
                .return_members()
                .iter()
                .all(|member| agrees(&member.reference))
        {
            emit_sketch_relation_finding(
                decode,
                findings,
                &relation.id,
                "Fusion sketch relation typed operands disagree with its indexed references",
            )?;
        }
        for member in decode.admit_iter(&relation.members()[..], "scan F3D sketch relation members")?
            .map(|member| member.reference.record_index())
            .chain(decode.admit_iter(&relation.return_members()[..], "scan F3D sketch relation return members")?
                .map(|member| member.reference.record_index())) {
            if !decode.contains_hash_set(&typed_sketch_records, &(native_stream, member), "find F3D relation typed sketch record")? {
                continue;
            }
            if decode
                .insert_hash_map(
                    &mut relation_owners,
                    (native_stream, member),
                    relation.owner_reference,
                    "index F3D sketch relation owners",
                )?
                .is_some_and(|owner| owner != relation.owner_reference)
            {
                emit_sketch_relation_finding(
                    decode,
                    findings,
                    &relation.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                )?;
            }
        }
    }
    for entity in decode.admit_iter(&native.design_entity_headers, "scan F3D sketch entity headers")?
        .filter(|entity| entity.in_sketch_module())
    {
        let native_stream = design_stream(&entity.id);
        let Ok(owner) = u32::try_from(entity.entity_id.suffix()) else {
            continue;
        };
        let Some(members) = entity.sketch_members() else { continue; };
        let (values, located) = members.storage_slices();
        for member in decode.admit_iter(values, "scan F3D sketch entity members")?
            .chain(decode.admit_iter(located, "scan F3D sketch entity located members")?.map(|row| &row.value)) {
            if !decode.contains_hash_set(&typed_sketch_records, &(native_stream, *member), "find F3D entity typed sketch record")? {
                continue;
            }
            if decode
                .insert_hash_map(
                    &mut relation_owners,
                    (native_stream, *member),
                    owner,
                    "index F3D sketch relation owners",
                )?
                .is_some_and(|existing| existing != owner)
            {
                emit_sketch_relation_finding(
                    decode,
                    findings,
                    &entity.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                )?;
            }
        }
    }
    for pair in ctx.decode.admit_iter(&native.design_dimension_locus_pairs[..], "scan F3D design dimension locus pairs")? {
        let native_stream = design_stream(&pair.id);
        let owner = companions_by_index
            .get(&(native_stream, pair.governing_companion_record_index))
            .and_then(|companion| {
                owners_by_index.get(&(native_stream, companion.owner_record_index()))
            })
            .and_then(|parameter_owner| {
                placements_by_scope.get(&(native_stream, parameter_owner.scope_record_index()))
            })
            .and_then(|placement| u32::try_from(placement.entity_id.suffix()).ok());
        let Some(owner) = owner else {
            continue;
        };
        for member in [
            pair.loci()[0].geometry_index(),
            pair.loci()[1].geometry_index(),
        ] {
            if decode
                .insert_hash_map(
                    &mut relation_owners,
                    (native_stream, member),
                    owner,
                    "index F3D sketch relation owners",
                )?
                .is_some_and(|existing| existing != owner)
            {
                emit_sketch_relation_finding(
                    decode,
                    findings,
                    &pair.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                )?;
            }
        }
    }
    for group in ctx.decode.admit_iter(&native.design_dimension_locus_groups, "scan F3D design dimension locus groups")? {
        let native_stream = design_stream(&group.id);
        for member in decode.admit_iter(&group.loci, "scan F3D locus geometry members")?
            .map(|locus| locus.geometry_record_index)
            .chain(decode.admit_iter(&group.loci, "scan F3D locus return members")?.map(|locus| locus.returned.value))
        {
            if decode
                .insert_hash_map(
                    &mut relation_owners,
                    (native_stream, member),
                    group.owner_reference,
                    "index F3D sketch relation owners",
                )?
                .is_some_and(|existing| existing != group.owner_reference)
            {
                emit_sketch_relation_finding(
                    decode,
                    findings,
                    &group.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                )?;
            }
        }
    }
    for pair in ctx.decode.admit_iter(&native.design_dimension_null_locus_pairs[..], "scan F3D design dimension null locus pairs")? {
        let native_stream = design_stream(&pair.id);
        let owner = companions_by_index
            .get(&(native_stream, pair.governing_companion_record_index))
            .and_then(|companion| {
                owners_by_index.get(&(native_stream, companion.owner_record_index()))
            })
            .and_then(|parameter_owner| {
                placements_by_scope.get(&(native_stream, parameter_owner.scope_record_index()))
            })
            .and_then(|placement| u32::try_from(placement.entity_id.suffix()).ok());
        let Some(owner) = owner else {
            continue;
        };
        if decode
            .insert_hash_map(
                &mut relation_owners,
                (native_stream, pair.loci()[1].geometry_index()),
                owner,
                "index F3D sketch relation owners",
            )?
            .is_some_and(|existing| existing != owner)
        {
            emit_sketch_relation_finding(
                decode,
                findings,
                &pair.id,
                "Fusion sketch member belongs to multiple sketch owners",
            )?;
        }
    }
    for (id, record_index, owner_reference) in decode.admit_iter(&native.sketch_points, "scan F3D sketch point owners")?
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            decode.admit_iter(&native.sketch_curve_identities, "scan F3D sketch curve owners")?
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
    {
        if relation_owners
            .get(&(design_stream(id), record_index))
            .copied()
            != owner_reference
        {
            emit_sketch_relation_finding(
                decode,
                findings,
                id,
                "Fusion sketch geometry owner disagrees with its relation graph",
            )?;
        }
    }
    Ok(())
}

/// Validate persistent body links and their history ordering.
fn validate_body_links(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let ir = ctx.ir;
    let body_ids = ctx.decode.collect_hash_set(
        ir.model.bodies.iter().map(|body| &body.id),
        "index F3D persistent body targets",
    )?;
    let mut body_links: std::collections::BTreeMap<_, Vec<_>> = std::collections::BTreeMap::new();
    for link in ctx.decode.admit_iter(&native.persistent_design_links, "scan F3D persistent design links")? {
        let target_key = match &link.target {
            cadmpeg_ir::attributes::AttributeTarget::Body(id) if body_ids.contains(id) => Some(id),
            _ => None,
        };
        let Some(target_key) = target_key else {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent body link has an invalid target or group payload",
                Some(
                    ctx.decode
                        .copy_retained_text(&link.id, "retain F3D validation entity")?,
                ),
            )?;
            continue;
        };
        ctx.decode.push_btree_group(
            &mut body_links,
            target_key,
            link,
            "index F3D persistent body link groups",
            "collect F3D persistent body link members",
        )?;
    }
    let mut links_keys_storage = ctx.decode.reserve_scoped(0, "scan F3D links groups")?;
    let links_keys = links_keys_storage.with_storage(|| {
        ctx.decode.collect_vec(body_links.keys().copied(), "stage F3D links group keys")
    })?;
    for key in ctx.decode.admit_iter(&links_keys, "scan F3D links groups")? {
        let links = ctx.decode.get_mut_btree_map(&mut body_links, key, "find F3D links group")?
            .ok_or_else(|| CodecError::malformed("F3D validation group is absent"))?;
        ctx.decode.stable_sort_by(
            links,
            |value| &value.ordinal,
            Ord::cmp,
            "f3d body links sort",
        )?;
        if links
            .iter()
            .enumerate()
            .any(|(ordinal, link)| u32::try_from(ordinal) != Ok(link.ordinal))
        {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent body links have noncanonical history ordering",
                links
                    .first()
                    .map(|link| {
                        ctx.decode
                            .copy_retained_text(&link.id, "retain F3D validation entity")
                    })
                    .transpose()?,
            )?;
        }
    }
    Ok(())
}

/// Validate persistent subentity tags and their group ordering.
fn validate_subentity_tags(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let ir = ctx.ir;
    let face_ids = ctx.decode.collect_hash_set(
        ir.model.faces.iter().map(|face| &face.id),
        "index F3D persistent face targets",
    )?;
    let edge_ids = ctx.decode.collect_hash_set(
        ir.model.edges.iter().map(|edge| &edge.id),
        "index F3D persistent edge targets",
    )?;
    let mut subentity_tags = std::collections::BTreeMap::new();
    for tag in ctx.decode.admit_iter(&native.persistent_subentity_tags, "scan F3D persistent subentity tags")? {
        let Some(target_key) = (match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(id) if face_ids.contains(id) => {
                Some((1_u8, id.as_str()))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(id) if edge_ids.contains(id) => {
                Some((0_u8, id.as_str()))
            }
            _ => None,
        }) else {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent subentity tag has an invalid target or group payload",
                Some(
                    ctx.decode
                        .copy_retained_text(&tag.id, "retain F3D validation entity")?,
                ),
            )?;
            continue;
        };
        ctx.decode.push_btree_group(
            &mut subentity_tags,
            target_key,
            tag,
            "index F3D persistent subentity tag groups",
            "collect F3D persistent subentity tag members",
        )?;
    }
    let mut tags_keys_storage = ctx.decode.reserve_scoped(0, "scan F3D tags groups")?;
    let tags_keys = tags_keys_storage.with_storage(|| {
        ctx.decode.collect_vec(subentity_tags.keys().copied(), "stage F3D tags group keys")
    })?;
    for key in ctx.decode.admit_iter(&tags_keys, "scan F3D tags groups")? {
        let tags = ctx.decode.get_mut_btree_map(&mut subentity_tags, key, "find F3D tags group")?
            .ok_or_else(|| CodecError::malformed("F3D validation group is absent"))?;
        ctx.decode.stable_sort_by(
            tags,
            |value| &value.ordinal,
            Ord::cmp,
            "f3d subentity tags sort",
        )?;
        if tags
            .iter()
            .enumerate()
            .any(|(ordinal, tag)| u32::try_from(ordinal) != Ok(tag.ordinal))
        {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent subentity tags have noncanonical group ordering",
                tags.first()
                    .map(|tag| {
                        ctx.decode
                            .copy_retained_text(&tag.id, "retain F3D validation entity")
                    })
                    .transpose()?,
            )?;
        }
    }
    Ok(())
}

/// Validate each ASM history graph as a coherent state chain.
fn validate_history_graphs(
    decode: &DecodeContext<'_>,
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    for history in ctx.decode.admit_iter(&native.asm_histories, "scan F3D asm histories")? {
        let coherent = history::graph_is_coherent_charged(decode, history)?;
        if !coherent {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ASM history graph is not a coherent doubly linked state chain",
                Some(
                    ctx.decode
                        .copy_retained_text(&history.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
