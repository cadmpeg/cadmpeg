// SPDX-License-Identifier: Apache-2.0
//! Validation of construction operand groups and identities.

use super::fillets::mirror_group_role_admitted;
use super::path_features::is_path_seed_group;
use super::{design_stream, push_constant_finding, Ctx};
use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_scope;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::{design, records};
use cadmpeg_ir::report::check::{Check, Finding};
use std::collections::HashSet;

/// Validate construction operand groups and their role discriminators.
pub(super) fn validate_construction_operand_groups(ctx: &Ctx, findings: &mut Vec<Finding>) {
    use crate::records::feature::scope::DesignFeatureKind;

    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut operand_group_slots = HashSet::new();
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        let scope = scopes_by_index.get(&(native_stream, group.scope_record_index));
        let header = records_by_index.get(&(native_stream, group.record_index));
        let frame = &group.frame;
        let member_run_end = group.members().last().map_or_else(
            || frame.member_count_offset.checked_add(4),
            |member| member.offset.checked_add(10),
        );
        let member_count_offset = frame.member_count_offset.checked_sub(group.byte_offset);
        let kind = scope.map(|scope| scope.kind());
        let prefixed_tool = matches!(
            (kind.as_ref(), group.role()),
            (
                Some(DesignFeatureKind::SplitFace),
                DesignOperandRole::ROLE_0X21
            ) | (
                Some(DesignFeatureKind::Split),
                DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X21
            )
        );
        let prefixed = kind == Some(DesignFeatureKind::SurfaceStitch) || prefixed_tool;
        let compact_split_tool = kind == Some(DesignFeatureKind::Split)
            && group.role() == DesignOperandRole::ROLE_0X21
            && group.scope_reference_ordinal == 0
            && !group.members().is_empty()
            && frame.auxiliary_records.is_empty()
            && frame.trailing_records().len() == 1
            && !frame.variant
            && !frame.closing_flag;
        let compact_count = cadmpeg_core::decode::u64_from_index(
            crate::layout::split_body_compact_group_prefix::MEMBER_COUNT,
        );
        let count_offset_valid = member_count_offset == Some(if prefixed { 88 } else { 21 })
            || (compact_split_tool && member_count_offset == Some(compact_count));
        let frame_valid = count_offset_valid
            && group.members().first().is_none_or(|member| {
                frame
                    .member_count_offset
                    .checked_add(5)
                    .is_some_and(|expected_offset| member.offset == expected_offset)
            })
            && frame.trailing_records().first().is_none_or(|record| {
                group
                    .role_offset()
                    .checked_sub(record.offset)
                    .is_some_and(|span| matches!(span, 10 | 50))
            })
            && member_run_end.is_some_and(|end| group.role_offset() >= end)
            && group.role().raw().trailing_zeros() >= 32
            && frame
                .opaque_scalar_offset()
                .checked_add(8)
                .is_some_and(|expected_offset| group.paired_byte_offset > expected_offset)
            && frame
                .auxiliary_records
                .iter()
                .map(|record| &record.value)
                .chain(frame.trailing_records().iter().map(|record| &record.value))
                .all(|record_index| records_by_index.contains_key(&(native_stream, *record_index)))
            && frame.trailing_transforms().iter().all(|transform| {
                frame
                    .trailing_records()
                    .iter()
                    .any(|record| record.value == transform.record_index())
                    && records_by_index
                        .get(&(native_stream, transform.record_index()))
                        .is_some_and(|header| {
                            header.byte_offset == transform.byte_offset()
                                && header.class_tag == transform.class_tag
                        })
                    // The following header can be the lone paired header of an
                    // omitted primary frame, which has no header entry.
                    && records_by_index
                        .get(&(native_stream, transform.following_record_index()))
                        .is_none_or(|header| {
                            header.byte_offset == transform.following_byte_offset()
                                && header.class_tag == transform.following_class_tag
                        })
            })
            && frame.trailing_dual_transforms().iter().all(|transform| {
                frame
                    .trailing_records()
                    .iter()
                    .any(|record| record.value == transform.record_index)
                    && records_by_index
                        .get(&(native_stream, transform.record_index))
                        .is_some_and(|header| {
                            header.byte_offset == transform.byte_offset
                                && header.class_tag == transform.class_tag
                        })
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
            })
            && frame.trailing_flags().iter().all(|flag| {
                frame
                    .trailing_records()
                    .iter()
                    .any(|record| record.value == flag.record_index)
                    && records_by_index
                        .get(&(native_stream, flag.record_index))
                        .is_some_and(|header| {
                            header.byte_offset == flag.byte_offset
                                && header.class_tag == flag.class_tag
                        })
                    && flag
                        .byte_offset
                        .checked_add(22)
                        .is_some_and(|expected_offset| flag.value_offset == expected_offset)
            })
            && frame.auxiliary_paths().iter().all(|path| {
                frame
                    .auxiliary_records
                    .iter()
                    .any(|record| record.value == path.record_index())
                    && records_by_index
                        .get(&(native_stream, path.record_index()))
                        .is_some_and(|header| {
                            header.byte_offset == path.byte_offset()
                                && header.class_tag == path.class_tag
                        })
                    && path.scope_record_index == group.scope_record_index
                    && records_by_index.contains_key(&(native_stream, path.nested_record_index()))
                    && records_by_index
                        .get(&(native_stream, path.following_record_index()))
                        .is_some_and(|header| {
                            header.byte_offset == path.following_byte_offset()
                                && header.class_tag == path.following_class_tag
                        })
            });
        let valid = scope.is_some_and(|scope| {
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
                                    || design::decode::operands::extrude_has_selected_termination(scope)
                                    || (group.extrude_face_role()
                                        == Some(records::topology::extrude_selection::DesignExtrudeFaceRole::Termination)
                                        && group.members().len() == 1
                                        && design::decode::operands::allows_slotted_extrude_face_targets(scope))
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
                    let admitted = |candidate: &records::topology::construction::DesignConstructionOperandGroup| {
                        design_stream(&candidate.id) == native_stream
                            && candidate.scope_record_index == group.scope_record_index
                            && scope.coil_target_group_admits(
                                candidate.role(),
                                candidate.scope_reference_ordinal,
                            )
                    };
                    admitted(group)
                        && group.extrude_role().is_none()
                        && group.members().len() == 1
                        && native
                            .design_construction_operand_groups
                            .iter()
                            .filter(|candidate| admitted(candidate))
                            .count()
                            == 1
                }
                Some(design::DesignFeatureFamily::Move) => {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::OffsetFaces) => {
                    group.role() == DesignOperandRole::ROLE_0X10 && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Draft) => {
                    (matches!(group.role(), DesignOperandRole::ROLE_0X10 | DesignOperandRole::ROLE_0X21)
                        || (group.role() == DesignOperandRole::BODIES_B
                            && scope.draft_operation().and_then(|operation| operation.flags)
                                .and_then(|flags| flags.parting).is_some_and(|parting| {
                                    parting.tool.value == group.record_index
                                        || parting.fixed_edges.is_some_and(|fixed| fixed.value == group.record_index)
                                }))) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::ReplaceFace) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X9
                            | DesignOperandRole::ROLE_0X10
                            | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Revolve) => {
                    (matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X21
                            | DesignOperandRole::PROFILE
                    ) || (group.role() == DesignOperandRole::ROLE_0X5
                        && matches!(
                            scope.payload(),
                            records::feature::scope::DesignScopePayload::Revolve(
                                None | Some(
                                    records::feature::path_features::DesignRevolveConstruction {
                                        travel:
                                            records::feature::path_features::DesignRevolveTravel::ToTargets(
                                                _
                                            ),
                                        ..
                                    }
                                )
                            )
                        )))
                        && group.extrude_role().is_none()
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
                                | DesignOperandRole::BODIES_B
                                | DesignOperandRole::ROLE_0X5
                                | DesignOperandRole::PROFILE
                                | DesignOperandRole::ROLE_0X43
                                | DesignOperandRole::ROLE_0X7
                                | DesignOperandRole::ROLE_0X21
                        ))
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Sweep) => {
                    (!scope.has_path_construction()
                        || matches!(
                            group.role(),
                            DesignOperandRole::BODIES_A
                                | DesignOperandRole::BODIES_B
                                | DesignOperandRole::ROLE_0X5
                                | DesignOperandRole::FACES
                                | DesignOperandRole::PROFILE
                        ))
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Pipe) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X5
                            | DesignOperandRole::BODIES_A
                            | DesignOperandRole::BODIES_B
                    ) && group.extrude_role().is_none()
                }
                Some(
                    design::DesignFeatureFamily::CircularPattern
                    | design::DesignFeatureFamily::RectangularPattern,
                ) => {
                    (matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X10
                    ) || is_path_seed_group(group))
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Mirror) => {
                    mirror_group_role_admitted(group.role()) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfacePatch) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::PROFILE
                            | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfaceOffset) => {
                    group.role() == DesignOperandRole::PROFILE && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfaceRuled) => {
                    group.role() == DesignOperandRole::BODIES_B
                        && group.extrude_role().is_none()
                        && scope.ruled_surface_operation().is_some_and(|operation| {
                            operation
                                .edge_group_record_indices
                                .contains(&group.record_index)
                        })
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
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X5
                            | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfaceTrim) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::ROLE_0X9
                            | DesignOperandRole::ROLE_0X21
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
                        && scope.thread_construction().is_none_or(|construction| {
                            construction
                                .face_group_record_indices
                                .contains(&group.record_index)
                        })
                }
                Some(design::DesignFeatureFamily::SheetMetalEdgeFlange) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X21
                            | DesignOperandRole::ROLE_0X43
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SheetMetalUnfold) => {
                    matches!(group.role(), DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X10) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SheetMetalRefold) => {
                    group.role() == DesignOperandRole::BODIES_B && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SheetMetalBend) => {
                    matches!(group.role(), DesignOperandRole::ROLE_0X10 | DesignOperandRole::ROLE_0X21) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SheetMetalHem) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_B | DesignOperandRole::ROLE_0X43
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Emboss) =>
                {
                    scope.emboss_operand_construction().is_some_and(|construction| {
                        (group.record_index == construction.profile_group.value
                            && group.role() == DesignOperandRole::PROFILE)
                            || (group.record_index == construction.face_group.value
                                && group.role() == DesignOperandRole::ROLE_0X10)
                    }) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SilhouetteSplit) => {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                Some(_) => false,
                None if matches!(
                    scope.kind(),
                    crate::records::feature::scope::DesignFeatureKind::RemoveBody
                        | crate::records::feature::scope::DesignFeatureKind::ReverseNormal
                        | crate::records::feature::scope::DesignFeatureKind::Modify
                ) =>
                {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::SurfaceStitch =>
                {
                    group.role() == DesignOperandRole::ROLE_0X5 && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::SplitFace =>
                {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X10 | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                None if matches!(
                    scope.kind(),
                    crate::records::feature::scope::DesignFeatureKind::MoveFace
                        | crate::records::feature::scope::DesignFeatureKind::DeleteFace
                        | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                        | crate::records::feature::scope::DesignFeatureKind::UnStitch
                ) =>
                {
                    group.role() == DesignOperandRole::ROLE_0X10 && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::PathPattern =>
                {
                    let seed = matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X10
                    ) && group.scope_reference_ordinal == 1;
                    let path = group.role() == DesignOperandRole::ROLE_0X5
                        && group.scope_reference_ordinal > 1;
                    (seed || path || is_path_seed_group(group)) && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::Decal =>
                {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                None if scope.mesh_face_groups().is_some() => {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::Align =>
                {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X5
                    ) && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::BaseFlange =>
                {
                    group.role() == DesignOperandRole::PROFILE
                        && group.extrude_role().is_none()
                        && scope.base_flange_profile().as_ref().is_some_and(|profile| {
                            group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([profile.record_index])
                        })
                }
                None if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hem => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_B | DesignOperandRole::ROLE_0X43
                    ) && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::SurfaceUntrim =>
                {
                    group.role() == DesignOperandRole::ROLE_0X9 && group.extrude_role().is_none()
                }
                None => false,
            };
            role_is_valid
                && usize::try_from(group.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.record_index)
                && group
                    .members()
                    .iter()
                    .map(|member| &member.value)
                    .all(|member| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == member)
                    })
        }) && header.is_some_and(|header| {
            header.byte_offset == group.byte_offset && header.class_tag == group.class_tag
        }) && frame_valid
            && (!group.members().is_empty()
                || scope.is_some_and(|scope| {
                    matches!(
                        design::design_feature_family(&scope.kind()),
                        Some(
                            design::DesignFeatureFamily::CircularPattern
                                | design::DesignFeatureFamily::RectangularPattern
                        )
                    ) || scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::PathPattern
                }) && is_path_seed_group(group))
            && {
                let mut seen = HashSet::new();
                for member in group.members() {
                    seen.insert(member.value);
                }
                seen.len() == group.members().len()
            }
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .all(|member| records_by_index.contains_key(&(native_stream, *member)))
            && operand_group_slots.insert((native_stream, group.scope_record_index, group.scope_reference_ordinal));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has an invalid frame",
                Some(group.id.clone()),
            );
        }
    }
}

/// Validate construction operand identity chains; returns identity-backed groups.
pub(super) fn validate_construction_operand_identities<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut operand_identity_groups = HashSet::new();
    for identity in &native.design_construction_operand_identities {
        let native_stream = design_stream(&identity.id);
        let group = operand_groups_by_index.get(&(native_stream, identity.group_record_index));
        let selected_profile = group
            .and_then(|group| scopes_by_index.get(&(native_stream, group.scope_record_index)))
            .and_then(|scope| scope.extrude_profile());
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
        let chain_entry = identity
            .wrappers()
            .first()
            .map(|wrapper| {
                (
                    wrapper.record_index,
                    wrapper.byte_offset,
                    &wrapper.class_tag,
                )
            })
            .or_else(|| {
                identity.tracking_path().map(|path| {
                    (
                        path.wrapper_record_index(),
                        path.wrapper_byte_offset(),
                        &path.wrapper_class_tag,
                    )
                })
            })
            .unwrap_or((
                identity.following_record_index(),
                identity.following_byte_offset(),
                identity.following_class_tag(),
            ));
        let chain_entry_shape = transform.is_none_or(|transform| {
            chain_entry
                == (
                    transform.following_record_index(),
                    transform.following_byte_offset(),
                    &transform.following_class_tag,
                )
        });
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
            transform
                .map(super::records::topology::construction::DesignConstructionOperandTransform::record_index)
                .or_else(|| {
                    identity
                        .wrappers()
                        .first()
                        .map(|wrapper| wrapper.record_index)
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
            && operand_identity_groups.insert((native_stream, identity.group_record_index));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand identity has an invalid nested frame",
                Some(identity.id.clone()),
            );
        }
    }
    operand_identity_groups
}
